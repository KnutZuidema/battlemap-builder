//! Routing model engine multiplexing benchmark and generation requests across providers.
//!
//! Inspects model identity against the model catalog and provider kind (ADR-0010),
//! routing OpenRouter models to `OpenRouterAdapter` and FLUX 3 Klein LoRA models to `WaveSpeedAdapter`.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, instrument};

use crate::adapters::{OpenRouterAdapter, WaveSpeedAdapter};
use crate::domain::{
    build_wavespeed_prompt, generate_benchmark_id, BenchmarkArchetype, BenchmarkMetadata,
    BenchmarkModelSpec, BenchmarkResult, GenerationRequest, GenerationResponse, ModelDescriptor,
    ModelProvider,
};
use crate::ports::model_engine::{ModelEngine, ModelEngineError};

/// Multi-provider routing model engine directing requests by provider kind (ADR-0010).
#[derive(Clone)]
pub struct RoutingModelEngine {
    openrouter: Arc<dyn ModelEngine + Send + Sync>,
    wavespeed: Arc<dyn ModelEngine + Send + Sync>,
    registry: Option<Arc<crate::services::ModelRegistry>>,
}

impl RoutingModelEngine {
    /// Creates a new `RoutingModelEngine` with explicit engine ports for OpenRouter and WaveSpeed.
    pub fn new(
        openrouter: Arc<dyn ModelEngine + Send + Sync>,
        wavespeed: Arc<dyn ModelEngine + Send + Sync>,
    ) -> Self {
        Self {
            openrouter,
            wavespeed,
            registry: None,
        }
    }

    /// Attaches a model registry for dynamic LoRA adapter resolution (ADR-0010).
    pub fn with_registry(mut self, registry: Arc<crate::services::ModelRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Reference to configured model registry if attached.
    pub fn registry(&self) -> Option<&Arc<crate::services::ModelRegistry>> {
        self.registry.as_ref()
    }

    /// Convenience constructor wrapping concrete adapter instances.
    pub fn from_adapters(openrouter: OpenRouterAdapter, wavespeed: WaveSpeedAdapter) -> Self {
        Self::new(Arc::new(openrouter), Arc::new(wavespeed))
    }

    /// Direct reference to the OpenRouter model engine port.
    pub fn openrouter_engine(&self) -> &Arc<dyn ModelEngine + Send + Sync> {
        &self.openrouter
    }

    /// Direct reference to the WaveSpeed model engine port.
    pub fn wavespeed_engine(&self) -> &Arc<dyn ModelEngine + Send + Sync> {
        &self.wavespeed
    }

    /// Determines whether a given model identifier routes to WaveSpeed.
    pub fn is_wavespeed_model(model: &str) -> bool {
        if let Some(spec) = BenchmarkModelSpec::find(model) {
            return spec.provider == ModelProvider::WaveSpeed;
        }
        if model.starts_with("wavespeed")
            || model.contains("flux-3-klein")
            || model.ends_with("-edit")
        {
            return true;
        }
        !model.starts_with("google/")
            && !model.starts_with("meta/")
            && !model.starts_with("bytedance")
    }
    /// Asynchronously resolves a model descriptor via registry or static specs.
    pub async fn resolve_descriptor(&self, model: &str) -> Option<ModelDescriptor> {
        if let Some(reg) = &self.registry {
            if let Ok(Some(desc)) = reg.find_including_retired(model).await {
                return Some(desc);
            }
        }
        BenchmarkModelSpec::find(model).map(ModelDescriptor::from)
    }

    /// Asynchronously determines whether a model routes to WaveSpeed using the registry.
    pub async fn resolve_is_wavespeed(&self, model: &str) -> bool {
        if let Some(desc) = self.resolve_descriptor(model).await {
            return desc.provider == ModelProvider::WaveSpeed;
        }
        Self::is_wavespeed_model(model)
    }

    /// Resolves the destination model engine for a target model ID.
    pub fn engine_for_model(&self, model: &str) -> &Arc<dyn ModelEngine + Send + Sync> {
        if Self::is_wavespeed_model(model) {
            &self.wavespeed
        } else {
            &self.openrouter
        }
    }
}

#[async_trait]
impl ModelEngine for RoutingModelEngine {
    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn generate_image(&self, request: &GenerationRequest) -> Result<GenerationResponse, ModelEngineError> {
        let desc = self.resolve_descriptor(&request.model).await;
        let is_ws = desc
            .as_ref()
            .map(|d| d.provider == ModelProvider::WaveSpeed)
            .unwrap_or_else(|| Self::is_wavespeed_model(&request.model));
        debug!(
            model = %request.model,
            provider = if is_ws { "wavespeed" } else { "openrouter" },
            "Routing generation request to provider engine"
        );
        if is_ws {
            let mut req = request.clone();
            if req.loras.is_empty() {
                if let Some(d) = &desc {
                    if let (Some(path), Some(scale)) = (&d.lora_path, d.lora_scale) {
                        req.loras = vec![crate::domain::LoraWeight::new(path, scale)];
                    }
                }
            }
            self.wavespeed.generate_image(&req).await
        } else {
            self.openrouter.generate_image(request).await
        }
    }

    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError> {
        let desc = self.resolve_descriptor(model).await;
        let is_ws = desc
            .as_ref()
            .map(|d| d.provider == ModelProvider::WaveSpeed)
            .unwrap_or_else(|| Self::is_wavespeed_model(model));
        if is_ws {
            let prompt = build_wavespeed_prompt(archetype.prompt());
            let mut req = GenerationRequest::new(model, prompt);
            if let Some(d) = &desc {
                if let (Some(path), Some(scale)) = (&d.lora_path, d.lora_scale) {
                    req = req.with_lora(path, scale);
                }
            }
            let response = self.wavespeed.generate_image(&req).await?;
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
        } else {
            self.openrouter.run_benchmark(model, archetype).await
        }
    }

    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
        if models.is_empty() || archetypes.is_empty() {
            return Ok(Vec::new());
        }

        let mut wavespeed_models = Vec::new();
        let mut openrouter_models = Vec::new();
        for model in models {
            if self.resolve_is_wavespeed(model).await {
                wavespeed_models.push(model.clone());
            } else {
                openrouter_models.push(model.clone());
            }
        }

        let mut results = Vec::new();

        if !openrouter_models.is_empty() {
            let mut or_results = self
                .openrouter
                 .run_benchmark_suite(&openrouter_models, archetypes)
                .await?;
            results.append(&mut or_results);
        }

        if !wavespeed_models.is_empty() {
            for model in &wavespeed_models {
                for archetype in archetypes {
                    let res = self.run_benchmark(model, archetype).await?;
                    results.push(res);
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
