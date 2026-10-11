use crate::domain::{Candidate, CuratedMap, RejectionReason, Theme};
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

    /// Retrieve the raw image bytes for a staged candidate by ID.
    fn get_staging_image(&self, id: &str) -> Result<Vec<u8>, StorageError>;

    /// List all candidates currently in staging.
    fn list_staging(&self) -> Result<Vec<Candidate>, StorageError>;

    /// Delete a candidate and its image from staging by ID.
    fn delete_staging(&self, id: &str) -> Result<(), StorageError>;

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
