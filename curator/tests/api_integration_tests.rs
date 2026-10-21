use std::sync::Arc;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use async_trait::async_trait;
use curator::adapters::{
    FileCostLedger, InMemoryBlobStorage, InMemoryCatalogRepository, OpenRouterAdapter,
};
use curator::api::{create_router, AppState, DescribeResponse, SuggestThemesResponse};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use curator::ports::cost_tracker::{CostTracker, GenerationCostRecord};
use curator::domain::{
    Candidate, CandidateLayer, CuratedMap, GridConfig, HarvestedMedia, LayerRole, ParsedTags,
    RedditHarvestQuery, RedditPost, Source, Theme,
};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::domain::CuratedLayer;
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
/// Default test generator: echoes the request tags and distinguishes image content,
/// so multi-section curation tests receive crop-specific descriptions.
struct TaggedMockDescriptionGen;

#[async_trait]
impl DescriptionGenerator for TaggedMockDescriptionGen {
    async fn generate_description(
        &self,
        image_bytes: &[u8],
        _filename: Option<&str>,
        tags: &[String],
        _title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        let checksum = image_bytes
            .iter()
            .fold(0u64, |acc, byte| acc.wrapping_mul(31).wrapping_add(u64::from(*byte)));
        Ok(format!(
            "Default test battlemap description for {} (checksum {checksum})",
            tags.join(", ")
        ))
    }
}

/// Records the `tags` and `title` arguments passed to the generator.
#[derive(Default)]
struct CapturingDescriptionGen {
    calls: std::sync::Mutex<Vec<(Vec<String>, Option<String>)>>,
}

#[async_trait]
impl DescriptionGenerator for CapturingDescriptionGen {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _filename: Option<&str>,
        tags: &[String],
        title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        self.calls
            .lock()
            .unwrap()
            .push((tags.to_vec(), title.map(str::to_string)));
        Ok("Captured physical description.".to_string())
    }
}

/// Fresh in-memory catalog and blob ports: the routes are served from these doubles alone.
fn in_memory_ports() -> (Arc<InMemoryCatalogRepository>, Arc<InMemoryBlobStorage>) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

fn setup_test_app() -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let (catalog, blobs) = in_memory_ports();
    let desc_service = Arc::new(DescriptionService::new(Some(Arc::new(
        TaggedMockDescriptionGen,
    ))));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_description_service(desc_service);
    let router = create_router(state);
    (router, catalog, blobs)
}

/// Seeds a staging candidate and its primary layer blob at the canonical port addresses.
async fn seed_staging(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<InMemoryBlobStorage>,
    candidate: &Candidate,
    png: &[u8],
) {
    catalog.save_candidate(candidate).await.expect("save candidate");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: candidate.id.clone(),
                index: 0,
            },
            png,
        )
        .await
        .expect("write staging blob");
}

/// Seeds a staging bundle manifest and one `staging/{id}/{index}.png` blob per layer.
async fn seed_staging_bundle(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<InMemoryBlobStorage>,
    candidate: &Candidate,
    layer_bytes: &[Vec<u8>],
) {
    catalog.save_candidate(candidate).await.expect("save candidate");
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
            .expect("write staging layer");
    }
}

/// Blob key of a curated map's `Ground` layer, resolved like the dataset export service.
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

/// Seeds a curated map with an explicit Ground layer and its layer blob.
async fn seed_curated(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<InMemoryBlobStorage>,
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
    catalog.save_curated(&stored).await.expect("save curated map");
    blobs
        .write_blob(&curated_ground_key(&stored), png)
        .await
        .expect("write curated Ground blob");
}

