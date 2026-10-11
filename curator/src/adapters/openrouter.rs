//! OpenRouter AI model generation and benchmark engine adapter.
//!
//! Provides bounded concurrency execution (max 2 parallel requests in-flight),
//! routes requests across chat completions and image generation endpoints,
//! computes inference metrics (latency, token usage, USD cost),
//! and extracts raster image outputs from model responses.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use base64::prelude::*;
use regex::Regex;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use tracing::{debug, info, instrument};

use crate::domain::{
    generate_benchmark_id, BenchmarkArchetype, BenchmarkMetadata, BenchmarkModelSpec,
    BenchmarkResult, GenerationRequest, GenerationResponse, ModelApiType, MODEL_GEMINI_31_FLASH,
    MODEL_GEMINI_31_FLASH_LITE, MODEL_MUSE, MODEL_SEEDREAM_50_FLASH,
};
use crate::ports::model_engine::{ModelEngine, ModelEngineError};

/// Default OpenRouter API endpoint base URL.
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Default concurrency limit (max 2 parallel requests in-flight).
pub const DEFAULT_CONCURRENCY_LIMIT: usize = 2;

/// Standard application title header sent to OpenRouter.
const APPLICATION_TITLE: &str = "Dungeon Forge Curator";

/// Inner state of the OpenRouter adapter shared across clones.
struct OpenRouterInner {
    api_key: String,
    base_url: String,
    concurrency_limit: usize,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
}

/// OpenRouter benchmark engine adapter implementing the `ModelEngine` port trait.
#[derive(Clone)]
pub struct OpenRouterAdapter {
    inner: Arc<OpenRouterInner>,
}

impl OpenRouterAdapter {
    /// Create a new `OpenRouterAdapter` with an explicit API key and default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(api_key, DEFAULT_OPENROUTER_BASE_URL, DEFAULT_CONCURRENCY_LIMIT)
    }

    /// Create an `OpenRouterAdapter` by resolving `OPENROUTER_API_KEY` from `server/.env`
    /// or process environment variables.
    pub fn from_env() -> Result<Self, ModelEngineError> {
        let api_key = resolve_api_key()?;
        Ok(Self::new(api_key))
    }

    /// Create an `OpenRouterAdapter` with custom base URL and concurrency limit.
    pub fn with_config(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        concurrency_limit: usize,
    ) -> Self {
        let limit = if concurrency_limit == 0 { 1 } else { concurrency_limit };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default();

        Self {
            inner: Arc::new(OpenRouterInner {
                api_key: api_key.into(),
                base_url: base_url.into().trim_end_matches('/').to_string(),
                concurrency_limit: limit,
                semaphore: Arc::new(Semaphore::new(limit)),
                client,
            }),
        }
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

    /// Direct reference to the internal semaphore.
    pub fn semaphore(&self) -> &Arc<Semaphore> {
        &self.inner.semaphore
    }

    /// Determine which endpoint kind to use for a given model.
    pub fn endpoint_type_for_model(model_id: &str) -> ModelApiType {
        if let Some(spec) = BenchmarkModelSpec::find(model_id) {
            return spec.api_type;
        }

        match model_id {
            MODEL_MUSE | MODEL_SEEDREAM_50_FLASH => ModelApiType::Images,
            MODEL_GEMINI_31_FLASH | MODEL_GEMINI_31_FLASH_LITE => ModelApiType::Chat,
            m if m.contains("image") && (m.starts_with("meta/") || m.starts_with("bytedance")) => {
                ModelApiType::Images
            }
            _ => ModelApiType::Chat,
        }
    }

    /// URL for the chat completions endpoint.
    pub fn chat_endpoint_url(&self) -> String {
        format!("{}/chat/completions", self.inner.base_url)
    }

    /// URL for the images endpoint.
    pub fn images_endpoint_url(&self) -> String {
        format!("{}/images", self.inner.base_url)
    }
}

