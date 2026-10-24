//! WaveSpeed AI model generation and benchmark engine adapter.
//!
//! Provides bounded concurrency execution, passes LoRA adapters (`loras: [{ path, scale }]`),
//! uses full-scene captions (`Trigger Phrase, Physical Description`) with host-side prompt expansion disabled,
//! polls task completion, extracts generated raster image bytes, and computes flat per-image costs ($0.015 / $0.021).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::prelude::*;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use tracing::{debug, info, instrument};

use crate::domain::{
    build_wavespeed_prompt, generate_benchmark_id, BenchmarkArchetype, BenchmarkMetadata,
    BenchmarkResult, GenerationRequest, GenerationResponse, MODEL_WAVESPEED_KLEIN_EDIT_LORA,
    MODEL_WAVESPEED_KLEIN_T2I_LORA,
};
use crate::ports::model_engine::{ModelEngine, ModelEngineError};

/// Default WaveSpeed API endpoint base URL.
pub const DEFAULT_WAVESPEED_BASE_URL: &str = "https://api.wavespeed.ai";

/// Default concurrency limit (max 2 parallel requests in-flight).
pub const DEFAULT_WAVESPEED_CONCURRENCY_LIMIT: usize = 2;

/// Default polling interval between task status checks.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Default maximum polling attempts before timing out (~60s with 500ms intervals).
pub const DEFAULT_MAX_POLL_ATTEMPTS: usize = 120;

/// Default publicly accessible FLUX 3 Klein LoRA weights used when none are explicitly provided.
pub const DEFAULT_KLEIN_LORA_URL: &str =
    "https://huggingface.co/ausboss/FLUX-3-Image-Klein-Restore-LoRA/resolve/main/flux-3-image-klein-restore.safetensors";

/// Default LoRA scaling factor for the distilled FLUX 3 Klein checkpoint (ADR-0008).
pub const DEFAULT_KLEIN_LORA_SCALE: f64 = 0.75;

/// Inner state of the WaveSpeed adapter shared across clones.
struct WaveSpeedInner {
    api_key: String,
    base_url: String,
    concurrency_limit: usize,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
    default_lora_path: String,
    default_lora_scale: f64,
    poll_interval: Duration,
    max_poll_attempts: usize,
}

/// WaveSpeed benchmark engine adapter implementing the `ModelEngine` port trait.
#[derive(Clone)]
pub struct WaveSpeedAdapter {
    inner: Arc<WaveSpeedInner>,
}

