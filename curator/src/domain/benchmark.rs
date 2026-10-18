//! Domain models and specifications for AI battlemap generation and benchmarks.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::{Candidate, Source, Theme};
pub use crate::domain::prompts::{base64_bytes, ReferenceImage, SYSTEM_INSTRUCTION_GENERATE};

static BENCHMARK_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Target AI generation model identifiers matching `server/services/modelCatalog.js`.
pub const MODEL_GEMINI_31_FLASH: &str = "google/gemini-3.1-flash-image";
pub const MODEL_GEMINI_31_FLASH_LITE: &str = "google/gemini-3.1-flash-lite-image";
pub const MODEL_MUSE: &str = "meta/muse-image";
pub const MODEL_SEEDREAM_50_FLASH: &str = "bytedance-seed/seedream-5-0-flash";

/// All standard benchmark models.
pub const ALL_BENCHMARK_MODELS: &[&str] = &[
    MODEL_GEMINI_31_FLASH,
    MODEL_GEMINI_31_FLASH_LITE,
    MODEL_MUSE,
    MODEL_SEEDREAM_50_FLASH,
];

/// The API endpoint kind used by OpenRouter for a given model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelApiType {
    /// Chat completions endpoint (`/api/v1/chat/completions`) with modalities=["image", "text"].
    Chat,
    /// Direct images endpoint (`/api/v1/images`).
    Images,
}

impl fmt::Display for ModelApiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chat => write!(f, "chat"),
            Self::Images => write!(f, "images"),
        }
    }
}

/// Pricing structure for calculating token or per-image inference costs in USD.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPricing {
    /// Cost per 1,000,000 input prompt tokens in USD.
    pub input_per_million: f64,
    /// Cost per 1,000,000 output completion tokens in USD.
    pub output_per_million: f64,
    /// Flat cost per generated image in USD (if applicable).
    pub image_output: f64,
}

impl ModelPricing {
    pub const fn new(input_per_million: f64, output_per_million: f64, image_output: f64) -> Self {
        Self {
            input_per_million,
            output_per_million,
            image_output,
        }
    }

    /// Calculate estimated cost in USD given prompt tokens and completion tokens.
    pub fn calculate_cost(&self, prompt_tokens: Option<u32>, completion_tokens: Option<u32>) -> f64 {
        if self.input_per_million == 0.0 && self.output_per_million == 0.0 && self.image_output > 0.0 {
            return self.image_output;
        }

        let p_cost = prompt_tokens
            .map(|pt| (pt as f64 / 1_000_000.0) * self.input_per_million)
            .unwrap_or(0.0);
        let c_cost = completion_tokens
            .map(|ct| (ct as f64 / 1_000_000.0) * self.output_per_million)
            .unwrap_or(0.0);

        p_cost + c_cost + self.image_output
    }
}

/// Detailed specification for a known benchmark model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkModelSpec {
    pub id: String,
    pub label: String,
    pub api_type: ModelApiType,
    pub pricing: ModelPricing,
    #[serde(default)]
    pub has_vision: bool,
}

impl BenchmarkModelSpec {
    pub fn find(model_id: &str) -> Option<Self> {
        match model_id {
            MODEL_GEMINI_31_FLASH => Some(Self {
                id: MODEL_GEMINI_31_FLASH.to_string(),
                label: "Google Gemini 3.1 Flash Image".to_string(),
                api_type: ModelApiType::Chat,
                pricing: ModelPricing::new(0.50, 3.00, 0.00006),
                has_vision: true,
            }),
            MODEL_GEMINI_31_FLASH_LITE => Some(Self {
                id: MODEL_GEMINI_31_FLASH_LITE.to_string(),
                label: "Google Gemini 3.1 Flash Lite Image".to_string(),
                api_type: ModelApiType::Chat,
                pricing: ModelPricing::new(0.25, 1.50, 0.00003),
                has_vision: true,
            }),
            MODEL_MUSE => Some(Self {
                id: MODEL_MUSE.to_string(),
                label: "Meta Muse Image".to_string(),
                api_type: ModelApiType::Images,
                pricing: ModelPricing::new(0.0, 0.0, 0.01),
                has_vision: true,
            }),
            MODEL_SEEDREAM_50_FLASH => Some(Self {
                id: MODEL_SEEDREAM_50_FLASH.to_string(),
                label: "ByteDance Seedream 5.0 Flash".to_string(),
                api_type: ModelApiType::Images,
                pricing: ModelPricing::new(0.0, 0.0, 0.018),
                has_vision: true,
            }),
            _ => None,
        }
    }

