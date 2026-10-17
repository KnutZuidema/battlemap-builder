use crate::domain::{
    BenchmarkMetadata, Candidate, CandidateLayer, CuratedMap, CuratedMapUpdate, PurgeStagingResult,
    RedditSidecarMetadata, RejectionReason, StorageStats, Theme,
};
use thiserror::Error;

/// Errors arising from storage repository operations.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Item not found: {0}")]
    NotFound(String),

    #[error("Item already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("ZIP archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Port defining storage operations for staging, curated themes, and rejection records.
pub trait StorageRepository: Send + Sync {
    /// Save a candidate image and its metadata sidecar to staging.
    fn save_staging(&self, candidate: &Candidate, data: &[u8]) -> Result<(), StorageError>;

    /// Save a candidate image and its metadata sidecar to a specific subfolder within staging (e.g. "reddit").
    fn save_staging_subfolder(
        &self,
        subfolder: &str,
        candidate: &Candidate,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let _ = subfolder;
        self.save_staging(candidate, data)
    }

    /// Save a Reddit candidate image and companion sidecar metadata into staging/reddit/.
    fn save_staging_reddit(
        &self,
        metadata: &RedditSidecarMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let candidate = metadata.to_candidate();
        self.save_staging_subfolder("reddit", &candidate, data)
    }

    /// Save a generated candidate image and companion benchmark metadata into staging/generated/.
    fn save_staging_generated(
        &self,
        metadata: &BenchmarkMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let candidate = metadata.to_candidate();
        self.save_staging_subfolder("generated", &candidate, data)
    }

    /// Save a multi-layer candidate bundle (`manifest.json` and `0.png`, `1.png`, ...) to staging.
    fn save_staging_bundle(
        &self,
        subfolder: Option<&str>,
        candidate: &Candidate,
        layer_images: &[Vec<u8>],
    ) -> Result<(), StorageError> {
        let first = layer_images
            .first()
            .ok_or_else(|| StorageError::InvalidData("Bundle must contain at least one layer image".into()))?;
        match subfolder {
            Some(sub) => self.save_staging_subfolder(sub, candidate, first),
            None => self.save_staging(candidate, first),
        }
    }

