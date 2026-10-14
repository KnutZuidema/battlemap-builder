//! Integration tests for OpenRouter Benchmark Engine, Worker Pool, and Persistence.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;

use curator::adapters::{FilesystemStorage, OpenRouterAdapter};
use curator::domain::{
    BenchmarkArchetype, BenchmarkMetadata, GenerationRequest, ModelApiType,
    ALL_BENCHMARK_MODELS, MODEL_GEMINI_31_FLASH, MODEL_GEMINI_31_FLASH_LITE, MODEL_MUSE,
    MODEL_SEEDREAM_50_FLASH,
};
use curator::ports::model_engine::{ModelEngine, ModelEngineError};
use curator::ports::storage::StorageRepository;
use curator::services::BenchmarkService;

const TEST_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[derive(Clone)]
struct MockServerState {
    in_flight: Arc<AtomicUsize>,
    peak_in_flight: Arc<AtomicUsize>,
    chat_requests: Arc<AtomicUsize>,
    images_requests: Arc<AtomicUsize>,
}

impl MockServerState {
    fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            chat_requests: Arc::new(AtomicUsize::new(0)),
            images_requests: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn enter(&self) {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(current, Ordering::SeqCst);
    }

    fn exit(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn handle_chat_completions(
    State(state): State<MockServerState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.enter();
    state.chat_requests.fetch_add(1, Ordering::SeqCst);

    // Sleep briefly to simulate network/GPU inference latency and test concurrency limits
    tokio::time::sleep(Duration::from_millis(50)).await;

    state.exit();

    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");

    let response = json!({
        "id": "gen-chat-12345",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": format!("Map for {model}"),
                    "images": [
                        format!("data:image/png;base64,{TEST_PNG_BASE64}")
                    ]
                }
            }
        ],
        "usage": {
            "prompt_tokens": 120,
            "completion_tokens": 480,
            "total_tokens": 600,
            "cost": 0.035
        }
    });

    (StatusCode::OK, Json(response))
}

async fn handle_images(
    State(state): State<MockServerState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.enter();
    state.images_requests.fetch_add(1, Ordering::SeqCst);

    tokio::time::sleep(Duration::from_millis(50)).await;

    state.exit();

    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");
    let cost = if model.contains("seedream") { 0.018 } else { 0.01 };

    let response = json!({
        "data": [
            {
                "b64_json": TEST_PNG_BASE64
            }
        ],
        "usage": {
            "cost": cost
        }
    });

    (StatusCode::OK, Json(response))
}

async fn spawn_mock_openrouter_server() -> (String, MockServerState) {
    let state = MockServerState::new();
    let app = Router::new()
        .route("/chat/completions", post(handle_chat_completions))
        .route("/images", post(handle_images))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound ephemeral port");
    let addr = listener.local_addr().expect("local addr resolved");
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server serves");
    });

    (base_url, state)
}

#[tokio::test]
async fn test_worker_pool_bounded_concurrency_max_two() {
    let (base_url, state) = spawn_mock_openrouter_server().await;
    let adapter = OpenRouterAdapter::with_config("test_api_key", &base_url, 2);

    assert_eq!(adapter.concurrency_limit(), 2);
    assert_eq!(adapter.available_permits(), 2);

    // Fire 6 simultaneous requests through the adapter
    let mut handles = Vec::new();
    for i in 0..6 {
        let adapter_clone = adapter.clone();
        let prompt = format!("Stone Crypt map {i}");
        handles.push(tokio::spawn(async move {
            let req = GenerationRequest::new(MODEL_GEMINI_31_FLASH, prompt);
            adapter_clone.generate_image(&req).await
        }));
    }

    for handle in handles {
        let res = handle.await.unwrap();
        assert!(res.is_ok(), "request succeeded");
    }

    // Verify peak concurrency strictly never exceeded 2!
    let peak = state.peak_in_flight.load(Ordering::SeqCst);
    assert!(
        peak <= 2,
        "Peak concurrency should be <= 2, but observed: {peak}"
    );
    assert!(peak >= 1, "Peak concurrency should be at least 1");
    assert_eq!(adapter.available_permits(), 2, "All permits returned to pool");
}

