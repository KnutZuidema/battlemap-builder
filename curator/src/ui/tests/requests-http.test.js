import test from "node:test";
import assert from "node:assert/strict";

import { RequestError, requestJson } from "../assets/requests/http.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

function enabledTimers(t) {
    // Toasts schedule their own removal; the tests must not wait for those timers.
    t.mock.timers.enable({ apis: ["setTimeout"] });
}

test("a 200 JSON body resolves with the parsed payload", async (t) => {
    enabledTimers(t);
    const ui = installRequestDom();
    const server = stubFetch(async () => jsonResponse({ description: "vaulted hall" }));
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    const payload = await requestJson("/api/curated/m1/describe", {
        method: "POST",
        body: { tags: ["dungeon"] },
        operation: "Describe curated map",
    });

    assert.deepEqual(payload, { description: "vaulted hall" });
    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].path, "/api/curated/m1/describe");
    assert.equal(server.calls[0].init.method, "POST");
    assert.deepEqual(JSON.parse(server.calls[0].init.body), { tags: ["dungeon"] });
    assert.equal(ui.toastContainer.children.length, 0, "a success raises no toast");
});

test("a non-OK response becomes a RequestError carrying status, code, debug, path and operation", async (t) => {
    enabledTimers(t);
    const ui = installRequestDom();
    const server = stubFetch(async () =>
        jsonResponse(
            { error: "candidate has no ground layer", code: "invalid_payload", debug: "trace-abc" },
            { status: 400, statusText: "Bad Request" },
        ),
    );
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    await assert.rejects(
        requestJson("/api/curate", { method: "POST", body: { id: 12 }, operation: "Curate candidate" }),
        (error) => {
            assert.ok(error instanceof RequestError, "the envelope is a RequestError");
            assert.equal(error.name, "RequestError");
            assert.equal(error.message, "candidate has no ground layer");
            assert.equal(error.status, 400);
            assert.equal(error.code, "invalid_payload");
            assert.equal(error.debug, "trace-abc");
            assert.equal(error.path, "/api/curate");
            assert.equal(error.operation, "Curate candidate");
            return true;
        },
    );

    // The one failure policy saw it: pill raised, one toast fired.
    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Curate candidate failed");
    assert.match(ui.pill.title, /invalid_payload/);
    assert.equal(ui.toastContainer.children.length, 1);
});

test("a non-JSON error body yields a readable status-line message", async (t) => {
    enabledTimers(t);
    const ui = installRequestDom();
    const server = stubFetch(async () => ({
        ok: false,
        status: 502,
        statusText: "Bad Gateway",
        json: async () => {
            throw new SyntaxError("Unexpected token '<'");
        },
    }));
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    await assert.rejects(
        requestJson("/api/curated/m1/describe", { method: "POST", body: {}, operation: "Describe curated map" }),
        (error) => {
            assert.ok(error instanceof RequestError);
            assert.equal(error.message, "HTTP 502 Bad Gateway");
            assert.equal(error.status, 502);
            assert.equal(error.code, undefined);
            assert.equal(error.debug, undefined);
            return true;
        },
    );
    assert.equal(ui.toastContainer.children.length, 1);
});

test("a signal abort propagates untouched and is not reported as a failure", async (t) => {
    enabledTimers(t);
    const ui = installRequestDom();
    const controller = new AbortController();
    const abortError = Object.assign(new Error("The operation was aborted."), { name: "AbortError" });
    const server = stubFetch(async () => {
        controller.abort();
        throw abortError;
    });
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    await assert.rejects(
        requestJson("/api/staging/c1/describe", {
            method: "POST",
            body: {},
            signal: controller.signal,
            operation: "Describe staging candidate",
        }),
        (error) => {
            assert.equal(error.name, "AbortError");
            assert.equal(error instanceof RequestError, false, "an abort is not mapped onto the envelope");
            return true;
        },
    );

    assert.equal(ui.toastContainer.children.length, 0, "a cancelled request raises no toast");
    assert.equal(ui.pill.className, "status-pill status-connected");
});

test("a network failure becomes a RequestError naming the path", async (t) => {
    enabledTimers(t);
    const ui = installRequestDom();
    const server = stubFetch(async () => {
        throw new TypeError("Failed to fetch");
    });
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });

    await assert.rejects(
        requestJson("/api/curated/m1/suggest-themes", { method: "POST", operation: "Suggest curated themes" }),
        (error) => {
            assert.ok(error instanceof RequestError);
            assert.equal(error.message, "Could not reach /api/curated/m1/suggest-themes: Failed to fetch");
            assert.equal(error.status, undefined);
            assert.equal(error.path, "/api/curated/m1/suggest-themes");
            return true;
        },
    );
    assert.equal(ui.toastContainer.children.length, 1);
});
