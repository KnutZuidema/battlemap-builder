use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

impl RejectionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GridBurnedIn => "GridBurnedIn",
            Self::IsometricTilt => "IsometricTilt",
            Self::BlurryLowRes => "BlurryLowRes",
            Self::BadGeometry => "BadGeometry",
            Self::Irrelevant => "Irrelevant",
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
            _ => Err(format!("Unknown rejection reason: {s}")),
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub source: Source,
    pub filename: String,
    pub dimensions: (u32, u32),
    pub created_at: DateTime<Utc>,
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
}

/// An approved battlemap filed into a theme folder with quality rating and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratedMap {
    pub id: String,
    pub theme: Theme,
    pub rating: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    pub dimensions: (u32, u32),
    pub curated_at: DateTime<Utc>,
}

impl CuratedMap {
    pub fn new(
        id: impl Into<String>,
        theme: Theme,
        rating: u8,
        tags: Vec<String>,
        dimensions: (u32, u32),
        curated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            theme,
            rating,
            tags,
            dimensions,
            curated_at,
        }
    }

    pub fn with_now(
        id: impl Into<String>,
        theme: Theme,
        rating: u8,
        tags: Vec<String>,
        dimensions: (u32, u32),
    ) -> Self {
        Self::new(id, theme, rating, tags, dimensions, Utc::now())
    }

    /// Validates that the quality rating falls within the required 1-5 range.
    pub fn is_valid_rating(rating: u8) -> bool {
        (1..=5).contains(&rating)
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
    fn test_curated_map_rating_and_serde() {
        let curated = CuratedMap::with_now(
            "map_456",
            Theme::Cave,
            5,
            vec!["water".into(), "stalagmites".into()],
            (4096, 4096),
        );
        assert!(CuratedMap::is_valid_rating(curated.rating));
        assert!(!CuratedMap::is_valid_rating(0));
        assert!(!CuratedMap::is_valid_rating(6));

        let json = serde_json::to_string(&curated).unwrap();
        let parsed: CuratedMap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "map_456");
        assert_eq!(parsed.theme, Theme::Cave);
        assert_eq!(parsed.rating, 5);
        assert_eq!(parsed.tags, vec!["water", "stalagmites"]);
    }
}
