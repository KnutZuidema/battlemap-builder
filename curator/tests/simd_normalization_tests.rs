//! Integration tests for SIMD 1024x1024 Normalization and Kohya LoRA Captioning (Issue #29).

use std::io::Cursor;
use std::sync::Arc;

use async_trait::async_trait;
use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository, SimdImageProcessor};
use curator::domain::{
    Candidate, CandidateLayer, CaptionGenerator, LayerRole, Source, Theme,
    TRIGGER_PHRASE,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::{ImageProcessor, ResizeStrategy, LORA_TARGET_SIZE, PADDING_COLOR_RGBA};
use curator::services::curation::{CurationService, CurationWorkflow};
use curator::services::description::{DescriptionError, DescriptionGenerator, DescriptionService};
use image::{ImageFormat, Rgba, RgbaImage};

/// Deterministic description generator so curation never calls a vision model.
struct FixedDescriptionGen;

#[async_trait]
impl DescriptionGenerator for FixedDescriptionGen {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _filename: Option<&str>,
        _tags: &[String],
        _title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        Ok("A stone crypt with sarcophagus and torches.".to_string())
    }
}

fn in_memory_ports() -> (Arc<dyn CatalogRepository>, Arc<dyn BlobStorage>) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

fn curation_service(
    catalog: Arc<dyn CatalogRepository>,
    blobs: Arc<dyn BlobStorage>,
) -> CurationService {
    CurationService::new(
        catalog,
        blobs,
        Arc::new(DescriptionService::new(Some(Arc::new(FixedDescriptionGen)))),
    )
}

/// Helper to create a solid-color test PNG with specified dimensions.
fn create_colored_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(width, height, Rgba(color));
    let mut bytes = Vec::new();
    let mut cursor = Cursor::new(&mut bytes);
    img.write_to(&mut cursor, ImageFormat::Png).unwrap();
    bytes
}

#[test]
fn test_simd_resizing_produces_exact_1024_dimensions() {
    let processor = SimdImageProcessor::new();

    let test_sizes = [(800, 600), (300, 900), (1200, 1200), (64, 64), (1920, 1080)];
    for (w, h) in test_sizes {
        let input_bytes = create_colored_png(w, h, [100, 150, 200, 255]);

        let normalized_bytes = processor
            .normalize_lora(&input_bytes, ResizeStrategy::FitWithPadding)
            .expect("normalization succeeds");

        let decoded = image::load_from_memory(&normalized_bytes).expect("valid png");
        assert_eq!(
            decoded.width(),
            LORA_TARGET_SIZE,
            "width must be {LORA_TARGET_SIZE} for input {w}x{h}"
        );
        assert_eq!(
            decoded.height(),
            LORA_TARGET_SIZE,
            "height must be {LORA_TARGET_SIZE} for input {w}x{h}"
        );
    }
}

#[test]
fn test_fit_with_padding_letterbox_color() {
    let processor = SimdImageProcessor::new();

    // 800x400 (aspect 2:1) -> fits to 1024x512 with top/bottom padding of 256px
    let red_input = create_colored_png(800, 400, [255, 0, 0, 255]);
    let normalized = processor
        .normalize_lora(&red_input, ResizeStrategy::FitWithPadding)
        .expect("normalize");

    let decoded = image::load_from_memory(&normalized).expect("decoded").to_rgba8();
    assert_eq!(decoded.dimensions(), (1024, 1024));

    // Top padding area (0, 0) and (512, 100) must be #0b0f17 (RGB [11, 15, 23])
    let top_left = decoded.get_pixel(0, 0);
    assert_eq!(
        top_left.0, PADDING_COLOR_RGBA,
        "top-left padding pixel must match #0b0f17 [11, 15, 23, 255]"
    );

    let top_center = decoded.get_pixel(512, 100);
    assert_eq!(
        top_center.0, PADDING_COLOR_RGBA,
        "top-center padding pixel must match #0b0f17"
    );

    // Bottom padding area (512, 950) must be #0b0f17
    let bottom_center = decoded.get_pixel(512, 950);
    assert_eq!(
        bottom_center.0, PADDING_COLOR_RGBA,
        "bottom-center padding pixel must match #0b0f17"
    );

    // Center area (512, 512) must be the red image content
    let center = decoded.get_pixel(512, 512);
    assert_eq!(center.0[0], 255, "red channel in image content");
    assert_eq!(center.0[1], 0);
    assert_eq!(center.0[2], 0);
}

