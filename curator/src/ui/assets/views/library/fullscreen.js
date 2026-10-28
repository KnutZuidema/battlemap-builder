/**
 * Fullscreen stage overlay with its own canvas render loop, pan/zoom loops, layer deck
 * and per-map grid calibration. Kept separate from the triage stage until the Stage
 * Viewport extraction.
 */

import { dom } from "../../dom.js";
import { state } from "../../store.js";
import { escapeHtml } from "../../kit/dom.js";
import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    MAX_GRID_SIZE,
    MIN_ZOOM,
    MAX_ZOOM,
    calculateLatticeInfo,
} from "../../kit/grid-math.js";
import {
    getLibraryFullscreenDimensions,
    PAN_SPEED,
    ZOOM_SPEED,
} from "../../stage/canvas.js";
import { setLibraryFocus } from "../library.js";
import { showToast, invalidateDatasetReadiness } from "../../app.js";

// ==========================================================================
// Fullscreen Stage Overlay with Reused 2D Canvas & HUD Controls (Issue #134)
// ==========================================================================

export function resizeFullscreenCanvas() {
    if (!dom.libraryFullscreenCanvas || !dom.libraryFullscreenCanvas.parentElement) return;
    const container = dom.libraryFullscreenCanvas.parentElement;
    const width = container.clientWidth;
    const height = container.clientHeight;
    const dpr = window.devicePixelRatio || 1;

    if (dom.libraryFullscreenCanvas.width !== width * dpr || dom.libraryFullscreenCanvas.height !== height * dpr) {
        dom.libraryFullscreenCanvas.width = width * dpr;
        dom.libraryFullscreenCanvas.height = height * dpr;
        dom.libraryFullscreenCanvas.style.width = `${width}px`;
        dom.libraryFullscreenCanvas.style.height = `${height}px`;
    }
}

let fsRenderPending = false;
export function requestFullscreenRender() {
    if (!fsRenderPending) {
        fsRenderPending = true;
        requestAnimationFrame(() => {
            fsRenderPending = false;
            renderFullscreenCanvas();
        });
    }
}

/**
 * High-performance 60fps Canvas 2D render loop for the Fullscreen Stage Overlay.
 * Applies devicePixelRatio scaling, pan/zoom transformation matrix,
 * renders crisp pixel examination for zoom > 200%,
 * and single-draw-call 64px VTT grid overlay.
 */
function renderFullscreenCanvas() {
    if (!dom.libraryFullscreenCtx || !dom.libraryFullscreenCanvas) return;
    const c = dom.libraryFullscreenCtx;
    const dpr = window.devicePixelRatio || 1;

    c.save();
    c.clearRect(0, 0, dom.libraryFullscreenCanvas.width, dom.libraryFullscreenCanvas.height);

    // Fill background
    c.fillStyle = "#070a10";
    c.fillRect(0, 0, dom.libraryFullscreenCanvas.width, dom.libraryFullscreenCanvas.height);

    // Normalize devicePixelRatio coordinates
    c.scale(dpr, dpr);

    if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.complete && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
        const img = state.libraryFullscreenCurrentImage;
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;

        c.save();
        // Apply pan and zoom transformation
        c.translate(state.libraryFullscreenPan.x, state.libraryFullscreenPan.y);
        c.scale(state.libraryFullscreenZoom, state.libraryFullscreenZoom);

        // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
        c.imageSmoothingEnabled = state.libraryFullscreenZoom < 2.0;
        c.imageSmoothingQuality = "high";

        // Draw active layer battlemap at origin (0, 0)
        c.globalAlpha = 1.0;
        c.drawImage(img, 0, 0, imgW, imgH);

        const isFsCalibrationActive = Boolean(
            state.libraryFullscreenGridPanelOpen ||
            state.libraryFullscreenGridAlignMode ||
            state.libraryFullscreenIsDraggingGrid
        );
        // Draw crisp 64px VTT grid overlay in image space
        if (state.libraryFullscreenGridVisible || isFsCalibrationActive) {
            drawFullscreenVttGrid(c, imgW, imgH, isFsCalibrationActive);
        }

        // Draw image bounding border
        c.beginPath();
        c.rect(0, 0, imgW, imgH);
        c.lineWidth = Math.max(1.0, 1.5 / state.libraryFullscreenZoom);
        c.strokeStyle = "rgba(56, 189, 248, 0.75)";
        c.stroke();

        c.restore();
    }

    c.restore();
}