#[tokio::test]
async fn test_chat_completions_endpoint_gemini_models() {
    let (base_url, state) = spawn_mock_openrouter_server().await;
    let adapter = OpenRouterAdapter::with_config("test_api_key", &base_url, 2);

    let req = GenerationRequest::new(
        MODEL_GEMINI_31_FLASH,
        "Flat 2D stone dungeon crypt, central sarcophagus, poché masonry walls, iron torches, gridless.",
    );

    let res = adapter
        .generate_image(&req)
        .await
        .expect("generation succeeded");

    assert_eq!(res.model, MODEL_GEMINI_31_FLASH);
    assert!(!res.image_bytes.is_empty());
    assert_eq!(res.prompt_tokens, Some(120));
    assert_eq!(res.completion_tokens, Some(480));
    assert_eq!(res.cost_usd, 0.035);
    assert!(res.latency_ms > 0);

    assert_eq!(state.chat_requests.load(Ordering::SeqCst), 1);
    assert_eq!(state.images_requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_images_endpoint_muse_and_seedream_models() {
    let (base_url, state) = spawn_mock_openrouter_server().await;
    let adapter = OpenRouterAdapter::with_config("test_api_key", &base_url, 2);

    // Muse
    let muse_req = GenerationRequest::new(
        MODEL_MUSE,
        "Flat 2D fantasy tavern interior, wooden tables, hearth fireplace, bar counter, timber poché walls, gridless.",
    );
    let muse_res = adapter
        .generate_image(&muse_req)
        .await
        .expect("muse generation succeeded");
    assert_eq!(muse_res.model, MODEL_MUSE);
    assert_eq!(muse_res.cost_usd, 0.01);

    // Seedream
    let seedream_req = GenerationRequest::new(
        MODEL_SEEDREAM_50_FLASH,
        "Flat 2D forest river crossing, wooden footbridge, rocky banks, dirt path, gridless.",
    );
    let seedream_res = adapter
        .generate_image(&seedream_req)
        .await
        .expect("seedream generation succeeded");
    assert_eq!(seedream_res.model, MODEL_SEEDREAM_50_FLASH);
    assert_eq!(seedream_res.cost_usd, 0.018);

    assert_eq!(state.images_requests.load(Ordering::SeqCst), 2);
}

#[test]
fn test_standard_benchmark_archetypes_suite() {
    let archetypes = BenchmarkArchetype::ALL;
    assert_eq!(archetypes.len(), 6);

    let expected_names = [
        "Stone Crypt",
        "Riverside Ambush",
        "Tavern Common Room",
        "Underdark Cavern",
        "Desert Ruins",
        "Sewers & Sluice",
    ];

    for (arch, &expected) in archetypes.iter().zip(expected_names.iter()) {
        assert_eq!(arch.name(), expected);
        assert!(arch.prompt().contains("Flat 2D"));
        assert!(arch.prompt().contains("gridless"));
        assert!(!arch.slug().is_empty());
    }
}

#[test]
fn test_target_models_catalog() {
    assert_eq!(ALL_BENCHMARK_MODELS.len(), 4);
    assert!(ALL_BENCHMARK_MODELS.contains(&MODEL_GEMINI_31_FLASH));
    assert!(ALL_BENCHMARK_MODELS.contains(&MODEL_GEMINI_31_FLASH_LITE));
    assert!(ALL_BENCHMARK_MODELS.contains(&MODEL_MUSE));
    assert!(ALL_BENCHMARK_MODELS.contains(&MODEL_SEEDREAM_50_FLASH));

    assert_eq!(
        OpenRouterAdapter::endpoint_type_for_model(MODEL_GEMINI_31_FLASH),
        ModelApiType::Chat
    );
    assert_eq!(
        OpenRouterAdapter::endpoint_type_for_model(MODEL_MUSE),
        ModelApiType::Images
    );
}

#[tokio::test]
async fn test_benchmark_persistence_to_staging_generated() {
    let (base_url, _) = spawn_mock_openrouter_server().await;
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_api_key",
        &base_url,
        2,
    ));

    let temp_dir = TempDir::new().unwrap();
    let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
    let service = BenchmarkService::new(adapter, storage.clone());

    // Run single benchmark with persistence
    let bench_result = service
        .run_and_persist(MODEL_GEMINI_31_FLASH, &BenchmarkArchetype::StoneCrypt)
        .await
        .expect("benchmark execution and persistence succeeds");

    let generated_dir = temp_dir.path().join("staging").join("generated");
    let bundle_dir = generated_dir.join(&bench_result.metadata.id);
    let png_path = if bundle_dir.join("0.png").is_file() {
        bundle_dir.join("0.png")
    } else {
        generated_dir.join(format!("{}.png", bench_result.metadata.id))
    };
    let json_path = if bundle_dir.join("manifest.json").is_file() {
        bundle_dir.join("manifest.json")
    } else {
        generated_dir.join(format!("{}.json", bench_result.metadata.id))
    };

    assert!(png_path.is_file(), "PNG output exists in data/staging/generated");
    assert!(json_path.is_file(), "JSON metadata exists in data/staging/generated");
    // Read companion JSON and verify fields match specifications
    let json_str = std::fs::read_to_string(&json_path).expect("read metadata json");
    let meta: BenchmarkMetadata = serde_json::from_str(&json_str).expect("parsed benchmark metadata");

    assert_eq!(meta.id, bench_result.metadata.id);
    assert_eq!(meta.model, MODEL_GEMINI_31_FLASH);
    assert_eq!(meta.prompt_name, "Stone Crypt");
    assert_eq!(
        meta.prompt_text,
        "Flat 2D stone dungeon crypt, central sarcophagus, poché masonry walls, iron torches, gridless."
    );
    assert_eq!(meta.prompt_tokens, Some(120));
    assert_eq!(meta.completion_tokens, Some(480));
    assert_eq!(meta.cost_usd, 0.035);
    assert!(meta.latency_ms > 0);

    // Verify storage list_staging discovers it as Candidate with Source::Generated
    let staged = storage.list_staging().expect("list staging succeeds");
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].id, bench_result.metadata.id);
    assert_eq!(staged[0].source, curator::domain::Source::Generated);

    // Verify storage get_staging_image retrieves the bytes
    let image_bytes = storage
        .get_staging_image(&bench_result.metadata.id)
        .expect("retrieved staged image bytes");
    assert_eq!(image_bytes, bench_result.image_bytes);
}

