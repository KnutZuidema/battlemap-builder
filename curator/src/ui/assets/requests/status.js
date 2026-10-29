/**
 * The server status request behind the connection pill.
 */

import { requestJson } from "./http.js";

/** Queue and library counts for the pill, plus the liveness the pill reports. */
export function fetchServerStatus({ signal } = {}) {
    return requestJson("/api/status", { signal, operation: "Check server status" });
}
