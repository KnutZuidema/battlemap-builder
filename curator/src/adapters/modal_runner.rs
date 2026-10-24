//! HTTP adapter for the Modal training runner FastAPI service (ADR-0008, ADR-0009).

use async_trait::async_trait;
use serde_json::json;

use crate::domain::TrainingRun;
use crate::ports::training_runner::{
    RunnerRunResponse, TrainingRunner, TrainingRunnerError,
};

/// HTTP training runner communicating with Modal's FastAPI service.
#[derive(Debug, Clone)]
pub struct ModalTrainingRunner {
    endpoint: String,
    token: String,
    client: reqwest::Client,
}

impl ModalTrainingRunner {
    /// Constructs a runner with explicit endpoint and bearer token.
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Constructs a runner with a custom `reqwest::Client` (useful for timeouts or mock harnesses).
    pub fn with_client(
        endpoint: impl Into<String>,
        token: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            token: token.into(),
            client,
        }
    }

    /// Initializes from `CURATOR_MODAL_ENDPOINT` and `CURATOR_MODAL_TOKEN` environment variables.
    pub fn from_env() -> Result<Self, TrainingRunnerError> {
        let endpoint = std::env::var("CURATOR_MODAL_ENDPOINT").map_err(|_| {
            TrainingRunnerError::NotConfigured("CURATOR_MODAL_ENDPOINT environment variable is missing".to_string())
        })?;
        let token = std::env::var("CURATOR_MODAL_TOKEN").map_err(|_| {
            TrainingRunnerError::NotConfigured("CURATOR_MODAL_TOKEN environment variable is missing".to_string())
        })?;

        let endpoint = endpoint.trim().to_string();
        let token = token.trim().to_string();

        if endpoint.is_empty() || token.is_empty() {
            return Err(TrainingRunnerError::NotConfigured(
                "CURATOR_MODAL_ENDPOINT or CURATOR_MODAL_TOKEN is empty".to_string(),
            ));
        }

        Ok(Self::new(endpoint, token))
    }

    /// Helper to check if configuration environment variables are present and non-empty.
    pub fn is_configured() -> bool {
        match (
            std::env::var("CURATOR_MODAL_ENDPOINT"),
            std::env::var("CURATOR_MODAL_TOKEN"),
        ) {
            (Ok(ep), Ok(tok)) => !ep.trim().is_empty() && !tok.trim().is_empty(),
            _ => false,
        }
    }
}

#[async_trait]
impl TrainingRunner for ModalTrainingRunner {
    async fn submit_run(&self, run: &TrainingRun) -> Result<RunnerRunResponse, TrainingRunnerError> {
        let url = format!("{}/run", self.endpoint);
        let payload = json!({
            "run_id": run.id,
            "dataset_url": run.dataset_url,
            "recipe": run.recipe,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        if status.is_success() {
            let body = resp
                .json::<RunnerRunResponse>()
                .await
                .map_err(|e| TrainingRunnerError::InvalidResponse(e.to_string()))?;
            Ok(body)
        } else {
            let error_text = resp.text().await.unwrap_or_default();
            Err(TrainingRunnerError::RunnerError {
                status,
                message: error_text,
            })
        }
    }

    async fn poll_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError> {
        let url = format!("{}/run/{}", self.endpoint, remote_job_id);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(TrainingRunnerError::NotFound(remote_job_id.to_string()));
        }

        if status.is_success() {
            let body = resp
                .json::<RunnerRunResponse>()
                .await
                .map_err(|e| TrainingRunnerError::InvalidResponse(e.to_string()))?;
            Ok(body)
        } else {
            let error_text = resp.text().await.unwrap_or_default();
            Err(TrainingRunnerError::RunnerError {
                status,
                message: error_text,
            })
        }
    }

    async fn cancel_run(&self, remote_job_id: &str) -> Result<RunnerRunResponse, TrainingRunnerError> {
        let url = format!("{}/run/{}/cancel", self.endpoint, remote_job_id);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(TrainingRunnerError::NotFound(remote_job_id.to_string()));
        }

        if status.is_success() {
            let body = resp
                .json::<RunnerRunResponse>()
                .await
                .map_err(|e| TrainingRunnerError::InvalidResponse(e.to_string()))?;
            Ok(body)
        } else {
            let error_text = resp.text().await.unwrap_or_default();
            Err(TrainingRunnerError::RunnerError {
                status,
                message: error_text,
            })
        }
    }
}
