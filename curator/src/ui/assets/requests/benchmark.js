/**
 * The benchmark requests: the spend telemetry the HUD and the drawer show, the selectable
 * model catalog, the streamed batch run, and the cancellation of one.
 *
 * The run's own progress events (`progress`, `complete`, `cancelled`, `error`) are the view's
 * to interpret; this module only carries them.
 */

import { requestJson } from "./http.js";
import { requestEventStream } from "./stream.js";

/** Spend telemetry: session and lifetime spend, per-generation and per-training totals. */
export function fetchCostStats({ signal } = {}) {
    return requestJson("/api/cost/stats", { signal, operation: "Load cost stats" });
}

/** The selectable benchmark models (ADR-0010): identity, price and provider from the registry. */
export function fetchBenchmarkModels({ signal } = {}) {
    return requestJson("/api/models", { signal, operation: "Load model catalog" });
}

/**
 * Run a benchmark batch.
 *
 * With an event stream the progress arrives through `onEvent` and the call resolves with
 * `undefined`; an endpoint answering in one shot resolves with the batch summary. Aborting
 * `signal` cancels the run's transport and is not reported as a failure.
 */
export function runBenchmark(payload, { onEvent, signal } = {}) {
    return requestEventStream("/api/benchmark/run", {
        method: "POST",
        body: payload,
        signal,
        onEvent,
        operation: "Run benchmark",
    });
}

/** Ask the server to stop the running benchmark batch. */
export function cancelBenchmarkRun({ signal } = {}) {
    return requestJson("/api/benchmark/cancel", {
        method: "POST",
        signal,
        operation: "Cancel benchmark run",
    });
}
