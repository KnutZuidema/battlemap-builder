use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Thematic classification for battlemaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[serde(alias = "Dungeon")]
    Dungeon,
    #[serde(alias = "Cave")]
    Cave,
    #[serde(alias = "Tavern")]
    Tavern,
    #[serde(alias = "Wilderness")]
    Wilderness,
    #[serde(alias = "Ruins")]
    Ruins,
    #[serde(alias = "Urban")]
    Urban,
    #[serde(alias = "SciFi", alias = "scifi", alias = "sci-fi", alias = "Sci-Fi")]
    SciFi,
    #[serde(
        alias = "Building",
        alias = "building",
        alias = "Structure",
        alias = "structure",
        alias = "Outpost",
        alias = "outpost",
        alias = "Homestead",
        alias = "homestead",
        alias = "Cabin",
        alias = "cabin",
        alias = "Cottage",
        alias = "cottage"
    )]
    Building,
}

impl Theme {
    /// All theme variants.
    pub const ALL: &'static [Theme] = &[
        Theme::Dungeon,
        Theme::Cave,
        Theme::Tavern,
        Theme::Wilderness,
        Theme::Ruins,
        Theme::Urban,
        Theme::SciFi,
        Theme::Building,
    ];

    /// Lowercase string representation of the theme.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dungeon => "dungeon",
            Self::Cave => "cave",
            Self::Tavern => "tavern",
            Self::Wilderness => "wilderness",
            Self::Ruins => "ruins",
            Self::Urban => "urban",
            Self::SciFi => "scifi",
            Self::Building => "building",
        }
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Theme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace(['-', '_'], "").as_str() {
            "dungeon" => Ok(Self::Dungeon),
            "cave" => Ok(Self::Cave),
            "tavern" => Ok(Self::Tavern),
            "wilderness" => Ok(Self::Wilderness),
            "ruins" => Ok(Self::Ruins),
            "urban" => Ok(Self::Urban),
            "scifi" => Ok(Self::SciFi),
            "building" | "structure" | "outpost" | "homestead" | "cabin" | "cottage" => {
                Ok(Self::Building)
            }
            _ => Err(format!("Unknown theme: {s}")),
        }
    }
}

/// Provenance source of a candidate battlemap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[serde(alias = "Reddit")]
    Reddit,
    #[serde(alias = "Generated")]
    Generated,
    #[serde(alias = "Manual")]
    Manual,
}

impl Source {
    /// Lowercase string representation of the source.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reddit => "reddit",
            Self::Generated => "generated",
            Self::Manual => "manual",
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "reddit" => Ok(Self::Reddit),
            "generated" => Ok(Self::Generated),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("Unknown source: {s}")),
        }
    }
}

/// Reason for rejecting a candidate map from curation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RejectionReason {
    #[serde(alias = "grid_burned_in", alias = "grid-burned-in")]
    GridBurnedIn,
    #[serde(alias = "isometric_tilt", alias = "isometric-tilt")]
    IsometricTilt,
    #[serde(alias = "blurry_low_res", alias = "blurry-low-res")]
    BlurryLowRes,
    #[serde(alias = "bad_geometry", alias = "bad-geometry")]
    BadGeometry,
    #[serde(alias = "irrelevant")]
    Irrelevant,
    #[default]
    #[serde(alias = "unspecified")]
    Unspecified,
}

impl RejectionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GridBurnedIn => "GridBurnedIn",
            Self::IsometricTilt => "IsometricTilt",
            Self::BlurryLowRes => "BlurryLowRes",
            Self::BadGeometry => "BadGeometry",
            Self::Irrelevant => "Irrelevant",
            Self::Unspecified => "Unspecified",
        }
    }
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for RejectionReason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace(['-', '_'], "").as_str() {
            "gridburnedin" => Ok(Self::GridBurnedIn),
            "isometrictilt" => Ok(Self::IsometricTilt),
            "blurrylowres" => Ok(Self::BlurryLowRes),
            "badgeometry" => Ok(Self::BadGeometry),
            "irrelevant" => Ok(Self::Irrelevant),
            "unspecified" => Ok(Self::Unspecified),
            _ => Err(format!("Unknown rejection reason: {s}")),
        }
    }
}


/// Role of an individual image layer within a battlemap bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayerRole {
    #[default]
    #[serde(alias = "Ground", alias = "GROUND")]
    Ground,
    #[serde(
        alias = "UpperFloor",
        alias = "UPPER_FLOOR",
        alias = "upper",
        alias = "Upper",
        alias = "upperfloor",
        alias = "upper-floor"
    )]
    UpperFloor,
    #[serde(
        alias = "Basement",
        alias = "BASEMENT",
        alias = "lower",
        alias = "Lower",
        alias = "lower_floor",
        alias = "lower-floor",
        alias = "lowerfloor",
        alias = "cellar",
        alias = "Cellar"
    )]
    Basement,
    #[serde(alias = "Roof", alias = "ROOF")]
    Roof,
    #[serde(alias = "Variant", alias = "VARIANT")]
    Variant,
    #[serde(
        alias = "Exclude",
        alias = "EXCLUDE",
        alias = "excluded",
        alias = "Excluded"
    )]
    Exclude,
    #[serde(alias = "Unassigned", alias = "UNASSIGNED")]
    Unassigned,
}

impl LayerRole {
    /// Canonical lowercase snake_case string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ground => "ground",
            Self::UpperFloor => "upper_floor",
            Self::Basement => "basement",
            Self::Roof => "roof",
            Self::Variant => "variant",
            Self::Exclude => "exclude",
            Self::Unassigned => "unassigned",
        }
    }

    /// True if this layer is excluded from curation.
    pub fn is_excluded(&self) -> bool {
        matches!(self, Self::Exclude)
    }

    /// True if this layer is still unassigned and requires manual resolution.
    pub fn is_unassigned(&self) -> bool {
        matches!(self, Self::Unassigned)
    }

    /// True if this layer will be included in the curated bundle (`role != Exclude`).
    pub fn is_included(&self) -> bool {
        !self.is_excluded()
    }
}

impl std::fmt::Display for LayerRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for LayerRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().replace(['-', '_'], "").as_str() {
            "ground" => Ok(Self::Ground),
            "upperfloor" | "upper" => Ok(Self::UpperFloor),
            "basement" | "lower" | "lowerfloor" | "cellar" => Ok(Self::Basement),
            "roof" => Ok(Self::Roof),
            "variant" => Ok(Self::Variant),
            "exclude" | "excluded" => Ok(Self::Exclude),
            "unassigned" => Ok(Self::Unassigned),
            _ => Err(format!("Unknown layer role: {s}")),
        }
    }
}

/// Metadata for a single candidate layer in a staged map bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateLayer {
    pub index: usize,
    pub filename: String,
    pub role: LayerRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<i8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default)]
    pub dimensions: (u32, u32),
}

impl CandidateLayer {
    pub fn new(
        index: usize,
        filename: impl Into<String>,
        role: LayerRole,
        elevation: Option<i8>,
        dimensions: (u32, u32),
    ) -> Self {
        Self {
            index,
            filename: filename.into(),
            role,
            elevation,
            caption: None,
            source_url: None,
            dimensions,
        }
    }

    pub fn with_caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    pub fn with_source_url(mut self, source_url: impl Into<String>) -> Self {
        self.source_url = Some(source_url.into());
        self
    }
}

/// Persisted metadata per approved layer in a curated map bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratedLayer {
    pub index: usize,
    pub filename: String,
    pub role: LayerRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<i8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default)]
    pub dimensions: (u32, u32),
}

impl CuratedLayer {
    pub fn new(
        index: usize,
        filename: impl Into<String>,
        role: LayerRole,
        elevation: Option<i8>,
        dimensions: (u32, u32),
    ) -> Self {
        Self {
            index,
            filename: filename.into(),
            role,
            elevation,
            caption: None,
            dimensions,
        }
    }

    pub fn with_caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Deterministically formats the curated layer filename for a given curated index, role, elevation, and extension.
    /// E.g. `0_ground.png`, `1_upper_1.png`, `2_basement_1.png`, `3_roof.png`, `4_variant.png`.
    pub fn format_filename(
        curated_index: usize,
        role: LayerRole,
        elevation: Option<i8>,
        ext: &str,
    ) -> String {
        let clean_ext = ext.trim_start_matches('.');
        let ext_part = if clean_ext.is_empty() { "png" } else { clean_ext };
        let slug = match role {
            LayerRole::Ground => "ground".to_string(),
            LayerRole::UpperFloor => {
                let lvl = elevation.unwrap_or(1).unsigned_abs().max(1);
                format!("upper_{lvl}")
            }
            LayerRole::Basement => {
                let lvl = elevation.unwrap_or(-1).unsigned_abs().max(1);
                format!("basement_{lvl}")
            }
            LayerRole::Roof => "roof".to_string(),
            LayerRole::Variant => "variant".to_string(),
            LayerRole::Exclude => "exclude".to_string(),
            LayerRole::Unassigned => "unassigned".to_string(),
        };
        format!("{curated_index}_{slug}.{ext_part}")
    }
}

/// Validates that a slice of `(LayerRole, Option<i8>)` layer assignments is ready for curation:
/// - At least 1 layer must be included (`role != LayerRole::Exclude`).
/// - Zero included layers may have `role == LayerRole::Unassigned`.
pub fn validate_layers_for_curation(layers: &[(LayerRole, Option<i8>)]) -> Result<(), String> {
    let included: Vec<&(LayerRole, Option<i8>)> =
        layers.iter().filter(|(r, _)| r.is_included()).collect();
    if included.is_empty() {
        return Err("At least one layer must be included (not excluded) to curate a bundle".into());
    }
    if included.iter().any(|(r, _)| r.is_unassigned()) {
        return Err(
            "All included layers must have an assigned role before curation (none may remain unassigned)".into(),
        );
    }
    Ok(())
}
/// Persistent record of a rejected candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionRecord {
    pub id: String,
    pub reason: RejectionReason,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub notes: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl RejectionRecord {
    pub fn new(
        id: impl Into<String>,
        reason: RejectionReason,
        notes: Option<impl Into<String>>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            reason,
            notes: notes.map(Into::into),
            timestamp,
        }
    }

    pub fn with_now(
        id: impl Into<String>,
        reason: RejectionReason,
        notes: Option<impl Into<String>>,
    ) -> Self {
        Self::new(id, reason, notes, Utc::now())
    }
}

/// An uncurated map candidate residing in the staging area.
/// Configuration for battlemap grid alignment and cell dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GridConfig {
    pub cell_size_px: u32,
    pub offset_x: u32,
    pub offset_y: u32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_size_px: 64,
            offset_x: 0,
            offset_y: 0,
        }
    }
}

impl GridConfig {
    pub const MIN_CELL_SIZE: u32 = 16;
    pub const MAX_CELL_SIZE: u32 = 300;
    pub const DEFAULT_CELL_SIZE: u32 = 64;

    pub fn new(cell_size_px: u32, offset_x: u32, offset_y: u32) -> Self {
        let clamped_size = cell_size_px.clamp(Self::MIN_CELL_SIZE, Self::MAX_CELL_SIZE);
        let ox = offset_x % clamped_size;
        let oy = offset_y % clamped_size;
        Self {
            cell_size_px: clamped_size,
            offset_x: ox,
            offset_y: oy,
        }
    }

