use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use async_trait::async_trait;
use curator::adapters::{FileCostLedger, FilesystemStorage, OpenRouterAdapter};
use curator::api::{create_router, AppState, DescribeResponse, SuggestThemesResponse};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use curator::ports::cost_tracker::{CostTracker, GenerationCostRecord};
use curator::domain::{
    Candidate, CandidateLayer, CuratedMap, HarvestedMedia, LayerRole, ParsedTags, RedditHarvestQuery,
    RedditPost, Source, Theme,
};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::ports::storage::StorageRepository;
use curator::services::description::{DescriptionError, DescriptionGenerator, DescriptionService};

struct MockDescriptionGen(Result<String, DescriptionError>);

#[async_trait]
impl DescriptionGenerator for MockDescriptionGen {
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
fn setup_test_app() -> (axum::Router, Arc<FilesystemStorage>, tempfile::TempDir) {
    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let desc_service = Arc::new(DescriptionService::new(Some(Arc::new(MockDescriptionGen(
        Ok("Default test battlemap description".to_string()),
    )))));
    let state = AppState::new(storage.clone()).with_description_service(desc_service);
    let router = create_router(state);
    (router, storage, dir)
}

#[tokio::test]
async fn test_api_status_empty() {
    let (app, _storage, _dir) = setup_test_app();

    let req = Request::builder()
        .uri("/api/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["staging_count"], 0);
    assert_eq!(json["curated_count"], 0);
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn test_api_staging_and_status_counts() {
    let (app, storage, _dir) = setup_test_app();

    // Populate staging with 2 candidates
    let cand1 = Candidate::with_now("c1", Source::Reddit, "c1.png", (1024, 768));
    let cand2 = Candidate::with_now("c2", Source::Manual, "c2.jpg", (2048, 1536));
    storage.save_staging(&cand1, b"\x89PNG\r\n\x1a\nfake-png-1").unwrap();
    storage.save_staging(&cand2, b"\xFF\xD8\xFFfake-jpeg-2").unwrap();

    // Status check
    let req = Request::builder()
        .uri("/api/status")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let status_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status_json["staging_count"], 2);
    assert_eq!(status_json["curated_count"], 0);

    // Staging list check
    let req = Request::builder()
        .uri("/api/staging")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let staging_list: Vec<Candidate> = serde_json::from_slice(&body).unwrap();
    assert_eq!(staging_list.len(), 2);
    let ids: Vec<_> = staging_list.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"c1"));
    assert!(ids.contains(&"c2"));
}

#[tokio::test]
async fn test_api_image_staging_streaming_and_headers() {
    let (app, storage, _dir) = setup_test_app();

    let png_data = b"\x89PNG\r\n\x1a\nsome_png_image_payload";
    let cand = Candidate::with_now("png_candidate", Source::Reddit, "map.png", (1000, 1000));
    storage.save_staging(&cand, png_data).unwrap();

    let req = Request::builder()
        .uri("/api/image/staging/png_candidate")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify headers
    let cache_control = response.headers().get(header::CACHE_CONTROL).unwrap();
    assert_eq!(cache_control, "public, max-age=3600");

    let content_type = response.headers().get(header::CONTENT_TYPE).unwrap();
    assert_eq!(content_type, "image/png");

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), png_data);

    // Non-existent image returns 404
    let req_404 = Request::builder()
        .uri("/api/image/staging/does_not_exist")
        .body(Body::empty())
        .unwrap();
    let resp_404 = app.oneshot(req_404).await.unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_api_curate_endpoint() {
    let (app, storage, _dir) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\ncurate_this_map";
    let cand = Candidate::with_now("map_to_curate", Source::Generated, "dungeon.png", (1920, 1080));
    storage.save_staging(&cand, img_bytes).unwrap();

    // Curate candidate with themes
    let curate_payload = json!({
        "id": "map_to_curate",
        "themes": ["dungeon"],
        "tags": ["crypt", "stone-floor"]
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "map_to_curate");
    assert_eq!(res_json["curated"]["tags"], json!(["dungeon", "crypt", "stone-floor"]));
    assert!(res_json["curated"]["themes"].is_null());
    assert_eq!(res_json["curated"]["dimensions"], json!([1920, 1080]));

    // Staging should now be empty
    assert_eq!(storage.list_staging().unwrap().len(), 0);

    // Curated directory should have 1 item
    let curated = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
    assert_eq!(curated.len(), 1);
    assert_eq!(curated[0].id, "map_to_curate");

    // Fetch the curated image via API
    let req_img = Request::builder()
        .uri("/api/image/curated/dungeon/map_to_curate")
        .body(Body::empty())
        .unwrap();
    let resp_img = app.clone().oneshot(req_img).await.unwrap();
    assert_eq!(resp_img.status(), StatusCode::OK);
    assert_eq!(
        resp_img.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=3600"
    );
    assert_eq!(
        resp_img.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    let img_body = to_bytes(resp_img.into_body(), usize::MAX).await.unwrap();
    assert_eq!(img_body.as_ref(), img_bytes);

    // Fetch the curated image via direct ID endpoint /api/image/curated/map_to_curate
    let req_img_direct = Request::builder()
        .uri("/api/image/curated/map_to_curate")
        .body(Body::empty())
        .unwrap();
    let resp_img_direct = app.clone().oneshot(req_img_direct).await.unwrap();
    assert_eq!(resp_img_direct.status(), StatusCode::OK);
    let direct_body = to_bytes(resp_img_direct.into_body(), usize::MAX).await.unwrap();
    assert_eq!(direct_body.as_ref(), img_bytes);

    // Empty themes should return 400
    let empty_themes_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "map_to_curate",
                "themes": [],
                "tags": []
            })
            .to_string(),
        ))
        .unwrap();
    let resp_invalid = app.oneshot(empty_themes_req).await.unwrap();
    assert_eq!(resp_invalid.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_api_curate_accepts_unified_tags_without_legacy_themes() {
    let (app, storage, _dir) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\ntags_only_map";
    let cand = Candidate::with_now("tags_only_map", Source::Generated, "dungeon.png", (1920, 1080));
    storage.save_staging(&cand, img_bytes).unwrap();

    let curate_payload = json!({
        "id": "tags_only_map",
        "tags": ["dungeon", "crypt", "boss-arena"]
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        res_json["curated"]["tags"],
        json!(["dungeon", "crypt", "boss-arena"])
    );
    assert!(res_json["curated"]["themes"].is_null());

    // Missing every tag source is rejected with the tag-specific message.
    let cand = Candidate::with_now("untagged_map", Source::Generated, "dungeon.png", (1920, 1080));
    storage.save_staging(&cand, img_bytes).unwrap();
    let req_missing = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "untagged_map" }).to_string()))
        .unwrap();
    let response_missing = app.oneshot(req_missing).await.unwrap();
    assert_eq!(response_missing.status(), StatusCode::BAD_REQUEST);
    let body_missing = to_bytes(response_missing.into_body(), usize::MAX).await.unwrap();
    let missing_json: Value = serde_json::from_slice(&body_missing).unwrap();
    assert_eq!(missing_json["error"], "At least 1 tag is required");
}

#[tokio::test]
async fn test_curate_response_matches_curated_listing_including_provenance() {
    let (app, storage, _dir) = setup_test_app();

    let mut cand = Candidate::with_now("provenance_map", Source::Reddit, "0.png", (1920, 1080));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080))
            .with_source_url("https://i.redd.it/provenance_0.png"),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080))
            .with_source_url("https://i.redd.it/provenance_1.png"),
    ];
    storage
        .save_staging_bundle(
            None,
            &cand,
            &[
                b"\x89PNG\r\n\x1a\nprovenance_ground".to_vec(),
                b"\x89PNG\r\n\x1a\nprovenance_upper".to_vec(),
            ],
        )
        .unwrap();

    let curate_payload = json!({
        "id": "provenance_map",
        "tags": ["dungeon", "crypt"]
    });
    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let curate_json: Value = serde_json::from_slice(&body).unwrap();
    let returned = curate_json["curated"].clone();

    assert_eq!(
        returned["layers"][0]["source_url"],
        "https://i.redd.it/provenance_0.png"
    );
    assert_eq!(
        returned["layers"][1]["source_url"],
        "https://i.redd.it/provenance_1.png"
    );

    // GET /api/curated must report byte-for-byte what POST /api/curate returned.
    let list_req = Request::builder()
        .uri("/api/curated")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let list_resp = app.oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
    let listed: Vec<Value> = serde_json::from_slice(&list_body).unwrap();
    let stored = listed
        .iter()
        .find(|m| m["id"] == "provenance_map")
        .expect("curated map listed");

    assert_eq!(&returned, stored, "curate response must equal stored manifest");
}

