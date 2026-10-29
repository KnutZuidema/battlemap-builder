/**
 * Slide-over inspector drawer: the metadata draft, layer role/elevation editing, tag
 * chips, and the in-place AI refinement engine.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { escapeHtml } from "../kit/dom.js";
import { formatRoleName } from "../kit/format.js";
import {
    CANONICAL_THEMES,
    isCanonicalThemeTag,
    normalizeLayerRole,
} from "../kit/tags.js";
import { applyLibraryFilters, getScrubPillRoleClass } from "./library.js";
import { openLibraryFullscreen } from "./library/fullscreen.js";
import { openDeleteCuratedModal } from "./library/bulk.js";
import { showToast } from "../shell/status.js";
import { invalidateDatasetReadiness } from "./readiness.js";
import { describeCuratedMap, suggestCuratedThemes } from "../requests/ai.js";
import { saveCuratedMap } from "../requests/library.js";
import { join, latest } from "../requests/tracker.js";

// ==========================================================================
// Slide-Over Inspector Drawer & Refinement Engine (Issue #135 & Issue #136)
// ==========================================================================

export function openLibraryInspector(map) {
    if (!map) return;
    state.libraryDrawerActive = true;
    state.libraryDrawerMap = map;

    // Populates draft state from selected map (deep copy)
    const tagsList = map.tags ? [...map.tags] : [];
    const descriptionText = map.description || "";
    const layersList = (map.layers && map.layers.length > 0)
        ? map.layers.map(l => ({ ...l }))
        : [{
            index: 0,
            filename: "0_ground.png",
            role: "ground",
            elevation: 0,
            caption: null,
            dimensions: map.dimensions || [1024, 1024],
        }];

    state.libraryDraft = {
        tags: tagsList,
        description: descriptionText,
        layers: layersList,
    };
    state.libraryDraftDirty = false;

    if (dom.inspectorMapIdBadge) {
        dom.inspectorMapIdBadge.textContent = map.id || "—";
        dom.inspectorMapIdBadge.title = map.id || "";
    }

    updateInspectorDirtyVisual(false);
    renderInspectorLayers();
    renderInspectorThemeChips();
    renderInspectorTagChips();

    if (dom.inspectorDescriptionInput) {
        dom.inspectorDescriptionInput.value = state.libraryDraft.description;
    }
    updateInspectorDescriptionCounter(state.libraryDraft.description);

    if (dom.inspectorAiSuggestionsBox) {
        dom.inspectorAiSuggestionsBox.classList.add("hidden");
    }
    if (dom.inspectorAiSuggestionsPills) {
        dom.inspectorAiSuggestionsPills.innerHTML = "";
    }

    if (dom.libraryInspectorDrawer) {
        dom.libraryInspectorDrawer.classList.remove("hidden");
    }
}

export function closeLibraryInspector() {
    if (!state.libraryDrawerActive) return;
    state.libraryDrawerActive = false;
    if (dom.libraryInspectorDrawer) {
        dom.libraryInspectorDrawer.classList.add("hidden");
    }
}

export function toggleLibraryInspector() {
    if (state.libraryDrawerActive) {
        closeLibraryInspector();
    } else {
        if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
            const targetIdx = state.libraryFocusedIndex >= 0 ? state.libraryFocusedIndex : 0;
            openLibraryInspector(state.libraryFilteredMaps[targetIdx]);
        }
    }
}

function markLibraryDraftDirty(dirty = true) {
    state.libraryDraftDirty = dirty;
    updateInspectorDirtyVisual(dirty);
}

function updateInspectorDirtyVisual(dirty) {
    if (dom.inspectorUnsavedBadge) {
        dom.inspectorUnsavedBadge.classList.toggle("hidden", !dirty);
    }
    if (dom.btnInspectorSave) {
        dom.btnInspectorSave.classList.toggle("dirty", dirty);
    }
}

export function renderInspectorLayers() {
    if (!dom.inspectorLayersList || !state.libraryDrawerMap) return;
    dom.inspectorLayersList.innerHTML = "";

    const layers = state.libraryDraft.layers || [];
    if (dom.inspectorLayerCountBadge) {
        dom.inspectorLayerCountBadge.textContent = `${layers.length} Layer${layers.length === 1 ? "" : "s"}`;
    }

    const map = state.libraryDrawerMap;

    const fragment = document.createDocumentFragment();

    layers.forEach((layer, idx) => {
        const card = document.createElement("div");
        card.className = "inspector-layer-card";
        card.dataset.layerIndex = idx;

        const layerIdx = layer.index !== undefined ? layer.index : idx;
        const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

        const roleCls = getScrubPillRoleClass(layer.role);
        const roleName = formatRoleName(layer.role, layer.elevation);
        const normalizedRole = normalizeLayerRole(layer.role);
        const elevText = (typeof layer.elevation === "number")
            ? `${layer.elevation >= 0 ? `+${layer.elevation}` : layer.elevation}`
            : "0";
        const dimsText = (layer.dimensions && Array.isArray(layer.dimensions) && layer.dimensions.length >= 2)
            ? `${layer.dimensions[0]} × ${layer.dimensions[1]}`
            : "—";

        card.innerHTML = `
            <div class="inspector-layer-thumb-box">
                <img class="inspector-layer-thumb" src="${directUrl}" alt="Layer ${layerIdx}" loading="lazy" />
            </div>
            <div class="inspector-layer-details">
                <div class="inspector-layer-meta-row">
                    <span class="inspector-layer-filename mono" title="${escapeHtml(layer.filename || `Layer ${layerIdx}`)}">${escapeHtml(layer.filename || `Layer ${layerIdx}`)}</span>
                    <div class="inspector-layer-badges">
                        <span class="inspector-role-badge ${roleCls}">${escapeHtml(roleName)}</span>
                        <span class="inspector-elev-badge mono">${escapeHtml(elevText)}</span>
                        <span class="inspector-dim-badge mono">${escapeHtml(dimsText)}</span>
                    </div>
                </div>
                <div class="inspector-layer-controls-row">
                    <div class="inspector-control-group" style="flex: 1;">
                        <label class="inspector-control-label" for="role-sel-${idx}">Role</label>
                        <select id="role-sel-${idx}" class="inspector-role-select" aria-label="Layer role">
                            <option value="ground" ${normalizedRole === "ground" ? "selected" : ""}>Ground</option>
                            <option value="upper_floor" ${normalizedRole === "upper_floor" ? "selected" : ""}>UpperFloor</option>
                            <option value="basement" ${normalizedRole === "basement" ? "selected" : ""}>Basement</option>
                            <option value="roof" ${normalizedRole === "roof" ? "selected" : ""}>Roof</option>
                            <option value="variant" ${normalizedRole === "variant" ? "selected" : ""}>Variant</option>
                        </select>
                    </div>
                    <div class="inspector-control-group">
                        <label class="inspector-control-label" for="elev-inp-${idx}">Elev</label>
                        <input type="number" id="elev-inp-${idx}" class="inspector-elev-input mono" min="-10" max="10" step="1" value="${typeof layer.elevation === "number" ? layer.elevation : (normalizedRole === "ground" ? 0 : 1)}" aria-label="Elevation level" />
                    </div>
                </div>
            </div>
        `;

        const roleSelect = card.querySelector(".inspector-role-select");
        const elevInput = card.querySelector(".inspector-elev-input");
        const roleBadge = card.querySelector(".inspector-role-badge");
        const elevBadge = card.querySelector(".inspector-elev-badge");

        roleSelect?.addEventListener("change", (e) => {
            const newRole = e.target.value;
            layer.role = newRole;

            // Adjust elevation default if switching roles
            if (newRole === "ground") {
                layer.elevation = 0;
                if (elevInput) elevInput.value = "0";
            } else if (newRole === "upper_floor") {
                if (typeof layer.elevation !== "number" || layer.elevation <= 0) {
                    layer.elevation = 1;
                    if (elevInput) elevInput.value = "1";
                }
            } else if (newRole === "basement") {
                if (typeof layer.elevation !== "number" || layer.elevation >= 0) {
                    layer.elevation = -1;
                    if (elevInput) elevInput.value = "-1";
                }
            }

            if (roleBadge) {
                roleBadge.className = `inspector-role-badge ${getScrubPillRoleClass(newRole)}`;
                roleBadge.textContent = formatRoleName(newRole, layer.elevation);
            }
            if (elevBadge) {
                elevBadge.textContent = layer.elevation >= 0 ? `+${layer.elevation}` : `${layer.elevation}`;
            }

            markLibraryDraftDirty();
        });

        elevInput?.addEventListener("input", (e) => {
            let val = parseInt(e.target.value, 10);
            if (isNaN(val)) val = 0;
            val = Math.max(-10, Math.min(10, val));
            layer.elevation = val;

            if (elevBadge) {
                elevBadge.textContent = val >= 0 ? `+${val}` : `${val}`;
            }
            if (roleBadge) {
                roleBadge.textContent = formatRoleName(layer.role, val);
            }

            markLibraryDraftDirty();
        });

        fragment.appendChild(card);
    });

    dom.inspectorLayersList.appendChild(fragment);
}

export function renderInspectorThemeChips() {
    if (!dom.inspectorThemeChips) return;
    dom.inspectorThemeChips.innerHTML = "";

    const themes = (state.libraryDraft.tags || []).filter(isCanonicalThemeTag);
    themes.forEach((theme) => {
        const chip = document.createElement("span");
        chip.className = "chip chip-theme";
        chip.innerHTML = `${escapeHtml(theme)}<button type="button" class="btn-chip-remove" aria-label="Remove theme ${escapeHtml(theme)}" title="Remove">×</button>`;

        const btnRemove = chip.querySelector(".btn-chip-remove");
        btnRemove?.addEventListener("click", (e) => {
            e.stopPropagation();
            removeInspectorTheme(theme);
        });

        dom.inspectorThemeChips.appendChild(chip);
    });
}

function addInspectorTheme(themeStr) {
    if (!themeStr) return;
    const normalized = themeStr.trim().toLowerCase();
    if (!normalized) return;

    if (!state.libraryDraft.tags) {
        state.libraryDraft.tags = [];
    }

    if (!state.libraryDraft.tags.some(t => t.toLowerCase() === normalized)) {
        state.libraryDraft.tags.push(normalized);
        markLibraryDraftDirty();
        renderInspectorThemeChips();
        renderInspectorTagChips();
    }

    if (dom.inspectorThemeInput) {
        dom.inspectorThemeInput.value = "";
    }
}

function removeInspectorTheme(themeStr) {
    removeInspectorTag(themeStr);
}

export function renderInspectorTagChips() {
    if (!dom.inspectorTagChips) return;
    dom.inspectorTagChips.innerHTML = "";

    const tags = (state.libraryDraft.tags || []).filter(t => !isCanonicalThemeTag(t));
    tags.forEach((tag) => {
        const chip = document.createElement("span");
        chip.className = "chip chip-tag";
        chip.innerHTML = `${escapeHtml(tag)}<button type="button" class="btn-chip-remove" aria-label="Remove tag ${escapeHtml(tag)}" title="Remove">×</button>`;

        const btnRemove = chip.querySelector(".btn-chip-remove");
        btnRemove?.addEventListener("click", (e) => {
            e.stopPropagation();
            removeInspectorTag(tag);
        });

        dom.inspectorTagChips.appendChild(chip);
    });
}

function addInspectorTag(tagStr) {
    if (!tagStr) return;
    const normalized = tagStr.trim().toLowerCase();
    if (!normalized) return;

    if (!state.libraryDraft.tags) {
        state.libraryDraft.tags = [];
    }

    if (!state.libraryDraft.tags.some(t => t.toLowerCase() === normalized)) {
        state.libraryDraft.tags.push(normalized);
        markLibraryDraftDirty();
        renderInspectorThemeChips();
        renderInspectorTagChips();
    }

    if (dom.inspectorTagInput) {
        dom.inspectorTagInput.value = "";
    }
}

function removeInspectorTag(tagStr) {
    if (!state.libraryDraft.tags) return;
    const idx = state.libraryDraft.tags.findIndex(t => t.toLowerCase() === tagStr.toLowerCase());
    if (idx !== -1) {
        state.libraryDraft.tags.splice(idx, 1);
        markLibraryDraftDirty();
        renderInspectorThemeChips();
        renderInspectorTagChips();
    }
}

export function updateInspectorDescriptionCounter(text) {
    if (!dom.inspectorDescCounter) return;
    const trimmed = (text || "").trim();
    const wordCount = trimmed ? trimmed.split(/\s+/).length : 0;
    const charCount = (text || "").length;

    dom.inspectorDescCounter.textContent = `${wordCount} word${wordCount === 1 ? "" : "s"} · ${charCount} char${charCount === 1 ? "" : "s"}`;

    dom.inspectorDescCounter.classList.remove("counter-optimal", "counter-suboptimal");
    if (wordCount >= 40 && wordCount <= 90) {
        dom.inspectorDescCounter.classList.add("counter-optimal");
    } else if (wordCount > 0) {
        dom.inspectorDescCounter.classList.add("counter-suboptimal");
    }
}

export async function saveLibraryInspectorChanges() {
    if (!state.libraryDrawerMap || !state.libraryDrawerActive) return;

    const mapId = state.libraryDrawerMap.id;
    if (!mapId) return;

    if (dom.btnInspectorSave) {
        dom.btnInspectorSave.disabled = true;
        dom.btnInspectorSave.classList.add("loading");
    }

    try {
        const payload = {
            tags: state.libraryDraft.tags,
            description: state.libraryDraft.description,
            layers: state.libraryDraft.layers.map((l, idx) => ({
                index: l.index !== undefined ? l.index : idx,
                filename: l.filename || `layer_${idx}.png`,
                role: l.role || "ground",
                elevation: typeof l.elevation === "number" ? l.elevation : (l.role === "ground" ? 0 : 1),
                caption: l.caption || null,
                source_url: l.source_url || null,
                dimensions: (l.dimensions && Array.isArray(l.dimensions) && l.dimensions.length >= 2) ? l.dimensions : (state.libraryDrawerMap.dimensions || [1024, 1024]),
            })),
        };

        const updatedMap = await saveCuratedMap(mapId, payload);

        // Update in state.curatedMaps
        if (state.curatedMaps && Array.isArray(state.curatedMaps)) {
            const idx = state.curatedMaps.findIndex(m => m.id === mapId);
            if (idx !== -1) {
                state.curatedMaps[idx] = updatedMap;
            } else {
                state.curatedMaps.unshift(updatedMap);
            }
        }

        state.libraryDrawerMap = updatedMap;
        state.libraryDraft = {
            tags: [...(updatedMap.tags || [])],
            description: updatedMap.description || "",
            layers: updatedMap.layers ? updatedMap.layers.map(l => ({ ...l })) : [...state.libraryDraft.layers],
        };

        // Tags, description and layer roles all feed the audit, so the stored report is stale.
        invalidateDatasetReadiness();
        markLibraryDraftDirty(false);
        applyLibraryFilters();
        showToast(`Saved changes for map ${mapId}`, "success");
    } catch (_) {
        // The failure policy names the operation; the draft stays editable for a retry.
    } finally {
        if (dom.btnInspectorSave) {
            dom.btnInspectorSave.disabled = false;
            dom.btnInspectorSave.classList.remove("loading");
        }
    }
}

function discardLibraryInspectorChanges() {
    if (!state.libraryDrawerMap) return;

    const map = state.libraryDrawerMap;
    const tagsList = map.tags ? [...map.tags] : [];
    const descriptionText = map.description || "";
    const layersList = (map.layers && map.layers.length > 0)
        ? map.layers.map(l => ({ ...l }))
        : [{
            index: 0,
            filename: "0_ground.png",
            role: "ground",
            elevation: 0,
            caption: null,
            dimensions: map.dimensions || [1024, 1024],
        }];

    state.libraryDraft = {
        tags: tagsList,
        description: descriptionText,
        layers: layersList,
    };
    markLibraryDraftDirty(false);

    renderInspectorLayers();
    renderInspectorThemeChips();
    renderInspectorTagChips();
    if (dom.inspectorDescriptionInput) {
        dom.inspectorDescriptionInput.value = descriptionText;
    }
    updateInspectorDescriptionCounter(descriptionText);
    showToast("Discarded unsaved changes", "info");
}

// --- AI Refinements (Issue #136) ---

/** Suggest themes and tags for the open map; a second click joins the request already running. */
async function suggestInspectorThemes() {
    if (!state.libraryDrawerMap) return;
    const mapId = state.libraryDrawerMap.id;

    const spinner = dom.btnInspectorSuggestThemes?.querySelector(".ai-spinner");
    if (dom.btnInspectorSuggestThemes) {
        dom.btnInspectorSuggestThemes.disabled = true;
        spinner?.classList.remove("hidden");
    }

    try {
        const suggestions = await join(`suggest:curated:${mapId}`, (signal) =>
            suggestCuratedThemes(mapId, { signal }),
        );
        if (state.libraryDrawerMap?.id !== mapId) return;

        renderInspectorAiSuggestions(suggestions);
        if (suggestions.length > 0) {
            showToast(`Generated ${suggestions.length} theme/tag suggestions`, "info");
        } else {
            showToast("No new theme suggestions generated", "info");
        }
    } catch {
        // The request module raised the pill and fired the toast; the drawer keeps its draft.
    } finally {
        if (dom.btnInspectorSuggestThemes) {
            dom.btnInspectorSuggestThemes.disabled = false;
            spinner?.classList.add("hidden");
        }
    }
}

