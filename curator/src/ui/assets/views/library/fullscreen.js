/**
 * Fullscreen stage overlay: the shared Stage Viewport mounted on the overlay canvas,
 * plus the layer deck and per-map Grid Configuration persistence.
 *
 * Pan, zoom, fit, the grid overlay and the overlay flags all live in the viewport, so
 * this module only loads maps/layers, binds the controls, and writes the edited Grid
 * Configuration back to the curated map that owns the burst.
 */

import { dom } from "../../dom.js";
import { state } from "../../store.js";
import { escapeHtml } from "../../kit/dom.js";
import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    calculateLatticeInfo,
    formatLatticePanel,
    formatLatticeStatus,
} from "../../kit/grid-math.js";
import { getLibraryFullscreenDimensions } from "../../kit/layers.js";
import { createViewport, drawGridOverlay, isGridCalibrating } from "../../stage/viewport.js";
import { setLibraryFocus } from "../library.js";
import { invalidateDatasetReadiness } from "../readiness.js";
import { saveCuratedMap } from "../../requests/library.js";

const WHEEL_ZOOM_FACTOR = 1.15;
const BUTTON_ZOOM_FACTOR = 1.2;

// ==========================================================================
// Fullscreen Stage Overlay mounted on the Stage Viewport (Issues #134, #265)
// ==========================================================================

let viewport = null;

/**
 * Pointer capture for the overlay: drag-to-align and drag-to-pan start at different
 * points and are tracked here, not in the shared store.
 */
const pointer = {
    panning: false,
    panStart: { x: 0, y: 0 },
    draggingGrid: false,
    gridStart: { x: 0, y: 0 },
    gridOffsetStart: { x: 0, y: 0 },
};

/**
 * The fullscreen Stage Viewport; null until `setupFullscreenControls()` or
 * `openLibraryFullscreen()` has mounted it.
 */
export function getFullscreenViewport() {
    return viewport;
}

/**
 * Content bounds `fit()` measures: the layer image the frame draws at its own
 * `naturalWidth`/`naturalHeight`. Curated layers can differ in pixel bounds from the
 * ground layer, so J/K landing on an upper floor must fit that floor, not the ground.
 * The ground resolver only covers frames drawn before the image has loaded.
 */
function getFullscreenContentSize() {
    const img = state.libraryFullscreenCurrentImage;
    if (img && img.naturalWidth > 0 && img.naturalHeight > 0) {
        return { width: img.naturalWidth, height: img.naturalHeight };
    }
    return getLibraryFullscreenDimensions();
}

function ensureFullscreenViewport() {
    if (viewport || !dom.libraryFullscreenCanvas) return viewport;
    viewport = createViewport(dom.libraryFullscreenCanvas, {
        draw: drawFullscreenFrame,
        contentSize: getFullscreenContentSize,
        gridOwner: () => (state.libraryFullscreenMap ? state.libraryFullscreenMap.id : null),
        persistGrid: persistFullscreenGrid,
    });
    viewport.subscribe(updateFullscreenHud);
    return viewport;
}

/**
 * High-performance 60fps Canvas 2D render callback for the Fullscreen Stage Overlay.
 * Applies devicePixelRatio scaling, the pan/zoom transform, crisp pixel examination
 * past 200%, and the shared single-draw-call VTT grid overlay.
 */
function drawFullscreenFrame(c, view) {
    const { width, height, dpr } = view;

    c.save();
    c.clearRect(0, 0, width * dpr, height * dpr);

    // Fill background
    c.fillStyle = "#070a10";
    c.fillRect(0, 0, width * dpr, height * dpr);

    // Normalize devicePixelRatio coordinates
    c.scale(dpr, dpr);

    const img = state.libraryFullscreenCurrentImage;
    if (img && img.complete && img.naturalWidth > 0) {
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;

        c.save();
        // Apply pan and zoom transformation
        c.translate(view.pan.x, view.pan.y);
        c.scale(view.zoom, view.zoom);

        // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
        c.imageSmoothingEnabled = view.zoom < 2.0;
        c.imageSmoothingQuality = "high";

        // Draw active layer battlemap at origin (0, 0)
        c.globalAlpha = 1.0;
        c.drawImage(img, 0, 0, imgW, imgH);

        const calibrating = isGridCalibrating(view, pointer.draggingGrid);
        // Draw crisp 64px VTT grid overlay in image space
        if (view.gridVisible || calibrating) {
            drawGridOverlay(c, imgW, imgH, view, calibrating);
        }

        // Draw image bounding border
        c.beginPath();
        c.rect(0, 0, imgW, imgH);
        c.lineWidth = Math.max(1.0, 1.5 / view.zoom);
        c.strokeStyle = "rgba(56, 189, 248, 0.75)";
        c.stroke();

        c.restore();
    }

    c.restore();
}

