/**
 * The dataset readiness audit request.
 *
 * Every readiness panel reads the same scoped audit: the readiness view, the training launch
 * gate and the freeze refusal all ask the same question, so they ask it the same way.
 */

import { requestJson } from "./http.js";
import { datasetScopeQuery } from "../dataset-scope.js";

/** Audit the curated maps in the active dataset scope. */
export function auditDatasetReadiness({ signal } = {}) {
    return requestJson(`/api/dataset/readiness${datasetScopeQuery()}`, {
        signal,
        operation: "Audit dataset readiness",
    });
}
