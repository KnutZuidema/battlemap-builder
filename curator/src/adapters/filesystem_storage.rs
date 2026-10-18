use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

use crate::adapters::SimdImageProcessor;
use crate::domain::{
    infer_bundle_layer_roles, normalize_tags, validate_layers_for_curation, BenchmarkMetadata, Candidate,
    CaptionGenerator, CuratedMap, CuratedMapUpdate, DirectoryStats,
    Layer, LayerRole, PurgeStagingResult, RedditSidecarMetadata, RejectionReason, RejectionRecord,
    Source, StorageStats, Theme, ThemeStorageStats,
};
use crate::ports::image_processor::{ImageProcessor, ResizeStrategy};
use crate::ports::curation_store::CurationStore;
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
    /// Automatically runs idempotent legacy flat-file migration if `staging/` or `curated/` exist.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        let storage = Self {
            root_dir: root_dir.into(),
        };
        let _ = storage.migrate_legacy_storage();
        storage
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

    /// Directory where telemetry and cost logs reside.
    pub fn telemetry_dir(&self) -> PathBuf {
        self.root_dir.join("telemetry")
    }

    /// Recursively scans a directory to calculate total byte size and total file count.
    /// Skips files and directories starting with `.`.
    pub fn scan_dir_stats(dir: &Path) -> DirectoryStats {
        if !dir.is_dir() {
            return DirectoryStats::default();
        }
        let mut total_bytes = 0u64;
        let mut total_files = 0usize;
        let mut stack = vec![dir.to_path_buf()];

        while let Some(current) = stack.pop() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if file_name.starts_with('.') {
                        continue;
                    }
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            stack.push(path);
                        } else if file_type.is_file() {
                            total_files += 1;
                            if let Ok(meta) = entry.metadata() {
                                total_bytes += meta.len();
                            }
                        }
                    }
                }
            }
        }

        DirectoryStats {
            bytes: total_bytes,
            file_count: total_files,
        }
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

    /// Extract the lowercase image extension from a filename, defaulting to `"png"`.
    fn extract_extension(filename: &str) -> String {
        Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.trim().to_lowercase())
            .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
            .unwrap_or_else(|| "png".to_string())
    }

    fn is_image_ext(ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
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
    /// Helper to process primary image bytes and write LoRA 1024x1024 PNG and companion .txt caption in bundle dir.
    fn write_lora_companions(
        &self,
        id: &str,
        primary_image_bytes: &[u8],
        caption_text: &str,
        bundle_dir: &Path,
    ) {
        let _ = std::fs::create_dir_all(bundle_dir);

        // Always write caption text (.txt)
        let bundle_txt = bundle_dir.join(format!("{id}_lora1024.txt"));
        let _ = Self::atomic_write(&bundle_txt, caption_text.as_bytes());

        // Try normalizing to 1024x1024 PNG using SIMD Lanczos3 resizer
        let processor = SimdImageProcessor::new();
        match processor.normalize_lora(primary_image_bytes, ResizeStrategy::FitWithPadding) {
            Ok(lora_png) => {
                let bundle_png = bundle_dir.join(format!("{id}_lora1024.png"));
                let _ = Self::atomic_write(&bundle_png, &lora_png);
            }
            Err(e) => {
                tracing::debug!("Skipping LoRA 1024x1024 image normalization for '{id}': {e}");
            }
        }
    }

    /// Parse a JSON string as a `Candidate` (falling back to `BenchmarkMetadata`).
    fn parse_candidate_json(content: &str) -> Option<Candidate> {
        serde_json::from_str::<Candidate>(content).ok().or_else(|| {
            serde_json::from_str::<BenchmarkMetadata>(content)
                .ok()
                .map(|b| b.to_candidate())
        })
    }

    /// Locate a staging bundle directory `<dir>/<id>/` (containing `manifest.json`).
    fn find_staging_bundle_dir(&self, id: &str) -> Option<PathBuf> {
        let staging = self.staging_dir();
        if !staging.is_dir() {
            return None;
        }
        for sub in [None, Some("reddit"), Some("generated")] {
            let base = match sub {
                Some(s) => staging.join(s),
                None => staging.clone(),
            };
            let bundle_dir = base.join(id);
            if bundle_dir.join("manifest.json").is_file() {
                return Some(bundle_dir);
            }
        }
        Self::find_bundle_dir_recursive(&staging, id)
    }

    fn find_bundle_dir_recursive(dir: &Path, id: &str) -> Option<PathBuf> {
        if !dir.is_dir() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if dir_name.starts_with('.') {
                    continue;
                }
                if dir_name == id && path.join("manifest.json").is_file() {
                    return Some(path);
                }
                if !path.join("manifest.json").is_file() {
                    if let Some(found) = Self::find_bundle_dir_recursive(&path, id) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Locate candidate metadata sidecar (`manifest.json` inside bundle dir or legacy `<id>.json`) by ID.
    fn find_staging_sidecar_path(&self, id: &str) -> Option<PathBuf> {
        if let Some(bundle_dir) = self.find_staging_bundle_dir(id) {
            return Some(bundle_dir.join("manifest.json"));
        }

        let staging = self.staging_dir();
        let target_name = format!("{id}.json");

        let direct = staging.join(&target_name);
        if direct.is_file() {
            return Some(direct);
        }

        let reddit_path = staging.join("reddit").join(&target_name);
        if reddit_path.is_file() {
            return Some(reddit_path);
        }

        let generated_path = staging.join("generated").join(&target_name);
        if generated_path.is_file() {
            return Some(generated_path);
        }

        Self::find_file_recursive(&staging, &target_name)
    }

    /// Recursively search for a file named `target_name` in `dir`, skipping hidden paths.
    fn find_file_recursive(dir: &Path, target_name: &str) -> Option<PathBuf> {
        if !dir.is_dir() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if file_name.starts_with('.') {
                    continue;
                }
                if path.is_file() && file_name == target_name {
                    return Some(path);
                } else if path.is_dir() {
                    if let Some(found) = Self::find_file_recursive(&path, target_name) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Recursively search for a flat image file whose stem matches `id`, skipping bundle directories.
    fn find_image_by_stem_recursive(dir: &Path, id: &str) -> Option<PathBuf> {
        if !dir.is_dir() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if file_name.starts_with('.') {
                    continue;
                }
                if path.is_file() {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if stem == id {
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                if Self::is_image_ext(ext) {
                                    return Some(path);
                                }
                            }
                        }
                    }
                } else if path.is_dir() && !path.join("manifest.json").is_file() {
                    if let Some(found) = Self::find_image_by_stem_recursive(&path, id) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Locate layer image path in a staging bundle or legacy flat file.
    fn find_staging_layer_image_path(&self, id: &str, layer_index: usize) -> Option<PathBuf> {
        if let Some(bundle_dir) = self.find_staging_bundle_dir(id) {
            let manifest_path = bundle_dir.join("manifest.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Some(cand) = Self::parse_candidate_json(&content) {
                    let layers = cand.resolved_layers();
                    if let Some(layer) = layers
                        .iter()
                        .find(|l| l.index == layer_index)
                        .or_else(|| layers.get(layer_index))
                    {
                        let p = bundle_dir.join(&layer.filename);
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
            for ext in &["png", "jpg", "jpeg", "webp"] {
                let p = bundle_dir.join(format!("{layer_index}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
            return None;
        }

        if layer_index == 0 {
            let sidecar_path = self.find_staging_sidecar_path(id);
            let cand = sidecar_path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|c| Self::parse_candidate_json(&c));
            return self.find_staging_image_path(id, cand.as_ref());
        }

        None
    }

    /// Locate primary candidate image in staging or any subdirectories by ID and candidate metadata.
    fn find_staging_image_path(&self, id: &str, candidate: Option<&Candidate>) -> Option<PathBuf> {
        if let Some(bundle_dir) = self.find_staging_bundle_dir(id) {
            let layers = candidate
                .map(|c| c.resolved_layers())
                .or_else(|| {
                    std::fs::read_to_string(bundle_dir.join("manifest.json"))
                        .ok()
                        .and_then(|s| Self::parse_candidate_json(&s))
                        .map(|c| c.resolved_layers())
                })
                .unwrap_or_default();
            if let Some(first) = layers.first() {
                let p = bundle_dir.join(&first.filename);
                if p.is_file() {
                    return Some(p);
                }
            }
            for ext in &["png", "jpg", "jpeg", "webp"] {
                let p = bundle_dir.join(format!("0.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }

        let staging = self.staging_dir();
        if let Some(cand) = candidate {
            if !cand.filename.is_empty() {
                let direct = staging.join(&cand.filename);
                if direct.is_file() {
                    return Some(direct);
                }
                let reddit_cand = staging.join("reddit").join(&cand.filename);
                if reddit_cand.is_file() {
                    return Some(reddit_cand);
                }
                if let Some(sidecar_path) = self.find_staging_sidecar_path(id) {
                    if let Some(parent) = sidecar_path.parent() {
                        let in_parent = parent.join(&cand.filename);
                        if in_parent.is_file() {
                            return Some(in_parent);
                        }
                    }
                }
            }
        }

        for ext in &["png", "jpg", "jpeg", "webp"] {
            let direct = staging.join(format!("{id}.{ext}"));
            if direct.is_file() {
                return Some(direct);
            }
            let reddit_path = staging.join("reddit").join(format!("{id}.{ext}"));
            if reddit_path.is_file() {
                return Some(reddit_path);
            }
            let gen_path = staging.join("generated").join(format!("{id}.{ext}"));
            if gen_path.is_file() {
                return Some(gen_path);
            }
        }

        if let Some(sidecar_path) = self.find_staging_sidecar_path(id) {
            if let Some(parent) = sidecar_path.parent() {
                for ext in &["png", "jpg", "jpeg", "webp"] {
                    let in_parent = parent.join(format!("{id}.{ext}"));
                    if in_parent.is_file() {
                        return Some(in_parent);
                    }
                }
            }
        }

        Self::find_image_by_stem_recursive(&staging, id)
    }

    /// Helper to find the bundle directory for a curated map ID (flat curated/<id>/ first, fallback to legacy theme dirs).
    pub fn find_curated_bundle_dir(&self, id: &str) -> Option<PathBuf> {
        let flat = self.curated_dir().join(id);
        if flat.is_dir() {
            return Some(flat);
        }
        for theme in Theme::ALL {
            let legacy = self.curated_theme_dir(theme).join(id);
            if legacy.is_dir() {
                return Some(legacy);
            }
        }
        None
    }

    /// Locate curated layer image by ID and layer index.
    fn find_curated_layer_image_path(
        &self,
        theme: &Theme,
        id: &str,
        layer_index: usize,
    ) -> Option<PathBuf> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            let manifest_path = bundle_dir.join("manifest.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
                    if let Some(layer) = map
                        .layers
                        .iter()
                        .find(|l| l.index == layer_index)
                        .or_else(|| map.layers.get(layer_index))
                    {
                        let p = bundle_dir.join(&layer.filename);
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
            let prefix = format!("{layer_index}_");
            if let Ok(entries) = std::fs::read_dir(&bundle_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let name = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default();
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                        if name.starts_with(&prefix) || stem == layer_index.to_string() {
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                if Self::is_image_ext(ext) {
                                    return Some(p);
                                }
                            }
                        }
                    }
                }
            }
            return None;
        }

        if layer_index == 0 {
            return self.find_curated_image_path(theme, id);
        }
        None
    }

    /// Locate curated layer image by ID and layer index directly, checking bundle dir,
    /// falling back across themes and loose images.
    pub fn find_curated_layer_image_by_id_path(
        &self,
        id: &str,
        layer_index: usize,
    ) -> Option<PathBuf> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            let manifest_path = bundle_dir.join("manifest.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
                    if let Some(layer) = map
                        .layers
                        .iter()
                        .find(|l| l.index == layer_index)
                        .or_else(|| map.layers.get(layer_index))
                    {
                        let p = bundle_dir.join(&layer.filename);
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
            let prefix = format!("{layer_index}_");
            if let Ok(entries) = std::fs::read_dir(&bundle_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let name = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default();
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                        if (name.starts_with(&prefix) || stem == layer_index.to_string())
                            && !name.contains("lora1024")
                        {
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                if Self::is_image_ext(ext) {
                                    return Some(p);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback across themes
        for theme in Theme::ALL {
            if let Some(p) = self.find_curated_layer_image_path(theme, id, layer_index) {
                return Some(p);
            }
        }

        // Check flat curated directory for loose images if layer_index == 0
        if layer_index == 0 {
            for ext in &["png", "jpg", "jpeg", "webp"] {
                let candidate_path = self.curated_dir().join(format!("{id}.{ext}"));
                if candidate_path.is_file() {
                    return Some(candidate_path);
                }
            }
        }

        None
    }

    /// Package a collection of curated maps' LoRA 1024x1024 PNG and companion .txt captions into a ZIP archive.
    fn create_lora_zip(&self, curated_maps: &[CuratedMap]) -> Result<Vec<u8>, StorageError> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            for map in curated_maps {
                let theme_str = map.primary_theme();
                let primary_theme = map.theme().unwrap_or(Theme::Dungeon);

                let image_bytes = match self.get_curated_lora_image(&primary_theme, &map.id) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        if let Ok(primary) = self.get_curated_image(&primary_theme, &map.id) {
                            let processor = SimdImageProcessor::new();
                            match processor.normalize_lora(&primary, ResizeStrategy::FitWithPadding) {
                                Ok(normalized) => normalized,
                                Err(_) => primary,
                            }
                        } else {
                            continue;
                        }
                    }
                };

                let caption_text = match self.get_curated_lora_caption(&primary_theme, &map.id) {
                    Ok(c) => c,
                    Err(_) => CaptionGenerator::generate_for_curated(map, None),
                };

                let img_filename = format!("{theme_str}/{}_lora1024.png", map.id);
                zip.start_file(&img_filename, options)?;
                zip.write_all(&image_bytes)?;

                let txt_filename = format!("{theme_str}/{}_lora1024.txt", map.id);
                zip.start_file(&txt_filename, options)?;
                zip.write_all(caption_text.as_bytes())?;
            }

            zip.finish()?;
        }

        Ok(buffer.into_inner())
    }

    /// Locate primary curated image by ID.
    fn find_curated_image_path(&self, theme: &Theme, id: &str) -> Option<PathBuf> {
        if let Some(_bundle_dir) = self.find_curated_bundle_dir(id) {
            if let Some(p) = self.find_curated_layer_image_path(theme, id, 0) {
                return Some(p);
            }
        }

        // Check flat curated directory for loose images
        for ext in &["png", "jpg", "jpeg", "webp"] {
            let candidate_path = self.curated_dir().join(format!("{id}.{ext}"));
            if candidate_path.is_file() {
                return Some(candidate_path);
            }
        }

        // Fallback for unmigrated legacy theme directories
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
                                if Self::is_image_ext(ext) {
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
    pub fn read_rejections(&self) -> Result<Vec<RejectionRecord>, StorageError> {
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

    /// Idempotent startup migration that converts legacy flat staging/curated files
    /// (including sibling `{post_id}_0`, `{post_id}_1` Reddit items) into uniform `<id>/`
    /// bundle directories containing `manifest.json` and indexed layer images.
    #[allow(clippy::type_complexity)]
    pub fn migrate_legacy_storage(&self) -> Result<(), StorageError> {
        let staging = self.staging_dir();
        if staging.is_dir() {
            self.migrate_staging_directory(&staging)?;
            let reddit_dir = staging.join("reddit");
            if reddit_dir.is_dir() {
                self.migrate_staging_directory(&reddit_dir)?;
            }
            let generated_dir = staging.join("generated");
            if generated_dir.is_dir() {
                self.migrate_staging_directory(&generated_dir)?;
            }
        }

        let curated = self.curated_dir();
        if curated.is_dir() {
            for theme in Theme::ALL {
                let theme_dir = self.curated_theme_dir(theme);
                if theme_dir.is_dir() {
                    let _ = self.migrate_curated_theme_directory(theme, &theme_dir);

                    // Move all subdirectories from theme_dir up to curated/<id>
                    if let Ok(entries) = std::fs::read_dir(&theme_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                let id = match path.file_name().and_then(|n| n.to_str()) {
                                    Some(name) => name.to_string(),
                                    None => continue,
                                };
                                let target_dir = curated.join(&id);
                                if !target_dir.exists() {
                                    let _ = Self::atomic_move(&path, &target_dir);
                                } else {
                                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                                        for sub in sub_entries.flatten() {
                                            let sub_path = sub.path();
                                            if let Some(sub_name) = sub_path.file_name() {
                                                let target_file = target_dir.join(sub_name);
                                                if !target_file.exists() {
                                                    let _ = Self::atomic_move(&sub_path, &target_file);
                                                }
                                            }
                                        }
                                    }
                                    let _ = std::fs::remove_dir_all(&path);
                                }
                            }
                        }
                    }

                    // Remove empty theme_dir
                    let is_empty = std::fs::read_dir(&theme_dir)
                        .map(|mut r| r.next().is_none())
                        .unwrap_or(false);
                    if is_empty {
                        let _ = std::fs::remove_dir(&theme_dir);
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn migrate_staging_directory(&self, dir: &Path) -> Result<(), StorageError> {
        let mut flat_items: Vec<(String, PathBuf, PathBuf, Candidate)> = Vec::new();

        let entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .collect();

        for json_path in &entries {
            if !json_path.is_file() {
                continue;
            }
            let file_name = json_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if file_name.starts_with('.') || file_name == "manifest.json" {
                continue;
            }
            let ext = json_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !ext.eq_ignore_ascii_case("json") {
                continue;
            }
            let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };

            let content = match std::fs::read_to_string(json_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let cand = match Self::parse_candidate_json(&content) {
                Some(c) => c,
                None => continue,
            };

            // Find matching flat image file in `dir`
            let mut img_path_opt: Option<PathBuf> = None;
            if !cand.filename.is_empty() {
                let p = dir.join(
                    Path::new(&cand.filename)
                        .file_name()
                        .unwrap_or_default(),
                );
                if p.is_file() {
                    img_path_opt = Some(p);
                }
            }
            if img_path_opt.is_none() {
                for img_ext in &["png", "jpg", "jpeg", "webp"] {
                    let p = dir.join(format!("{stem}.{img_ext}"));
                    if p.is_file() {
                        img_path_opt = Some(p);
                        break;
                    }
                }
            }

            if let Some(img_path) = img_path_opt {
                flat_items.push((stem, json_path.clone(), img_path, cand));
            }
        }

        if flat_items.is_empty() {
            return Ok(());
        }

        // Partition into Reddit multi-part groups (`{post_id}_{idx}` without `parent_group_id`)
        // vs standalone flat items.
        let mut reddit_groups: BTreeMap<String, Vec<(usize, String, PathBuf, PathBuf, Candidate)>> =
            BTreeMap::new();
        let mut standalone: Vec<(String, PathBuf, PathBuf, Candidate)> = Vec::new();

        for (stem, json_path, img_path, cand) in flat_items {
            let mut grouped = false;
            if cand.source == Source::Reddit && cand.parent_group_id.is_none() {
                if let Some((base_id, suffix)) = stem.rsplit_once('_') {
                    if !base_id.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(idx) = suffix.parse::<usize>() {
                            reddit_groups
                                .entry(base_id.to_string())
                                .or_default()
                                .push((idx, stem.clone(), json_path.clone(), img_path.clone(), cand.clone()));
                            grouped = true;
                        }
                    }
                }
            }
            if !grouped {
                standalone.push((stem, json_path, img_path, cand));
            }
        }

        for (base_id, mut group) in reddit_groups {
            group.sort_by_key(|(idx, _, _, _, _)| *idx);
            let bundle_dir = dir.join(&base_id);
            std::fs::create_dir_all(&bundle_dir)?;

            let captions: Vec<Option<String>> = group
                .iter()
                .map(|(_, _, _, _, cand)| {
                    cand.layers
                        .first()
                        .and_then(|l| l.caption.clone())
                })
                .collect();
            let title_opt = group.first().and_then(|(_, _, _, _, c)| c.title.as_deref());
            let inferred_roles = infer_bundle_layer_roles(&captions, title_opt);

            let mut layers = Vec::with_capacity(group.len());
            for (new_idx, ((_, _, json_path, img_path, cand), (role, elevation))) in
                group.iter().zip(inferred_roles).enumerate()
            {
                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let layer_filename = format!("{new_idx}.{ext}");
                let target_img = bundle_dir.join(&layer_filename);
                Self::atomic_move(img_path, &target_img)?;
                let _ = std::fs::remove_file(json_path);

                let mut dims = cand.dimensions;
                if dims == (0, 0) {
                    dims = cand
                        .parsed_dimensions
                        .or_else(|| image::image_dimensions(&target_img).ok())
                        .unwrap_or((0, 0));
                }

                let caption = cand.layers.first().and_then(|l| l.caption.clone());
                layers.push(Layer {
                    index: new_idx,
                    filename: layer_filename,
                    role,
                    elevation,
                    caption,
                    source_url: cand.source_url.clone(),
                    dimensions: dims,
                });
            }

            let first_cand = &group[0].4;
            let first_layer_file = layers
                .first()
                .map(|l| l.filename.clone())
                .unwrap_or_else(|| "0.png".to_string());
            let first_dims = layers
                .first()
                .map(|l| l.dimensions)
                .unwrap_or(first_cand.dimensions);

            let bundle_cand = Candidate {
                id: base_id.clone(),
                source: Source::Reddit,
                filename: format!("{base_id}/{first_layer_file}"),
                dimensions: first_dims,
                created_at: first_cand.created_at,
                source_url: first_cand.source_url.clone(),
                title: first_cand.title.clone(),
                author: first_cand.author.clone(),
                parsed_gridless: first_cand.parsed_gridless,
                parsed_dimensions: first_cand.parsed_dimensions,
                layers,
                parent_group_id: None,
            };

            let manifest_json = serde_json::to_string_pretty(&bundle_cand)?;
            Self::atomic_write(&bundle_dir.join("manifest.json"), manifest_json.as_bytes())?;
        }

        for (stem, json_path, img_path, mut cand) in standalone {
            let bundle_dir = dir.join(&stem);
            std::fs::create_dir_all(&bundle_dir)?;

            let ext = img_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let layer_filename = format!("0.{ext}");
            let target_img = bundle_dir.join(&layer_filename);

            Self::atomic_move(&img_path, &target_img)?;

            // If the original json_path was a BenchmarkMetadata file, also preserve benchmark fields
            // or serialize the unified Candidate into manifest.json.
            let _ = std::fs::remove_file(&json_path);

            if cand.dimensions == (0, 0) {
                cand.dimensions = cand
                    .parsed_dimensions
                    .or_else(|| image::image_dimensions(&target_img).ok())
                    .unwrap_or((0, 0));
            }
            cand.id = stem.clone();
            if cand.filename.is_empty() {
                cand.filename = format!("{stem}/{layer_filename}");
            }
            if cand.layers.is_empty() {
                cand.layers = vec![Layer {
                    index: 0,
                    filename: layer_filename,
                    role: LayerRole::Ground,
                    elevation: Some(0),
                    caption: None,
                    source_url: cand.source_url.clone(),
                    dimensions: cand.dimensions,
                }];
            }

            let manifest_json = serde_json::to_string_pretty(&cand)?;
            Self::atomic_write(&bundle_dir.join("manifest.json"), manifest_json.as_bytes())?;
        }

        Ok(())
    }

    fn migrate_curated_theme_directory(
        &self,
        theme: &Theme,
        theme_dir: &Path,
    ) -> Result<(), StorageError> {
        let entries: Vec<PathBuf> = std::fs::read_dir(theme_dir)?
            .flatten()
            .map(|e| e.path())
            .collect();

        for json_path in &entries {
            if !json_path.is_file() {
                continue;
            }
            let file_name = json_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if file_name.starts_with('.') || file_name == "manifest.json" {
                continue;
            }
            let ext = json_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !ext.eq_ignore_ascii_case("json") {
                continue;
            }
            let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };

            let content = match std::fs::read_to_string(json_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let mut map: CuratedMap = match serde_json::from_str(&content) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mut img_path_opt: Option<PathBuf> = None;
            for img_ext in &["png", "jpg", "jpeg", "webp"] {
                let p = theme_dir.join(format!("{stem}.{img_ext}"));
                if p.is_file() {
                    img_path_opt = Some(p);
                    break;
                }
            }

            if let Some(img_path) = img_path_opt {
                let bundle_dir = theme_dir.join(&stem);
                std::fs::create_dir_all(&bundle_dir)?;

                let img_ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let layer_filename = Layer::format_filename(0, LayerRole::Ground, Some(0), img_ext);
                let target_img = bundle_dir.join(&layer_filename);

                Self::atomic_move(&img_path, &target_img)?;
                let _ = std::fs::remove_file(json_path);

                map.id = stem;
                if map.tags.is_empty() {
                    map.tags = vec![theme.as_str().to_string()];
                }
                if map.layers.is_empty() {
                    map.layers = vec![Layer::new(
                        0,
                        layer_filename,
                        LayerRole::Ground,
                        Some(0),
                        map.dimensions,
                    )];
                }
                let manifest_json = serde_json::to_string_pretty(&map)?;
                Self::atomic_write(&bundle_dir.join("manifest.json"), manifest_json.as_bytes())?;
            }
        }

        Ok(())
    }

    /// Helper to persist a candidate bundle to a target staging directory (`staging/` or `staging/<subfolder>/`).
    fn write_staging_bundle_to_dir(
        &self,
        target_base_dir: &Path,
        candidate: &Candidate,
        layer_images: &[Vec<u8>],
    ) -> Result<(), StorageError> {
        if candidate.id.trim().is_empty() {
            return Err(StorageError::InvalidData(
                "Candidate ID cannot be empty".into(),
            ));
        }
        if layer_images.is_empty() {
            return Err(StorageError::InvalidData(
                "Bundle must contain at least one layer image".into(),
            ));
        }

        let bundle_dir = target_base_dir.join(&candidate.id);
        std::fs::create_dir_all(&bundle_dir)?;

        let mut stored_candidate = candidate.clone();
        if stored_candidate.dimensions == (0, 0) {
            if let Some(dims) = stored_candidate.parsed_dimensions {
                stored_candidate.dimensions = dims;
            }
        }

        if stored_candidate.layers.is_empty() {
            let default_ext = Self::extract_extension(&stored_candidate.filename);
            if layer_images.len() == 1 {
                let layer_file = format!("0.{default_ext}");
                stored_candidate.layers = vec![Layer {
                    index: 0,
                    filename: layer_file,
                    role: LayerRole::Ground,
                    elevation: Some(0),
                    caption: None,
                    source_url: stored_candidate.source_url.clone(),
                    dimensions: stored_candidate.dimensions,
                }];
            } else {
                let captions: Vec<Option<String>> = vec![None; layer_images.len()];
                let roles =
                    infer_bundle_layer_roles(&captions, stored_candidate.title.as_deref());
                stored_candidate.layers = roles
                    .into_iter()
                    .enumerate()
                    .map(|(i, (role, elevation))| Layer {
                        index: i,
                        filename: format!("{i}.{default_ext}"),
                        role,
                        elevation,
                        caption: None,
                        source_url: stored_candidate.source_url.clone(),
                        dimensions: stored_candidate.dimensions,
                    })
                    .collect();
            }
        }

        if stored_candidate.layers.len() != layer_images.len() {
            return Err(StorageError::InvalidData(format!(
                "Candidate layers count ({}) does not match layer_images count ({})",
                stored_candidate.layers.len(),
                layer_images.len()
            )));
        }

        for (idx, (layer, bytes)) in stored_candidate
            .layers
            .iter_mut()
            .zip(layer_images.iter())
            .enumerate()
        {
            layer.index = idx;
            if layer.filename.trim().is_empty() {
                layer.filename = format!("{idx}.png");
            } else {
                layer.filename = Self::sanitize_filename(&layer.filename, &idx.to_string());
            }
            if layer.dimensions == (0, 0) {
                layer.dimensions = image::load_from_memory(bytes)
                    .map(|img| (img.width(), img.height()))
                    .unwrap_or(stored_candidate.dimensions);
            }
            let img_path = bundle_dir.join(&layer.filename);
            Self::atomic_write(&img_path, bytes)?;
        }

        if stored_candidate.dimensions == (0, 0) {
            if let Some(first) = stored_candidate.layers.first() {
                stored_candidate.dimensions = first.dimensions;
            }
        }
        if stored_candidate.filename.trim().is_empty() {
            let first_file = stored_candidate
                .layers
                .first()
                .map(|l| l.filename.as_str())
                .unwrap_or("0.png");
            stored_candidate.filename = format!("{}/{first_file}", stored_candidate.id);
        }

        let manifest_path = bundle_dir.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&stored_candidate)?;
        Self::atomic_write(&manifest_path, manifest_json.as_bytes())?;

        Ok(())
    }
    /// Retrieve candidate metadata from staging by ID directly ($O(1)$) without listing directory.
    pub fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        if let Some(bundle_dir) = self.find_staging_bundle_dir(id) {
            let manifest_path = bundle_dir.join("manifest.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Some(cand) = Self::parse_candidate_json(&content) {
                    return Ok(cand);
                }
            }
        }

        if let Some(sidecar_path) = self.find_staging_sidecar_path(id) {
            if sidecar_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&sidecar_path) {
                    if let Some(cand) = Self::parse_candidate_json(&content) {
                        return Ok(cand);
                    }
                }
            }
        }

        if let Some(image_path) = self.find_staging_image_path(id, None) {
            if image_path.is_file() {
                let dimensions = image::image_dimensions(&image_path).unwrap_or((0, 0));
                let created_at = image_path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(DateTime::<Utc>::from)
                    .unwrap_or_else(Utc::now);
                let ext = image_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let layer_filename = format!("0.{ext}");
                let rel_filename = image_path
                    .strip_prefix(&self.staging_dir())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| format!("{id}.{ext}"));
                let cand = Candidate::new(
                    id.to_string(),
                    Source::Manual,
                    rel_filename,
                    dimensions,
                    created_at,
                )
                .with_layers(vec![Layer {
                    index: 0,
                    filename: layer_filename,
                    role: LayerRole::Ground,
                    elevation: Some(0),
                    caption: None,
                    source_url: None,
                    dimensions,
                }]);
                return Ok(cand);
            }
        }

        Err(StorageError::NotFound(format!(
            "Candidate '{id}' not found in staging"
        )))
    }

}

impl StorageRepository for FilesystemStorage {
    fn save_staging(&self, candidate: &Candidate, data: &[u8]) -> Result<(), StorageError> {
        let staging = self.staging_dir();
        self.write_staging_bundle_to_dir(&staging, candidate, &[data.to_vec()])
    }

    fn save_staging_subfolder(
        &self,
        subfolder: &str,
        candidate: &Candidate,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let target_dir = self.staging_dir().join(subfolder);
        self.write_staging_bundle_to_dir(&target_dir, candidate, &[data.to_vec()])
    }

    fn save_staging_bundle(
        &self,
        subfolder: Option<&str>,
        candidate: &Candidate,
        layer_images: &[Vec<u8>],
    ) -> Result<(), StorageError> {
        let target_dir = match subfolder {
            Some(sub) if !sub.trim().is_empty() => self.staging_dir().join(sub),
            _ => self.staging_dir(),
        };
        self.write_staging_bundle_to_dir(&target_dir, candidate, layer_images)
    }

    fn save_staging_reddit(
        &self,
        metadata: &RedditSidecarMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        if metadata.id.trim().is_empty() {
            return Err(StorageError::InvalidData(
                "Metadata ID cannot be empty".into(),
            ));
        }
        let candidate = metadata.to_candidate();
        self.save_staging_subfolder("reddit", &candidate, data)
    }

    fn save_staging_generated(
        &self,
        metadata: &BenchmarkMetadata,
        data: &[u8],
    ) -> Result<(), StorageError> {
        if metadata.id.trim().is_empty() {
            return Err(StorageError::InvalidData(
                "Metadata ID cannot be empty".into(),
            ));
        }
        let candidate = metadata.to_candidate();
        let target_dir = self.staging_dir().join("generated");
        self.write_staging_bundle_to_dir(&target_dir, &candidate, &[data.to_vec()])?;
        let bundle_dir = target_dir.join(&metadata.id);
        let meta_json = serde_json::to_string_pretty(metadata)?;
        Self::atomic_write(&bundle_dir.join("manifest.json"), meta_json.as_bytes())?;
        Ok(())
    }
    fn get_staging_image(&self, id: &str) -> Result<Vec<u8>, StorageError> {
        self.get_staging_layer_image(id, 0)
    }

    fn get_staging_layer_image(
        &self,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let image_path = self
            .find_staging_layer_image_path(id, layer_index)
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Staging image layer {layer_index} for candidate '{id}' not found"
                ))
            })?;
        let bytes = std::fs::read(image_path)?;
        Ok(bytes)
    }

    fn list_staging(&self) -> Result<Vec<Candidate>, StorageError> {
        let staging = self.staging_dir();
        if !staging.is_dir() {
            return Ok(Vec::new());
        }

        // Migrate any legacy flat `{id}.json` + `{id}.png` pairs (including `{post_id}_{idx}`) first
        let _ = self.migrate_legacy_storage();

        let mut candidates = Vec::new();
        let mut known_ids = HashSet::new();

        // Scan staging root and subfolder directories (`reddit`, `generated`, etc.)
        let mut search_dirs = vec![staging.clone()];
        if let Ok(entries) = std::fs::read_dir(&staging) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && !path.join("manifest.json").is_file() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if !name.starts_with('.') {
                        search_dirs.push(path);
                    }
                }
            }
        }

        // Pass 1: Read bundle directories (`<dir>/<id>/manifest.json`)
        for base_dir in &search_dirs {
            let entries: Vec<PathBuf> = match std::fs::read_dir(base_dir) {
                Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
                Err(_) => continue,
            };
            for entry_path in entries {
                if !entry_path.is_dir() {
                    continue;
                }
                let dir_name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if dir_name.starts_with('.') {
                    continue;
                }
                let manifest_path = entry_path.join("manifest.json");
                if !manifest_path.is_file() {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Some(mut candidate) = Self::parse_candidate_json(&content) {
                        if candidate.id.is_empty() {
                            candidate.id = dir_name.to_string();
                        }
                        if candidate.dimensions == (0, 0) {
                            if let Some(dims) = candidate.parsed_dimensions {
                                candidate.dimensions = dims;
                            }
                        }
                        if candidate.layers.is_empty() {
                            candidate.layers = candidate.resolved_layers();
                        }
                        if candidate.filename.is_empty() {
                            let first_file = candidate
                                .layers
                                .first()
                                .map(|l| l.filename.as_str())
                                .unwrap_or("0.png");
                            candidate.filename = format!("{}/{first_file}", candidate.id);
                        }
                        known_ids.insert(candidate.id.clone());
                        candidates.push(candidate);
                    }
                }
            }
        }

        // Pass 2: Discover raw drop-in image files without sidecars and convert them into 1-layer bundles
        for base_dir in &search_dirs {
            let entries: Vec<PathBuf> = match std::fs::read_dir(base_dir) {
                Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
                Err(_) => continue,
            };
            for path in entries {
                if !path.is_file() {
                    continue;
                }
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if file_name.starts_with('.') {
                    continue;
                }
                let ext = match path.extension().and_then(|e| e.to_str()) {
                    Some(e) if Self::is_image_ext(e) => e.to_lowercase(),
                    _ => continue,
                };
                let stem = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                if known_ids.contains(&stem) {
                    continue;
                }

                let dimensions = image::image_dimensions(&path).unwrap_or((0, 0));
                let created_at = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(DateTime::<Utc>::from)
                    .unwrap_or_else(Utc::now);

                let rel_filename = path
                    .strip_prefix(&staging)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| file_name.to_string());

                let layer_filename = format!("0.{ext}");
                let bundle_dir = base_dir.join(&stem);
                std::fs::create_dir_all(&bundle_dir)?;
                let target_img = bundle_dir.join(&layer_filename);
                Self::atomic_move(&path, &target_img)?;

                let candidate = Candidate::new(
                    stem.clone(),
                    Source::Manual,
                    rel_filename,
                    dimensions,
                    created_at,
                )
                .with_layers(vec![Layer {
                    index: 0,
                    filename: layer_filename,
                    role: LayerRole::Ground,
                    elevation: Some(0),
                    caption: None,
                    source_url: None,
                    dimensions,
                }]);

                if let Ok(manifest_json) = serde_json::to_string_pretty(&candidate) {
                    let _ = Self::atomic_write(
                        &bundle_dir.join("manifest.json"),
                        manifest_json.as_bytes(),
                    );
                }

                known_ids.insert(stem);
                candidates.push(candidate);
            }
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

        let mut deleted_any = false;

        if let Some(bundle_dir) = self.find_staging_bundle_dir(id) {
            if std::fs::remove_dir_all(&bundle_dir).is_ok() {
                deleted_any = true;
            }
        }

        let sidecar_path = self.find_staging_sidecar_path(id);
        let candidate: Option<Candidate> = sidecar_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|c| Self::parse_candidate_json(&c));

        if let Some(image_path) = self.find_staging_image_path(id, candidate.as_ref()) {
            if std::fs::remove_file(image_path).is_ok() {
                deleted_any = true;
            }
        }

        if let Some(p) = &sidecar_path {
            if p.is_file() && std::fs::remove_file(p).is_ok() {
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
        map.validate_tags().map_err(StorageError::InvalidData)?;
        map.validate_layers().map_err(StorageError::InvalidData)?;

        let bundle_dir = self.curated_dir().join(&map.id);
        std::fs::create_dir_all(&bundle_dir)?;

        let mut stored_map = map.clone();
        if stored_map.layers.is_empty() {
            let filename = Layer::format_filename(0, LayerRole::Ground, Some(0), "png");
            stored_map.layers = vec![Layer::new(
                0,
                filename,
                LayerRole::Ground,
                Some(0),
                stored_map.dimensions,
            )];
        }

        let first_filename = stored_map.layers[0].filename.clone();
        Self::atomic_write(&bundle_dir.join(&first_filename), data)?;

        let manifest_json = serde_json::to_string_pretty(&stored_map)?;
        Self::atomic_write(&bundle_dir.join("manifest.json"), manifest_json.as_bytes())?;

        let caption = CaptionGenerator::generate_for_curated(&stored_map, None);
        self.write_lora_companions(&map.id, data, &caption, &bundle_dir);
        Ok(())
    }

    fn get_curated_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError> {
        self.get_curated_layer_image(theme, id, 0)
    }

    fn get_curated_layer_image(
        &self,
        theme: &Theme,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let image_path = self
            .find_curated_layer_image_path(theme, id, layer_index)
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Curated image layer {layer_index} for '{id}' not found"
                ))
            })?;

        let bytes = std::fs::read(image_path)?;
        Ok(bytes)
    }
    fn get_curated_lora_image(&self, theme: &Theme, id: &str) -> Result<Vec<u8>, StorageError> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            let bundle_png = bundle_dir.join(format!("{id}_lora1024.png"));
            if bundle_png.is_file() {
                return std::fs::read(bundle_png).map_err(StorageError::Io);
            }
        }
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_png = theme_dir.join(id).join(format!("{id}_lora1024.png"));
        if bundle_png.is_file() {
            return std::fs::read(bundle_png).map_err(StorageError::Io);
        }
        let theme_png = theme_dir.join(format!("{id}_lora1024.png"));
        if theme_png.is_file() {
            return std::fs::read(theme_png).map_err(StorageError::Io);
        }
        Err(StorageError::NotFound(format!(
            "Curated LoRA image for candidate '{id}' not found"
        )))
    }

    fn get_curated_lora_caption(&self, theme: &Theme, id: &str) -> Result<String, StorageError> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            let bundle_txt = bundle_dir.join(format!("{id}_lora1024.txt"));
            if bundle_txt.is_file() {
                return std::fs::read_to_string(bundle_txt).map_err(StorageError::Io);
            }
        }
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_txt = theme_dir.join(id).join(format!("{id}_lora1024.txt"));
        if bundle_txt.is_file() {
            return std::fs::read_to_string(bundle_txt).map_err(StorageError::Io);
        }
        let theme_txt = theme_dir.join(format!("{id}_lora1024.txt"));
        if theme_txt.is_file() {
            return std::fs::read_to_string(theme_txt).map_err(StorageError::Io);
        }
        Err(StorageError::NotFound(format!(
            "Curated LoRA caption for candidate '{id}' not found"
        )))
    }

    fn is_lora_normalized(&self, theme: &Theme, id: &str) -> Result<bool, StorageError> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            let bundle_png = bundle_dir.join(format!("{id}_lora1024.png"));
            if bundle_png.is_file() {
                return Ok(true);
            }
        }
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_png = theme_dir.join(id).join(format!("{id}_lora1024.png"));
        let theme_png = theme_dir.join(format!("{id}_lora1024.png"));
        Ok(bundle_png.is_file() || theme_png.is_file())
    }

    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        let _ = self.migrate_legacy_storage();
        let mut results = Vec::new();
        let curated_dir = self.curated_dir();
        if !curated_dir.is_dir() {
            return Ok(results);
        }

        if let Ok(entries) = std::fs::read_dir(&curated_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    let manifest_path = path.join("manifest.json");
                    if manifest_path.is_file() {
                        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                            if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
                                let matches_theme = match theme {
                                    Some(t) => map.tags.iter().any(|m_theme| {
                                        m_theme.eq_ignore_ascii_case(t.as_str())
                                            || m_theme.parse::<Theme>().ok().as_ref() == Some(t)
                                    }),
                                    None => true,
                                };
                                if matches_theme {
                                    results.push(map);
                                }
                            }
                        }
                    }
                } else if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if ext == "json" && name != "manifest.json" {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
                                    let matches_theme = match theme {
                                        Some(t) => map.tags.iter().any(|m_theme| {
                                            m_theme.eq_ignore_ascii_case(t.as_str())
                                                || m_theme.parse::<Theme>().ok().as_ref() == Some(t)
                                        }),
                                        None => true,
                                    };
                                    if matches_theme {
                                        results.push(map);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        results.sort_by(|a, b| a.curated_at.cmp(&b.curated_at).then_with(|| a.id.cmp(&b.id)));
        Ok(results)
    }

    fn export_lora_dataset(&self, theme: Option<&Theme>) -> Result<Vec<u8>, StorageError> {
        let curated_maps = self.list_curated(theme)?;
        self.create_lora_zip(&curated_maps)
    }

    fn export_selected_lora_dataset(&self, ids: &[String]) -> Result<Vec<u8>, StorageError> {
        let all_maps = self.list_curated(None)?;
        let mut selected_maps = Vec::new();
        let mut seen = HashSet::new();

        for id in ids {
            if !seen.insert(id.as_str()) {
                continue;
            }
            if let Some(map) = all_maps.iter().find(|m| &m.id == id) {
                selected_maps.push(map.clone());
            } else if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
                let manifest_path = bundle_dir.join("manifest.json");
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
                        selected_maps.push(map);
                    }
                }
            }
        }

        self.create_lora_zip(&selected_maps)
    }

    fn get_curated_layer_image_by_id(
        &self,
        id: &str,
        layer_index: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let path = self
            .find_curated_layer_image_by_id_path(id, layer_index)
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Curated image layer {layer_index} for '{id}' not found"
                ))
            })?;
        std::fs::read(path).map_err(StorageError::Io)
    }

    fn delete_curated(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut deleted = false;
        if let Some(dir) = self.find_curated_bundle_dir(id) {
            std::fs::remove_dir_all(&dir)?;
            deleted = true;
        }

        // Also check and clean up any loose legacy files across curated and theme directories
        for ext in &["png", "jpg", "jpeg", "webp", "json", "txt"] {
            let flat_file = self.curated_dir().join(format!("{id}.{ext}"));
            if flat_file.is_file() {
                let _ = std::fs::remove_file(flat_file);
                deleted = true;
            }
            let lora_png = self.curated_dir().join(format!("{id}_lora1024.png"));
            if lora_png.is_file() {
                let _ = std::fs::remove_file(lora_png);
                deleted = true;
            }
            let lora_txt = self.curated_dir().join(format!("{id}_lora1024.txt"));
            if lora_txt.is_file() {
                let _ = std::fs::remove_file(lora_txt);
                deleted = true;
            }
            for theme in Theme::ALL {
                let theme_dir = self.curated_theme_dir(theme);
                let theme_file = theme_dir.join(format!("{id}.{ext}"));
                if theme_file.is_file() {
                    let _ = std::fs::remove_file(theme_file);
                    deleted = true;
                }
                let lora_theme_png = theme_dir.join(format!("{id}_lora1024.png"));
                if lora_theme_png.is_file() {
                    let _ = std::fs::remove_file(lora_theme_png);
                    deleted = true;
                }
                let lora_theme_txt = theme_dir.join(format!("{id}_lora1024.txt"));
                if lora_theme_txt.is_file() {
                    let _ = std::fs::remove_file(lora_theme_txt);
                    deleted = true;
                }
            }
        }

        if !deleted {
            return Err(StorageError::NotFound(format!("Curated map '{id}' not found")));
        }

        self.log_rejection(id, reason, notes)?;
        Ok(())
    }

    fn update_curated(
        &self,
        id: &str,
        update: CuratedMapUpdate,
    ) -> Result<CuratedMap, StorageError> {
        let bundle_dir = self.find_curated_bundle_dir(id).ok_or_else(|| {
            StorageError::NotFound(format!("Curated map '{id}' not found"))
        })?;
        let manifest_path = bundle_dir.join("manifest.json");
        if !manifest_path.is_file() {
            return Err(StorageError::NotFound(format!(
                "Curated manifest for '{id}' not found"
            )));
        }
        let content = std::fs::read_to_string(&manifest_path)?;
        let mut map: CuratedMap = serde_json::from_str(&content)?;

        let old_ground = map
            .layers
            .iter()
            .find(|l| l.role == LayerRole::Ground)
            .or_else(|| map.layers.first())
            .cloned();
        let old_ground_bytes = old_ground
            .as_ref()
            .and_then(|l| std::fs::read(bundle_dir.join(&l.filename)).ok());
        let old_description = map.description.clone();
        let mut tags_updated = false;

        if let Some(tags) = update.tags {
            map.tags = normalize_tags(tags);
            tags_updated = true;
        }

        // Legacy `themes` payloads fold into the single `tags` list. An empty or
        // whitespace-only array is a no-op so tags-only clients can still PATCH.
        if let Some(themes) = update.themes {
            let legacy_tags = normalize_tags(themes);
            if !legacy_tags.is_empty() {
                map.tags = normalize_tags(map.tags.into_iter().chain(legacy_tags));
                tags_updated = true;
            }
        }

        if tags_updated {
            map.validate_tags().map_err(StorageError::InvalidData)?;
        }

        if let Some(description) = update.description {
            let trimmed = description.trim();
            map.description = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        if let Some(new_layers) = update.layers {
            validate_layers_for_curation(&new_layers)
                .map_err(StorageError::InvalidData)?;

            let mut effective_layers: Vec<Layer> = if new_layers.iter().any(|l| l.role.is_excluded()) {
                new_layers
                    .into_iter()
                    .filter(|l| l.role.is_included())
                    .collect()
            } else {
                new_layers
            };

            // PATCH payloads may omit provenance fields; carry them over from the
            // existing layer at the same index (falling back to the same filename).
            for layer in &mut effective_layers {
                let existing = map
                    .layers
                    .iter()
                    .find(|l| l.index == layer.index)
                    .or_else(|| map.layers.iter().find(|l| l.filename == layer.filename));
                let Some(existing) = existing else { continue };
                if layer.source_url.is_none() {
                    layer.source_url = existing.source_url.clone();
                }
                if layer.caption.is_none() {
                    layer.caption = existing.caption.clone();
                }
            }

            let mut temp_map = map.clone();
            temp_map.layers = effective_layers.clone();
            temp_map
                .validate_layers()
                .map_err(StorageError::InvalidData)?;

            // Resolve source file for each layer in effective_layers
            let mut layer_sources = Vec::new();
            for layer in &effective_layers {
                let mut src_path = bundle_dir.join(&layer.filename);
                if !src_path.is_file() {
                    if let Some(old_l) = map.layers.iter().find(|l| l.index == layer.index) {
                        let p = bundle_dir.join(&old_l.filename);
                        if p.is_file() {
                            src_path = p;
                        }
                    }
                }
                if !src_path.is_file() {
                    if let Some(old_l) = map.layers.get(layer.index) {
                        let p = bundle_dir.join(&old_l.filename);
                        if p.is_file() {
                            src_path = p;
                        }
                    }
                }
                if !src_path.is_file() {
                    let prefix = format!("{}_", layer.index);
                    if let Ok(entries) = std::fs::read_dir(&bundle_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_file() {
                                let name = p
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or_default();
                                if name.starts_with(&prefix) && !name.contains("lora1024") {
                                    src_path = p;
                                    break;
                                }
                            }
                        }
                    }
                }
                if !src_path.is_file() {
                    if let Some(old_l) = map.layers.iter().find(|l| l.role == layer.role) {
                        let p = bundle_dir.join(&old_l.filename);
                        if p.is_file() {
                            src_path = p;
                        }
                    }
                }
                if !src_path.is_file() && effective_layers.len() == 1 && map.layers.len() == 1 {
                    let p = bundle_dir.join(&map.layers[0].filename);
                    if p.is_file() {
                        src_path = p;
                    }
                }
                if !src_path.is_file() {
                    return Err(StorageError::NotFound(format!(
                        "Layer image source file not found for layer {:?}",
                        layer
                    )));
                }

                let bytes = std::fs::read(&src_path)?;
                let ext = src_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let dims = if layer.dimensions != (0, 0) {
                    layer.dimensions
                } else {
                    image::image_dimensions(&src_path).unwrap_or((0, 0))
                };
                layer_sources.push((layer.clone(), bytes, ext.to_string(), dims));
            }

            // Deterministically format filenames and stage files
            let mut updated_layers = Vec::new();
            for (new_idx, (mut layer, bytes, ext, dims)) in layer_sources.into_iter().enumerate() {
                let dest_filename = Layer::format_filename(
                    new_idx,
                    layer.role,
                    layer.elevation,
                    &ext,
                );
                layer.index = new_idx;
                layer.filename = dest_filename;
                layer.dimensions = dims;

                let tmp_file = bundle_dir.join(format!("__tmp_layer_{new_idx}.tmp"));
                std::fs::write(&tmp_file, &bytes)?;
                updated_layers.push(layer);
            }

            // Clean up old layer files in bundle_dir
            for old_l in &map.layers {
                let old_p = bundle_dir.join(&old_l.filename);
                if old_p.is_file() {
                    let _ = std::fs::remove_file(old_p);
                }
            }
            if let Ok(entries) = std::fs::read_dir(&bundle_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let name = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default();
                        if name != "manifest.json"
                            && !name.contains("lora1024")
                            && !name.starts_with("__tmp_layer_")
                        {
                            let _ = std::fs::remove_file(p);
                        }
                    }
                }
            }

            // Atomically move temporary layer files into their destination filenames
            for (new_idx, layer) in updated_layers.iter().enumerate() {
                let tmp_file = bundle_dir.join(format!("__tmp_layer_{new_idx}.tmp"));
                let dest_file = bundle_dir.join(&layer.filename);
                Self::atomic_move(&tmp_file, &dest_file)?;
            }

            if map.dimensions == (0, 0) {
                map.dimensions = updated_layers
                    .first()
                    .map(|l| l.dimensions)
                    .unwrap_or((0, 0));
            }

            map.layers = updated_layers;
        }

        // Check if ground layer or description changed
        let new_ground = map
            .layers
            .iter()
            .find(|l| l.role == LayerRole::Ground)
            .or_else(|| map.layers.first())
            .cloned();
        let new_ground_bytes = new_ground
            .as_ref()
            .and_then(|l| std::fs::read(bundle_dir.join(&l.filename)).ok());

        let ground_layer_image_changed = old_ground_bytes != new_ground_bytes;
        let ground_layer_changed = ground_layer_image_changed
            || (old_ground.as_ref().map(|l| (&l.role, l.elevation))
                != new_ground.as_ref().map(|l| (&l.role, l.elevation)));
        let description_changed = old_description != map.description;

        if ground_layer_changed || description_changed {
            let caption = CaptionGenerator::generate_for_curated(&map, None);
            if ground_layer_image_changed {
                if let Some(primary_bytes) = &new_ground_bytes {
                    self.write_lora_companions(&map.id, primary_bytes, &caption, &bundle_dir);
                }
            } else {
                let bundle_txt = bundle_dir.join(format!("{}_lora1024.txt", map.id));
                let _ = Self::atomic_write(&bundle_txt, caption.as_bytes());
            }

            if description_changed {
                let primary_idx = map
                    .layers
                    .iter()
                    .position(|l| l.role == LayerRole::Ground)
                    .unwrap_or(0);
                if let Some(first) = map.layers.get_mut(primary_idx) {
                    first.caption = map.description.clone();
                }
            }
        } else if tags_updated {
            let caption = CaptionGenerator::generate_for_curated(&map, None);
            let bundle_txt = bundle_dir.join(format!("{}_lora1024.txt", map.id));
            let _ = Self::atomic_write(&bundle_txt, caption.as_bytes());
        }


        let curated_json = serde_json::to_string_pretty(&map)?;
        Self::atomic_write(&manifest_path, curated_json.as_bytes())?;

        Ok(map)
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
        if records.iter().any(|r| r.id == id) {
            return Ok(true);
        }
        if let Some((base_id, suffix)) = id.rsplit_once('_') {
            if suffix.chars().all(|c| c.is_ascii_digit())
                && records.iter().any(|r| r.id == base_id)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn is_in_staging(&self, id: &str) -> Result<bool, StorageError> {
        if self.find_staging_bundle_dir(id).is_some() {
            return Ok(true);
        }
        if self.find_staging_sidecar_path(id).is_some() {
            return Ok(true);
        }
        if self.find_staging_image_path(id, None).is_some() {
            return Ok(true);
        }
        Ok(false)
    }

    fn is_curated(&self, id: &str) -> Result<bool, StorageError> {
        if let Some(bundle_dir) = self.find_curated_bundle_dir(id) {
            if bundle_dir.join("manifest.json").is_file() {
                return Ok(true);
            }
        }
        for theme in Theme::ALL {
            let theme_dir = self.curated_theme_dir(theme);
            let bundle_manifest = theme_dir.join(id).join("manifest.json");
            if bundle_manifest.is_file() {
                return Ok(true);
            }
            let sidecar = theme_dir.join(format!("{id}.json"));
            if sidecar.is_file() {
                return Ok(true);
            }
            if self.find_curated_image_path(theme, id).is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn is_duplicate(&self, id: &str) -> Result<bool, StorageError> {
        if self.is_rejected(id)? {
            return Ok(true);
        }
        if self.is_in_staging(id)? {
            return Ok(true);
        }
        if self.is_curated(id)? {
            return Ok(true);
        }
        if let Some((base_id, suffix)) = id.rsplit_once('_') {
            if suffix.chars().all(|c| c.is_ascii_digit())
                && (self.is_rejected(base_id)?
                    || self.is_in_staging(base_id)?
                    || self.is_curated(base_id)?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn curate_staging(&self, map: &CuratedMap) -> Result<CuratedMap, StorageError> {
        map.validate_tags().map_err(StorageError::InvalidData)?;

        if let Some(bundle_dir) = self.find_staging_bundle_dir(&map.id) {
            let manifest_path = bundle_dir.join("manifest.json");
            let content = std::fs::read_to_string(&manifest_path)?;
            let candidate = Self::parse_candidate_json(&content).ok_or_else(|| {
                StorageError::InvalidData(format!(
                    "Invalid manifest.json for candidate '{}'",
                    map.id
                ))
            })?;

            let candidate_layers = candidate.resolved_layers();

            // Determine effective layer roles/elevations (either overridden on `map.layers` or from `candidate`)
            let effective_layers: Vec<Layer> = if !map.layers.is_empty() {
                map.layers
                    .iter()
                    .map(|cl| {
                        let src_layer = candidate_layers
                            .iter()
                            .find(|l| l.index == cl.index)
                            .or_else(|| candidate_layers.get(cl.index));
                        let src_filename = src_layer
                            .map(|l| l.filename.clone())
                            .unwrap_or_else(|| format!("{}.png", cl.index));
                        let dims = if cl.dimensions != (0, 0) {
                            cl.dimensions
                        } else {
                            src_layer.map(|l| l.dimensions).unwrap_or(map.dimensions)
                        };
                        let caption = cl
                            .caption
                            .clone()
                            .or_else(|| src_layer.and_then(|l| l.caption.clone()));
                        Layer {
                            index: cl.index,
                            filename: src_filename,
                            role: cl.role,
                            elevation: cl.elevation,
                            caption,
                            source_url: cl
                                .source_url
                                .clone()
                                .or_else(|| src_layer.and_then(|l| l.source_url.clone())),
                            dimensions: dims,
                        }
                    })
                    .collect()
            } else {
                candidate_layers
            };

            crate::domain::validate_layers_for_curation(&effective_layers)
                .map_err(StorageError::InvalidData)?;

            let curated_bundle_dir = self.curated_dir().join(&map.id);
            std::fs::create_dir_all(&curated_bundle_dir)?;

            let mut curated_layers = Vec::new();
            for src_layer in effective_layers.iter().filter(|l| l.role.is_included()) {
                let src_img_path = bundle_dir.join(&src_layer.filename);
                if !src_img_path.is_file() {
                    return Err(StorageError::NotFound(format!(
                        "Staging layer image '{}' for candidate '{}' not found",
                        src_layer.filename, map.id
                    )));
                }
                let curated_idx = curated_layers.len();
                let ext = src_img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let dest_filename = Layer::format_filename(
                    curated_idx,
                    src_layer.role,
                    src_layer.elevation,
                    ext,
                );
                let dest_img_path = curated_bundle_dir.join(&dest_filename);
                Self::atomic_move(&src_img_path, &dest_img_path)?;

                let mut layer = Layer::new(
                    curated_idx,
                    dest_filename,
                    src_layer.role,
                    src_layer.elevation,
                    if src_layer.dimensions != (0, 0) {
                        src_layer.dimensions
                    } else {
                        map.dimensions
                    },
                );
                if let Some(c) = &src_layer.caption {
                    layer = layer.with_caption(c.clone());
                }
                if let Some(source_url) = &src_layer.source_url {
                    layer = layer.with_source_url(source_url.clone());
                }
                curated_layers.push(layer);
            }

            let mut stored_map = map.clone();
            if stored_map.dimensions == (0, 0) {
                stored_map.dimensions = curated_layers
                    .first()
                    .map(|l| l.dimensions)
                    .unwrap_or(candidate.dimensions);
            }
            if let Some(desc) = &stored_map.description {
                let primary_idx = curated_layers
                    .iter()
                    .position(|l| l.role == LayerRole::Ground)
                    .unwrap_or(0);
                if let Some(layer) = curated_layers.get_mut(primary_idx) {
                    if layer.caption.is_none() {
                        layer.caption = Some(desc.clone());
                    }
                }
            }
            stored_map.layers = curated_layers;
            stored_map
                .validate_layers()
                .map_err(StorageError::InvalidData)?;
            let curated_json = serde_json::to_string_pretty(&stored_map)?;
            Self::atomic_write(
                &curated_bundle_dir.join("manifest.json"),
                curated_json.as_bytes(),
            )?;
            let primary_layer = stored_map
                .layers
                .iter()
                .find(|l| l.role == LayerRole::Ground)
                .or_else(|| stored_map.layers.first());

            if let Some(layer) = primary_layer {
                let primary_path = curated_bundle_dir.join(&layer.filename);
                if let Ok(primary_bytes) = std::fs::read(&primary_path) {
                    let caption = CaptionGenerator::generate_for_curated(
                        &stored_map,
                        Some(&candidate),
                    );
                    self.write_lora_companions(
                        &map.id,
                        &primary_bytes,
                        &caption,
                        &curated_bundle_dir,
                    );
                }
            }

            let _ = std::fs::remove_dir_all(&bundle_dir);
            return Ok(stored_map);
        }

        // Fallback for raw drop-in image or unmigrated flat file in staging
        let staging_sidecar = self.find_staging_sidecar_path(&map.id);
        let candidate: Option<Candidate> = if let Some(sc) = &staging_sidecar {
            let sidecar_content = std::fs::read_to_string(sc)?;
            Self::parse_candidate_json(&sidecar_content)
        } else {
            None
        };

        if let Some(cand) = &candidate {
            cand.validate_for_curation()
                .map_err(StorageError::InvalidData)?;
        }

        let staging_image = if let Some(cand) = &candidate {
            self.find_staging_image_path(&map.id, Some(cand))
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

        let curated_bundle_dir = self.curated_dir().join(&map.id);
        std::fs::create_dir_all(&curated_bundle_dir)?;

        let dest_filename = Layer::format_filename(0, LayerRole::Ground, Some(0), ext);
        let target_image = curated_bundle_dir.join(&dest_filename);
        let target_manifest = curated_bundle_dir.join("manifest.json");

        Self::atomic_move(&staging_image, &target_image)?;

        // Only one image is physically moved for a flat/sidecar-only candidate, so the
        // persisted manifest must describe exactly that file rather than the caller's
        // (possibly multi-layer) assignment list.
        let mut stored_map = map.clone();
        let mut layer = stored_map
            .layers
            .iter()
            .find(|l| l.role.is_ground())
            .cloned()
            .unwrap_or_else(|| {
                Layer::new(
                    0,
                    &dest_filename,
                    LayerRole::Ground,
                    Some(0),
                    stored_map.dimensions,
                )
            });
        layer.index = 0;
        layer.filename = dest_filename;
        layer.role = LayerRole::Ground;
        layer.elevation = Some(0);
        if layer.dimensions == (0, 0) {
            layer.dimensions = stored_map.dimensions;
        }
        if layer.caption.is_none() {
            layer.caption = stored_map.description.clone();
        }
        stored_map.layers = vec![layer];
        let curated_json = serde_json::to_string_pretty(&stored_map)?;
        Self::atomic_write(&target_manifest, curated_json.as_bytes())?;
        if let Ok(primary_bytes) = std::fs::read(&target_image) {
            let caption = CaptionGenerator::generate_for_curated(
                &stored_map,
                candidate.as_ref(),
            );
            self.write_lora_companions(
                &map.id,
                &primary_bytes,
                &caption,
                &curated_bundle_dir,
            );
        }

        if let Some(sc) = &staging_sidecar {
            if sc.is_file() {
                let _ = std::fs::remove_file(sc);
            }
        }

        Ok(stored_map)
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

    fn update_staging_layers(
        &self,
        id: &str,
        layers: Vec<Layer>,
    ) -> Result<Candidate, StorageError> {
        let bundle_dir = self.find_staging_bundle_dir(id).ok_or_else(|| {
            StorageError::NotFound(format!("Candidate bundle '{id}' not found in staging"))
        })?;
        let manifest_path = bundle_dir.join("manifest.json");
        let content = std::fs::read_to_string(&manifest_path)?;
        let mut candidate = Self::parse_candidate_json(&content).ok_or_else(|| {
            StorageError::InvalidData(format!("Invalid manifest.json for '{id}'"))
        })?;

        if layers.is_empty() {
            return Err(StorageError::InvalidData(
                "Candidate layers cannot be empty".into(),
            ));
        }

        candidate.layers = layers;
        let manifest_json = serde_json::to_string_pretty(&candidate)?;
        Self::atomic_write(&manifest_path, manifest_json.as_bytes())?;

        Ok(candidate)
    }

    fn split_staging_bundle(&self, id: &str) -> Result<Vec<Candidate>, StorageError> {
        let bundle_dir = self.find_staging_bundle_dir(id).ok_or_else(|| {
            StorageError::NotFound(format!("Candidate bundle '{id}' not found in staging"))
        })?;
        let parent_dir = bundle_dir
            .parent()
            .ok_or_else(|| StorageError::InvalidData("Invalid bundle directory parent".into()))?
            .to_path_buf();

        let manifest_path = bundle_dir.join("manifest.json");
        let content = std::fs::read_to_string(&manifest_path)?;
        let candidate = Self::parse_candidate_json(&content).ok_or_else(|| {
            StorageError::InvalidData(format!("Invalid manifest.json for '{id}'"))
        })?;

        let layers = candidate.resolved_layers();
        if layers.len() <= 1 {
            return Err(StorageError::InvalidData(format!(
                "Cannot split single-layer candidate '{id}'"
            )));
        }

        let mut split_candidates = Vec::with_capacity(layers.len());
        for (idx, layer) in layers.iter().enumerate() {
            let child_id = format!("{id}_{idx}");
            let child_dir = parent_dir.join(&child_id);
            std::fs::create_dir_all(&child_dir)?;

            let src_img = bundle_dir.join(&layer.filename);
            if !src_img.is_file() {
                return Err(StorageError::NotFound(format!(
                    "Layer image '{}' not found in bundle '{id}'",
                    layer.filename
                )));
            }
            let ext = src_img
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let child_img_filename = format!("0.{ext}");
            let dest_img = child_dir.join(&child_img_filename);
            Self::atomic_move(&src_img, &dest_img)?;

            let dims = if layer.dimensions != (0, 0) {
                layer.dimensions
            } else {
                candidate.dimensions
            };

            let child_layer = Layer {
                index: 0,
                filename: child_img_filename.clone(),
                role: LayerRole::Ground,
                elevation: Some(0),
                caption: layer.caption.clone(),
                source_url: layer
                    .source_url
                    .clone()
                    .or_else(|| candidate.source_url.clone()),
                dimensions: dims,
            };

            let child_cand = Candidate {
                id: child_id.clone(),
                source: candidate.source,
                filename: format!("{child_id}/{child_img_filename}"),
                dimensions: dims,
                created_at: candidate.created_at,
                source_url: child_layer.source_url.clone(),
                title: candidate.title.clone(),
                author: candidate.author.clone(),
                parsed_gridless: candidate.parsed_gridless,
                parsed_dimensions: candidate.parsed_dimensions,
                layers: vec![child_layer],
                parent_group_id: Some(id.to_string()),
            };

            let child_json = serde_json::to_string_pretty(&child_cand)?;
            Self::atomic_write(&child_dir.join("manifest.json"), child_json.as_bytes())?;
            split_candidates.push(child_cand);
        }

        let _ = std::fs::remove_dir_all(&bundle_dir);
        Ok(split_candidates)
    }

    fn regroup_staging_bundle(&self, parent_id: &str) -> Result<Candidate, StorageError> {
        let all_staged = self.list_staging()?;
        let prefix = format!("{parent_id}_");

        let mut matching: Vec<(usize, PathBuf, Candidate)> = Vec::new();
        for cand in all_staged {
            let matches_parent = cand.parent_group_id.as_deref() == Some(parent_id)
                || cand
                    .id
                    .strip_prefix(&prefix)
                    .map(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false);
            if matches_parent {
                let suffix_idx = cand
                    .id
                    .strip_prefix(&prefix)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(matching.len());
                if let Some(bundle_dir) = self.find_staging_bundle_dir(&cand.id) {
                    matching.push((suffix_idx, bundle_dir, cand));
                }
            }
        }

        if matching.is_empty() {
            return Err(StorageError::NotFound(format!(
                "No split staging candidates found for parent bundle '{parent_id}'"
            )));
        }

        matching.sort_by_key(|(idx, _, _)| *idx);

        let target_parent_dir = matching[0]
            .1
            .parent()
            .ok_or_else(|| StorageError::InvalidData("Invalid child bundle parent dir".into()))?
            .to_path_buf();
        let regrouped_dir = target_parent_dir.join(parent_id);
        std::fs::create_dir_all(&regrouped_dir)?;

        let captions: Vec<Option<String>> = matching
            .iter()
            .map(|(_, _, c)| c.layers.first().and_then(|l| l.caption.clone()))
            .collect();
        let title_opt = matching[0].2.title.as_deref();
        let inferred_roles = infer_bundle_layer_roles(&captions, title_opt);

        let mut regrouped_layers = Vec::with_capacity(matching.len());
        for (new_idx, ((_, child_dir, child_cand), (role, elevation))) in
            matching.iter().zip(inferred_roles).enumerate()
        {
            let child_layers = child_cand.resolved_layers();
            let first_layer = &child_layers[0];
            let src_img = child_dir.join(&first_layer.filename);
            if !src_img.is_file() {
                return Err(StorageError::NotFound(format!(
                    "Child image '{}' not found in '{}'",
                    first_layer.filename, child_cand.id
                )));
            }
            let ext = src_img
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let layer_filename = format!("{new_idx}.{ext}");
            let dest_img = regrouped_dir.join(&layer_filename);
            Self::atomic_move(&src_img, &dest_img)?;
            let _ = std::fs::remove_dir_all(child_dir);

            regrouped_layers.push(Layer {
                index: new_idx,
                filename: layer_filename,
                role,
                elevation,
                caption: first_layer.caption.clone(),
                source_url: first_layer
                    .source_url
                    .clone()
                    .or_else(|| child_cand.source_url.clone()),
                dimensions: first_layer.dimensions,
            });
        }

        let first_cand = &matching[0].2;
        let first_file = regrouped_layers
            .first()
            .map(|l| l.filename.clone())
            .unwrap_or_else(|| "0.png".to_string());
        let first_dims = regrouped_layers
            .first()
            .map(|l| l.dimensions)
            .unwrap_or(first_cand.dimensions);

        let regrouped = Candidate {
            id: parent_id.to_string(),
            source: first_cand.source,
            filename: format!("{parent_id}/{first_file}"),
            dimensions: first_dims,
            created_at: first_cand.created_at,
            source_url: first_cand.source_url.clone(),
            title: first_cand.title.clone(),
            author: first_cand.author.clone(),
            parsed_gridless: first_cand.parsed_gridless,
            parsed_dimensions: first_cand.parsed_dimensions,
            layers: regrouped_layers,
            parent_group_id: None,
        };

        let manifest_json = serde_json::to_string_pretty(&regrouped)?;
        Self::atomic_write(&regrouped_dir.join("manifest.json"), manifest_json.as_bytes())?;

        Ok(regrouped)
    }

    fn get_storage_stats(&self) -> Result<StorageStats, StorageError> {
        let _ = self.migrate_legacy_storage();

        // 1. Staging Breakdown & Totals
        let mut staging_breakdown: HashMap<String, DirectoryStats> = HashMap::new();
        staging_breakdown.insert("root".to_string(), DirectoryStats::default());
        staging_breakdown.insert("reddit".to_string(), DirectoryStats::default());
        staging_breakdown.insert("generated".to_string(), DirectoryStats::default());

        let staging_dir = self.staging_dir();
        if staging_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&staging_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        if path.join("manifest.json").is_file() {
                            // Bundle directly in staging root
                            let bundle_stats = Self::scan_dir_stats(&path);
                            let root_stats = staging_breakdown.entry("root".to_string()).or_default();
                            root_stats.bytes += bundle_stats.bytes;
                            root_stats.file_count += bundle_stats.file_count;
                        } else {
                            // Subdirectory (e.g. "reddit", "generated", etc.)
                            let sub_stats = Self::scan_dir_stats(&path);
                            staging_breakdown.insert(name.to_string(), sub_stats);
                        }
                    } else if path.is_file() {
                        let root_stats = staging_breakdown.entry("root".to_string()).or_default();
                        root_stats.file_count += 1;
                        if let Ok(meta) = entry.metadata() {
                            root_stats.bytes += meta.len();
                        }
                    }
                }
            }
        }

        let staging_bytes: u64 = staging_breakdown.values().map(|s| s.bytes).sum();
        let staging_files: usize = staging_breakdown.values().map(|s| s.file_count).sum();
        let staging = DirectoryStats::new(staging_bytes, staging_files);

        // 2. Curated By Theme & Curated Totals
        let mut curated_by_theme = HashMap::new();
        for theme in Theme::ALL {
            curated_by_theme.insert(
                *theme,
                ThemeStorageStats::new(*theme, 0, 0, 0),
            );
        }

        let curated_dir = self.curated_dir();
        let curated = Self::scan_dir_stats(&curated_dir);

        if let Ok(all_maps) = self.list_curated(None) {
            for map in all_maps {
                let map_dir = curated_dir.join(&map.id);
                let map_stats = if map_dir.is_dir() {
                    Self::scan_dir_stats(&map_dir)
                } else {
                    DirectoryStats::default()
                };
                for theme_str in &map.tags {
                    if let Ok(th) = theme_str.parse::<Theme>() {
                        if let Some(entry) = curated_by_theme.get_mut(&th) {
                            entry.map_count += 1;
                            entry.bytes += map_stats.bytes;
                            entry.file_count += map_stats.file_count;
                        }
                    }
                }
            }
        }

        // 3. Rejected Stats (rejections.json size and rejection records count)
        let rejections_file = self.rejections_file();
        let rejected = if rejections_file.is_file() {
            let bytes = std::fs::metadata(&rejections_file)
                .map(|m| m.len())
                .unwrap_or(0);
            let records = self.read_rejections().unwrap_or_default();
            DirectoryStats::new(bytes, records.len())
        } else {
            DirectoryStats::default()
        };

        // 4. Telemetry Stats
        let telemetry = Self::scan_dir_stats(&self.telemetry_dir());

        // 5. Total Footprint
        let total_bytes = staging.bytes + curated.bytes + rejected.bytes + telemetry.bytes;
        let total_files =
            staging.file_count + curated.file_count + rejected.file_count + telemetry.file_count;

        Ok(StorageStats {
            staging,
            staging_breakdown,
            curated,
            curated_by_theme,
            rejected,
            telemetry,
            total_bytes,
            total_files,
        })
    }

    fn purge_staging(&self) -> Result<PurgeStagingResult, StorageError> {
        let staging_dir = self.staging_dir();
        let staging_stats = Self::scan_dir_stats(&staging_dir);
        let purged_files = staging_stats.file_count;
        let reclaimed_bytes = staging_stats.bytes;

        if staging_dir.is_dir() {
            for entry in std::fs::read_dir(&staging_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                } else if path.is_file() {
                    std::fs::remove_file(&path)?;
                }
            }
        }

        std::fs::create_dir_all(&staging_dir)?;
        let remaining_staging_files = Self::scan_dir_stats(&staging_dir).file_count;

        Ok(PurgeStagingResult {
            purged_files,
            reclaimed_bytes,
            remaining_staging_files,
        })
    }
}

impl CurationStore for FilesystemStorage {
    fn get_staging_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        self.get_staging_candidate(id)
    }

    fn get_staging_layer_bytes(&self, id: &str, layer_index: usize) -> Result<Vec<u8>, StorageError> {
        self.get_staging_layer_image(id, layer_index)
    }

    fn commit_curation(&self, map: &CuratedMap) -> Result<CuratedMap, StorageError> {
        self.curate_staging(map)
    }

    fn reject_candidate(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        self.reject_staging(id, reason, notes)
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
        assert_eq!(dungeon_maps[0].tags, vec!["dungeon", "boss-room", "gridless"]);
        assert_eq!(dungeon_maps[0].theme(), Some(Theme::Dungeon));

        // Other theme list should be empty
        let cave_maps = storage.list_curated(Some(&Theme::Cave)).unwrap();
        assert_eq!(cave_maps.len(), 0);

        // List all curated
        let all_curated = storage.list_curated(None).unwrap();
        assert_eq!(all_curated.len(), 1);
    }

    #[test]
    fn test_curate_staging_preserves_description_and_lora_companions() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let candidate = Candidate::with_now(
            "map_desc_1",
            Source::Reddit,
            "map_desc_1.png",
            (1024, 1024),
        ).with_title("Ancient Crypt [50x50]");

        let image_bytes = b"\x89PNG\r\n\x1a\nfake-curated-png-bytes";
        storage.save_staging(&candidate, image_bytes).unwrap();

        let desc_text = "Ancient stone crypt with carved limestone sarcophagus and glowing braziers.";
        let curated = CuratedMap::with_now(
            "map_desc_1",
            Theme::Dungeon,
            vec!["boss-room".into()],
            (1024, 1024),
        ).with_description(desc_text);

        storage.curate_staging(&curated).unwrap();

        // Verify manifest.json contains description
        let manifest_path = dir.path().join("curated").join("map_desc_1").join("manifest.json");
        let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest_str.contains(desc_text));

        // Verify <id>_lora1024.txt companion exists and incorporates description
        let lora_txt_path = dir.path().join("curated").join("map_desc_1").join("map_desc_1_lora1024.txt");
        assert!(lora_txt_path.is_file());
        let lora_txt = std::fs::read_to_string(&lora_txt_path).unwrap();
        assert!(lora_txt.contains("limestone"));
        assert!(lora_txt.contains("sarcophagus"));

        // Verify list_curated preserves description and layer caption
        let list = storage.list_curated(None).unwrap();
        assert_eq!(list.len(), 1);
        let map = &list[0];
        assert_eq!(map.description.as_deref(), Some(desc_text));
        let ground_layer = map.layers.iter().find(|l| l.role == LayerRole::Ground).unwrap();
        assert_eq!(ground_layer.caption.as_deref(), Some(desc_text));

        // Verify pure read queries: list_curated does NOT invent description if absent
        let map_without_desc = CuratedMap::with_now(
            "map_without_desc",
            Theme::Dungeon,
            vec!["boss-room".into()],
            (1024, 1024),
        );
        let bundle_no_desc = dir.path().join("curated").join("map_without_desc");
        std::fs::create_dir_all(&bundle_no_desc).unwrap();
        std::fs::write(
            bundle_no_desc.join("manifest.json"),
            serde_json::to_string_pretty(&map_without_desc).unwrap(),
        ).unwrap();

        let list_no_desc = storage.list_curated(None).unwrap();
        let fetched = list_no_desc.iter().find(|m| m.id == "map_without_desc").unwrap();
        assert_eq!(fetched.description, None);
    }

    #[test]
    fn test_curation_store_trait_and_pure_reads() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());
        let store: &dyn CurationStore = &storage;

        // 1. Stage a multi-layer candidate bundle
        let mut candidate = Candidate::with_now(
            "castle_store_test",
            Source::Reddit,
            "castle_store_test/0.png",
            (1920, 1080),
        ).with_title("Castle Courtyard");
        candidate.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080)),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080)),
        ];
        let img0 = b"test-ground-layer-bytes";
        let img1 = b"test-upper-layer-bytes";
        storage.save_staging_bundle(None, &candidate, &[img0.to_vec(), img1.to_vec()]).unwrap();

        // 2. Test get_staging_candidate (O(1) direct lookup)
        let loaded = store.get_staging_candidate("castle_store_test").expect("direct candidate lookup");
        assert_eq!(loaded.id, "castle_store_test");
        assert_eq!(loaded.layers.len(), 2);
        assert_eq!(loaded.layers[0].role, LayerRole::Ground);
        assert_eq!(loaded.layers[1].role, LayerRole::UpperFloor);

        // Not found check
        assert!(matches!(
            store.get_staging_candidate("non_existent"),
            Err(StorageError::NotFound(_))
        ));

        // 3. Test get_staging_layer_bytes
        let bytes0 = store.get_staging_layer_bytes("castle_store_test", 0).expect("layer 0 bytes");
        assert_eq!(bytes0, img0);
        let bytes1 = store.get_staging_layer_bytes("castle_store_test", 1).expect("layer 1 bytes");
        assert_eq!(bytes1, img1);
        assert!(matches!(
            store.get_staging_layer_bytes("castle_store_test", 99),
            Err(StorageError::NotFound(_))
        ));

        // 4. Test commit_curation
        let curated = CuratedMap::with_now(
            "castle_store_test",
            Theme::Building,
            vec!["courtyard".into(), "medieval".into()],
            (1920, 1080),
        ).with_description("Castle courtyard with upper battlements");
        store.commit_curation(&curated).expect("commit curation");

        // Staging bundle must be cleaned up
        assert!(!dir.path().join("staging").join("castle_store_test").exists());

        // Curated directory must contain manifest.json and LoRA companions
        let curated_bundle = dir.path().join("curated").join("castle_store_test");
        assert!(curated_bundle.join("manifest.json").is_file());
        assert!(curated_bundle.join("castle_store_test_lora1024.txt").is_file());

        // 5. Test pure read queries: manifest without description is NOT mutated
        let pure_map = CuratedMap::with_now(
            "pure_read_map",
            Theme::Dungeon,
            vec!["crypt".into()],
            (800, 600),
        );
        let pure_bundle = dir.path().join("curated").join("pure_read_map");
        std::fs::create_dir_all(&pure_bundle).unwrap();
        let pure_manifest_path = pure_bundle.join("manifest.json");
        std::fs::write(
            &pure_manifest_path,
            serde_json::to_string_pretty(&pure_map).unwrap(),
        ).unwrap();

        let list_before = storage.list_curated(None).unwrap();
        let target = list_before.iter().find(|m| m.id == "pure_read_map").unwrap();
        assert_eq!(target.description, None);
        if let Some(first_layer) = target.layers.first() {
            assert_eq!(first_layer.caption, None);
        }

        let _ = storage.export_selected_lora_dataset(&["pure_read_map".into()]);

        let manifest_content_after = std::fs::read_to_string(&pure_manifest_path).unwrap();
        assert!(!manifest_content_after.contains("\"caption\":"));
        assert!(!manifest_content_after.contains("\"description\":"));

        // 6. Test reject_candidate
        let mut reject_cand = Candidate::with_now(
            "reject_store_test",
            Source::Generated,
            "reject_store_test/0.png",
            (1024, 1024),
        );
        reject_cand.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024)),
        ];
        storage.save_staging_bundle(None, &reject_cand, &[b"bad-map-content".to_vec()]).unwrap();
        assert!(dir.path().join("staging").join("reject_store_test").exists());

        store.reject_candidate("reject_store_test", RejectionReason::BlurryLowRes, Some("Artifacts present")).unwrap();
        assert!(!dir.path().join("staging").join("reject_store_test").exists());
        assert!(storage.is_rejected("reject_store_test").unwrap());
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
    fn test_curated_themes_validation() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let invalid_map_empty = CuratedMap::with_now(
            "invalid_empty",
            Vec::<String>::new(),
            vec![],
            (1024, 1024),
        );
        let err = storage.save_curated(&invalid_map_empty, b"test").unwrap_err();
        match err {
            StorageError::InvalidData(msg) => assert!(msg.contains("1 tag label")),
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

        // Sidecar manifest must now be persisted on disk
        let bundle_dir = staging.join("raw_map");
        assert!(bundle_dir.join("manifest.json").is_file() || sidecar_path.is_file());

        // get_staging_image returns the raw image bytes
        let retrieved = storage.get_staging_image("raw_map").unwrap();
        assert_eq!(retrieved, png_data);

        // curate_staging curates the raw image to Theme::Dungeon
        let curated = CuratedMap::with_now(
            "raw_map",
            Theme::Dungeon,
            vec!["dungeon-crawl".into()],
            (800, 600),
        );
        storage.curate_staging(&curated).unwrap();

        // Staging should now be empty
        assert_eq!(storage.list_staging().unwrap().len(), 0);
        assert!(storage.get_staging_image("raw_map").is_err());
        assert!(!image_path.exists());
        assert!(!sidecar_path.exists());
        assert!(!bundle_dir.exists());

        // Curated image and metadata exist in curated theme folder
        let curated_image = storage
            .get_curated_image(&Theme::Dungeon, "raw_map")
            .unwrap();
        assert_eq!(curated_image, png_data);
        let curated_maps = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
        assert_eq!(curated_maps.len(), 1);
        assert_eq!(curated_maps[0].id, "raw_map");
        assert_eq!(curated_maps[0].tags, vec!["dungeon", "dungeon-crawl"]);
        assert_eq!(curated_maps[0].theme(), Some(Theme::Dungeon));
    }

    #[tokio::test]
    async fn test_curate_flat_staging_image_via_service_rewrites_manifest_layer() {
        use crate::services::{CurationService, CurationWorkflow, DescriptionService};
        use std::sync::Arc;

        let dir = tempdir().unwrap();
        let storage = Arc::new(FilesystemStorage::new(dir.path()));
        let staging = storage.staging_dir();
        std::fs::create_dir_all(&staging).unwrap();

        // Flat drop-in image: no bundle directory and no sidecar manifest.
        let png_data = create_test_png(640, 480);
        std::fs::write(staging.join("flat_cand.png"), &png_data).unwrap();
        assert!(!staging.join("flat_cand.json").exists());
        assert!(!staging.join("flat_cand").exists());

        let service = CurationService::new(
            storage.clone(),
            Arc::new(DescriptionService::new(None)),
        );

        let stored = service
            .curate(
                "flat_cand",
                vec!["crypt".into()],
                None,
                Some("Damp stone crypt beneath a ruined keep.".into()),
            )
            .await
            .unwrap();

        assert_eq!(stored.layers.len(), 1);
        let layer = &stored.layers[0];
        assert_eq!(layer.index, 0);
        assert_eq!(layer.filename, "0_ground.png");
        assert_eq!(layer.role, LayerRole::Ground);
        assert_eq!(layer.elevation, Some(0));
        assert_eq!(layer.dimensions, (640, 480));
        assert_eq!(
            layer.caption.as_deref(),
            Some("Damp stone crypt beneath a ruined keep.")
        );

        let bundle_dir = storage.find_curated_bundle_dir("flat_cand").unwrap();
        assert!(bundle_dir.join("0_ground.png").is_file());
        assert_eq!(
            std::fs::read(bundle_dir.join("0_ground.png")).unwrap(),
            png_data
        );

        let manifest: CuratedMap = serde_json::from_str(
            &std::fs::read_to_string(bundle_dir.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.layers.len(), 1);
        assert_eq!(manifest.layers[0].filename, "0_ground.png");
        assert_eq!(manifest.layers[0].role, LayerRole::Ground);
        assert_eq!(manifest.layers[0].elevation, Some(0));
        assert_eq!(manifest.layers[0].dimensions, (640, 480));
    }

    #[test]
    fn test_update_curated_preserves_omitted_source_url_and_caption() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let png = create_test_png(512, 512);
        let candidate = Candidate::with_now(
            "map_upd_provenance",
            Source::Reddit,
            "0.png",
            (512, 512),
        )
        .with_source_url("https://reddit.example/post/abc");
        storage.save_staging(&candidate, &png).unwrap();

        let curated = CuratedMap::with_now(
            "map_upd_provenance",
            Theme::Dungeon,
            vec!["crypt".into()],
            (512, 512),
        )
        .with_description("Flooded crypt with shattered sarcophagi.");
        storage.curate_staging(&curated).unwrap();

        let stored = storage.list_curated(None).unwrap();
        let stored_map = stored
            .iter()
            .find(|m| m.id == "map_upd_provenance")
            .unwrap();
        assert_eq!(
            stored_map.layers[0].source_url.as_deref(),
            Some("https://reddit.example/post/abc")
        );

        // PATCH layer omits source_url and caption: both are carried over.
        let update = CuratedMapUpdate::default().with_layers(vec![Layer::new(
            0,
            "0_ground.png",
            LayerRole::Ground,
            Some(0),
            (512, 512),
        )]);
        let updated = storage
            .update_curated("map_upd_provenance", update)
            .unwrap();
        assert_eq!(
            updated.layers[0].source_url.as_deref(),
            Some("https://reddit.example/post/abc")
        );
        assert_eq!(
            updated.layers[0].caption.as_deref(),
            Some("Flooded crypt with shattered sarcophagi.")
        );

        // Explicit source_url overrides the stored provenance.
        let mut explicit = Layer::new(
            0,
            "0_ground.png",
            LayerRole::Ground,
            Some(0),
            (512, 512),
        )
        .with_source_url("https://example.com/new-source");
        explicit.caption = Some("Explicit caption.".into());
        let updated = storage
            .update_curated(
                "map_upd_provenance",
                CuratedMapUpdate::default().with_layers(vec![explicit]),
            )
            .unwrap();
        assert_eq!(
            updated.layers[0].source_url.as_deref(),
            Some("https://example.com/new-source")
        );
        assert_eq!(updated.layers[0].caption.as_deref(), Some("Explicit caption."));

        let manifest: CuratedMap = serde_json::from_str(
            &std::fs::read_to_string(
                storage
                    .find_curated_bundle_dir("map_upd_provenance")
                    .unwrap()
                    .join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest.layers[0].source_url.as_deref(),
            Some("https://example.com/new-source")
        );
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
            vec!["crystal-cave".into()],
            (500, 500),
        );
        storage.curate_staging(&curated).unwrap();

        assert!(!image_path.exists());
        let curated_image = storage.get_curated_image(&Theme::Cave, "direct_map").unwrap();
        assert_eq!(curated_image, png_data);
    }

    #[test]
    fn test_staging_reddit_subfolder_lifecycle() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let meta = RedditSidecarMetadata {
            id: "reddit_dungeon_1".to_string(),
            source: Source::Reddit,
            source_url: "https://reddit.com/r/battlemaps/comments/123/ | https://i.redd.it/123.png".to_string(),
            title: "[40x30] Forgotten Dungeon [No Grid]".to_string(),
            author: "dungeon_artist".to_string(),
            parsed_gridless: true,
            parsed_dimensions: Some((40, 30)),
            created_at: Utc::now(),
        };

        let png_bytes = create_test_png(800, 600);
        storage.save_staging_reddit(&meta, &png_bytes).unwrap();

        // Verify files exist in staging/reddit/ (in uniform bundle directory or legacy flat)
        let reddit_dir = storage.staging_dir().join("reddit");
        let bundle_dir = reddit_dir.join("reddit_dungeon_1");
        assert!(bundle_dir.join("0.png").is_file() || reddit_dir.join("reddit_dungeon_1.png").is_file());
        assert!(bundle_dir.join("manifest.json").is_file() || reddit_dir.join("reddit_dungeon_1.json").is_file());

        // Test is_in_staging
        assert!(storage.is_in_staging("reddit_dungeon_1").unwrap());
        assert!(storage.is_duplicate("reddit_dungeon_1").unwrap());
        assert!(!storage.is_curated("reddit_dungeon_1").unwrap());

        // Test get_staging_image
        let retrieved_bytes = storage.get_staging_image("reddit_dungeon_1").unwrap();
        assert_eq!(retrieved_bytes, png_bytes);

        // Test list_staging discovers subfolder candidate
        let candidates = storage.list_staging().unwrap();
        assert_eq!(candidates.len(), 1);
        let cand = &candidates[0];
        assert_eq!(cand.id, "reddit_dungeon_1");
        assert_eq!(cand.source, Source::Reddit);
        assert_eq!(cand.dimensions, (40, 30));
        assert_eq!(cand.parsed_gridless, Some(true));
        assert_eq!(cand.author.as_deref(), Some("dungeon_artist"));

        // Test curate_staging from staging/reddit/
        let curated = CuratedMap::with_now(
            "reddit_dungeon_1",
            Theme::Dungeon,
            vec!["gridless".into(), "40x30".into()],
            (40, 30),
        );
        storage.curate_staging(&curated).unwrap();

        // Verify staging files deleted
        assert!(!bundle_dir.exists());
        assert!(!reddit_dir.join("reddit_dungeon_1.png").exists());
        assert!(!reddit_dir.join("reddit_dungeon_1.json").exists());
        assert!(!storage.is_in_staging("reddit_dungeon_1").unwrap());

        // Verify curated storage has files
        assert!(storage.is_curated("reddit_dungeon_1").unwrap());
        let curated_image = storage.get_curated_image(&Theme::Dungeon, "reddit_dungeon_1").unwrap();
        assert_eq!(curated_image, png_bytes);
    }

    #[test]
    fn test_reject_staging_in_subfolder() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let meta = RedditSidecarMetadata {
            id: "bad_reddit_map".to_string(),
            source: Source::Reddit,
            source_url: "https://i.redd.it/bad.png".to_string(),
            title: "Bad Map".to_string(),
            author: "spammer".to_string(),
            parsed_gridless: false,
            parsed_dimensions: None,
            created_at: Utc::now(),
        };

        let png_bytes = create_test_png(400, 400);
        storage.save_staging_reddit(&meta, &png_bytes).unwrap();

        let reddit_dir = storage.staging_dir().join("reddit");
        let bundle_dir = reddit_dir.join("bad_reddit_map");
        assert!(bundle_dir.join("0.png").is_file() || reddit_dir.join("bad_reddit_map.png").is_file());
        assert!(bundle_dir.join("manifest.json").is_file() || reddit_dir.join("bad_reddit_map.json").is_file());

        storage
            .reject_staging("bad_reddit_map", RejectionReason::Irrelevant, Some("Not a battlemap"))
            .unwrap();

        assert!(!bundle_dir.exists());
        assert!(!reddit_dir.join("bad_reddit_map.png").exists());
        assert!(!reddit_dir.join("bad_reddit_map.json").exists());
        assert!(storage.is_rejected("bad_reddit_map").unwrap());
        assert!(storage.is_duplicate("bad_reddit_map").unwrap());
    }

    #[test]
    fn test_deduplication_checks_and_indexed_id_support() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        // Initially not rejected or duplicate
        assert!(!storage.is_rejected("post_xyz").unwrap());
        assert!(!storage.is_duplicate("post_xyz").unwrap());
        assert!(!storage.is_duplicate("post_xyz_0").unwrap());

        // Log rejection for base post id
        storage
            .log_rejection("post_xyz", RejectionReason::BlurryLowRes, None)
            .unwrap();

        assert!(storage.is_rejected("post_xyz").unwrap());
        assert!(storage.is_rejected("post_xyz_0").unwrap());
        assert!(storage.is_rejected("post_xyz_1").unwrap());
        assert!(storage.is_duplicate("post_xyz").unwrap());
        assert!(storage.is_duplicate("post_xyz_0").unwrap());
    }

    #[test]
    fn test_commit_curation_preserves_layer_source_url_provenance() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let mut candidate = Candidate::with_now(
            "provenance_bundle",
            Source::Reddit,
            "provenance_bundle/0.png",
            (1024, 1024),
        );
        candidate.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024))
                .with_source_url("https://i.redd.it/ground.png"),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1024, 1024))
                .with_source_url("https://i.redd.it/upper.png"),
        ];
        storage
            .save_staging_bundle(
                None,
                &candidate,
                &[b"ground-bytes".to_vec(), b"upper-bytes".to_vec()],
            )
            .unwrap();

        let curated = CuratedMap::with_now(
            "provenance_bundle",
            Theme::Dungeon,
            vec!["provenance".into()],
            (1024, 1024),
        )
        .with_description("Provenance-preserving dungeon");

        let stored = storage.commit_curation(&curated).unwrap();
        assert_eq!(
            stored.layers[0].source_url.as_deref(),
            Some("https://i.redd.it/ground.png")
        );
        assert_eq!(
            stored.layers[1].source_url.as_deref(),
            Some("https://i.redd.it/upper.png")
        );

        let manifest_path = dir
            .path()
            .join("curated")
            .join("provenance_bundle")
            .join("manifest.json");
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest.contains("https://i.redd.it/ground.png"));
        assert!(manifest.contains("https://i.redd.it/upper.png"));

        let listed = storage.list_curated(None).unwrap();
        let persisted = listed.iter().find(|m| m.id == "provenance_bundle").unwrap();
        assert_eq!(persisted.layers, stored.layers);
    }

    #[test]
    fn test_uniform_bundle_directory_layout_staging() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let mut candidate = Candidate::with_now(
            "castle_bundle",
            Source::Reddit,
            "castle_bundle/0.png",
            (1920, 1080),
        );
        candidate.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080)),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080)),
        ];
        let img0 = b"png-bytes-ground-layer";
        let img1 = b"png-bytes-upper-layer";

        storage
            .save_staging_bundle(None, &candidate, &[img0.to_vec(), img1.to_vec()])
            .unwrap();

        // Verify uniform staging bundle directory layout: <id>/manifest.json + 0.png, 1.png
        let bundle_dir = storage.staging_dir().join("castle_bundle");
        assert!(bundle_dir.is_dir());
        let manifest_path = bundle_dir.join("manifest.json");
        assert!(manifest_path.is_file());
        let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest_content.contains("castle_bundle"));
        assert!(manifest_content.contains("upper_floor"));

        let img0_path = bundle_dir.join("0.png");
        assert!(img0_path.is_file());
        assert_eq!(std::fs::read(&img0_path).unwrap(), img0);

        let img1_path = bundle_dir.join("1.png");
        assert!(img1_path.is_file());
        assert_eq!(std::fs::read(&img1_path).unwrap(), img1);

        // Verify retrieval via storage repository methods
        assert_eq!(
            storage.get_staging_layer_image("castle_bundle", 0).unwrap(),
            img0
        );
        assert_eq!(
            storage.get_staging_layer_image("castle_bundle", 1).unwrap(),
            img1
        );

        let staged_list = storage.list_staging().unwrap();
        assert_eq!(staged_list.len(), 1);
        assert_eq!(staged_list[0].id, "castle_bundle");
        assert_eq!(staged_list[0].resolved_layers().len(), 2);
    }

    #[test]
    fn test_uniform_bundle_directory_layout_curated() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let mut candidate = Candidate::with_now(
            "keep_bundle",
            Source::Reddit,
            "keep_bundle/0.png",
            (2048, 2048),
        );
        candidate.layers = vec![
            Layer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
            Layer::new(2, "2.png", LayerRole::Exclude, None, (2048, 2048)),
        ];
        let img0 = b"keep-ground-bytes";
        let img1 = b"keep-upper-bytes";
        let img2 = b"keep-exclude-grid-bytes";

        storage
            .save_staging_bundle(
                None,
                &candidate,
                &[img0.to_vec(), img1.to_vec(), img2.to_vec()],
            )
            .unwrap();

        let curated = CuratedMap::with_now(
            "keep_bundle",
            Theme::Dungeon,
            vec!["stronghold".into()],
            (2048, 2048),
        );
        storage.curate_staging(&curated).unwrap();

        // Staging bundle directory is cleaned up
        assert!(!storage.staging_dir().join("keep_bundle").exists());

        // Curated directory layout: <id>/manifest.json + deterministic included layer filenames
        let curated_bundle_dir = storage.curated_dir().join("keep_bundle");
        assert!(curated_bundle_dir.is_dir());

        let manifest_path = curated_bundle_dir.join("manifest.json");
        assert!(manifest_path.is_file());
        let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest_content.contains("keep_bundle"));

        // Included layers: 0_ground.png and 1_upper_1.png
        let dest_img0 = curated_bundle_dir.join("0_ground.png");
        assert!(dest_img0.is_file());
        assert_eq!(std::fs::read(&dest_img0).unwrap(), img0);

        let dest_img1 = curated_bundle_dir.join("1_upper_1.png");
        assert!(dest_img1.is_file());
        assert_eq!(std::fs::read(&dest_img1).unwrap(), img1);

        // Excluded layer is not stored in curated bundle
        assert!(!curated_bundle_dir.join("2.png").exists());
        assert!(!curated_bundle_dir.join("2_exclude.png").exists());

        // Verify retrieval via storage repository methods
        assert_eq!(
            storage
                .get_curated_layer_image(&Theme::Dungeon, "keep_bundle", 0)
                .unwrap(),
            img0
        );
        assert_eq!(
            storage
                .get_curated_layer_image(&Theme::Dungeon, "keep_bundle", 1)
                .unwrap(),
            img1
        );

        let curated_list = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
        assert_eq!(curated_list.len(), 1);
        assert_eq!(curated_list[0].id, "keep_bundle");
        assert_eq!(curated_list[0].layers.len(), 2);
    }

    #[test]
    fn test_idempotent_startup_migration_legacy_storage() {
        let dir = tempdir().unwrap();
        let staging_reddit = dir.path().join("staging").join("reddit");
        std::fs::create_dir_all(&staging_reddit).unwrap();
        let staging_root = dir.path().join("staging");
        let curated_dungeon = dir.path().join("curated").join("dungeon");
        std::fs::create_dir_all(&curated_dungeon).unwrap();

        // 1. Legacy Reddit multi-part staging flat files: {post_id}_0 and {post_id}_1
        let cand0 = Candidate {
            id: "legacy_post_0".to_string(),
            source: Source::Reddit,
            filename: "legacy_post_0.png".to_string(),
            dimensions: (1000, 1000),
            created_at: Utc::now(),
            source_url: None,
            title: Some("Castle Siege".to_string()),
            author: Some("Cartographer".to_string()),
            parsed_gridless: Some(true),
            parsed_dimensions: Some((1000, 1000)),
            layers: vec![Layer {
                index: 0,
                filename: "legacy_post_0.png".to_string(),
                role: LayerRole::Ground,
                elevation: Some(0),
                caption: Some("Ground floor".to_string()),
                source_url: None,
                dimensions: (1000, 1000),
            }],
            parent_group_id: None,
        };
        let cand1 = Candidate {
            id: "legacy_post_1".to_string(),
            source: Source::Reddit,
            filename: "legacy_post_1.png".to_string(),
            dimensions: (1000, 1000),
            created_at: Utc::now(),
            source_url: None,
            title: Some("Castle Siege".to_string()),
            author: Some("Cartographer".to_string()),
            parsed_gridless: Some(true),
            parsed_dimensions: Some((1000, 1000)),
            layers: vec![Layer {
                index: 0,
                filename: "legacy_post_1.png".to_string(),
                role: LayerRole::UpperFloor,
                elevation: Some(1),
                caption: Some("2nd floor".to_string()),
                source_url: None,
                dimensions: (1000, 1000),
            }],
            parent_group_id: None,
        };
        let img0_bytes = b"legacy-ground-image";
        let img1_bytes = b"legacy-upper-image";
        std::fs::write(staging_reddit.join("legacy_post_0.png"), img0_bytes).unwrap();
        std::fs::write(
            staging_reddit.join("legacy_post_0.json"),
            serde_json::to_string_pretty(&cand0).unwrap(),
        )
        .unwrap();
        std::fs::write(staging_reddit.join("legacy_post_1.png"), img1_bytes).unwrap();
        std::fs::write(
            staging_reddit.join("legacy_post_1.json"),
            serde_json::to_string_pretty(&cand1).unwrap(),
        )
        .unwrap();

        // 2. Standalone legacy flat staging map
        let cand_standalone = Candidate::with_now(
            "standalone_map",
            Source::Reddit,
            "standalone_map.png",
            (800, 600),
        );
        let standalone_bytes = b"standalone-image-bytes";
        std::fs::write(staging_root.join("standalone_map.png"), standalone_bytes).unwrap();
        std::fs::write(
            staging_root.join("standalone_map.json"),
            serde_json::to_string_pretty(&cand_standalone).unwrap(),
        )
        .unwrap();

        // 3. Legacy flat curated map
        let curated_map = CuratedMap::with_now(
            "legacy_dungeon_flat",
            Theme::Dungeon,
            vec!["crypt".into()],
            (1200, 1200),
        );
        let curated_bytes = b"curated-crypt-image-bytes";
        std::fs::write(curated_dungeon.join("legacy_dungeon_flat.png"), curated_bytes).unwrap();
        std::fs::write(
            curated_dungeon.join("legacy_dungeon_flat.json"),
            serde_json::to_string_pretty(&curated_map).unwrap(),
        )
        .unwrap();

        // Initialize FilesystemStorage: constructor automatically invokes migrate_legacy_storage()
        let storage = FilesystemStorage::new(dir.path());

        // Verify flat files have been removed
        assert!(!staging_reddit.join("legacy_post_0.png").exists());
        assert!(!staging_reddit.join("legacy_post_0.json").exists());
        assert!(!staging_reddit.join("legacy_post_1.png").exists());
        assert!(!staging_reddit.join("legacy_post_1.json").exists());
        assert!(!staging_root.join("standalone_map.png").exists());
        assert!(!staging_root.join("standalone_map.json").exists());
        assert!(!curated_dungeon.join("legacy_dungeon_flat.png").exists());
        assert!(!curated_dungeon.join("legacy_dungeon_flat.json").exists());

        // Verify Reddit sibling files were regrouped into uniform bundle directory: staging/reddit/legacy_post/
        let reddit_bundle = staging_reddit.join("legacy_post");
        assert!(reddit_bundle.is_dir());
        assert!(reddit_bundle.join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(reddit_bundle.join("0.png")).unwrap(),
            img0_bytes
        );
        assert_eq!(
            std::fs::read(reddit_bundle.join("1.png")).unwrap(),
            img1_bytes
        );

        // Verify standalone map migrated into uniform bundle: staging/standalone_map/
        let standalone_bundle = staging_root.join("standalone_map");
        assert!(standalone_bundle.is_dir());
        assert!(standalone_bundle.join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(standalone_bundle.join("0.png")).unwrap(),
            standalone_bytes
        );

        // Verify curated flat map migrated into uniform bundle: curated/legacy_dungeon_flat/
        let curated_bundle = dir.path().join("curated").join("legacy_dungeon_flat");
        assert!(curated_bundle.is_dir());
        assert!(curated_bundle.join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(curated_bundle.join("0_ground.png")).unwrap(),
            curated_bytes
        );
        assert!(!curated_dungeon.exists());
        // Verify storage queries work on migrated bundles
        assert_eq!(
            storage.get_staging_layer_image("legacy_post", 0).unwrap(),
            img0_bytes
        );
        assert_eq!(
            storage.get_staging_layer_image("legacy_post", 1).unwrap(),
            img1_bytes
        );
        assert_eq!(
            storage
                .get_curated_layer_image(&Theme::Dungeon, "legacy_dungeon_flat", 0)
                .unwrap(),
            curated_bytes
        );

        // Verify idempotence: running migration again succeeds and preserves the bundles
        assert!(storage.migrate_legacy_storage().is_ok());
        assert!(reddit_bundle.join("manifest.json").is_file());
        assert!(standalone_bundle.join("manifest.json").is_file());
        assert!(curated_bundle.join("manifest.json").is_file());
    }

    #[test]
    fn test_reversible_bundle_split_and_regroup() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let mut candidate = Candidate::with_now(
            "split_regroup_parent",
            Source::Reddit,
            "split_regroup_parent/0.png",
            (1920, 1080),
        );
        candidate.title = Some("Two-story tavern".to_string());
        let mut layer0 = Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080));
        layer0.caption = Some("1st Floor Tavern".to_string());
        let mut layer1 =
            Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080));
        layer1.caption = Some("2nd Floor Rooms".to_string());
        candidate.layers = vec![layer0, layer1];

        let img0 = b"layer-0-tavern-bytes";
        let img1 = b"layer-1-rooms-bytes";

        storage
            .save_staging_bundle(None, &candidate, &[img0.to_vec(), img1.to_vec()])
            .unwrap();

        // 1. Split bundle
        let split_candidates = storage
            .split_staging_bundle("split_regroup_parent")
            .unwrap();
        assert_eq!(split_candidates.len(), 2);
        assert_eq!(split_candidates[0].id, "split_regroup_parent_0");
        assert_eq!(split_candidates[1].id, "split_regroup_parent_1");
        assert_eq!(
            split_candidates[0].parent_group_id.as_deref(),
            Some("split_regroup_parent")
        );
        assert_eq!(
            split_candidates[1].parent_group_id.as_deref(),
            Some("split_regroup_parent")
        );

        // Original parent bundle directory should be removed
        assert!(!storage.staging_dir().join("split_regroup_parent").exists());

        // Split child bundle directories exist and hold their images
        assert_eq!(
            storage
                .get_staging_layer_image("split_regroup_parent_0", 0)
                .unwrap(),
            img0
        );
        assert_eq!(
            storage
                .get_staging_layer_image("split_regroup_parent_1", 0)
                .unwrap(),
            img1
        );

        // 2. Regroup bundle
        let regrouped = storage
            .regroup_staging_bundle("split_regroup_parent")
            .unwrap();
        assert_eq!(regrouped.id, "split_regroup_parent");
        assert_eq!(regrouped.layers.len(), 2);
        assert_eq!(regrouped.layers[0].role, LayerRole::Ground);
        assert_eq!(regrouped.layers[0].elevation, Some(0));
        assert_eq!(regrouped.layers[1].role, LayerRole::UpperFloor);
        assert_eq!(regrouped.layers[1].elevation, Some(1));

        // Split child directories should be cleaned up
        assert!(!storage
            .staging_dir()
            .join("split_regroup_parent_0")
            .exists());
        assert!(!storage
            .staging_dir()
            .join("split_regroup_parent_1")
            .exists());

        // Parent bundle restored and retrievable
        let parent_dir = storage.staging_dir().join("split_regroup_parent");
        assert!(parent_dir.is_dir());
        assert!(parent_dir.join("manifest.json").is_file());
        assert_eq!(
            storage
                .get_staging_layer_image("split_regroup_parent", 0)
                .unwrap(),
            img0
        );
        assert_eq!(
            storage
                .get_staging_layer_image("split_regroup_parent", 1)
                .unwrap(),
            img1
        );
    }
    #[test]
    fn test_delete_curated_success_and_rejection_record() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let png = create_test_png(1024, 1024);
        let candidate = Candidate::with_now(
            "map_del_1",
            Source::Reddit,
            "0.png",
            (1024, 1024),
        );
        storage.save_staging(&candidate, &png).unwrap();

        let curated = CuratedMap::with_now(
            "map_del_1",
            Theme::Dungeon,
            vec!["crypt".into()],
            (1024, 1024),
        )
        .with_description("An ancient crypt");
        storage.curate_staging(&curated).unwrap();

        assert!(storage.is_curated("map_del_1").unwrap());
        assert!(!storage.is_rejected("map_del_1").unwrap());
        assert!(storage.find_curated_bundle_dir("map_del_1").is_some());

        // Delete curated map with rejection reason
        storage
            .delete_curated(
                "map_del_1",
                RejectionReason::GridBurnedIn,
                Some("Visible square grid artifacts on tiles"),
            )
            .unwrap();

        // Post-deletion assertions
        assert!(!storage.is_curated("map_del_1").unwrap());
        assert!(storage.is_rejected("map_del_1").unwrap());
        assert!(storage.is_duplicate("map_del_1").unwrap());
        assert!(storage.find_curated_bundle_dir("map_del_1").is_none());
        assert!(storage.get_curated_layer_image_by_id("map_del_1", 0).is_err());

        let rejections = storage.read_rejections().unwrap();
        let record = rejections.iter().find(|r| r.id == "map_del_1").unwrap();
        assert_eq!(record.reason, RejectionReason::GridBurnedIn);
        assert_eq!(
            record.notes.as_deref(),
            Some("Visible square grid artifacts on tiles")
        );

        // Deleting non-existent map returns NotFound
        let err = storage
            .delete_curated("map_del_1", RejectionReason::Unspecified, None)
            .unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn test_update_curated_metadata_and_description() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let png = create_test_png(1024, 1024);
        let candidate = Candidate::with_now(
            "map_upd_meta",
            Source::Reddit,
            "0.png",
            (1024, 1024),
        );
        storage.save_staging(&candidate, &png).unwrap();

        let curated = CuratedMap::with_now(
            "map_upd_meta",
            Theme::Dungeon,
            vec!["dungeon-crawl".into()],
            (1024, 1024),
        )
        .with_description("Original dungeon description with carved stone walls.");
        storage.curate_staging(&curated).unwrap();

        // Update themes, tags, and description
        let update = CuratedMapUpdate::default()
            .with_themes(vec!["cave".into(), "underdark".into()])
            .with_tags(vec!["stalactites".into(), "bioluminescence".into()])
            .with_description("Luminescent fungal cavern with damp stone floors.");

        let updated = storage.update_curated("map_upd_meta", update).unwrap();
        assert!(updated.tags.contains(&"cave".to_string()));
        assert!(updated.tags.contains(&"underdark".to_string()));
        assert!(updated.tags.contains(&"stalactites".to_string()));
        assert!(updated.tags.contains(&"bioluminescence".to_string()));
        assert_eq!(
            updated.description.as_deref(),
            Some("Luminescent fungal cavern with damp stone floors.")
        );

        // Verify manifest on disk
        let list = storage.list_curated(None).unwrap();
        let reloaded = list.iter().find(|m| m.id == "map_upd_meta").unwrap();
        assert!(reloaded.tags.contains(&"cave".to_string()));
        assert!(reloaded.tags.contains(&"underdark".to_string()));
        assert!(reloaded.tags.contains(&"stalactites".to_string()));
        assert!(reloaded.tags.contains(&"bioluminescence".to_string()));
        assert_eq!(
            reloaded.description.as_deref(),
            Some("Luminescent fungal cavern with damp stone floors.")
        );

        // Verify LoRA .txt companion is updated with new description
        let bundle_dir = storage.find_curated_bundle_dir("map_upd_meta").unwrap();
        let lora_txt =
            std::fs::read_to_string(bundle_dir.join("map_upd_meta_lora1024.txt")).unwrap();
        assert!(lora_txt.contains("luminescent"));
        assert!(lora_txt.contains("fungal cavern"));

        // Empty legacy themes is a no-op
        let unchanged = storage
            .update_curated("map_upd_meta", CuratedMapUpdate::default().with_themes(vec![]))
            .unwrap();
        assert_eq!(
            unchanged.tags,
            vec!["stalactites", "bioluminescence", "cave", "underdark"]
        );

        // Updating non-existent curated map returns NotFound
        let err_nf = storage
            .update_curated("non_existent_map", CuratedMapUpdate::default())
            .unwrap_err();
        assert!(matches!(err_nf, StorageError::NotFound(_)));
    }

    #[test]
    fn test_update_curated_layers_reindexing_and_companions() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let img0 = create_test_png(1000, 1000);
        let img1 = create_test_png(1200, 1200);

        let bundle_cand = Candidate::with_now(
            "map_upd_layers",
            Source::Reddit,
            "0.png",
            (1000, 1000),
        );
        storage
            .save_staging_bundle(None, &bundle_cand, &[img0.clone(), img1.clone()])
            .unwrap();
        storage
            .update_staging_layers(
                "map_upd_layers",
                vec![
                    Layer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
                    Layer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1200, 1200)),
                ],
            )
            .unwrap();

        let curated = CuratedMap::with_now(
            "map_upd_layers",
            Theme::Dungeon,
            vec!["tower".into()],
            (1000, 1000),
        )
        .with_description("Multi-level watchtower");
        storage.curate_staging(&curated).unwrap();

        // Verify initial layer images
        assert_eq!(
            storage
                .get_curated_layer_image_by_id("map_upd_layers", 0)
                .unwrap(),
            img0
        );
        assert_eq!(
            storage
                .get_curated_layer_image_by_id("map_upd_layers", 1)
                .unwrap(),
            img1
        );

        // Update: Swap roles so layer 1 (img1) becomes Ground, and layer 0 (img0) becomes Roof
        let new_layers = vec![
            Layer::new(
                0,
                "1_upper_1.png",
                LayerRole::Ground,
                Some(0),
                (1200, 1200),
            ),
            Layer::new(
                1,
                "0_ground.png",
                LayerRole::Roof,
                None,
                (1000, 1000),
            ),
        ];

        let update = CuratedMapUpdate::default()
            .with_layers(new_layers)
            .with_description("Watchtower with fortified observation deck");

        let updated = storage.update_curated("map_upd_layers", update).unwrap();
        assert_eq!(updated.layers.len(), 2);
        assert_eq!(updated.layers[0].filename, "0_ground.png");
        assert_eq!(updated.layers[0].role, LayerRole::Ground);
        assert_eq!(updated.layers[1].filename, "1_roof.png");
        assert_eq!(updated.layers[1].role, LayerRole::Roof);

        // Verify retrieved images: Ground (0) now has img1 bytes!
        assert_eq!(
            storage
                .get_curated_layer_image_by_id("map_upd_layers", 0)
                .unwrap(),
            img1
        );
        assert_eq!(
            storage
                .get_curated_layer_image_by_id("map_upd_layers", 1)
                .unwrap(),
            img0
        );

        // Verify bundle directory has 0_ground.png and 1_roof.png, and old 1_upper_1.png was cleaned up
        let bundle_dir = storage.find_curated_bundle_dir("map_upd_layers").unwrap();
        assert!(bundle_dir.join("0_ground.png").is_file());
        assert!(bundle_dir.join("1_roof.png").is_file());
        assert!(!bundle_dir.join("1_upper_1.png").exists());

        // Verify invalid layer updates: all excluded or unassigned fails
        let unassigned_update = CuratedMapUpdate::default().with_layers(vec![
            Layer::new(0, "0_ground.png", LayerRole::Unassigned, None, (1000, 1000)),
        ]);
        assert!(storage
            .update_curated("map_upd_layers", unassigned_update)
            .is_err());
    }

    #[test]
    fn test_get_curated_layer_image_by_id() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        let img0 = create_test_png(800, 600);
        let candidate = Candidate::with_now(
            "map_layer_test",
            Source::Reddit,
            "0.png",
            (800, 600),
        );
        storage.save_staging(&candidate, &img0).unwrap();

        let curated = CuratedMap::with_now(
            "map_layer_test",
            Theme::Cave,
            vec!["grottos".into()],
            (800, 600),
        );
        storage.curate_staging(&curated).unwrap();

        // Layer 0 retrieval
        let retrieved = storage
            .get_curated_layer_image_by_id("map_layer_test", 0)
            .unwrap();
        assert_eq!(retrieved, img0);

        // Layer 99 out of bounds
        let err_oob = storage
            .get_curated_layer_image_by_id("map_layer_test", 99)
            .unwrap_err();
        assert!(matches!(err_oob, StorageError::NotFound(_)));

        // Non-existent map
        let err_nf = storage
            .get_curated_layer_image_by_id("no_such_map", 0)
            .unwrap_err();
        assert!(matches!(err_nf, StorageError::NotFound(_)));
    }

    #[test]
    fn test_export_selected_lora_dataset() {
        let dir = tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path());

        // Create 3 maps
        for i in 1..=3 {
            let id = format!("map_export_{i}");
            let png = create_test_png(1024, 1024);
            let cand = Candidate::with_now(&id, Source::Reddit, "0.png", (1024, 1024));
            storage.save_staging(&cand, &png).unwrap();
            let curated = CuratedMap::with_now(
                &id,
                Theme::Dungeon,
                vec!["dungeon".into()],
                (1024, 1024),
            )
            .with_description(format!("Dungeon map number {i}"));
            storage.curate_staging(&curated).unwrap();
        }

        // Export only map_export_1 and map_export_3 (with a duplicate request)
        let zip_bytes = storage
            .export_selected_lora_dataset(&[
                "map_export_1".to_string(),
                "map_export_3".to_string(),
                "map_export_1".to_string(), // duplicate
            ])
            .unwrap();

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
        let mut file_names = Vec::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            file_names.push(file.name().to_string());
        }

        // Contains map 1 and 3 files
        assert!(file_names.iter().any(|n| n.contains("map_export_1_lora1024.png")));
        assert!(file_names.iter().any(|n| n.contains("map_export_1_lora1024.txt")));
        assert!(file_names.iter().any(|n| n.contains("map_export_3_lora1024.png")));
        assert!(file_names.iter().any(|n| n.contains("map_export_3_lora1024.txt")));

        // Does NOT contain map 2 files
        assert!(!file_names.iter().any(|n| n.contains("map_export_2")));

        // Exactly 4 files (2 maps * 2 files each) due to deduplication
        assert_eq!(file_names.len(), 4);

        // Empty selection produces valid empty zip
        let empty_zip = storage.export_selected_lora_dataset(&[]).unwrap();
        let empty_archive = zip::ZipArchive::new(std::io::Cursor::new(empty_zip)).unwrap();
        assert_eq!(empty_archive.len(), 0);
    }
}
