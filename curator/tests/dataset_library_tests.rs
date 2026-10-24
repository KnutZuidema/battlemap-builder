use std::io::Cursor;
use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use tower::ServiceExt;
use zip::ZipArchive;

use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::api::{create_router, AppState};
use curator::domain::{Candidate, CuratedMap, CuratedLayer, GridConfig, LayerRole, MapSection, Source, Theme};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::services::curation::{CurationService, CurationWorkflow};
use curator::services::description::DescriptionService;
use curator::services::DatasetExportService;
use image::GenericImageView;

/// Fresh in-memory catalog and blob ports: these tests exercise the dataset routes.
fn in_memory_ports() -> (
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

/// Router plus the in-memory catalog/blob ports it serves every route from.
fn setup_test_app() -> (
    axum::Router,
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
) {
    let (catalog, blobs) = in_memory_ports();
    let state = AppState::new(catalog.clone(), blobs.clone());
    let router = create_router(state);
    (router, catalog, blobs)
}

/// Blob key of a curated map's `Ground` layer, resolved exactly like the dataset export service.
fn curated_ground_key(map: &CuratedMap) -> BlobKey {
    match map
        .layers
        .iter()
        .find(|layer| layer.role == LayerRole::Ground)
        .or_else(|| map.layers.first())
    {
        Some(layer) => BlobKey::CuratedLayer {
            id: map.id.clone(),
            index: layer.index,
            role: layer.role,
            elevation: layer.elevation,
        },
        None => BlobKey::CuratedLayer {
            id: map.id.clone(),
            index: 0,
            role: LayerRole::Ground,
            elevation: Some(0),
        },
    }
}

/// Seeds a curated map into the ports the dataset routes read: the catalog entry with the
/// implicit Ground layer, plus the Ground layer blob under the key both the export and the
/// readiness audit resolve.
async fn seed_curated_ports(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    map: &CuratedMap,
    ground_png: &[u8],
) {
    let mut stored = map.clone();
    if stored.layers.is_empty() {
        stored.layers = vec![CuratedLayer::new(
            0,
            CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), "png"),
            LayerRole::Ground,
            Some(0),
            stored.dimensions,
        )];
    }
    catalog
        .save_curated(&stored)
        .await
        .expect("save curated map in the catalog");
    blobs
        .write_blob(&curated_ground_key(&stored), ground_png)
        .await
        .expect("write curated Ground layer blob");
}

fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(width, height);
    let mut bytes = Vec::new();
    let mut cursor = Cursor::new(&mut bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
    bytes
}

