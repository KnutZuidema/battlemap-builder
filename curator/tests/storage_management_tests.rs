use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;

use curator::adapters::{FilesystemStorage, InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::api::{create_router, AppState};
use curator::domain::{
    BenchmarkMetadata, Candidate, CuratedLayer, CuratedMap, LayerRole, PurgeStagingResult,
    RedditSidecarMetadata, RejectionReason, Source, StorageStats, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use curator::ports::storage::StorageRepository;

/// Fresh in-memory catalog and blob ports: these tests exercise the storage management routes.
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
/// The storage maintenance routes were cut over to the ports, so a test targeting them seeds the
/// ports; the filesystem repository keeps serving the routes that still read `state.storage`.
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

/// Blob key of a curated map's `Ground` layer, resolved exactly like the storage maintenance
/// service resolves the bytes it accounts for.
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

/// Seeds a curated map into the ports the cut-over storage routes read: the catalog entry with
/// the implicit Ground layer [`FilesystemStorage::save_curated`] synthesizes, plus the Ground
/// layer blob the curated byte accounting sums.
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
    let img = image::RgbImage::new(width, height);
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("write png");
    bytes
}

#[test]
fn test_get_storage_stats_empty() {
    let dir = tempdir().expect("tempdir");
    let storage = FilesystemStorage::new(dir.path());

    let stats: StorageStats = storage.get_storage_stats().expect("stats succeeds");

    assert_eq!(stats.staging.bytes, 0);
    assert_eq!(stats.staging.file_count, 0);

    assert_eq!(stats.staging_breakdown.get("root").unwrap().bytes, 0);
    assert_eq!(stats.staging_breakdown.get("root").unwrap().file_count, 0);
    assert_eq!(stats.staging_breakdown.get("reddit").unwrap().bytes, 0);
    assert_eq!(stats.staging_breakdown.get("reddit").unwrap().file_count, 0);
    assert_eq!(stats.staging_breakdown.get("generated").unwrap().bytes, 0);
    assert_eq!(stats.staging_breakdown.get("generated").unwrap().file_count, 0);

    assert_eq!(stats.curated.bytes, 0);
    assert_eq!(stats.curated.file_count, 0);

    for theme in Theme::ALL {
        let theme_stats = stats.curated_by_theme.get(theme).expect("theme present");
        assert_eq!(theme_stats.theme, *theme);
        assert_eq!(theme_stats.bytes, 0);
        assert_eq!(theme_stats.file_count, 0);
        assert_eq!(theme_stats.map_count, 0);
    }

    assert_eq!(stats.rejected.bytes, 0);
    assert_eq!(stats.rejected.file_count, 0);

    assert_eq!(stats.telemetry.bytes, 0);
    assert_eq!(stats.telemetry.file_count, 0);

    assert_eq!(stats.total_bytes, 0);
    assert_eq!(stats.total_files, 0);
}

#[test]
fn test_get_storage_stats_populated() {
    let dir = tempdir().expect("tempdir");
    let storage = FilesystemStorage::new(dir.path());

    // 1. Populate staging/reddit
    let reddit_meta = RedditSidecarMetadata {
        id: "reddit_post_1".to_string(),
        source: Source::Reddit,
        source_url: "https://reddit.com/r/battlemaps/1".to_string(),
        title: "Dungeon Map 30x30".to_string(),
        author: "cartographer".to_string(),
        parsed_gridless: true,
        parsed_dimensions: Some((1920, 1080)),
        created_at: chrono::Utc::now(),
    };
    let reddit_bytes = b"\x89PNG\r\n\x1a\nreddit-image-data-payload";
    storage
        .save_staging_reddit(&reddit_meta, reddit_bytes)
        .expect("save reddit");

    // 2. Populate staging/generated
    let benchmark_meta = BenchmarkMetadata::new(
        "gen_bench_1",
        "openai/dall-e-3",
        "cavern",
        "A moody cavern battlemap",
        Some(120),
        Some(1024),
        1500,
        0.04,
    );
    let gen_bytes = b"\x89PNG\r\n\x1a\ngenerated-image-data-payload";
    storage
        .save_staging_generated(&benchmark_meta, gen_bytes)
        .expect("save generated");

    // 3. Populate direct staging root
    let root_cand = Candidate::with_now(
        "root_cand_1",
        Source::Manual,
        "root.png",
        (2048, 2048),
    );
    let root_bytes = b"\x89PNG\r\n\x1a\nroot-candidate-data-payload";
    storage
        .save_staging(&root_cand, root_bytes)
        .expect("save root staging");

    // 4. Populate curated themes: Dungeon & Wilderness
    let dungeon_map = CuratedMap::with_now(
        "dungeon_map_1",
        Theme::Dungeon,
        vec!["dungeon".into(), "stone".into()],
        (1920, 1080),
    );
    let dungeon_bytes = b"\x89PNG\r\n\x1a\ncurated-dungeon-map-bytes";
    storage
        .save_curated(&dungeon_map, dungeon_bytes)
        .expect("save curated dungeon");

    let wilderness_map = CuratedMap::with_now(
        "wilderness_map_1",
        Theme::Wilderness,
        vec!["forest".into(), "river".into()],
        (2048, 2048),
    );
    let wilderness_bytes = b"\x89PNG\r\n\x1a\ncurated-wilderness-map-bytes";
    storage
        .save_curated(&wilderness_map, wilderness_bytes)
        .expect("save curated wilderness");

    // 5. Populate rejections.json
    storage
        .reject_candidate(
            "bad_candidate_1",
            RejectionReason::GridBurnedIn,
            Some("Burned in 50px grid"),
        )
        .expect("reject candidate");
    storage
        .reject_candidate(
            "bad_candidate_2",
            RejectionReason::BlurryLowRes,
            Some("Only 300x300"),
        )
        .expect("reject candidate");

    // 6. Populate telemetry directory
    let telemetry_dir = dir.path().join("telemetry");
    std::fs::create_dir_all(&telemetry_dir).expect("telemetry dir");
    let ledger_payload = br#"{"cumulative_spend": 0.08, "generation_count": 2}"#;
    std::fs::write(telemetry_dir.join("cost_ledger.json"), ledger_payload)
        .expect("write telemetry");

    // Compute stats
    let stats = storage.get_storage_stats().expect("get storage stats");

    // Validate Staging. `get_storage_stats` runs the startup flatten first, so the fixture's
    // legacy `staging/{reddit,generated}/{id}/` bundles are now uniform `staging/{id}/` bundles:
    // the entire footprint lands in the `root` bucket, while the pre-seeded `reddit`/`generated`
    // buckets remain present (keeping the SPA-facing JSON shape stable) but empty.
    assert!(stats.staging.bytes > 0);
    assert_eq!(stats.staging.file_count, 6); // 3 candidates, each bundle has 1 png + 1 manifest.json

    let reddit_stats = stats.staging_breakdown.get("reddit").expect("reddit breakdown");
    assert_eq!(reddit_stats.bytes, 0);
    assert_eq!(reddit_stats.file_count, 0);

    let gen_stats = stats.staging_breakdown.get("generated").expect("generated breakdown");
    assert_eq!(gen_stats.bytes, 0);
    assert_eq!(gen_stats.file_count, 0);

    let root_stats = stats.staging_breakdown.get("root").expect("root breakdown");
    assert_eq!(root_stats.bytes, stats.staging.bytes);
    assert_eq!(root_stats.file_count, 6);

    assert_eq!(
        stats.staging.bytes,
        reddit_stats.bytes + gen_stats.bytes + root_stats.bytes
    );
    assert_eq!(
        stats.staging.file_count,
        reddit_stats.file_count + gen_stats.file_count + root_stats.file_count
    );

    // Validate Curated
    assert!(stats.curated.bytes > 0);
    assert!(stats.curated.file_count >= 4); // 2 curated maps, each bundle has layer image + manifest.json
    let dungeon_theme_stats = stats.curated_by_theme.get(&Theme::Dungeon).expect("dungeon stats");
    assert_eq!(dungeon_theme_stats.map_count, 1);
    assert!(dungeon_theme_stats.bytes > 0);
    assert!(dungeon_theme_stats.file_count >= 2);

    let wilderness_theme_stats = stats.curated_by_theme.get(&Theme::Wilderness).expect("wilderness stats");
    assert_eq!(wilderness_theme_stats.map_count, 1);
    assert!(wilderness_theme_stats.bytes > 0);
    assert!(wilderness_theme_stats.file_count >= 2);

    let cave_theme_stats = stats.curated_by_theme.get(&Theme::Cave).expect("cave stats");
    assert_eq!(cave_theme_stats.map_count, 0);
    assert_eq!(cave_theme_stats.bytes, 0);
    assert_eq!(cave_theme_stats.file_count, 0);

    // Validate Rejected
    assert_eq!(stats.rejected.file_count, 2);
    assert!(stats.rejected.bytes > 0);

    // Validate Telemetry
    assert_eq!(stats.telemetry.file_count, 1);
    assert_eq!(stats.telemetry.bytes, ledger_payload.len() as u64);

    // Validate Totals
    assert_eq!(
        stats.total_bytes,
        stats.staging.bytes + stats.curated.bytes + stats.rejected.bytes + stats.telemetry.bytes
    );
    assert_eq!(
        stats.total_files,
        stats.staging.file_count + stats.curated.file_count + stats.rejected.file_count + stats.telemetry.file_count
    );
}

#[test]
fn test_purge_staging_resets_staging_and_preserves_curated_rejected_telemetry() {
    let dir = tempdir().expect("tempdir");
    let storage = FilesystemStorage::new(dir.path());

    // 1. Populate staging candidates across subfolders
    let reddit_meta = RedditSidecarMetadata {
        id: "reddit_purge_1".to_string(),
        source: Source::Reddit,
        source_url: "https://reddit.com/1".to_string(),
        title: "Test Purge".to_string(),
        author: "author".to_string(),
        parsed_gridless: true,
        parsed_dimensions: Some((1000, 1000)),
        created_at: chrono::Utc::now(),
    };
    storage
        .save_staging_reddit(&reddit_meta, b"reddit-image-data-payload-to-purge")
        .expect("save reddit");

    let cand_root = Candidate::with_now("cand_root", Source::Manual, "root.png", (800, 800));
    storage
        .save_staging(&cand_root, b"root-image-data-payload-to-purge")
        .expect("save root");

    // 2. Populate Curated Map with LoRA companion files
    let dungeon_map = CuratedMap::with_now(
        "curated_keep_1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (1920, 1080),
    );
    let curated_image_data = create_test_png(200, 200);
    storage
        .save_curated(&dungeon_map, &curated_image_data)
        .expect("save curated");

    // Verify curated files exist before purge and companion files are NOT on disk
    let dungeon_bundle_dir = dir.path().join("curated/curated_keep_1");
    let lora_png = dungeon_bundle_dir.join("curated_keep_1_lora1024.png");
    let lora_txt = dungeon_bundle_dir.join("curated_keep_1_lora1024.txt");
    assert!(!lora_png.exists(), "LoRA companion png should NOT exist on disk");
    assert!(!lora_txt.exists(), "LoRA companion txt should NOT exist on disk");
    assert!(dungeon_bundle_dir.join("manifest.json").is_file());
    assert!(dungeon_bundle_dir.join("0_ground.png").is_file());

    // 3. Populate Rejection Log
    storage
        .reject_candidate(
            "bad_rejected_keep",
            RejectionReason::BlurryLowRes,
            Some("Visible blur"),
        )
        .expect("reject candidate");
    let rejections_file = dir.path().join("rejected/rejections.json");
    assert!(rejections_file.is_file(), "rejections.json must exist");
    let rejections_content_before = std::fs::read_to_string(&rejections_file).expect("read rejections");

    // 4. Populate Telemetry Ledger
    let telemetry_file = dir.path().join("telemetry/cost_ledger.json");
    std::fs::create_dir_all(telemetry_file.parent().unwrap()).expect("create telemetry dir");
    let ledger_content = br#"{"session_spend": 0.05, "total_spend": 1.25}"#;
    std::fs::write(&telemetry_file, ledger_content).expect("write cost ledger");

    // Check stats before purge
    let stats_before = storage.get_storage_stats().expect("stats before");
    assert!(stats_before.staging.file_count > 0);
    assert!(stats_before.staging.bytes > 0);
    let expected_purged_files = stats_before.staging.file_count;
    let expected_reclaimed_bytes = stats_before.staging.bytes;

    // Execute PURGE
    let purge_result: PurgeStagingResult = storage.purge_staging().expect("purge succeeds");

    assert_eq!(purge_result.purged_files, expected_purged_files);
    assert_eq!(purge_result.reclaimed_bytes, expected_reclaimed_bytes);
    assert_eq!(purge_result.remaining_staging_files, 0);

    // Verify Staging is completely empty
    let staging_candidates = storage.list_staging().expect("list staging post-purge");
    assert_eq!(staging_candidates.len(), 0);

    let stats_after = storage.get_storage_stats().expect("stats after");
    assert_eq!(stats_after.staging.file_count, 0);
    assert_eq!(stats_after.staging.bytes, 0);
    assert_eq!(stats_after.staging_breakdown.get("root").unwrap().file_count, 0);
    assert_eq!(stats_after.staging_breakdown.get("reddit").unwrap().file_count, 0);
    assert_eq!(stats_after.staging_breakdown.get("generated").unwrap().file_count, 0);

    // Verify Curated maps & companion files remain STRICTLY PRESERVED
    let curated_retrieved = storage
        .get_curated_image(&Theme::Dungeon, "curated_keep_1")
        .expect("curated image still exists");
    assert_eq!(curated_retrieved, curated_image_data);

    let curated_list = storage.list_curated(Some(&Theme::Dungeon)).expect("list curated");
    assert_eq!(curated_list.len(), 1);
    assert_eq!(curated_list[0].id, "curated_keep_1");

    assert!(!lora_png.exists(), "LoRA companion png should NOT exist on disk");
    assert!(!lora_txt.exists(), "LoRA companion txt should NOT exist on disk");
    assert!(dungeon_bundle_dir.join("manifest.json").is_file());
    assert!(dungeon_bundle_dir.join("0_ground.png").is_file());

    assert_eq!(stats_after.curated.bytes, stats_before.curated.bytes);
    assert_eq!(stats_after.curated.file_count, stats_before.curated.file_count);

    // Verify Rejection Catalog remains STRICTLY PRESERVED
    assert!(storage.is_rejected("bad_rejected_keep").expect("is_rejected"));
    assert!(rejections_file.is_file());
    let rejections_content_after = std::fs::read_to_string(&rejections_file).expect("read rejections");
    assert_eq!(rejections_content_before, rejections_content_after);
    assert_eq!(stats_after.rejected.bytes, stats_before.rejected.bytes);
    assert_eq!(stats_after.rejected.file_count, stats_before.rejected.file_count);

    // Verify Telemetry Ledger remains STRICTLY PRESERVED
    assert!(telemetry_file.is_file());
    let telemetry_content_after = std::fs::read(&telemetry_file).expect("read telemetry");
    assert_eq!(telemetry_content_after, ledger_content);
    assert_eq!(stats_after.telemetry.bytes, stats_before.telemetry.bytes);
    assert_eq!(stats_after.telemetry.file_count, stats_before.telemetry.file_count);
}

#[tokio::test]
async fn test_api_storage_stats_and_purge_endpoints() {
    let (app, _storage, catalog, blobs, dir) = setup_test_app();

    // 1. Initial GET /api/storage/stats should return empty stats
    let req = Request::builder()
        .uri("/api/storage/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let stats_json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(stats_json["staging"]["bytes"], 0);
    assert_eq!(stats_json["staging"]["file_count"], 0);
    for key in ["root", "reddit", "generated"] {
        assert_eq!(stats_json["staging_breakdown"][key]["bytes"], 0);
        assert_eq!(stats_json["staging_breakdown"][key]["file_count"], 0);
    }
    assert_eq!(stats_json["curated"]["bytes"], 0);
    assert_eq!(stats_json["curated"]["file_count"], 0);
    assert_eq!(stats_json["rejected"]["bytes"], 0);
    assert_eq!(stats_json["rejected"]["file_count"], 0);
    assert_eq!(stats_json["telemetry"]["bytes"], 0);
    assert_eq!(stats_json["telemetry"]["file_count"], 0);
    assert_eq!(stats_json["total_bytes"], 0);
    assert_eq!(stats_json["total_files"], 0);

    // 2. Populate the ports the cut-over storage routes read: one staged candidate with its layer
    //    blob, one curated map with its Ground layer blob, and one rejection record.
    let cand1 = Candidate::with_now("api_c1", Source::Reddit, "c1.png", (1024, 768));
    catalog.save_candidate(&cand1).await.unwrap();
    let staging_payload = b"\x89PNG\r\n\x1a\napi-test-candidate-1";
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "api_c1".to_string(),
                index: 0,
            },
            staging_payload,
        )
        .await
        .unwrap();

    let dungeon_map = CuratedMap::with_now(
        "api_curated_1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (1024, 768),
    );
    let curated_payload = b"\x89PNG\r\n\x1a\napi-curated-map";
    seed_curated_ports(&catalog, &blobs, &dungeon_map, curated_payload).await;

    catalog
        .log_rejection("api_rej_1", RejectionReason::GridBurnedIn, None)
        .await
        .unwrap();
    let rejections_payload = br#"{"id":"api_rej_1","reason":"grid_burned_in"}"#;
    blobs
        .write_blob(&BlobKey::RejectionLog, rejections_payload)
        .await
        .unwrap();
    let ledger_payload = br#"{"cumulative_spend":0.12}"#;
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            },
            ledger_payload,
        )
        .await
        .unwrap();
    let audit_payload = br#"{"runs":3}"#;
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "generation_audit.json".to_string(),
            },
            audit_payload,
        )
        .await
        .unwrap();

    // 3. GET /api/storage/stats with populated data
    let req = Request::builder()
        .uri("/api/storage/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let stats_json: Value = serde_json::from_slice(&body_bytes).unwrap();

    // Port-based accounting counts blob-addressed files: the staging bundle's layer blob is one
    // file, while its manifest is catalog metadata rather than a blob.
    assert_eq!(stats_json["staging"]["bytes"], staging_payload.len() as u64);
    assert_eq!(stats_json["staging"]["file_count"], 1);
    assert_eq!(stats_json["curated"]["bytes"], curated_payload.len() as u64);
    assert_eq!(stats_json["curated"]["file_count"], 1);
    // Staging is one uniform bundle layout, so the whole footprint is the `root` bucket and the
    // two legacy source buckets stay present at zero; the SPA renders all three.
    assert_eq!(
        stats_json["staging_breakdown"]["root"]["bytes"],
        stats_json["staging"]["bytes"]
    );
    assert_eq!(
        stats_json["staging_breakdown"]["root"]["file_count"],
        stats_json["staging"]["file_count"]
    );
    assert_eq!(stats_json["staging_breakdown"]["reddit"]["bytes"], 0);
    assert_eq!(stats_json["staging_breakdown"]["reddit"]["file_count"], 0);
    assert_eq!(stats_json["staging_breakdown"]["generated"]["bytes"], 0);
    assert_eq!(stats_json["staging_breakdown"]["generated"]["file_count"], 0);
    assert_eq!(
        stats_json["staging"]["bytes"],
        stats_json["staging_breakdown"]["root"]["bytes"].as_u64().unwrap()
            + stats_json["staging_breakdown"]["reddit"]["bytes"].as_u64().unwrap()
            + stats_json["staging_breakdown"]["generated"]["bytes"].as_u64().unwrap()
    );
    // The rejection log and the telemetry ledgers are addressable blobs, so the endpoint reports
    // their real footprint instead of zeros.
    assert_eq!(
        stats_json["rejected"]["bytes"],
        rejections_payload.len() as u64
    );
    assert_eq!(stats_json["rejected"]["file_count"], 1);
    assert_eq!(
        stats_json["telemetry"]["bytes"],
        (ledger_payload.len() + audit_payload.len()) as u64
    );
    assert_eq!(stats_json["telemetry"]["file_count"], 2);
    assert_eq!(
        stats_json["total_bytes"],
        stats_json["staging"]["bytes"].as_u64().unwrap()
            + stats_json["curated"]["bytes"].as_u64().unwrap()
            + stats_json["rejected"]["bytes"].as_u64().unwrap()
            + stats_json["telemetry"]["bytes"].as_u64().unwrap()
    );
    assert_eq!(
        stats_json["total_files"],
        stats_json["staging"]["file_count"].as_u64().unwrap()
            + stats_json["curated"]["file_count"].as_u64().unwrap()
            + stats_json["rejected"]["file_count"].as_u64().unwrap()
            + stats_json["telemetry"]["file_count"].as_u64().unwrap()
    );

    // Verify /api/status shows 1 staging candidate
    let req = Request::builder()
        .uri("/api/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let body_bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let status_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(status_json["staging_count"], 1);
    assert_eq!(status_json["curated_count"], 1);

    // 4. POST /api/storage/purge-staging
    let req = Request::builder()
        .uri("/api/storage/purge-staging")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let purge_json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(purge_json["purged_files"], 1);
    assert_eq!(
        purge_json["reclaimed_bytes"],
        staging_payload.len() as u64
    );
    assert_eq!(purge_json["remaining_staging_files"], 0);

    // 5. Verify /api/status has 0 staging candidates, but curated remains 1
    let req = Request::builder()
        .uri("/api/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let body_bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let status_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(status_json["staging_count"], 0);
    assert_eq!(status_json["curated_count"], 1);

    // 6. The staging area is drained through the ports the purge acts on: no candidate row and no
    //    staging blob is left. `GET /api/staging` still reads the filesystem repository (cutting
    //    that listing over is issue #174's territory) and this test seeds only the ports, so the
    //    drained state is asserted on the ports themselves rather than on that endpoint's empty
    //    filesystem tree.
    assert!(catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap()
        .is_empty());
    assert!(blobs
        .list_prefix(&BlobKey::StagingDir { id: None })
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        blobs
            .list_prefix(&BlobKey::CuratedDir { id: None })
            .await
            .unwrap()
            .len(),
        1
    );

    // `/api/staging` answers 200 with the filesystem repository's own (untouched, empty) view.
    let req = Request::builder()
        .uri("/api/staging")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let staging_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(staging_json.as_array().unwrap().is_empty());

    // 7. Verify GET /api/storage/stats shows 0 staging bytes
    let req = Request::builder()
        .uri("/api/storage/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let body_bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let stats_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(stats_json["staging"]["bytes"], 0);
    assert_eq!(stats_json["staging"]["file_count"], 0);
    assert_eq!(stats_json["staging_breakdown"]["root"]["bytes"], 0);
    assert_eq!(stats_json["staging_breakdown"]["root"]["file_count"], 0);
    assert_eq!(stats_json["curated"]["bytes"], curated_payload.len() as u64);
    assert_eq!(stats_json["curated"]["file_count"], 1);
    // The purge only enumerates `staging/`, so the rejection log and the telemetry ledgers keep
    // their footprint.
    assert_eq!(
        stats_json["rejected"]["bytes"],
        rejections_payload.len() as u64
    );
    assert_eq!(stats_json["rejected"]["file_count"], 1);
    assert_eq!(
        stats_json["telemetry"]["bytes"],
        (ledger_payload.len() + audit_payload.len()) as u64
    );
    assert_eq!(stats_json["telemetry"]["file_count"], 2);
    assert_eq!(
        stats_json["total_bytes"],
        curated_payload.len() as u64
            + rejections_payload.len() as u64
            + (ledger_payload.len() + audit_payload.len()) as u64
    );
    assert_eq!(stats_json["total_files"], 1 + 1 + 2);

    let _ = dir;
}
