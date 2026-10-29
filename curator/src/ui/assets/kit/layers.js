/**
 * Candidate layer resolution shared by the triage deck, the curation actions,
 * and the canvas engine.
 */

import { state } from "../store.js";

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

/**
 * Retrieves Ground layer dimensions for the current triage candidate.
 */
export function getCurrentGroundDimensions() {
    const cand = state.candidates && state.candidates[state.currentIndex];
    if (cand) {
        const layers = getCandidateLayers(cand);
        const ground = layers.find(l => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "ground";
        });
        if (ground && ground.dimensions && ground.dimensions[0] > 0 && ground.dimensions[1] > 0) {
            return { width: ground.dimensions[0], height: ground.dimensions[1] };
        }
        const l0 = layers.find(l => l.index === 0);
        if (l0 && l0.dimensions && l0.dimensions[0] > 0 && l0.dimensions[1] > 0) {
            return { width: l0.dimensions[0], height: l0.dimensions[1] };
        }
        if (layers.length > 0 && layers[0].dimensions && layers[0].dimensions[0] > 0 && layers[0].dimensions[1] > 0) {
            return { width: layers[0].dimensions[0], height: layers[0].dimensions[1] };
        }
        if (cand.dimensions && Array.isArray(cand.dimensions) && cand.dimensions[0] > 0 && cand.dimensions[1] > 0) {
            return { width: cand.dimensions[0], height: cand.dimensions[1] };
        }
        if (cand.dimensions && cand.dimensions.width && cand.dimensions.height) {
            return { width: cand.dimensions.width, height: cand.dimensions.height };
        }
    }
    if (state.currentImage && state.currentImage.naturalWidth > 0) {
        return { width: state.currentImage.naturalWidth, height: state.currentImage.naturalHeight };
    }
    return { width: 2048, height: 2048 };
}
