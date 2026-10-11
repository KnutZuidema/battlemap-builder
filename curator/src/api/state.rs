use std::sync::Arc;
use std::time::Instant;

use crate::adapters::{FileCostLedger, RedditCrawler};
use crate::ports::cost_tracker::CostTracker;
use crate::ports::harvester::RedditHarvester;
use crate::ports::storage::StorageRepository;

/// Shared application state for the Axum HTTP server.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageRepository + Send + Sync>,
    pub harvester: Arc<dyn RedditHarvester + Send + Sync>,
    pub cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    pub start_time: Instant,
}

impl AppState {
    /// Creates a new `AppState` wrapping the given storage repository and default RedditCrawler.
    pub fn new(storage: Arc<dyn StorageRepository + Send + Sync>) -> Self {
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker: Arc::new(FileCostLedger::default()),
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom harvester (useful for testing or customized adapters).
    pub fn with_harvester(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            harvester,
            cost_tracker: Arc::new(FileCostLedger::default()),
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom cost tracker.
    pub fn with_cost_tracker(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with custom storage, harvester, and cost tracker.
    pub fn with_all(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            harvester,
            cost_tracker,
            start_time: Instant::now(),
        }
    }
}
