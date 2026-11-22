//! Integration tests for `BattlemapExportService`: full-resolution, verbatim-Layer ZIP bundles
//! over the in-memory catalog and blob doubles (ADR-0003, ADR-0015).

use std::io::{Cursor, Read};
use std::sync::Arc;

use async_trait::async_trait;
use tempfile::tempdir;
use zip::ZipArchive;

use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{CuratedMap, GridConfig, Layer, LayerRole, MapSection, Theme};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::storage_error::StorageError;
use curator::services::{ArchiveSpool, BattlemapExportService};

fn test_spool() -> (tempfile::TempDir, Arc<ArchiveSpool>) {
    let dir = tempdir().expect("spool tempdir");
    let spool = Arc::new(ArchiveSpool::new(dir.path()));
    (dir, spool)
}

fn in_memory_service() -> (
    tempfile::TempDir,
    BattlemapExportService,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let (spool_dir, spool) = test_spool();
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    (
        spool_dir,
        BattlemapExportService::new(catalog.clone(), blobs.clone(), spool),
        catalog,
        blobs,
    )
}

fn layer_key(id: &str, layer: &Layer) -> BlobKey {
    BlobKey::CuratedLayer {
        id: id.to_string(),
        index: layer.index,
        role: layer.role,
        elevation: layer.elevation,
    }
}

fn read_member_bytes(archive: &mut ZipArchive<Cursor<&Vec<u8>>>, name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    archive
        .by_name(name)
        .unwrap_or_else(|err| panic!("member '{name}' missing: {err}"))
        .read_to_end(&mut buf)
        .expect("read member bytes");
    buf
}

fn multi_layer_map(id: &str, theme: Theme) -> CuratedMap {
    CuratedMap::with_now(id, theme, vec!["fortress".into(), "stone".into()], (3072, 2048))
        .with_grid(GridConfig::new(70, 4, 8))
        .with_description("Multi-level basalt fortress with vaulted halls and a watchtower roof.")
        .with_layers(vec![
            Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (3072, 2048)),
            Layer::new(1, "1_upper_1.png", LayerRole::UpperFloor, Some(1), (3072, 2048)),
            Layer::new(2, "2_basement_1.png", LayerRole::Basement, Some(-1), (2048, 2048)),
            Layer::new(3, "3_roof.png", LayerRole::Roof, None, (3072, 2048)),
            Layer::new(4, "4_variant.png", LayerRole::Variant, None, (3072, 2048)),
            Layer::new(5, "5_exclude.png", LayerRole::Exclude, None, (1024, 1024)),
        ])
        .with_sections(vec![MapSection::new(
            0,
            0,
            "Fortified gatehouse with iron portcullis and guard alcoves.",
        )])
}

