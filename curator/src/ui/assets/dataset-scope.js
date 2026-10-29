/**
 * The theme scope the dataset endpoints and the library export both share.
 */

import { state } from "./store.js";
import { CANONICAL_THEMES } from "./kit/tags.js";

/** Active library tag when it is a theme the dataset endpoints can scope to, else null. */
export function datasetScopeTheme() {
    const tag = (state.libraryActiveTag || "").toLowerCase();
    return CANONICAL_THEMES.includes(tag) ? tag : null;
}

/** Theme query for the current library tag filter, mirroring the export scope exactly. */
export function datasetScopeQuery() {
    const theme = datasetScopeTheme();
    return theme ? `?theme=${encodeURIComponent(theme)}` : "";
}
