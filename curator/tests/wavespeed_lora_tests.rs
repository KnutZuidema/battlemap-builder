//! Integration tests for WaveSpeed LoRA inference engine, RoutingModelEngine, and Benchmark integration.
//!
//! Tests verify:
//! - WaveSpeed request payload shape (`loras: [{ path, scale }]`, `enable_prompt_expansion: false`, Trigger Phrase + Physical Description).
//! - Asynchronous polling lifecycle and raster image byte extraction.
//! - Flat per-image cost calculations ($0.015 for t2i-lora, $0.021 for edit-lora).
//! - Upstream HTTP error propagation (401 Auth, 402/429 Quota/RateLimit, 500 Api, task failures).
//! - `RoutingModelEngine` provider routing (OpenRouter vs WaveSpeed).
//! - `BenchmarkService` integration with cost ledger recording and staging candidate persistence.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use curator::adapters::{
    FileCostLedger, InMemoryBlobStorage, InMemoryCatalogRepository, OpenRouterAdapter,
    RoutingModelEngine, WaveSpeedAdapter, DEFAULT_KLEIN_LORA_SCALE, DEFAULT_KLEIN_LORA_URL,
};
use curator::domain::{
    BenchmarkArchetype, GenerationRequest, ReferenceImage, MODEL_GEMINI_31_FLASH,
    MODEL_WAVESPEED_KLEIN_EDIT_LORA, MODEL_WAVESPEED_KLEIN_T2I_LORA, WAVESPEED_TRIGGER_PHRASE,
};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use curator::ports::cost_tracker::CostTracker;
use curator::ports::model_engine::{ModelEngine, ModelEngineError};
use curator::services::BenchmarkService;

const TEST_PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
    0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[tokio::test]
async fn test_wavespeed_payload_shape_and_polling_lifecycle() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/generated_map.png", mock_server.uri());
    let poll_url = format!("{}/api/v3/predictions/task-t2i-456/result", mock_server.uri());

    // 1. Mock CDN image download
    Mock::given(method("GET"))
        .and(path("/cdn/generated_map.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(TEST_PNG_1X1.to_vec())
                .insert_header("content-type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    // 2. Mock polling responses (1st: processing, 2nd: completed)
    let poll_counter = Arc::new(AtomicUsize::new(0));
    let poll_counter_clone = poll_counter.clone();
    let cdn_url_clone = cdn_url.clone();

    Mock::given(method("GET"))
        .and(path("/api/v3/predictions/task-t2i-456/result"))
        .and(header("authorization", "Bearer wsk_test_12345"))
        .respond_with(move |_: &Request| {
            let count = poll_counter_clone.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                ResponseTemplate::new(200).set_body_json(json!({
                    "code": 200,
                    "message": "success",
                    "data": {
                        "id": "task-t2i-456",
                        "status": "processing",
                        "outputs": [],
                        "timings": { "inference": 1500 }
                    }
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "code": 200,
                    "message": "success",
                    "data": {
                        "id": "task-t2i-456",
                        "status": "completed",
                        "outputs": [cdn_url_clone.as_str()],
                        "timings": { "inference": 3200 }
                    }
                }))
            }
        })
        .mount(&mock_server)
        .await;

    // 3. Mock initial task submission with assertion on request body
    let captured_request = Arc::new(tokio::sync::Mutex::new(None));
    let captured_clone = captured_request.clone();
    let poll_url_clone = poll_url.clone();

    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .and(header("authorization", "Bearer wsk_test_12345"))
        .and(header("content-type", "application/json"))
        .respond_with(move |req: &Request| {
            let body_val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_clone.try_lock().unwrap() = Some(body_val);

            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": {
                    "id": "task-t2i-456",
                    "status": "created",
                    "urls": {
                        "get": poll_url_clone.as_str()
                    }
                }
            }))
        })
        .mount(&mock_server)
        .await;

    // Instantiate WaveSpeedAdapter targeting mock server
    let adapter = WaveSpeedAdapter::with_config(
        "wsk_test_12345",
        mock_server.uri(),
        2,
    )
    .with_poll_interval(Duration::from_millis(10));

    // Execute generation with Stone Crypt archetype prompt
    let req = GenerationRequest::new(
        MODEL_WAVESPEED_KLEIN_T2I_LORA,
        BenchmarkArchetype::StoneCrypt.prompt(),
    );

    let res = adapter.generate_image(&req).await.expect("generation success");

    // Verify response
    assert_eq!(res.model, MODEL_WAVESPEED_KLEIN_T2I_LORA);
    assert_eq!(res.image_bytes, TEST_PNG_1X1);
    assert_eq!(res.cost_usd, 0.015);
    assert!(res.prompt_tokens.is_none());
    assert!(res.completion_tokens.is_none());
    assert!(res.latency_ms > 0);

    // Verify submitted request body
    let sent_body = captured_request.lock().await.clone().expect("body captured");
    assert_eq!(sent_body["aspect_ratio"], "1:1");
    assert_eq!(sent_body["resolution"], "1k");
    assert_eq!(sent_body["output_format"], "png");
    assert_eq!(sent_body["enable_prompt_expansion"], false);

    // Prompt MUST be full-scene caption starting with Trigger Phrase verbatim
    let prompt_str = sent_body["prompt"].as_str().expect("prompt string");
    assert!(
        prompt_str.starts_with(WAVESPEED_TRIGGER_PHRASE),
        "Prompt does not start with Trigger Phrase: {prompt_str}"
    );
    assert!(
        prompt_str.contains("Flat 2D stone dungeon crypt"),
        "Prompt missing physical description: {prompt_str}"
    );

    // Default LoRA weights supplied
    let loras = sent_body["loras"].as_array().expect("loras array");
    assert_eq!(loras.len(), 1);
    assert_eq!(loras[0]["path"], DEFAULT_KLEIN_LORA_URL);
    assert_eq!(loras[0]["scale"], DEFAULT_KLEIN_LORA_SCALE);
}

