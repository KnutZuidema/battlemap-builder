/**
 * The AI vision clients: a Physical Description and theme/tag suggestions, for a staging
 * candidate or for an already curated map.
 *
 * One describe implementation and one suggest implementation sit behind four named
 * functions, so triage, the readiness batch, the inspector and the decision modal all speak
 * the same request. Failures are raised through `requestJson` (pill + one toast) and rethrown;
 * callers only decide how their draft degrades.
 *
 * Callers route these through `tracker`: `join` for suggestions and batch repair (a request
 * already in flight is reused), `latest` for a describe that a newer one supersedes.
 */

import { requestJson } from "./http.js";

function stagingPath(candidateId, suffix) {
    return `/api/staging/${encodeURIComponent(candidateId)}/${suffix}`;
}

function curatedPath(mapId, suffix) {
    return `/api/curated/${encodeURIComponent(mapId)}/${suffix}`;
}

/**
 * Regenerate a staging candidate's Physical Description.
 *
 * @param {string} candidateId
 * @param {{ signal?: AbortSignal, tags?: string[] }} [options]
 * @returns {Promise<{ description: string }>}
 */
export function describeStagingCandidate(candidateId, { signal, tags } = {}) {
    return describe(stagingPath(candidateId, "describe"), "Describe staging candidate", { signal, tags });
}

/**
 * Suggest themes and tags for a staging candidate.
 *
 * @param {string} candidateId
 * @param {{ signal?: AbortSignal }} [options]
 * @returns {Promise<string[]>}
 */
export function suggestStagingThemes(candidateId, { signal } = {}) {
    return suggest(stagingPath(candidateId, "suggest-themes"), "Suggest staging themes", { signal });
}

/**
 * Regenerate a curated map's Physical Description.
 *
 * `apply` asks the server to persist the result with the map's Map Sections; the payload then
 * carries `applied`, which the caller checks.
 *
 * @param {string} mapId
 * @param {{ signal?: AbortSignal, tags?: string[], apply?: boolean }} [options]
 * @returns {Promise<{ description: string, applied?: boolean }>}
 */
export function describeCuratedMap(mapId, { signal, tags, apply = false } = {}) {
    return describe(curatedPath(mapId, "describe"), "Describe curated map", { signal, tags, apply });
}

/**
 * Suggest themes and tags for a curated map.
 *
 * @param {string} mapId
 * @param {{ signal?: AbortSignal }} [options]
 * @returns {Promise<string[]>}
 */
export function suggestCuratedThemes(mapId, { signal } = {}) {
    return suggest(curatedPath(mapId, "suggest-themes"), "Suggest curated themes", { signal });
}

async function describe(path, operation, { signal, tags, apply = false } = {}) {
    const body = {};
    if (Array.isArray(tags)) body.tags = tags;
    if (apply) body.apply = true;

    const payload = await requestJson(path, { method: "POST", body, signal, operation });
    const result = {
        description: payload && typeof payload.description === "string" ? payload.description.trim() : "",
    };
    if (payload && typeof payload.applied === "boolean") {
        result.applied = payload.applied;
    }
    return result;
}

async function suggest(path, operation, { signal } = {}) {
    const payload = await requestJson(path, { method: "POST", signal, operation });
    return payload && Array.isArray(payload.suggestions) ? payload.suggestions : [];
}
