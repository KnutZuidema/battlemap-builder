/**
 * Canvas 2D engine for the triage stage: sizing, the render loop, the VTT grid
 * overlay, and the continuous WASD pan / bracket zoom loops.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE, MIN_ZOOM, MAX_ZOOM } from "../kit/grid-math.js";
import { getCandidateLayers } from "../kit/layers.js";
import { updateHud, resizeFullscreenCanvas, requestFullscreenRender } from "../app.js";

// ==========================================================================
// Canvas 2D Engine & Rendering
// ==========================================================================

export function setupCanvas() {
    resizeCanvas();
    window.addEventListener("resize", () => {
        resizeCanvas();
        requestRender();
        if (state.libraryFullscreenActive) {
            resizeFullscreenCanvas();
            requestFullscreenRender();
        }
    });
}

export function resizeCanvas() {
    const container = dom.canvas.parentElement;
    const width = container.clientWidth;
    const height = container.clientHeight;
    const dpr = window.devicePixelRatio || 1;

    if (dom.canvas.width !== width * dpr || dom.canvas.height !== height * dpr) {
        dom.canvas.width = width * dpr;
        dom.canvas.height = height * dpr;
        dom.canvas.style.width = `${width}px`;
        dom.canvas.style.height = `${height}px`;
    }
}

let renderPending = false;
export function requestRender() {
    if (!renderPending) {
        renderPending = true;
        requestAnimationFrame(() => {
            renderPending = false;
            render();
        });
    }
}

/**
 * Retrieves Ground layer dimensions for the current triage candidate.
 */
export function getCurrentGroundDimensions() {
    const cand = state.candidates && state.candidates[state.currentIndex];
    if (cand) {
        const layers = getCandidateLayers(cand);
        const ground = layers.find(l => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "ground";
        });
        if (ground && ground.dimensions && ground.dimensions[0] > 0 && ground.dimensions[1] > 0) {
            return { width: ground.dimensions[0], height: ground.dimensions[1] };
        }
        const l0 = layers.find(l => l.index === 0);
        if (l0 && l0.dimensions && l0.dimensions[0] > 0 && l0.dimensions[1] > 0) {
            return { width: l0.dimensions[0], height: l0.dimensions[1] };
        }
        if (layers.length > 0 && layers[0].dimensions && layers[0].dimensions[0] > 0 && layers[0].dimensions[1] > 0) {
            return { width: layers[0].dimensions[0], height: layers[0].dimensions[1] };
        }
        if (cand.dimensions && Array.isArray(cand.dimensions) && cand.dimensions[0] > 0 && cand.dimensions[1] > 0) {
            return { width: cand.dimensions[0], height: cand.dimensions[1] };
        }
        if (cand.dimensions && cand.dimensions.width && cand.dimensions.height) {
            return { width: cand.dimensions.width, height: cand.dimensions.height };
        }
    }
    if (state.currentImage && state.currentImage.naturalWidth > 0) {
        return { width: state.currentImage.naturalWidth, height: state.currentImage.naturalHeight };
    }
    return { width: 2048, height: 2048 };
}

/**
 * Retrieves Ground layer dimensions for the current fullscreen map.
 */
export function getLibraryFullscreenDimensions() {
    if (state.libraryFullscreenMap) {
        const map = state.libraryFullscreenMap;
        if (map.layers && Array.isArray(map.layers) && map.layers.length > 0) {
            const ground = map.layers.find(l => {
                const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                return r === "ground";
            });
            if (ground && ground.dimensions && ground.dimensions[0] > 0 && ground.dimensions[1] > 0) {
                return { width: ground.dimensions[0], height: ground.dimensions[1] };
            }
            const l0 = map.layers.find(l => l.index === 0);
            if (l0 && l0.dimensions && l0.dimensions[0] > 0 && l0.dimensions[1] > 0) {
                return { width: l0.dimensions[0], height: l0.dimensions[1] };
            }
            if (map.layers[0].dimensions && map.layers[0].dimensions[0] > 0 && map.layers[0].dimensions[1] > 0) {
                return { width: map.layers[0].dimensions[0], height: map.layers[0].dimensions[1] };
            }
        }
        if (map.dimensions && Array.isArray(map.dimensions) && map.dimensions[0] > 0 && map.dimensions[1] > 0) {
            return { width: map.dimensions[0], height: map.dimensions[1] };
        }
        if (map.dimensions && map.dimensions.width && map.dimensions.height) {
            return { width: map.dimensions.width, height: map.dimensions.height };
        }
    }
    if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
        return {
            width: state.libraryFullscreenCurrentImage.naturalWidth,
            height: state.libraryFullscreenCurrentImage.naturalHeight,
        };
    }
    return { width: 2048, height: 2048 };
}

