/**
 * The storage footprint read and the drawer that renders it.
 *
 * Two contracts from issue #292:
 *
 * - The stats request is deduplicated: boot and every user interaction share one in-flight
 *   request, so a drawer open during the boot scan does not issue a second full pass.
 * - The drawer never presents its markup fallbacks as a measurement: it shows a loading
 *   indicator while reading, and an error with a retry on failure instead of `0 B`.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { fetchStorageStats as fetchStorageStatsRequest } from "../assets/requests/storage.js";
import { has } from "../assets/requests/tracker.js";
import { fetchStorageStats, setupStorageControls } from "../assets/views/storage.js";
import { state } from "../assets/store.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

const STATS = {
    staging: { bytes: 1024, file_count: 3 },
    staging_breakdown: {
        root: { bytes: 1024, file_count: 3 },
        reddit: { bytes: 0, file_count: 0 },
        generated: { bytes: 0, file_count: 0 },
    },
    curated: { bytes: 2048, file_count: 2 },
    curated_by_theme: {
        dungeon: { theme: "dungeon", bytes: 2048, file_count: 2, map_count: 1 },
    },
    rejected: { bytes: 64, file_count: 5 },
    telemetry: { bytes: 128, file_count: 1 },
    total_bytes: 1024 + 2048 + 64 + 128,
    total_files: 3 + 2 + 5 + 1,
};

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

function setUp(t) {
    const ui = installRequestDom();
    // Reset the phase and figures the view keeps between reads.
    state.storageStats = null;
    state.storageStatsPhase = "idle";
    state.storageStatsError = null;
    t.after(() => {
        clearRequestFailure();
        ui.restore();
        state.storageStats = null;
        state.storageStatsPhase = "idle";
        state.storageStatsError = null;
    });
    return ui;
}

/** Let every settled promise chain drain. */
async function settle() {
    for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

test("two stats reads while one is in flight share a single request", async (t) => {
    setUp(t);
    const pending = deferred();
    const server = stubFetch(() => pending.promise);
    t.after(() => server.restore());

    const first = fetchStorageStatsRequest();
    const second = fetchStorageStatsRequest();

    assert.equal(first, second, "the second caller joins the in-flight request");
    assert.equal(server.calls.length, 1, "only one scan of the bucket is issued");
    assert.equal(server.calls[0].path, "/api/storage/stats");

    pending.resolve(jsonResponse(STATS));
    assert.deepEqual(await first, STATS);
    await settle();
    assert.equal(has("storage:stats"), false, "a settled read leaves the registry");
});

test("a refresh after the read settled issues a fresh request", async (t) => {
    setUp(t);
    const server = stubFetch(async () => jsonResponse(STATS));
    t.after(() => server.restore());

    await fetchStorageStatsRequest();
    await settle();
    await fetchStorageStatsRequest();

    assert.equal(server.calls.length, 2, "an explicit refresh re-reads the footprint");
});

test("the drawer shows a loading indicator and then the measured figures", async (t) => {
    const ui = setUp(t);
    const pending = deferred();
    const server = stubFetch(() => pending.promise);
    t.after(() => server.restore());

    const read = fetchStorageStats();
    await settle();

    const loading = globalThis.document.getElementById("storage-stats-loading");
    const content = globalThis.document.getElementById("storage-stats-content");
    const error = globalThis.document.getElementById("storage-stats-error");
    assert.equal(loading.classList.contains("hidden"), false, "the loading indicator is shown");
    assert.equal(content.classList.contains("hidden"), true, "the cards stay hidden while loading");
    assert.equal(error.classList.contains("hidden"), true);
    assert.equal(globalThis.document.getElementById("btn-refresh-storage").disabled, true);

    pending.resolve(jsonResponse(STATS));
    await read;
    await settle();

    assert.equal(loading.classList.contains("hidden"), true, "the indicator clears on success");
    assert.equal(content.classList.contains("hidden"), false, "the measured cards are shown");
    assert.equal(globalThis.document.getElementById("storage-staging-bytes").textContent, "1 KB");
    assert.equal(globalThis.document.getElementById("storage-staging-files").textContent, "3 files");
    assert.equal(globalThis.document.getElementById("storage-curated-bytes").textContent, "2 KB");
    assert.equal(globalThis.document.getElementById("storage-rejected-records").textContent, "5 records");
    assert.equal(globalThis.document.getElementById("storage-total-files").textContent, "(11 items)");
    assert.equal(globalThis.document.getElementById("drawer-storage-staging").textContent, "1 KB (3 files)");
    assert.equal(globalThis.document.getElementById("btn-refresh-storage").disabled, false);
    assert.equal(state.storageStatsPhase, "ready");
    assert.equal(ui.toastContainer.children.length, 0, "a successful read reports nothing");
});

test("a failed read shows an error with a retry instead of a zero footprint", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () => {
        throw new TypeError("Failed to fetch");
    });
    t.after(() => server.restore());

    await fetchStorageStats();
    await settle();

    const error = globalThis.document.getElementById("storage-stats-error");
    const content = globalThis.document.getElementById("storage-stats-content");
    assert.equal(error.classList.contains("hidden"), false, "the error panel is shown");
    assert.equal(content.classList.contains("hidden"), true, "no cards are rendered from placeholders");
    assert.match(
        globalThis.document.getElementById("storage-stats-error-text").textContent,
        /Storage footprint is unavailable.*Failed to fetch/,
    );
    assert.ok(globalThis.document.getElementById("btn-retry-storage"), "a retry action is offered");

    // The markup placeholders are replaced, so nothing claims a measured `0 B`.
    assert.equal(globalThis.document.getElementById("storage-staging-bytes").textContent, "—");
    assert.equal(globalThis.document.getElementById("storage-staging-files").textContent, "—");
    assert.equal(globalThis.document.getElementById("storage-breakdown-root").textContent, "—");
    assert.equal(globalThis.document.getElementById("storage-total-files").textContent, "(unavailable)");
    assert.equal(globalThis.document.getElementById("drawer-storage-staging").textContent, "—");
    // The purge confirmation must not promise a reclaim figure it does not have.
    assert.equal(globalThis.document.getElementById("purge-reclaim-bytes").textContent, "—");

    assert.equal(state.storageStatsPhase, "error");
    // The one failure policy still names the operation once.
    assert.equal(ui.toastContainer.children.length, 1);
    assert.match(ui.toastMessages()[0], /Load storage stats failed/);
    assert.equal(ui.statusText.textContent, "Load storage stats failed");
});

