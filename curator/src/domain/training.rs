//! Training Run lifecycle, LoRA Recipe, and Adapter Catalog domain models (ADR-0008, ADR-0009).

use std::collections::BTreeMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::caption::TRIGGER_PHRASE;
use crate::ports::blob_storage::sha256_digest;

/// Lifecycle state of a Curator training run (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrainingRunState {
    #[default]
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TrainingRunState {
    /// Returns true when the state represents a terminal completion status.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TrainingRunState::Succeeded
                | TrainingRunState::Failed
                | TrainingRunState::Cancelled
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TrainingRunState::Queued => "queued",
            TrainingRunState::Running => "running",
            TrainingRunState::Succeeded => "succeeded",
            TrainingRunState::Failed => "failed",
            TrainingRunState::Cancelled => "cancelled",
        }
    }
}

/// Baseline vendor recipe defaults for FLUX 3 Image Klein LoRA training (ADR-0008).
pub const DEFAULT_BASE_MODEL: &str = "black-forest-labs/flux-3-image-klein-base";
pub const DEFAULT_ARCH: &str = "flux3";
pub const CALIBRATION_STEPS: usize = 1500;
pub const FULL_STEPS: usize = 4000;
pub const DEFAULT_SAVE_EVERY: usize = 250;
pub const DEFAULT_RANK: usize = 32;
pub const DEFAULT_ALPHA: usize = 32;
pub const DEFAULT_LEARNING_RATE: f64 = 1e-4;
pub const DEFAULT_WARMUP_STEPS: usize = 200;
pub const DEFAULT_LR_SCHEDULER: &str = "linear";
pub const DEFAULT_OPTIMIZER: &str = "adamw";
pub const DEFAULT_BETAS: [f64; 2] = [0.9, 0.95];
pub const DEFAULT_BATCH_SIZE: usize = 1;
pub const DEFAULT_DTYPE: &str = "bf16";
pub const DEFAULT_WIDTH: u32 = 1024;
pub const DEFAULT_HEIGHT: u32 = 1024;

fn default_base_model() -> String {
    DEFAULT_BASE_MODEL.to_string()
}

fn default_arch() -> String {
    DEFAULT_ARCH.to_string()
}

fn default_policy() -> String {
    "calibration".to_string()
}

fn default_steps() -> usize {
    CALIBRATION_STEPS
}

fn default_save_every() -> usize {
    DEFAULT_SAVE_EVERY
}

fn default_rank() -> usize {
    DEFAULT_RANK
}

fn default_alpha() -> usize {
    DEFAULT_ALPHA
}

fn default_learning_rate() -> f64 {
    DEFAULT_LEARNING_RATE
}

fn default_warmup_steps() -> usize {
    DEFAULT_WARMUP_STEPS
}

fn default_lr_scheduler() -> String {
    DEFAULT_LR_SCHEDULER.to_string()
}

fn default_optimizer() -> String {
    DEFAULT_OPTIMIZER.to_string()
}

fn default_betas() -> [f64; 2] {
    DEFAULT_BETAS
}

fn default_batch_size() -> usize {
    DEFAULT_BATCH_SIZE
}

fn default_dtype() -> String {
    DEFAULT_DTYPE.to_string()
}

fn default_width() -> u32 {
    DEFAULT_WIDTH
}

fn default_height() -> u32 {
    DEFAULT_HEIGHT
}

fn default_trigger_phrase() -> String {
    TRIGGER_PHRASE.to_string()
}

/// Training recipe specifying fine-tuning hyperparameters for a TrainingRun (ADR-0008, ADR-0009).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingRecipe {
    #[serde(default = "default_base_model")]
    pub base_model: String,
    #[serde(default = "default_arch")]
    pub arch: String,
    #[serde(default = "default_policy")]
    pub policy: String,
    #[serde(default = "default_steps")]
    pub steps: usize,
    #[serde(default = "default_save_every")]
    pub save_every: usize,
    #[serde(default = "default_rank")]
    pub rank: usize,
    #[serde(default = "default_alpha")]
    pub alpha: usize,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,
    #[serde(default = "default_warmup_steps")]
    pub warmup_steps: usize,
    #[serde(default = "default_lr_scheduler")]
    pub lr_scheduler: String,
    #[serde(default = "default_optimizer")]
    pub optimizer: String,
    #[serde(default = "default_betas")]
    pub betas: [f64; 2],
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_dtype")]
    pub dtype: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_trigger_phrase")]
    pub trigger_phrase: String,
}

