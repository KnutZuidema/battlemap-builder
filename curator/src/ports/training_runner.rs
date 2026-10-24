//! Outbound port for submitting, polling, and cancelling fine-tuning training runs on Modal (ADR-0008, ADR-0009).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{TrainingRun, TrainingRunState};

/// Error returned by training runner operations.
#[derive(Debug, Error)]
pub enum TrainingRunnerError {
    #[error("Modal runner is not configured: {0}")]
    NotConfigured(String),

    #[error("Remote training run '{0}' was not found")]
    NotFound(String),

    #[error("HTTP error communicating with training runner: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid runner response: {0}")]
    InvalidResponse(String),

    #[error("Training runner returned error {status}: {message}")]
    RunnerError {
        status: reqwest::StatusCode,
        message: String,
    },
}

/// Checkpoint information returned by the training runner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunnerCheckpoint {
    pub step: usize,
    pub artifact_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_size: Option<u64>,
}

/// Status payload returned from training runner operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunnerRunResponse {
    pub run_id: String,
    pub state: TrainingRunState,
    pub step: usize,
    pub total_steps: usize,
    pub elapsed_seconds: f64,
    pub cost_usd: f64,
    #[serde(default)]
    pub checkpoints: Vec<RunnerCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Outbound port defining fine-tuning runner execution on Modal (ADR-0009).
#[async_trait]
pub trait TrainingRunner: Send + Sync {
    /// Submits a training run to the runner.
    async fn submit_run(&self, run: &TrainingRun) -> Result<RunnerRunResponse, TrainingRunnerError>;

    /// Polls status and checkpoint progress for an active or completed remote job.
    async fn poll_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError>;

    /// Cancels a running or queued remote training run.
    async fn cancel_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError>;
}

#[async_trait]
impl<T: ?Sized + TrainingRunner> TrainingRunner for std::sync::Arc<T> {
    async fn submit_run(&self, run: &TrainingRun) -> Result<RunnerRunResponse, TrainingRunnerError> {
        (**self).submit_run(run).await
    }

    async fn poll_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError> {
        (**self).poll_run(remote_job_id).await
    }

    async fn cancel_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError> {
        (**self).cancel_run(remote_job_id).await
    }
}