#[tokio::test]
async fn test_api_curated_listing_with_and_without_theme_filtering() {
    let (app, catalog, blobs) = setup_test_app();
    let png_bytes = create_test_png(64, 64);

    // Save curated maps in different themes
    let map_dungeon1 = CuratedMap::with_now(
        "dungeon_crypt",
        Theme::Dungeon,
        vec!["crypt".into(), "stone-walls".into()],
        (1024, 1024),
    );
    let map_dungeon2 = CuratedMap::with_now(
        "dungeon_prison",
        Theme::Dungeon,
        vec!["cells".into(), "iron-bars".into()],
        (1024, 1024),
    );
    let map_cave = CuratedMap::with_now(
        "cave_chasm",
        Theme::Cave,
        vec!["stalagmites".into(), "chasm".into()],
        (1024, 1024),
    );
    let map_wilderness = CuratedMap::with_now(
        "forest_clearing",
        Theme::Wilderness,
        vec!["trees".into(), "river".into()],
        (1024, 1024),
    );

    seed_curated_ports(&catalog, &blobs, &map_dungeon1, &png_bytes).await;
    seed_curated_ports(&catalog, &blobs, &map_dungeon2, &png_bytes).await;
    seed_curated_ports(&catalog, &blobs, &map_cave, &png_bytes).await;
    seed_curated_ports(&catalog, &blobs, &map_wilderness, &png_bytes).await;

    // 1. GET /api/curated without theme query returns all 4 maps
    let req_all = Request::builder()
        .uri("/api/curated")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_all = app.clone().oneshot(req_all).await.unwrap();
    assert_eq!(resp_all.status(), StatusCode::OK);

    let body_all = to_bytes(resp_all.into_body(), usize::MAX).await.unwrap();
    let list_all: Vec<CuratedMap> = serde_json::from_slice(&body_all).unwrap();
    assert_eq!(list_all.len(), 4);

    // Verify fields on returned maps (pure reads preserve metadata without synthetic caption mutation)
    for map in &list_all {
        assert!(!map.id.is_empty());
        assert!(!map.tags.is_empty());
        assert!(!map.layers.is_empty());
    }

    // 2. GET /api/curated?theme=dungeon returns only the 2 dungeon maps
    let req_dungeon = Request::builder()
        .uri("/api/curated?theme=dungeon")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_dungeon = app.clone().oneshot(req_dungeon).await.unwrap();
    assert_eq!(resp_dungeon.status(), StatusCode::OK);

    let body_dungeon = to_bytes(resp_dungeon.into_body(), usize::MAX).await.unwrap();
    let list_dungeon: Vec<CuratedMap> = serde_json::from_slice(&body_dungeon).unwrap();
    assert_eq!(list_dungeon.len(), 2);
    assert!(list_dungeon.iter().all(|m| m.theme() == Some(Theme::Dungeon)));

    // 3. GET /api/curated?theme=cave returns 1 map
    let req_cave = Request::builder()
        .uri("/api/curated?theme=cave")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_cave = app.clone().oneshot(req_cave).await.unwrap();
    assert_eq!(resp_cave.status(), StatusCode::OK);

    let body_cave = to_bytes(resp_cave.into_body(), usize::MAX).await.unwrap();
    let list_cave: Vec<CuratedMap> = serde_json::from_slice(&body_cave).unwrap();
    assert_eq!(list_cave.len(), 1);
    assert_eq!(list_cave[0].id, "cave_chasm");

    // 4. GET /api/curated?theme=all returns all 4 maps
    let req_theme_all = Request::builder()
        .uri("/api/curated?theme=all")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_theme_all = app.clone().oneshot(req_theme_all).await.unwrap();
    assert_eq!(resp_theme_all.status(), StatusCode::OK);
    let body_theme_all = to_bytes(resp_theme_all.into_body(), usize::MAX).await.unwrap();
    let list_theme_all: Vec<CuratedMap> = serde_json::from_slice(&body_theme_all).unwrap();
    assert_eq!(list_theme_all.len(), 4);

    // 5. GET /api/curated?theme=invalid_theme returns 400 Bad Request
    let req_invalid = Request::builder()
        .uri("/api/curated?theme=nonexistent_theme")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_invalid = app.oneshot(req_invalid).await.unwrap();
    assert_eq!(resp_invalid.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_storage_export_lora_dataset_paired_png_and_txt() {
    let (catalog, blobs) = in_memory_ports();
    // 1024x1024 matches the maps' declared dimensions and default 64px grid so the exact
    // 16x16 Grid Cell section box resolves.
    let png_bytes = create_test_png(1024, 1024);

    let map1 = CuratedMap::with_now(
        "dungeon_temple",
        Theme::Dungeon,
        vec!["altar".into(), "pillars".into()],
        (1024, 1024),
    );
    let map2 = CuratedMap::with_now(
        "cave_grotto",
        Theme::Cave,
        vec!["water-stream".into()],
        (1024, 1024),
    );

    seed_curated_ports(&catalog, &blobs, &map1, &png_bytes).await;
    seed_curated_ports(&catalog, &blobs, &map2, &png_bytes).await;

    // Export all themes
    let export = DatasetExportService::new(catalog, blobs);
    let zip_bytes = export.export_lora_dataset(None).await.unwrap();
    assert!(!zip_bytes.is_empty());

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    assert_eq!(archive.len(), 4); // 2 pngs + 2 txts

    let mut filenames: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    filenames.sort();

    assert!(filenames.contains(&"dungeon/dungeon_temple_c0_r0_lora1024.png".to_string()));
    assert!(filenames.contains(&"dungeon/dungeon_temple_c0_r0_lora1024.txt".to_string()));
    assert!(filenames.contains(&"cave/cave_grotto_c0_r0_lora1024.png".to_string()));
    assert!(filenames.contains(&"cave/cave_grotto_c0_r0_lora1024.txt".to_string()));

    // Verify caption content inside ZIP
    let mut txt_file = archive
        .by_name("dungeon/dungeon_temple_c0_r0_lora1024.txt")
        .unwrap();
    let mut caption_content = String::new();
    std::io::Read::read_to_string(&mut txt_file, &mut caption_content).unwrap();
    assert!(caption_content.contains("dungeonforge battlemap"));
    assert!(caption_content.contains("dungeon"));

    // Verify filtered export for Theme::Dungeon only
    let dungeon_zip = export.export_lora_dataset(Some(&Theme::Dungeon)).await.unwrap();
    let mut dungeon_archive = ZipArchive::new(Cursor::new(&dungeon_zip)).unwrap();
    assert_eq!(dungeon_archive.len(), 2);

    let d_names: Vec<String> = (0..dungeon_archive.len())
        .map(|i| dungeon_archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(d_names.contains(&"dungeon/dungeon_temple_c0_r0_lora1024.png".to_string()));
    assert!(d_names.contains(&"dungeon/dungeon_temple_c0_r0_lora1024.txt".to_string()));
    assert!(!d_names.iter().any(|n| n.contains("cave")));
}

#[tokio::test]
async fn test_api_dataset_export_headers_and_stream() {
    let (app, catalog, blobs) = setup_test_app();
    // 1024x1024 matches the map's declared dimensions and default 64px grid so the exact
    // 16x16 Grid Cell section box resolves.
    let png_bytes = create_test_png(1024, 1024);

    let map = CuratedMap::with_now(
        "catacomb_map",
        Theme::Dungeon,
        vec!["sarcophagus".into()],
        (1024, 1024),
    );
    seed_curated_ports(&catalog, &blobs, &map, &png_bytes).await;

    let req = Request::builder()
        .uri("/api/dataset/export")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify required HTTP response headers
    let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap();
    assert_eq!(content_type, "application/zip");

    let content_disp = resp.headers().get(header::CONTENT_DISPOSITION).unwrap();
    assert_eq!(
        content_disp,
        "attachment; filename=\"lora-dataset.zip\""
    );

    // Verify body is a valid ZIP stream containing the expected files
    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&body_bytes)).unwrap();
    assert_eq!(archive.len(), 2);

    let file0 = archive.by_index(0).unwrap();
    assert!(file0.name().starts_with("dungeon/catacomb_map_c0_r0_lora1024."));
}

#[tokio::test]
async fn test_empty_theme_export_handling() {
    let (app, catalog, blobs) = setup_test_app();

    // 1. Direct service call on empty storage
    let empty_zip = DatasetExportService::new(catalog, blobs)
        .export_lora_dataset(Some(&Theme::SciFi))
        .await
        .unwrap();
    assert!(!empty_zip.is_empty(), "Empty ZIP must contain valid EOCD bytes");

    let archive = ZipArchive::new(Cursor::new(&empty_zip)).unwrap();
    assert_eq!(archive.len(), 0);

    // 2. HTTP endpoint call for empty theme
    let req = Request::builder()
        .uri("/api/dataset/export?theme=scifi")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );
    assert_eq!(
        resp.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"lora-dataset.zip\""
    );

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let http_archive = ZipArchive::new(Cursor::new(&body_bytes)).unwrap();
    assert_eq!(http_archive.len(), 0);
}