/// Resolves `OPENROUTER_API_KEY` by checking `server/.env`, `.env`, and environment variables.
pub fn resolve_api_key() -> Result<String, ModelEngineError> {
    // 1. Direct environment variable check
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() && trimmed != "your_openrouter_api_key_here" {
            return Ok(trimmed.to_string());
        }
    }

    // 2. Search common candidate locations for server/.env or .env
    let candidates = [
        PathBuf::from("server/.env"),
        PathBuf::from("../server/.env"),
        PathBuf::from("../../server/.env"),
        PathBuf::from(".env"),
        PathBuf::from("../.env"),
    ];

    for path in &candidates {
        if path.is_file() {
            let _ = dotenvy::from_path(path);
            if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                let trimmed = key.trim();
                if !trimmed.is_empty() && trimmed != "your_openrouter_api_key_here" {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    // 3. General dotenvy fallback
    let _ = dotenvy::dotenv();
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() && trimmed != "your_openrouter_api_key_here" {
            return Ok(trimmed.to_string());
        }
    }

    Err(ModelEngineError::Auth(
        "OPENROUTER_API_KEY is not set in server/.env or environment".to_string(),
    ))
}

#[async_trait]
impl ModelEngine for OpenRouterAdapter {
    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn generate_image(&self, request: &GenerationRequest) -> Result<GenerationResponse, ModelEngineError> {
        if self.inner.api_key.trim().is_empty() {
            return Err(ModelEngineError::Auth("API key is empty".to_string()));
        }

        // 1. Acquire bounded concurrency permit from the worker pool
        debug!(
            model = %request.model,
            available_permits = self.inner.semaphore.available_permits(),
            "Acquiring worker pool permit for generation request..."
        );
        let _permit = self.inner.semaphore.acquire().await.map_err(|e| {
            ModelEngineError::WorkerPool(format!("Failed to acquire worker pool permit: {e}"))
        })?;

        let start_time = Instant::now();
        let api_type = Self::endpoint_type_for_model(&request.model);

        // 2. Dispatch request to appropriate endpoint
        let (payload, latency_ms) = match api_type {
            ModelApiType::Chat => {
                let endpoint = self.chat_endpoint_url();
                let mut body = json!({
                    "model": request.model,
                    "modalities": ["image", "text"],
                    "messages": [
                        {
                            "role": "user",
                            "content": request.prompt
                        }
                    ]
                });

                if let Some(sys) = &request.system_instruction {
                    if !sys.trim().is_empty() {
                        body["messages"] = json!([
                            {
                                "role": "system",
                                "content": sys
                            },
                            {
                                "role": "user",
                                "content": request.prompt
                            }
                        ]);
                    }
                }

                let res = self
                    .inner
                    .client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {}", self.inner.api_key))
                    .header("Content-Type", "application/json")
                    .header("X-Title", APPLICATION_TITLE)
                    .json(&body)
                    .send()
                    .await?;

                let status = res.status();
                let latency = start_time.elapsed().as_millis() as u64;

                if !status.is_success() {
                    let err_text = res.text().await.unwrap_or_default();
                    return Err(classify_http_error(status.as_u16(), &err_text));
                }

                let json: Value = res.json().await.map_err(|e| {
                    ModelEngineError::Api {
                        status: status.as_u16(),
                        message: format!("Failed to parse response JSON: {e}"),
                    }
                })?;

                (json, latency)
            }
            ModelApiType::Images => {
                let endpoint = self.images_endpoint_url();
                let full_prompt = if let Some(sys) = &request.system_instruction {
                    if !sys.trim().is_empty() {
                        format!("{sys}\n\n[USER REQUEST]\n{}", request.prompt)
                    } else {
                        request.prompt.clone()
                    }
                } else {
                    request.prompt.clone()
                };

                let body = json!({
                    "model": request.model,
                    "prompt": full_prompt,
                    "aspect_ratio": request.aspect_ratio
                });

                let res = self
                    .inner
                    .client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {}", self.inner.api_key))
                    .header("Content-Type", "application/json")
                    .header("X-Title", APPLICATION_TITLE)
                    .json(&body)
                    .send()
                    .await?;

                let status = res.status();
                let latency = start_time.elapsed().as_millis() as u64;

                if !status.is_success() {
                    let err_text = res.text().await.unwrap_or_default();
                    return Err(classify_http_error(status.as_u16(), &err_text));
                }

                let json: Value = res.json().await.map_err(|e| {
                    ModelEngineError::Api {
                        status: status.as_u16(),
                        message: format!("Failed to parse response JSON: {e}"),
                    }
                })?;

                (json, latency)
            }
        };

        // 3. Extract token metrics and calculate cost
        let (prompt_tokens, completion_tokens, cost_usd) = extract_usage_and_cost(&payload, &request.model);

        // 4. Extract raster image bytes
        let image_bytes = extract_image_bytes(&self.inner.client, &payload).await?;

        info!(
            model = %request.model,
            latency_ms = latency_ms,
            cost_usd = cost_usd,
            image_bytes_len = image_bytes.len(),
            "Generated image successfully"
        );

        Ok(GenerationResponse {
            image_bytes,
            model: request.model.clone(),
            prompt: request.prompt.clone(),
            prompt_tokens,
            completion_tokens,
            latency_ms,
            cost_usd,
        })
    }

    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError> {
        let request = GenerationRequest::new(model, archetype.prompt());
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

                join_set.spawn(async move {
                    adapter.run_benchmark(&m, &a).await
                });
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
                        "Task join error during benchmark suite: {join_err}"
                    )));
                }
            }
        }

        // Sort results deterministically by model then prompt_name
        results.sort_by(|a, b| {
            a.metadata
                .model
                .cmp(&b.metadata.model)
                .then_with(|| a.metadata.prompt_name.cmp(&b.metadata.prompt_name))
        });

        Ok(results)
    }
}