#[test]
fn test_fit_with_padding_pillarbox_color() {
    let processor = SimdImageProcessor::new();

    // 400x800 (aspect 1:2) -> fits to 512x1024 with left/right padding of 256px
    let blue_input = create_colored_png(400, 800, [0, 0, 255, 255]);
    let normalized = processor
        .normalize_lora(&blue_input, ResizeStrategy::FitWithPadding)
        .expect("normalize");

    let decoded = image::load_from_memory(&normalized).expect("decoded").to_rgba8();
    assert_eq!(decoded.dimensions(), (1024, 1024));

    // Left padding area (50, 512) must be #0b0f17
    let left_pad = decoded.get_pixel(50, 512);
    assert_eq!(
        left_pad.0, PADDING_COLOR_RGBA,
        "left pillarbox pixel must match #0b0f17"
    );

    // Right padding area (950, 512) must be #0b0f17
    let right_pad = decoded.get_pixel(950, 512);
    assert_eq!(
        right_pad.0, PADDING_COLOR_RGBA,
        "right pillarbox pixel must match #0b0f17"
    );

    // Center area (512, 512) must be blue image content
    let center = decoded.get_pixel(512, 512);
    assert_eq!(center.0[0], 0);
    assert_eq!(center.0[1], 0);
    assert_eq!(center.0[2], 255, "blue channel in image content");
}

#[test]
fn test_center_crop_strategy() {
    let processor = SimdImageProcessor::new();

    // 1200x600 green image: center-crop crops centered 600x600 square and scales to 1024x1024
    let green_input = create_colored_png(1200, 600, [0, 200, 0, 255]);
    let normalized = processor
        .normalize_lora(&green_input, ResizeStrategy::CenterCrop)
        .expect("normalize center crop");

    let decoded = image::load_from_memory(&normalized).expect("decoded").to_rgba8();
    assert_eq!(decoded.dimensions(), (1024, 1024));

    // In center crop, the corners are part of the image, NOT padding
    let corner = decoded.get_pixel(0, 0);
    assert_eq!(corner.0[1], 200, "green content at corner in center crop");
    assert_ne!(
        corner.0, PADDING_COLOR_RGBA,
        "corner should not be padding color in center crop"
    );

    let center = decoded.get_pixel(512, 512);
    assert_eq!(center.0[1], 200, "green content at center");
}

#[test]
fn test_caption_generator_format_adherence() {
    // 1. Base format with theme only
    let caption_dungeon = CaptionGenerator::generate(&Theme::Dungeon, &[], None);
    assert_eq!(
        caption_dungeon,
        format!("{TRIGGER_PHRASE}, dungeon")
    );

    // 2. Tavern with tags and prompt architectural features
    let caption_tavern = CaptionGenerator::generate(
        &Theme::Tavern,
        &["rustic".into(), "fireplace".into()],
        Some("Fantasy tavern common room with wooden tables, bar counter and timber walls."),
    );
    assert!(caption_tavern.starts_with(TRIGGER_PHRASE));
    assert!(caption_tavern.contains("tavern"));
    assert!(caption_tavern.contains("fireplace"));
    assert!(caption_tavern.contains("rustic"));
    assert!(caption_tavern.contains("bar counter"));
    assert!(caption_tavern.contains("timber walls"));

    // 3. Sanitizes brackets, dimensions, redundant grid mentions
    let caption_scifi = CaptionGenerator::generate(
        &Theme::SciFi,
        &["spire".into(), "gridless".into()],
        Some("[50x50] Crystal Spire [No Grid]!"),
    );
    assert!(caption_scifi.contains("scifi"));
    assert!(caption_scifi.contains("crystal spire"));
    assert!(!caption_scifi.contains("50x50"));
    assert!(!caption_scifi.contains("no grid"));
    assert!(!caption_scifi.contains("gridless, gridless"));
}

