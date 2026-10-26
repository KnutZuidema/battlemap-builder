//! API routing, request handling, and serialization.

pub mod error;
pub mod routes;
pub mod state;

pub use error::ApiError;
pub use routes::*;
pub use state::AppState;

use axum::{
    routing::{get, patch, post},
    Router,
};
use axum::http::{Request, Response};
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, MakeSpan, OnResponse, TraceLayer};
use tracing::{Level, Span};

/// Builds the complete Axum application router mounting API endpoints,
/// CORS and tracing middleware, and the static UI assets fallback.
pub fn create_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/staging", get(get_staging))
        .route("/api/curated", get(get_curated))
        .route("/api/curated/{id}", patch(patch_curated).delete(delete_curated))
        .route("/api/curated/bulk-delete", post(post_curated_bulk_delete))
        .route(
            "/api/curated/{id}/suggest-themes",
            post(post_curated_suggest_themes).get(post_curated_suggest_themes),
        )
        .route(
            "/api/curated/{id}/describe",
            post(post_curated_describe).get(post_curated_describe),
        )
        .route("/api/dataset/export", get(get_dataset_export))
        .route("/api/dataset/export-selected", post(post_dataset_export_selected))
        .route("/api/dataset/readiness", get(get_dataset_readiness))
        .route(
            "/api/training/datasets",
            get(get_training_datasets).post(post_training_dataset),
        )
        .route(
            "/api/training/datasets/freeze/status",
            get(get_training_dataset_freeze_status),
        )
        .route(
            "/api/training/datasets/{id}",
            get(get_training_dataset).delete(delete_training_dataset),
        )
        .route(
            "/api/training/datasets/{id}/archive",
            get(get_training_dataset_archive),
        )
        .route(
            "/api/training/datasets/{id}/download",
            get(get_training_dataset_archive),
        )
        .route(
            "/api/training/runs",
            get(get_training_runs).post(post_training_run),
        )
        .route("/api/training/runs/{id}", get(get_training_run))
        .route("/api/training/runs/{id}/dispatch", post(post_training_run_dispatch))
        .route("/api/training/runs/{id}/cancel", post(post_training_run_cancel))
        .route("/api/training/runs/{id}/publish", post(post_training_run_publish))
        .route("/api/training/runs/{id}/verify", post(post_training_run_verify))
        .route("/api/training/runs/{id}/metrics", get(get_training_run_metrics))
        .route("/api/training/adapters", get(get_training_adapters))
        .route("/api/training/adapters/{id}", get(get_training_adapter))
        .route("/api/training/adapters/{id}/retire", post(post_retire_training_adapter))
        .route("/api/image/staging/{id}", get(get_staging_image))
        .route("/api/image/staging/{id}/{layer_index}", get(get_staging_layer_image))
        .route("/api/image/curated/{id}", get(get_curated_image_by_id))
        .route("/api/image/curated/{id}/{layer_index}", get(get_curated_image_or_layer))
        .route(
            "/api/staging/{id}/layers",
            patch(patch_staging_layers).post(patch_staging_layers),
        )
        .route("/api/staging/{id}/grid", patch(patch_staging_grid))
        .route("/api/staging/{id}/split", post(post_split_staging))
        .route("/api/staging/{parent_id}/regroup", post(post_regroup_staging))
        .route(
            "/api/staging/{id}/suggest-themes",
            post(post_suggest_themes).get(post_suggest_themes),
        )
        .route(
            "/api/staging/{id}/describe",
            post(post_describe_candidate).get(post_describe_candidate),
        )
        .route("/api/curate", post(post_curate))
        .route("/api/reject", post(post_reject))
        .route("/api/reject/undo", post(post_undo_reject))
        .route("/api/harvest/reddit", post(post_harvest_reddit))
        .route("/api/cost/stats", get(get_cost_stats))
        .route("/api/cost/reset-session", post(post_reset_cost_session))
        .route("/api/models", get(get_models))
        .route("/api/benchmark/run", post(post_benchmark_run))
        .route("/api/benchmarks/run", post(post_benchmark_run))
        .route("/api/benchmark/generate", post(post_benchmark_run))
        .route("/api/benchmarks/generate", post(post_benchmark_run))
        .route("/api/benchmark/cancel", post(post_benchmark_cancel))
        .route("/api/benchmarks/cancel", post(post_benchmark_cancel))
        .route("/api/storage/stats", get(get_storage_stats))
        .route("/api/storage/purge-staging", post(post_purge_staging));

    Router::new()
        .merge(api_routes)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(CuratorMakeSpan::new())
                .on_response(CuratorOnResponse::new()),
        )
        .fallback(crate::ui::static_handler)
        .with_state(state)
}