#[tokio::test]
async fn test_export_battlemap_packages_all_included_layers_verbatim_and_skips_excluded() {
    let (_spool_dir, service, catalog, blobs) = in_memory_service();
    let map = multi_layer_map("citadel_01", Theme::Dungeon);
    catalog.save_curated(&map).await.expect("save curated map");

    // Deliberately use raw sentinel byte sequences (not valid PNGs) to prove the service never
    // decodes or re-encodes Layer blobs and ships exact bytes verbatim.
    let payloads: Vec<(&Layer, Vec<u8>)> = vec![
        (&map.layers[0], b"RAW-GROUND-FULLRES-BYTES-0001".to_vec()),
        (&map.layers[1], b"RAW-UPPER-FLOOR-1-BYTES-0002".to_vec()),
        (&map.layers[2], b"RAW-BASEMENT-1-BYTES-0003".to_vec()),
        (&map.layers[3], b"RAW-ROOF-BYTES-0004".to_vec()),
        (&map.layers[4], b"RAW-VARIANT-BYTES-0005".to_vec()),
        (&map.layers[5], b"RAW-EXCLUDED-MUST-NOT-SHIP".to_vec()),
    ];
    for (layer, bytes) in &payloads {
        blobs
            .write_blob(&layer_key(&map.id, layer), bytes)
            .await
            .expect("write layer blob");
    }

    let spooled = service
        .export_battlemap("citadel_01")
        .await
        .expect("export single battlemap succeeds");
    let zip_bytes = std::fs::read(spooled.path()).expect("read spooled archive");
    drop(spooled);

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    // 1 manifest.json + 5 included layers (Exclude is skipped)
    assert_eq!(archive.len(), 6);

    // Every member must be Stored (ADR-0015).
    for i in 0..archive.len() {
        let member = archive.by_index(i).expect("member");
        assert_eq!(
            member.compression(),
            zip::CompressionMethod::Stored,
            "member {} must use CompressionMethod::Stored",
            member.name()
        );
    }

    // manifest.json is pretty-printed and round-trips to the exact CuratedMap.
    let manifest_bytes = read_member_bytes(&mut archive, "citadel_01/manifest.json");
    let manifest_str = String::from_utf8(manifest_bytes).expect("manifest is UTF-8");
    assert!(
        manifest_str.contains('\n'),
        "manifest.json must be pretty-printed"
    );
    let parsed_map: CuratedMap =
        serde_json::from_str(&manifest_str).expect("manifest deserializes as CuratedMap");
    assert_eq!(parsed_map.id, map.id);
    assert_eq!(parsed_map.tags, map.tags);
    assert_eq!(parsed_map.dimensions, map.dimensions);
    assert_eq!(parsed_map.grid, map.grid);
    assert_eq!(parsed_map.description, map.description);
    assert_eq!(parsed_map.sections, map.sections);
    assert_eq!(parsed_map.layers, map.layers);

    // Included layers match byte-for-byte with canonical filenames.
    for (expected_name, expected_bytes) in [
        ("citadel_01/0_ground.png", b"RAW-GROUND-FULLRES-BYTES-0001".as_slice()),
        ("citadel_01/1_upper_1.png", b"RAW-UPPER-FLOOR-1-BYTES-0002".as_slice()),
        ("citadel_01/2_basement_1.png", b"RAW-BASEMENT-1-BYTES-0003".as_slice()),
        ("citadel_01/3_roof.png", b"RAW-ROOF-BYTES-0004".as_slice()),
        ("citadel_01/4_variant.png", b"RAW-VARIANT-BYTES-0005".as_slice()),
    ] {
        assert_eq!(
            read_member_bytes(&mut archive, expected_name),
            expected_bytes,
            "layer {expected_name} must be verbatim"
        );
    }
    assert!(
        archive.by_name("citadel_01/5_exclude.png").is_err(),
        "excluded layer must not be packaged"
    );
}

#[tokio::test]
async fn test_export_battlemap_legacy_empty_layers_falls_back_to_zero_ground() {
    let (_spool_dir, service, catalog, blobs) = in_memory_service();
    let legacy = CuratedMap::with_now("legacy_crypt", Theme::Ruins, vec![], (1024, 1024));
    assert!(legacy.layers.is_empty());
    catalog.save_curated(&legacy).await.expect("save legacy map");

    let ground_bytes = b"LEGACY-GROUND-VERBATIM".to_vec();
    blobs
        .write_blob(
            &BlobKey::CuratedLayer {
                id: "legacy_crypt".to_string(),
                index: 0,
                role: LayerRole::Ground,
                elevation: Some(0),
            },
            &ground_bytes,
        )
        .await
        .expect("write legacy ground blob");

    let spooled = service
        .export_battlemap("legacy_crypt")
        .await
        .expect("legacy map exports");
    let zip_bytes = std::fs::read(spooled.path()).expect("read zip");
    drop(spooled);

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).expect("readable archive");
    assert_eq!(archive.len(), 2);
    assert!(archive.by_name("legacy_crypt/manifest.json").is_ok());
    assert_eq!(
        read_member_bytes(&mut archive, "legacy_crypt/0_ground.png"),
        ground_bytes
    );
}

#[tokio::test]
async fn test_export_battlemap_strict_returns_not_found_on_missing_map_or_layer() {
    let (spool_dir, service, catalog, blobs) = in_memory_service();

    // Missing catalog entry
    let err = service
        .export_battlemap("does_not_exist")
        .await
        .expect_err("unknown map must return NotFound");
    assert!(matches!(err, StorageError::NotFound(_)), "{err:?}");

    // Existing map with 2 included layers where second blob is missing
    let map = CuratedMap::with_now("partial_strict", Theme::Cave, vec![], (1024, 1024)).with_layers(
        vec![
            Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (1024, 1024)),
            Layer::new(1, "1_upper_1.png", LayerRole::UpperFloor, Some(1), (1024, 1024)),
        ],
    );
    catalog.save_curated(&map).await.expect("save map");
    blobs
        .write_blob(&layer_key("partial_strict", &map.layers[0]), b"GROUND-OK")
        .await
        .expect("write ground");

    let err = service
        .export_battlemap("partial_strict")
        .await
        .expect_err("missing layer blob must fail strict single-map export");
    assert!(matches!(err, StorageError::NotFound(_)), "{err:?}");

    // Partial spool file must be cleaned up on error
    let leftovers: Vec<_> = std::fs::read_dir(spool_dir.path())
        .map(|rd| rd.filter_map(Result::ok).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "failed export must not leave spool files behind: {leftovers:?}"
    );
}