impl WaveSpeedAdapter {
    /// Create a new `WaveSpeedAdapter` with an explicit API key and default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(api_key, DEFAULT_WAVESPEED_BASE_URL, DEFAULT_WAVESPEED_CONCURRENCY_LIMIT)
    }

    /// Create a `WaveSpeedAdapter` by resolving `WAVESPEED_API_KEY` from process environment variables
    /// or local config/secret files.
    pub fn from_env() -> Result<Self, ModelEngineError> {
        let api_key = resolve_wavespeed_api_key()?;
        let base_url = std::env::var("WAVESPEED_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_WAVESPEED_BASE_URL.to_string());
        let default_lora = std::env::var("WAVESPEED_DEFAULT_LORA_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_KLEIN_LORA_URL.to_string());
        let default_scale = std::env::var("WAVESPEED_DEFAULT_LORA_SCALE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(DEFAULT_KLEIN_LORA_SCALE);

        let mut adapter = Self::with_config(api_key, base_url, DEFAULT_WAVESPEED_CONCURRENCY_LIMIT);
        adapter = adapter.with_default_lora(default_lora, default_scale);
        Ok(adapter)
    }

    /// Create a `WaveSpeedAdapter` with custom base URL and concurrency limit.
    pub fn with_config(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        concurrency_limit: usize,
    ) -> Self {
        let limit = if concurrency_limit == 0 { 1 } else { concurrency_limit };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .no_proxy()
            .build()
            .unwrap_or_default();

        Self {
            inner: Arc::new(WaveSpeedInner {
                api_key: api_key.into(),
                base_url: base_url.into().trim_end_matches('/').to_string(),
                concurrency_limit: limit,
                semaphore: Arc::new(Semaphore::new(limit)),
                client,
                default_lora_path: DEFAULT_KLEIN_LORA_URL.to_string(),
                default_lora_scale: DEFAULT_KLEIN_LORA_SCALE,
                poll_interval: DEFAULT_POLL_INTERVAL,
                max_poll_attempts: DEFAULT_MAX_POLL_ATTEMPTS,
            }),
        }
    }

    /// Configure a custom polling interval between task status requests.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.poll_interval = interval;
        } else {
            self.inner = Arc::new(WaveSpeedInner {
                api_key: self.inner.api_key.clone(),
                base_url: self.inner.base_url.clone(),
                concurrency_limit: self.inner.concurrency_limit,
                semaphore: self.inner.semaphore.clone(),
                client: self.inner.client.clone(),
                default_lora_path: self.inner.default_lora_path.clone(),
                default_lora_scale: self.inner.default_lora_scale,
                poll_interval: interval,
                max_poll_attempts: self.inner.max_poll_attempts,
            });
        }
        self
    }

    /// Configure custom maximum polling attempts.
    pub fn with_max_poll_attempts(mut self, attempts: usize) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.max_poll_attempts = attempts;
        } else {
            self.inner = Arc::new(WaveSpeedInner {
                api_key: self.inner.api_key.clone(),
                base_url: self.inner.base_url.clone(),
                concurrency_limit: self.inner.concurrency_limit,
                semaphore: self.inner.semaphore.clone(),
                client: self.inner.client.clone(),
                default_lora_path: self.inner.default_lora_path.clone(),
                default_lora_scale: self.inner.default_lora_scale,
                poll_interval: self.inner.poll_interval,
                max_poll_attempts: attempts,
            });
        }
        self
    }

    /// Configure custom default LoRA weights and scale factor.
    pub fn with_default_lora(mut self, path: impl Into<String>, scale: f64) -> Self {
        let p = path.into();
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.default_lora_path = p;
            inner.default_lora_scale = scale;
        } else {
            self.inner = Arc::new(WaveSpeedInner {
                api_key: self.inner.api_key.clone(),
                base_url: self.inner.base_url.clone(),
                concurrency_limit: self.inner.concurrency_limit,
                semaphore: self.inner.semaphore.clone(),
                client: self.inner.client.clone(),
                default_lora_path: p,
                default_lora_scale: scale,
                poll_interval: self.inner.poll_interval,
                max_poll_attempts: self.inner.max_poll_attempts,
            });
        }
        self
    }

    /// Base URL configured for this adapter.
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// Concurrency limit configured for this adapter.
    pub fn concurrency_limit(&self) -> usize {
        self.inner.concurrency_limit
    }

    /// Number of available permits in the worker pool.
    pub fn available_permits(&self) -> usize {
        self.inner.semaphore.available_permits()
    }

    /// Extracts the physical description clause from raw user prompts or formatted OpenRouter prompts.
    pub fn extract_physical_description(prompt: &str) -> &str {
        let trimmed = prompt.trim();
        if let Some(rest) = trimmed.strip_prefix("Details:") {
            if let Some((details, _)) = rest.split_once(". Perspective:") {
                return details.trim();
            }
            return rest.trim();
        }
        trimmed
    }

    /// Whether a model identifier routes to the edit engine rather than text-to-image.
    pub fn is_edit_model(model: &str) -> bool {
        model == MODEL_WAVESPEED_KLEIN_EDIT_LORA || model.ends_with("-edit")
    }

    /// Formats the prompt specifically for WaveSpeed FLUX 3 Klein LoRA (`Trigger Phrase, Physical Description`).
    pub fn format_full_scene_prompt(prompt: &str) -> String {
        let desc = Self::extract_physical_description(prompt);
        build_wavespeed_prompt(desc)
    }

    /// Builds the WaveSpeed task submission JSON payload for a generation request.
    pub fn build_payload(&self, request: &GenerationRequest) -> Result<Value, ModelEngineError> {
        let formatted_prompt = Self::format_full_scene_prompt(&request.prompt);

        let loras = if !request.loras.is_empty() {
            request
                .loras
                .iter()
                .map(|l| {
                    json!({
                        "path": l.path,
                        "scale": l.scale,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            vec![json!({
                "path": self.inner.default_lora_path,
                "scale": self.inner.default_lora_scale,
            })]
        };

        let mut body = json!({
            "prompt": formatted_prompt,
            "aspect_ratio": request.aspect_ratio,
            "resolution": "1k",
            "output_format": "png",
            "enable_prompt_expansion": false,
            "loras": loras,
        });

        if Self::is_edit_model(&request.model) {
            if request.images.is_empty() {
                return Err(ModelEngineError::InvalidRequest(
                    "FLUX 3 Klein Edit LoRA requires at least one reference image".to_string(),
                ));
            }
            let images: Vec<String> = request.images.iter().map(|img| img.to_data_url()).collect();
            body["images"] = json!(images);
        }

        Ok(body)
    }

    /// Calculates flat per-image cost in USD for WaveSpeed endpoints.
    pub fn cost_for_model(model_id: &str) -> f64 {
        if Self::is_edit_model(model_id) {
            0.021
        } else {
            0.015
        }
    }
}

#[async_trait]
impl ModelEngine for WaveSpeedAdapter {
    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn generate_image(&self, request: &GenerationRequest) -> Result<GenerationResponse, ModelEngineError> {
        if self.inner.api_key.trim().is_empty() {
            return Err(ModelEngineError::Auth("WaveSpeed API key is empty".to_string()));
        }

        debug!(
            model = %request.model,
            available_permits = self.inner.semaphore.available_permits(),
            "Acquiring worker pool permit for WaveSpeed generation request..."
        );
        let _permit = self.inner.semaphore.acquire().await.map_err(|e| {
            ModelEngineError::WorkerPool(format!("Failed to acquire worker pool permit: {e}"))
        })?;

        let start_time = Instant::now();
        let payload = self.build_payload(request)?;
        let engine_name = if Self::is_edit_model(&request.model) {
            MODEL_WAVESPEED_KLEIN_EDIT_LORA
        } else {
            MODEL_WAVESPEED_KLEIN_T2I_LORA
        };
        let endpoint = format!("{}/api/v3/{}", self.inner.base_url, engine_name);

        debug!(
            endpoint = %endpoint,
            "Submitting generation task to WaveSpeed API..."
        );

        let res = self
            .inner
            .client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", self.inner.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(classify_wavespeed_http_error(status.as_u16(), &err_text));
        }

        let initial_json: Value = res.json().await.map_err(|e| ModelEngineError::Api {
            status: status.as_u16(),
            message: format!("Failed to parse WaveSpeed initial response JSON: {e}"),
        })?;

        // Extract initial output if completed synchronously, or polling URL
        let task_status = initial_json
            .pointer("/data/status")
            .and_then(|v| v.as_str())
            .unwrap_or("created");

        let image_url = if task_status == "completed" {
            extract_output_url(&initial_json)?
        } else {
            // Determine polling URL
            let poll_url = if let Some(url) = initial_json.pointer("/data/urls/get").and_then(|v| v.as_str()) {
                url.to_string()
            } else if let Some(id) = initial_json.pointer("/data/id").and_then(|v| v.as_str()) {
                format!("{}/api/v3/predictions/{}/result", self.inner.base_url, id)
            } else {
                return Err(ModelEngineError::Api {
                    status: 200,
                    message: "WaveSpeed task response missing task ID and polling URL".to_string(),
                });
            };

            // Poll until completed, failed, or timed out
            self.poll_for_completion(&poll_url).await?
        };

        // Download raster image bytes
        let image_bytes = download_image_bytes(&self.inner.client, &image_url).await?;
        let latency_ms = start_time.elapsed().as_millis() as u64;
        let cost_usd = Self::cost_for_model(&request.model);

        info!(
            model = %request.model,
            latency_ms = latency_ms,
            cost_usd = cost_usd,
            image_bytes_len = image_bytes.len(),
            "WaveSpeed generated battlemap successfully"
        );

        Ok(GenerationResponse {
            image_bytes,
            model: request.model.clone(),
            prompt: request.prompt.clone(),
            prompt_tokens: None,
            completion_tokens: None,
            latency_ms,
            cost_usd,
        })
    }

    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError> {
        let prompt = build_wavespeed_prompt(archetype.prompt());
        let mut request = GenerationRequest::new(model, prompt);
        if Self::is_edit_model(model) {
            request = request.with_image(crate::domain::ReferenceImage::png(
                "default_reference",
                vec![
                    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49,
                    0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06,
                    0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44,
                    0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d,
                    0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
                    0x60, 0x82,
                ],
            ));
        }
        let response = self.generate_image(&request).await?;

        let id = generate_benchmark_id(model, archetype.slug());
        let metadata = BenchmarkMetadata::new(
            id,
            model,
            archetype.name(),
            archetype.prompt(),
            response.prompt_tokens,
            response.completion_tokens,
            response.latency_ms,
            response.cost_usd,
        );

        Ok(BenchmarkResult::new(metadata, response.image_bytes))
    }

    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
        if models.is_empty() || archetypes.is_empty() {
            return Ok(Vec::new());
        }

        let mut join_set = tokio::task::JoinSet::new();

        for model in models {
            for archetype in archetypes {
                let adapter = self.clone();
                let m = model.clone();
                let a = *archetype;

                join_set.spawn(async move { adapter.run_benchmark(&m, &a).await });
            }
        }

        let mut results = Vec::new();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok(Ok(benchmark_result)) => {
                    results.push(benchmark_result);
                }
                Ok(Err(engine_err)) => {
                    return Err(engine_err);
                }
                Err(join_err) => {
                    return Err(ModelEngineError::Internal(format!(
                        "Task join error during WaveSpeed benchmark suite: {join_err}"
                    )));
                }
            }
        }

        results.sort_by(|a, b| {
            a.metadata
                .model
                .cmp(&b.metadata.model)
                .then_with(|| a.metadata.prompt_name.cmp(&b.metadata.prompt_name))
        });

        Ok(results)
    }
}