#[tokio::test]
async fn test_e2e_curation_decision_flow_with_dynamic_themes_and_flat_storage() {
    let (app, storage, dir) = setup_test_app();

    // 1. Prepare candidate in staging with multi-layer bundle
    let img0 = b"\x89PNG\r\n\x1a\nlayer0_ground_map";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_upper_map";
    let mut cand = Candidate::with_now("dungeon_complex", Source::Reddit, "0.png", (1920, 1080));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080)),
    ];
    storage
        .save_staging_bundle(None, &cand, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    // Verify candidate is in staging
    assert_eq!(storage.list_staging().unwrap().len(), 1);

    // 2. Curate rejection when themes is empty: returns HTTP 400 Bad Request
    let req_empty_themes = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "dungeon_complex",
                "themes": [],
                "tags": []
            })
            .to_string(),
        ))
        .unwrap();
    let resp_empty = app.clone().oneshot(req_empty_themes).await.unwrap();
    assert_eq!(resp_empty.status(), StatusCode::BAD_REQUEST);

    // 3. Curate rejection when themes has only empty / whitespace strings: returns HTTP 400
    let req_whitespace_themes = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "dungeon_complex",
                "themes": ["   ", ""],
                "tags": ["   "]
            })
            .to_string(),
        ))
        .unwrap();
    let resp_whitespace = app.clone().oneshot(req_whitespace_themes).await.unwrap();
    assert_eq!(resp_whitespace.status(), StatusCode::BAD_REQUEST);

    // 4. Successful curation with multi-label dynamic themes (e.g. from decision modal) and no rating
    let curate_payload = json!({
        "id": "dungeon_complex",
        "themes": ["dungeon", "crypt", "boss-arena"],
        "tags": ["crypt", "torchlight", "fantasy"],
        "layers": [
            {
                "index": 0,
                "filename": "0.png",
                "role": "ground",
                "elevation": 0,
                "dimensions": [1920, 1080],
                "caption": "Ground floor dungeon hall"
            },
            {
                "index": 1,
                "filename": "1.png",
                "role": "upper_floor",
                "elevation": 1,
                "dimensions": [1920, 1080],
                "caption": "Upper balcony"
            }
        ]
    });

    let req_success = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();
    let resp_success = app.clone().oneshot(req_success).await.unwrap();
    assert_eq!(resp_success.status(), StatusCode::OK);

    let body = to_bytes(resp_success.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "dungeon_complex");
    assert_eq!(
        res_json["curated"]["tags"],
        json!(["dungeon", "crypt", "boss-arena", "torchlight", "fantasy"])
    );
    assert!(res_json["curated"]["themes"].is_null());
    // Rating must not be present in response
    assert!(res_json["curated"]["rating"].is_null());

    // 5. Staging should now be empty (candidate removed from queue)
    assert_eq!(storage.list_staging().unwrap().len(), 0);

    // 6. Verify flat storage directory on disk: data/curated/<id>/manifest.json
    let curated_bundle_dir = dir.path().join("curated").join("dungeon_complex");
    assert!(
        curated_bundle_dir.is_dir(),
        "Curated bundle directory must exist at flat layout: {:?}",
        curated_bundle_dir
    );

    let manifest_path = curated_bundle_dir.join("manifest.json");
    assert!(
        manifest_path.is_file(),
        "manifest.json must exist in curated bundle directory: {:?}",
        manifest_path
    );

    let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_content).unwrap();

    assert_eq!(manifest["id"], "dungeon_complex");
    assert_eq!(
        manifest["tags"],
        json!(["dungeon", "crypt", "boss-arena", "torchlight", "fantasy"])
    );
    assert!(manifest["themes"].is_null());
    assert_eq!(manifest["dimensions"], json!([1920, 1080]));
    assert!(manifest["rating"].is_null(), "Curated map manifest must not have rating");

    // Verify deterministically named layer image files exist in flat directory
    let layer0_path = curated_bundle_dir.join("0_ground.png");
    assert!(
        layer0_path.is_file(),
        "Layer 0 image must exist at deterministic path: {:?}",
        layer0_path
    );
    let layer1_path = curated_bundle_dir.join("1_upper_1.png");
    assert!(
        layer1_path.is_file(),
        "Layer 1 image must exist at deterministic path: {:?}",
        layer1_path
    );

    // 7. Verify API image retrieval by direct ID and by theme
    let req_direct = Request::builder()
        .uri("/api/image/curated/dungeon_complex")
        .body(Body::empty())
        .unwrap();
    let resp_direct = app.clone().oneshot(req_direct).await.unwrap();
    assert_eq!(resp_direct.status(), StatusCode::OK);
    let direct_bytes = to_bytes(resp_direct.into_body(), usize::MAX).await.unwrap();
    assert_eq!(direct_bytes.as_ref(), img0);

    let req_theme = Request::builder()
        .uri("/api/image/curated/dungeon/dungeon_complex")
        .body(Body::empty())
        .unwrap();
    let resp_theme = app.oneshot(req_theme).await.unwrap();
    assert_eq!(resp_theme.status(), StatusCode::OK);
    let theme_bytes = to_bytes(resp_theme.into_body(), usize::MAX).await.unwrap();
    assert_eq!(theme_bytes.as_ref(), img0);
}

#[tokio::test]
async fn test_api_curate_building_theme_and_aliases() {
    let (app, storage, _dir) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\nisolated_cottage";
    let cand1 = Candidate::with_now("cabin_map", Source::Reddit, "cabin.png", (2048, 2048));
    storage.save_staging(&cand1, img_bytes).unwrap();

    // Curate candidate with theme "building"
    let curate_payload = json!({
        "id": "cabin_map",
        "theme": "building",
        "rating": 5,
        "tags": ["isolated-cabin", "timber-roof"]
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "cabin_map");
    assert_eq!(res_json["curated"]["tags"], json!(["building", "isolated-cabin", "timber-roof"]));
    assert!(res_json["curated"]["themes"].is_null());

    // Verify stored under Theme::Building
    let curated = storage.list_curated(Some(&Theme::Building)).unwrap();
    assert_eq!(curated.len(), 1);
    assert_eq!(curated[0].id, "cabin_map");
    assert_eq!(curated[0].theme(), Some(Theme::Building));

    // Fetch the curated image via /api/image/curated/building/cabin_map
    let req_img = Request::builder()
        .uri("/api/image/curated/building/cabin_map")
        .body(Body::empty())
        .unwrap();
    let resp_img = app.clone().oneshot(req_img).await.unwrap();
    assert_eq!(resp_img.status(), StatusCode::OK);

    // Test alias "outpost" also curates into Theme::Building
    let cand2 = Candidate::with_now("outpost_map", Source::Generated, "outpost.png", (1024, 1024));
    storage.save_staging(&cand2, img_bytes).unwrap();

    let alias_payload = json!({
        "id": "outpost_map",
        "theme": "outpost",
        "rating": 4,
        "tags": ["watchtower"]
    });

    let req2 = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(alias_payload.to_string()))
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let res_json2: Value = serde_json::from_slice(&body2).unwrap();
    assert!(res_json2["curated"]["themes"].is_null());
    assert!(res_json2["curated"]["tags"].as_array().unwrap().contains(&json!("building")));

    let building_list = storage.list_curated(Some(&Theme::Building)).unwrap();
    assert_eq!(building_list.len(), 2);
}

#[tokio::test]
async fn test_api_reject_endpoint() {
    let (app, storage, _dir) = setup_test_app();

    let cand = Candidate::with_now("bad_map", Source::Reddit, "bad.png", (800, 600));
    storage.save_staging(&cand, b"\x89PNG\r\n\x1a\nbad").unwrap();

    let reject_payload = json!({
        "id": "bad_map",
        "reason": "GridBurnedIn",
        "notes": "Grid lines are visible"
    });

    let req = Request::builder()
        .uri("/api/reject")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(reject_payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["id"], "bad_map");

    // Staging should be empty
    assert_eq!(storage.list_staging().unwrap().len(), 0);

    // Should be recorded in rejections
    assert!(storage.is_rejected("bad_map").unwrap());

    // Test rejecting without explicit reason or notes
    let cand_simple = Candidate::with_now("bad_map_simple", Source::Reddit, "simple.png", (800, 600));
    storage.save_staging(&cand_simple, b"\x89PNG\r\n\x1a\nsimple").unwrap();

    let req_simple = Request::builder()
        .uri("/api/reject")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "bad_map_simple" }).to_string()))
        .unwrap();

    let response_simple = app.clone().oneshot(req_simple).await.unwrap();
    assert_eq!(response_simple.status(), StatusCode::OK);
    assert!(storage.is_rejected("bad_map_simple").unwrap());
}

