//! Service coordinating Training Run lifecycle, dispatch serialization, polling, and LoRA adapter catalog (ADR-0008, ADR-0009).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{
    ArtifactVerification, ArtifactVerificationState, CheckpointRecord, LoraAdapter,
    TrainingMetricSnapshot, TrainingRecipe, TrainingRun, TrainingRunState,
};
use crate::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::cost_tracker::{CostTracker, CostTrackerError, TrainingCostRecord};
use crate::ports::storage_error::StorageError;
use crate::ports::training_runner::{
    RunnerCheckpoint, TrainingRunner, TrainingRunnerError,
};

static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Computes exponential poll backoff in seconds for unreachable runner: 15 * 2^failures capped at 300s.
pub fn compute_poll_backoff(consecutive_failures: u32) -> u64 {
    if consecutive_failures == 0 {
        return 0;
    }
    let multiplier = 1u64.checked_shl(consecutive_failures).unwrap_or(u64::MAX);
    15u64.saturating_mul(multiplier).min(300)
}

/// Compares an artifact's observed size and checksum against its expectations and returns the
/// resulting verification record.
///
/// A size mismatch is reported under `size_method`; a checksum mismatch under `sha_method`. A
/// matching artifact is recorded as `sha_method`, falling back to `size_only` when no checksum
/// could be compared and `allow_size_only` is set (HEAD verification without a stored digest).
fn compare_artifact(
    expected_size: Option<u64>,
    expected_sha: Option<&str>,
    actual_size: u64,
    actual_sha: Option<&str>,
    size_method: &str,
    sha_method: &str,
    allow_size_only: bool,
) -> ArtifactVerification {
    if let Some(expected_size) = expected_size {
        if actual_size != expected_size {
            return ArtifactVerification::mismatch(
                size_method,
                format!(
                    "Size mismatch: expected {}, got {}",
                    expected_size, actual_size
                ),
            );
        }
    }
    match (expected_sha, actual_sha) {
        (Some(expected), Some(actual)) if expected != actual => ArtifactVerification::mismatch(
            sha_method,
            format!("Checksum mismatch: expected {}, got {}", expected, actual),
        ),
        (_, Some(_)) => ArtifactVerification::verified(sha_method, None),
        _ if allow_size_only => ArtifactVerification::verified("size_only", None),
        _ => ArtifactVerification::verified(sha_method, None),
    }
}

/// Errors originating from `TrainingService` operations.
#[derive(Debug, Error)]
pub enum TrainingServiceError {
    #[error("Modal runner is not configured: {0}")]
    ModalNotConfigured(String),

    #[error("Training run '{0}' was not found")]
    RunNotFound(String),

    #[error("LoRA adapter '{0}' was not found")]
    AdapterNotFound(String),

    #[error("Dataset snapshot '{0}' was not found")]
    SnapshotNotFound(String),

    #[error("Dataset snapshot '{0}' ZIP archive blob is missing from storage")]
    SnapshotBlobMissing(String),

    #[error("Cannot dispatch training run: another run '{active_run_id}' is currently running")]
    RunAlreadyActive { active_run_id: String },