    pub fn with_signed_offsets(cell_size_px: u32, offset_x: i64, offset_y: i64) -> Self {
        let clamped_size = cell_size_px.clamp(Self::MIN_CELL_SIZE, Self::MAX_CELL_SIZE);
        let size_i64 = clamped_size as i64;
        let ox = offset_x.rem_euclid(size_i64) as u32;
        let oy = offset_y.rem_euclid(size_i64) as u32;
        Self {
            cell_size_px: clamped_size,
            offset_x: ox,
            offset_y: oy,
        }
    }

    pub fn auto_seed(parsed_dims: (u32, u32), image_dims: (u32, u32)) -> Self {
        let (cols, rows) = parsed_dims;
        let (width, height) = image_dims;
        if cols == 0 || rows == 0 || width == 0 || height == 0 {
            return Self::default();
        }

        let img_ratio = width as f64 / height as f64;
        let grid_ratio_normal = cols as f64 / rows as f64;
        let grid_ratio_swapped = rows as f64 / cols as f64;

        let matches_normal = (img_ratio - grid_ratio_normal).abs() / img_ratio <= 0.05;
        let matches_swapped = (img_ratio - grid_ratio_swapped).abs() / img_ratio <= 0.05;

        if matches_normal {
            let cell_w = width as f64 / cols as f64;
            let cell_h = height as f64 / rows as f64;
            let avg_cell = ((cell_w + cell_h) / 2.0).round() as u32;
            Self::new(avg_cell, 0, 0)
        } else if matches_swapped {
            let cell_w = width as f64 / rows as f64;
            let cell_h = height as f64 / cols as f64;
            let avg_cell = ((cell_w + cell_h) / 2.0).round() as u32;
            Self::new(avg_cell, 0, 0)
        } else {
            Self::default()
        }
    }

    /// Calculates the number of full grid columns and rows that fit into the given dimensions.
    /// Edge remnants smaller than `cell_size_px` are discarded.
    pub fn lattice_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        if self.cell_size_px == 0 {
            return (0, 0);
        }
        let cols = width.saturating_sub(self.offset_x) / self.cell_size_px;
        let rows = height.saturating_sub(self.offset_y) / self.cell_size_px;
        (cols, rows)
    }

    /// Calculates the number of 16×16 section windows along X and Y axes needed to cover
    /// the full-grid lattice. Returns None if the grid yields fewer than 16 full cells in either dimension.
    pub fn section_counts(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        let (cols, rows) = self.lattice_dimensions(width, height);
        if cols < 16 || rows < 16 {
            None
        } else {
            Some((cols.div_ceil(16), rows.div_ceil(16)))
        }
    }

    /// Computes the zero-indexed top-left grid cell coordinate origins `(col, row)`
    /// for overlapping 16×16 Map Sections evenly spaced across the full grid lattice.
    pub fn section_origins(&self, width: u32, height: u32) -> Vec<(u32, u32)> {
        let Some((kx, ky)) = self.section_counts(width, height) else {
            return Vec::new();
        };
        let (cols, rows) = self.lattice_dimensions(width, height);
        let col_origins: Vec<u32> = if kx == 1 {
            vec![0]
        } else {
            (0..kx)
                .map(|i| ((i as f64 * (cols - 16) as f64) / (kx - 1) as f64).round() as u32)
                .collect()
        };
        let row_origins: Vec<u32> = if ky == 1 {
            vec![0]
        } else {
            (0..ky)
                .map(|j| ((j as f64 * (rows - 16) as f64) / (ky - 1) as f64).round() as u32)
                .collect()
        };
        let mut origins = Vec::with_capacity((kx * ky) as usize);
        for &r in &row_origins {
            for &c in &col_origins {
                origins.push((c, r));
            }
        }
        origins
    }

    /// Whether the candidate meets the minimum 16×16 full grid cells threshold required for curation.
    pub fn is_curation_eligible(&self, width: u32, height: u32) -> bool {
        self.section_counts(width, height).is_some()
    }
}

/// Crops the exact `16 × 16` Grid Cell Map Section box at `(col, row)` from `img`, aligned to
/// `grid` (`x = offset_x + col * cell_size_px`, `y = offset_y + row * cell_size_px`, side length
/// `16 * cell_size_px`).
///
/// Returns `None` when the box does not fit inside the image, so callers never rescale, magnify,
/// or center-crop a partial box: an unresolvable section is skipped instead of distorting scale.
pub fn crop_section_box(
    img: &image::DynamicImage,
    grid: &GridConfig,
    col: u32,
    row: u32,
) -> Option<image::DynamicImage> {
    let cell_size = if grid.cell_size_px > 0 {
        grid.cell_size_px
    } else {
        GridConfig::DEFAULT_CELL_SIZE
    };
    let box_size = 16 * cell_size;
    let x = grid.offset_x.saturating_add(col.saturating_mul(cell_size));
    let y = grid.offset_y.saturating_add(row.saturating_mul(cell_size));

    if x < img.width() && y < img.height() && box_size <= img.width() - x && box_size <= img.height() - y
    {
        Some(img.crop_imm(x, y, box_size, box_size))
    } else {
        None
    }
}

/// Builds the Map Section list for a curated map from its grid section origins.
///
/// Policy: an empty origin list yields no sections; a single origin yields one `(0, 0)` Map
/// Section reusing `single_description`; multiple origins yield one Map Section per origin, each
/// described by `describe_section`. Callers keep vision and caption generation in the closures.
pub fn synthesize_sections<F>(
    origins: &[(u32, u32)],
    single_description: impl FnOnce() -> String,
    mut describe_section: F,
) -> Vec<MapSection>
where
    F: FnMut(u32, u32) -> String,
{
    if origins.is_empty() {
        return Vec::new();
    }
    if origins.len() == 1 {
        return vec![MapSection::new(0, 0, single_description())];
    }

    let mut sections = Vec::with_capacity(origins.len());
    for &(col, row) in origins {
        sections.push(MapSection::new(col, row, describe_section(col, row)));
    }
    sections
}

#[derive(Deserialize)]
struct GridConfigHelper {
    #[serde(default = "default_cell_size_px")]
    cell_size_px: u32,
    #[serde(default)]
    offset_x: i64,
    #[serde(default)]
    offset_y: i64,
}

fn default_cell_size_px() -> u32 {
    64
}

impl<'de> Deserialize<'de> for GridConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = GridConfigHelper::deserialize(deserializer)?;
        Ok(GridConfig::with_signed_offsets(
            helper.cell_size_px,
            helper.offset_x,
            helper.offset_y,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub source: Source,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub dimensions: (u32, u32),
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_gridless: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_dimensions: Option<(u32, u32)>,
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub layers: Vec<CandidateLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_group_id: Option<String>,
}

impl Candidate {
    pub fn new(
        id: impl Into<String>,
        source: Source,
        filename: impl Into<String>,
        dimensions: (u32, u32),
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            source,
            filename: filename.into(),
            dimensions,
            created_at,
            source_url: None,
            title: None,
            author: None,
            parsed_gridless: None,
            parsed_dimensions: None,
            grid: GridConfig::default(),
            layers: Vec::new(),
            parent_group_id: None,
        }
    }

    pub fn with_now(
        id: impl Into<String>,
        source: Source,
        filename: impl Into<String>,
        dimensions: (u32, u32),
    ) -> Self {
        Self::new(id, source, filename, dimensions, Utc::now())
    }

    pub fn with_source_url(mut self, url: impl Into<String>) -> Self {
        self.source_url = Some(url.into());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_parsed_gridless(mut self, gridless: bool) -> Self {
        self.parsed_gridless = Some(gridless);
        self
    }

    pub fn with_parsed_dimensions(mut self, dimensions: (u32, u32)) -> Self {
        self.parsed_dimensions = Some(dimensions);
        self
    }
    pub fn with_grid(mut self, grid: GridConfig) -> Self {
        self.grid = grid;
        self
    }


    pub fn with_layers(mut self, layers: Vec<CandidateLayer>) -> Self {
        self.layers = layers;
        self
    }

    pub fn with_parent_group_id(mut self, parent_group_id: impl Into<String>) -> Self {
        self.parent_group_id = Some(parent_group_id.into());
        self
    }

    /// Returns the assigned filename or fallback to `{id}.png`.
    pub fn resolved_filename(&self) -> String {
        if self.filename.is_empty() {
            format!("{}.png", self.id)
        } else {
            self.filename.clone()
        }
    }

    /// Returns the explicit `layers` if non-empty, or synthesizes a single `Ground` layer
    /// for legacy single-image candidates.
    pub fn resolved_layers(&self) -> Vec<CandidateLayer> {
        if !self.layers.is_empty() {
            return self.layers.clone();
        }
        let ext = std::path::Path::new(&self.filename)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("png");
        vec![CandidateLayer {
            index: 0,
            filename: format!("0.{ext}"),
            role: LayerRole::Ground,
            elevation: Some(0),
            caption: None,
            source_url: self.source_url.clone(),
            dimensions: self.dimensions,
        }]
    }

    /// Validates that this candidate's layers are ready for curation.
    pub fn validate_for_curation(&self) -> Result<(), String> {
        let resolved = self.resolved_layers();
        let pairs: Vec<(LayerRole, Option<i8>)> =
            resolved.iter().map(|l| (l.role, l.elevation)).collect();
        validate_layers_for_curation(&pairs)
    }
}

/// Formats Reddit permalink and direct media URL into a canonical source URL string.
pub fn format_source_url(permalink: &str, direct_url: &str) -> String {
    let p = permalink.trim();
    let d = direct_url.trim();
    if p.is_empty() || p == d {
        d.to_string()
    } else if d.is_empty() {
        p.to_string()
    } else {
        format!("{p} | {d}")
    }
}

/// Companion sidecar metadata for a harvested Reddit battlemap item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedditSidecarMetadata {
    pub id: String,
    pub source: Source,
    pub source_url: String,
    pub title: String,
    pub author: String,
    pub parsed_gridless: bool,
    pub parsed_dimensions: Option<(u32, u32)>,
    pub created_at: DateTime<Utc>,
}

impl RedditSidecarMetadata {
    pub fn to_candidate(&self) -> Candidate {
        self.clone().into()
    }
}

impl From<RedditSidecarMetadata> for Candidate {
    fn from(meta: RedditSidecarMetadata) -> Self {
        Self {
            id: meta.id.clone(),
            source: meta.source,
            filename: format!("{}.png", meta.id),
            dimensions: meta.parsed_dimensions.unwrap_or((0, 0)),
            created_at: meta.created_at,
            source_url: Some(meta.source_url),
            title: Some(meta.title),
            author: Some(meta.author),
            parsed_gridless: Some(meta.parsed_gridless),
            parsed_dimensions: meta.parsed_dimensions,
            grid: GridConfig::default(),
            layers: Vec::new(),
            parent_group_id: None,
        }
    }
}

impl TryFrom<Candidate> for RedditSidecarMetadata {
    type Error = &'static str;

    fn try_from(c: Candidate) -> Result<Self, Self::Error> {
        Ok(Self {
            id: c.id,
            source: c.source,
            source_url: c.source_url.unwrap_or_default(),
            title: c.title.unwrap_or_default(),
            author: c.author.unwrap_or_default(),
            parsed_gridless: c.parsed_gridless.unwrap_or(false),
            parsed_dimensions: c.parsed_dimensions.or_else(|| {
                if c.dimensions != (0, 0) {
                    Some(c.dimensions)
                } else {
                    None
                }
            }),
            created_at: c.created_at,
        })
    }
}