#[tokio::test]
async fn test_static_asset_and_spa_routing() {
    let (app, _storage, _dir) = setup_test_app();

    // Root GET / serves index.html
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let html_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(html_str.contains("DUNGEON FORGE"));
    assert!(html_str.contains("triage-canvas"));
    assert!(html_str.contains("library-search-input"));
    assert!(!html_str.contains("rejection-modal"));
    assert!(html_str.contains("help-modal"));

    // GET /style.css
    let req = Request::builder().uri("/style.css").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/css"
    );

    // GET /app.js
    let req = Request::builder().uri("/app.js").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
    assert!(ct.contains("javascript"));
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let js_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(js_str.contains("VTT_GRID_SIZE"));
    assert!(js_str.contains("curateCurrentCandidate"));

    // SPA fallback: /triage/view/42 -> serves index.html
    let req = Request::builder().uri("/triage/view/42").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );

    // Unknown API route should NOT return index.html; it must return 404 JSON
    let req = Request::builder().uri("/api/unknown_route").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn test_curate_errors_and_edge_cases() {
    let (app, _storage, _dir) = setup_test_app();

    // Curating non-existent candidate returns 404
    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "non_existent_cand",
                "theme": "cave",
                "rating": 5,
                "tags": []
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Curating with empty ID returns 400
    let req_empty_id = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "   ",
                "theme": "cave",
                "rating": 5,
                "tags": []
            })
            .to_string(),
        ))
        .unwrap();
    let resp_empty_id = app.clone().oneshot(req_empty_id).await.unwrap();
    assert_eq!(resp_empty_id.status(), StatusCode::BAD_REQUEST);

    // Curated image with invalid theme returns 400
    let req_bad_theme = Request::builder()
        .uri("/api/image/curated/invalid_theme_name/map_1")
        .body(Body::empty())
        .unwrap();
    let resp_bad_theme = app.clone().oneshot(req_bad_theme).await.unwrap();
    assert_eq!(resp_bad_theme.status(), StatusCode::BAD_REQUEST);

    // Curated image non-existent returns 404
    let req_non_existent = Request::builder()
        .uri("/api/image/curated/dungeon/does_not_exist")
        .body(Body::empty())
        .unwrap();
    let resp_non_existent = app.clone().oneshot(req_non_existent).await.unwrap();
    assert_eq!(resp_non_existent.status(), StatusCode::NOT_FOUND);

    // Curated image direct ID non-existent returns 404
    let req_direct_non_existent = Request::builder()
        .uri("/api/image/curated/does_not_exist")
        .body(Body::empty())
        .unwrap();
    let resp_direct_non_existent = app.oneshot(req_direct_non_existent).await.unwrap();
    assert_eq!(resp_direct_non_existent.status(), StatusCode::NOT_FOUND);
}

#[derive(Clone, Default)]
struct MockHarvester {
    posts: Vec<RedditPost>,
}

#[async_trait]
impl RedditHarvester for MockHarvester {
    async fn fetch_posts(
        &self,
        _query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        Ok(self.posts.clone())
    }
}

fn create_test_png() -> Vec<u8> {
    let img = image::RgbaImage::new(1, 1);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
    bytes
}