impl WaveSpeedAdapter {
    /// Polls the prediction result endpoint until completion or terminal failure.
    async fn poll_for_completion(&self, poll_url: &str) -> Result<String, ModelEngineError> {
        for attempt in 0..self.inner.max_poll_attempts {
            tokio::time::sleep(self.inner.poll_interval).await;

            let res = self
                .inner
                .client
                .get(poll_url)
                .header("Authorization", format!("Bearer {}", self.inner.api_key))
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() {
                let err_text = res.text().await.unwrap_or_default();
                return Err(classify_wavespeed_http_error(status.as_u16(), &err_text));
            }

            let poll_json: Value = res.json().await.map_err(|e| ModelEngineError::Api {
                status: status.as_u16(),
                message: format!("Failed to parse polling JSON response: {e}"),
            })?;

            let task_status = poll_json
                .pointer("/data/status")
                .and_then(|v| v.as_str())
                .unwrap_or("processing");

            match task_status {
                "completed" | "succeeded" => {
                    return extract_output_url(&poll_json);
                }
                "failed" => {
                    let err_msg = poll_json
                        .pointer("/data/error")
                        .and_then(|v| v.as_str())
                        .or_else(|| poll_json.pointer("/message").and_then(|v| v.as_str()))
                        .unwrap_or("Unknown task error");
                    return Err(ModelEngineError::Api {
                        status: 500,
                        message: format!("WaveSpeed task failed: {err_msg}"),
                    });
                }
                "cancelled" => {
                    return Err(ModelEngineError::Api {
                        status: 500,
                        message: "WaveSpeed task was cancelled".to_string(),
                    });
                }
                _ => {
                    debug!(
                        attempt = attempt + 1,
                        status = task_status,
                        "WaveSpeed task still in-flight, continuing polling..."
                    );
                }
            }
        }

        Err(ModelEngineError::Api {
            status: 504,
            message: format!(
                "WaveSpeed task timed out after {} polling attempts",
                self.inner.max_poll_attempts
            ),
        })
    }
}

