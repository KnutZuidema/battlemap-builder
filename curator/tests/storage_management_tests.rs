use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::api::{create_router, AppState};
use curator::domain::{
    BenchmarkMetadata, Candidate, CuratedLayer, CuratedMap, LayerRole, PurgeStagingResult,
    RejectionReason, Source, StorageStats, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use curator::services::StorageMaintenanceService;

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

/// Seeds a curated map into the ports the storage routes read: the catalog entry with the
/// implicit Ground layer, plus the Ground layer blob the curated byte accounting sums.
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

/// Seeds a staging candidate and its single layer blob into the ports.
async fn seed_staging(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    candidate: &Candidate,
    png: &[u8],
) {
    catalog
        .save_candidate(candidate)
        .await
        .expect("save staging candidate");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: candidate.id.clone(),
                index: 0,
            },
            png,
        )
        .await
        .expect("write staging layer blob");
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

#[tokio::test]
async fn test_get_storage_stats_empty() {
    let (catalog, blobs) = in_memory_ports();
    let maintenance = StorageMaintenanceService::new(catalog, blobs);

    let stats: StorageStats = maintenance.storage_stats().await.expect("stats succeeds");

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

#[tokio::test]
async fn test_get_storage_stats_populated() {
    let (catalog, blobs) = in_memory_ports();

    // 1. Populate staging with three candidates, one layer blob each.
    let reddit_cand = Candidate::with_now(
        "reddit_post_1",
        Source::Reddit,
        "reddit_post_1.png",
        (1920, 1080),
    )
    .with_source_url("https://reddit.com/r/battlemaps/1")
    .with_title("Dungeon Map 30x30")
    .with_author("cartographer")
    .with_parsed_gridless(true)
    .with_parsed_dimensions((1920, 1080));
    let reddit_bytes = b"\x89PNG\r\n\x1a\nreddit-image-data-payload";
    seed_staging(&catalog, &blobs, &reddit_cand, reddit_bytes).await;

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
    seed_staging(&catalog, &blobs, &benchmark_meta.to_candidate(), gen_bytes).await;

    let root_cand = Candidate::with_now("root_cand_1", Source::Manual, "root.png", (2048, 2048));
    let root_bytes = b"\x89PNG\r\n\x1a\nroot-candidate-data-payload";
    seed_staging(&catalog, &blobs, &root_cand, root_bytes).await;

    // 2. Populate curated themes: Dungeon & Wilderness.
    let dungeon_map = CuratedMap::with_now(
        "dungeon_map_1",
        Theme::Dungeon,
        vec!["dungeon".into(), "stone".into()],
        (1920, 1080),
    );
    let dungeon_bytes = b"\x89PNG\r\n\x1a\ncurated-dungeon-map-bytes";
    seed_curated_ports(&catalog, &blobs, &dungeon_map, dungeon_bytes).await;

    let wilderness_map = CuratedMap::with_now(
        "wilderness_map_1",
        Theme::Wilderness,
        vec!["forest".into(), "river".into()],
        (2048, 2048),
    );
    let wilderness_bytes = b"\x89PNG\r\n\x1a\ncurated-wilderness-map-bytes";
    seed_curated_ports(&catalog, &blobs, &wilderness_map, wilderness_bytes).await;

    // 3. Populate the rejection log: the catalog holds the records, the log blob the bytes.
    let rejections_payload = br#"{"records":[]}"#;
    catalog
        .log_rejection(
            "bad_candidate_1",
            RejectionReason::GridBurnedIn,
            Some("Burned in 50px grid"),
        )
        .await
        .expect("reject candidate");
    catalog
        .log_rejection(
            "bad_candidate_2",
            RejectionReason::BlurryLowRes,
            Some("Only 300x300"),
        )
        .await
        .expect("reject candidate");
    blobs
        .write_blob(&BlobKey::RejectionLog, rejections_payload)
        .await
        .expect("write rejection log");

    // 4. Populate telemetry.
    let ledger_payload = br#"{"cumulative_spend": 0.08, "generation_count": 2}"#;
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            },
            ledger_payload,
        )
        .await
        .expect("write telemetry");

    let stats = StorageMaintenanceService::new(catalog, blobs)
        .storage_stats()
        .await
        .expect("get storage stats");

    // Staging is one uniform bundle layout: three layer blobs and no other blob.
    assert!(stats.staging.bytes > 0);
    assert_eq!(stats.staging.file_count, 3);

    let reddit_stats = stats.staging_breakdown.get("reddit").expect("reddit breakdown");
    assert_eq!(reddit_stats.bytes, 0);
    assert_eq!(reddit_stats.file_count, 0);

    let gen_stats = stats.staging_breakdown.get("generated").expect("generated breakdown");
    assert_eq!(gen_stats.bytes, 0);
    assert_eq!(gen_stats.file_count, 0);

    let root_stats = stats.staging_breakdown.get("root").expect("root breakdown");
    assert_eq!(root_stats.bytes, stats.staging.bytes);
    assert_eq!(root_stats.file_count, 3);

    assert_eq!(
        stats.staging.bytes,
        reddit_stats.bytes + gen_stats.bytes + root_stats.bytes
    );
    assert_eq!(
        stats.staging.file_count,
        reddit_stats.file_count + gen_stats.file_count + root_stats.file_count
    );

    // Curated: two maps, one Ground layer blob each.
    assert!(stats.curated.bytes > 0);
    assert_eq!(stats.curated.file_count, 2);
    let dungeon_theme_stats = stats.curated_by_theme.get(&Theme::Dungeon).expect("dungeon stats");
    assert_eq!(dungeon_theme_stats.map_count, 1);
    assert!(dungeon_theme_stats.bytes > 0);
    assert_eq!(dungeon_theme_stats.file_count, 1);

    let wilderness_theme_stats = stats.curated_by_theme.get(&Theme::Wilderness).expect("wilderness stats");
    assert_eq!(wilderness_theme_stats.map_count, 1);
    assert!(wilderness_theme_stats.bytes > 0);
    assert_eq!(wilderness_theme_stats.file_count, 1);

    let cave_theme_stats = stats.curated_by_theme.get(&Theme::Cave).expect("cave stats");
    assert_eq!(cave_theme_stats.map_count, 0);
    assert_eq!(cave_theme_stats.bytes, 0);
    assert_eq!(cave_theme_stats.file_count, 0);

    assert_eq!(stats.rejected.file_count, 2);
    assert_eq!(stats.rejected.bytes, rejections_payload.len() as u64);

    assert_eq!(stats.telemetry.file_count, 1);
    assert_eq!(stats.telemetry.bytes, ledger_payload.len() as u64);

    assert_eq!(
        stats.total_bytes,
        stats.staging.bytes + stats.curated.bytes + stats.rejected.bytes + stats.telemetry.bytes
    );
    assert_eq!(
        stats.total_files,
        stats.staging.file_count + stats.curated.file_count + stats.rejected.file_count + stats.telemetry.file_count
    );
}

