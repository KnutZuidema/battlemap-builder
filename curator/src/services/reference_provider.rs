//! Virtuous cycle reference provider: resolves visual conditioning images for battlemap generation.
//!
//! Prioritizes 5-star and top-rated curated battlemaps from `StorageRepository` matching the prompt's
//! theme, falling back to curated static reference images (`data/battlemap-reference-*.jpeg`)
//! when no high-rating curated maps are available.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ExtendedColorType;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::domain::{
    crop_section_box, CuratedMap, GridConfig, LayerRole, MapSection, ReferenceImage, Theme,
};
use crate::ports::storage::{StorageError, StorageRepository};

// Embedded fallback reference image bytes so resolution succeeds reliably in any environment
const STATIC_BYTES_REF_1: &[u8] = include_bytes!("../../../data/battlemap-reference-1.jpeg");
const STATIC_BYTES_REF_2: &[u8] = include_bytes!("../../../data/battlemap-reference-2.jpg");
const STATIC_BYTES_REF_3: &[u8] = include_bytes!("../../../data/battlemap-reference-3.png");
const STATIC_BYTES_REF_4: &[u8] = include_bytes!("../../../data/battlemap-reference-4.jpeg");

/// Provenance of a resolved reference image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceProvenance {
    Curated { id: String, theme: Theme },
    StaticFallback { name: String, theme: Theme },
}

/// Resolution output containing the preprocessed reference image and its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceResolution {
    pub image: ReferenceImage,
    pub provenance: ReferenceProvenance,
}

/// Errors arising during reference resolution or image preprocessing.
#[derive(Debug, Error)]
pub enum ReferenceProviderError {
    #[error("Storage repository error: {0}")]
    Storage(#[from] StorageError),

    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Reference not found: {0}")]
    NotFound(String),
}

/// Definition of a static reference battlemap image.
#[derive(Debug, Clone)]
pub struct StaticReferenceDef {
    pub id: &'static str,
    pub filename: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
    pub embedded_bytes: &'static [u8],
}

/// The 4 standard curated static reference definitions matching `server/services/referenceImages.js`.
pub const STATIC_REFERENCES: &[StaticReferenceDef] = &[
    StaticReferenceDef {
        id: "stone-dungeon",
        filename: "battlemap-reference-4.jpeg",
        label: "Perspective Reference: 2D Stone Dungeon Plan View",
        keywords: &[
            "dungeon", "crypt", "tomb", "cave", "cavern", "cell", "prison",
            "stone", "underground", "vault", "catacomb", "temple", "sewer",
            "mine", "labyrinth", "shrine", "altar", "corridor",
        ],
        embedded_bytes: STATIC_BYTES_REF_4,
    },
    StaticReferenceDef {
        id: "ruins-courtyard",
        filename: "battlemap-reference-3.png",
        label: "Perspective Reference: 2D Ruins & Courtyard Plan View",
        keywords: &[
            "ruin", "ruins", "courtyard", "river", "bridge", "stream", "outdoor",
            "forest", "garden", "trees", "road", "path", "plaza", "swamp",
            "camp", "nature", "water", "waterfall", "cliff",
        ],
        embedded_bytes: STATIC_BYTES_REF_3,
    },
    StaticReferenceDef {
        id: "tavern-inn",
        filename: "battlemap-reference-1.jpeg",
        label: "Perspective Reference: 2D Tavern & Rooms Plan View",
        keywords: &[
            "tavern", "inn", "bar", "pub", "bed", "bedroom", "room", "house",
            "timber", "wood", "furniture", "kitchen", "cabin", "cottage", "innkeeper",
        ],
        embedded_bytes: STATIC_BYTES_REF_1,
    },
    StaticReferenceDef {
        id: "grand-hall-portal",
        filename: "battlemap-reference-2.jpg",
        label: "Perspective Reference: 2D Grand Hall & Portal Plan View",
        keywords: &[
            "portal", "hall", "great hall", "market", "shop", "chasm", "pit",
            "arena", "keep", "fortress", "mansion", "castle", "throne",
        ],
        embedded_bytes: STATIC_BYTES_REF_2,
    },
];

/// Maps a `Theme` to its matching default static reference definition.
pub fn static_reference_for_theme(theme: &Theme) -> &'static StaticReferenceDef {
    match theme {
        Theme::Dungeon | Theme::Cave => &STATIC_REFERENCES[0], // stone-dungeon
        Theme::Wilderness | Theme::Ruins => &STATIC_REFERENCES[1], // ruins-courtyard
        Theme::Tavern | Theme::Building => &STATIC_REFERENCES[2], // tavern-inn
        Theme::Urban | Theme::SciFi => &STATIC_REFERENCES[3], // grand-hall-portal
    }
}

/// Preprocesses raw image bytes:
/// 1. Decodes image format (PNG, JPEG, WebP, etc.).
/// 2. Center-crops to 1:1 square if aspect ratio is non-square.
/// 3. Resizes to `target_size x target_size` using high-quality Lanczos3 filter.
/// 4. Encodes to JPEG format with quality ~85 for optimal payload size.
pub fn preprocess_reference_image(
    raw_bytes: &[u8],
    target_size: u32,
) -> Result<Vec<u8>, ReferenceProviderError> {
    let img = image::load_from_memory(raw_bytes)?;
    let (width, height) = (img.width(), img.height());

    let cropped = if width != height {
        let size = width.min(height);
        let x = (width - size) / 2;
        let y = (height - size) / 2;
        img.crop_imm(x, y, size, size)
    } else {
        img
    };

    let resized = cropped.resize_exact(target_size, target_size, FilterType::Lanczos3);

    let mut output_bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut output_bytes, 85);
    encoder.encode(
        resized.to_rgb8().as_raw(),
        target_size,
        target_size,
        ExtendedColorType::Rgb8,
    )?;

    Ok(output_bytes)
}