function renderInspectorAiSuggestions(suggestions) {
    if (!dom.inspectorAiSuggestionsBox || !dom.inspectorAiSuggestionsPills) return;

    if (!suggestions || suggestions.length === 0) {
        dom.inspectorAiSuggestionsBox.classList.add("hidden");
        dom.inspectorAiSuggestionsPills.innerHTML = "";
        return;
    }

    dom.inspectorAiSuggestionsBox.classList.remove("hidden");
    dom.inspectorAiSuggestionsPills.innerHTML = "";

    const fragment = document.createDocumentFragment();

    suggestions.forEach((sug) => {
        const pill = document.createElement("button");
        pill.type = "button";
        pill.className = "ai-suggestion-pill";
        pill.innerHTML = `<span>${escapeHtml(sug)}</span><span class="pill-add-icon">+</span>`;
        pill.title = `Add "${sug}" to themes/tags`;

        pill.addEventListener("click", () => {
            addSuggestedItem(sug);
            pill.classList.add("added");
        });

        fragment.appendChild(pill);
    });

    dom.inspectorAiSuggestionsPills.appendChild(fragment);
}

function addSuggestedItem(sug) {
    if (!sug) return;
    const normalized = sug.trim().toLowerCase();
    if (CANONICAL_THEMES.includes(normalized)) {
        addInspectorTheme(normalized);
    } else {
        addInspectorTag(normalized);
    }
}

