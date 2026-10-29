import test from "node:test";
import assert from "node:assert/strict";

import { createViewport } from "../assets/stage/viewport.js";
import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    MAX_GRID_SIZE,
    MIN_ZOOM,
    MAX_ZOOM,
} from "../assets/kit/grid-math.js";

function createFrameClock() {
    let nextId = 1;
    let queue = new Map();
    return {
        requestFrame(callback) {
            const id = nextId++;
            queue.set(id, callback);
            return id;
        },
        cancelFrame(id) {
            queue.delete(id);
        },
        pending() {
            return queue.size;
        },
        step(timestamp = 16) {
            const due = [...queue.values()];
            queue = new Map();
            for (const callback of due) callback(timestamp);
        },
    };
}

function createFakeCanvas(width = 800, height = 600) {
    return {
        width: 0,
        height: 0,
        clientWidth: width,
        clientHeight: height,
        style: {},
        parentElement: null,
        getContext(kind) {
            assert.equal(kind, "2d");
            return {};
        },
    };
}

function createHarness({ width = 800, height = 600, content = null } = {}) {
    const clock = createFrameClock();
    const canvas = createFakeCanvas(width, height);
    const draws = [];
    const persisted = [];
    const viewport = createViewport(canvas, {
        draw: (ctx, view) => draws.push(view),
        persistGrid: (view) => persisted.push(view),
        contentSize: () => content,
        requestFrame: (callback) => clock.requestFrame(callback),
        cancelFrame: (handle) => clock.cancelFrame(handle),
    });
    return { clock, canvas, viewport, draws, persisted };
}

test("coalesces N requestRender calls into one draw and one notification per subscriber", () => {
    const h = createHarness({ content: { width: 1024, height: 768 } });
    h.viewport.resize();
    const notifications = [];
    const secondSubscriber = [];
    const unsubscribe = h.viewport.subscribe((view) => notifications.push(view));
    h.viewport.subscribe(() => secondSubscriber.push("notified"));

    for (let i = 0; i < 4; i++) h.viewport.requestRender();
    assert.equal(h.clock.pending(), 1);
    assert.equal(h.draws.length, 0);

    h.clock.step();
    assert.equal(h.draws.length, 1);
    assert.equal(notifications.length, 1);
    assert.equal(secondSubscriber.length, 1);

    unsubscribe();
    h.viewport.requestRender();
    h.clock.step();
    assert.equal(h.draws.length, 2);
    assert.equal(notifications.length, 1, "unsubscribed listener must not be notified again");
    assert.equal(secondSubscriber.length, 2);
});

test("notifies subscribers after drawing, with the rendered view", () => {
    const h = createHarness();
    const seen = [];
    h.viewport.subscribe((view) => {
        seen.push({ drawn: h.draws.length, view });
    });

    h.viewport.resize();
    h.clock.step();

    assert.equal(seen.length, 1);
    assert.equal(seen[0].drawn, 1, "the frame must be drawn before subscribers run");
    assert.deepEqual(seen[0].view, h.viewport.getView());
});

test("resize scales the backing store by the device pixel ratio", (t) => {
    const h = createHarness({ width: 800, height: 600 });
    t.after(() => {
        delete globalThis.devicePixelRatio;
    });

    globalThis.devicePixelRatio = 2;
    h.viewport.resize();
    const view = h.viewport.getView();
    assert.equal(view.width, 800);
    assert.equal(view.height, 600);
    assert.equal(view.dpr, 2);
    assert.equal(h.canvas.width, 1600);
    assert.equal(h.canvas.height, 1200);
    assert.equal(h.canvas.style.width, "800px");
    assert.equal(h.canvas.style.height, "600px");

    globalThis.devicePixelRatio = 3;
    h.canvas.clientWidth = 400;
    h.canvas.clientHeight = 300;
    h.viewport.resize();
    assert.equal(h.viewport.getView().width, 400);
    assert.equal(h.canvas.width, 1200);
    assert.equal(h.canvas.height, 900);
});

