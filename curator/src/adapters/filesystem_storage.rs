use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

use crate::adapters::SimdImageProcessor;
use crate::domain::{
    infer_bundle_layer_roles, BenchmarkMetadata, Candidate, CandidateLayer, CaptionGenerator,
    CuratedLayer, CuratedMap, DirectoryStats, LayerRole, PurgeStagingResult, RedditSidecarMetadata,
    RejectionReason, RejectionRecord, Source, StorageStats, Theme, ThemeStorageStats,
};
use crate::ports::image_processor::{ImageProcessor, ResizeStrategy};
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
    /// Helper to process primary image bytes and write LoRA 1024x1024 PNG and companion .txt caption.
    fn write_lora_companions(
        &self,
        theme: &Theme,
        id: &str,
        primary_image_bytes: &[u8],
        caption_text: &str,
        bundle_dir: Option<&Path>,
    ) {
        let theme_dir = self.curated_theme_dir(theme);
        let _ = std::fs::create_dir_all(&theme_dir);

        // Always write caption text (.txt)
        let theme_txt = theme_dir.join(format!("{id}_lora1024.txt"));
        let _ = Self::atomic_write(&theme_txt, caption_text.as_bytes());
        if let Some(b_dir) = bundle_dir {
            let bundle_txt = b_dir.join(format!("{id}_lora1024.txt"));
            let _ = Self::atomic_write(&bundle_txt, caption_text.as_bytes());
        }

        // Try normalizing to 1024x1024 PNG using SIMD Lanczos3 resizer
        let processor = SimdImageProcessor::new();
        match processor.normalize_lora(primary_image_bytes, ResizeStrategy::FitWithPadding) {
            Ok(lora_png) => {
                let theme_png = theme_dir.join(format!("{id}_lora1024.png"));
                let _ = Self::atomic_write(&theme_png, &lora_png);
                if let Some(b_dir) = bundle_dir {
                    let bundle_png = b_dir.join(format!("{id}_lora1024.png"));
                    let _ = Self::atomic_write(&bundle_png, &lora_png);
                }
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

    /// Locate curated layer image in a theme directory by ID and layer index.
    fn find_curated_layer_image_path(
        &self,
        theme: &Theme,
        id: &str,
        layer_index: usize,
    ) -> Option<PathBuf> {
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_dir = theme_dir.join(id);
        if bundle_dir.is_dir() {
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

    /// Locate primary curated image in a theme directory by ID.
    fn find_curated_image_path(&self, theme: &Theme, id: &str) -> Option<PathBuf> {
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_dir = theme_dir.join(id);
        if bundle_dir.is_dir() {
            if let Some(p) = self.find_curated_layer_image_path(theme, id, 0) {
                return Some(p);
            }
        }

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
                    self.migrate_curated_theme_directory(theme, &theme_dir)?;
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
                layers.push(CandidateLayer {
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
                cand.layers = vec![CandidateLayer {
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
                let layer_filename = CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), img_ext);
                let target_img = bundle_dir.join(&layer_filename);

                Self::atomic_move(&img_path, &target_img)?;
                let _ = std::fs::remove_file(json_path);

                map.id = stem;
                map.theme = *theme;
                if map.layers.is_empty() {
                    map.layers = vec![CuratedLayer {
                        index: 0,
                        filename: layer_filename,
                        role: LayerRole::Ground,
                        elevation: Some(0),
                        caption: None,
                        dimensions: map.dimensions,
                    }];
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
                stored_candidate.layers = vec![CandidateLayer {
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
                    .map(|(i, (role, elevation))| CandidateLayer {
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
                .with_layers(vec![CandidateLayer {
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
        if !CuratedMap::is_valid_rating(map.rating) {
            return Err(StorageError::InvalidData(format!(
                "Rating must be between 1 and 5, got {}",
                map.rating
            )));
        }
        map.validate_layers().map_err(StorageError::InvalidData)?;

        let bundle_dir = self.curated_theme_dir(&map.theme).join(&map.id);
        std::fs::create_dir_all(&bundle_dir)?;

        let mut stored_map = map.clone();
        if stored_map.layers.is_empty() {
            let filename = CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), "png");
            stored_map.layers = vec![CuratedLayer {
                index: 0,
                filename,
                role: LayerRole::Ground,
                elevation: Some(0),
                caption: None,
                dimensions: stored_map.dimensions,
            }];
        }

        let first_filename = stored_map.layers[0].filename.clone();
        Self::atomic_write(&bundle_dir.join(&first_filename), data)?;

        let manifest_json = serde_json::to_string_pretty(&stored_map)?;
        Self::atomic_write(&bundle_dir.join("manifest.json"), manifest_json.as_bytes())?;

        let caption = CaptionGenerator::generate_for_curated(&stored_map, None);
        self.write_lora_companions(&map.theme, &map.id, data, &caption, Some(&bundle_dir));
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
            "Curated LoRA image for candidate '{id}' not found in theme '{theme}'"
        )))
    }

    fn get_curated_lora_caption(&self, theme: &Theme, id: &str) -> Result<String, StorageError> {
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
            "Curated LoRA caption for candidate '{id}' not found in theme '{theme}'"
        )))
    }

    fn is_lora_normalized(&self, theme: &Theme, id: &str) -> Result<bool, StorageError> {
        let theme_dir = self.curated_theme_dir(theme);
        let bundle_png = theme_dir.join(id).join(format!("{id}_lora1024.png"));
        let theme_png = theme_dir.join(format!("{id}_lora1024.png"));
        Ok(bundle_png.is_file() || theme_png.is_file())
    }

    fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        let _ = self.migrate_legacy_storage();
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

            if let Ok(entries) = std::fs::read_dir(&theme_dir) {
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
                                    results.push(map);
                                }
                            }
                        }
                    } else if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if ext == "json" {
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    if let Ok(map) = serde_json::from_str::<CuratedMap>(&content) {
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

    fn curate_staging(&self, map: &CuratedMap) -> Result<(), StorageError> {
        if !CuratedMap::is_valid_rating(map.rating) {
            return Err(StorageError::InvalidData(format!(
                "Rating must be between 1 and 5, got {}",
                map.rating
            )));
        }
        map.validate_layers().map_err(StorageError::InvalidData)?;

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
            let effective_layers: Vec<CandidateLayer> = if !map.layers.is_empty() {
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
                        CandidateLayer {
                            index: cl.index,
                            filename: src_filename,
                            role: cl.role,
                            elevation: cl.elevation,
                            caption,
                            source_url: src_layer.and_then(|l| l.source_url.clone()),
                            dimensions: dims,
                        }
                    })
                    .collect()
            } else {
                candidate_layers
            };

            let role_pairs: Vec<(LayerRole, Option<i8>)> = effective_layers
                .iter()
                .map(|l| (l.role, l.elevation))
                .collect();
            crate::domain::validate_layers_for_curation(&role_pairs)
                .map_err(StorageError::InvalidData)?;

            let curated_bundle_dir = self.curated_theme_dir(&map.theme).join(&map.id);
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
                let dest_filename = CuratedLayer::format_filename(
                    curated_idx,
                    src_layer.role,
                    src_layer.elevation,
                    ext,
                );
                let dest_img_path = curated_bundle_dir.join(&dest_filename);
                Self::atomic_move(&src_img_path, &dest_img_path)?;

                curated_layers.push(CuratedLayer {
                    index: curated_idx,
                    filename: dest_filename,
                    role: src_layer.role,
                    elevation: src_layer.elevation,
                    caption: src_layer.caption.clone(),
                    dimensions: if src_layer.dimensions != (0, 0) {
                        src_layer.dimensions
                    } else {
                        map.dimensions
                    },
                });
            }

            let mut stored_map = map.clone();
            if stored_map.dimensions == (0, 0) {
                stored_map.dimensions = curated_layers
                    .first()
                    .map(|l| l.dimensions)
                    .unwrap_or(candidate.dimensions);
            }
            stored_map.layers = curated_layers;

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
                        &map.theme,
                        &map.id,
                        &primary_bytes,
                        &caption,
                        Some(&curated_bundle_dir),
                    );
                }
            }

            let _ = std::fs::remove_dir_all(&bundle_dir);
            return Ok(());
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

        let curated_bundle_dir = self.curated_theme_dir(&map.theme).join(&map.id);
        std::fs::create_dir_all(&curated_bundle_dir)?;

        let dest_filename = CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), ext);
        let target_image = curated_bundle_dir.join(&dest_filename);
        let target_manifest = curated_bundle_dir.join("manifest.json");

        Self::atomic_move(&staging_image, &target_image)?;

        let mut stored_map = map.clone();
        if stored_map.layers.is_empty() {
            stored_map.layers = vec![CuratedLayer {
                index: 0,
                filename: dest_filename,
                role: LayerRole::Ground,
                elevation: Some(0),
                caption: None,
                dimensions: stored_map.dimensions,
            }];
        }
        let curated_json = serde_json::to_string_pretty(&stored_map)?;
        Self::atomic_write(&target_manifest, curated_json.as_bytes())?;
        if let Ok(primary_bytes) = std::fs::read(&target_image) {
            let caption = CaptionGenerator::generate_for_curated(
                &stored_map,
                candidate.as_ref(),
            );
            self.write_lora_companions(
                &map.theme,
                &map.id,
                &primary_bytes,
                &caption,
                Some(&curated_bundle_dir),
            );
        }

        if let Some(sc) = &staging_sidecar {
            if sc.is_file() {
                let _ = std::fs::remove_file(sc);
            }
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

    fn update_staging_layers(
        &self,
        id: &str,
        layers: Vec<CandidateLayer>,
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

            let child_layer = CandidateLayer {
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

            regrouped_layers.push(CandidateLayer {
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
        let mut curated_total_bytes = 0u64;
        let mut curated_total_files = 0usize;

        for theme in Theme::ALL {
            let theme_dir = self.curated_theme_dir(theme);
            let stats = Self::scan_dir_stats(&theme_dir);
            let map_count = self
                .list_curated(Some(theme))
                .map(|maps| maps.len())
                .unwrap_or(0);

            curated_total_bytes += stats.bytes;
            curated_total_files += stats.file_count;

            curated_by_theme.insert(
                *theme,
                ThemeStorageStats::new(*theme, stats.bytes, stats.file_count, map_count),
            );
        }

        // Account for any loose top-level files in curated/ root if any exist
        let curated_dir = self.curated_dir();
        if curated_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&curated_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default();
                        if !name.starts_with('.') {
                            curated_total_files += 1;
                            if let Ok(meta) = entry.metadata() {
                                curated_total_bytes += meta.len();
                            }
                        }
                    }
                }
            }
        }

        let curated = DirectoryStats::new(curated_total_bytes, curated_total_files);

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
        assert!(!bundle_dir.exists());

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
            5,
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
            CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080)),
            CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080)),
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
            CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (2048, 2048)),
            CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
            CandidateLayer::new(2, "2.png", LayerRole::Exclude, None, (2048, 2048)),
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
            5,
            vec!["stronghold".into()],
            (2048, 2048),
        );
        storage.curate_staging(&curated).unwrap();

        // Staging bundle directory is cleaned up
        assert!(!storage.staging_dir().join("keep_bundle").exists());

        // Curated directory layout: <theme>/<id>/manifest.json + deterministic included layer filenames
        let curated_bundle_dir = storage.curated_theme_dir(&Theme::Dungeon).join("keep_bundle");
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
            layers: vec![CandidateLayer {
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
            layers: vec![CandidateLayer {
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
            4,
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

        // Verify curated flat map migrated into uniform bundle: curated/dungeon/legacy_dungeon_flat/
        let curated_bundle = curated_dungeon.join("legacy_dungeon_flat");
        assert!(curated_bundle.is_dir());
        assert!(curated_bundle.join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(curated_bundle.join("0_ground.png")).unwrap(),
            curated_bytes
        );

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
        let mut layer0 = CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080));
        layer0.caption = Some("1st Floor Tavern".to_string());
        let mut layer1 =
            CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080));
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
}
