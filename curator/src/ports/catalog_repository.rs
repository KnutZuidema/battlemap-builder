//! Port for persisting domain entities (`Candidate`, `CuratedMap`, rejection records).

use async_trait::async_trait;

use crate::domain::{
    Candidate, CuratedMap, DatasetSnapshot, RejectionReason, RejectionRecord, Source, Theme,
};
use crate::ports::storage_error::StorageError;

/// Query filter for staging candidates. Every populated field must match (conjunction).
#[derive(Debug, Clone, Default)]
pub struct CandidateFilter {
    /// Restrict to candidates harvested from this provenance source.
    pub source: Option<Source>,
    /// Restrict to candidates grouped under this parent bundle id.
    pub parent_group_id: Option<String>,
    /// Maximum number of results taken after ordering by id.
    pub limit: Option<usize>,
    /// Number of leading results skipped after ordering by id.
    pub offset: Option<usize>,
}

/// Query filter for curated maps. Every populated field must match (conjunction).
#[derive(Debug, Clone, Default)]
pub struct CuratedFilter {
    /// Restrict to maps carrying this theme as one of their tags.
    pub theme: Option<Theme>,
    /// Restrict to maps carrying every one of these tags.
    pub tags: Vec<String>,
    /// Restrict to maps whose id is contained in this list.
    pub ids: Option<Vec<String>>,
    /// Maximum number of results taken after ordering by id.
    pub limit: Option<usize>,
    /// Number of leading results skipped after ordering by id.
    pub offset: Option<usize>,
}

/// Port defining persistence of catalogued domain entities.
///
/// Contract: `get_*` on a missing entity fails with [`StorageError::NotFound`], `delete_*` of a
/// missing entity succeeds (idempotent), and `save_*` overwrites any existing entity with the
/// same id. Filtered listings are ordered by id and honour `limit`/`offset`.
#[async_trait]
pub trait CatalogRepository: Send + Sync {
    /// Load the staging candidate with `id`.
    async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError>;

    /// Insert or overwrite the staging candidate.
    async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError>;

    /// List staging candidates matching `filter`, ordered by id.
    async fn list_candidates(&self, filter: &CandidateFilter) -> Result<Vec<Candidate>, StorageError>;

    /// Delete the staging candidate with `id`. Succeeds when it does not exist.
    async fn delete_candidate(&self, id: &str) -> Result<(), StorageError>;

    /// Load the curated map with `id`.
    async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError>;

    /// Insert or overwrite the curated map.
    async fn save_curated(&self, map: &CuratedMap) -> Result<(), StorageError>;

    /// List curated maps matching `filter`, ordered by id.
    async fn list_curated(&self, filter: &CuratedFilter) -> Result<Vec<CuratedMap>, StorageError>;

    /// Delete the curated map with `id`. Succeeds when it does not exist.
    async fn delete_curated(&self, id: &str) -> Result<(), StorageError>;

    /// List every record in the rejection log, in append order.
    ///
    /// Returns an empty list when no log has been written yet; a malformed log fails with
    /// [`StorageError::InvalidData`], matching the other catalog reads.
    async fn rejection_records(&self) -> Result<Vec<RejectionRecord>, StorageError>;

    /// Returns whether `id` has previously been rejected.
    async fn is_rejected(&self, id: &str) -> Result<bool, StorageError>;

    /// Append a rejection record for `id`.
    async fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Load the frozen dataset snapshot with `id`.
    async fn get_dataset_snapshot(&self, id: &str) -> Result<DatasetSnapshot, StorageError>;

    /// Insert or overwrite the dataset snapshot.
    async fn save_dataset_snapshot(&self, snapshot: &DatasetSnapshot) -> Result<(), StorageError>;

    /// List frozen dataset snapshots matching `theme` (or all if `None`), ordered by `created_at` descending.
    async fn list_dataset_snapshots(
        &self,
        theme: Option<&Theme>,
    ) -> Result<Vec<DatasetSnapshot>, StorageError>;