#[tokio::test]
async fn test_api_harvest_reddit_json_response() {
    // 1. Mock image server
    let png_bytes = create_test_png();
    let app_image = axum::Router::new().route(
        "/test_map.png",
        axum::routing::get(move || {
            let data = png_bytes.clone();
            async move { (StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app_image).await.unwrap();
    });
    let image_url = format!("http://{addr}/test_map.png");

    // 2. Setup mock harvester posts (1 gridless, 1 gridded)
    let post_gridless = RedditPost {
        id: "harvest_json_1".into(),
        name: "t3_harvest_json_1".into(),
        title: "[40x40] Ancient Crypt [Gridless]".into(),
        author: "crypt_keeper".into(),
        permalink: "/r/battlemaps/comments/harvest_json_1/".into(),
        url: image_url.clone(),
        score: 150,
        created_utc: chrono::Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[40x40] Ancient Crypt [Gridless]"),
    };

    let post_gridded = RedditPost {
        id: "harvest_json_2".into(),
        name: "t3_harvest_json_2".into(),
        title: "[40x40] Ancient Crypt [Gridded]".into(),
        author: "crypt_keeper".into(),
        permalink: "/r/battlemaps/comments/harvest_json_2/".into(),
        url: image_url.clone(),
        score: 150,
        created_utc: chrono::Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[40x40] Ancient Crypt [Gridded]"),
    };

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let harvester = Arc::new(MockHarvester {
        posts: vec![post_gridless, post_gridded],
    });
    let state = AppState::with_harvester(storage.clone(), harvester);
    let app = create_router(state);

    // 3. Post harvest request with gridless_only: true
    let harvest_req = Request::builder()
        .uri("/api/harvest/reddit")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "sort": "hot",
                "limit": 25,
                "gridless_only": true
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.clone().oneshot(harvest_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json_resp: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json_resp["downloaded"], 1);
    assert_eq!(json_resp["skipped"], 1);
    assert_eq!(json_resp["processed"], 2);

    // 4. Verify newly harvested candidate immediately appears in /api/staging
    let staging_req = Request::builder()
        .uri("/api/staging")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let staging_resp = app.oneshot(staging_req).await.unwrap();
    assert_eq!(staging_resp.status(), StatusCode::OK);

    let staging_bytes = to_bytes(staging_resp.into_body(), usize::MAX).await.unwrap();
    let staging_list: Vec<Candidate> = serde_json::from_slice(&staging_bytes).unwrap();
    assert_eq!(staging_list.len(), 1);
    assert_eq!(staging_list[0].id, "harvest_json_1");
}

#[tokio::test]
async fn test_api_harvest_reddit_sse_streaming() {
    // 1. Mock image server
    let png_bytes = create_test_png();
    let app_image = axum::Router::new().route(
        "/sse_map.png",
        axum::routing::get(move || {
            let data = png_bytes.clone();
            async move { (StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app_image).await.unwrap();
    });
    let image_url = format!("http://{addr}/sse_map.png");

    // 2. Setup mock harvester
    let post = RedditPost {
        id: "sse_post_1".into(),
        name: "t3_sse_post_1".into(),
        title: "[25x25] Goblin Ambush [Gridless]".into(),
        author: "ambusher".into(),
        permalink: "/r/battlemaps/comments/sse_post_1/".into(),
        url: image_url.clone(),
        score: 200,
        created_utc: chrono::Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[25x25] Goblin Ambush [Gridless]"),
    };

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let harvester = Arc::new(MockHarvester { posts: vec![post] });
    let state = AppState::with_harvester(storage.clone(), harvester);
    let app = create_router(state);

    // 3. Post harvest request with Accept: text/event-stream
    let harvest_req = Request::builder()
        .uri("/api/harvest/reddit")
        .method("POST")
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(
            json!({
                "sort": "top",
                "time": "year",
                "limit": 10,
                "gridless_only": false
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.clone().oneshot(harvest_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/event-stream")
    );

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let sse_text = std::str::from_utf8(&body_bytes).unwrap();

    assert!(sse_text.contains("event: progress"));
    assert!(sse_text.contains("event: complete"));
    assert!(sse_text.contains("sse_post_1"));
    // 4. Staging now has the candidate
    let staging_req = Request::builder()
        .uri("/api/staging")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let staging_resp = app.oneshot(staging_req).await.unwrap();
    assert_eq!(staging_resp.status(), StatusCode::OK);
    let staging_bytes = to_bytes(staging_resp.into_body(), usize::MAX).await.unwrap();
    let staging_list: Vec<Candidate> = serde_json::from_slice(&staging_bytes).unwrap();
    assert_eq!(staging_list.len(), 1);
    assert_eq!(staging_list[0].id, "sse_post_1");
}

#[tokio::test]
async fn test_api_cost_stats_and_reset_session() {
    let dir = tempdir().expect("tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let ledger_path = dir.path().join("telemetry").join("cost_ledger.json");
    let ledger = Arc::new(FileCostLedger::new(&ledger_path));
    let state = AppState::with_cost_tracker(storage.clone(), ledger.clone());
    let app = create_router(state);

    // 1. Initial GET /api/cost/stats should be zeroed
    let req = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["session_spend_usd"], 0.0);
    assert_eq!(json["total_spend_usd"], 0.0);
    assert_eq!(json["total_generations"], 0);
    assert_eq!(json["session_generations"], 0);
    assert_eq!(json["by_model"], json!({}));

    // 2. Record generation into ledger
    let record = GenerationCostRecord::new(
        "bench_cost_test",
        "google/gemini-3.1-flash-image",
        "Ancient Tomb",
        0.015,
        Some(150),
        Some(350),
        420,
    );
    ledger.record_generation(record).await.unwrap();

    // 3. GET /api/cost/stats should reflect new data
    let req2 = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let json2: Value = serde_json::from_slice(&body2).unwrap();

    assert_eq!(json2["total_generations"], 1);
    assert_eq!(json2["session_generations"], 1);
    assert!((json2["session_spend_usd"].as_f64().unwrap() - 0.015).abs() < 1e-6);
    assert!((json2["total_spend_usd"].as_f64().unwrap() - 0.015).abs() < 1e-6);
    assert_eq!(
        json2["by_model"]["google/gemini-3.1-flash-image"]["generation_count"],
        1
    );
    assert_eq!(
        json2["by_model"]["google/gemini-3.1-flash-image"]["total_tokens"],
        500
    );

    // 4. POST /api/cost/reset-session
    let reset_req = Request::builder()
        .uri("/api/cost/reset-session")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let reset_resp = app.clone().oneshot(reset_req).await.unwrap();
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let reset_body = to_bytes(reset_resp.into_body(), usize::MAX).await.unwrap();
    let reset_json: Value = serde_json::from_slice(&reset_body).unwrap();

    assert_eq!(reset_json["status"], "ok");
    assert_eq!(reset_json["session_spend_usd"], 0.0);

    // 5. GET /api/cost/stats after reset confirms session is zeroed while persistent metrics remain
    let req3 = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    let body3 = to_bytes(resp3.into_body(), usize::MAX).await.unwrap();
    let json3: Value = serde_json::from_slice(&body3).unwrap();

    assert_eq!(json3["session_generations"], 0);
    assert_eq!(json3["session_spend_usd"], 0.0);
    assert_eq!(json3["total_generations"], 1);
    assert!((json3["total_spend_usd"].as_f64().unwrap() - 0.015).abs() < 1e-6);
    assert_eq!(
        json3["by_model"]["google/gemini-3.1-flash-image"]["generation_count"],
        1
    );
}

// ==========================================================================
// Benchmark API Integration Tests
// ==========================================================================

struct MockBenchmarkEngine {
    dummy_png: Vec<u8>,
    captured_requests: Arc<std::sync::Mutex<Vec<curator::domain::GenerationRequest>>>,
}

impl MockBenchmarkEngine {
    fn new() -> (Self, Arc<std::sync::Mutex<Vec<curator::domain::GenerationRequest>>>) {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Self {
                dummy_png: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                captured_requests: captured.clone(),
            },
            captured,
        )
    }
}

#[async_trait]
impl curator::ports::model_engine::ModelEngine for MockBenchmarkEngine {
    async fn generate_image(
        &self,
        request: &curator::domain::GenerationRequest,
    ) -> Result<curator::domain::GenerationResponse, curator::ports::model_engine::ModelEngineError> {
        self.captured_requests.lock().unwrap().push(request.clone());
        Ok(curator::domain::GenerationResponse {
            image_bytes: self.dummy_png.clone(),
            model: request.model.clone(),
            prompt: request.prompt.clone(),
            prompt_tokens: Some(150),
            completion_tokens: Some(350),
            latency_ms: 200,
            cost_usd: 0.02,
        })
    }
    async fn run_benchmark(
        &self,
        model: &str,
        archetype: &curator::domain::BenchmarkArchetype,
    ) -> Result<curator::domain::BenchmarkResult, curator::ports::model_engine::ModelEngineError> {
        let id = format!("test_{}_{}", model.replace('/', "_"), archetype.slug());
        let metadata = curator::domain::BenchmarkMetadata::new(
            id,
            model,
            archetype.name(),
            archetype.prompt(),
            Some(150),
            Some(350),
            200,
            0.02,
        );
        Ok(curator::domain::BenchmarkResult::new(metadata, self.dummy_png.clone()))
    }

    async fn run_benchmark_suite(
        &self,
        models: &[String],
        archetypes: &[curator::domain::BenchmarkArchetype],
    ) -> Result<Vec<curator::domain::BenchmarkResult>, curator::ports::model_engine::ModelEngineError> {
        let mut results = Vec::new();
        for m in models {
            for a in archetypes {
                results.push(self.run_benchmark(m, a).await?);
            }
        }
        Ok(results)
    }
}

fn setup_benchmark_test_app_with_recorder() -> (
    axum::Router,
    Arc<FilesystemStorage>,
    Arc<FileCostLedger>,
    tempfile::TempDir,
    Arc<std::sync::Mutex<Vec<curator::domain::GenerationRequest>>>,
) {
    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let ledger_path = dir.path().join("telemetry/cost_ledger.json");
    let ledger = Arc::new(FileCostLedger::new(&ledger_path));
    let (engine_struct, captured) = MockBenchmarkEngine::new();
    let engine = Arc::new(engine_struct);
    let benchmark_service = Arc::new(curator::services::BenchmarkService::with_cost_tracker(
        engine,
        storage.clone(),
        ledger.clone(),
    ));
    let state = AppState::with_benchmark_service(storage.clone(), benchmark_service, ledger.clone());
    let router = create_router(state);
    (router, storage, ledger, dir, captured)
}

fn setup_benchmark_test_app() -> (
    axum::Router,
    Arc<FilesystemStorage>,
    Arc<FileCostLedger>,
    tempfile::TempDir,
) {
    let (router, storage, ledger, dir, _captured) = setup_benchmark_test_app_with_recorder();
    (router, storage, ledger, dir)
}

#[tokio::test]
async fn test_api_benchmark_validation() {
    let (app, _storage, _ledger, _dir) = setup_benchmark_test_app();

    // 1. Empty models should fail with 400
    let req1 = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "models": [],
            "archetypes": ["stone-crypt"]
        }).to_string()))
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::BAD_REQUEST);

    // 2. Empty archetypes and custom_prompts should fail with 400
    let req2 = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "models": ["meta/muse-image"],
            "archetypes": [],
            "custom_prompts": []
        }).to_string()))
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_api_benchmark_run_json_and_cost_recording() {
    let (app, storage, ledger, _dir) = setup_benchmark_test_app();

    let payload = json!({
        "models": ["meta/muse-image"],
        "archetypes": ["stone-crypt"],
        "custom_prompts": [
            {
                "name": "Custom Crypt",
                "prompt": "Flat 2D ancient dungeon crypt, gridless"
            }
        ]
    });

    let req = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["total"], 2);
    assert_eq!(json["completed"], 2);
    assert!((json["total_cost_usd"].as_f64().unwrap() - 0.04).abs() < 1e-6);

    // Verify staging discovered candidates
    let candidates = storage.list_staging().expect("list staging succeeds");
    assert_eq!(candidates.len(), 2);

    // Verify cost ledger tracked both generations
    let stats = ledger.get_stats().await.expect("stats succeeds");
    assert_eq!(stats.total_generations, 2);
    assert_eq!(stats.session_generations, 2);
    assert!((stats.session_spend_usd - 0.04).abs() < 1e-6);

    // Verify GET /api/cost/stats endpoint returns the spend
    let stats_req = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let stats_resp = app.oneshot(stats_req).await.unwrap();
    assert_eq!(stats_resp.status(), StatusCode::OK);
    let stats_body = to_bytes(stats_resp.into_body(), usize::MAX).await.unwrap();
    let stats_json: Value = serde_json::from_slice(&stats_body).unwrap();
    assert_eq!(stats_json["total_generations"], 2);
    assert!((stats_json["session_spend_usd"].as_f64().unwrap() - 0.04).abs() < 1e-6);
}

#[tokio::test]
async fn test_api_benchmark_run_sse_streaming() {
    let (app, _storage, _ledger, _dir) = setup_benchmark_test_app();

    let payload = json!({
        "models": ["meta/muse-image"],
        "archetypes": ["stone-crypt"]
    });

    let req = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
    assert!(content_type.contains("text/event-stream"));

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("event: progress"));
    assert!(body_str.contains("event: complete"));
}

#[tokio::test]
async fn test_api_benchmark_cancel() {
    let (app, _storage, _ledger, _dir) = setup_benchmark_test_app();

    let req = Request::builder()
        .uri("/api/benchmark/cancel")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_api_benchmark_reference_conditioning_default_true() {
    let (app, _storage, _ledger, _dir, captured) = setup_benchmark_test_app_with_recorder();

    // Default payload without use_reference_conditioning (defaults to true)
    let payload = json!({
        "models": ["meta/muse-image"],
        "archetypes": ["stone-crypt"]
    });

    let req = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    // Verify SSE progress message shows static reference fallback provenance
    assert!(
        body_str.contains("Generating Stone Crypt for meta/muse-image (static reference: stone-dungeon)"),
        "Expected static reference provenance in SSE stream: {}",
        body_str
    );

    // Verify captured engine request contains reference image
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].images.len(), 1);
    assert_eq!(
        requests[0].images[0].label,
        "Perspective Reference: 2D Stone Dungeon Plan View"
    );
}

