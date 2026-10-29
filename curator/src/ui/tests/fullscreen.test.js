/**
 * Fullscreen stage regression: the overlay mounts the shared Stage Viewport and
 * `fit()` measures the layer image that is actually displayed, so a curated map whose
 * upper floor has different pixel bounds than its ground layer still fits.
 *
 * These tests boot the real fullscreen glue (`views/library/fullscreen.js`) and the
 * library's fullscreen key handler (`views/library.js`) on stub DOM/canvas/frame/
 * image/fetch objects, and drive the same entry points the buttons and keys use.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { VTT_GRID_SIZE } from "../assets/kit/grid-math.js";
import {
    closeLibraryFullscreen,
    getFullscreenViewport,
    openLibraryFullscreen,
    switchFullscreenLayer,
} from "../assets/views/library/fullscreen.js";
import { handleLibraryKeydown } from "../assets/views/library.js";

const GRID_PERSIST_DEBOUNCE_MS = 300;
const CANVAS_WIDTH = 1200;
const CANVAS_HEIGHT = 800;
// `stage/viewport.js` fits with a 32px inset on every side.
const AVAILABLE_WIDTH = CANVAS_WIDTH - 32 * 2;
const AVAILABLE_HEIGHT = CANVAS_HEIGHT - 32 * 2;

// ==========================================================================
// Stub DOM, canvas, frame clock, images and fetch
// ==========================================================================

function fakeClassList() {
    const names = new Set();
    return {
        add: (...next) => next.forEach((name) => names.add(name)),
        remove: (...next) => next.forEach((name) => names.delete(name)),
        toggle: (name, force) => {
            const on = force === undefined ? !names.has(name) : Boolean(force);
            if (on) names.add(name);
            else names.delete(name);
            return on;
        },
        contains: (name) => names.has(name),
    };
}

function fakeElement() {
    return {
        className: "",
        innerHTML: "",
        textContent: "",
        title: "",
        value: "",
        dataset: {},
        style: {},
        children: [],
        classList: fakeClassList(),
        appendChild(child) {
            this.children.push(child);
        },
        addEventListener() {},
        focus() {},
        scrollIntoView() {},
        querySelectorAll: () => [],
    };
}

function fakeContext() {
    const noop = () => {};
    return new Proxy({}, {
        get: (target, prop) => (prop in target ? target[prop] : noop),
        set: (target, prop, value) => {
            target[prop] = value;
            return true;
        },
    });
}

function fakeCanvas(width = CANVAS_WIDTH, height = CANVAS_HEIGHT) {
    return {
        width: 0,
        height: 0,
        clientWidth: width,
        clientHeight: height,
        style: {},
        classList: fakeClassList(),
        parentElement: { clientWidth: width, clientHeight: height, classList: fakeClassList() },
        getContext: () => fakeContext(),
        addEventListener() {},
        focus() {},
    };
}

let frameQueue = new Map();
let nextFrameId = 1;

function stepFrames(timestamp = 16, times = 2) {
    for (let i = 0; i < times; i += 1) {
        const due = [...frameQueue.values()];
        frameQueue = new Map();
        for (const callback of due) callback(timestamp);
    }
}

globalThis.window = { addEventListener() {} };
globalThis.devicePixelRatio = 1;
globalThis.requestAnimationFrame = (callback) => {
    const id = nextFrameId++;
    frameQueue.set(id, callback);
    return id;
};
globalThis.cancelAnimationFrame = (id) => {
    frameQueue.delete(id);
};
globalThis.document = {
    getElementById: () => null,
    querySelectorAll: () => [],
    createElement: () => fakeElement(),
    createDocumentFragment: () => fakeElement(),
    activeElement: { tagName: "BODY", blur() {} },
};

/** Image sizes registry: `/api/image/curated/{mapId}/{layerIndex}` → [width, height]. */
const imageSizes = new Map();

class FakeImage {
    constructor() {
        this.naturalWidth = 0;
        this.naturalHeight = 0;
        this.complete = false;
        this.onload = null;
        this._src = "";
    }

