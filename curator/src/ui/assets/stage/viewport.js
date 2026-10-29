/**
 * Stage Viewport: the reusable pan, zoom, Grid Configuration display state,
 * device-pixel sizing, render scheduling and movement/zoom loops behind a canvas.
 *
 * One viewport is created per canvas with a content-drawing callback. Callers read
 * a snapshot through `getView()` and subscribe for changes, which are coalesced to
 * one notification per rendered frame. Grid edits are handed to the caller-supplied
 * `persistGrid` callback at most once per debounce burst.
 */

import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    MAX_GRID_SIZE,
    MIN_ZOOM,
    MAX_ZOOM,
} from "../kit/grid-math.js";

export const PAN_SPEED = 700; // pixels per second
export const ZOOM_SPEED = 2.0; // zoom factor per second of held bracket key

const FIT_PADDING = 32;
const GRID_PERSIST_DEBOUNCE_MS = 300;
const MAX_FRAME_SECONDS = 0.1;
const ZOOM_PRECISION = 1000;
const INV_SQRT_2 = 0.70710678;

const PAN_KEY_NAMES = ["w", "a", "s", "d"];

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

/**
 * Creates the viewport driving one canvas.
 *
 * @param {HTMLCanvasElement} canvas
 * @param {object} options
 * @param {(ctx: CanvasRenderingContext2D, view: object) => void} options.draw
 *        Paints one frame in CSS pixels; the context is already cleared.
 * @param {(view: object) => void} options.persistGrid
 *        Called after a burst of Grid Configuration edits settles.
 * @param {() => ({ width: number, height: number } | null)} [options.contentSize]
 *        Current content bounds in content pixels; `fit()` needs them.
 * @param {(callback: (timestamp: number) => void) => number} [options.requestFrame]
 * @param {(handle: number) => void} [options.cancelFrame]
 */