/// Classifies HTTP status codes and response messages into typed `ModelEngineError` variants.
fn classify_http_error(status: u16, text: &str) -> ModelEngineError {
    if status == 401 {
        return ModelEngineError::Auth(format!(
            "OpenRouter authentication failed (401): {text}"
        ));
    }
    if status == 402 || text.contains("insufficient_quota") || text.contains("credits") {
        return ModelEngineError::QuotaOrRateLimit(format!(
            "Insufficient OpenRouter credits or quota (status {status}): {text}"
        ));
    }
    if status == 429 {
        return ModelEngineError::QuotaOrRateLimit(format!(
            "OpenRouter rate limit exceeded (429): {text}"
        ));
    }

    ModelEngineError::Api {
        status,
        message: text.to_string(),
    }
}

/// Extracts token metrics and calculates USD cost from OpenRouter payload.
pub fn extract_usage_and_cost(payload: &Value, model_id: &str) -> (Option<u32>, Option<u32>, f64) {
    let usage = payload.get("usage");

    let prompt_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|t| t.as_u64())
        .map(|t| t as u32);

    let completion_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_u64())
        .map(|t| t as u32);

    // 1. Direct cost reporting from OpenRouter `usage.cost`
    let reported_cost = usage
        .and_then(|u| u.get("cost"))
        .and_then(|c| c.as_f64())
        .or_else(|| {
            usage
                .and_then(|u| u.get("cost_details"))
                .and_then(|cd| cd.get("upstream_inference_cost"))
                .and_then(|c| c.as_f64())
        });

    if let Some(cost) = reported_cost {
        return (prompt_tokens, completion_tokens, cost);
    }

    // 2. Fallback to model catalog pricing table
    if let Some(spec) = BenchmarkModelSpec::find(model_id) {
        let cost = spec.pricing.calculate_cost(prompt_tokens, completion_tokens);
        return (prompt_tokens, completion_tokens, cost);
    }

    // Default fallback based on model heuristics
    let default_cost = if model_id.contains("lite") {
        0.035
    } else if model_id.contains("flash") {
        0.070
    } else {
        0.010
    };

    (prompt_tokens, completion_tokens, default_cost)
}

