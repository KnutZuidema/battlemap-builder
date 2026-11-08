
//! Filesystem-backed [`BlobStorage`] implementation resolving every [`BlobKey`] below one root.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use crate::ports::blob_storage::{BlobEntry, BlobKey, BlobStat, BlobStorage};
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

    /// Walks every file below `prefix`, pairing its key with the size its own directory entry
    /// reported.
    ///
    /// The size comes from `DirEntry::metadata()`, i.e. from the very walk that discovers the
    /// file — never from a second `stat` per blob — and an entry that disappears mid-walk (a
    /// concurrent writer renaming its atomic-write temp file away) is simply skipped, since a
    /// directory listing is a snapshot.
    async fn walk_entries(&self, prefix: &BlobKey) -> Result<Vec<BlobEntry>, StorageError> {
        let start = match prefix {
            BlobKey::StagingDir { .. }
            | BlobKey::CuratedDir { .. }
            | BlobKey::CuratedThumbsDir { .. }
            | BlobKey::TelemetryDir
            | BlobKey::ArtifactsDir
            | BlobKey::DatasetSnapshotsDir
            | BlobKey::RunsDir
            | BlobKey::RunCheckpointsDir { .. }
            | BlobKey::CheckpointDir { .. }
            | BlobKey::PublishedAdaptersDir => self.root_dir.join(prefix.to_path()),
            _ => self.root_dir.clone(),
        };

        let mut entries = Vec::new();
        let mut stack = vec![start];
        while let Some(current) = stack.pop() {
            let dir_entries = match std::fs::read_dir(&current) {
                Ok(dir_entries) => dir_entries,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(StorageError::Io(err)),
            };
            for entry in dir_entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => return Err(StorageError::Io(err)),
                };
                // `metadata()` on a `DirEntry` does not traverse a symlink and reports the entry's
                // own size, so a file replaced between `read_dir` and here is the only case that
                // can fail — and that race means the file is no longer part of the snapshot.
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => return Err(StorageError::Io(err)),
                };
                let path = entry.path();
                if metadata.is_dir() {
                    stack.push(path);
                } else if metadata.is_file() {
                    let Some(relative) = path
                        .strip_prefix(&self.root_dir)
                        .ok()
                        .and_then(|relative| relative.to_str())
                    else {
                        continue;
                    };
                    let relative = relative.replace(std::path::MAIN_SEPARATOR, "/");
                    if let Some(key) = BlobKey::from_path(&relative) {
                        if prefix.is_prefix_of(&key) {
                            entries.push(BlobEntry::new(key, metadata.len()));
                        }
                    }
                }
            }
        }

        entries.sort_by_key(|entry| entry.key.to_path());
        Ok(entries)
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
    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        let path = self.path_for(key);
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                let size = metadata.len();
                let sha256 = match std::fs::read(&path) {
                    Ok(bytes) => Some(crate::ports::blob_storage::sha256_digest(&bytes)),
                    Err(_) => None,
                };
                Ok(BlobStat { size, sha256 })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(StorageError::NotFound(
                format!("Blob '{}' not found", key.to_path()),
            )),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        _sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        self.write_blob(key, bytes).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        Ok(self
            .walk_entries(prefix)
            .await?
            .into_iter()
            .map(|entry| entry.key)
            .collect())
    }

    /// Sizes come from the directory walk itself (`DirEntry::metadata()`), so aggregating a
    /// directory never re-opens a file or issues a second `stat` per blob.
    async fn list_prefix_with_sizes(
        &self,
        prefix: &BlobKey,
    ) -> Result<Vec<BlobEntry>, StorageError> {
        self.walk_entries(prefix).await
    }
}