#[tokio::test]
async fn test_wavespeed_custom_lora_path_and_scale() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/custom_lora.png", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/cdn/custom_lora.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(TEST_PNG_1X1.to_vec())
                .insert_header("content-type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    let captured_body = Arc::new(tokio::sync::Mutex::new(None));
    let captured_clone = captured_body.clone();

    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .respond_with(move |req: &Request| {
            let body_val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_clone.try_lock().unwrap() = Some(body_val);

            // Synchronous completed return
            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": {
                    "id": "task-sync-1",
                    "status": "completed",
                    "outputs": [cdn_url.as_str()]
                }
            }))
        })
        .mount(&mock_server)
        .await;

    let adapter = WaveSpeedAdapter::with_config("key", mock_server.uri(), 2);

    let req = GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "underground tavern")
        .with_lora("https://pub-r2.dev/artifacts/run-001-step-1500.safetensors", 0.85);

    let res = adapter.generate_image(&req).await.unwrap();
    assert_eq!(res.cost_usd, 0.015);

    let body = captured_body.lock().await.clone().unwrap();
    let loras = body["loras"].as_array().unwrap();
    assert_eq!(loras.len(), 1);
    assert_eq!(loras[0]["path"], "https://pub-r2.dev/artifacts/run-001-step-1500.safetensors");
    assert_eq!(loras[0]["scale"], 0.85);
}