/**
 * Regenerate the open map's Physical Description into its draft.
 *
 * The request is keyed per map and supersedes an earlier describe of the same map. The draft
 * is captured before awaiting: if the drawer has moved on to another map by the time the
 * answer lands, the answer is dropped instead of overwriting the newer draft.
 */
export async function describeInspectorMap() {
    if (!state.libraryDrawerMap) return;
    const mapId = state.libraryDrawerMap.id;
    const draft = state.libraryDraft;
    const tags = [...(draft.tags || [])];

    const spinner = dom.btnInspectorDescribe?.querySelector(".ai-spinner");
    if (dom.btnInspectorDescribe) {
        dom.btnInspectorDescribe.disabled = true;
        spinner?.classList.remove("hidden");
    }

    try {
        const { description } = await latest(`describe:curated:${mapId}`, (signal) =>
            describeCuratedMap(mapId, { signal, tags }),
        );
        if (!state.libraryDrawerActive || state.libraryDraft !== draft || state.libraryDrawerMap?.id !== mapId) {
            return;
        }

        draft.description = description;
        if (dom.inspectorDescriptionInput) {
            dom.inspectorDescriptionInput.value = description;
        }
        updateInspectorDescriptionCounter(description);
        markLibraryDraftDirty();
        showToast("Generated physical description", "success");
    } catch {
        // The request module raised the pill and fired the toast; the draft keeps its text.
    } finally {
        if (dom.btnInspectorDescribe) {
            dom.btnInspectorDescribe.disabled = false;
            spinner?.classList.add("hidden");
        }
    }
}

