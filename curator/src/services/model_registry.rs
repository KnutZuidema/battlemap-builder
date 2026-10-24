//! Derived Model Registry computing model identity and capabilities at read time (ADR-0010).
//!
//! Exposes available AI models derived dynamically from the OpenRouter model catalog
//! and published, non-retired `LoraAdapter` descriptors stored in `CatalogRepository`
//! whose checkpoint artifacts exist in `BlobStorage`.

use std::sync::Arc;

use crate::domain::{
    BenchmarkModelSpec, LoraAdapter, ModelApiType, ModelDescriptor, ModelPricing, ModelProvider,
    MODEL_GEMINI_31_FLASH, MODEL_GEMINI_31_FLASH_LITE, MODEL_MUSE, MODEL_SEEDREAM_50_FLASH,
};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::storage_error::StorageError;

/// Derived registry managing model identity, pricing, and capabilities across providers.
#[derive(Clone)]
pub struct ModelRegistry {
    catalog: Arc<dyn CatalogRepository + Send + Sync>,
    blobs: Arc<dyn BlobStorage + Send + Sync>,
}

impl ModelRegistry {
    /// Creates a new `ModelRegistry` backed by the catalog and blob storage ports.
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        Self { catalog, blobs }
    }

    /// Returns the static list of standard OpenRouter baseline model descriptors.
    pub fn openrouter_models() -> Vec<ModelDescriptor> {
        vec![
            BenchmarkModelSpec::find(MODEL_GEMINI_31_FLASH).unwrap().into(),
            BenchmarkModelSpec::find(MODEL_GEMINI_31_FLASH_LITE).unwrap().into(),
            BenchmarkModelSpec::find(MODEL_MUSE).unwrap().into(),
            BenchmarkModelSpec::find(MODEL_SEEDREAM_50_FLASH).unwrap().into(),
        ]
    }

    /// Whether a LoRA adapter's checkpoint weights artifact exists in storage.
    ///
    /// Mirrors [`crate::services::TrainingService::list_adapters`]`(selectable_only = true)`: the
    /// checkpoint blob key is derived from the adapter's run and step, so an entry is selectable
    /// only while the exact weights it names are still present.
    pub async fn adapter_artifact_exists(&self, adapter: &LoraAdapter) -> bool {
        self.blobs
            .stat_blob(&BlobKey::CheckpointAdapter {
                run_id: adapter.run_id.clone(),
                step: adapter.step,
            })
            .await
            .is_ok()
    }

    /// Produces two selectable entries for a published `LoraAdapter`:
    /// 1. `{adapter.id}` for the text-to-image inference engine ($0.015/image).
    /// 2. `{adapter.id}-edit` for the multi-modal edit inference engine ($0.021/image).
    pub fn adapter_descriptors(adapter: &LoraAdapter) -> (ModelDescriptor, ModelDescriptor) {
        let t2i_id = adapter.id.clone();
        let edit_id = format!("{}-edit", adapter.id);

        let t2i = ModelDescriptor {
            id: t2i_id.clone(),
            display_name: format!("FLUX 3 Klein LoRA ({})", adapter.id),
            provider: ModelProvider::WaveSpeed,
            api_type: ModelApiType::Images,
            has_vision: false,
            supports_reference_image: false,
            pricing: ModelPricing::per_image(0.015),
            estimated_cost_per_image: 0.015,
            adapter_id: Some(adapter.id.clone()),
            run_id: Some(adapter.run_id.clone()),
            step: Some(adapter.step),
            lora_path: Some(adapter.artifact_url.clone()),
            lora_scale: Some(adapter.scale),
        };

        let edit = ModelDescriptor {
            id: edit_id.clone(),
            display_name: format!("FLUX 3 Klein Edit LoRA ({})", adapter.id),
            provider: ModelProvider::WaveSpeed,
            api_type: ModelApiType::Images,
            has_vision: true,
            supports_reference_image: true,
            pricing: ModelPricing::per_image(0.021),
            estimated_cost_per_image: 0.021,
            adapter_id: Some(adapter.id.clone()),
            run_id: Some(adapter.run_id.clone()),
            step: Some(adapter.step),
            lora_path: Some(adapter.artifact_url.clone()),
            lora_scale: Some(adapter.scale),
        };

        (t2i, edit)
    }

    /// List all selectable model descriptors derived at read time from OpenRouter specs
    /// plus published, non-retired LoRA adapters whose artifacts exist in storage (ADR-0010).
    pub async fn list_selectable(&self) -> Result<Vec<ModelDescriptor>, StorageError> {
        let mut list = Self::openrouter_models();

        let adapters = self.catalog.list_lora_adapters().await?;
        for adapter in adapters {
            if adapter.retired {
                continue;
            }
            if self.adapter_artifact_exists(&adapter).await {
                let (t2i, edit) = Self::adapter_descriptors(&adapter);
                list.push(t2i);
                list.push(edit);
            }
        }

        Ok(list)
    }

    /// Finds a selectable model descriptor by model ID.
    pub async fn find(&self, model_id: &str) -> Result<Option<ModelDescriptor>, StorageError> {
        let models = self.list_selectable().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }

    /// Resolves a model descriptor by ID even if the adapter is retired or its artifact is missing.
    /// Used for historical benchmark results and cost ledger accounting (ADR-0010).
    pub async fn find_including_retired(
        &self,
        model_id: &str,
    ) -> Result<Option<ModelDescriptor>, StorageError> {
        if let Some(desc) = self.find(model_id).await? {
            return Ok(Some(desc));
        }

        // A retired adapter, or one whose artifact vanished, still resolves for recorded results
        // and cost entries: the descriptor is rebuilt from its persisted record.
        let is_edit = model_id.ends_with("-edit");
        let base_id = model_id.strip_suffix("-edit").unwrap_or(model_id);
        if let Ok(adapter) = self.catalog.get_lora_adapter(base_id).await {
            let (t2i, edit) = Self::adapter_descriptors(&adapter);
            return Ok(Some(if is_edit { edit } else { t2i }));
        }

        Ok(BenchmarkModelSpec::find(model_id).map(ModelDescriptor::from))
    }
}
