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
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::cost_tracker::{CostTracker, CostTrackerError, TrainingCostRecord};
use crate::ports::storage_error::StorageError;
use crate::ports::training_runner::{
    RunnerCheckpoint, TrainingRunner, TrainingRunnerError,
};

static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

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
            step: 0,
            total_steps,
            elapsed_seconds: 0.0,
            cost_usd: 0.0,
            checkpoints: Vec::new(),
            published_adapter_ids: Vec::new(),
            error: None,
            failure_code: None,
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

    /// Internal polling helper: polls runner, updates checkpoints and metrics, verifies on success.
    async fn poll_active_run_internal(
        &self,
        run: &mut TrainingRun,
    ) -> Result<(), TrainingServiceError> {
        let runner = match self.runner.as_ref() {
            Some(r) => r,
            None => {
                run.last_polled_at = Some(Utc::now());
                run.last_poll_error = Some("Modal runner is not configured".to_string());
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
            Err(TrainingRunnerError::NotFound(_)) => {
                // Lost remote job terminates run as failed per ADR-0009
                run.state = TrainingRunState::Failed;
                run.failure_code = Some("LOST_REMOTE".to_string());
                run.error = Some("Remote job not found on training runner".to_string());
                run.finished_at = Some(Utc::now());
                run.last_polled_at = Some(Utc::now());

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
                // Unreachable or error keeps run running, records error per ADR-0009
                run.last_polled_at = Some(Utc::now());
                run.last_poll_error = Some(err.to_string());
                self.catalog.save_training_run(run).await?;
                return Ok(());
            }
        };

        // Update progress
        run.step = poll_resp.step;
        run.total_steps = poll_resp.total_steps;
        run.elapsed_seconds = poll_resp.elapsed_seconds;
        run.cost_usd = poll_resp.cost_usd;
        run.last_polled_at = Some(Utc::now());
        run.last_poll_error = None;

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
                let mut all_valid = true;
                let mut missing_artifact = None;

                for cp in &mut run.checkpoints {
                    let key = BlobKey::CheckpointAdapter {
                        run_id: run.id.clone(),
                        step: cp.step,
                    };

                    match self.blobs.stat_blob(&key).await {
                        Ok(stat) => {
                            if let Some(expected_size) = cp.artifact_size {
                                if stat.size != expected_size {
                                    cp.artifact_verification = ArtifactVerification::mismatch(
                                        "stat",
                                        format!(
                                            "Size mismatch: expected {}, got {}",
                                            expected_size, stat.size
                                        ),
                                    );
                                    all_valid = false;
                                    missing_artifact = Some(key.to_path());
                                    continue;
                                }
                            }
                            if let (Some(expected_sha), Some(stat_sha)) =
                                (&cp.artifact_sha256, &stat.sha256)
                            {
                                if expected_sha != stat_sha {
                                    cp.artifact_verification = ArtifactVerification::mismatch(
                                        "stat",
                                        format!(
                                            "Checksum mismatch: expected {}, got {}",
                                            expected_sha, stat_sha
                                        ),
                                    );
                                    all_valid = false;
                                    missing_artifact = Some(key.to_path());
                                    continue;
                                }
                            }
                            cp.artifact_verification =
                                ArtifactVerification::verified("stat", None);
                        }
                        Err(_) => {
                            cp.artifact_verification = ArtifactVerification::mismatch(
                                "stat",
                                "Artifact missing in storage".to_string(),
                            );
                            all_valid = false;
                            missing_artifact = Some(key.to_path());
                        }
                    }
                }

                if all_valid && !run.checkpoints.is_empty() {
                    run.state = TrainingRunState::Succeeded;
                    run.finished_at = Some(Utc::now());
                } else {
                    run.state = TrainingRunState::Failed;
                    run.failure_code = Some("ARTIFACT_VERIFICATION_FAILED".to_string());
                    run.error = Some(format!(
                        "Checkpoint adapter artifact missing or invalid: {}",
                        missing_artifact.unwrap_or_else(|| "no checkpoints found".to_string())
                    ));
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
                run.state = poll_resp.state;
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

        for cp in &mut run.checkpoints {
            let key = BlobKey::CheckpointAdapter {
                run_id: run.id.clone(),
                step: cp.step,
            };

            match self.blobs.stat_blob(&key).await {
                Ok(stat) => {
                    if let Some(size) = cp.artifact_size {
                        if stat.size != size {
                            cp.artifact_verification = ArtifactVerification::mismatch(
                                "stat",
                                format!("Size mismatch: expected {}, got {}", size, stat.size),
                            );
                            continue;
                        }
                    }
                    if let (Some(sha), Some(stat_sha)) = (&cp.artifact_sha256, &stat.sha256) {
                        if sha != stat_sha {
                            cp.artifact_verification = ArtifactVerification::mismatch(
                                "stat",
                                format!("Checksum mismatch: expected {}, got {}", sha, stat_sha),
                            );
                            continue;
                        }
                    }
                    cp.artifact_verification = ArtifactVerification::verified("stat", None);
                }
                Err(_) => {
                    cp.artifact_verification = ArtifactVerification::mismatch(
                        "stat",
                        "Artifact missing in storage".to_string(),
                    );
                }
            }
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

        if cp.artifact_verification.status == ArtifactVerificationState::Mismatch {
            return Err(TrainingServiceError::InvalidState(format!(
                "Checkpoint at step {} failed artifact verification",
                step
            )));
        }

        // Check that underlying adapter artifact exists in storage
        let adapter_blob_key = BlobKey::CheckpointAdapter {
            run_id: run.id.clone(),
            step,
        };
        if self.blobs.stat_blob(&adapter_blob_key).await.is_err() {
            return Err(TrainingServiceError::InvalidState(format!(
                "Checkpoint adapter artifact '{}' is missing in storage",
                adapter_blob_key.to_path()
            )));
        }

        let adapter_id = LoraAdapter::format_id(&run.id, step);
        if self.catalog.get_lora_adapter(&adapter_id).await.is_ok() {
            return Err(TrainingServiceError::Conflict(format!(
                "Adapter '{}' has already been published",
                adapter_id
            )));
        }

        let adapter = LoraAdapter {
            id: adapter_id.clone(),
            run_id: run.id.clone(),
            step,
            scale: scale.unwrap_or(0.75),
            base_model: run.recipe.base_model.clone(),
            trigger_phrase: run.recipe.trigger_phrase.clone(),
            artifact_key: cp.artifact_key,
            artifact_url: cp.artifact_url,
            sha256: cp.artifact_sha256,
            size_bytes: cp.artifact_size,
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
