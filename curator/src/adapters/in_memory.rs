//! Pure in-memory test doubles for the storage ports: no filesystem access, no temp dirs.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::domain::{Candidate, CuratedMap, RejectionReason, RejectionRecord, Theme};
use crate::ports::blob_storage::{BlobKey, BlobStat, BlobStorage};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use crate::ports::storage_error::StorageError;

#[derive(Debug, Clone)]
struct StoredBlob {
    bytes: Vec<u8>,
    sha256: Option<String>,
}

/// [`BlobStorage`] double keeping every blob in a process-local map.
#[derive(Debug, Default)]
pub struct InMemoryBlobStorage {
    blobs: RwLock<HashMap<BlobKey, StoredBlob>>,
}

impl InMemoryBlobStorage {
    /// Creates an empty in-memory blob store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BlobStorage for InMemoryBlobStorage {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        self.blobs
            .read()
            .expect("in-memory blob store lock poisoned")
            .get(key)
            .map(|b| b.bytes.clone())
            .ok_or_else(|| StorageError::NotFound(format!("Blob '{}' not found", key.to_path())))
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        self.write_blob_with_sha256(key, bytes, None).await
    }

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        let sha256_val = sha256
            .map(|s| s.to_string())
            .unwrap_or_else(|| crate::ports::blob_storage::sha256_digest(bytes));
        self.blobs
            .write()
            .expect("in-memory blob store lock poisoned")
            .insert(
                key.clone(),
                StoredBlob {
                    bytes: bytes.to_vec(),
                    sha256: Some(sha256_val),
                },
            );
        Ok(())
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        self.blobs
            .write()
            .expect("in-memory blob store lock poisoned")
            .remove(key);
        Ok(())
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        Ok(self
            .blobs
            .read()
            .expect("in-memory blob store lock poisoned")
            .contains_key(key))
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.blobs
            .read()
            .expect("in-memory blob store lock poisoned")
            .get(key)
            .map(|b| b.bytes.len() as u64)
            .ok_or_else(|| StorageError::NotFound(format!("Blob '{}' not found", key.to_path())))
    }

    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        self.blobs
            .read()
            .expect("in-memory blob store lock poisoned")
            .get(key)
            .map(|b| BlobStat {
                size: b.bytes.len() as u64,
                sha256: b.sha256.clone(),
            })
            .ok_or_else(|| StorageError::NotFound(format!("Blob '{}' not found", key.to_path())))
    }
    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        let blobs = self
            .blobs
            .read()
            .expect("in-memory blob store lock poisoned");
        let mut keys: Vec<BlobKey> = blobs
            .keys()
            .filter(|key| prefix.is_prefix_of(key))
            .cloned()
            .collect();
        keys.sort_by_key(|key| key.to_path());
        Ok(keys)
    }
}

/// [`CatalogRepository`] double keeping candidates, curated maps, and the rejection log in memory.
///
/// Listings are always ordered by id; `limit`/`offset` are applied after filtering and ordering.
#[derive(Debug, Default)]
pub struct InMemoryCatalogRepository {
    candidates: RwLock<HashMap<String, Candidate>>,
    curated: RwLock<HashMap<String, CuratedMap>>,
    rejections: RwLock<Vec<RejectionRecord>>,
}

impl InMemoryCatalogRepository {
    /// Creates an empty in-memory catalog.
    pub fn new() -> Self {
        Self::default()
    }
}

/// A curated map matches a theme when any of its tags parses to that theme, mirroring the
/// tag-based theme resolution of the on-disk storage layout.
fn matches_theme(map: &CuratedMap, theme: Theme) -> bool {
    map.tags
        .iter()
        .any(|tag| tag.parse::<Theme>().ok() == Some(theme))
}

#[async_trait]
impl CatalogRepository for InMemoryCatalogRepository {
    async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        self.candidates
            .read()
            .expect("in-memory catalog lock poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(format!("Candidate '{id}' not found")))
    }

    async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        self.candidates
            .write()
            .expect("in-memory catalog lock poisoned")
            .insert(candidate.id.clone(), candidate.clone());
        Ok(())
    }

    async fn list_candidates(&self, filter: &CandidateFilter) -> Result<Vec<Candidate>, StorageError> {
        let candidates = self
            .candidates
            .read()
            .expect("in-memory catalog lock poisoned");
        let mut matched: Vec<Candidate> = candidates
            .values()
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
            .cloned()
            .collect();
        matched.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(paginate(matched, filter.limit, filter.offset))
    }

    async fn delete_candidate(&self, id: &str) -> Result<(), StorageError> {
        self.candidates
            .write()
            .expect("in-memory catalog lock poisoned")
            .remove(id);
        Ok(())
    }

    async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError> {
        self.curated
            .read()
            .expect("in-memory catalog lock poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(format!("Curated map '{id}' not found")))
    }

    async fn save_curated(&self, map: &CuratedMap) -> Result<(), StorageError> {
        self.curated
            .write()
            .expect("in-memory catalog lock poisoned")
            .insert(map.id.clone(), map.clone());
        Ok(())
    }

    async fn list_curated(&self, filter: &CuratedFilter) -> Result<Vec<CuratedMap>, StorageError> {
        let curated = self
            .curated
            .read()
            .expect("in-memory catalog lock poisoned");
        let mut matched: Vec<CuratedMap> = curated
            .values()
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
            .cloned()
            .collect();
        matched.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(paginate(matched, filter.limit, filter.offset))
    }

    async fn delete_curated(&self, id: &str) -> Result<(), StorageError> {
        self.curated
            .write()
            .expect("in-memory catalog lock poisoned")
            .remove(id);
        Ok(())
    }

    async fn rejection_records(&self) -> Result<Vec<RejectionRecord>, StorageError> {
        Ok(self
            .rejections
            .read()
            .expect("in-memory catalog lock poisoned")
            .clone())
    }

    async fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        Ok(self
            .rejections
            .read()
            .expect("in-memory catalog lock poisoned")
            .iter()
            .any(|record| record.id == id))
    }

    async fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        self.rejections
            .write()
            .expect("in-memory catalog lock poisoned")
            .push(RejectionRecord::with_now(id, reason, notes.map(str::to_string)));
        Ok(())
    }
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