#[tokio::test]
async fn test_export_battlemaps_filters_by_theme_and_skips_missing_blobs_gracefully() {
    let (_spool_dir, service, catalog, blobs) = in_memory_service();

    // 1. Complete dungeon map (2 layers)
    let dungeon_ok = CuratedMap::with_now("dungeon_ok", Theme::Dungeon, vec![], (1024, 1024))
        .with_layers(vec![
            Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (1024, 1024)),
            Layer::new(1, "1_roof.png", LayerRole::Roof, None, (1024, 1024)),
        ]);
    catalog.save_curated(&dungeon_ok).await.unwrap();
    blobs
        .write_blob(&layer_key("dungeon_ok", &dungeon_ok.layers[0]), b"D-GROUND")
        .await
        .unwrap();
    blobs
        .write_blob(&layer_key("dungeon_ok", &dungeon_ok.layers[1]), b"D-ROOF")
        .await
        .unwrap();

    // 2. Dungeon map where 1 of 2 layers is missing -> remaining layer + manifest still ship
    let dungeon_partial =
        CuratedMap::with_now("dungeon_partial", Theme::Dungeon, vec![], (1024, 1024)).with_layers(
            vec![
                Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (1024, 1024)),
                Layer::new(1, "1_upper_1.png", LayerRole::UpperFloor, Some(1), (1024, 1024)),
            ],
        );
    catalog.save_curated(&dungeon_partial).await.unwrap();
    blobs
        .write_blob(&layer_key("dungeon_partial", &dungeon_partial.layers[1]), b"D-UPPER-ONLY")
        .await
        .unwrap();

    // 3. Dungeon map with 0 readable layers -> manifest.json must be omitted too
    let dungeon_empty = CuratedMap::with_now("dungeon_missing_all", Theme::Dungeon, vec![], (1024, 1024))
        .with_layers(vec![Layer::new(
            0,
            "0_ground.png",
            LayerRole::Ground,
            Some(0),
            (1024, 1024),
        )]);
    catalog.save_curated(&dungeon_empty).await.unwrap();

    // 4. Cave map
    let cave = CuratedMap::with_now("cave_01", Theme::Cave, vec![], (1024, 1024)).with_layers(vec![
        Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (1024, 1024)),
    ]);
    catalog.save_curated(&cave).await.unwrap();
    blobs
        .write_blob(&layer_key("cave_01", &cave.layers[0]), b"CAVE-GROUND")
        .await
        .unwrap();

    // Export dungeon theme only
    let spooled = service
        .export_battlemaps(Some(&Theme::Dungeon))
        .await
        .expect("themed export succeeds");
    let zip_bytes = std::fs::read(spooled.path()).unwrap();
    drop(spooled);

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    assert_eq!(archive.len(), 5);
    assert!(archive.by_name("dungeon_ok/manifest.json").is_ok());
    assert_eq!(read_member_bytes(&mut archive, "dungeon_ok/0_ground.png"), b"D-GROUND");
    assert_eq!(read_member_bytes(&mut archive, "dungeon_ok/1_roof.png"), b"D-ROOF");
    assert!(archive.by_name("dungeon_partial/manifest.json").is_ok());
    assert!(archive.by_name("dungeon_partial/0_ground.png").is_err());
    assert_eq!(
        read_member_bytes(&mut archive, "dungeon_partial/1_upper_1.png"),
        b"D-UPPER-ONLY"
    );
    assert!(
        archive.by_name("dungeon_missing_all/manifest.json").is_err(),
        "map with 0 readable layers must omit manifest.json"
    );
    assert!(archive.by_name("cave_01/manifest.json").is_err());

    // Export all themes
    let spooled_all = service.export_battlemaps(None).await.expect("all export succeeds");
    let all_bytes = std::fs::read(spooled_all.path()).unwrap();
    drop(spooled_all);
    let mut all_archive = ZipArchive::new(Cursor::new(&all_bytes)).unwrap();
    assert_eq!(all_archive.len(), 7);
    assert_eq!(read_member_bytes(&mut all_archive, "cave_01/0_ground.png"), b"CAVE-GROUND");
}

