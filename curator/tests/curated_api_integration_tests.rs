use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use curator::adapters::{
    FilesystemBlobStorage, FilesystemCatalogRepository, InMemoryBlobStorage,
    InMemoryCatalogRepository, OpenRouterAdapter,
};
use curator::api::{create_router, AppState, DescribeResponse, SuggestThemesResponse};
use curator::domain::{
    Candidate, CandidateLayer, CuratedLayer, CuratedMap, CuratedMapUpdate, GridConfig, LayerRole,
    RejectionReason, Source, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::services::curation::{CurationService, CurationWorkflow};
use async_trait::async_trait;
use curator::services::description::{DescriptionError, DescriptionGenerator, DescriptionService};

/// Fresh in-memory catalog and blob ports: the curation lifecycle tests drive the HTTP surface
/// through `AppState` with zero filesystem fixtures.
fn in_memory_ports() -> (
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

struct MockCuratedGen(Result<String, DescriptionError>);

#[async_trait]
impl DescriptionGenerator for MockCuratedGen {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _filename: Option<&str>,
        _tags: &[String],
        _title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        self.0.clone()
    }
}

fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(width, height);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).expect("write PNG");
    bytes
}

/// Router plus the in-memory catalog/blob ports it serves every route from.
fn setup_test_app() -> (
    axum::Router,
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
) {
    let (catalog, blobs) = in_memory_ports();
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let router = create_router(state);
    (router, catalog, blobs)
}

/// Router over the filesystem-backed ports: the SPA regression tests must exercise the adapter
/// that re-derives every [`BlobKey`] from the file name on `list_prefix`.
fn setup_filesystem_test_app(
    root: &std::path::Path,
) -> (
    axum::Router,
    Arc<dyn CatalogRepository + Send + Sync>,
    Arc<dyn BlobStorage + Send + Sync>,
) {
    let catalog: Arc<dyn CatalogRepository + Send + Sync> =
        Arc::new(FilesystemCatalogRepository::new(root));
    let blobs: Arc<dyn BlobStorage + Send + Sync> = Arc::new(FilesystemBlobStorage::new(root));
    let router = create_router(AppState::new(catalog.clone(), blobs.clone()));
    (router, catalog, blobs)
}

