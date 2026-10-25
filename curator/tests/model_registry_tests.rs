//! Tests for registry-derived model identity and adapter selection (ADR-0010, issue #219).
//!
//! Covers the derived `GET /api/models` catalog, the reference-conditioning capability gate,
//! adapter retirement, registry-driven benchmark routing / cost accounting, and the drawer's
//! dependence on the registry instead of a hardcoded checklist and price table.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use curator::adapters::{
    FileCostLedger, InMemoryBlobStorage, InMemoryCatalogRepository, OpenRouterAdapter,
    RoutingModelEngine, WaveSpeedAdapter,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    BenchmarkArchetype, LoraAdapter, ModelApiType, ModelDescriptor, ModelProvider,
    MODEL_GEMINI_31_FLASH, MODEL_GEMINI_31_FLASH_LITE, MODEL_MUSE, MODEL_SEEDREAM_50_FLASH,
    MODEL_WAVESPEED_KLEIN_EDIT_LORA, MODEL_WAVESPEED_KLEIN_T2I_LORA,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::cost_tracker::CostTracker;
use curator::services::{BenchmarkService, ModelRegistry};

const TEST_PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
    0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const ADAPTER_RUN_ID: &str = "run-219";
const ADAPTER_STEP: usize = 5;
const ADAPTER_SCALE: f64 = 0.8;
const ADAPTER_ID: &str = "run-219-s5";
const ADAPTER_EDIT_ID: &str = "run-219-s5-edit";
const ADAPTER_URL: &str =
    "https://cdn.example.com/artifacts/adapters/run-219-s5.safetensors";

fn checkpoint_key() -> BlobKey {
    BlobKey::CheckpointAdapter {
        run_id: ADAPTER_RUN_ID.to_string(),
        step: ADAPTER_STEP,
    }
}

fn in_memory_ports() -> (Arc<InMemoryCatalogRepository>, Arc<InMemoryBlobStorage>) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

fn sample_adapter() -> LoraAdapter {
    LoraAdapter {
        id: ADAPTER_ID.to_string(),
        run_id: ADAPTER_RUN_ID.to_string(),
        step: ADAPTER_STEP,
        scale: ADAPTER_SCALE,
        base_model: "black-forest-labs/flux-3-klein".to_string(),
        trigger_phrase: "dungeonforge battlemap".to_string(),
        artifact_key: checkpoint_key().to_path(),
        artifact_url: ADAPTER_URL.to_string(),
        sha256: Some("adapter-sha".to_string()),
        size_bytes: Some(2048),
        recipe_digest: "recipe-digest".to_string(),
        dataset_snapshot_id: "snapshot-219".to_string(),
        dataset_digest: "dataset-digest".to_string(),
        created_at: chrono::Utc::now(),
        retired: false,
        retired_at: None,
    }
}

/// Persists an adapter record and its checkpoint weights blob.
async fn seed_published_adapter(
    catalog: &InMemoryCatalogRepository,
    blobs: &InMemoryBlobStorage,
) -> LoraAdapter {
    let adapter = sample_adapter();
    catalog.save_lora_adapter(&adapter).await.unwrap();
    blobs
        .write_blob(&checkpoint_key(), b"checkpoint-weights")
        .await
        .unwrap();
    adapter
}

async fn retire_adapter(catalog: &InMemoryCatalogRepository) {
    let mut adapter = catalog.get_lora_adapter(ADAPTER_ID).await.unwrap();
    adapter.retire();
    catalog.save_lora_adapter(&adapter).await.unwrap();
}

async fn fetch_models_json(app: &axum::Router) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn fetch_models(app: &axum::Router) -> Vec<ModelDescriptor> {
    let (status, body) = fetch_models_json(app).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_value(body).expect("model descriptors deserialize")
}