#[tokio::test]
async fn test_export_selected_battlemaps_preserves_order_dedups_and_ignores_unknown() {
    let (_spool_dir, service, catalog, blobs) = in_memory_service();

    for (id, payload) in [
        ("alpha", b"ALPHA-BYTES".as_slice()),
        ("beta", b"BETA-BYTES".as_slice()),
    ] {
        let map = CuratedMap::with_now(id, Theme::Tavern, vec![], (1024, 1024)).with_layers(vec![
            Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (1024, 1024)),
        ]);
        catalog.save_curated(&map).await.unwrap();
        blobs.write_blob(&layer_key(id, &map.layers[0]), payload).await.unwrap();
    }

    let ids = vec![
        "beta".to_string(),
        "unknown_id".to_string(),
        "beta".to_string(),
        "alpha".to_string(),
    ];
    let spooled = service
        .export_selected_battlemaps(&ids)
        .await
        .expect("selected export succeeds");
    let zip_bytes = std::fs::read(spooled.path()).unwrap();
    drop(spooled);

    let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "beta/manifest.json",
            "beta/0_ground.png",
            "alpha/manifest.json",
            "alpha/0_ground.png",
        ]
    );
}

/// Blob double that returns a non-NotFound I/O error on every read, verifying non-NotFound errors
/// propagate rather than being swallowed as skipped layers.
struct IoFailingBlobStorage;

#[async_trait]
impl BlobStorage for IoFailingBlobStorage {
    async fn read_blob(&self, _key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::Io(std::io::Error::other("simulated disk I/O error")))
    }
    async fn write_blob(&self, _key: &BlobKey, _bytes: &[u8]) -> Result<(), StorageError> {
        unreachable!()
    }
    async fn delete_blob(&self, _key: &BlobKey) -> Result<(), StorageError> {
        unreachable!()
    }
    async fn exists_blob(&self, _key: &BlobKey) -> Result<bool, StorageError> {
        unreachable!()
    }
    async fn blob_size(&self, _key: &BlobKey) -> Result<u64, StorageError> {
        unreachable!()
    }
    async fn list_prefix(&self, _prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        unreachable!()
    }
}

#[tokio::test]
async fn test_export_battlemaps_propagates_non_not_found_storage_errors() {
    let (_spool_dir, spool) = test_spool();
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let map = CuratedMap::with_now("io_err_map", Theme::Dungeon, vec![], (1024, 1024))
        .with_layers(vec![Layer::new(
            0,
            "0_ground.png",
            LayerRole::Ground,
            Some(0),
            (1024, 1024),
        )]);
    catalog.save_curated(&map).await.unwrap();

    let service = BattlemapExportService::new(catalog, Arc::new(IoFailingBlobStorage), spool);
    let err = service
        .export_battlemaps(None)
        .await
        .expect_err("I/O error must propagate");
    assert!(matches!(err, StorageError::Io(_)), "{err:?}");
}

#[tokio::test]
async fn test_battlemap_exports_are_pure_reads() {
    let (_spool_dir, service, catalog, blobs) = in_memory_service();
    let map = multi_layer_map("pure_read_map", Theme::Wilderness);
    catalog.save_curated(&map).await.unwrap();
    for layer in map.layers.iter().filter(|l| l.role.is_included()) {
        blobs
            .write_blob(&layer_key(&map.id, layer), b"PURE-READ-BYTES")
            .await
            .unwrap();
    }

    let before_maps = catalog.list_curated(&Default::default()).await.unwrap();
    let before_snapshots = catalog.list_dataset_snapshots(None).await.unwrap();
    let before_readiness = catalog.list_map_readiness().await.unwrap();
    let before_blobs = blobs.list_prefix(&BlobKey::CuratedDir { id: None }).await.unwrap();

    let a1 = service.export_battlemap("pure_read_map").await.unwrap();
    drop(a1);
    let a2 = service.export_battlemaps(Some(&Theme::Wilderness)).await.unwrap();
    drop(a2);
    let a3 = service
        .export_selected_battlemaps(&["pure_read_map".to_string()])
        .await
        .unwrap();
    drop(a3);

    assert_eq!(catalog.list_curated(&Default::default()).await.unwrap(), before_maps);
    assert_eq!(catalog.list_dataset_snapshots(None).await.unwrap(), before_snapshots);
    assert_eq!(catalog.list_map_readiness().await.unwrap(), before_readiness);
    assert_eq!(
        blobs.list_prefix(&BlobKey::CuratedDir { id: None }).await.unwrap(),
        before_blobs
    );
}