    /// Whether this model supports multi-modal visual reference conditioning.
    pub fn has_vision(&self) -> bool {
        self.has_vision
    }

    /// Estimated cost per map in USD for UI pre-flight cost projections.
    pub fn estimated_cost_per_map(&self) -> f64 {
        match self.id.as_str() {
            MODEL_GEMINI_31_FLASH => 0.07,
            MODEL_GEMINI_31_FLASH_LITE => 0.035,
            MODEL_MUSE => 0.01,
            MODEL_SEEDREAM_50_FLASH => 0.018,
            _ => 0.05,
        }
    }
}

/// The 6 standard battlemap benchmark archetypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkArchetype {
    StoneCrypt,
    RiversideAmbush,
    TavernCommonRoom,
    UnderdarkCavern,
    DesertRuins,
    SewersSluice,
}

impl BenchmarkArchetype {
    /// All 6 standard benchmark archetypes in canonical evaluation order.
    pub const ALL: [BenchmarkArchetype; 6] = [
        Self::StoneCrypt,
        Self::RiversideAmbush,
        Self::TavernCommonRoom,
        Self::UnderdarkCavern,
        Self::DesertRuins,
        Self::SewersSluice,
    ];

    /// Human-readable archetype title.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::StoneCrypt => "Stone Crypt",
            Self::RiversideAmbush => "Riverside Ambush",
            Self::TavernCommonRoom => "Tavern Common Room",
            Self::UnderdarkCavern => "Underdark Cavern",
            Self::DesertRuins => "Desert Ruins",
            Self::SewersSluice => "Sewers & Sluice",
        }
    }

    /// URL/filename friendly slug.
    pub const fn slug(&self) -> &'static str {
        match self {
            Self::StoneCrypt => "stone-crypt",
            Self::RiversideAmbush => "riverside-ambush",
            Self::TavernCommonRoom => "tavern-common-room",
            Self::UnderdarkCavern => "underdark-cavern",
            Self::DesertRuins => "desert-ruins",
            Self::SewersSluice => "sewers-sluice",
        }
    }

    /// Canonical benchmark prompt text.
    pub const fn prompt(&self) -> &'static str {
        match self {
            Self::StoneCrypt => {
                "Flat 2D stone dungeon crypt, central sarcophagus, poché masonry walls, iron torches, gridless."
            }
            Self::RiversideAmbush => {
                "Flat 2D forest river crossing, wooden footbridge, rocky banks, dirt path, gridless."
            }
            Self::TavernCommonRoom => {
                "Flat 2D fantasy tavern interior, wooden tables, hearth fireplace, bar counter, timber poché walls, gridless."
            }
            Self::UnderdarkCavern => {
                "Flat 2D subterranean cavern, bioluminescent mushrooms, underground stream, rough natural stone walls, gridless."
            }
            Self::DesertRuins => {
                "Flat 2D ancient sandstone temple ruins, crumbling pillars, sand drifts, exposed flagstones, gridless."
            }
            Self::SewersSluice => {
                "Flat 2D underground masonry sewer junction, iron grating, green water channels, stone walkways, gridless."
            }
        }
    }

    /// Associated thematic curation bucket.
    pub const fn default_theme(&self) -> Theme {
        match self {
            Self::StoneCrypt => Theme::Dungeon,
            Self::RiversideAmbush => Theme::Wilderness,
            Self::TavernCommonRoom => Theme::Tavern,
            Self::UnderdarkCavern => Theme::Cave,
            Self::DesertRuins => Theme::Ruins,
            Self::SewersSluice => Theme::Dungeon,
        }
    }
}