async fn post_benchmark(app: &axum::Router, payload: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/benchmark/run")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

// ---------------------------------------------------------------------------
// Acceptance 1: GET /api/models derives every selectable entry at read time.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_models_endpoint_derives_openrouter_and_adapter_entries() {
    let (catalog, blobs) = in_memory_ports();
    seed_published_adapter(&catalog, &blobs).await;
    let app = create_router(AppState::new(catalog, blobs));

    let models = fetch_models(&app).await;
    assert_eq!(
        models.len(),
        6,
        "four OpenRouter baselines plus two entries per published adapter"
    );

    // Every entry carries the identity fields the drawer and runner consume.
    for model in &models {
        assert!(!model.id.is_empty());
        assert!(!model.display_name.is_empty());
        assert!(model.estimated_cost_per_image > 0.0);
    }

    let flash = models
        .iter()
        .find(|m| m.id == MODEL_GEMINI_31_FLASH)
        .expect("gemini flash entry");
    assert_eq!(flash.provider, ModelProvider::OpenRouter);
    assert_eq!(flash.api_type, ModelApiType::Chat);
    assert!(flash.has_vision);
    assert!(flash.supports_reference_image);
    assert!(flash.adapter_id.is_none());

    let flash_lite = models
        .iter()
        .find(|m| m.id == MODEL_GEMINI_31_FLASH_LITE)
        .expect("gemini flash lite entry");
    assert_eq!(flash_lite.api_type, ModelApiType::Chat);
    assert!(flash_lite.has_vision);
    assert!(flash_lite.supports_reference_image);

    let muse = models
        .iter()
        .find(|m| m.id == MODEL_MUSE)
        .expect("muse entry");
    assert_eq!(muse.api_type, ModelApiType::Images);
    assert!(muse.has_vision);
    assert!(muse.supports_reference_image);
    assert_eq!(muse.pricing.image_output, 0.01);

    let seedream = models
        .iter()
        .find(|m| m.id == MODEL_SEEDREAM_50_FLASH)
        .expect("seedream entry");
    assert_eq!(seedream.api_type, ModelApiType::Images);
    assert!(seedream.has_vision);
    assert!(seedream.supports_reference_image);

    // Text-to-image engine entry: `{adapter.id}`.
    let t2i = models
        .iter()
        .find(|m| m.id == ADAPTER_ID)
        .expect("adapter text-to-image entry");
    assert_eq!(t2i.provider, ModelProvider::WaveSpeed);
    assert_eq!(t2i.api_type, ModelApiType::Images);
    assert!(!t2i.has_vision);
    assert!(!t2i.supports_reference_image);
    assert_eq!(t2i.pricing.image_output, 0.015);
    assert_eq!(t2i.estimated_cost_per_image, 0.015);
    assert_eq!(t2i.adapter_id.as_deref(), Some(ADAPTER_ID));
    assert_eq!(t2i.run_id.as_deref(), Some(ADAPTER_RUN_ID));
    assert_eq!(t2i.step, Some(ADAPTER_STEP));
    assert_eq!(t2i.lora_path.as_deref(), Some(ADAPTER_URL));
    assert_eq!(t2i.lora_scale, Some(ADAPTER_SCALE));

    // Edit engine entry: `{adapter.id}-edit`.
    let edit = models
        .iter()
        .find(|m| m.id == ADAPTER_EDIT_ID)
        .expect("adapter edit entry");
    assert_eq!(edit.provider, ModelProvider::WaveSpeed);
    assert_eq!(edit.api_type, ModelApiType::Images);
    assert!(edit.has_vision);
    assert!(edit.supports_reference_image);
    assert_eq!(edit.pricing.image_output, 0.021);
    assert_eq!(edit.estimated_cost_per_image, 0.021);
    assert_eq!(edit.adapter_id.as_deref(), Some(ADAPTER_ID));
    assert_eq!(edit.run_id.as_deref(), Some(ADAPTER_RUN_ID));
    assert_eq!(edit.step, Some(ADAPTER_STEP));
    assert_eq!(edit.lora_path.as_deref(), Some(ADAPTER_URL));
    assert_eq!(edit.lora_scale, Some(ADAPTER_SCALE));

    // The wire shape the drawer reads: exact keys, provider and pricing included.
    let (_, raw) = fetch_models_json(&app).await;
    let adapter_json = raw
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == json!(ADAPTER_ID))
        .expect("adapter json entry");
    for key in [
        "id",
        "display_name",
        "provider",
        "api_type",
        "has_vision",
        "supports_reference_image",
        "pricing",
        "estimated_cost_per_image",
        "adapter_id",
        "run_id",
        "step",
        "lora_path",
        "lora_scale",
    ] {
        assert!(
            adapter_json.get(key).is_some(),
            "adapter entry must serialize `{key}`"
        );
    }
    assert_eq!(adapter_json["provider"], json!("wavespeed"));
    assert_eq!(adapter_json["supports_reference_image"], json!(false));
    assert_eq!(adapter_json["pricing"]["image_output"], json!(0.015));
}

