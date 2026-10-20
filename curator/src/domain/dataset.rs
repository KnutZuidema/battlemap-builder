//! Dynamic `16×16` Map Section LoRA `Training Dataset` contract (ADR-0007).
//!
//! A curated bundle is exported on demand into `lora-dataset.zip`: every `Map Section` becomes
//! one `1024×1024` sample (`16` Grid Cells at a uniform `64px = 1 Grid Cell`) whose companion
//! `.txt` is the `Trigger Phrase` followed by that section's perspective-neutral
//! `Physical Description`.
//!
//! This module owns the pure half of that contract — which description a sample is captioned
//! with, and which shapes of stored data make a sample unusable for a `Training Run` — so the
//! ZIP export and the dataset readiness audit share one definition instead of drifting apart.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::domain::MapSection;

/// Camera angles, perspective directives, art-style meta-prompts and grid mechanics that are
/// renderer-owned and therefore never allowed inside a `Physical Description` (ADR-0002).
///
/// The list is deliberately the vocabulary the Dungeon Forge description prompt forbids plus the
/// `Trigger Phrase` tokens themselves: a description carrying any of them is caption text that
/// leaked into the dataset's descriptive field.
static NON_NEUTRAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(ttrpg|battlemap|battlemaps|flat\s+2d|2d|top[\s-]?down|bird'?s[\s-]?eye|isometric|aerial|orthographic|perpendicular|camera|plan\s+view|perspective|poch[eé]|cut\s+walls?|gridless|no\s+grid|grid|photorealistic|digital\s+art|concept\s+art|highly\s+detailed|4k|8k)\b",
    )
    .expect("valid non-neutral description regex")
});

/// Whether a `Physical Description` is free of camera, perspective, art-style and grid-mechanic
/// directives, i.e. describes only tangible features of the encounter space.
pub fn is_perspective_neutral(description: &str) -> bool {
    !NON_NEUTRAL_RE.is_match(description)
}

/// Resolves the `Physical Description` a `Map Section` sample is captioned with: the section's
/// own cached description, otherwise the map-level description verbatim (same encounter space).
///
/// Never returns a synthesized caption. The `Trigger Phrase` is applied exactly once, by
/// [`crate::domain::CaptionGenerator`] at caption generation time, so it can neither be
/// duplicated nor mistaken for descriptive prose.
pub fn sample_description<'a>(
    section: &'a MapSection,
    map_description: Option<&'a str>,
) -> Option<&'a str> {
    let section_description = section.description.trim();
    if !section_description.is_empty() {
        return Some(section_description);
    }
    map_description
        .map(str::trim)
        .filter(|description| !description.is_empty())
}

/// Why a curated map cannot contribute a clean set of `Map Section` samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetIssueCode {
    /// No Ground layer image could be read from the curated bundle.
    GroundLayerMissing,
    /// The Ground layer bytes exist but do not decode as an image.
    GroundImageUnreadable,
    /// The Grid Configuration yields no full `16×16` Grid Cell window.
    GridBelowMinimum,
    /// A section's exact `16×16` Grid Cell box does not fit inside the Ground layer.
    SectionUnresolvable,
    /// A section resolves to no `Physical Description` at all: its caption would carry only the
    /// `Trigger Phrase`, themes and tags.
    PhysicalDescriptionMissing,
    /// A section's `Physical Description` holds camera, perspective, art-style or grid-mechanic
    /// directives instead of perspective-neutral prose (ADR-0002).
    PhysicalDescriptionNotNeutral,
}

impl DatasetIssueCode {
    /// Stable machine-readable name, also used as the JSON discriminator.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GroundLayerMissing => "ground_layer_missing",
            Self::GroundImageUnreadable => "ground_image_unreadable",
            Self::GridBelowMinimum => "grid_below_minimum",
            Self::SectionUnresolvable => "section_unresolvable",
            Self::PhysicalDescriptionMissing => "physical_description_missing",
            Self::PhysicalDescriptionNotNeutral => "physical_description_not_neutral",
        }
    }
}

/// A single dataset defect with the operator-facing detail needed to locate it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetIssue {
    pub code: DatasetIssueCode,
    pub detail: String,
}