    #[error("Invalid operation for run state: {0}")]
    InvalidState(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Runner error: {0}")]
    Runner(#[from] TrainingRunnerError),

    #[error("Cost tracker error: {0}")]
    Cost(#[from] CostTrackerError),
}

/// Request parameters for creating a new training run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrainingRunRequest {
    /// ID of the frozen DatasetSnapshot to fine-tune on.
    pub dataset_snapshot_id: String,
    /// Explicit hyperparameters (if omitted, policy preset or calibration default is used).
    #[serde(default)]
    pub recipe: Option<TrainingRecipe>,
    /// Policy preset ("calibration" or "full").
    #[serde(default)]
    pub policy: Option<String>,
    /// Optional explicit run ID (generated if omitted).
    #[serde(default)]
    pub id: Option<String>,
}

/// Service managing the lifecycle of training runs and published LoRA adapters.
#[derive(Clone)]
pub struct TrainingService {
    catalog: Arc<dyn CatalogRepository + Send + Sync>,
    blobs: Arc<dyn BlobStorage + Send + Sync>,
    runner: Option<Arc<dyn TrainingRunner + Send + Sync>>,
    cost_tracker: Arc<dyn CostTracker + Send + Sync>,
}

impl TrainingService {
    /// Constructs a new `TrainingService`.
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
        runner: Option<Arc<dyn TrainingRunner + Send + Sync>>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        Self {
            catalog,
            blobs,
            runner,
            cost_tracker,
        }
    }

    /// Creates a new `TrainingRun` in `queued` state from a frozen `DatasetSnapshot`.
    pub async fn create_training_run(
        &self,
        req: CreateTrainingRunRequest,
    ) -> Result<TrainingRun, TrainingServiceError> {
        // 1. Verify snapshot exists in CatalogRepository
        let snapshot = match self.catalog.get_dataset_snapshot(&req.dataset_snapshot_id).await {
            Ok(s) => s,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::SnapshotNotFound(req.dataset_snapshot_id));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        // 2. Verify snapshot ZIP archive exists in BlobStorage
        let snapshot_blob_key = BlobKey::DatasetSnapshot {
            id: snapshot.id.clone(),
        };
        if self.blobs.stat_blob(&snapshot_blob_key).await.is_err() {
            return Err(TrainingServiceError::SnapshotBlobMissing(snapshot.id.clone()));
        }

        // 3. Resolve recipe and policy
        let recipe = if let Some(r) = req.recipe {
            r
        } else if let Some(p) = req.policy {
            TrainingRecipe::for_policy(&p)
                .map_err(TrainingServiceError::InvalidState)?
        } else {
            TrainingRecipe::calibration()
        };

        let recipe_digest = recipe.digest();
        let dataset_url = snapshot.artifact_url.clone().or_else(|| {
            std::env::var("R2_PUBLIC_URL")
                .ok()
                .and_then(|u| snapshot_blob_key.public_url(&u))
        });

        // 4. Generate unique run ID
        let run_id = req.id.unwrap_or_else(|| {
            let ts = Utc::now().format("%Y%m%d_%H%M%S");
            let counter = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
            format!("run_{}_{:02}", ts, counter)
        });

        let total_steps = recipe.steps;

        let run = TrainingRun {
            id: run_id,
            remote_job_id: None,
            state: TrainingRunState::Queued,
            dataset_snapshot_id: snapshot.id,
            dataset_digest: snapshot.sha256,
            dataset_url,
            recipe,
            recipe_digest,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            last_polled_at: None,
            last_poll_error: None,
            consecutive_poll_failures: 0,
            poll_backoff_seconds: 0,
            step: 0,
            total_steps,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: Vec::new(),
            published_adapter_ids: Vec::new(),
            error: None,
            failure_code: None,
            metrics: Vec::new(),
        };

        self.catalog.save_training_run(&run).await?;
        Ok(run)
    }

    /// Dispatches a `queued` run by handing the dataset and recipe to the training runner.
    /// Serializes execution: refuses with `RUN_ALREADY_ACTIVE` if another run is currently `running`.
    pub async fn dispatch_run(&self, id: &str) -> Result<TrainingRun, TrainingServiceError> {
        let runner = self.runner.as_ref().ok_or_else(|| {
            TrainingServiceError::ModalNotConfigured(
                "CURATOR_MODAL_ENDPOINT or CURATOR_MODAL_TOKEN is not configured".to_string(),
            )
        })?;

        let mut run = match self.catalog.get_training_run(id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if run.state != TrainingRunState::Queued {
            return Err(TrainingServiceError::InvalidState(format!(
                "Run '{}' is in state '{}' and cannot be dispatched; must be 'queued'",
                id,
                run.state.as_str()
            )));
        }

        // Serialization check: refuse if another run is running
        let existing_runs = self.catalog.list_training_runs().await?;
        if let Some(active) = existing_runs
            .iter()
            .find(|r| r.id != id && r.state == TrainingRunState::Running)
        {
            return Err(TrainingServiceError::RunAlreadyActive {
                active_run_id: active.id.clone(),
            });
        }

        // Submit to training runner
        let submit_resp = runner.submit_run(&run).await?;

        run.state = TrainingRunState::Running;
        run.remote_job_id = Some(submit_resp.run_id);
        run.started_at = Some(Utc::now());
        run.last_polled_at = Some(Utc::now());
        run.last_poll_error = None;
        run.consecutive_poll_failures = 0;
        run.poll_backoff_seconds = 0;
        self.catalog.save_training_run(&run).await?;

        // Record initial metric snapshot
        let initial_metric = TrainingMetricSnapshot {
            timestamp: Utc::now(),
            step: 0,
            total_steps: run.total_steps,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            state: TrainingRunState::Running,
        };
        let _ = self.catalog.append_training_metric(&run.id, &initial_metric).await;

        // Start adaptive polling so a dispatched run advances without an external poller.
        let service = self.clone();
        let run_id = run.id.clone();
        tokio::spawn(async move {
            service.adaptive_polling_loop(&run_id).await;
        });

        Ok(run)
    }

    /// Retrieves run metadata, polling runner progress if currently active.
    pub async fn get_run(&self, id: &str) -> Result<TrainingRun, TrainingServiceError> {
        let mut run = match self.catalog.get_training_run(id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if run.state == TrainingRunState::Running {
            self.poll_active_run_internal(&mut run).await?;
        }
        run.metrics = self.catalog.list_training_metrics(id).await.unwrap_or_default();

        Ok(run)
    }

    /// Explicitly polls active run progress and updates stored checkpoints and metrics.
    pub async fn poll_run(&self, id: &str) -> Result<TrainingRun, TrainingServiceError> {
        let mut run = match self.catalog.get_training_run(id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if run.state == TrainingRunState::Running {
            self.poll_active_run_internal(&mut run).await?;
        }
        run.metrics = self.catalog.list_training_metrics(id).await.unwrap_or_default();

        Ok(run)
    }

    /// Cancels a running or queued training run and records accrued cost.
    pub async fn cancel_run(&self, id: &str) -> Result<TrainingRun, TrainingServiceError> {
        let mut run = match self.catalog.get_training_run(id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if run.state.is_terminal() {
            return Ok(run);
        }

        if let Some(runner) = &self.runner {
            if let Some(remote_id) = &run.remote_job_id {
                let _ = runner.cancel_run(remote_id).await;
            }
        }

        run.state = TrainingRunState::Cancelled;
        run.finished_at = Some(Utc::now());

        if run.cost_usd > 0.0 {
            let cost_rec = TrainingCostRecord::new(
                &run.id,
                &run.recipe.policy,
                run.step,
                run.elapsed_seconds,
                run.cost_usd,
                run.state.as_str(),
            );
            let _ = self.cost_tracker.record_training_cost(cost_rec).await;
        }

        self.catalog.save_training_run(&run).await?;
        Ok(run)
    }

    /// Lists all training runs ordered newest first.
    pub async fn list_runs(&self) -> Result<Vec<TrainingRun>, TrainingServiceError> {
        let runs = self.catalog.list_training_runs().await?;
        Ok(runs)
    }

    /// Returns chronological metric snapshots recorded in `metrics.jsonl` for a training run.
    pub async fn get_run_metrics(
        &self,
        id: &str,
    ) -> Result<Vec<TrainingMetricSnapshot>, TrainingServiceError> {
        match self.catalog.get_training_run(id).await {
            Ok(_) => {}
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        }

        let metrics = self.catalog.list_training_metrics(id).await?;
        Ok(metrics)
    }

    /// Finds every non-terminal `running` run across restarts, re-polls it immediately,
    /// and spawns/continues adaptive polling for runs that stay `running`.
    pub async fn resume_non_terminal_runs(&self) -> Result<Vec<TrainingRun>, TrainingServiceError> {
        let runs = self.catalog.list_training_runs().await?;
        let mut resumed = Vec::new();

        for mut run in runs {
            if run.state.is_terminal() {
                continue;
            }
            // A run with a remote job was dispatched: it must never be stranded in `queued`
            // across a restart, so treat it as running and poll it immediately.
            if run.state == TrainingRunState::Running || run.remote_job_id.is_some() {
                run.state = TrainingRunState::Running;
                // A single failing run MUST NOT abort the boot sweep for the remaining runs.
                if let Err(err) = self.poll_active_run_internal(&mut run).await {
                    tracing::warn!(
                        run_id = %run.id,
                        error = %err,
                        "Failed to resume polling for training run; continuing sweep"
                    );
                }
                if run.state == TrainingRunState::Running {
                    let service = self.clone();
                    let run_id = run.id.clone();
                    tokio::spawn(async move {
                        service.adaptive_polling_loop(&run_id).await;
                    });
                }
                resumed.push(run);
            }
        }

        Ok(resumed)
    }

    /// Background polling loop with exponential backoff for an active training run.
    pub async fn adaptive_polling_loop(&self, run_id: &str) {
        loop {
            let run = match self.catalog.get_training_run(run_id).await {
                Ok(r) => r,
                Err(_) => break,
            };
            if run.state.is_terminal() {
                break;
            }
            let delay_secs = if run.poll_backoff_seconds > 0 {
                run.poll_backoff_seconds
            } else {
                15
            };
            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;

            if let Ok(r) = self.poll_run(run_id).await {
                if r.state.is_terminal() {
                    break;
                }
            }
        }
    }

    /// Internal polling helper: polls runner, updates checkpoints and metrics, verifies on success.
    async fn poll_active_run_internal(
        &self,
        run: &mut TrainingRun,
    ) -> Result<(), TrainingServiceError> {
        let runner = match self.runner.as_ref() {
            Some(r) => r,
            None => {
                run.last_poll_error = Some("Modal runner is not configured".to_string());
                run.consecutive_poll_failures = run.consecutive_poll_failures.saturating_add(1);
                run.poll_backoff_seconds = compute_poll_backoff(run.consecutive_poll_failures);
                self.catalog.save_training_run(run).await?;
                return Ok(());
            }
        };

        let remote_id = match run.remote_job_id.as_deref() {
            Some(id) => id,
            None => &run.id,
        };

        let poll_resp = match runner.poll_run(remote_id).await {
            Ok(resp) => resp,
            Err(TrainingRunnerError::NotFound(job_id)) => {
                let job_name = if job_id.is_empty() { remote_id } else { &job_id };
                run.state = TrainingRunState::Failed;
                run.failure_code = Some("LOST_REMOTE".to_string());
                run.error = Some(format!("Remote runner job '{job_name}' not found"));
                run.finished_at = Some(Utc::now());
                run.last_polled_at = Some(Utc::now());
                run.consecutive_poll_failures = 0;
                run.poll_backoff_seconds = 0;

                if run.cost_usd > 0.0 {
                    let cost_rec = TrainingCostRecord::new(
                        &run.id,
                        &run.recipe.policy,
                        run.step,
                        run.elapsed_seconds,
                        run.cost_usd,
                        run.state.as_str(),
                    );
                    let _ = self.cost_tracker.record_training_cost(cost_rec).await;
                }

                self.catalog.save_training_run(run).await?;
                return Ok(());
            }
            Err(err) => {
                run.last_poll_error = Some(err.to_string());
                run.consecutive_poll_failures = run.consecutive_poll_failures.saturating_add(1);
                run.poll_backoff_seconds = compute_poll_backoff(run.consecutive_poll_failures);
                self.catalog.save_training_run(run).await?;
                return Ok(());
            }
        };

        // Successful poll: update progress, reset consecutive failures and backoff
        run.step = poll_resp.step;
        run.total_steps = poll_resp.total_steps;
        run.elapsed_seconds = poll_resp.elapsed_seconds;
        run.cost_usd = poll_resp.cost_usd;
        run.last_polled_at = Some(Utc::now());
        run.last_poll_error = None;
        run.consecutive_poll_failures = 0;
        run.poll_backoff_seconds = 0;

        // Append metric snapshot
        let metric = TrainingMetricSnapshot {
            timestamp: Utc::now(),
            step: poll_resp.step,
            total_steps: poll_resp.total_steps,
            elapsed_seconds: poll_resp.elapsed_seconds,
            cost_usd: poll_resp.cost_usd,
            state: poll_resp.state,
        };
        let _ = self.catalog.append_training_metric(&run.id, &metric).await;

        // Synchronize and verify checkpoints
        self.update_checkpoints(run, &poll_resp.checkpoints).await;

        // Handle terminal states
        match poll_resp.state {
            TrainingRunState::Succeeded => {
                // Verify all checkpoint adapter artifacts in BlobStorage
                let mut first_failure: Option<(String, usize, String)> = None;

                if run.checkpoints.is_empty() {
                    first_failure = Some((
                        "stat_missing".to_string(),
                        0,
                        "No checkpoints reported by training runner".to_string(),
                    ));
                }

                for cp in &mut run.checkpoints {
                    let key = BlobKey::CheckpointAdapter {
                        run_id: run.id.clone(),
                        step: cp.step,
                    };

                    match self.blobs.stat_blob(&key).await {
                        Ok(stat) => {
                            let expected_size = cp.artifact_size;
                            let expected_sha = cp.artifact_sha256.clone();
                            let verification = compare_artifact(
                                expected_size,
                                expected_sha.as_deref(),
                                stat.size,
                                stat.sha256.as_deref(),
                                "head_size",
                                "head_sha256",
                                true,
                            );
                            // Adopt size/checksum discovered on the object when not yet known.
                            if expected_size.is_none() {
                                cp.artifact_size = Some(stat.size);
                            }
                            if expected_sha.is_none() {
                                cp.artifact_sha256 = stat.sha256.clone();
                            }
                            if verification.status == ArtifactVerificationState::Mismatch
                                && first_failure.is_none()
                            {
                                if let (Some(method), Some(detail)) =
                                    (verification.method.clone(), verification.detail.clone())
                                {
                                    first_failure = Some((method, cp.step, detail));
                                }
                            }
                            let mismatch =
                                verification.status == ArtifactVerificationState::Mismatch;
                            cp.artifact_verification = verification;
                            if mismatch {
                                continue;
                            }
                        }
                        Err(_) => {
                            let msg = format!("Artifact missing in storage: {}", key.to_path());
                            cp.artifact_verification = ArtifactVerification::mismatch(
                                "stat_missing",
                                msg.clone(),
                            );
                            if first_failure.is_none() {
                                first_failure =
                                    Some(("stat_missing".to_string(), cp.step, msg));
                            }
                        }
                    }

                    if cp.sample_grid_url.is_some() {
                        let sample_key = BlobKey::CheckpointSamples {
                            run_id: run.id.clone(),
                            step: cp.step,
                        };
                        match self.blobs.stat_blob(&sample_key).await {
                            Ok(stat) => {
                                let expected_size = cp.sample_grid_size;
                                let expected_sha = cp.sample_grid_sha256.clone();
                                cp.sample_grid_verification = compare_artifact(
                                    expected_size,
                                    expected_sha.as_deref(),
                                    stat.size,
                                    stat.sha256.as_deref(),
                                    "head_size",
                                    "head_sha256",
                                    true,
                                );
                                if expected_size.is_none() {
                                    cp.sample_grid_size = Some(stat.size);
                                }
                                if expected_sha.is_none() {
                                    cp.sample_grid_sha256 = stat.sha256.clone();
                                }
                            }
                            Err(_) => {
                                cp.sample_grid_verification = ArtifactVerification::mismatch(
                                    "stat_missing",
                                    "Sample grid missing in storage".to_string(),
                                );
                            }
                        }
                    }
                }

                let any_unverified = run.checkpoints.iter().any(|c| {
                    c.artifact_verification.status != ArtifactVerificationState::Verified
                });

                if !any_unverified && !run.checkpoints.is_empty() {
                    run.state = TrainingRunState::Succeeded;
                    run.finished_at = Some(Utc::now());
                } else {
                    run.state = TrainingRunState::Failed;
                    run.failure_code = Some("ARTIFACT_VERIFICATION_FAILED".to_string());
                    if let Some((method, step, detail)) = first_failure {
                        run.error = Some(format!(
                            "Checkpoint step {step} adapter verification failed ({method}): {detail}"
                        ));
                    } else {
                        run.error = Some(
                            "Checkpoint adapter artifact missing or invalid: unverified checkpoints remain"
                                .to_string(),
                        );
                    }
                    run.finished_at = Some(Utc::now());
                }

                if run.cost_usd > 0.0 {
                    let cost_rec = TrainingCostRecord::new(
                        &run.id,
                        &run.recipe.policy,
                        run.step,
                        run.elapsed_seconds,
                        run.cost_usd,
                        run.state.as_str(),
                    );
                    let _ = self.cost_tracker.record_training_cost(cost_rec).await;
                }
            }
            TrainingRunState::Failed => {
                run.state = TrainingRunState::Failed;
                run.error = poll_resp
                    .error
                    .or_else(|| Some("Remote training run failed".to_string()));
                run.failure_code = Some("REMOTE_TRAINING_FAILED".to_string());
                run.finished_at = Some(Utc::now());

                if run.cost_usd > 0.0 {
                    let cost_rec = TrainingCostRecord::new(
                        &run.id,
                        &run.recipe.policy,
                        run.step,
                        run.elapsed_seconds,
                        run.cost_usd,
                        run.state.as_str(),
                    );
                    let _ = self.cost_tracker.record_training_cost(cost_rec).await;
                }
            }
            TrainingRunState::Cancelled => {
                run.state = TrainingRunState::Cancelled;
                run.finished_at = Some(Utc::now());

                if run.cost_usd > 0.0 {
                    let cost_rec = TrainingCostRecord::new(
                        &run.id,
                        &run.recipe.policy,
                        run.step,
                        run.elapsed_seconds,
                        run.cost_usd,
                        run.state.as_str(),
                    );
                    let _ = self.cost_tracker.record_training_cost(cost_rec).await;
                }
            }
            TrainingRunState::Running | TrainingRunState::Queued => {
                // A dispatched run stays `running` even while the runner reports `queued`
                // during its startup window; never regress it.
                run.state = TrainingRunState::Running;
            }
        }

        self.catalog.save_training_run(run).await?;
        Ok(())
    }

    /// Synchronizes checkpoints reported by the runner with local records.
    async fn update_checkpoints(
        &self,
        run: &mut TrainingRun,
        reported: &[RunnerCheckpoint],
    ) {
        for rc in reported {
            let adapter_key = BlobKey::CheckpointAdapter {
                run_id: run.id.clone(),
                step: rc.step,
            }
            .to_path();

            let sample_key = BlobKey::CheckpointSamples {
                run_id: run.id.clone(),
                step: rc.step,
            }
            .to_path();

            if let Some(existing) = run.checkpoints.iter_mut().find(|c| c.step == rc.step) {
                existing.artifact_url = rc.artifact_url.clone();
                existing.artifact_sha256 = rc.artifact_sha256.clone();
                existing.artifact_size = rc.artifact_size;
                existing.sample_grid_url = rc.sample_grid_url.clone();
                existing.sample_grid_sha256 = rc.sample_grid_sha256.clone();
                existing.sample_grid_size = rc.sample_grid_size;
            } else {
                run.checkpoints.push(CheckpointRecord {
                    step: rc.step,
                    artifact_key: adapter_key,
                    artifact_url: rc.artifact_url.clone(),
                    artifact_sha256: rc.artifact_sha256.clone(),
                    artifact_size: rc.artifact_size,
                    artifact_verification: ArtifactVerification::pending(),
                    sample_grid_key: Some(sample_key),
                    sample_grid_url: rc.sample_grid_url.clone(),
                    sample_grid_sha256: rc.sample_grid_sha256.clone(),
                    sample_grid_size: rc.sample_grid_size,
                    sample_grid_verification: ArtifactVerification::pending(),
                });
            }
        }
        run.checkpoints.sort_by_key(|c| c.step);
    }

    /// Escape hatch to verify all checkpoint artifacts for a run against storage (ADR-0009).
    pub async fn verify_run_artifacts(&self, id: &str) -> Result<TrainingRun, TrainingServiceError> {
        let mut run = match self.catalog.get_training_run(id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        let mut first_failed_adapter: Option<(String, usize, String)> = None;

        for cp in &mut run.checkpoints {
            let key = BlobKey::CheckpointAdapter {
                run_id: run.id.clone(),
                step: cp.step,
            };

            // Full download and hash verification
            match self.blobs.read_blob(&key).await {
                Ok(bytes) => {
                    let actual_size = bytes.len() as u64;
                    let computed_sha = sha256_digest(&bytes);

                    let expected_size = cp.artifact_size;
                    let expected_sha = cp.artifact_sha256.clone();
                    let verification = compare_artifact(
                        expected_size,
                        expected_sha.as_deref(),
                        actual_size,
                        Some(&computed_sha),
                        "download_size",
                        "download_sha256",
                        false,
                    );
                    if verification.status == ArtifactVerificationState::Verified {
                        cp.artifact_sha256 = Some(computed_sha);
                        cp.artifact_size = Some(actual_size);
                    } else if first_failed_adapter.is_none() {
                        if let (Some(method), Some(detail)) =
                            (verification.method.clone(), verification.detail.clone())
                        {
                            first_failed_adapter = Some((method, cp.step, detail));
                        }
                    }
                    cp.artifact_verification = verification;
                }
                Err(_) => {
                    let msg = format!("Adapter artifact missing in storage: {}", key.to_path());
                    cp.artifact_verification = ArtifactVerification::mismatch(
                        "stat_missing",
                        msg.clone(),
                    );
                    if first_failed_adapter.is_none() {
                        first_failed_adapter =
                            Some(("stat_missing".to_string(), cp.step, msg));
                    }
                }
            }

            // Full download of sample grid if present
            let sample_key = BlobKey::CheckpointSamples {
                run_id: run.id.clone(),
                step: cp.step,
            };
            if cp.sample_grid_url.is_some() || self.blobs.stat_blob(&sample_key).await.is_ok() {
                match self.blobs.read_blob(&sample_key).await {
                    Ok(sample_bytes) => {
                        let sample_size = sample_bytes.len() as u64;
                        let sample_sha = sha256_digest(&sample_bytes);
                        let expected_size = cp.sample_grid_size;
                        let expected_sha = cp.sample_grid_sha256.clone();
                        cp.sample_grid_verification = compare_artifact(
                            expected_size,
                            expected_sha.as_deref(),
                            sample_size,
                            Some(&sample_sha),
                            "download_size",
                            "download_sha256",
                            false,
                        );
                        if cp.sample_grid_verification.status
                            == ArtifactVerificationState::Verified
                        {
                            cp.sample_grid_sha256 = Some(sample_sha);
                            cp.sample_grid_size = Some(sample_size);
                        }
                    }
                    Err(_) => {
                        if cp.sample_grid_url.is_some() {
                            cp.sample_grid_verification = ArtifactVerification::mismatch(
                                "stat_missing",
                                format!("Sample grid missing in storage: {}", sample_key.to_path()),
                            );
                        }
                    }
                }
            }
        }

        if let Some((failed_method, failed_step, reason)) = first_failed_adapter {
            run.state = TrainingRunState::Failed;
            run.failure_code = Some("ARTIFACT_VERIFICATION_FAILED".to_string());
            run.error = Some(format!(
                "Checkpoint step {failed_step} adapter verification failed ({failed_method}): {reason}"
            ));
            run.finished_at = Some(Utc::now());
        }
        self.catalog.save_training_run(&run).await?;
        Ok(run)
    }

    /// Publishes a chosen checkpoint as an immutable LoRA Adapter descriptor (ADR-0009).
    pub async fn publish_adapter(
        &self,
        run_id: &str,
        step: usize,
        scale: Option<f64>,
    ) -> Result<LoraAdapter, TrainingServiceError> {
        let mut run = match self.catalog.get_training_run(run_id).await {
            Ok(r) => r,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::RunNotFound(run_id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if run.state != TrainingRunState::Succeeded {
            return Err(TrainingServiceError::InvalidState(format!(
                "Run '{}' is in state '{}'; only succeeded runs can publish adapters",
                run_id,
                run.state.as_str()
            )));
        }

        let cp = run
            .checkpoints
            .iter()
            .find(|c| c.step == step)
            .ok_or_else(|| {
                TrainingServiceError::InvalidState(format!(
                    "Step {} not found in checkpoints of run '{}'",
                    step, run_id
                ))
            })?
            .clone();

        if cp.artifact_verification.status != ArtifactVerificationState::Verified {
            return Err(TrainingServiceError::InvalidState(format!(
                "Checkpoint at step {} is not verified (status: {:?})",
                step, cp.artifact_verification.status
            )));
        }

        if !cp.has_sample_grid() {
            return Err(TrainingServiceError::InvalidState(format!(
                "Checkpoint at step {} has no sample grid URL and cannot be published",
                step
            )));
        }

        if cp.sample_grid_verification.status == ArtifactVerificationState::Mismatch {
            return Err(TrainingServiceError::InvalidState(format!(
                "Checkpoint at step {} failed sample grid verification",
                step
            )));
        }

        let scale_val = scale.unwrap_or(0.75);
        if scale_val <= 0.0 || scale_val.is_nan() {
            return Err(TrainingServiceError::InvalidState(format!(
                "Adapter scale must be positive and non-zero, got {}",
                scale_val
            )));
        }

        let adapter_id = LoraAdapter::format_id(&run.id, step);
        if run.published_adapter_ids.contains(&adapter_id)
            || self.catalog.get_lora_adapter(&adapter_id).await.is_ok()
        {
            return Err(TrainingServiceError::Conflict(format!(
                "Adapter '{}' has already been published",
                adapter_id
            )));
        }

        let adapter_blob_key = BlobKey::CheckpointAdapter {
            run_id: run.id.clone(),
            step,
        };
        let stat = self
            .blobs
            .stat_blob(&adapter_blob_key)
            .await
            .map_err(|e| {
                TrainingServiceError::InvalidState(format!(
                    "Checkpoint adapter artifact '{}' is missing in storage: {e}",
                    adapter_blob_key.to_path()
                ))
            })?;

        let bytes = self
            .blobs
            .read_blob(&adapter_blob_key)
            .await
            .map_err(|e| {
                TrainingServiceError::InvalidState(format!(
                    "Checkpoint adapter artifact '{}' is unreadable in storage: {e}",
                    adapter_blob_key.to_path()
                ))
            })?;

        let sha256 = cp
            .artifact_sha256
            .filter(|s| !s.trim().is_empty())
            .or_else(|| stat.sha256.filter(|s| !s.trim().is_empty()))
            .or_else(|| Some(crate::ports::blob_storage::sha256_digest(&bytes)));

        let size_bytes = cp.artifact_size.or(Some(stat.size));

        let adapter = LoraAdapter {
            id: adapter_id.clone(),
            run_id: run.id.clone(),
            step,
            scale: scale_val,
            base_model: run.recipe.base_model.clone(),
            trigger_phrase: run.recipe.trigger_phrase.clone(),
            artifact_key: cp.artifact_key,
            artifact_url: cp.artifact_url,
            sha256,
            size_bytes,
            recipe_digest: run.recipe_digest.clone(),
            dataset_snapshot_id: run.dataset_snapshot_id.clone(),
            dataset_digest: run.dataset_digest.clone(),
            created_at: Utc::now(),
            retired: false,
            retired_at: None,
        };

        self.catalog.save_lora_adapter(&adapter).await?;

        if !run.published_adapter_ids.contains(&adapter_id) {
            run.published_adapter_ids.push(adapter_id);
            self.catalog.save_training_run(&run).await?;
        }

        Ok(adapter)
    }

    /// Lists LoRA adapters, optionally filtering only selectable/usable ones.
    /// An adapter whose underlying artifact is missing or unreadable is omitted when `selectable_only` is true.
    pub async fn list_adapters(
        &self,
        selectable_only: bool,
    ) -> Result<Vec<LoraAdapter>, TrainingServiceError> {
        let mut adapters = self.catalog.list_lora_adapters().await?;

        if selectable_only {
            let mut usable = Vec::new();
            for adapter in adapters {
                if adapter.retired {
                    continue;
                }
                let blob_key = BlobKey::CheckpointAdapter {
                    run_id: adapter.run_id.clone(),
                    step: adapter.step,
                };
                if self.blobs.stat_blob(&blob_key).await.is_ok() {
                    usable.push(adapter);
                }
            }
            adapters = usable;
        }

        Ok(adapters)
    }

    /// Retrieves one LoRA adapter by ID.
    pub async fn get_adapter(&self, id: &str) -> Result<LoraAdapter, TrainingServiceError> {
        match self.catalog.get_lora_adapter(id).await {
            Ok(a) => Ok(a),
            Err(StorageError::NotFound(_)) => {
                Err(TrainingServiceError::AdapterNotFound(id.to_string()))
            }
            Err(e) => Err(TrainingServiceError::Storage(e)),
        }
    }

    /// Retires or restores a LoRA adapter (soft retirement per ADR-0009).
    pub async fn set_adapter_retired(
        &self,
        id: &str,
        retired: bool,
    ) -> Result<LoraAdapter, TrainingServiceError> {
        let mut adapter = match self.catalog.get_lora_adapter(id).await {
            Ok(a) => a,
            Err(StorageError::NotFound(_)) => {
                return Err(TrainingServiceError::AdapterNotFound(id.to_string()));
            }
            Err(e) => return Err(TrainingServiceError::Storage(e)),
        };

        if retired {
            adapter.retire();
        } else {
            adapter.restore();
        }

        self.catalog.save_lora_adapter(&adapter).await?;
        Ok(adapter)
    }
}
