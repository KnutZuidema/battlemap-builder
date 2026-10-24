//! Port for raw byte I/O addressed by strongly-typed [`BlobKey`]s.

use async_trait::async_trait;

use crate::domain::{Layer, LayerRole};
use crate::ports::storage_error::StorageError;

/// Strongly-typed address of a stored blob.
///
/// Each variant is the single source of truth for one location in the storage layout
/// (`staging/{id}/{index}.png`, flat `curated/{id}/` bundles per ADR-0003, dataset export
/// archives, the rejection log, telemetry ledgers, and public training artifacts), so no caller
/// ever hand-builds a path string.
///
/// # Identity is the canonical path
///
/// Equality and hashing are defined by [`BlobKey::to_path`], never by the struct fields: some
/// variants carry fields that do not influence the resolved path, because
/// [`Layer::format_filename`] drops `elevation` for the `Roof`/`Variant`/`Ground` roles. A
/// [`BlobKey::CuratedLayer`] built by a caller (`{index: 1, role: Roof, elevation: Some(1)}`) and
/// the same address re-derived from the file name by [`BlobStorage::list_prefix`]
/// (`{index: 1, role: Roof, elevation: None}`) therefore resolve to one file and MUST compare
/// equal — otherwise a writer would delete the blob it just wrote. This is the same rule
/// `ExportArchive` was fixed for in #171, applied to the whole enum.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Dataset snapshot ZIP archive stored at `artifacts/datasets/{id}.zip`.
    ///
    /// `id` is the bare snapshot identifier and MUST NOT carry a `.zip` extension:
    /// [`BlobKey::to_path`] always appends `.zip`.
    DatasetSnapshot { id: String },
    /// Checkpoint LoRA adapter weights stored at
    /// `artifacts/runs/{run_id}/checkpoints/step-{step}/adapter.safetensors`.
    CheckpointAdapter { run_id: String, step: usize },
    /// Checkpoint sample grid preview stored at
    /// `artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png`.
    CheckpointSamples { run_id: String, step: usize },
    /// Published LoRA adapter weights stored at `artifacts/adapters/{id}.safetensors`.
    ///
    /// `id` is the bare adapter identifier (e.g. `{run_id}-s{step}`) and MUST NOT carry a
    /// `.safetensors` extension: [`BlobKey::to_path`] always appends `.safetensors`.
    PublishedAdapter { id: String },
    /// Directory prefix `artifacts/`, matching every public training artifact.
    ArtifactsDir,
    /// Directory prefix `artifacts/datasets/`, matching every dataset snapshot.
    DatasetSnapshotsDir,
    /// Directory prefix `artifacts/runs/`, matching every run directory.
    RunsDir,
    /// Directory prefix `artifacts/runs/{run_id}/checkpoints/`, matching all checkpoints of a run.
    RunCheckpointsDir { run_id: String },
    /// Directory prefix `artifacts/runs/{run_id}/checkpoints/step-{step}/`, matching all artifacts
    /// of a specific checkpoint.
    CheckpointDir { run_id: String, step: usize },
    /// Directory prefix `artifacts/adapters/`, matching every published adapter.
    PublishedAdaptersDir,
}

/// A `BlobKey` equals another exactly when both resolve to the same canonical path (see the
/// type-level invariant above), so keys that address one file always agree even when their
/// fields differ.
impl PartialEq for BlobKey {
    fn eq(&self, other: &Self) -> bool {
        self.to_path() == other.to_path()
    }
}

impl Eq for BlobKey {}