/// Extracts image bytes from diverse OpenRouter response structures:
/// - `data[].b64_json` or `data[].url`
/// - `choices[0].message.images[]`
/// - `choices[0].message.content[]` (inline_data, image_url, source)
/// - `choices[0].message.content` (markdown images, data URLs, raw base64)
pub async fn extract_image_bytes(
    client: &reqwest::Client,
    payload: &Value,
) -> Result<Vec<u8>, ModelEngineError> {
    // 1. Check `data` array (standard image API response)
    if let Some(data) = payload.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(s) = item.as_str() {
                if let Some(bytes) = decode_image_source(client, s).await {
                    return Ok(bytes);
                }
            }
            if let Some(b64) = item.get("b64_json").and_then(|b| b.as_str()) {
                if let Ok(bytes) = decode_base64_string(b64) {
                    return Ok(bytes);
                }
            }
            if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                if let Some(bytes) = decode_image_source(client, url).await {
                    return Ok(bytes);
                }
            }
        }
    }

    // 2. Check `choices[0].message`
    if let Some(message) = payload
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|choice| choice.get("message"))
    {
        // 2a. `message.images` array
        if let Some(images) = message.get("images").and_then(|img| img.as_array()) {
            for img in images {
                if let Some(s) = img.as_str() {
                    if let Some(bytes) = decode_image_source(client, s).await {
                        return Ok(bytes);
                    }
                }
                if let Some(url) = img.get("url").and_then(|u| u.as_str()) {
                    if let Some(bytes) = decode_image_source(client, url).await {
                        return Ok(bytes);
                    }
                }
                if let Some(image_url) = img.get("image_url") {
                    if let Some(url) = image_url.get("url").and_then(|u| u.as_str()) {
                        if let Some(bytes) = decode_image_source(client, url).await {
                            return Ok(bytes);
                        }
                    }
                }
                if let Some(b64) = img.get("b64_json").and_then(|b| b.as_str()) {
                    if let Ok(bytes) = decode_base64_string(b64) {
                        return Ok(bytes);
                    }
                }
                if let Some(data) = img.get("data").and_then(|d| d.as_str()) {
                    if let Ok(bytes) = decode_base64_string(data) {
                        return Ok(bytes);
                    }
                }
            }
        }

        // 2b. `message.content` as array of parts
        if let Some(parts) = message.get("content").and_then(|c| c.as_array()) {
            for part in parts {
                if let Some(s) = part.as_str() {
                    if let Some(bytes) = decode_image_source(client, s).await {
                        return Ok(bytes);
                    }
                }
                if let Some(image_url) = part.get("image_url") {
                    if let Some(url) = image_url.get("url").and_then(|u| u.as_str()) {
                        if let Some(bytes) = decode_image_source(client, url).await {
                            return Ok(bytes);
                        }
                    }
                }
                if let Some(source) = part.get("source") {
                    if let Some(data) = source.get("data").and_then(|d| d.as_str()) {
                        if let Ok(bytes) = decode_base64_string(data) {
                            return Ok(bytes);
                        }
                    }
                }
                if let Some(inline) = part.get("inline_data") {
                    if let Some(data) = inline.get("data").and_then(|d| d.as_str()) {
                        if let Ok(bytes) = decode_base64_string(data) {
                            return Ok(bytes);
                        }
                    }
                }
            }
        }

        // 2c. `message.content` as raw string
        if let Some(content_str) = message.get("content").and_then(|c| c.as_str()) {
            if let Some(bytes) = extract_image_from_text(client, content_str).await {
                return Ok(bytes);
            }
        }
    }

    Err(ModelEngineError::ImageExtraction(
        "No supported image data (data URL, b64_json, or image URL) found in OpenRouter response"
            .to_string(),
    ))
}

