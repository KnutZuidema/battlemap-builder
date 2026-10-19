use crate::domain::{Candidate, CuratedMap, RejectionReason};
use crate::ports::storage::StorageError;

/// Narrow persistence seam for staging candidate lookup, curation commit, and rejection.
pub trait CurationStore: Send + Sync {
    /// Look up candidate directly in staging directory without scanning full directory ($O(1)$).
    fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError>;

    /// Retrieve layer image bytes from staging.
    fn get_staging_layer_bytes(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError>;

    /// Atomically commit an approved candidate into curated storage:
    /// performs atomic moves, writes `<id>/manifest.json`, and cleans up the staging bundle.
    /// No `<id>_lora1024.*` companions are written: the dynamic dataset ZIP export derives
    /// per-MapSection LoRA assets on demand.
    ///
    /// Returns the `CuratedMap` exactly as persisted to disk, so callers never have to
    /// re-derive a response that could disagree with the stored manifest.
    fn commit_curation(&self, map: &CuratedMap) -> Result<CuratedMap, StorageError>;

    /// Reject candidate: purges staging bundle and records rejection log.
    fn reject_candidate(&self, id: &str, reason: RejectionReason, notes: Option<&str>) -> Result<(), StorageError>;
}

impl<T: ?Sized + CurationStore> CurationStore for std::sync::Arc<T> {
    fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        (**self).get_staging_candidate(id)
    }

    fn get_staging_layer_bytes(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError> {
        (**self).get_staging_layer_bytes(id, layer_index)
    }

    fn commit_curation(&self, map: &CuratedMap) -> Result<CuratedMap, StorageError> {
        (**self).commit_curation(map)
    }

    fn reject_candidate(&self, id: &str, reason: RejectionReason, notes: Option<&str>) -> Result<(), StorageError> {
        (**self).reject_candidate(id, reason, notes)
    }
}