/// Custom MakeSpan strategy that emits DEBUG spans for /api/status and INFO for all other endpoints.
#[derive(Clone, Debug)]
pub struct CuratorMakeSpan {
    info_make_span: DefaultMakeSpan,
    debug_make_span: DefaultMakeSpan,
}

impl CuratorMakeSpan {
    pub fn new() -> Self {
        Self {
            info_make_span: DefaultMakeSpan::new().level(Level::INFO),
            debug_make_span: DefaultMakeSpan::new().level(Level::DEBUG),
        }
    }
}

impl Default for CuratorMakeSpan {
    fn default() -> Self {
        Self::new()
    }
}


impl<B> MakeSpan<B> for CuratorMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        if request.uri().path() == "/api/status" {
            self.debug_make_span.make_span(request)
        } else {
            self.info_make_span.make_span(request)
        }
    }
}

/// Custom OnResponse strategy that emits DEBUG response logs for /api/status and INFO for all other endpoints.
#[derive(Clone, Debug)]
pub struct CuratorOnResponse {
    info_on_response: DefaultOnResponse,
    debug_on_response: DefaultOnResponse,
}

impl CuratorOnResponse {
    pub fn new() -> Self {
        Self {
            info_on_response: DefaultOnResponse::new().level(Level::INFO),
            debug_on_response: DefaultOnResponse::new().level(Level::DEBUG),
        }
    }
}

impl Default for CuratorOnResponse {
    fn default() -> Self {
        Self::new()
    }
}


impl<B> OnResponse<B> for CuratorOnResponse {
    fn on_response(self, response: &Response<B>, latency: Duration, span: &Span) {
        let is_debug = span.is_disabled()
            || span
                .metadata()
                .is_some_and(|meta| *meta.level() == Level::DEBUG);

        if is_debug {
            self.debug_on_response.on_response(response, latency, span);
        } else {
            self.info_on_response.on_response(response, latency, span);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    #[test]
    fn test_curator_make_span_levels() {
        let mut make_span = CuratorMakeSpan::new();

        let status_req = Request::builder()
            .uri("/api/status")
            .body(())
            .unwrap();
        let status_span = make_span.make_span(&status_req);
        assert_eq!(
            status_span.metadata().map(|m| *m.level()),
            Some(Level::DEBUG)
        );

        let staging_req = Request::builder()
            .uri("/api/staging")
            .body(())
            .unwrap();
        let staging_span = make_span.make_span(&staging_req);
        assert_eq!(
            staging_span.metadata().map(|m| *m.level()),
            Some(Level::INFO)
        );

        let root_req = Request::builder()
            .uri("/")
            .body(())
            .unwrap();
        let root_span = make_span.make_span(&root_req);
        assert_eq!(
            root_span.metadata().map(|m| *m.level()),
            Some(Level::INFO)
        );
    }

    #[derive(Default)]
    struct LevelCaptureLayer {
        levels: Arc<Mutex<Vec<Level>>>,
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LevelCaptureLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.levels.lock().unwrap().push(*event.metadata().level());
        }
    }

    #[test]
    fn test_curator_on_response_levels() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let layer = LevelCaptureLayer {
            levels: captured.clone(),
        };
        let subscriber = Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            let on_response = CuratorOnResponse::new();
            let response = Response::builder().status(200).body(()).unwrap();

            let debug_span = tracing::span!(Level::DEBUG, "request");
            let _guard = debug_span.enter();
            on_response.clone().on_response(&response, Duration::from_millis(5), &debug_span);

            let info_span = tracing::span!(Level::INFO, "request");
            let _guard2 = info_span.enter();
            on_response.on_response(&response, Duration::from_millis(5), &info_span);
        });

        let events = captured.lock().unwrap().clone();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], Level::DEBUG);
        assert_eq!(events[1], Level::INFO);
    }

    #[test]
    fn test_curator_on_response_disabled_span_does_not_emit_info() {
        use tracing_subscriber::filter::LevelFilter;

        let captured = Arc::new(Mutex::new(Vec::new()));
        let layer = LevelCaptureLayer {
            levels: captured.clone(),
        };
        let subscriber = Registry::default().with(layer).with(LevelFilter::INFO);

        tracing::subscriber::with_default(subscriber, || {
            let mut make_span = CuratorMakeSpan::new();
            let on_response = CuratorOnResponse::new();

            let req = Request::builder()
                .uri("/api/status")
                .body(())
                .unwrap();
            let span = make_span.make_span(&req);
            assert!(span.is_disabled());

            let response = Response::builder().status(200).body(()).unwrap();
            on_response.on_response(&response, Duration::from_millis(5), &span);
        });

        let events = captured.lock().unwrap().clone();
        assert!(
            events.is_empty(),
            "expected no events emitted for disabled /api/status span under INFO filter, got: {events:?}"
        );
    }
}