#[tokio::test]
async fn test_api_status_empty() {
    let (app, _catalog, _blobs) = setup_test_app();

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
    let (app, catalog, blobs) = setup_test_app();

    // Populate staging with 2 candidates
    let cand1 = Candidate::with_now("c1", Source::Reddit, "c1.png", (1024, 768));
    let cand2 = Candidate::with_now("c2", Source::Manual, "c2.jpg", (2048, 1536));
    seed_staging(&catalog, &blobs, &cand1, b"\x89PNG\r\n\x1a\nfake-png-1").await;
    seed_staging(&catalog, &blobs, &cand2, b"\xFF\xD8\xFFfake-jpeg-2").await;

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
    let (app, catalog, blobs) = setup_test_app();

    let png_data = b"\x89PNG\r\n\x1a\nsome_png_image_payload";
    let cand = Candidate::with_now("png_candidate", Source::Reddit, "map.png", (1000, 1000));
    seed_staging(&catalog, &blobs, &cand, png_data).await;

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
    let (app, catalog, blobs) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\ncurate_this_map";
    let cand = Candidate::with_now("map_to_curate", Source::Generated, "dungeon.png", (1920, 1080));
    seed_staging(&catalog, &blobs, &cand, img_bytes).await;

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
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 0);

    // Curated directory should have 1 item
    let curated = catalog.list_curated(&CuratedFilter { theme: Some(Theme::Dungeon), ..Default::default() }).await.unwrap();
    assert_eq!(curated.len(), 1);
    assert_eq!(curated[0].id, "map_to_curate");

    // Fetch the curated Ground image by direct ID.
    let req_img = Request::builder()
        .uri("/api/image/curated/map_to_curate")
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
    let (app, catalog, blobs) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\ntags_only_map";
    let cand = Candidate::with_now("tags_only_map", Source::Generated, "dungeon.png", (1920, 1080));
    seed_staging(&catalog, &blobs, &cand, img_bytes).await;

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
    seed_staging(&catalog, &blobs, &cand, img_bytes).await;
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
    let (app, catalog, blobs) = setup_test_app();

    let mut cand = Candidate::with_now("provenance_map", Source::Reddit, "0.png", (1920, 1080));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080))
            .with_source_url("https://i.redd.it/provenance_0.png"),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080))
            .with_source_url("https://i.redd.it/provenance_1.png"),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[
                b"\x89PNG\r\n\x1a\nprovenance_ground".to_vec(),
                b"\x89PNG\r\n\x1a\nprovenance_upper".to_vec(),
            ]).await;

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
    let (app, catalog, blobs) = setup_test_app();

    // 1. Prepare candidate in staging with multi-layer bundle
    let img0 = b"\x89PNG\r\n\x1a\nlayer0_ground_map";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_upper_map";
    let mut cand = Candidate::with_now("dungeon_complex", Source::Reddit, "0.png", (1920, 1080));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1920, 1080)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1920, 1080)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[img0.to_vec(), img1.to_vec()]).await;

    // Verify candidate is in staging
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 1);

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
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 0);

    // 6. Verify the curated manifest persisted in the catalog matches the response.
    let manifest = catalog.get_curated("dungeon_complex").await.unwrap();
    assert_eq!(manifest.id, "dungeon_complex");
    assert_eq!(
        manifest.tags,
        vec!["dungeon", "crypt", "boss-arena", "torchlight", "fantasy"]
    );
    assert_eq!(manifest.dimensions, (1920, 1080));
    assert_eq!(manifest.layers[0].filename, "0_ground.png");
    assert_eq!(manifest.layers[1].filename, "1_upper_1.png");

    // Deterministic curated layer blobs exist at their canonical addresses.
    let curated_keys = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("dungeon_complex".to_string()),
        })
        .await
        .unwrap();
    assert!(curated_keys.iter().any(|key| key.to_path().ends_with("0_ground.png")));
    assert!(curated_keys.iter().any(|key| key.to_path().ends_with("1_upper_1.png")));

    // 7. Verify API image retrieval by direct ID.
    let req_direct = Request::builder()
        .uri("/api/image/curated/dungeon_complex")
        .body(Body::empty())
        .unwrap();
    let resp_direct = app.oneshot(req_direct).await.unwrap();
    assert_eq!(resp_direct.status(), StatusCode::OK);
    let direct_bytes = to_bytes(resp_direct.into_body(), usize::MAX).await.unwrap();
    assert_eq!(direct_bytes.as_ref(), img0);
}

#[tokio::test]
async fn test_api_curate_building_theme_and_aliases() {
    let (app, catalog, blobs) = setup_test_app();

    let img_bytes = b"\x89PNG\r\n\x1a\nisolated_cottage";
    let cand1 = Candidate::with_now("cabin_map", Source::Reddit, "cabin.png", (2048, 2048));
    seed_staging(&catalog, &blobs, &cand1, img_bytes).await;

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
    let curated = catalog.list_curated(&CuratedFilter { theme: Some(Theme::Building), ..Default::default() }).await.unwrap();
    assert_eq!(curated.len(), 1);
    assert_eq!(curated[0].id, "cabin_map");
    assert_eq!(curated[0].theme(), Some(Theme::Building));

    // Fetch the curated image via /api/image/curated/cabin_map
    let req_img = Request::builder()
        .uri("/api/image/curated/cabin_map")
        .body(Body::empty())
        .unwrap();
    let resp_img = app.clone().oneshot(req_img).await.unwrap();
    assert_eq!(resp_img.status(), StatusCode::OK);

    // Test alias "outpost" also curates into Theme::Building
    let cand2 = Candidate::with_now("outpost_map", Source::Generated, "outpost.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand2, img_bytes).await;

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

    let building_list = catalog.list_curated(&CuratedFilter { theme: Some(Theme::Building), ..Default::default() }).await.unwrap();
    assert_eq!(building_list.len(), 2);
}

#[tokio::test]
async fn test_api_reject_endpoint() {
    let (app, catalog, blobs) = setup_test_app();

    let cand = Candidate::with_now("bad_map", Source::Reddit, "bad.png", (800, 600));
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nbad").await;

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
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 0);

    // Should be recorded in rejections
    assert!(catalog.is_rejected("bad_map").await.unwrap());

    // Test rejecting without explicit reason or notes
    let cand_simple = Candidate::with_now("bad_map_simple", Source::Reddit, "simple.png", (800, 600));
    seed_staging(&catalog, &blobs, &cand_simple, b"\x89PNG\r\n\x1a\nsimple").await;

    let req_simple = Request::builder()
        .uri("/api/reject")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "bad_map_simple" }).to_string()))
        .unwrap();

    let response_simple = app.clone().oneshot(req_simple).await.unwrap();
    assert_eq!(response_simple.status(), StatusCode::OK);
    assert!(catalog.is_rejected("bad_map_simple").await.unwrap());
}