#[tokio::test]
async fn test_curation_does_not_write_lora_files_to_disk() {
    let (catalog, blobs) = in_memory_ports();
    let png_bytes = create_test_png(1024, 1024);

    let cand = Candidate::with_now(
        "clean_bundle_map",
        Source::Reddit,
        "clean_bundle_map.png",
        (1024, 1024),
    )
    .with_title("Clean Crypt Without Disk Artifacts");
    catalog.save_candidate(&cand).await.unwrap();
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "clean_bundle_map".to_string(),
                index: 0,
            },
            &png_bytes,
        )
        .await
        .unwrap();

    let service = CurationService::new(
        catalog.clone(),
        blobs.clone(),
        Arc::new(DescriptionService::new(None)),
    );
    service
        .curate(
            "clean_bundle_map",
            vec!["crypt".into(), "clean".into()],
            None,
            Some("A clean crypt bundle".to_string()),
        )
        .await
        .expect("curate staging candidate");

    assert!(catalog.get_curated("clean_bundle_map").await.is_ok());
    assert!(catalog.get_candidate("clean_bundle_map").await.is_err());

    // The curated bundle holds exactly its Ground layer blob: no LoRA companion artifacts exist.
    let keys = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("clean_bundle_map".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].to_path().ends_with("0_ground.png"));
    assert!(!keys.iter().any(|key| key.to_path().contains("lora1024")));
}