impl fmt::Display for BenchmarkArchetype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl FromStr for BenchmarkArchetype {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase().replace(['_', ' '], "-");
        match normalized.as_str() {
            "stone-crypt" | "stone-dungeon-crypt" | "crypt" => Ok(Self::StoneCrypt),
            "riverside-ambush" | "riverside" | "river-crossing" => Ok(Self::RiversideAmbush),
            "tavern-common-room" | "tavern" | "tavern-interior" => Ok(Self::TavernCommonRoom),
            "underdark-cavern" | "underdark" | "cavern" => Ok(Self::UnderdarkCavern),
            "desert-ruins" | "ruins" | "desert-temple-ruins" => Ok(Self::DesertRuins),
            "sewers-sluice" | "sewers" | "sewer-junction" => Ok(Self::SewersSluice),
            _ => Err(format!("Unknown benchmark archetype: '{s}'")),
        }
    }
}

/// Request to generate an image via a model engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
    #[serde(default = "default_aspect_ratio")]
    pub aspect_ratio: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ReferenceImage>,
}

fn default_aspect_ratio() -> String {
    "1:1".to_string()
}

impl GenerationRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            system_instruction: None,
            aspect_ratio: default_aspect_ratio(),
            images: Vec::new(),
        }
    }

    pub fn with_system_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.system_instruction = Some(instruction.into());
        self
    }

    pub fn with_aspect_ratio(mut self, aspect_ratio: impl Into<String>) -> Self {
        self.aspect_ratio = aspect_ratio.into();
        self
    }

    pub fn with_image(mut self, image: ReferenceImage) -> Self {
        self.images.push(image);
        self
    }

    pub fn with_images(mut self, images: impl IntoIterator<Item = ReferenceImage>) -> Self {
        self.images.extend(images);
        self
    }

    pub fn with_battlemap_defaults(mut self) -> Self {
        self.system_instruction = Some(SYSTEM_INSTRUCTION_GENERATE.to_string());
        self
    }

    pub fn with_default_battlemap_system_instruction(self) -> Self {
        self.with_battlemap_defaults()
    }
}

/// Output from a successful generation request.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationResponse {
    pub image_bytes: Vec<u8>,
    pub model: String,
    pub prompt: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
    pub cost_usd: f64,
}

/// Companion metadata stored in `data/staging/generated/{id}.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMetadata {
    pub id: String,
    pub model: String,
    pub prompt_name: String,
    pub prompt_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
    pub cost_usd: f64,
}

impl BenchmarkMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        prompt_name: impl Into<String>,
        prompt_text: impl Into<String>,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
        latency_ms: u64,
        cost_usd: f64,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            prompt_name: prompt_name.into(),
            prompt_text: prompt_text.into(),
            prompt_tokens,
            completion_tokens,
            latency_ms,
            cost_usd,
        }
    }

    /// Converts benchmark metadata into a staging `Candidate`.
    pub fn to_candidate(&self) -> Candidate {
        Candidate {
            id: self.id.clone(),
            source: Source::Generated,
            filename: format!("{}.png", self.id),
            dimensions: (1024, 1024),
            created_at: Utc::now(),
            source_url: Some(format!("openrouter://{}", self.model)),
            title: Some(format!("{}: {}", self.prompt_name, self.model)),
            author: Some(self.model.clone()),
            parsed_gridless: Some(true),
            parsed_dimensions: Some((1024, 1024)),
            grid: Default::default(),
            layers: Vec::new(),
            parent_group_id: None,
        }
    }
}

/// Complete benchmark generation result containing both metadata and image bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkResult {
    pub metadata: BenchmarkMetadata,
    pub image_bytes: Vec<u8>,
}

impl BenchmarkResult {
    pub fn new(metadata: BenchmarkMetadata, image_bytes: Vec<u8>) -> Self {
        Self {
            metadata,
            image_bytes,
        }
    }

    pub fn id(&self) -> &str {
        &self.metadata.id
    }
}

/// Generates a standardized, filesystem-safe benchmark ID.
/// Example: `bench_gemini_3_1_flash_image_stone_crypt_1728658000_1`
pub fn generate_benchmark_id(model_id: &str, archetype_slug: &str) -> String {
    let clean_model = model_id
        .replace('/', "_")
        .replace(['-', '.'], "_")
        .to_lowercase();
    let clean_slug = archetype_slug
        .replace(['-', ' '], "_")
        .to_lowercase();
    let timestamp = Utc::now().timestamp();
    let counter = BENCHMARK_COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("bench_{clean_model}_{clean_slug}_{timestamp}_{counter}")
}