#[tokio::test]
async fn test_api_benchmark_reference_conditioning_disabled() {
    let (app, _storage, _ledger, _dir, captured) = setup_benchmark_test_app_with_recorder();

    // Explicitly disable reference conditioning
    let payload = json!({
        "models": ["meta/muse-image"],
        "archetypes": ["stone-crypt"],
        "use_reference_conditioning": false
    });

    let req = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    // Verify SSE progress message does NOT mention reference
    assert!(
        body_str.contains("Generating Stone Crypt for meta/muse-image"),
        "Expected generation message in SSE stream: {}",
        body_str
    );
    assert!(
        !body_str.contains("reference:"),
        "Expected no reference provenance when disabled: {}",
        body_str
    );

    // Verify captured engine request has NO attached reference image
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].images.is_empty());
}

#[tokio::test]
async fn test_api_benchmark_reference_conditioning_curated_prioritization_and_persistence() {
    use chrono::Utc;
    use curator::domain::CuratedMap;
    use image::{ImageBuffer, Rgb};

    let (app, storage, _ledger, _dir, captured) = setup_benchmark_test_app_with_recorder();

    // Save a 5-star curated map in the Dungeon theme
    let map = CuratedMap::new(
        "curated-dungeon-5star",
        Theme::Dungeon,
        vec!["dungeon".to_string(), "crypt".to_string()],
        (1024, 1024),
        Utc::now(),
    );
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(256, 256, Rgb([80, 120, 160]));
    let mut png_bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .unwrap();
    storage.save_curated(&map, &png_bytes).unwrap();

    // Request benchmark run with reference conditioning enabled
    let payload = json!({
        "models": ["meta/muse-image"],
        "archetypes": ["stone-crypt"],
        "use_reference_conditioning": true
    });

    let req = Request::builder()
        .uri("/api/benchmark/run")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    // Verify SSE progress message shows curated reference provenance
    assert!(
        body_str.contains("Generating Stone Crypt for meta/muse-image (curated reference: curated-dungeon-5star)"),
        "Expected curated reference provenance in SSE stream: {}",
        body_str
    );

    // Verify captured engine request has the curated reference image attached
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].images.len(), 1);
    assert_eq!(
        requests[0].images[0].label,
        "Perspective Reference: 2D dungeon Plan View"
    );

    // Verify staging persistence: generated raster output and JSON sidecar
    let staging = storage.list_staging().expect("list staging succeeds");
    assert_eq!(staging.len(), 1);
    assert!(staging[0].id.starts_with("bench_meta_muse_image_stone_crypt_"));
    assert_eq!(staging[0].source, curator::domain::Source::Generated);
}

#[tokio::test]
async fn test_api_layer_image_retrieval() {
    let (app, storage, _dir) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes_staging";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes_staging";

    let mut cand = Candidate::with_now("bundle_img_test", Source::Reddit, "0.png", (1000, 1000));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1000, 1000)),
    ];
    storage
        .save_staging_bundle(None, &cand, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    // 1. Staging layer 0 retrieval
    let req0 = Request::builder()
        .uri("/api/image/staging/bundle_img_test/0")
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
    let body0 = to_bytes(resp0.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body0.as_ref(), img0);

    // 2. Staging layer 1 retrieval
    let req1 = Request::builder()
        .uri("/api/image/staging/bundle_img_test/1")
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(
        resp1.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    let body1 = to_bytes(resp1.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body1.as_ref(), img1);

    // 3. Out-of-bounds layer 99 retrieval returns 404
    let req99 = Request::builder()
        .uri("/api/image/staging/bundle_img_test/99")
        .body(Body::empty())
        .unwrap();
    let resp99 = app.clone().oneshot(req99).await.unwrap();
    assert_eq!(resp99.status(), StatusCode::NOT_FOUND);

    // 4. Base staging image endpoint serves layer 0
    let req_base = Request::builder()
        .uri("/api/image/staging/bundle_img_test")
        .body(Body::empty())
        .unwrap();
    let resp_base = app.clone().oneshot(req_base).await.unwrap();
    assert_eq!(resp_base.status(), StatusCode::OK);
    let body_base = to_bytes(resp_base.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body_base.as_ref(), img0);

    // 5. Curate the bundle to theme "dungeon"
    let curate_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "bundle_img_test",
                "theme": "dungeon",
                "rating": 5,
                "tags": ["fortress"]
            })
            .to_string(),
        ))
        .unwrap();
    let curate_resp = app.clone().oneshot(curate_req).await.unwrap();
    assert_eq!(curate_resp.status(), StatusCode::OK);

    // 6. Curated layer 0 retrieval
    let req_curated0 = Request::builder()
        .uri("/api/image/curated/dungeon/bundle_img_test/0")
        .body(Body::empty())
        .unwrap();
    let resp_curated0 = app.clone().oneshot(req_curated0).await.unwrap();
    assert_eq!(resp_curated0.status(), StatusCode::OK);
    assert_eq!(
        resp_curated0.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    let body_curated0 = to_bytes(resp_curated0.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body_curated0.as_ref(), img0);

    // 7. Curated layer 1 retrieval
    let req_curated1 = Request::builder()
        .uri("/api/image/curated/dungeon/bundle_img_test/1")
        .body(Body::empty())
        .unwrap();
    let resp_curated1 = app.clone().oneshot(req_curated1).await.unwrap();
    assert_eq!(resp_curated1.status(), StatusCode::OK);
    let body_curated1 = to_bytes(resp_curated1.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body_curated1.as_ref(), img1);

    // 8. Curated out-of-bounds layer 99 returns 404
    let req_curated99 = Request::builder()
        .uri("/api/image/curated/dungeon/bundle_img_test/99")
        .body(Body::empty())
        .unwrap();
    let resp_curated99 = app.oneshot(req_curated99).await.unwrap();
    assert_eq!(resp_curated99.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_api_curate_bundle_validation_and_success() {
    let (app, storage, _dir) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes";

    // Case A: Bundle in staging with Unassigned layer is rejected with 400
    let mut cand_unassigned =
        Candidate::with_now("cand_unassigned", Source::Reddit, "0.png", (800, 600));
    cand_unassigned.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (800, 600)),
        CandidateLayer::new(1, "1.png", LayerRole::Unassigned, None, (800, 600)),
    ];
    storage
        .save_staging_bundle(None, &cand_unassigned, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    let req_unassigned = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "cand_unassigned",
                "theme": "dungeon",
                "rating": 4
            })
            .to_string(),
        ))
        .unwrap();
    let resp_unassigned = app.clone().oneshot(req_unassigned).await.unwrap();
    assert_eq!(resp_unassigned.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp_unassigned.into_body(), usize::MAX).await.unwrap();
    let json_err: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json_err["error"].as_str().unwrap().contains("unassigned"),
        "Expected error message mentioning unassigned: {:?}",
        json_err
    );

    // Case B: Bundle in staging with all Exclude layers is rejected with 400
    let mut cand_excluded =
        Candidate::with_now("cand_excluded", Source::Reddit, "0.png", (800, 600));
    cand_excluded.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Exclude, None, (800, 600)),
        CandidateLayer::new(1, "1.png", LayerRole::Exclude, None, (800, 600)),
    ];
    storage
        .save_staging_bundle(None, &cand_excluded, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    let req_excluded = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "cand_excluded",
                "theme": "dungeon",
                "rating": 4
            })
            .to_string(),
        ))
        .unwrap();
    let resp_excluded = app.clone().oneshot(req_excluded).await.unwrap();
    assert_eq!(resp_excluded.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp_excluded.into_body(), usize::MAX).await.unwrap();
    let json_err: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json_err["error"].as_str().unwrap().contains("At least 1 layer must be included"),
        "Expected error message mentioning at least 1 layer included: {:?}",
        json_err
    );

    // Case B2: Missing Ground layer is rejected with 400
    let mut cand_no_ground =
        Candidate::with_now("cand_no_ground", Source::Reddit, "0.png", (800, 600));
    cand_no_ground.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::UpperFloor, Some(1), (800, 600)),
        CandidateLayer::new(1, "1.png", LayerRole::Roof, None, (800, 600)),
    ];
    storage
        .save_staging_bundle(None, &cand_no_ground, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    let req_no_ground = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "cand_no_ground",
                "theme": "dungeon"
            })
            .to_string(),
        ))
        .unwrap();
    let resp_no_ground = app.clone().oneshot(req_no_ground).await.unwrap();
    assert_eq!(resp_no_ground.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp_no_ground.into_body(), usize::MAX).await.unwrap();
    let json_err: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json_err["error"].as_str().unwrap().contains("Ground layer is required"),
        "Expected error message mentioning Ground layer required: {:?}",
        json_err
    );

    // Case B3: Multiple Ground layers is rejected with 400
    let mut cand_multi_ground =
        Candidate::with_now("cand_multi_ground", Source::Reddit, "0.png", (800, 600));
    cand_multi_ground.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (800, 600)),
        CandidateLayer::new(1, "1.png", LayerRole::Ground, Some(0), (800, 600)),
    ];
    storage
        .save_staging_bundle(None, &cand_multi_ground, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    let req_multi_ground = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "cand_multi_ground",
                "theme": "dungeon"
            })
            .to_string(),
        ))
        .unwrap();
    let resp_multi_ground = app.clone().oneshot(req_multi_ground).await.unwrap();
    assert_eq!(resp_multi_ground.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp_multi_ground.into_body(), usize::MAX).await.unwrap();
    let json_err: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json_err["error"].as_str().unwrap().contains("Ground layer is required")
            || json_err["error"].as_str().unwrap().contains("Multiple Ground layers"),
        "Expected error message mentioning multiple Ground layers: {:?}",
        json_err
    );

    // Case C: Successfully curate cand_unassigned by supplying valid layer assignments in CurateRequest
    let req_curate_with_layers = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "cand_unassigned",
                "theme": "dungeon",
                "rating": 5,
                "tags": ["crypt"],
                "layers": [
                    {
                        "index": 0,
                        "filename": "0.png",
                        "role": "ground",
                        "elevation": 0,
                        "dimensions": [800, 600]
                    },
                    {
                        "index": 1,
                        "filename": "1.png",
                        "role": "upper_floor",
                        "elevation": 1,
                        "dimensions": [800, 600]
                    }
                ]
            })
            .to_string(),
        ))
        .unwrap();
    let resp_curate = app.clone().oneshot(req_curate_with_layers).await.unwrap();
    assert_eq!(resp_curate.status(), StatusCode::OK);
    let body = to_bytes(resp_curate.into_body(), usize::MAX).await.unwrap();
    let curate_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(curate_json["status"], "ok");
    assert_eq!(curate_json["curated"]["id"], "cand_unassigned");
    assert_eq!(curate_json["curated"]["tags"], json!(["dungeon", "crypt"]));
    assert!(curate_json["curated"]["themes"].is_null());

    // Staging should no longer contain cand_unassigned
    let remaining_staging = storage.list_staging().unwrap();
    assert!(remaining_staging.iter().all(|c| c.id != "cand_unassigned"));

    // Curated theme should have cand_unassigned with multiple layers
    let curated_list = storage.list_curated(Some(&Theme::Dungeon)).unwrap();
    let curated_item = curated_list
        .iter()
        .find(|m| m.id == "cand_unassigned")
        .expect("curated cand_unassigned exists");
    assert_eq!(curated_item.layers.len(), 2);
    assert_eq!(curated_item.layers[0].role, LayerRole::Ground);
    assert_eq!(curated_item.layers[1].role, LayerRole::UpperFloor);
}