test("fit centres the content and clamps to the zoom bounds", () => {
    const framed = createHarness({ width: 800, height: 600, content: { width: 1600, height: 1200 } });
    framed.viewport.resize();
    framed.clock.step();
    assert.equal(framed.viewport.fit(), true);
    const framedView = framed.viewport.getView();
    assert.equal(framedView.zoom, 536 / 1200);
    assert.deepEqual(framedView.pan, { x: 43, y: 32 });

    const small = createHarness({ width: 800, height: 600, content: { width: 200, height: 100 } });
    small.viewport.resize();
    small.clock.step();
    small.viewport.fit();
    assert.equal(small.viewport.getView().zoom, 1, "fit is capped at 100%");
    assert.deepEqual(small.viewport.getView().pan, { x: 300, y: 250 });

    const huge = createHarness({ width: 800, height: 600, content: { width: 100000, height: 100000 } });
    huge.viewport.resize();
    huge.clock.step();
    huge.viewport.fit();
    assert.equal(huge.viewport.getView().zoom, MIN_ZOOM);
    assert.deepEqual(huge.viewport.getView().pan, { x: -4600, y: -4700 });

    const empty = createHarness();
    empty.viewport.resize();
    empty.clock.step();
    assert.equal(empty.viewport.fit(), false, "fit without content is a no-op");
});

test("zoomBy keeps the anchor point stable and clamps to MIN_ZOOM/MAX_ZOOM", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();

    h.viewport.zoomBy(2, { x: 200, y: 100 });
    let view = h.viewport.getView();
    assert.equal(view.zoom, 2);
    assert.deepEqual(view.pan, { x: -200, y: -100 });
    assert.equal((200 - view.pan.x) / view.zoom, 200, "anchor content x is unchanged");
    assert.equal((100 - view.pan.y) / view.zoom, 100, "anchor content y is unchanged");

    h.viewport.zoomBy(1000);
    view = h.viewport.getView();
    assert.equal(view.zoom, MAX_ZOOM);
    assert.deepEqual(view.pan, { x: -1100, y: -700 });
    assert.equal((400 - view.pan.x) / view.zoom, 300, "viewport centre content x is unchanged");

    h.viewport.zoomBy(0.000001, { x: 0, y: 0 });
    view = h.viewport.getView();
    assert.equal(view.zoom, MIN_ZOOM);
    assert.deepEqual(view.pan, { x: -22, y: -14 });
    assert.equal((0 - view.pan.x) / view.zoom, 220, "zero anchor content x is unchanged");

    h.viewport.zoomTo(1.23456);
    assert.equal(h.viewport.getView().zoom, 1.235);
    h.viewport.zoomTo(10);
    assert.equal(h.viewport.getView().zoom, MAX_ZOOM);
    h.viewport.zoomTo(0.0001);
    assert.equal(h.viewport.getView().zoom, MIN_ZOOM);
});

test("grid APIs clamp the cell size and round the offset", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();

    assert.equal(h.viewport.getView().gridSize, VTT_GRID_SIZE);

    h.viewport.setGridSize(4);
    assert.equal(h.viewport.getView().gridSize, MIN_GRID_SIZE);

    h.viewport.setGridSize("48");
    assert.equal(h.viewport.getView().gridSize, 48);

    h.viewport.setGridSize(1000);
    assert.equal(h.viewport.getView().gridSize, MAX_GRID_SIZE);

    h.viewport.nudgeGrid(3, -2);
    assert.deepEqual(h.viewport.getView().gridOffset, { x: 3, y: -2 });

    h.viewport.setGrid({ x: 10.4, y: -7.6 });
    assert.deepEqual(h.viewport.getView().gridOffset, { x: 10, y: -8 });

    h.viewport.resetGrid();
    assert.equal(h.viewport.getView().gridSize, VTT_GRID_SIZE);
    assert.deepEqual(h.viewport.getView().gridOffset, { x: 0, y: 0 });
});

