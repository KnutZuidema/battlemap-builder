/**
 * The staging queue requests: the candidate listing, a bundle split or regroup, and the two
 * PATCHes a triage candidate carries (layer metadata and Grid Configuration).
 */

import { requestJson } from "./http.js";

function candidatePath(candidateId, suffix) {
    return `/api/staging/${encodeURIComponent(candidateId)}/${suffix}`;
}

/** The current staging queue. */
export function fetchStagingQueue({ signal } = {}) {
    return requestJson("/api/staging", { signal, operation: "Load staging queue" });
}

/** Persist one candidate's layer roles, elevations and captions. */
export function saveStagingLayers(candidateId, layers, { signal } = {}) {
    return requestJson(candidatePath(candidateId, "layers"), {
        method: "PATCH",
        body: { layers },
        signal,
        operation: "Save staging layers",
    });
}

/** Split a multi-layer bundle into standalone candidates; resolves with the split candidates. */
export function splitStagingBundle(candidateId, { signal } = {}) {
    return requestJson(candidatePath(candidateId, "split"), {
        method: "POST",
        signal,
        operation: "Split staging bundle",
    });
}

/** Recombine the split children of a bundle; resolves with the regrouped candidate. */
export function regroupStagingBundle(parentId, { signal } = {}) {
    return requestJson(candidatePath(parentId, "regroup"), {
        method: "POST",
        signal,
        operation: "Regroup staging bundle",
    });
}

/** Persist one candidate's Grid Configuration (`{ cell_size_px, offset_x, offset_y }`). */
export function saveStagingGrid(candidateId, grid, { signal } = {}) {
    return requestJson(candidatePath(candidateId, "grid"), {
        method: "PATCH",
        body: grid,
        signal,
        operation: "Save staging grid",
    });
}