export function createViewport(canvas, options) {
    const {
        draw,
        persistGrid,
        contentSize = () => null,
        requestFrame = (callback) => globalThis.requestAnimationFrame(callback),
        cancelFrame = (handle) => globalThis.cancelAnimationFrame(handle),
    } = options;

    const pan = { x: 0, y: 0 };
    let zoom = 1.0;
    let gridSize = VTT_GRID_SIZE;
    const gridOffset = { x: 0, y: 0 };

    let metrics = { width: 0, height: 0, dpr: 1 };
    let context = null;

    let suspended = false;
    let frameHandle = null;
    let frameScheduled = false;
    const listeners = new Set();

    const panKeys = { w: false, a: false, s: false, d: false };
    let panFrameHandle = null;
    let lastPanTimestamp = null;

    const zoomKeys = { in: false, out: false };
    let zoomFrameHandle = null;
    let lastZoomTimestamp = null;

    let gridDirty = false;
    let gridPersistTimer = null;

    function getView() {
        return {
            pan: { x: pan.x, y: pan.y },
            zoom,
            gridSize,
            gridOffset: { x: gridOffset.x, y: gridOffset.y },
            width: metrics.width,
            height: metrics.height,
            dpr: metrics.dpr,
        };
    }

    function context2d() {
        if (!context) {
            context = canvas.getContext("2d");
        }
        return context;
    }

    function renderFrame() {
        frameHandle = null;
        frameScheduled = false;
        if (suspended) return;

        const view = getView();
        draw(context2d(), view);
        for (const listener of [...listeners]) listener(view);
    }

    function requestRender() {
        if (suspended || frameScheduled) return;
        frameScheduled = true;
        frameHandle = requestFrame(renderFrame);
    }

    function subscribe(listener) {
        listeners.add(listener);
        return () => listeners.delete(listener);
    }

    // ======================================================================
    // Sizing
    // ======================================================================

    function resize() {
        const parent = canvas.parentElement;
        const width = Math.round((parent && parent.clientWidth) || canvas.clientWidth || 0);
        const height = Math.round((parent && parent.clientHeight) || canvas.clientHeight || 0);
        const dpr = globalThis.devicePixelRatio || 1;
        metrics = { width, height, dpr };

        const backingWidth = Math.round(width * dpr);
        const backingHeight = Math.round(height * dpr);
        if (canvas.width !== backingWidth || canvas.height !== backingHeight) {
            canvas.width = backingWidth;
            canvas.height = backingHeight;
            if (canvas.style) {
                canvas.style.width = `${width}px`;
                canvas.style.height = `${height}px`;
            }
        }
        requestRender();
    }

    // ======================================================================
    // Pan & Zoom
    // ======================================================================

    function setZoom(nextZoom, clientPoint, round) {
        let target = clamp(nextZoom, MIN_ZOOM, MAX_ZOOM);
        if (round) target = Math.round(target * ZOOM_PRECISION) / ZOOM_PRECISION;
        if (target === zoom) return false;

        const focusX = clientPoint ? clientPoint.x : metrics.width / 2;
        const focusY = clientPoint ? clientPoint.y : metrics.height / 2;
        const ratio = target / zoom;

        // Keep the focal point stationary during zoom.
        pan.x = focusX - (focusX - pan.x) * ratio;
        pan.y = focusY - (focusY - pan.y) * ratio;
        zoom = target;
        requestRender();
        return true;
    }

    function zoomTo(targetZoom, clientPoint = null) {
        return setZoom(targetZoom, clientPoint, true);
    }

    function zoomBy(factor, clientPoint = null) {
        return setZoom(zoom * factor, clientPoint, true);
    }

    function panBy(dx, dy) {
        pan.x += dx;
        pan.y += dy;
        requestRender();
    }

    function fit() {
        const content = contentSize();
        if (!content || !content.width || !content.height) return false;
        if (!metrics.width || !metrics.height) return false;

        const availableWidth = Math.max(20, metrics.width - FIT_PADDING * 2);
        const availableHeight = Math.max(20, metrics.height - FIT_PADDING * 2);

        // Fit within the viewport, capped at 100% on initial load.
        zoom = clamp(Math.min(availableWidth / content.width, availableHeight / content.height, 1.0), MIN_ZOOM, MAX_ZOOM);
        pan.x = Math.round((metrics.width - content.width * zoom) / 2);
        pan.y = Math.round((metrics.height - content.height * zoom) / 2);
        requestRender();
        return true;
    }

    // ======================================================================
    // Movement & Zoom Loops
    // ======================================================================

    function startPanLoop() {
        if (panFrameHandle !== null || suspended) return;
        lastPanTimestamp = null;
        panFrameHandle = requestFrame(panLoop);
    }

    function stopPanLoop() {
        if (panFrameHandle !== null) {
            cancelFrame(panFrameHandle);
            panFrameHandle = null;
        }
        lastPanTimestamp = null;
        for (const key of PAN_KEY_NAMES) panKeys[key] = false;
    }

    function panLoop(timestamp) {
        panFrameHandle = null;
        if (suspended) {
            lastPanTimestamp = null;
            return;
        }
        if (lastPanTimestamp === null) lastPanTimestamp = timestamp;
        const dt = Math.min(Math.max(0, (timestamp - lastPanTimestamp) / 1000), MAX_FRAME_SECONDS);
        lastPanTimestamp = timestamp;

        let moveX = 0;
        let moveY = 0;
        if (panKeys.w) moveY += 1;
        if (panKeys.s) moveY -= 1;
        if (panKeys.a) moveX += 1;
        if (panKeys.d) moveX -= 1;

        if (moveX !== 0 || moveY !== 0) {
            if (moveX !== 0 && moveY !== 0) {
                moveX *= INV_SQRT_2;
                moveY *= INV_SQRT_2;
            }
            pan.x += moveX * PAN_SPEED * dt;
            pan.y += moveY * PAN_SPEED * dt;
            requestRender();
            panFrameHandle = requestFrame(panLoop);
        } else {
            lastPanTimestamp = null;
        }
    }

    function pressPanKey(key) {
        const name = String(key).toLowerCase();
        if (!Object.prototype.hasOwnProperty.call(panKeys, name)) return;
        panKeys[name] = true;
        startPanLoop();
    }

    function releasePanKey(key) {
        const name = String(key).toLowerCase();
        if (!Object.prototype.hasOwnProperty.call(panKeys, name)) return;
        panKeys[name] = false;
        if (!panKeys.w && !panKeys.a && !panKeys.s && !panKeys.d) stopPanLoop();
    }

    function startZoomLoop() {
        if (zoomFrameHandle !== null || suspended) return;
        lastZoomTimestamp = null;
        zoomFrameHandle = requestFrame(zoomLoop);
    }

    function stopZoomLoop() {
        if (zoomFrameHandle !== null) {
            cancelFrame(zoomFrameHandle);
            zoomFrameHandle = null;
        }
        lastZoomTimestamp = null;
        zoomKeys.in = false;
        zoomKeys.out = false;
    }

    function zoomLoop(timestamp) {
        zoomFrameHandle = null;
        if (suspended) {
            lastZoomTimestamp = null;
            return;
        }
        if (lastZoomTimestamp === null) lastZoomTimestamp = timestamp;
        const dt = Math.min(Math.max(0, (timestamp - lastZoomTimestamp) / 1000), MAX_FRAME_SECONDS);
        lastZoomTimestamp = timestamp;

        if (zoomKeys.in || zoomKeys.out) {
            let factor = 1.0;
            if (zoomKeys.in && !zoomKeys.out) {
                factor = Math.pow(ZOOM_SPEED, dt);
            } else if (zoomKeys.out && !zoomKeys.in) {
                factor = Math.pow(1 / ZOOM_SPEED, dt);
            }
            if (factor !== 1.0) setZoom(zoom * factor, null, false);
            zoomFrameHandle = requestFrame(zoomLoop);
        } else {
            lastZoomTimestamp = null;
        }
    }

    function setZoomKey(direction, pressed) {
        if (direction !== "in" && direction !== "out") return;
        zoomKeys[direction] = Boolean(pressed);
        if (zoomKeys.in || zoomKeys.out) {
            startZoomLoop();
        } else {
            stopZoomLoop();
        }
    }

    // ======================================================================
    // Grid Configuration
    // ======================================================================

    function flushGrid() {
        if (gridPersistTimer !== null) {
            clearTimeout(gridPersistTimer);
            gridPersistTimer = null;
        }
        if (!gridDirty) return;
        gridDirty = false;
        persistGrid(getView());
    }

    function scheduleGridPersist() {
        gridDirty = true;
        clearTimeout(gridPersistTimer); // trailing debounce: each edit re-arms the burst
        gridPersistTimer = setTimeout(flushGrid, GRID_PERSIST_DEBOUNCE_MS);
    }

    function setGridSize(size) {
        const numeric = Number.isFinite(size) ? size : parseInt(size, 10);
        if (Number.isNaN(numeric)) return;
        const clamped = clamp(Math.round(numeric), MIN_GRID_SIZE, MAX_GRID_SIZE);
        if (clamped === gridSize) return;
        gridSize = clamped;
        scheduleGridPersist();
        requestRender();
    }

    function setGridOffset(x, y) {
        const nextX = Math.round(x);
        const nextY = Math.round(y);
        if (nextX === gridOffset.x && nextY === gridOffset.y) return;
        gridOffset.x = nextX;
        gridOffset.y = nextY;
        scheduleGridPersist();
        requestRender();
    }

    function nudgeGrid(dx, dy) {
        setGridOffset(gridOffset.x + dx, gridOffset.y + dy);
    }

    function setGrid(offset) {
        if (!offset) return;
        setGridOffset(offset.x ?? 0, offset.y ?? 0);
    }

    function resetGrid() {
        gridSize = VTT_GRID_SIZE;
        gridOffset.x = 0;
        gridOffset.y = 0;
        scheduleGridPersist();
        requestRender();
    }

    // ======================================================================
    // Lifecycle
    // ======================================================================

    function suspend() {
        suspended = true;
        stopPanLoop();
        stopZoomLoop();
        if (frameHandle !== null) {
            cancelFrame(frameHandle);
            frameHandle = null;
        }
        frameScheduled = false;
    }

    function resume() {
        suspended = false;
        requestRender();
    }

    function destroy() {
        suspend();
        if (gridPersistTimer !== null) {
            clearTimeout(gridPersistTimer);
            gridPersistTimer = null;
        }
        gridDirty = false;
        listeners.clear();
    }

    return {
        getView,
        subscribe,
        requestRender,
        resize,
        fit,
        zoomBy,
        zoomTo,
        panBy,
        pressPanKey,
        releasePanKey,
        setZoomKey,
        setGridSize,
        nudgeGrid,
        setGrid,
        resetGrid,
        suspend,
        resume,
        flushGrid,
        destroy,
    };
}