/// PATCHes `/api/curated/{id}` with `payload` and returns the response status.
async fn patch_curated(app: &axum::Router, id: &str, payload: Value) -> StatusCode {
    let req = Request::builder()
        .uri(format!("/api/curated/{id}"))
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

/// GETs `/api/image/curated/{id}/{layer_index}` and returns the status plus body bytes.
async fn get_curated_layer(
    app: &axum::Router,
    id: &str,
    layer_index: usize,
) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .uri(format!("/api/image/curated/{id}/{layer_index}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
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
/// implicit `0_ground.png` layer, plus the Ground layer blob under the key the export resolves.
async fn seed_curated_ports(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    map: &CuratedMap,
    png: &[u8],
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
        .write_blob(&curated_ground_key(&stored), png)
        .await
        .expect("write curated Ground layer blob");
}

async fn curate_test_map(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    theme: Theme,
    tags: Vec<String>,
    desc: Option<&str>,
) -> CuratedMap {
    // 1024x1024 with the default 64px grid spans exactly 16x16 Grid Cells, so the single
    // (0, 0) Map Section resolves to the exact 1024x1024 box for dataset export.
    let png = create_test_png(1024, 1024);
    let mut map = CuratedMap::with_now(id, theme, tags, (1024, 1024));
    if let Some(d) = desc {
        map = map.with_description(d);
    }
    seed_curated_ports(catalog, blobs, &map, &png).await;
    map.layers = vec![CuratedLayer::new(
        0,
        CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), "png"),
        LayerRole::Ground,
        Some(0),
        map.dimensions,
    )];
    map
}

/// Generator that records the `title` and `tags` arguments it receives while returning a fixed result.
struct RecordingDescriptionGen {
    result: Result<String, DescriptionError>,
    seen_titles: std::sync::Mutex<Vec<Option<String>>>,
    seen_tags: std::sync::Mutex<Vec<Vec<String>>>,
}

impl RecordingDescriptionGen {
    fn new(result: Result<String, DescriptionError>) -> Self {
        Self {
            result,
            seen_titles: std::sync::Mutex::new(Vec::new()),
            seen_tags: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl DescriptionGenerator for RecordingDescriptionGen {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _filename: Option<&str>,
        tags: &[String],
        title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        self.seen_titles
            .lock()
            .unwrap()
            .push(title.map(str::to_string));
        self.seen_tags.lock().unwrap().push(tags.to_vec());
        self.result.clone()
    }
}

async fn curate_map_with_grid(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    dimensions: (u32, u32),
    cell_size_px: u32,
    desc: Option<&str>,
) -> CuratedMap {
    let png = create_test_png(dimensions.0, dimensions.1);
    let mut map = CuratedMap::with_now(id, Theme::Dungeon, vec!["dungeon".into()], dimensions)
        .with_grid(GridConfig::new(cell_size_px, 0, 0));
    if let Some(d) = desc {
        map = map.with_description(d);
    }
    seed_curated_ports(catalog, blobs, &map, &png).await;
    map.layers = vec![CuratedLayer::new(
        0,
        CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), "png"),
        LayerRole::Ground,
        Some(0),
        dimensions,
    )];
    map
}

/// 512×512 curated map with a 32px grid: exactly 16×16 full cells, i.e. one Map Section.
async fn curate_single_section_map(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    desc: Option<&str>,
) -> CuratedMap {
    curate_map_with_grid(catalog, blobs, id, (512, 512), 32, desc).await
}

/// 2048×2048 curated map with the default 64px grid: 32×32 cells, i.e. four Map Sections.
async fn curate_multi_section_map(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    desc: Option<&str>,
) -> CuratedMap {
    curate_map_with_grid(catalog, blobs, id, (2048, 2048), 64, desc).await
}

fn regrid_payload(cell_size_px: u32) -> Value {
    json!({
        "grid": {
            "cell_size_px": cell_size_px,
            "offset_x": 0,
            "offset_y": 0
        }
    })
}

async fn curate_test_bundle(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    tags: Vec<String>,
) -> (CuratedMap, Vec<u8>, Vec<u8>) {
    let img0 = create_test_png(300, 300);
    let img1 = create_test_png(300, 300);

    // A 16px grid gives an 18×18 cell lattice, so the bundle is curation-eligible.
    let cand = Candidate::with_now(id, Source::Manual, "0.png", (300, 300))
        .with_grid(GridConfig::new(16, 0, 0));
    catalog.save_candidate(&cand).await.expect("save candidate");
    for (index, bytes) in [&img0, &img1].into_iter().enumerate() {
        blobs
            .write_blob(
                &BlobKey::StagingLayer {
                    id: id.to_string(),
                    index,
                },
                bytes,
            )
            .await
            .expect("write staging layer");
    }

    let mut updated = cand;
    updated.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (300, 300)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (300, 300)),
    ];
    catalog
        .save_candidate(&updated)
        .await
        .expect("update staging layers");

    let curated = CurationService::new(
        catalog.clone(),
        blobs.clone(),
        Arc::new(DescriptionService::new(None)),
    )
    .curate(id, tags, None, Some("Bundle test description".to_string()))
    .await
    .expect("curate staging");
    (curated, img0, img1)
}

#[tokio::test]
async fn test_patch_curated_metadata_success() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(
        &catalog,
        &blobs,
        "map_to_patch",
        Theme::Dungeon,
        vec!["old_tag".into()],
        Some("Old dungeon description"),
    ).await;

    let update_payload = json!({
        "themes": ["tavern", "coastal"],
        "tags": ["alehouse", "dockside"],
        "description": "Lively harbor tavern with warm hearth"
    });

    let req = Request::builder()
        .uri("/api/curated/map_to_patch")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated: CuratedMap = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated.id, "map_to_patch");
    assert!(updated.tags.contains(&"tavern".to_string()));
    assert!(updated.tags.contains(&"coastal".to_string()));
    assert!(updated.tags.contains(&"alehouse".to_string()));
    assert!(updated.tags.contains(&"dockside".to_string()));
    assert_eq!(
        updated.description.as_deref(),
        Some("Lively harbor tavern with warm hearth")
    );

    // Verify storage persistence
    let listed = catalog.list_curated(&Default::default()).await.unwrap();
    let stored = listed.iter().find(|m| m.id == "map_to_patch").unwrap();
    assert!(stored.tags.contains(&"tavern".to_string()));
    assert!(stored.tags.contains(&"coastal".to_string()));
    assert!(stored.tags.contains(&"alehouse".to_string()));
    assert!(stored.tags.contains(&"dockside".to_string()));
    assert_eq!(
        stored.description.as_deref(),
        Some("Lively harbor tavern with warm hearth")
    );
}