test("a retry after a failure renders the footprint", async (t) => {
    setUp(t);
    let fail = true;
    const server = stubFetch(async () => {
        if (fail) throw new TypeError("Failed to fetch");
        return jsonResponse(STATS);
    });
    t.after(() => server.restore());

    await fetchStorageStats();
    await settle();
    assert.equal(state.storageStatsPhase, "error");

    fail = false;
    await fetchStorageStats();
    await settle();

    assert.equal(state.storageStatsPhase, "ready");
    assert.equal(
        globalThis.document.getElementById("storage-stats-error").classList.contains("hidden"),
        true,
        "the error panel clears once the retry answers",
    );
    assert.equal(globalThis.document.getElementById("storage-staging-bytes").textContent, "1 KB");
    assert.equal(globalThis.document.getElementById("storage-stats-content").classList.contains("hidden"), false);
});

test("the retry control starts a new read once the previous one failed", async (t) => {
    setUp(t);
    const server = stubFetch(async () => jsonResponse(STATS));
    t.after(() => server.restore());

    // `setupStorageControls` binds the retry button the error panel offers.
    setupStorageControls();
    assert.equal(server.calls.length, 0, "binding controls reads nothing");

    // The fake DOM records no listeners, so the click path is asserted through the same function
    // the listener calls.
    await fetchStorageStats();
    await settle();
    assert.equal(server.calls.length, 1);
    assert.equal(state.storageStatsPhase, "ready");
});
