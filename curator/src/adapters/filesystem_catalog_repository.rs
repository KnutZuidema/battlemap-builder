//! Filesystem-backed [`CatalogRepository`] storing one JSON manifest per staging/curated bundle.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::domain::{
    BenchmarkMetadata, Candidate, CuratedMap, RejectionReason, RejectionRecord, Theme,
};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use crate::ports::storage_error::StorageError;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// [`CatalogRepository`] adapter persisting entities as `{staging,curated}/{id}/manifest.json`.
///
/// Theme directories are never written: every curated bundle lives in the flat `curated/{id}/`
/// layout, with theme membership derived from the map's tags.
#[derive(Debug, Clone)]
pub struct FilesystemCatalogRepository {
    root_dir: PathBuf,
}

impl FilesystemCatalogRepository {
    /// Creates a catalog repository anchored at `root_dir`; directories are created on demand.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// Root directory of the catalog layout.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    fn staging_dir(&self) -> PathBuf {
        self.root_dir.join("staging")
    }

    fn curated_dir(&self) -> PathBuf {
        self.root_dir.join("curated")
    }

    fn candidate_manifest(&self, id: &str) -> PathBuf {
        self.staging_dir().join(id).join("manifest.json")
    }

    fn curated_manifest(&self, id: &str) -> PathBuf {
        self.curated_dir().join(id).join("manifest.json")
    }

    /// Full path to the `rejections.json` log file.
    fn rejections_file(&self) -> PathBuf {
        self.root_dir.join("rejected").join("rejections.json")
    }

    /// Atomically replaces `path` with `data` via a uniquely named temporary file in the same
    /// directory, so a crash never leaves a partial manifest at the final path.
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

    fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(value)?;
        Self::atomic_write(path, json.as_bytes())
    }

    fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T, StorageError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound(format!("{label} not found")));
            }
            Err(err) => return Err(StorageError::Io(err)),
        };
        serde_json::from_slice(&bytes)
            .map_err(|err| StorageError::InvalidData(format!("Invalid {label}: {err}")))
    }

    /// Replaces the manifest file content used by both entity kinds.
    fn delete_manifest(path: &Path) -> Result<(), StorageError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    /// Scans `dir/*/manifest.json`, silently skipping directories without a readable manifest.
    fn scan_manifests<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>, StorageError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(StorageError::Io(err)),
        };
        let mut entities = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let manifest = entry.path().join("manifest.json");
            let bytes = match std::fs::read(&manifest) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if let Ok(entity) = serde_json::from_slice(&bytes) {
                entities.push(entity);
            }
        }
        Ok(entities)
    }

    /// One-shot, best-effort migration of pre-#176 generated staging bundles, called once at
    /// startup from the composition root.
    ///
    /// Before #176 the generated-image pipeline persisted the raw [`BenchmarkMetadata`] document
    /// as the bundle manifest (`staging/{id}/manifest.json`, formerly
    /// `staging/generated/{id}/manifest.json`), and the retired monolithic storage repository
    /// mapped it through [`BenchmarkMetadata::to_candidate`] while serving it.
    /// [`CatalogRepository`] only reads [`Candidate`] manifests, so those bundles would silently
    /// disappear from `list_candidates` and fail `get_candidate`. Rewriting each such manifest in
    /// place restores them; the layer blobs are left untouched.
    ///
    /// Idempotent (a manifest that already parses as `Candidate` is skipped, so a second run is a
    /// no-op) and never fatal: manifests that parse as neither shape, and rewrites that fail, are
    /// logged and skipped so startup always continues. Returns the number of manifests migrated.
    pub fn migrate_legacy_generated_manifests(&self) -> usize {
        let entries = match std::fs::read_dir(self.staging_dir()) {
            Ok(entries) => entries,
            Err(_) => return 0,
        };
        let mut migrated = 0;
        for entry in entries.flatten() {
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let manifest = entry.path().join("manifest.json");
            let bytes = match std::fs::read(&manifest) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if serde_json::from_slice::<Candidate>(&bytes).is_ok() {
                continue;
            }
            let Ok(metadata) = serde_json::from_slice::<BenchmarkMetadata>(&bytes) else {
                continue;
            };
            match Self::write_json(&manifest, &metadata.to_candidate()) {
                Ok(()) => migrated += 1,
                Err(err) => tracing::warn!(
                    path = %manifest.display(),
                    error = %err,
                    "Skipping legacy generated staging manifest migration"
                ),
            }
        }
        migrated
    }
}