// ---------------------------------------------------------------------------
// Acceptance 6: publishing/retiring changes GET /api/models on the next request.
// Acceptance 5: retired entries are absent and unselectable but still resolve for history.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_publish_and_retire_change_models_endpoint_without_restart() {
    let (catalog, blobs) = in_memory_ports();
    let state = AppState::new(catalog.clone(), blobs.clone());
    let registry = state.model_registry.clone();
    let app = create_router(state);

    let baseline = fetch_models(&app).await;
    assert_eq!(baseline.len(), 4, "only the OpenRouter baselines are selectable");

    // Publishing an adapter makes both entries appear on the next request — same process.
    seed_published_adapter(&catalog, &blobs).await;
    let after_publish = fetch_models(&app).await;
    assert_eq!(after_publish.len(), 6);
    assert!(after_publish.iter().any(|m| m.id == ADAPTER_ID));
    assert!(after_publish.iter().any(|m| m.id == ADAPTER_EDIT_ID));

    // Retiring it hides both entries on the next request.
    retire_adapter(&catalog).await;
    let after_retire = fetch_models(&app).await;
    assert_eq!(after_retire.len(), 4);
    assert!(!after_retire.iter().any(|m| m.id == ADAPTER_ID));
    assert!(!after_retire.iter().any(|m| m.id == ADAPTER_EDIT_ID));
    assert!(registry.find(ADAPTER_ID).await.unwrap().is_none());

    // History still resolves a retired entry, engine and adapter metadata intact.
    let historical = registry
        .find_including_retired(ADAPTER_ID)
        .await
        .unwrap()
        .expect("retired adapter resolves for history");
    assert_eq!(historical.provider, ModelProvider::WaveSpeed);
    assert_eq!(historical.lora_path.as_deref(), Some(ADAPTER_URL));
    let historical_edit = registry
        .find_including_retired(ADAPTER_EDIT_ID)
        .await
        .unwrap()
        .expect("retired edit entry resolves for history");
    assert!(historical_edit.supports_reference_image);
}

#[tokio::test]
async fn test_adapter_without_checkpoint_artifact_is_unselectable() {
    let (catalog, blobs) = in_memory_ports();
    // Record the descriptor but never write the checkpoint weights blob.
    catalog
        .save_lora_adapter(&sample_adapter())
        .await
        .unwrap();

    let state = AppState::new(catalog.clone(), blobs.clone());
    let registry = state.model_registry.clone();
    let app = create_router(state);

    let models = fetch_models(&app).await;
    assert_eq!(models.len(), 4);
    assert!(registry.find(ADAPTER_ID).await.unwrap().is_none());

    // Still resolvable for historical results and cost entries.
    assert!(registry
        .find_including_retired(ADAPTER_ID)
        .await
        .unwrap()
        .is_some());
}