#[tokio::test]
async fn test_patch_curated_layers() {
    let (app, catalog, blobs) = setup_test_app();
    let (_map, _img0, _img1) =
        curate_test_bundle(&catalog, &blobs, "bundle_patch_layers", vec!["tower".into()]).await;

    let update = CuratedMapUpdate::default().with_layers(vec![
        CuratedLayer::new(0, "0.png", LayerRole::Basement, Some(-1), (300, 300)),
        CuratedLayer::new(1, "1.png", LayerRole::Ground, Some(0), (300, 300)),
    ]);

    let req = Request::builder()
        .uri("/api/curated/bundle_patch_layers")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&update).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated: CuratedMap = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated.layers.len(), 2);
    assert_eq!(updated.layers[0].role, LayerRole::Basement);
    assert_eq!(updated.layers[0].elevation, Some(-1));
    assert_eq!(updated.layers[1].role, LayerRole::Ground);
    assert_eq!(updated.layers[1].elevation, Some(0));
}

#[tokio::test]
async fn test_patch_curated_tags_only_payload_without_themes() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_bundle(&catalog, &blobs, "tags_only_patch", vec!["tower".into()]).await;

    // The library inspector sends tags, description, and layers — never `themes`.
    let payload = json!({
        "tags": ["tower", "crypt"],
        "description": "A fortified tower with a flooded crypt below",
        "layers": [
            { "index": 0, "filename": "0.png", "role": "ground", "elevation": 0, "dimensions": [300, 300] },
            { "index": 1, "filename": "1.png", "role": "upper_floor", "elevation": 1, "dimensions": [300, 300] }
        ]
    });

    let req = Request::builder()
        .uri("/api/curated/tags_only_patch")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["tags"], json!(["tower", "crypt"]));
    assert_eq!(
        updated["description"],
        "A fortified tower with a flooded crypt below"
    );
    assert!(updated.get("themes").is_none());

    let listed = catalog.list_curated(&Default::default()).await.unwrap();
    let stored = listed.iter().find(|m| m.id == "tags_only_patch").unwrap();
    assert_eq!(stored.tags, vec!["tower", "crypt"]);
}

#[tokio::test]
async fn test_patch_curated_not_found_and_validation() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "map_val_1", Theme::Cave, vec![], None).await;

    // 404 for non-existent map
    let req_404 = Request::builder()
        .uri("/api/curated/non_existent_map")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "description": "New" }).to_string()))
        .unwrap();
    let resp_404 = app.clone().oneshot(req_404).await.unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);

    // Empty legacy themes is a no-op, not a validation failure
    let req_empty_themes = Request::builder()
        .uri("/api/curated/map_val_1")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "themes": [] }).to_string()))
        .unwrap();
    let resp_empty_themes = app.clone().oneshot(req_empty_themes).await.unwrap();
    assert_eq!(resp_empty_themes.status(), StatusCode::OK);
    let empty_body = to_bytes(resp_empty_themes.into_body(), usize::MAX).await.unwrap();
    let unchanged: CuratedMap = serde_json::from_slice(&empty_body).unwrap();
    assert_eq!(unchanged.tags, vec!["cave".to_string()]);

    // 400 for whitespace/empty ID
    let req_empty_id = Request::builder()
        .uri("/api/curated/%20")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "description": "New" }).to_string()))
        .unwrap();
    let resp_empty_id = app.oneshot(req_empty_id).await.unwrap();
    assert_eq!(resp_empty_id.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_delete_curated_with_reason_and_notes() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "map_to_delete", Theme::Ruins, vec![], None).await;

    let req = Request::builder()
        .uri("/api/curated/map_to_delete")
        .method("DELETE")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "reason": "GridBurnedIn",
                "notes": "Visible square grid"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify map was removed from storage
    let listed = catalog.list_curated(&Default::default()).await.unwrap();
    assert!(listed.iter().all(|m| m.id != "map_to_delete"));

    // Verify rejection record
    let rejections = catalog.rejection_records().await.unwrap();
    let rec = rejections.iter().find(|r| r.id == "map_to_delete").expect("rejection record");
    assert_eq!(rec.reason, RejectionReason::GridBurnedIn);
    assert_eq!(rec.notes.as_deref(), Some("Visible square grid"));
}

#[tokio::test]
async fn test_delete_curated_default_empty_body() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "map_del_empty", Theme::Dungeon, vec![], None).await;

    let req = Request::builder()
        .uri("/api/curated/map_del_empty")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify removal
    let listed = catalog.list_curated(&Default::default()).await.unwrap();
    assert!(listed.iter().all(|m| m.id != "map_del_empty"));

    // Verify default unspecified rejection
    let rejections = catalog.rejection_records().await.unwrap();
    let rec = rejections.iter().find(|r| r.id == "map_del_empty").expect("rejection record");
    assert_eq!(rec.reason, RejectionReason::Unspecified);
}

