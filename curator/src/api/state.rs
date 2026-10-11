use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::{FileCostLedger, OpenRouterAdapter, RedditCrawler};
use crate::api::error::ApiError;
use crate::ports::cost_tracker::CostTracker;
use crate::ports::harvester::RedditHarvester;
use crate::ports::storage::StorageRepository;
use crate::services::BenchmarkService;
/// Shared application state for the Axum HTTP server.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageRepository + Send + Sync>,
    pub harvester: Arc<dyn RedditHarvester + Send + Sync>,
    pub cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    pub benchmark_service: Option<Arc<BenchmarkService>>,
    pub benchmark_cancelled: Arc<AtomicBool>,
    pub start_time: Instant,
}

impl AppState {
    /// Creates a new `AppState` wrapping the given storage repository and default RedditCrawler.
    pub fn new(storage: Arc<dyn StorageRepository + Send + Sync>) -> Self {
        let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::default());
        let benchmark_service = Self::default_benchmark_service(storage.clone(), cost_tracker.clone());
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom harvester (useful for testing or customized adapters).
    pub fn with_harvester(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
    ) -> Self {
        let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::default());
        let benchmark_service = Self::default_benchmark_service(storage.clone(), cost_tracker.clone());
        Self {
            storage,
            harvester,
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom cost tracker.
    pub fn with_cost_tracker(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        let benchmark_service = Self::default_benchmark_service(storage.clone(), cost_tracker.clone());
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with custom storage, harvester, and cost tracker.
    pub fn with_all(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        let benchmark_service = Self::default_benchmark_service(storage.clone(), cost_tracker.clone());
        Self {
            storage,
            harvester,
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    /// Creates an `AppState` with an explicit benchmark service (ideal for testing with mock engines).
    pub fn with_benchmark_service(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        benchmark_service: Arc<BenchmarkService>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Self {
        Self {
            storage,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service: Some(benchmark_service),
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    /// Helper to try instantiating OpenRouterAdapter from environment if keys are present.
    fn default_benchmark_service(
        storage: Arc<dyn StorageRepository + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    ) -> Option<Arc<BenchmarkService>> {
        OpenRouterAdapter::from_env()
            .ok()
            .map(|adapter| {
                Arc::new(BenchmarkService::with_cost_tracker(
                    Arc::new(adapter),
                    storage,
                    cost_tracker,
                ))
            })
    }

    /// Obtains reference to existing benchmark service or attempts on-demand resolution.
    pub fn get_or_create_benchmark_service(&self) -> Result<Arc<BenchmarkService>, ApiError> {
        if let Some(service) = &self.benchmark_service {
            return Ok(service.clone());
        }

        if let Some(service) = Self::default_benchmark_service(self.storage.clone(), self.cost_tracker.clone()) {
            return Ok(service);
        }

        Err(ApiError::bad_request(
            "Benchmark engine not available: OPENROUTER_API_KEY is not configured in server/.env or environment",
        ))
    }
}
