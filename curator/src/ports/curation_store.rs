use crate::domain::{Candidate, CuratedMap, RejectionReason};
use crate::ports::storage::StorageError;

/// Narrow persistence seam for staging candidate lookup, curation commit, and rejection.
pub trait CurationStore: Send + Sync {
    /// Look up candidate directly in staging directory without scanning full directory ($O(1)$).
    fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError>;

    /// Retrieve layer image bytes from staging.
    fn get_staging_layer_bytes(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError>;

    /// Atomically commit an approved candidate into curated storage:
    /// performs atomic moves, writes `<id>/manifest.json`, writes LoRA companion files
    /// (`<id>_lora1024.png` / `<id>_lora1024.txt`), and cleans up staging bundle.
    fn commit_curation(&self, map: &CuratedMap) -> Result<(), StorageError>;

    /// Reject candidate: purges staging bundle and records rejection log.
    fn reject_candidate(&self, id: &str, reason: RejectionReason, notes: Option<&str>) -> Result<(), StorageError>;
}
