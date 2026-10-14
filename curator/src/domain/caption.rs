//! LoRA training caption generation formatted for Kohya and AI-Toolkit.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::domain::{Candidate, CuratedMap, Theme};

/// Standard fixed prefix for Kohya / AI-Toolkit LoRA battlemap training.
pub const LORA_CAPTION_PREFIX: &str =
    "ttrpg battlemap, flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless";

/// Known architectural feature patterns to detect in candidate prompts, titles, or tags.
pub const ARCHITECTURAL_FEATURES: &[(&str, &[&str])] = &[
    ("stone walls", &["stone wall", "stone walls", "masonry", "stone crypt"]),
    ("timber walls", &["timber wall", "timber walls", "wood wall", "wooden wall"]),
    (
        "cavern walls",
        &["cavern wall", "rough natural stone", "natural stone walls", "cave wall", "rock walls"],
    ),
    ("doors", &["door", "doors", "doorway", "entryway"]),
    (
        "corridors",
        &["corridor", "corridors", "hallway", "hallways", "passageway", "passageways"],
    ),
    ("stairs", &["stair", "stairs", "staircase", "steps"]),
    ("pillars", &["pillar", "pillars", "column", "columns"]),
    ("altar", &["altar", "shrine"]),
    ("sarcophagus", &["sarcophagus", "tomb", "crypt"]),
    ("fireplace", &["fireplace", "hearth"]),
    ("bar counter", &["bar counter", "tavern bar"]),
    (
        "tables and chairs",
        &["tables", "table", "chairs", "chair", "tavern seating"],
    ),
    ("bookshelves", &["bookshelf", "bookshelves", "library"]),
    ("chests", &["chest", "chests", "treasure chest"]),
    (
        "torches",
        &["torch", "torches", "sconce", "sconces", "brazier", "braziers"],
    ),
    ("bridge", &["bridge", "footbridge", "rope bridge"]),
    ("water stream", &["water stream", "stream", "river", "canal", "pool"]),
    ("crystal spire", &["crystal spire", "crystal", "spire"]),
    ("prison cells", &["cell", "cells", "prison cell", "iron bars", "dungeon cell"]),
    ("throne", &["throne", "dais"]),
    ("fountain", &["fountain"]),
];

static DIMENSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[?\b\d+\s*[xX×]\s*\d+\b\]?").expect("valid dimension regex")
});

static REDUNDANT_TERMS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(ttrpg|battlemap|battlemaps|flat\s+2d|2d|plan\s+view|perpendicular|orthographic|camera|poch[eé]|cut\s+walls?|strictly|gridless|no\s+grid|without\s+grid|gridded|grid)\b",
    )
    .expect("valid redundant terms regex")
});

static SPECIAL_CHARS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[\[\]\(\)\{\}"'<>!?;:+=#@$%^&*~`|/\\]"#).expect("valid special chars regex")
});

static MULTI_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid multi space regex"));

/// Generator for standardized LoRA fine-tuning companion caption files.
pub struct CaptionGenerator;

impl CaptionGenerator {
    /// Generates a standardized LoRA caption string adhering to Kohya / AI-Toolkit format:
    /// `ttrpg battlemap, flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless, {theme}, {sanitized_tags_or_prompt}`
    pub fn generate(theme: &Theme, tags: &[String], prompt_or_title: Option<&str>) -> String {
        let theme_str = theme.as_str();
        let mut tokens: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Seed seen with theme and prefix terms so they are never duplicated in the suffix
        seen.insert(theme_str.to_lowercase());
        for word in &[
            "ttrpg", "battlemap", "battlemaps", "flat", "2d", "plan", "view",
            "perpendicular", "orthographic", "camera", "poché", "poche", "cut",
            "walls", "wall", "strictly", "gridless", "grid", "no grid",
        ] {
            seen.insert((*word).to_string());
        }

        // 1. Detect architectural features from prompt or tags
        let combined_text = format!(
            "{} {}",
            prompt_or_title.unwrap_or_default(),
            tags.join(" ")
        )
        .to_lowercase();

        let mut detected_features = Vec::new();
        for (feature_canonical, patterns) in ARCHITECTURAL_FEATURES {
            if patterns.iter().any(|pat| combined_text.contains(pat)) {
                let canonical_lower = feature_canonical.to_lowercase();
                if seen.insert(canonical_lower.clone()) {
                    detected_features.push(canonical_lower);
                }
            }
        }

        // 2. Sanitize and add user tags
        for tag in tags {
            let cleaned = Self::sanitize_fragment(tag);
            if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
                tokens.push(cleaned);
            }
        }

