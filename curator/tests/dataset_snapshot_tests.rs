//! Integration tests for Issue #216: Frozen Dataset Snapshot (ADR-0009).
//!
//! Acceptance criteria:
//! 1. Freezing a scope produces an immutable DatasetSnapshot (ZIP stored in BlobStorage at
//!    `BlobKey::DatasetSnapshot { id }`, catalog record in `CatalogRepository`).
//! 2. Survives restart & byte-identical re-fetch on filesystem adapters.
//! 3. Readiness-gated freeze: refused with HTTP 422/400 and `DATASET_NOT_READY` with blocking
//!    issues and remediations when scope is unready or empty.
//! 4. Reference-checked deletion: refused with HTTP 409 `SNAPSHOT_IN_USE` when referenced by a run;
//!    deletes both blob and catalog record when unreferenced.
//! 5. Dataset Library UI exposes freeze and snapshot library controls.
//! 6. `GET /api/dataset/export` stays a pure read and never mutates snapshots.

use std::io::Cursor;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;
use zip::ZipArchive;

use curator::adapters::{
    FilesystemBlobStorage, FilesystemCatalogRepository, InMemoryBlobStorage,
    InMemoryCatalogRepository,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    CuratedLayer, CuratedMap, DatasetIssueCode, GridConfig, LayerRole, MapSection, Theme,
    TRIGGER_PHRASE,
};
use curator::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::services::DatasetExportService;

fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(width, height);
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("write test PNG");
    bytes
}

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

fn ready_multisection_map(id: &str, theme: Theme) -> CuratedMap {
    CuratedMap::with_now(id, theme, vec![theme.as_str().to_string()], (2048, 2048))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_description("Granite chamber with carved pillars and iron braziers.")
        .with_sections(vec![
            MapSection::new(0, 0, "Northwest crypt with a shattered sarcophagus lid."),
            MapSection::new(16, 0, "Northeast guard room lined with weapon racks."),
            MapSection::new(0, 16, "Southwest flooded cell behind iron bars."),
            MapSection::new(16, 16, "Southeast torture chamber with an iron maiden."),
        ])
}

fn setup_test_app() -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let state = AppState::new(catalog.clone(), blobs.clone());
    let router = create_router(state);
    (router, catalog, blobs)
}

