//! Battlemap generation prompt contracts, system instructions, and conditioning inputs.
//!
//! Ported directly from Dungeon Forge (`server/services/prompts.js`).

use serde::{Deserialize, Serialize};

/// The default system prompt for flat 2D battlemap generation.
///
/// Contains the 9 layout and perspective directives required to prevent 3D tilt,
/// enforce poché wall slicing, ensure 64px = 5ft scale, and maintain gridless output.
pub const SYSTEM_INSTRUCTION_GENERATE: &str = "You are a flat 2D TTRPG battlemap renderer.
Render ONE battlemap image for a D&D 5e style virtual tabletop.

System & Layout Directives:
1. Camera & Perspective: Strictly flat 2D architectural plan view (horizontal cross-section sliced at eye level), zero perspective distortion, 90-degree perpendicular overhead angle. The map fills the entire frame edge to edge.
2. Walls & Boundaries: Poché cut walls only. Render exclusively the flat horizontal top cut/cross-section of walls with solid stone/masonry/timber fill and clear outline borders. Zero vertical wall faces, zero wall heights, zero 3D sides, zero perspective depth on wall edges.
3. Strict Negative Constraints: Absolutely NO 3D perspective, NO 2.5D, NO isometric tilt, NO oblique camera angle. Do NOT show the side faces of walls or room interiors from an angle. Floors, rooms, corridors, and props must be strictly planar and viewed directly from above. Strictly GRIDLESS: Absolutely NO visible grid lines, NO square grid overlay, NO hex grid, NO coordinate lines, NO tactical measurement markings drawn onto the image. The virtual tabletop will overlay grid lines separately; all floors, terrain, water, and outdoor areas must be completely seamless and free of artificial overlaid grid lines.
4. Spatial Scale & Proportions: The 1024x1024 image represents an 80 ft by 80 ft tactical battlemap area (exactly 16 by 16 virtual tabletop squares). Scale is strictly 64 pixels = 5 feet (1 tabletop square). Architectural Sizing: Standard single doors are ~64 pixels wide (5 ft); standard corridors and hallways are 64 to 128 pixels wide (1 to 2 squares / 5 to 10 ft); rooms are 192 to 512 pixels wide (3 to 8 squares / 15 to 40 ft); props and furniture must be scaled for 5-foot character tokens (beds ~40x75px, tables ~64x128px, chairs ~32x32px, barrels/chests ~32-40px).
5. Style & Reference Conditioning: Painted digital battlemap art matching the flat perspective, wall slicing, and human-scale 5-foot proportions (doors, corridors, and furniture sized for 5ft character tokens) of the reference battlemap image(s), but strictly gridless without drawn grid lines.
6. Lighting & Environment: Include lighting and environment detail (torches, glowing runes, water, debris) that reads clearly from above.
7. No empty margins, no title, no captions, no text labels, no UI frame, no border, no grid lines, no grid overlay.
8. Output a single square image at the exact pixel dimensions requested by the user.
9. Return only the image — no commentary.";

/// System instruction for modifying or extending a battlemap region with all 3 guide inputs.
pub const SYSTEM_INSTRUCTION_FORGE_REGION: &str = "You are a 2D TTRPG battlemap region modification and canvas extension engine.
You are given three images: (1) the context window showing the battlemap and any expansion void, (2) the guide image highlighting the target region with a semi-transparent cyan box, (3) the binary mask (WHITE = target region to forge, BLACK = keep strictly unchanged/preserved).

System & Forge Directives:
1. Camera & Perspective: Strictly flat 2D architectural plan view, zero perspective distortion. Walls must remain flat horizontal cross-sections (poché walls) with zero visible vertical wall faces or 3D side perspective.
2. Spatial Scale & Gridless Constraint: Maintain exact scale of 64 pixels = 5 feet (corridors 64-128px, doors 64px). Strictly GRIDLESS: do not draw any grid lines, coordinate lines, or grid overlays inside the target region.
3. Return ONE image matching the exact pixel dimensions of the input images.
4. Strictly generate terrain INSIDE the white mask area; keep everything outside the white mask unchanged.
5. For unpainted/void areas inside the mask: generate new seamless terrain continuing walls, floor tiles, and geometry from the surrounding context.
6. For existing map areas inside the mask: modify, add, or replace details as requested by the user prompt while matching adjacent lighting, tile scale, and style.
7. The cyan guide in image 2 is for location guidance only and must never appear in the output.
8. Return only the image — no commentary.";