    get src() {
        return this._src;
    }

    set src(url) {
        this._src = url;
        const size = imageSizes.get(url);
        if (!size) return;
        this.naturalWidth = size[0];
        this.naturalHeight = size[1];
        this.complete = true;
    }
}

globalThis.Image = FakeImage;

const patches = [];

globalThis.fetch = (url, init = {}) => {
    if (init.method === "PATCH") {
        patches.push({ url, body: JSON.parse(init.body) });
    }
    return Promise.resolve({ ok: true, json: async () => ({}), text: async () => "{}" });
};

// ==========================================================================
// Fixtures
// ==========================================================================

/**
 * A curated map whose ground layer is 1024×768 while the displayed upper floor is
 * 2048×1536, so a fit that measures the ground layer is visibly wrong on the floor.
 */
function mixedLayerMap() {
    return {
        id: "map-a",
        grid: { cell_size_px: 64, offset_x: 0, offset_y: 0 },
        layers: [
            { index: 0, role: "ground", dimensions: [1024, 768] },
            { index: 1, role: "upper_floor", dimensions: [2048, 1536] },
        ],
    };
}

/** A single-layer map whose ground lattice is 16×16-eligible (25 × 20 cells). */
function largeGroundMap() {
    return {
        id: "map-b",
        grid: { cell_size_px: 64, offset_x: 0, offset_y: 0 },
        layers: [{ index: 0, role: "ground", dimensions: [1600, 1280] }],
    };
}

function registerImages(map) {
    for (const layer of map.layers) {
        imageSizes.set(`/api/image/curated/${encodeURIComponent(map.id)}/${layer.index}`, layer.dimensions);
    }
}

function mountDom() {
    dom.libraryFullscreenCanvas = fakeCanvas();
    dom.libraryFullscreenOverlay = fakeElement();
    dom.libraryFullscreenLayerDeck = fakeElement();
    dom.fsLayerFilmstripStrip = fakeElement();
    dom.fsLayerDeckCount = fakeElement();
    dom.libraryFullscreenGridPanel = fakeElement();
    dom.libraryGrid = null;
    dom.fsMapIdBadge = fakeElement();
    dom.fsMapDimensions = fakeElement();
    dom.fsMapCounter = fakeElement();
    dom.fsZoomReadout = fakeElement();
    dom.btnFsGridToggle = fakeElement();
    dom.btnFsGridAdjust = fakeElement();
    dom.btnFsPrevMap = fakeElement();
    dom.btnFsNextMap = fakeElement();
    dom.fsGridStatusBadge = fakeElement();
    dom.fsGridSizeInput = fakeElement();
    dom.fsGridSizeSlider = fakeElement();
    dom.fsGridOffsetXVal = fakeElement();
    dom.fsGridOffsetYVal = fakeElement();
    dom.fsGridLatticeCells = fakeElement();
    dom.fsGridLatticeSections = fakeElement();
}

const maps = () => [mixedLayerMap(), largeGroundMap()];

function reset() {
    if (state.libraryFullscreenActive) closeLibraryFullscreen();
    const viewport = getFullscreenViewport();
    if (viewport) {
        // Normalize the shared viewport between tests: no owner means the restore
        // burst is dropped instead of PATCHing a map this test never opened.
        state.libraryFullscreenMap = null;
        viewport.setGridSize(VTT_GRID_SIZE);
        viewport.setGridOffset({ x: 0, y: 0 });
        viewport.flushGrid();
    }
    state.libraryFilteredMaps = maps();
    state.libraryFullscreenMap = null;
    state.libraryFullscreenIndex = -1;
    state.libraryFullscreenLayerIndex = 0;
    state.libraryFullscreenCurrentImage = null;
    state.libraryFullscreenImageCache.clear();
    state.libraryFocusedIndex = -1;
    imageSizes.clear();
    for (const map of state.libraryFilteredMaps) registerImages(map);
    patches.length = 0;
    frameQueue = new Map();
}

mountDom();
reset();