// ---------------------------------------------------------------------------
// Acceptance 5: retired/missing-adapter ids are refused for new runs.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_retired_and_unknown_models_refused_for_new_runs() {
    let (catalog, blobs) = in_memory_ports();
    seed_published_adapter(&catalog, &blobs).await;
    retire_adapter(&catalog).await;
    let app = create_router(AppState::new(catalog, blobs));

    let (status, body) = post_benchmark(
        &app,
        json!({ "models": [ADAPTER_ID], "archetypes": ["stone-crypt"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = body["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("retired") || message.contains("unselectable"),
        "retired adapter refusal names the reason, got: {message}"
    );

    let (status, body) = post_benchmark(
        &app,
        json!({ "models": ["no-such-model"], "archetypes": ["stone-crypt"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("Unknown"));

    // An adapter whose weights artifact never landed is equally unselectable.
    let (catalog, blobs) = in_memory_ports();
    catalog
        .save_lora_adapter(&sample_adapter())
        .await
        .unwrap();
    let app = create_router(AppState::new(catalog, blobs));
    let (status, _) = post_benchmark(
        &app,
        json!({ "models": [ADAPTER_ID], "archetypes": ["stone-crypt"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Acceptance 4: reference conditioning is refused for incapable models.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_reference_conditioning_refused_with_model_not_reference_capable() {
    let (catalog, blobs) = in_memory_ports();
    seed_published_adapter(&catalog, &blobs).await;
    let app = create_router(AppState::new(catalog, blobs));

    // The adapter's text-to-image engine entry cannot accept an image.
    let (status, body) = post_benchmark(
        &app,
        json!({
            "models": [ADAPTER_ID],
            "archetypes": ["stone-crypt"],
            "use_reference_conditioning": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], json!("MODEL_NOT_REFERENCE_CAPABLE"));
    assert_eq!(body["model"], json!(ADAPTER_ID));
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains(ADAPTER_ID));

    // The edit engine entry does accept reference conditioning, so the gate must not fire there
    // (the request proceeds to the runner and fails later only because no provider key is set).
    let (status, body) = post_benchmark(
        &app,
        json!({
            "models": [ADAPTER_EDIT_ID],
            "archetypes": ["stone-crypt"],
            "use_reference_conditioning": true,
        }),
    )
    .await;
    assert_ne!(
        body["code"],
        json!("MODEL_NOT_REFERENCE_CAPABLE"),
        "the edit engine entry accepts reference conditioning (status {status})"
    );
}

// ---------------------------------------------------------------------------
// Acceptance 3: `{run_id}-s{step}[-edit]` runs WaveSpeed with that adapter's URL + scale
// and records cost under that exact id.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_benchmark_runner_resolves_adapter_ids_through_registry() {
    let mock_server = MockServer::start().await;

    let t2i_cdn = format!("{}/cdn/t2i.png", mock_server.uri());
    let edit_cdn = format!("{}/cdn/edit.png", mock_server.uri());
    for cdn in [&t2i_cdn, &edit_cdn] {
        Mock::given(method("GET"))
            .and(path(cdn.strip_prefix(&mock_server.uri()).unwrap().to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(TEST_PNG_1X1.to_vec()))
            .mount(&mock_server)
            .await;
    }

    let t2i_body = Arc::new(tokio::sync::Mutex::new(None::<Value>));
    let t2i_capture = t2i_body.clone();
    let t2i_cdn_clone = t2i_cdn.clone();
    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .respond_with(move |req: &wiremock::Request| {
            *t2i_capture.try_lock().unwrap() = serde_json::from_slice(&req.body).ok();
            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": { "id": "t2i-task", "status": "completed", "outputs": [t2i_cdn_clone.as_str()] }
            }))
        })
        .mount(&mock_server)
        .await;

    let edit_body = Arc::new(tokio::sync::Mutex::new(None::<Value>));
    let edit_capture = edit_body.clone();
    let edit_cdn_clone = edit_cdn.clone();
    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_EDIT_LORA)))
        .respond_with(move |req: &wiremock::Request| {
            *edit_capture.try_lock().unwrap() = serde_json::from_slice(&req.body).ok();
            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": { "id": "edit-task", "status": "completed", "outputs": [edit_cdn_clone.as_str()] }
            }))
        })
        .mount(&mock_server)
        .await;

    let (catalog, blobs) = in_memory_ports();
    seed_published_adapter(&catalog, &blobs).await;

    let registry = Arc::new(ModelRegistry::new(catalog.clone(), blobs.clone()));
    let wavespeed = WaveSpeedAdapter::with_config("wsk-test", mock_server.uri(), 2)
        .with_poll_interval(Duration::from_millis(5));
    let routing_engine = Arc::new(
        RoutingModelEngine::from_adapters(OpenRouterAdapter::new("dummy"), wavespeed)
            .with_registry(registry.clone()),
    );

    let ledger_dir = tempfile::tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> =
        Arc::new(FileCostLedger::new(ledger_dir.path().join("cost_ledger.json")));

    let service = BenchmarkService::with_cost_tracker(
        routing_engine,
        cost_tracker.clone(),
        catalog.clone(),
        blobs.clone(),
    )
    .with_model_registry(registry.clone());

    // Text-to-image engine entry.
    let t2i_result = service
        .run_archetype(ADAPTER_ID, &BenchmarkArchetype::StoneCrypt)
        .await
        .expect("t2i adapter benchmark succeeds");
    assert_eq!(t2i_result.metadata.model, ADAPTER_ID);
    assert_eq!(t2i_result.metadata.cost_usd, 0.015);

    let t2i_payload = t2i_body.lock().await.clone().expect("t2i payload captured");
    let loras = t2i_payload["loras"]
        .as_array()
        .expect("loras payload array");
    assert_eq!(loras.len(), 1);
    assert_eq!(loras[0]["path"], json!(ADAPTER_URL));
    assert_eq!(loras[0]["scale"], json!(ADAPTER_SCALE));
    assert!(t2i_payload.get("images").is_none());

    // Edit engine entry.
    let edit_result = service
        .run_archetype(ADAPTER_EDIT_ID, &BenchmarkArchetype::StoneCrypt)
        .await
        .expect("edit adapter benchmark succeeds");
    assert_eq!(edit_result.metadata.model, ADAPTER_EDIT_ID);
    assert_eq!(edit_result.metadata.cost_usd, 0.021);

    let edit_payload = edit_body.lock().await.clone().expect("edit payload captured");
    let edit_loras = edit_payload["loras"]
        .as_array()
        .expect("edit loras payload array");
    assert_eq!(edit_loras[0]["path"], json!(ADAPTER_URL));
    assert_eq!(edit_loras[0]["scale"], json!(ADAPTER_SCALE));
    assert!(
        edit_payload["images"]
            .as_array()
            .is_some_and(|images| !images.is_empty()),
        "the edit engine receives a reference image"
    );

    // Cost is recorded under the exact registry ids.
    let stats = cost_tracker.get_stats().await.unwrap();
    let t2i_entry = stats
        .by_model
        .get(ADAPTER_ID)
        .expect("t2i cost recorded under the adapter id");
    assert_eq!(t2i_entry.generation_count, 1);
    assert!((t2i_entry.spend_usd - 0.015).abs() < 1e-9);
    let edit_entry = stats
        .by_model
        .get(ADAPTER_EDIT_ID)
        .expect("edit cost recorded under the adapter id");
    assert_eq!(edit_entry.generation_count, 1);
    assert!((edit_entry.spend_usd - 0.021).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Acceptance 2: the drawer derives its checklist and price table from the registry.
// ---------------------------------------------------------------------------

#[test]
fn test_drawer_assets_no_longer_hardcode_the_model_catalog() {
    let html = include_str!("../src/ui/assets/index.html");
    let js = include_str!("../src/ui/assets/app.js");

    assert!(
        !html.contains("name=\"benchmark-model\" value="),
        "the model checklist must be rendered from GET /api/models, not shipped in markup"
    );
    assert!(
        !js.contains("MODEL_PRICES"),
        "the pre-flight price table must come from the registry, not a hardcoded map"
    );
    assert!(
        js.contains("/api/models"),
        "the drawer fetches its checklist from GET /api/models"
    );
    assert!(
        js.contains("supports_reference_image"),
        "the drawer gates reference conditioning on the registry capability flag"
    );
    assert!(
        js.contains("benchmark-reference-warning"),
        "the drawer surfaces an inline warning for reference-incapable models"
    );
}
