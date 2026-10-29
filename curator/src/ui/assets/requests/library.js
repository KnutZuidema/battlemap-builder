/**
 * The curated library requests: the map listing, one map's save / delete / refine, the bulk
 * delete and tag writes, and the two dataset exports.
 *
 * The exports answer with a ZIP archive, so they resolve with a `Blob`; everything else
 * resolves with the JSON payload the endpoint documents.
 */

import { requestBlob, requestJson } from "./http.js";
import { datasetScopeQuery } from "../dataset-scope.js";

function mapPath(mapId) {
    return `/api/curated/${encodeURIComponent(mapId)}`;
}

/** The curated map listing the library filters and the inspector read. */
export function fetchCuratedMaps({ signal } = {}) {
    return requestJson("/api/curated", { signal, operation: "Load curated maps" });
}

/**
 * Persist a curated map. The caller sends the fields it owns: the inspector sends its whole
 * draft, the grid alignment sends `{ grid }`, the bulk tag loop sends `{ tags }`.
 */
export function saveCuratedMap(mapId, patch, { signal } = {}) {
    return requestJson(mapPath(mapId), {
        method: "PATCH",
        body: patch,
        signal,
        operation: "Save curated map",
    });
}

/** Delete one curated map, logging the rejection reason. */
export function deleteCuratedMap(mapId, { reason, notes } = {}, { signal } = {}) {
    return requestJson(mapPath(mapId), {
        method: "DELETE",
        body: { reason, notes },
        signal,
        operation: "Delete curated map",
    });
}

/** Delete many curated maps in one request; resolves with `{ deleted_count }`. */
export function bulkDeleteCuratedMaps(ids, { reason } = {}, { signal } = {}) {
    return requestJson("/api/curated/bulk-delete", {
        method: "POST",
        body: { ids, reason },
        signal,
        operation: "Delete curated maps",
    });
}

/** AI-refine one curated map; resolves with the updated map. */
export function refineCuratedMap(mapId, { signal } = {}) {
    return requestJson(`${mapPath(mapId)}/refine`, {
        method: "POST",
        signal,
        operation: "Refine curated map",
    });
}

/** Package the maps in the active dataset scope as a LoRA dataset ZIP. */
export function exportDataset({ signal } = {}) {
    return requestBlob(`/api/dataset/export${datasetScopeQuery()}`, { signal, operation: "Export dataset" });
}

/** Package exactly the selected curated maps as a LoRA dataset ZIP. */
export function exportSelectedMaps(ids, { signal } = {}) {
    return requestBlob("/api/dataset/export-selected", {
        method: "POST",
        body: { ids },
        signal,
        operation: "Export selected maps",
    });
}
