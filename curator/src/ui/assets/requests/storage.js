/**
 * The storage footprint requests: the stats the storage drawer renders and the staging purge.
 */

import { requestJson } from "./http.js";

/** Staging and curated storage footprint, by source. */
export function fetchStorageStats({ signal } = {}) {
    return requestJson("/api/storage/stats", { signal, operation: "Load storage stats" });
}

/** Permanently delete every staging candidate; resolves with the purge summary. */
export function purgeStaging({ signal } = {}) {
    return requestJson("/api/storage/purge-staging", {
        method: "POST",
        signal,
        operation: "Purge staging",
    });
}