function pressKey(key) {
    let prevented = 0;
    handleLibraryKeydown({ key, preventDefault: () => { prevented += 1; } });
    return prevented;
}

function fitZoomFor(width, height) {
    return Math.min(AVAILABLE_WIDTH / width, AVAILABLE_HEIGHT / height, 1.0);
}

// ==========================================================================
// Open & fit
// ==========================================================================

test("opening a mixed-layer map fits the ground layer it displays", () => {
    reset();
    openLibraryFullscreen(0);
    stepFrames();

    const view = getFullscreenViewport().getView();
    assert.equal(state.libraryFullscreenMap.id, "map-a");
    assert.equal(state.libraryFullscreenLayerIndex, 0);
    assert.equal(view.zoom, fitZoomFor(1024, 768), "the ground image is 1024 × 768");
    assert.deepEqual(view.pan, { x: 109, y: 32 }, "the ground image is centred inside the 32px inset");
});

test("switching to an upper floor fits that floor's bounds, not the ground layer's", () => {
    reset();
    openLibraryFullscreen(0);
    stepFrames();

    switchFullscreenLayer(1);
    stepFrames();

    const view = getFullscreenViewport().getView();
    assert.equal(state.libraryFullscreenLayerIndex, 1);
    assert.equal(view.zoom, fitZoomFor(2048, 1536), "fit measures the displayed upper floor");
    assert.notEqual(view.zoom, fitZoomFor(1024, 768), "the ground layer must not drive the fit");
    assert.deepEqual(view.pan, { x: 109, y: 32 }, "the upper floor is centred inside the inset");
    assert.equal(dom.fsMapDimensions.textContent, "2048 × 1536", "the HUD names the displayed layer's pixels");
});

test("F refits the displayed layer after a manual zoom", () => {
    reset();
    openLibraryFullscreen(0);
    switchFullscreenLayer(1);
    stepFrames();

    const viewport = getFullscreenViewport();
    viewport.zoomTo(1);
    assert.equal(viewport.getView().zoom, 1);

    assert.equal(pressKey("f"), 1, "F is claimed by the fullscreen stage");
    assert.equal(viewport.getView().zoom, fitZoomFor(2048, 1536));
    assert.deepEqual(viewport.getView().pan, { x: 109, y: 32 });

    // `0` is the alternate fit key.
    viewport.zoomTo(1);
    pressKey("0");
    assert.equal(viewport.getView().zoom, fitZoomFor(2048, 1536));
});

test("J/K navigate curated maps, fitting each map's displayed layer", () => {
    reset();
    openLibraryFullscreen(0);
    stepFrames();

    assert.equal(pressKey("j"), 1, "J is claimed by the fullscreen stage");
    stepFrames();

    let view = getFullscreenViewport().getView();
    assert.equal(state.libraryFullscreenMap.id, "map-b");
    assert.equal(view.zoom, fitZoomFor(1600, 1280), "map-b's 1600 × 1280 ground fits");
    assert.equal(dom.fsMapCounter.textContent, "Map 2 of 2");
    assert.equal(dom.fsMapDimensions.textContent, "1600 × 1280");

    assert.equal(pressKey("k"), 1, "K is claimed by the fullscreen stage");
    stepFrames();

    view = getFullscreenViewport().getView();
    assert.equal(state.libraryFullscreenMap.id, "map-a");
    assert.equal(view.zoom, fitZoomFor(1024, 768), "back on map-a's ground layer");
    assert.equal(dom.fsMapCounter.textContent, "Map 1 of 2");

    // J at the last map and K at the first are no-ops, not errors.
    pressKey("k");
    pressKey("j");
    pressKey("j");
    stepFrames();
    assert.equal(state.libraryFullscreenMap.id, "map-b");
});

// ==========================================================================
// Lattice badge
// ==========================================================================