async function runAllInspectorAiRefinements() {
    if (!state.libraryDrawerMap) return;
    const mapId = state.libraryDrawerMap.id;
    const draft = state.libraryDraft;

    const comboSpinner = dom.btnInspectorRunAll?.querySelector(".ai-spinner");
    if (dom.btnInspectorRunAll) {
        dom.btnInspectorRunAll.disabled = true;
        comboSpinner?.classList.remove("hidden");
    }

    try {
        // Step 1: Suggest themes & tags, auto-accepting the canonical themes.
        const suggestions = await join(`suggest:curated:${mapId}`, (signal) =>
            suggestCuratedThemes(mapId, { signal }),
        );
        if (!state.libraryDrawerActive || state.libraryDraft !== draft || state.libraryDrawerMap?.id !== mapId) {
            return;
        }
        renderInspectorAiSuggestions(suggestions);
        suggestions.forEach((sug) => {
            const norm = sug.trim().toLowerCase();
            if (CANONICAL_THEMES.includes(norm)) {
                addInspectorTheme(norm);
            }
        });

        // Step 2: Describe from the tags the refinement just settled on.
        const tags = [...(draft.tags || [])];
        const { description } = await latest(`describe:curated:${mapId}`, (signal) =>
            describeCuratedMap(mapId, { signal, tags }),
        );
        if (!state.libraryDrawerActive || state.libraryDraft !== draft || state.libraryDrawerMap?.id !== mapId) {
            return;
        }

        draft.description = description;
        if (dom.inspectorDescriptionInput) {
            dom.inspectorDescriptionInput.value = description;
        }
        updateInspectorDescriptionCounter(description);
        markLibraryDraftDirty();

        showToast("All AI refinements applied successfully", "success");
    } catch {
        // The request module raised the pill and fired the toast; the drawer keeps its draft.
    } finally {
        if (dom.btnInspectorRunAll) {
            dom.btnInspectorRunAll.disabled = false;
            comboSpinner?.classList.add("hidden");
        }
    }
}

