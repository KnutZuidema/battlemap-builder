/**
 * Candidate layer resolution shared by the triage deck, the curation actions,
 * and the canvas engine.
 */

import { state } from "../store.js";

const DEFAULT_DIMENSIONS = { width: 2048, height: 2048 };

export function getCandidateLayers(candidate) {
    if (!candidate) return [];
    if (candidate.layers && candidate.layers.length > 0) {
        return candidate.layers;
    }
    return [{
        index: 0,
        filename: candidate.filename || `${candidate.id}.png`,
        role: "ground",
        elevation: 0,
        dimensions: candidate.dimensions || [2048, 2048],
    }];
}

function isUsableDimensionPair(dimensions) {
    return Boolean(dimensions) && dimensions[0] > 0 && dimensions[1] > 0;
}

function isUsableSize(size) {
    return Boolean(size) && size.width > 0 && size.height > 0;
}

/**
 * Resolves the Ground layer dimensions of a layer bundle.
 *
 * Walks the layer list for a normalised `ground` role, then index 0, then the first
 * layer, and finally falls back to the map/candidate dimensions (array or `{width,
 * height}` form) and the loaded image's natural size. Everything missing yields the
 * default 2048×2048 bounds.
 *
 * `getCurrentGroundDimensions` and `getLibraryFullscreenDimensions` are the two state
 * readers over it; the resolution itself is identical for candidates and library maps.
 */
function resolveGroundDimensions(layers, dimensions, image) {
    const list = Array.isArray(layers) ? layers : [];
    const ground = list.find(l => (l.role || "").toLowerCase().replace(/[-_]/g, "") === "ground");
    if (ground && isUsableDimensionPair(ground.dimensions)) {
        return { width: ground.dimensions[0], height: ground.dimensions[1] };
    }
    const indexZero = list.find(l => l.index === 0);
    if (indexZero && isUsableDimensionPair(indexZero.dimensions)) {
        return { width: indexZero.dimensions[0], height: indexZero.dimensions[1] };
    }
    if (list.length > 0 && isUsableDimensionPair(list[0].dimensions)) {
        return { width: list[0].dimensions[0], height: list[0].dimensions[1] };
    }
    if (Array.isArray(dimensions) && isUsableDimensionPair(dimensions)) {
        return { width: dimensions[0], height: dimensions[1] };
    }
    if (isUsableSize(dimensions)) {
        return { width: dimensions.width, height: dimensions.height };
    }
    if (image && image.naturalWidth > 0) {
        return { width: image.naturalWidth, height: image.naturalHeight };
    }
    return { ...DEFAULT_DIMENSIONS };
}

/**
 * Retrieves Ground layer dimensions for the current triage candidate.
 */
export function getCurrentGroundDimensions() {
    const cand = state.candidates && state.candidates[state.currentIndex];
    return resolveGroundDimensions(
        cand ? getCandidateLayers(cand) : null,
        cand ? cand.dimensions : null,
        state.currentImage
    );
}

/**
 * Retrieves Ground layer dimensions for the current fullscreen map.
 */
export function getLibraryFullscreenDimensions() {
    const map = state.libraryFullscreenMap;
    return resolveGroundDimensions(
        map ? map.layers : null,
        map ? map.dimensions : null,
        state.libraryFullscreenCurrentImage
    );
}
