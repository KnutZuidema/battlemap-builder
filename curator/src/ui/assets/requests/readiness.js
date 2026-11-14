/**
 * The dataset readiness audit request.
 *
 * Every readiness panel reads the same scoped audit: the readiness view, the training launch
 * gate and the freeze refusal all ask the same question, so they ask it the same way.
 */

import { requestJson } from "./http.js";
import { datasetScopeQuery } from "../dataset-scope.js";

/**
 * Audit the curated maps in the active dataset scope.
 *
 * The server reuses its persisted per-map audits and only audits maps that are new or changed;
 * `refresh` forces a full re-audit of the scope (the panel's explicit "Re-audit").
 */
export function auditDatasetReadiness({ signal, refresh = false } = {}) {
    const scope = datasetScopeQuery();
    const query = refresh ? `${scope}${scope ? "&" : "?"}refresh=true` : scope;
    return requestJson(`/api/dataset/readiness${query}`, {
        signal,
        operation: "Audit dataset readiness",
    });
}
