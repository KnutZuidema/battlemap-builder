use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::domain::Theme;

/// File count and byte size footprint for a storage directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DirectoryStats {
    pub bytes: u64,
    pub file_count: usize,
}

impl DirectoryStats {
    /// Creates a new `DirectoryStats` instance.
    pub fn new(bytes: u64, file_count: usize) -> Self {
        Self { bytes, file_count }
    }
}

/// Thematic storage statistics for curated maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeStorageStats {
    pub theme: Theme,
    pub bytes: u64,
    pub file_count: usize,
    pub map_count: usize,
}

impl ThemeStorageStats {
    /// Creates a new `ThemeStorageStats` instance.
    pub fn new(theme: Theme, bytes: u64, file_count: usize, map_count: usize) -> Self {
        Self {
            theme,
            bytes,
            file_count,
            map_count,
        }
    }
}

/// Comprehensive storage quota and disk usage statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StorageStats {
    /// Total staging storage footprint.
    pub staging: DirectoryStats,
    /// Breakdown of staging storage by subdirectory (e.g. "reddit", "generated", "root").
    pub staging_breakdown: HashMap<String, DirectoryStats>,
    /// Total curated storage footprint across all themes.
    pub curated: DirectoryStats,
    /// Detailed storage breakdown per theme.
    pub curated_by_theme: HashMap<Theme, ThemeStorageStats>,
    /// Rejection log footprint: the byte size of `rejected/rejections.json` plus the number of
    /// rejection *records* it holds — the only `DirectoryStats.file_count` that counts records
    /// rather than files, because the operator UI renders it as `N records`. Both numbers are
    /// zero while the log is absent.
    pub rejected: DirectoryStats,
    /// Telemetry log footprint: combined byte size and file count of the blobs below
    /// `telemetry/`.
    pub telemetry: DirectoryStats,
    /// Combined byte size across all managed directories.
    pub total_bytes: u64,
    /// Combined total file count across all managed directories.
    pub total_files: usize,
}

/// Result payload returned upon purging the staging area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PurgeStagingResult {
    /// Total count of files purged from staging.
    pub purged_files: usize,
    /// Total disk space reclaimed in bytes.
    pub reclaimed_bytes: u64,
    /// Count of remaining files in staging after purge (normally 0).
    pub remaining_staging_files: usize,
}