/// Attempts to parse an image from raw text content (e.g. Markdown image tags, raw URLs, data URLs).
async fn extract_image_from_text(client: &reqwest::Client, text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Direct data URL
    if trimmed.starts_with("data:image/") {
        return decode_data_url(trimmed).ok();
    }

    // Markdown image syntax: ![alt](url)
    let md_re = Regex::new(r"!\[.*?\]\((https?://[^\s\)]+|data:image/[^\s\)]+)\)").ok()?;
    if let Some(captures) = md_re.captures(trimmed) {
        if let Some(match_url) = captures.get(1) {
            return decode_image_source(client, match_url.as_str()).await;
        }
    }

    // Direct HTTP(S) URL
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return fetch_url_bytes(client, trimmed).await.ok();
    }

    // Possible bare base64 payload
    if trimmed.len() > 100 {
        if let Ok(bytes) = decode_base64_string(trimmed) {
            if looks_like_image(&bytes) {
                return Some(bytes);
            }
        }
    }

    None
}

/// Decodes either a data URL or fetches an HTTP(S) URL.
async fn decode_image_source(client: &reqwest::Client, source: &str) -> Option<Vec<u8>> {
    let trimmed = source.trim();
    if trimmed.starts_with("data:image/") || trimmed.starts_with("data:") {
        decode_data_url(trimmed).ok()
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        fetch_url_bytes(client, trimmed).await.ok()
    } else if let Ok(bytes) = decode_base64_string(trimmed) {
        if looks_like_image(&bytes) {
            Some(bytes)
        } else {
            None
        }
    } else {
        None
    }
}

/// Decodes a data URI into raw bytes.
fn decode_data_url(data_url: &str) -> Result<Vec<u8>, ModelEngineError> {
    let comma_idx = data_url.find(',').ok_or_else(|| {
        ModelEngineError::ImageExtraction("Invalid data URL: missing comma separator".to_string())
    })?;

    let b64_part = &data_url[comma_idx + 1..];
    decode_base64_string(b64_part)
}

/// Decodes standard or URL-safe base64 string, ignoring any whitespace.
fn decode_base64_string(b64_str: &str) -> Result<Vec<u8>, ModelEngineError> {
    let cleaned: String = b64_str.chars().filter(|c| !c.is_whitespace()).collect();
    BASE64_STANDARD
        .decode(&cleaned)
        .or_else(|_| BASE64_STANDARD_NO_PAD.decode(&cleaned))
        .or_else(|_| BASE64_URL_SAFE.decode(&cleaned))
        .map_err(|e| {
            ModelEngineError::ImageExtraction(format!("Base64 decoding failed: {e}"))
        })
}

