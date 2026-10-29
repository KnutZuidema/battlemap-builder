/**
 * Triage stage glue: mounts the shared Stage Viewport on the triage canvas with
 * the candidate drawer, the VTT grid overlay and staging grid persistence.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE } from "../kit/grid-math.js";
import { createViewport, drawGridOverlay, isGridCalibrating } from "./viewport.js";
import { saveStagingGrid } from "../requests/staging.js";

let viewport = null;

/**
 * The candidate id whose Grid Configuration the viewport displays. An edit burst
 * captures it at its first edit and persists against it, so a burst armed for one
 * candidate can never land on the candidate the queue advances to instead.
 */
let gridOwnerId = null;

/**
 * The triage Stage Viewport; null until `setupCanvas()` runs.
 */
export function getTriageViewport() {
    return viewport;
}

/**
 * Binds subsequent Grid Configuration edits to a candidate id, and `null` when no
 * candidate is loaded. Call before restoring a candidate's grid into the viewport.
 */
export function setGridOwner(candidateId) {
    gridOwnerId = candidateId ?? null;
}

/**
 * Applies a candidate's persisted Grid Configuration to the viewport and binds the
 * persistence owner to it.
 */
export function restoreCandidateGrid(candidate) {
    if (!candidate) {
        setGridOwner(null);
        return;
    }
    setGridOwner(candidate.id);
    if (!viewport) return;
    viewport.setGridSize(candidate.grid?.cell_size_px ?? VTT_GRID_SIZE);
    viewport.setGridOffset({
        x: candidate.grid?.offset_x ?? 0,
        y: candidate.grid?.offset_y ?? 0,
    });
}

/**
 * Persists a pending Grid Configuration burst now, against the candidate that owns it.
 * Call before replacing or splicing `state.candidates` so the burst still resolves.
 */
export function flushGridEdits() {
    if (viewport) viewport.flushGrid();
}

export function setupCanvas() {
    if (!viewport) {
        viewport = createViewport(dom.canvas, {
            draw: drawTriageFrame,
            persistGrid: persistTriageGrid,
            gridOwner: () => gridOwnerId,
            contentSize: triageContentSize,
        });
    }
    viewport.resize();
    window.addEventListener("resize", () => viewport.resize());
}

export function requestRender() {
    if (viewport) viewport.requestRender();
}

export function resizeCanvas() {
    if (viewport) viewport.resize();
}

export function fitToScreen() {
    if (viewport) viewport.fit();
}

/**
 * Stops the triage pan and zoom loops, e.g. while a modal takes the keyboard.
 */
export function stopStageMotion() {
    if (!viewport) return;
    for (const key of ["w", "a", "s", "d"]) viewport.releasePanKey(key);
    viewport.setZoomKey("in", false);
    viewport.setZoomKey("out", false);
}

function triageContentSize() {
    const image = state.currentImage;
    if (!image) return null;
    const width = image.naturalWidth || image.width;
    const height = image.naturalHeight || image.height;
    return width > 0 && height > 0 ? { width, height } : null;
}

/**
 * Persists the Grid Configuration of the candidate that owned the settled edit burst.
 *
 * The viewport debounces this to one call per burst and passes the owner it captured
 * when the burst was armed; the candidate is looked up by that id, never by
 * `state.currentIndex`, because the queue may have been reordered or spliced since.
 * A burst whose owner has left the queue is dropped. A call that would write the
 * candidate's current (or default) configuration is skipped, so restoring a
 * candidate's grid on load never issues a request of its own.
 */
function persistTriageGrid(ownerId, view) {
    if (ownerId === null || ownerId === undefined) return;

    const candidate = state.candidates.find((c) => c.id === ownerId);
    if (!candidate) return;

    const payload = {
        cell_size_px: view.gridSize,
        offset_x: view.gridOffset.x,
        offset_y: view.gridOffset.y,
    };
    const stored = candidate.grid;
    const unchanged = stored
        ? stored.cell_size_px === payload.cell_size_px
            && stored.offset_x === payload.offset_x
            && stored.offset_y === payload.offset_y
        : payload.cell_size_px === VTT_GRID_SIZE && payload.offset_x === 0 && payload.offset_y === 0;
    candidate.grid = payload;
    if (unchanged) return;

    saveStagingGrid(candidate.id, payload).catch(() => {
        // The request module reported the failure once; the candidate keeps the local
        // configuration so the next burst retries it.
    });
}

/**
 * High-performance 60fps Canvas 2D render loop.
 * Applies devicePixelRatio scaling, pan/zoom transformation matrix,
 * draws the battlemap, and renders the single-draw-call 64px VTT grid overlay.
 */
function drawTriageFrame(ctx, view) {
    const { width, height, dpr } = view;

    ctx.save();
    ctx.clearRect(0, 0, width * dpr, height * dpr);

    // Fill background
    ctx.fillStyle = "#070a10";
    ctx.fillRect(0, 0, width * dpr, height * dpr);

    // Normalize devicePixelRatio coordinates
    ctx.scale(dpr, dpr);

    if (state.currentImage && state.currentImage.complete && state.currentImage.naturalWidth > 0) {
        const img = state.currentImage;
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;

        ctx.save();
        // Apply pan and zoom transformation
        ctx.translate(view.pan.x, view.pan.y);
        ctx.scale(view.zoom, view.zoom);

        // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
        ctx.imageSmoothingEnabled = view.zoom < 2.0;
        ctx.imageSmoothingQuality = "high";

        // Draw active layer battlemap at origin (0, 0)
        ctx.globalAlpha = 1.0;
        ctx.drawImage(img, 0, 0, imgW, imgH);
        const calibrating = isGridCalibrating(view, state.isDraggingGrid);
        // Draw crisp 64px VTT grid overlay in image space
        if (view.gridVisible || calibrating) {
            drawGridOverlay(ctx, imgW, imgH, view, calibrating);
        }

        // Draw image bounding border
        ctx.beginPath();
        ctx.rect(0, 0, imgW, imgH);
        ctx.lineWidth = Math.max(1.0, 1.5 / view.zoom);
        ctx.strokeStyle = "rgba(56, 189, 248, 0.75)";
        ctx.stroke();

        ctx.restore();
    }

    ctx.restore();
}
