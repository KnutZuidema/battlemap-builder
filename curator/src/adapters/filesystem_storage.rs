use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

use crate::domain::{Candidate, CuratedMap, RejectionReason, RejectionRecord, Source, Theme};
use crate::ports::storage::{StorageError, StorageRepository};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Filesystem-backed storage adapter implementing the `StorageRepository` port.
/// Manages staging, curated thematic directories, and rejection logs.
#[derive(Debug, Clone)]
pub struct FilesystemStorage {
    root_dir: PathBuf,
}

impl FilesystemStorage {
    /// Creates a new `FilesystemStorage` instance anchored at the given root directory.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// Root directory of the storage.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Staging directory where incoming candidate images and metadata reside.
    pub fn staging_dir(&self) -> PathBuf {
        self.root_dir.join("staging")
    }

    /// Base curated directory.
    pub fn curated_dir(&self) -> PathBuf {
        self.root_dir.join("curated")
    }

    /// Curated directory for a specific theme (e.g. `<root>/curated/dungeon/`).
    pub fn curated_theme_dir(&self, theme: &Theme) -> PathBuf {
        self.curated_dir().join(theme.as_str())
    }

    /// Directory where rejection logs reside.
    pub fn rejected_dir(&self) -> PathBuf {
        self.root_dir.join("rejected")
    }

    /// Full path to the `rejections.json` log file.
    pub fn rejections_file(&self) -> PathBuf {
        self.rejected_dir().join("rejections.json")
    }

    /// Sanitize a filename to prevent directory traversal.
    fn sanitize_filename(filename: &str, fallback_id: &str) -> String {
        let trimmed = filename.trim();
        if trimmed.is_empty() {
            return format!("{fallback_id}.png");
        }
        Path::new(trimmed)
            .file_name()
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{fallback_id}.png"))
    }

    /// Atomically writes data to the target path using a process-unique temporary file.
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

