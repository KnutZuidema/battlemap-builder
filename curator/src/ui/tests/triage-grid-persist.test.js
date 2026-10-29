/**
 * Grid persistence regression: the /grid PATCH must always target the candidate that
 * was edited, even when the queue is spliced or refetched before the 300 ms debounce
 * settles. These tests boot the real triage glue (`stage/canvas.js`) on stubs.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import {
    flushGridEdits,
    getTriageViewport,
    restoreCandidateGrid,
    setGridOwner,
    setupCanvas,
} from "../assets/stage/canvas.js";

const GRID_PERSIST_DEBOUNCE_MS = 300;

globalThis.window = { addEventListener() {} };
globalThis.devicePixelRatio = 1;
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => {};

const patches = [];

globalThis.fetch = (url, init = {}) => {
    if (init.method === "PATCH" && String(url).endsWith("/grid")) {
        patches.push({ url, body: JSON.parse(init.body) });
    }
    return Promise.resolve({ ok: true, json: async () => ({}), text: async () => "{}" });
};

function fakeCanvas(width = 800, height = 600) {
    return {
        width: 0,
        height: 0,
        clientWidth: width,
        clientHeight: height,
        style: {},
        parentElement: null,
        getContext: () => ({}),
    };
}

function candidate(id, grid) {
    return { id, grid: grid ? { ...grid } : undefined };
}

function boot() {
    dom.canvas = fakeCanvas();
    setupCanvas();
    return getTriageViewport();
}

// Mirrors `loadCandidate(index)`: flush the pending burst for its owner, then restore
// the incoming candidate's grid (which binds the owner for the next burst).
function loadLike(viewport, index) {
    flushGridEdits();
    state.currentIndex = index;
    restoreCandidateGrid(state.candidates[index]);
    return viewport;
}

test("a burst armed for a candidate PATCHes that candidate after the queue advances", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const viewport = boot();

    state.candidates = [
        candidate(101, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
        candidate(102, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
        candidate(103, { cell_size_px: 48, offset_x: -4, offset_y: 6 }),
    ];
    loadLike(viewport, 0);
    flushGridEdits();
    const before = patches.length;

    // Switch to 102 (restore arms a burst owned by 102), then edit its grid.
    loadLike(viewport, 1);
    viewport.setGridSize(96);

    // Reject/curate splice the candidate out, flushing first (views/curation.js).
    flushGridEdits();
    state.candidates.splice(1, 1);

    // `loadCandidate` flushes again and restores whatever now sits at the index.
    loadLike(viewport, 1);

    assert.equal(patches.length, before + 1, "exactly one PATCH for the burst");
    assert.equal(patches[before].url, "/api/staging/102/grid", "the edited candidate owns the write");
    assert.deepEqual(patches[before].body, { cell_size_px: 96, offset_x: 0, offset_y: 0 });

    // The restored grid of 103 is unchanged, so its own burst issues nothing.
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, before + 1);
});

test("a burst whose candidate left the queue is dropped instead of landing on its neighbour", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const viewport = boot();

    state.candidates = [
        candidate(201, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
        candidate(202, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
        candidate(203, { cell_size_px: 48, offset_x: -4, offset_y: 6 }),
    ];
    loadLike(viewport, 0);
    flushGridEdits();
    const before = patches.length;

    // A burst armed for 202 while it is current.
    loadLike(viewport, 1);
    viewport.nudgeGrid(2, 0);
    assert.equal(viewport.getView().gridOffset.x, 2);

    // 202 is spliced out *before* the burst flushes; 203 slides into its index.
    state.candidates.splice(1, 1);
    state.currentIndex = 1;
    flushGridEdits();

    assert.equal(patches.length, before, "a removed candidate produces no PATCH");
});

test("fast navigation persists one PATCH per burst and never retargets a switch", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const viewport = boot();

    state.candidates = [
        candidate(301, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
        candidate(302, { cell_size_px: 48, offset_x: -4, offset_y: 6 }),
        candidate(303, { cell_size_px: 64, offset_x: 0, offset_y: 0 }),
    ];
    loadLike(viewport, 0);
    flushGridEdits();
    const before = patches.length;

    // Edit 301's grid, then navigate away inside the debounce window.
    viewport.setGridSize(80);
    viewport.nudgeGrid(3, -3);
    loadLike(viewport, 1);

    assert.equal(patches.length, before + 1, "one PATCH per burst");
    assert.equal(patches[before].url, "/api/staging/301/grid");
    assert.deepEqual(patches[before].body, { cell_size_px: 80, offset_x: 3, offset_y: -3 });

    // 302's restore burst is unchanged; letting it settle adds nothing.
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, before + 1);

    // Clearing the queue drops the owner, so later edits cannot target anything.
    flushGridEdits();
    setGridOwner(null);
    state.candidates = [];
    state.currentIndex = -1;
    viewport.setGridSize(112);
    t.mock.timers.tick(GRID_PERSIST_DEBOUNCE_MS);
    assert.equal(patches.length, before + 1, "no owner means no PATCH");
});