/// A curated map matches a theme when any of its tags parses to that theme.
fn matches_theme(map: &CuratedMap, theme: Theme) -> bool {
    map.tags
        .iter()
        .any(|tag| tag.parse::<Theme>().ok() == Some(theme))
}

/// Applies `offset` then `limit` to an already ordered result list.
fn paginate<T>(mut items: Vec<T>, limit: Option<usize>, offset: Option<usize>) -> Vec<T> {
    let offset = offset.unwrap_or(0);
    if offset > 0 {
        if offset >= items.len() {
            items.clear();
        } else {
            items.drain(..offset);
        }
    }
    if let Some(limit) = limit {
        items.truncate(limit);
    }
    items
}

#[async_trait]
impl CatalogRepository for FilesystemCatalogRepository {
    async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        Self::read_json(
            &self.candidate_manifest(id),
            &format!("Candidate '{id}'"),
        )
    }

    async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        Self::write_json(&self.candidate_manifest(&candidate.id), candidate)
    }

    async fn list_candidates(
        &self,
        filter: &CandidateFilter,
    ) -> Result<Vec<Candidate>, StorageError> {
        let mut candidates: Vec<Candidate> = Self::scan_manifests::<Candidate>(&self.staging_dir())?
            .into_iter()
            .filter(|candidate| {
                if let Some(source) = filter.source {
                    if candidate.source != source {
                        return false;
                    }
                }
                if let Some(parent_group_id) = &filter.parent_group_id {
                    if candidate.parent_group_id.as_deref() != Some(parent_group_id.as_str()) {
                        return false;
                    }
                }
                true
            })
            .collect();
        candidates.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(paginate(candidates, filter.limit, filter.offset))
    }

    async fn delete_candidate(&self, id: &str) -> Result<(), StorageError> {
        Self::delete_manifest(&self.candidate_manifest(id))
    }

    async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError> {
        Self::read_json(&self.curated_manifest(id), &format!("Curated map '{id}'"))
    }

    async fn save_curated(&self, map: &CuratedMap) -> Result<(), StorageError> {
        Self::write_json(&self.curated_manifest(&map.id), map)
    }

    async fn list_curated(&self, filter: &CuratedFilter) -> Result<Vec<CuratedMap>, StorageError> {
        let mut maps: Vec<CuratedMap> = Self::scan_manifests::<CuratedMap>(&self.curated_dir())?
            .into_iter()
            .filter(|map| {
                if let Some(theme) = filter.theme {
                    if !matches_theme(map, theme) {
                        return false;
                    }
                }
                if !filter.tags.iter().all(|tag| map.tags.contains(tag)) {
                    return false;
                }
                if let Some(ids) = &filter.ids {
                    if !ids.contains(&map.id) {
                        return false;
                    }
                }
                true
            })
            .collect();
        maps.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(paginate(maps, filter.limit, filter.offset))
    }

    async fn delete_curated(&self, id: &str) -> Result<(), StorageError> {
        Self::delete_manifest(&self.curated_manifest(id))
    }

    async fn rejection_records(&self) -> Result<Vec<RejectionRecord>, StorageError> {
        let path = self.rejections_file();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        Self::read_json(&path, "Rejection log")
    }

    async fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        Ok(self
            .rejection_records()
            .await?
            .iter()
            .any(|record| record.id == id))
    }

    async fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut records = self.rejection_records().await?;
        records.push(RejectionRecord::with_now(
            id,
            reason,
            notes.map(str::to_string),
        ));
        Self::write_json(&self.rejections_file(), &records)
    }
}
