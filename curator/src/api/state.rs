use std::sync::Arc;
use std::time::Instant;

use crate::ports::storage::StorageRepository;

/// Shared application state for the Axum HTTP server.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageRepository + Send + Sync>,
    pub start_time: Instant,
}

impl AppState {
    /// Creates a new `AppState` wrapping the given storage repository.
    pub fn new(storage: Arc<dyn StorageRepository + Send + Sync>) -> Self {
        Self {
            storage,
            start_time: Instant::now(),
        }
    }
}
