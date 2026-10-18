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
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};

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
        .route("/api/image/staging/{id}", get(get_staging_image))
        .route("/api/image/staging/{id}/{layer_index}", get(get_staging_layer_image))
        .route("/api/image/curated/{id}", get(get_curated_image_by_id))
        .route("/api/image/curated/{id}/{layer_index}", get(get_curated_image_or_layer))
        .route("/api/image/curated/{theme}/{id}/{layer_index}", get(get_curated_layer_image))
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
        .route("/api/harvest/reddit", post(post_harvest_reddit))
        .route("/api/cost/stats", get(get_cost_stats))
        .route("/api/cost/reset-session", post(post_reset_cost_session))
        .route("/api/benchmark/run", post(post_benchmark_run))
        .route("/api/benchmark/cancel", post(post_benchmark_cancel))
        .route("/api/storage/stats", get(get_storage_stats))
        .route("/api/storage/purge-staging", post(post_purge_staging));

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