/**
 * High-performance 60fps Canvas 2D render loop.
 * Applies devicePixelRatio scaling, pan/zoom transformation matrix,
 * draws the battlemap, and renders the single-draw-call 64px VTT grid overlay.
 */
function render() {
    if (!dom.ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const viewWidth = dom.canvas.clientWidth;
    const viewHeight = dom.canvas.clientHeight;

    dom.ctx.save();
    dom.ctx.clearRect(0, 0, dom.canvas.width, dom.canvas.height);

    // Fill background
    dom.ctx.fillStyle = "#070a10";
    dom.ctx.fillRect(0, 0, dom.canvas.width, dom.canvas.height);

    // Normalize devicePixelRatio coordinates
    dom.ctx.scale(dpr, dpr);

    if (state.currentImage && state.currentImage.complete && state.currentImage.naturalWidth > 0) {
        const img = state.currentImage;
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;

        dom.ctx.save();
        // Apply pan and zoom transformation
        dom.ctx.translate(state.pan.x, state.pan.y);
        dom.ctx.scale(state.zoom, state.zoom);

        // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
        dom.ctx.imageSmoothingEnabled = state.zoom < 2.0;
        dom.ctx.imageSmoothingQuality = "high";

        // Draw active layer battlemap at origin (0, 0)
        dom.ctx.globalAlpha = 1.0;
        dom.ctx.drawImage(img, 0, 0, imgW, imgH);
        const isCalibrationActive = Boolean(state.gridPanelOpen || state.gridAlignMode || state.isDraggingGrid);
        // Draw crisp 64px VTT grid overlay in image space
        if (state.gridVisible || isCalibrationActive) {
            drawVttGrid(dom.ctx, imgW, imgH, isCalibrationActive);
        }

        // Draw image bounding border
        dom.ctx.beginPath();
        dom.ctx.rect(0, 0, imgW, imgH);
        dom.ctx.lineWidth = Math.max(1.0, 1.5 / state.zoom);
        dom.ctx.strokeStyle = "rgba(56, 189, 248, 0.75)";
        dom.ctx.stroke();

        dom.ctx.restore();
    }

    dom.ctx.restore();
}

/**
 * Single-draw-call crisp 64px VTT grid overlay.
 * Evaluates 5ft D&D tactical scale, door alignment, and poché wall consistency.
 */
function drawVttGrid(c, width, height, isCalibrationActive = false) {
    c.save();
    c.beginPath();
    c.rect(0, 0, width, height);
    c.clip();

    const size = state.gridSize || VTT_GRID_SIZE;
    const ox = ((Math.round(state.gridOffset.x) % size) + size) % size;
    const oy = ((Math.round(state.gridOffset.y) % size) + size) % size;
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

    // Inversely scale line width with zoom so grid remains ~1 screen pixel thin
    c.lineWidth = Math.max(0.5, 1.0 / state.zoom);
    c.strokeStyle = "rgba(56, 189, 248, 0.4)";
    c.stroke();

    // Draw boundary border around active full grid
    if (isCalibrationActive && cols > 0 && rows > 0) {
        c.save();
        c.beginPath();
        c.rect(ox, oy, fullW, fullH);
        c.lineWidth = Math.max(1.5, 2.5 / state.zoom);
        const isEligible = cols >= 16 && rows >= 16;
        c.strokeStyle = isEligible ? "rgba(56, 189, 248, 0.95)" : "rgba(244, 63, 94, 0.95)";
        c.stroke();
        c.restore();
    }

    c.restore();
}

/**
 * Fits the current battlemap image centered within the viewport.
 */
export function fitToScreen() {
    if (!state.currentImage) return;
    const img = state.currentImage;
    const imgW = img.naturalWidth || img.width;
    const imgH = img.naturalHeight || img.height;
    const viewW = dom.canvas.clientWidth;
    const viewH = dom.canvas.clientHeight;

    if (!imgW || !imgH || !viewW || !viewH) return;

    const padding = 32;
    const availW = Math.max(20, viewW - padding * 2);
    const availH = Math.max(20, viewH - padding * 2);

    // Fit within viewport bounds, capped at 100% on initial load
    const fitScale = Math.min(availW / imgW, availH / imgH, 1.0);
    state.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, fitScale));

    // Center in viewport
    state.pan.x = Math.round((viewW - imgW * state.zoom) / 2);
    state.pan.y = Math.round((viewH - imgH * state.zoom) / 2);

    requestRender();
    updateHud();
}