/**
 * HUD refresh, driven by the viewport so zoom, grid display state and the lattice
 * badge always come from the frame that was just drawn.
 */
function updateFullscreenHud(view = viewport ? viewport.getView() : null) {
    if (!view || !state.libraryFullscreenActive || !state.libraryFullscreenMap) return;
    const map = state.libraryFullscreenMap;

    if (dom.fsMapIdBadge) {
        dom.fsMapIdBadge.textContent = map.id;
        dom.fsMapIdBadge.title = `Curated Map ID: ${map.id}`;
    }

    if (dom.fsMapDimensions) {
        let dimText = "";
        if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
            dimText = `${state.libraryFullscreenCurrentImage.naturalWidth} × ${state.libraryFullscreenCurrentImage.naturalHeight}`;
        } else if (map.dimensions && map.dimensions.width && map.dimensions.height) {
            dimText = `${map.dimensions.width} × ${map.dimensions.height}`;
        } else {
            dimText = "2048 × 2048";
        }
        dom.fsMapDimensions.textContent = dimText;
    }

    if (dom.fsMapCounter) {
        const total = state.libraryFilteredMaps ? state.libraryFilteredMaps.length : 1;
        const current = (state.libraryFullscreenIndex >= 0 ? state.libraryFullscreenIndex : 0) + 1;
        dom.fsMapCounter.textContent = `Map ${current} of ${total}`;
    }

    if (dom.fsZoomReadout) {
        const pct = Math.round(view.zoom * 100);
        dom.fsZoomReadout.textContent = `${pct}%`;
    }

    if (dom.btnFsGridToggle) {
        dom.btnFsGridToggle.classList.toggle("active", view.gridVisible);
    }

    if (dom.btnFsGridAdjust) {
        dom.btnFsGridAdjust.classList.toggle("active", view.gridPanelOpen);
    }

    if (dom.btnFsPrevMap) {
        dom.btnFsPrevMap.disabled = state.libraryFullscreenIndex <= 0;
    }
    if (dom.btnFsNextMap) {
        dom.btnFsNextMap.disabled = !state.libraryFilteredMaps || state.libraryFullscreenIndex >= state.libraryFilteredMaps.length - 1;
    }
    if (dom.fsGridStatusBadge) {
        const fsDims = getLibraryFullscreenDimensions();
        const fsLattice = calculateLatticeInfo(
            fsDims.width,
            fsDims.height,
            view.gridSize,
            view.gridOffset.x,
            view.gridOffset.y
        );
        dom.fsGridStatusBadge.textContent = formatLatticeStatus(fsLattice);
        dom.fsGridStatusBadge.classList.toggle("hud-badge-warning", !fsLattice.isEligible);
    }
}

