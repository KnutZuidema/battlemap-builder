use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::adapters::{FileCostLedger, OpenRouterAdapter, RedditCrawler, RoutingModelEngine, WaveSpeedAdapter};
use crate::api::error::ApiError;
use crate::ports::blob_storage::BlobStorage;
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::cost_tracker::CostTracker;
use crate::ports::harvester::RedditHarvester;
use crate::services::BenchmarkService;
use crate::services::curation::{CurationService, CurationWorkflow};
use crate::services::description::{DescriptionGenerator, DescriptionService};

/// Shared application state for the Axum HTTP server.
#[derive(Clone)]
pub struct AppState {
    pub catalog: Arc<dyn CatalogRepository + Send + Sync>,
    pub blobs: Arc<dyn BlobStorage + Send + Sync>,
    pub harvester: Arc<dyn RedditHarvester + Send + Sync>,
    pub cost_tracker: Arc<dyn CostTracker + Send + Sync>,
    pub benchmark_service: Option<Arc<BenchmarkService>>,
    pub benchmark_cancelled: Arc<AtomicBool>,
    pub theme_suggestions_cache: Arc<RwLock<HashMap<String, Vec<String>>>>,
    pub openrouter: Option<Arc<OpenRouterAdapter>>,
    pub wavespeed: Option<Arc<WaveSpeedAdapter>>,
    pub description_service: Arc<DescriptionService>,
    pub curation_service: Arc<dyn CurationWorkflow>,
    pub start_time: Instant,
}

impl AppState {
    fn default_description_service(openrouter: Option<&Arc<OpenRouterAdapter>>) -> Arc<DescriptionService> {
        let generator: Option<Arc<dyn DescriptionGenerator>> =
            openrouter.map(|a| a.clone() as Arc<dyn DescriptionGenerator>);
        Arc::new(DescriptionService::new(generator))
    }
    fn default_curation_service(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
        description_service: Arc<DescriptionService>,
    ) -> Arc<dyn CurationWorkflow> {
        Arc::new(CurationService::new(catalog, blobs, description_service))
    }


