//! API routing, request handling, and serialization.

pub mod error;
pub mod routes;
pub mod state;

pub use error::ApiError;
pub use routes::*;
pub use state::AppState;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};

/// Builds the complete Axum application router mounting API endpoints,
/// CORS and tracing middleware, and the static UI assets fallback.
pub fn create_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/staging", get(get_staging))
        .route("/api/image/staging/{id}", get(get_staging_image))
        .route("/api/image/curated/{theme}/{id}", get(get_curated_image))
        .route("/api/curate", post(post_curate))
        .route("/api/reject", post(post_reject))
        .route("/api/harvest/reddit", post(post_harvest_reddit))
        .route("/api/cost/stats", get(get_cost_stats))
        .route("/api/cost/reset-session", post(post_reset_cost_session))
        .route("/api/benchmark/run", post(post_benchmark_run))
        .route("/api/benchmark/cancel", post(post_benchmark_cancel));

    Router::new()
        .merge(api_routes)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .fallback(crate::ui::static_handler)
        .with_state(state)
}
