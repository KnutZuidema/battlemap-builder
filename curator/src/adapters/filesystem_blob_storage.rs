//! Filesystem-backed [`BlobStorage`] implementation resolving every [`BlobKey`] below one root.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use crate::domain::LayerRole;
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::storage_error::StorageError;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`BlobStorage`] adapter writing bytes to the canonical [`BlobKey::to_path`] below `root_dir`.
#[derive(Debug, Clone)]
pub struct FilesystemBlobStorage {
    root_dir: PathBuf,
}

impl FilesystemBlobStorage {
    /// Creates a blob store anchored at `root_dir`; directories are created on demand.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// Root directory of the storage layout.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Absolute path of `key` under the storage root.
    fn path_for(&self, key: &BlobKey) -> PathBuf {
        self.root_dir.join(key.to_path())
    }

    /// Atomically replaces `path` with `data` via a uniquely named temporary file in the same
    /// directory, so a crash never leaves a partial blob at the final path.
    fn atomic_write(path: &Path, data: &[u8]) -> Result<(), StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("tmp");
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = path.with_file_name(format!(
            ".{file_name}.{}.{count}.tmp",
            std::process::id()
        ));

        std::fs::write(&tmp_path, data)?;
        if let Err(err) = std::fs::rename(&tmp_path, path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(StorageError::Io(err));
        }
        Ok(())
    }
}

#[async_trait]
impl BlobStorage for FilesystemBlobStorage {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        let path = self.path_for(key);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(StorageError::NotFound(
                format!("Blob '{}' not found", key.to_path()),
            )),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        Self::atomic_write(&self.path_for(key), bytes)
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        match std::fs::remove_file(self.path_for(key)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        Ok(self.path_for(key).is_file())
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        let path = self.path_for(key);
        match std::fs::metadata(&path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(StorageError::NotFound(
                format!("Blob '{}' not found", key.to_path()),
            )),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        let start = match prefix {
            BlobKey::StagingDir { .. } | BlobKey::CuratedDir { .. } => {
                self.root_dir.join(prefix.to_path())
            }
            _ => self.root_dir.clone(),
        };

        let mut files = Vec::new();
        let mut stack = vec![start];
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
                    stack.push(path);
                } else if file_type.is_file() {
                    if let Ok(relative) = path.strip_prefix(&self.root_dir) {
                        if let Some(relative) = relative.to_str() {
                            files.push(relative.replace(std::path::MAIN_SEPARATOR, "/"));
                        }
                    }
                }
            }
        }

        let mut keys: Vec<BlobKey> = files
            .iter()
            .filter_map(|relative| key_from_relative_path(relative))
            .filter(|key| prefix.is_prefix_of(key))
            .collect();
        keys.sort_by_key(|key| key.to_path());
        Ok(keys)
    }
}

/// Parses a canonical storage-relative path back into the [`BlobKey`] that produces it.
///
/// Returns `None` for files that no [`BlobKey`] variant addresses (manifests, foreign files), and
/// verifies the canonical round-trip so every returned key satisfies `key.to_path() == relative`.
fn key_from_relative_path(relative: &str) -> Option<BlobKey> {
    let parts: Vec<&str> = relative.split('/').collect();
    let key = match parts.as_slice() {
        ["staging", id, file] => {
            let index = parse_canonical_usize(file.strip_suffix(".png")?)?;
            BlobKey::StagingLayer {
                id: (*id).to_string(),
                index,
            }
        }
        ["curated", id, file] => {
            let (index, role, elevation) = parse_curated_filename(file)?;
            BlobKey::CuratedLayer {
                id: (*id).to_string(),
                index,
                role,
                elevation,
            }
        }
        ["exports", file] => BlobKey::ExportArchive {
            name: file.strip_suffix(".zip")?.to_string(),
        },
        _ => return None,
    };
    (key.to_path() == relative).then_some(key)
}

/// Parses a canonical curated layer filename (`{index}_{slug}.png`) back into its components.
fn parse_curated_filename(file: &str) -> Option<(usize, LayerRole, Option<i8>)> {
    let stem = file.strip_suffix(".png")?;
    let (index, slug) = stem.split_once('_')?;
    let index = parse_canonical_usize(index)?;
    let (role, elevation) = match slug {
        "ground" => (LayerRole::Ground, Some(0)),
        "roof" => (LayerRole::Roof, None),
        "variant" => (LayerRole::Variant, None),
        "exclude" => (LayerRole::Exclude, None),
        "unassigned" => (LayerRole::Unassigned, None),
        _ => {
            if let Some(level) = slug.strip_prefix("upper_") {
                let level = parse_canonical_usize(level)?;
                (LayerRole::UpperFloor, Some(i8::try_from(level).ok()?))
            } else {
                let level = parse_canonical_usize(slug.strip_prefix("basement_")?)?;
                (
                    LayerRole::Basement,
                    Some(i8::try_from(level).ok()?.checked_neg()?),
                )
            }
        }
    };
    Some((index, role, elevation))
}

/// Parses `value` only when it is the canonical decimal rendering of the number (no padding).
fn parse_canonical_usize(value: &str) -> Option<usize> {
    let parsed: usize = value.parse().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}