test("a grid edit burst schedules exactly one persistGrid call", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const h = createHarness();
    h.viewport.resize();

    h.viewport.nudgeGrid(1, 0);
    h.viewport.nudgeGrid(1, 0);
    h.viewport.setGridSize(96);
    h.viewport.setGridSize(128);
    assert.equal(h.persisted.length, 0, "nothing is persisted synchronously");

    t.mock.timers.tick(299);
    assert.equal(h.persisted.length, 0);

    t.mock.timers.tick(1);
    assert.equal(h.persisted.length, 1);
    assert.equal(h.persisted[0].gridSize, 128);
    assert.deepEqual(h.persisted[0].gridOffset, { x: 2, y: 0 });

    t.mock.timers.tick(300);
    assert.equal(h.persisted.length, 1, "an idle burst schedules nothing more");
});

test("flushGrid forces the pending persist exactly once", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();

    h.viewport.nudgeGrid(4, 0);
    h.viewport.resetGrid();
    h.viewport.flushGrid();
    assert.equal(h.persisted.length, 1);
    assert.equal(h.persisted[0].gridSize, VTT_GRID_SIZE);
    assert.deepEqual(h.persisted[0].gridOffset, { x: 0, y: 0 });

    h.viewport.flushGrid();
    assert.equal(h.persisted.length, 1, "a second flush has nothing pending");

    h.viewport.setGridSize(72);
    h.viewport.flushGrid();
    assert.equal(h.persisted.length, 2);
    assert.equal(h.persisted[1].gridSize, 72);
});

test("grid edits still coalesce to one notification per frame", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();
    let notifications = 0;
    h.viewport.subscribe(() => notifications++);

    h.viewport.setGridSize(80);
    h.viewport.nudgeGrid(1, 0);
    h.viewport.nudgeGrid(0, 1);
    assert.equal(h.clock.pending(), 1);
    h.clock.step();
    assert.equal(notifications, 1);
    assert.equal(h.draws.length, 2);
});

test("suspend() stops rAF work until resume()", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();
    assert.equal(h.draws.length, 1);

    h.viewport.suspend();
    h.viewport.requestRender();
    assert.equal(h.clock.pending(), 0, "a suspended viewport schedules no frames");
    h.clock.step();
    assert.equal(h.draws.length, 1);

    h.viewport.pressPanKey("w");
    assert.equal(h.clock.pending(), 0, "the pan loop stays stopped while suspended");

    h.viewport.resume();
    assert.equal(h.clock.pending(), 1);
    h.clock.step();
    assert.equal(h.draws.length, 2);
});

test("the pan loop moves the view at PAN_SPEED and stops on release", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();

    h.viewport.pressPanKey("w");
    h.clock.step(1000);
    h.clock.step(1100);
    const moved = h.viewport.getView();
    assert.ok(Math.abs(moved.pan.y - 70) < 1e-9, `expected 700px/s over 100ms, got ${moved.pan.y}`);
    assert.equal(moved.pan.x, 0);

    h.viewport.releasePanKey("w");
    h.clock.step(1200);
    h.clock.step(1300);
    assert.equal(h.clock.pending(), 0, "no loop frames remain after release");
});

test("the zoom loop scales the view while a bracket key is held", () => {
    const h = createHarness();
    h.viewport.resize();
    h.clock.step();

    h.viewport.setZoomKey("in", true);
    h.clock.step(0);
    h.clock.step(100);
    const zoomedIn = h.viewport.getView();
    assert.ok(Math.abs(zoomedIn.zoom - Math.pow(2, 0.1)) < 1e-12);
    assert.equal((400 - zoomedIn.pan.x) / zoomedIn.zoom, 400, "centre content x stays anchored");

    h.viewport.setZoomKey("in", false);
    h.viewport.setZoomKey("out", true);
    h.clock.step(200);
    h.clock.step(300);
    assert.ok(h.viewport.getView().zoom < zoomedIn.zoom);
    h.viewport.setZoomKey("out", false);

    h.clock.step(400);
    h.clock.step(500);
    assert.equal(h.clock.pending(), 0, "no loop frames remain after release");
});