#[tokio::test]
async fn test_multisection_map_export_produces_four_distinct_pairs_with_per_section_descriptions() {
    let (app, catalog, blobs) = setup_test_app();
    let png_bytes = create_test_png(2048, 2048);

    let sections = vec![
        MapSection::new(0, 0, "northwest crypt chamber with obsidian altar"),
        MapSection::new(16, 0, "northeast guard room with weapon racks"),
        MapSection::new(0, 16, "southwest flooded dungeon cell with iron bars"),
        MapSection::new(16, 16, "southeast torture dungeon with iron maiden"),
    ];

    let multi_map = CuratedMap::with_now(
        "multi_sec_map",
        Theme::Dungeon,
        vec!["dungeon".into(), "stone".into()],
        (2048, 2048),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_sections(sections);

    seed_curated_ports(&catalog, &blobs, &multi_map, &png_bytes).await;

    // GET /api/dataset/export?theme=dungeon
    let req = Request::builder()
        .uri("/api/dataset/export?theme=dungeon")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&body_bytes)).unwrap();
    assert_eq!(archive.len(), 8); // 4 pngs + 4 txts

    let mut names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    names.sort();

    let expected_names = [
        "dungeon/multi_sec_map_c0_r0_lora1024.png",
        "dungeon/multi_sec_map_c0_r0_lora1024.txt",
        "dungeon/multi_sec_map_c0_r16_lora1024.png",
        "dungeon/multi_sec_map_c0_r16_lora1024.txt",
        "dungeon/multi_sec_map_c16_r0_lora1024.png",
        "dungeon/multi_sec_map_c16_r0_lora1024.txt",
        "dungeon/multi_sec_map_c16_r16_lora1024.png",
        "dungeon/multi_sec_map_c16_r16_lora1024.txt",
    ];

    for expected in &expected_names {
        assert!(names.contains(&expected.to_string()), "Missing entry: {expected}");
    }

    // Verify each PNG is exact 1024x1024 without letterbox bars
    for (col, row) in [(0, 0), (16, 0), (0, 16), (16, 16)] {
        let png_name = format!("dungeon/multi_sec_map_c{col}_r{row}_lora1024.png");
        let mut png_entry = archive.by_name(&png_name).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut png_entry, &mut bytes).unwrap();
        let decoded = image::load_from_memory(&bytes).expect("valid png bytes");
        assert_eq!(decoded.dimensions(), (1024, 1024));
    }

    // Verify each TXT caption contains its respective section's physical description
    let checks = [
        ("dungeon/multi_sec_map_c0_r0_lora1024.txt", "obsidian altar"),
        ("dungeon/multi_sec_map_c16_r0_lora1024.txt", "weapon racks"),
        ("dungeon/multi_sec_map_c0_r16_lora1024.txt", "iron bars"),
        ("dungeon/multi_sec_map_c16_r16_lora1024.txt", "iron maiden"),
    ];
    for (txt_name, keyword) in checks {
        let mut txt_entry = archive.by_name(txt_name).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut txt_entry, &mut text).unwrap();
        assert!(text.contains("dungeonforge battlemap"));
        assert!(text.contains("dungeon"));
        assert!(text.contains(keyword), "Caption {txt_name} should contain '{keyword}', got: {text}");
    }
}

#[tokio::test]
async fn test_post_export_selected_multisection_and_custom_grid() {
    let (app, catalog, blobs) = setup_test_app();
    let png_bytes = create_test_png(2048, 2048);

    let multi_map = CuratedMap::with_now(
        "selected_multi",
        Theme::Cave,
        vec!["crystal".into()],
        (2048, 2048),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("Sprawling underground cavern network");

    seed_curated_ports(&catalog, &blobs, &multi_map, &png_bytes).await;

    let req = Request::builder()
        .uri("/api/dataset/export-selected")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({
            "ids": ["selected_multi"]
        }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&body_bytes)).unwrap();
    assert_eq!(archive.len(), 8); // 4 png + 4 txt

    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();

    assert!(names.contains(&"cave/selected_multi_c0_r0_lora1024.png".to_string()));
    assert!(names.contains(&"cave/selected_multi_c0_r0_lora1024.txt".to_string()));
    assert!(names.contains(&"cave/selected_multi_c16_r16_lora1024.png".to_string()));
    assert!(names.contains(&"cave/selected_multi_c16_r16_lora1024.txt".to_string()));
}
