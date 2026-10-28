/**
 * Candidate layer resolution shared by the triage deck, the curation actions,
 * and the canvas engine.
 */

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