#[tokio::test]
async fn test_full_curation_workflow_bundle_lora_files() {
    let (catalog, blobs) = in_memory_ports();

    // 1. Stage a multi-layer bundle with a real image for layer 0.
    // 1200x1200 spans 18x18 full Grid Cells at the default 64px cell size, so the ground
    // layer is curation-eligible and its first exact 16x16 Grid Cell section resolves.
    let test_png = create_colored_png(1200, 1200, [180, 100, 50, 255]);
    let candidate = Candidate::with_now("crypt_bundle_1", Source::Reddit, "0.png", (1200, 1200))
        .with_title("Ancient Stone Crypt [30x30] with Sarcophagus and Torches")
        .with_layers(vec![CandidateLayer::new(
            0,
            "0.png",
            LayerRole::Ground,
            Some(0),
            (1200, 1200),
        )]);
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "crypt_bundle_1".to_string(),
                index: 0,
            },
            &test_png,
        )
        .await
        .unwrap();
    catalog.save_candidate(&candidate).await.unwrap();

    let curated = curation_service(catalog.clone(), blobs.clone())
        .curate(
            "crypt_bundle_1",
            vec!["crypt".into(), "stone".into()],
            None,
            None,
        )
        .await
        .expect("curate bundle");

    // 2. Staging is superseded by the curated bundle.
    assert!(catalog.get_candidate("crypt_bundle_1").await.is_err());
    assert!(catalog.get_curated("crypt_bundle_1").await.is_ok());
    assert_eq!(curated.layers.len(), 1);
    assert_eq!(curated.layers[0].filename, "0_ground.png");

    // 3. No LoRA companion blob is written: ADR-0007 derives samples on demand.
    let curated_keys = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("crypt_bundle_1".to_string()),
        })
        .await
        .unwrap();
    assert!(curated_keys.iter().all(|key| !key.to_path().contains("lora1024")));
    assert!(curated_keys.iter().any(|key| key.to_path().ends_with("0_ground.png")));

    let stored = catalog.get_curated("crypt_bundle_1").await.unwrap();
    assert_eq!(stored.layers[0].role, LayerRole::Ground);
    assert!(stored
        .description
        .as_deref()
        .is_some_and(|description| description.contains("sarcophagus")));
}

#[tokio::test]
async fn test_full_curation_workflow_flat_candidate_lora_files() {
    let (catalog, blobs) = in_memory_ports();

    // 1. Stage a flat (layer-less) candidate.
    // 1200x1200 spans 18x18 full Grid Cells at the default 64px cell size, so the curated
    // map is eligible and its first exact 16x16 Grid Cell section resolves.
    let test_png = create_colored_png(1200, 1200, [50, 120, 200, 255]);
    let candidate =
        Candidate::with_now("tavern_map_42", Source::Reddit, "tavern_map_42.png", (1200, 1200))
            .with_title("Cozy Tavern with Hearth Fireplace and Wooden Tables [No Grid]");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "tavern_map_42".to_string(),
                index: 0,
            },
            &test_png,
        )
        .await
        .unwrap();
    catalog.save_candidate(&candidate).await.unwrap();

    let curated = curation_service(catalog.clone(), blobs.clone())
        .curate("tavern_map_42", vec!["cozy".into()], None, None)
        .await
        .expect("curate flat candidate");

    // 2. No LoRA companions exist; the canonical Ground layer carries the sample source.
    let curated_keys = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("tavern_map_42".to_string()),
        })
        .await
        .unwrap();
    assert!(curated_keys.iter().all(|key| !key.to_path().contains("lora1024")));
    assert!(curated_keys
        .iter()
        .any(|key| key.to_path().ends_with("0_ground.png")));
    assert_eq!(curated.layers[0].filename, "0_ground.png");
    assert_eq!(curated.layers[0].role, LayerRole::Ground);
}