        // 3. Sanitize and add prompt / title phrases if provided
        if let Some(text) = prompt_or_title {
            let phrases = Self::extract_prompt_phrases(text);
            for phrase in phrases {
                if !phrase.is_empty() && seen.insert(phrase.clone()) {
                    tokens.push(phrase);
                }
            }
        }

        // 4. Append detected architectural features
        for feature in detected_features {
            if !tokens.contains(&feature) {
                tokens.push(feature);
            }
        }

        // 5. Build final Kohya / AI-Toolkit caption
        if tokens.is_empty() {
            format!("{LORA_CAPTION_PREFIX}, {theme_str}")
        } else {
            let details = tokens.join(", ");
            format!("{LORA_CAPTION_PREFIX}, {theme_str}, {details}")
        }
    }

    /// Convenience helper to generate caption for an approved `CuratedMap` and optional `Candidate`.
    pub fn generate_for_curated(map: &CuratedMap, candidate: Option<&Candidate>) -> String {
        let title_or_prompt = candidate.and_then(|c| c.title.as_deref());
        Self::generate(&map.theme, &map.tags, title_or_prompt)
    }

    /// Sanitizes an individual tag or short fragment:
    /// lowercases, removes special characters, commas, redundant prefix words, and excess whitespace.
    pub fn sanitize_fragment(input: &str) -> String {
        let mut s = input.to_lowercase();
        s = DIMENSION_RE.replace_all(&s, " ").to_string();
        s = REDUNDANT_TERMS_RE.replace_all(&s, " ").to_string();
        s = SPECIAL_CHARS_RE.replace_all(&s, " ").to_string();
        s = s.replace(',', " ");
        s = MULTI_SPACE_RE.replace_all(&s, " ").to_string();
        s.trim().trim_matches('-').trim().to_string()
    }

    /// Breaks a longer prompt or title into clean, comma-free descriptive phrases.
    pub fn extract_prompt_phrases(input: &str) -> Vec<String> {
        let mut results = Vec::new();
        for segment in input.split([';', '.', ',']) {
            let cleaned = Self::sanitize_fragment(segment);
            if !cleaned.is_empty() {
                results.push(cleaned);
            }
        }
        results
    }
}

/// Generates a standardized LoRA caption string adhering to Kohya / AI-Toolkit format.
pub fn generate_lora_caption(
    theme: &Theme,
    tags: &[String],
    prompt_or_title: Option<&str>,
) -> String {
    CaptionGenerator::generate(theme, tags, prompt_or_title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_caption_generator_format_and_prefix() {
        let caption = CaptionGenerator::generate(&Theme::Dungeon, &[], None);
        assert_eq!(
            caption,
            "ttrpg battlemap, flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless, dungeon"
        );
    }

    #[test]
    fn test_caption_generator_sanitizes_and_deduplicates() {
        let tags = vec![
            "boss-room".to_string(),
            "Dungeon".to_string(),
            "GRIDLESS".to_string(),
        ];
        let caption = CaptionGenerator::generate(
            &Theme::Dungeon,
            &tags,
            Some("[50x50] Crystal Spire [No Grid]"),
        );

        assert!(caption.starts_with(LORA_CAPTION_PREFIX));
        assert!(caption.contains("dungeon"));
        assert!(caption.contains("boss-room"));
        assert!(caption.contains("crystal spire"));
        // Redundant words should not appear
        assert!(!caption.contains("50x50"));
        assert!(!caption.contains("no grid"));
    }

    #[test]
    fn test_caption_generator_detects_architectural_features() {
        let prompt = "Stone crypt with central sarcophagus, iron torches and timber walls.";
        let caption = CaptionGenerator::generate(&Theme::Dungeon, &[], Some(prompt));

        assert!(caption.contains("sarcophagus"));
        assert!(caption.contains("torches"));
        assert!(caption.contains("stone walls"));
        assert!(caption.contains("timber walls"));
    }
}
