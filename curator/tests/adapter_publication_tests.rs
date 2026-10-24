//! Integration tests for Issue #217:
//! Checkpoint samples, operator publication and LoRA Adapter retirement (ADR-0008, ADR-0009).

use std::sync::Arc;
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use curator::adapters::{
    FileCostLedger, InMemoryBlobStorage, InMemoryCatalogRepository,
};
use curator::api::{create_router, AppState};
use curator::domain::{
    ArtifactVerification, CheckpointRecord,
    LoraAdapter, TrainingRecipe, TrainingRun, TrainingRunState,
};
use curator::ports::blob_storage::{BlobKey, BlobStat, BlobStorage, sha256_digest};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::cost_tracker::CostTracker;
use curator::ports::storage_error::StorageError;
use curator::services::{TrainingService, TrainingServiceError};

/// Helper constructing a test environment with router and services.
fn setup_test_app() -> (
    axum::Router,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
    TrainingService,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));
    let service = TrainingService::new(catalog.clone(), blobs.clone(), None, cost_tracker.clone());
    let state = AppState::with_cost_tracker(cost_tracker, catalog.clone(), blobs.clone());
    let router = create_router(state);
    (router, catalog, blobs, service)
}

/// Helper creating a succeeded training run ready for publication tests.
fn make_succeeded_run(run_id: &str, checkpoints: Vec<CheckpointRecord>) -> TrainingRun {
    let recipe = TrainingRecipe::calibration();
    TrainingRun {
        id: run_id.to_string(),
        remote_job_id: Some(format!("job-{run_id}")),
        state: TrainingRunState::Succeeded,
        dataset_snapshot_id: "snap-test-1".to_string(),
        dataset_digest: "sha256-snap-digest".to_string(),
        dataset_url: Some("https://r2.example.com/datasets/snap-test-1.zip".to_string()),
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
        elapsed_seconds: 1200.0,
        cost_usd: 1.95,
        checkpoints,
        published_adapter_ids: Vec::new(),
        error: None,
        failure_code: None,
        metrics: Vec::new(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 1: Checkpoint Sample Grid Requirement
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_checkpoint_sample_grid_requirement() {
    let (app, catalog, blobs, service) = setup_test_app();
    let run_id = "run-sample-req";
    let step = 1000;

    let adapter_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };
    let adapter_bytes = b"adapter_safetensors_bytes_step_1000";
    blobs.write_blob(&adapter_key, adapter_bytes).await.unwrap();

    // 1. Missing sample_grid_url (None)
    let run_no_samples = make_succeeded_run(
        run_id,
        vec![CheckpointRecord {
            step,
            artifact_key: adapter_key.to_path(),
            artifact_url: format!("https://r2.example.com/{}", adapter_key.to_path()),
            artifact_sha256: Some("sha_adapter".into()),
            artifact_size: Some(adapter_bytes.len() as u64),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: None,
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run_no_samples).await.unwrap();

    let err = service
        .publish_adapter(run_id, step, Some(0.75))
        .await
        .unwrap_err();
    match err {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("sample grid"),
                "Expected error message mentioning sample grid, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }

    // HTTP test: should return 400 Bad Request
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "step": step }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 2. Empty / whitespace sample_grid_url
    let mut run_empty_url = run_no_samples;
    run_empty_url.checkpoints[0].sample_grid_url = Some("   ".to_string());
    catalog.save_training_run(&run_empty_url).await.unwrap();

    let err = service
        .publish_adapter(run_id, step, Some(0.75))
        .await
        .unwrap_err();
    assert!(matches!(err, TrainingServiceError::InvalidState(_)));

    // 3. Sample grid verification status is Mismatch
    let mut run_mismatch = run_empty_url;
    run_mismatch.checkpoints[0].sample_grid_url =
        Some("https://r2.example.com/samples.png".to_string());
    run_mismatch.checkpoints[0].sample_grid_verification =
        ArtifactVerification::mismatch("stat", "checksum failed");
    catalog.save_training_run(&run_mismatch).await.unwrap();

    let err = service
        .publish_adapter(run_id, step, Some(0.75))
        .await
        .unwrap_err();
    match err {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("sample grid verification"),
                "Expected error mentioning sample grid verification, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }

    // 4. Valid sample_grid_url and non-mismatch verification -> succeeds
    let mut run_valid = run_mismatch;
    run_valid.checkpoints[0].sample_grid_verification = ArtifactVerification::pending();
    catalog.save_training_run(&run_valid).await.unwrap();

    // The run record lists every checkpoint with its step and sample grid URL.
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}"))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let run_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(run_json["checkpoints"][0]["step"], step);
    assert_eq!(
        run_json["checkpoints"][0]["sample_grid_url"],
        "https://r2.example.com/samples.png"
    );

    let published = service
        .publish_adapter(run_id, step, Some(0.75))
        .await
        .expect("Publication should succeed with valid sample grid");
    assert_eq!(published.id, format!("{run_id}-s{step}"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 2: Immutable Adapter Descriptor on Publication & Scale Validation
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_immutable_adapter_descriptor_and_scale_validation() {
    let (app, catalog, blobs, service) = setup_test_app();
    let run_id = "run-descriptor-test";
    let step = 1500;

    let adapter_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };
    let adapter_bytes = b"adapter_weights_safetensors_data";
    blobs.write_blob(&adapter_key, adapter_bytes).await.unwrap();

    let run = make_succeeded_run(
        run_id,
        vec![CheckpointRecord {
            step,
            artifact_key: adapter_key.to_path(),
            artifact_url: format!("https://r2.example.com/{}", adapter_key.to_path()),
            artifact_sha256: Some("explicit_sha256_hex".into()),
            artifact_size: Some(adapter_bytes.len() as u64),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: Some(format!("artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png")),
            sample_grid_url: Some(format!("https://r2.example.com/artifacts/runs/{run_id}/checkpoints/step-{step}/samples.png")),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run).await.unwrap();

    // 1. Non-positive or NaN scale rejected via API (HTTP 400) and service
    for invalid_scale in [0.0, -0.5, -100.0] {
        let req = Request::builder()
            .uri(format!("/api/training/runs/{run_id}/publish"))
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({ "step": step, "scale": invalid_scale }).to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let err = service
            .publish_adapter(run_id, step, Some(invalid_scale))
            .await
            .unwrap_err();
        assert!(matches!(err, TrainingServiceError::InvalidState(_)));
    }

    // NaN scale rejected
    let err_nan = service
        .publish_adapter(run_id, step, Some(f64::NAN))
        .await
        .unwrap_err();
    assert!(matches!(err_nan, TrainingServiceError::InvalidState(_)));

    // 2. Publication with explicit scale via HTTP
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "step": step, "scale": 0.85 }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let desc: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(desc["id"], format!("{run_id}-s{step}"));
    assert_eq!(desc["run_id"], run_id);
    assert_eq!(desc["step"], step);
    assert_eq!(desc["scale"], 0.85);
    assert_eq!(desc["base_model"], run.recipe.base_model);
    assert_eq!(desc["trigger_phrase"], run.recipe.trigger_phrase);
    assert_eq!(desc["artifact_key"], adapter_key.to_path());
    assert_eq!(desc["sha256"], "explicit_sha256_hex");
    assert_eq!(desc["size_bytes"], adapter_bytes.len());
    assert_eq!(desc["recipe_digest"], run.recipe_digest);
    assert_eq!(desc["dataset_snapshot_id"], run.dataset_snapshot_id);
    assert_eq!(desc["dataset_digest"], run.dataset_digest);
    assert_eq!(desc["retired"], false);
    assert!(desc["retired_at"].is_null());

    // 3. Step not found in checkpoints returns InvalidState
    let err_step = service
        .publish_adapter(run_id, 9999, Some(0.75))
        .await
        .unwrap_err();
    assert!(matches!(err_step, TrainingServiceError::InvalidState(_)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 2: SHA-256 Resolution Modes
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_adapter_sha256_resolution_modes() {
    let (_, catalog, blobs, service) = setup_test_app();

    // Mode A: Resolved from checkpoint's artifact_sha256
    let run_a = "run-sha-a";
    let step_a = 500;
    let key_a = BlobKey::CheckpointAdapter {
        run_id: run_a.to_string(),
        step: step_a,
    };
    blobs.write_blob(&key_a, b"weights_a").await.unwrap();
    let run_rec_a = make_succeeded_run(
        run_a,
        vec![CheckpointRecord {
            step: step_a,
            artifact_key: key_a.to_path(),
            artifact_url: format!("https://r2.example.com/{}", key_a.to_path()),
            artifact_sha256: Some("sha_from_checkpoint_record".to_string()),
            artifact_size: Some(9),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: Some("https://r2.example.com/samples_a.png".to_string()),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run_rec_a).await.unwrap();
    let adapter_a = service.publish_adapter(run_a, step_a, None).await.unwrap();
    assert_eq!(
        adapter_a.sha256,
        Some("sha_from_checkpoint_record".to_string())
    );
    // Default scale should be 0.75
    assert_eq!(adapter_a.scale, 0.75);

    // Mode B: Resolved from blob stat.sha256 when checkpoint artifact_sha256 is None
    let run_b = "run-sha-b";
    let step_b = 500;
    let key_b = BlobKey::CheckpointAdapter {
        run_id: run_b.to_string(),
        step: step_b,
    };
    blobs
        .write_blob_with_sha256(&key_b, b"weights_b", Some("sha_from_stat_header"))
        .await
        .unwrap();
    let run_rec_b = make_succeeded_run(
        run_b,
        vec![CheckpointRecord {
            step: step_b,
            artifact_key: key_b.to_path(),
            artifact_url: format!("https://r2.example.com/{}", key_b.to_path()),
            artifact_sha256: None,
            artifact_size: Some(9),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: Some("https://r2.example.com/samples_b.png".to_string()),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run_rec_b).await.unwrap();
    let adapter_b = service.publish_adapter(run_b, step_b, None).await.unwrap();
    assert_eq!(adapter_b.sha256, Some("sha_from_stat_header".to_string()));

    // Mode C: Computed from blob bytes when both checkpoint and stat sha256 are None
    let run_c = "run-sha-c";
    let step_c = 500;
    let key_c = BlobKey::CheckpointAdapter {
        run_id: run_c.to_string(),
        step: step_c,
    };
    let weights_c = b"weights_data_computed_on_demand";
    let expected_hash = sha256_digest(weights_c);
    blobs.write_blob(&key_c, weights_c).await.unwrap();
    let run_rec_c = make_succeeded_run(
        run_c,
        vec![CheckpointRecord {
            step: step_c,
            artifact_key: key_c.to_path(),
            artifact_url: format!("https://r2.example.com/{}", key_c.to_path()),
            artifact_sha256: None,
            artifact_size: None,
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: Some("https://r2.example.com/samples_c.png".to_string()),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run_rec_c).await.unwrap();
    let adapter_c = service.publish_adapter(run_c, step_c, None).await.unwrap();
    assert_eq!(adapter_c.sha256, Some(expected_hash));
    assert_eq!(adapter_c.size_bytes, Some(weights_c.len() as u64));
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 3: Conflict on Duplicate Step Publication
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_conflict_on_duplicate_step_publication() {
    let (app, catalog, blobs, service) = setup_test_app();
    let run_id = "run-dup-test";
    let step = 1000;

    let adapter_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };
    blobs.write_blob(&adapter_key, b"adapter_data").await.unwrap();

    let run = make_succeeded_run(
        run_id,
        vec![CheckpointRecord {
            step,
            artifact_key: adapter_key.to_path(),
            artifact_url: format!("https://r2.example.com/{}", adapter_key.to_path()),
            artifact_sha256: Some("sha_dup".into()),
            artifact_size: Some(12),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: Some("https://r2.example.com/samples_dup.png".to_string()),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run).await.unwrap();

    // 1. First publication succeeds
    let first = service
        .publish_adapter(run_id, step, Some(0.8))
        .await
        .unwrap();
    assert_eq!(first.scale, 0.8);

    // 2. Second publication via service returns Conflict
    let err = service
        .publish_adapter(run_id, step, Some(0.9))
        .await
        .unwrap_err();
    match err {
        TrainingServiceError::Conflict(msg) => {
            assert!(
                msg.contains("already been published"),
                "Expected conflict message, got: {msg}"
            );
        }
        other => panic!("Expected Conflict, got {other:?}"),
    }

    // 3. Second publication via HTTP returns 409 Conflict
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "step": step, "scale": 0.9 }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 4. Verify existing adapter in catalog was not overwritten
    let adapter_id = format!("{run_id}-s{step}");
    let stored = catalog.get_lora_adapter(&adapter_id).await.unwrap();
    assert_eq!(stored.scale, 0.8, "Original scale must remain unchanged");

    // 5. Run published_adapter_ids list contains adapter_id once
    let run_updated = catalog.get_training_run(run_id).await.unwrap();
    assert_eq!(
        run_updated
            .published_adapter_ids
            .iter()
            .filter(|id| *id == &adapter_id)
            .count(),
        1
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 4: Traceable Adapter Identity (parse_id)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_traceable_adapter_identity_parse_id() {
    // Standard adapter IDs
    assert_eq!(
        LoraAdapter::parse_id("run-123-s250"),
        Some(("run-123", 250))
    );
    // The `-edit` suffix belongs to the engine's job identity (#219), never an adapter ID (#217).
    assert_eq!(LoraAdapter::parse_id("run-123-s250-edit"), None);
    assert_eq!(
        LoraAdapter::parse_id("run-20261024-abc-s1500"),
        Some(("run-20261024-abc", 1500))
    );
    assert_eq!(LoraAdapter::parse_id("run-20261024-abc-s1500-edit"), None);
    assert_eq!(
        LoraAdapter::parse_id("complex-id-with-s-letters-s42"),
        Some(("complex-id-with-s-letters", 42))
    );
    assert_eq!(
        LoraAdapter::parse_id("complex-id-with-s-letters-s42-edit"),
        None
    );
    assert_eq!(
        LoraAdapter::parse_id("modal-job-42-s0"),
        Some(("modal-job-42", 0))
    );
    assert_eq!(LoraAdapter::parse_id("modal-job-42-s0-edit"), None);

    // Round-trip with format_id
    let formatted = LoraAdapter::format_id("run-xyz", 3000);
    assert_eq!(LoraAdapter::parse_id(&formatted), Some(("run-xyz", 3000)));
    let formatted_edit = format!("{formatted}-edit");
    assert_eq!(LoraAdapter::parse_id(&formatted_edit), None);

    // Invalid formats
    assert_eq!(LoraAdapter::parse_id(""), None);
    assert_eq!(LoraAdapter::parse_id("invalid-without-step"), None);
    assert_eq!(LoraAdapter::parse_id("-s250"), None);
    assert_eq!(LoraAdapter::parse_id("-s250-edit"), None);
    assert_eq!(LoraAdapter::parse_id("run-s"), None);
    assert_eq!(LoraAdapter::parse_id("run-s-edit"), None);
    assert_eq!(LoraAdapter::parse_id("run-snotanumber"), None);
    assert_eq!(LoraAdapter::parse_id("run-s100-other"), None);
    assert_eq!(LoraAdapter::parse_id("s250"), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 5: Retirement Preserves History
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_adapter_retirement_preserves_history() {
    let (app, catalog, blobs, service) = setup_test_app();
    let run_id = "run-retire-test";

    // Write underlying checkpoint artifacts for two checkpoints
    for step in [500, 1000] {
        let key = BlobKey::CheckpointAdapter {
            run_id: run_id.to_string(),
            step,
        };
        blobs.write_blob(&key, b"weights").await.unwrap();
    }

    let run = make_succeeded_run(
        run_id,
        vec![
            CheckpointRecord {
                step: 500,
                artifact_key: format!("artifacts/runs/{run_id}/checkpoints/step-500/adapter.safetensors"),
                artifact_url: format!("https://r2.example.com/artifacts/runs/{run_id}/checkpoints/step-500/adapter.safetensors"),
                artifact_sha256: Some("sha_500".into()),
                artifact_size: Some(7),
                artifact_verification: ArtifactVerification::verified("stat", None),
                sample_grid_key: None,
                sample_grid_url: Some("https://r2.example.com/s500.png".into()),
                sample_grid_sha256: None,
                sample_grid_size: None,
                sample_grid_verification: ArtifactVerification::pending(),
            },
            CheckpointRecord {
                step: 1000,
                artifact_key: format!("artifacts/runs/{run_id}/checkpoints/step-1000/adapter.safetensors"),
                artifact_url: format!("https://r2.example.com/artifacts/runs/{run_id}/checkpoints/step-1000/adapter.safetensors"),
                artifact_sha256: Some("sha_1000".into()),
                artifact_size: Some(7),
                artifact_verification: ArtifactVerification::verified("stat", None),
                sample_grid_key: None,
                sample_grid_url: Some("https://r2.example.com/s1000.png".into()),
                sample_grid_sha256: None,
                sample_grid_size: None,
                sample_grid_verification: ArtifactVerification::pending(),
            },
        ],
    );
    catalog.save_training_run(&run).await.unwrap();

    let adapter1 = service.publish_adapter(run_id, 500, Some(0.75)).await.unwrap();
    let adapter2 = service.publish_adapter(run_id, 1000, Some(0.8)).await.unwrap();

    // Both adapters selectable initially
    let selectable = service.list_adapters(true).await.unwrap();
    assert_eq!(selectable.len(), 2);

    // 1. Soft-retire adapter1 via HTTP POST /api/training/adapters/{id}/retire
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{}/retire", adapter1.id))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "retired": true }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let retired_res: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(retired_res["retired"], true);
    assert!(!retired_res["retired_at"].is_null());

    // 2. Selectable list excludes retired adapter1
    let req = Request::builder()
        .uri("/api/training/adapters?selectable=true")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let sel_list: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sel_list.as_array().unwrap().len(), 1);
    assert_eq!(sel_list[0]["id"], adapter2.id);

    // 3. Full list (selectable=false) still includes retired adapter1
    let req = Request::builder()
        .uri("/api/training/adapters")
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let all_list: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(all_list.as_array().unwrap().len(), 2);

    // 4. Detail endpoint /api/training/adapters/{id} continues to resolve for retired adapter
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{}", adapter1.id))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let detail: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["id"], adapter1.id);
    assert_eq!(detail["retired"], true);

    // 5. Restore adapter1 via POST /api/training/adapters/{id}/retire with retired: false
    let req = Request::builder()
        .uri(format!("/api/training/adapters/{}/retire", adapter1.id))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "retired": false }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Both selectable again
    let selectable_after = service.list_adapters(true).await.unwrap();
    assert_eq!(selectable_after.len(), 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Criterion 6: Missing or Unreadable Artifact Never Yields a Publishable Adapter
// ─────────────────────────────────────────────────────────────────────────────

/// Custom storage wrapper where read_blob fails with an I/O error to test unreadable artifacts.
struct UnreadableBlobStorage {
    inner: Arc<InMemoryBlobStorage>,
}

#[async_trait]
impl BlobStorage for UnreadableBlobStorage {
    async fn read_blob(&self, _key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::Io(std::io::Error::other(
            "Corrupted disk or unreadable payload",
        )))
    }

    async fn write_blob(&self, key: &BlobKey, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write_blob(key, data).await
    }

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        data: &[u8],
        expected_sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        self.inner.write_blob_with_sha256(key, data, expected_sha256).await
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        self.inner.delete_blob(key).await
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        self.inner.exists_blob(key).await
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.inner.blob_size(key).await
    }

    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        // Mirrors a backend that records no digest, so publication must fall back to reading bytes.
        Ok(BlobStat {
            size: self.inner.blob_size(key).await?,
            sha256: None,
        })
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        self.inner.list_prefix(prefix).await
    }
}

#[tokio::test]
async fn test_missing_or_unreadable_or_unverified_artifact_refuses_publication() {
    let (app, catalog, blobs, service) = setup_test_app();
    let run_id = "run-artifact-checks";
    let step = 1500;

    let adapter_key = BlobKey::CheckpointAdapter {
        run_id: run_id.to_string(),
        step,
    };

    // Case 1: Artifact completely missing in storage
    let run_missing = make_succeeded_run(
        run_id,
        vec![CheckpointRecord {
            step,
            artifact_key: adapter_key.to_path(),
            artifact_url: format!("https://r2.example.com/{}", adapter_key.to_path()),
            artifact_sha256: Some("sha_missing".into()),
            artifact_size: Some(100),
            artifact_verification: ArtifactVerification::verified("stat", None),
            sample_grid_key: None,
            sample_grid_url: Some("https://r2.example.com/samples.png".into()),
            sample_grid_sha256: None,
            sample_grid_size: None,
            sample_grid_verification: ArtifactVerification::pending(),
        }],
    );
    catalog.save_training_run(&run_missing).await.unwrap();

    let err_missing = service.publish_adapter(run_id, step, None).await.unwrap_err();
    match err_missing {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("missing in storage"),
                "Expected error mentioning missing in storage, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }

    // HTTP test: should return 400 Bad Request
    let req = Request::builder()
        .uri(format!("/api/training/runs/{run_id}/publish"))
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "step": step }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Case 2: Artifact verification status is Pending
    blobs.write_blob(&adapter_key, b"valid_bytes").await.unwrap();
    let mut run_pending = run_missing.clone();
    run_pending.checkpoints[0].artifact_verification = ArtifactVerification::pending();
    catalog.save_training_run(&run_pending).await.unwrap();

    let err_pending = service.publish_adapter(run_id, step, None).await.unwrap_err();
    match err_pending {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("not verified"),
                "Expected error mentioning not verified, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }

    // Case 3: Artifact verification status is Mismatch
    let mut run_mismatch = run_missing.clone();
    run_mismatch.checkpoints[0].artifact_verification =
        ArtifactVerification::mismatch("stat", "size mismatch");
    catalog.save_training_run(&run_mismatch).await.unwrap();

    let err_mismatch = service.publish_adapter(run_id, step, None).await.unwrap_err();
    assert!(matches!(err_mismatch, TrainingServiceError::InvalidState(_)));

    // Case 4: No digest on the checkpoint or in storage, so publication must read the blob and the
    // read itself fails with an I/O error.
    let unreadable_blobs = Arc::new(UnreadableBlobStorage {
        inner: blobs.clone(),
    });
    let ledger_dir = tempdir().unwrap();
    let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::new(
        ledger_dir.path().join("telemetry/cost_ledger.json"),
    ));
    let unreadable_service = TrainingService::new(
        catalog.clone(),
        unreadable_blobs,
        None,
        cost_tracker,
    );

    let mut run_verified = run_missing.clone();
    run_verified.checkpoints[0].artifact_verification =
        ArtifactVerification::verified("stat", None);
    run_verified.checkpoints[0].artifact_sha256 = None;
    catalog.save_training_run(&run_verified).await.unwrap();

    let err_unreadable = unreadable_service
        .publish_adapter(run_id, step, None)
        .await
        .unwrap_err();
    match err_unreadable {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("unreadable in storage"),
                "Expected error mentioning unreadable in storage, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }

    // Case 5: Run is not in Succeeded state (e.g. Running)
    let mut run_running = run_verified;
    run_running.state = TrainingRunState::Running;
    catalog.save_training_run(&run_running).await.unwrap();

    let err_state = service.publish_adapter(run_id, step, None).await.unwrap_err();
    match err_state {
        TrainingServiceError::InvalidState(msg) => {
            assert!(
                msg.contains("only succeeded runs can publish adapters"),
                "Expected error mentioning only succeeded runs, got: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got {other:?}"),
    }
}