impl DatasetIssue {
    pub fn new(code: DatasetIssueCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// Readiness of one curated map's 16×16 `Map Section` samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapDatasetReadiness {
    pub id: String,
    /// Zip folder of this map, i.e. its primary theme tag.
    pub theme: String,
    /// Number of `Map Section` samples the map contributes to the dataset.
    pub sections: usize,
    /// Samples free of any issue.
    pub ready_sections: usize,
    pub issues: Vec<DatasetIssue>,
}

impl MapDatasetReadiness {
    pub fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }
}

/// A baseline `Training Dataset` audit over the curated store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetReadiness {
    /// True when every listed curated map contributes only clean samples.
    pub ready: bool,
    pub maps: Vec<MapDatasetReadiness>,
    pub maps_total: usize,
    pub maps_ready: usize,
    pub sections_total: usize,
    pub sections_ready: usize,
}

impl DatasetReadiness {
    pub fn from_maps(maps: Vec<MapDatasetReadiness>) -> Self {
        let maps_total = maps.len();
        let maps_ready = maps.iter().filter(|map| map.is_ready()).count();
        let sections_total = maps.iter().map(|map| map.sections).sum();
        let sections_ready = maps.iter().map(|map| map.ready_sections).sum();

        Self {
            ready: maps_total > 0 && maps_ready == maps_total,
            maps,
            maps_total,
            maps_ready,
            sections_total,
            sections_ready,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perspective_neutral_accepts_physical_prose() {
        let prose = "Rectangular chamber of fitted granite blocks with a shallow dais at the \
            northern end, two braziers flanking the entry, and a collapsed stair descending into \
            standing water to the east.";
        assert!(is_perspective_neutral(prose));
    }

    #[test]
    fn test_perspective_neutral_rejects_camera_style_and_grid_directives() {
        for contaminated in [
            "ttrpg battlemap, flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless",
            "Top-down view of a stone crypt with a sarcophagus",
            "Highly detailed digital art of a tavern interior",
            "A 40x30 grid of timber floorboards (1 Grid Cell = 5ft)",
        ] {
            assert!(
                !is_perspective_neutral(contaminated),
                "must be rejected: {contaminated}"
            );
        }
    }

    #[test]
    fn test_sample_description_prefers_section_then_map_description() {
        let section = MapSection::new(0, 0, "  Crypt with shattered sarcophagus  ");
        assert_eq!(
            sample_description(&section, Some("Map level prose")),
            Some("Crypt with shattered sarcophagus")
        );

        let undescribed = MapSection::new(16, 0, "   ");
        assert_eq!(
            sample_description(&undescribed, Some("Map level prose")),
            Some("Map level prose")
        );
        assert_eq!(sample_description(&undescribed, Some("  ")), None);
        assert_eq!(sample_description(&undescribed, None), None);
    }

    #[test]
    fn test_readiness_requires_every_map_to_be_clean() {
        let clean = MapDatasetReadiness {
            id: "clean".into(),
            theme: "dungeon".into(),
            sections: 4,
            ready_sections: 4,
            issues: Vec::new(),
        };
        let dirty = MapDatasetReadiness {
            id: "dirty".into(),
            theme: "cave".into(),
            sections: 2,
            ready_sections: 1,
            issues: vec![DatasetIssue::new(
                DatasetIssueCode::PhysicalDescriptionNotNeutral,
                "section (16, 0)",
            )],
        };

        let all_clean = DatasetReadiness::from_maps(vec![clean.clone()]);
        assert!(all_clean.ready);
        assert_eq!(all_clean.sections_total, 4);
        assert_eq!(all_clean.sections_ready, 4);

        let mixed = DatasetReadiness::from_maps(vec![clean, dirty]);
        assert!(!mixed.ready);
        assert_eq!(mixed.maps_total, 2);
        assert_eq!(mixed.maps_ready, 1);
        assert_eq!(mixed.sections_total, 6);
        assert_eq!(mixed.sections_ready, 5);

        // An empty store is not a dataset.
        assert!(!DatasetReadiness::from_maps(Vec::new()).ready);
    }

    #[test]
    fn test_issue_codes_serialize_as_snake_case() {
        let issue = DatasetIssue::new(DatasetIssueCode::SectionUnresolvable, "section (0, 16)");
        let json = serde_json::to_string(&issue).unwrap();
        assert!(json.contains("\"section_unresolvable\""), "{json}");
    }
}
