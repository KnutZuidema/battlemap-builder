//! Storage accounting and staging reclamation over the catalog and blob ports.
//!
//! The service reports the same [`StorageStats`] shape the filesystem repository used to compute
//! by walking directories, but derives every number from the ports: blob counts and byte sizes
//! come from [`BlobStorage::list_prefix`] plus [`BlobStorage::blob_size`] (never
//! [`BlobStorage::read_blob`]), while the per-theme curated breakdown and the rejection record
//! count come from the catalog.

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{
    DirectoryStats, PurgeStagingResult, StorageStats, Theme, ThemeStorageStats,
};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use crate::ports::storage_error::StorageError;

/// Service reporting storage usage and reclaiming the staging area.
///
/// Stateless: it borrows the catalog and blob ports, so handlers construct it per request from
/// cheap `Arc` clones.
#[derive(Clone)]
pub struct StorageMaintenanceService {
    catalog: Arc<dyn CatalogRepository>,
    blobs: Arc<dyn BlobStorage>,
}

impl StorageMaintenanceService {
    /// Creates a service reading staging candidates from `catalog` and blob sizes from `blobs`.
    pub fn new(catalog: Arc<dyn CatalogRepository>, blobs: Arc<dyn BlobStorage>) -> Self {
        Self { catalog, blobs }
    }

    /// Aggregates the storage footprint of the staging and curated areas.
    ///
    /// `staging_breakdown` keeps its three legacy keys: staging is one uniform `staging/{id}/`
    /// bundle layout with no nested per-source subdirectories, so the entire footprint sits in
    /// `root` (`root` equals `staging`), while `reddit` and `generated` stay present — the SPA
    /// renders all three — at zero. `rejected` is the rejection log blob's byte size plus the
    /// log's *record count* (the only `DirectoryStats.file_count` that counts records rather than
    /// files, because the operator UI renders it as `N records`), and `telemetry` the byte size
    /// and file count of the telemetry log blobs below `telemetry/`.
    pub async fn storage_stats(&self) -> Result<StorageStats, StorageError> {
        let staging = self.directory_stats(&BlobKey::StagingDir { id: None }).await?;
        let curated = self.directory_stats(&BlobKey::CuratedDir { id: None }).await?;

        let mut staging_breakdown = HashMap::new();
        staging_breakdown.insert("root".to_string(), staging);
        staging_breakdown.insert("reddit".to_string(), DirectoryStats::default());
        staging_breakdown.insert("generated".to_string(), DirectoryStats::default());

        let mut curated_by_theme = HashMap::new();
        for theme in Theme::ALL {
            curated_by_theme.insert(*theme, ThemeStorageStats::new(*theme, 0, 0, 0));
        }

        for map in self.catalog.list_curated(&CuratedFilter::default()).await? {
            let map_stats = self
                .directory_stats(&BlobKey::CuratedDir {
                    id: Some(map.id.clone()),
                })
                .await?;
            for tag in &map.tags {
                if let Ok(theme) = tag.parse::<Theme>() {
                    if let Some(entry) = curated_by_theme.get_mut(&theme) {
                        entry.map_count += 1;
                        entry.bytes += map_stats.bytes;
                        entry.file_count += map_stats.file_count;
                    }
                }
            }
        }

        // The rejection log's two numbers are measured independently and mean different things:
        // `bytes` is the log blob's file size (an absent log is zero, not an error) while
        // `file_count` is the number of rejection records the catalog holds. The latter matches the
        // pre-cut-over storage repository's stats implementation, which filled the field with the
        // rejection record count and which the operator UI renders as `N records`.
        let rejected_bytes = match self.blobs.blob_size(&BlobKey::RejectionLog).await {
            Ok(bytes) => bytes,
            Err(StorageError::NotFound(_)) => 0,
            Err(err) => return Err(err),
        };
        let rejected = DirectoryStats::new(
            rejected_bytes,
            self.catalog.rejection_records().await?.len(),
        );
        let telemetry = self.directory_stats(&BlobKey::TelemetryDir).await?;

        let total_bytes = staging.bytes + curated.bytes + rejected.bytes + telemetry.bytes;
        let total_files =
            staging.file_count + curated.file_count + rejected.file_count + telemetry.file_count;

        Ok(StorageStats {
            staging,
            staging_breakdown,
            curated,
            curated_by_theme,
            rejected,
            telemetry,
            total_bytes,
            total_files,
        })
    }

    /// Deletes every staging blob and the staging candidates describing them.
    ///
    /// Curated bundles (`curated/`), the rejection log and the telemetry ledgers are never
    /// touched: only the `staging/` prefix is enumerated, and the catalog rows removed are exactly
    /// the staging candidates.
    pub async fn purge_staging(&self) -> Result<PurgeStagingResult, StorageError> {
        let mut purged_files = 0usize;
        let mut reclaimed_bytes = 0u64;

        for key in self.blobs.list_prefix(&BlobKey::StagingDir { id: None }).await? {
            reclaimed_bytes += self.blobs.blob_size(&key).await?;
            self.blobs.delete_blob(&key).await?;
            purged_files += 1;
        }

        for candidate in self.catalog.list_candidates(&CandidateFilter::default()).await? {
            self.catalog.delete_candidate(&candidate.id).await?;
        }

        let remaining_staging_files = self
            .blobs
            .list_prefix(&BlobKey::StagingDir { id: None })
            .await?
            .len();

        Ok(PurgeStagingResult {
            purged_files,
            reclaimed_bytes,
            remaining_staging_files,
        })
    }

    /// Byte size and file count of every blob below `prefix`.
    ///
    /// A key that disappears between the listing and its size read is skipped instead of failing
    /// the aggregation: a listing is a snapshot, and a concurrent writer deleting the blob (or
    /// renaming its atomic-write temp file away) simply means that key is no longer part of the
    /// footprint — counting it as an error would turn an ordinary race into a `404` from
    /// `GET /api/storage/stats`. The skipped key contributes neither bytes nor a file count.
    /// Every other error — including a genuine I/O failure — still propagates.
    async fn directory_stats(&self, prefix: &BlobKey) -> Result<DirectoryStats, StorageError> {
        let mut bytes = 0u64;
        let mut file_count = 0usize;
        for key in self.blobs.list_prefix(prefix).await? {
            match self.blobs.blob_size(&key).await {
                Ok(size) => {
                    bytes += size;
                    file_count += 1;
                }
                Err(StorageError::NotFound(_)) => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(DirectoryStats::new(bytes, file_count))
    }
}