/// Helper trait allowing flexible theme initialization for `CuratedMap`.
pub trait IntoThemes {
    fn into_themes(self) -> Vec<String>;
}

impl<T: Into<String>> IntoThemes for Vec<T> {
    fn into_themes(self) -> Vec<String> {
        self.into_iter().map(Into::into).collect()
    }
}

impl<T: Into<String>, const N: usize> IntoThemes for [T; N] {
    fn into_themes(self) -> Vec<String> {
        self.into_iter().map(Into::into).collect()
    }
}

impl IntoThemes for &[&str] {
    fn into_themes(self) -> Vec<String> {
        self.iter().map(|s| s.to_string()).collect()
    }
}

impl IntoThemes for String {
    fn into_themes(self) -> Vec<String> {
        vec![self]
    }
}

impl IntoThemes for &str {
    fn into_themes(self) -> Vec<String> {
        vec![self.to_string()]
    }
}

impl IntoThemes for Theme {
    fn into_themes(self) -> Vec<String> {
        vec![self.as_str().to_string()]
    }
}

impl IntoThemes for &Theme {
    fn into_themes(self) -> Vec<String> {
        vec![self.as_str().to_string()]
    }
}

/// An overlapping 16×16 Map Section with cached physical description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapSection {
    pub col: u32,
    pub row: u32,
    pub description: String,
}

impl MapSection {
    pub fn new(col: u32, row: u32, description: impl Into<String>) -> Self {
        Self {
            col,
            row,
            description: description.into(),
        }
    }
}

/// An approved battlemap filed into curated storage with dynamic theme labels and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CuratedMap {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub themes: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub dimensions: (u32, u32),
    pub curated_at: DateTime<Utc>,
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub layers: Vec<CuratedLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<MapSection>,
}

#[derive(Deserialize)]
struct CuratedMapHelper {
    id: String,
    #[serde(default)]
    source: Option<Source>,
    #[serde(default)]
    themes: Vec<String>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default)]
    rating: Option<u8>,
    #[serde(default)]
    tags: Vec<String>,
    dimensions: (u32, u32),
    curated_at: DateTime<Utc>,
    #[serde(default)]
    grid: Option<GridConfig>,
    #[serde(default)]
    layers: Vec<CuratedLayer>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    sections: Vec<MapSection>,
}

impl<'de> Deserialize<'de> for CuratedMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = CuratedMapHelper::deserialize(deserializer)?;
        let mut themes = helper.themes;
        if themes.is_empty() {
            if let Some(t) = helper.theme {
                if !t.trim().is_empty() {
                    themes.push(t);
                }
            }
        }
        let _ = helper.rating;
        Ok(CuratedMap {
            id: helper.id,
            source: helper.source,
            themes,
            tags: helper.tags,
            dimensions: helper.dimensions,
            curated_at: helper.curated_at,
            grid: helper.grid.unwrap_or_default(),
            layers: helper.layers,
            description: helper.description,
            sections: helper.sections,
        })
    }
}

impl CuratedMap {
    pub fn new(
        id: impl Into<String>,
        themes: impl IntoThemes,
        tags: Vec<String>,
        dimensions: (u32, u32),
        curated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            source: None,
            themes: themes.into_themes(),
            tags,
            dimensions,
            curated_at,
            grid: GridConfig::default(),
            layers: Vec::new(),
            description: None,
            sections: Vec::new(),
        }
    }

    pub fn with_now(
        id: impl Into<String>,
        themes: impl IntoThemes,
        tags: Vec<String>,
        dimensions: (u32, u32),
    ) -> Self {
        Self::new(id, themes, tags, dimensions, Utc::now())
    }

    pub fn with_layers(mut self, layers: Vec<CuratedLayer>) -> Self {
        self.layers = layers;
        self
    }
    pub fn with_grid(mut self, grid: GridConfig) -> Self {
        self.grid = grid;
        self
    }


    pub fn with_source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        let desc = description.into();
        self.description = if desc.trim().is_empty() { None } else { Some(desc) };
        self
    }

    pub fn with_optional_description(mut self, description: Option<String>) -> Self {
        self.description = description.filter(|s| !s.trim().is_empty());
        self
    }

    pub fn with_sections(mut self, sections: Vec<MapSection>) -> Self {
        self.sections = sections;
        self
    }

    /// Primary (first) theme string, defaulting to "dungeon" if themes is empty.
    pub fn primary_theme(&self) -> &str {
        self.themes.first().map(|s| s.as_str()).unwrap_or("dungeon")
    }

    /// First theme parsed into `Theme` enum if it matches one of the canonical variants.
    pub fn theme(&self) -> Option<Theme> {
        self.themes.iter().find_map(|t| t.parse::<Theme>().ok())
    }

    /// Validates that the map has at least one theme label.
    pub fn validate_themes(&self) -> Result<(), String> {
        if self.themes.is_empty() || self.themes.iter().all(|t| t.trim().is_empty()) {
            return Err("CuratedMap requires at least 1 theme label".into());
        }
        Ok(())
    }

    /// Validates that the quality rating falls within the required 1-5 range (legacy helper).
    pub fn is_valid_rating(rating: u8) -> bool {
        (1..=5).contains(&rating)
    }

    /// Validates that `layers` (when populated) contains at least one included layer and no
    /// `Exclude` or `Unassigned` layers.
    pub fn validate_layers(&self) -> Result<(), String> {
        if self.layers.is_empty() {
            return Ok(());
        }
        if self.layers.iter().any(|l| l.role.is_excluded()) {
            return Err("CuratedMap layers must not include excluded layers".into());
        }
        if self.layers.iter().any(|l| l.role.is_unassigned()) {
            return Err("CuratedMap layers must not include unassigned layers".into());
        }
        Ok(())
    }
}

/// Partial update payload for modifying an existing curated map's metadata and layer assignments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CuratedMapUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub themes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<CuratedLayer>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<MapSection>>,
}

impl CuratedMapUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_themes(mut self, themes: Vec<String>) -> Self {
        self.themes = Some(themes);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_layers(mut self, layers: Vec<CuratedLayer>) -> Self {
        self.layers = Some(layers);
        self
    }
    pub fn with_grid(mut self, grid: GridConfig) -> Self {
        self.grid = Some(grid);
        self
    }

    pub fn with_sections(mut self, sections: Vec<MapSection>) -> Self {
        self.sections = Some(sections);
        self
    }
}

static LAYER_EXCLUDE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(gridded|with\s+grid|hex(?:\s+grid)?|grid\s+version|grid)\b")
        .expect("valid layer exclude regex")
});

static LAYER_ROOF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(roof|rooftop|roofs|canopy|treetop|treetops|exterior(?:\s+roof)?)\b")
        .expect("valid layer roof regex")
});

static LAYER_BASEMENT_EXPLICIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:b(\d+)|(?:basement|cellar|crypt|sub-?level|lower\s+level|lower\s+floor|dungeon\s+level)\s*#?\s*(\d+))\b",
    )
    .expect("valid layer basement explicit regex")
});

static LAYER_BASEMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(basement|cellar|crypt|underdark|sewer|sewers|dungeon\s+level|lower\s+level|lower\s+floor|lower|sub-?level|underground|b\d+)\b",
    )
    .expect("valid layer basement regex")
});

static LAYER_UPPER_EXPLICIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:(2nd|second)\s+(?:floor|level|story|storey)|(3rd|third)\s+(?:floor|level|story|storey)|(4th|fourth)\s+(?:floor|level|story|storey)|(5th|fifth)\s+(?:floor|level|story|storey)|(?:floor|level|f)\s*([2-9])|1st\s+upper(?:\s+floor)?|(2nd|second)\s+upper(?:\s+floor)?)\b",
    )
    .expect("valid layer upper explicit regex")
});

static LAYER_UPPER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(2nd\s+floor|3rd\s+floor|4th\s+floor|5th\s+floor|second\s+floor|third\s+floor|fourth\s+floor|fifth\s+floor|upper\s+floor|upper\s+level|upper|attic|mezzanine|balcony|tower\s+level|floor\s+[2-9]|level\s+[2-9]|f[2-9]|1st\s+upper|2nd\s+upper)\b",
    )
    .expect("valid layer upper regex")
});

static LAYER_GROUND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(ground|ground\s+floor|ground\s+level|1st\s+floor|first\s+floor|1st\s+level|first\s+level|main\s+floor|main\s+level|base|base\s+map|interior|inside|street\s+level|floor\s+1|level\s+1|f1|courtyard)\b",
    )
    .expect("valid layer ground regex")
});

static LAYER_VARIANT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(night|midnight|evening|dusk|dawn|sunset|day|daylight|daytime|rain|rainy|storm|stormy|snow|snowy|winter|autumn|fall|spring|summer|blood|bloody|ruined|destroyed|burning|fire|flooded|empty|unfurnished|prop-?free|no[- ]?props|clean|blank|fog|foggy|mist|misty|astral|fey|feywild|shadow|shadowfell|variant|alt|alternate|patron|seasonal|haunted|massacre|abandoned|lit|unlit)\b",
    )
    .expect("valid layer variant regex")
});

/// Infers a `(LayerRole, Option<i8>)` from a single caption or label string.
/// Returns `None` if no unambiguous role keyword is matched.
/// Note: when `Some((LayerRole::UpperFloor, None))` or `Some((LayerRole::Basement, None))` is returned,
/// the caller (such as `infer_bundle_layer_roles`) can assign sequential floor offsets (`+1, +2...` or `-1, -2...`).
pub fn infer_layer_role_from_text(text: &str) -> Option<(LayerRole, Option<i8>)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Check for gridded exclusion first (ignoring gridless / no-grid / ungridded)
    let without_gridless = GRIDLESS_RE.replace_all(trimmed, " ");
    if LAYER_EXCLUDE_RE.is_match(&without_gridless) {
        return Some((LayerRole::Exclude, None));
    }

    // 2. Check for Roof (before UpperFloor/Ground so "exterior roof" or "upper roof" matches Roof)
    if LAYER_ROOF_RE.is_match(trimmed) {
        return Some((LayerRole::Roof, None));
    }

    // 3. Check for Basement / Lower levels
    if let Some(caps) = LAYER_BASEMENT_EXPLICIT_RE.captures(trimmed) {
        let num_str = caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str());
        if let Some(n) = num_str.and_then(|s| s.parse::<i8>().ok()) {
            let elev = -(n.abs().max(1));
            return Some((LayerRole::Basement, Some(elev)));
        }
    }
    if LAYER_BASEMENT_RE.is_match(trimmed) {
        return Some((LayerRole::Basement, None));
    }

    // 4. Check for Upper Floor levels
    if let Some(caps) = LAYER_UPPER_EXPLICIT_RE.captures(trimmed) {
        if caps.get(1).is_some() {
            // 2nd floor / second floor -> +1
            return Some((LayerRole::UpperFloor, Some(1)));
        }
        if caps.get(2).is_some() || caps.get(6).is_some() {
            // 3rd floor / third floor / 2nd upper -> +2
            return Some((LayerRole::UpperFloor, Some(2)));
        }
        if caps.get(3).is_some() {
            // 4th floor / fourth floor -> +3
            return Some((LayerRole::UpperFloor, Some(3)));
        }
        if caps.get(4).is_some() {
            // 5th floor / fifth floor -> +4
            return Some((LayerRole::UpperFloor, Some(4)));
        }
        if let Some(m) = caps.get(5) {
            if let Ok(floor_num) = m.as_str().parse::<i8>() {
                let offset = (floor_num - 1).max(1);
                return Some((LayerRole::UpperFloor, Some(offset)));
            }
        }
        // "1st upper"
        return Some((LayerRole::UpperFloor, Some(1)));
    }
    if LAYER_UPPER_RE.is_match(trimmed) {
        return Some((LayerRole::UpperFloor, None));
    }

    // 5. Check for Ground floor / Base / Interior
    if LAYER_GROUND_RE.is_match(trimmed) {
        return Some((LayerRole::Ground, Some(0)));
    }

    // 6. Check for Variant (Night, Rain, Unfurnished, etc.)
    if LAYER_VARIANT_RE.is_match(trimmed) {
        return Some((LayerRole::Variant, None));
    }

    None
}