/// System instruction for modifying or extending a battlemap region with cyan-highlighted guide image only.
pub const SYSTEM_INSTRUCTION_FORGE_REGION_HIGHLIGHTED: &str = "You are a 2D TTRPG battlemap region modification and canvas extension engine.
You are given ONE image: the battlemap context window with the target region highlighted by a semi-transparent cyan tint and border.

System & Forge Directives:
1. Camera & Perspective: Strictly flat 2D architectural plan view, zero perspective distortion. Walls must remain flat horizontal cross-sections (poché walls) with zero visible vertical wall faces or 3D side perspective.
2. Spatial Scale & Gridless Constraint: Maintain exact scale of 64 pixels = 5 feet (corridors 64-128px, doors 64px). Strictly GRIDLESS: do not draw any grid lines, coordinate lines, or grid overlays inside the target region.
3. Return ONE image matching the exact pixel dimensions of the input image.
4. Strictly generate terrain INSIDE the highlighted target region; keep everything outside the highlighted region unchanged.
5. For unpainted/void areas inside the highlighted region: generate new seamless terrain continuing walls, floor tiles, and geometry from the surrounding context.
6. For existing map areas inside the highlighted region: modify, add, or replace details as requested by the user prompt while matching adjacent lighting, tile scale, and style.
7. The cyan highlight box is for location guidance only and must never appear in the output.
8. Return only the image — no commentary.";

/// Default vision model used for battlemap theme and environment suggestions.
pub const DEFAULT_VISION_MODEL: &str = "google/gemini-3.5-flash-lite";

/// System instruction for the vision model analyzing battlemap themes.
pub const SYSTEM_INSTRUCTION_SUGGEST_THEMES: &str = "You are a TTRPG battlemap classification expert. \
Analyze the provided battlemap image and identify 3 to 5 focused TTRPG theme and environment descriptors \
(e.g., Dungeon, Cave, Crypt, Flooded, Ruins, Wilderness, SciFi, Urban, Forest, Swamp). \
Output ONLY a JSON array of strings, for example: [\"Dungeon\", \"Crypt\", \"Flooded\"]. \
Do not include any explanation, markdown, or extra commentary.";

/// User prompt instructing the vision model to analyze the image and return tags.
pub const USER_PROMPT_SUGGEST_THEMES: &str =
    "Identify 3 to 5 focused TTRPG battlemap theme and environment tags for this image. Return strictly a JSON array of strings.";

/// Default architectural prompt details used when user input is empty or whitespace.
pub const DEFAULT_BATTLEMAP_PROMPT_DETAILS: &str =
    "A detailed battle map with rooms, corridors, lighting, and environmental hazards";

/// Formats a user prompt with strict 2D architectural plan directives matching `buildBattlemapPrompt`.
pub fn build_battlemap_prompt(user_prompt: &str) -> String {
    let trimmed = user_prompt.trim();
    let details = if trimmed.is_empty() {
        DEFAULT_BATTLEMAP_PROMPT_DETAILS
    } else {
        trimmed
    };
    format!(
        "Details: {details}. Perspective: Strict 2D architectural floor plan cut; render flat poché wall cross-sections only, with zero visible vertical wall sides or 3D perspective, scale: 64px = 5ft square, strictly gridless with no visible grid lines."
    )
}

/// Serde serialization helper for binary data encoded as base64 string.
pub mod base64_bytes {
    use base64::prelude::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = BASE64_STANDARD.encode(bytes);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Base64OrBytesVisitor;

        impl<'de> serde::de::Visitor<'de> for Base64OrBytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a base64 string or a sequence of bytes")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let cleaned: String = v.chars().filter(|c| !c.is_whitespace()).collect();
                BASE64_STANDARD
                    .decode(&cleaned)
                    .map_err(serde::de::Error::custom)
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.to_vec())
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(Base64OrBytesVisitor)
    }
}

/// Reference image conditioning supplied alongside a generation or benchmark prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceImage {
    pub label: String,
    pub mime_type: String,
    #[serde(with = "crate::domain::prompts::base64_bytes")]
    pub data: Vec<u8>,
}