/**
 * Single-draw-call crisp 64px VTT grid overlay for Fullscreen Stage.
 */
function drawFullscreenVttGrid(c, width, height, isCalibrationActive = false) {
    c.save();
    c.beginPath();
    c.rect(0, 0, width, height);
    c.clip();

    const size = state.libraryFullscreenGridSize || VTT_GRID_SIZE;
    const ox = ((Math.round(state.libraryFullscreenGridOffset.x) % size) + size) % size;
    const oy = ((Math.round(state.libraryFullscreenGridOffset.y) % size) + size) % size;
    const cols = Math.floor(Math.max(0, width - ox) / size);
    const rows = Math.floor(Math.max(0, height - oy) / size);
    const fullW = cols * size;
    const fullH = rows * size;

    // When calibration panel or align mode is active:
    // Shade/dim discarded partial edge strips outside the active full-grid lattice [ox, oy, fullW, fullH]
    if (isCalibrationActive && (cols > 0 || rows > 0)) {
        c.save();
        c.beginPath();
        c.rect(0, 0, width, height);
        c.rect(ox, oy, fullW, fullH);
        c.fillStyle = "rgba(10, 15, 30, 0.65)";
        c.fill("evenodd");
        c.restore();
    }

    c.beginPath();

    // Vertical grid lines
    for (let x = ox; x <= width; x += size) {
        c.moveTo(x, 0);
        c.lineTo(x, height);
    }

    // Horizontal grid lines
    for (let y = oy; y <= height; y += size) {
        c.moveTo(0, y);
        c.lineTo(width, y);
    }

    c.lineWidth = Math.max(0.5, 1.0 / state.libraryFullscreenZoom);
    c.strokeStyle = "rgba(56, 189, 248, 0.4)";
    c.stroke();

    // Draw boundary border around active full grid
    if (isCalibrationActive && cols > 0 && rows > 0) {
        c.save();
        c.beginPath();
        c.rect(ox, oy, fullW, fullH);
        c.lineWidth = Math.max(1.5, 2.5 / state.libraryFullscreenZoom);
        const isEligible = cols >= 16 && rows >= 16;
        c.strokeStyle = isEligible ? "rgba(56, 189, 248, 0.95)" : "rgba(244, 63, 94, 0.95)";
        c.stroke();
        c.restore();
    }

    c.restore();
}

export function fullscreenFitToScreen() {
    if (!state.libraryFullscreenCurrentImage || !dom.libraryFullscreenCanvas) return;
    const img = state.libraryFullscreenCurrentImage;
    const imgW = img.naturalWidth || img.width;
    const imgH = img.naturalHeight || img.height;
    const viewW = dom.libraryFullscreenCanvas.clientWidth;
    const viewH = dom.libraryFullscreenCanvas.clientHeight;

    if (!imgW || !imgH || !viewW || !viewH) return;

    const padding = 40;
    const availW = Math.max(20, viewW - padding * 2);
    const availH = Math.max(20, viewH - padding * 2);

    const fitScale = Math.min(availW / imgW, availH / imgH, 1.0);
    state.libraryFullscreenZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, fitScale));

    state.libraryFullscreenPan.x = Math.round((viewW - imgW * state.libraryFullscreenZoom) / 2);
    state.libraryFullscreenPan.y = Math.round((viewH - imgH * state.libraryFullscreenZoom) / 2);

    updateFullscreenHud();
    requestFullscreenRender();
}

export function fullscreenZoomStep(factor, clientPoint = null) {
    if (!state.libraryFullscreenCurrentImage || !dom.libraryFullscreenCanvas) return;
    const prevZoom = state.libraryFullscreenZoom;
    let newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
    newZoom = Math.round(newZoom * 1000) / 1000;

    if (newZoom === prevZoom) return;

    const viewW = dom.libraryFullscreenCanvas.clientWidth;
    const viewH = dom.libraryFullscreenCanvas.clientHeight;
    const focusX = clientPoint ? clientPoint.x : viewW / 2;
    const focusY = clientPoint ? clientPoint.y : viewH / 2;

    state.libraryFullscreenPan.x = focusX - (focusX - state.libraryFullscreenPan.x) * (newZoom / prevZoom);
    state.libraryFullscreenPan.y = focusY - (focusY - state.libraryFullscreenPan.y) * (newZoom / prevZoom);
    state.libraryFullscreenZoom = newZoom;

    updateFullscreenHud();
    requestFullscreenRender();
}