/// Preprocesses a curated ground image for a specific section:
/// 1. Crops the exact `16 * cell_size_px` Map Section box from the image via
///    [`crop_section_box`]. A section that does not fit is unresolvable and returns an error
///    (the caller falls back to the static reference) instead of a scale-changing crop.
/// 2. Resizes to `target_size x target_size` using high-quality Lanczos3 filter.
/// 3. Encodes to JPEG format with quality 85.
pub fn preprocess_section_reference_image(
    raw_bytes: &[u8],
    grid: &GridConfig,
    section: &MapSection,
    target_size: u32,
) -> Result<Vec<u8>, ReferenceProviderError> {
    let img = image::load_from_memory(raw_bytes)?;
    let cropped = crop_section_box(&img, grid, section.col, section.row).ok_or_else(|| {
        ReferenceProviderError::NotFound(format!(
            "16x16 Map Section ({}, {}) does not fit inside the {}x{} ground image",
            section.col,
            section.row,
            img.width(),
            img.height()
        ))
    })?;
    let resized = cropped.resize_exact(target_size, target_size, FilterType::Lanczos3);

    let mut output_bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut output_bytes, 85);
    encoder.encode(
        resized.to_rgb8().as_raw(),
        target_size,
        target_size,
        ExtendedColorType::Rgb8,
    )?;

    Ok(output_bytes)
}

