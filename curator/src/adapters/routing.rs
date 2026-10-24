//! Routing model engine multiplexing benchmark and generation requests across providers.
//!
//! Inspects model identity against the model catalog and provider kind (ADR-0010),
//! routing OpenRouter models to `OpenRouterAdapter` and FLUX 3 Klein LoRA models to `WaveSpeedAdapter`.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, instrument};

use crate::adapters::{OpenRouterAdapter, WaveSpeedAdapter};
use crate::domain::{
    BenchmarkArchetype, BenchmarkModelSpec, BenchmarkResult, GenerationRequest,
    GenerationResponse, ModelProvider,
};
use crate::ports::model_engine::{ModelEngine, ModelEngineError};

/// Multi-provider routing model engine directing requests by provider kind (ADR-0010).
#[derive(Clone)]
pub struct RoutingModelEngine {
    openrouter: Arc<dyn ModelEngine + Send + Sync>,
    wavespeed: Arc<dyn ModelEngine + Send + Sync>,
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
        }
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
        model.starts_with("wavespeed") || model.contains("flux-3-klein")
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
        let is_ws = Self::is_wavespeed_model(&request.model);
        debug!(
            model = %request.model,
            provider = if is_ws { "wavespeed" } else { "openrouter" },
            "Routing generation request to provider engine"
        );
        self.engine_for_model(&request.model).generate_image(request).await
    }

    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError> {
        self.engine_for_model(model).run_benchmark(model, archetype).await
    }

    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
        if models.is_empty() || archetypes.is_empty() {
            return Ok(Vec::new());
        }

        let (wavespeed_models, openrouter_models): (Vec<String>, Vec<String>) =
            models.iter().cloned().partition(|m| Self::is_wavespeed_model(m));

        let mut results = Vec::new();

        if !openrouter_models.is_empty() {
            let mut or_results = self
                .openrouter
                .run_benchmark_suite(&openrouter_models, archetypes)
                .await?;
            results.append(&mut or_results);
        }

        if !wavespeed_models.is_empty() {
            let mut ws_results = self
                .wavespeed
                .run_benchmark_suite(&wavespeed_models, archetypes)
                .await?;
            results.append(&mut ws_results);
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
