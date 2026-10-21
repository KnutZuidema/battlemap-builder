//! Port for raw byte I/O addressed by strongly-typed [`BlobKey`]s.

use async_trait::async_trait;

use crate::domain::{Layer, LayerRole};
use crate::ports::storage_error::StorageError;

/// Strongly-typed address of a stored blob.
///
/// Each variant is the single source of truth for one location in the storage layout
/// (`staging/{id}/{index}.png`, flat `curated/{id}/` bundles per ADR-0003, dataset export
/// archives, the rejection log, and telemetry ledgers), so no caller ever hand-builds a path
/// string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlobKey {
    /// Staging bundle layer `staging/{id}/{index}.png`.
    StagingLayer { id: String, index: usize },
    /// Curated bundle layer inside the flat `curated/{id}/` directory.
    CuratedLayer { id: String, index: usize, role: LayerRole, elevation: Option<i8> },
    /// ZIP archive containing an exported dataset, stored at `exports/{name}.zip`.
    ///
    /// `name` is the bare archive name and MUST NOT carry a file extension: [`BlobKey::to_path`]
    /// always appends exactly one `.zip`, so no two distinct names can collide on one path and
    /// `name` is the single source of truth for the archive's identity.
    ExportArchive { name: String },
    /// Append-only rejection log at `rejected/rejections.json`, the record of every rejected
    /// candidate.
    RejectionLog,
    /// Telemetry ledger at `telemetry/{name}`, e.g. the cost ledger `telemetry/cost_ledger.json`.
    ///
    /// `name` is the bare file name and MUST NOT carry a directory separator: [`BlobKey::to_path`]
    /// always places it directly below `telemetry/`, so no two distinct names can collide on one
    /// path and `name` is the single source of truth for the ledger's identity.
    TelemetryLog { name: String },
    /// Directory prefix `staging/` (id=None) or `staging/{id}/`.
    StagingDir { id: Option<String> },
    /// Directory prefix `curated/` (id=None) or `curated/{id}/`.
    CuratedDir { id: Option<String> },
    /// Directory prefix `telemetry/`, used to enumerate every [`BlobKey::TelemetryLog`].
    TelemetryDir,
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
            BlobKey::ExportArchive { name } => format!("exports/{name}.zip"),
            BlobKey::RejectionLog => "rejected/rejections.json".to_string(),
            BlobKey::TelemetryLog { name } => format!("telemetry/{name}"),
            BlobKey::StagingDir { id } => directory_prefix("staging", id.as_deref()),
            BlobKey::CuratedDir { id } => directory_prefix("curated", id.as_deref()),
            BlobKey::TelemetryDir => directory_prefix("telemetry", None),
        }
    }

    /// Returns true when `self` is a directory prefix containing `other`, i.e. when the canonical
    /// path of `other` starts with the canonical path of `self`. This defines the matching rule of
    /// [`BlobStorage::list_prefix`].
    ///
    /// Directory prefixes render with a trailing `/` (see [`BlobKey::to_path`]), so a prefix never
    /// swallows a sibling whose path merely starts with the same text.
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

    /// Byte length of the blob stored at `key`, failing with [`StorageError::NotFound`] when
    /// nothing is stored there.
    ///
    /// Size accounting ([`crate::services::StorageMaintenanceService`]) reads this instead of
    /// [`BlobStorage::read_blob`], so aggregating a directory never pulls its payloads into memory.
    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError>;

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

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        (**self).blob_size(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        (**self).list_prefix(prefix).await
    }
}
