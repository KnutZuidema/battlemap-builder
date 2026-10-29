/**
 * The freeze refusal panel, at the `views/snapshots.js` boundary.
 *
 * The server reports the same refusal in two shapes: inside a 200 event stream an `error`
 * frame carries the fields directly, and the non-SSE branch answers 422 with
 * `{ code, error, blocking_issues, readiness }`, which the request module hands to the view as
 * a `RequestError` with that body in `payload`. Both must land the panel in the same state —
 * above all, both must store and render the per-map audit the refusal carries, since that
 * audit is the reason the refusal is rendered inline rather than toasted.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { showFreezeRefusal } from "../assets/views/snapshots.js";
import { RequestError } from "../assets/requests/http.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { element, installRequestDom } from "./support/fakes.js";

/** One refusal as `FreezeDatasetError::NotReady` serializes it (both response shapes share it). */
const REFUSAL = {
    error: "Dataset is not ready: 1 of 2 maps blocked",
    code: "DATASET_NOT_READY",
    blocking_issues: [{ code: "physical_description_missing", count: 1 }],
    readiness: {
        ready: false,
        maps_total: 2,
        maps_ready: 1,
        sections_total: 4,
        sections_ready: 2,
        maps: [{ id: "m1", issues: [{ code: "physical_description_missing" }] }],
    },
};

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();

    const stored = new Map();
    const previousStorage = globalThis.localStorage;
    globalThis.localStorage = {
        getItem: (key) => (stored.has(key) ? stored.get(key) : null),
        setItem: (key, value) => stored.set(key, String(value)),
        removeItem: (key) => stored.delete(key),
    };

    dom.datasetReadinessStatus = element("span");
    dom.datasetReadinessIcon = element("span");
    dom.datasetReadinessSummary = element("div");
    dom.datasetReadinessIssues = element("ul");
    dom.datasetReadinessPanel = element("div");
    // The refusal reveals the panel in place: the library view is already on screen.
    state.currentView = "library";
    state.libraryActiveTag = null;
    state.readinessScopeKey = "all";
    state.readinessReport = null;
    state.readinessAuditedAt = null;
    state.readinessRefusal = null;

    t.after(() => {
        clearRequestFailure();
        globalThis.localStorage = previousStorage;
        delete dom.datasetReadinessStatus;
        delete dom.datasetReadinessIcon;
        delete dom.datasetReadinessSummary;
        delete dom.datasetReadinessIssues;
        delete dom.datasetReadinessPanel;
        ui.restore();
    });

    return {
        reset() {
            stored.clear();
            state.readinessReport = null;
            state.readinessAuditedAt = null;
            state.readinessRefusal = null;
            dom.datasetReadinessStatus.textContent = "";
            dom.datasetReadinessIcon.textContent = "";
            dom.datasetReadinessIcon.className = "";
            dom.datasetReadinessSummary.innerHTML = "";
            dom.datasetReadinessIssues.innerHTML = "";
        },
        /** What the operator sees, plus the audit the refusal stored for the scope. */
        panelState() {
            return {
                status: dom.datasetReadinessStatus.textContent,
                refused: dom.datasetReadinessStatus.classList.contains("has-issues"),
                icon: dom.datasetReadinessIcon.textContent,
                summary: dom.datasetReadinessSummary.innerHTML,
                issues: dom.datasetReadinessIssues.innerHTML,
                report: state.readinessReport,
                audited: Boolean(state.readinessAuditedAt),
                refusal: state.readinessRefusal,
                audits: [...stored.values()].flatMap((raw) =>
                    Object.entries(JSON.parse(raw)).map(([scope, entry]) => [scope, entry.report]),
                ),
            };
        },
    };
}

test("an in-stream refusal and a 422 RequestError leave the panel in the same state", (t) => {
    const panel = setUp(t);

    showFreezeRefusal({ ...REFUSAL }, null, { toast: false });
    const streamed = panel.panelState();
    assert.notEqual(streamed.report, null, "the in-stream refusal's audit is rendered");
    assert.match(streamed.issues, /m1/, "the in-stream refusal lists the flagged map");
    assert.deepEqual(streamed.audits, [["all", REFUSAL.readiness]], "the audit is stored with the scope");

    panel.reset();

    const error = new RequestError(REFUSAL.error, {
        status: 422,
        code: REFUSAL.code,
        path: "/api/training/datasets?format=json",
        operation: "Freeze dataset snapshot",
        payload: { ...REFUSAL },
    });
    showFreezeRefusal(error, null, { toast: false });
    const refusedResponse = panel.panelState();

    assert.deepEqual(
        refusedResponse,
        streamed,
        "a refusal delivered in `RequestError.payload` renders and stores the same audit",
    );
    assert.notEqual(refusedResponse.report, null, "the 422 refusal does not clear the readiness cache");
    assert.match(refusedResponse.issues, /m1/, "the 422 refusal lists the flagged map");
});
