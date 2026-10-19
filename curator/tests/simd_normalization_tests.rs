//! Integration tests for SIMD 1024x1024 Normalization and Kohya LoRA Captioning (Issue #29).

use std::io::Cursor;
use tempfile::tempdir;

use curator::adapters::{FilesystemStorage, SimdImageProcessor};
use curator::domain::{
    Candidate, CandidateLayer, CaptionGenerator, CuratedMap, LayerRole, Source, Theme,
    LORA_CAPTION_PREFIX,
};
use curator::ports::{
    ImageProcessor, ResizeStrategy, StorageRepository, LORA_TARGET_SIZE, PADDING_COLOR_RGBA,
};
use image::{GenericImageView, ImageFormat, Rgba, RgbaImage};

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
        format!("{LORA_CAPTION_PREFIX}, dungeon")
    );

    // 2. Tavern with tags and prompt architectural features
    let caption_tavern = CaptionGenerator::generate(
        &Theme::Tavern,
        &["rustic".into(), "fireplace".into()],
        Some("Fantasy tavern common room with wooden tables, bar counter and timber walls."),
    );
    assert!(caption_tavern.starts_with(LORA_CAPTION_PREFIX));
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

#[test]
fn test_full_curation_workflow_bundle_lora_files() {
    let dir = tempdir().unwrap();
    let storage = FilesystemStorage::new(dir.path());

    // 1. Create a staged multi-layer bundle with a real image for layer 0
    // 1200x1200 spans 18x18 full Grid Cells at the default 64px cell size, so the ground
    // layer is curation-eligible and its first exact 16x16 Grid Cell section resolves.
    let test_png = create_colored_png(1200, 1200, [180, 100, 50, 255]);
    let candidate = Candidate::with_now(
        "crypt_bundle_1",
        Source::Reddit,
        "0.png",
        (1200, 1200),
    )
    .with_title("Ancient Stone Crypt [30x30] with Sarcophagus and Torches")
    .with_layers(vec![CandidateLayer::new(
        0,
        "0.png",
        LayerRole::Ground,
        Some(0),
        (1200, 1200),
    )]);

    storage
        .save_staging_bundle(None, &candidate, &[test_png])
        .unwrap();

    assert!(storage.is_in_staging("crypt_bundle_1").unwrap());
    assert!(!storage.is_lora_normalized(&Theme::Dungeon, "crypt_bundle_1").unwrap());

    // 2. Curate candidate
    let curated = CuratedMap::with_now(
        "crypt_bundle_1",
        Theme::Dungeon,
        vec!["crypt".into(), "stone".into()],
        (1200, 1200),
    );

    storage.curate_staging(&curated).unwrap();

    // Staging must be cleaned up
    assert!(!storage.is_in_staging("crypt_bundle_1").unwrap());
    assert!(storage.is_curated("crypt_bundle_1").unwrap());

    // 3. Verify LoRA companion files are NOT on disk
    let bundle_dir = storage.curated_dir().join("crypt_bundle_1");
    let bundle_lora_png = bundle_dir.join("crypt_bundle_1_lora1024.png");
    let bundle_lora_txt = bundle_dir.join("crypt_bundle_1_lora1024.txt");

    assert!(!bundle_lora_png.exists(), "bundle_lora_png must NOT exist in bundle dir");
    assert!(!bundle_lora_txt.exists(), "bundle_lora_txt must NOT exist in bundle dir");
    assert!(bundle_dir.join("manifest.json").is_file());
    assert!(bundle_dir.join("0_ground.png").is_file());

    // 4. Verify storage methods
    assert!(storage.is_lora_normalized(&Theme::Dungeon, "crypt_bundle_1").unwrap());

    let lora_bytes = storage
        .get_curated_lora_image(&Theme::Dungeon, "crypt_bundle_1")
        .expect("retrieve lora image");
    let decoded = image::load_from_memory(&lora_bytes).expect("valid png bytes");
    assert_eq!(decoded.dimensions(), (1024, 1024));

    let caption = storage
        .get_curated_lora_caption(&Theme::Dungeon, "crypt_bundle_1")
        .expect("retrieve lora caption");
    assert!(caption.starts_with(LORA_CAPTION_PREFIX));
    assert!(caption.contains("dungeon"));
    assert!(caption.contains("sarcophagus"));
    assert!(caption.contains("torches"));
}

#[test]
fn test_full_curation_workflow_flat_candidate_lora_files() {
    let dir = tempdir().unwrap();
    let storage = FilesystemStorage::new(dir.path());

    // 1. Create a flat candidate in staging
    // 1200x1200 spans 18x18 full Grid Cells at the default 64px cell size, so the curated
    // map is eligible and its first exact 16x16 Grid Cell section resolves.
    let test_png = create_colored_png(1200, 1200, [50, 120, 200, 255]);
    let candidate = Candidate::with_now(
        "tavern_map_42",
        Source::Reddit,
        "tavern_map_42.png",
        (1200, 1200),
    )
    .with_title("Cozy Tavern with Hearth Fireplace and Wooden Tables [No Grid]");

    storage.save_staging(&candidate, &test_png).unwrap();

    // 2. Curate candidate
    let curated = CuratedMap::with_now(
        "tavern_map_42",
        Theme::Tavern,
        vec!["cozy".into()],
        (1200, 1200),
    );

    storage.curate_staging(&curated).unwrap();

    // 3. Verify companion files are NOT on disk and storage reports normalized
    let bundle_dir = storage.curated_dir().join("tavern_map_42");
    assert!(!bundle_dir.join("tavern_map_42_lora1024.png").exists());
    assert!(!bundle_dir.join("tavern_map_42_lora1024.txt").exists());
    assert!(bundle_dir.join("manifest.json").is_file());
    assert!(bundle_dir.join("0_ground.png").is_file());
    assert!(storage.is_lora_normalized(&Theme::Tavern, "tavern_map_42").unwrap());

    let lora_png = storage
        .get_curated_lora_image(&Theme::Tavern, "tavern_map_42")
        .unwrap();
    let decoded = image::load_from_memory(&lora_png).unwrap();
    assert_eq!(decoded.dimensions(), (1024, 1024));

    let lora_txt = storage
        .get_curated_lora_caption(&Theme::Tavern, "tavern_map_42")
        .unwrap();
    assert!(lora_txt.starts_with(LORA_CAPTION_PREFIX));
    assert!(lora_txt.contains("tavern"));
    assert!(lora_txt.contains("fireplace"));
    assert!(lora_txt.contains("wooden tables"));
}