impl ReferenceImage {
    /// Creates a new reference image with the given label, MIME type, and raw data.
    pub fn new(
        label: impl Into<String>,
        mime_type: impl Into<String>,
        data: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            label: label.into(),
            mime_type: mime_type.into(),
            data: data.into(),
        }
    }

    /// Creates a new JPEG reference image.
    pub fn jpeg(label: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::new(label, "image/jpeg", data)
    }

    /// Creates a new PNG reference image.
    pub fn png(label: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::new(label, "image/png", data)
    }

    /// Creates a new WebP reference image.
    pub fn webp(label: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::new(label, "image/webp", data)
    }

    /// Returns the data URL representation (`data:<mime_type>;base64,<data>`).
    pub fn to_data_url(&self) -> String {
        use base64::prelude::*;
        let encoded = BASE64_STANDARD.encode(&self.data);
        format!("data:{};base64,{}", self.mime_type, encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_instruction_generate_directives() {
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("You are a flat 2D TTRPG battlemap renderer."));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("1. Camera & Perspective: Strictly flat 2D architectural plan view"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("2. Walls & Boundaries: Poché cut walls only."));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("3. Strict Negative Constraints: Absolutely NO 3D perspective"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("Strictly GRIDLESS: Absolutely NO visible grid lines"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("4. Spatial Scale & Proportions: The 1024x1024 image represents an 80 ft by 80 ft"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("Scale is strictly 64 pixels = 5 feet"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("5. Style & Reference Conditioning: Painted digital battlemap art"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("6. Lighting & Environment:"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("7. No empty margins"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("8. Output a single square image"));
        assert!(SYSTEM_INSTRUCTION_GENERATE.contains("9. Return only the image — no commentary."));
    }

    #[test]
    fn test_build_battlemap_prompt_with_custom_prompt() {
        let prompt = build_battlemap_prompt("Ancient dragon lair with lava pools and hoarded gold");
        assert_eq!(
            prompt,
            "Details: Ancient dragon lair with lava pools and hoarded gold. Perspective: Strict 2D architectural floor plan cut; render flat poché wall cross-sections only, with zero visible vertical wall sides or 3D perspective, scale: 64px = 5ft square, strictly gridless with no visible grid lines."
        );
    }

    #[test]
    fn test_build_battlemap_prompt_with_empty_and_whitespace() {
        let expected = format!(
            "Details: {DEFAULT_BATTLEMAP_PROMPT_DETAILS}. Perspective: Strict 2D architectural floor plan cut; render flat poché wall cross-sections only, with zero visible vertical wall sides or 3D perspective, scale: 64px = 5ft square, strictly gridless with no visible grid lines."
        );

        assert_eq!(build_battlemap_prompt(""), expected);
        assert_eq!(build_battlemap_prompt("   \t\n  "), expected);
    }

    #[test]
    fn test_reference_image_constructors() {
        let raw = vec![0x89, 0x50, 0x4E, 0x47];
        let png = ReferenceImage::png("ref-png", raw.clone());
        assert_eq!(png.label, "ref-png");
        assert_eq!(png.mime_type, "image/png");
        assert_eq!(png.data, raw);

        let jpeg = ReferenceImage::jpeg("ref-jpg", raw.clone());
        assert_eq!(jpeg.label, "ref-jpg");
        assert_eq!(jpeg.mime_type, "image/jpeg");
        assert_eq!(jpeg.data, raw);

        let webp = ReferenceImage::webp("ref-webp", raw.clone());
        assert_eq!(webp.label, "ref-webp");
        assert_eq!(webp.mime_type, "image/webp");
        assert_eq!(webp.data, raw);

        let custom = ReferenceImage::new("custom", "image/bmp", raw.clone());
        assert_eq!(custom.label, "custom");
        assert_eq!(custom.mime_type, "image/bmp");
        assert_eq!(custom.data, raw);
    }

    #[test]
    fn test_reference_image_to_data_url() {
        let img = ReferenceImage::png("test", b"hello world");
        let data_url = img.to_data_url();
        assert_eq!(data_url, "data:image/png;base64,aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_reference_image_serialization_roundtrip() {
        let img = ReferenceImage::png("layout-ref", b"\x00\x01\x02\x03\xFF");
        let serialized = serde_json::to_string(&img).expect("serialize");

        // Verify data is serialized as base64 string
        assert!(serialized.contains("\"data\":\"AAECA/8=\""));

        let deserialized: ReferenceImage = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, img);
    }

    #[test]
    fn test_reference_image_deserialize_from_bytes_array() {
        let json_seq = r#"{"label":"seq","mime_type":"image/png","data":[0,1,2,3,255]}"#;
        let img: ReferenceImage = serde_json::from_str(json_seq).expect("deserialize seq");
        assert_eq!(img.label, "seq");
        assert_eq!(img.data, vec![0, 1, 2, 3, 255]);
    }
}