    /// Delete the dataset snapshot with `id`. Succeeds when it does not exist (idempotent).
    async fn delete_dataset_snapshot(&self, id: &str) -> Result<(), StorageError>;

    /// Returns whether any TrainingRun references the dataset snapshot with `id`.
    async fn is_dataset_snapshot_referenced(&self, id: &str) -> Result<bool, StorageError>;
    /// Save candidate to trash.
    async fn save_trashed_candidate(&self, candidate: &Candidate) -> Result<(), StorageError>;

    /// Retrieve a trashed candidate.
    async fn get_trashed_candidate(&self, id: &str) -> Result<Candidate, StorageError>;

    /// Delete a candidate from trash. Succeeds when not found.
    async fn delete_trashed_candidate(&self, id: &str) -> Result<(), StorageError>;

    /// Remove rejection record for `id` from rejection log. Succeeds when not found.
    async fn remove_rejection(&self, id: &str) -> Result<(), StorageError>;

    /// Returns the most recently logged rejection record, if any.
    async fn latest_rejection(&self) -> Result<Option<RejectionRecord>, StorageError>;
}

#[async_trait]
impl<T: ?Sized + CatalogRepository> CatalogRepository for std::sync::Arc<T> {
    async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        (**self).get_candidate(id).await
    }

    async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        (**self).save_candidate(candidate).await
    }

    async fn list_candidates(&self, filter: &CandidateFilter) -> Result<Vec<Candidate>, StorageError> {
        (**self).list_candidates(filter).await
    }

    async fn delete_candidate(&self, id: &str) -> Result<(), StorageError> {
        (**self).delete_candidate(id).await
    }

    async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError> {
        (**self).get_curated(id).await
    }

    async fn save_curated(&self, map: &CuratedMap) -> Result<(), StorageError> {
        (**self).save_curated(map).await
    }

    async fn list_curated(&self, filter: &CuratedFilter) -> Result<Vec<CuratedMap>, StorageError> {
        (**self).list_curated(filter).await
    }

    async fn delete_curated(&self, id: &str) -> Result<(), StorageError> {
        (**self).delete_curated(id).await
    }

    async fn rejection_records(&self) -> Result<Vec<RejectionRecord>, StorageError> {
        (**self).rejection_records().await
    }

    async fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_rejected(id).await
    }

    async fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).log_rejection(id, reason, notes).await
    }

    async fn get_dataset_snapshot(&self, id: &str) -> Result<DatasetSnapshot, StorageError> {
        (**self).get_dataset_snapshot(id).await
    }

    async fn save_dataset_snapshot(&self, snapshot: &DatasetSnapshot) -> Result<(), StorageError> {
        (**self).save_dataset_snapshot(snapshot).await
    }

    async fn list_dataset_snapshots(
        &self,
        theme: Option<&Theme>,
    ) -> Result<Vec<DatasetSnapshot>, StorageError> {
        (**self).list_dataset_snapshots(theme).await
    }

    async fn delete_dataset_snapshot(&self, id: &str) -> Result<(), StorageError> {
        (**self).delete_dataset_snapshot(id).await
    }

    async fn is_dataset_snapshot_referenced(&self, id: &str) -> Result<bool, StorageError> {
        (**self).is_dataset_snapshot_referenced(id).await
    async fn save_trashed_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        (**self).save_trashed_candidate(candidate).await
    }

    async fn get_trashed_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        (**self).get_trashed_candidate(id).await
    }

    async fn delete_trashed_candidate(&self, id: &str) -> Result<(), StorageError> {
        (**self).delete_trashed_candidate(id).await
    }

    async fn remove_rejection(&self, id: &str) -> Result<(), StorageError> {
        (**self).remove_rejection(id).await
    }

    async fn latest_rejection(&self) -> Result<Option<RejectionRecord>, StorageError> {
        (**self).latest_rejection().await
    }
}