export function setupInspectorDrawerControls() {
    dom.btnCloseInspector?.addEventListener("click", () => closeLibraryInspector());
    dom.btnInspectorDiscard?.addEventListener("click", () => discardLibraryInspectorChanges());
    dom.btnInspectorSave?.addEventListener("click", () => saveLibraryInspectorChanges());

    dom.btnInspectorSuggestThemes?.addEventListener("click", () => suggestInspectorThemes());
    dom.btnInspectorDescribe?.addEventListener("click", () => describeInspectorMap());
    dom.btnInspectorRunAll?.addEventListener("click", () => runAllInspectorAiRefinements());

    dom.btnAcceptAllSuggestions?.addEventListener("click", () => {
        if (!dom.inspectorAiSuggestionsPills) return;
        const pills = dom.inspectorAiSuggestionsPills.querySelectorAll(".ai-suggestion-pill:not(.added)");
        pills.forEach((p) => {
            const text = p.querySelector("span")?.textContent || "";
            if (text) addSuggestedItem(text);
            p.classList.add("added");
        });
        showToast("Added all suggestions", "info");
    });

    dom.btnAddTheme?.addEventListener("click", () => {
        if (dom.inspectorThemeInput) {
            addInspectorTheme(dom.inspectorThemeInput.value);
        }
    });

    dom.inspectorThemeInput?.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
            e.preventDefault();
            addInspectorTheme(dom.inspectorThemeInput.value);
        } else if (e.key === "Backspace" && !dom.inspectorThemeInput.value) {
            const themeTags = (state.libraryDraft.tags || []).filter(isCanonicalThemeTag);
            if (themeTags.length > 0) {
                removeInspectorTheme(themeTags[themeTags.length - 1]);
            }
        }
    });

    dom.btnAddTag?.addEventListener("click", () => {
        if (dom.inspectorTagInput) {
            addInspectorTag(dom.inspectorTagInput.value);
        }
    });

    dom.inspectorTagInput?.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
            e.preventDefault();
            addInspectorTag(dom.inspectorTagInput.value);
        } else if (e.key === "Backspace" && !dom.inspectorTagInput.value) {
            if (state.libraryDraft.tags && state.libraryDraft.tags.length > 0) {
                removeInspectorTag(state.libraryDraft.tags[state.libraryDraft.tags.length - 1]);
            }
        }
    });

    dom.inspectorDescriptionInput?.addEventListener("input", (e) => {
        state.libraryDraft.description = e.target.value;
        updateInspectorDescriptionCounter(e.target.value);
        markLibraryDraftDirty();
    });

    dom.btnInspectorFullscreen?.addEventListener("click", () => {
        if (!state.libraryDrawerMap) return;
        const mapId = state.libraryDrawerMap.id;
        let targetIdx = 0;
        if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
            const found = state.libraryFilteredMaps.findIndex(m => m.id === mapId);
            if (found !== -1) targetIdx = found;
        }
        openLibraryFullscreen(targetIdx);
    });

    dom.btnInspectorDelete?.addEventListener("click", () => {
        if (state.libraryDrawerMap) {
            openDeleteCuratedModal(state.libraryDrawerMap.id);
        }
    });
}