// --- Fullscreen Map & Layer Loading ---
export function loadFullscreenMap(index, layerIndex = 0) {
    if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;
    if (!ensureFullscreenViewport()) return;
    const clampedIndex = Math.max(0, Math.min(index, state.libraryFilteredMaps.length - 1));
    const map = state.libraryFilteredMaps[clampedIndex];
    if (!map) return;

    // Settle any burst armed for the outgoing map before it stops being current.
    viewport.flushGrid();

    state.libraryFullscreenIndex = clampedIndex;
    state.libraryFullscreenMap = map;

    // Restore this curated map's persisted Grid Configuration.
    viewport.setGridSize(map.grid?.cell_size_px ?? VTT_GRID_SIZE);
    viewport.setGridOffset({
        x: map.grid?.offset_x ?? 0,
        y: map.grid?.offset_y ?? 0,
    });
    updateFullscreenGridPanel();

    const layers = map.layers && map.layers.length > 0 ? map.layers : [{ index: 0, role: "ground" }];
    const validLayerIndex = Math.max(0, Math.min(layerIndex, layers.length - 1));
    state.libraryFullscreenLayerIndex = validLayerIndex;

    const layerObj = layers[validLayerIndex];
    const actualLayerIdx = (layerObj && layerObj.index !== undefined) ? layerObj.index : validLayerIndex;
    const cacheKey = `${map.id}_layer_${actualLayerIdx}`;

    const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${actualLayerIdx}`;

    let img = state.libraryFullscreenImageCache.get(cacheKey);
    if (img && img.complete && img.naturalWidth > 0) {
        state.libraryFullscreenCurrentImage = img;
        viewport.fit();
        renderFullscreenLayerDeck();
    } else {
        if (!img) {
            img = new Image();
            img.src = directUrl;
            state.libraryFullscreenImageCache.set(cacheKey, img);
        }
        img.onload = () => {
            if (state.libraryFullscreenMap && state.libraryFullscreenMap.id === map.id && state.libraryFullscreenLayerIndex === validLayerIndex) {
                state.libraryFullscreenCurrentImage = img;
                viewport.fit();
                renderFullscreenLayerDeck();
            }
        };
        if (img.complete && img.naturalWidth > 0) {
            state.libraryFullscreenCurrentImage = img;
            viewport.fit();
            renderFullscreenLayerDeck();
        }
    }

    // Prefetch ground layer
    let groundIdx = layers.findIndex(l => (l.role || "").toLowerCase().replace(/[-_]/g, "") === "ground");
    if (groundIdx < 0 && layers.length > 0) groundIdx = 0;
    if (groundIdx >= 0 && groundIdx !== validLayerIndex) {
        const gLayer = layers[groundIdx];
        const gActualIdx = (gLayer && gLayer.index !== undefined) ? gLayer.index : groundIdx;
        const gKey = `${map.id}_layer_${gActualIdx}`;
        if (!state.libraryFullscreenImageCache.get(gKey)) {
            const gImg = new Image();
            gImg.src = `/api/image/curated/${encodeURIComponent(map.id)}/${gActualIdx}`;
            state.libraryFullscreenImageCache.set(gKey, gImg);
        }
    }

    // Prefetch other layers of this map
    layers.forEach((l, idx) => {
        const lIdx = l.index !== undefined ? l.index : idx;
        const k = `${map.id}_layer_${lIdx}`;
        if (!state.libraryFullscreenImageCache.get(k)) {
            const pImg = new Image();
            pImg.src = `/api/image/curated/${encodeURIComponent(map.id)}/${lIdx}`;
            state.libraryFullscreenImageCache.set(k, pImg);
        }
    });

    // Prefetch adjacent maps
    [-1, 1, 2].forEach(offset => {
        const targetIdx = clampedIndex + offset;
        if (targetIdx >= 0 && targetIdx < state.libraryFilteredMaps.length) {
            const adjMap = state.libraryFilteredMaps[targetIdx];
            const adjKey = `${adjMap.id}_layer_0`;
            if (!state.libraryFullscreenImageCache.get(adjKey)) {
                const adjImg = new Image();
                adjImg.src = `/api/image/curated/${encodeURIComponent(adjMap.id)}/0`;
                state.libraryFullscreenImageCache.set(adjKey, adjImg);
            }
        }
    });

    renderFullscreenLayerDeck();
}

function renderFullscreenLayerDeck() {
    if (!dom.libraryFullscreenLayerDeck || !dom.fsLayerFilmstripStrip) return;
    const map = state.libraryFullscreenMap;

    if (!map || !map.layers || map.layers.length <= 1) {
        dom.libraryFullscreenLayerDeck.classList.add("hidden");
        return;
    }

    dom.libraryFullscreenLayerDeck.classList.remove("hidden");
    const total = map.layers.length;
    if (dom.fsLayerDeckCount) {
        dom.fsLayerDeckCount.textContent = `(${state.libraryFullscreenLayerIndex + 1} / ${total})`;
    }

    dom.fsLayerFilmstripStrip.innerHTML = "";
    const fragment = document.createDocumentFragment();

    map.layers.forEach((layer, idx) => {
        const card = document.createElement("div");
        const isActive = idx === state.libraryFullscreenLayerIndex;
        card.className = `fs-layer-card${isActive ? " active" : ""}`;
        card.dataset.index = idx;

        const layerIdx = layer.index !== undefined ? layer.index : idx;
        const thumbUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

        const role = (layer.role || "ground").toLowerCase().replace(/[-_]/g, "");
        let roleClass = "fs-role-ground";
        let roleName = "Ground";
        if (role === "upperfloor" || role === "upper") {
            roleClass = "fs-role-upper";
            roleName = "Upper";
        } else if (role === "basement" || role === "lower" || role === "cellar") {
            roleClass = "fs-role-basement";
            roleName = "Basement";
        } else if (role === "roof") {
            roleClass = "fs-role-roof";
            roleName = "Roof";
        } else if (role === "variant") {
            roleClass = "fs-role-variant";
            roleName = "Variant";
        }

        const elevText = typeof layer.elevation === "number"
            ? (layer.elevation > 0 ? `+${layer.elevation}` : `${layer.elevation}`)
            : (role === "ground" ? "0" : (role === "upperfloor" || role === "upper" ? "+1" : (role === "basement" ? "-1" : "0")));

        const hotkeyKbd = idx < 6 ? `<div class="fs-layer-card-key"><kbd>${idx + 1}</kbd></div>` : "";

        card.innerHTML = `
            <div class="fs-layer-card-thumb-wrap">
                ${hotkeyKbd}
                <img class="fs-layer-card-img" src="${thumbUrl}" alt="Layer ${idx + 1}" loading="lazy">
            </div>
            <div class="fs-layer-card-meta">
                <span class="fs-role-chip ${roleClass}">${escapeHtml(roleName)}</span>
                <span class="fs-elevation-badge mono">${escapeHtml(elevText)}</span>
            </div>
        `;

        card.addEventListener("click", () => {
            switchFullscreenLayer(idx);
        });

        fragment.appendChild(card);
    });

    dom.fsLayerFilmstripStrip.appendChild(fragment);
}

export function switchFullscreenLayer(layerIndex) {
    if (!state.libraryFullscreenMap) return;
    const layers = state.libraryFullscreenMap.layers || [];
    if (layerIndex < 0 || layerIndex >= layers.length) return;

    state.libraryFullscreenLayerIndex = layerIndex;
    loadFullscreenMap(state.libraryFullscreenIndex, layerIndex);
}

// --- Fullscreen Grid Configuration Controls ---
export function toggleFullscreenGridPanel(force) {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    const next = typeof force === "boolean" ? force : !vp.getView().gridPanelOpen;
    vp.setGridPanelOpen(next);
    if (dom.libraryFullscreenGridPanel) {
        dom.libraryFullscreenGridPanel.classList.toggle("hidden", !next);
    }
    if (dom.btnFsGridAdjust) {
        dom.btnFsGridAdjust.classList.toggle("active", next);
    }
    if (next) {
        updateFullscreenGridPanel();
    }
}

function toggleFullscreenAlignMode(force) {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    const next = typeof force === "boolean" ? force : !vp.getView().gridAlignMode;
    vp.setGridAlignMode(next);
    if (dom.btnFsGridAlignDrag) {
        dom.btnFsGridAlignDrag.classList.toggle("active", next);
    }
    if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
        dom.libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", next);
    }
}

function toggleFullscreenGridVisible() {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    vp.setGridVisible(!vp.getView().gridVisible);
}

function updateFullscreenGridPanel() {
    if (!viewport || !dom.fsGridSizeInput || !dom.fsGridSizeSlider) return;
    const view = viewport.getView();
    const size = view.gridSize || VTT_GRID_SIZE;
    dom.fsGridSizeInput.value = size;
    dom.fsGridSizeSlider.value = Math.min(256, Math.max(MIN_GRID_SIZE, size));

    if (dom.fsGridOffsetXVal) dom.fsGridOffsetXVal.textContent = Math.round(view.gridOffset.x);
    if (dom.fsGridOffsetYVal) dom.fsGridOffsetYVal.textContent = Math.round(view.gridOffset.y);

    const chips = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
    chips.forEach(chip => {
        const chipSize = parseInt(chip.dataset.size, 10);
        chip.classList.toggle("active", chipSize === size);
    });

    const fsDims = getLibraryFullscreenDimensions();
    const fsLattice = calculateLatticeInfo(
        fsDims.width,
        fsDims.height,
        view.gridSize,
        view.gridOffset.x,
        view.gridOffset.y
    );
    const readout = formatLatticePanel(fsLattice);
    if (dom.fsGridLatticeCells) {
        dom.fsGridLatticeCells.textContent = readout.cells;
    }
    if (dom.fsGridLatticeSections) {
        dom.fsGridLatticeSections.textContent = readout.sections;
        dom.fsGridLatticeSections.className = fsLattice.isEligible
            ? "grid-lattice-sections-val mono badge-sections badge-sections-valid"
            : "grid-lattice-sections-val mono badge-sections badge-sections-warning";
    }
}

function applyFullscreenGridSize(size) {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    vp.setGridSize(size);
    updateFullscreenGridPanel();
}

function nudgeFullscreenGrid(dx, dy) {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    vp.nudgeGrid(dx, dy);
    updateFullscreenGridPanel();
}

function resetFullscreenGridToDefault() {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    vp.resetGrid();
    updateFullscreenGridPanel();
}

/**
 * PATCHes the Grid Configuration of the curated map that owned the settled edit burst.
 *
 * The viewport debounces this to one call per burst and passes the map id it captured
 * when the burst was armed, so a J/K switch mid-burst cannot retarget the write. A call
 * that would write the map's current (or default) configuration is skipped, so restoring
 * a map's grid on load never issues a request of its own.
 */
function persistFullscreenGrid(mapId, view) {
    if (!mapId) return;
    const map = (state.libraryFilteredMaps || []).find((m) => m.id === mapId)
        || (state.libraryFullscreenMap && state.libraryFullscreenMap.id === mapId ? state.libraryFullscreenMap : null);
    if (!map) return;

    const payload = {
        grid: {
            cell_size_px: view.gridSize,
            offset_x: view.gridOffset.x,
            offset_y: view.gridOffset.y,
        },
    };
    const stored = map.grid;
    const unchanged = stored
        ? stored.cell_size_px === payload.grid.cell_size_px
            && stored.offset_x === payload.grid.offset_x
            && stored.offset_y === payload.grid.offset_y
        : payload.grid.cell_size_px === VTT_GRID_SIZE
            && payload.grid.offset_x === 0
            && payload.grid.offset_y === 0;
    map.grid = { ...payload.grid };
    if (unchanged) return;

    saveCuratedMap(mapId, payload).then(() => {
        // The regrid recomputes Map Section descriptions server-side, so the cached
        // audit this map was flagged in no longer describes the bundle.
        invalidateDatasetReadiness();
    }).catch(() => {
        // The request module reported it; the map keeps the local configuration so the next
        // burst retries it.
    });
}

// --- Fullscreen Pointer Interaction ---

/** Starts a drag-to-align (Shift or align mode) or drag-to-pan from a canvas press. */
function beginFullscreenPointer(button, shiftKey, clientX, clientY) {
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    const view = vp.getView();
    if (button === 0 && (shiftKey || view.gridAlignMode)) {
        pointer.draggingGrid = true;
        pointer.gridStart = { x: clientX, y: clientY };
        pointer.gridOffsetStart = { x: view.gridOffset.x, y: view.gridOffset.y };
        if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
            dom.libraryFullscreenCanvas.parentElement.classList.add("grid-dragging");
        }
        return;
    }
    if (button === 0 || button === 1) {
        pointer.panning = true;
        pointer.panStart = { x: clientX - view.pan.x, y: clientY - view.pan.y };
        if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
            dom.libraryFullscreenCanvas.parentElement.classList.add("panning");
        }
    }
}

/**
 * Continues the active pointer interaction. Returns true when the move belonged to the
 * fullscreen stage, so the triage handlers can be skipped.
 */
export function moveFullscreenPointer(clientX, clientY) {
    if (!viewport) return false;
    if (pointer.draggingGrid) {
        const view = viewport.getView();
        const zoom = view.zoom || 1;
        viewport.setGridOffset({
            x: Math.round(pointer.gridOffsetStart.x + (clientX - pointer.gridStart.x) / zoom),
            y: Math.round(pointer.gridOffsetStart.y + (clientY - pointer.gridStart.y) / zoom),
        });
        updateFullscreenGridPanel();
        return true;
    }
    if (pointer.panning) {
        const view = viewport.getView();
        viewport.panBy(
            clientX - pointer.panStart.x - view.pan.x,
            clientY - pointer.panStart.y - view.pan.y
        );
        return true;
    }
    return false;
}

/** Ends whichever pointer interaction is active; a finished grid drag settles its burst. */
export function endFullscreenPointer() {
    let wasActive = false;
    if (pointer.draggingGrid) {
        pointer.draggingGrid = false;
        if (viewport) viewport.flushGrid();
        if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
            dom.libraryFullscreenCanvas.parentElement.classList.remove("grid-dragging");
        }
        wasActive = true;
    }
    if (pointer.panning) {
        pointer.panning = false;
        if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
            dom.libraryFullscreenCanvas.parentElement.classList.remove("panning");
        }
        wasActive = true;
    }
    return wasActive;
}

/** Mirrors Shift/align-mode into the overlay's `grid-aligning` class while not dragging. */
export function syncFullscreenAligningClass(shiftKey) {
    if (!viewport || pointer.draggingGrid) return;
    if (!dom.libraryFullscreenCanvas || !dom.libraryFullscreenCanvas.parentElement) return;
    dom.libraryFullscreenCanvas.parentElement.classList.toggle(
        "grid-aligning",
        Boolean(shiftKey) || viewport.getView().gridAlignMode
    );
}

/** Stops the fullscreen pan and zoom loops, e.g. while a modal takes the keyboard. */
export function stopFullscreenMotion() {
    if (!viewport) return;
    for (const key of ["w", "a", "s", "d"]) viewport.releasePanKey(key);
    viewport.setZoomKey("in", false);
    viewport.setZoomKey("out", false);
}

// --- Fullscreen Open & Close Lifecycle ---
export function openLibraryFullscreen(index) {
    if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;
    const clampedIndex = Math.max(0, Math.min(index, state.libraryFilteredMaps.length - 1));

    state.libraryFullscreenActive = true;
    if (dom.libraryFullscreenOverlay) {
        dom.libraryFullscreenOverlay.classList.remove("hidden");
    }

    setLibraryFocus(clampedIndex, false);
    const vp = ensureFullscreenViewport();
    if (!vp) return;
    vp.resize();
    vp.resume();
    loadFullscreenMap(clampedIndex, 0);

    if (dom.libraryFullscreenCanvas) {
        dom.libraryFullscreenCanvas.focus();
    }
}

export function closeLibraryFullscreen() {
    if (!state.libraryFullscreenActive) return;

    endFullscreenPointer();
    stopFullscreenMotion();
    if (viewport) {
        // No edit survives the close: settle the pending burst, then stop rendering.
        viewport.flushGrid();
        viewport.suspend();
    }

    state.libraryFullscreenActive = false;
    if (dom.libraryFullscreenOverlay) {
        dom.libraryFullscreenOverlay.classList.add("hidden");
    }

    toggleFullscreenGridPanel(false);
    toggleFullscreenAlignMode(false);

    // Restore focus on card in library view and scroll into view smoothly
    if (state.libraryFullscreenIndex >= 0) {
        setLibraryFocus(state.libraryFullscreenIndex, true);
    }
}

export function setupFullscreenControls() {
    if (!dom.libraryFullscreenCanvas) return;
    ensureFullscreenViewport();

    window.addEventListener("resize", () => {
        if (!state.libraryFullscreenActive || !viewport) return;
        viewport.resize();
    });

    // Canvas Mouse Pan Dragging & Dynamic Grid Align Drag
    dom.libraryFullscreenCanvas.addEventListener("mousedown", (e) => {
        if (e.button !== 0 && e.button !== 1) return;
        beginFullscreenPointer(e.button, e.shiftKey, e.clientX, e.clientY);
        e.preventDefault();
    });

    // Wheel Zoom
    dom.libraryFullscreenCanvas.addEventListener("wheel", (e) => {
        e.preventDefault();
        if (!viewport || !state.libraryFullscreenCurrentImage) return;

        const rect = dom.libraryFullscreenCanvas.getBoundingClientRect();
        const point = {
            x: e.clientX - rect.left,
            y: e.clientY - rect.top,
        };

        const factor = e.deltaY < 0 ? WHEEL_ZOOM_FACTOR : (1 / WHEEL_ZOOM_FACTOR);
        viewport.zoomBy(factor, point);
    }, { passive: false });

    // HUD Buttons
    dom.btnFsGridToggle?.addEventListener("click", () => toggleFullscreenGridVisible());

    dom.btnFsGridAdjust?.addEventListener("click", () => toggleFullscreenGridPanel());
    dom.btnFsFit?.addEventListener("click", () => viewport?.fit());
    dom.btnFsZoomOut?.addEventListener("click", () => viewport?.zoomBy(1 / BUTTON_ZOOM_FACTOR));
    dom.btnFsZoomIn?.addEventListener("click", () => viewport?.zoomBy(BUTTON_ZOOM_FACTOR));

    dom.btnFsPrevMap?.addEventListener("click", () => {
        if (state.libraryFullscreenIndex > 0) {
            loadFullscreenMap(state.libraryFullscreenIndex - 1, 0);
            setLibraryFocus(state.libraryFullscreenIndex, true);
        }
    });

    dom.btnFsNextMap?.addEventListener("click", () => {
        if (state.libraryFilteredMaps && state.libraryFullscreenIndex < state.libraryFilteredMaps.length - 1) {
            loadFullscreenMap(state.libraryFullscreenIndex + 1, 0);
            setLibraryFocus(state.libraryFullscreenIndex, true);
        }
    });

    dom.btnFsClose?.addEventListener("click", () => closeLibraryFullscreen());

    // Fullscreen Grid Panel Controls
    dom.btnFsCloseGridPanel?.addEventListener("click", () => toggleFullscreenGridPanel(false));
    dom.btnFsGridDone?.addEventListener("click", () => toggleFullscreenGridPanel(false));
    dom.btnFsGridReset?.addEventListener("click", () => resetFullscreenGridToDefault());

    dom.btnFsGridSizeDec?.addEventListener("click", () => applyFullscreenGridSize(viewport.getView().gridSize - 1));
    dom.btnFsGridSizeInc?.addEventListener("click", () => applyFullscreenGridSize(viewport.getView().gridSize + 1));

    if (dom.fsGridSizeInput) {
        dom.fsGridSizeInput.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) applyFullscreenGridSize(val);
        });
        dom.fsGridSizeInput.addEventListener("change", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) applyFullscreenGridSize(val);
        });
    }

    if (dom.fsGridSizeSlider) {
        dom.fsGridSizeSlider.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) applyFullscreenGridSize(val);
        });
    }

    const fsPresets = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
    fsPresets.forEach(chip => {
        chip.addEventListener("click", () => {
            const size = parseInt(chip.dataset.size, 10);
            if (!isNaN(size)) applyFullscreenGridSize(size);
        });
    });

    dom.btnFsGridNudgeUp?.addEventListener("click", () => nudgeFullscreenGrid(0, -1));
    dom.btnFsGridNudgeDown?.addEventListener("click", () => nudgeFullscreenGrid(0, 1));
    dom.btnFsGridNudgeLeft?.addEventListener("click", () => nudgeFullscreenGrid(-1, 0));
    dom.btnFsGridNudgeRight?.addEventListener("click", () => nudgeFullscreenGrid(1, 0));

    dom.btnFsGridAlignDrag?.addEventListener("click", () => toggleFullscreenAlignMode());
}
