/**
 * The storage footprint read, at the `views/storage.js` boundary: deduplication, the loading
 * state and the retryable error state.
 *
 * `app.js` fetched the footprint on boot and `views/storage.js` fetched it again on every drawer
 * open and refresh, so the two concurrent reads of a ~94s endpoint competed for the same pod.
 * A failed read was swallowed by an empty `catch (_) {}`, so the modal kept the markup
 * fallbacks — `0 B`, `0 files`, `0 B (0)` — as if they had been measured.
 *
 * These tests pin the three states the drawer must be able to be in:
 *
 *   loading  the fetch is in flight and the drawer says so instead of showing figures;
 *   ready    the measured figures;
 *   error    an explicit failure with a retry, never a measured-looking zero.
 *
 * The element ids below are the contract the view writes to (see `index.html`):
 * `#storage-stats-loading`, `#storage-stats-error`, `#btn-retry-storage-stats`.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { formatBytes } from "../assets/kit/format.js";
import { fetchStorageStats, setupStorageControls } from "../assets/views/storage.js";
import { has } from "../assets/requests/tracker.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { element, jsonResponse, stubFetch } from "./support/fakes.js";

/** The ids the storage view reads or writes; everything else the controls touch is inert. */
const STORAGE_IDS = [
    "toast-container",
    "storage-stats-loading",
    "storage-stats-error",
    "btn-retry-storage-stats",
    "storage-staging-bytes",
    "storage-staging-files",
    "storage-breakdown-reddit",
    "storage-breakdown-generated",
    "storage-breakdown-root",
    "storage-curated-bytes",
    "storage-curated-files",
    "storage-themes-list",
    "storage-rejected-bytes",
    "storage-rejected-records",
    "storage-telemetry-bytes",
    "storage-telemetry-files",
    "storage-total-bytes",
    "storage-total-files",
    "drawer-storage-staging",
    "drawer-storage-curated",
];

/** One storage footprint as `GET /api/storage/stats` serializes it. */
function statsPayload() {
    return {
        staging: { bytes: 2048, file_count: 2 },
        staging_breakdown: {
            root: { bytes: 2048, file_count: 2 },
            reddit: { bytes: 0, file_count: 0 },
            generated: { bytes: 0, file_count: 0 },
        },
        curated: { bytes: 4096, file_count: 4 },
        curated_by_theme: {
            dungeon: { theme: "dungeon", bytes: 4096, file_count: 4, map_count: 1 },
        },
        rejected: { bytes: 12, file_count: 1 },
        telemetry: { bytes: 340, file_count: 2 },
        total_bytes: 6496,
        total_files: 9,
    };
}

/**
 * Install a `document` whose elements record their listeners, so a test can activate the retry
 * control the way a click would.
 */
function installStorageDom() {
    const previousDocument = globalThis.document;
    const nodes = new Map();

    function node(id) {
        if (!nodes.has(id)) {
            const created = element("div");
            const listeners = [];
            created.listeners = listeners;
            created.addEventListener = (type, handler) => listeners.push({ type, handler });
            created.trigger = (type, event = {}) => {
                for (const listener of listeners.filter((entry) => entry.type === type)) {
                    listener.handler(event);
                }
            };
            nodes.set(id, created);
        }
        return nodes.get(id);
    }

    for (const id of STORAGE_IDS) node(id);

    globalThis.document = {
        readyState: "complete",
        activeElement: null,
        getElementById: (id) => (nodes.has(id) ? nodes.get(id) : node(id)),
        createElement: (tag) => {
            const created = element(tag);
            const listeners = [];
            created.listeners = listeners;
            created.addEventListener = (type, handler) => listeners.push({ type, handler });
            return created;
        },
        createDocumentFragment: () => element("#fragment"),
        querySelector: () => null,
        querySelectorAll: () => [],
        addEventListener() {},
    };

    return {
        node,
        restore() {
            globalThis.document = previousDocument;
        },
    };
}