export function setFullscreenZoom100() {
    if (!state.libraryFullscreenCurrentImage || !dom.libraryFullscreenCanvas) return;
    const prevZoom = state.libraryFullscreenZoom;
    const newZoom = 1.0;
    if (newZoom === prevZoom) return;

    const viewW = dom.libraryFullscreenCanvas.clientWidth;
    const viewH = dom.libraryFullscreenCanvas.clientHeight;
    const focusX = viewW / 2;
    const focusY = viewH / 2;

    state.libraryFullscreenPan.x = focusX - (focusX - state.libraryFullscreenPan.x) * (newZoom / prevZoom);
    state.libraryFullscreenPan.y = focusY - (focusY - state.libraryFullscreenPan.y) * (newZoom / prevZoom);
    state.libraryFullscreenZoom = newZoom;

    updateFullscreenHud();
    requestFullscreenRender();
}

// --- Fullscreen Continuous Smooth WASD Pan Loop ---
export const activeFsPanKeys = { w: false, a: false, s: false, d: false };
let fsPanAnimationId = null;
let lastFsPanTimestamp = 0;

function fsPanLoop(timestamp) {
    if (!lastFsPanTimestamp) lastFsPanTimestamp = timestamp;
    const dt = Math.min((timestamp - lastFsPanTimestamp) / 1000, 0.1);
    lastFsPanTimestamp = timestamp;

    let moveX = 0;
    let moveY = 0;
    if (activeFsPanKeys.w) moveY += 1;
    if (activeFsPanKeys.s) moveY -= 1;
    if (activeFsPanKeys.a) moveX += 1;
    if (activeFsPanKeys.d) moveX -= 1;

    if (moveX !== 0 || moveY !== 0) {
        if (moveX !== 0 && moveY !== 0) {
            const invSqrt2 = 0.70710678;
            moveX *= invSqrt2;
            moveY *= invSqrt2;
        }
        if (state.libraryFullscreenCurrentImage) {
            state.libraryFullscreenPan.x += moveX * PAN_SPEED * dt;
            state.libraryFullscreenPan.y += moveY * PAN_SPEED * dt;
            requestFullscreenRender();
        }
        fsPanAnimationId = requestAnimationFrame(fsPanLoop);
    } else {
        fsPanAnimationId = null;
        lastFsPanTimestamp = 0;
    }
}

export function startFsPanLoop() {
    if (!fsPanAnimationId) {
        lastFsPanTimestamp = performance.now();
        fsPanAnimationId = requestAnimationFrame(fsPanLoop);
    }
}

export function stopFsPanLoop() {
    if (fsPanAnimationId) {
        cancelAnimationFrame(fsPanAnimationId);
        fsPanAnimationId = null;
    }
    lastFsPanTimestamp = 0;
    activeFsPanKeys.w = false;
    activeFsPanKeys.a = false;
    activeFsPanKeys.s = false;
    activeFsPanKeys.d = false;
}

// --- Fullscreen Continuous Smooth Zoom Loop ---
export const activeFsZoomKeys = { in: false, out: false };
let fsZoomAnimationId = null;
let lastFsZoomTimestamp = 0;

function fsZoomLoop(timestamp) {
    if (!lastFsZoomTimestamp) lastFsZoomTimestamp = timestamp;
    const dt = Math.min((timestamp - lastFsZoomTimestamp) / 1000, 0.1);
    lastFsZoomTimestamp = timestamp;

    if (activeFsZoomKeys.in || activeFsZoomKeys.out) {
        let factor = 1.0;
        if (activeFsZoomKeys.in && !activeFsZoomKeys.out) {
            factor = Math.pow(ZOOM_SPEED, dt);
        } else if (activeFsZoomKeys.out && !activeFsZoomKeys.in) {
            factor = Math.pow(1 / ZOOM_SPEED, dt);
        }

        if (factor !== 1.0 && state.libraryFullscreenCurrentImage) {
            fullscreenZoomStep(factor);
        }
        fsZoomAnimationId = requestAnimationFrame(fsZoomLoop);
    } else {
        fsZoomAnimationId = null;
        lastFsZoomTimestamp = 0;
    }
}

export function startFsZoomLoop() {
    if (!fsZoomAnimationId) {
        lastFsZoomTimestamp = performance.now();
        fsZoomAnimationId = requestAnimationFrame(fsZoomLoop);
    }
}

