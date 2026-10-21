use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use curator::adapters::{
    FilesystemStorage, InMemoryBlobStorage, InMemoryCatalogRepository, OpenRouterAdapter,
};
use curator::api::{create_router, AppState, DescribeResponse, SuggestThemesResponse};
use curator::domain::{
    Candidate, CandidateLayer, CuratedLayer, CuratedMap, CuratedMapUpdate, GridConfig, LayerRole,
    RejectionReason, Source, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::storage::StorageRepository;
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

/// Router plus the filesystem repository and the in-memory catalog/blob ports it is paired with.
///
/// Routes that were cut over to the ports (dataset export, storage maintenance) read the ports,
/// so a test targeting them must seed the ports; the filesystem repository keeps serving the
/// routes that still read `state.storage`.
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
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let router = create_router(state);
    (router, storage, catalog, blobs, dir)
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

/// Seeds a curated map into the ports the cut-over dataset routes read: the catalog entry with
/// the implicit `0_ground.png` layer [`FilesystemStorage::save_curated`] synthesizes, plus the
/// Ground layer blob under the key the export resolves.
async fn seed_curated_ports(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
    theme: Theme,
    tags: Vec<String>,
) {
    let png = create_test_png(1024, 1024);
    let mut map = CuratedMap::with_now(id, theme, tags, (1024, 1024));
    map.layers = vec![CuratedLayer::new(
        0,
        CuratedLayer::format_filename(0, LayerRole::Ground, Some(0), "png"),
        LayerRole::Ground,
        Some(0),
        map.dimensions,
    )];
    catalog.save_curated(&map).await.expect("save curated map in the catalog");
    blobs
        .write_blob(&curated_ground_key(&map), &png)
        .await
        .expect("write curated Ground layer blob");
}

fn curate_test_map(
    storage: &FilesystemStorage,
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
    storage.save_curated(&map, &png).expect("save curated test map");
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

fn curate_map_with_grid(
    storage: &FilesystemStorage,
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
    storage.save_curated(&map, &png).expect("save curated test map");
    map
}

/// 512×512 curated map with a 32px grid: exactly 16×16 full cells, i.e. one Map Section.
fn curate_single_section_map(
    storage: &FilesystemStorage,
    id: &str,
    desc: Option<&str>,
) -> CuratedMap {
    curate_map_with_grid(storage, id, (512, 512), 32, desc)
}

/// 2048×2048 curated map with the default 64px grid: 32×32 cells, i.e. four Map Sections.
fn curate_multi_section_map(
    storage: &FilesystemStorage,
    id: &str,
    desc: Option<&str>,
) -> CuratedMap {
    curate_map_with_grid(storage, id, (2048, 2048), 64, desc)
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

fn curate_test_bundle(
    storage: &FilesystemStorage,
    id: &str,
    theme: Theme,
    tags: Vec<String>,
) -> (CuratedMap, Vec<u8>, Vec<u8>) {
    let img0 = create_test_png(300, 300);
    let img1 = create_test_png(300, 300);

    let cand = Candidate::with_now(id, Source::Manual, "0.png", (300, 300));
    storage
        .save_staging_bundle(None, &cand, &[img0.clone(), img1.clone()])
        .expect("save staging bundle");

    storage
        .update_staging_layers(
            id,
            vec![
                CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (300, 300)),
                CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (300, 300)),
            ],
        )
        .expect("update staging layers");

    let curated = CuratedMap::with_now(id, theme, tags, (300, 300));
    storage.curate_staging(&curated).expect("curate staging");
    (curated, img0, img1)
}

#[tokio::test]
async fn test_patch_curated_metadata_success() {
    let (app, storage, ..) = setup_test_app();
    curate_test_map(
        &storage,
        "map_to_patch",
        Theme::Dungeon,
        vec!["old_tag".into()],
        Some("Old dungeon description"),
    );

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
    let listed = storage.list_curated(None).unwrap();
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
    let (app, storage, ..) = setup_test_app();
    let (_map, _img0, _img1) =
        curate_test_bundle(&storage, "bundle_patch_layers", Theme::Dungeon, vec!["tower".into()]);

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
    let (app, storage, ..) = setup_test_app();
    curate_test_bundle(&storage, "tags_only_patch", Theme::Dungeon, vec!["tower".into()]);

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

    let listed = storage.list_curated(None).unwrap();
    let stored = listed.iter().find(|m| m.id == "tags_only_patch").unwrap();
    assert_eq!(stored.tags, vec!["tower", "crypt"]);
}

#[tokio::test]
async fn test_patch_curated_not_found_and_validation() {
    let (app, storage, ..) = setup_test_app();
    curate_test_map(&storage, "map_val_1", Theme::Cave, vec![], None);

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
    let (app, storage, ..) = setup_test_app();
    curate_test_map(&storage, "map_to_delete", Theme::Ruins, vec![], None);

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
    let listed = storage.list_curated(None).unwrap();
    assert!(listed.iter().all(|m| m.id != "map_to_delete"));

    // Verify rejection record
    let rejections = storage.read_rejections().unwrap();
    let rec = rejections.iter().find(|r| r.id == "map_to_delete").expect("rejection record");
    assert_eq!(rec.reason, RejectionReason::GridBurnedIn);
    assert_eq!(rec.notes.as_deref(), Some("Visible square grid"));
}

#[tokio::test]
async fn test_delete_curated_default_empty_body() {
    let (app, storage, ..) = setup_test_app();
    curate_test_map(&storage, "map_del_empty", Theme::Dungeon, vec![], None);

    let req = Request::builder()
        .uri("/api/curated/map_del_empty")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify removal
    let listed = storage.list_curated(None).unwrap();
    assert!(listed.iter().all(|m| m.id != "map_del_empty"));

    // Verify default unspecified rejection
    let rejections = storage.read_rejections().unwrap();
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
    let (app, storage, ..) = setup_test_app();
    curate_test_map(&storage, "bulk_1", Theme::Dungeon, vec![], None);
    curate_test_map(&storage, "bulk_2", Theme::Wilderness, vec![], None);
    curate_test_map(&storage, "bulk_3", Theme::Cave, vec![], None);

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
    let remaining = storage.list_curated(None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "bulk_2");

    // Rejections recorded
    let rejections = storage.read_rejections().unwrap();
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
    let (app, storage, ..) = setup_test_app();
    curate_test_map(&storage, "theme_map_test", Theme::Dungeon, vec!["torch".into()], None);

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
    let (app, storage, ..) = setup_test_app();
    curate_test_map(
        &storage,
        "describe_map_test",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern"),
    );

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
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(
        "Rustic wooden tavern with roaring fireplace.".to_string(),
    )));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    let stored = curate_test_map(
        &storage,
        "describe_map_title",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern description that must not become a title"),
    );

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
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(
        "Merged description.".to_string(),
    )));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    curate_test_map(
        &storage,
        "describe_map_merge",
        Theme::Dungeon,
        vec!["stone".into()],
        Some("Stored description"),
    );

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
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let mock_desc = "Rustic wooden tavern with roaring fireplace and sturdy oak tables.";
    let mock_gen = Arc::new(MockCuratedGen(Ok(mock_desc.to_string())));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(mock_gen))));
    let app = create_router(state);

    curate_test_map(
        &storage,
        "describe_map_success",
        Theme::Tavern,
        vec!["wooden".into(), "fireplace".into()],
        Some("A cozy roadside tavern"),
    );

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
    let (app, _storage, catalog, blobs, _dir) = setup_test_app();
    seed_curated_ports(&catalog, &blobs, "sel_export_1", Theme::Dungeon, vec!["dungeon".into()]).await;
    seed_curated_ports(&catalog, &blobs, "sel_export_2", Theme::Wilderness, vec!["forest".into()]).await;

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
    let (app, storage, ..) = setup_test_app();
    let (_map, img0, img1) =
        curate_test_bundle(&storage, "multi_layer_stream", Theme::Dungeon, vec!["temple".into()]);

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
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(None)));
    let app = create_router(state);

    curate_multi_section_map(&storage, "regrid_unconfigured", None);

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
    let stored = storage
        .list_curated(None)
        .unwrap()
        .into_iter()
        .find(|m| m.id == "regrid_unconfigured")
        .unwrap();
    assert_eq!(stored.description, None);
    assert!(stored.sections.is_empty());
}

