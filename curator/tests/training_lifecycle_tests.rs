//! Comprehensive integration tests for Issue #207:
//! Curator Training Run lifecycle and LoRA Adapter catalog (ADR-0008, ADR-0009).

use std::sync::Arc;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;
use wiremock::matchers::{bearer_token, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use curator::adapters::{
    FileCostLedger, FilesystemCatalogRepository, InMemoryBlobStorage,
    InMemoryCatalogRepository, ModalTrainingRunner,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    ArtifactVerification, ArtifactVerificationState, CheckpointRecord, DatasetSnapshot,
    LoraAdapter, TrainingMetricSnapshot, TrainingRecipe, TrainingRun, TrainingRunState,
    TRIGGER_PHRASE,
};
use curator::ports::cost_tracker::TrainingCostRecord;
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::cost_tracker::CostTracker;
use curator::ports::training_runner::TrainingRunner;

/// Helper: creates a mock frozen DatasetSnapshot in the catalog and blob storage.
async fn create_mock_snapshot(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    snapshot_id: &str,
) -> DatasetSnapshot {
    let zip_bytes = b"PK\x05\x06dummy_zip_content";
    let sha256 = curator::ports::blob_storage::sha256_digest(zip_bytes);

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
        maps_count: 5,
        samples_count: 20,
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
// Criterion 1: Domain Models, Serialization, and Canonical Digest
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_domain_models_and_recipe_digest() {
    // 1. TrainingRunState lowercase serialization and terminal checks
    for (state, expected_str) in [
        (TrainingRunState::Queued, "queued"),
        (TrainingRunState::Running, "running"),
        (TrainingRunState::Succeeded, "succeeded"),
        (TrainingRunState::Failed, "failed"),
        (TrainingRunState::Cancelled, "cancelled"),
    ] {
        let serialized = serde_json::to_string(&state).unwrap();
        assert_eq!(serialized, format!("\"{expected_str}\""));
        let deserialized: TrainingRunState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, state);
    }
    assert!(!TrainingRunState::Queued.is_terminal());
    assert!(!TrainingRunState::Running.is_terminal());
    assert!(TrainingRunState::Succeeded.is_terminal());
    assert!(TrainingRunState::Failed.is_terminal());
    assert!(TrainingRunState::Cancelled.is_terminal());

    // 2. TrainingRecipe defaults and policies
    let cal = TrainingRecipe::calibration();
    assert_eq!(cal.policy, "calibration");
    assert_eq!(cal.steps, 1500);
    assert_eq!(cal.rank, 32);
    assert_eq!(cal.alpha, 32);
    assert_eq!(cal.learning_rate, 1e-4);
    assert_eq!(cal.trigger_phrase, "dungeonforge battlemap");

    let full = TrainingRecipe::full();
    assert_eq!(full.policy, "full");
    assert_eq!(full.steps, 4000);

    // 3. Deterministic SHA-256 hex digest of canonical JSON
    let digest1 = cal.digest();
    let digest2 = cal.digest();
    assert_eq!(digest1, digest2);
    assert_eq!(digest1.len(), 64);
    assert_ne!(cal.digest(), full.digest());

    // 4. ArtifactVerificationState and ArtifactVerification
    let pending = ArtifactVerification::pending();
    assert_eq!(pending.status, ArtifactVerificationState::Pending);
    assert!(pending.checked_at.is_none());

    let verified = ArtifactVerification::verified("stat", Some("all good".to_string()));
    assert_eq!(verified.status, ArtifactVerificationState::Verified);
    assert!(verified.checked_at.is_some());

    let mismatch = ArtifactVerification::mismatch("stat", "checksum failed");
    assert_eq!(mismatch.status, ArtifactVerificationState::Mismatch);
    assert_eq!(mismatch.detail.as_deref(), Some("checksum failed"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 2: Persistence in CatalogRepository (Filesystem & InMemory)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_catalog_persistence_filesystem_and_in_memory() {
    let dir = tempdir().expect("tempdir");
    let fs_catalog: Arc<dyn CatalogRepository + Send + Sync> =
        Arc::new(FilesystemCatalogRepository::new(dir.path()));
    let mem_catalog: Arc<dyn CatalogRepository + Send + Sync> =
        Arc::new(InMemoryCatalogRepository::new());

    for catalog in [&fs_catalog, &mem_catalog] {
        let recipe = TrainingRecipe::calibration();
        let run_id = "test-run-001";
        let snapshot_id = "ds-snap-001";

        let run = TrainingRun {
            id: run_id.to_string(),
            remote_job_id: Some("job-123".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot_id.to_string(),
            dataset_digest: "dummy_sha256".to_string(),
            dataset_url: Some("https://r2.example.com/dataset.zip".to_string()),
            recipe: recipe.clone(),
            recipe_digest: recipe.digest(),
            created_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
            last_polled_at: Some(chrono::Utc::now()),
            last_poll_error: None,
            step: 250,
            total_steps: 1500,
            elapsed_seconds: 180.5,
            cost_usd: 0.15,
            checkpoints: vec![CheckpointRecord {
                step: 250,
                artifact_key: format!("artifacts/runs/{run_id}/checkpoints/step-250/adapter.safetensors"),
                artifact_url: format!("https://r2.example.com/artifacts/runs/{run_id}/checkpoints/step-250/adapter.safetensors"),
                artifact_sha256: Some("sha_cp_250".to_string()),
                artifact_size: Some(1024),
                artifact_verification: ArtifactVerification::verified("stat", None),
                sample_grid_key: None,
                sample_grid_url: None,
                sample_grid_sha256: None,
                sample_grid_size: None,
                sample_grid_verification: ArtifactVerification::pending(),
            }],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            metrics: vec![],
        };

        // 1. Save and retrieve TrainingRun
        catalog.save_training_run(&run).await.unwrap();
        let retrieved = catalog.get_training_run(run_id).await.unwrap();
        assert_eq!(retrieved.id, run_id);
        assert_eq!(retrieved.step, 250);
        assert_eq!(retrieved.checkpoints.len(), 1);

        // 2. Reference check: snapshot is referenced by the run
        assert!(
            catalog.is_dataset_snapshot_referenced(snapshot_id).await.unwrap(),
            "Snapshot must be referenced while run exists"
        );
        assert!(
            !catalog.is_dataset_snapshot_referenced("unreferenced-snap").await.unwrap(),
            "Unreferenced snapshot must return false"
        );

        // 3. Append and list metrics
        let metric1 = TrainingMetricSnapshot {
            timestamp: chrono::Utc::now(),
            step: 250,
            total_steps: 1500,
            elapsed_seconds: 180.5,
            cost_usd: 0.15,
            state: TrainingRunState::Running,
        };
        catalog.append_training_metric(run_id, &metric1).await.unwrap();
        let metrics = catalog.list_training_metrics(run_id).await.unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].step, 250);

        // 4. Save and retrieve LoraAdapter
        let adapter_id = format!("{run_id}-s250");
        let adapter = LoraAdapter {
            id: adapter_id.clone(),
            run_id: run_id.to_string(),
            step: 250,
            scale: 0.75,
            base_model: recipe.base_model.clone(),
            trigger_phrase: recipe.trigger_phrase.clone(),
            artifact_key: format!("artifacts/runs/{run_id}/checkpoints/step-250/adapter.safetensors"),
            artifact_url: format!("https://r2.example.com/artifacts/runs/{run_id}/checkpoints/step-250/adapter.safetensors"),
            sha256: Some("sha_cp_250".to_string()),
            size_bytes: Some(1024),
            recipe_digest: recipe.digest(),
            dataset_snapshot_id: snapshot_id.to_string(),
            dataset_digest: "dummy_sha256".to_string(),
            created_at: chrono::Utc::now(),
            retired: false,
            retired_at: None,
        };
        catalog.save_lora_adapter(&adapter).await.unwrap();
        let loaded_adapter = catalog.get_lora_adapter(&adapter_id).await.unwrap();
        assert_eq!(loaded_adapter.id, adapter_id);
        assert_eq!(loaded_adapter.step, 250);
        assert!(!loaded_adapter.retired);

        let adapters = catalog.list_lora_adapters().await.unwrap();
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].id, adapter_id);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 3: Dispatch & Readiness-Gated Snapshot Contract
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_and_dispatch_training_run_contract() {
    let (catalog, blobs) = (
        Arc::new(InMemoryCatalogRepository::new()) as Arc<dyn CatalogRepository + Send + Sync>,
        Arc::new(InMemoryBlobStorage::new()) as Arc<dyn BlobStorage + Send + Sync>,
    );
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));
    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone());
    let app = create_router(state);

    // 1. Missing snapshot returns 404
    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "dataset_snapshot_id": "nonexistent_snapshot" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 2. Snapshot metadata exists but blob is missing from BlobStorage -> 400 Bad Request
    let orphan_snapshot = DatasetSnapshot {
        id: "ds_orphan".to_string(),
        theme: None,
        created_at: chrono::Utc::now(),
        sha256: "orphan_sha".to_string(),
        size_bytes: 100,
        maps_count: 1,
        samples_count: 4,
        trigger_phrase: TRIGGER_PHRASE.to_string(),
        artifact_key: "artifacts/datasets/ds_orphan.zip".to_string(),
        artifact_url: None,
    };
    catalog.save_dataset_snapshot(&orphan_snapshot).await.unwrap();

    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "dataset_snapshot_id": "ds_orphan" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. Create clean ready snapshot with ZIP blob
    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds_clean_001").await;

    // 4. Create run in queued state
    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({
            "dataset_snapshot_id": snapshot.id,
            "policy": "calibration"
        }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let run1: Value = serde_json::from_slice(&body).unwrap();
    let run1_id = run1["id"].as_str().unwrap().to_string();
    assert_eq!(run1["state"], "queued");
    assert_eq!(run1["dataset_snapshot_id"], snapshot.id);

    // 5. Create a second run referencing the same snapshot (ADR-0009: multiple runs backed by one snapshot)
    let req = Request::builder()
        .uri("/api/training/runs")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({
            "dataset_snapshot_id": snapshot.id,
            "policy": "full"
        }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let run2: Value = serde_json::from_slice(&body).unwrap();
    let run2_id = run2["id"].as_str().unwrap().to_string();
    assert_ne!(run1_id, run2_id);

    // 6. Attempt dispatch without Modal credentials configured -> HTTP 503 MODAL_NOT_CONFIGURED
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run1_id}/dispatch"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["code"], "MODAL_NOT_CONFIGURED");
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 4: State Transitions, Polling, Cancellation, and WireMock Modal Runner
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_modal_runner_lifecycle_dispatch_and_conflict() {
    let mock_server = MockServer::start().await;

    let (catalog, blobs) = (
        Arc::new(InMemoryCatalogRepository::new()) as Arc<dyn CatalogRepository + Send + Sync>,
        Arc::new(InMemoryBlobStorage::new()) as Arc<dyn BlobStorage + Send + Sync>,
    );
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));

    let token = "test_modal_secret_token";
    let modal_runner: Arc<dyn TrainingRunner + Send + Sync> =
        Arc::new(ModalTrainingRunner::new(mock_server.uri(), token));

    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone())
        .with_training_runner(modal_runner.clone());
    let app = create_router(state);

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds_modal_test").await;

    // Create two queued runs
    let run1 = catalog
        .save_training_run(&TrainingRun {
            id: "run-alpha".to_string(),
            remote_job_id: None,
            state: TrainingRunState::Queued,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            step: 0,
            total_steps: 1500,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: vec![],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            metrics: vec![],
        })
        .await;
    assert!(run1.is_ok());

    let run2 = catalog
        .save_training_run(&TrainingRun {
            id: "run-beta".to_string(),
            remote_job_id: None,
            state: TrainingRunState::Queued,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::full(),
            recipe_digest: TrainingRecipe::full().digest(),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            step: 0,
            total_steps: 4000,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: vec![],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            metrics: vec![],
        })
        .await;
    assert!(run2.is_ok());

    // Mock Modal POST /run response for run-alpha
    Mock::given(method("POST"))
        .and(path("/run"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-alpha",
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

    // Dispatch run-alpha -> transitions to running
    let req = Request::builder()
        .uri("/api/training/runs/run-alpha/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let dispatched: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(dispatched["state"], "running");

    // Serialization check: Dispatch run-beta while run-alpha is running -> HTTP 409 RUN_ALREADY_ACTIVE
    let req = Request::builder()
        .uri("/api/training/runs/run-beta/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["code"], "RUN_ALREADY_ACTIVE");
    assert_eq!(err_json["active_run_id"], "run-alpha");

    // Verify run-beta stayed in queued state
    let run_beta = catalog.get_training_run("run-beta").await.unwrap();
    assert_eq!(run_beta.state, TrainingRunState::Queued);

    // Cancel run-alpha
    Mock::given(method("POST"))
        .and(path("/run/run-alpha/cancel"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-alpha",
            "state": "cancelled",
            "step": 100,
            "total_steps": 1500,
            "elapsed_seconds": 15.0,
            "cost_usd": 0.02,
            "checkpoints": [],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    let req = Request::builder()
        .uri("/api/training/runs/run-alpha/cancel")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cancelled_run = catalog.get_training_run("run-alpha").await.unwrap();
    assert_eq!(cancelled_run.state, TrainingRunState::Cancelled);
}

#[tokio::test]
async fn test_artifact_verification_failure_and_success_flow() {
    let mock_server = MockServer::start().await;

    let (catalog, blobs) = (
        Arc::new(InMemoryCatalogRepository::new()) as Arc<dyn CatalogRepository + Send + Sync>,
        Arc::new(InMemoryBlobStorage::new()) as Arc<dyn BlobStorage + Send + Sync>,
    );
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));

    let token = "test_modal_token";
    let modal_runner: Arc<dyn TrainingRunner + Send + Sync> =
        Arc::new(ModalTrainingRunner::new(mock_server.uri(), token));

    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone())
        .with_training_runner(modal_runner.clone());
    let app = create_router(state);

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds_verify_test").await;

    // Create and dispatch run-verify
    catalog
        .save_training_run(&TrainingRun {
            id: "run-verify".to_string(),
            remote_job_id: Some("run-verify".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            step: 0,
            total_steps: 1500,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: vec![],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            metrics: vec![],
        })
        .await
        .unwrap();

    // 1. Modal reports "succeeded", but checkpoint adapter artifact is MISSING in BlobStorage!
    Mock::given(method("GET"))
        .and(path("/run/run-verify"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-verify",
            "state": "succeeded",
            "step": 1500,
            "total_steps": 1500,
            "elapsed_seconds": 1200.0,
            "cost_usd": 1.95,
            "checkpoints": [
                {
                    "step": 1500,
                    "artifact_url": "https://r2.example.com/artifacts/runs/run-verify/checkpoints/step-1500/adapter.safetensors",
                    "artifact_sha256": "fake_sha",
                    "artifact_size": 2048,
                    "sample_grid_url": null,
                    "sample_grid_sha256": null,
                    "sample_grid_size": null
                }
            ],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    // GET /api/training/runs/run-verify triggers poll and artifact verification
    let req = Request::builder()
        .uri("/api/training/runs/run-verify")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let polled: Value = serde_json::from_slice(&body).unwrap();

    // Must be reported as FAILED with ARTIFACT_VERIFICATION_FAILED rather than succeeded!
    assert_eq!(polled["state"], "failed");
    assert_eq!(polled["failure_code"], "ARTIFACT_VERIFICATION_FAILED");
    assert!(polled["error"].as_str().unwrap().contains("adapter.safetensors"));

    // 2. Now write the checkpoint adapter artifact to storage and test clean verification
    let cp_key = BlobKey::CheckpointAdapter {
        run_id: "run-success".to_string(),
        step: 1500,
    };
    let cp_bytes = b"valid_safetensors_bytes_2048";
    let cp_sha = curator::ports::blob_storage::sha256_digest(cp_bytes);
    blobs
        .write_blob_with_sha256(&cp_key, cp_bytes, Some(&cp_sha))
        .await
        .unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: "run-success".to_string(),
            remote_job_id: Some("run-success".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            step: 0,
            total_steps: 1500,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: vec![],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            metrics: vec![],
        })
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/run/run-success"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-success",
            "state": "succeeded",
            "step": 1500,
            "total_steps": 1500,
            "elapsed_seconds": 1200.0,
            "cost_usd": 1.95,
            "checkpoints": [
                {
                    "step": 1500,
                    "artifact_url": "https://r2.example.com/artifacts/runs/run-success/checkpoints/step-1500/adapter.safetensors",
                    "artifact_sha256": cp_sha,
                    "artifact_size": cp_bytes.len(),
                    "sample_grid_url": null,
                    "sample_grid_sha256": null,
                    "sample_grid_size": null
                }
            ],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    let req = Request::builder()
        .uri("/api/training/runs/run-success")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let polled_succ: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(polled_succ["state"], "succeeded");
    assert_eq!(polled_succ["checkpoints"][0]["artifact_verification"]["status"], "verified");
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 5: LoRA Adapter Catalog and Publication
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_lora_adapter_catalog_and_publication() {
    let (catalog, blobs) = (
        Arc::new(InMemoryCatalogRepository::new()) as Arc<dyn CatalogRepository + Send + Sync>,
        Arc::new(InMemoryBlobStorage::new()) as Arc<dyn BlobStorage + Send + Sync>,
    );
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));
    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone());
    let app = create_router(state);

    let run_id = "run-pub-test";
    let step = 1500;
    let cp_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };
    blobs
        .write_blob(&cp_key, b"checkpoint_safetensors_bytes")
        .await
        .unwrap();

    let recipe = TrainingRecipe::calibration();
    let run = TrainingRun {
        id: run_id.to_string(),
        remote_job_id: Some("job-pub".to_string()),
        state: TrainingRunState::Succeeded,
        dataset_snapshot_id: "ds_snap".to_string(),
        dataset_digest: "sha_ds".to_string(),
        dataset_url: Some("https://r2.example.com/ds.zip".to_string()),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: chrono::Utc::now(),
        started_at: Some(chrono::Utc::now()),
        finished_at: Some(chrono::Utc::now()),
        last_polled_at: Some(chrono::Utc::now()),
        last_poll_error: None,
        step: 1500,
        total_steps: 1500,
        elapsed_seconds: 1200.0,
        cost_usd: 1.95,
        checkpoints: vec![CheckpointRecord {
            step: 1500,
            artifact_key: cp_key.to_path(),
            artifact_url: format!("https://r2.example.com/{}", cp_key.to_path()),
            artifact_sha256: Some("sha_weights".to_string()),
            artifact_size: Some(28),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: None,
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        metrics: vec![],
    };
    catalog.save_training_run(&run).await.unwrap();

    // 1. Publish adapter
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "step": 1500, "scale": 0.8 }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let adapter: Value = serde_json::from_slice(&body).unwrap();
    let expected_id = format!("{run_id}-s1500");
    assert_eq!(adapter["id"], expected_id);
    assert_eq!(adapter["scale"], 0.8);
    assert_eq!(adapter["retired"], false);

    // Verify run record now includes the published adapter ID
    let updated_run = catalog.get_training_run(run_id).await.unwrap();
    assert!(updated_run.published_adapter_ids.contains(&expected_id));

    // 2. Conflict on re-publishing the same step
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "step": 1500 }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 3. GET /api/training/adapters and GET /api/training/adapters/{id}
    let req = Request::builder()
        .uri("/api/training/adapters")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    // 4. Soft-retire adapter via POST /api/training/adapters/{id}/retire
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{expected_id}/retire"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "retired": true }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // With selectable=true query, retired adapter is excluded
    let req = Request::builder()
        .uri("/api/training/adapters?selectable=true")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let filtered_list: Value = serde_json::from_slice(&body).unwrap();
    assert!(filtered_list.as_array().unwrap().is_empty());

    // Restore adapter
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{expected_id}/retire"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "retired": false }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Restored adapter is now selectable again
    let req = Request::builder()
        .uri("/api/training/adapters?selectable=true")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list_restored: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list_restored.as_array().unwrap().len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 6: Cost Ledger Union and Stats Integration
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_cost_ledger_union_and_stats_integration() {
    let temp_dir = tempdir().unwrap();
    let ledger_path = temp_dir.path().join("telemetry").join("cost_ledger.json");

    // Write a legacy cost ledger file with untagged entries
    let legacy_json = json!({
        "total_spend_usd": 0.045,
        "total_generations": 3,
        "by_model": {},
        "history": [
            {
                "id": "gen_1",
                "model": "google/gemini-3.1-flash-image",
                "prompt_name": "Tomb",
                "cost_usd": 0.015,
                "latency_ms": 250,
                "timestamp": "2026-10-24T09:00:00Z"
            }
        ]
    });
    std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
    std::fs::write(&ledger_path, serde_json::to_string_pretty(&legacy_json).unwrap()).unwrap();

    let cost_tracker: Arc<dyn CostTracker + Send + Sync> =
        Arc::new(FileCostLedger::new(&ledger_path));
    let (catalog, blobs) = (
        Arc::new(InMemoryCatalogRepository::new()) as Arc<dyn CatalogRepository + Send + Sync>,
        Arc::new(InMemoryBlobStorage::new()) as Arc<dyn BlobStorage + Send + Sync>,
    );
    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone());
    let app = create_router(state);

    // Initial stats check: legacy entries automatically migrated
    let req = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats["generation_spend"], 0.045);
    assert_eq!(stats["training_spend"], 0.0);
    assert_eq!(stats["training_runs"], 0);
    assert_eq!(stats["all_time_spend"], 0.045);

    // Record training cost
    cost_tracker
        .record_training_cost(TrainingCostRecord::new(
            "run-e2e-1",
            "calibration",
            1500,
            1200.0,
            3.30,
            "succeeded",
        ))
        .await
        .unwrap();

    // Query stats again
    let req = Request::builder()
        .uri("/api/cost/stats")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let updated_stats: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(updated_stats["training_runs"], 1);
    assert!((updated_stats["training_spend"].as_f64().unwrap() - 3.30).abs() < 1e-6);
    assert!((updated_stats["generation_spend"].as_f64().unwrap() - 0.045).abs() < 1e-6);
    assert!((updated_stats["all_time_spend"].as_f64().unwrap() - 3.345).abs() < 1e-6);
    assert!((updated_stats["session_spend"].as_f64().unwrap() - 3.30).abs() < 1e-6);
}