export function stopFsZoomLoop() {
    if (fsZoomAnimationId) {
        cancelAnimationFrame(fsZoomAnimationId);
        fsZoomAnimationId = null;
    }
    lastFsZoomTimestamp = 0;
    activeFsZoomKeys.in = false;
    activeFsZoomKeys.out = false;
}

// --- Fullscreen Map & Layer Loading ---
export function loadFullscreenMap(index, layerIndex = 0) {
    if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;
    const clampedIndex = Math.max(0, Math.min(index, state.libraryFilteredMaps.length - 1));
    const map = state.libraryFilteredMaps[clampedIndex];
    if (!map) return;

    state.libraryFullscreenIndex = clampedIndex;
    state.libraryFullscreenMap = map;

    // Restore or initialize grid settings for this curated map from the persisted grid
    if (map.grid) {
        state.libraryFullscreenGridSize = map.grid.cell_size_px ?? 64;
        state.libraryFullscreenGridOffset = {
            x: map.grid.offset_x ?? 0,
            y: map.grid.offset_y ?? 0,
        };
    } else {
        state.libraryFullscreenGridSize = 64;
        state.libraryFullscreenGridOffset = { x: 0, y: 0 };
    }
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
        fullscreenFitToScreen();
        renderFullscreenLayerDeck();
        updateFullscreenHud();
    } else {
        if (!img) {
            img = new Image();
            img.src = directUrl;
            state.libraryFullscreenImageCache.set(cacheKey, img);
        }
        img.onload = () => {
            if (state.libraryFullscreenMap && state.libraryFullscreenMap.id === map.id && state.libraryFullscreenLayerIndex === validLayerIndex) {
                state.libraryFullscreenCurrentImage = img;
                fullscreenFitToScreen();
                renderFullscreenLayerDeck();
                updateFullscreenHud();
            }
        };
        if (img.complete && img.naturalWidth > 0) {
            state.libraryFullscreenCurrentImage = img;
            fullscreenFitToScreen();
            renderFullscreenLayerDeck();
            updateFullscreenHud();
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

    updateFullscreenHud();
    renderFullscreenLayerDeck();
}

function updateFullscreenHud() {
    if (!state.libraryFullscreenActive || !state.libraryFullscreenMap) return;
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
        const pct = Math.round((state.libraryFullscreenZoom || 1.0) * 100);
        dom.fsZoomReadout.textContent = `${pct}%`;
    }

    if (dom.btnFsGridToggle) {
        dom.btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
    }

    if (dom.btnFsGridAdjust) {
        dom.btnFsGridAdjust.classList.toggle("active", state.libraryFullscreenGridPanelOpen);
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
            state.libraryFullscreenGridSize,
            state.libraryFullscreenGridOffset.x,
            state.libraryFullscreenGridOffset.y
        );
        if (fsLattice.isEligible) {
            dom.fsGridStatusBadge.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells (${fsLattice.kx} × ${fsLattice.ky} sections)`;
            dom.fsGridStatusBadge.classList.remove("hud-badge-warning");
        } else {
            dom.fsGridStatusBadge.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells (< 16×16 cells)`;
            dom.fsGridStatusBadge.classList.add("hud-badge-warning");
        }
    }
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

// --- Fullscreen Grid Calibration Controls ---
export function toggleFullscreenGridPanel(force) {
    state.libraryFullscreenGridPanelOpen = typeof force === "boolean" ? force : !state.libraryFullscreenGridPanelOpen;
    if (dom.libraryFullscreenGridPanel) {
        dom.libraryFullscreenGridPanel.classList.toggle("hidden", !state.libraryFullscreenGridPanelOpen);
    }
    if (dom.btnFsGridAdjust) {
        dom.btnFsGridAdjust.classList.toggle("active", state.libraryFullscreenGridPanelOpen);
    }
    if (state.libraryFullscreenGridPanelOpen) {
        updateFullscreenGridPanel();
    }
    requestFullscreenRender();
}