#[tokio::test]
async fn test_delete_curated_not_found() {
    let (app, ..) = setup_test_app();

    let req = Request::builder()
        .uri("/api/curated/non_existent_curated")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_bulk_delete_curated() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "bulk_1", Theme::Dungeon, vec![], None).await;
    curate_test_map(&catalog, &blobs, "bulk_2", Theme::Wilderness, vec![], None).await;
    curate_test_map(&catalog, &blobs, "bulk_3", Theme::Cave, vec![], None).await;

    let req = Request::builder()
        .uri("/api/curated/bulk-delete")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "ids": ["bulk_1", "bulk_3", "non_existent"],
                "reason": "BlurryLowRes"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json_res: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json_res["deleted_count"], 2);

    // bulk_1 and bulk_3 deleted, bulk_2 remains
    let remaining = catalog.list_curated(&Default::default()).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "bulk_2");

    // Rejections recorded
    let rejections = catalog.rejection_records().await.unwrap();
    assert!(rejections.iter().any(|r| r.id == "bulk_1" && r.reason == RejectionReason::BlurryLowRes));
    assert!(rejections.iter().any(|r| r.id == "bulk_3" && r.reason == RejectionReason::BlurryLowRes));

    // Empty ids array returns 0
    let empty_req = Request::builder()
        .uri("/api/curated/bulk-delete")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ids": [] }).to_string()))
        .unwrap();
    let empty_resp = app.oneshot(empty_req).await.unwrap();
    let empty_body = to_bytes(empty_resp.into_body(), usize::MAX).await.unwrap();
    let empty_json: Value = serde_json::from_slice(&empty_body).unwrap();
    assert_eq!(empty_json["deleted_count"], 0);
}

#[tokio::test]
async fn test_curated_suggest_themes_and_caching() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "theme_map_test", Theme::Dungeon, vec!["torch".into()], None).await;

    let req = Request::builder()
        .uri("/api/curated/theme_map_test/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: SuggestThemesResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(res.id, "theme_map_test");

    // Non-existent map returns 404
    let req_404 = Request::builder()
        .uri("/api/curated/no_such_map/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp_404 = app.oneshot(req_404).await.unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_curated_describe_strictly_fails_when_unconfigured() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(
        &catalog,
        &blobs,
        "describe_map_test",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern"),
    ).await;

    // Call describe with empty body while unconfigured -> must return 503 SERVICE_UNAVAILABLE (no silent fallback)
    let req = Request::builder()
        .uri("/api/curated/describe_map_test/describe")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    // Non-existent map returns 404
    let req_404 = Request::builder()
        .uri("/api/curated/no_such_map/describe")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp_404 = app.oneshot(req_404).await.unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_curated_describe_never_sends_description_as_title_and_merges_tags() {
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(
        "Rustic wooden tavern with roaring fireplace.".to_string(),
    )));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    let stored = curate_test_map(
        &catalog,
        &blobs,
        "describe_map_title",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern description that must not become a title"),
    ).await;

    // Empty body: fall back to the stored tags and the suggestion cache, never the description.
    let req = Request::builder()
        .uri("/api/curated/describe_map_title/describe")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        *generator.seen_titles.lock().unwrap(),
        vec![None],
        "curated describe must not forward the stored description as the title"
    );
    assert_eq!(
        *generator.seen_tags.lock().unwrap(),
        vec![stored.tags.clone()],
        "curated describe must merge stored themes and tags"
    );
    assert!(
        !generator.seen_tags.lock().unwrap()[0]
            .iter()
            .any(|tag| tag.contains("roadside")),
        "stored description must never leak into tags"
    );
}

#[tokio::test]
async fn test_curated_describe_merges_requested_themes_and_tags() {
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(
        "Merged description.".to_string(),
    )));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    curate_test_map(
        &catalog,
        &blobs,
        "describe_map_merge",
        Theme::Dungeon,
        vec!["stone".into()],
        Some("Stored description"),
    ).await;

    let req = Request::builder()
        .uri("/api/curated/describe_map_merge/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "themes": ["flooded"],
                "tags": ["stone", "Torchlit", "flooded"]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(*generator.seen_titles.lock().unwrap(), vec![None]);
    assert_eq!(
        *generator.seen_tags.lock().unwrap(),
        vec![vec![
            "flooded".to_string(),
            "stone".to_string(),
            "Torchlit".to_string(),
        ]]
    );
}

