/**
 * The storage footprint requests: the stats the storage drawer renders and the staging purge.
 *
 * The stats read is deduplicated through `tracker`: boot (`app.js`) and every user interaction
 * (drawer open, modal open, refresh, post-purge refresh) share one in-flight request per key
 * instead of each starting its own. A stats scan is the most expensive read the SPA issues, so
 * two concurrent ones are two full passes over the bucket — and issue #292 showed that on a
 * deployed bucket the redundancy is what turns a slow read into a starved container.
 */

import { requestJson } from "./http.js";
import { join } from "./tracker.js";

/** Tracker key for the stats read: one shared request for boot and every interaction. */
const STORAGE_STATS_KEY = "storage:stats";

/**
 * Staging and curated storage footprint, by source.
 *
 * Joins the in-flight stats request when one exists, so a click that lands while the page's boot
 * read is still running waits on that read instead of issuing a second full scan. The registry
 * drops a settled entry, so a later refresh starts a fresh request.
 */
export function fetchStorageStats({ signal } = {}) {
    return join(STORAGE_STATS_KEY, (trackedSignal) =>
        requestJson("/api/storage/stats", {
            // A caller's own signal (an abort on unmount) cancels the shared request too.
            signal: signal ? AbortSignal.any([trackedSignal, signal]) : trackedSignal,
            operation: "Load storage stats",
        }),
    );
}

/** Permanently delete every staging candidate; resolves with the purge summary. */
export function purgeStaging({ signal } = {}) {
    return requestJson("/api/storage/purge-staging", {
        method: "POST",
        signal,
        operation: "Purge staging",
    });
}