/// Infers `(LayerRole, Option<i8>)` for all layers in a bundle given their captions and optional post title.
/// - Single-image bundles (`captions.len() == 1`) always default `layers[0]` to `(LayerRole::Ground, Some(0))`.
/// - Multi-image bundles (`captions.len() > 1`) parse each layer's caption using regex heuristics,
///   auto-incrementing `UpperFloor` (`+1, +2, ...`) and `Basement` (`-1, -2, ...`) elevations in sequence,
///   and strictly falling back to `(LayerRole::Unassigned, None)` when a caption is missing or ambiguous.
pub fn infer_bundle_layer_roles(
    captions: &[Option<String>],
    _post_title: Option<&str>,
) -> Vec<(LayerRole, Option<i8>)> {
    if captions.is_empty() {
        return Vec::new();
    }
    if captions.len() == 1 {
        return vec![(LayerRole::Ground, Some(0))];
    }

    let mut results = Vec::with_capacity(captions.len());
    let mut next_upper: i8 = 1;
    let mut next_basement: i8 = -1;

    for cap_opt in captions {
        let inferred = cap_opt
            .as_deref()
            .and_then(infer_layer_role_from_text);
        match inferred {
            Some((LayerRole::Ground, _)) => {
                results.push((LayerRole::Ground, Some(0)));
            }
            Some((LayerRole::UpperFloor, Some(explicit_elev))) => {
                let elev = explicit_elev.max(1);
                if elev >= next_upper {
                    next_upper = elev.saturating_add(1);
                }
                results.push((LayerRole::UpperFloor, Some(elev)));
            }
            Some((LayerRole::UpperFloor, None)) => {
                let elev = next_upper;
                next_upper = next_upper.saturating_add(1);
                results.push((LayerRole::UpperFloor, Some(elev)));
            }
            Some((LayerRole::Basement, Some(explicit_elev))) => {
                let elev = if explicit_elev < 0 {
                    explicit_elev
                } else {
                    -(explicit_elev.max(1))
                };
                if elev <= next_basement {
                    next_basement = elev.saturating_sub(1);
                }
                results.push((LayerRole::Basement, Some(elev)));
            }
            Some((LayerRole::Basement, None)) => {
                let elev = next_basement;
                next_basement = next_basement.saturating_sub(1);
                results.push((LayerRole::Basement, Some(elev)));
            }
            Some((LayerRole::Roof, _)) => {
                results.push((LayerRole::Roof, None));
            }
            Some((LayerRole::Variant, _)) => {
                results.push((LayerRole::Variant, None));
            }
            Some((LayerRole::Exclude, _)) => {
                results.push((LayerRole::Exclude, None));
            }
            Some((LayerRole::Unassigned, _)) | None => {
                results.push((LayerRole::Unassigned, None));
            }
        }
    }

    results
}

static GRIDLESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[?(no[- ]?grid|gridless|ungridded)\]?").expect("valid gridless regex")
});

static GRIDDED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\[?\d+\s*[x×]\s*\d+\]?|\[?gridded?\]?)").expect("valid gridded regex")
});

static DIMENSIONS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[?(\d{2,3})\s*[x×]\s*(\d{2,3})\]?").expect("valid dimensions regex")
});

/// Sort order for querying Reddit listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RedditSort {
    #[default]
    #[serde(alias = "hot", alias = "Hot")]
    Hot,
    #[serde(alias = "top", alias = "Top")]
    Top,
    #[serde(alias = "new", alias = "New")]
    New,
}

impl RedditSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hot => "hot",
            Self::Top => "top",
            Self::New => "new",
        }
    }
}

impl std::fmt::Display for RedditSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for RedditSort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hot" => Ok(Self::Hot),
            "top" => Ok(Self::Top),
            "new" => Ok(Self::New),
            _ => Err(format!("Unknown Reddit sort: {s}")),
        }
    }
}

/// Time interval filter for Reddit top listing queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RedditTimeRange {
    #[serde(alias = "hour", alias = "Hour")]
    Hour,
    #[serde(alias = "day", alias = "Day")]
    Day,
    #[serde(alias = "week", alias = "Week")]
    Week,
    #[serde(alias = "month", alias = "Month")]
    Month,
    #[serde(alias = "year", alias = "Year")]
    Year,
    #[default]
    #[serde(alias = "all", alias = "All")]
    All,
}

impl RedditTimeRange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::All => "all",
        }
    }
}

impl std::fmt::Display for RedditTimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for RedditTimeRange {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "year" => Ok(Self::Year),
            "all" => Ok(Self::All),
            _ => Err(format!("Unknown Reddit time range: {s}")),
        }
    }
}

/// Query configuration for crawling Reddit posts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedditHarvestQuery {
    pub subreddit: String,
    pub sort: RedditSort,
    pub time: Option<RedditTimeRange>,
    pub limit: usize,
    pub after: Option<String>,
}

impl Default for RedditHarvestQuery {
    fn default() -> Self {
        Self {
            subreddit: "battlemaps".to_string(),
            sort: RedditSort::Hot,
            time: Some(RedditTimeRange::All),
            limit: 25,
            after: None,
        }
    }
}

impl RedditHarvestQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn battlemaps() -> Self {
        Self::default()
    }

    pub fn with_subreddit(mut self, subreddit: impl Into<String>) -> Self {
        self.subreddit = subreddit.into();
        self
    }

    pub fn with_sort(mut self, sort: RedditSort) -> Self {
        self.sort = sort;
        self
    }

    pub fn with_time(mut self, time: RedditTimeRange) -> Self {
        self.time = Some(time);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.clamp(1, 100);
        self
    }

    pub fn with_after(mut self, after: impl Into<String>) -> Self {
        self.after = Some(after.into());
        self
    }
}

/// Request parameters for harvesting Reddit posts via the UI drawer or API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedditHarvestRequest {
    #[serde(default)]
    pub sort: RedditSort,
    #[serde(default)]
    pub time: Option<RedditTimeRange>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub gridless_only: bool,
    #[serde(default)]
    pub subreddit: Option<String>,
}

impl Default for RedditHarvestRequest {
    fn default() -> Self {
        Self {
            sort: RedditSort::Hot,
            time: Some(RedditTimeRange::All),
            limit: Some(25),
            gridless_only: false,
            subreddit: None,
        }
    }
}

impl RedditHarvestRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sort(mut self, sort: RedditSort) -> Self {
        self.sort = sort;
        self
    }

    pub fn with_time(mut self, time: RedditTimeRange) -> Self {
        self.time = Some(time);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_gridless_only(mut self, gridless_only: bool) -> Self {
        self.gridless_only = gridless_only;
        self
    }

    pub fn with_subreddit(mut self, subreddit: impl Into<String>) -> Self {
        self.subreddit = Some(subreddit.into());
        self
    }

    /// Convert the harvest request into a standard crawler query.
    pub fn to_query(&self) -> RedditHarvestQuery {
        let subreddit = self.subreddit.as_deref().unwrap_or("battlemaps");
        let limit = self.limit.unwrap_or(25).clamp(1, 100);
        let mut query = RedditHarvestQuery::battlemaps()
            .with_subreddit(subreddit)
            .with_sort(self.sort)
            .with_limit(limit);
        if let Some(time) = self.time {
            query = query.with_time(time);
        }
        query
    }
}

/// Grid overlay status inferred from title metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GridStatus {
    #[serde(alias = "Gridless")]
    Gridless,
    #[serde(alias = "Gridded")]
    Gridded,
    #[default]
    #[serde(alias = "Unknown")]
    Unknown,
}

impl GridStatus {
    pub fn is_gridless(&self) -> bool {
        matches!(self, Self::Gridless)
    }

    pub fn is_gridded(&self) -> bool {
        matches!(self, Self::Gridded)
    }
}

impl std::fmt::Display for GridStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gridless => write!(f, "gridless"),
            Self::Gridded => write!(f, "gridded"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Extracted tags and dimensions from post titles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTags {
    pub grid_status: GridStatus,
    pub dimensions: Option<(u32, u32)>,
    pub raw_title: String,
}

impl ParsedTags {
    /// Parse grid status and dimension hints from a title string.
    pub fn parse_title(title: &str) -> Self {
        let grid_status = if GRIDLESS_RE.is_match(title) {
            GridStatus::Gridless
        } else if GRIDDED_RE.is_match(title) {
            GridStatus::Gridded
        } else {
            GridStatus::Unknown
        };

        let dimensions = DIMENSIONS_RE.captures(title).and_then(|caps| {
            let width = caps.get(1)?.as_str().parse::<u32>().ok()?;
            let height = caps.get(2)?.as_str().parse::<u32>().ok()?;
            Some((width, height))
        });

        Self {
            grid_status,
            dimensions,
            raw_title: title.to_string(),
        }
    }

    pub fn is_gridless(&self) -> bool {
        self.grid_status.is_gridless()
    }

    pub fn is_gridded(&self) -> bool {
        self.grid_status.is_gridded()
    }
}

/// A harvestable media image item from a Reddit post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarvestedMedia {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub caption: Option<String>,
}

impl HarvestedMedia {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            width: None,
            height: None,
            caption: None,
        }
    }

    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn with_caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }
}

/// A battlemap post harvested from Reddit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedditPost {
    pub id: String,
    pub name: String,
    pub title: String,
    pub author: String,
    pub permalink: String,
    pub url: String,
    pub score: i32,
    pub created_utc: DateTime<Utc>,
    pub media: Vec<HarvestedMedia>,
    pub tags: ParsedTags,
}

impl RedditPost {
    pub fn has_media(&self) -> bool {
        !self.media.is_empty()
    }

    pub fn primary_image_url(&self) -> Option<&str> {
        self.media.first().map(|m| m.url.as_str())
    }

    pub fn to_candidate(&self, media_index: usize) -> Option<Candidate> {
        let media = self.media.get(media_index)?;
        let filename = format!("{}_{}.png", self.id, media_index);
        let dimensions = match (media.width, media.height) {
            (Some(w), Some(h)) => (w, h),
            _ => self.tags.dimensions.unwrap_or((0, 0)),
        };
        let candidate = Candidate::new(
            format!("{}_{}", self.id, media_index),
            Source::Reddit,
            filename,
            dimensions,
            self.created_utc,
        )
        .with_source_url(format_source_url(&self.permalink, &media.url))
        .with_title(&self.title)
        .with_author(&self.author)
        .with_parsed_gridless(self.tags.is_gridless());

        let candidate = if let Some(dims) = self.tags.dimensions {
            candidate.with_parsed_dimensions(dims)
        } else {
            candidate
        };

        Some(candidate)
    }