/// Extracts the generated image output URL from a WaveSpeed response payload.
fn extract_output_url(payload: &Value) -> Result<String, ModelEngineError> {
    if let Some(outputs) = payload.pointer("/data/outputs").and_then(|v| v.as_array()) {
        if let Some(first) = outputs.first().and_then(|v| v.as_str()) {
            return Ok(first.to_string());
        }
    }

    if let Some(output) = payload.pointer("/data/output").and_then(|v| v.as_str()) {
        return Ok(output.to_string());
    }

    Err(ModelEngineError::ImageExtraction(
        "WaveSpeed completed task did not return output image URLs".to_string(),
    ))
}

/// Downloads image bytes from either a remote HTTP URL or decodes a data URL.
async fn download_image_bytes(
    client: &reqwest::Client,
    image_url: &str,
) -> Result<Vec<u8>, ModelEngineError> {
    if let Some(b64_part) = image_url.strip_prefix("data:") {
        if let Some((_, data)) = b64_part.split_once(";base64,") {
            let bytes = BASE64_STANDARD.decode(data.trim()).map_err(|e| {
                ModelEngineError::ImageExtraction(format!("Failed to decode base64 data URL: {e}"))
            })?;
            return Ok(bytes);
        }
    }

    let res = client.get(image_url).send().await.map_err(|e| {
        ModelEngineError::Network(e)
    })?;

    let status = res.status();
    if !status.is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(ModelEngineError::Api {
            status: status.as_u16(),
            message: format!("Failed to download generated image from CDN ({status}): {err_text}"),
        });
    }

    let bytes = res.bytes().await.map_err(|e| {
        ModelEngineError::ImageExtraction(format!("Failed to read image bytes stream: {e}"))
    })?;

    Ok(bytes.to_vec())
}

