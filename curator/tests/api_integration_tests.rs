use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use async_trait::async_trait;
use curator::adapters::{FileCostLedger, FilesystemStorage};
use curator::api::{create_router, AppState};
use curator::ports::cost_tracker::{CostTracker, GenerationCostRecord};
use curator::domain::{
    Candidate, HarvestedMedia, ParsedTags, RedditHarvestQuery, RedditPost, Source, Theme,
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

    // Curate candidate with rating 4
    let curate_payload = json!({
        "id": "map_to_curate",
        "theme": "dungeon",
        "rating": 4,
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
    assert_eq!(res_json["curated"]["theme"], "dungeon");
    assert_eq!(res_json["curated"]["rating"], 4);
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

    // Invalid rating should return 400
    let invalid_rating_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "map_to_curate",
                "theme": "dungeon",
                "rating": 6,
                "tags": []
            })
            .to_string(),
        ))
        .unwrap();
    let resp_invalid = app.oneshot(invalid_rating_req).await.unwrap();
    assert_eq!(resp_invalid.status(), StatusCode::BAD_REQUEST);
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
    assert!(html_str.contains("theme-selector"));
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
    let resp_non_existent = app.oneshot(req_non_existent).await.unwrap();
    assert_eq!(resp_non_existent.status(), StatusCode::NOT_FOUND);
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
