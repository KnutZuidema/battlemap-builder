//! Integration tests for the dynamic 16×16 Map Section `Training Dataset` contract:
//! the `Trigger Phrase` heading every exported caption, perspective-neutral `Physical
//! Descriptions`, and the dataset readiness audit behind `GET /api/dataset/readiness` (ADR-0007).

use std::io::Cursor;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;
use zip::ZipArchive;

use curator::adapters::{FilesystemStorage, InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::api::{create_router, AppState};
use curator::domain::{
    CuratedLayer, CuratedMap, DatasetIssueCode, GridConfig, LayerRole, MapSection, Theme,
    TRIGGER_PHRASE,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::storage::StorageRepository;

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

/// Router plus the filesystem repository and the in-memory catalog/blob ports it is paired with.
///
/// The readiness route was cut over to the ports, so a test targeting it seeds the ports; the
/// filesystem repository keeps serving the routes that still read `state.storage`.
fn setup_test_app() -> (
    axum::Router,
    Arc<FilesystemStorage>,
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
    tempfile::TempDir,
) {
    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone());
    let router = create_router(state);
    (router, storage, catalog, blobs, dir)
}

/// Blob key of a curated map's `Ground` layer, resolved exactly like the readiness audit.
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

/// Seeds a curated map into the ports the cut-over readiness route reads: the catalog entry with
/// the implicit Ground layer [`FilesystemStorage::save_curated`] synthesizes, plus the Ground
/// layer blob under the key the audit resolves.
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
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("write PNG");
    bytes
}

/// A 2048×2048 curated map at a 64px grid: four 16×16 Map Sections with neutral descriptions.
fn ready_multisection_map(id: &str) -> CuratedMap {
    CuratedMap::with_now(id, Theme::Dungeon, vec!["crypt".into()], (2048, 2048))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_description("Granite necropolis with carved pillars and iron braziers.")
        .with_sections(vec![
            MapSection::new(0, 0, "Northwest crypt with a shattered sarcophagus lid."),
            MapSection::new(16, 0, "Northeast guard room lined with weapon racks."),
            MapSection::new(0, 16, "Southwest flooded cell behind iron bars."),
            MapSection::new(16, 16, "Southeast torture chamber with a rusted iron maiden."),
        ])
}

fn caption_in_archive(archive: &mut ZipArchive<Cursor<&Vec<u8>>>, name: &str) -> String {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut archive.by_name(name).unwrap(), &mut text).unwrap();
    text
}

#[tokio::test]
async fn test_clean_multisection_dataset_is_ready_and_captions_apply_the_trigger_phrase_once() {
    let (_app, storage, ..) = setup_test_app();
    storage
        .save_curated(&ready_multisection_map("ready_multi"), &create_test_png(2048, 2048))
        .unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(report.ready, "{:?}", report.maps);
    assert_eq!(report.maps_total, 1);
    assert_eq!(report.maps_ready, 1);
    assert_eq!(report.sections_total, 4);
    assert_eq!(report.sections_ready, 4);
    assert_eq!(report.maps[0].theme, "dungeon");
    assert!(report.maps[0].issues.is_empty(), "{:?}", report.maps[0].issues);

    // The exported ZIP agrees with the audit: 4 pairs, every caption headed by the Trigger Phrase.
    let zip_bytes = storage.export_lora_dataset(None).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    assert_eq!(archive.len(), 8);

    for (col, row) in [(0u32, 0u32), (16, 0), (0, 16), (16, 16)] {
        let name = format!("dungeon/ready_multi_c{col}_r{row}_lora1024.txt");
        let caption = caption_in_archive(&mut archive, &name);
        assert!(caption.starts_with(TRIGGER_PHRASE), "{name}: {caption}");
        assert_eq!(
            caption.matches(TRIGGER_PHRASE).count(),
            1,
            "Trigger Phrase must be applied exactly once: {caption}"
        );
    }

    let southwest = caption_in_archive(
        &mut archive,
        "dungeon/ready_multi_c0_r16_lora1024.txt",
    );
    assert!(southwest.contains("flooded cell behind iron bars"), "{southwest}");
}