impl Default for TrainingRecipe {
    fn default() -> Self {
        Self::calibration()
    }
}

impl TrainingRecipe {
    /// Creates the standard calibration recipe (1500 steps).
    pub fn calibration() -> Self {
        Self {
            base_model: default_base_model(),
            arch: default_arch(),
            policy: "calibration".to_string(),
            steps: CALIBRATION_STEPS,
            save_every: DEFAULT_SAVE_EVERY,
            rank: DEFAULT_RANK,
            alpha: DEFAULT_ALPHA,
            learning_rate: DEFAULT_LEARNING_RATE,
            warmup_steps: DEFAULT_WARMUP_STEPS,
            lr_scheduler: default_lr_scheduler(),
            optimizer: default_optimizer(),
            betas: DEFAULT_BETAS,
            batch_size: DEFAULT_BATCH_SIZE,
            dtype: default_dtype(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            trigger_phrase: default_trigger_phrase(),
        }
    }

    /// Creates the standard full fine-tuning recipe (4000 steps).
    pub fn full() -> Self {
        let mut recipe = Self::calibration();
        recipe.policy = "full".to_string();
        recipe.steps = FULL_STEPS;
        recipe
    }

    /// Creates a recipe for a policy preset ("calibration" or "full").
    pub fn for_policy(policy: &str) -> Result<Self, String> {
        match policy.to_ascii_lowercase().as_str() {
            "calibration" => Ok(Self::calibration()),
            "full" => Ok(Self::full()),
            unknown => Err(format!(
                "Unknown training policy '{unknown}'; must be 'calibration' or 'full'"
            )),
        }
    }

    /// Computes the deterministic SHA-256 hex digest of the canonical JSON representation.
    pub fn digest(&self) -> String {
        let value = serde_json::to_value(self).expect("recipe serializes to Value");
        fn canonicalize(val: serde_json::Value) -> serde_json::Value {
            match val {
                serde_json::Value::Object(map) => {
                    let sorted: BTreeMap<String, serde_json::Value> =
                        map.into_iter().map(|(k, v)| (k, canonicalize(v))).collect();
                    serde_json::to_value(sorted).unwrap()
                }
                serde_json::Value::Array(arr) => {
                    serde_json::Value::Array(arr.into_iter().map(canonicalize).collect())
                }
                other => other,
            }
        }
        let canonical = canonicalize(value);
        let bytes = serde_json::to_vec(&canonical).expect("canonical json bytes");
        sha256_digest(&bytes)
    }
}

/// Verification status of a checkpoint artifact in storage (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactVerificationState {
    #[default]
    Pending,
    Verified,
    Mismatch,
}

/// Detailed artifact verification record with audit metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArtifactVerification {
    pub status: ArtifactVerificationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ArtifactVerification {
    pub fn pending() -> Self {
        Self {
            status: ArtifactVerificationState::Pending,
            method: None,
            checked_at: None,
            detail: None,
        }
    }

    pub fn verified(method: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            status: ArtifactVerificationState::Verified,
            method: Some(method.into()),
            checked_at: Some(Utc::now()),
            detail,
        }
    }

    pub fn mismatch(method: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: ArtifactVerificationState::Mismatch,
            method: Some(method.into()),
            checked_at: Some(Utc::now()),
            detail: Some(detail.into()),
        }
    }
}

/// Checkpoint artifact record generated at a step during a training run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub step: usize,
    pub artifact_key: String,
    pub artifact_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_size: Option<u64>,
    pub artifact_verification: ArtifactVerification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_grid_size: Option<u64>,
    pub sample_grid_verification: ArtifactVerification,
}

impl CheckpointRecord {
    /// Returns true if the checkpoint record has a non-empty sample grid URL.
    pub fn has_sample_grid(&self) -> bool {
        self.sample_grid_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
    }
}

/// Appended telemetry metric snapshot tracked in `metrics.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingMetricSnapshot {
    pub timestamp: DateTime<Utc>,
    pub step: usize,
    pub total_steps: usize,
    pub elapsed_seconds: f64,
    pub cost_usd: f64,
    pub state: TrainingRunState,
}