    /// Retrieve the raw image bytes for a specific layer of a staged candidate by ID and layer index.
    fn get_staging_layer_image(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError> {
        if layer_index == 0 {
            self.get_staging_image(id)
        } else {
            Err(StorageError::NotFound(format!(
                "Staging layer {layer_index} for candidate '{id}' not found"
            )))
        }
    }

    /// Retrieve the raw image bytes for a staged candidate by ID.
    fn get_staging_image(&self, id: &str) -> Result<Vec<u8>, StorageError>;

    /// List all candidates currently in staging (including subfolders).
    fn list_staging(&self) -> Result<Vec<Candidate>, StorageError>;

    /// Delete a candidate and its image from staging by ID (across subfolders).
    fn delete_staging(&self, id: &str) -> Result<(), StorageError>;

    /// Check if a candidate ID exists in staging.
    fn is_in_staging(&self, id: &str) -> Result<bool, StorageError>;

    /// Check if a candidate ID exists in curated storage.
    fn is_curated(&self, id: &str) -> Result<bool, StorageError>;

    /// Check if a candidate ID has been previously rejected, is already in staging, or is already in curated.
    fn is_duplicate(&self, id: &str) -> Result<bool, StorageError> {
        if self.is_rejected(id)? {
            return Ok(true);
        }
        if self.is_in_staging(id)? {
            return Ok(true);
        }
        if self.is_curated(id)? {
            return Ok(true);
        }
        Ok(false)
    }

    /// Save a curated map image and its metadata sidecar directly to theme storage.
    fn save_curated(&self, map: &CuratedMap, data: &[u8]) -> Result<(), StorageError>;

    /// Retrieve the raw image bytes for a curated map by theme and ID.
    fn get_curated_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError>;

    /// Retrieve the raw image bytes for a specific layer of a curated map by theme, ID, and layer index.
    fn get_curated_layer_image(
        &self,
        theme: &Theme,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        if layer_index == 0 {
            self.get_curated_image(theme, id)
        } else {
            Err(StorageError::NotFound(format!(
                "Curated layer {layer_index} for map '{id}' not found"
            )))
        }
    }
    /// Retrieve the raw 1024x1024 LoRA-normalized PNG image bytes for a curated map by theme and ID.
    fn get_curated_lora_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError>;

    /// Retrieve the LoRA companion caption text (.txt) for a curated map by theme and ID.
    fn get_curated_lora_caption(&self, theme: &Theme, id: &str) -> Result<String, StorageError>;

    /// Check if a curated map has been LoRA normalized to 1024x1024.
    fn is_lora_normalized(&self, theme: &Theme, id: &str) -> Result<bool, StorageError>;

    /// List curated maps, optionally filtered by theme (or all if None).
    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError>;
    /// Package all curated maps' 1024x1024 PNG and companion .txt captions into a valid .zip archive.
    fn export_lora_dataset(&self, theme: Option<&Theme>) -> Result<Vec<u8>, StorageError>;
    /// Package selected curated maps' 1024x1024 PNG and companion .txt captions into a valid .zip archive.
    fn export_selected_lora_dataset(&self, ids: &[String]) -> Result<Vec<u8>, StorageError> {
        let _ = ids;
        Err(StorageError::InvalidData(
            "export_selected_lora_dataset not implemented".into(),
        ))
    }

    /// Retrieve the raw image bytes for a specific layer of a curated map directly by ID and layer index.
    fn get_curated_layer_image_by_id(
        &self,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let _ = (id, layer_index);
        Err(StorageError::NotFound(format!(
            "Curated layer {layer_index} for map '{id}' not found"
        )))
    }

    /// Delete an approved curated map bundle and log its rejection reason to rejection records.
    fn delete_curated(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let _ = (id, reason, notes);
        Err(StorageError::InvalidData(
            "delete_curated not implemented".into(),
        ))
    }

    /// Update metadata (themes, tags, description) and/or layer assignments for an existing curated map.
    fn update_curated(
        &self,
        id: &str,
        update: CuratedMapUpdate,
    ) -> Result<CuratedMap, StorageError> {
        let _ = (id, update);
        Err(StorageError::InvalidData(
            "update_curated not implemented".into(),
        ))
    }


    /// Log a rejection for a candidate ID to the rejection record file.
    fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Check if a candidate ID has been previously rejected.
    fn is_rejected(&self, id: &str) -> Result<bool, StorageError>;

    /// Curate a candidate from staging: atomically moves the image to curated/<theme>/ and writes <id>.json metadata.
    fn curate_staging(&self, map: &CuratedMap) -> Result<(), StorageError>;

    /// Reject a candidate from staging: deletes it from staging and logs rejection to rejections.json.
    fn reject_staging(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Alias for `curate_staging`.
    fn curate_candidate(&self, map: &CuratedMap) -> Result<(), StorageError> {
        self.curate_staging(map)
    }

    /// Alias for `reject_staging`.
    fn reject_candidate(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        self.reject_staging(id, reason, notes)
    }
    /// Update the layer assignments (`role`, `elevation`, etc.) for a staged candidate and persist to its manifest.
    fn update_staging_layers(
        &self,
        id: &str,
        layers: Vec<CandidateLayer>,
    ) -> Result<Candidate, StorageError> {
        let _ = (id, layers);
        Err(StorageError::InvalidData(
            "update_staging_layers not implemented".into(),
        ))
    }

    /// Split a multi-layer staged bundle (`layers.len() > 1`) into standalone 1-layer candidates (`{id}_0`, `{id}_1`, ...)
    /// with `parent_group_id: Some(id)`.
    fn split_staging_bundle(&self, id: &str) -> Result<Vec<Candidate>, StorageError> {
        let _ = id;
        Err(StorageError::InvalidData(
            "split_staging_bundle not implemented".into(),
        ))
    }

    /// Regroup all remaining staged candidates with `parent_group_id == Some(parent_id)` back into a single
    /// multi-layer bundle `{parent_id}` with re-inferred layer roles.
    fn regroup_staging_bundle(&self, parent_id: &str) -> Result<Candidate, StorageError> {
        let _ = parent_id;
        Err(StorageError::InvalidData(
            "regroup_staging_bundle not implemented".into(),
        ))
    }

    /// Returns comprehensive storage footprint statistics across staging, curated themes,
    /// rejections, and telemetry.
    fn get_storage_stats(&self) -> Result<StorageStats, StorageError>;

    /// Purges all unreviewed candidates and files from the staging area while strictly preserving
    /// curated themes, rejection history, and telemetry.
    fn purge_staging(&self) -> Result<PurgeStagingResult, StorageError>;
}
impl<T: ?Sized + StorageRepository> StorageRepository for std::sync::Arc<T> {
    fn save_staging(&self, candidate: &Candidate, data: &[u8]) -> Result<(), StorageError> {
        (**self).save_staging(candidate, data)
    }

    fn save_staging_subfolder(
        &self,
        subfolder: &str,
        candidate: &Candidate,
        data: &[u8],
    ) -> Result<(), StorageError> {
        (**self).save_staging_subfolder(subfolder, candidate, data)
    }

    fn save_staging_reddit(
        &self,
        metadata: &RedditSidecarMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        (**self).save_staging_reddit(metadata, data)
    }

    fn save_staging_generated(
        &self,
        metadata: &BenchmarkMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        (**self).save_staging_generated(metadata, data)
    }


    fn save_staging_bundle(
        &self,
        subfolder: Option<&str>,
        candidate: &Candidate,
        layer_images: &[Vec<u8>],
    ) -> Result<(), StorageError> {
        (**self).save_staging_bundle(subfolder, candidate, layer_images)
    }

    fn get_staging_layer_image(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError> {
        (**self).get_staging_layer_image(id, layer_index)
    }
    fn get_staging_image(&self, id: &str) -> Result<Vec<u8>, StorageError> {
        (**self).get_staging_image(id)
    }

    fn list_staging(&self) -> Result<Vec<Candidate>, StorageError> {
        (**self).list_staging()
    }

    fn delete_staging(&self, id: &str) -> Result<(), StorageError> {
        (**self).delete_staging(id)
    }

    fn save_curated(&self, map: &CuratedMap, data: &[u8]) -> Result<(), StorageError> {
        (**self).save_curated(map, data)
    }

    fn get_curated_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError> {
        (**self).get_curated_image(theme, id)
    }


    fn get_curated_layer_image(
        &self,
        theme: &Theme,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        (**self).get_curated_layer_image(theme, id, layer_index)
    }
    fn get_curated_lora_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError> {
        (**self).get_curated_lora_image(theme, id)
    }

    fn get_curated_lora_caption(&self, theme: &Theme, id: &str) -> Result<String, StorageError> {
        (**self).get_curated_lora_caption(theme, id)
    }

    fn is_lora_normalized(&self, theme: &Theme, id: &str) -> Result<bool, StorageError> {
        (**self).is_lora_normalized(theme, id)
    }
    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        (**self).list_curated(theme)
    }
    fn export_lora_dataset(&self, theme: Option<&Theme>) -> Result<Vec<u8>, StorageError> {
        (**self).export_lora_dataset(theme)
    }


    fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).log_rejection(id, reason, notes)
    }

    fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_rejected(id)
    }

    fn is_in_staging(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_in_staging(id)
    }

    fn is_curated(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_curated(id)
    }

    fn is_duplicate(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_duplicate(id)
    }

    fn curate_staging(&self, map: &CuratedMap) -> Result<(), StorageError> {
        (**self).curate_staging(map)
    }
    fn export_selected_lora_dataset(&self, ids: &[String]) -> Result<Vec<u8>, StorageError> {
        (**self).export_selected_lora_dataset(ids)
    }

    fn get_curated_layer_image_by_id(
        &self,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        (**self).get_curated_layer_image_by_id(id, layer_index)
    }

    fn delete_curated(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).delete_curated(id, reason, notes)
    }

    fn update_curated(
        &self,
        id: &str,
        update: CuratedMapUpdate,
    ) -> Result<CuratedMap, StorageError> {
        (**self).update_curated(id, update)
    }


    fn reject_staging(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).reject_staging(id, reason, notes)
    }

    fn update_staging_layers(
        &self,
        id: &str,
        layers: Vec<CandidateLayer>,
    ) -> Result<Candidate, StorageError> {
        (**self).update_staging_layers(id, layers)
    }

    fn split_staging_bundle(&self, id: &str) -> Result<Vec<Candidate>, StorageError> {
        (**self).split_staging_bundle(id)
    }

    fn regroup_staging_bundle(&self, parent_id: &str) -> Result<Candidate, StorageError> {
        (**self).regroup_staging_bundle(parent_id)
    }

    fn get_storage_stats(&self) -> Result<StorageStats, StorageError> {
        (**self).get_storage_stats()
    }

    fn purge_staging(&self) -> Result<PurgeStagingResult, StorageError> {
        (**self).purge_staging()
    }
}