    /// Atomically moves a file from one path to another, falling back to copy+delete
    /// if moving across filesystem/mount boundaries (EXDEV).
    fn atomic_move(from: &Path, to: &Path) -> Result<(), StorageError> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::rename(from, to) {
            Ok(()) => Ok(()),
            Err(_) => {
                std::fs::copy(from, to)?;
                let _ = std::fs::remove_file(from);
                Ok(())
            }
        }
    }

    /// Locate candidate image in staging by ID and candidate metadata.
    fn find_staging_image_path(&self, id: &str, candidate: Option<&Candidate>) -> Option<PathBuf> {
        let staging = self.staging_dir();
        if let Some(cand) = candidate {
            let direct = staging.join(&cand.filename);
            if direct.is_file() {
                return Some(direct);
            }
        }

        // Check common extensions
        for ext in &["png", "jpg", "jpeg", "webp"] {
            let candidate_path = staging.join(format!("{id}.{ext}"));
            if candidate_path.is_file() {
                return Some(candidate_path);
            }
        }

        // Scan staging directory for any file with stem matching id
        if let Ok(entries) = std::fs::read_dir(&staging) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if stem == id {
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                if ext != "json" && ext != "tmp" {
                                    return Some(path);
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Locate curated image in a theme directory by ID.
    fn find_curated_image_path(&self, theme: &Theme, id: &str) -> Option<PathBuf> {
        let theme_dir = self.curated_theme_dir(theme);
        for ext in &["png", "jpg", "jpeg", "webp"] {
            let candidate_path = theme_dir.join(format!("{id}.{ext}"));
            if candidate_path.is_file() {
                return Some(candidate_path);
            }
        }

        if let Ok(entries) = std::fs::read_dir(&theme_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if stem == id {
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                if ext != "json" && ext != "tmp" && ext != "txt" {
                                    return Some(path);
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Read all rejection records from `rejections.json`.
    fn read_rejections(&self) -> Result<Vec<RejectionRecord>, StorageError> {
        let path = self.rejections_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let content = content.trim();
        if content.is_empty() {
            return Ok(Vec::new());
        }

        match serde_json::from_str::<Vec<RejectionRecord>>(content) {
            Ok(records) => Ok(records),
            Err(_) => {
                // Fallback to newline-delimited JSON
                let mut records = Vec::new();
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        if let Ok(rec) = serde_json::from_str::<RejectionRecord>(line) {
                            records.push(rec);
                        }
                    }
                }
                Ok(records)
            }
        }
    }
}

impl StorageRepository for FilesystemStorage {
    fn save_staging(&self, candidate: &Candidate, data: &[u8]) -> Result<(), StorageError> {
        if candidate.id.trim().is_empty() {
            return Err(StorageError::InvalidData(
                "Candidate ID cannot be empty".into(),
            ));
        }

        let staging = self.staging_dir();
        std::fs::create_dir_all(&staging)?;

        let filename = Self::sanitize_filename(&candidate.filename, &candidate.id);
        let image_path = staging.join(&filename);
        let sidecar_path = staging.join(format!("{}.json", candidate.id));

        Self::atomic_write(&image_path, data)?;

        let sidecar_json = serde_json::to_string_pretty(candidate)?;
        Self::atomic_write(&sidecar_path, sidecar_json.as_bytes())?;

        Ok(())
    }

    fn get_staging_image(&self, id: &str) -> Result<Vec<u8>, StorageError> {
        let staging = self.staging_dir();
        let sidecar_path = staging.join(format!("{id}.json"));

        let candidate: Option<Candidate> = if sidecar_path.is_file() {
            let content = std::fs::read_to_string(&sidecar_path)?;
            serde_json::from_str(&content).ok()
        } else {
            None
        };

        let image_path = self
            .find_staging_image_path(id, candidate.as_ref())
            .ok_or_else(|| {
                StorageError::NotFound(format!("Staging image for candidate '{id}' not found"))
            })?;

        let bytes = std::fs::read(image_path)?;
        Ok(bytes)
    }

    fn list_staging(&self) -> Result<Vec<Candidate>, StorageError> {
        let staging = self.staging_dir();
        if !staging.is_dir() {
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        let mut known_ids = HashSet::new();
        let mut known_filenames = HashSet::new();

        let entries: Vec<_> = std::fs::read_dir(&staging)?
            .filter_map(|e| e.ok())
            .collect();

        // Pass 1: Read existing JSON sidecars
        for entry in &entries {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("json") {
                        let file_name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default();
                        if !file_name.starts_with('.') {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(candidate) = serde_json::from_str::<Candidate>(&content) {
                                    known_ids.insert(candidate.id.clone());
                                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                        known_ids.insert(stem.to_string());
                                    }
                                    known_filenames.insert(candidate.filename.clone());
                                    if let Some(img_stem) = Path::new(&candidate.filename)
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                    {
                                        known_ids.insert(img_stem.to_string());
                                    }
                                    candidates.push(candidate);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Pass 2: Discover raw image files without sidecars
        for entry in &entries {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) if !name.starts_with('.') => name,
                _ => continue,
            };

            let is_image = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| {
                    ext.eq_ignore_ascii_case("png")
                        || ext.eq_ignore_ascii_case("jpg")
                        || ext.eq_ignore_ascii_case("jpeg")
                        || ext.eq_ignore_ascii_case("webp")
                })
                .unwrap_or(false);

            if !is_image {
                continue;
            }

            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };

            let sidecar_path = staging.join(format!("{stem}.json"));
            if known_filenames.contains(file_name)
                || known_ids.contains(stem)
                || sidecar_path.is_file()
            {
                continue;
            }

            let dimensions = image::image_dimensions(&path).unwrap_or((0, 0));
            let created_at = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(Utc::now);

            let candidate = Candidate {
                id: stem.to_string(),
                source: Source::Manual,
                filename: file_name.to_string(),
                dimensions,
                created_at,
            };

            if let Ok(sidecar_json) = serde_json::to_string_pretty(&candidate) {
                let _ = Self::atomic_write(&sidecar_path, sidecar_json.as_bytes());
            }
            known_ids.insert(stem.to_string());
            known_filenames.insert(file_name.to_string());
            candidates.push(candidate);
        }

        candidates.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        Ok(candidates)
    }

    fn delete_staging(&self, id: &str) -> Result<(), StorageError> {
        let staging = self.staging_dir();
        if !staging.is_dir() {
            return Err(StorageError::NotFound(format!(
                "Candidate '{id}' not found in staging"
            )));
        }

        let sidecar_path = staging.join(format!("{id}.json"));
        let candidate: Option<Candidate> = if sidecar_path.is_file() {
            let content = std::fs::read_to_string(&sidecar_path).ok();
            content.and_then(|c| serde_json::from_str(&c).ok())
        } else {
            None
        };

        let mut deleted_any = false;

        // Delete candidate image if found
        if let Some(image_path) = self.find_staging_image_path(id, candidate.as_ref()) {
            if std::fs::remove_file(image_path).is_ok() {
                deleted_any = true;
            }
        }

        // Delete sidecar json
        if sidecar_path.is_file() {
            if std::fs::remove_file(&sidecar_path).is_ok() {
                deleted_any = true;
            }
        }

        if deleted_any {
            Ok(())
        } else {
            Err(StorageError::NotFound(format!(
                "Candidate '{id}' not found in staging"
            )))
        }
    }

    fn save_curated(&self, map: &CuratedMap, data: &[u8]) -> Result<(), StorageError> {
        if !CuratedMap::is_valid_rating(map.rating) {
            return Err(StorageError::InvalidData(format!(
                "Rating must be between 1 and 5, got {}",
                map.rating
            )));
        }

        let theme_dir = self.curated_theme_dir(&map.theme);
        std::fs::create_dir_all(&theme_dir)?;

        let image_path = theme_dir.join(format!("{}.png", map.id));
        let sidecar_path = theme_dir.join(format!("{}.json", map.id));

        Self::atomic_write(&image_path, data)?;

        let sidecar_json = serde_json::to_string_pretty(map)?;
        Self::atomic_write(&sidecar_path, sidecar_json.as_bytes())?;

        Ok(())
    }

    fn get_curated_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError> {
        let image_path = self
            .find_curated_image_path(theme, id)
            .ok_or_else(|| StorageError::NotFound(format!("Curated image '{id}' not found")))?;

        let bytes = std::fs::read(image_path)?;
        Ok(bytes)
    }

    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        let mut results = Vec::new();

        let themes_to_scan: Vec<Theme> = match theme {
            Some(t) => vec![*t],
            None => Theme::ALL.to_vec(),
        };

        for t in themes_to_scan {
            let theme_dir = self.curated_theme_dir(&t);
            if !theme_dir.is_dir() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(theme_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if ext == "json" {
                                let name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or_default();
                                if !name.starts_with('.') {
                                    if let Ok(content) = std::fs::read_to_string(&path) {
                                        if let Ok(map) = serde_json::from_str::<CuratedMap>(&content)
                                        {
                                            results.push(map);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        results.sort_by(|a, b| a.curated_at.cmp(&b.curated_at));
        Ok(results)
    }

    fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let rejected_dir = self.rejected_dir();
        std::fs::create_dir_all(&rejected_dir)?;

        let mut records = self.read_rejections()?;
        records.push(RejectionRecord::with_now(id, reason, notes));

        let json = serde_json::to_string_pretty(&records)?;
        Self::atomic_write(&self.rejections_file(), json.as_bytes())?;

        Ok(())
    }

    fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        let records = self.read_rejections()?;
        Ok(records.iter().any(|r| r.id == id))
    }

    fn curate_staging(&self, map: &CuratedMap) -> Result<(), StorageError> {
        if !CuratedMap::is_valid_rating(map.rating) {
            return Err(StorageError::InvalidData(format!(
                "Rating must be between 1 and 5, got {}",
                map.rating
            )));
        }

        let staging = self.staging_dir();
        let staging_sidecar = staging.join(format!("{}.json", map.id));
        let staging_image = if staging_sidecar.is_file() {
            let sidecar_content = std::fs::read_to_string(&staging_sidecar)?;
            let candidate: Candidate = serde_json::from_str(&sidecar_content)?;
            self.find_staging_image_path(&map.id, Some(&candidate))
                .ok_or_else(|| {
                    StorageError::NotFound(format!(
                        "Image for candidate '{}' not found in staging",
                        map.id
                    ))
                })?
        } else {
            self.find_staging_image_path(&map.id, None)
                .ok_or_else(|| {
                    StorageError::NotFound(format!(
                        "Candidate '{}' not found in staging",
                        map.id
                    ))
                })?
        };

        let ext = staging_image
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");

        let theme_dir = self.curated_theme_dir(&map.theme);
        std::fs::create_dir_all(&theme_dir)?;

        let target_image = theme_dir.join(format!("{}.{ext}", map.id));
        let target_sidecar = theme_dir.join(format!("{}.json", map.id));

        // Atomically move image to curated theme folder
        Self::atomic_move(&staging_image, &target_image)?;

        // Write curated metadata sidecar
        let curated_json = serde_json::to_string_pretty(map)?;
        Self::atomic_write(&target_sidecar, curated_json.as_bytes())?;

        // Remove staging sidecar if it exists
        if staging_sidecar.is_file() {
            let _ = std::fs::remove_file(staging_sidecar);
        }

        Ok(())
    }

    fn reject_staging(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        self.log_rejection(id, reason, notes)?;
        let _ = self.delete_staging(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Source;
    use tempfile::tempdir;

    #[test]
    fn test_save_retrieve_and_list_staging() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let candidate = Candidate::with_now(
            "cand_1",
            Source::Reddit,
            "cand_1.png",
            (1920, 1080),
        );
        let image_data = b"fake-png-bytes-for-candidate-1";

        storage.save_staging(&candidate, image_data).unwrap();

        // Retrieve image
        let retrieved = storage.get_staging_image("cand_1").unwrap();
        assert_eq!(retrieved, image_data);

        // List staging
        let list = storage.list_staging().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "cand_1");
        assert_eq!(list[0].dimensions, (1920, 1080));

        // Delete staging
        storage.delete_staging("cand_1").unwrap();
        assert!(storage.get_staging_image("cand_1").is_err());
        assert_eq!(storage.list_staging().unwrap().len(), 0);
    }

    #[test]
    fn test_curate_candidate_from_staging() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let candidate = Candidate::with_now(
            "map_dungeon_1",
            Source::Reddit,
            "map_dungeon_1.png",
            (2048, 2048),
        );
        let image_bytes = b"real-curated-dungeon-image-bytes";

        storage.save_staging(&candidate, image_bytes).unwrap();

        let curated = CuratedMap::with_now(
            "map_dungeon_1",
            Theme::Dungeon,
            5,
            vec!["boss-room".into(), "gridless".into()],
            (2048, 2048),
        );

        storage.curate_staging(&curated).unwrap();

        // Staging must now be empty
        assert!(storage.get_staging_image("map_dungeon_1").is_err());
        assert_eq!(storage.list_staging().unwrap().len(), 0);

        // Curated image retrieved by theme and id
        let curated_image = storage
            .get_curated_image(&Theme::Dungeon, "map_dungeon_1")
            .unwrap();
        assert_eq!(curated_image, image_bytes);

        // Curated map listed by theme
        let dungeon_maps = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
        assert_eq!(dungeon_maps.len(), 1);
        assert_eq!(dungeon_maps[0].id, "map_dungeon_1");
        assert_eq!(dungeon_maps[0].rating, 5);
        assert_eq!(dungeon_maps[0].theme, Theme::Dungeon);

        // Other theme list should be empty
        let cave_maps = storage.list_curated(Some(&Theme::Cave)).unwrap();
        assert_eq!(cave_maps.len(), 0);

        // List all curated
        let all_curated = storage.list_curated(None).unwrap();
        assert_eq!(all_curated.len(), 1);
    }

    #[test]
    fn test_reject_candidate_workflow() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let candidate = Candidate::with_now(
            "bad_map_99",
            Source::Generated,
            "bad_map_99.png",
            (1024, 1024),
        );
        storage
            .save_staging(&candidate, b"bad-map-image-content")
            .unwrap();

        assert!(!storage.is_rejected("bad_map_99").unwrap());

        storage
            .reject_staging(
                "bad_map_99",
                RejectionReason::GridBurnedIn,
                Some("5ft square grid baked in"),
            )
            .unwrap();

        // Must be removed from staging
        assert!(storage.get_staging_image("bad_map_99").is_err());
        assert_eq!(storage.list_staging().unwrap().len(), 0);

        // Rejection must be logged and detected
        assert!(storage.is_rejected("bad_map_99").unwrap());
        assert!(!storage.is_rejected("unknown_candidate").unwrap());

        // Verify rejections.json content directly
        let rejections_file = storage.rejections_file();
        assert!(rejections_file.is_file());
        let records = storage.read_rejections().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "bad_map_99");
        assert_eq!(records[0].reason, RejectionReason::GridBurnedIn);
        assert_eq!(
            records[0].notes.as_deref(),
            Some("5ft square grid baked in")
        );
    }

    #[test]
    fn test_curated_rating_validation() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let invalid_map_zero = CuratedMap::with_now(
            "invalid_0",
            Theme::Tavern,
            0,
            vec![],
            (1024, 1024),
        );
        let err0 = storage.save_curated(&invalid_map_zero, b"test").unwrap_err();
        match err0 {
            StorageError::InvalidData(msg) => assert!(msg.contains("between 1 and 5")),
            other => panic!("Expected InvalidData error, got {other:?}"),
        }

        let invalid_map_six = CuratedMap::with_now(
            "invalid_6",
            Theme::Tavern,
            6,
            vec![],
            (1024, 1024),
        );
        let err6 = storage.save_curated(&invalid_map_six, b"test").unwrap_err();
        match err6 {
            StorageError::InvalidData(msg) => assert!(msg.contains("between 1 and 5")),
            other => panic!("Expected InvalidData error, got {other:?}"),
        }
    }

    #[test]
    fn test_save_curated_directly() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let map = CuratedMap::with_now(
            "direct_tavern",
            Theme::Tavern,
            4,
            vec!["cozy".into()],
            (1024, 768),
        );
        let data = b"tavern-image-bytes";

        storage.save_curated(&map, data).unwrap();

        let retrieved = storage
            .get_curated_image(&Theme::Tavern, "direct_tavern")
            .unwrap();
        assert_eq!(retrieved, data);

        let list = storage.list_curated(Some(&Theme::Tavern)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "direct_tavern");
    }
    #[test]
    fn test_multiple_rejections_accumulate() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        storage
            .log_rejection("id_1", RejectionReason::IsometricTilt, None)
            .unwrap();
        storage
            .log_rejection(
                "id_2",
                RejectionReason::BlurryLowRes,
                Some("low resolution"),
            )
            .unwrap();
        storage
            .log_rejection("id_3", RejectionReason::BadGeometry, None)
            .unwrap();

        assert!(storage.is_rejected("id_1").unwrap());
        assert!(storage.is_rejected("id_2").unwrap());
        assert!(storage.is_rejected("id_3").unwrap());
        assert!(!storage.is_rejected("id_4").unwrap());

        let records = storage.read_rejections().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].id, "id_1");
        assert_eq!(records[0].reason, RejectionReason::IsometricTilt);
        assert_eq!(records[1].id, "id_2");
        assert_eq!(records[1].reason, RejectionReason::BlurryLowRes);
        assert_eq!(records[2].id, "id_3");
        assert_eq!(records[2].reason, RejectionReason::BadGeometry);
    }

    #[test]
    fn test_curate_nonexistent_candidate_fails() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let map = CuratedMap::with_now(
            "ghost_map",
            Theme::Wilderness,
            3,
            vec![],
            (1024, 1024),
        );

        let res = storage.curate_staging(&map);
        assert!(matches!(res, Err(StorageError::NotFound(_))));
    }

    #[test]
    fn test_get_nonexistent_curated_image_fails() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let res = storage.get_curated_image(&Theme::Ruins, "nonexistent");
        assert!(matches!(res, Err(StorageError::NotFound(_))));
    }
    fn create_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::new(width, height);
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("failed to encode test png");
        bytes
    }

    #[test]
    fn test_raw_image_auto_discovery_and_curation() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        let staging = storage.staging_dir();
        std::fs::create_dir_all(&staging).unwrap();

        let png_data = create_test_png(800, 600);
        let image_path = staging.join("raw_map.png");
        std::fs::write(&image_path, &png_data).unwrap();

        // Ensure no sidecar JSON file exists initially
        let sidecar_path = staging.join("raw_map.json");
        assert!(!sidecar_path.exists());

        // list_staging must discover the raw image as Candidate with Source::Manual and dimensions
        let candidates = storage.list_staging().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "raw_map");
        assert_eq!(candidates[0].source, Source::Manual);
        assert_eq!(candidates[0].filename, "raw_map.png");
        assert_eq!(candidates[0].dimensions, (800, 600));

        // Sidecar must now be persisted on disk
        assert!(sidecar_path.is_file());

        // get_staging_image returns the raw image bytes
        let retrieved = storage.get_staging_image("raw_map").unwrap();
        assert_eq!(retrieved, png_data);

        // curate_staging curates the raw image to Theme::Dungeon
        let curated = CuratedMap::with_now(
            "raw_map",
            Theme::Dungeon,
            5,
            vec!["dungeon-crawl".into()],
            (800, 600),
        );
        storage.curate_staging(&curated).unwrap();

        // Staging should now be empty
        assert_eq!(storage.list_staging().unwrap().len(), 0);
        assert!(storage.get_staging_image("raw_map").is_err());
        assert!(!image_path.exists());
        assert!(!sidecar_path.exists());

        // Curated image and metadata exist in curated theme folder
        let curated_image = storage
            .get_curated_image(&Theme::Dungeon, "raw_map")
            .unwrap();
        assert_eq!(curated_image, png_data);
        let curated_maps = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
        assert_eq!(curated_maps.len(), 1);
        assert_eq!(curated_maps[0].id, "raw_map");
        assert_eq!(curated_maps[0].theme, Theme::Dungeon);
        assert_eq!(curated_maps[0].rating, 5);
    }

    #[test]
    fn test_raw_image_rejection_without_prior_sidecar() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        let staging = storage.staging_dir();
        std::fs::create_dir_all(&staging).unwrap();

        let png_data = create_test_png(400, 300);
        let image_path = staging.join("raw_bad_map.png");
        std::fs::write(&image_path, &png_data).unwrap();
        assert!(!staging.join("raw_bad_map.json").exists());

        // Reject without calling list_staging first
        storage
            .reject_staging(
                "raw_bad_map",
                RejectionReason::BlurryLowRes,
                Some("low resolution"),
            )
            .unwrap();

        assert!(!image_path.exists());
        assert!(!staging.join("raw_bad_map.json").exists());
        assert!(storage.is_rejected("raw_bad_map").unwrap());
        assert_eq!(storage.list_staging().unwrap().len(), 0);
    }

    #[test]
    fn test_raw_image_curation_without_prior_list_staging() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        let staging = storage.staging_dir();
        std::fs::create_dir_all(&staging).unwrap();

        let png_data = create_test_png(500, 500);
        let image_path = staging.join("direct_map.png");
        std::fs::write(&image_path, &png_data).unwrap();
        assert!(!staging.join("direct_map.json").exists());

        // Curate directly when no sidecar was ever generated
        let curated = CuratedMap::with_now(
            "direct_map",
            Theme::Cave,
            4,
            vec!["crystal-cave".into()],
            (500, 500),
        );
        storage.curate_staging(&curated).unwrap();

        assert!(!image_path.exists());
        let curated_image = storage.get_curated_image(&Theme::Cave, "direct_map").unwrap();
        assert_eq!(curated_image, png_data);
    }
}