#[tokio::test]
async fn test_curated_describe_success_with_service() {
    let (catalog, blobs) = in_memory_ports();
    let mock_desc = "Rustic wooden tavern with roaring fireplace and sturdy oak tables.";
    let mock_gen = Arc::new(MockCuratedGen(Ok(mock_desc.to_string())));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(mock_gen))));
    let app = create_router(state);

    curate_test_map(
        &catalog,
        &blobs,
        "describe_map_success",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern"),
    ).await;

    let req = Request::builder()
        .uri("/api/curated/describe_map_success/describe")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: DescribeResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(res.id, "describe_map_success");
    assert_eq!(res.description, mock_desc);
}

#[tokio::test]
async fn test_export_selected_lora_dataset() {
    let (app, catalog, blobs) = setup_test_app();
    curate_test_map(&catalog, &blobs, "sel_export_1", Theme::Dungeon, vec!["dungeon".into()], None).await;
    curate_test_map(&catalog, &blobs, "sel_export_2", Theme::Wilderness, vec!["forest".into()], None).await;

    let req = Request::builder()
        .uri("/api/dataset/export-selected")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ids": ["sel_export_1"] }).to_string()))
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
    assert!(!body_bytes.is_empty());

    let reader = std::io::Cursor::new(body_bytes);
    let zip = zip::ZipArchive::new(reader).expect("valid zip");
    let file_names: Vec<String> = zip.file_names().map(String::from).collect();

    // Verify sel_export_1 is packaged and sel_export_2 is not
    assert!(file_names.iter().any(|name| name.starts_with("dungeon/sel_export_1") && name.ends_with(".png")));
    assert!(file_names.iter().any(|name| name.starts_with("dungeon/sel_export_1") && name.ends_with(".txt")));
    assert!(!file_names.iter().any(|name| name.contains("sel_export_2")));
}

#[tokio::test]
async fn test_get_curated_layer_image_by_id() {
    let (app, catalog, blobs) = setup_test_app();
    let (_map, img0, img1) =
        curate_test_bundle(&catalog, &blobs, "multi_layer_stream", vec!["temple".into()]).await;

    // Stream layer 0 by direct ID
    let req0 = Request::builder()
        .uri("/api/image/curated/multi_layer_stream/0")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp0 = app.clone().oneshot(req0).await.unwrap();
    assert_eq!(resp0.status(), StatusCode::OK);
    assert_eq!(
        resp0.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    assert_eq!(
        resp0.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=3600"
    );
    let bytes0 = to_bytes(resp0.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes0.as_ref(), img0);

    // Stream layer 1 by direct ID
    let req1 = Request::builder()
        .uri("/api/image/curated/multi_layer_stream/1")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let bytes1 = to_bytes(resp1.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes1.as_ref(), img1);

    // Out of bounds layer returns 404
    let req_oob = Request::builder()
        .uri("/api/image/curated/multi_layer_stream/99")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp_oob = app.clone().oneshot(req_oob).await.unwrap();
    assert_eq!(resp_oob.status(), StatusCode::NOT_FOUND);

    // Non-existent map returns 404
    let req_nf = Request::builder()
        .uri("/api/image/curated/no_such_map_layer/0")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp_nf = app.oneshot(req_nf).await.unwrap();
    assert_eq!(resp_nf.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_curated_regrid_strictly_fails_when_vision_unconfigured() {
    let (catalog, blobs) = in_memory_ports();
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(None)));
    let app = create_router(state);

    curate_multi_section_map(&catalog, &blobs, "regrid_unconfigured", None).await;

    let req = Request::builder()
        .uri("/api/curated/regrid_unconfigured")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(regrid_payload(64).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        err["error"].as_str().unwrap().contains("not configured"),
        "Unexpected error payload: {err}"
    );

    // Nothing was synthesized or persisted: manifest still lacks description/sections.
    let stored = catalog.get_curated("regrid_unconfigured").await.unwrap();
    assert_eq!(stored.description, None);
    assert!(stored.sections.is_empty());
}

#[tokio::test]
async fn test_patch_curated_regrid_strictly_fails_when_vision_errors() {
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Err(
        DescriptionError::GenerationFailed("upstream 500".into()),
    )));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator))));
    let app = create_router(state);

    curate_multi_section_map(&catalog, &blobs, "regrid_failing_vision", None).await;

    let req = Request::builder()
        .uri("/api/curated/regrid_failing_vision")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(regrid_payload(64).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

    let stored = catalog.get_curated("regrid_failing_vision").await.unwrap();
    assert_eq!(stored.description, None);
    assert!(stored.sections.is_empty());
}

