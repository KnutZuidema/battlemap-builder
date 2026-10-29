import test from "node:test";
import assert from "node:assert/strict";

import { clearRequestFailure, reportRequestFailure } from "../assets/requests/failures.js";
import { RequestError } from "../assets/requests/http.js";
import { fetchStatus } from "../assets/shell/status.js";
import { installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

function serverError(operation, { status = 502, code = "upstream_failed", debug = "trace-1" } = {}) {
    return new RequestError("upstream model unavailable", {
        status,
        code,
        debug,
        path: "/api/curated/m1/describe",
        operation,
    });
}

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();
    t.after(() => {
        clearRequestFailure();
        ui.restore();
    });
    return ui;
}

test("the first failure raises the pill, names the operation and toasts once", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");

    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Describe curated map failed");
    assert.match(ui.pill.title, /upstream_failed/, "the pill title carries the server code");
    assert.match(ui.pill.title, /upstream model unavailable/);
    assert.match(ui.pill.title, /trace-1/, "the pill title carries the debug hint");
    assert.equal(ui.toastContainer.children.length, 1);
    assert.match(ui.toastMessages()[0], /Describe curated map failed \(upstream_failed\)/);
});

test("without a server code the pill title falls back to the HTTP status", (t) => {
    const ui = setUp(t);

    reportRequestFailure(
        new RequestError("no such map", { status: 404, path: "/api/reject", operation: "Reject candidate" }),
        "Reject candidate",
    );

    assert.match(ui.pill.title, /HTTP 404/, "the pill title carries the status");
    assert.equal(ui.toastMessages().length, 1);
    assert.match(ui.toastMessages()[0], /Reject candidate failed \(HTTP 404\)/);
});

test("a repeated identical failure while already failed does not toast again", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Suggest curated themes"), "Suggest curated themes");
    reportRequestFailure(serverError("Suggest curated themes"), "Suggest curated themes");
    reportRequestFailure(serverError("Suggest curated themes", { code: "timeout" }), "Suggest curated themes");

    assert.equal(ui.toastContainer.children.length, 1, "one transition, one toast");
    assert.equal(ui.pill.className, "status-pill status-error");
});

test("a different operation is a new transition", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");
    reportRequestFailure(serverError("Suggest curated themes", { status: 500, code: "model_error" }), "Suggest curated themes");

    assert.equal(ui.toastContainer.children.length, 2);
    assert.equal(ui.statusText.textContent, "Suggest curated themes failed");
    assert.match(ui.pill.title, /model_error/);
});

test("a success clears the failure, restores the pill and lets the next failure toast again", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");
    assert.equal(ui.pill.className, "status-pill status-error");

    clearRequestFailure();

    assert.equal(ui.pill.className, "status-pill status-connected");
    assert.equal(ui.pill.title, "Curator API Online | Staging: 2 | Curated: 5");
    assert.equal(ui.statusText.textContent, "Online (2)");

    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");
    assert.equal(ui.toastContainer.children.length, 2, "the same failure is a new transition after a success");
});

test("an aborted request is not a failure", (t) => {
    const ui = setUp(t);
    const aborted = Object.assign(new Error("The operation was aborted."), { name: "AbortError" });

    reportRequestFailure(aborted, "Describe staging candidate");

    assert.equal(ui.toastContainer.children.length, 0);
    assert.equal(ui.pill.className, "status-pill status-connected");
});

test("a status refresh cannot repaint the pill behind an active failure", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () => jsonResponse({ staging_count: 3, curated_count: 7 }));
    t.after(() => server.restore());

    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");
    assert.equal(ui.toastContainer.children.length, 1);

    await fetchStatus();
    assert.equal(ui.pill.className, "status-pill status-connected");
    assert.equal(ui.statusText.textContent, "Online (3)");

    // The status probe answered, so every recorded failure is forgotten rather than left active
    // behind a connected pill: the next identical failure is a new transition and toasts.
    reportRequestFailure(serverError("Describe curated map"), "Describe curated map");
    assert.equal(ui.toastContainer.children.length, 2, "the refresh cleared the recorded failure");
    assert.equal(ui.pill.className, "status-pill status-error");
});

test("another operation's success does not clear the failure the pill names", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Load training runs"), "Load training runs");
    assert.equal(ui.toastContainer.children.length, 1);

    // The same poll tick loaded cost telemetry successfully; that answers for its own
    // operation, not for the listing that kept failing.
    clearRequestFailure("Load cost stats");

    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Load training runs failed");

    reportRequestFailure(serverError("Load training runs", { code: "timeout" }), "Load training runs");
    assert.equal(ui.toastContainer.children.length, 1, "a retry of the recorded failure stays quiet");
});

test("clearing one operation repaints the newest remaining failure", (t) => {
    const ui = setUp(t);

    reportRequestFailure(serverError("Load training runs"), "Load training runs");
    reportRequestFailure(serverError("Load cost stats", { code: "cost_down" }), "Load cost stats");
    assert.equal(ui.statusText.textContent, "Load cost stats failed");
    assert.equal(ui.toastContainer.children.length, 2);

    clearRequestFailure("Load cost stats");

    assert.equal(ui.pill.className, "status-pill status-error", "one failure is still outstanding");
    assert.equal(ui.statusText.textContent, "Load training runs failed");
    assert.match(ui.pill.title, /Load training runs/);

    clearRequestFailure("Load training runs");

    assert.equal(ui.pill.className, "status-pill status-connected", "all failures recovered");
    assert.equal(ui.statusText.textContent, "Online (2)");
});
