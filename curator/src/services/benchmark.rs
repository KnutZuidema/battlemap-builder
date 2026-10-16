//! Service layer for orchestrating AI model generation benchmarks and persistence.

use std::path::Path;
use std::sync::Arc;

use thiserror::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::domain::{
    build_battlemap_prompt, generate_benchmark_id, BatchBenchmarkItem, BenchmarkArchetype,
    BenchmarkMetadata, BenchmarkResult, GenerationRequest, ALL_BENCHMARK_MODELS,
};
use crate::ports::cost_tracker::{CostTracker, CostTrackerError, GenerationCostRecord};
use crate::ports::model_engine::{ModelEngine, ModelEngineError};
use crate::ports::storage::{StorageError, StorageRepository};
use crate::services::reference_provider::{
    ReferenceProvenance, ReferenceProvider, ReferenceProviderError, ReferenceResolution,
};

/// Errors arising from benchmark service execution.
#[derive(Debug, Error)]
pub enum BenchmarkServiceError {
    #[error("Model engine error: {0}")]
    Engine(#[from] ModelEngineError),

    #[error("Storage repository error: {0}")]
    Storage(#[from] StorageError),

    #[error("Cost tracker error: {0}")]
    CostTracker(#[from] CostTrackerError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Reference provider error: {0}")]
    ReferenceProvider(#[from] ReferenceProviderError),
}

/// Real-time progress update event emitted during benchmark batch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkProgressUpdate {
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub completed: usize,
    pub total: usize,
    pub current_spend: f64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BenchmarkMetadata>,
}
/// Service orchestrating battlemap benchmark execution across models and storage persistence.
#[derive(Clone)]
pub struct BenchmarkService {
    engine: Arc<dyn ModelEngine + Send + Sync>,
    storage: Arc<dyn StorageRepository + Send + Sync>,
    cost_tracker: Option<Arc<dyn CostTracker + Send + Sync>>,
    reference_provider: ReferenceProvider,
    reference_conditioning_enabled: bool,
}

impl BenchmarkService {
    /// Creates a new `BenchmarkService` with model engine and storage repository ports.
    pub fn new(
        engine: Arc<dyn ModelEngine + Send + Sync>,
        storage: Arc<dyn StorageRepository + Send + Sync>,
    ) -> Self {
        let reference_provider = ReferenceProvider::new(storage.clone());
        Self {
            engine,
            storage,
            cost_tracker: None,
            reference_provider,
            reference_conditioning_enabled: true,
        }
    }

    /// Creates a new `BenchmarkService` with an attached cost tracker port.
    pub fn with_cost_tracker(
        engine: Arc<dyn ModelEngine + Send + Sync>,
        storage: Arc<dyn StorageRepository + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        let reference_provider = ReferenceProvider::new(storage.clone());
        Self {
            engine,
            storage,
            cost_tracker: Some(cost_tracker),
            reference_provider,
            reference_conditioning_enabled: true,
        }
    }

    /// Attaches a custom reference provider.
    pub fn with_reference_provider(mut self, reference_provider: ReferenceProvider) -> Self {
        self.reference_provider = reference_provider;
        self
    }

    /// Reference to the underlying reference provider.
    pub fn reference_provider(&self) -> &ReferenceProvider {
        &self.reference_provider
    }

    /// Enables or disables reference image conditioning for generation requests.
    pub fn with_reference_conditioning(mut self, enabled: bool) -> Self {
        self.reference_conditioning_enabled = enabled;
        self
    }

    /// Whether reference conditioning is currently enabled.
    pub fn reference_conditioning_enabled(&self) -> bool {
        self.reference_conditioning_enabled
    }

    /// Resolve reference image for a given batch item if reference conditioning is enabled.
    pub fn resolve_reference_for_item(
        &self,
        item: &BatchBenchmarkItem,
    ) -> Option<ReferenceResolution> {
        if !self.reference_conditioning_enabled {
            return None;
        }

        if let Ok(archetype) = item
            .slug
            .parse::<BenchmarkArchetype>()
            .or_else(|_| item.name.parse::<BenchmarkArchetype>())
        {
            self.reference_provider
                .resolve_reference_for_theme(&archetype.default_theme())
                .ok()
                .flatten()
        } else {
            self.reference_provider
                .resolve_reference_for_prompt(&item.prompt)
                .ok()
                .flatten()
        }
    }

    /// Reference to the underlying model engine.
    pub fn engine(&self) -> &Arc<dyn ModelEngine + Send + Sync> {
        &self.engine
    }

    /// Reference to the underlying storage repository.
    pub fn storage(&self) -> &Arc<dyn StorageRepository + Send + Sync> {
        &self.storage
    }

    /// Reference to the optional cost tracker.
    pub fn cost_tracker(&self) -> Option<&Arc<dyn CostTracker + Send + Sync>> {
        self.cost_tracker.as_ref()
    }

    /// Helper to record benchmark generation cost into the attached tracker if present.
    async fn record_cost(&self, metadata: &BenchmarkMetadata) -> Result<(), BenchmarkServiceError> {
        if let Some(cost_tracker) = &self.cost_tracker {
            let record = GenerationCostRecord::from(metadata);
            cost_tracker.record_generation(record).await?;
        }
        Ok(())
    }

    /// Run a benchmark for a single archetype on a specific model without persisting.
    pub async fn run_archetype(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, BenchmarkServiceError> {
        let prompt = build_battlemap_prompt(archetype.prompt());
        let mut request = GenerationRequest::new(model, prompt).with_battlemap_defaults();

        if self.reference_conditioning_enabled {
            let theme = archetype.default_theme();
            if let Ok(Some(resolution)) = self.reference_provider.resolve_reference_for_theme(&theme) {
                info!(
                    provenance = ?resolution.provenance,
                    "Applying visual reference conditioning to benchmark request"
                );
                request = request.with_image(resolution.image);
            }
        }

        let response = self.engine.generate_image(&request).await?;
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

        let result = BenchmarkResult::new(metadata, response.image_bytes);
        self.record_cost(&result.metadata).await?;
        Ok(result)
    }

    /// Run a benchmark for a single archetype and model, persisting raster output to
    /// `data/staging/generated/{id}.png` and metadata sidecar to `data/staging/generated/{id}.json`.
    #[instrument(skip(self), fields(model = %model, archetype = %archetype.name()))]
    pub async fn run_and_persist(
        &self,
        model: &str,
        archetype: &BenchmarkArchetype,
    ) -> Result<BenchmarkResult, BenchmarkServiceError> {
        let result = self.run_archetype(model, archetype).await?;
        info!(
            id = %result.metadata.id,
            model = %result.metadata.model,
            archetype = %result.metadata.prompt_name,
            cost_usd = result.metadata.cost_usd,
            latency_ms = result.metadata.latency_ms,
            "Persisting generated benchmark output to staging/generated/..."
        );

        self.storage
            .save_staging_generated(&result.metadata, &result.image_bytes)?;

        Ok(result)
    }
    /// Run a suite of benchmarks across multiple models and archetypes without persisting.
    pub async fn run_suite(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, BenchmarkServiceError> {
        let results = self.engine.run_benchmark_suite(models, archetypes).await?;
        for result in &results {
            self.record_cost(&result.metadata).await?;
        }
        Ok(results)
    }

    /// Run a suite of benchmarks across multiple models and archetypes, persisting each output.
    #[instrument(skip(self, models, archetypes))]
    pub async fn run_suite_and_persist(
        &self,
        models: &[String],
        archetypes: &[BenchmarkArchetype],
    ) -> Result<Vec<BenchmarkResult>, BenchmarkServiceError> {
        let results = self.engine.run_benchmark_suite(models, archetypes).await?;

        for result in &results {
            self.record_cost(&result.metadata).await?;
            self.storage
                .save_staging_generated(&result.metadata, &result.image_bytes)?;
        }

        info!(
            count = results.len(),
            "Successfully completed and persisted benchmark suite"
        );

        Ok(results)
    }

    /// Run the full standard benchmark suite (all 4 models x 6 archetypes = 24 combinations)
    /// and persist all raster images and JSON sidecars to `data/staging/generated/`.
    pub async fn run_full_benchmark_and_persist(
        &self,
    ) -> Result<Vec<BenchmarkResult>, BenchmarkServiceError> {
        let models: Vec<String> = ALL_BENCHMARK_MODELS.iter().map(|&m| m.to_string()).collect();
        self.run_suite_and_persist(&models, &BenchmarkArchetype::ALL)
            .await
    }

    /// Run a benchmark for a single item (archetype or custom prompt) and persist output.
    #[instrument(skip(self), fields(model = %model, item = %item.name))]
    pub async fn run_item_and_persist(
        &self,
        model: &str,
        item: &BatchBenchmarkItem,
    ) -> Result<BenchmarkResult, BenchmarkServiceError> {
        let result = if let Ok(archetype) = item
            .slug
            .parse::<BenchmarkArchetype>()
            .or_else(|_| item.name.parse::<BenchmarkArchetype>())
        {
            self.run_archetype(model, &archetype).await?
        } else {
            let prompt = build_battlemap_prompt(&item.prompt);
            let mut request = GenerationRequest::new(model, prompt).with_battlemap_defaults();

            if self.reference_conditioning_enabled {
                if let Ok(Some(resolution)) = self
                    .reference_provider
                    .resolve_reference_for_prompt(&item.prompt)
                {
                    info!(
                        provenance = ?resolution.provenance,
                        "Applying visual reference conditioning to benchmark request"
                    );
                    request = request.with_image(resolution.image);
                }
            }

            let response = self.engine.generate_image(&request).await?;
            let id = generate_benchmark_id(model, &item.slug);
            let metadata = BenchmarkMetadata::new(
                id,
                model,
                &item.name,
                &item.prompt,
                response.prompt_tokens,
                response.completion_tokens,
                response.latency_ms,
                response.cost_usd,
            );
            let bench_result = BenchmarkResult::new(metadata, response.image_bytes);
            self.record_cost(&bench_result.metadata).await?;
            bench_result
        };
        self.storage
            .save_staging_generated(&result.metadata, &result.image_bytes)?;

        Ok(result)
    }

    /// Execute a benchmark batch across models and items with live progress reporting and cancellation.
    pub async fn run_batch_with_progress<F>(
        &self,
        models: &[String],
        items: &[BatchBenchmarkItem],
        cancel_flag: Option<Arc<AtomicBool>>,
        mut on_progress: F,
    ) -> Result<Vec<BenchmarkResult>, BenchmarkServiceError>
    where
        F: FnMut(BenchmarkProgressUpdate) -> bool + Send,
    {
        let total = models.len() * items.len();
        if total == 0 {
            return Ok(Vec::new());
        }

        let mut completed = 0;
        let mut current_spend = 0.0;
        let mut results = Vec::with_capacity(total);

        let is_cancelled = || {
            if let Some(flag) = &cancel_flag {
                if flag.load(Ordering::Relaxed) {
                    return true;
                }
            }
            false
        };

        // Emit starting phase
        let cont = on_progress(BenchmarkProgressUpdate {
            phase: "starting".to_string(),
            model: None,
            prompt: None,
            completed: 0,
            total,
            current_spend: 0.0,
            message: format!("Starting benchmark batch of {total} generations..."),
            metadata: None,
        });

        if !cont || is_cancelled() {
            on_progress(BenchmarkProgressUpdate {
                phase: "cancelled".to_string(),
                model: None,
                prompt: None,
                completed: 0,
                total,
                current_spend: 0.0,
                message: "Benchmark batch cancelled before execution".to_string(),
                metadata: None,
            });
            return Ok(results);
        }

        for model in models {
            for item in items {
                if is_cancelled() {
                    on_progress(BenchmarkProgressUpdate {
                        phase: "cancelled".to_string(),
                        model: Some(model.clone()),
                        prompt: Some(item.name.clone()),
                        completed,
                        total,
                        current_spend,
                        message: format!("Benchmark batch cancelled at {} / {}", item.name, model),
                        metadata: None,
                    });
                    return Ok(results);
                }

                let cont = on_progress(BenchmarkProgressUpdate {
                    phase: "generating".to_string(),
                    model: Some(model.clone()),
                    prompt: Some(item.name.clone()),
                    completed,
                    total,
                    current_spend,
                    message: if let Some(ref_res) = self.resolve_reference_for_item(item) {
                        match ref_res.provenance {
                            ReferenceProvenance::Curated { id, .. } => {
                                format!(
                                    "Generating {} for {} (curated reference: {})",
                                    item.name, model, id
                                )
                            }
                            ReferenceProvenance::StaticFallback { name, .. } => {
                                format!(
                                    "Generating {} for {} (static reference: {})",
                                    item.name, model, name
                                )
                            }
                        }
                    } else {
                        format!("Generating {} for {}", item.name, model)
                    },
                    metadata: None,
                });

                if !cont || is_cancelled() {
                    on_progress(BenchmarkProgressUpdate {
                        phase: "cancelled".to_string(),
                        model: Some(model.clone()),
                        prompt: Some(item.name.clone()),
                        completed,
                        total,
                        current_spend,
                        message: format!("Benchmark batch cancelled at {} / {}", item.name, model),
                        metadata: None,
                    });
                    return Ok(results);
                }

                match self.run_item_and_persist(model, item).await {
                    Ok(result) => {
                        completed += 1;
                        current_spend += result.metadata.cost_usd;
                        let meta = result.metadata.clone();
                        results.push(result);

                        let cont = on_progress(BenchmarkProgressUpdate {
                            phase: "progress".to_string(),
                            model: Some(model.clone()),
                            prompt: Some(item.name.clone()),
                            completed,
                            total,
                            current_spend,
                            message: format!(
                                "Completed {} for {} ({}/{})",
                                item.name, model, completed, total
                            ),
                            metadata: Some(meta),
                        });

                        if !cont || is_cancelled() {
                            on_progress(BenchmarkProgressUpdate {
                                phase: "cancelled".to_string(),
                                model: Some(model.clone()),
                                prompt: Some(item.name.clone()),
                                completed,
                                total,
                                current_spend,
                                message: "Benchmark batch cancelled by user".to_string(),
                                metadata: None,
                            });
                            return Ok(results);
                        }
                    }
                    Err(err) => {
                        on_progress(BenchmarkProgressUpdate {
                            phase: "error".to_string(),
                            model: Some(model.clone()),
                            prompt: Some(item.name.clone()),
                            completed,
                            total,
                            current_spend,
                            message: format!("Failed {} for {}: {}", item.name, model, err),
                            metadata: None,
                        });
                        return Err(err);
                    }
                }
            }
        }

        on_progress(BenchmarkProgressUpdate {
            phase: "complete".to_string(),
            model: None,
            prompt: None,
            completed,
            total,
            current_spend,
            message: format!(
                "Benchmark batch completed: {}/{} generated (${:.4} total)",
                completed, total, current_spend
            ),
            metadata: None,
        });

        Ok(results)
    }
}

/// Helper function to atomically persist a generated raster output and companion JSON metadata
/// directly to a staging directory:
/// - `{staging_dir}/generated/{id}.png`
/// - `{staging_dir}/generated/{id}.json`
pub fn persist_generated_output(
    staging_dir: &Path,
    metadata: &BenchmarkMetadata,
    image_bytes: &[u8],
) -> Result<(), BenchmarkServiceError> {
    let generated_dir = staging_dir.join("generated");
    std::fs::create_dir_all(&generated_dir)?;

    let image_path = generated_dir.join(format!("{}.png", metadata.id));
    let sidecar_path = generated_dir.join(format!("{}.json", metadata.id));

    std::fs::write(&image_path, image_bytes)?;

    let json = serde_json::to_string_pretty(metadata)?;
    std::fs::write(&sidecar_path, json.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::GenerationResponse;
    use crate::ports::storage::StorageRepository;
    use async_trait::async_trait;
    use tempfile::TempDir;

    struct RecordingMockEngine {
        dummy_png: Vec<u8>,
        captured_requests: Arc<std::sync::Mutex<Vec<GenerationRequest>>>,
    }

    impl RecordingMockEngine {
        fn new() -> (Self, Arc<std::sync::Mutex<Vec<GenerationRequest>>>) {
            let captured_requests = Arc::new(std::sync::Mutex::new(Vec::new()));
            let engine = Self {
                dummy_png: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                captured_requests: captured_requests.clone(),
            };
            (engine, captured_requests)
        }
    }

    #[async_trait]
    impl ModelEngine for RecordingMockEngine {
        async fn generate_image(
            &self,
            request: &crate::domain::GenerationRequest,
        ) -> Result<GenerationResponse, ModelEngineError> {
            self.captured_requests.lock().unwrap().push(request.clone());
            Ok(GenerationResponse {
                image_bytes: self.dummy_png.clone(),
                model: request.model.clone(),
                prompt: request.prompt.clone(),
                prompt_tokens: Some(100),
                completion_tokens: Some(300),
                latency_ms: 250,
                cost_usd: 0.015,
            })
        }

        async fn run_benchmark(
            &self,
            model: &str,
            archetype: &BenchmarkArchetype,
        ) -> Result<BenchmarkResult, ModelEngineError> {
            let id = format!("test_{}_{}", model.replace('/', "_"), archetype.slug());
            let metadata = BenchmarkMetadata::new(
                id,
                model,
                archetype.name(),
                archetype.prompt(),
                Some(100),
                Some(300),
                250,
                0.015,
            );
            Ok(BenchmarkResult::new(metadata, self.dummy_png.clone()))
        }

        async fn run_benchmark_suite(
            &self,
            models: &[String],
            archetypes: &[BenchmarkArchetype],
        ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
            let mut results = Vec::new();
            for model in models {
                for archetype in archetypes {
                    results.push(self.run_benchmark(model, archetype).await?);
                }
            }
            Ok(results)
        }
    }

    struct MockEngine {
        dummy_png: Vec<u8>,
    }

    impl MockEngine {
        fn new() -> Self {
            Self {
                dummy_png: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            }
        }
    }
    #[async_trait]
    impl ModelEngine for MockEngine {
        async fn generate_image(
            &self,
            request: &crate::domain::GenerationRequest,
        ) -> Result<GenerationResponse, ModelEngineError> {
            Ok(GenerationResponse {
                image_bytes: self.dummy_png.clone(),
                model: request.model.clone(),
                prompt: request.prompt.clone(),
                prompt_tokens: Some(100),
                completion_tokens: Some(300),
                latency_ms: 250,
                cost_usd: 0.015,
            })
        }

        async fn run_benchmark(
            &self,
            model: &str,
            archetype: &BenchmarkArchetype,
        ) -> Result<BenchmarkResult, ModelEngineError> {
            let id = format!("test_{}_{}", model.replace('/', "_"), archetype.slug());
            let metadata = BenchmarkMetadata::new(
                id,
                model,
                archetype.name(),
                archetype.prompt(),
                Some(100),
                Some(300),
                250,
                0.015,
            );
            Ok(BenchmarkResult::new(metadata, self.dummy_png.clone()))
        }

        async fn run_benchmark_suite(
            &self,
            models: &[String],
            archetypes: &[BenchmarkArchetype],
        ) -> Result<Vec<BenchmarkResult>, ModelEngineError> {
            let mut results = Vec::new();
            for model in models {
                for archetype in archetypes {
                    results.push(self.run_benchmark(model, archetype).await?);
                }
            }
            Ok(results)
        }
    }

    #[tokio::test]
    async fn test_benchmark_service_run_and_persist() {
        use crate::adapters::FilesystemStorage;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let engine = Arc::new(MockEngine::new());
        let service = BenchmarkService::new(engine, storage.clone());

        let result = service
            .run_and_persist("meta/muse-image", &BenchmarkArchetype::StoneCrypt)
            .await
            .expect("run and persist succeeds");

        let generated_dir = temp_dir.path().join("staging").join("generated");
        let bundle_dir = generated_dir.join(&result.metadata.id);
        let png_file = if bundle_dir.join("0.png").is_file() {
            bundle_dir.join("0.png")
        } else {
            generated_dir.join(format!("{}.png", result.metadata.id))
        };
        let json_file = if bundle_dir.join("manifest.json").is_file() {
            bundle_dir.join("manifest.json")
        } else {
            generated_dir.join(format!("{}.json", result.metadata.id))
        };

        assert!(png_file.is_file(), "PNG file was created in staging/generated/");
        assert!(json_file.is_file(), "JSON file was created in staging/generated/");

        let json_content = std::fs::read_to_string(json_file).unwrap();
        assert!(json_content.contains("\"model\": \"meta/muse-image\""));
        assert!(json_content.contains("\"prompt_name\": \"Stone Crypt\""));

        // Verify that list_staging in storage discovers it as a candidate
        let candidates = storage.list_staging().expect("list staging succeeds");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, result.metadata.id);
        assert_eq!(candidates[0].source, crate::domain::Source::Generated);
    }

    #[tokio::test]
    async fn test_benchmark_service_with_cost_tracker() {
        use crate::adapters::{FileCostLedger, FilesystemStorage};

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let engine = Arc::new(MockEngine::new());
        let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");
        let cost_tracker: Arc<dyn CostTracker + Send + Sync> =
            Arc::new(FileCostLedger::new(&ledger_path));

        let service =
            BenchmarkService::with_cost_tracker(engine, storage, cost_tracker.clone());

        let result = service
            .run_and_persist("meta/muse-image", &BenchmarkArchetype::StoneCrypt)
            .await
            .expect("run and persist succeeds");

        let stats = cost_tracker.get_stats().await.expect("stats succeeds");
        assert_eq!(stats.total_generations, 1);
        assert_eq!(stats.session_generations, 1);
        assert!((stats.total_spend_usd - result.metadata.cost_usd).abs() < 1e-6);
        assert!((stats.session_spend_usd - result.metadata.cost_usd).abs() < 1e-6);
        assert_eq!(stats.history.len(), 1);
        assert_eq!(stats.history[0].id, result.metadata.id);
        assert_eq!(stats.history[0].prompt_name, "Stone Crypt");
        assert!(stats.by_model.contains_key("meta/muse-image"));
    }

    #[tokio::test]
    async fn test_benchmark_service_run_batch_with_progress() {
        use crate::adapters::FilesystemStorage;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let engine = Arc::new(MockEngine::new());
        let service = BenchmarkService::new(engine, storage.clone());

        let models = vec!["meta/muse-image".to_string()];
        let items = vec![
            BatchBenchmarkItem::from_archetype(&BenchmarkArchetype::StoneCrypt),
            BatchBenchmarkItem::new("Custom Tavern", "Flat 2D tavern interior with fireplace"),
        ];

        let mut updates = Vec::new();
        let results = service
            .run_batch_with_progress(&models, &items, None, |update| {
                updates.push(update);
                true
            })
            .await
            .expect("run batch succeeds");

        assert_eq!(results.len(), 2);
        assert!(updates.iter().any(|u| u.phase == "starting"));
        assert!(updates.iter().any(|u| u.phase == "generating"));
        assert!(updates.iter().any(|u| u.phase == "progress"));
        assert!(updates.iter().any(|u| u.phase == "complete"));

        let candidates = storage.list_staging().expect("list staging succeeds");
        assert_eq!(candidates.len(), 2);
    }

    #[tokio::test]
    async fn test_benchmark_service_cancellation() {
        use crate::adapters::FilesystemStorage;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let engine = Arc::new(MockEngine::new());
        let service = BenchmarkService::new(engine, storage.clone());

        let models = vec!["meta/muse-image".to_string(), "google/gemini-3.1-flash-image".to_string()];
        let items = vec![
            BatchBenchmarkItem::from_archetype(&BenchmarkArchetype::StoneCrypt),
            BatchBenchmarkItem::from_archetype(&BenchmarkArchetype::DesertRuins),
        ];

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let mut updates = Vec::new();

        let results = service
            .run_batch_with_progress(&models, &items, Some(cancel_flag), |update| {
                updates.push(update.clone());
                if update.completed == 1 {
                    cancel_flag_clone.store(true, Ordering::Relaxed);
                }
                true
            })
            .await
            .expect("batch with cancel succeeds");

        assert_eq!(results.len(), 1);
        assert!(updates.iter().any(|u| u.phase == "cancelled"));
    }

    #[tokio::test]
    async fn test_benchmark_service_visual_reference_static_fallback() {
        use crate::adapters::FilesystemStorage;
        use crate::domain::SYSTEM_INSTRUCTION_GENERATE;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let (engine, captured_requests) = RecordingMockEngine::new();
        let service = BenchmarkService::new(Arc::new(engine), storage);

        let result = service
            .run_archetype("meta/muse-image", &BenchmarkArchetype::StoneCrypt)
            .await
            .expect("run archetype succeeds");

        assert_eq!(result.metadata.prompt_name, "Stone Crypt");

        let requests = captured_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let req = &requests[0];

        // Verify battlemap defaults and prompt formatting
        assert_eq!(
            req.system_instruction.as_deref(),
            Some(SYSTEM_INSTRUCTION_GENERATE)
        );
        assert!(req.prompt.contains("Flat 2D stone dungeon crypt"));
        assert!(req.prompt.contains("64px = 5ft square"));
        // Verify attached reference image from static fallback
        assert_eq!(req.images.len(), 1);
        let ref_img = &req.images[0];
        assert_eq!(
            ref_img.label,
            "Perspective Reference: 2D Stone Dungeon Plan View"
        );
        assert_eq!(ref_img.mime_type, "image/jpeg");
        assert!(!ref_img.data.is_empty());

        // Verify preprocessed image is 512x512
        let decoded = image::load_from_memory(&ref_img.data).expect("valid jpeg");
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
    }

    #[tokio::test]
    async fn test_benchmark_service_visual_reference_curated_prioritization() {
        use chrono::Utc;
        use crate::adapters::FilesystemStorage;
        use crate::domain::{CuratedMap, Theme};
        use image::{ImageBuffer, Rgb};

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));

        // Save a 5-star curated map in the Dungeon theme
        let map = CuratedMap::new(
            "curated-crypt-champion",
            Theme::Dungeon,
            vec!["crypt".to_string(), "dungeon".to_string()],
            (1024, 1024),
            Utc::now(),
        );
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(256, 256, Rgb([50, 100, 150]));
        let mut png_bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();
        storage.save_curated(&map, &png_bytes).unwrap();

        let (engine, captured_requests) = RecordingMockEngine::new();
        let service = BenchmarkService::new(Arc::new(engine), storage);

        let result = service
            .run_archetype("meta/muse-image", &BenchmarkArchetype::StoneCrypt)
            .await
            .expect("run archetype succeeds");

        assert_eq!(result.metadata.prompt_name, "Stone Crypt");

        let requests = captured_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let req = &requests[0];

        // Verify curated map reference is attached instead of static fallback
        assert_eq!(req.images.len(), 1);
        let ref_img = &req.images[0];
        assert_eq!(
            ref_img.label,
            "Perspective Reference: 2D dungeon Plan View"
        );
        assert_eq!(ref_img.mime_type, "image/jpeg");

        // Verify preprocessed image is 512x512
        let decoded = image::load_from_memory(&ref_img.data).expect("valid jpeg");
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
    }

    #[tokio::test]
    async fn test_benchmark_service_custom_prompt_keyword_detection() {
        use crate::adapters::FilesystemStorage;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let (engine, captured_requests) = RecordingMockEngine::new();
        let service = BenchmarkService::new(Arc::new(engine), storage);

        // 1. Custom prompt matching Tavern
        let tavern_item = BatchBenchmarkItem::new(
            "Cozy Tavern",
            "Flat 2D fantasy tavern interior with hearth and bar counter",
        );
        service
            .run_item_and_persist("meta/muse-image", &tavern_item)
            .await
            .expect("run item succeeds");

        // 2. Custom prompt with no matching theme keywords
        let generic_item = BatchBenchmarkItem::new(
            "Abstract Test",
            "Random non-thematic test 998877",
        );
        service
            .run_item_and_persist("meta/muse-image", &generic_item)
            .await
            .expect("run item succeeds");

        let requests = captured_requests.lock().unwrap();
        assert_eq!(requests.len(), 2);

        // Tavern item should have resolved the tavern static reference
        assert_eq!(requests[0].images.len(), 1);
        assert_eq!(
            requests[0].images[0].label,
            "Perspective Reference: 2D Tavern & Rooms Plan View"
        );

        // Generic item without matching keywords should have no reference images attached
        assert!(requests[1].images.is_empty());
    }

    #[tokio::test]
    async fn test_benchmark_service_reference_conditioning_toggle() {
        use crate::adapters::FilesystemStorage;

        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let (engine, captured_requests) = RecordingMockEngine::new();
        let service = BenchmarkService::new(Arc::new(engine), storage)
            .with_reference_conditioning(false);

        assert!(!service.reference_conditioning_enabled());

        let result = service
            .run_archetype("meta/muse-image", &BenchmarkArchetype::StoneCrypt)
            .await
            .expect("run archetype succeeds");

        assert_eq!(result.metadata.prompt_name, "Stone Crypt");

        let requests = captured_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        // Images should be empty when reference conditioning is disabled
        assert!(requests[0].images.is_empty());
    }
}