#[tokio::test]
async fn test_wavespeed_edit_lora_pricing_and_images_payload() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/edit_result.png", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/cdn/edit_result.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(TEST_PNG_1X1.to_vec())
                .insert_header("content-type", "image/png"),
        )
        .mount(&mock_server)
        .await;

    let captured_body = Arc::new(tokio::sync::Mutex::new(None));
    let captured_clone = captured_body.clone();

    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_EDIT_LORA)))
        .respond_with(move |req: &Request| {
            let body_val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_clone.try_lock().unwrap() = Some(body_val);

            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": {
                    "id": "task-edit-99",
                    "status": "completed",
                    "outputs": [cdn_url.as_str()]
                }
            }))
        })
        .mount(&mock_server)
        .await;

    let adapter = WaveSpeedAdapter::with_config("key", mock_server.uri(), 2);

    let req = GenerationRequest::new(MODEL_WAVESPEED_KLEIN_EDIT_LORA, "Extend northern corridor")
        .with_image(ReferenceImage::png("Northern Room", TEST_PNG_1X1.to_vec()));

    let res = adapter.generate_image(&req).await.unwrap();

    // Published per-image price for edit-lora is $0.021 (ADR-0008, ADR-0010)
    assert_eq!(res.cost_usd, 0.021);

    let body = captured_body.lock().await.clone().unwrap();
    let images = body["images"].as_array().expect("images array present");
    assert_eq!(images.len(), 1);
    assert!(images[0].as_str().unwrap().starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn test_wavespeed_http_error_propagation() {
    let mock_server = MockServer::start().await;
    // Model identity is registry-derived; every WaveSpeed request targets the engine
    // endpoint, not the selectable model id.
    let endpoint = format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA);

    // 1. 401 Unauthorized
    Mock::given(method("POST"))
        .and(path(endpoint.as_str()))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"code": 401, "message": "Invalid API key."})),
        )
        .mount(&mock_server)
        .await;

    let adapter = WaveSpeedAdapter::with_config("invalid_key", mock_server.uri(), 2);
    let err = adapter
        .generate_image(&GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ModelEngineError::Auth(msg) if msg.contains("401")));
    mock_server.reset().await;

    // 2. 402 Insufficient Quota
    Mock::given(method("POST"))
        .and(path(endpoint.as_str()))
        .respond_with(
            ResponseTemplate::new(402)
                .set_body_json(json!({"code": 402, "message": "insufficient_quota: credits exhausted"})),
        )
        .mount(&mock_server)
        .await;

    let err = adapter
        .generate_image(&GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ModelEngineError::QuotaOrRateLimit(msg) if msg.contains("402")));
    mock_server.reset().await;

    // 3. 429 Rate Limit
    Mock::given(method("POST"))
        .and(path(endpoint.as_str()))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({"code": 429, "message": "Too many requests"})),
        )
        .mount(&mock_server)
        .await;

    let err = adapter
        .generate_image(&GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ModelEngineError::QuotaOrRateLimit(msg) if msg.contains("429")));
    mock_server.reset().await;

    // 4. 500 Upstream Server Error
    Mock::given(method("POST"))
        .and(path(endpoint.as_str()))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_string("Internal Gateway Error"),
        )
        .mount(&mock_server)
        .await;

    let err = adapter
        .generate_image(&GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ModelEngineError::Api { status: 500, ref message } if message.contains("Internal Gateway Error")));
    mock_server.reset().await;

    // 5. Task Polling Failure (status: "failed")
    Mock::given(method("POST"))
        .and(path(endpoint.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "id": "task-fail-1",
                "status": "created",
                "urls": {
                    "get": format!("{}/api/v3/predictions/task-fail-1/result", mock_server.uri())
                }
            }
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v3/predictions/task-fail-1/result"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "id": "task-fail-1",
                "status": "failed",
                "error": "Out of memory on GPU instance",
                "outputs": []
            }
        })))
        .mount(&mock_server)
        .await;

    let adapter_poll = adapter.clone().with_poll_interval(Duration::from_millis(5));
    let err = adapter_poll
        .generate_image(&GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ModelEngineError::Api { status: 500, ref message } if message.contains("Out of memory")));
}

