/**
 * The readiness batch's apply request, at the `views/readiness.js` boundary.
 *
 * The persisting repair must not share the tracker key of the inspector's apply-less describe:
 * `join` on a shared key would return the inspector's request without running the apply (no
 * persist, guard skipped, map reported as "reused"), and the inspector's `latest` would abort
 * the apply and land in the failed list. A cancellation is not a failed map either.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { describeInspectorMap } from "../assets/views/inspector.js";
import { setupReadinessControls } from "../assets/views/readiness.js";
import { abort, has } from "../assets/requests/tracker.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { element, installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

const DESCRIPTION_ISSUE = "physical_description_missing";

/** One map flagged for a missing Physical Description, as `/api/dataset/readiness` reports it. */
const flaggedReport = {
    ready: false,
    maps_total: 1,
    maps_ready: 0,
    sections_total: 1,
    sections_ready: 0,
    maps: [{ id: "m1", issues: [{ code: DESCRIPTION_ISSUE }] }],
};

const cleanReport = {
    ready: true,
    maps_total: 1,
    maps_ready: 1,
    sections_total: 1,
    sections_ready: 1,
    maps: [{ id: "m1", issues: [] }],
};

function abortError() {
    return Object.assign(new Error("The operation was aborted."), { name: "AbortError" });
}

/** Let every already-settled promise chain drain (the batch awaits a handful of them). */
async function settle() {
    for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();

    dom.datasetReadinessSummary = element("div");
    const issues = element("ul");
    let onClick = null;
    issues.addEventListener = (type, handler) => {
        if (type === "click") onClick = handler;
    };
    dom.datasetReadinessIssues = issues;
    setupReadinessControls();

    state.currentView = "triage";
    state.readinessReport = flaggedReport;
    state.readinessAuditedAt = new Date().toISOString();
    state.readinessScopeKey = "all";
    state.readinessRefusal = null;
    state.readinessAudit = null;
    state.readinessAuditEpoch = -1;
    state.readinessAuditScope = null;
    state.readinessBatch = null;
    state.readinessEpoch = 0;
    state.libraryDrawerActive = false;
    state.libraryDrawerMap = null;
    state.libraryDraft = null;

    t.after(() => {
        clearRequestFailure();
        state.readinessBatch = null;
        state.libraryDrawerMap = null;
        state.libraryDraft = null;
        delete dom.datasetReadinessSummary;
        delete dom.datasetReadinessIssues;
        ui.restore();
    });

    // A click on the "Fix descriptions" row, through the real control wiring.
    return {
        ui,
        clickFix(code = DESCRIPTION_ISSUE) {
            onClick({
                target: { closest: (selector) => (selector === "[data-fix]" ? { dataset: { issueCode: code } } : null) },
                preventDefault() {},
            });
        },
    };
}

function describeCalls(server) {
    return server.calls
        .filter((call) => call.path === "/api/curated/m1/describe")
        .map((call) => JSON.parse(call.init.body || "{}"));
}

test("an in-flight inspector describe is not joined by the batch, which sends its own apply", async (t) => {
    const { ui, clickFix } = setUp(t);
    let releaseInspector;
    const inspectorResponse = new Promise((resolve) => {
        releaseInspector = () => resolve(jsonResponse({ description: "inspector draft" }));
    });

    const server = stubFetch((path, init) => {
        if (path === "/api/curated/m1/describe") {
            const body = JSON.parse(init.body || "{}");
            if (body.apply) return jsonResponse({ description: "persisted", applied: true });
            return inspectorResponse;
        }
        if (path.startsWith("/api/dataset/readiness")) return jsonResponse(cleanReport);
        return jsonResponse({});
    });
    t.after(() => server.restore());

    state.libraryDrawerMap = { id: "m1" };
    state.libraryDraft = { tags: ["dungeon"] };
    const inspector = describeInspectorMap();
    assert.deepEqual(describeCalls(server), [{ tags: ["dungeon"] }], "the inspector's describe is in flight");

    clickFix();
    releaseInspector();
    await inspector;
    await settle();

    assert.deepEqual(
        describeCalls(server),
        [{ tags: ["dungeon"] }, { apply: true }],
        "the batch sends an apply of its own instead of reusing the inspector's request",
    );
    assert.deepEqual(ui.toastMessages(), ["Fixed 1 description"], "no regeneration is reported as reused");
});

test("an inspector describe started during the batch does not abort the apply", async (t) => {
    const { ui, clickFix } = setUp(t);
    let releaseApply;
    let applySignal = null;
    const applyResponse = new Promise((resolve) => {
        releaseApply = () => resolve(jsonResponse({ description: "persisted", applied: true }));
    });

    const server = stubFetch((path, init) => {
        if (path === "/api/curated/m1/describe") {
            const body = JSON.parse(init.body || "{}");
            if (body.apply) {
                applySignal = init.signal;
                return applyResponse;
            }
            return jsonResponse({ description: "inspector draft" });
        }
        if (path.startsWith("/api/dataset/readiness")) return jsonResponse(cleanReport);
        return jsonResponse({});
    });
    t.after(() => server.restore());

    clickFix();
    assert.deepEqual(describeCalls(server), [{ apply: true }], "the batch's apply is in flight");

    state.libraryDrawerMap = { id: "m1" };
    state.libraryDraft = { tags: ["dungeon"] };
    const inspector = describeInspectorMap();
    assert.equal(applySignal.aborted, false, "the inspector's latest does not cancel the batch's key");

    releaseApply();
    await inspector;
    await settle();

    assert.deepEqual(
        describeCalls(server),
        [{ apply: true }, { tags: ["dungeon"] }],
        "each flow sends its own request",
    );
    assert.deepEqual(ui.toastMessages(), ["Fixed 1 description"], "the apply counts as fixed");
});

test("an aborted apply is a cancellation, not a failed map", async (t) => {
    const { ui, clickFix } = setUp(t);
    let describeSignal = null;

    const server = stubFetch((path, init) => {
        if (path === "/api/curated/m1/describe") {
            describeSignal = init.signal;
            return new Promise((resolve, reject) => {
                init.signal.addEventListener("abort", () => reject(abortError()));
            });
        }
        if (path.startsWith("/api/dataset/readiness")) return jsonResponse(flaggedReport);
        return jsonResponse({});
    });
    t.after(() => server.restore());

    clickFix();
    assert.equal(describeCalls(server).length, 1, "the batch's apply is in flight");
    assert.equal(has("describe:curated:m1"), false, "the inspector's key owns no batch request");

    abort("describe:curated:m1:apply");
    assert.equal(describeSignal.aborted, true, "the batch owns the apply key");
    await settle();

    assert.equal(state.readinessBatch, null, "the batch finished");
    assert.deepEqual(
        ui.toastMessages().filter((message) => /failed/i.test(message)),
        [],
        "a cancellation is not reported as a failed map",
    );
    assert.deepEqual(
        ui.toastMessages(),
        ["1 map(s) still fail the description audit"],
        "the re-audit names the map that is still unpersisted",
    );
    assert.equal(ui.pill.className, "status-pill status-connected", "a cancellation leaves the pill alone");
});
