/**
 * The staging decision requests: curate an accepted candidate, reject one, undo a rejection.
 */

import { requestJson } from "./http.js";

/** Curate one staging candidate with the tags, layers and description the reviewer accepted. */
export function curateCandidate(payload) {
    return requestJson("/api/curate", { method: "POST", body: payload, operation: "Curate candidate" });
}

/** Reject one staging candidate. */
export function rejectCandidate(id) {
    return requestJson("/api/reject", { method: "POST", body: { id }, operation: "Reject candidate" });
}

/** Undo a rejection, restoring the candidate to the staging queue. */
export function undoReject(id) {
    return requestJson("/api/reject/undo", { method: "POST", body: { id }, operation: "Undo rejection" });
}
