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

    /// The ES module graph is served straight out of the binary: a file missing from
    /// `src/ui/assets/` breaks the page at import time, with no Rust compile error.
    #[test]
    fn test_assets_are_embedded() {
        for asset in [
            "index.html",
            "style.css",
            "app.js",
            "dom.js",
            "store.js",
            "dataset-scope.js",
            "kit/dom.js",
            "kit/format.js",
            "kit/grid-math.js",
            "kit/layers.js",
            "kit/tags.js",
            "stage/canvas.js",
            "stage/grid-panel.js",
            "views/triage.js",
            "views/curation.js",
            "views/inspector.js",
            "views/library.js",
            "views/library/bulk.js",
            "views/library/fullscreen.js",
            "views/readiness.js",
            "views/snapshots.js",
            "views/training.js",
        ] {
            assert!(Assets::get(asset).is_some(), "{asset} must be embedded");
        }
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
