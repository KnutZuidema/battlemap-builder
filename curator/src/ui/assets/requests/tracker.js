/**
 * In-flight request registry.
 *
 * `join` shares one promise per key — a prefetch and the click that needs its result join the
 * same request. `latest` supersedes whatever was running for the key: the older request is
 * aborted and its promise rejects with an abort error, so a renderer that awaits it can tell
 * "cancelled" from "failed". Entries leave the registry when their promise settles, success or
 * rejection, so a finished request never satisfies the next one.
 *
 * The factory receives the entry's `AbortSignal`; honouring it is what makes `abort`/`latest`
 * cancel the network request instead of only the bookkeeping.
 */

/** key -> { promise, controller } */
const inFlight = new Map();

/** Start a request for `key`, or return the one already in flight for it. */
export function join(key, factory) {
    const existing = inFlight.get(key);
    if (existing) return existing.promise;
    return start(key, factory);
}

/** Start a request for `key`, aborting the one it replaces. */
export function latest(key, factory) {
    abort(key);
    return start(key, factory);
}

/** Abort and forget the request in flight for `key`. */
export function abort(key) {
    const entry = inFlight.get(key);
    if (!entry) return;
    inFlight.delete(key);
    entry.controller.abort();
}

/** Forget the request in flight for `key` without aborting it. */
export function clear(key) {
    inFlight.delete(key);
}

/** Whether a request is currently in flight for `key`. */
export function has(key) {
    return inFlight.has(key);
}

function start(key, factory) {
    const controller = new AbortController();
    let promise;
    try {
        promise = Promise.resolve(factory(controller.signal));
    } catch (error) {
        promise = Promise.reject(error);
    }

    const entry = { promise, controller };
    inFlight.set(key, entry);
    const forget = () => {
        // A newer entry for the same key owns the registry now: only the entry that is still
        // the registry's own may be removed by settling.
        if (inFlight.get(key) === entry) inFlight.delete(key);
    };
    promise.then(forget, forget);
    return promise;
}