#[tokio::test]
async fn test_routing_model_engine_dispatches_by_provider() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/ws_map.png", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/cdn/ws_map.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(TEST_PNG_1X1.to_vec()))
        .mount(&mock_server)
        .await;

    // WaveSpeed mock
    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "id": "ws-1",
                "status": "completed",
                "outputs": [cdn_url.as_str()]
            }
        })))
        .mount(&mock_server)
        .await;

    // OpenRouter chat mock
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "![Generated Map](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==)"
                }
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 200
            }
        })))
        .mount(&mock_server)
        .await;

    let openrouter = OpenRouterAdapter::with_config("sk-test", format!("{}/api/v1", mock_server.uri()), 2);
    let wavespeed = WaveSpeedAdapter::with_config("wsk-test", mock_server.uri(), 2);
    let routing_engine = RoutingModelEngine::from_adapters(openrouter, wavespeed);

    // 1. Dispatch OpenRouter model
    let or_req = GenerationRequest::new(MODEL_GEMINI_31_FLASH, "dungeon crypt");
    let or_res = routing_engine.generate_image(&or_req).await.expect("OpenRouter success");
    assert_eq!(or_res.model, MODEL_GEMINI_31_FLASH);
    assert_eq!(or_res.prompt_tokens, Some(100));

    // 2. Dispatch WaveSpeed model
    let ws_req = GenerationRequest::new(MODEL_WAVESPEED_KLEIN_T2I_LORA, "dungeon crypt");
    let ws_res = routing_engine.generate_image(&ws_req).await.expect("WaveSpeed success");
    assert_eq!(ws_res.model, MODEL_WAVESPEED_KLEIN_T2I_LORA);
    assert_eq!(ws_res.cost_usd, 0.015);
    assert!(ws_res.prompt_tokens.is_none());

    // 3. Suite execution across mixed models
    let mixed_models = vec![
        MODEL_GEMINI_31_FLASH.to_string(),
        MODEL_WAVESPEED_KLEIN_T2I_LORA.to_string(),
    ];
    let archetypes = vec![BenchmarkArchetype::StoneCrypt];

    let suite_results = routing_engine
        .run_benchmark_suite(&mixed_models, &archetypes)
        .await
        .expect("suite success");

    assert_eq!(suite_results.len(), 2);
    let models_ran: Vec<&str> = suite_results.iter().map(|r| r.metadata.model.as_str()).collect();
    assert!(models_ran.contains(&MODEL_GEMINI_31_FLASH));
    assert!(models_ran.contains(&MODEL_WAVESPEED_KLEIN_T2I_LORA));
}

#[tokio::test]
async fn test_benchmark_service_wavespeed_persistence_and_cost_ledger() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/battlemap.png", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/cdn/battlemap.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(TEST_PNG_1X1.to_vec()))
        .mount(&mock_server)
        .await;

    let captured_body = Arc::new(tokio::sync::Mutex::new(None));
    let captured_clone = captured_body.clone();

    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .respond_with(move |req: &Request| {
            let body_val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_clone.try_lock().unwrap() = Some(body_val);

            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": {
                    "id": "bench-ws-task-1",
                    "status": "completed",
                    "outputs": [cdn_url.as_str()]
                }
            }))
        })
        .mount(&mock_server)
        .await;

    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let temp_ledger_dir = tempfile::tempdir().unwrap();
    let ledger_path = temp_ledger_dir.path().join("cost_ledger.json");
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> =
        Arc::new(FileCostLedger::new(&ledger_path));

    let wavespeed = WaveSpeedAdapter::with_config("wsk-test", mock_server.uri(), 2);
    let openrouter = OpenRouterAdapter::new("dummy");
    let routing_engine = Arc::new(RoutingModelEngine::from_adapters(openrouter, wavespeed));

    let service = BenchmarkService::with_cost_tracker(
        routing_engine,
        cost_tracker.clone(),
        catalog.clone(),
        blobs.clone(),
    );

    // Run Stone Crypt archetype on WaveSpeed Klein LoRA model
    let result = service
        .run_and_persist(
            MODEL_WAVESPEED_KLEIN_T2I_LORA,
            &BenchmarkArchetype::StoneCrypt,
        )
        .await
        .expect("benchmark run and persist success");

    // 1. Verify result cost is $0.015
    assert_eq!(result.metadata.cost_usd, 0.015);
    assert_eq!(result.metadata.model, MODEL_WAVESPEED_KLEIN_T2I_LORA);

    // 2. Verify candidate persisted to staging catalog
    let candidates = catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .expect("list candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, result.metadata.id);
    assert_eq!(
        candidates[0].source_url.as_deref(),
        Some(format!("wavespeed://{}", MODEL_WAVESPEED_KLEIN_T2I_LORA).as_str())
    );

    // 3. Verify recorded in cost ledger
    let stats = cost_tracker.get_stats().await.expect("get cost stats");
    assert_eq!(stats.total_generations, 1);
    assert!((stats.total_spend_usd - 0.015).abs() < 1e-6);

    let by_model = stats.by_model.get(MODEL_WAVESPEED_KLEIN_T2I_LORA).expect("by_model entry");
    assert_eq!(by_model.generation_count, 1);
    assert!((by_model.spend_usd - 0.015).abs() < 1e-6);

    // 4. Verify prompt sent was formatted with trigger phrase and host prompt expansion disabled
    let body = captured_body.lock().await.clone().unwrap();
    assert_eq!(body["enable_prompt_expansion"], false);
    let prompt_str = body["prompt"].as_str().unwrap();
    assert!(prompt_str.starts_with(WAVESPEED_TRIGGER_PHRASE));
    assert!(prompt_str.contains("Flat 2D stone dungeon crypt"));
}

