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
use tower_http::trace::TraceLayer;

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
        .route("/api/harvest/reddit", post(post_harvest_reddit));

    Router::new()
        .merge(api_routes)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .fallback(crate::ui::static_handler)
        .with_state(state)
}
