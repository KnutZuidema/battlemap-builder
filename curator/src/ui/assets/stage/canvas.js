/**
 * Triage stage glue: mounts the shared Stage Viewport on the triage canvas with
 * the candidate drawer, the VTT grid overlay and staging grid persistence.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE } from "../kit/grid-math.js";
import { createViewport } from "./viewport.js";

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

    fetch(`/api/staging/${encodeURIComponent(candidate.id)}/grid`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
    }).catch((err) => {
        console.error("Failed to save staging candidate grid config:", err);
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
        const isCalibrationActive = Boolean(state.gridPanelOpen || state.gridAlignMode || state.isDraggingGrid);
        // Draw crisp 64px VTT grid overlay in image space
        if (state.gridVisible || isCalibrationActive) {
            drawVttGrid(ctx, imgW, imgH, isCalibrationActive, view);
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

/**
 * Single-draw-call crisp 64px VTT grid overlay.
 * Evaluates 5ft D&D tactical scale, door alignment, and poché wall consistency.
 */
function drawVttGrid(c, width, height, isCalibrationActive, view) {
    c.save();
    c.beginPath();
    c.rect(0, 0, width, height);
    c.clip();

    const size = view.gridSize || VTT_GRID_SIZE;
    const ox = ((Math.round(view.gridOffset.x) % size) + size) % size;
    const oy = ((Math.round(view.gridOffset.y) % size) + size) % size;
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
    c.lineWidth = Math.max(0.5, 1.0 / view.zoom);
    c.strokeStyle = "rgba(56, 189, 248, 0.4)";
    c.stroke();

    // Draw boundary border around active full grid
    if (isCalibrationActive && cols > 0 && rows > 0) {
        c.save();
        c.beginPath();
        c.rect(ox, oy, fullW, fullH);
        c.lineWidth = Math.max(1.5, 2.5 / view.zoom);
        const isEligible = cols >= 16 && rows >= 16;
        c.strokeStyle = isEligible ? "rgba(56, 189, 248, 0.95)" : "rgba(244, 63, 94, 0.95)";
        c.stroke();
        c.restore();
    }

    c.restore();
}
