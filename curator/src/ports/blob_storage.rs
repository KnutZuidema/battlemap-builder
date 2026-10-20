//! Port for raw byte I/O addressed by strongly-typed [`BlobKey`]s.

use async_trait::async_trait;

use crate::domain::{Layer, LayerRole};
use crate::ports::storage_error::StorageError;

/// Strongly-typed address of a stored blob.
///
/// Each variant is the single source of truth for one location in the storage layout
/// (`staging/{id}/{index}.png`, flat `curated/{id}/` bundles per ADR-0003, LoRA companions,
/// and dataset export archives), so no caller ever hand-builds a path string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlobKey {
    /// Staging bundle layer `staging/{id}/{index}.png`.
    StagingLayer { id: String, index: usize },
    /// Curated bundle layer inside the flat `curated/{id}/` directory.
    CuratedLayer { id: String, index: usize, role: LayerRole, elevation: Option<i8> },
    /// Normalized LoRA conditioning image companion of a curated map.
    CuratedLoraImage { id: String },
    /// Caption companion of a curated map's LoRA conditioning image.
    CuratedLoraCaption { id: String },
    /// ZIP archive containing an exported dataset.
    ExportArchive { name: String },
    /// Directory prefix `staging/` (id=None) or `staging/{id}/`.
    StagingDir { id: Option<String> },
    /// Directory prefix `curated/` (id=None) or `curated/{id}/`.
    CuratedDir { id: Option<String> },
}

impl BlobKey {
    /// Canonical storage-relative path for this blob, mirroring the on-disk layout.
    pub fn to_path(&self) -> String {
        match self {
            BlobKey::StagingLayer { id, index } => format!("staging/{id}/{index}.png"),
            BlobKey::CuratedLayer { id, index, role, elevation } => format!(
                "curated/{id}/{}",
                Layer::format_filename(*index, *role, *elevation, "png")
            ),
            BlobKey::CuratedLoraImage { id } => format!("curated/{id}_lora1024.png"),
            BlobKey::CuratedLoraCaption { id } => format!("curated/{id}_lora1024.txt"),
            BlobKey::ExportArchive { name } => {
                if name.ends_with(".zip") {
                    format!("exports/{name}")
                } else {
                    format!("exports/{name}.zip")
                }
            }
            BlobKey::StagingDir { id } => directory_prefix("staging", id.as_deref()),
            BlobKey::CuratedDir { id } => directory_prefix("curated", id.as_deref()),
        }
    }

    /// Returns true when `self` is a directory prefix containing `other`, i.e. when the canonical
    /// path of `other` starts with the canonical path of `self`. This defines the matching rule of
    /// [`BlobStorage::list_prefix`].
    pub fn is_prefix_of(&self, other: &BlobKey) -> bool {
        other.to_path().starts_with(&self.to_path())
    }
}

fn directory_prefix(root: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{root}/{id}/"),
        None => format!("{root}/"),
    }
}

/// Port defining raw blob I/O under the canonical paths of [`BlobKey`].
///
/// Contract: reading a missing blob fails with [`StorageError::NotFound`], while deleting a
/// missing blob succeeds (idempotent); writing overwrites whatever occupied the key before.
#[async_trait]
pub trait BlobStorage: Send + Sync {
    /// Read the bytes stored at `key`.
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError>;

    /// Write `bytes` at `key`, replacing any previous content.
    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError>;

    /// Delete the blob at `key`. Succeeds when nothing was stored there.
    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError>;

    /// Returns whether a blob is stored at `key`.
    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError>;

    /// List the blobs below the `prefix` directory key, ordered by canonical path.
    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError>;
}

#[async_trait]
impl<T: ?Sized + BlobStorage> BlobStorage for std::sync::Arc<T> {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        (**self).read_blob(key).await
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        (**self).write_blob(key, bytes).await
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        (**self).delete_blob(key).await
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        (**self).exists_blob(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        (**self).list_prefix(prefix).await
    }
}
