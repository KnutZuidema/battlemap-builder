/**
 * Dataset tag vocabulary and layer-role normalisation.
 */

import { state } from "../store.js";

export const CANONICAL_THEMES = [
    "dungeon",
    "cave",
    "tavern",
    "wilderness",
    "ruins",
    "urban",
    "scifi",
    "building",
];

export function getKnownDatasetTags() {
    const tagMap = new Map();
    const defaultThemes = ["Dungeon", "Cave", "Tavern", "Wilderness", "Ruins", "Urban", "SciFi", "Building"];
    for (const t of defaultThemes) {
        tagMap.set(t.toLowerCase(), t);
    }

    if (state.curatedMaps && Array.isArray(state.curatedMaps)) {
        for (const map of state.curatedMaps) {
            const tags = map.tags || [];
            for (const t of tags) {
                if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
            }
        }
    }

    if (state.candidates && Array.isArray(state.candidates)) {
        for (const c of state.candidates) {
            const tags = c.tags || [];
            for (const t of tags) {
                if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
            }
        }
    }

    return Array.from(tagMap.values());
}

export function normalizeLayerRole(role) {
    const r = (role || "").toLowerCase().replace(/[-_]/g, "");
    if (r === "ground") return "ground";
    if (r === "upperfloor" || r === "upper") return "upper_floor";
    if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "basement";
    if (r === "roof") return "roof";
    if (r === "variant") return "variant";
    return "ground";
}

export function isCanonicalThemeTag(tag) {
    return CANONICAL_THEMES.includes((tag || "").trim().toLowerCase());
}