export function updateFullscreenGridPanel() {
    if (!dom.fsGridSizeInput || !dom.fsGridSizeSlider) return;
    const size = state.libraryFullscreenGridSize || 64;
    dom.fsGridSizeInput.value = size;
    dom.fsGridSizeSlider.value = Math.min(256, Math.max(16, size));

    if (dom.fsGridOffsetXVal) dom.fsGridOffsetXVal.textContent = Math.round(state.libraryFullscreenGridOffset.x);
    if (dom.fsGridOffsetYVal) dom.fsGridOffsetYVal.textContent = Math.round(state.libraryFullscreenGridOffset.y);

    const chips = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
    chips.forEach(chip => {
        const chipSize = parseInt(chip.dataset.size, 10);
        chip.classList.toggle("active", chipSize === size);
    });

    const fsDims = getLibraryFullscreenDimensions();
    const fsLattice = calculateLatticeInfo(
        fsDims.width,
        fsDims.height,
        state.libraryFullscreenGridSize,
        state.libraryFullscreenGridOffset.x,
        state.libraryFullscreenGridOffset.y
    );
    if (dom.fsGridLatticeCells) {
        dom.fsGridLatticeCells.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells`;
    }
    if (dom.fsGridLatticeSections) {
        if (fsLattice.isEligible) {
            dom.fsGridLatticeSections.textContent = `${fsLattice.kx} × ${fsLattice.ky} sections`;
            dom.fsGridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-valid";
        } else {
            dom.fsGridLatticeSections.textContent = "< 16×16 cells";
            dom.fsGridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-warning";
        }
    }
}

let saveFullscreenGridTimeout = null;
export function saveFullscreenMapGrid() {
    if (state.libraryFullscreenMap && state.libraryFullscreenMap.id) {
        const map = state.libraryFullscreenMap;
        const payload = {
            grid: {
                cell_size_px: state.libraryFullscreenGridSize,
                offset_x: state.libraryFullscreenGridOffset.x,
                offset_y: state.libraryFullscreenGridOffset.y,
            },
        };
        map.grid = { ...payload.grid };

        clearTimeout(saveFullscreenGridTimeout);
        saveFullscreenGridTimeout = setTimeout(async () => {
            try {
                const res = await fetch(`/api/curated/${encodeURIComponent(map.id)}`, {
                    method: "PATCH",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify(payload),
                });
                if (!res.ok) {
                    const errData = await res.json().catch(() => ({}));
                    throw new Error(errData.error || `HTTP ${res.status}`);
                }
                // The regrid recomputes Map Section descriptions server-side, so the cached
                // audit this map was flagged in no longer describes the bundle.
                invalidateDatasetReadiness();
            } catch (err) {
                showToast(`Grid not saved: ${err.message}`, "danger");
            }
        }, 300);
    }
}

function setFullscreenGridSize(size) {
    const num = Number.isFinite(size) ? size : parseInt(size, 10);
    if (isNaN(num)) return;
    const clamped = Math.max(MIN_GRID_SIZE, Math.min(MAX_GRID_SIZE, Math.round(num)));
    state.libraryFullscreenGridSize = clamped;
    saveFullscreenMapGrid();
    updateFullscreenGridPanel();
    updateFullscreenHud();
    requestFullscreenRender();
}

function nudgeFullscreenGridOffset(dx, dy) {
    state.libraryFullscreenGridOffset.x = Math.round(state.libraryFullscreenGridOffset.x + dx);
    state.libraryFullscreenGridOffset.y = Math.round(state.libraryFullscreenGridOffset.y + dy);
    saveFullscreenMapGrid();
    updateFullscreenGridPanel();
    updateFullscreenHud();
    requestFullscreenRender();
}

function resetFullscreenGrid() {
    state.libraryFullscreenGridSize = 64;
    state.libraryFullscreenGridOffset = { x: 0, y: 0 };
    saveFullscreenMapGrid();
    updateFullscreenGridPanel();
    updateFullscreenHud();
    requestFullscreenRender();
}

function toggleFullscreenGridAlignMode(force) {
    state.libraryFullscreenGridAlignMode = typeof force === "boolean" ? force : !state.libraryFullscreenGridAlignMode;
    if (dom.btnFsGridAlignDrag) {
        dom.btnFsGridAlignDrag.classList.toggle("active", state.libraryFullscreenGridAlignMode);
    }
    if (dom.libraryFullscreenCanvas && dom.libraryFullscreenCanvas.parentElement) {
        dom.libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", state.libraryFullscreenGridAlignMode);
    }
    requestFullscreenRender();
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
    resizeFullscreenCanvas();
    loadFullscreenMap(clampedIndex, 0);

    if (dom.libraryFullscreenCanvas) {
        dom.libraryFullscreenCanvas.focus();
    }
}

export function closeLibraryFullscreen() {
    if (!state.libraryFullscreenActive) return;

    stopFsPanLoop();
    stopFsZoomLoop();

    state.libraryFullscreenActive = false;
    if (dom.libraryFullscreenOverlay) {
        dom.libraryFullscreenOverlay.classList.add("hidden");
    }

    toggleFullscreenGridPanel(false);
    toggleFullscreenGridAlignMode(false);

    // Restore focus on card in library view and scroll into view smoothly
    if (state.libraryFullscreenIndex >= 0) {
        setLibraryFocus(state.libraryFullscreenIndex, true);
    }
}

export function setupFullscreenControls() {
    if (!dom.libraryFullscreenCanvas) return;

    // Canvas Mouse Pan Dragging & Dynamic Grid Align Drag
    dom.libraryFullscreenCanvas.addEventListener("mousedown", (e) => {
        if (e.button === 0 && (e.shiftKey || state.libraryFullscreenGridAlignMode)) {
            state.libraryFullscreenIsDraggingGrid = true;
            state.libraryFullscreenGridDragStart = { x: e.clientX, y: e.clientY };
            state.libraryFullscreenGridOffsetStart = { x: state.libraryFullscreenGridOffset.x, y: state.libraryFullscreenGridOffset.y };
            dom.libraryFullscreenCanvas.parentElement?.classList.add("grid-dragging");
            e.preventDefault();
            return;
        }

        if (e.button === 0 || e.button === 1) {
            state.libraryFullscreenIsPanning = true;
            state.libraryFullscreenPanStart = {
                x: e.clientX - state.libraryFullscreenPan.x,
                y: e.clientY - state.libraryFullscreenPan.y,
            };
            dom.libraryFullscreenCanvas.parentElement?.classList.add("panning");
            e.preventDefault();
        }
    });

    // Wheel Zoom
    dom.libraryFullscreenCanvas.addEventListener("wheel", (e) => {
        e.preventDefault();
        if (!state.libraryFullscreenCurrentImage) return;

        const rect = dom.libraryFullscreenCanvas.getBoundingClientRect();
        const point = {
            x: e.clientX - rect.left,
            y: e.clientY - rect.top,
        };

        const factor = e.deltaY < 0 ? 1.15 : (1 / 1.15);
        fullscreenZoomStep(factor, point);
    }, { passive: false });

    // HUD Buttons
    dom.btnFsGridToggle?.addEventListener("click", () => {
        state.libraryFullscreenGridVisible = !state.libraryFullscreenGridVisible;
        if (dom.btnFsGridToggle) dom.btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
        requestFullscreenRender();
    });

    dom.btnFsGridAdjust?.addEventListener("click", () => toggleFullscreenGridPanel());
    dom.btnFsFit?.addEventListener("click", () => fullscreenFitToScreen());
    dom.btnFsZoomOut?.addEventListener("click", () => fullscreenZoomStep(1 / 1.2));
    dom.btnFsZoomIn?.addEventListener("click", () => fullscreenZoomStep(1.2));

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
    dom.btnFsGridReset?.addEventListener("click", () => resetFullscreenGrid());

    dom.btnFsGridSizeDec?.addEventListener("click", () => setFullscreenGridSize(state.libraryFullscreenGridSize - 1));
    dom.btnFsGridSizeInc?.addEventListener("click", () => setFullscreenGridSize(state.libraryFullscreenGridSize + 1));

    if (dom.fsGridSizeInput) {
        dom.fsGridSizeInput.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setFullscreenGridSize(val);
        });
        dom.fsGridSizeInput.addEventListener("change", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setFullscreenGridSize(val);
        });
    }

    if (dom.fsGridSizeSlider) {
        dom.fsGridSizeSlider.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setFullscreenGridSize(val);
        });
    }

    const fsPresets = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
    fsPresets.forEach(chip => {
        chip.addEventListener("click", () => {
            const size = parseInt(chip.dataset.size, 10);
            if (!isNaN(size)) setFullscreenGridSize(size);
        });
    });

    dom.btnFsGridNudgeUp?.addEventListener("click", () => nudgeFullscreenGridOffset(0, -1));
    dom.btnFsGridNudgeDown?.addEventListener("click", () => nudgeFullscreenGridOffset(0, 1));
    dom.btnFsGridNudgeLeft?.addEventListener("click", () => nudgeFullscreenGridOffset(-1, 0));
    dom.btnFsGridNudgeRight?.addEventListener("click", () => nudgeFullscreenGridOffset(1, 0));

    dom.btnFsGridAlignDrag?.addEventListener("click", () => toggleFullscreenGridAlignMode());
}