/// Classifies WaveSpeed HTTP status codes and response messages into typed `ModelEngineError` variants.
fn classify_wavespeed_http_error(status: u16, text: &str) -> ModelEngineError {
    if status == 401 {
        return ModelEngineError::Auth(format!(
            "WaveSpeed authentication failed (401): {text}"
        ));
    }
    if status == 402 || text.contains("insufficient_quota") || text.contains("credits") {
        return ModelEngineError::QuotaOrRateLimit(format!(
            "Insufficient WaveSpeed credits or quota (status {status}): {text}"
        ));
    }
    if status == 429 {
        return ModelEngineError::QuotaOrRateLimit(format!(
            "WaveSpeed rate limit exceeded (429): {text}"
        ));
    }

    ModelEngineError::Api {
        status,
        message: text.to_string(),
    }
}

/// Resolves `WAVESPEED_API_KEY` by checking process environment variables, `.env`, and secret manifests.
pub fn resolve_wavespeed_api_key() -> Result<String, ModelEngineError> {
    if let Ok(key) = std::env::var("WAVESPEED_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() && trimmed != "your_wavespeed_api_key_here" {
            return Ok(trimmed.to_string());
        }
    }

    let candidates = [
        PathBuf::from("curator/.env"),
        PathBuf::from(".env"),
        PathBuf::from("../curator/.env"),
        PathBuf::from("../../curator/.env"),
        PathBuf::from("deploy/curator/secret.yaml"),
        PathBuf::from("../deploy/curator/secret.yaml"),
        PathBuf::from("../../deploy/curator/secret.yaml"),
        PathBuf::from("/home/knut/projects/Battlemap Builder/deploy/curator/secret.yaml"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(candidate) {
                for line in content.lines() {
                    let line = line.trim();
                    if let Some(val) = line
                        .strip_prefix("WAVESPEED_API_KEY:")
                        .or_else(|| line.strip_prefix("WAVESPEED_API_KEY="))
                    {
                        let clean = val.trim().trim_matches('"').trim_matches('\'').trim();
                        if !clean.is_empty() && clean != "your_wavespeed_api_key_here" {
                            return Ok(clean.to_string());
                        }
                    }
                }
            }
        }
    }

    Err(ModelEngineError::Auth(
        "WAVESPEED_API_KEY is not set in environment or configuration files".to_string(),
    ))
}