#[tokio::test]
async fn test_purge_staging_resets_staging_and_preserves_curated_rejected_telemetry() {
    let (catalog, blobs) = in_memory_ports();

    // 1. Populate staging candidates.
    let reddit_cand = Candidate::with_now(
        "reddit_purge_1",
        Source::Reddit,
        "reddit_purge_1.png",
        (1000, 1000),
    )
    .with_source_url("https://reddit.com/1")
    .with_title("Test Purge")
    .with_author("author")
    .with_parsed_gridless(true)
    .with_parsed_dimensions((1000, 1000));
    seed_staging(
        &catalog,
        &blobs,
        &reddit_cand,
        b"reddit-image-data-payload-to-purge",
    )
    .await;

    let cand_root = Candidate::with_now("cand_root", Source::Manual, "root.png", (800, 800));
    seed_staging(&catalog, &blobs, &cand_root, b"root-image-data-payload-to-purge").await;

    // 2. Populate a curated map: only its Ground layer blob may exist.
    let dungeon_map = CuratedMap::with_now(
        "curated_keep_1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (1920, 1080),
    );
    let curated_image_data = create_test_png(200, 200);
    seed_curated_ports(&catalog, &blobs, &dungeon_map, &curated_image_data).await;
    let curated_keys_before = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("curated_keep_1".to_string()),
        })
        .await
        .expect("list curated blobs");
    assert!(curated_keys_before
        .iter()
        .all(|key| !key.to_path().contains("lora1024")));
    assert!(curated_keys_before
        .iter()
        .any(|key| key.to_path().ends_with("0_ground.png")));

    // 3. Populate the rejection log.
    catalog
        .log_rejection(
            "bad_rejected_keep",
            RejectionReason::BlurryLowRes,
            Some("Visible blur"),
        )
        .await
        .expect("reject candidate");
    let rejections_payload = br#"{"records":[{"id":"bad_rejected_keep"}]}"#;
    blobs
        .write_blob(&BlobKey::RejectionLog, rejections_payload)
        .await
        .expect("write rejection log");

    // 4. Populate telemetry.
    let telemetry_key = BlobKey::TelemetryLog {
        name: "cost_ledger.json".to_string(),
    };
    let ledger_content = br#"{"session_spend": 0.05, "total_spend": 1.25}"#;
    blobs
        .write_blob(&telemetry_key, ledger_content)
        .await
        .expect("write cost ledger");

    let maintenance = StorageMaintenanceService::new(catalog.clone(), blobs.clone());
    let stats_before = maintenance.storage_stats().await.expect("stats before");
    assert_eq!(stats_before.staging.file_count, 2);
    assert!(stats_before.staging.bytes > 0);
    let expected_purged_files = stats_before.staging.file_count;
    let expected_reclaimed_bytes = stats_before.staging.bytes;

    // Execute PURGE.
    let purge_result: PurgeStagingResult = maintenance.purge_staging().await.expect("purge succeeds");

    assert_eq!(purge_result.purged_files, expected_purged_files);
    assert_eq!(purge_result.reclaimed_bytes, expected_reclaimed_bytes);
    assert_eq!(purge_result.remaining_staging_files, 0);

    assert!(catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .expect("list staging post-purge")
        .is_empty());
    assert!(blobs
        .list_prefix(&BlobKey::StagingDir { id: None })
        .await
        .expect("list staging blobs")
        .is_empty());

    let stats_after = maintenance.storage_stats().await.expect("stats after");
    assert_eq!(stats_after.staging.file_count, 0);
    assert_eq!(stats_after.staging.bytes, 0);
    assert_eq!(stats_after.staging_breakdown.get("root").unwrap().file_count, 0);
    assert_eq!(stats_after.staging_breakdown.get("reddit").unwrap().file_count, 0);
    assert_eq!(stats_after.staging_breakdown.get("generated").unwrap().file_count, 0);

    // Curated maps and their blobs remain strictly preserved.
    let curated_ground = blobs
        .read_blob(&curated_ground_key(&dungeon_map))
        .await
        .expect("curated Ground blob still exists");
    assert_eq!(curated_ground, curated_image_data);
    let curated_list = catalog
        .list_curated(&curator::ports::catalog_repository::CuratedFilter {
            theme: Some(Theme::Dungeon),
            ..Default::default()
        })
        .await
        .expect("list curated");
    assert_eq!(curated_list.len(), 1);
    assert_eq!(curated_list[0].id, "curated_keep_1");
    assert_eq!(stats_after.curated.bytes, stats_before.curated.bytes);
    assert_eq!(stats_after.curated.file_count, stats_before.curated.file_count);

    // Rejection catalog and its log blob remain strictly preserved.
    assert!(catalog
        .is_rejected("bad_rejected_keep")
        .await
        .expect("is_rejected"));
    assert_eq!(
        blobs.read_blob(&BlobKey::RejectionLog).await.expect("read rejection log"),
        rejections_payload
    );
    assert_eq!(stats_after.rejected.bytes, stats_before.rejected.bytes);
    assert_eq!(stats_after.rejected.file_count, stats_before.rejected.file_count);

    // Telemetry remains strictly preserved.
    assert_eq!(
        blobs.read_blob(&telemetry_key).await.expect("read telemetry"),
        ledger_content
    );
    assert_eq!(stats_after.telemetry.bytes, stats_before.telemetry.bytes);
    assert_eq!(stats_after.telemetry.file_count, stats_before.telemetry.file_count);
}

#[tokio::test]
async fn test_api_storage_stats_and_purge_endpoints() {
    let (app, catalog, blobs) = setup_test_app();

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
    // their real footprint instead of zeros. `rejected.bytes` is the log file's size while
    // `rejected.file_count` is the number of rejection records the catalog holds (one here).
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
    //    staging blob is left.
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
}