/// Finds the section in `map.sections` whose `(col, row)` is closest to the center
/// of the full-grid lattice `(cols / 2 - 8, rows / 2 - 8)`. If `map.sections` is empty,
/// returns a fallback `(0, 0)` section.
pub fn select_center_most_section(map: &CuratedMap) -> MapSection {
    if map.sections.is_empty() {
        return MapSection::new(0, 0, map.description.clone().unwrap_or_default());
    }

    let (ground_w, ground_h) = map
        .layers
        .iter()
        .find(|l| l.role == LayerRole::Ground)
        .map(|l| l.dimensions)
        .filter(|&(w, h)| w > 0 && h > 0)
        .unwrap_or(map.dimensions);

    let (cols, rows) = if map.grid.cell_size_px > 0 {
        map.grid.lattice_dimensions(ground_w, ground_h)
    } else {
        (0, 0)
    };

    let target_col = (cols as f64 / 2.0) - 8.0;
    let target_row = (rows as f64 / 2.0) - 8.0;

    map.sections
        .iter()
        .min_by(|a, b| {
            let da = (a.col as f64 - target_col).powi(2) + (a.row as f64 - target_row).powi(2);
            let db = (b.col as f64 - target_col).powi(2) + (b.row as f64 - target_row).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
        .unwrap_or_else(|| MapSection::new(0, 0, map.description.clone().unwrap_or_default()))
}

/// Detects the most appropriate theme for a prompt based on keyword frequency.
/// If no keywords match, returns None.
pub fn detect_theme_for_prompt(prompt: &str) -> Option<Theme> {
    let lower = prompt.to_lowercase();

    let theme_keywords: &[(Theme, &[&str])] = &[
        (
            Theme::Dungeon,
            &[
                "dungeon", "crypt", "tomb", "cell", "prison", "vault",
                "catacomb", "sewer", "labyrinth", "corridor", "underground",
                "masonry", "sarcophagus", "sluice",
            ],
        ),
        (
            Theme::Cave,
            &[
                "cave", "cavern", "underdark", "stalactite", "stalagmite",
                "subterranean", "grotto", "bioluminescent", "rough natural stone",
            ],
        ),
        (
            Theme::Tavern,
            &[
                "tavern", "inn", "bar", "pub", "bedroom", "hearth",
                "fireplace", "timber", "wood", "innkeeper", "counter",
            ],
        ),
        (
            Theme::Wilderness,
            &[
                "wilderness", "river", "bridge", "crossing", "stream", "forest",
                "garden", "trees", "road", "path", "swamp", "camp", "nature",
                "waterfall", "cliff", "jungle", "banks", "footbridge",
            ],
        ),
        (
            Theme::Ruins,
            &[
                "ruin", "ruins", "courtyard", "crumbling", "pillars",
                "sandstone", "temple ruins", "flagstones", "ancient",
            ],
        ),
        (
            Theme::Urban,
            &[
                "urban", "city", "street", "alley", "market", "shop",
                "plaza", "dock", "harbor", "town",
            ],
        ),
        (
            Theme::SciFi,
            &[
                "scifi", "sci-fi", "space", "station", "starship", "spaceship",
                "cyberpunk", "hangar", "lab", "laboratory", "terminal", "airlock",
            ],
        ),
        (
            Theme::Building,
            &[
                "building", "mansion", "castle", "keep", "fortress", "palace",
                "manor", "estate", "hall", "great hall", "throne",
            ],
        ),
    ];

    let mut best_theme = None;
    let mut best_score = 0;

    for (theme, keywords) in theme_keywords {
        let mut score = 0;
        if lower.contains(theme.as_str()) {
            score += 3;
        }
        for &kw in *keywords {
            if lower.contains(kw) {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            best_theme = Some(*theme);
        }
    }

    best_theme
}
/// Scores keyword overlap between a prompt and a curated map's unified tags and section description:
/// - +2 per matching tag (legacy themes are folded into tags, so they score identically).
/// - +1 per matching word in the section `Physical Description` (split on non-alphanumeric, length >= 2).
pub fn score_curated_section_for_prompt(
    map: &CuratedMap,
    section: &MapSection,
    prompt: &str,
) -> usize {
    let prompt_lower = prompt.to_lowercase();
    let prompt_words: std::collections::HashSet<&str> = prompt_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|s| s.trim())
        .filter(|s| s.len() >= 2)
        .collect();

    let mut score = 0usize;
    for tag in &map.tags {
        let tag_lower = tag.to_lowercase();
        let matches = prompt_words.contains(tag_lower.as_str()) || prompt_lower.contains(&tag_lower);
        if matches {
            score += 2;
        }
    }
    let desc_lower = section.description.to_lowercase();
    let desc_words: std::collections::HashSet<&str> = desc_lower
        .split(|c: char| !c.is_alphanumeric())
        .map(|s| s.trim())
        .filter(|s| s.len() >= 2)
        .collect();
    for word in desc_words {
        if prompt_words.contains(word) {
            score += 1;
        }
    }
    score
}

/// Service providing visual reference conditioning images for few-shot benchmark generation.
#[derive(Clone)]
pub struct ReferenceProvider {
    storage: Arc<dyn StorageRepository + Send + Sync>,
    static_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    target_size: u32,
}

impl ReferenceProvider {
    /// Creates a new `ReferenceProvider` backed by the given storage repository.
    pub fn new(storage: Arc<dyn StorageRepository + Send + Sync>) -> Self {
        Self {
            storage,
            static_cache: Arc::new(RwLock::new(HashMap::new())),
            target_size: 512,
        }
    }

    /// Sets the target dimension for preprocessed square reference images (default: 512).
    pub fn with_target_size(mut self, target_size: u32) -> Self {
        self.target_size = target_size;
        self
    }

    /// Underlying storage repository reference.
    pub fn storage(&self) -> &Arc<dyn StorageRepository + Send + Sync> {
        &self.storage
    }

    /// Resolves reference image for a given theme:
    /// 1. Queries curated maps in storage for the theme.
    /// 2. If a map is found, selects the most recently curated map.
    /// 3. Otherwise, falls back to the static reference image for the theme.
    pub fn resolve_reference_for_theme(
        &self,
        theme: &Theme,
    ) -> Result<Option<ReferenceResolution>, ReferenceProviderError> {
        // 1. Query curated maps in storage
        let curated_maps = self.storage.list_curated(Some(theme))?;

        // 2. Select top curated map (tie-breaking on curated_at, most recent first)
        let top_curated = curated_maps
            .into_iter()
            .max_by(|a, b| a.curated_at.cmp(&b.curated_at));

        // 3. If found, load bytes and preprocess center-most section
        if let Some(map) = top_curated {
            let section = select_center_most_section(&map);
            match self.storage.get_curated_ground_image_bytes(&map) {
                Ok(raw_bytes) => {
                    match preprocess_section_reference_image(
                        &raw_bytes,
                        &map.grid,
                        &section,
                        self.target_size,
                    ) {
                        Ok(jpeg_bytes) => {
                            let label =
                                format!("Perspective Reference: 2D {} Plan View", theme.as_str());
                            let image = ReferenceImage::jpeg(label, jpeg_bytes);
                            info!(
                                id = %map.id,
                                theme = %theme,
                                col = section.col,
                                row = section.row,
                                "Resolved curated map center section reference image"
                            );
                            return Ok(Some(ReferenceResolution {
                                image,
                                provenance: ReferenceProvenance::Curated {
                                    id: map.id,
                                    theme: *theme,
                                },
                            }));
                        }
                        Err(err) => {
                            warn!(
                                id = %map.id,
                                error = %err,
                                "Failed to preprocess curated section image; falling back to static reference"
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        id = %map.id,
                        error = %err,
                        "Failed to load curated image bytes from storage; falling back to static reference"
                    );
                }
            }
        }

        // 4. Static fallback
        let static_def = static_reference_for_theme(theme);
        let jpeg_bytes = self.get_static_reference_bytes(static_def)?;
        let image = ReferenceImage::jpeg(static_def.label, jpeg_bytes);

        Ok(Some(ReferenceResolution {
            image,
            provenance: ReferenceProvenance::StaticFallback {
                name: static_def.id.to_string(),
                theme: *theme,
            },
        }))
    }

    /// Resolves reference image by scoring (CuratedMap, MapSection) pairs on prompt keyword overlap,
    /// tie-breaking on curated_at (most recent first). Falls back to static reference if none match.
    pub fn resolve_reference_for_prompt(
        &self,
        prompt: &str,
    ) -> Result<Option<ReferenceResolution>, ReferenceProviderError> {
        let curated_maps = self.storage.list_curated(None)?;

        let mut section_candidates: Vec<(usize, CuratedMap, MapSection)> = Vec::new();

        for map in curated_maps {
            if map.sections.is_empty() {
                let default_desc = map.description.clone().unwrap_or_default();
                let section = MapSection::new(0, 0, default_desc);
                let score = score_curated_section_for_prompt(&map, &section, prompt);
                if score > 0 {
                    section_candidates.push((score, map, section));
                }
            } else {
                for section in &map.sections {
                    let score = score_curated_section_for_prompt(&map, section, prompt);
                    if score > 0 {
                        section_candidates.push((score, map.clone(), section.clone()));
                    }
                }
            }
        }

        let top_match =
            section_candidates
                .into_iter()
                .max_by(|(score_a, map_a, _), (score_b, map_b, _)| {
                    score_a
                        .cmp(score_b)
                        .then_with(|| map_a.curated_at.cmp(&map_b.curated_at))
                });

        if let Some((_, map, section)) = top_match {
            let primary_theme = map.theme().unwrap_or(Theme::Dungeon);
            match self.storage.get_curated_ground_image_bytes(&map) {
                Ok(raw_bytes) => {
                    match preprocess_section_reference_image(
                        &raw_bytes,
                        &map.grid,
                        &section,
                        self.target_size,
                    ) {
                        Ok(jpeg_bytes) => {
                            let label = format!(
                                "Perspective Reference: 2D {} Plan View",
                                primary_theme.as_str()
                            );
                            let image = ReferenceImage::jpeg(label, jpeg_bytes);
                            info!(
                                id = %map.id,
                                theme = %primary_theme,
                                col = section.col,
                                row = section.row,
                                "Resolved curated map section reference image for prompt"
                            );
                            return Ok(Some(ReferenceResolution {
                                image,
                                provenance: ReferenceProvenance::Curated {
                                    id: map.id,
                                    theme: primary_theme,
                                },
                            }));
                        }
                        Err(err) => {
                            warn!(
                                id = %map.id,
                                error = %err,
                                "Failed to preprocess curated section image; falling back to static reference"
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        id = %map.id,
                        error = %err,
                        "Failed to load curated image bytes from storage; falling back to static reference"
                    );
                }
            }
        }

        // Fall back to static reference if theme keywords detected in prompt
        if let Some(theme) = detect_theme_for_prompt(prompt) {
            let static_def = static_reference_for_theme(&theme);
            let jpeg_bytes = self.get_static_reference_bytes(static_def)?;
            let image = ReferenceImage::jpeg(static_def.label, jpeg_bytes);
            Ok(Some(ReferenceResolution {
                image,
                provenance: ReferenceProvenance::StaticFallback {
                    name: static_def.id.to_string(),
                    theme,
                },
            }))
        } else {
            Ok(None)
        }
    }

    /// Retrieves preprocessed bytes for a static reference definition with in-memory caching.
    fn get_static_reference_bytes(
        &self,
        def: &StaticReferenceDef,
    ) -> Result<Vec<u8>, ReferenceProviderError> {
        let cache_key = format!("{}-{}", def.id, self.target_size);

        if let Ok(cache) = self.static_cache.read() {
            if let Some(bytes) = cache.get(&cache_key) {
                return Ok(bytes.clone());
            }
        }

        let raw_bytes = self
            .load_static_file(def.filename)
            .unwrap_or_else(|| def.embedded_bytes.to_vec());

        let processed = preprocess_reference_image(&raw_bytes, self.target_size)?;

        if let Ok(mut cache) = self.static_cache.write() {
            cache.insert(cache_key, processed.clone());
        }

        Ok(processed)
    }

    /// Attempts to read static reference file from data directory.
    fn load_static_file(&self, filename: &str) -> Option<Vec<u8>> {
        let candidates = [
            PathBuf::from("data").join(filename),
            PathBuf::from("../data").join(filename),
            PathBuf::from("../../data").join(filename),
            PathBuf::from("server/assets/references").join(filename),
            PathBuf::from("../server/assets/references").join(filename),
        ];

        for path in &candidates {
            if path.is_file() {
                if let Ok(bytes) = std::fs::read(path) {
                    return Some(bytes);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::adapters::FilesystemStorage;
    use crate::domain::{CuratedLayer, CuratedMap};
    use image::{ImageBuffer, Rgb};
    use tempfile::TempDir;

    fn create_test_image_bytes(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgb(color));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("image write succeeds");
        bytes
    }

    #[test]
    fn test_preprocess_reference_image_crops_and_resizes() {
        // Create 200x100 non-square image
        let raw_png = create_test_image_bytes(200, 100, [255, 0, 0]);
        let preprocessed = preprocess_reference_image(&raw_png, 512).expect("preprocess succeeds");

        // Verify decoded output is 512x512
        let decoded = image::load_from_memory(&preprocessed).expect("decode preprocessed JPEG");
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
    }

    #[test]
    fn test_detect_theme_for_prompt() {
        assert_eq!(
            detect_theme_for_prompt("A dark stone dungeon with crypt and sarcophagus"),
            Some(Theme::Dungeon)
        );
        assert_eq!(
            detect_theme_for_prompt("Underdark cavern with bioluminescent mushrooms"),
            Some(Theme::Cave)
        );
        assert_eq!(
            detect_theme_for_prompt("Warm fantasy tavern interior with hearth and bar"),
            Some(Theme::Tavern)
        );
        assert_eq!(
            detect_theme_for_prompt("Forest river crossing with wooden bridge"),
            Some(Theme::Wilderness)
        );
        assert_eq!(
            detect_theme_for_prompt("Ancient temple ruins with sandstone pillars"),
            Some(Theme::Ruins)
        );
        assert_eq!(
            detect_theme_for_prompt("Cyberpunk space station airlock and hangar"),
            Some(Theme::SciFi)
        );
        assert_eq!(
            detect_theme_for_prompt("Cobblestone city street in urban market"),
            Some(Theme::Urban)
        );
        assert_eq!(
            detect_theme_for_prompt("Grand castle keep with fortress throne hall"),
            Some(Theme::Building)
        );
        assert_eq!(detect_theme_for_prompt("completely unrelated text 12345"), None);
    }

    #[test]
    fn test_static_fallback_when_storage_empty() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage);

        let resolution = provider
            .resolve_reference_for_theme(&Theme::Dungeon)
            .expect("resolution succeeds")
            .expect("reference exists");

        match resolution.provenance {
            ReferenceProvenance::StaticFallback { name, theme } => {
                assert_eq!(name, "stone-dungeon");
                assert_eq!(theme, Theme::Dungeon);
            }
            _ => panic!("Expected StaticFallback"),
        }

        assert_eq!(resolution.image.mime_type, "image/jpeg");
        assert_eq!(
            resolution.image.label,
            "Perspective Reference: 2D Stone Dungeon Plan View"
        );
        assert!(!resolution.image.data.is_empty());

        // Verify decoded size is 512x512
        let decoded = image::load_from_memory(&resolution.image.data).expect("valid jpeg");
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
    }

    #[test]
    fn test_curated_map_recency_prioritization() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        let older_time = Utc::now() - chrono::Duration::hours(2);
        let newer_time = Utc::now();

        let map_older = CuratedMap::new(
            "curated-dungeon-older",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (1024, 1024),
            older_time,
        );
        let img_bytes = create_test_image_bytes(1024, 1024, [0, 255, 0]);
        storage.save_curated(&map_older, &img_bytes).unwrap();

        let map_newer = CuratedMap::new(
            "curated-dungeon-newer",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (1024, 1024),
            newer_time,
        );
        storage.save_curated(&map_newer, &img_bytes).unwrap();

        let res = provider
            .resolve_reference_for_theme(&Theme::Dungeon)
            .unwrap()
            .unwrap();
        assert_eq!(
            res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-dungeon-newer".to_string(),
                theme: Theme::Dungeon,
            }
        );
    }

    #[test]
    fn test_prompt_keyword_overlap_prioritization() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        // Map A: 1 overlap with prompt ("dungeon")
        let map_a = CuratedMap::new(
            "curated-a",
            vec!["dungeon".to_string()],
            vec!["corridor".to_string()],
            (1024, 1024),
            Utc::now() - chrono::Duration::hours(1),
        );
        let img_bytes = create_test_image_bytes(1024, 1024, [0, 0, 255]);
        storage.save_curated(&map_a, &img_bytes).unwrap();

        // Map B: 3 overlaps with prompt ("dungeon", "crypt", "sarcophagus")
        let map_b = CuratedMap::new(
            "curated-b",
            vec!["dungeon".to_string(), "crypt".to_string()],
            vec!["sarcophagus".to_string(), "torches".to_string()],
            (1024, 1024),
            Utc::now() - chrono::Duration::hours(2),
        );
        storage.save_curated(&map_b, &img_bytes).unwrap();

        let prompt = "Stone dungeon crypt with central sarcophagus and iron gate";
        let res = provider.resolve_reference_for_prompt(prompt).unwrap().unwrap();
        assert_eq!(
            res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-b".to_string(),
                theme: Theme::Dungeon,
            }
        );
    }

    #[test]
    fn test_curated_map_unmatched_prompt_falls_back_to_static() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        // Storage only has dungeon maps
        let map_dungeon = CuratedMap::new(
            "curated-dungeon-only",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (1024, 1024),
            Utc::now(),
        );
        let img_bytes = create_test_image_bytes(1024, 1024, [100, 100, 100]);
        storage.save_curated(&map_dungeon, &img_bytes).unwrap();

        // Prompt is about wilderness
        let res = provider
            .resolve_reference_for_prompt("Forest river crossing with wooden bridge")
            .unwrap()
            .unwrap();

        assert_eq!(
            res.provenance,
            ReferenceProvenance::StaticFallback {
                name: "ruins-courtyard".to_string(),
                theme: Theme::Wilderness,
            }
        );
    }

    #[test]
    fn test_score_curated_section_for_prompt_includes_description() {
        let map = CuratedMap::new(
            "map-no-desc",
            vec!["dungeon".to_string()],
            vec!["stone".to_string()],
            (1024, 1024),
            Utc::now(),
        );

        let prompt = "Stone dungeon crypt with carved limestone sarcophagus and burning braziers";
        let undescribed_section = MapSection::new(0, 0, "");
        let base_score = score_curated_section_for_prompt(&map, &undescribed_section, prompt);
        // Unified tags: "dungeon" tag (+2) + "stone" tag (+2) = 4
        assert_eq!(base_score, 4);

        let described_section = MapSection::new(
            0,
            0,
            "Ancient crypt with carved limestone sarcophagus, burning braziers, and iron gates.",
        );
        let score_with_desc = score_curated_section_for_prompt(&map, &described_section, prompt);
        assert!(score_with_desc > base_score);
        // base_score (4) + 8 overlapping words ("crypt", "with", "carved", "limestone", "sarcophagus", "and", "burning", "braziers")
        assert_eq!(score_with_desc, 12);
    }

    #[test]
    fn test_score_curated_section_for_prompt_unified_tags_score_equally() {
        let map = CuratedMap::new(
            "map-unified-tags",
            vec!["dungeon".to_string()],
            vec!["cavern".to_string(), "crypt".to_string()],
            (1024, 1024),
            Utc::now(),
        );
        let section = MapSection::new(0, 0, "");

        // Each matched tag earns +2 regardless of whether it was initialized via themes or tags
        assert_eq!(
            score_curated_section_for_prompt(&map, &section, "dungeon entrance"),
            2
        );
        assert_eq!(
            score_curated_section_for_prompt(&map, &section, "cavern entrance"),
            2
        );
        assert_eq!(
            score_curated_section_for_prompt(&map, &section, "crypt entrance"),
            2
        );
        assert_eq!(
            score_curated_section_for_prompt(&map, &section, "dungeon cavern crypt"),
            6
        );
    }

    #[test]
    fn test_prompt_keyword_overlap_prioritizes_description_match() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        // Map A: Newer, but only generic theme ("dungeon")
        let map_a = CuratedMap::new(
            "curated-a-generic",
            vec!["dungeon".to_string()],
            vec!["stone".to_string()],
            (1024, 1024),
            Utc::now(),
        );
        let img_bytes = create_test_image_bytes(1024, 1024, [0, 0, 255]);
        storage.save_curated(&map_a, &img_bytes).unwrap();

        // Map B: Older, but rich description matching specific prompt elements ("altar", "obsidian", "braziers")
        let map_b = CuratedMap::new(
            "curated-b-rich-desc",
            vec!["dungeon".to_string()],
            vec!["room".to_string()],
            (1024, 1024),
            Utc::now() - chrono::Duration::hours(5),
        ).with_description("Dark sacrificial chamber with obsidian altar, glowing runes, and twin iron braziers.");
        storage.save_curated(&map_b, &img_bytes).unwrap();

        let prompt = "Dark dungeon with obsidian altar and flaming braziers";
        let res = provider.resolve_reference_for_prompt(prompt).unwrap().unwrap();
        assert_eq!(
            res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-b-rich-desc".to_string(),
                theme: Theme::Dungeon,
            }
        );
    }

    #[test]
    fn test_score_curated_section_for_prompt() {
        let map = CuratedMap::new(
            "map-section-scoring",
            vec!["dungeon".to_string()],
            vec!["stone".to_string(), "crypt".to_string()],
            (1024, 1024),
            Utc::now(),
        );

        let altar_section = MapSection::new(
            0,
            0,
            "Ancient obsidian altar with glowing runes and skull offerings.",
        );
        let bridge_section = MapSection::new(
            16,
            0,
            "Rickety wooden rope bridge spanning a deep chasm with mist.",
        );

        // Prompt focusing on altar elements
        let altar_prompt = "Dark stone dungeon with obsidian altar and skull offerings";
        let altar_score = score_curated_section_for_prompt(&map, &altar_section, altar_prompt);
        let bridge_score_for_altar =
            score_curated_section_for_prompt(&map, &bridge_section, altar_prompt);

        // "dungeon" tag (+2) + "stone" tag (+2) = 4 base score
        // Section words matching: "with", "obsidian", "altar", "and", "skull", "offerings" = 6
        assert_eq!(altar_score, 10);
        // Bridge section matches "dungeon" (+2) and "stone" (+2) tags, plus the word "with" (+1) = 5
        assert_eq!(bridge_score_for_altar, 5);
        assert!(altar_score > bridge_score_for_altar);

        // Prompt focusing on bridge elements
        let bridge_prompt = "Wooden rope bridge spanning deep chasm";
        let bridge_score = score_curated_section_for_prompt(&map, &bridge_section, bridge_prompt);
        let altar_score_for_bridge =
            score_curated_section_for_prompt(&map, &altar_section, bridge_prompt);

        // Bridge section matches "wooden", "rope", "bridge", "spanning", "deep", "chasm" = 6
        assert_eq!(bridge_score, 6);
        // Altar section has 0 theme/tag/word matches with bridge_prompt
        assert_eq!(altar_score_for_bridge, 0);
        assert!(bridge_score > altar_score_for_bridge);
    }

    #[test]
    fn test_select_center_most_section_lattice_center() {
        let grid = GridConfig::new(16, 0, 0);
        // 48x48 cell lattice (768x768 px). Center target is (48/2 - 8, 48/2 - 8) = (16, 16)
        let map = CuratedMap::new(
            "center-map",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (768, 768),
            Utc::now(),
        )
        .with_grid(grid)
        .with_sections(vec![
            MapSection::new(0, 0, "Northwest corner"),
            MapSection::new(16, 16, "Exact center chamber"),
            MapSection::new(32, 32, "Southeast corner"),
        ]);

        let center_section = select_center_most_section(&map);
        assert_eq!(center_section.col, 16);
        assert_eq!(center_section.row, 16);
        assert_eq!(center_section.description, "Exact center chamber");

        // Empty sections fallback to (0, 0)
        let empty_map = CuratedMap::new(
            "empty-sections-map",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (512, 512),
            Utc::now(),
        )
        .with_description("Default map description");

        let fallback_sec = select_center_most_section(&empty_map);
        assert_eq!(fallback_sec.col, 0);
        assert_eq!(fallback_sec.row, 0);
        assert_eq!(fallback_sec.description, "Default map description");
    }

    #[test]
    fn test_crop_section_box_exact_pixel_box() {
        // 512x512 image, 4 quadrants of 256x256:
        // (0, 0): Red [255, 0, 0]
        // (16, 0): Green [0, 255, 0]
        // (0, 16): Blue [0, 0, 255]
        // (16, 16): Yellow [255, 255, 0]
        let width = 512u32;
        let height = 512u32;
        let mut img = image::RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let color = match (x < 256, y < 256) {
                    (true, true) => [255, 0, 0],     // top-left: Red
                    (false, true) => [0, 255, 0],    // top-right: Green
                    (true, false) => [0, 0, 255],    // bottom-left: Blue
                    (false, false) => [255, 255, 0], // bottom-right: Yellow
                };
                img.put_pixel(x, y, Rgb(color));
            }
        }
        let dyn_img = image::DynamicImage::ImageRgb8(img);

        let grid = GridConfig::new(16, 0, 0); // 16 * 16 = 256px per section

        // 1. Top-left section (0, 0)
        let cropped_tl = crop_section_box(&dyn_img, &grid, 0, 0).expect("top-left box fits");
        assert_eq!(cropped_tl.width(), 256);
        assert_eq!(cropped_tl.height(), 256);
        let rgb_tl = cropped_tl.to_rgb8();
        assert_eq!(rgb_tl.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(rgb_tl.get_pixel(255, 255).0, [255, 0, 0]);

        // 2. Top-right section (16, 0)
        let cropped_tr = crop_section_box(&dyn_img, &grid, 16, 0).expect("top-right box fits");
        assert_eq!(cropped_tr.width(), 256);
        assert_eq!(cropped_tr.height(), 256);
        let rgb_tr = cropped_tr.to_rgb8();
        assert_eq!(rgb_tr.get_pixel(0, 0).0, [0, 255, 0]);
        assert_eq!(rgb_tr.get_pixel(255, 255).0, [0, 255, 0]);

        // 3. Bottom-left section (0, 16)
        let cropped_bl = crop_section_box(&dyn_img, &grid, 0, 16).expect("bottom-left box fits");
        assert_eq!(cropped_bl.width(), 256);
        assert_eq!(cropped_bl.height(), 256);
        let rgb_bl = cropped_bl.to_rgb8();
        assert_eq!(rgb_bl.get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(rgb_bl.get_pixel(255, 255).0, [0, 0, 255]);

        // 4. Bottom-right section (16, 16)
        let cropped_br = crop_section_box(&dyn_img, &grid, 16, 16).expect("bottom-right box fits");
        assert_eq!(cropped_br.width(), 256);
        assert_eq!(cropped_br.height(), 256);
        let rgb_br = cropped_br.to_rgb8();
        assert_eq!(rgb_br.get_pixel(0, 0).0, [255, 255, 0]);
        assert_eq!(rgb_br.get_pixel(255, 255).0, [255, 255, 0]);
    }

    #[test]
    fn test_multi_section_map_resolution_altar_vs_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        // Create a 512x256 image with 2 sections (cell_size_px = 16):
        // Section 0 at (col 0, row 0): Red [255, 0, 0] (pixels 0..256 x 0..256)
        // Section 1 at (col 16, row 0): Green [0, 255, 0] (pixels 256..512 x 0..256)
        let mut img = image::RgbImage::new(512, 256);
        for y in 0..256 {
            for x in 0..512 {
                let color = if x < 256 { [255, 0, 0] } else { [0, 255, 0] };
                img.put_pixel(x, y, Rgb(color));
            }
        }
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let grid = GridConfig::new(16, 0, 0);
        let sections = vec![
            MapSection::new(
                0,
                0,
                "Ancient obsidian altar chamber with glowing sacrificial runes",
            ),
            MapSection::new(
                16,
                0,
                "Rickety wooden rope bridge spanning a deep chasm with mist",
            ),
        ];

        let map = CuratedMap::new(
            "curated-multi-section",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (512, 256),
            Utc::now(),
        )
        .with_grid(grid)
        .with_sections(sections);

        storage.save_curated(&map, &png_bytes).unwrap();

        // 1. Resolve for prompt matching the altar section
        let altar_prompt = "Obsidian altar with glowing sacrificial runes in dungeon";
        let altar_res = provider
            .resolve_reference_for_prompt(altar_prompt)
            .unwrap()
            .expect("should resolve altar section");

        assert_eq!(
            altar_res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-multi-section".to_string(),
                theme: Theme::Dungeon,
            }
        );

        let decoded_altar = image::load_from_memory(&altar_res.image.data)
            .unwrap()
            .to_rgb8();
        assert_eq!(decoded_altar.width(), 512);
        assert_eq!(decoded_altar.height(), 512);
        // Dominant color in altar crop should be Red
        let center_pixel = decoded_altar.get_pixel(256, 256);
        assert!(
            center_pixel[0] > 200,
            "Expected red > 200, got {}",
            center_pixel[0]
        );
        assert!(
            center_pixel[1] < 50,
            "Expected green < 50, got {}",
            center_pixel[1]
        );
        assert!(
            center_pixel[2] < 50,
            "Expected blue < 50, got {}",
            center_pixel[2]
        );

        // 2. Resolve for prompt matching the bridge section
        let bridge_prompt = "Rickety wooden rope bridge spanning deep chasm";
        let bridge_res = provider
            .resolve_reference_for_prompt(bridge_prompt)
            .unwrap()
            .expect("should resolve bridge section");

        assert_eq!(
            bridge_res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-multi-section".to_string(),
                theme: Theme::Dungeon,
            }
        );

        let decoded_bridge = image::load_from_memory(&bridge_res.image.data)
            .unwrap()
            .to_rgb8();
        assert_eq!(decoded_bridge.width(), 512);
        assert_eq!(decoded_bridge.height(), 512);
        // Dominant color in bridge crop should be Green
        let center_pixel = decoded_bridge.get_pixel(256, 256);
        assert!(
            center_pixel[1] > 200,
            "Expected green > 200, got {}",
            center_pixel[1]
        );
        assert!(
            center_pixel[0] < 50,
            "Expected red < 50, got {}",
            center_pixel[0]
        );
        assert!(
            center_pixel[2] < 50,
            "Expected blue < 50, got {}",
            center_pixel[2]
        );
    }

    #[test]
    fn test_resolve_reference_for_theme_selects_center_most_section() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
        let provider = ReferenceProvider::new(storage.clone());

        // Create 768x256 image with 3 horizontal sections (48x16 lattice, cell_size_px = 16):
        // Section (0, 0): Red [255, 0, 0] (0..256 x 0..256)
        // Section (16, 0): Green [0, 255, 0] (256..512 x 0..256) -> EXACT CENTER
        // Section (32, 0): Blue [0, 0, 255] (512..768 x 0..256)
        let mut img = image::RgbImage::new(768, 256);
        for y in 0..256 {
            for x in 0..768 {
                let color = if x < 256 {
                    [255, 0, 0]
                } else if x < 512 {
                    [0, 255, 0]
                } else {
                    [0, 0, 255]
                };
                img.put_pixel(x, y, Rgb(color));
            }
        }
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let grid = GridConfig::new(16, 0, 0);
        let sections = vec![
            MapSection::new(0, 0, "West corridor"),
            MapSection::new(16, 0, "Center sanctuary"),
            MapSection::new(32, 0, "East exit"),
        ];

        let map = CuratedMap::new(
            "curated-center-theme",
            Theme::Cave,
            vec!["cave".to_string()],
            (768, 256),
            Utc::now(),
        )
        .with_grid(grid)
        .with_sections(sections);

        storage.save_curated(&map, &png_bytes).unwrap();

        let res = provider
            .resolve_reference_for_theme(&Theme::Cave)
            .unwrap()
            .expect("should resolve center-most section");

        assert_eq!(
            res.provenance,
            ReferenceProvenance::Curated {
                id: "curated-center-theme".to_string(),
                theme: Theme::Cave,
            }
        );

        let decoded = image::load_from_memory(&res.image.data).unwrap().to_rgb8();
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
        // Center-most section is Green
        let center_pixel = decoded.get_pixel(256, 256);
        assert!(
            center_pixel[1] > 200,
            "Expected green > 200, got {}",
            center_pixel[1]
        );
        assert!(
            center_pixel[0] < 50,
            "Expected red < 50, got {}",
            center_pixel[0]
        );
        assert!(
            center_pixel[2] < 50,
            "Expected blue < 50, got {}",
            center_pixel[2]
        );
    }

    #[test]
    fn test_select_center_most_section_uses_ground_layer_dimensions() {
        // `map.dimensions` (1024x1024 -> 16x16 lattice, centre target (0, 0)) disagrees with the
        // Ground layer dimensions the sections were generated against (3072x3072 -> 48x48
        // lattice, centre target (16, 16)). The Ground layer must win.
        let ground_layer = CuratedLayer::new(1, "1.png", LayerRole::Ground, None, (3072, 3072));
        let map = CuratedMap::new(
            "ground-dims-map",
            Theme::Dungeon,
            vec!["dungeon".to_string()],
            (1024, 1024),
            Utc::now(),
        )
        .with_grid(GridConfig::new(64, 0, 0))
        .with_layers(vec![ground_layer])
        .with_sections(vec![
            MapSection::new(0, 0, "Northwest corner"),
            MapSection::new(16, 16, "Exact center chamber"),
            MapSection::new(32, 32, "Southeast corner"),
        ]);

        let center_section = select_center_most_section(&map);
        assert_eq!(center_section.col, 16);
        assert_eq!(center_section.row, 16);
        assert_eq!(center_section.description, "Exact center chamber");
    }

    #[test]
    fn test_crop_section_box_out_of_bounds_is_unresolved() {
        // 512x300 image: the exact 256x256 box for section (0, 16) starts at y = 256, leaving
        // only 44px below it. The box does not fit, so the section is unresolvable -- no
        // magnified, center-cropped, or otherwise scale-changing fallback crop is produced.
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(512, 300));
        let grid = GridConfig::new(16, 0, 0); // 16 * 16 = 256px per section

        assert!(crop_section_box(&img, &grid, 0, 16).is_none());
        // In-bounds boxes still resolve to the exact 256x256 box.
        let cropped = crop_section_box(&img, &grid, 0, 0).expect("top-left box fits");
        assert_eq!((cropped.width(), cropped.height()), (256, 256));

        // The reference provider treats an unresolvable section as unresolved (error), which
        // callers answer with the static fallback reference.
        let section = MapSection::new(0, 16, "Overflowing section");
        let raw_bytes = create_test_image_bytes(512, 300, [255, 0, 0]);
        assert!(matches!(
            preprocess_section_reference_image(&raw_bytes, &grid, &section, 512),
            Err(ReferenceProviderError::NotFound(_))
        ));
    }
}
