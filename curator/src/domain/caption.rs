//! LoRA training caption generation formatted for Kohya and AI-Toolkit.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::domain::{Candidate, CuratedMap, Theme};

/// The Dungeon Forge `Trigger Phrase`: the fixed head of every exported LoRA caption.
///
/// Every caption is `"{TRIGGER_PHRASE}, {theme}, {details}"`, so the phrase is applied exactly
/// once and only here — it is never stored inside a `Physical Description` (ADR-0002, ADR-0007).
pub const TRIGGER_PHRASE: &str = "dungeonforge battlemap";

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
        r"(?i)\b(dungeonforge|ttrpg|battlemap|battlemaps|flat\s+2d|2d|plan\s+view|perpendicular|orthographic|camera|poch[eé]|cut\s+walls?|strictly|gridless|no\s+grid|without\s+grid|gridded|grid)\b",
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
    /// `dungeonforge battlemap, {theme}, {sanitized_tags_or_prompt}`
    pub fn generate_with_theme_str(theme_str: &str, tags: &[String], prompt_or_title: Option<&str>) -> String {
        let mut tokens: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Seed seen with theme and prefix terms so they are never duplicated in the suffix
        seen.insert(theme_str.to_lowercase());
        for word in &[
            "dungeonforge", "ttrpg", "battlemap", "battlemaps", "flat", "2d",
            "plan", "view", "perpendicular", "orthographic", "camera", "poché",
            "poche", "cut", "walls", "wall", "strictly", "gridless", "grid",
            "no grid",
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
            format!("{TRIGGER_PHRASE}, {theme_str}")
        } else {
            let details = tokens.join(", ");
            format!("{TRIGGER_PHRASE}, {theme_str}, {details}")
        }
    }

    /// Generates a standardized LoRA caption string adhering to Kohya / AI-Toolkit format from a `Theme`.
    pub fn generate(theme: &Theme, tags: &[String], prompt_or_title: Option<&str>) -> String {
        Self::generate_with_theme_str(theme.as_str(), tags, prompt_or_title)
    }

    /// Generates a standardized LoRA caption string adhering to Kohya / AI-Toolkit format from a theme string, tags, and description.
    pub fn generate_with_description(
        theme_str: &str,
        tags: &[String],
        description: Option<&str>,
    ) -> String {
        Self::generate_with_theme_str(theme_str, tags, description)
    }

    /// Convenience helper to generate caption for an approved `CuratedMap` and optional `Candidate`.
    ///
    /// If `map.description` is present and non-empty, it is used as the descriptive text source;
    /// otherwise falls back to `candidate.title`.
    pub fn generate_for_curated(map: &CuratedMap, candidate: Option<&Candidate>) -> String {
        let text_source = map
            .description
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| candidate.and_then(|c| c.title.as_deref()));
        Self::generate_with_theme_str(map.primary_theme(), &map.tags, text_source)
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
    use crate::domain::Source;

    #[test]
    fn test_caption_generator_format_and_prefix() {
        let caption = CaptionGenerator::generate(&Theme::Dungeon, &[], None);
        assert_eq!(caption, "dungeonforge battlemap, dungeon");
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

        assert!(caption.starts_with(TRIGGER_PHRASE));
        assert!(caption.contains("dungeon"));
        assert!(caption.contains("boss-room"));
        assert!(caption.contains("crystal spire"));
        // Redundant words should not appear
        assert!(!caption.contains("50x50"));
        assert!(!caption.contains("no grid"));
    }

    #[test]
    fn test_caption_applies_trigger_phrase_exactly_once_for_legacy_descriptions() {
        // Manifests curated before Map Section descriptions existed stored a whole caption in the
        // section description field. Re-captioning such a legacy description must not duplicate
        // the Trigger Phrase.
        let legacy = format!("{TRIGGER_PHRASE}, dungeon, obsidian altar, iron braziers");
        let caption =
            CaptionGenerator::generate_with_description("dungeon", &["crypt".into()], Some(&legacy));
        assert!(caption.starts_with(TRIGGER_PHRASE), "{caption}");
        assert_eq!(caption.matches(TRIGGER_PHRASE).count(), 1, "{caption}");
        // "dungeonforge" survives only inside the single applied Trigger Phrase.
        assert_eq!(caption.matches("dungeonforge").count(), 1, "{caption}");
        assert!(caption.contains("obsidian altar"), "{caption}");
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

    #[test]
    fn test_caption_generator_generate_with_description() {
        let tags = vec!["boss-room".to_string()];
        let description = "Ancient crypt with carved limestone sarcophagus, burning braziers, and solid timber doors.";
        let caption = CaptionGenerator::generate_with_description("dungeon", &tags, Some(description));

        assert!(caption.starts_with(TRIGGER_PHRASE));
        assert!(caption.contains("dungeon"));
        assert!(caption.contains("boss-room"));
        assert!(caption.contains("sarcophagus"));
        assert!(caption.contains("doors"));

        let no_desc = CaptionGenerator::generate_with_description("cave", &[], None);
        assert_eq!(no_desc, "dungeonforge battlemap, cave");
    }

    #[test]
    fn test_caption_generator_generate_for_curated_prioritizes_description() {
        let mut map = CuratedMap::with_now(
            "curated_test_map",
            vec!["dungeon".to_string()],
            vec!["crypt".to_string()],
            (2048, 2048),
        );

        // 1. Map without description and no candidate
        let cap_plain = CaptionGenerator::generate_for_curated(&map, None);
        assert!(cap_plain.starts_with(TRIGGER_PHRASE));
        assert!(cap_plain.contains("dungeon"));
        assert!(cap_plain.contains("crypt"));

        // 2. Map without description, candidate present
        let candidate = Candidate::with_now("cand_1", Source::Reddit, "raw.png", (2048, 2048))
            .with_title("Wooden bridge over river [50x50]");
        let cap_cand = CaptionGenerator::generate_for_curated(&map, Some(&candidate));
        assert!(cap_cand.contains("bridge"));
        assert!(cap_cand.contains("water stream"));

        // 3. Map with description present -> description takes priority over candidate title
        map = map.with_description("Granite throne room with stone pillars and ornate iron altar.");
        let cap_desc = CaptionGenerator::generate_for_curated(&map, Some(&candidate));
        assert!(cap_desc.contains("throne"));
        assert!(cap_desc.contains("pillars"));
        assert!(cap_desc.contains("altar"));
        assert!(!cap_desc.contains("bridge"));

        // 4. Map with description and no candidate
        let cap_desc_no_cand = CaptionGenerator::generate_for_curated(&map, None);
        assert!(cap_desc_no_cand.contains("throne"));
        assert!(cap_desc_no_cand.contains("pillars"));
        assert!(cap_desc_no_cand.contains("altar"));
    }
}