#[tokio::test]
async fn test_reference_conditioning_gating_by_model_capability() {
    let mock_server = MockServer::start().await;

    let cdn_url = format!("{}/cdn/edit_ref.png", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/cdn/edit_ref.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(TEST_PNG_1X1.to_vec()))
        .mount(&mock_server)
        .await;

    let captured_t2i_body = Arc::new(tokio::sync::Mutex::new(None));
    let captured_t2i_clone = captured_t2i_body.clone();
    let cdn_url_t2i = cdn_url.clone();
    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_T2I_LORA)))
        .respond_with(move |req: &Request| {
            let val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_t2i_clone.try_lock().unwrap() = Some(val);
            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": { "id": "t2i-ref-1", "status": "completed", "outputs": [cdn_url_t2i.as_str()] }
            }))
        })
        .mount(&mock_server)
        .await;

    let captured_edit_body = Arc::new(tokio::sync::Mutex::new(None));
    let captured_edit_clone = captured_edit_body.clone();

    Mock::given(method("POST"))
        .and(path(format!("/api/v3/{}", MODEL_WAVESPEED_KLEIN_EDIT_LORA)))
        .respond_with(move |req: &Request| {
            let val: Value = serde_json::from_slice(&req.body).unwrap();
            *captured_edit_clone.try_lock().unwrap() = Some(val);
            ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "message": "success",
                "data": { "id": "edit-ref-1", "status": "completed", "outputs": [cdn_url.as_str()] }
            }))
        })
        .mount(&mock_server)
        .await;

    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> =
        Arc::new(FileCostLedger::new(tempfile::tempdir().unwrap().path().join("cost.json")));

    let wavespeed = WaveSpeedAdapter::with_config("wsk-test", mock_server.uri(), 2);
    let routing_engine = Arc::new(RoutingModelEngine::from_adapters(
        OpenRouterAdapter::new("dummy"),
        wavespeed,
    ));

    let service = BenchmarkService::with_cost_tracker(
        routing_engine,
        cost_tracker,
        catalog,
        blobs,
    )
    .with_reference_conditioning(true);

    // 1. Run t2i-lora: supports_reference_image is false (ADR-0010)
    // Request payload MUST NOT include an "images" field
    let _ = service
        .run_archetype(
            MODEL_WAVESPEED_KLEIN_T2I_LORA,
            &BenchmarkArchetype::StoneCrypt,
        )
        .await
        .unwrap();

    let t2i_body = captured_t2i_body.lock().await.clone().unwrap();
    assert!(
        t2i_body.get("images").is_none(),
        "t2i-lora must not receive reference images even when conditioning is enabled"
    );
}

#[test]
fn test_prompt_extraction_and_formatting() {
    assert_eq!(
        WaveSpeedAdapter::extract_physical_description(
            "Details: Ancient mossy crypt. Perspective: Strict 2D architectural floor plan cut..."
        ),
        "Ancient mossy crypt"
    );

    assert_eq!(
        WaveSpeedAdapter::extract_physical_description("Plain user prompt without wrapper"),
        "Plain user prompt without wrapper"
    );

    assert_eq!(
        WaveSpeedAdapter::format_full_scene_prompt(
            "Details: Ancient mossy crypt. Perspective: Strict 2D architectural floor plan cut..."
        ),
        "dungeonforge battlemap, Ancient mossy crypt"
    );

    assert_eq!(
        WaveSpeedAdapter::format_full_scene_prompt("dungeonforge battlemap, already prefixed"),
        "dungeonforge battlemap, already prefixed"
    );
}
