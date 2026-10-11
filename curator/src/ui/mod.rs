//! Embedded web UI assets and static file serving.

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// Embedded static assets for the Curator single-page application.
#[derive(RustEmbed)]
#[folder = "src/ui/assets/"]
pub struct Assets;

/// Axum fallback handler for serving embedded static assets with MIME detection
/// and SPA routing fallback to `index.html`.
pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let raw_path = uri.path();
    let path = raw_path.trim_start_matches('/');

    // Do not handle unknown API routes in the static fallback
    if path == "api" || path.starts_with("api/") {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"error":"API endpoint not found"}"#))
            .unwrap();
    }

    let file_path = if path.is_empty() {
        "index.html"
    } else {
        path
    };

    match Assets::get(file_path) {
        Some(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            let content_type = HeaderValue::from_str(mime.as_ref())
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // SPA fallback: return index.html for unmatched browser routes
            match Assets::get("index.html") {
                Some(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/html; charset=utf-8"),
                    )
                    .body(Body::from(content.data))
                    .unwrap(),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    )
                    .body(Body::from("404 Not Found"))
                    .unwrap(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_assets_exist() {
        assert!(Assets::get("index.html").is_some());
        assert!(Assets::get("style.css").is_some());
        assert!(Assets::get("app.js").is_some());
    }

    #[test]
    fn test_embedded_index_html_contains_triage_deck_elements() {
        let file = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(file.data.as_ref()).expect("index.html must be valid utf8");

        assert!(html.contains("DUNGEON FORGE"));
        assert!(html.contains("triage-canvas"));
        assert!(html.contains("theme-selector"));
        assert!(html.contains("data-theme=\"building\""));
        assert!(html.contains("rating-selector"));
        assert!(html.contains("btn-grid-toggle"));
        assert!(html.contains("btn-fit-screen"));
        assert!(html.contains("btn-reject"));
        assert!(html.contains("btn-curate"));
        assert!(html.contains("rejection-modal"));
        assert!(html.contains("help-modal"));
        assert!(html.contains("toast-container"));
        assert!(html.contains("empty-state"));
    }

    #[test]
    fn test_embedded_index_html_contains_ingestion_drawer_elements() {
        let file = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(file.data.as_ref()).expect("index.html must be valid utf8");

        assert!(html.contains("btn-open-drawer"));
        assert!(html.contains("ingestion-drawer"));
        assert!(html.contains("harvest-sort-group"));
        assert!(html.contains("harvest-time-group"));
        assert!(html.contains("harvest-limit"));
        assert!(html.contains("harvest-limit-value"));
        assert!(html.contains("harvest-gridless-only"));
        assert!(html.contains("btn-start-harvest"));
        assert!(html.contains("harvest-progress-container"));
        assert!(html.contains("harvest-progress-bar"));
        assert!(html.contains("harvest-progress-text"));
    }

    #[test]
    fn test_embedded_app_js_contains_triage_engine_and_hotkeys() {
        let file = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(file.data.as_ref()).expect("app.js must be valid utf8");

        // Canvas 2D and 64px VTT grid overlay
        assert!(js.contains("VTT_GRID_SIZE"));
        assert!(js.contains("drawVttGrid"));
        assert!(js.contains("fitToScreen"));

        // In-memory prefetching
        assert!(js.contains("imageCache"));
        assert!(js.contains("prefetchUpcoming"));

        // Hotkeys & triage API calls
        assert!(js.contains("/api/staging"));
        assert!(js.contains("/api/curate"));
        assert!(js.contains("/api/reject"));
        assert!(js.contains("curateCurrentCandidate"));
        assert!(js.contains("confirmRejection"));

        assert!(js.contains("\"building\""));
        // Ingestion drawer & live harvesting
        assert!(js.contains("/api/harvest/reddit"));
        assert!(js.contains("openIngestionDrawer"));
        assert!(js.contains("closeIngestionDrawer"));
        assert!(js.contains("startRedditHarvest"));
        assert!(js.contains("handleHarvestEvent"));
    }

    #[test]
    fn test_embedded_style_css_contains_triage_styling() {
        let file = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(file.data.as_ref()).expect("style.css must be valid utf8");

        assert!(css.contains("triage-viewport"));
        assert!(css.contains("theme-badge"));
        assert!(css.contains("rating-stars"));
        assert!(css.contains("[data-theme=\"building\"]"));
        assert!(css.contains("modal-backdrop"));
        assert!(css.contains("toast-container"));

        // Ingestion drawer & harvesting styling
        assert!(css.contains("drawer-backdrop"));
        assert!(css.contains("drawer-panel"));
        assert!(css.contains("btn-harvest"));
        assert!(css.contains("slider-switch"));
        assert!(css.contains("progress-bar-fill"));
    }

    #[tokio::test]
    async fn test_static_handler_index() {
        let uri = "/".parse::<Uri>().unwrap();
        let resp = static_handler(uri).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/html"
        );
    }

    #[tokio::test]
    async fn test_static_handler_css() {
        let uri = "/style.css".parse::<Uri>().unwrap();
        let resp = static_handler(uri).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/css"
        );
    }

    #[tokio::test]
    async fn test_static_handler_spa_fallback() {
        let uri = "/candidates/123".parse::<Uri>().unwrap();
        let resp = static_handler(uri).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/html; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn test_static_handler_api_fallback_404() {
        let uri = "/api/unknown".parse::<Uri>().unwrap();
        let resp = static_handler(uri).await.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
    }
}
