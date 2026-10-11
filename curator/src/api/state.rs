use std::sync::Arc;
use std::time::Instant;

use crate::adapters::RedditCrawler;
use crate::ports::harvester::RedditHarvester;
use crate::ports::storage::StorageRepository;

/// Shared application state for the Axum HTTP server.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageRepository + Send + Sync>,
    pub harvester: Arc<dyn RedditHarvester + Send + Sync>,
    pub start_time: Instant,
}

impl AppState {
    /// Creates a new `AppState` wrapping the given storage repository and default RedditCrawler.
    pub fn new(storage: Arc<dyn StorageRepository + Send + Sync>) -> Self {
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::new()),
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
            start_time: Instant::now(),
        }
    }
}
