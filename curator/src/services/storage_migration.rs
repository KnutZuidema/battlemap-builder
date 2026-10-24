//! One-time offline migration service copying disk blobs to S3/R2 storage.
//!
//! Scans a local filesystem data directory for all valid [`BlobKey`] assets
//! (`staging/`, `curated/`, `telemetry/`, `rejected/`, `exports/`, `artifacts/`),
//! checks if each asset is already present in the target [`BlobStorage`], and
//! uploads missing blobs with bounded concurrency.
//!
//! A marker file (`.disk_to_s3_migrated`) is written upon successful completion
//! to guarantee idempotency and avoid duplicate work on subsequent runs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{debug, info};

use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::storage_error::StorageError;

/// Marker file written to the root of `data_dir` once migration succeeds.
pub const MIGRATION_MARKER_FILE: &str = ".disk_to_s3_migrated";

/// Default concurrency for S3 upload tasks.
pub const DEFAULT_MIGRATION_CONCURRENCY: usize = 16;

/// Configuration options for the disk-to-S3 storage migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOptions {
    /// Re-run migration even if the marker file is present.
    pub force: bool,
    /// Inspect disk and target S3 storage without writing any blobs or marker file.
    pub dry_run: bool,
    /// Maximum concurrent S3 upload operations.
    pub concurrency: usize,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            force: false,
            dry_run: false,
            concurrency: DEFAULT_MIGRATION_CONCURRENCY,
        }
    }
}

/// Structured summary report of a storage migration run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MigrationReport {
    pub completed_at: String,
    pub total_discovered: usize,
    pub migrated_count: usize,
    pub skipped_count: usize,
    pub total_bytes_migrated: u64,
    pub dry_run: bool,
}

/// Checks whether the migration marker file is present in `data_dir`.
pub fn is_migration_completed(data_dir: &Path) -> bool {
    data_dir.join(MIGRATION_MARKER_FILE).is_file()
}

/// Recursively scans `data_dir` for all files that map to a canonical [`BlobKey`].
///
/// Ignores non-blob files (e.g. `manifest.json`, `.disk_to_s3_migrated`, `.DS_Store`,
/// and legacy pre-rendered LoRA crops).
pub fn discover_disk_blobs(data_dir: &Path) -> Result<Vec<(BlobKey, PathBuf)>, StorageError> {
    let mut discovered = Vec::new();
    let mut stack = vec![data_dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(StorageError::Io(err)),
        };

        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();

            if file_type.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue;
                    }
                }
                stack.push(path);
            } else if file_type.is_file() {
                if let Ok(relative) = path.strip_prefix(data_dir) {
                    if let Some(rel_str) = relative.to_str() {
                        let normalized = rel_str.replace(std::path::MAIN_SEPARATOR, "/");
                        if let Some(key) = BlobKey::from_path(&normalized) {
                            discovered.push((key, path));
                        }
                    }
                }
            }
        }
    }

    discovered.sort_by_key(|a| a.0.to_path());
    Ok(discovered)
}

/// Executes the disk-to-S3 storage migration.
pub async fn run_storage_migration(
    data_dir: &Path,
    target: Arc<dyn BlobStorage + Send + Sync>,
    options: MigrationOptions,
) -> Result<MigrationReport, StorageError> {
    let marker_path = data_dir.join(MIGRATION_MARKER_FILE);

    if !options.force && marker_path.is_file() {
        info!(
            marker = %marker_path.display(),
            "Disk-to-S3 storage migration already completed; skipping (use --force to re-run)"
        );
        if let Ok(content) = std::fs::read_to_string(&marker_path) {
            if let Ok(report) = serde_json::from_str::<MigrationReport>(&content) {
                return Ok(report);
            }
        }
        return Ok(MigrationReport {
            completed_at: chrono::Utc::now().to_rfc3339(),
            total_discovered: 0,
            migrated_count: 0,
            skipped_count: 0,
            total_bytes_migrated: 0,
            dry_run: false,
        });
    }

    info!(
        data_dir = %data_dir.display(),
        force = options.force,
        dry_run = options.dry_run,
        concurrency = options.concurrency,
        "Discovering disk blobs for storage migration..."
    );

    let discovered = discover_disk_blobs(data_dir)?;
    info!(
        total_discovered = discovered.len(),
        "Discovered {} potential blob(s) on disk",
        discovered.len()
    );

    if discovered.is_empty() {
        let report = MigrationReport {
            completed_at: chrono::Utc::now().to_rfc3339(),
            total_discovered: 0,
            migrated_count: 0,
            skipped_count: 0,
            total_bytes_migrated: 0,
            dry_run: options.dry_run,
        };
        if !options.dry_run {
            let json = serde_json::to_string_pretty(&report)
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
            std::fs::write(&marker_path, json)?;
        }
        return Ok(report);
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(options.concurrency.max(1)));
    let mut join_set = tokio::task::JoinSet::new();

    for (key, path) in discovered {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        let target = target.clone();
        let dry_run = options.dry_run;

        join_set.spawn(async move {
            let _permit = permit;
            let key_str = key.to_path();

            let exists = target.exists_blob(&key).await?;
            if exists {
                debug!(key = %key_str, "Blob already exists in target storage; skipping");
                return Ok::<_, StorageError>((key, None));
            }

            let metadata = tokio::fs::metadata(&path).await.map_err(StorageError::Io)?;
            let file_size = metadata.len();

            if dry_run {
                info!(key = %key_str, bytes = file_size, "[DRY RUN] Would migrate blob to S3");
                return Ok((key, Some(file_size)));
            }

            let bytes = tokio::fs::read(&path).await.map_err(StorageError::Io)?;
            target.write_blob(&key, &bytes).await?;
            info!(key = %key_str, bytes = bytes.len(), "Migrated blob to target S3 storage");

            Ok((key, Some(bytes.len() as u64)))
        });
    }

    let mut migrated_count = 0usize;
    let mut skipped_count = 0usize;
    let mut total_bytes_migrated = 0u64;

    while let Some(res) = join_set.join_next().await {
        let (_key, uploaded) = res.map_err(|e| StorageError::Io(std::io::Error::other(e)))??;
        match uploaded {
            Some(bytes) => {
                migrated_count += 1;
                total_bytes_migrated += bytes;
            }
            None => {
                skipped_count += 1;
            }
        }
    }

    let report = MigrationReport {
        completed_at: chrono::Utc::now().to_rfc3339(),
        total_discovered: migrated_count + skipped_count,
        migrated_count,
        skipped_count,
        total_bytes_migrated,
        dry_run: options.dry_run,
    };

    info!(
        total = report.total_discovered,
        migrated = report.migrated_count,
        skipped = report.skipped_count,
        bytes = report.total_bytes_migrated,
        dry_run = report.dry_run,
        "Storage migration completed"
    );

    if !options.dry_run {
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        std::fs::write(&marker_path, json)?;
        info!(marker = %marker_path.display(), "Wrote migration completion marker");
    }

    Ok(report)
}