#[tokio::test]
async fn test_legacy_bundle_without_sections_reuses_the_neutral_map_description() {
    let (_app, storage, ..) = setup_test_app();
    // Pre-ADR-0007 manifests carry neither sections nor a grid: the full lattice contributes one
    // 16×16 window, described by the map-level Physical Description.
    let legacy = CuratedMap::with_now(
        "legacy_flat",
        Theme::Tavern,
        vec!["hearth".into()],
        (1024, 1024),
    )
    .with_description("Timber hall with a stone hearth and long trestle tables.");

    storage.save_curated(&legacy, &create_test_png(1024, 1024)).unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(report.ready, "{:?}", report.maps);
    assert_eq!(report.sections_total, 1);
    assert_eq!(report.sections_ready, 1);

    let zip_bytes = storage.export_lora_dataset(None).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    let caption = caption_in_archive(&mut archive, "tavern/legacy_flat_c0_r0_lora1024.txt");
    assert_eq!(caption.matches(TRIGGER_PHRASE).count(), 1, "{caption}");
    assert!(caption.contains("stone hearth"), "{caption}");
}

#[tokio::test]
async fn test_caption_contaminated_section_description_is_flagged_not_neutral() {
    let (_app, storage, ..) = setup_test_app();
    // Exactly what the retired per-section caption fallback persisted into a Map Section
    // description: the Trigger Phrase plus camera directives inside a Physical Description.
    let contaminated = format!("{TRIGGER_PHRASE}, dungeon, crypt, obsidian altar");

    let map = CuratedMap::with_now(
        "contaminated",
        Theme::Dungeon,
        vec!["crypt".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("Granite hall with carved pillars.")
    .with_sections(vec![MapSection::new(0, 0, contaminated.clone())]);

    storage.save_curated(&map, &create_test_png(1024, 1024)).unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(!report.ready);
    assert_eq!(report.maps_ready, 0);
    assert_eq!(report.sections_total, 1);
    assert_eq!(report.sections_ready, 0);

    let codes: Vec<DatasetIssueCode> = report.maps[0].issues.iter().map(|i| i.code).collect();
    assert!(
        codes.contains(&DatasetIssueCode::PhysicalDescriptionNotNeutral),
        "{:?}",
        report.maps[0].issues
    );
    let flagged = report.maps[0]
        .issues
        .iter()
        .find(|issue| issue.code == DatasetIssueCode::PhysicalDescriptionNotNeutral)
        .unwrap();
    assert!(flagged.detail.contains("Map Section (0, 0)"), "{}", flagged.detail);

    // Exporting still yields a single Trigger Phrase, however contaminated the stored prose is.
    let zip_bytes = storage.export_lora_dataset(None).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    let caption = caption_in_archive(&mut archive, "dungeon/contaminated_c0_r0_lora1024.txt");
    assert_eq!(caption.matches(TRIGGER_PHRASE).count(), 1, "{caption}");
    assert_eq!(caption.matches("orthographic").count(), 1, "{caption}");
}

#[tokio::test]
async fn test_section_without_any_physical_description_is_flagged() {
    let (_app, storage, ..) = setup_test_app();
    let map = CuratedMap::with_now(
        "undescribed",
        Theme::Cave,
        vec!["grotto".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_sections(vec![MapSection::new(0, 0, "   ")]);

    storage.save_curated(&map, &create_test_png(1024, 1024)).unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(!report.ready);
    assert_eq!(report.sections_ready, 0);
    assert_eq!(
        report.maps[0].issues[0].code,
        DatasetIssueCode::PhysicalDescriptionMissing
    );
}

#[tokio::test]
async fn test_unresolvable_sections_and_below_minimum_grids_are_flagged() {
    let (_app, storage, ..) = setup_test_app();

    // (a) A section whose 16×16 box starts past the Ground layer edge: (16, 16) at 64px needs
    //     1024..2048 on a 1024×1024 image.
    let out_of_bounds = CuratedMap::with_now(
        "out_of_bounds",
        Theme::Dungeon,
        vec!["crypt".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("Granite hall with carved pillars.")
    .with_sections(vec![
        MapSection::new(0, 0, "Northwest chamber with a stone dais."),
        MapSection::new(16, 16, "South-east chamber beyond the map edge."),
    ]);
    storage
        .save_curated(&out_of_bounds, &create_test_png(1024, 1024))
        .unwrap();

    // (b) An 8×8 cell map cannot contribute a single full 16×16 window.
    let too_small = CuratedMap::with_now(
        "too_small",
        Theme::Wilderness,
        vec!["river".into()],
        (512, 512),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("River ford with a broken footbridge.");
    storage
        .save_curated(&too_small, &create_test_png(512, 512))
        .unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(!report.ready);
    assert_eq!(report.maps_total, 2);

    let out_of_bounds_report = report
        .maps
        .iter()
        .find(|map| map.id == "out_of_bounds")
        .unwrap();
    assert_eq!(out_of_bounds_report.sections, 2);
    assert_eq!(out_of_bounds_report.ready_sections, 1);
    assert_eq!(
        out_of_bounds_report.issues.len(),
        1,
        "{:?}",
        out_of_bounds_report.issues
    );
    assert_eq!(
        out_of_bounds_report.issues[0].code,
        DatasetIssueCode::SectionUnresolvable
    );
    assert!(
        out_of_bounds_report.issues[0].detail.contains("(16, 16)"),
        "{}",
        out_of_bounds_report.issues[0].detail
    );

    let too_small_report = report.maps.iter().find(|map| map.id == "too_small").unwrap();
    assert_eq!(too_small_report.sections, 0);
    assert_eq!(
        too_small_report.issues[0].code,
        DatasetIssueCode::GridBelowMinimum
    );
}

#[tokio::test]
async fn test_unreadable_ground_layer_is_flagged() {
    let (_app, storage, ..) = setup_test_app();
    let map = ready_multisection_map("corrupt_ground");

    storage.save_curated(&map, b"\x89PNG\r\n\x1a\n not really a png").unwrap();

    let report = storage.audit_dataset_readiness(None).unwrap();
    assert!(!report.ready);
    assert_eq!(
        report.maps[0].issues[0].code,
        DatasetIssueCode::GroundImageUnreadable
    );
}

#[tokio::test]
async fn test_readiness_endpoint_reports_the_baseline_dataset_and_filters_by_theme() {
    let (app, _storage, catalog, blobs, _dir) = setup_test_app();
    seed_curated_ports(
        &catalog,
        &blobs,
        &ready_multisection_map("ready_multi"),
        &create_test_png(2048, 2048),
    )
    .await;

    let contaminated = CuratedMap::with_now(
        "cave_legacy",
        Theme::Cave,
        vec!["grotto".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("Limestone cave with a shallow pool.")
    .with_sections(vec![MapSection::new(0, 0, format!("{TRIGGER_PHRASE}, cave"))]);
    seed_curated_ports(&catalog, &blobs, &contaminated, &create_test_png(1024, 1024)).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dataset/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let report: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(report["ready"], false);
    assert_eq!(report["maps_total"], 2);
    assert_eq!(report["maps_ready"], 1);
    assert_eq!(report["sections_total"], 5);
    assert_eq!(report["sections_ready"], 4);
    let maps = report["maps"].as_array().unwrap();
    let cave = maps
        .iter()
        .find(|map| map["id"] == "cave_legacy")
        .expect("cave_legacy must be audited");
    assert_eq!(cave["theme"], "cave");
    assert_eq!(cave["sections"], 1);
    assert_eq!(cave["ready_sections"], 0);
    assert_eq!(
        cave["issues"][0]["code"],
        "physical_description_not_neutral"
    );

    // Theme filter mirrors the ZIP export's folder scope.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dataset/readiness?theme=dungeon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let scoped: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(scoped["ready"], true);
    assert_eq!(scoped["maps_total"], 1);
    assert_eq!(scoped["maps"][0]["id"], "ready_multi");

    // An unknown theme value is rejected, exactly like the export endpoint.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dataset/readiness?theme=bogus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_empty_store_is_not_a_ready_dataset() {
    let (app, ..) = setup_test_app();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/dataset/readiness")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let report: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["ready"], false);
    assert_eq!(report["maps_total"], 0);
    assert_eq!(report["sections_total"], 0);
}