    pub fn to_sidecar_metadata(&self, media_index: usize) -> Option<RedditSidecarMetadata> {
        let media = self.media.get(media_index)?;
        let id = if self.media.len() <= 1 {
            self.id.clone()
        } else {
            format!("{}_{}", self.id, media_index)
        };
        Some(RedditSidecarMetadata {
            id,
            source: Source::Reddit,
            source_url: format_source_url(&self.permalink, &media.url),
            title: self.title.clone(),
            author: self.author.clone(),
            parsed_gridless: self.tags.is_gridless(),
            parsed_dimensions: self.tags.dimensions,
            created_at: Utc::now(),
        })
    }

    pub fn to_bundle_candidate(&self) -> Candidate {
        let primary_media = self.media.first();
        let primary_dimensions = match primary_media.and_then(|m| match (m.width, m.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
            _ => None,
        }) {
            Some(dims) => dims,
            None => self.tags.dimensions.unwrap_or((0, 0)),
        };

        let captions: Vec<Option<String>> = self.media.iter().map(|m| m.caption.clone()).collect();
        let roles = infer_bundle_layer_roles(&captions, Some(&self.title));

        let layers: Vec<CandidateLayer> = self
            .media
            .iter()
            .zip(roles)
            .enumerate()
            .map(|(idx, (media, (role, elevation)))| {
                let layer_dims = match (media.width, media.height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
                    _ => primary_dimensions,
                };
                let mut layer = CandidateLayer::new(
                    idx,
                    format!("{idx}.png"),
                    role,
                    elevation,
                    layer_dims,
                );
                if let Some(caption) = &media.caption {
                    layer = layer.with_caption(caption.clone());
                }
                layer.with_source_url(format_source_url(&self.permalink, &media.url))
            })
            .collect();

        let source_url = match self.media.first() {
            Some(first) => format_source_url(&self.permalink, &first.url),
            None => self.permalink.clone(),
        };

        let mut candidate = Candidate::new(
            self.id.clone(),
            Source::Reddit,
            format!("{}.png", self.id),
            primary_dimensions,
            self.created_utc,
        )
        .with_source_url(source_url)
        .with_title(&self.title)
        .with_author(&self.author)
        .with_parsed_gridless(self.tags.is_gridless());

        if let Some(dims) = self.tags.dimensions {
            candidate = candidate.with_parsed_dimensions(dims);
        }
        candidate.layers = layers;
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_theme_string_conversions() {
        assert_eq!(Theme::Dungeon.as_str(), "dungeon");
        assert_eq!(Theme::SciFi.as_str(), "scifi");
        assert_eq!(Theme::from_str("Dungeon").unwrap(), Theme::Dungeon);
        assert_eq!(Theme::from_str("dungeon").unwrap(), Theme::Dungeon);
        assert_eq!(Theme::from_str("sci-fi").unwrap(), Theme::SciFi);
        assert_eq!(Theme::from_str("SciFi").unwrap(), Theme::SciFi);
        assert_eq!(Theme::from_str("scifi").unwrap(), Theme::SciFi);
        assert_eq!(Theme::Building.as_str(), "building");
        assert_eq!(Theme::from_str("Building").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("building").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("structure").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("outpost").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("homestead").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("cabin").unwrap(), Theme::Building);
        assert_eq!(Theme::from_str("cottage").unwrap(), Theme::Building);
        assert!(Theme::from_str("unknown").is_err());
    }

    #[test]
    fn test_theme_serde() {
        let serialized = serde_json::to_string(&Theme::Dungeon).unwrap();
        assert_eq!(serialized, "\"dungeon\"");
        let deserialized: Theme = serde_json::from_str("\"dungeon\"").unwrap();
        assert_eq!(deserialized, Theme::Dungeon);
        let deserialized_capital: Theme = serde_json::from_str("\"Dungeon\"").unwrap();
        assert_eq!(deserialized_capital, Theme::Dungeon);
        let deserialized_scifi: Theme = serde_json::from_str("\"sci-fi\"").unwrap();
        assert_eq!(deserialized_scifi, Theme::SciFi);
        let serialized_building = serde_json::to_string(&Theme::Building).unwrap();
        assert_eq!(serialized_building, "\"building\"");
        let deserialized_building: Theme = serde_json::from_str("\"building\"").unwrap();
        assert_eq!(deserialized_building, Theme::Building);
        let deserialized_structure: Theme = serde_json::from_str("\"structure\"").unwrap();
        assert_eq!(deserialized_structure, Theme::Building);
        let deserialized_outpost: Theme = serde_json::from_str("\"Outpost\"").unwrap();
        assert_eq!(deserialized_outpost, Theme::Building);
    }

    #[test]
    fn test_source_conversions_and_serde() {
        assert_eq!(Source::Reddit.as_str(), "reddit");
        assert_eq!(Source::from_str("Reddit").unwrap(), Source::Reddit);
        assert_eq!(Source::from_str("manual").unwrap(), Source::Manual);
        let serialized = serde_json::to_string(&Source::Generated).unwrap();
        assert_eq!(serialized, "\"generated\"");
        let deserialized: Source = serde_json::from_str("\"generated\"").unwrap();
        assert_eq!(deserialized, Source::Generated);
    }

    #[test]
    fn test_rejection_reason_conversions_and_serde() {
        assert_eq!(RejectionReason::GridBurnedIn.to_string(), "GridBurnedIn");
        assert_eq!(
            RejectionReason::from_str("grid_burned_in").unwrap(),
            RejectionReason::GridBurnedIn
        );
        assert_eq!(
            RejectionReason::from_str("IsometricTilt").unwrap(),
            RejectionReason::IsometricTilt
        );
        let json = "\"grid_burned_in\"";
        let reason: RejectionReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, RejectionReason::GridBurnedIn);
        assert_eq!(RejectionReason::default(), RejectionReason::Unspecified);
        assert_eq!(
            RejectionReason::from_str("unspecified").unwrap(),
            RejectionReason::Unspecified
        );
    }

    #[test]
    fn test_candidate_serde() {
        let candidate = Candidate::with_now(
            "test_123",
            Source::Reddit,
            "test_123.png",
            (2048, 2048),
        );
        let json = serde_json::to_string(&candidate).unwrap();
        let parsed: Candidate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test_123");
        assert_eq!(parsed.source, Source::Reddit);
        assert_eq!(parsed.filename, "test_123.png");
        assert_eq!(parsed.dimensions, (2048, 2048));
        assert_eq!(parsed.created_at, candidate.created_at);
    }


    #[test]
    fn test_candidate_sidecar_serde_and_compatibility() {
        // Sidecar JSON without filename and dimensions, matching Issue #25 schema
        let json = r#"{
            "id": "reddit_abc123",
            "source": "Reddit",
            "source_url": "https://reddit.com/r/battlemaps/comments/abc/ | https://i.redd.it/abc.png",
            "title": "[40x30] Ancient Dungeon [Gridless]",
            "author": "mapper_joe",
            "parsed_gridless": true,
            "parsed_dimensions": [40, 30],
            "created_at": "2026-10-11T00:00:00Z"
        }"#;

        let parsed: Candidate = serde_json::from_str(json).expect("deserialize sidecar as Candidate");
        assert_eq!(parsed.id, "reddit_abc123");
        assert_eq!(parsed.source, Source::Reddit);
        assert_eq!(parsed.filename, "");
        assert_eq!(parsed.resolved_filename(), "reddit_abc123.png");
        assert_eq!(parsed.dimensions, (0, 0));
        assert_eq!(
            parsed.source_url.as_deref(),
            Some("https://reddit.com/r/battlemaps/comments/abc/ | https://i.redd.it/abc.png")
        );
        assert_eq!(parsed.title.as_deref(), Some("[40x30] Ancient Dungeon [Gridless]"));
        assert_eq!(parsed.author.as_deref(), Some("mapper_joe"));
        assert_eq!(parsed.parsed_gridless, Some(true));
        assert_eq!(parsed.parsed_dimensions, Some((40, 30)));

