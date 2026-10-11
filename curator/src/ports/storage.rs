use crate::domain::{
    BenchmarkMetadata, Candidate, CuratedMap, RedditSidecarMetadata, RejectionReason, Theme,
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

    /// List curated maps, optionally filtered by theme (or all if None).
    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError>;

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

    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        (**self).list_curated(theme)
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

    fn reject_staging(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).reject_staging(id, reason, notes)
    }
}
