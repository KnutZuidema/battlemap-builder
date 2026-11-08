//! Storage accounting and staging reclamation over the catalog and blob ports.
//!
//! The service reports the same [`StorageStats`] shape the filesystem repository used to compute
//! by walking directories, but derives every number from the ports: blob counts and byte sizes
//! come from a single [`BlobStorage::list_prefix_with_sizes`] per prefix, while the per-theme
//! curated breakdown and the rejection record count come from the catalog.
//!
//! # Why the sizes come from the listing
//!
//! `GET /api/storage/stats` is a plain read an operator opens a drawer with, yet its cost used to
//! scale with the *number of blobs*: the listing returned keys only, so accounting called
//! [`BlobStorage::blob_size`] once per blob — an individual signed `head_object` over the WAN for
//! every object on S3. Against Cloudflare R2 that was 931 sequential HTTPS calls and ~94 seconds
//! for one drawer open, long enough to starve the liveness probe and have kubelet restart the pod
//! (issue #292).
//!
//! The listing response already carries each object's size, so the whole footprint now costs one
//! paginated call per prefix: `staging/`, `curated/`, `telemetry/`, plus the rejection log's
//! single size query. The per-theme curated breakdown is then computed in memory by joining that
//! one `curated/` listing against `catalog.list_curated()`, instead of issuing one listing plus a
//! `head_object` batch per curated map.

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{
    CuratedMap, DirectoryStats, PurgeStagingResult, StorageStats, Theme, ThemeStorageStats,
};
use crate::ports::blob_storage::{BlobEntry, BlobKey, BlobStorage};
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
    ///
    /// Four storage reads in total, all concurrent: the `staging/`, `curated/` and `telemetry/`
    /// listings plus the rejection log's size. Nothing here scales with the blob count (see the
    /// module docs).
    pub async fn storage_stats(&self) -> Result<StorageStats, StorageError> {
        // The filter is bound to a `let` rather than written inline: `tokio::join!` holds each
        // future across the whole wait, and the future returned by `list_curated` borrows the
        // filter, so a `&CuratedFilter::default()` temporary created in the macro's argument
        // position would be dropped at the end of this statement while still borrowed (E0716).
        // The `BlobKey` literals below need no such binding: they are constants that the compiler
        // promotes to `'static`.
        let curated_filter = CuratedFilter::default();
        let (staging, curated_listing, curated_maps, telemetry, rejected_bytes) = tokio::join!(
            self.directory_stats(&BlobKey::StagingDir { id: None }),
            self.blobs
                .list_prefix_with_sizes(&BlobKey::CuratedDir { id: None }),
            self.catalog.list_curated(&curated_filter),
            self.directory_stats(&BlobKey::TelemetryDir),
            self.rejection_log_size(),
        );

        let staging = staging?;
        let curated_listing = curated_listing?;
        let curated_maps = curated_maps?;
        let telemetry = telemetry?;
        let rejected_bytes = rejected_bytes?;

        let curated = directory_stats_of(&curated_listing);
        let curated_by_theme = theme_breakdown(&curated_maps, &curated_listing);

        let mut staging_breakdown = HashMap::new();
        staging_breakdown.insert("root".to_string(), staging);
        staging_breakdown.insert("reddit".to_string(), DirectoryStats::default());
        staging_breakdown.insert("generated".to_string(), DirectoryStats::default());

        // The rejection log's two numbers are measured independently and mean different things:
        // `bytes` is the log blob's file size (an absent log is zero, not an error) while
        // `file_count` is the number of rejection records the catalog holds. The latter matches the
        // pre-cut-over storage repository's stats implementation, which filled the field with the
        // rejection record count and which the operator UI renders as `N records`.
        let rejected = DirectoryStats::new(rejected_bytes, self.catalog.rejection_records().await?.len());

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
    /// the staging candidates. The reclaimed byte total comes from the sizing listing, so the
    /// purge issues no per-blob size query beyond the deletes themselves.
    pub async fn purge_staging(&self) -> Result<PurgeStagingResult, StorageError> {
        let mut purged_files = 0usize;
        let mut reclaimed_bytes = 0u64;

        let staging_entries = self
            .blobs
            .list_prefix_with_sizes(&BlobKey::StagingDir { id: None })
            .await?;
        for entry in &staging_entries {
            self.blobs.delete_blob(&entry.key).await?;
            reclaimed_bytes += entry.size;
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

    /// Byte size of the rejection log, or zero when no log has been written yet.
    ///
    /// The log is one known key rather than a prefix, so this is a single-object size query. An
    /// absent log is an empty footprint, not an error — the operator UI renders `0 records`.
    async fn rejection_log_size(&self) -> Result<u64, StorageError> {
        match self.blobs.blob_size(&BlobKey::RejectionLog).await {
            Ok(bytes) => Ok(bytes),
            Err(StorageError::NotFound(_)) => Ok(0),
            Err(err) => Err(err),
        }
    }

    /// Byte size and file count of every blob below `prefix`, from one sizing listing.
    ///
    /// A key that disappears between the listing and its size being reported is simply absent
    /// from that listing — a listing is a snapshot, and a concurrent writer deleting the blob (or
    /// renaming its atomic-write temp file away) means that key is no longer part of the
    /// footprint. Adapters that can only enumerate keys keep that rule by skipping the key, so an
    /// ordinary race never turns into a `404` from `GET /api/storage/stats`; every other error —
    /// including a genuine I/O failure — still propagates.
    async fn directory_stats(&self, prefix: &BlobKey) -> Result<DirectoryStats, StorageError> {
        Ok(directory_stats_of(
            &self.blobs.list_prefix_with_sizes(prefix).await?,
        ))
    }
}

/// Sums a sizing listing into the byte size and file count of the directory it came from.
fn directory_stats_of(entries: &[BlobEntry]) -> DirectoryStats {
    let mut bytes = 0u64;
    for entry in entries {
        bytes += entry.size;
    }
    DirectoryStats::new(bytes, entries.len())
}

/// Per-theme curated footprint, computed in memory from one `curated/` listing plus the catalog.
///
/// Bytes and file counts are attributed to a map by the `curated/{id}/` directory its layer blobs
/// live in, so a curated map's size is exactly what a `curated/{id}/` listing would have reported —
/// without issuing that listing. Every theme keeps an entry even at zero (the SPA renders the whole
/// set), a map contributes to each of its themes, and a map whose tags repeat a theme is counted
/// once. Bundle bytes no catalog row describes (an orphaned `curated/{id}/`) stay inside the
/// overall `curated` total without inflating any theme.
fn theme_breakdown(
    catalog_maps: &[CuratedMap],
    curated_entries: &[BlobEntry],
) -> HashMap<Theme, ThemeStorageStats> {
    let mut per_map: HashMap<&str, DirectoryStats> = HashMap::new();
    for entry in curated_entries {
        if let BlobKey::CuratedLayer { id, .. } = &entry.key {
            let stats = per_map.entry(id.as_str()).or_default();
            stats.bytes += entry.size;
            stats.file_count += 1;
        }
    }

    let mut curated_by_theme: HashMap<Theme, ThemeStorageStats> = Theme::ALL
        .iter()
        .map(|theme| (*theme, ThemeStorageStats::new(*theme, 0, 0, 0)))
        .collect();

    for map in catalog_maps {
        let map_stats = per_map.get(map.id.as_str()).copied().unwrap_or_default();
        let mut themes: Vec<Theme> = Vec::new();
        for tag in &map.tags {
            if let Ok(theme) = tag.parse::<Theme>() {
                if !themes.contains(&theme) {
                    themes.push(theme);
                }
            }
        }
        for theme in themes {
            if let Some(entry) = curated_by_theme.get_mut(&theme) {
                entry.map_count += 1;
                entry.bytes += map_stats.bytes;
                entry.file_count += map_stats.file_count;
            }
        }
    }

    curated_by_theme
}