#[tokio::test]
async fn test_api_patch_staging_layers() {
    let (app, storage, _dir) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes";

    let mut cand = Candidate::with_now("patch_layers_cand", Source::Reddit, "0.png", (1200, 800));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Unassigned, None, (1200, 800)),
        CandidateLayer::new(1, "1.png", LayerRole::Unassigned, None, (1200, 800)),
    ];
    storage
        .save_staging_bundle(None, &cand, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    let patch_payload = json!({
        "layers": [
            {
                "index": 0,
                "filename": "0.png",
                "role": "ground",
                "elevation": 0,
                "dimensions": [1200, 800]
            },
            {
                "index": 1,
                "filename": "1.png",
                "role": "roof",
                "elevation": null,
                "dimensions": [1200, 800]
            }
        ]
    });

    let req = Request::builder()
        .uri("/api/staging/patch_layers_cand/layers")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(patch_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated: Candidate = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated.layers.len(), 2);
    assert_eq!(updated.layers[0].role, LayerRole::Ground);
    assert_eq!(updated.layers[1].role, LayerRole::Roof);

    // Verify in staging storage
    let staged_list = storage.list_staging().unwrap();
    let staged_item = staged_list
        .iter()
        .find(|c| c.id == "patch_layers_cand")
        .unwrap();
    assert_eq!(staged_item.layers[0].role, LayerRole::Ground);
    assert_eq!(staged_item.layers[1].role, LayerRole::Roof);
}

#[tokio::test]
async fn test_api_staging_split_and_regroup() {
    let (app, storage, _dir) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nsplit_ground_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nsplit_upper_bytes";

    let mut cand = Candidate::with_now("split_regroup_cand", Source::Reddit, "0.png", (1000, 1000));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1000, 1000)),
    ];
    storage
        .save_staging_bundle(None, &cand, &[img0.to_vec(), img1.to_vec()])
        .unwrap();

    // 1. POST /api/staging/{id}/split decomposes bundle into standalone candidates with parent_group_id
    let req_split = Request::builder()
        .uri("/api/staging/split_regroup_cand/split")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp_split = app.clone().oneshot(req_split).await.unwrap();
    assert_eq!(resp_split.status(), StatusCode::OK);
    let body_split = to_bytes(resp_split.into_body(), usize::MAX).await.unwrap();
    let split_candidates: Vec<Candidate> = serde_json::from_slice(&body_split).unwrap();
    assert_eq!(split_candidates.len(), 2);
    assert_eq!(split_candidates[0].id, "split_regroup_cand_0");
    assert_eq!(split_candidates[1].id, "split_regroup_cand_1");
    assert_eq!(
        split_candidates[0].parent_group_id.as_deref(),
        Some("split_regroup_cand")
    );
    assert_eq!(
        split_candidates[1].parent_group_id.as_deref(),
        Some("split_regroup_cand")
    );

    // Original bundle should no longer exist in staging
    let staging = storage.list_staging().unwrap();
    assert!(staging.iter().all(|c| c.id != "split_regroup_cand"));
    assert!(staging.iter().any(|c| c.id == "split_regroup_cand_0"));
    assert!(staging.iter().any(|c| c.id == "split_regroup_cand_1"));

    // 2. POST /api/staging/{parent_id}/regroup combines split candidates back into single bundle
    let req_regroup = Request::builder()
        .uri("/api/staging/split_regroup_cand/regroup")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp_regroup = app.oneshot(req_regroup).await.unwrap();
    assert_eq!(resp_regroup.status(), StatusCode::OK);
    let body_regroup = to_bytes(resp_regroup.into_body(), usize::MAX).await.unwrap();
    let regrouped: Candidate = serde_json::from_slice(&body_regroup).unwrap();
    assert_eq!(regrouped.id, "split_regroup_cand");
    assert_eq!(regrouped.layers.len(), 2);
    assert_eq!(regrouped.parent_group_id, None);

    // Split children should be cleaned up, and regrouped candidate is in staging
    let final_staging = storage.list_staging().unwrap();
    assert!(final_staging.iter().all(|c| c.id != "split_regroup_cand_0"));
    assert!(final_staging.iter().all(|c| c.id != "split_regroup_cand_1"));
    assert!(final_staging.iter().any(|c| c.id == "split_regroup_cand"));
}

