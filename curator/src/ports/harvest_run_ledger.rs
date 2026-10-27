//! Port recording Reddit harvest runs in the audit ledger (ADR-0011).
//!
//! Scheduled harvesting needs a durable account of every attempt: which subreddit and sort ran,
//! when it started and finished, how many posts it scanned, and why it failed. The ledger is
//! optional — a backend without durable storage reports no run id and the ingestion service falls
//! back to logging the outcome.

use async_trait::async_trait;

use crate::domain::RedditHarvestQuery;
use crate::ports::storage_error::StorageError;

/// Counters recorded for one finished harvest run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarvestRunOutcome {
    /// Media items processed, whether downloaded or skipped.
    pub posts_scanned: usize,
    pub downloaded: usize,
    pub skipped: usize,
    /// Failure that aborted the run, if any.
    pub error: Option<String>,
}

impl HarvestRunOutcome {
    /// Outcome of a run that completed without error.
    pub fn success(posts_scanned: usize, downloaded: usize, skipped: usize) -> Self {
        Self {
            posts_scanned,
            downloaded,
            skipped,
            error: None,
        }
    }

    /// Outcome of a run that aborted with `error`.
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

/// Port defining the harvest run ledger: one row per attempted harvest, updated on completion.
#[async_trait]
pub trait HarvestRunLedger: Send + Sync {
    /// Records the start of a harvest run.
    ///
    /// Returns the ledger row id, or `None` when the backend keeps no ledger.
    async fn start_run(&self, query: &RedditHarvestQuery) -> Result<Option<String>, StorageError>;

    /// Records the completion of the run identified by `run_id`.
    ///
    /// A `None` run id means the run was never recorded and nothing is persisted.
    async fn finish_run(
        &self,
        run_id: Option<&str>,
        outcome: &HarvestRunOutcome,
    ) -> Result<(), StorageError>;
}