export function zoomStep(factor, clientPoint = null) {
    if (!state.currentImage) return;
    const prevZoom = state.zoom;
    let newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
    newZoom = Math.round(newZoom * 1000) / 1000;

    if (newZoom === prevZoom) return;

    const viewW = dom.canvas.clientWidth;
    const viewH = dom.canvas.clientHeight;
    const focusX = clientPoint ? clientPoint.x : viewW / 2;
    const focusY = clientPoint ? clientPoint.y : viewH / 2;

    // Keep the focal point stationary during zoom:
    state.pan.x = focusX - (focusX - state.pan.x) * (newZoom / prevZoom);
    state.pan.y = focusY - (focusY - state.pan.y) * (newZoom / prevZoom);
    state.zoom = newZoom;

    requestRender();
    updateHud();
}

// ==========================================================================
// Viewport Panning Loop & Hotkey Controls
// ==========================================================================

export const activePanKeys = { w: false, a: false, s: false, d: false };
let panAnimationId = null;
let lastPanTimestamp = 0;
export const PAN_SPEED = 700; // pixels per second

function panLoop(timestamp) {
    if (!lastPanTimestamp) lastPanTimestamp = timestamp;
    const dt = Math.min((timestamp - lastPanTimestamp) / 1000, 0.1);
    lastPanTimestamp = timestamp;

    let moveX = 0;
    let moveY = 0;
    if (activePanKeys.w) moveY += 1;
    if (activePanKeys.s) moveY -= 1;
    if (activePanKeys.a) moveX += 1;
    if (activePanKeys.d) moveX -= 1;

    if (moveX !== 0 || moveY !== 0) {
        if (moveX !== 0 && moveY !== 0) {
            const invSqrt2 = 0.70710678;
            moveX *= invSqrt2;
            moveY *= invSqrt2;
        }
        if (state.currentImage) {
            state.pan.x += moveX * PAN_SPEED * dt;
            state.pan.y += moveY * PAN_SPEED * dt;
            requestRender();
            updateHud();
        }
        panAnimationId = requestAnimationFrame(panLoop);
    } else {
        panAnimationId = null;
        lastPanTimestamp = 0;
    }
}

export function startPanLoop() {
    if (!panAnimationId) {
        lastPanTimestamp = performance.now();
        panAnimationId = requestAnimationFrame(panLoop);
    }
}

export function stopPanLoop() {
    if (panAnimationId) {
        cancelAnimationFrame(panAnimationId);
        panAnimationId = null;
    }
    lastPanTimestamp = 0;
    activePanKeys.w = false;
    activePanKeys.a = false;
    activePanKeys.s = false;
    activePanKeys.d = false;
}

function panStep(dx, dy) {
    if (!state.currentImage) return;
    state.pan.x += dx;
    state.pan.y += dy;
    requestRender();
    updateHud();
}

// ==========================================================================
// Viewport Zooming Loop & Hotkey Controls
// ==========================================================================

export const activeZoomKeys = { in: false, out: false };
let zoomAnimationId = null;
let lastZoomTimestamp = 0;
export const ZOOM_SPEED = 2.0;

function zoomLoop(timestamp) {
    if (!lastZoomTimestamp) lastZoomTimestamp = timestamp;
    const dt = Math.min((timestamp - lastZoomTimestamp) / 1000, 0.1);
    lastZoomTimestamp = timestamp;

    if (activeZoomKeys.in || activeZoomKeys.out) {
        let factor = 1.0;
        if (activeZoomKeys.in && !activeZoomKeys.out) {
            factor = Math.pow(ZOOM_SPEED, dt);
        } else if (activeZoomKeys.out && !activeZoomKeys.in) {
            factor = Math.pow(1 / ZOOM_SPEED, dt);
        }

        if (factor !== 1.0 && state.currentImage) {
            const prevZoom = state.zoom;
            const newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
            if (newZoom !== prevZoom) {
                const focusX = dom.canvas.clientWidth / 2;
                const focusY = dom.canvas.clientHeight / 2;
                state.pan.x = focusX - (focusX - state.pan.x) * (newZoom / prevZoom);
                state.pan.y = focusY - (focusY - state.pan.y) * (newZoom / prevZoom);
                state.zoom = newZoom;
                requestRender();
                updateHud();
            }
        }
        zoomAnimationId = requestAnimationFrame(zoomLoop);
    } else {
        zoomAnimationId = null;
        lastZoomTimestamp = 0;
    }
}

export function startZoomLoop() {
    if (!zoomAnimationId) {
        lastZoomTimestamp = performance.now();
        zoomAnimationId = requestAnimationFrame(zoomLoop);
    }
}

export function stopZoomLoop() {
    if (zoomAnimationId) {
        cancelAnimationFrame(zoomAnimationId);
        zoomAnimationId = null;
    }
    lastZoomTimestamp = 0;
    activeZoomKeys.in = false;
    activeZoomKeys.out = false;
}
