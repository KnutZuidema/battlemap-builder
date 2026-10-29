/**
 * The frozen dataset snapshot requests: the shelf listing, the freeze itself (streamed, with a
 * one-shot JSON answer when the caller cannot stream), the freeze-status poll every tab reads,
 * and the deletion of one snapshot.
 */

import { requestJson } from "./http.js";
import { requestEventStream } from "./stream.js";
import { datasetScopeQuery } from "../dataset-scope.js";

/** The frozen snapshots in the active dataset scope. */
export function fetchDatasetSnapshots({ signal } = {}) {
    return requestJson(`/api/training/datasets${datasetScopeQuery()}`, {
        signal,
        operation: "Load dataset snapshots",
    });
}

/**
 * Freeze the current dataset scope into a snapshot.
 *
 * With `onEvent` the server streams audit and freeze progress (`progress`, then `complete`, or
 * an `error` frame the caller renders as its refusal); without a body it answers once with the
 * finished snapshot, which this resolves with. A streamed freeze resolves with `undefined`, so
 * the caller takes the snapshot from the `complete` event.
 */
export function freezeDatasetSnapshot(scopeTheme, { onEvent, signal } = {}) {
    const query = scopeTheme ? `?theme=${encodeURIComponent(scopeTheme)}` : "";
    return requestEventStream(`/api/training/datasets${query}`, {
        method: "POST",
        body: scopeTheme ? { theme: scopeTheme } : {},
        signal,
        onEvent,
        operation: "Freeze dataset snapshot",
    });
}

/**
 * The freeze every tab polls for: `{ in_progress, progress }`.
 *
 * This is a background poll, so a failure names its operation once: the failure policy keeps
 * the pill raised and the toast to the transition, not to every attempt.
 */
export function fetchFreezeStatus({ signal } = {}) {
    return requestJson("/api/training/datasets/freeze/status", {
        signal,
        operation: "Check freeze status",
    });
}

/** Delete one frozen snapshot. A snapshot a training run still uses answers 409. */
export function deleteDatasetSnapshot(snapshotId, { signal } = {}) {
    return requestJson(`/api/training/datasets/${encodeURIComponent(snapshotId)}`, {
        method: "DELETE",
        signal,
        operation: "Delete dataset snapshot",
    });
}
