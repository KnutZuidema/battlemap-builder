//! Integration tests for Issue #209:
//! Curator UI: Training Runs and LoRA Adapter selection (ADR-0009, ADR-0010).

use std::sync::Arc;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;
use wiremock::matchers::{bearer_token, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use curator::adapters::{
    FileCostLedger, InMemoryBlobStorage, InMemoryCatalogRepository, ModalTrainingRunner,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    ArtifactVerification, ArtifactVerificationState, CheckpointRecord, DatasetSnapshot,
    TrainingRecipe, TrainingRun, TrainingRunState, TRIGGER_PHRASE,
};
use curator::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::cost_tracker::CostTracker;
use curator::ports::training_runner::TrainingRunner;

/// Creates a test app router with in-memory stores and an optional mock runner.
fn setup_test_app(
    runner: Option<Arc<dyn TrainingRunner + Send + Sync>>,
) -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
    Arc<dyn CostTracker + Send + Sync>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));

    let mut state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone());
    if let Some(r) = runner {
        state = state.with_training_runner(r);
    }
    let router = create_router(state);
    (router, catalog, blobs, cost_tracker)
}

/// Helper creating a mock frozen DatasetSnapshot.
async fn create_mock_snapshot(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<InMemoryBlobStorage>,
    snapshot_id: &str,
) -> DatasetSnapshot {
    let zip_bytes = b"PK\x05\x06test_snapshot_zip_data";
    let sha256 = sha256_digest(zip_bytes);

    let blob_key = BlobKey::DatasetSnapshot {
        id: snapshot_id.to_string(),
    };
    blobs
        .write_blob_with_sha256(&blob_key, zip_bytes, Some(&sha256))
        .await
        .expect("write dataset snapshot blob");

    let snapshot = DatasetSnapshot {
        id: snapshot_id.to_string(),
        theme: None,
        created_at: chrono::Utc::now(),
        sha256,
        size_bytes: zip_bytes.len() as u64,
        maps_count: 3,
        samples_count: 12,
        trigger_phrase: TRIGGER_PHRASE.to_string(),
        artifact_key: blob_key.to_path(),
        artifact_url: Some(format!("https://r2.example.com/{}", blob_key.to_path())),
    };

    catalog
        .save_dataset_snapshot(&snapshot)
        .await
        .expect("save dataset snapshot metadata");

    snapshot
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: Embedded UI assets and ES modules are served
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_embedded_ui_assets_served() {
    let (app, _, _, _) = setup_test_app(None);

    // GET / (index.html)
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.starts_with("text/html"));

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(!body_bytes.is_empty(), "index.html body must not be empty");
    std::str::from_utf8(&body_bytes).unwrap();

    // GET /style.css
    let req = Request::builder().uri("/style.css").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.starts_with("text/css"));

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(!body_bytes.is_empty(), "style.css body must not be empty");
    std::str::from_utf8(&body_bytes).unwrap();

    // GET /app.js
    let req = Request::builder().uri("/app.js").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.contains("javascript"));

    let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(!body_bytes.is_empty(), "app.js body must not be empty");
    std::str::from_utf8(&body_bytes).unwrap();

    // The view modules and shared helpers are served as native ES modules.
    for module in ["/views/training.js", "/views/snapshots.js", "/dataset-scope.js"] {
        let req = Request::builder().uri(module).body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{module} must be served");
        let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(
            content_type.contains("javascript"),
            "{module} must be served as JavaScript"
        );
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(!body_bytes.is_empty(), "{module} body must not be empty");
        std::str::from_utf8(&body_bytes).unwrap();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: Criterion 1 - Training Run Creation, Dispatch, and Polling Flow
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_training_runs_creation_dispatch_and_polling_flow() {
    let mock_server = MockServer::start().await;
    let token = "test_bearer_token";
    let runner: Arc<dyn TrainingRunner + Send + Sync> =
        Arc::new(ModalTrainingRunner::new(mock_server.uri(), token));
    let (app, catalog, blobs, _) = setup_test_app(Some(runner));

    let snapshot = create_mock_snapshot(&catalog, &blobs, "snap_test_run").await;

    // 1. POST /api/training/runs -> Created 201 in "queued" state
    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "dataset_snapshot_id": snapshot.id,
                "policy": "calibration"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let created: Value = serde_json::from_slice(&body).unwrap();
    let run_id = created["id"].as_str().unwrap().to_string();

    assert_eq!(created["state"], "queued");
    assert_eq!(created["step"], 0);
    assert_eq!(created["total_steps"], 1500);
    assert_eq!(created["recipe"]["policy"], "calibration");
    assert_eq!(created["dataset_snapshot_id"], snapshot.id);

    // 2. GET /api/training/runs -> Listed
    let req = Request::builder().uri("/api/training/runs").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], run_id);
    assert_eq!(list[0]["state"], "queued");

    // 3. Dispatch run via Modal mock runner
    Mock::given(method("POST"))
        .and(path("/run"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "state": "running",
            "step": 0,
            "total_steps": 1500,
            "elapsed_seconds": 0.0,
            "cost_usd": 0.0,
            "checkpoints": [],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/run/{run_id}")))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": run_id,
            "state": "running",
            "step": 0,
            "total_steps": 1500,
            "elapsed_seconds": 0.0,
            "cost_usd": 0.0,
            "checkpoints": [],
            "error": null
        })))
        .mount(&mock_server)
        .await;
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/dispatch"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let dispatched: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(dispatched["state"], "running");

    // 4. Polling GET /api/training/runs/{id} returns live running state
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let live: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(live["state"], "running");
    assert_eq!(live["id"], run_id);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: Criterion 2 - Readiness Gate Refusal with Actionable Remediation in UI
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_readiness_gate_refusal_with_remediation_in_ui() {
    let (app, _, _, _) = setup_test_app(None);

    // Scope has no curated maps -> GET /api/dataset/readiness reports ready: false
    let req = Request::builder()
        .uri("/api/dataset/readiness")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let readiness: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(readiness["ready"], false);

    // Attempting to freeze a snapshot against unready scope -> HTTP 422 DATASET_NOT_READY
    let req = Request::builder()
        .uri("/api/training/datasets")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["code"], "DATASET_NOT_READY");
    assert!(err_json["error"].is_string());

    // Must surface actionable remediation rather than bare failure
    let blocking = err_json["blocking_issues"].as_array().expect("blocking_issues must be array");
    assert!(!blocking.is_empty(), "Must report blocking issues");
    for issue in blocking {
        assert!(issue["code"].is_string(), "Issue must have code");
        let remediation = issue["remediation"].as_str().expect("Issue must have actionable remediation");
        assert!(!remediation.is_empty(), "Remediation cannot be empty");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: Criterion 3 - Run Cancellation Reflects Cancelled Immediately
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_run_cancellation_reflects_cancelled_immediately() {
    let (app, catalog, blobs, _) = setup_test_app(None);
    let snapshot = create_mock_snapshot(&catalog, &blobs, "snap_cancel_test").await;

    // Create queued run
    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "dataset_snapshot_id": snapshot.id,
                "policy": "calibration"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let run: Value = serde_json::from_slice(&body).unwrap();
    let run_id = run["id"].as_str().unwrap();

    // Cancel run -> returns 200 OK with cancelled state
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/cancel"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let cancelled: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cancelled["state"], "cancelled");

    // GET /api/training/runs/{id} confirms cancelled state
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let record: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(record["state"], "cancelled");

    // Second cancel attempt is idempotent and succeeds with cancelled state
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/cancel"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let second_cancel: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(second_cancel["state"], "cancelled");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: Criterion 4 - Checkpoint Inspection, Publication, Retirement & Models
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_checkpoint_inspection_publication_retirement_and_models_flow() {
    let (app, catalog, blobs, _) = setup_test_app(None);

    let run_id = "run-pub-bench-test";
    let step = 1500;
    let adapter_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };
    let sample_key = BlobKey::CheckpointSamples {
        run_id: run_id.to_string(),
        step,
    };

    let adapter_bytes = b"dummy_safetensors_bytes";
    let sample_bytes = b"dummy_sample_grid_jpeg";

    blobs.write_blob(&adapter_key, adapter_bytes).await.unwrap();
    blobs.write_blob(&sample_key, sample_bytes).await.unwrap();

    let checkpoint = CheckpointRecord {
        step,
        artifact_key: adapter_key.to_path(),
        artifact_url: format!("https://r2.example.com/{}", adapter_key.to_path()),
        artifact_sha256: Some(sha256_digest(adapter_bytes)),
        artifact_size: Some(adapter_bytes.len() as u64),
        artifact_verification: ArtifactVerification {
            status: ArtifactVerificationState::Pending,
            method: None,
            checked_at: None,
            detail: None,
        },
        sample_grid_key: Some(sample_key.to_path()),
        sample_grid_url: Some(format!("https://r2.example.com/{}", sample_key.to_path())),
        sample_grid_sha256: Some(sha256_digest(sample_bytes)),
        sample_grid_size: Some(sample_bytes.len() as u64),
        sample_grid_verification: ArtifactVerification {
            status: ArtifactVerificationState::Pending,
            method: None,
            checked_at: None,
            detail: None,
        },
    };

    let recipe = TrainingRecipe::calibration();
    let succeeded_run = TrainingRun {
        id: run_id.to_string(),
        remote_job_id: Some("job-123".to_string()),
        state: TrainingRunState::Succeeded,
        dataset_snapshot_id: "snap-test".to_string(),
        dataset_digest: "sha-test".to_string(),
        dataset_url: Some("https://r2.example.com/snap.zip".to_string()),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: Some(chrono::Utc::now()),
        finished_at: Some(chrono::Utc::now()),
        last_polled_at: Some(chrono::Utc::now()),
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 1500,
        total_steps: 1500,
        elapsed_seconds: 900.0,
        cost_usd: 1.25,
        checkpoints: vec![checkpoint],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };

    catalog.save_training_run(&succeeded_run).await.unwrap();

    // 1. Verify Artifacts: POST /api/training/runs/{id}/verify
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/verify"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let verified_run: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        verified_run["checkpoints"][0]["artifact_verification"]["status"],
        "verified"
    );

    // 2. Initial GET /api/models: Does not contain unpublished adapter
    let req = Request::builder().uri("/api/models").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let initial_models: Vec<Value> = serde_json::from_slice(&body).unwrap();
    let expected_adapter_id = format!("{run_id}-s1500");
    assert!(
        !initial_models.iter().any(|m| m["id"] == expected_adapter_id),
        "Unpublished adapter must not appear in GET /api/models"
    );

    // 3. Publish Adapter: POST /api/training/runs/{id}/publish { step: 1500, scale: 0.75 }
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "step": 1500, "scale": 0.75 }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let adapter: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(adapter["id"], expected_adapter_id);
    assert_eq!(adapter["scale"], 0.75);
    assert_eq!(adapter["retired"], false);

    // 4. GET /api/training/adapters lists the published adapter
    let req = Request::builder().uri("/api/training/adapters").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let adapters: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(adapters.iter().any(|a| a["id"] == expected_adapter_id));

    // 5. Benchmark Drawer Integration: GET /api/models now includes {id} and {id}-edit!
    let req = Request::builder().uri("/api/models").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated_models: Vec<Value> = serde_json::from_slice(&body).unwrap();

    let t2i_model = updated_models.iter().find(|m| m["id"] == expected_adapter_id);
    assert!(t2i_model.is_some(), "Published adapter must appear as T2I model in GET /api/models");

    let edit_id = format!("{expected_adapter_id}-edit");
    let edit_model = updated_models.iter().find(|m| m["id"] == edit_id);
    assert!(edit_model.is_some(), "Published adapter must appear as Edit model in GET /api/models");

    // 6. Soft-Retire Adapter: POST /api/training/adapters/{id}/retire { retired: true }
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{expected_adapter_id}/retire"))
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "retired": true }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let retired_adapter: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(retired_adapter["retired"], true);

    // 7. GET /api/models: Retired adapter disappears from selection
    let req = Request::builder().uri("/api/models").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let models_after_retire: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        !models_after_retire.iter().any(|m| m["id"] == expected_adapter_id),
        "Retired adapter must be excluded from GET /api/models"
    );
    assert!(
        !models_after_retire.iter().any(|m| m["id"] == edit_id),
        "Retired adapter edit engine must be excluded from GET /api/models"
    );

    // 8. Restore Adapter: POST /api/training/adapters/{id}/retire { retired: false }
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{expected_adapter_id}/retire"))
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "retired": false }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let restored: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(restored["retired"], false);

    // GET /api/models: Restored adapter is selectable again
    let req = Request::builder().uri("/api/models").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let models_after_restore: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(models_after_restore.iter().any(|m| m["id"] == expected_adapter_id));

    // 9. Cost stats reflects unified spend
    let req = Request::builder().uri("/api/cost/stats").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats: Value = serde_json::from_slice(&body).unwrap();
    assert!(stats["all_time_spend"].is_number());
    assert!(stats["training_spend"].is_number());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: Criterion 5 - Detailed Error Surfacing on Active Runs and Runner Failures
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_detailed_error_surfacing_run_already_active_and_modal_not_configured() {
    let (app, catalog, blobs, _) = setup_test_app(None);
    let snapshot = create_mock_snapshot(&catalog, &blobs, "snap_err_test").await;

    // 1. Without runner configured -> 503 MODAL_NOT_CONFIGURED
    let recipe = TrainingRecipe::calibration();
    let queued_run = TrainingRun {
        id: "run-unconfigured".to_string(),
        remote_job_id: None,
        state: TrainingRunState::Queued,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        last_polled_at: None,
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 0,
        total_steps: 1500,
        elapsed_seconds: 0.0,
        cost_usd: 0.0,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog.save_training_run(&queued_run).await.unwrap();

    let req = Request::builder()
        .uri("/api/training/runs/run-unconfigured/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["code"], "MODAL_NOT_CONFIGURED");
    assert!(err_json["error"].is_string());

    // 2. Active run conflict -> 409 RUN_ALREADY_ACTIVE naming the active run
    let mock_server = MockServer::start().await;
    let token = "test_modal_token_conflict";
    let runner: Arc<dyn TrainingRunner + Send + Sync> =
        Arc::new(ModalTrainingRunner::new(mock_server.uri(), token));
    let (app_runner, catalog_runner, blobs_runner, _) = setup_test_app(Some(runner));
    let snap_runner = create_mock_snapshot(&catalog_runner, &blobs_runner, "snap_conflict_test").await;

    let active_run_id = "run-active-gpu-1";
    let active_run = TrainingRun {
        id: active_run_id.to_string(),
        remote_job_id: Some("job-active".to_string()),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snap_runner.id.clone(),
        dataset_digest: snap_runner.sha256.clone(),
        dataset_url: snap_runner.artifact_url.clone(),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: Some(chrono::Utc::now()),
        finished_at: None,
        last_polled_at: Some(chrono::Utc::now()),
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 350,
        total_steps: 1500,
        elapsed_seconds: 240.0,
        cost_usd: 0.42,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog_runner.save_training_run(&active_run).await.unwrap();

    let queued_run_2 = TrainingRun {
        id: "run-waiting".to_string(),
        remote_job_id: None,
        state: TrainingRunState::Queued,
        dataset_snapshot_id: snap_runner.id.clone(),
        dataset_digest: snap_runner.sha256.clone(),
        dataset_url: snap_runner.artifact_url.clone(),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: None,
        finished_at: None,
        last_polled_at: None,
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 0,
        total_steps: 1500,
        elapsed_seconds: 0.0,
        cost_usd: 0.0,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog_runner.save_training_run(&queued_run_2).await.unwrap();

    let req = Request::builder()
        .uri("/api/training/runs/run-waiting/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app_runner.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let conflict_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(conflict_json["code"], "RUN_ALREADY_ACTIVE");
    assert_eq!(conflict_json["active_run_id"], active_run_id);
    assert!(
        conflict_json["error"].as_str().unwrap().contains(active_run_id),
        "Error message must name the active run"
    );
    // 3. Failed run displays failure details
    let failed_run = TrainingRun {
        id: "run-failed-oom".to_string(),
        remote_job_id: Some("job-oom".to_string()),
        state: TrainingRunState::Failed,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: Some(chrono::Utc::now()),
        finished_at: Some(chrono::Utc::now()),
        last_polled_at: Some(chrono::Utc::now()),
        last_poll_error: Some("Connection reset by remote peer".to_string()),
        consecutive_poll_failures: 1,
        poll_backoff_seconds: 15,
        step: 120,
        total_steps: 1500,
        elapsed_seconds: 90.0,
        cost_usd: 0.18,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: Some("CUDA out of memory error on GPU tier".to_string()),
        failure_code: Some("cuda_oom".to_string()),
        metrics: vec![],
    };
    catalog.save_training_run(&failed_run).await.unwrap();

    let req = Request::builder()
        .uri("/api/training/runs/run-failed-oom")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let detail: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["state"], "failed");
    assert_eq!(detail["failure_code"], "cuda_oom");
    assert_eq!(detail["error"], "CUDA out of memory error on GPU tier");
    assert_eq!(detail["last_poll_error"], "Connection reset by remote peer");
}