    /// Creates a new `AppState` over the catalog and blob ports with the default RedditCrawler.
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::default());
        let benchmark_service = Self::default_benchmark_service(
            cost_tracker.clone(),
            catalog.clone(),
            blobs.clone(),
        );
        let openrouter = OpenRouterAdapter::from_env().ok().map(Arc::new);
        let wavespeed = WaveSpeedAdapter::from_env().ok().map(Arc::new);
        let description_service = Self::default_description_service(openrouter.as_ref());
        let curation_service = Self::default_curation_service(catalog.clone(), blobs.clone(), description_service.clone());
        Self {
            catalog,
            blobs,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            theme_suggestions_cache: Arc::new(RwLock::new(HashMap::new())),
            openrouter,
            wavespeed,
            description_service,
            curation_service,
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom harvester (useful for testing or customized adapters).
    pub fn with_harvester(
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        let cost_tracker: Arc<dyn CostTracker + Send + Sync> = Arc::new(FileCostLedger::default());
        let benchmark_service = Self::default_benchmark_service(
            cost_tracker.clone(),
            catalog.clone(),
            blobs.clone(),
        );
        let openrouter = OpenRouterAdapter::from_env().ok().map(Arc::new);
        let wavespeed = WaveSpeedAdapter::from_env().ok().map(Arc::new);
        let description_service = Self::default_description_service(openrouter.as_ref());
        let curation_service = Self::default_curation_service(catalog.clone(), blobs.clone(), description_service.clone());
        Self {
            catalog,
            blobs,
            harvester,
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            theme_suggestions_cache: Arc::new(RwLock::new(HashMap::new())),
            openrouter,
            wavespeed,
            description_service,
            curation_service,
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom cost tracker.
    pub fn with_cost_tracker(
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        let benchmark_service = Self::default_benchmark_service(
            cost_tracker.clone(),
            catalog.clone(),
            blobs.clone(),
        );
        let openrouter = OpenRouterAdapter::from_env().ok().map(Arc::new);
        let wavespeed = WaveSpeedAdapter::from_env().ok().map(Arc::new);
        let description_service = Self::default_description_service(openrouter.as_ref());
        let curation_service = Self::default_curation_service(catalog.clone(), blobs.clone(), description_service.clone());
        Self {
            catalog,
            blobs,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            theme_suggestions_cache: Arc::new(RwLock::new(HashMap::new())),
            openrouter,
            wavespeed,
            description_service,
            curation_service,
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with a custom harvester and cost tracker.
    pub fn with_all(
        harvester: Arc<dyn RedditHarvester + Send + Sync>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        let benchmark_service = Self::default_benchmark_service(
            cost_tracker.clone(),
            catalog.clone(),
            blobs.clone(),
        );
        let openrouter = OpenRouterAdapter::from_env().ok().map(Arc::new);
        let wavespeed = WaveSpeedAdapter::from_env().ok().map(Arc::new);
        let description_service = Self::default_description_service(openrouter.as_ref());
        let curation_service = Self::default_curation_service(catalog.clone(), blobs.clone(), description_service.clone());
        Self {
            catalog,
            blobs,
            harvester,
            cost_tracker,
            benchmark_service,
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            theme_suggestions_cache: Arc::new(RwLock::new(HashMap::new())),
            openrouter,
            wavespeed,
            description_service,
            curation_service,
            start_time: Instant::now(),
        }
    }

    /// Creates an `AppState` with an explicit benchmark service (ideal for testing with mock engines).
    pub fn with_benchmark_service(
        benchmark_service: Arc<BenchmarkService>,
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        let openrouter = OpenRouterAdapter::from_env().ok().map(Arc::new);
        let wavespeed = WaveSpeedAdapter::from_env().ok().map(Arc::new);
        let description_service = Self::default_description_service(openrouter.as_ref());
        let curation_service = Self::default_curation_service(catalog.clone(), blobs.clone(), description_service.clone());
        Self {
            catalog,
            blobs,
            harvester: Arc::new(RedditCrawler::from_env()),
            cost_tracker,
            benchmark_service: Some(benchmark_service),
            benchmark_cancelled: Arc::new(AtomicBool::new(false)),
            theme_suggestions_cache: Arc::new(RwLock::new(HashMap::new())),
            openrouter,
            wavespeed,
            description_service,
            curation_service,
            start_time: Instant::now(),
        }
    }

    /// Creates a new `AppState` with an explicit OpenRouter adapter (ideal for testing).
    pub fn with_openrouter(mut self, openrouter: Arc<OpenRouterAdapter>) -> Self {
        self.openrouter = Some(openrouter.clone());
        let desc_service = Arc::new(DescriptionService::new(Some(
            openrouter as Arc<dyn DescriptionGenerator>,
        )));
        self.description_service = desc_service.clone();
        self.curation_service = Self::default_curation_service(
            self.catalog.clone(),
            self.blobs.clone(),
            desc_service,
        );
        self
    }

    /// Creates a new `AppState` with an explicit description service (ideal for testing).
    pub fn with_description_service(mut self, description_service: Arc<DescriptionService>) -> Self {
        self.description_service = description_service.clone();
        self.curation_service = Self::default_curation_service(
            self.catalog.clone(),
            self.blobs.clone(),
            description_service,
        );
        self
    }

    /// Creates a new `AppState` with an explicit curation workflow (ideal for testing).
    pub fn with_curation_service(mut self, curation_service: Arc<dyn CurationWorkflow>) -> Self {
        self.curation_service = curation_service;
        self
    }

    /// Obtains reference to existing OpenRouter adapter or attempts on-demand resolution.
    pub fn openrouter_client(&self) -> Option<Arc<OpenRouterAdapter>> {
        if let Some(adapter) = &self.openrouter {
            return Some(adapter.clone());
        }
        OpenRouterAdapter::from_env().ok().map(Arc::new)
    }

    /// Obtains reference to existing WaveSpeed adapter or attempts on-demand resolution.
    pub fn wavespeed_client(&self) -> Option<Arc<WaveSpeedAdapter>> {
        if let Some(adapter) = &self.wavespeed {
            return Some(adapter.clone());
        }
        WaveSpeedAdapter::from_env().ok().map(Arc::new)
    }

    /// Helper to try instantiating RoutingModelEngine from environment if keys are present.
    fn default_benchmark_service(
        cost_tracker: Arc<dyn CostTracker + Send + Sync>,
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Option<Arc<BenchmarkService>> {
        let openrouter_ok = OpenRouterAdapter::from_env().is_ok();
        let wavespeed_ok = WaveSpeedAdapter::from_env().is_ok();

        if !openrouter_ok && !wavespeed_ok {
            return None;
        }

        let openrouter = OpenRouterAdapter::from_env().unwrap_or_else(|_| OpenRouterAdapter::new(""));
        let wavespeed = WaveSpeedAdapter::from_env().unwrap_or_else(|_| WaveSpeedAdapter::new(""));
        let routing_engine = Arc::new(RoutingModelEngine::new(
            Arc::new(openrouter),
            Arc::new(wavespeed),
        ));

        Some(Arc::new(BenchmarkService::with_cost_tracker(
            routing_engine,
            cost_tracker,
            catalog,
            blobs,
        )))
    }

    /// Obtains reference to existing benchmark service or attempts on-demand resolution.
    pub fn get_or_create_benchmark_service(&self) -> Result<Arc<BenchmarkService>, ApiError> {
        if let Some(service) = &self.benchmark_service {
            return Ok(service.clone());
        }

        if let Some(service) = Self::default_benchmark_service(
            self.cost_tracker.clone(),
            self.catalog.clone(),
            self.blobs.clone(),
        ) {
            return Ok(service);
        }

        Err(ApiError::bad_request(
            "Benchmark engine not available: neither OPENROUTER_API_KEY nor WAVESPEED_API_KEY is configured in server/.env or environment",
        ))
    }
}