#[tokio::test]
async fn test_benchmark_suite_execution_and_persistence() {
    let (base_url, _) = spawn_mock_openrouter_server().await;
    let adapter = Arc::new(OpenRouterAdapter::with_config(
        "test_api_key",
        &base_url,
        2,
    ));

    let temp_dir = TempDir::new().unwrap();
    let storage = Arc::new(FilesystemStorage::new(temp_dir.path()));
    let service = BenchmarkService::new(adapter, storage.clone());

    let models = vec![MODEL_GEMINI_31_FLASH.to_string(), MODEL_MUSE.to_string()];
    let archetypes = [
        BenchmarkArchetype::StoneCrypt,
        BenchmarkArchetype::RiversideAmbush,
    ];

    let results = service
        .run_suite_and_persist(&models, &archetypes)
        .await
        .expect("suite run and persistence succeeds");

    // 2 models * 2 archetypes = 4 generated benchmarks
    assert_eq!(results.len(), 4);

    let generated_dir = temp_dir.path().join("staging").join("generated");
    for res in &results {
        let bundle_dir = generated_dir.join(&res.metadata.id);
        let png = if bundle_dir.join("0.png").is_file() {
            bundle_dir.join("0.png")
        } else {
            generated_dir.join(format!("{}.png", res.metadata.id))
        };
        let json = if bundle_dir.join("manifest.json").is_file() {
            bundle_dir.join("manifest.json")
        } else {
            generated_dir.join(format!("{}.json", res.metadata.id))
        };
        assert!(png.is_file());
        assert!(json.is_file());
    }

    let staged = storage.list_staging().expect("list staging succeeds");
    assert_eq!(staged.len(), 4);
}

#[tokio::test]
async fn test_error_handling_unauthorized_and_quota() {
    let app = Router::new()
        .route(
            "/chat/completions",
            post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Invalid API key"}})),
                )
            }),
        )
        .route(
            "/images",
            post(|| async {
                (
                    StatusCode::PAYMENT_REQUIRED,
                    Json(json!({"error": {"message": "Insufficient credits"}})),
                )
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let adapter = OpenRouterAdapter::with_config("bad_key", format!("http://{addr}"), 2);

    // Chat endpoint 401
    let chat_err = adapter
        .generate_image(&GenerationRequest::new(MODEL_GEMINI_31_FLASH, "test"))
        .await
        .unwrap_err();
    assert!(matches!(chat_err, ModelEngineError::Auth(_)));

    // Images endpoint 402
    let img_err = adapter
        .generate_image(&GenerationRequest::new(MODEL_MUSE, "test"))
        .await
        .unwrap_err();
    assert!(matches!(img_err, ModelEngineError::QuotaOrRateLimit(_)));
}