/// Downloads image bytes from an external HTTP/HTTPS URL.
async fn fetch_url_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, ModelEngineError> {
    let res = client
        .get(url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(ModelEngineError::Network)?;

    let status = res.status();
    if !status.is_success() {
        return Err(ModelEngineError::Api {
            status: status.as_u16(),
            message: format!("Failed to download image from URL: {url}"),
        });
    }

    let bytes = res.bytes().await.map_err(ModelEngineError::Network)?;
    Ok(bytes.to_vec())
}

/// Quick heuristic check for image header magic bytes (PNG, JPEG, WEBP, GIF).
fn looks_like_image(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    // PNG: \x89PNG
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return true;
    }
    // JPEG: \xFF\xD8\xFF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    // GIF: GIF8
    if bytes.starts_with(b"GIF8") {
        return true;
    }
    // RIFF....WEBP
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_PNG_B64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    #[test]
    fn test_looks_like_image() {
        let png_bytes = BASE64_STANDARD.decode(DUMMY_PNG_B64).unwrap();
        assert!(looks_like_image(&png_bytes));
        assert!(!looks_like_image(b"not an image file"));
    }

    #[test]
    fn test_decode_data_url() {
        let data_url = format!("data:image/png;base64,{DUMMY_PNG_B64}");
        let bytes = decode_data_url(&data_url).expect("decoded data url");
        assert!(looks_like_image(&bytes));
    }

    #[test]
    fn test_extract_usage_and_cost_reported() {
        let payload = json!({
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 500,
                "cost": 0.042
            }
        });

        let (p, c, cost) = extract_usage_and_cost(&payload, MODEL_GEMINI_31_FLASH);
        assert_eq!(p, Some(150));
        assert_eq!(c, Some(500));
        assert_eq!(cost, 0.042);
    }

    #[test]
    fn test_extract_usage_and_cost_calculated() {
        let payload = json!({
            "usage": {
                "prompt_tokens": 1_000_000,
                "completion_tokens": 1_000_000
            }
        });

        // Gemini Flash: 0.50 per M in, 3.00 per M out, + 0.00006 image output
        let (_, _, cost) = extract_usage_and_cost(&payload, MODEL_GEMINI_31_FLASH);
        assert!((cost - 3.50006).abs() < 1e-5);

        // Muse: flat 0.01
        let (_, _, muse_cost) = extract_usage_and_cost(&payload, MODEL_MUSE);
        assert_eq!(muse_cost, 0.01);
    }

    #[tokio::test]
    async fn test_extract_image_bytes_chat_format() {
        let client = reqwest::Client::new();
        let payload = json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": format!("Here is your map:\n![Map](data:image/png;base64,{DUMMY_PNG_B64})")
                    }
                }
            ]
        });

        let bytes = extract_image_bytes(&client, &payload)
            .await
            .expect("extracted image");
        assert!(looks_like_image(&bytes));
    }

    #[tokio::test]
    async fn test_extract_image_bytes_images_api_format() {
        let client = reqwest::Client::new();
        let payload = json!({
            "data": [
                {
                    "b64_json": DUMMY_PNG_B64
                }
            ]
        });

        let bytes = extract_image_bytes(&client, &payload)
            .await
            .expect("extracted image");
        assert!(looks_like_image(&bytes));
    }

    #[tokio::test]
    async fn test_concurrency_limiter_semaphore() {
        let adapter = OpenRouterAdapter::with_config("dummy_key", "http://localhost:1234", 2);
        assert_eq!(adapter.concurrency_limit(), 2);
        assert_eq!(adapter.available_permits(), 2);

        // Acquire permit 1
        let permit1 = adapter.semaphore().clone().acquire_owned().await.unwrap();
        assert_eq!(adapter.available_permits(), 1);

        // Acquire permit 2
        let permit2 = adapter.semaphore().clone().acquire_owned().await.unwrap();
        assert_eq!(adapter.available_permits(), 0);

        // Drop permit 1
        drop(permit1);
        assert_eq!(adapter.available_permits(), 1);

        // Drop permit 2
        drop(permit2);
        assert_eq!(adapter.available_permits(), 2);
    }

    #[test]
    fn test_endpoint_type_for_model() {
        assert_eq!(
            OpenRouterAdapter::endpoint_type_for_model(MODEL_GEMINI_31_FLASH),
            ModelApiType::Chat
        );
        assert_eq!(
            OpenRouterAdapter::endpoint_type_for_model(MODEL_GEMINI_31_FLASH_LITE),
            ModelApiType::Chat
        );
        assert_eq!(
            OpenRouterAdapter::endpoint_type_for_model(MODEL_MUSE),
            ModelApiType::Images
        );
        assert_eq!(
            OpenRouterAdapter::endpoint_type_for_model(MODEL_SEEDREAM_50_FLASH),
            ModelApiType::Images
        );
    }
}
