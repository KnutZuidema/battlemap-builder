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
    fn test_embedded_assets_contain_training_prototype() {
        assert!(Assets::get("training-prototype.js").is_some());
        assert!(Assets::get("training-prototype.css").is_some());

        let index_file = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_file.data.as_ref()).expect("index.html must be valid utf8");
        assert!(html.contains("training-prototype.css"));
        assert!(html.contains("training-prototype.js"));

        let proto_file = Assets::get("training-prototype.js").expect("training-prototype.js must be embedded");
        let js = std::str::from_utf8(proto_file.data.as_ref()).expect("training-prototype.js must be valid utf8");
        assert!(js.contains("VARIANTS"));
        assert!(js.contains("proto-switcher-bar"));
        assert!(js.contains("vA-drawer"));
        assert!(js.contains("vB-workbench-view"));
        assert!(js.contains("vC-dynamic-island"));
        assert!(js.contains("dungeonforge battlemap"));
        assert!(js.contains("black-forest-labs/FLUX.1-dev"));
        assert!(js.contains("publishAdapterFromCheckpoint"));
        assert!(js.contains("edit-lora"));
        assert!(js.contains("text-to-image-lora"));
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
        assert!(!html.contains("rejection-modal"));
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
        assert!(js.contains("rejectCurrentCandidate"));

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
    fn test_embedded_app_js_contains_smooth_zoom_loop() {
        let file = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(file.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("activeZoomKeys"));
        assert!(js.contains("zoomLoop"));
        assert!(js.contains("startZoomLoop"));
        assert!(js.contains("stopZoomLoop"));
        assert!(js.contains("ZOOM_SPEED"));
        assert!(js.contains("BracketLeft"));
        assert!(js.contains("BracketRight"));
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
        // The persisted grid is authoritative: no in-memory per-candidate grid cache shadows it.
        assert!(!js.contains("candidateGridSettings"));
        assert!(js.contains("candidate.grid.cell_size_px"));
        assert!(js.contains("map.grid.cell_size_px"));

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
        assert!(html.contains("decision-description-input"));

        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("fetchThemeSuggestions"));
        assert!(js.contains("openCurateDecisionModal"));
        assert!(js.contains("closeCurateDecisionModal"));
        assert!(js.contains("validateCandidateLayers"));
        assert!(js.contains("handleDecisionReject"));
        assert!(js.contains("handleDecisionAccept"));
        assert!(js.contains("handleDecisionUndo"));
        assert!(js.contains("renderAutocompletePills"));
        assert!(js.contains("submitDecisionCuration"));
        assert!(js.contains("decisionModalOpen"));
        assert!(js.contains("suggest-themes"));
        assert!(js.contains("_fetchPromise"));
        assert!(js.contains("_suggestionsOffline"));
        assert!(js.contains("decision-description-input"));
        assert!(js.contains("curated-description-box"));
        assert!(js.contains("fetchCandidateDescription"));
        assert!(js.contains("renderCuratedCard"));

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
        assert!(css.contains("decision-description-input"));
        assert!(css.contains("curated-description-box"));
    }
    #[test]
    fn test_embedded_assets_contain_left_dock_layer_deck() {
        let style_css = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(style_css.data.as_ref()).expect("style.css must be valid utf8");

        // Layer deck anchored to left side rather than bottom center
        assert!(css.contains(".layer-deck {\n    position: absolute;\n    top: 1rem;\n    left: 1rem;"));
        assert!(!css.contains(".layer-deck {\n    position: absolute;\n    bottom: 1rem;\n    left: 50%;"));
        // Filmstrip is vertical column with scroll
        assert!(css.contains(".layer-filmstrip"));
        assert!(css.contains("flex-direction: column;"));
        assert!(css.contains("overflow-y: auto;"));

        // Cards are horizontal rows
        assert!(css.contains(".layer-card"));
        assert!(css.contains("flex-direction: row;"));
        assert!(css.contains(".layer-card-info"));

        // Viewport HUD moved to bottom right to avoid left-side dock collision
        assert!(css.contains(".viewport-hud"));
        assert!(css.contains("right: 1rem;"));

        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");
        assert!(js.contains("layer-card-info"));
    }

    #[test]
    fn test_embedded_assets_contain_fullscreen_stage_overlay() {
        let index_file = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_file.data.as_ref()).expect("index.html must be valid utf8");

        // Fullscreen overlay container & canvas
        assert!(html.contains("id=\"library-fullscreen-overlay\""));
        assert!(html.contains("fullscreen-overlay hidden"));
        assert!(html.contains("id=\"library-fullscreen-canvas\""));

        // Floating HUD overlay bar elements
        assert!(html.contains("id=\"library-fullscreen-hud\""));
        assert!(html.contains("id=\"fs-map-id-badge\""));
        assert!(html.contains("id=\"fs-map-dimensions\""));
        assert!(html.contains("id=\"fs-map-counter\""));
        assert!(html.contains("id=\"btn-fs-grid-toggle\""));
        assert!(html.contains("id=\"btn-fs-grid-adjust\""));
        assert!(html.contains("id=\"btn-fs-fit\""));
        assert!(html.contains("id=\"btn-fs-zoom-out\""));
        assert!(html.contains("id=\"btn-fs-zoom-in\""));
        assert!(html.contains("id=\"fs-zoom-readout\""));
        assert!(html.contains("id=\"btn-fs-onion-skin\""));
        assert!(html.contains("id=\"btn-fs-prev-map\""));
        assert!(html.contains("id=\"btn-fs-next-map\""));
        assert!(html.contains("id=\"btn-fs-close\""));

        // Bottom layer deck filmstrip
        assert!(html.contains("id=\"library-fullscreen-layer-deck\""));
        assert!(html.contains("id=\"fs-layer-filmstrip-strip\""));

        // CSS styles
        let css_file = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(css_file.data.as_ref()).expect("style.css must be valid utf8");

        assert!(css.contains(".fullscreen-overlay"));
        assert!(css.contains(".fullscreen-hud-bar"));
        assert!(css.contains(".fullscreen-layer-deck"));
        assert!(css.contains(".fs-layer-card"));
        assert!(css.contains(".fs-role-chip"));
        assert!(css.contains(".btn-card-fullscreen"));

        // JavaScript engine and hotkeys
        let js_file = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(js_file.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("libraryFullscreenActive"));
        assert!(js.contains("libraryFullscreenMap"));
        assert!(js.contains("libraryFullscreenLayerIndex"));
        assert!(js.contains("renderFullscreenCanvas"));
        assert!(js.contains("drawFullscreenVttGrid"));
        assert!(js.contains("fullscreenFitToScreen"));
        assert!(js.contains("fullscreenZoomStep"));
        assert!(js.contains("activeFsPanKeys"));
        assert!(js.contains("fsPanLoop"));
        assert!(js.contains("loadFullscreenMap"));
        assert!(js.contains("renderFullscreenLayerDeck"));
        assert!(js.contains("switchFullscreenLayer"));
        assert!(js.contains("openLibraryFullscreen"));
        assert!(js.contains("closeLibraryFullscreen"));
        assert!(js.contains("btn-card-fullscreen"));
    }

    #[test]
    fn test_embedded_assets_contain_slide_over_inspector_drawer_and_ai_refinement() {
        let index_file = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_file.data.as_ref()).expect("index.html must be valid utf8");

        // Inspector Drawer Container
        assert!(html.contains("id=\"library-inspector-drawer\""));
        assert!(html.contains("library-drawer hidden"));

        // Header elements
        assert!(html.contains("Map Inspector &amp; Refinement") || html.contains("Map Inspector & Refinement"));
        assert!(html.contains("id=\"inspector-map-id-badge\""));
        assert!(html.contains("id=\"btn-close-inspector\""));
        assert!(html.contains("id=\"inspector-unsaved-badge\""));

        // Layer Bundle Visualizer elements
        assert!(html.contains("Layer Bundle Visualizer"));
        assert!(html.contains("id=\"inspector-layer-count-badge\""));
        assert!(html.contains("id=\"inspector-layers-list\""));

        // Themes & Tags Editor elements
        assert!(html.contains("Themes &amp; Tags Editor") || html.contains("Themes & Tags Editor"));
        assert!(html.contains("id=\"inspector-theme-chips\""));
        assert!(html.contains("id=\"inspector-theme-input\""));
        assert!(html.contains("id=\"btn-add-theme\""));
        assert!(html.contains("id=\"inspector-tag-chips\""));
        assert!(html.contains("id=\"inspector-tag-input\""));
        assert!(html.contains("id=\"btn-add-tag\""));
        assert!(html.contains("id=\"canonical-themes-datalist\""));

        // Description Editor elements
        assert!(html.contains("DungeonForge Physical Description"));
        assert!(html.contains("id=\"inspector-desc-counter\""));
        assert!(html.contains("id=\"inspector-description-input\""));

        // AI Refinements (Issue #136)
        assert!(html.contains("id=\"btn-inspector-suggest-themes\""));
        assert!(html.contains("Suggest Themes &amp; Tags") || html.contains("Suggest Themes & Tags"));
        assert!(html.contains("id=\"btn-inspector-describe\""));
        assert!(html.contains("Generate Description"));
        assert!(html.contains("id=\"btn-inspector-run-all\""));
        assert!(html.contains("Run All AI Refinements"));
        assert!(html.contains("id=\"inspector-ai-suggestions-box\""));
        assert!(html.contains("id=\"inspector-ai-suggestions-pills\""));

        // Footer Actions
        assert!(html.contains("id=\"btn-inspector-fullscreen\""));
        assert!(html.contains("id=\"btn-inspector-discard\""));
        assert!(html.contains("id=\"btn-inspector-save\""));

        // CSS styles
        let css_file = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(css_file.data.as_ref()).expect("style.css must be valid utf8");

        assert!(css.contains(".library-drawer"));
        assert!(css.contains(".library-drawer-header"));
        assert!(css.contains(".library-drawer-body"));
        assert!(css.contains(".inspector-layer-card"));
        assert!(css.contains(".inspector-role-badge"));
        assert!(css.contains(".inspector-elev-badge"));
        assert!(css.contains(".chip-theme"));
        assert!(css.contains(".chip-tag"));
        assert!(css.contains(".inspector-textarea"));
        assert!(css.contains(".desc-counter"));
        assert!(css.contains(".btn-ai-action"));
        assert!(css.contains(".btn-ai-combo"));
        assert!(css.contains(".ai-suggestion-pill"));
        assert!(css.contains(".btn-card-edit"));
        assert!(css.contains(".btn-save-changes"));

        // JavaScript functions & state
        let js_file = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(js_file.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("libraryDrawerActive"));
        assert!(js.contains("libraryDrawerMap"));
        assert!(js.contains("libraryDraft"));
        assert!(js.contains("libraryDraftDirty"));
        assert!(js.contains("openLibraryInspector"));
        assert!(js.contains("closeLibraryInspector"));
        assert!(js.contains("toggleLibraryInspector"));
        assert!(js.contains("renderInspectorLayers"));
        assert!(js.contains("renderInspectorThemeChips"));
        assert!(js.contains("renderInspectorTagChips"));
        assert!(js.contains("updateInspectorDescriptionCounter"));
        assert!(js.contains("saveLibraryInspectorChanges"));
        assert!(js.contains("discardLibraryInspectorChanges"));
        assert!(js.contains("suggestInspectorThemes"));
        assert!(js.contains("describeInspectorMap"));
        assert!(js.contains("runAllInspectorAiRefinements"));
        assert!(js.contains("setupInspectorDrawerControls"));
        assert!(js.contains("btn-card-edit"));
    }

    #[test]
    fn test_embedded_assets_contain_library_filter_empty_state() {
        let index_html = Assets::get("index.html").expect("index.html must be embedded");
        let html = std::str::from_utf8(index_html.data.as_ref()).expect("index.html must be valid utf8");

        assert!(html.contains("id=\"library-filter-empty\""));
        assert!(html.contains("id=\"btn-clear-library-filters\""));
        assert!(html.contains("id=\"library-empty\""));
        assert!(!html.contains("No battlemaps matching the selected filter have been approved yet."));

        let app_js = Assets::get("app.js").expect("app.js must be embedded");
        let js = std::str::from_utf8(app_js.data.as_ref()).expect("app.js must be valid utf8");

        assert!(js.contains("libraryFilterEmpty"));
        assert!(js.contains("btnClearLibraryFilters"));
        assert!(js.contains("clearLibraryFilters"));
        assert!(js.contains("hasAnyCuratedMaps"));

        let style_css = Assets::get("style.css").expect("style.css must be embedded");
        let css = std::str::from_utf8(style_css.data.as_ref()).expect("style.css must be valid utf8");

        assert!(css.contains(".library-filter-empty"));
        assert!(css.contains(".library-scroll-container {\n    flex: 1;\n    overflow-y: auto;\n    padding: 1.25rem;\n    position: relative;\n}"));
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
    async fn test_static_handler_training_prototype() {
        let uri_js = "/training-prototype.js".parse::<Uri>().unwrap();
        let resp_js = static_handler(uri_js).await.into_response();
        assert_eq!(resp_js.status(), StatusCode::OK);

        let uri_css = "/training-prototype.css".parse::<Uri>().unwrap();
        let resp_css = static_handler(uri_css).await.into_response();
        assert_eq!(resp_css.status(), StatusCode::OK);
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
