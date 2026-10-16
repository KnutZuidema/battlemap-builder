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
use curator::api::{create_router, AppState, SuggestThemesResponse};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use curator::ports::cost_tracker::{CostTracker, GenerationCostRecord};
use curator::domain::{
    Candidate, CandidateLayer, HarvestedMedia, LayerRole, ParsedTags, RedditHarvestQuery,
    RedditPost, Source, Theme,
};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::ports::storage::StorageRepository;
fn setup_test_app() -> (axum::Router, Arc<FilesystemStorage>, tempfile::TempDir) {
    let dir = tempdir().expect("Failed to create tempdir");
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let state = AppState::new(storage.clone());
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
    assert_eq!(res_json["curated"]["themes"], json!(["dungeon"]));
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
    assert_eq!(res_json["curated"]["themes"], json!(["building"]));

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
    assert_eq!(res_json2["curated"]["themes"], json!(["building"]));

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
    assert!(html_str.contains("rejection-modal"));
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
        json_err["error"].as_str().unwrap().contains("Unassigned"),
        "Expected error message mentioning Unassigned: {:?}",
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
    assert_eq!(curate_json["curated"]["themes"], json!(["dungeon"]));

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
