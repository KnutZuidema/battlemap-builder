//! Port interface and data structures for persistent generation cost tracking and telemetry.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::BenchmarkMetadata;

/// Errors arising during cost tracking and ledger persistence operations.
#[derive(Debug, Error)]
pub enum CostTrackerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Lock error: {0}")]
    Lock(String),

    #[error("Storage error: {0}")]
    Storage(String),
}

/// Detailed telemetry and cost record for a single image generation run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationCostRecord {
    /// Unique identifier of the generation run.
    pub id: String,
    /// Model identifier (e.g. "google/gemini-3.1-flash-image").
    pub model: String,
    /// Prompt or archetype name (e.g. "Stone Crypt").
    pub prompt_name: String,
    /// Incurred cost in USD for this generation.
    pub cost_usd: f64,
    /// Number of prompt/input tokens used, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    /// Number of completion/output tokens used, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    /// Total roundtrip latency in milliseconds.
    pub latency_ms: u64,
    /// Timestamp when generation was recorded.
    pub timestamp: DateTime<Utc>,
}

impl GenerationCostRecord {
    /// Creates a new `GenerationCostRecord` with the current UTC timestamp.
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        prompt_name: impl Into<String>,
        cost_usd: f64,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
        latency_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            prompt_name: prompt_name.into(),
            cost_usd,
            prompt_tokens,
            completion_tokens,
            latency_ms,
            timestamp: Utc::now(),
        }
    }
    /// Sets an explicit timestamp on the record.
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }
}

impl From<&BenchmarkMetadata> for GenerationCostRecord {
    fn from(meta: &BenchmarkMetadata) -> Self {
        Self {
            id: meta.id.clone(),
            model: meta.model.clone(),
            prompt_name: meta.prompt_name.clone(),
            cost_usd: meta.cost_usd,
            prompt_tokens: meta.prompt_tokens,
            completion_tokens: meta.completion_tokens,
            latency_ms: meta.latency_ms,
            timestamp: Utc::now(),
        }
    }
}

impl From<BenchmarkMetadata> for GenerationCostRecord {
    fn from(meta: BenchmarkMetadata) -> Self {
        Self::from(&meta)
    }
}

/// Aggregated cost and generation metrics for a single model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelCostStats {
    /// Cumulative spend in USD for this model.
    pub spend_usd: f64,
    /// Total number of images generated with this model.
    pub generation_count: u64,
    /// Total tokens (prompt + completion) consumed by this model.
    pub total_tokens: u64,
}

/// Comprehensive cost statistics including session, all-time, and per-model breakdowns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CostStats {
    /// Total dollar spend in the current application session.
    pub session_spend_usd: f64,
    /// Cumulative dollar spend across all sessions (from persistent ledger).
    pub total_spend_usd: f64,
    /// Total number of images generated across all sessions.
    pub total_generations: u64,
    /// Number of images generated in the current application session.
    pub session_generations: u64,
    /// Breakdown of spending, image counts, and tokens keyed by model ID.
    pub by_model: HashMap<String, ModelCostStats>,
    /// Historical log of individual generation records.
    #[serde(default)]
    pub history: Vec<GenerationCostRecord>,
}

/// Port trait defining operations for persistent cost tracking, telemetry, and session metrics.
#[async_trait]
pub trait CostTracker: Send + Sync {
    /// Records generation cost and telemetry for a single image generation.
    async fn record_generation(
        &self,
        record: GenerationCostRecord,
    ) -> Result<(), CostTrackerError>;

    /// Retrieves cumulative cost statistics, session spend, and per-model breakdown.
    async fn get_stats(&self) -> Result<CostStats, CostTrackerError>;

    /// Resets the in-memory session spend counters without modifying persistent ledger.
    async fn reset_session(&self) -> Result<(), CostTrackerError>;

    /// Retrieves the timestamped generation history.
    async fn get_history(&self) -> Result<Vec<GenerationCostRecord>, CostTrackerError> {
        let stats = self.get_stats().await?;
        Ok(stats.history)
    }
}

#[async_trait]
impl<T: ?Sized + CostTracker> CostTracker for Arc<T> {
    async fn record_generation(
        &self,
        record: GenerationCostRecord,
    ) -> Result<(), CostTrackerError> {
        (**self).record_generation(record).await
    }

    async fn get_stats(&self) -> Result<CostStats, CostTrackerError> {
        (**self).get_stats().await
    }

    async fn reset_session(&self) -> Result<(), CostTrackerError> {
        (**self).reset_session().await
    }

    async fn get_history(&self) -> Result<Vec<GenerationCostRecord>, CostTrackerError> {
        (**self).get_history().await
    }
}