#[tokio::test]
async fn test_patch_curated_regrid_strictly_fails_when_vision_errors() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(RecordingDescriptionGen::new(Err(
        DescriptionError::GenerationFailed("upstream 500".into()),
    )));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator))));
    let app = create_router(state);

    curate_multi_section_map(&storage, "regrid_failing_vision", None);

    let req = Request::builder()
        .uri("/api/curated/regrid_failing_vision")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(regrid_payload(64).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

    let stored = storage
        .list_curated(None)
        .unwrap()
        .into_iter()
        .find(|m| m.id == "regrid_failing_vision")
        .unwrap();
    assert_eq!(stored.description, None);
    assert!(stored.sections.is_empty());
}

#[tokio::test]
async fn test_patch_curated_regrid_persists_vision_description_without_description_title() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let (catalog, blobs) = in_memory_ports();
    let generated = "Vision-generated subterranean vault with flooded channels";
    let generator = Arc::new(RecordingDescriptionGen::new(Ok(generated.to_string())));
    let state = AppState::new(storage.clone(), catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    let app = create_router(state);

    curate_single_section_map(&storage, "regrid_success", None);

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

    let manifest_path = dir
        .path()
        .join("curated")
        .join("regrid_success")
        .join("manifest.json");
    let persisted: CuratedMap =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).unwrap()).unwrap();
    assert_eq!(persisted.description.as_deref(), Some(generated));
    assert_eq!(persisted.sections[0].description, generated);
}

/// `AppState` whose curation workflow runs over fresh in-memory catalog and blob doubles.
///
/// `AppState` still carries a `StorageRepository` until #176 deletes the monolithic port, so the
/// slot holds an empty filesystem store; the curation endpoints never read it, which keeps the
/// curation lifecycle exercised here free of disk fixtures.
fn setup_in_memory_curation_app(
    description_service: Arc<DescriptionService>,
) -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
    tempfile::TempDir,
) {
    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let state = AppState::new(storage, catalog.clone(), blobs.clone())
        .with_description_service(description_service);
    (create_router(state), catalog, blobs, dir)
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
    let (app, catalog, blobs, _dir) =
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
async fn test_reject_endpoint_logs_rejection_and_purges_in_memory_staging() {
    let (app, catalog, blobs, _dir) =
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