#[tokio::test]
async fn test_patch_curated_regrid_persists_vision_description_without_description_title() {
    let (catalog, blobs) = in_memory_ports();
    let generated = "Vision-generated subterranean vault with flooded channels";
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(generated.to_string())));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    curate_single_section_map(&catalog, &blobs, "regrid_success", None).await;

    let req = Request::builder()
        .uri("/api/curated/regrid_success")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(regrid_payload(32).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated: CuratedMap = serde_json::from_slice(&body).unwrap();

    assert_eq!(updated.description.as_deref(), Some(generated));
    assert_eq!(updated.sections.len(), 1);
    assert_eq!(updated.sections[0].col, 0);
    assert_eq!(updated.sections[0].row, 0);
    assert_eq!(updated.sections[0].description, generated);

    // The description must never be passed as the generator's `title` argument.
    assert_eq!(
        *generator.seen_titles.lock().unwrap(),
        vec![None],
        "PATCH regrid must call the generator with no title, not the description"
    );

    let persisted = catalog.get_curated("regrid_success").await.unwrap();
    assert_eq!(persisted.description.as_deref(), Some(generated));
    assert_eq!(persisted.sections[0].description, generated);
}

/// `AppState` whose curation workflow runs over fresh in-memory catalog and blob doubles.
fn setup_in_memory_curation_app(
    description_service: Arc<DescriptionService>,
) -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(description_service);
    (create_router(state), catalog, blobs)
}