/// Hashes the canonical path, so equal keys always land in the same bucket.
impl std::hash::Hash for BlobKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.to_path().hash(state);
    }
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
            BlobKey::DatasetSnapshot { id } => format!("artifacts/datasets/{id}.zip"),
            BlobKey::CheckpointAdapter { run_id, step } => {
                format!("artifacts/runs/{run_id}/checkpoints/step-{step}/adapter.safetensors")
            }
            BlobKey::CheckpointSamples { run_id, step } => {
                format!("artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png")
            }
            BlobKey::PublishedAdapter { id } => format!("artifacts/adapters/{id}.safetensors"),
            BlobKey::ArtifactsDir => "artifacts/".to_string(),
            BlobKey::DatasetSnapshotsDir => "artifacts/datasets/".to_string(),
            BlobKey::RunsDir => "artifacts/runs/".to_string(),
            BlobKey::RunCheckpointsDir { run_id } => {
                format!("artifacts/runs/{run_id}/checkpoints/")
            }
            BlobKey::CheckpointDir { run_id, step } => {
                format!("artifacts/runs/{run_id}/checkpoints/step-{step}/")
            }
            BlobKey::PublishedAdaptersDir => "artifacts/adapters/".to_string(),
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

    /// Returns true when `self` is an artifact written for external consumption under the
    /// `artifacts/` prefix (e.g. dataset snapshots, adapter safetensors, checkpoint samples).
    ///
    /// Curated (`curated/...`) and staging (`staging/...`) objects stay outside `artifacts/`
    /// and return `false`.
    pub fn is_public_artifact(&self) -> bool {
        matches!(
            self,
            BlobKey::DatasetSnapshot { .. }
                | BlobKey::CheckpointAdapter { .. }
                | BlobKey::CheckpointSamples { .. }
                | BlobKey::PublishedAdapter { .. }
        )
    }

    /// Resolves the external HTTPS URL for a public artifact given `base_url` (e.g. `R2_PUBLIC_URL`).
    ///
    /// Returns `Some("https://.../artifacts/...")` for public artifacts, or `None` if `self` is an
    /// internal/private object (such as staging or curated layers).
    pub fn public_url(&self, base_url: &str) -> Option<String> {
        if self.is_public_artifact() {
            let base = base_url.trim_end_matches('/');
            Some(format!("{base}/{}", self.to_path()))
        } else {
            None
        }
    }

    /// Parses a canonical storage-relative path back into the [`BlobKey`] that produces it.
    ///
    /// Returns `None` for paths that no [`BlobKey`] variant addresses, foreign files,
    /// hidden files, and paths that do not round-trip canonically.
    pub fn from_path(relative: &str) -> Option<BlobKey> {
        let parts: Vec<&str> = relative.split('/').collect();
        let key = match parts.as_slice() {
            ["staging", ""] => BlobKey::StagingDir { id: None },
            ["staging", id, ""] => BlobKey::StagingDir { id: Some((*id).to_string()) },
            ["curated", ""] => BlobKey::CuratedDir { id: None },
            ["curated", id, ""] => BlobKey::CuratedDir { id: Some((*id).to_string()) },
            ["telemetry", ""] => BlobKey::TelemetryDir,
            ["artifacts", ""] => BlobKey::ArtifactsDir,
            ["artifacts", "datasets", ""] => BlobKey::DatasetSnapshotsDir,
            ["artifacts", "runs", ""] => BlobKey::RunsDir,
            ["artifacts", "runs", run_id, "checkpoints", ""] => BlobKey::RunCheckpointsDir {
                run_id: (*run_id).to_string(),
            },
            ["artifacts", "runs", run_id, "checkpoints", step_dir, ""] => {
                let step_str = step_dir.strip_prefix("step-")?;
                let step = parse_canonical_usize(step_str)?;
                BlobKey::CheckpointDir {
                    run_id: (*run_id).to_string(),
                    step,
                }
            }
            ["artifacts", "adapters", ""] => BlobKey::PublishedAdaptersDir,
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
            ["rejected", "rejections.json"] => BlobKey::RejectionLog,
            ["telemetry", name] if !name.starts_with('.') => BlobKey::TelemetryLog {
                name: (*name).to_string(),
            },
            ["artifacts", "datasets", file] => {
                let id = file.strip_suffix(".zip")?;
                BlobKey::DatasetSnapshot { id: id.to_string() }
            }
            ["artifacts", "runs", run_id, "checkpoints", step_dir, "adapter.safetensors"] => {
                let step_str = step_dir.strip_prefix("step-")?;
                let step = parse_canonical_usize(step_str)?;
                BlobKey::CheckpointAdapter {
                    run_id: (*run_id).to_string(),
                    step,
                }
            }
            ["artifacts", "runs", run_id, "checkpoints", step_dir, "samples.png"] => {
                let step_str = step_dir.strip_prefix("step-")?;
                let step = parse_canonical_usize(step_str)?;
                BlobKey::CheckpointSamples {
                    run_id: (*run_id).to_string(),
                    step,
                }
            }
            ["artifacts", "adapters", file] => {
                let id = file.strip_suffix(".safetensors")?;
                BlobKey::PublishedAdapter { id: id.to_string() }
            }
            _ => return None,
        };
        (key.to_path() == relative).then_some(key)
    }
}

fn directory_prefix(root: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{root}/{id}/"),
        None => format!("{root}/"),
    }
}

/// Parses a canonical curated layer filename (`{index}_{slug}.png`) back into its components.
pub fn parse_curated_filename(file: &str) -> Option<(usize, LayerRole, Option<i8>)> {
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
pub fn parse_canonical_usize(value: &str) -> Option<usize> {
    let parsed: usize = value.parse().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}

/// Computes the lowercase hex-encoded SHA-256 digest of a byte slice.
pub fn sha256_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Metadata returned by [`BlobStorage::stat_blob`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlobStat {
    /// Object size in bytes.
    pub size: u64,
    /// SHA-256 hex digest, when recorded or declared on upload.
    pub sha256: Option<String>,
}

impl BlobStat {
    pub fn new(size: u64, sha256: Option<String>) -> Self {
        Self { size, sha256 }
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

    /// Write `bytes` at `key`, attaching the given `sha256` digest (or computing it if None).
    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        let _ = sha256;
        self.write_blob(key, bytes).await
    }

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

    /// Query byte length and SHA-256 digest of the blob stored at `key` without downloading its
    /// content.
    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        let size = self.blob_size(key).await?;
        Ok(BlobStat { size, sha256: None })
    }

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

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        (**self).write_blob_with_sha256(key, bytes, sha256).await
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

    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        (**self).stat_blob(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        (**self).list_prefix(prefix).await
    }
}