#[tokio::test]
async fn test_static_asset_and_spa_routing() {
    let (app, _catalog, _blobs) = setup_test_app();

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
    let (app, _catalog, _blobs) = setup_test_app();

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

    let (catalog, blobs) = in_memory_ports();
    let harvester = Arc::new(MockHarvester {
        posts: vec![post_gridless, post_gridded],
    });
    let state = AppState::with_harvester(harvester, catalog, blobs);
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

    let (catalog, blobs) = in_memory_ports();
    let harvester = Arc::new(MockHarvester { posts: vec![post] });
    let state = AppState::with_harvester(harvester, catalog, blobs);
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
    let (catalog, blobs) = in_memory_ports();
    let ledger_path = dir.path().join("telemetry").join("cost_ledger.json");
    let ledger = Arc::new(FileCostLedger::new(&ledger_path));
    let state = AppState::with_cost_tracker(ledger.clone(), catalog, blobs);
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

/// Router, ports, cost ledger, and the engine request recorder of the benchmark test app.
type BenchmarkTestApp = (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
    Arc<FileCostLedger>,
    Arc<std::sync::Mutex<Vec<curator::domain::GenerationRequest>>>,
);

fn setup_benchmark_test_app_with_recorder() -> BenchmarkTestApp {
    let dir = tempdir().expect("Failed to create tempdir");
    let (catalog, blobs) = in_memory_ports();
    let ledger_path = dir.path().join("telemetry/cost_ledger.json");
    let ledger = Arc::new(FileCostLedger::new(&ledger_path));
    let (engine_struct, captured) = MockBenchmarkEngine::new();
    let engine = Arc::new(engine_struct);
    let benchmark_service = Arc::new(curator::services::BenchmarkService::with_cost_tracker(
        engine,
        ledger.clone(),
        catalog.clone(),
        blobs.clone(),
    ));
    let state =
        AppState::with_benchmark_service(benchmark_service, ledger.clone(), catalog.clone(), blobs.clone());
    let router = create_router(state);
    (router, catalog, blobs, ledger, captured)
}

fn setup_benchmark_test_app() -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
    Arc<FileCostLedger>,
) {
    let (router, catalog, blobs, ledger, _captured) = setup_benchmark_test_app_with_recorder();
    (router, catalog, blobs, ledger)
}

#[tokio::test]
async fn test_api_benchmark_validation() {
    let (app, _catalog, _blobs, _ledger) = setup_benchmark_test_app();

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
    let (app, catalog, _blobs, ledger) = setup_benchmark_test_app();

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
    let candidates = catalog.list_candidates(&CandidateFilter::default()).await.expect("list staging succeeds");
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
    let (app, _catalog, _blobs, _ledger) = setup_benchmark_test_app();

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
    let (app, _catalog, _blobs, _ledger) = setup_benchmark_test_app();

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
    let (app, _catalog, _blobs, _ledger, captured) = setup_benchmark_test_app_with_recorder();

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
    let (app, _catalog, _blobs, _ledger, captured) = setup_benchmark_test_app_with_recorder();

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

    let (app, catalog, blobs, _ledger, captured) = setup_benchmark_test_app_with_recorder();

    // Save a 5-star curated map in the Dungeon theme
    let map = CuratedMap::new(
        "curated-dungeon-5star",
        Theme::Dungeon,
        vec!["dungeon".to_string(), "crypt".to_string()],
        (1024, 1024),
        Utc::now(),
    );
    // 1024x1024 matches the map's declared dimensions and default 64px grid so the exact
    // 16x16 Grid Cell section box resolves without rescaling.
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(1024, 1024, Rgb([80, 120, 160]));
    let mut png_bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .unwrap();
    seed_curated(&catalog, &blobs, &map, &png_bytes).await;

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

    // Verify staging persistence: generated raster output and JSON sidecar
    let staging = catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .expect("list staging succeeds");
    assert_eq!(staging.len(), 1);
    assert!(staging[0].id.starts_with("bench_meta_muse_image_stone_crypt_"));
    assert_eq!(staging[0].source, curator::domain::Source::Generated);

    // Verify captured engine request has the curated reference image attached
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].images.len(), 1);
    assert_eq!(
        requests[0].images[0].label,
        "Perspective Reference: 2D dungeon Plan View"
    );
}

#[tokio::test]
async fn test_api_layer_image_retrieval() {
    let (app, catalog, blobs) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes_staging";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes_staging";

    let mut cand = Candidate::with_now("bundle_img_test", Source::Reddit, "0.png", (1024, 1024));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1024, 1024)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[img0.to_vec(), img1.to_vec()]).await;

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
        .uri("/api/image/curated/bundle_img_test/0")
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
        .uri("/api/image/curated/bundle_img_test/1")
        .body(Body::empty())
        .unwrap();
    let resp_curated1 = app.clone().oneshot(req_curated1).await.unwrap();
    assert_eq!(resp_curated1.status(), StatusCode::OK);
    let body_curated1 = to_bytes(resp_curated1.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body_curated1.as_ref(), img1);

    // 8. Curated out-of-bounds layer 99 returns 404
    let req_curated99 = Request::builder()
        .uri("/api/image/curated/bundle_img_test/99")
        .body(Body::empty())
        .unwrap();
    let resp_curated99 = app.oneshot(req_curated99).await.unwrap();
    assert_eq!(resp_curated99.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_api_curate_bundle_validation_and_success() {
    let (app, catalog, blobs) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes";

    // Case A: Bundle in staging with Unassigned layer is rejected with 400
    let mut cand_unassigned =
        Candidate::with_now("cand_unassigned", Source::Reddit, "0.png", (1024, 1024));
    cand_unassigned.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024)),
        CandidateLayer::new(1, "1.png", LayerRole::Unassigned, None, (1024, 1024)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand_unassigned, &[img0.to_vec(), img1.to_vec()]).await;

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
        Candidate::with_now("cand_excluded", Source::Reddit, "0.png", (1024, 1024));
    cand_excluded.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Exclude, None, (1024, 1024)),
        CandidateLayer::new(1, "1.png", LayerRole::Exclude, None, (1024, 1024)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand_excluded, &[img0.to_vec(), img1.to_vec()]).await;

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
    seed_staging_bundle(&catalog, &blobs, &cand_no_ground, &[img0.to_vec(), img1.to_vec()]).await;

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
    seed_staging_bundle(&catalog, &blobs, &cand_multi_ground, &[img0.to_vec(), img1.to_vec()]).await;

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
                        "dimensions": [1024, 1024]
                    },
                    {
                        "index": 1,
                        "filename": "1.png",
                        "role": "upper_floor",
                        "elevation": 1,
                        "dimensions": [1024, 1024]
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
    let remaining_staging = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
    assert!(remaining_staging.iter().all(|c| c.id != "cand_unassigned"));

    // Curated theme should have cand_unassigned with multiple layers
    let curated_list = catalog.list_curated(&CuratedFilter { theme: Some(Theme::Dungeon), ..Default::default() }).await.unwrap();
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
    let (app, catalog, blobs) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nlayer0_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nlayer1_bytes";

    let mut cand = Candidate::with_now("patch_layers_cand", Source::Reddit, "0.png", (1200, 800));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Unassigned, None, (1200, 800)),
        CandidateLayer::new(1, "1.png", LayerRole::Unassigned, None, (1200, 800)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[img0.to_vec(), img1.to_vec()]).await;

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
    let staged_list = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
    let staged_item = staged_list
        .iter()
        .find(|c| c.id == "patch_layers_cand")
        .unwrap();
    assert_eq!(staged_item.layers[0].role, LayerRole::Ground);
    assert_eq!(staged_item.layers[1].role, LayerRole::Roof);
}