/// Seeds the in-memory staging bundle of `candidate` at the canonical port addresses
/// (`staging/{id}/manifest.json` plus `staging/{id}/{index}.png`).
async fn seed_staging_bundle(
    catalog: &InMemoryCatalogRepository,
    blobs: &InMemoryBlobStorage,
    candidate: &Candidate,
    layer_bytes: &[Vec<u8>],
) {
    catalog.save_candidate(candidate).await.unwrap();
    for (index, bytes) in layer_bytes.iter().enumerate() {
        blobs
            .write_blob(
                &BlobKey::StagingLayer {
                    id: candidate.id.clone(),
                    index,
                },
                bytes,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_curate_endpoint_persists_curated_bundle_from_in_memory_ports() {
    let (app, catalog, blobs) =
        setup_in_memory_curation_app(Arc::new(DescriptionService::new(None)));

    let ground_bytes = create_test_png(1024, 1024);
    let upper_bytes = create_test_png(64, 64);
    let mut candidate = Candidate::with_now("memory_curate", Source::Reddit, "0.png", (1024, 1024));
    candidate.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1024, 1024)),
    ];
    seed_staging_bundle(
        &catalog,
        &blobs,
        &candidate,
        &[ground_bytes.clone(), upper_bytes.clone()],
    )
    .await;

    let payload = json!({
        "id": "memory_curate",
        "themes": ["dungeon"],
        "tags": ["crypt"],
        "description": "A flooded stone crypt"
    });
    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["status"], "ok");
    assert_eq!(res["curated"]["id"], "memory_curate");
    assert_eq!(res["curated"]["description"], "A flooded stone crypt");
    assert_eq!(res["curated"]["layers"][0]["filename"], "0_ground.png");
    assert_eq!(
        res["curated"]["layers"][0]["caption"],
        "A flooded stone crypt"
    );
    assert_eq!(res["curated"]["layers"][1]["filename"], "1_upper_1.png");

    // The response is exactly the manifest committed to the catalog.
    let persisted = catalog.get_curated("memory_curate").await.unwrap();
    assert_eq!(serde_json::to_value(&persisted).unwrap(), res["curated"]);

    // Both curated layer blobs landed at their canonical addresses with the staging bytes.
    assert_eq!(
        blobs
            .read_blob(&BlobKey::CuratedLayer {
                id: "memory_curate".into(),
                index: 0,
                role: LayerRole::Ground,
                elevation: Some(0),
            })
            .await
            .unwrap(),
        ground_bytes
    );
    assert_eq!(
        blobs
            .read_blob(&BlobKey::CuratedLayer {
                id: "memory_curate".into(),
                index: 1,
                role: LayerRole::UpperFloor,
                elevation: Some(1),
            })
            .await
            .unwrap(),
        upper_bytes
    );

    // The staging bundle is gone: candidate manifest and every staging blob.
    assert!(catalog.get_candidate("memory_curate").await.is_err());
    assert!(blobs
        .list_prefix(&BlobKey::StagingDir {
            id: Some("memory_curate".into())
        })
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_patch_curated_layers_reindexing_reassigns_bytes_and_cleans_stale_keys() {
    let (app, catalog, blobs) = setup_test_app();
    let (_map, img0, img1) =
        curate_test_bundle(&catalog, &blobs, "bundle_reindex", vec!["tower".into()]).await;

    // The payload reassigns the existing files across indices: index 0 (Ground) names layer 1's
    // file and index 1 (Roof) names layer 0's file.
    let payload = json!({
        "description": "Watchtower with fortified observation deck",
        "layers": [
            { "index": 0, "filename": "1_upper_1.png", "role": "ground", "elevation": 0, "dimensions": [300, 300] },
            { "index": 1, "filename": "0_ground.png", "role": "roof", "elevation": null, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(
        patch_curated(&app, "bundle_reindex", payload).await,
        StatusCode::OK
    );

    let stored = catalog.get_curated("bundle_reindex").await.unwrap();
    assert_eq!(stored.layers.len(), 2);
    assert_eq!(stored.layers[0].filename, "0_ground.png");
    assert_eq!(stored.layers[0].role, LayerRole::Ground);
    assert_eq!(stored.layers[1].filename, "1_roof.png");
    assert_eq!(stored.layers[1].role, LayerRole::Roof);

    // The bytes follow the payload filenames: Ground now serves the former upper-floor image.
    assert_eq!(get_curated_layer(&app, "bundle_reindex", 0).await, (StatusCode::OK, img1.clone()));
    assert_eq!(get_curated_layer(&app, "bundle_reindex", 1).await, (StatusCode::OK, img0.clone()));

    // Stale-key cleanup: the superseded `1_upper_1.png` is gone, only the canonical names remain.
    let paths: Vec<String> = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("bundle_reindex".into()),
        })
        .await
        .unwrap()
        .iter()
        .map(BlobKey::to_path)
        .collect();
    assert_eq!(
        paths,
        vec![
            "curated/bundle_reindex/0_ground.png".to_string(),
            "curated/bundle_reindex/1_roof.png".to_string(),
        ]
    );

    // Unassigned and all-excluded payloads are rejected without touching the stored layers.
    let unassigned = json!({
        "layers": [
            { "index": 0, "filename": "0_ground.png", "role": "unassigned", "elevation": null, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(
        patch_curated(&app, "bundle_reindex", unassigned).await,
        StatusCode::BAD_REQUEST
    );
    let all_excluded = json!({
        "layers": [
            { "index": 0, "filename": "0_ground.png", "role": "exclude", "elevation": null, "dimensions": [300, 300] },
            { "index": 1, "filename": "1_roof.png", "role": "exclude", "elevation": null, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(
        patch_curated(&app, "bundle_reindex", all_excluded).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(get_curated_layer(&app, "bundle_reindex", 0).await, (StatusCode::OK, img1));
    assert_eq!(get_curated_layer(&app, "bundle_reindex", 1).await, (StatusCode::OK, img0));
}

#[tokio::test]
async fn test_patch_curated_preserves_omitted_source_url_and_caption() {
    let (app, catalog, blobs) = setup_test_app();
    let png = create_test_png(300, 300);

    let candidate = Candidate::with_now("map_prov", Source::Reddit, "0.png", (300, 300))
        .with_source_url("https://reddit.example/post/abc")
        .with_grid(GridConfig::new(16, 0, 0));
    catalog.save_candidate(&candidate).await.unwrap();
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "map_prov".into(),
                index: 0,
            },
            &png,
        )
        .await
        .unwrap();
    let curated = CurationService::new(
        catalog.clone(),
        blobs.clone(),
        Arc::new(DescriptionService::new(None)),
    )
    .curate(
        "map_prov",
        vec!["crypt".into()],
        None,
        Some("Flooded crypt with shattered sarcophagi.".into()),
    )
    .await
    .unwrap();
    assert_eq!(
        curated.layers[0].source_url.as_deref(),
        Some("https://reddit.example/post/abc")
    );

    // A layer payload omitting source_url and caption carries both over from the stored layer.
    let omitted = json!({
        "layers": [
            { "index": 0, "filename": "0_ground.png", "role": "ground", "elevation": 0, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(patch_curated(&app, "map_prov", omitted).await, StatusCode::OK);
    let carried = catalog.get_curated("map_prov").await.unwrap();
    assert_eq!(
        carried.layers[0].source_url.as_deref(),
        Some("https://reddit.example/post/abc")
    );
    assert_eq!(
        carried.layers[0].caption.as_deref(),
        Some("Flooded crypt with shattered sarcophagi.")
    );

    // An explicit payload value overrides the stored provenance.
    let explicit = json!({
        "layers": [
            {
                "index": 0,
                "filename": "0_ground.png",
                "role": "ground",
                "elevation": 0,
                "dimensions": [300, 300],
                "source_url": "https://example.com/new-source",
                "caption": "Explicit caption."
            }
        ]
    });
    assert_eq!(patch_curated(&app, "map_prov", explicit).await, StatusCode::OK);
    let overridden = catalog.get_curated("map_prov").await.unwrap();
    assert_eq!(
        overridden.layers[0].source_url.as_deref(),
        Some("https://example.com/new-source")
    );
    assert_eq!(overridden.layers[0].caption.as_deref(), Some("Explicit caption."));
}

/// Regression for the shipped SPA flow on the filesystem adapters: set an upper-floor layer's
/// role to `roof` (the role select keeps its elevation), save, then save again. The second PATCH
/// used to delete `curated/{id}/1_roof.png` because `list_prefix` re-derives that key with
/// `elevation: None` while the writer held `Some(1)`.
#[tokio::test]
async fn test_patch_curated_roof_role_saved_twice_keeps_layer_blob_served() {
    let dir = tempdir().unwrap();
    let (app, catalog, blobs) = setup_filesystem_test_app(dir.path());
    let (_map, img0, img1) =
        curate_test_bundle(&catalog, &blobs, "roof_repatch", vec!["tower".into()]).await;

    // First save: the upper-floor layer becomes `roof`, keeping elevation 1.
    let first = json!({
        "layers": [
            { "index": 0, "filename": "0_ground.png", "role": "ground", "elevation": 0, "dimensions": [300, 300] },
            { "index": 1, "filename": "1_upper_1.png", "role": "roof", "elevation": 1, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(patch_curated(&app, "roof_repatch", first).await, StatusCode::OK);
    let saved = catalog.get_curated("roof_repatch").await.unwrap();
    assert_eq!(saved.layers[1].filename, "1_roof.png");
    assert_eq!(saved.layers[1].role, LayerRole::Roof);
    assert_eq!(saved.layers[1].elevation, Some(1));
    assert_eq!(
        get_curated_layer(&app, "roof_repatch", 1).await,
        (StatusCode::OK, img1.clone())
    );

    // Second save of the same map, as the inspector would re-send it after reloading.
    let second = json!({
        "layers": [
            { "index": 0, "filename": "0_ground.png", "role": "ground", "elevation": 0, "dimensions": [300, 300] },
            { "index": 1, "filename": "1_roof.png", "role": "roof", "elevation": 1, "dimensions": [300, 300] }
        ]
    });
    assert_eq!(patch_curated(&app, "roof_repatch", second).await, StatusCode::OK);

    // The freshly written blob survived the stale-key cleanup and is still served.
    let roof_key = BlobKey::CuratedLayer {
        id: "roof_repatch".into(),
        index: 1,
        role: LayerRole::Roof,
        elevation: Some(1),
    };
    assert!(blobs.exists_blob(&roof_key).await.unwrap());
    assert_eq!(
        get_curated_layer(&app, "roof_repatch", 1).await,
        (StatusCode::OK, img1)
    );
    assert_eq!(
        get_curated_layer(&app, "roof_repatch", 0).await,
        (StatusCode::OK, img0)
    );
}

#[tokio::test]
async fn test_reject_endpoint_logs_rejection_and_purges_in_memory_staging() {
    let (app, catalog, blobs) =
        setup_in_memory_curation_app(Arc::new(DescriptionService::new(None)));

    let candidate = Candidate::with_now("memory_reject", Source::Reddit, "0.png", (1024, 1024));
    seed_staging_bundle(&catalog, &blobs, &candidate, &[create_test_png(1024, 1024)]).await;

    let req = Request::builder()
        .uri("/api/reject")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "memory_reject",
                "reason": "BlurryLowRes",
                "notes": "Too blurry"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["status"], "ok");
    assert_eq!(res["id"], "memory_reject");

    // Rejection is recorded in the catalog and the staging bundle is purged.
    assert!(catalog.is_rejected("memory_reject").await.unwrap());
    assert!(catalog.get_candidate("memory_reject").await.is_err());
    assert!(blobs
        .list_prefix(&BlobKey::StagingDir {
            id: Some("memory_reject".into())
        })
        .await
        .unwrap()
        .is_empty());
}
