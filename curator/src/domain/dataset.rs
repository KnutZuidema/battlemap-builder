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

use crate::domain::{MapSection, Theme};

/// Camera angles, perspective directives, art-style meta-prompts and grid mechanics that are
/// renderer-owned and therefore never allowed inside a `Physical Description` (ADR-0002).
///
/// The list is deliberately the vocabulary the Dungeon Forge description prompt forbids plus the
/// `Trigger Phrase` tokens themselves: a description carrying any of them is caption text that
/// leaked into the dataset's descriptive field.
static NON_NEUTRAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(dungeonforge|ttrpg|battlemap|battlemaps|flat\s+2d|2d|top[\s-]?down|bird'?s[\s-]?eye|isometric|aerial|orthographic|perpendicular|camera|plan\s+view|perspective|poch[eé]|cut\s+walls?|gridless|no\s+grid|grid|photorealistic|digital\s+art|concept\s+art|highly\s+detailed|4k|8k)\b",
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

    /// Remediation instruction to resolve this defect.
    pub fn remediation(&self) -> &'static str {
        match self {
            Self::GroundLayerMissing => "Restore 0_ground.png in data/curated/<id>/",
            Self::GroundImageUnreadable => "Re-ingest the map; an export would skip it entirely",
            Self::GridBelowMinimum => {
                "Recalibrate the grid (PATCH /api/curated/{id} with a grid that fits the Ground layer)"
            }
            Self::SectionUnresolvable => {
                "Regrid, or re-curate the map; the export skips that section"
            }
            Self::PhysicalDescriptionMissing => {
                "POST /api/curated/{id}/describe with {\"apply\": true} to regenerate and persist the description and its Map Section descriptions"
            }
            Self::PhysicalDescriptionNotNeutral => {
                "POST /api/curated/{id}/describe with {\"apply\": true} to regenerate the description under the strict neutral prompt"
            }
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

    /// Extracts aggregated blocking issues with remediation advice.
    pub fn blocking_issues(&self) -> Vec<BlockingIssue> {
        if self.maps_total == 0 {
            return vec![BlockingIssue {
                code: "empty_dataset".to_string(),
                remediation: "Curate at least one battlemap with 16×16 Map Sections in this scope before freezing a dataset snapshot".to_string(),
                count: 0,
                maps: Vec::new(),
            }];
        }

        let mut issues_by_code: std::collections::BTreeMap<&'static str, (usize, Vec<String>)> =
            std::collections::BTreeMap::new();

        for map in &self.maps {
            for issue in &map.issues {
                let entry = issues_by_code
                    .entry(issue.code.as_str())
                    .or_insert_with(|| (0, Vec::new()));
                entry.0 += 1;
                if !entry.1.contains(&map.id) {
                    entry.1.push(map.id.clone());
                }
            }
        }

        issues_by_code
            .into_iter()
            .map(|(code_str, (count, maps))| {
                let remediation = match code_str {
                    "ground_layer_missing" => DatasetIssueCode::GroundLayerMissing.remediation(),
                    "ground_image_unreadable" => DatasetIssueCode::GroundImageUnreadable.remediation(),
                    "grid_below_minimum" => DatasetIssueCode::GridBelowMinimum.remediation(),
                    "section_unresolvable" => DatasetIssueCode::SectionUnresolvable.remediation(),
                    "physical_description_missing" => DatasetIssueCode::PhysicalDescriptionMissing.remediation(),
                    "physical_description_not_neutral" => DatasetIssueCode::PhysicalDescriptionNotNeutral.remediation(),
                    _ => "Review and fix dataset issues in the Curated Library",
                };
                BlockingIssue {
                    code: code_str.to_string(),
                    remediation: remediation.to_string(),
                    count,
                    maps,
                }
            })
            .collect()
    }
}

/// A blocking issue preventing a dataset scope from being frozen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockingIssue {
    /// Machine-readable snake_case issue code.
    pub code: String,
    /// Remediation instruction to resolve the issue.
    pub remediation: String,
    /// Number of occurrences of this issue across all affected maps.
    pub count: usize,
    /// IDs of curated maps affected by this issue.
    pub maps: Vec<String>,
}

/// An immutable frozen copy of a Training Dataset carrying a content digest (ADR-0009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetSnapshot {
    /// Unique snapshot identifier (e.g. `ds-dungeon-20261024-180000-001`).
    pub id: String,
    /// Frozen scope theme (`None` means all themes).
    pub theme: Option<Theme>,
    /// Creation timestamp (UTC).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Hex-encoded SHA-256 content digest of the exported dataset ZIP archive.
    pub sha256: String,
    /// Size of the ZIP archive in bytes.
    pub size_bytes: u64,
    /// Number of ready curated maps included in the snapshot.
    pub maps_count: usize,
    /// Number of 16×16 Map Section samples included in the ZIP archive.
    pub samples_count: usize,
    /// Trigger Phrase prepended to captions (`TRIGGER_PHRASE`).
    pub trigger_phrase: String,
    /// Storage blob key path (`artifacts/datasets/{id}.zip`).
    pub artifact_key: String,
    /// Public URL for the artifact (when configured with R2_PUBLIC_URL), or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_url: Option<String>,
}

impl DatasetSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        theme: Option<Theme>,
        created_at: chrono::DateTime<chrono::Utc>,
        sha256: impl Into<String>,
        size_bytes: u64,
        maps_count: usize,
        samples_count: usize,
        trigger_phrase: impl Into<String>,
        artifact_key: impl Into<String>,
        artifact_url: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            theme,
            created_at,
            sha256: sha256.into(),
            size_bytes,
            maps_count,
            samples_count,
            trigger_phrase: trigger_phrase.into(),
            artifact_key: artifact_key.into(),
            artifact_url,
        }
    }
}
/// Real-time progress update emitted during dataset snapshot freezing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreezeProgressUpdate {
    /// Progress phase: "starting", "auditing", "packaging", "finalizing", "complete", "error".
    pub phase: String,
    /// Number of curated maps audited or packaged so far.
    pub completed_maps: usize,
    /// Total number of curated maps in scope.
    pub total_maps: usize,
    /// Number of 16x16 sample tiles cropped, resampled, and written to the archive so far.
    pub completed_samples: usize,
    /// Total number of 16x16 sample tiles expected to be written to the archive.
    pub total_samples: usize,
    /// ID of the map currently being processed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_map: Option<String>,
    /// (col, row) coordinates of the section currently being cropped/resampled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_section: Option<(u32, u32)>,
    /// Human-readable status description for UI progress banners.
    pub message: String,
    /// The resulting DatasetSnapshot once phase == "complete".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<DatasetSnapshot>,
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
            "dungeonforge battlemap",
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
