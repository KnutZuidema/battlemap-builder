//! Integration tests for Milestone 13 Issue #218: Training Run resilience,
//! restart survival, exponential backoff, progress timeline persistence, and artifact verification.

use std::sync::Arc;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Utc;
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;
use wiremock::matchers::{bearer_token, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use curator::adapters::{
    FileCostLedger, FilesystemBlobStorage, FilesystemCatalogRepository, InMemoryBlobStorage,
    InMemoryCatalogRepository, ModalTrainingRunner,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    ArtifactVerification, CheckpointRecord, DatasetSnapshot,
    TrainingMetricSnapshot, TrainingRecipe, TrainingRun, TrainingRunState,
};
use curator::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::cost_tracker::CostTracker;
use curator::ports::training_runner::TrainingRunner;
use curator::services::compute_poll_backoff;

/// Helper: creates a mock frozen DatasetSnapshot in the catalog and blob storage.
async fn create_mock_snapshot(
    catalog: &Arc<dyn CatalogRepository + Send + Sync>,
    blobs: &Arc<dyn BlobStorage + Send + Sync>,
    id: &str,
) -> DatasetSnapshot {
    let dummy_archive = b"PK\x03\x04test_dataset_zip_bytes";
    let sha = sha256_digest(dummy_archive);
    let key = BlobKey::DatasetSnapshot { id: id.to_string() };
    blobs
        .write_blob_with_sha256(&key, dummy_archive, Some(&sha))
        .await
        .unwrap();

    let snapshot = DatasetSnapshot {
        id: id.to_string(),
        theme: None,
        created_at: Utc::now(),
        sha256: sha,
        size_bytes: dummy_archive.len() as u64,
        maps_count: 5,
        samples_count: 20,
        trigger_phrase: "bmap style".to_string(),
        artifact_key: key.to_path(),
        artifact_url: Some(format!("https://r2.example.com/{}", key.to_path())),
    };

    catalog.save_dataset_snapshot(&snapshot).await.unwrap();
    snapshot
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 1: Restart survival & lost_remote detection
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_restart_survival_and_lost_remote_detection() {
    let mock_server = MockServer::start().await;
    let data_dir = tempdir().unwrap();

    let catalog = Arc::new(FilesystemCatalogRepository::new(data_dir.path()))
        as Arc<dyn CatalogRepository + Send + Sync>;
    let blobs = Arc::new(FilesystemBlobStorage::new(data_dir.path()))
        as Arc<dyn BlobStorage + Send + Sync>;
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        data_dir.path().join("telemetry/cost_ledger.json"),
    ));

    let token = "test_modal_token";
    let modal_runner: Arc<dyn TrainingRunner + Send + Sync> =
        Arc::new(ModalTrainingRunner::new(mock_server.uri(), token));

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds-snap-resilience").await;

    // 1. Create two running runs before "restart"
    // run-1 stays active on Modal
    let run1 = TrainingRun {
        id: "run-active".to_string(),
        remote_job_id: Some("job-active".to_string()),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: TrainingRecipe::calibration(),
        recipe_digest: TrainingRecipe::calibration().digest(),
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        last_polled_at: Some(Utc::now()),
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 250,
        total_steps: 1500,
        elapsed_seconds: 60.0,
        cost_usd: 0.10,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog.save_training_run(&run1).await.unwrap();

    // run-2 has a lost remote job on Modal (runner returns 404)
    let run2 = TrainingRun {
        id: "run-lost".to_string(),
        remote_job_id: Some("job-lost-404".to_string()),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: TrainingRecipe::calibration(),
        recipe_digest: TrainingRecipe::calibration().digest(),
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        last_polled_at: Some(Utc::now()),
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 100,
        total_steps: 1500,
        elapsed_seconds: 25.0,
        cost_usd: 0.05,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog.save_training_run(&run2).await.unwrap();

    // Mock Modal responses: job-active is still running, job-lost-404 returns 404 Not Found
    Mock::given(method("GET"))
        .and(path("/run/job-active"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "job-active",
            "state": "running",
            "step": 500,
            "total_steps": 1500,
            "elapsed_seconds": 120.0,
            "cost_usd": 0.20,
            "checkpoints": [],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/run/job-lost-404"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "detail": "Job not found"
        })))
        .mount(&mock_server)
        .await;

    // 2. Simulate server restart: reconstruct state & service from filesystem
    let restored_catalog = Arc::new(FilesystemCatalogRepository::new(data_dir.path()))
        as Arc<dyn CatalogRepository + Send + Sync>;
    let state = AppState::with_cost_tracker(cost_tracker.clone(), restored_catalog.clone(), blobs.clone())
        .with_training_runner(modal_runner.clone());

    // Call resume_non_terminal_runs on restored service
    let resumed = state.training_service.resume_non_terminal_runs().await.unwrap();
    assert_eq!(resumed.len(), 2);

    // Verify run-lost: transitioned to failed, failure_code is LOST_REMOTE, error names remote job
    let lost_run = restored_catalog.get_training_run("run-lost").await.unwrap();
    assert_eq!(lost_run.state, TrainingRunState::Failed);
    assert_eq!(lost_run.failure_code.as_deref(), Some("LOST_REMOTE"));
    assert!(
        lost_run
            .error
            .as_deref()
            .unwrap()
            .contains("job-lost-404"),
        "error must name the lost remote job ID: {:?}",
        lost_run.error
    );

    // Verify run-active: stayed running and updated progress
    let active_run = restored_catalog.get_training_run("run-active").await.unwrap();
    assert_eq!(active_run.state, TrainingRunState::Running);
    assert_eq!(active_run.step, 500);
    assert_eq!(active_run.elapsed_seconds, 120.0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 2: Unreachable runner preserves last_polled_at and backs off exponentially
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_unreachable_runner_preserves_last_polled_at_and_backs_off() {
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

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds-snap-backoff").await;

    // Initial successful poll timestamp
    let t0 = Utc::now() - chrono::Duration::seconds(60);

    let run = TrainingRun {
        id: "run-unreachable".to_string(),
        remote_job_id: Some("job-unreachable".to_string()),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: TrainingRecipe::calibration(),
        recipe_digest: TrainingRecipe::calibration().digest(),
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        last_polled_at: Some(t0),
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 100,
        total_steps: 1500,
        elapsed_seconds: 25.0,
        cost_usd: 0.05,
        checkpoints: vec![],
        published_adapter_ids: vec![],
        error: None,
        failure_code: None,
        metrics: vec![],
    };
    catalog.save_training_run(&run).await.unwrap();

    // 1. Mock runner returning 500 Internal Server Error (unreachable / crashing)
    Mock::given(method("GET"))
        .and(path("/run/job-unreachable"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Runner Error"))
        .mount(&mock_server)
        .await;

    // Poll failure 1
    let polled = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(polled.state, TrainingRunState::Running);
    // last_polled_at MUST NOT be overwritten on failure!
    assert_eq!(polled.last_polled_at, Some(t0));
    assert_eq!(polled.consecutive_poll_failures, 1);
    assert_eq!(polled.poll_backoff_seconds, 30); // 15 * 2^1
    assert!(polled.last_poll_error.is_some());

    // Poll failure 2
    let polled = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(polled.last_polled_at, Some(t0));
    assert_eq!(polled.consecutive_poll_failures, 2);
    assert_eq!(polled.poll_backoff_seconds, 60); // 15 * 2^2

    // Poll failure 3
    let polled = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(polled.consecutive_poll_failures, 3);
    assert_eq!(polled.poll_backoff_seconds, 120); // 15 * 2^3

    // Poll failure 4
    let polled = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(polled.consecutive_poll_failures, 4);
    assert_eq!(polled.poll_backoff_seconds, 240); // 15 * 2^4

    // Poll failure 5 (caps at 300)
    let polled = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(polled.consecutive_poll_failures, 5);
    assert_eq!(polled.poll_backoff_seconds, 300); // capped at 300!
    assert_eq!(polled.last_polled_at, Some(t0));

    // Verify backoff helper math directly
    assert_eq!(compute_poll_backoff(0), 0);
    assert_eq!(compute_poll_backoff(1), 30);
    assert_eq!(compute_poll_backoff(2), 60);
    assert_eq!(compute_poll_backoff(3), 120);
    assert_eq!(compute_poll_backoff(4), 240);
    assert_eq!(compute_poll_backoff(5), 300);
    assert_eq!(compute_poll_backoff(10), 300);

    // 2. Runner recovers: mock 200 OK
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/run/job-unreachable"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "job-unreachable",
            "state": "running",
            "step": 200,
            "total_steps": 1500,
            "elapsed_seconds": 50.0,
            "cost_usd": 0.08,
            "checkpoints": [],
            "error": null
        })))
        .mount(&mock_server)
        .await;

    // Successful poll resets failures and backoff, and updates last_polled_at
    let recovered = state.training_service.poll_run("run-unreachable").await.unwrap();
    assert_eq!(recovered.consecutive_poll_failures, 0);
    assert_eq!(recovered.poll_backoff_seconds, 0);
    assert!(recovered.last_poll_error.is_none());
    assert!(recovered.last_polled_at.unwrap() > t0);
    assert_eq!(recovered.step, 200);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 3: Progress timeline (metrics.jsonl) survives restart
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_progress_timeline_metrics_survives_restart() {
    let data_dir = tempdir().unwrap();

    let catalog = Arc::new(FilesystemCatalogRepository::new(data_dir.path()))
        as Arc<dyn CatalogRepository + Send + Sync>;
    let blobs = Arc::new(FilesystemBlobStorage::new(data_dir.path()))
        as Arc<dyn BlobStorage + Send + Sync>;
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        data_dir.path().join("telemetry/cost_ledger.json"),
    ));

    let run_id = "run-timeline-test";
    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds-timeline").await;

    let run = TrainingRun {
        id: run_id.to_string(),
        remote_job_id: Some("job-timeline".to_string()),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot.id.clone(),
        dataset_digest: snapshot.sha256.clone(),
        dataset_url: snapshot.artifact_url.clone(),
        recipe: TrainingRecipe::calibration(),
        recipe_digest: TrainingRecipe::calibration().digest(),
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        last_polled_at: Some(Utc::now()),
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
    catalog.save_training_run(&run).await.unwrap();

    // Append 3 metric snapshots to metrics.jsonl
    let m1 = TrainingMetricSnapshot {
        timestamp: Utc::now() - chrono::Duration::seconds(30),
        step: 0,
        total_steps: 1500,
        elapsed_seconds: 0.0,
        cost_usd: 0.0,
        state: TrainingRunState::Running,
    };
    let m2 = TrainingMetricSnapshot {
        timestamp: Utc::now() - chrono::Duration::seconds(20),
        step: 250,
        total_steps: 1500,
        elapsed_seconds: 45.0,
        cost_usd: 0.08,
        state: TrainingRunState::Running,
    };
    let m3 = TrainingMetricSnapshot {
        timestamp: Utc::now() - chrono::Duration::seconds(10),
        step: 500,
        total_steps: 1500,
        elapsed_seconds: 90.0,
        cost_usd: 0.16,
        state: TrainingRunState::Running,
    };
    catalog.append_training_metric(run_id, &m1).await.unwrap();
    catalog.append_training_metric(run_id, &m2).await.unwrap();
    catalog.append_training_metric(run_id, &m3).await.unwrap();

    let state = AppState::with_cost_tracker(cost_tracker.clone(), catalog.clone(), blobs.clone());
    let app = create_router(state);

    // Verify GET /api/training/runs/{id}/metrics returns 3 snapshots
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/metrics"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let timeline: Vec<TrainingMetricSnapshot> = serde_json::from_slice(&body).unwrap();
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0].step, 0);
    assert_eq!(timeline[1].step, 250);
    assert_eq!(timeline[2].step, 500);

    // Verify get_run includes populated metrics
    let _run_with_metrics = catalog.get_training_run(run_id).await.unwrap();
    let service = curator::services::TrainingService::new(catalog.clone(), blobs.clone(), None, cost_tracker.clone());
    let fetched = service.get_run(run_id).await.unwrap();
    assert_eq!(fetched.metrics.len(), 3);

    // ─────────────────────────────────────────────────────────────────────────
    // Reconstruct across restart
    // ─────────────────────────────────────────────────────────────────────────
    let restored_catalog = Arc::new(FilesystemCatalogRepository::new(data_dir.path()))
        as Arc<dyn CatalogRepository + Send + Sync>;
    let restored_state = AppState::with_cost_tracker(cost_tracker.clone(), restored_catalog.clone(), blobs.clone());
    let restored_app = create_router(restored_state);

    // Verify historical snapshots remain intact after restart
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/metrics"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = restored_app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let restored_timeline: Vec<TrainingMetricSnapshot> = serde_json::from_slice(&body).unwrap();
    assert_eq!(restored_timeline.len(), 3);

    // Append new snapshot after restart
    let m4 = TrainingMetricSnapshot {
        timestamp: Utc::now(),
        step: 750,
        total_steps: 1500,
        elapsed_seconds: 135.0,
        cost_usd: 0.24,
        state: TrainingRunState::Running,
    };
    restored_catalog.append_training_metric(run_id, &m4).await.unwrap();

    let updated_timeline = restored_catalog.list_training_metrics(run_id).await.unwrap();
    assert_eq!(updated_timeline.len(), 4);
    assert_eq!(updated_timeline[3].step, 750);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 4: Serialized dispatch
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_serialized_dispatch_conflict_and_retention() {
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

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds-serialize").await;

    // Create run-1 and run-2 in queued state
    catalog
        .save_training_run(&TrainingRun {
            id: "run-first".to_string(),
            remote_job_id: None,
            state: TrainingRunState::Queued,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
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
        })
        .await
        .unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: "run-second".to_string(),
            remote_job_id: None,
            state: TrainingRunState::Queued,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
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
        })
        .await
        .unwrap();

    Mock::given(method("POST"))
        .and(path("/run"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "job-first",
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

    // Dispatch run-first -> transitions to running
    let req = Request::builder()
        .uri("/api/training/runs/run-first/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Attempt to dispatch run-second while run-first is running
    let req = Request::builder()
        .uri("/api/training/runs/run-second/dispatch")
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["code"], "RUN_ALREADY_ACTIVE");
    assert_eq!(err_json["active_run_id"], "run-first");

    // Verify run-second remained in queued state
    let run2_in_catalog = catalog.get_training_run("run-second").await.unwrap();
    assert_eq!(run2_in_catalog.state, TrainingRunState::Queued);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 5 & 6: Artifact verification states, full download, and no succeeded on unverified
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_artifact_verification_head_checks_and_full_download() {
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

    let snapshot = create_mock_snapshot(&catalog, &blobs, "ds-verify").await;

    // ─────────────────────────────────────────────────────────────────────────
    // Sub-test A: HEAD size mismatch blocks succeeded with "head_size"
    // ─────────────────────────────────────────────────────────────────────────
    let run_a_id = "run-head-size-mismatch";
    let cp_key = BlobKey::CheckpointAdapter {
        run_id: run_a_id.to_string(),
        step: 500,
    };
    let actual_bytes = b"real_bytes_len_19";
    blobs.write_blob(&cp_key, actual_bytes).await.unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: run_a_id.to_string(),
            remote_job_id: Some("job-a".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
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
        })
        .await
        .unwrap();

    // Modal reports succeeded with artifact_size = 9999 (size mismatch!)
    Mock::given(method("GET"))
        .and(path("/run/job-a"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "job-a",
            "state": "succeeded",
            "step": 500,
            "total_steps": 1500,
            "elapsed_seconds": 100.0,
            "cost_usd": 0.20,
            "checkpoints": [
                {
                    "step": 500,
                    "artifact_url": format!("https://r2.example.com/{}", cp_key.to_path()),
                    "artifact_sha256": null,
                    "artifact_size": 9999,
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
        .uri(format!("/api/training/runs/{run_a_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let polled_a: Value = serde_json::from_slice(&body).unwrap();

    // Must fail artifact verification with head_size check named
    assert_eq!(polled_a["state"], "failed");
    assert_eq!(polled_a["failure_code"], "ARTIFACT_VERIFICATION_FAILED");
    assert!(
        polled_a["error"].as_str().unwrap().contains("head_size"),
        "error must name head_size check: {:?}",
        polled_a["error"]
    );
    assert_eq!(
        polled_a["checkpoints"][0]["artifact_verification"]["method"],
        "head_size"
    );
    assert_eq!(
        polled_a["checkpoints"][0]["artifact_verification"]["status"],
        "mismatch"
    );

    // ─────────────────────────────────────────────────────────────────────────
    // Sub-test B: HEAD sha256 mismatch blocks succeeded with "head_sha256"
    // ─────────────────────────────────────────────────────────────────────────
    let run_b_id = "run-head-sha-mismatch";
    let cp_b_key = BlobKey::CheckpointAdapter {
        run_id: run_b_id.to_string(),
        step: 500,
    };
    let bytes_b = b"bytes_b_content";
    let sha_b = sha256_digest(bytes_b);
    blobs
        .write_blob_with_sha256(&cp_b_key, bytes_b, Some(&sha_b))
        .await
        .unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: run_b_id.to_string(),
            remote_job_id: Some("job-b".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
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
        })
        .await
        .unwrap();

    // Modal reports different sha256
    Mock::given(method("GET"))
        .and(path("/run/job-b"))
        .and(bearer_token(token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "job-b",
            "state": "succeeded",
            "step": 500,
            "total_steps": 1500,
            "elapsed_seconds": 100.0,
            "cost_usd": 0.20,
            "checkpoints": [
                {
                    "step": 500,
                    "artifact_url": format!("https://r2.example.com/{}", cp_b_key.to_path()),
                    "artifact_sha256": "wrong_expected_sha256",
                    "artifact_size": bytes_b.len(),
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
        .uri(format!("/api/training/runs/{run_b_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let polled_b: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(polled_b["state"], "failed");
    assert_eq!(polled_b["failure_code"], "ARTIFACT_VERIFICATION_FAILED");
    assert!(
        polled_b["error"].as_str().unwrap().contains("head_sha256"),
        "error must name head_sha256 check: {:?}",
        polled_b["error"]
    );

    // ─────────────────────────────────────────────────────────────────────────
    // Sub-test C: Full download POST /verify forces byte download & hash
    // ─────────────────────────────────────────────────────────────────────────
    let run_c_id = "run-full-verify";
    let cp_c_key = BlobKey::CheckpointAdapter {
        run_id: run_c_id.to_string(),
        step: 1000,
    };
    let c_bytes = b"adapter_model_weights_full_test_bytes";
    let c_sha = sha256_digest(c_bytes);
    blobs.write_blob(&cp_c_key, c_bytes).await.unwrap();

    let sample_c_key = BlobKey::CheckpointSamples {
        run_id: run_c_id.to_string(),
        step: 1000,
    };
    let sample_bytes = b"sample_grid_image_png_bytes";
    let sample_sha = sha256_digest(sample_bytes);
    blobs.write_blob(&sample_c_key, sample_bytes).await.unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: run_c_id.to_string(),
            remote_job_id: Some("job-c".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            step: 1000,
            total_steps: 1500,
            elapsed_seconds: 200.0,
            cost_usd: 0.35,
            checkpoints: vec![CheckpointRecord {
                step: 1000,
                artifact_key: cp_c_key.to_path(),
                artifact_url: format!("https://r2.example.com/{}", cp_c_key.to_path()),
                artifact_sha256: Some(c_sha.clone()),
                artifact_size: Some(c_bytes.len() as u64),
                artifact_verification: ArtifactVerification::pending(),
                sample_grid_key: Some(sample_c_key.to_path()),
                sample_grid_url: Some(format!("https://r2.example.com/{}", sample_c_key.to_path())),
                sample_grid_sha256: Some(sample_sha.clone()),
                sample_grid_size: Some(sample_bytes.len() as u64),
                sample_grid_verification: ArtifactVerification::pending(),
            }],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            metrics: vec![],
        })
        .await
        .unwrap();

    // Call POST /api/training/runs/{id}/verify
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_c_id}/verify"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let verified_c: Value = serde_json::from_slice(&body).unwrap();

    // Check that verification was performed with download_sha256
    let cp_resp = &verified_c["checkpoints"][0];
    assert_eq!(cp_resp["artifact_verification"]["status"], "verified");
    assert_eq!(cp_resp["artifact_verification"]["method"], "download_sha256");
    assert_eq!(cp_resp["sample_grid_verification"]["status"], "verified");
    assert_eq!(cp_resp["sample_grid_verification"]["method"], "download_sha256");

    // ─────────────────────────────────────────────────────────────────────────
    // Sub-test D: POST /verify with corrupted file fails with download_sha256
    // ─────────────────────────────────────────────────────────────────────────
    let run_d_id = "run-corrupted-verify";
    let cp_d_key = BlobKey::CheckpointAdapter {
        run_id: run_d_id.to_string(),
        step: 500,
    };
    let corrupt_bytes = b"corrupted_bytes_that_do_not_match_hash";
    blobs.write_blob(&cp_d_key, corrupt_bytes).await.unwrap();

    catalog
        .save_training_run(&TrainingRun {
            id: run_d_id.to_string(),
            remote_job_id: Some("job-d".to_string()),
            state: TrainingRunState::Running,
            dataset_snapshot_id: snapshot.id.clone(),
            dataset_digest: snapshot.sha256.clone(),
            dataset_url: snapshot.artifact_url.clone(),
            recipe: TrainingRecipe::calibration(),
            recipe_digest: TrainingRecipe::calibration().digest(),
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            step: 500,
            total_steps: 1500,
            elapsed_seconds: 100.0,
            cost_usd: 0.20,
            checkpoints: vec![CheckpointRecord {
                step: 500,
                artifact_key: cp_d_key.to_path(),
                artifact_url: format!("https://r2.example.com/{}", cp_d_key.to_path()),
                artifact_sha256: Some("expected_valid_sha256_which_differs".to_string()),
                artifact_size: Some(corrupt_bytes.len() as u64),
                artifact_verification: ArtifactVerification::pending(),
                sample_grid_key: None,
                sample_grid_url: None,
                sample_grid_sha256: None,
                sample_grid_size: None,
                sample_grid_verification: ArtifactVerification::pending(),
            }],
            published_adapter_ids: vec![],
            error: None,
            failure_code: None,
            metrics: vec![],
        })
        .await
        .unwrap();

    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_d_id}/verify"))
        .method("POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let verified_d: Value = serde_json::from_slice(&body).unwrap();

    // Must transition to failed with ARTIFACT_VERIFICATION_FAILED naming download_sha256
    assert_eq!(verified_d["state"], "failed");
    assert_eq!(verified_d["failure_code"], "ARTIFACT_VERIFICATION_FAILED");
    assert!(
        verified_d["error"].as_str().unwrap().contains("download_sha256"),
        "error must name download_sha256: {:?}",
        verified_d["error"]
    );
    assert_eq!(
        verified_d["checkpoints"][0]["artifact_verification"]["method"],
        "download_sha256"
    );
    assert_eq!(
        verified_d["checkpoints"][0]["artifact_verification"]["status"],
        "mismatch"
    );
}