/// A Curator-managed fine-tuning training run (ADR-0009).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingRun {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_job_id: Option<String>,
    pub state: TrainingRunState,
    pub dataset_snapshot_id: String,
    pub dataset_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_url: Option<String>,
    pub recipe: TrainingRecipe,
    pub recipe_digest: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_polled_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_poll_error: Option<String>,
    #[serde(default)]
    pub consecutive_poll_failures: u32,
    #[serde(default)]
    pub poll_backoff_seconds: u64,
    pub step: usize,
    pub total_steps: usize,
    pub elapsed_seconds: f64,
    pub cost_usd: f64,
    #[serde(default)]
    pub checkpoints: Vec<CheckpointRecord>,
    #[serde(default)]
    pub published_adapter_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<TrainingMetricSnapshot>,
}

fn default_adapter_scale() -> f64 {
    0.75
}

/// An immutable published LoRA Adapter descriptor (ADR-0009).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoraAdapter {
    /// Unique identifier formatted as `{run_id}-s{step}`.
    pub id: String,
    pub run_id: String,
    pub step: usize,
    #[serde(default = "default_adapter_scale")]
    pub scale: f64,
    pub base_model: String,
    pub trigger_phrase: String,
    pub artifact_key: String,
    pub artifact_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    pub recipe_digest: String,
    pub dataset_snapshot_id: String,
    pub dataset_digest: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub retired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<DateTime<Utc>>,
}

impl LoraAdapter {
    /// Canonical adapter ID format: `{run_id}-s{step}`.
    pub fn format_id(run_id: &str, step: usize) -> String {
        format!("{run_id}-s{step}")
    }

    /// Parses an adapter ID formatted strictly as `{run_id}-s{step}` (the inverse of
    /// [`LoraAdapter::format_id`]), returning the run ID and step number. Anything else — including
    /// the engine's `-edit` suffix from #219 — is rejected so the catalog key stays canonical.
    pub fn parse_id(id: &str) -> Option<(&str, usize)> {
        let (run_id, step_str) = id.rsplit_once("-s")?;
        if run_id.is_empty() {
            return None;
        }
        let step = step_str.parse::<usize>().ok()?;
        Some((run_id, step))
    }

    /// Mark this adapter as retired.
    pub fn retire(&mut self) {
        self.retired = true;
        self.retired_at = Some(Utc::now());
    }

    /// Restore this adapter from retirement.
    pub fn restore(&mut self) {
        self.retired = false;
        self.retired_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_training_recipe_digest_stability() {
        let r1 = TrainingRecipe::calibration();
        let r2 = TrainingRecipe::calibration();
        assert_eq!(r1.digest(), r2.digest());

        let full = TrainingRecipe::full();
        assert_ne!(r1.digest(), full.digest());
    }

    #[test]
    fn test_training_run_state_serde() {
        let state = TrainingRunState::Queued;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"queued\"");

        let deserialized: TrainingRunState = serde_json::from_str("\"running\"").unwrap();
        assert_eq!(deserialized, TrainingRunState::Running);
        assert!(!deserialized.is_terminal());

        let succeeded: TrainingRunState = serde_json::from_str("\"succeeded\"").unwrap();
        assert!(succeeded.is_terminal());
    }

    #[test]
    fn test_lora_adapter_id_formatting_and_parsing() {
        assert_eq!(LoraAdapter::format_id("run-123", 250), "run-123-s250");
        assert_eq!(LoraAdapter::parse_id("run-123-s250"), Some(("run-123", 250)));
        assert_eq!(LoraAdapter::parse_id("run-123-s250-edit"), None);
        assert_eq!(
            LoraAdapter::parse_id("run-complex-id-123-s500"),
            Some(("run-complex-id-123", 500))
        );
        assert_eq!(
            LoraAdapter::parse_id("run-complex-id-123-s500-edit"),
            None
        );
        assert_eq!(LoraAdapter::parse_id("invalid"), None);
        assert_eq!(LoraAdapter::parse_id("-s250"), None);
        assert_eq!(LoraAdapter::parse_id("run-s"), None);
        assert_eq!(LoraAdapter::parse_id("run-s-edit"), None);
        assert_eq!(LoraAdapter::parse_id("run-sabc"), None);
        assert_eq!(LoraAdapter::parse_id("run-s250-unknown"), None);
    }
}
