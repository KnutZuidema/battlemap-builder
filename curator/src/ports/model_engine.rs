//! Port trait defining interface for AI model generation and benchmark execution.

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{
    BenchmarkArchetype, BenchmarkResult, GenerationRequest, GenerationResponse,
};

/// Errors that can occur during AI model generation or benchmark execution.
#[derive(Debug, Error)]
pub enum ModelEngineError {
    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("Quota or rate limit exceeded: {0}")]
    QuotaOrRateLimit(String),

    #[error("Image extraction error: {0}")]
    ImageExtraction(String),

    #[error("Worker pool error: {0}")]
    WorkerPool(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Port defining interface for generating images and benchmark runs.
#[async_trait]
pub trait ModelEngine: Send + Sync {
    /// Generate a battlemap raster image for a single generation request.
    async fn generate_image(&self, request: &GenerationRequest) -> Result<GenerationResponse, ModelEngineError>;

    /// Run a benchmark for a single archetype and model, returning a complete benchmark result.
    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError>;

    /// Run a benchmark suite across multiple models and archetypes.
    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, ModelEngineError>;
}

#[async_trait]
impl<T: ?Sized + ModelEngine> ModelEngine for std::sync::Arc<T> {
    async fn generate_image(&self, request: &GenerationRequest) -> Result<GenerationResponse, ModelEngineError> {
        (**self).generate_image(request).await
    }

    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, ModelEngineError> {
        (**self).run_benchmark(model, archetype).await
    }

    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
        (**self).run_benchmark_suite(models, archetypes).await
    }
}
