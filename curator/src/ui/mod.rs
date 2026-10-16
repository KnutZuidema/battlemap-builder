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
        assert!(!html.contains("theme-selector"));
        assert!(!html.contains("rating-selector"));
        assert!(html.contains("library-search-input"));
        assert!(html.contains("library-tag-pills"));
        assert!(html.contains("btn-grid-toggle"));
        assert!(html.contains("btn-fit-screen"));
        assert!(html.contains("btn-reject"));
        assert!(html.contains("btn-curate"));
        assert!(html.contains("title=\"Curate Candidate (Hotkey: Enter)\""));
        assert!(html.contains("title=\"Reject Candidate (Hotkey: Esc)\""));
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
        assert!(js.contains("panStep"));
        assert!(js.contains("zoomStep"));
        assert!(js.contains("activePanKeys"));
        assert!(js.contains("panLoop"));
        assert!(js.contains("startPanLoop"));
        assert!(js.contains("stopPanLoop"));
        // In-memory prefetching
        assert!(js.contains("imageCache"));
        assert!(js.contains("prefetchUpcoming"));

        // Hotkeys & triage API calls
        assert!(js.contains("/api/staging"));
        assert!(js.contains("/api/curate"));
        assert!(js.contains("/api/reject"));
        assert!(js.contains("curateCurrentCandidate"));
        assert!(js.contains("confirmRejection"));

        // Two-step labeling mode hotkeys & selectors
        assert!(js.contains("enterLabelingMode"));
        assert!(js.contains("exitLabelingMode"));
        assert!(js.contains("selecting-mode"));

        assert!(js.contains("\"building\""));
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
        assert!(css.contains("search-input"));
        assert!(css.contains("library-tag-pills"));
        assert!(css.contains("selecting-mode"));
        assert!(css.contains("modal-backdrop"));
        assert!(css.contains("toast-container"));
        // Ingestion drawer & harvesting styling
        assert!(css.contains("drawer-backdrop"));
        assert!(css.contains("drawer-panel"));
        assert!(css.contains("btn-harvest"));
        assert!(css.contains("slider-switch"));
        assert!(css.contains("progress-bar-fill"));
    }
    #[test]
    fn test_embedded_assets_contain_two_step_labeling_workflow() {
        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");

        // Verify state initialization
        assert!(js.contains("labelingMode: null"));

        // Verify helper functions
        assert!(js.contains("function enterLabelingMode(mode)"));
        assert!(js.contains("function exitLabelingMode()"));
        assert!(js.contains("roleSelectorEl.classList.add(\"selecting-mode\")"));
        assert!(js.contains("roleSelectorEl.classList.remove(\"selecting-mode\")"));

        // Verify upfront theme and rating labeling mode triggers are stripped
        assert!(!js.contains("enterLabelingMode(\"theme\")"));
        assert!(!js.contains("enterLabelingMode(\"rating\")"));

        // Verify role mode hotkey dispatching
        assert!(js.contains("enterLabelingMode(\"role\")"));
        assert!(js.contains("state.labelingMode === \"role\""));
        // Verify Escape dismisses labeling mode
        assert!(js.contains("if (state.labelingMode)"));
        assert!(js.contains("exitLabelingMode();"));

        // Verify HTML tooltips & guide
        let index_html = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_html.data.as_ref()).expect("index.html must be valid utf8");
        assert!(!html.contains("Theme Selection (Press 1, then 1-8)"));
        assert!(!html.contains("Quality Rating (Press 2, then 1-5)"));
        assert!(!html.contains("Theme Classification (Two-Step Hotkey)"));
        assert!(!html.contains("Quality Rating (Two-Step Hotkey)"));
        assert!(html.contains("Trigger Layer Role mode"));

        // Verify CSS styling
        let style_css = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(style_css.data.as_ref()).expect("style.css must be valid utf8");
        assert!(css.contains(".role-selector.selecting-mode"));
        assert!(css.contains("selecting-mode-pulse-violet"));
    }

    #[test]
    fn test_embedded_assets_contain_dynamic_grid_overlay() {
        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");

        // Verify dynamic grid state initialization
        assert!(js.contains("gridSize: 64"));
        assert!(js.contains("gridOffset: { x: 0, y: 0 }"));
        assert!(js.contains("gridAlignMode: false"));
        assert!(js.contains("gridPanelOpen: false"));
        assert!(js.contains("isDraggingGrid: false"));
        assert!(js.contains("candidateGridSettings = new Map()"));

        // Verify grid helper functions
        assert!(js.contains("function setGridSize("));
        assert!(js.contains("function nudgeGridOffset("));
        assert!(js.contains("function setGridOffset("));
        assert!(js.contains("function resetGrid("));
        assert!(js.contains("function toggleGridPanel("));
        assert!(js.contains("function toggleGridAlignMode("));
        assert!(js.contains("function updateGridPanel()"));

        // Verify drawVttGrid clipping and modulo offset
        assert!(js.contains("c.clip()"));
        assert!(js.contains("state.gridOffset.x"));
        assert!(js.contains("state.gridOffset.y"));

        // Verify canvas drag alignment & class toggling
        assert!(js.contains("grid-dragging"));
        assert!(js.contains("grid-aligning"));

        // Verify hotkeys: Shift + G, Shift + Arrows, Shift + [ / ]
        assert!(js.contains("e.shiftKey && (e.key === \"g\" || e.key === \"G\")"));
        assert!(js.contains("toggleGridPanel()"));
        assert!(js.contains("nudgeGridOffset(0, -step)"));
        assert!(js.contains("nudgeGridOffset(0, step)"));
        assert!(js.contains("nudgeGridOffset(-step, 0)"));
        assert!(js.contains("nudgeGridOffset(step, 0)"));

        // Verify index.html contains grid calibration elements
        let index_html = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_html.data.as_ref()).expect("index.html must be valid utf8");
        assert!(html.contains("btn-grid-adjust"));
        assert!(html.contains("grid-adjust-panel"));
        assert!(html.contains("btn-close-grid-panel"));
        assert!(html.contains("grid-size-input"));
        assert!(html.contains("grid-size-slider"));
        assert!(html.contains("preset-chip"));
        assert!(html.contains("data-size=\"32\""));
        assert!(html.contains("data-size=\"50\""));
        assert!(html.contains("data-size=\"64\""));
        assert!(html.contains("data-size=\"70\""));
        assert!(html.contains("data-size=\"100\""));
        assert!(html.contains("data-size=\"128\""));
        assert!(html.contains("data-size=\"140\""));
        assert!(html.contains("grid-offset-x-val"));
        assert!(html.contains("grid-offset-y-val"));
        assert!(html.contains("btn-grid-nudge-up"));
        assert!(html.contains("btn-grid-nudge-down"));
        assert!(html.contains("btn-grid-nudge-left"));
        assert!(html.contains("btn-grid-nudge-right"));
        assert!(html.contains("btn-grid-align-drag"));
        assert!(html.contains("btn-grid-reset"));
        assert!(html.contains("btn-grid-done"));
        assert!(html.contains("Shift+G"));
        assert!(html.contains("Shift+Drag"));
        assert!(html.contains("Shift+Arrow Keys"));

        // Verify style.css contains styling rules
        let style_css = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(style_css.data.as_ref()).expect("style.css must be valid utf8");
        assert!(css.contains(".grid-adjust-panel"));
        assert!(css.contains(".grid-size-controls"));
        assert!(css.contains(".grid-slider"));
        assert!(css.contains(".preset-chip"));
        assert!(css.contains(".grid-nudge-pad"));
        assert!(css.contains(".btn-align-mode"));
        assert!(css.contains(".grid-aligning"));
        assert!(css.contains(".grid-dragging"));
    }

    #[test]
    fn test_embedded_assets_contain_decision_dialog_and_cards() {
        let index_html = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_html.data.as_ref()).expect("index.html must be valid utf8");

        assert!(html.contains("curate-decision-modal"));
        assert!(html.contains("decision-card-step"));
        assert!(html.contains("decision-custom-step"));
        assert!(html.contains("decision-card"));
        assert!(html.contains("decision-btn-reject"));
        assert!(html.contains("decision-btn-accept"));
        assert!(html.contains("decision-btn-undo"));
        assert!(html.contains("decision-tag-input"));
        assert!(html.contains("decision-autocomplete-list"));
        assert!(html.contains("decision-accepted-tags"));

        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("fetchThemeSuggestions"));
        assert!(js.contains("openCurateDecisionModal"));
        assert!(js.contains("closeCurateDecisionModal"));
        assert!(js.contains("handleDecisionReject"));
        assert!(js.contains("handleDecisionAccept"));
        assert!(js.contains("handleDecisionUndo"));
        assert!(js.contains("renderAutocompletePills"));
        assert!(js.contains("submitDecisionCuration"));
        assert!(js.contains("decisionModalOpen"));
        assert!(js.contains("suggest-themes"));

        let style_css = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(style_css.data.as_ref()).expect("style.css must be valid utf8");

        assert!(css.contains("#curate-decision-modal"));
        assert!(css.contains(".decision-card"));
        assert!(css.contains(".decision-btn-reject"));
        assert!(css.contains(".decision-btn-accept"));
        assert!(css.contains(".tag-chip"));
        assert!(css.contains(".autocomplete-pill"));
        assert!(css.contains(".shake"));
        assert!(css.contains("@keyframes shake"));
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
