/**
 * The one event-stream reader, at the `requests/stream.js` boundary.
 *
 * The three streamed endpoints (harvest, benchmark, freeze) share this reader, so its framing
 * rules are what keeps them agreeing: frames split across network chunks are buffered, a
 * frame's `event:` name reaches the caller, a non-OK answer is a `RequestError` before any
 * event is dispatched, and a cancelled stream is not a failure.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { RequestError } from "../assets/requests/http.js";
import { requestEventStream } from "../assets/requests/stream.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import {
    eventStreamResponse,
    installRequestDom,
    jsonResponse,
    stubFetch,
} from "./support/fakes.js";

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();
    t.after(() => {
        clearRequestFailure();
        ui.restore();
    });
    return ui;
}

function abortError() {
    return Object.assign(new Error("The operation was aborted."), { name: "AbortError" });
}

test("every event in the stream reaches onEvent with its name, in order", async (t) => {
    const ui = setUp(t);
    const response = eventStreamResponse([
        "event: progress\ndata: {\"processed\":1,\"total\":3}\n\n",
        "event: progress\ndata: {\"processed\":2,\"total\":3}\n\n",
        "event: complete\ndata: {\"processed\":3,\"total\":3}\n\n",
    ]);
    const server = stubFetch(async () => response);
    t.after(() => server.restore());

    const events = [];
    const summary = await requestEventStream("/api/harvest/reddit", {
        body: { limit: 3 },
        operation: "Harvest Reddit maps",
        onEvent: (event, data) => events.push([event, data]),
    });

    assert.equal(summary, undefined, "a streamed request resolves without a one-shot payload");
    assert.deepEqual(events, [
        ["progress", { processed: 1, total: 3 }],
        ["progress", { processed: 2, total: 3 }],
        ["complete", { processed: 3, total: 3 }],
    ]);
    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].init.method, "POST");
    assert.equal(server.calls[0].init.headers.Accept, "text/event-stream");
    assert.equal(server.calls[0].init.headers["Content-Type"], "application/json");
    assert.deepEqual(JSON.parse(server.calls[0].init.body), { limit: 3 });
    assert.equal(ui.toastContainer.children.length, 0, "a completed stream raises no toast");
});

test("a frame split across two chunks is buffered until its line arrives", async (t) => {
    setUp(t);
    const server = stubFetch(async () =>
        eventStreamResponse([
            "event: progress\ndata: {\"processed\":4,",
            "\"total\":9}\n\nevent: complete\ndata: {\"done\":true}\n",
        ]),
    );
    t.after(() => server.restore());

    const events = [];
    await requestEventStream("/api/benchmark/run", {
        operation: "Run benchmark",
        onEvent: (event, data) => events.push([event, data]),
    });

    assert.deepEqual(events, [
        ["progress", { processed: 4, total: 9 }],
        ["complete", { done: true }],
    ]);
});

test("an error frame is handed to onEvent so the view can render its refusal", async (t) => {
    setUp(t);
    const refusal = { phase: "error", error: "Dataset is not ready", code: "DATASET_NOT_READY" };
    const server = stubFetch(async () =>
        eventStreamResponse([`event: error\ndata: ${JSON.stringify(refusal)}\n\n`]),
    );
    t.after(() => server.restore());

    const events = [];
    await requestEventStream("/api/training/datasets", {
        operation: "Freeze dataset snapshot",
        onEvent: (event, data) => events.push([event, data]),
    });

    assert.deepEqual(events, [["error", refusal]]);
});

test("a non-OK answer is a reported RequestError before any event is dispatched", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () =>
        jsonResponse(
            { error: "At least one model must be selected", code: "invalid_payload", debug: "trace-9" },
            { status: 400, statusText: "Bad Request" },
        ),
    );
    t.after(() => server.restore());

    const events = [];
    await assert.rejects(
        requestEventStream("/api/benchmark/run", {
            body: { models: [] },
            operation: "Run benchmark",
            onEvent: (event, data) => events.push([event, data]),
        }),
        (error) => {
            assert.ok(error instanceof RequestError);
            assert.equal(error.status, 400);
            assert.equal(error.code, "invalid_payload");
            assert.equal(error.operation, "Run benchmark");
            assert.deepEqual(error.payload, {
                error: "At least one model must be selected",
                code: "invalid_payload",
                debug: "trace-9",
            });
            return true;
        },
    );

    assert.deepEqual(events, [], "the failure arrives before any event");
    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Run benchmark failed");
    assert.equal(ui.toastContainer.children.length, 1);
});

test("a one-shot JSON answer resolves with the summary instead of reading a stream", async (t) => {
    setUp(t);
    const summary = { completed: 4, total: 4, total_cost_usd: 0.5 };
    const server = stubFetch(async () => jsonResponse(summary));
    t.after(() => server.restore());

    const events = [];
    const payload = await requestEventStream("/api/benchmark/run", {
        operation: "Run benchmark",
        onEvent: (event, data) => events.push([event, data]),
    });

    assert.deepEqual(payload, summary);
    assert.deepEqual(events, []);
});

test("an aborted stream rejects untouched and raises no failure", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () => {
        throw abortError();
    });
    t.after(() => server.restore());

    const controller = new AbortController();
    await assert.rejects(
        requestEventStream("/api/harvest/reddit", {
            body: { limit: 5 },
            signal: controller.signal,
            operation: "Harvest Reddit maps",
            onEvent: () => {},
        }),
        (error) => error.name === "AbortError",
    );

    assert.equal(ui.pill.className, "status-pill status-connected", "an abort leaves the pill alone");
    assert.equal(ui.toastContainer.children.length, 0);
});

test("an abort mid-stream stops the body and raises no failure", async (t) => {
    const ui = setUp(t);
    let read = 0;
    let cancelled = false;
    const response = eventStreamResponse([]);
    // A body whose second read rejects the way an aborted fetch does.
    response.body.getReader = () => ({
        async read() {
            read += 1;
            if (read === 1) {
                return { done: false, value: new TextEncoder().encode("event: progress\ndata: {\"processed\":1}\n\n") };
            }
            throw abortError();
        },
        async cancel() {
            cancelled = true;
        },
        releaseLock() {},
    });
    const server = stubFetch(async () => response);
    t.after(() => server.restore());

    const events = [];
    await assert.rejects(
        requestEventStream("/api/training/datasets", {
            operation: "Freeze dataset snapshot",
            onEvent: (event, data) => events.push([event, data]),
        }),
        (error) => error.name === "AbortError",
    );

    assert.equal(cancelled, true, "the unread body is stopped");
    assert.deepEqual(events, [["progress", { processed: 1 }]]);
    assert.equal(ui.pill.className, "status-pill status-connected");
    assert.equal(ui.toastContainer.children.length, 0);
});

test("a data line that is not JSON fails the request instead of vanishing", async (t) => {
    const ui = setUp(t);
    const server = stubFetch(async () =>
        eventStreamResponse(["event: progress\ndata: not-json\n\n"]),
    );
    t.after(() => server.restore());

    await assert.rejects(
        requestEventStream("/api/harvest/reddit", {
            operation: "Harvest Reddit maps",
            onEvent: () => {},
        }),
        (error) => {
            assert.ok(error instanceof RequestError);
            assert.match(error.message, /Invalid event from \/api\/harvest\/reddit/);
            return true;
        },
    );

    assert.equal(ui.statusText.textContent, "Harvest Reddit maps failed");
    assert.equal(ui.toastContainer.children.length, 1);
});