/// Helper to convert an arbitrary prompt name into a filesystem-safe slug.
pub fn slugify_prompt_name(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in name.chars() {
        if c == '\'' {
            continue;
        }
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "custom-prompt".to_string()
    } else {
        trimmed
    }
}

/// Custom prompt specification for ad-hoc benchmark generation runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPrompt {
    pub name: String,
    pub prompt: String,
}

impl CustomPrompt {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
        }
    }

    pub fn slug(&self) -> String {
        slugify_prompt_name(&self.name)
    }
}

/// Unified batch benchmark item combining archetypes and custom prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchBenchmarkItem {
    pub name: String,
    pub prompt: String,
    pub slug: String,
}

impl BatchBenchmarkItem {
    pub fn from_archetype(archetype: &BenchmarkArchetype) -> Self {
        Self {
            name: archetype.name().to_string(),
            prompt: archetype.prompt().to_string(),
            slug: archetype.slug().to_string(),
        }
    }

    pub fn from_custom(custom: &CustomPrompt) -> Self {
        Self {
            name: custom.name.clone(),
            prompt: custom.prompt.clone(),
            slug: custom.slug(),
        }
    }

    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        let name_str = name.into();
        let slug = slugify_prompt_name(&name_str);
        Self {
            name: name_str,
            prompt: prompt.into(),
            slug,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archetypes_count_and_prompts() {
        assert_eq!(BenchmarkArchetype::ALL.len(), 6);
        for arch in BenchmarkArchetype::ALL {
            assert!(!arch.name().is_empty());
            assert!(!arch.slug().is_empty());
            assert!(!arch.prompt().is_empty());
            assert!(arch.prompt().contains("gridless"));
        }
    }

    #[test]
    fn test_archetype_from_str() {
        assert_eq!(
            "stone-crypt".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::StoneCrypt
        );
        assert_eq!(
            "riverside_ambush".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::RiversideAmbush
        );
        assert_eq!(
            "Tavern Common Room".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::TavernCommonRoom
        );
        assert_eq!(
            "underdark-cavern".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::UnderdarkCavern
        );
        assert_eq!(
            "desert-ruins".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::DesertRuins
        );
        assert_eq!(
            "sewers-sluice".parse::<BenchmarkArchetype>().unwrap(),
            BenchmarkArchetype::SewersSluice
        );
        assert!("invalid-archetype".parse::<BenchmarkArchetype>().is_err());
    }

    #[test]
    fn test_model_specs() {
        assert_eq!(ALL_BENCHMARK_MODELS.len(), 4);
        for model_id in ALL_BENCHMARK_MODELS {
            let spec = BenchmarkModelSpec::find(model_id).expect("model spec found");
            assert_eq!(&spec.id, model_id);
            assert!(!spec.label.is_empty());
            assert!(spec.has_vision());
            assert!(spec.has_vision);
        }

        let flash = BenchmarkModelSpec::find(MODEL_GEMINI_31_FLASH).unwrap();
        assert_eq!(flash.api_type, ModelApiType::Chat);

        let muse = BenchmarkModelSpec::find(MODEL_MUSE).unwrap();
        assert_eq!(muse.api_type, ModelApiType::Images);
        assert_eq!(muse.pricing.calculate_cost(None, None), 0.01);
    }

    #[test]
    fn test_benchmark_metadata_serialization() {
        let meta = BenchmarkMetadata::new(
            "bench_test_123",
            MODEL_GEMINI_31_FLASH,
            "Stone Crypt",
            "Flat 2D stone dungeon crypt...",
            Some(120),
            Some(450),
            1540,
            0.035,
        );

        let json = serde_json::to_string_pretty(&meta).expect("serialized");
        assert!(json.contains("\"id\": \"bench_test_123\""));
        assert!(json.contains("\"model\": \"google/gemini-3.1-flash-image\""));
        assert!(json.contains("\"prompt_name\": \"Stone Crypt\""));
        assert!(json.contains("\"prompt_text\": \"Flat 2D stone dungeon crypt...\""));
        assert!(json.contains("\"prompt_tokens\": 120"));
        assert!(json.contains("\"completion_tokens\": 450"));
        assert!(json.contains("\"latency_ms\": 1540"));
        assert!(json.contains("\"cost_usd\": 0.035"));

        let deserialized: BenchmarkMetadata = serde_json::from_str(&json).expect("deserialized");
        assert_eq!(deserialized, meta);

        let candidate = meta.to_candidate();
        assert_eq!(candidate.id, "bench_test_123");
        assert_eq!(candidate.source, Source::Generated);
        assert_eq!(candidate.filename, "bench_test_123.png");
    }

    #[test]
    fn test_generate_benchmark_id() {
        let id1 = generate_benchmark_id(MODEL_GEMINI_31_FLASH, "stone-crypt");
        let id2 = generate_benchmark_id(MODEL_GEMINI_31_FLASH, "stone-crypt");
        assert!(id1.starts_with("bench_google_gemini_3_1_flash_image_stone_crypt_"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_custom_prompt_and_batch_item() {
        let custom = CustomPrompt::new("Dragon's Lair", "Flat 2D cavern with gold piles");
        assert_eq!(custom.slug(), "dragons-lair");
        assert_eq!(custom.name, "Dragon's Lair");

        let batch_item = BatchBenchmarkItem::from_custom(&custom);
        assert_eq!(batch_item.name, "Dragon's Lair");
        assert_eq!(batch_item.slug, "dragons-lair");
        assert_eq!(batch_item.prompt, "Flat 2D cavern with gold piles");

        let arch_item = BatchBenchmarkItem::from_archetype(&BenchmarkArchetype::StoneCrypt);
        assert_eq!(arch_item.name, "Stone Crypt");
        assert_eq!(arch_item.slug, "stone-crypt");
        assert!(arch_item.prompt.contains("stone dungeon crypt"));
    }

    #[test]
    fn test_model_spec_estimated_costs() {
        let flash = BenchmarkModelSpec::find(MODEL_GEMINI_31_FLASH).unwrap();
        assert_eq!(flash.estimated_cost_per_map(), 0.07);

        let flash_lite = BenchmarkModelSpec::find(MODEL_GEMINI_31_FLASH_LITE).unwrap();
        assert_eq!(flash_lite.estimated_cost_per_map(), 0.035);

        let muse = BenchmarkModelSpec::find(MODEL_MUSE).unwrap();
        assert_eq!(muse.estimated_cost_per_map(), 0.01);

        let seedream = BenchmarkModelSpec::find(MODEL_SEEDREAM_50_FLASH).unwrap();
        assert_eq!(seedream.estimated_cost_per_map(), 0.018);
    }

    #[test]
    fn test_generation_request_battlemap_defaults_and_images() {
        let req = GenerationRequest::new(MODEL_GEMINI_31_FLASH, "A dark crypt")
            .with_battlemap_defaults()
            .with_aspect_ratio("1:1")
            .with_image(ReferenceImage::png("ref1", vec![1, 2, 3]))
            .with_images([ReferenceImage::jpeg("ref2", vec![4, 5, 6])]);

        assert_eq!(req.model, MODEL_GEMINI_31_FLASH);
        assert_eq!(req.prompt, "A dark crypt");
        assert_eq!(req.aspect_ratio, "1:1");
        assert_eq!(
            req.system_instruction,
            Some(SYSTEM_INSTRUCTION_GENERATE.to_string())
        );
        assert_eq!(req.images.len(), 2);
        assert_eq!(req.images[0].label, "ref1");
        assert_eq!(req.images[1].label, "ref2");

        // Check serialization
        let json = serde_json::to_string(&req).expect("serialize req");
        assert!(json.contains("\"images\":["));
        assert!(json.contains("\"system_instruction\":"));

        let deserialized: GenerationRequest =
            serde_json::from_str(&json).expect("deserialize req");
        assert_eq!(deserialized, req);

        // Also verify with_default_battlemap_system_instruction alias
        let req_alias = GenerationRequest::new(MODEL_MUSE, "Cave")
            .with_default_battlemap_system_instruction();
        assert_eq!(
            req_alias.system_instruction,
            Some(SYSTEM_INSTRUCTION_GENERATE.to_string())
        );
        assert!(req_alias.images.is_empty());

        // Serialized without images should omit images field
        let json_no_images = serde_json::to_string(&req_alias).expect("serialize");
        assert!(!json_no_images.contains("\"images\""));
    }
}