        // Reserialize Candidate and verify compatibility
        let reserialized = serde_json::to_string(&parsed).expect("serialize candidate");
        let roundtrip: Candidate = serde_json::from_str(&reserialized).expect("roundtrip");
        assert_eq!(roundtrip.id, parsed.id);
        assert_eq!(roundtrip.source_url, parsed.source_url);
    }

    #[test]
    fn test_reddit_sidecar_metadata_conversions() {
        let meta = RedditSidecarMetadata {
            id: "red_42".to_string(),
            source: Source::Reddit,
            source_url: "https://i.redd.it/red42.png".to_string(),
            title: "Crystal Cavern".to_string(),
            author: "cave_builder".to_string(),
            parsed_gridless: false,
            parsed_dimensions: Some((50, 50)),
            created_at: Utc::now(),
        };

        let json = serde_json::to_string_pretty(&meta).expect("serialize sidecar");
        assert!(json.contains("\"id\": \"red_42\""));
        assert!(json.contains("\"parsed_gridless\": false"));

        let parsed_meta: RedditSidecarMetadata = serde_json::from_str(&json).expect("deserialize sidecar");
        assert_eq!(parsed_meta, meta);

        // Conversion to Candidate
        let cand = meta.to_candidate();
        assert_eq!(cand.id, "red_42");
        assert_eq!(cand.filename, "red_42.png");
        assert_eq!(cand.dimensions, (50, 50));
        assert_eq!(cand.parsed_gridless, Some(false));

        // Conversion from Candidate
        let back: RedditSidecarMetadata = cand.try_into().expect("try_into metadata");
        assert_eq!(back.id, meta.id);
        assert_eq!(back.title, meta.title);
    }

    #[test]
    fn test_format_source_url() {
        assert_eq!(
            format_source_url("https://reddit.com/post", "https://i.redd.it/img.png"),
            "https://reddit.com/post | https://i.redd.it/img.png"
        );
        assert_eq!(
            format_source_url("", "https://i.redd.it/img.png"),
            "https://i.redd.it/img.png"
        );
        assert_eq!(
            format_source_url("https://i.redd.it/img.png", "https://i.redd.it/img.png"),
            "https://i.redd.it/img.png"
        );
    }
    #[test]
    fn test_curated_map_themes_and_serde() {
        let curated = CuratedMap::with_now(
            "map_456",
            vec!["cave".to_string(), "underdark".to_string()],
            vec!["water".into(), "stalagmites".into()],
            (4096, 4096),
        );
        assert!(curated.validate_themes().is_ok());
        assert_eq!(curated.primary_theme(), "cave");
        assert_eq!(curated.theme(), Some(Theme::Cave));

        let empty_themes = CuratedMap::with_now(
            "map_empty",
            Vec::<String>::new(),
            vec![],
            (1024, 1024),
        );
        assert!(empty_themes.validate_themes().is_err());

        // New format serialization & deserialization
        let json = serde_json::to_string(&curated).unwrap();
        assert!(!json.contains("\"rating\""));
        assert!(json.contains("\"themes\":[\"cave\",\"underdark\"]"));
        let parsed: CuratedMap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "map_456");
        assert_eq!(parsed.themes, vec!["cave", "underdark"]);
        assert_eq!(parsed.tags, vec!["water", "stalagmites"]);

        // Legacy format deserialization (single "theme", "rating" present)
        let legacy_json = r#"{
            "id": "legacy_map_1",
            "theme": "dungeon",
            "rating": 5,
            "tags": ["crypt", "stone"],
            "dimensions": [2048, 2048],
            "curated_at": "2026-10-16T12:00:00Z"
        }"#;
        let legacy_parsed: CuratedMap = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(legacy_parsed.id, "legacy_map_1");
        assert_eq!(legacy_parsed.themes, vec!["dungeon"]);
        assert_eq!(legacy_parsed.theme(), Some(Theme::Dungeon));
        assert_eq!(legacy_parsed.primary_theme(), "dungeon");
        assert_eq!(legacy_parsed.tags, vec!["crypt", "stone"]);
    }

    #[test]
    fn test_curated_map_description_serde_and_backward_compatibility() {
        let map_without_desc = CuratedMap::with_now(
            "map_nodesc",
            vec!["dungeon".to_string()],
            vec!["stone".into()],
            (2048, 2048),
        );
        assert_eq!(map_without_desc.description, None);

        // Serializing without description must omit "description" field
        let json_nodesc = serde_json::to_string(&map_without_desc).unwrap();
        assert!(!json_nodesc.contains("\"description\""));

        // Deserializing without description
        let parsed_nodesc: CuratedMap = serde_json::from_str(&json_nodesc).unwrap();
        assert_eq!(parsed_nodesc.description, None);

        // With description
        let desc_text = "Ancient limestone crypt with carved granite pillars and glowing braziers.";
        let map_with_desc = map_without_desc.clone().with_description(desc_text);
        assert_eq!(map_with_desc.description.as_deref(), Some(desc_text));

        let json_with_desc = serde_json::to_string(&map_with_desc).unwrap();
        assert!(json_with_desc.contains("\"description\":\"Ancient limestone crypt"));

        let parsed_with_desc: CuratedMap = serde_json::from_str(&json_with_desc).unwrap();
        assert_eq!(parsed_with_desc.description.as_deref(), Some(desc_text));
        assert_eq!(parsed_with_desc, map_with_desc);

        // Backward compatibility with legacy manifest JSON lacking "description"
        let legacy_manifest = r#"{
            "id": "legacy_map_no_desc",
            "themes": ["crypt"],
            "tags": ["sarcophagus"],
            "dimensions": [1024, 1024],
            "curated_at": "2026-10-16T12:00:00Z",
            "layers": []
        }"#;
        let legacy_map: CuratedMap = serde_json::from_str(legacy_manifest).unwrap();
        assert_eq!(legacy_map.id, "legacy_map_no_desc");
        assert_eq!(legacy_map.description, None);

        // Empty or whitespace description via with_description sets None
        let map_empty_desc = map_without_desc.clone().with_description("   \t  ");
        assert_eq!(map_empty_desc.description, None);
    }

    #[test]
    fn test_curated_map_sections_serde_and_backward_compatibility() {
        // Direct MapSection serde
        let section = MapSection::new(0, 16, "Flooded subterranean corridor with damp moss");
        let section_json = serde_json::to_string(&section).unwrap();
        assert_eq!(
            section_json,
            r#"{"col":0,"row":16,"description":"Flooded subterranean corridor with damp moss"}"#
        );
        let parsed_section: MapSection = serde_json::from_str(&section_json).unwrap();
        assert_eq!(parsed_section, section);

        // CuratedMap without sections omits field in serialized JSON
        let map_without_sections = CuratedMap::with_now(
            "map_no_sec",
            vec!["dungeon".to_string()],
            vec!["stone".into()],
            (1024, 1024),
        );
        assert!(map_without_sections.sections.is_empty());
        let json_no_sec = serde_json::to_string(&map_without_sections).unwrap();
        assert!(!json_no_sec.contains("\"sections\""));

        let parsed_no_sec: CuratedMap = serde_json::from_str(&json_no_sec).unwrap();
        assert!(parsed_no_sec.sections.is_empty());

        // CuratedMap with sections serializes and deserializes cleanly
        let sections = vec![
            MapSection::new(0, 0, "Northwest armory with rusted weapon racks"),
            MapSection::new(16, 0, "Northeast altar with blackened brazier"),
            MapSection::new(0, 16, "Southwest prison cells with iron bars"),
            MapSection::new(16, 16, "Southeast torture chamber with stone table"),
        ];
        let map_with_sections = map_without_sections.clone().with_sections(sections.clone());
        assert_eq!(map_with_sections.sections.len(), 4);

        let json_with_sections = serde_json::to_string(&map_with_sections).unwrap();
        assert!(json_with_sections.contains("\"sections\":["));
        assert!(json_with_sections.contains("Northwest armory"));

        let parsed_with_sections: CuratedMap = serde_json::from_str(&json_with_sections).unwrap();
        assert_eq!(parsed_with_sections.sections, sections);
        assert_eq!(parsed_with_sections, map_with_sections);

        // Backward compatibility with legacy manifest JSON lacking "sections" field
        let legacy_manifest = r#"{
            "id": "legacy_map_no_sections",
            "themes": ["crypt"],
            "tags": ["bones"],
            "dimensions": [1024, 1024],
            "curated_at": "2026-10-16T12:00:00Z",
            "layers": []
        }"#;
        let legacy_map: CuratedMap = serde_json::from_str(legacy_manifest).unwrap();
        assert_eq!(legacy_map.id, "legacy_map_no_sections");
        assert_eq!(legacy_map.sections, Vec::<MapSection>::new());

        // CuratedMapUpdate with sections serde
        let update = CuratedMapUpdate::default().with_sections(vec![MapSection::new(0, 0, "Updated section")]);
        let update_json = serde_json::to_string(&update).unwrap();
        assert!(update_json.contains("\"sections\":["));
        let parsed_update: CuratedMapUpdate = serde_json::from_str(&update_json).unwrap();
        assert_eq!(parsed_update.sections.as_ref().unwrap().len(), 1);
        assert_eq!(parsed_update.sections.as_ref().unwrap()[0].description, "Updated section");
    }

    #[test]
    fn test_reddit_sort_and_timerange_serde() {
        assert_eq!(RedditSort::Hot.as_str(), "hot");
        assert_eq!(RedditSort::Top.as_str(), "top");
        assert_eq!(RedditSort::New.as_str(), "new");
        assert_eq!(RedditSort::from_str("hot").unwrap(), RedditSort::Hot);
        assert_eq!(RedditSort::from_str("TOP").unwrap(), RedditSort::Top);
        assert_eq!(RedditSort::from_str("New").unwrap(), RedditSort::New);
        assert!(RedditSort::from_str("invalid").is_err());

        assert_eq!(RedditTimeRange::All.as_str(), "all");
        assert_eq!(RedditTimeRange::Day.as_str(), "day");
        assert_eq!(RedditTimeRange::from_str("week").unwrap(), RedditTimeRange::Week);
        assert_eq!(RedditTimeRange::from_str("Year").unwrap(), RedditTimeRange::Year);
        assert_eq!(RedditTimeRange::from_str("MONTH").unwrap(), RedditTimeRange::Month);
        assert!(RedditTimeRange::from_str("invalid").is_err());

        let sort_json = serde_json::to_string(&RedditSort::Top).unwrap();
        assert_eq!(sort_json, "\"top\"");
        let time_json = serde_json::to_string(&RedditTimeRange::Month).unwrap();
        assert_eq!(time_json, "\"month\"");
    }

    #[test]
    fn test_reddit_harvest_query_builder() {
        let default_query = RedditHarvestQuery::default();
        assert_eq!(default_query.subreddit, "battlemaps");
        assert_eq!(default_query.sort, RedditSort::Hot);
        assert_eq!(default_query.limit, 25);

        let query = RedditHarvestQuery::new()
            .with_subreddit("dndmaps")
            .with_sort(RedditSort::Top)
            .with_time(RedditTimeRange::Month)
            .with_limit(150) // clamped to 100
            .with_after("t3_abc123");

        assert_eq!(query.subreddit, "dndmaps");
        assert_eq!(query.sort, RedditSort::Top);
        assert_eq!(query.time, Some(RedditTimeRange::Month));
        assert_eq!(query.limit, 100);
        assert_eq!(query.after, Some("t3_abc123".into()));
    }

    #[test]
    fn test_title_parsing_grid_status() {
        let t1 = ParsedTags::parse_title("[30x40] Forest Shrine [Gridless]");
        assert_eq!(t1.grid_status, GridStatus::Gridless);
        assert!(t1.is_gridless());
        assert!(!t1.is_gridded());
        assert_eq!(t1.dimensions, Some((30, 40)));

        let t2 = ParsedTags::parse_title("Dungeon Corridor (No Grid) - 25x25");
        assert_eq!(t2.grid_status, GridStatus::Gridless);
        assert_eq!(t2.dimensions, Some((25, 25)));

        let t3 = ParsedTags::parse_title("[No-Grid] Desert Oasis [50x50]");
        assert_eq!(t3.grid_status, GridStatus::Gridless);
        assert_eq!(t3.dimensions, Some((50, 50)));

        let t4 = ParsedTags::parse_title("Castle Ruins - ungridded");
        assert_eq!(t4.grid_status, GridStatus::Gridless);
        assert_eq!(t4.dimensions, None);

        let t5 = ParsedTags::parse_title("[40x30] Wizard's Tower");
        assert_eq!(t5.grid_status, GridStatus::Gridded);
        assert!(t5.is_gridded());
        assert!(!t5.is_gridless());
        assert_eq!(t5.dimensions, Some((40, 30)));

        let t6 = ParsedTags::parse_title("Mountain Pass [Gridded]");
        assert_eq!(t6.grid_status, GridStatus::Gridded);
        assert_eq!(t6.dimensions, None);

        let t7 = ParsedTags::parse_title("A scenic view of our campaign world");
        assert_eq!(t7.grid_status, GridStatus::Unknown);
        assert_eq!(t7.dimensions, None);
    }

    #[test]
    fn test_title_parsing_dimensions() {
        assert_eq!(
            ParsedTags::parse_title("[30x40] Forest").dimensions,
            Some((30, 40))
        );
        assert_eq!(
            ParsedTags::parse_title("Temple 30 x 40").dimensions,
            Some((30, 40))
        );
        assert_eq!(
            ParsedTags::parse_title("Cave 45×60 [Gridless]").dimensions,
            Some((45, 60))
        );
        assert_eq!(
            ParsedTags::parse_title("[100x120] Massive Dungeon").dimensions,
            Some((100, 120))
        );
        assert_eq!(
            ParsedTags::parse_title("Battlemap 20X30").dimensions,
            Some((20, 30))
        );
    }

    #[test]
    fn test_reddit_post_candidate_conversion() {
        let tags = ParsedTags::parse_title("[30x40] Forest [Gridless]");
        let post = RedditPost {
            id: "xyz123".into(),
            name: "t3_xyz123".into(),
            title: "[30x40] Forest [Gridless]".into(),
            author: "cartographer".into(),
            permalink: "https://www.reddit.com/r/battlemaps/comments/xyz123/".into(),
            url: "https://i.redd.it/xyz123.png".into(),
            score: 250,
            created_utc: Utc::now(),
            media: vec![
                HarvestedMedia::new("https://i.redd.it/xyz123.png")
                    .with_dimensions(3000, 4000),
            ],
            tags,
        };

        assert!(post.has_media());
        assert_eq!(post.primary_image_url(), Some("https://i.redd.it/xyz123.png"));

        let candidate = post.to_candidate(0).expect("candidate generated");
        assert_eq!(candidate.id, "xyz123_0");
        assert_eq!(candidate.source, Source::Reddit);
        assert_eq!(candidate.filename, "xyz123_0.png");
        assert_eq!(candidate.dimensions, (3000, 4000));
    }

    #[test]
    fn test_reddit_post_bundle_candidate_conversion() {
        let tags = ParsedTags::parse_title("[40x30] Royal Castle [Gridless]");
        let post = RedditPost {
            id: "castle123".into(),
            name: "t3_castle123".into(),
            title: "[40x30] Royal Castle [Gridless]".into(),
            author: "king_mapper".into(),
            permalink: "https://reddit.com/r/battlemaps/comments/castle123/".into(),
            url: "https://i.redd.it/castle_0.png".into(),
            score: 250,
            created_utc: Utc::now(),
            media: vec![
                HarvestedMedia::new("https://i.redd.it/castle_0.png")
                    .with_dimensions(4000, 3000)
                    .with_caption("Ground Floor"),
                HarvestedMedia::new("https://i.redd.it/castle_1.png")
                    .with_dimensions(4000, 3000)
                    .with_caption("Second Floor"),
            ],
            tags,
        };

        let candidate = post.to_bundle_candidate();
        assert_eq!(candidate.id, "castle123");
        assert_eq!(candidate.source, Source::Reddit);
        assert_eq!(candidate.filename, "castle123.png");
        assert_eq!(candidate.dimensions, (4000, 3000));
        assert_eq!(candidate.title.as_deref(), Some("[40x30] Royal Castle [Gridless]"));
        assert_eq!(candidate.author.as_deref(), Some("king_mapper"));
        assert_eq!(candidate.parsed_gridless, Some(true));
        assert_eq!(candidate.parsed_dimensions, Some((40, 30)));
        assert_eq!(candidate.layers.len(), 2);

        assert_eq!(candidate.layers[0].index, 0);
        assert_eq!(candidate.layers[0].filename, "0.png");
        assert_eq!(candidate.layers[0].role, LayerRole::Ground);
        assert_eq!(candidate.layers[0].elevation, Some(0));
        assert_eq!(candidate.layers[0].caption.as_deref(), Some("Ground Floor"));
        assert!(candidate.layers[0].source_url.as_deref().unwrap().contains("castle_0.png"));

        assert_eq!(candidate.layers[1].index, 1);
        assert_eq!(candidate.layers[1].filename, "1.png");
        assert_eq!(candidate.layers[1].role, LayerRole::UpperFloor);
        assert_eq!(candidate.layers[1].elevation, Some(1));
        assert_eq!(candidate.layers[1].caption.as_deref(), Some("Second Floor"));
        assert!(candidate.layers[1].source_url.as_deref().unwrap().contains("castle_1.png"));
    }

    #[test]
    fn test_reddit_harvest_request_defaults_and_conversion() {
        let req = RedditHarvestRequest::default();
        assert_eq!(req.sort, RedditSort::Hot);
        assert_eq!(req.time, Some(RedditTimeRange::All));
        assert_eq!(req.limit, Some(25));
        assert!(!req.gridless_only);
        assert_eq!(req.subreddit, None);

        let query = req.to_query();
        assert_eq!(query.subreddit, "battlemaps");
        assert_eq!(query.sort, RedditSort::Hot);
        assert_eq!(query.time, Some(RedditTimeRange::All));
        assert_eq!(query.limit, 25);

        let custom = RedditHarvestRequest::new()
            .with_sort(RedditSort::Top)
            .with_time(RedditTimeRange::Month)
            .with_limit(50)
            .with_gridless_only(true)
            .with_subreddit("dndmaps");

        assert!(custom.gridless_only);
        let custom_query = custom.to_query();
        assert_eq!(custom_query.subreddit, "dndmaps");
        assert_eq!(custom_query.sort, RedditSort::Top);
        assert_eq!(custom_query.time, Some(RedditTimeRange::Month));
        assert_eq!(custom_query.limit, 50);
    }

    #[test]
    fn test_reddit_harvest_request_serde() {
        let json_str = r#"{"sort":"top","time":"year","limit":30,"gridless_only":true}"#;
        let req: RedditHarvestRequest = serde_json::from_str(json_str).expect("deserialize");
        assert_eq!(req.sort, RedditSort::Top);
        assert_eq!(req.time, Some(RedditTimeRange::Year));
        assert_eq!(req.limit, Some(30));
        assert!(req.gridless_only);

        let query = req.to_query();
        assert_eq!(query.sort, RedditSort::Top);
        assert_eq!(query.time, Some(RedditTimeRange::Year));
        assert_eq!(query.limit, 30);
    }

    #[test]
    fn test_layer_role_regex_inference_from_text() {
        // Ground / Base / 1st Floor / Interior -> Ground (0)
        assert_eq!(
            infer_layer_role_from_text("Ground floor"),
            Some((LayerRole::Ground, Some(0)))
        );
        assert_eq!(
            infer_layer_role_from_text("Base map"),
            Some((LayerRole::Ground, Some(0)))
        );
        assert_eq!(
            infer_layer_role_from_text("1st Floor"),
            Some((LayerRole::Ground, Some(0)))
        );
        assert_eq!(
            infer_layer_role_from_text("Interior"),
            Some((LayerRole::Ground, Some(0)))
        );

        // 2nd Floor / 3rd Floor / Upper / Attic -> UpperFloor
        assert_eq!(
            infer_layer_role_from_text("2nd Floor"),
            Some((LayerRole::UpperFloor, Some(1)))
        );
        assert_eq!(
            infer_layer_role_from_text("3rd Floor"),
            Some((LayerRole::UpperFloor, Some(2)))
        );
        assert_eq!(
            infer_layer_role_from_text("Upper level"),
            Some((LayerRole::UpperFloor, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Attic"),
            Some((LayerRole::UpperFloor, None))
        );

        // Basement / Cellar / Crypt / Lower -> Basement
        assert_eq!(
            infer_layer_role_from_text("Basement"),
            Some((LayerRole::Basement, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Cellar"),
            Some((LayerRole::Basement, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Crypt"),
            Some((LayerRole::Basement, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Lower"),
            Some((LayerRole::Basement, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Basement 2"),
            Some((LayerRole::Basement, Some(-2)))
        );

        // Roof / Canopy / Exterior -> Roof (None)
        assert_eq!(
            infer_layer_role_from_text("Roof"),
            Some((LayerRole::Roof, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Canopy"),
            Some((LayerRole::Roof, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Exterior"),
            Some((LayerRole::Roof, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Exterior roof"),
            Some((LayerRole::Roof, None))
        );

        // Night / Storm / Massacre / Ruined / Variant -> Variant (None)
        assert_eq!(
            infer_layer_role_from_text("Night"),
            Some((LayerRole::Variant, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Storm"),
            Some((LayerRole::Variant, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Massacre"),
            Some((LayerRole::Variant, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Ruined"),
            Some((LayerRole::Variant, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Variant"),
            Some((LayerRole::Variant, None))
        );

        // Gridded / Hex -> Exclude (None)
        assert_eq!(
            infer_layer_role_from_text("Gridded"),
            Some((LayerRole::Exclude, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Hex"),
            Some((LayerRole::Exclude, None))
        );
        assert_eq!(
            infer_layer_role_from_text("Hex grid"),
            Some((LayerRole::Exclude, None))
        );

        // Unrecognized / empty -> None
        assert_eq!(infer_layer_role_from_text(""), None);
        assert_eq!(infer_layer_role_from_text("Mysterious dragon cave"), None);
    }

    #[test]
    fn test_infer_bundle_layer_roles_single_and_multi_item() {
        // Single-item post -> defaults to Ground (0) regardless of text
        let single_none = infer_bundle_layer_roles(&[None], None);
        assert_eq!(single_none, vec![(LayerRole::Ground, Some(0))]);

        let single_random = infer_bundle_layer_roles(&[Some("Dragon Cave".into())], None);
        assert_eq!(single_random, vec![(LayerRole::Ground, Some(0))]);

        let single_variant = infer_bundle_layer_roles(&[Some("Night storm".into())], None);
        assert_eq!(single_variant, vec![(LayerRole::Ground, Some(0))]);

        // Multi-item post captions with full spectrum of roles and auto-increment
        let captions = vec![
            Some("Ground floor".to_string()),
            Some("2nd Floor".to_string()),
            Some("3rd Floor".to_string()),
            Some("Upper".to_string()),
            Some("Attic".to_string()),
            Some("Basement".to_string()),
            Some("Cellar".to_string()),
            Some("Crypt".to_string()),
            Some("Lower".to_string()),
            Some("Roof".to_string()),
            Some("Canopy".to_string()),
            Some("Exterior".to_string()),
            Some("Night".to_string()),
            Some("Storm".to_string()),
            Some("Massacre".to_string()),
            Some("Ruined".to_string()),
            Some("Variant".to_string()),
            Some("Gridded".to_string()),
            Some("Hex".to_string()),
            None,
            Some("Unknown chamber".to_string()),
        ];

        let inferred = infer_bundle_layer_roles(&captions, None);
        assert_eq!(inferred.len(), 21);

        // Ground (0)
        assert_eq!(inferred[0], (LayerRole::Ground, Some(0)));

        // UpperFloors (+1, +2, +3, +4)
        assert_eq!(inferred[1], (LayerRole::UpperFloor, Some(1)));
        assert_eq!(inferred[2], (LayerRole::UpperFloor, Some(2)));
        assert_eq!(inferred[3], (LayerRole::UpperFloor, Some(3)));
        assert_eq!(inferred[4], (LayerRole::UpperFloor, Some(4)));

        // Basements (-1, -2, -3, -4)
        assert_eq!(inferred[5], (LayerRole::Basement, Some(-1)));
        assert_eq!(inferred[6], (LayerRole::Basement, Some(-2)));
        assert_eq!(inferred[7], (LayerRole::Basement, Some(-3)));
        assert_eq!(inferred[8], (LayerRole::Basement, Some(-4)));

        // Roofs (None)
        assert_eq!(inferred[9], (LayerRole::Roof, None));
        assert_eq!(inferred[10], (LayerRole::Roof, None));
        assert_eq!(inferred[11], (LayerRole::Roof, None));

        // Variants (None)
        assert_eq!(inferred[12], (LayerRole::Variant, None));
        assert_eq!(inferred[13], (LayerRole::Variant, None));
        assert_eq!(inferred[14], (LayerRole::Variant, None));
        assert_eq!(inferred[15], (LayerRole::Variant, None));
        assert_eq!(inferred[16], (LayerRole::Variant, None));

        // Exclude (None)
        assert_eq!(inferred[17], (LayerRole::Exclude, None));
        assert_eq!(inferred[18], (LayerRole::Exclude, None));

        // Unrecognized / missing caption -> Unassigned (None)
        assert_eq!(inferred[19], (LayerRole::Unassigned, None));
        assert_eq!(inferred[20], (LayerRole::Unassigned, None));
    }

    #[test]
    fn test_bundle_curation_validation() {
        // Fails when all layers are Excluded
        let all_excluded = vec![
            (LayerRole::Exclude, None),
            (LayerRole::Exclude, None),
        ];
        assert!(validate_layers_for_curation(&all_excluded).is_err());

        // Fails when any included layer is Unassigned
        let with_unassigned = vec![
            (LayerRole::Ground, Some(0)),
            (LayerRole::Unassigned, None),
            (LayerRole::Exclude, None),
        ];
        assert!(validate_layers_for_curation(&with_unassigned).is_err());

        // Passes with valid mix of Ground, UpperFloor, Roof, Exclude
        let valid_mix = vec![
            (LayerRole::Ground, Some(0)),
            (LayerRole::UpperFloor, Some(1)),
            (LayerRole::Roof, None),
            (LayerRole::Exclude, None),
        ];
        assert!(validate_layers_for_curation(&valid_mix).is_ok());

        // Candidate validation method checks
        let mut cand_all_excluded = Candidate::with_now(
            "cand_excl",
            Source::Reddit,
            "cand_excl/0.png",
            (1000, 1000),
        );
        cand_all_excluded.layers = vec![
            CandidateLayer::new(0, "0.png", LayerRole::Exclude, None, (1000, 1000)),
            CandidateLayer::new(1, "1.png", LayerRole::Exclude, None, (1000, 1000)),
        ];
        assert!(cand_all_excluded.validate_for_curation().is_err());

        let mut cand_unassigned = Candidate::with_now(
            "cand_unass",
            Source::Reddit,
            "cand_unass/0.png",
            (1000, 1000),
        );
        cand_unassigned.layers = vec![
            CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
            CandidateLayer::new(1, "1.png", LayerRole::Unassigned, None, (1000, 1000)),
        ];
        assert!(cand_unassigned.validate_for_curation().is_err());

        let mut cand_valid = Candidate::with_now(
            "cand_ok",
            Source::Reddit,
            "cand_ok/0.png",
            (1000, 1000),
        );
        cand_valid.layers = vec![
            CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
            CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1000, 1000)),
            CandidateLayer::new(2, "2.png", LayerRole::Roof, None, (1000, 1000)),
            CandidateLayer::new(3, "3.png", LayerRole::Exclude, None, (1000, 1000)),
        ];
        assert!(cand_valid.validate_for_curation().is_ok());

        // Default single-image candidate defaults to Ground(0) and passes
        let cand_single = Candidate::with_now(
            "cand_single",
            Source::Reddit,
            "cand_single.png",
            (1000, 1000),
        );
        assert!(cand_single.validate_for_curation().is_ok());
    }

    #[test]
    fn test_curated_map_update_serialization() {
        let update = CuratedMapUpdate::default()
            .with_themes(vec!["tavern".into(), "coastal".into()])
            .with_tags(vec!["dockside".into()])
            .with_description("A seaside tavern with salty breeze and wooden tables.")
            .with_layers(vec![CuratedLayer::new(
                0,
                "0_ground.png",
                LayerRole::Ground,
                Some(0),
                (2048, 2048),
            )]);

        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("tavern"));
        assert!(json.contains("dockside"));
        assert!(json.contains("seaside tavern"));
        assert!(json.contains("0_ground.png"));

        let deserialized: CuratedMapUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(update, deserialized);

        // Empty update deserializes to all None
        let empty: CuratedMapUpdate = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, CuratedMapUpdate::default());
        assert!(empty.themes.is_none());
        assert!(empty.tags.is_none());
        assert!(empty.description.is_none());
        assert!(empty.layers.is_none());
    }
    #[test]
    fn test_grid_config_normalization_and_serde() {
        let default_grid = GridConfig::default();
        assert_eq!(default_grid.cell_size_px, 64);
        assert_eq!(default_grid.offset_x, 0);
        assert_eq!(default_grid.offset_y, 0);

        // Clamping & modulo
        let small = GridConfig::new(10, 100, 200);
        assert_eq!(small.cell_size_px, 16);
        assert_eq!(small.offset_x, 100 % 16);
        assert_eq!(small.offset_y, 200 % 16);

        let big = GridConfig::new(500, 350, 400);
        assert_eq!(big.cell_size_px, 300);
        assert_eq!(big.offset_x, 350 % 300);
        assert_eq!(big.offset_y, 400 % 300);

        // Signed offset normalization with Euclidean modulo
        let signed = GridConfig::with_signed_offsets(64, -5, -70);
        assert_eq!(signed.cell_size_px, 64);
        assert_eq!(signed.offset_x, 59); // -5 mod 64 = 59
        assert_eq!(signed.offset_y, 58); // -70 mod 64 = 58

        // Deserialization with legacy / missing fields
        let legacy_json = "{}";
        let parsed_legacy: GridConfig = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed_legacy, GridConfig::default());

        // Deserialization with signed negative offsets in JSON
        let json = r#"{"cell_size_px": 50, "offset_x": -10, "offset_y": 60}"#;
        let parsed: GridConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.cell_size_px, 50);
        assert_eq!(parsed.offset_x, 40);
        assert_eq!(parsed.offset_y, 10);
    }

    #[test]
    fn test_grid_config_auto_seed() {
        // Matching normal aspect ratio within 5%
        // 2000x1500 image, 40x30 grid -> cell size 50px
        let seeded = GridConfig::auto_seed((40, 30), (2000, 1500));
        assert_eq!(seeded.cell_size_px, 50);
        assert_eq!(seeded.offset_x, 0);
        assert_eq!(seeded.offset_y, 0);

        // Matching swapped aspect ratio (image 1500x2000, parsed 40x30)
        let seeded_swapped = GridConfig::auto_seed((40, 30), (1500, 2000));
        assert_eq!(seeded_swapped.cell_size_px, 50);

        // Mismatched aspect ratio -> defaults to 64px
        let mismatched = GridConfig::auto_seed((50, 10), (2000, 1500));
        assert_eq!(mismatched.cell_size_px, 64);
    }

    #[test]
    fn test_grid_config_lattice_dimensions() {
        let grid = GridConfig::new(64, 0, 0);
        // 2048 x 2048 -> 32 x 32 cells exactly
        assert_eq!(grid.lattice_dimensions(2048, 2048), (32, 32));

        // Partial edge remnant discarded: 2040 x 1500 -> 31 x 23 cells
        assert_eq!(grid.lattice_dimensions(2040, 1500), (31, 23));

        // With offsets: offset_x = 20, offset_y = 10
        let offset_grid = GridConfig::new(64, 20, 10);
        // (2048 - 20) / 64 = 2028 / 64 = 31
        // (2048 - 10) / 64 = 2038 / 64 = 31
        assert_eq!(offset_grid.lattice_dimensions(2048, 2048), (31, 31));

        // Image smaller than offset or cell size
        assert_eq!(offset_grid.lattice_dimensions(10, 5), (0, 0));
    }

    #[test]
    fn test_grid_config_section_counts_and_eligibility() {
        let grid = GridConfig::new(64, 0, 0);

        // 1024 x 1024 -> 16 x 16 cells -> minimum eligible (1 section x 1 section)
        assert_eq!(grid.lattice_dimensions(1024, 1024), (16, 16));
        assert!(grid.is_curation_eligible(1024, 1024));
        assert_eq!(grid.section_counts(1024, 1024), Some((1, 1)));

        // 1023 x 1024 -> 15 x 16 cells -> ineligible (< 16 cols)
        assert_eq!(grid.lattice_dimensions(1023, 1024), (15, 16));
        assert!(!grid.is_curation_eligible(1023, 1024));
        assert_eq!(grid.section_counts(1023, 1024), None);

        // 1024 x 1023 -> 16 x 15 cells -> ineligible (< 16 rows)
        assert_eq!(grid.lattice_dimensions(1024, 1023), (16, 15));
        assert!(!grid.is_curation_eligible(1024, 1023));
        assert_eq!(grid.section_counts(1024, 1023), None);

        // 2048 x 1536 -> 32 x 24 cells -> eligible -> (32.div_ceil(16), 24.div_ceil(16)) = (2, 2)
        assert_eq!(grid.lattice_dimensions(2048, 1536), (32, 24));
        assert!(grid.is_curation_eligible(2048, 1536));
        assert_eq!(grid.section_counts(2048, 1536), Some((2, 2)));

        // 2560 x 1024 -> 40 x 16 cells -> (3, 1)
        assert_eq!(grid.section_counts(2560, 1024), Some((3, 1)));
    }

    #[test]
    fn test_grid_config_section_origins() {
        let grid = GridConfig::new(64, 0, 0);

        // Undersized map returns empty origins
        assert_eq!(grid.section_origins(960, 960), Vec::<(u32, u32)>::new());

        // Exactly 16x16 cells (1024x1024 px) -> single origin at (0, 0)
        assert_eq!(grid.section_origins(1024, 1024), vec![(0, 0)]);

        // 32x32 cells (2048x2048 px) -> kx=2, ky=2 -> origins at col: [0, 16], row: [0, 16]
        // Cartesian product: (0, 0), (16, 0), (0, 16), (16, 16)
        assert_eq!(
            grid.section_origins(2048, 2048),
            vec![(0, 0), (16, 0), (0, 16), (16, 16)]
        );

        // 20x20 cells (1280x1280 px) -> kx=2, ky=2 -> cols-16 = 4 -> col origins [0, 4]
        // Cartesian product: (0, 0), (4, 0), (0, 4), (4, 4)
        assert_eq!(
            grid.section_origins(1280, 1280),
            vec![(0, 0), (4, 0), (0, 4), (4, 4)]
        );

        // 40x16 cells (2560x1024 px) -> kx=3, ky=1
        // cols-16 = 24; i=0 -> 0, i=1 -> 12, i=2 -> 24. row: [0]
        // Cartesian product: (0, 0), (12, 0), (24, 0)
        assert_eq!(
            grid.section_origins(2560, 1024),
            vec![(0, 0), (12, 0), (24, 0)]
        );
    }

    #[test]
    fn test_crop_section_box_exact_box_or_none() {
        let grid = GridConfig::new(16, 0, 0); // 16 * 16 = 256px per section
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(512, 512));

        // In-bounds boxes crop the exact 256x256 pixel box.
        let top_left = crop_section_box(&img, &grid, 0, 0).expect("top-left box fits");
        assert_eq!((top_left.width(), top_left.height()), (256, 256));
        let bottom_right = crop_section_box(&img, &grid, 16, 16).expect("bottom-right box fits");
        assert_eq!((bottom_right.width(), bottom_right.height()), (256, 256));

        // Out-of-bounds boxes are unresolvable rather than rescaled or center-cropped:
        // (17, 0) starts at x = 272, so its 256px box overruns the 512px image width.
        assert!(crop_section_box(&img, &grid, 17, 0).is_none());
        assert!(crop_section_box(&img, &grid, 0, 17).is_none());
    }

    #[test]
    fn test_synthesize_sections_single_reuses_description_and_multi_describes() {
        // Empty origins produce no sections.
        assert!(synthesize_sections(&[], || "unused".to_string(), |_, _| "unused".to_string())
            .is_empty());

        // A single origin yields one (0, 0) section reusing the map description.
        let single = synthesize_sections(
            &[(0, 0)],
            || "Reused map description".to_string(),
            |_, _| panic!("single origin must not call the describer"),
        );
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].col, 0);
        assert_eq!(single[0].row, 0);
        assert_eq!(single[0].description, "Reused map description");

        // Multiple origins yield one section per origin, described individually.
        let origins = [(0, 0), (16, 0), (0, 16), (16, 16)];
        let multi = synthesize_sections(
            &origins,
            || panic!("multi origin must not reuse the map description"),
            |col, row| format!("section {col},{row}"),
        );
        assert_eq!(multi.len(), 4);
        for (section, (col, row)) in multi.iter().zip(origins) {
            assert_eq!((section.col, section.row), (col, row));
            assert_eq!(section.description, format!("section {col},{row}"));
        }
    }
}