test("the lattice badge stays ground-based and renders both branches", () => {
    reset();
    openLibraryFullscreen(0);
    stepFrames();

    // map-a's ground layer is 1024 × 768 at 64px: 16 × 12 cells, below 16×16.
    assert.equal(dom.fsGridStatusBadge.textContent, "16 × 12 cells (< 16×16 cells)");
    assert.equal(dom.fsGridStatusBadge.classList.contains("hud-badge-warning"), true);
    assert.equal(dom.fsGridLatticeCells.textContent, "16 × 12 cells");
    assert.equal(dom.fsGridLatticeSections.textContent, "< 16×16 cells");

    // The displayed upper floor is 2048 × 1536 (32 × 24 cells), but the lattice is
    // ground-based per ADR-0007: the badge must not follow the layer image.
    switchFullscreenLayer(1);
    stepFrames();
    assert.equal(dom.fsGridStatusBadge.textContent, "16 × 12 cells (< 16×16 cells)");
    assert.equal(dom.fsGridLatticeCells.textContent, "16 × 12 cells");

    // map-b's ground layer is 1600 × 1280 at 64px: 25 × 20 cells, eligible.
    pressKey("j");
    stepFrames();
    assert.equal(dom.fsGridStatusBadge.textContent, "25 × 20 cells (2 × 2 sections)");
    assert.equal(dom.fsGridStatusBadge.classList.contains("hud-badge-warning"), false);
    assert.equal(dom.fsGridLatticeCells.textContent, "25 × 20 cells");
    assert.equal(dom.fsGridLatticeSections.textContent, "2 × 2 sections");
});

// ==========================================================================
// Grid persistence targeting
// ==========================================================================

test("an edit burst PATCHes the map that owned it, even when J switches mid-burst", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    reset();
    openLibraryFullscreen(0);
    stepFrames();
    assert.equal(patches.length, 0, "restoring map-a's unchanged grid issues no write");

    getFullscreenViewport().nudgeGrid(3, -3);
    pressKey("j");

    assert.equal(patches.length, 1, "J flushes the burst it interrupted");
    assert.equal(patches[0].url, "/api/curated/map-a");
    assert.deepEqual(patches[0].body, { grid: { cell_size_px: 64, offset_x: 3, offset_y: -3 } });

    // The switch restored map-b's unchanged grid; its own burst writes nothing.
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, 1);

    // An edit on map-b lands on map-b after the debounce, not back on map-a.
    getFullscreenViewport().setGridSize(80);
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, 2);
    assert.equal(patches[1].url, "/api/curated/map-b");
    assert.deepEqual(patches[1].body, { grid: { cell_size_px: 80, offset_x: 0, offset_y: 0 } });
    assert.equal(state.libraryFilteredMaps[1].grid.cell_size_px, 80, "the cached map mirrors the write");
});

// ==========================================================================
// Close & reopen
// ==========================================================================

test("closing flushes and suspends; reopening fits and restores the saved grid", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    reset();
    openLibraryFullscreen(0);
    stepFrames();

    getFullscreenViewport().nudgeGrid(1, 0);
    closeLibraryFullscreen();

    assert.equal(patches.length, 1, "close settles the pending burst");
    assert.equal(patches[0].url, "/api/curated/map-a");
    assert.equal(state.libraryFullscreenActive, false);
    assert.equal(dom.libraryFullscreenOverlay.classList.contains("hidden"), true);
    assert.equal(getFullscreenViewport().getView().gridPanelOpen, false);
    assert.equal(frameQueue.size, 0, "a suspended viewport cancels its queued frame");

    // No write survives the close: the debounce timer was replaced by the flush.
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, 1);

    openLibraryFullscreen(0);
    stepFrames();

    const view = getFullscreenViewport().getView();
    assert.equal(state.libraryFullscreenActive, true);
    assert.equal(state.libraryFullscreenMap.id, "map-a");
    assert.equal(view.zoom, fitZoomFor(1024, 768), "reopen fits the reloaded layer");
    assert.deepEqual(view.pan, { x: 109, y: 32 });
    assert.equal(view.gridOffset.x, 1, "reopen restores map-a's saved offset");
    assert.equal(view.gridSize, 64);

    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, 1, "reopening a map with its own grid writes nothing");
});
