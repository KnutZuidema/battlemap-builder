//! Ledger adapter for harvest runs on backends without durable storage.
//!
//! When `DATABASE_URL` is unset the catalog lives on the filesystem and there is nowhere to record
//! a harvest run; outcomes are reported through `tracing` instead.

use async_trait::async_trait;
use tracing::{info, warn};

use crate::domain::RedditHarvestQuery;
use crate::ports::harvest_run_ledger::{HarvestRunLedger, HarvestRunOutcome};
use crate::ports::storage_error::StorageError;

/// [`HarvestRunLedger`] that keeps no ledger and logs every run outcome.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHarvestRunLedger;

#[async_trait]
impl HarvestRunLedger for NoopHarvestRunLedger {
    async fn start_run(&self, query: &RedditHarvestQuery) -> Result<Option<String>, StorageError> {
        tracing::debug!(
            subreddit = %query.subreddit,
            sort = %query.sort,
            "Harvest run ledger disabled (no DATABASE_URL); outcome will be logged"
        );
        Ok(None)
    }

    async fn finish_run(
        &self,
        run_id: Option<&str>,
        outcome: &HarvestRunOutcome,
    ) -> Result<(), StorageError> {
        match &outcome.error {
            Some(error) => warn!(
                run_id = ?run_id,
                posts_scanned = outcome.posts_scanned,
                downloaded = outcome.downloaded,
                skipped = outcome.skipped,
                error = %error,
                "Reddit harvest run failed (harvest run ledger disabled)"
            ),
            None => info!(
                run_id = ?run_id,
                posts_scanned = outcome.posts_scanned,
                downloaded = outcome.downloaded,
                skipped = outcome.skipped,
                "Reddit harvest run completed (harvest run ledger disabled)"
            ),
        }
        Ok(())
    }
}
