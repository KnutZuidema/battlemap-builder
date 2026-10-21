//! Integration tests for `DatasetExportService`: the in-memory LoRA ZIP archive, the selected-ids
//! export and the dataset readiness audit, all over the in-memory catalog and blob doubles
//! (ADR-0007).

use std::io::{Cursor, Read};
use std::sync::Arc;

use zip::ZipArchive;

use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{
    CuratedMap, DatasetIssueCode, GridConfig, Layer, LayerRole, MapSection, Theme, TRIGGER_PHRASE,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::services::DatasetExportService;

/// Fresh in-memory catalog and blob ports, plus the service wired to them.
fn in_memory_service() -> (
    DatasetExportService,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    (
        DatasetExportService::new(catalog.clone(), blobs.clone()),
        catalog,
        blobs,
    )
}

/// Blob key of a curated map's Ground layer, as the service resolves it from `map.layers`.
fn ground_key(id: &str) -> BlobKey {
    BlobKey::CuratedLayer {
        id: id.to_string(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    }
}

fn test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(width, height);
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("write PNG");
    bytes
}

fn ground_layer(width: u32, height: u32) -> Vec<Layer> {
    vec![Layer::new(
        0,
        "0_ground.png",
        LayerRole::Ground,
        Some(0),
        (width, height),
    )]
}

/// A 2048×2048 curated map at a 64px grid: four 16×16 Map Sections with neutral descriptions.
fn multisection_map(id: &str) -> CuratedMap {
    CuratedMap::with_now(id, Theme::Dungeon, vec!["crypt".into()], (2048, 2048))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_description("Granite necropolis with carved pillars and iron braziers.")
        .with_layers(ground_layer(2048, 2048))
        .with_sections(vec![
            MapSection::new(0, 0, "Northwest crypt with a shattered sarcophagus lid."),
            MapSection::new(16, 0, "Northeast guard room lined with weapon racks."),
            MapSection::new(0, 16, "Southwest flooded cell behind iron bars."),
            MapSection::new(16, 16, "Southeast torture chamber with a rusted iron maiden."),
        ])
}

async fn save_map(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<InMemoryBlobStorage>,
    map: &CuratedMap,
    ground_png: &[u8],
) {
    catalog.save_curated(map).await.expect("save curated map");
    blobs
        .write_blob(&ground_key(&map.id), ground_png)
        .await
        .expect("write ground layer");
}

fn caption_in(archive: &mut ZipArchive<Cursor<&Vec<u8>>>, name: &str) -> String {
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap_or_else(|err| panic!("caption '{name}' missing: {err}"))
        .read_to_string(&mut text)
        .expect("read caption");
    text
}

#[tokio::test]
async fn test_export_lora_dataset_builds_one_deflated_png_and_txt_pair_per_section() {
    let (service, catalog, blobs) = in_memory_service();
    save_map(
        &catalog,
        &blobs,
        &multisection_map("ready_multi"),
        &test_png(2048, 2048),
    )
    .await;

    let zip_bytes = service
        .export_lora_dataset(None)
        .await
        .expect("export succeeds");

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    assert_eq!(archive.len(), 8, "four sections ship one PNG and one caption each");

    for (col, row) in [(0u32, 0u32), (16, 0), (0, 16), (16, 16)] {
        let png_name = format!("dungeon/ready_multi_c{col}_r{row}_lora1024.png");
        let txt_name = format!("dungeon/ready_multi_c{col}_r{row}_lora1024.txt");

        assert_eq!(
            archive.by_name(&png_name).expect("PNG member").compression(),
            zip::CompressionMethod::Deflated,
            "{png_name} must be DEFLATE-compressed"
        );

        let mut png_bytes = Vec::new();
        archive
            .by_name(&png_name)
            .expect("PNG member")
            .read_to_end(&mut png_bytes)
            .expect("read PNG member");
        let decoded = image::load_from_memory(&png_bytes).expect("member decodes as an image");
        assert_eq!((decoded.width(), decoded.height()), (1024, 1024));

        let caption = caption_in(&mut archive, &txt_name);
        assert!(caption.starts_with(TRIGGER_PHRASE), "{caption}");
        assert_eq!(
            caption.matches(TRIGGER_PHRASE).count(),
            1,
            "Trigger Phrase must be applied exactly once: {caption}"
        );
    }

    let southwest = caption_in(&mut archive, "dungeon/ready_multi_c0_r16_lora1024.txt");
    assert!(
        southwest.contains("flooded cell behind iron bars"),
        "{southwest}"
    );
}

#[tokio::test]
async fn test_export_skips_unresolvable_section_instead_of_rescaling_it() {
    let (service, catalog, blobs) = in_memory_service();
    // The (16, 0) window starts at 1024px on a 1024px Ground layer: it cannot be cropped without
    // rescaling, so only the first section reaches the archive.
    let map = CuratedMap::with_now("partial", Theme::Cave, vec!["cavern".into()], (1024, 1024))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_layers(ground_layer(1024, 1024))
        .with_sections(vec![
            MapSection::new(0, 0, "Entry chamber with a collapsed archway."),
            MapSection::new(16, 0, "Gallery beyond the archway."),
        ]);
    save_map(&catalog, &blobs, &map, &test_png(1024, 1024)).await;

    let zip_bytes = service
        .export_lora_dataset(None)
        .await
        .expect("export succeeds");

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    assert_eq!(archive.len(), 2, "only the resolvable section ships");
    assert!(archive.by_name("cave/partial_c0_r0_lora1024.png").is_ok());
    assert!(archive.by_name("cave/partial_c0_r0_lora1024.txt").is_ok());
    assert!(archive.by_name("cave/partial_c16_r0_lora1024.png").is_err());
}

#[tokio::test]
async fn test_export_skips_map_whose_ground_layer_cannot_be_read() {
    let (service, catalog, _blobs) = in_memory_service();
    let map = multisection_map("no_bundle");
    catalog.save_curated(&map).await.expect("save curated map");

    let zip_bytes = service
        .export_lora_dataset(None)
        .await
        .expect("a missing ground layer never fails the whole export");

    let archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    assert_eq!(archive.len(), 0);
}

#[tokio::test]
async fn test_export_selected_lora_dataset_honours_requested_ids_and_order() {
    let (service, catalog, blobs) = in_memory_service();
    save_map(&catalog, &blobs, &multisection_map("beta"), &test_png(2048, 2048)).await;
    save_map(&catalog, &blobs, &multisection_map("alpha"), &test_png(2048, 2048)).await;

    let selected = vec!["beta".to_string(), "missing".to_string(), "beta".to_string(), "alpha".to_string()];
    let zip_bytes = service
        .export_selected_lora_dataset(&selected)
        .await
        .expect("export succeeds");

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    assert_eq!(archive.len(), 16, "two maps ship four sections each, once each");
    assert!(
        archive.by_index(0).unwrap().name().starts_with("dungeon/beta_"),
        "first requested id leads the archive"
    );
    assert!(
        archive
            .by_index(archive.len() - 1)
            .unwrap()
            .name()
            .starts_with("dungeon/alpha_"),
        "last requested id closes the archive"
    );
    assert!(archive.by_name("dungeon/missing_c0_r0_lora1024.png").is_err());

    let empty = service
        .export_selected_lora_dataset(&[])
        .await
        .expect("empty selection succeeds");
    assert_eq!(
        ZipArchive::new(Cursor::new(&empty)).unwrap().len(),
        0,
        "an empty selection yields an empty archive"
    );
}

#[tokio::test]
async fn test_audit_dataset_readiness_reports_every_section_issue_code() {
    let (service, catalog, blobs) = in_memory_service();

    let clean = multisection_map("clean_map");
    save_map(&catalog, &blobs, &clean, &test_png(2048, 2048)).await;

    // Section (0, 0) leaks camera directives, (16, 16) resolves to no description at all and
    // (64, 0) starts beyond the Ground layer, so all three sample defects show up at once.
    let dirty = CuratedMap::with_now("dirty_map", Theme::Cave, vec!["cavern".into()], (2048, 2048))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_layers(ground_layer(2048, 2048))
        .with_sections(vec![
            MapSection::new(0, 0, "Flat 2D plan view of a cavern with a sinkhole."),
            MapSection::new(16, 16, "   "),
            MapSection::new(64, 0, "Chamber beyond the eastern wall."),
        ]);
    save_map(&catalog, &blobs, &dirty, &test_png(2048, 2048)).await;

    // A curated map whose Ground layer blob was never written.
    let mut missing = CuratedMap::with_now(
        "missing_ground",
        Theme::Ruins,
        vec!["temple".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_layers(ground_layer(1024, 1024))
    .with_description("Sunken temple hall with a broken colonnade.");
    missing.sections = vec![MapSection::new(0, 0, "Nave with a toppled altar stone.")];
    catalog.save_curated(&missing).await.expect("save curated map");

    let report = service
        .audit_dataset_readiness(None)
        .await
        .expect("audit succeeds");

    assert!(!report.ready, "{report:?}");
    assert_eq!(report.maps_total, 3);
    assert_eq!(report.maps_ready, 1);
    assert_eq!(report.sections_total, 8);
    assert_eq!(report.sections_ready, 4);

    let clean_report = report
        .maps
        .iter()
        .find(|map| map.id == "clean_map")
        .expect("clean map reported");
    assert_eq!(clean_report.theme, "dungeon");
    assert_eq!(clean_report.sections, 4);
    assert_eq!(clean_report.ready_sections, 4);
    assert!(clean_report.issues.is_empty(), "{:?}", clean_report.issues);

    let dirty_report = report
        .maps
        .iter()
        .find(|map| map.id == "dirty_map")
        .expect("dirty map reported");
    assert_eq!(dirty_report.theme, "cave");
    assert_eq!(dirty_report.sections, 3);
    assert_eq!(dirty_report.ready_sections, 0);
    let mut codes: Vec<DatasetIssueCode> = dirty_report.issues.iter().map(|issue| issue.code).collect();
    codes.sort_by_key(|code| code.as_str());
    let mut expected = vec![
        DatasetIssueCode::PhysicalDescriptionMissing,
        DatasetIssueCode::PhysicalDescriptionNotNeutral,
        DatasetIssueCode::SectionUnresolvable,
    ];
    expected.sort_by_key(|code| code.as_str());
    assert_eq!(codes, expected, "{:?}", dirty_report.issues);

    let missing_report = report
        .maps
        .iter()
        .find(|map| map.id == "missing_ground")
        .expect("missing ground layer reported");
    assert_eq!(missing_report.sections, 0);
    assert_eq!(missing_report.ready_sections, 0);
    assert_eq!(missing_report.issues.len(), 1);
    assert_eq!(
        missing_report.issues[0].code,
        DatasetIssueCode::GroundLayerMissing
    );

    // The JSON payload the triage SPA renders keeps the stable snake_case issue codes.
    let json = serde_json::to_string(&report).expect("serialize report");
    assert!(json.contains("\"code\":\"ground_layer_missing\""), "{json}");
    assert!(
        json.contains("\"code\":\"physical_description_not_neutral\""),
        "{json}"
    );

    let cave_only = service
        .audit_dataset_readiness(Some(&Theme::Cave))
        .await
        .expect("filtered audit succeeds");
    assert_eq!(cave_only.maps_total, 1);
    assert_eq!(cave_only.maps[0].id, "dirty_map");
}

#[tokio::test]
async fn test_audit_dataset_readiness_of_empty_store_is_not_a_ready_dataset() {
    let (service, _catalog, _blobs) = in_memory_service();

    let report = service
        .audit_dataset_readiness(None)
        .await
        .expect("audit succeeds");

    assert!(!report.ready);
    assert_eq!(report.maps_total, 0);
    assert_eq!(report.sections_total, 0);
    assert!(report.maps.is_empty());
}
