import test from "node:test";
import assert from "node:assert/strict";

import { abort, has, join, latest } from "../assets/requests/tracker.js";

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

/** A request that only settles when the tracker's signal aborts it. */
function abortable(signal) {
    return new Promise((resolve, reject) => {
        signal.addEventListener(
            "abort",
            () => {
                const error = new Error("request cancelled");
                error.name = "AbortError";
                reject(error);
            },
            { once: true },
        );
    });
}

test("join shares one promise per key and starts a fresh one after it settles", async () => {
    const first = deferred();
    let started = 0;
    const factory = () => {
        started += 1;
        return first.promise;
    };

    const a = join("describe:curated:m1", factory);
    const b = join("describe:curated:m1", factory);
    assert.equal(a, b, "a second caller reuses the in-flight promise");
    assert.equal(started, 1);

    first.resolve("done");
    assert.equal(await a, "done");
    await Promise.resolve();
    assert.equal(has("describe:curated:m1"), false, "a settled request leaves the registry");

    const c = join("describe:curated:m1", () => Promise.resolve("again"));
    assert.notEqual(c, a);
    assert.equal(await c, "again");
});

test("join drops a rejected request from the registry", async () => {
    const rejected = deferred();
    const failing = join("suggest:staging:c1", () => rejected.promise);
    rejected.reject(new Error("upstream 502"));
    await assert.rejects(failing, /upstream 502/);
    assert.equal(has("suggest:staging:c1"), false);

    const fresh = join("suggest:staging:c1", () => Promise.resolve(["cave"]));
    assert.notEqual(fresh, failing);
    assert.deepEqual(await fresh, ["cave"]);
});

test("latest cancels the superseded request with an abort error", async () => {
    const signals = [];
    const factory = (signal) => {
        signals.push(signal);
        return abortable(signal);
    };

    const superseded = latest("describe:staging:c1", factory);
    const replacement = latest("describe:staging:c1", factory);

    assert.notEqual(superseded, replacement);
    assert.equal(signals[0].aborted, true, "the older request's signal is aborted");

    // A cancellation surfaces as an abort error, which is what keeps it out of the failure
    // policy: `reportRequestFailure` ignores errors named AbortError.
    await assert.rejects(superseded, (error) => {
        assert.equal(error.name, "AbortError");
        return true;
    });
    assert.equal(has("describe:staging:c1"), true, "the replacement still owns the key");
});

test("a superseded request cannot remove the entry that replaced it", async () => {
    const first = deferred();
    const second = deferred();

    const a = join("describe:curated:m1", () => first.promise);
    abort("describe:curated:m1");
    const b = join("describe:curated:m1", () => second.promise);
    assert.notEqual(a, b);

    first.resolve("stale");
    await a;
    assert.equal(has("describe:curated:m1"), true);

    second.resolve("fresh");
    assert.equal(await b, "fresh");
    await Promise.resolve();
    assert.equal(has("describe:curated:m1"), false);
});

test("abort forgets a registered request and an unknown key is a no-op", async () => {
    let signal;
    const pending = join("suggest:curated:m1", (s) => {
        signal = s;
        return abortable(s);
    });
    assert.equal(has("suggest:curated:m1"), true);

    abort("suggest:curated:m1");
    assert.equal(has("suggest:curated:m1"), false);
    assert.equal(signal.aborted, true, "abort cancels the request");
    await assert.rejects(pending, (error) => error.name === "AbortError");
    assert.equal(has("suggest:curated:never-started"), false);
    abort("suggest:curated:never-started");
});
