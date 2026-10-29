/**
 * Background polling failure policy, at the request-module boundary the polls use.
 *
 * A poll that fails every tick is one failing operation, not one event per attempt: the pill
 * names it, one toast announces the transition, and the attempts after it stay quiet. The
 * training poll asks two endpoints in the same tick, so an answer that succeeds must not clear
 * the other endpoint's failure and turn every tick into a fresh toast.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { clearRequestFailure } from "../assets/requests/failures.js";
import { fetchFreezeStatus } from "../assets/requests/snapshots.js";
import { fetchTrainingRuns } from "../assets/requests/training.js";
import { auditDatasetReadiness } from "../assets/requests/readiness.js";
import { fetchCostStats } from "../assets/views/benchmark.js";
import { installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();
    t.after(() => {
        clearRequestFailure();
        ui.restore();
    });
    return ui;
}

function serverError(message = "upstream unavailable") {
    return jsonResponse({ error: message, code: "upstream_failed" }, { status: 502, statusText: "Bad Gateway" });
}

/** Let every settled promise chain drain (a poll awaits a couple of requests). */
async function settle() {
    for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

test("three failing freeze-status polls raise the pill once and toast once", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () => serverError("freeze status unavailable"));
    t.after(() => server.restore());

    for (let attempt = 0; attempt < 3; attempt += 1) {
        await assert.rejects(fetchFreezeStatus(), /freeze status unavailable/);
        await settle();
    }

    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Check freeze status failed");
    assert.match(ui.pill.title, /Check freeze status/);
    assert.match(ui.pill.title, /upstream_failed/);
    assert.equal(ui.toastContainer.children.length, 1, "one transition, one toast");
});

test("a poll's recovery restores the pill, and the next failure is a new transition", async (t) => {
    const ui = setUp(t);
    let fail = true;
    const server = stubFetch(async () => (fail ? serverError() : jsonResponse({ in_progress: false })));
    t.after(() => server.restore());

    await assert.rejects(fetchFreezeStatus());
    await settle();
    assert.equal(ui.toastContainer.children.length, 1);

    fail = false;
    const status = await fetchFreezeStatus();
    await settle();
    assert.equal(status.in_progress, false);
    assert.equal(ui.pill.className, "status-pill status-connected");
    assert.equal(ui.statusText.textContent, "Online (2)");

    fail = true;
    await assert.rejects(fetchFreezeStatus());
    await settle();
    assert.equal(ui.toastContainer.children.length, 2, "the recovered failure toasts again");
});

test("the training poll's failing endpoint stays quiet while the other answer succeeds", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async (path) => {
        if (path === "/api/training/runs") throw new TypeError("Failed to fetch");
        return jsonResponse({ session_spend: 0.1, total_spend_usd: 0.2, total_generations: 3, training_runs: 2 });
    });
    t.after(() => server.restore());

    // Three ticks of `pollTrainingState`: the runs listing fails, the cost telemetry answers.
    for (let tick = 0; tick < 3; tick += 1) {
        await assert.rejects(fetchTrainingRuns(), /Could not reach \/api\/training\/runs/);
        await fetchCostStats();
        await settle();
    }

    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Load training runs failed", "the failing endpoint still names the pill");
    assert.equal(ui.toastContainer.children.length, 1, "the succeeding endpoint does not re-arm the toast");
});

test("each polling operation is its own transition", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async (path) => {
        if (path.startsWith("/api/dataset/readiness")) {
            throw new TypeError("Failed to fetch");
        }
        return serverError();
    });
    t.after(() => server.restore());

    await assert.rejects(fetchFreezeStatus());
    await settle();
    await assert.rejects(auditDatasetReadiness());
    await settle();
    // A second attempt at each: both failures are already recorded.
    await assert.rejects(fetchFreezeStatus());
    await assert.rejects(auditDatasetReadiness());
    await settle();

    assert.equal(ui.toastContainer.children.length, 2, "one transition per failing operation");
    assert.equal(ui.statusText.textContent, "Audit dataset readiness failed");
});