// ---------------------------------------------------------------------------
// 1. Freezing a scope produces an immutable DatasetSnapshot
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_freeze_ready_scope_produces_immutable_dataset_snapshot() {
    std::env::set_var("R2_PUBLIC_URL", "https://pub-r2.example.com");

    let (app, catalog, blobs) = setup_test_app();
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    let map1 = ready_multisection_map("dungeon-map-1", Theme::Dungeon);
    let map2 = ready_multisection_map("dungeon-map-2", Theme::Dungeon);
    let png_bytes = create_test_png(2048, 2048);
    seed_curated_ports(&cat_trait, &blob_trait, &map1, &png_bytes).await;
    seed_curated_ports(&cat_trait, &blob_trait, &map2, &png_bytes).await;

    // Freeze snapshot via POST /api/training/datasets with JSON body
    let req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"theme":"dungeon"}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let snapshot: Value = serde_json::from_slice(&body_bytes).unwrap();

    let id = snapshot["id"].as_str().expect("snapshot id must be string");
    assert!(id.starts_with("ds_dungeon_"), "id was: {id}");
    assert_eq!(snapshot["theme"], "dungeon");
    assert_eq!(snapshot["trigger_phrase"], TRIGGER_PHRASE);
    assert_eq!(snapshot["maps_count"], 2);
    assert_eq!(snapshot["samples_count"], 8);
    assert_eq!(
        snapshot["artifact_key"],
        format!("artifacts/datasets/{id}.zip")
    );
    assert_eq!(
        snapshot["artifact_url"],
        format!("https://pub-r2.example.com/artifacts/datasets/{id}.zip")
    );

    let sha256 = snapshot["sha256"].as_str().expect("sha256 string");
    assert_eq!(sha256.len(), 64, "sha256 hex must be 64 characters");
    let size_bytes = snapshot["size_bytes"].as_u64().expect("size_bytes u64");
    assert!(size_bytes > 0);

    // Verify blob in BlobStorage
    let blob_key = BlobKey::DatasetSnapshot { id: id.to_string() };
    let stored_bytes = blob_trait
        .read_blob(&blob_key)
        .await
        .expect("blob must exist in storage");
    assert_eq!(stored_bytes.len() as u64, size_bytes);
    assert_eq!(sha256_digest(&stored_bytes), sha256);

    // Verify retrievable by ID via GET /api/training/datasets/{id}
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/api/training/datasets/{id}"))
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_val: Value =
        serde_json::from_slice(&to_bytes(get_resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(get_val["id"], id);
    assert_eq!(get_val["sha256"], sha256);

    // Verify ZIP archive bytes retrievable via GET /api/training/datasets/{id}/archive
    let arch_req = Request::builder()
        .method("GET")
        .uri(format!("/api/training/datasets/{id}/archive"))
        .body(Body::empty())
        .unwrap();
    let arch_resp = app.clone().oneshot(arch_req).await.unwrap();
    assert_eq!(arch_resp.status(), StatusCode::OK);
    assert_eq!(
        arch_resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );
    let arch_bytes = to_bytes(arch_resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(arch_bytes.to_vec(), stored_bytes);

    // Also verify alias /download
    let dl_req = Request::builder()
        .method("GET")
        .uri(format!("/api/training/datasets/{id}/download"))
        .body(Body::empty())
        .unwrap();
    let dl_resp = app.clone().oneshot(dl_req).await.unwrap();
    assert_eq!(dl_resp.status(), StatusCode::OK);
    let dl_bytes = to_bytes(dl_resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(dl_bytes.to_vec(), stored_bytes);

    // Verify listing via GET /api/training/datasets?theme=dungeon
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/training/datasets?theme=dungeon")
        .body(Body::empty())
        .unwrap();
    let list_resp = app.oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_val: Value =
        serde_json::from_slice(&to_bytes(list_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let list = list_val.as_array().expect("array of snapshots");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);

    std::env::remove_var("R2_PUBLIC_URL");
}

// ---------------------------------------------------------------------------
// 2. Survives restart & byte-identical re-fetch
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_snapshot_survives_restart_with_byte_identical_refetch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path();

    let orig_catalog = Arc::new(FilesystemCatalogRepository::new(data_dir));
    let orig_blobs = Arc::new(FilesystemBlobStorage::new(data_dir));

    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = orig_catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = orig_blobs.clone();

    let map = ready_multisection_map("survive-map-1", Theme::Dungeon);
    let png = create_test_png(2048, 2048);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &png).await;

    let service = DatasetExportService::new(cat_trait.clone(), blob_trait.clone());
    let created = service
        .freeze_dataset_snapshot(Some(&Theme::Dungeon), Some("ds_frozen_test_01".into()))
        .await
        .expect("freeze snapshot must succeed");

    assert_eq!(created.id, "ds_frozen_test_01");
    let original_bytes = service
        .read_dataset_snapshot_archive("ds_frozen_test_01")
        .await
        .expect("read archive bytes");
    assert_eq!(sha256_digest(&original_bytes), created.sha256);

    // Simulate Curator pod restart: drop original instances and create fresh ones pointing to the same data directory
    drop(service);
    drop(orig_catalog);
    drop(orig_blobs);

    let restarted_catalog = Arc::new(FilesystemCatalogRepository::new(data_dir));
    let restarted_blobs = Arc::new(FilesystemBlobStorage::new(data_dir));
    let restarted_service = DatasetExportService::new(restarted_catalog, restarted_blobs);

    // Catalog record must survive with exact same digest and metadata
    let loaded = restarted_service
        .get_dataset_snapshot("ds_frozen_test_01")
        .await
        .expect("snapshot record must survive restart");
    assert_eq!(loaded.id, created.id);
    assert_eq!(loaded.sha256, created.sha256);
    assert_eq!(loaded.size_bytes, created.size_bytes);
    assert_eq!(loaded.maps_count, created.maps_count);
    assert_eq!(loaded.samples_count, created.samples_count);
    assert_eq!(loaded.trigger_phrase, created.trigger_phrase);

    // Re-fetched ZIP yields byte-identical content
    let restarted_bytes = restarted_service
        .read_dataset_snapshot_archive("ds_frozen_test_01")
        .await
        .expect("read archive after restart");
    assert_eq!(
        restarted_bytes, original_bytes,
        "archive bytes after restart must be strictly byte-identical"
    );
    assert_eq!(sha256_digest(&restarted_bytes), created.sha256);
}

// ---------------------------------------------------------------------------
// 3. Readiness-gated freeze: refused when scope is unready or empty
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_readiness_gated_freeze_refusal_with_issue_codes_and_remediations() {
    let (app, catalog, blobs) = setup_test_app();
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    // 1. Empty catalog freeze refusal
    let empty_req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets?theme=dungeon")
        .body(Body::empty())
        .unwrap();
    let empty_resp = app.clone().oneshot(empty_req).await.unwrap();
    assert_eq!(empty_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let empty_body: Value =
        serde_json::from_slice(&to_bytes(empty_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(empty_body["code"], "DATASET_NOT_READY");
    let issues = empty_body["blocking_issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["code"], "empty_dataset");
    assert!(issues[0]["remediation"].as_str().unwrap().contains("Curate"));

    // 2. Curated map with defects (out of bounds section & contaminated description)
    let dirty_map = CuratedMap::with_now(
        "dirty-dungeon-1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (1024, 1024),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_description("Contaminated top-down perpendicular camera view")
    .with_sections(vec![
        // Out of bounds section (col 16 is at 1024px with 16*64 width = 2048px > 1024px)
        MapSection::new(16, 0, "Contaminated top-down perpendicular camera view"),
    ]);

    seed_curated_ports(&cat_trait, &blob_trait, &dirty_map, &create_test_png(1024, 1024)).await;

    let dirty_req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"theme":"dungeon"}"#))
        .unwrap();

    let dirty_resp = app.clone().oneshot(dirty_req).await.unwrap();

    let dirty_body: Value =
        serde_json::from_slice(&to_bytes(dirty_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(dirty_body["code"], "DATASET_NOT_READY");
    assert!(dirty_body["error"].as_str().unwrap().contains("not ready"));

    let blocking_issues = dirty_body["blocking_issues"].as_array().unwrap();
    assert!(
        !blocking_issues.is_empty(),
        "blocking_issues must explicitly name every issue"
    );

    let section_unres = blocking_issues
        .iter()
        .find(|b| b["code"] == "section_unresolvable")
        .expect("must contain section_unresolvable");
    assert_eq!(
        section_unres["remediation"],
        DatasetIssueCode::SectionUnresolvable.remediation()
    );
    assert_eq!(section_unres["count"], 1);
    assert_eq!(section_unres["maps"], serde_json::json!(["dirty-dungeon-1"]));

    let desc_not_neutral = blocking_issues
        .iter()
        .find(|b| b["code"] == "physical_description_not_neutral")
        .expect("must contain physical_description_not_neutral");
    assert_eq!(
        desc_not_neutral["remediation"],
        DatasetIssueCode::PhysicalDescriptionNotNeutral.remediation()
    );

    // Verify nothing was stored in catalog or blobs
    let list = catalog
        .list_dataset_snapshots(None)
        .await
        .unwrap();
    assert!(list.is_empty(), "no snapshot must be saved on refusal");
}

// ---------------------------------------------------------------------------
// 4. Reference-checked deletion
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reference_checked_deletion() {
    let (app, catalog, blobs) = setup_test_app();
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    let map = ready_multisection_map("delete-check-map", Theme::Dungeon);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &create_test_png(2048, 2048)).await;

    // Freeze snapshot
    let service = DatasetExportService::new(cat_trait.clone(), blob_trait.clone());
    let snapshot = service
        .freeze_dataset_snapshot(Some(&Theme::Dungeon), Some("snap-ref-check".into()))
        .await
        .unwrap();

    // 1. When referenced by a training run -> Refuse deletion with HTTP 409 SNAPSHOT_IN_USE
    catalog.register_snapshot_reference(&snapshot.id);

    let del_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/training/datasets/{}", snapshot.id))
        .body(Body::empty())
        .unwrap();
    let del_resp = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::CONFLICT);

    let del_err: Value =
        serde_json::from_slice(&to_bytes(del_resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(del_err["code"], "SNAPSHOT_IN_USE");
    assert!(del_err["error"].as_str().unwrap().contains("referenced"));

    // Verify snapshot and blob still exist
    assert!(catalog.get_dataset_snapshot(&snapshot.id).await.is_ok());
    assert!(blobs
        .read_blob(&BlobKey::DatasetSnapshot {
            id: snapshot.id.clone()
        })
        .await
        .is_ok());

    // 2. When unreferenced -> Delete succeeds and removes both blob and catalog record
    catalog.unregister_snapshot_reference(&snapshot.id);

    let del_req_2 = Request::builder()
        .method("DELETE")
        .uri(format!("/api/training/datasets/{}", snapshot.id))
        .body(Body::empty())
        .unwrap();
    let del_resp_2 = app.clone().oneshot(del_req_2).await.unwrap();
    assert_eq!(del_resp_2.status(), StatusCode::OK);

    // Verify catalog record deleted
    assert!(catalog.get_dataset_snapshot(&snapshot.id).await.is_err());

    // Verify blob deleted from storage
    assert!(blobs
        .read_blob(&BlobKey::DatasetSnapshot {
            id: snapshot.id.clone()
        })
        .await
        .is_err());
}

#[tokio::test]
async fn test_reference_checked_deletion_on_filesystem_run_manifest() {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path();

    let catalog = Arc::new(FilesystemCatalogRepository::new(data_dir));
    let blobs = Arc::new(FilesystemBlobStorage::new(data_dir));

    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    let map = ready_multisection_map("fs-run-map", Theme::Dungeon);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &create_test_png(2048, 2048)).await;

    let service = DatasetExportService::new(cat_trait.clone(), blob_trait.clone());
    let snapshot = service
        .freeze_dataset_snapshot(Some(&Theme::Dungeon), Some("snap-fs-check".into()))
        .await
        .unwrap();

    // Create a training run on disk referencing this snapshot
    let run_dir = data_dir.join("training").join("run-001");
    std::fs::create_dir_all(&run_dir).unwrap();
    let run_json = serde_json::json!({
        "id": "run-001",
        "dataset_snapshot_id": snapshot.id,
        "state": "running"
    });
    std::fs::write(run_dir.join("run.json"), serde_json::to_string(&run_json).unwrap()).unwrap();

    // Check reference check reports true
    assert!(
        catalog
            .is_dataset_snapshot_referenced(&snapshot.id)
            .await
            .unwrap()
    );

    // Deletion must fail with Referenced
    let del_res = service.delete_dataset_snapshot(&snapshot.id).await;
    assert!(del_res.is_err());

    // Remove the run directory
    std::fs::remove_dir_all(&run_dir).unwrap();
    assert!(
        !catalog
            .is_dataset_snapshot_referenced(&snapshot.id)
            .await
            .unwrap()
    );

    // Now deletion succeeds
    service
        .delete_dataset_snapshot(&snapshot.id)
        .await
        .expect("unreferenced deletion must succeed");
    assert!(catalog.get_dataset_snapshot(&snapshot.id).await.is_err());
}

// ---------------------------------------------------------------------------
// 6. GET /api/dataset/export stays a pure read
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_get_dataset_export_stays_pure_read() {
    let (app, catalog, blobs) = setup_test_app();
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    let map = ready_multisection_map("export-pure-map", Theme::Dungeon);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &create_test_png(2048, 2048)).await;

    // Verify baseline: 0 snapshots in catalog and 0 dataset blobs
    let initial_snapshots = catalog.list_dataset_snapshots(None).await.unwrap();
    assert_eq!(initial_snapshots.len(), 0);

    let prefix = BlobKey::DatasetSnapshotsDir;
    let initial_blobs = blobs.list_prefix(&prefix).await.unwrap();
    assert_eq!(initial_blobs.len(), 0);

    // Call GET /api/dataset/export
    let req = Request::builder()
        .method("GET")
        .uri("/api/dataset/export?theme=dungeon")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );

    let export_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(!export_bytes.is_empty());

    // Verify ZipArchive contains samples
    let archive = ZipArchive::new(Cursor::new(&export_bytes)).expect("valid zip");
    assert!(!archive.is_empty());

    // Verify strictly NO dataset snapshot was written or mutated in catalog or blob storage
    let after_snapshots = catalog.list_dataset_snapshots(None).await.unwrap();
    assert_eq!(
        after_snapshots.len(),
        0,
        "GET /api/dataset/export must NEVER save a DatasetSnapshot record"
    );

    let after_blobs = blobs.list_prefix(&prefix).await.unwrap();
    assert_eq!(
        after_blobs.len(),
        0,
        "GET /api/dataset/export must NEVER write to BlobKey::DatasetSnapshot"
    );
}

// ---------------------------------------------------------------------------
// 5. Dataset Library UI verification
// ---------------------------------------------------------------------------
#[test]
fn test_ui_assets_expose_dataset_snapshot_controls() {
    let html = include_str!("../src/ui/assets/index.html");
    let js = include_str!("../src/ui/assets/app.js");
    let css = include_str!("../src/ui/assets/style.css");

    // Freeze Snapshot button in toolbar
    assert!(html.contains("id=\"btn-freeze-snapshot\""));
    assert!(html.contains("Freeze Snapshot"));

    // Dataset Snapshots panel
    assert!(html.contains("id=\"dataset-snapshots-panel\""));
    assert!(html.contains("id=\"snapshots-list\""));
    assert!(html.contains("id=\"freeze-progress-panel\""));
    assert!(html.contains("id=\"freeze-progress-bar\""));
    assert!(html.contains("id=\"freeze-phase-badge\""));

    // JavaScript handlers & API calls
    assert!(js.contains("/api/training/datasets"));
    assert!(js.contains("DATASET_NOT_READY"));
    assert!(js.contains("SNAPSHOT_IN_USE"));
    assert!(js.contains("DATASET_ISSUE_ACTIONS"));
    assert!(js.contains("freezeDatasetSnapshot"));
    assert!(js.contains("deleteDatasetSnapshot"));
    assert!(js.contains("fetchDatasetSnapshots"));
    assert!(js.contains("updateFreezeProgressUI"));
    assert!(js.contains("checkActiveFreezeStatus"));
    assert!(js.contains("performFreezeSnapshotWithProgress"));

    // CSS styling
    assert!(css.contains(".dataset-snapshots-panel"));
    assert!(css.contains(".snapshot-card"));
    assert!(css.contains(".readiness-issue-remediation"));
    assert!(css.contains(".freeze-progress-panel"));
    assert!(css.contains(".freeze-progress-bar"));
    assert!(css.contains(".freeze-phase-badge"));
}

/// Issue #245: the readiness modal fixes flagged maps in place, so the assets must expose the
/// remediation controls and the API wiring behind them.
#[test]
fn test_ui_assets_expose_dataset_readiness_remediation_actions() {
    let html = include_str!("../src/ui/assets/index.html");
    let js = include_str!("../src/ui/assets/app.js");
    let css = include_str!("../src/ui/assets/style.css");

    assert!(html.contains("id=\"btn-fix-all-descriptions\""));
    assert!(html.contains("Fix All Descriptions"));

    // Per-issue action buttons and the handlers behind them.
    assert!(js.contains("DATASET_ISSUE_ACTIONS"));
    assert!(js.contains("renderDatasetIssueRow"));
    assert!(js.contains("data-remediate="));
    assert!(js.contains("remediateDatasetIssue"));
    assert!(js.contains("fixAllDatasetDescriptions"));
    assert!(js.contains("fixableDescriptionMapIds"));
    assert!(js.contains("openGridCalibrationForMap"));
    assert!(js.contains("openCuratedInspectorById"));
    assert!(js.contains("apply: true"));
    assert!(js.contains("fetchDatasetReadiness"));
    // The action buttons are delegated from the issue list, so a re-render never stacks handlers.
    assert!(js.contains("datasetReadinessIssues?.addEventListener(\"click\""));
    assert!(js.contains("closest(\"[data-remediate]\")"));
    // Both the per-map action and the batch action share one in-flight request per map.
    assert!(js.contains("state.describeRequests"));

    // Every successful remediation re-audits the scope it was opened for.
    assert!(js.contains("renderDatasetReadinessReport(refusal.readiness)"));

    assert!(css.contains(".readiness-issue-row"));
    assert!(css.contains(".readiness-issue-actions"));
    assert!(css.contains(".readiness-action-btn"));
    assert!(css.contains(".readiness-action-btn.is-busy"));
    assert!(css.contains(".readiness-action-spinner"));
}

/// Bug: `fixAllDatasetDescriptions` skipped every map that already had a regeneration in flight,
/// so a batch could await nothing and still toast "Fixed 0 descriptions". A second click must
/// reuse the in-flight request, and the batch must report what it sent, reused and failed.
#[test]
fn test_batch_description_fix_reuses_in_flight_requests() {
    let js = include_str!("../src/ui/assets/app.js");

    // Single-flight per map: the helper hands back the request already in flight, and forgets it
    // only once it settles.
    assert!(js.contains("function applyDescriptionOnce(mapId)"));
    assert!(js.contains("const inFlight = state.describeRequests.get(mapId);"));
    assert!(js.contains("if (inFlight) return inFlight;"));
    assert!(js.contains(
        "applyDatasetDescription(mapId).finally(() => state.describeRequests.delete(mapId))"
    ));
    assert!(js.contains("state.describeRequests.set(mapId, request);"));

    // Both entry points go through it, so the batch observes requests a per-map click started...
    assert_eq!(js.matches("await applyDescriptionOnce(mapId);").count(), 2);
    assert!(js.contains("const joinedInFlight = state.describeRequests.has(mapId);"));
    // ...instead of skipping the map, which is what produced the zero-request batch, and nothing
    // else tracks in-flight regenerations.
    assert!(!js.contains("datasetRemediating"));
    assert!(!js.contains("await applyDatasetDescription(mapId)"));

    // The batch reports sent/reused/failed counts instead of collapsing them into one number.
    assert!(js.contains("function reportBatchDescriptionOutcome(sent, reused, failed)"));
    assert!(js.contains("reused ${reused} in-flight regeneration"));
    assert!(js.contains("failed.push(`${mapId} (${err.message})`)"));
    assert!(js.contains("reportBatchDescriptionOutcome(sent, reused, failed);"));
}

// ---------------------------------------------------------------------------
// 7. Live SSE progress stream and status observation for snapshot freeze
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_freeze_snapshot_with_sse_progress_stream_and_status() {
    let (app, catalog, blobs) = setup_test_app();
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    // 1. Initial status when idle
    let status_req = Request::builder()
        .method("GET")
        .uri("/api/training/datasets/freeze/status")
        .body(Body::empty())
        .unwrap();
    let status_resp = app.clone().oneshot(status_req).await.unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_bytes = to_bytes(status_resp.into_body(), usize::MAX).await.unwrap();
    let status_json: Value = serde_json::from_slice(&status_bytes).unwrap();
    assert_eq!(status_json["in_progress"], false);

    // Seed map
    let map = ready_multisection_map("sse-dungeon-map-1", Theme::Dungeon);
    let png_bytes = create_test_png(2048, 2048);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &png_bytes).await;

    // 2. Request freeze with Accept: text/event-stream
    let req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets")
        .header(header::ACCEPT, "text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"theme":"dungeon"}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );

    let stream_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stream_str = String::from_utf8(stream_bytes.to_vec()).unwrap();

    // Verify stream contains events
    assert!(stream_str.contains("event: progress") || stream_str.contains("event: complete"));
    assert!(stream_str.contains("starting") || stream_str.contains("auditing") || stream_str.contains("packaging"));
    assert!(stream_str.contains("event: complete"));
    assert!(stream_str.contains("ds_dungeon_"));

    // 3. Status after completion must return in_progress: false
    let post_status_req = Request::builder()
        .method("GET")
        .uri("/api/training/datasets/freeze/status")
        .body(Body::empty())
        .unwrap();
    let post_status_resp = app.clone().oneshot(post_status_req).await.unwrap();
    assert_eq!(post_status_resp.status(), StatusCode::OK);
    let post_status_bytes = to_bytes(post_status_resp.into_body(), usize::MAX).await.unwrap();
    let post_status_json: Value = serde_json::from_slice(&post_status_bytes).unwrap();
    assert_eq!(post_status_json["in_progress"], false);
}

#[tokio::test]
async fn test_freeze_snapshot_sse_refusal_when_unready() {
    let (app, _catalog, _blobs) = setup_test_app();

    // Freeze empty scope with Accept: text/event-stream
    let req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets")
        .header(header::ACCEPT, "text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"theme":"dungeon"}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );

    let stream_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stream_str = String::from_utf8(stream_bytes.to_vec()).unwrap();

    // Verify stream contains terminal error event with DATASET_NOT_READY code
    assert!(stream_str.contains("event: error"));
    assert!(stream_str.contains(r#""code":"DATASET_NOT_READY""#));
    assert!(stream_str.contains(r#""blocking_issues""#));

    // Verify status returns in_progress: false after refusal
    let status_req = Request::builder()
        .method("GET")
        .uri("/api/training/datasets/freeze/status")
        .body(Body::empty())
        .unwrap();
    let status_resp = app.clone().oneshot(status_req).await.unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_bytes = to_bytes(status_resp.into_body(), usize::MAX).await.unwrap();
    let status_json: Value = serde_json::from_slice(&status_bytes).unwrap();
    assert_eq!(status_json["in_progress"], false);
 }

#[tokio::test]
async fn test_freeze_snapshot_concurrency_lock_refuses_concurrent_freeze() {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let cat_trait: Arc<dyn CatalogRepository + Send + Sync> = catalog.clone();
    let blob_trait: Arc<dyn BlobStorage + Send + Sync> = blobs.clone();

    let map = ready_multisection_map("lock-dungeon-map-1", Theme::Dungeon);
    let png_bytes = create_test_png(2048, 2048);
    seed_curated_ports(&cat_trait, &blob_trait, &map, &png_bytes).await;

    let state = AppState::new(catalog.clone(), blobs.clone());
    let router = create_router(state.clone());

    // Acquire lock manually to simulate concurrent freeze running
    let _guard = state.snapshot_freeze_lock.try_lock().unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/training/datasets")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"theme":"dungeon"}"#))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(json["error"].as_str().unwrap().contains("already in progress"));
}
