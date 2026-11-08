/**
 * The storage footprint requests: the stats the storage drawer renders and the staging purge.
 *
 * The stats read is deduplicated through `tracker`: boot (`app.js`) and every user interaction
 * (drawer open, refresh, post-purge refresh) join one in-flight request instead of each starting
 * its own. A footprint scan is the most expensive read the SPA issues, so two concurrent ones are
 * two full passes over the bucket — and #292 showed that on a deployed bucket that redundancy is
 * exactly what turns a slow read into a starved container.
 */

import { requestJson } from "./http.js";
import { join } from "./tracker.js";

/** Tracker key for the stats read: one shared request for boot and every interaction. */
const STORAGE_STATS_KEY = "storage:stats";

/**
 * Staging and curated storage footprint, by source.
 *
 * Joins the in-flight stats request when one exists, so a click that lands while the page's boot
 * read is still running waits on that read instead of issuing a second full scan. The shared
 * request carries the tracker's signal rather than a caller's: it outlives any single caller, so
 * one caller walking away must not cancel a read another is waiting for. The registry drops a
 * settled entry, so a later refresh starts a fresh request.
 */
export function fetchStorageStats() {
    return join(STORAGE_STATS_KEY, (signal) =>
        requestJson("/api/storage/stats", { signal, operation: "Load storage stats" }),
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