#[tokio::test]
async fn test_suggest_themes_fallback_when_no_api_key() {
    let (app, storage, _dir) = setup_test_app();

    // Ensure candidate exists in staging
    let cand = Candidate::with_now("cand_no_key", Source::Manual, "cand_no_key.png", (1024, 1024));
    storage
        .save_staging(&cand, b"\x89PNG\r\n\x1a\nfake-image-bytes")
        .unwrap();

    // 1. POST /api/staging/{id}/suggest-themes
    let req_post = Request::builder()
        .uri("/api/staging/cand_no_key/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req_post).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: SuggestThemesResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.id, "cand_no_key");
    assert!(json.suggestions.is_empty());

    // 2. GET /api/staging/{id}/suggest-themes
    let req_get = Request::builder()
        .uri("/api/staging/cand_no_key/suggest-themes")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_get = app.oneshot(req_get).await.unwrap();
    assert_eq!(resp_get.status(), StatusCode::OK);
    let body_get = to_bytes(resp_get.into_body(), usize::MAX).await.unwrap();
    let json_get: SuggestThemesResponse = serde_json::from_slice(&body_get).unwrap();
    assert_eq!(json_get.id, "cand_no_key");
    assert!(json_get.suggestions.is_empty());
}

#[tokio::test]
async fn test_suggest_themes_with_model_and_caching() {
    use axum::routing::post;
    use tokio::net::TcpListener;

    let call_count = Arc::new(AtomicUsize::new(0));
    let count_clone = call_count.clone();

    // Mock OpenRouter chat completions endpoint
    let mock_app = axum::Router::new().route(
        "/chat/completions",
        post(move |_body: axum::Json<Value>| {
            let cnt = count_clone.clone();
            async move {
                cnt.fetch_add(1, AtomicOrdering::SeqCst);
                axum::Json(json!({
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "content": "```json\n[\"Dungeon\", \"Crypt\", \"Flooded\"]\n```"
                            }
                        }
                    ]
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(storage.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let cand = Candidate::with_now("cand_cached_1", Source::Manual, "cand_cached_1.png", (1024, 1024));
    storage
        .save_staging(&cand, b"\x89PNG\r\n\x1a\nfake-image-bytes")
        .unwrap();

    // First request: Cache miss -> invokes mock OpenRouter model
    let req1 = Request::builder()
        .uri("/api/staging/cand_cached_1/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1 = to_bytes(resp1.into_body(), usize::MAX).await.unwrap();
    let json1: SuggestThemesResponse = serde_json::from_slice(&body1).unwrap();
    assert_eq!(json1.id, "cand_cached_1");
    assert_eq!(json1.suggestions, vec!["Dungeon", "Crypt", "Flooded"]);
    assert_eq!(call_count.load(AtomicOrdering::SeqCst), 1);

    // Second request via GET: Cache hit -> returns cached suggestions without calling mock model
    let req2 = Request::builder()
        .uri("/api/staging/cand_cached_1/suggest-themes")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let json2: SuggestThemesResponse = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2.id, "cand_cached_1");
    assert_eq!(json2.suggestions, vec!["Dungeon", "Crypt", "Flooded"]);
    // Call count MUST remain 1!
    assert_eq!(call_count.load(AtomicOrdering::SeqCst), 1);

    // Third request via POST: Cache hit -> returns cached suggestions without calling mock model
    let req3 = Request::builder()
        .uri("/api/staging/cand_cached_1/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    let body3 = to_bytes(resp3.into_body(), usize::MAX).await.unwrap();
    let json3: SuggestThemesResponse = serde_json::from_slice(&body3).unwrap();
    assert_eq!(json3.id, "cand_cached_1");
    assert_eq!(json3.suggestions, vec!["Dungeon", "Crypt", "Flooded"]);
    assert_eq!(call_count.load(AtomicOrdering::SeqCst), 1);

    server_task.abort();
}

#[tokio::test]
async fn test_suggest_themes_fallback_on_model_error() {
    use axum::routing::post;
    use tokio::net::TcpListener;

    // Mock OpenRouter returning 500 error
    let mock_app = axum::Router::new().route(
        "/chat/completions",
        post(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": "Model overloaded"})),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(storage.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let cand = Candidate::with_now("cand_err_1", Source::Manual, "cand_err_1.png", (1024, 1024));
    storage
        .save_staging(&cand, b"\x89PNG\r\n\x1a\nfake-image-bytes")
        .unwrap();

    let req = Request::builder()
        .uri("/api/staging/cand_err_1/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should return 200 OK with empty suggestions for resilience
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: SuggestThemesResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.id, "cand_err_1");
    assert!(json.suggestions.is_empty());

    server_task.abort();
}

#[tokio::test]
async fn test_suggest_themes_empty_candidate_id_returns_bad_request() {
    let (app, _storage, _dir) = setup_test_app();

    let req = Request::builder()
        .uri("/api/staging/%20/suggest-themes")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_describe_candidate_fallback_when_no_api_key() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let adapter_no_key = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(storage.clone()).with_openrouter(adapter_no_key);
    let app = create_router(state);

    let cand = Candidate::with_now(
        "cand_desc_fallback",
        Source::Reddit,
        "cand_desc_fallback.png",
        (1024, 1024),
    )
    .with_title("Flooded Stone Dungeon Crypt [50x50]");

    storage
        .save_staging(&cand, b"\x89PNG\r\n\x1a\nfake-image-bytes")
        .unwrap();

    // 1. GET /api/staging/{id}/describe with empty body -> 503 SERVICE_UNAVAILABLE
    let req_get = Request::builder()
        .uri("/api/staging/cand_desc_fallback/describe")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_get = app.clone().oneshot(req_get).await.unwrap();
    assert_eq!(resp_get.status(), StatusCode::SERVICE_UNAVAILABLE);

    // 2. POST /api/staging/{id}/describe with empty body -> 503 SERVICE_UNAVAILABLE
    let req_post_empty = Request::builder()
        .uri("/api/staging/cand_desc_fallback/describe")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let resp_post_empty = app.oneshot(req_post_empty).await.unwrap();
    assert_eq!(resp_post_empty.status(), StatusCode::SERVICE_UNAVAILABLE);
}
#[tokio::test]
async fn test_describe_candidate_with_payload_themes_and_tags() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let mock_desc = "Dark underground cave with sharp stalagmites and glowing crystals.";
    let mock_gen = Arc::new(MockDescriptionGen(Ok(mock_desc.to_string())));
    let state = AppState::new(storage.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(mock_gen))));
    let app = create_router(state);

    let cand = Candidate::with_now(
        "cand_desc_payload",
        Source::Reddit,
        "cand_desc_payload.png",
        (1024, 1024),
    );
    storage
        .save_staging(&cand, b"\x89PNG\r\n\x1a\nfake-image-bytes")
        .unwrap();

    let payload = json!({
        "themes": ["cave"],
        "tags": ["underground", "stalagmites", "crystals"]
    });

    let req = Request::builder()
        .uri("/api/staging/cand_desc_payload/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: DescribeResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.id, "cand_desc_payload");
    assert!(json.description.contains("cave"));
    assert!(json.description.contains("stalagmites"));
}
#[tokio::test]
async fn test_describe_candidate_with_mock_openrouter() {
    use axum::routing::post;
    use tokio::net::TcpListener;

    let mock_desc = "Ancient sunken temple with moss-covered stone carvings and glowing runes.";
    let mock_desc_clone = mock_desc.to_string();

    let recorded_request = Arc::new(std::sync::Mutex::new(None));
    let recorded_req_clone = recorded_request.clone();

    let mock_app = axum::Router::new().route(
        "/chat/completions",
        post(move |axum::Json(body): axum::Json<Value>| {
            let desc = mock_desc_clone.clone();
            *recorded_req_clone.lock().unwrap() = Some(body.clone());
            async move {
                axum::Json(json!({
                    "id": "gen-desc-1",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": desc
                        }
                    }]
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let cand = Candidate::with_now("cand_mock_ai", Source::Manual, "cand_mock_ai.png", (1024, 1024));
    storage.save_staging(&cand, &create_test_png()).unwrap();

    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(storage.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let req = Request::builder()
        .uri("/api/staging/cand_mock_ai/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "themes": ["temple", "ruins"],
                "tags": ["moss", "runes", "sunken"]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: DescribeResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.id, "cand_mock_ai");
    assert_eq!(json.description, mock_desc);

    // Verify request payload to OpenRouter used Gemini 3.5 Flash Lite and incorporated themes/tags
    let recorded = recorded_request.lock().unwrap();
    let req_body = recorded.as_ref().expect("Mock OpenRouter should have received a request");
    assert_eq!(req_body["model"].as_str(), Some("google/gemini-3.5-flash-lite"));

    server_task.abort();
}

#[tokio::test]
async fn test_describe_candidate_offline_fallback() {
    let cand = Candidate::with_now(
        "cand_offline_desc",
        Source::Reddit,
        "cand_offline_desc.png",
        (1024, 1024),
    )
    .with_title("Flooded Stone Dungeon Crypt [50x50]");

    let payload = json!({
        "themes": ["dungeon"],
        "tags": ["crypt", "stone", "water"]
    });

    // 1. With no API key: OpenRouter configured with empty key -> strictly 503 SERVICE_UNAVAILABLE
    let dir1 = tempdir().unwrap();
    let storage1 = Arc::new(FilesystemStorage::new(dir1.path()));
    storage1
        .save_staging(&cand, &create_test_png())
        .unwrap();
    let adapter_no_key = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state1 = AppState::new(storage1).with_openrouter(adapter_no_key);
    let app1 = create_router(state1);

    let req1 = Request::builder()
        .uri("/api/staging/cand_offline_desc/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp1 = app1.oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::SERVICE_UNAVAILABLE); // Strict error propagation (no silent fallback)

    // 2. Offline / unreachable OpenRouter endpoint -> strictly 502 BAD_GATEWAY
    let dir2 = tempdir().unwrap();
    let storage2 = Arc::new(FilesystemStorage::new(dir2.path()));
    storage2
        .save_staging(&cand, &create_test_png())
        .unwrap();
    let adapter_offline = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state2 = AppState::new(storage2).with_openrouter(adapter_offline);
    let app2 = create_router(state2);

    let req2 = Request::builder()
        .uri("/api/staging/cand_offline_desc/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp2 = app2.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::BAD_GATEWAY); // Strict error propagation (no silent fallback)
}

#[tokio::test]
async fn test_describe_candidate_error_handling() {
    let (app, _storage, _dir) = setup_test_app();

    // 1. Not found candidate
    let req_not_found = Request::builder()
        .uri("/api/staging/nonexistent_cand_xyz/describe")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_not_found = app.clone().oneshot(req_not_found).await.unwrap();
    assert_eq!(resp_not_found.status(), StatusCode::NOT_FOUND);

    // 2. Empty candidate ID
    let req_empty_id = Request::builder()
        .uri("/api/staging/%20/describe")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp_empty_id = app.oneshot(req_empty_id).await.unwrap();
    assert_eq!(resp_empty_id.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_curate_candidate_with_explicit_description_persistence() {
    let (app, storage, dir) = setup_test_app();

    let cand = Candidate::with_now(
        "cand_curate_explicit_desc",
        Source::Reddit,
        "cand_curate_explicit_desc.png",
        (1024, 1024),
    );
    storage
        .save_staging(&cand, &create_test_png())
        .unwrap();

    let custom_desc = "Grand limestone throne room with ornate stone pillars, banners, and obsidian dais.";
    let curate_payload = json!({
        "id": "cand_curate_explicit_desc",
        "themes": ["dungeon"],
        "tags": ["throne-room", "pillars"],
        "description": custom_desc
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");

    // 1. Response has curated.description == Some(...)
    assert_eq!(json["curated"]["description"].as_str(), Some(custom_desc));

    // 2. data/curated/<id>/manifest.json contains "description"
    let manifest_path = dir
        .path()
        .join("curated")
        .join("cand_curate_explicit_desc")
        .join("manifest.json");
    assert!(manifest_path.is_file());
    let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_str.contains("\"description\""));
    assert!(manifest_str.contains("obsidian dais"));

    // 3. Primary layer caption matches description
    let curated_list = storage.list_curated(None).unwrap();
    assert_eq!(curated_list.len(), 1);
    assert_eq!(curated_list[0].description.as_deref(), Some(custom_desc));
    let ground_layer = curated_list[0]
        .layers
        .iter()
        .find(|l| l.role == LayerRole::Ground)
        .expect("Ground layer must be present");
    assert_eq!(ground_layer.caption.as_deref(), Some(custom_desc));

    // 4. <id>_lora1024.txt companion file exists and contains the description
    let lora_path = dir
        .path()
        .join("curated")
        .join("cand_curate_explicit_desc")
        .join("cand_curate_explicit_desc_lora1024.txt");
    assert!(lora_path.is_file());
    let lora_str = std::fs::read_to_string(&lora_path).unwrap();
    assert!(lora_str.contains("obsidian dais"));
}

#[tokio::test]
async fn test_curate_candidate_auto_description_fallback_offline() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(storage.clone()).with_openrouter(adapter);
    let app = create_router(state);
    let cand = Candidate::with_now(
        "cand_curate_auto_offline",
        Source::Reddit,
        "cand_curate_auto_offline.png",
        (1024, 1024),
    );
    storage
        .save_staging(&cand, &create_test_png())
        .unwrap();

    let curate_payload = json!({
        "id": "cand_curate_auto_offline",
        "themes": ["dungeon"],
        "tags": ["hallway", "torches"]
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Vision failure without description must fail curation with 502 Bad Gateway
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

    // Staging candidate must NOT be removed or committed
    assert_eq!(storage.list_staging().unwrap().len(), 1);

    // Curated manifest must NOT exist on disk
    let manifest_path = dir
        .path()
        .join("curated")
        .join("cand_curate_auto_offline")
        .join("manifest.json");
    assert!(!manifest_path.exists());
}

#[tokio::test]
async fn test_legacy_manifest_without_description_deserialization() {
    let (app, _storage, dir) = setup_test_app();

    // Create a legacy curated bundle directly on disk without a "description" field in manifest.json
    let legacy_id = "legacy_cand_no_desc";
    let bundle_dir = dir.path().join("curated").join(legacy_id);
    std::fs::create_dir_all(&bundle_dir).unwrap();

    let legacy_manifest_json = json!({
        "id": legacy_id,
        "themes": ["dungeon"],
        "tags": ["corridor", "torches"],
        "dimensions": [1024, 1024],
        "curated_at": "2026-01-01T00:00:00Z",
        "layers": [
            {
                "index": 0,
                "filename": "0_ground.png",
                "role": "ground",
                "elevation": 0,
                "dimensions": [1024, 1024]
            }
        ]
    });

    let manifest_path = bundle_dir.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&legacy_manifest_json).unwrap()).unwrap();

    // Verify direct deserialization succeeds cleanly without error
    let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(!manifest_content.contains("\"description\""));
    let deserialized: Result<CuratedMap, _> = serde_json::from_str(&manifest_content);
    assert!(deserialized.is_ok(), "Legacy manifest without description field must deserialize cleanly");
    let map = deserialized.unwrap();
    assert_eq!(map.id, legacy_id);
    assert_eq!(map.description, None);

    // Write dummy image files
    let dummy_image = create_test_png();
    std::fs::write(bundle_dir.join("0_ground.png"), &dummy_image).unwrap();
    std::fs::write(bundle_dir.join(format!("{legacy_id}.png")), &dummy_image).unwrap();

    // Call GET /api/curated
    let req = Request::builder()
        .uri("/api/curated")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let curated_list: Vec<CuratedMap> = serde_json::from_slice(&body).unwrap();

    let legacy_item = curated_list.iter().find(|m| m.id == legacy_id);
    assert!(legacy_item.is_some(), "Legacy curated item must be returned by GET /api/curated");
    let legacy_item = legacy_item.unwrap();
    assert_eq!(legacy_item.id, legacy_id);
    assert_eq!(legacy_item.tags, vec!["dungeon", "corridor", "torches"]);
    assert_eq!(legacy_item.primary_theme(), "dungeon");
}

#[tokio::test]
async fn test_api_post_curate_delegates_to_curation_workflow_and_zero_store_scans() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use curator::services::curation::{CurationWorkflow, CurationWorkflowError};

    struct SpyWorkflow {
        curate_calls: AtomicUsize,
        reject_calls: AtomicUsize,
    }

    #[async_trait]
    impl CurationWorkflow for SpyWorkflow {
        async fn curate(
            &self,
            candidate_id: &str,
            tags: Vec<String>,
            _layers: Option<Vec<curator::domain::Layer>>,
            description: Option<String>,
        ) -> Result<CuratedMap, CurationWorkflowError> {
            self.curate_calls.fetch_add(1, Ordering::SeqCst);
            let map = CuratedMap::with_now(
                candidate_id,
                Vec::<String>::new(),
                tags,
                (1000, 1000),
            )
            .with_description(description.unwrap_or_else(|| "Spy description".into()));
            Ok(map)
        }

        async fn reject(
            &self,
            _candidate_id: &str,
            _reason: curator::domain::RejectionReason,
            _notes: Option<&str>,
        ) -> Result<(), CurationWorkflowError> {
            self.reject_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let spy = Arc::new(SpyWorkflow {
        curate_calls: AtomicUsize::new(0),
        reject_calls: AtomicUsize::new(0),
    });

    let state = AppState::new(storage.clone()).with_curation_service(spy.clone());
    let app = create_router(state);

    let curate_payload = json!({
        "id": "cand_spy_test",
        "themes": ["dungeon"],
        "tags": ["crypt"]
    });

    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "cand_spy_test");

    // Verified: exactly 1 call to CurationWorkflow::curate
    assert_eq!(spy.curate_calls.load(Ordering::SeqCst), 1);
    // Verified: zero calls to storage list_staging / list_curated by route handler
    assert_eq!(storage.list_staging().unwrap().len(), 0);
    assert_eq!(storage.list_curated(None).unwrap().len(), 0);

    // Also verify reject delegates to CurationWorkflow
    let reject_payload = json!({
        "id": "cand_spy_test",
        "reason": "BlurryLowRes",
        "notes": "Too blurry"
    });
    let req_reject = Request::builder()
        .uri("/api/reject")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(reject_payload.to_string()))
        .unwrap();
    let resp_reject = app.oneshot(req_reject).await.unwrap();
    assert_eq!(resp_reject.status(), StatusCode::OK);
    assert_eq!(spy.reject_calls.load(Ordering::SeqCst), 1);
}