/** Let the settled promise chain drain (a read awaits the tracker, the request and the render). */
async function settle() {
    for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

/** Whether an indicator is showing: it either carries text or is not hidden. */
function isShowing(el) {
    return el.textContent.trim() !== "" && !el.classList.contains("hidden");
}

test("boot and drawer open share one in-flight stats request", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installStorageDom();
    let release;
    const pending = new Promise((resolve) => {
        release = resolve;
    });
    const server = stubFetch(() => pending);
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    const boot = fetchStorageStats();
    const drawerOpen = fetchStorageStats();

    assert.equal(
        server.calls.length,
        1,
        "the drawer open joins the boot read instead of starting a second one",
    );
    assert.equal(has("storage:stats"), true, "the read is registered under the storage:stats key");

    release(jsonResponse(statsPayload()));
    await Promise.all([boot, drawerOpen]);
    await settle();

    assert.equal(has("storage:stats"), false, "a settled read leaves the registry");
    assert.equal(ui.node("storage-staging-bytes").textContent, formatBytes(2048));
});

test("a settled read is not reused, so an explicit refresh re-reads", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installStorageDom();
    const server = stubFetch(async () => jsonResponse(statsPayload()));
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    await fetchStorageStats();
    await fetchStorageStats();

    assert.equal(server.calls.length, 2, "each settled read is a new request");
});

test("a fetch in flight shows a loading indicator, then the measured figures", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installStorageDom();
    let release;
    const pending = new Promise((resolve) => {
        release = resolve;
    });
    const server = stubFetch(() => pending);
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    const read = fetchStorageStats();
    await settle();

    assert.ok(
        isShowing(ui.node("storage-stats-loading")),
        "the drawer says it is loading instead of showing figures",
    );
    assert.equal(
        ui.node("storage-staging-bytes").textContent,
        "",
        "no figure is claimed while the read is in flight",
    );

    release(jsonResponse(statsPayload()));
    await read;
    await settle();

    assert.equal(
        isShowing(ui.node("storage-stats-loading")),
        false,
        "the loading indicator clears once the figures arrive",
    );
    assert.equal(ui.node("storage-staging-bytes").textContent, formatBytes(2048));
    assert.equal(ui.node("storage-curated-bytes").textContent, formatBytes(4096));
    assert.equal(ui.node("storage-total-bytes").textContent, formatBytes(6496));
    assert.equal(
        isShowing(ui.node("storage-stats-error")),
        false,
        "a successful read leaves no error behind",
    );
});

test("a failed read shows a retryable error instead of a measured zero footprint", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installStorageDom();
    const responses = [
        jsonResponse(
            { error: "storage backend unavailable", code: "storage_unavailable" },
            { status: 503, statusText: "Service Unavailable" },
        ),
        jsonResponse(statsPayload()),
    ];
    const server = stubFetch(async () => responses.shift());
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });
    setupStorageControls();

    await fetchStorageStats();
    await settle();

    const error = ui.node("storage-stats-error");
    assert.ok(isShowing(error), "a failed read is reported in the drawer, not swallowed");
    assert.match(
        error.textContent,
        /storage backend unavailable|Load storage stats failed/,
        "the error names the failure",
    );
    assert.notEqual(
        ui.node("storage-staging-bytes").textContent,
        formatBytes(0),
        "a failed read is never rendered as a measured zero footprint",
    );
    assert.equal(
        isShowing(ui.node("storage-stats-loading")),
        false,
        "the loading indicator gives way to the error",
    );

    // The retry control the error offers re-reads the footprint.
    ui.node("btn-retry-storage-stats").trigger("click");
    assert.equal(server.calls.length, 2, "retrying issues a new request");
    await settle();

    assert.equal(
        isShowing(ui.node("storage-stats-error")),
        false,
        "a successful retry clears the error state",
    );
    assert.equal(
        ui.node("storage-staging-bytes").textContent,
        formatBytes(2048),
        "the retry renders the measured figures",
    );
});