#[tokio::test]
async fn test_api_staging_split_and_regroup() {
    let (app, catalog, blobs) = setup_test_app();

    let img0 = b"\x89PNG\r\n\x1a\nsplit_ground_bytes";
    let img1 = b"\x89PNG\r\n\x1a\nsplit_upper_bytes";

    let mut cand = Candidate::with_now("split_regroup_cand", Source::Reddit, "0.png", (1000, 1000));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1000, 1000)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1000, 1000)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[img0.to_vec(), img1.to_vec()]).await;

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
    let staging = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
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
    let final_staging = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
    assert!(final_staging.iter().all(|c| c.id != "split_regroup_cand_0"));
    assert!(final_staging.iter().all(|c| c.id != "split_regroup_cand_1"));
    assert!(final_staging.iter().any(|c| c.id == "split_regroup_cand"));
}

#[tokio::test]
async fn test_suggest_themes_fallback_when_no_api_key() {
    let (app, catalog, blobs) = setup_test_app();

    // Ensure candidate exists in staging
    let cand = Candidate::with_now("cand_no_key", Source::Manual, "cand_no_key.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

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

    let (catalog, blobs) = in_memory_ports();
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let cand = Candidate::with_now("cand_cached_1", Source::Manual, "cand_cached_1.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

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

    let (catalog, blobs) = in_memory_ports();
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let cand = Candidate::with_now("cand_err_1", Source::Manual, "cand_err_1.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

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
    let (app, _catalog, _blobs) = setup_test_app();

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
    let (catalog, blobs) = in_memory_ports();
    let adapter_no_key = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter_no_key);
    let app = create_router(state);

    let cand = Candidate::with_now(
        "cand_desc_fallback",
        Source::Reddit,
        "cand_desc_fallback.png",
        (1024, 1024),
    )
    .with_title("Flooded Stone Dungeon Crypt [50x50]");

    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

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
async fn test_describe_candidate_passes_candidate_title_and_merged_tags() {
    let (catalog, blobs) = in_memory_ports();
    let generator = Arc::new(CapturingDescriptionGen::default());
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(generator.clone()))));
    state.theme_suggestions_cache.write().await.insert(
        "cand_desc_title".to_string(),
        vec!["crypt".to_string(), "flooded".to_string()],
    );
    let app = create_router(state);

    let cand = Candidate::with_now(
        "cand_desc_title",
        Source::Reddit,
        "cand_desc_title.png",
        (1024, 1024),
    )
    .with_title("Flooded Stone Dungeon Crypt [50x50]");
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

    let payload = json!({
        "tags": ["stone", "Crypt", "  "]
    });

    let req = Request::builder()
        .uri("/api/staging/cand_desc_title/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let calls = generator.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "generator should be called exactly once");
    assert_eq!(
        calls[0].1.as_deref(),
        Some("Flooded Stone Dungeon Crypt [50x50]"),
        "describe must forward the candidate title, not theme suggestions"
    );
    assert_eq!(
        calls[0].0,
        vec!["crypt", "flooded", "stone"],
        "describe must normalize and merge cached themes with payload tags"
    );
}

#[tokio::test]
async fn test_describe_candidate_with_payload_themes_and_tags() {
    let (catalog, blobs) = in_memory_ports();
    let mock_desc = "Dark underground cave with sharp stalagmites and glowing crystals.";
    let mock_gen = Arc::new(MockDescriptionGen(Ok(mock_desc.to_string())));
    let state = AppState::new(catalog.clone(), blobs.clone())
        .with_description_service(Arc::new(DescriptionService::new(Some(mock_gen))));
    let app = create_router(state);

    let cand = Candidate::with_now(
        "cand_desc_payload",
        Source::Reddit,
        "cand_desc_payload.png",
        (1024, 1024),
    );
    seed_staging(&catalog, &blobs, &cand, b"\x89PNG\r\n\x1a\nfake-image-bytes").await;

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

    let (catalog, blobs) = in_memory_ports();
    let cand = Candidate::with_now("cand_mock_ai", Source::Manual, "cand_mock_ai.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand, &create_test_png()).await;

    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
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

/// Stored candidates carry a relative path (`<id>/0.png`) as their filename. Passing that path
/// straight through as the data-URL MIME type makes upstream providers reject the request with
/// `mimeType parameter with value cand_mock_path/0.png, which is not supported` (502 Bad Gateway).
#[tokio::test]
async fn test_describe_candidate_sends_image_mime_for_path_shaped_filename() {
    use axum::routing::post;
    use tokio::net::TcpListener;

    let recorded_request = Arc::new(std::sync::Mutex::new(None));
    let recorded_req_clone = recorded_request.clone();

    let mock_app = axum::Router::new().route(
        "/chat/completions",
        post(move |axum::Json(body): axum::Json<Value>| {
            *recorded_req_clone.lock().unwrap() = Some(body.clone());
            async move {
                axum::Json(json!({
                    "id": "gen-desc-path",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "Flooded crypt with shattered flagstones and toppled pillars."
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

    let (catalog, blobs) = in_memory_ports();
    // Production staging candidates carry the relative `<id>/<layer>` shape as their filename.
    let cand = Candidate::with_now("cand_mock_path", Source::Reddit, "cand_mock_path/0.png", (1024, 1024));
    seed_staging(&catalog, &blobs, &cand, &create_test_png()).await;
    assert_eq!(
        catalog.get_candidate("cand_mock_path").await.unwrap().resolved_filename(),
        "cand_mock_path/0.png",
        "regression precondition: stored filename is a relative path, not a MIME type"
    );

    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        format!("http://127.0.0.1:{}", addr.port()),
        2,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let app = create_router(state);

    let req = Request::builder()
        .uri("/api/staging/cand_mock_path/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "tags": ["crypt"] }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let recorded = recorded_request.lock().unwrap();
    let req_body = recorded.as_ref().expect("Mock OpenRouter should have received a request");
    let image_url = req_body["messages"][1]["content"][1]["image_url"]["url"]
        .as_str()
        .expect("describe request must attach the candidate image");
    assert!(
        image_url.starts_with("data:image/png;base64,"),
        "image data URL must declare a real image MIME type, got: {}",
        &image_url[..image_url.find(',').unwrap_or(image_url.len()).min(64)]
    );

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
    let (catalog1, blobs1) = in_memory_ports();
    seed_staging(&catalog1, &blobs1, &cand, &create_test_png()).await;
    let adapter_no_key = Arc::new(OpenRouterAdapter::with_config(
        "",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state1 = AppState::new(catalog1, blobs1).with_openrouter(adapter_no_key);
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
    let (catalog2, blobs2) = in_memory_ports();
    seed_staging(&catalog2, &blobs2, &cand, &create_test_png()).await;
    let adapter_offline = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state2 = AppState::new(catalog2, blobs2).with_openrouter(adapter_offline);
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
    let (app, _catalog, _blobs) = setup_test_app();

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
    let (app, catalog, blobs) = setup_test_app();

    let cand = Candidate::with_now(
        "cand_curate_explicit_desc",
        Source::Reddit,
        "cand_curate_explicit_desc.png",
        (1024, 1024),
    );
    seed_staging(&catalog, &blobs, &cand, &create_test_png()).await;

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

    // 2. The catalog manifest persists the description.
    let manifest = catalog.get_curated("cand_curate_explicit_desc").await.unwrap();
    assert_eq!(manifest.description.as_deref(), Some(custom_desc));

    // 3. Primary layer caption matches description
    let curated_list = catalog.list_curated(&CuratedFilter::default()).await.unwrap();
    assert_eq!(curated_list.len(), 1);
    assert_eq!(curated_list[0].description.as_deref(), Some(custom_desc));
    let ground_layer = curated_list[0]
        .layers
        .iter()
        .find(|l| l.role == LayerRole::Ground)
        .expect("Ground layer must be present");
    assert_eq!(ground_layer.caption.as_deref(), Some(custom_desc));

    // 4. No `<id>_lora1024` companion blob exists: samples are derived on demand (ADR-0007).
    let curated_keys = blobs
        .list_prefix(&BlobKey::CuratedDir {
            id: Some("cand_curate_explicit_desc".to_string()),
        })
        .await
        .unwrap();
    assert!(!curated_keys.iter().any(|key| key.to_path().contains("lora1024")));
    assert!(!curated_keys.is_empty());
}

#[tokio::test]
async fn test_curate_candidate_auto_description_fallback_offline() {
    let (catalog, blobs) = in_memory_ports();
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "sk-test-key",
        "http://127.0.0.1:1".to_string(),
        1,
    ));
    let state = AppState::new(catalog.clone(), blobs.clone()).with_openrouter(adapter);
    let app = create_router(state);
    let cand = Candidate::with_now(
        "cand_curate_auto_offline",
        Source::Reddit,
        "cand_curate_auto_offline.png",
        (1024, 1024),
    );
    seed_staging(&catalog, &blobs, &cand, &create_test_png()).await;

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
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 1);

    // Curated manifest must NOT have been committed.
    assert!(catalog.get_curated("cand_curate_auto_offline").await.is_err());
}

#[tokio::test]
async fn test_legacy_manifest_without_description_deserialization() {
    let (app, catalog, blobs) = setup_test_app();

    // A legacy curated manifest carries neither a `description` field nor a schema version.
    let legacy_id = "legacy_cand_no_desc";
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

    let manifest_content = serde_json::to_string(&legacy_manifest_json).unwrap();
    assert!(!manifest_content.contains("\"description\""));
    let map: CuratedMap = serde_json::from_str(&manifest_content)
        .expect("legacy manifest without description must deserialize cleanly");
    assert_eq!(map.id, legacy_id);
    assert_eq!(map.description, None);

    let dummy_image = create_test_png();
    seed_curated(&catalog, &blobs, &map, &dummy_image).await;

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

    let (catalog, blobs) = in_memory_ports();
    let spy = Arc::new(SpyWorkflow {
        curate_calls: AtomicUsize::new(0),
        reject_calls: AtomicUsize::new(0),
    });

    let state = AppState::new(catalog.clone(), blobs.clone()).with_curation_service(spy.clone());
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
    assert_eq!(catalog.list_candidates(&CandidateFilter::default()).await.unwrap().len(), 0);
    assert_eq!(catalog.list_curated(&CuratedFilter::default()).await.unwrap().len(), 0);

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

#[tokio::test]
async fn test_staging_grid_patch_and_curation_persistence() {
    let (app, catalog, blobs) = setup_test_app();

    // Stage candidate with default grid (64, 0, 0)
    let cand = Candidate::with_now("cand_grid_test", Source::Reddit, "cand_grid_test.png", (2560, 2560));
    let dummy_image = create_test_png();
    seed_staging(&catalog, &blobs, &cand, &dummy_image).await;

    // Verify initial candidate grid in storage
    let staging_list = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
    let initial_cand = staging_list.iter().find(|c| c.id == "cand_grid_test").unwrap();
    assert_eq!(initial_cand.grid.cell_size_px, 64);
    assert_eq!(initial_cand.grid.offset_x, 0);
    assert_eq!(initial_cand.grid.offset_y, 0);
    // 1. PATCH /api/staging/{id}/grid updates staging candidate grid
    let grid_payload = json!({
        "cell_size_px": 128,
        "offset_x": 10,
        "offset_y": 20
    });
    let req = Request::builder()
        .uri("/api/staging/cand_grid_test/grid")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(grid_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated_cand: Candidate = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated_cand.grid.cell_size_px, 128);
    assert_eq!(updated_cand.grid.offset_x, 10);
    assert_eq!(updated_cand.grid.offset_y, 20);

    // Verify persisted on disk for staging candidate
    let staging_list = catalog.list_candidates(&CandidateFilter::default()).await.unwrap();
    let persisted_staging = staging_list.iter().find(|c| c.id == "cand_grid_test").unwrap();
    assert_eq!(persisted_staging.grid.cell_size_px, 128);
    assert_eq!(persisted_staging.grid.offset_x, 10);
    assert_eq!(persisted_staging.grid.offset_y, 20);
    // 2. Curation carries candidate grid to curated map
    let curate_payload = json!({
        "id": "cand_grid_test",
        "themes": ["dungeon"],
        "tags": ["crypt", "stone"]
    });
    let req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let curated_list = catalog.list_curated(&CuratedFilter::default()).await.unwrap();
    let curated_map = curated_list.iter().find(|m| m.id == "cand_grid_test").unwrap();
    assert_eq!(curated_map.grid.cell_size_px, 128);
    assert_eq!(curated_map.grid.offset_x, 10);
    assert_eq!(curated_map.grid.offset_y, 20);

    // 3. PATCH /api/curated/{id} updates curated map grid
    let patch_curated_payload = json!({
        "grid": {
            "cell_size_px": 70,
            "offset_x": 5,
            "offset_y": 15
        }
    });
    let req = Request::builder()
        .uri("/api/curated/cand_grid_test")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(patch_curated_payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated_curated: CuratedMap = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated_curated.grid.cell_size_px, 70);
    assert_eq!(updated_curated.grid.offset_x, 5);
    assert_eq!(updated_curated.grid.offset_y, 15);

    // Verify persisted on disk for curated map
    let curated_list = catalog.list_curated(&CuratedFilter::default()).await.unwrap();
    let persisted_curated = curated_list.iter().find(|m| m.id == "cand_grid_test").unwrap();
    assert_eq!(persisted_curated.grid.cell_size_px, 70);
    assert_eq!(persisted_curated.grid.offset_x, 5);
    assert_eq!(persisted_curated.grid.offset_y, 15);
}

#[tokio::test]
async fn test_api_curate_gate_rejects_undersized_grid() {
    let (app, catalog, blobs) = setup_test_app();

    // 1. Single-image candidate with < 16 full cells (e.g. 800x600 px with 64px grid -> 12x9 cells)
    let small_cand = Candidate::with_now("small_map", Source::Reddit, "small.png", (800, 600));
    seed_staging(&catalog, &blobs, &small_cand, b"\x89PNG\r\n\x1a\nsmall").await;

    let curate_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "small_map",
                "theme": "dungeon"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(curate_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json_err: Value = serde_json::from_slice(&body).unwrap();
    let err_msg = json_err["error"].as_str().unwrap();
    assert!(
        err_msg.contains("Candidate Ground layer yields fewer than 16×16 full Grid Cells"),
        "Unexpected error message: {}",
        err_msg
    );
    assert!(
        err_msg.contains("12×9 cells"),
        "Unexpected error message: {}",
        err_msg
    );

    // 2. Candidate with Ground layer having offset that pushes full cells below 16
    // 1024x1024 with 64px is normally 16x16, but offset_x = 10 -> (1024-10)/64 = 15 cols
    let mut offset_cand = Candidate::with_now("offset_map", Source::Reddit, "offset.png", (1024, 1024));
    offset_cand.grid = GridConfig::new(64, 10, 0);
    seed_staging(&catalog, &blobs, &offset_cand, b"\x89PNG\r\n\x1a\noffset").await;

    let offset_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "offset_map",
                "theme": "cave"
            })
            .to_string(),
        ))
        .unwrap();

    let resp_offset = app.clone().oneshot(offset_req).await.unwrap();
    assert_eq!(resp_offset.status(), StatusCode::BAD_REQUEST);
    let body_offset = to_bytes(resp_offset.into_body(), usize::MAX).await.unwrap();
    let json_err_offset: Value = serde_json::from_slice(&body_offset).unwrap();
    assert!(
        json_err_offset["error"].as_str().unwrap().contains("15×16 cells"),
        "Unexpected error: {:?}",
        json_err_offset
    );
}

#[tokio::test]
async fn test_api_curate_gate_respects_payload_layer_role_updates() {
    let (app, catalog, blobs) = setup_test_app();

    let img_large = b"\x89PNG\r\n\x1a\nlarge_layer";
    let img_small = b"\x89PNG\r\n\x1a\nsmall_layer";

    // Candidate initially has:
    // Layer 0: Ground, 1024x1024 (16x16 with 64px) -> eligible
    // Layer 1: UpperFloor, 800x600 (12x9 with 64px) -> ineligible
    let mut cand = Candidate::with_now("role_update_cand", Source::Reddit, "0.png", (1024, 1024));
    cand.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (1024, 1024)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (800, 600)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand, &[img_large.to_vec(), img_small.to_vec()]).await;

    // 1. Curate payload swaps roles: Layer 1 becomes Ground (800x600), Layer 0 becomes UpperFloor.
    // The gate MUST check the updated payload.layers and reject because Ground is 12x9 (<16x16).
    let curate_reject_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "role_update_cand",
                "theme": "dungeon",
                "layers": [
                    {
                        "index": 0,
                        "filename": "0.png",
                        "role": "upper_floor",
                        "elevation": 1,
                        "dimensions": [1024, 1024]
                    },
                    {
                        "index": 1,
                        "filename": "1.png",
                        "role": "ground",
                        "elevation": 0,
                        "dimensions": [800, 600]
                    }
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp_reject = app.clone().oneshot(curate_reject_req).await.unwrap();
    assert_eq!(resp_reject.status(), StatusCode::BAD_REQUEST);
    let body_reject = to_bytes(resp_reject.into_body(), usize::MAX).await.unwrap();
    let json_reject: Value = serde_json::from_slice(&body_reject).unwrap();
    let err_msg = json_reject["error"].as_str().unwrap();
    assert!(
        err_msg.contains("Candidate Ground layer yields fewer than 16×16 full Grid Cells"),
        "Unexpected error message: {}",
        err_msg
    );
    assert!(
        err_msg.contains("12×9 cells"),
        "Expected 12×9 cells in error message, got: {}",
        err_msg
    );

    // 2. Candidate where staging initially has undersized layer as Ground (800x600)
    // and large layer as UpperFloor (1024x1024).
    let mut cand2 = Candidate::with_now("role_update_cand2", Source::Reddit, "0.png", (800, 600));
    cand2.layers = vec![
        CandidateLayer::new(0, "0.png", LayerRole::Ground, Some(0), (800, 600)),
        CandidateLayer::new(1, "1.png", LayerRole::UpperFloor, Some(1), (1024, 1024)),
    ];
    seed_staging_bundle(&catalog, &blobs, &cand2, &[img_small.to_vec(), img_large.to_vec()]).await;

    // Curate payload promotes Layer 1 (1024x1024) to Ground, and demotes Layer 0 to UpperFloor.
    // The gate MUST check payload.layers, see Ground is 1024x1024 (16x16 cells), and accept.
    let curate_accept_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": "role_update_cand2",
                "theme": "dungeon",
                "layers": [
                    {
                        "index": 0,
                        "filename": "0.png",
                        "role": "upper_floor",
                        "elevation": 1,
                        "dimensions": [800, 600]
                    },
                    {
                        "index": 1,
                        "filename": "1.png",
                        "role": "ground",
                        "elevation": 0,
                        "dimensions": [1024, 1024]
                    }
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp_accept = app.oneshot(curate_accept_req).await.unwrap();
    assert_eq!(resp_accept.status(), StatusCode::OK);
    let body_accept = to_bytes(resp_accept.into_body(), usize::MAX).await.unwrap();
    let json_accept: Value = serde_json::from_slice(&body_accept).unwrap();
    assert_eq!(json_accept["status"], "ok");
    assert_eq!(json_accept["curated"]["id"], "role_update_cand2");
}

#[tokio::test]
async fn test_curate_exactly_16x16_produces_single_map_section_reusing_description() {
    let (app, catalog, blobs) = setup_test_app();

    // 16x16 cells at 64px = 1024x1024 px
    let img_bytes = create_test_png();
    let cand = Candidate::with_now(
        "cand_16x16_single",
        Source::Generated,
        "cand_16x16_single.png",
        (1024, 1024),
    );
    seed_staging(&catalog, &blobs, &cand, &img_bytes).await;

    let custom_desc = "Compact 16x16 arcane summoning chamber with inscribed runic circle.";
    let curate_payload = json!({
        "id": "cand_16x16_single",
        "themes": ["dungeon"],
        "tags": ["runes", "circle"],
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
    let res_json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "cand_16x16_single");
    assert_eq!(res_json["curated"]["description"].as_str(), Some(custom_desc));

    // Must produce exactly 1 MapSection at (0, 0) reusing the map description
    let sections = res_json["curated"]["sections"].as_array().expect("sections array exists");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["col"], 0);
    assert_eq!(sections[0]["row"], 0);
    assert_eq!(sections[0]["description"].as_str(), Some(custom_desc));

    // Verify the persisted catalog manifest.
    let curated_manifest = catalog.get_curated("cand_16x16_single").await.unwrap();
    assert_eq!(curated_manifest.sections.len(), 1);
    assert_eq!(curated_manifest.sections[0].col, 0);
    assert_eq!(curated_manifest.sections[0].row, 0);
    assert_eq!(curated_manifest.sections[0].description, custom_desc);
}

#[tokio::test]
async fn test_curate_multi_section_32x32_produces_all_map_sections() {
    let (app, catalog, blobs) = setup_test_app();

    // 32x32 cells at 64px = 2048x2048 px
    // Generate valid 2048x2048 PNG image
    let img = image::RgbaImage::new(2048, 2048);
    let mut img_bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut img_bytes), image::ImageFormat::Png).unwrap();

    let cand = Candidate::with_now(
        "cand_32x32_multi",
        Source::Generated,
        "cand_32x32_multi.png",
        (2048, 2048),
    );
    seed_staging(&catalog, &blobs, &cand, &img_bytes).await;

    let curate_payload = json!({
        "id": "cand_32x32_multi",
        "themes": ["dungeon", "crypt"],
        "tags": ["stone", "sarcophagus"],
        "description": "Massive ancient necropolis spanning multiple wings."
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
    let res_json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["curated"]["id"], "cand_32x32_multi");

    // 32x32 cells -> kx = 2, ky = 2 -> 4 sections: (0, 0), (16, 0), (0, 16), (16, 16)
    let sections = res_json["curated"]["sections"].as_array().expect("sections array exists");
    assert_eq!(sections.len(), 4);

    let origins: Vec<(u64, u64)> = sections
        .iter()
        .map(|s| (s["col"].as_u64().unwrap(), s["row"].as_u64().unwrap()))
        .collect();
    assert_eq!(origins, vec![(0, 0), (16, 0), (0, 16), (16, 16)]);

    for s in sections {
        let desc = s["description"].as_str().unwrap();
        assert!(!desc.trim().is_empty());
        assert!(desc.contains("dungeon") || desc.contains("stone"));
    }

    // Verify the persisted catalog manifest.
    let curated_manifest = catalog.get_curated("cand_32x32_multi").await.unwrap();
    assert_eq!(curated_manifest.sections.len(), 4);
    assert_eq!(curated_manifest.sections[0].col, 0);
    assert_eq!(curated_manifest.sections[0].row, 0);
    assert_eq!(curated_manifest.sections[1].col, 16);
    assert_eq!(curated_manifest.sections[1].row, 0);
    assert_eq!(curated_manifest.sections[2].col, 0);
    assert_eq!(curated_manifest.sections[2].row, 16);
    assert_eq!(curated_manifest.sections[3].col, 16);
    assert_eq!(curated_manifest.sections[3].row, 16);
}

#[tokio::test]
async fn test_patch_curated_grid_validates_curation_gate_and_recomputes_sections() {
    let (app, catalog, blobs) = setup_test_app();

    // 1. Stage and curate a 2048x2048 map with default 64px grid (32x32 cells -> 4 sections)
    let img = image::RgbaImage::new(2048, 2048);
    let mut img_bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut img_bytes), image::ImageFormat::Png).unwrap();

    let cand = Candidate::with_now(
        "map_to_regrid",
        Source::Generated,
        "map_to_regrid.png",
        (2048, 2048),
    );
    seed_staging(&catalog, &blobs, &cand, &img_bytes).await;

    let curate_payload = json!({
        "id": "map_to_regrid",
        "themes": ["dungeon"],
        "tags": ["crypt"],
        "description": "A sprawling subterranean temple complex."
    });
    let curate_req = Request::builder()
        .uri("/api/curate")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(curate_payload.to_string()))
        .unwrap();
    let curate_resp = app.clone().oneshot(curate_req).await.unwrap();
    assert_eq!(curate_resp.status(), StatusCode::OK);

    // Initial curated map has 4 sections
    let curated_list = catalog.list_curated(&CuratedFilter::default()).await.unwrap();
    let initial_curated = curated_list.iter().find(|m| m.id == "map_to_regrid").unwrap();
    assert_eq!(initial_curated.sections.len(), 4);

    // 2. Reject undersized grid configuration (< 16x16 full cells)
    // 2048px with 200px cell size -> 2048 / 200 = 10 cells < 16 -> HTTP 400 Bad Request
    let undersized_payload = json!({
        "grid": {
            "cell_size_px": 200,
            "offset_x": 0,
            "offset_y": 0
        }
    });
    let reject_req = Request::builder()
        .uri("/api/curated/map_to_regrid")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(undersized_payload.to_string()))
        .unwrap();
    let reject_resp = app.clone().oneshot(reject_req).await.unwrap();
    assert_eq!(reject_resp.status(), StatusCode::BAD_REQUEST);
    let reject_body = to_bytes(reject_resp.into_body(), usize::MAX).await.unwrap();
    let reject_json: Value = serde_json::from_slice(&reject_body).unwrap();
    assert!(
        reject_json["error"].as_str().unwrap().contains("fewer than 16×16 full Grid Cells"),
        "Error message must specify fewer than 16x16: {:?}",
        reject_json
    );

    // 3. Update with valid grid yielding exactly 16x16 cells (128px on 2048px -> 16x16 cells)
    // Should recompute sections down from 4 to 1 MapSection at (0, 0) reusing map description
    let valid_regrid_payload = json!({
        "grid": {
            "cell_size_px": 128,
            "offset_x": 0,
            "offset_y": 0
        }
    });
    let valid_req = Request::builder()
        .uri("/api/curated/map_to_regrid")
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_regrid_payload.to_string()))
        .unwrap();
    let valid_resp = app.oneshot(valid_req).await.unwrap();
    assert_eq!(valid_resp.status(), StatusCode::OK);
    let valid_body = to_bytes(valid_resp.into_body(), usize::MAX).await.unwrap();
    let valid_json: Value = serde_json::from_slice(&valid_body).unwrap();

    assert_eq!(valid_json["grid"]["cell_size_px"], 128);
    let updated_sections = valid_json["sections"].as_array().expect("sections array exists");
    assert_eq!(updated_sections.len(), 1);
    assert_eq!(updated_sections[0]["col"], 0);
    assert_eq!(updated_sections[0]["row"], 0);
    assert_eq!(
        updated_sections[0]["description"].as_str(),
        Some("A sprawling subterranean temple complex.")
    );

    // 4. Verify the updated catalog manifest.
    let updated_manifest = catalog.get_curated("map_to_regrid").await.unwrap();
    assert_eq!(updated_manifest.grid.cell_size_px, 128);
    assert_eq!(updated_manifest.sections.len(), 1);
    assert_eq!(updated_manifest.sections[0].col, 0);
    assert_eq!(updated_manifest.sections[0].row, 0);
    assert_eq!(
        updated_manifest.sections[0].description,
        "A sprawling subterranean temple complex."
    );
}
