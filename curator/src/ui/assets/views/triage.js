/**
 * Staging queue, candidate loading and prefetching, and the multi-layer triage deck.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { formatRoleName } from "../kit/format.js";
import { requestRender, fitToScreen } from "../stage/canvas.js";
import { enterLabelingMode, exitLabelingMode, updateGridPanel } from "../stage/grid-panel.js";
import { fetchThemeSuggestions } from "./curation.js";
import { showLoading, showEmptyState, showToast, updateHud } from "../app.js";

    // Constants
    const PREFETCH_AHEAD = 3;

    // ==========================================================================
    // Pre-fetching & Candidate Queue Management
    // ==========================================================================

    export async function fetchStagingQueue() {
        showLoading(true, "Fetching staging queue...");
        try {
            const res = await fetch("/api/staging");
            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }
            const data = await res.json();
            // A candidate whose curation request is still in flight is already gone from the
            // reviewer's local queue; do not resurrect it from the server's staging listing.
            state.candidates = (Array.isArray(data) ? data : []).filter(
                (c) => !state.pendingCurationIds.has(c.id)
            );

            if (state.candidates.length > 0) {
                loadCandidate(0);
            } else {
                state.currentIndex = -1;
                state.currentImage = null;
                showEmptyState(true);
                showLoading(false);
                updateHud();
            }
        } catch (err) {
            showToast(`Failed to load staging queue: ${err.message}`, "danger");
            showLoading(false);
            showEmptyState(true);
        }
    }

    /**
     * Loads candidate by index, leveraging in-memory prefetch cache for 0ms transitions.
     */
    export function loadCandidate(index) {
        exitLabelingMode();
        if (index < 0 || index >= state.candidates.length) {
            state.currentIndex = -1;
            state.currentLayerIndex = 0;
            state.currentImage = null;
            requestRender();
            renderLayerDeck();
            updateHud();
            showEmptyState(state.candidates.length === 0);
            return;
        }

        showEmptyState(false);
        state.currentIndex = index;
        state.currentLayerIndex = 0;
        const candidate = state.candidates[index];
        fetchThemeSuggestions(candidate);
        if (candidate.grid) {
            state.gridSize = candidate.grid.cell_size_px ?? 64;
            state.gridOffset = {
                x: candidate.grid.offset_x ?? 0,
                y: candidate.grid.offset_y ?? 0,
            };
        } else {
            state.gridSize = 64;
            state.gridOffset = { x: 0, y: 0 };
        }
        updateGridPanel();
        const cacheKey = `${candidate.id}_layer_0`;

        // Check if layer 0 (or candidate) is already cached in memory
        let cachedImg = state.imageCache.get(cacheKey) || state.imageCache.get(candidate.id);

        if (cachedImg && cachedImg.complete && cachedImg.naturalWidth > 0) {
            // Instant 0ms visual delay transition!
            state.currentImage = cachedImg;
            showLoading(false);
            fitToScreen();
            renderLayerDeck();
            prefetchUpcoming();
            updateHud();
        } else {
            showLoading(true, `Loading candidate #${candidate.id}...`);
            if (!cachedImg) {
                cachedImg = new Image();
                cachedImg.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/0`;
                state.imageCache.set(cacheKey, cachedImg);
                state.imageCache.set(candidate.id, cachedImg);
            }

            cachedImg.onload = () => {
                // Ensure candidate hasn't shifted while loading
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    state.currentImage = cachedImg;
                    showLoading(false);
                    fitToScreen();
                    renderLayerDeck();
                    prefetchUpcoming();
                    updateHud();
                }
            };

            cachedImg.onerror = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    showLoading(false);
                    showToast(`Error loading candidate ${candidate.id}`, "danger");
                }
            };
        }
    }

    /**
     * In-memory pre-fetching of all layers for active candidate, upcoming 3 candidates,
     * and previous 1 candidate for instant, zero-latency transitions upon hotkeys.
     */
    export function prefetchUpcoming() {
        let readyCount = 0;

        // Prefetch all layers of current candidate
        if (state.currentIndex >= 0 && state.currentIndex < state.candidates.length) {
            const curCand = state.candidates[state.currentIndex];
            const curLayers = getCandidateLayers(curCand);
            curLayers.forEach((l, idx) => {
                const key = `${curCand.id}_layer_${idx}`;
                let img = state.imageCache.get(key);
                if (!img) {
                    img = new Image();
                    img.src = `/api/image/staging/${encodeURIComponent(curCand.id)}/${idx}`;
                    state.imageCache.set(key, img);
                    img.addEventListener("load", () => updateHud(), { once: true });
                }
                if (img.complete && img.naturalWidth > 0) readyCount++;
            });
            if (!state.imageCache.get(curCand.id)) {
                const l0 = state.imageCache.get(`${curCand.id}_layer_0`);
                if (l0) state.imageCache.set(curCand.id, l0);
            }
        }

        // Prefetch next candidates
        for (let offset = 1; offset <= PREFETCH_AHEAD; offset++) {
            const nextIdx = state.currentIndex + offset;
            if (nextIdx < state.candidates.length) {
                const nextCand = state.candidates[nextIdx];
                fetchThemeSuggestions(nextCand);
                const layers = getCandidateLayers(nextCand);
                layers.forEach((l, idx) => {
                    const key = `${nextCand.id}_layer_${idx}`;
                    let img = state.imageCache.get(key);
                    if (!img) {
                        img = new Image();
                        img.src = `/api/image/staging/${encodeURIComponent(nextCand.id)}/${idx}`;
                        state.imageCache.set(key, img);
                        img.addEventListener("load", () => updateHud(), { once: true });
                    }
                    if (img.complete && img.naturalWidth > 0) readyCount++;
                });
                if (!state.imageCache.get(nextCand.id)) {
                    const l0 = state.imageCache.get(`${nextCand.id}_layer_0`);
                    if (l0) state.imageCache.set(nextCand.id, l0);
                }
            }
        }

        // Prefetch previous 1 candidate
        if (state.currentIndex > 0) {
            const prevCand = state.candidates[state.currentIndex - 1];
            const layers = getCandidateLayers(prevCand);
            layers.forEach((l, idx) => {
                const key = `${prevCand.id}_layer_${idx}`;
                let prevImg = state.imageCache.get(key);
                if (!prevImg) {
                    prevImg = new Image();
                    prevImg.src = `/api/image/staging/${encodeURIComponent(prevCand.id)}/${idx}`;
                    state.imageCache.set(key, prevImg);
                    prevImg.addEventListener("load", () => updateHud(), { once: true });
                }
                if (prevImg.complete && prevImg.naturalWidth > 0) readyCount++;
            });
            if (!state.imageCache.get(prevCand.id)) {
                const l0 = state.imageCache.get(`${prevCand.id}_layer_0`);
                if (l0) state.imageCache.set(prevCand.id, l0);
            }
        }

        return readyCount;
    }

    export function navigateNext() {
        if (state.currentIndex < state.candidates.length - 1) {
            loadCandidate(state.currentIndex + 1);
        } else {
            showToast("Reached end of staging queue", "info");
        }
    }

    export function navigatePrev() {
        if (state.currentIndex > 0) {
            loadCandidate(state.currentIndex - 1);
        } else {
            showToast("At beginning of staging queue", "info");
        }
    }

    // ==========================================================================
    // Multi-Layer Canvas Triage Deck & Hotkey Controls
    // ==========================================================================

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

    export function getRoleCssClass(role) {
        const r = (role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") return "ground";
        if (r === "upperfloor" || r === "upper") return "upper_floor";
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "basement";
        if (r === "roof") return "roof";
        if (r === "variant") return "variant";
        if (r === "exclude" || r === "excluded") return "exclude";
        return "unassigned";
    }

    /**
     * Synchronized in-place layer switching.
     * Preserves state.pan and state.zoom exactly without resetting the viewport!
     */
    export function setLayer(layerIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (layerIndex < 0 || layerIndex >= layers.length) return;

        state.currentLayerIndex = layerIndex;
        const cacheKey = `${candidate.id}_layer_${layerIndex}`;
        let cachedImg = state.imageCache.get(cacheKey);

        if (cachedImg && cachedImg.complete && cachedImg.naturalWidth > 0) {
            // Instant 0ms transition with camera pan and zoom preserved!
            state.currentImage = cachedImg;
            requestRender();
            renderLayerDeck();
            updateHud();
        } else {
            showLoading(true, `Loading layer #${layerIndex}...`);
            if (!cachedImg) {
                cachedImg = new Image();
                cachedImg.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/${layerIndex}`;
                state.imageCache.set(cacheKey, cachedImg);
            }
            cachedImg.onload = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id && state.currentLayerIndex === layerIndex) {
                    state.currentImage = cachedImg;
                    showLoading(false);
                    requestRender();
                    renderLayerDeck();
                    updateHud();
                }
            };
            cachedImg.onerror = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    showLoading(false);
                    showToast(`Error loading layer ${layerIndex}`, "danger");
                }
            };
        }
    }

    /**
     * Cycles to next or previous layer within the current bundle without resetting camera.
     */
    export function cycleLayer(delta) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (!layers || layers.length <= 1) return;

        const nextIdx = (state.currentLayerIndex + delta + layers.length) % layers.length;
        setLayer(nextIdx);
    }

    /**
     * Renders the left-side layer deck, filmstrip cards, and role selectors.
     */
    export function renderLayerDeck() {
        if (!dom.layerDeck) return;
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            dom.layerDeck.classList.add("hidden");
            if (dom.btnSplitBundleHud) dom.btnSplitBundleHud.classList.add("hidden");
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);

        // Update counter
        if (dom.layerDeckCount) {
            dom.layerDeckCount.textContent = `${state.currentLayerIndex + 1} / ${layers.length}`;
        }

        const isSplitChild = Boolean(candidate.parent_group_id);
        const isMultiLayer = layers.length > 1;

        if (dom.layerDeckBundleTag) {
            if (isSplitChild) {
                dom.layerDeckBundleTag.textContent = "Split Child";
                dom.layerDeckBundleTag.className = "badge badge-bundle";
            } else if (isMultiLayer) {
                dom.layerDeckBundleTag.textContent = "Multi-Layer Bundle";
                dom.layerDeckBundleTag.className = "badge badge-bundle";
            } else {
                dom.layerDeckBundleTag.textContent = "Standalone Map";
                dom.layerDeckBundleTag.className = "badge";
            }
        }

        if (dom.btnSplitBundle) {
            if (isSplitChild) {
                dom.btnSplitBundle.classList.remove("hidden");
                if (dom.btnSplitLabel) dom.btnSplitLabel.textContent = "Regroup Bundle";
                dom.btnSplitBundle.title = "Regroup split candidates back into bundle (Hotkey: U)";
            } else if (isMultiLayer) {
                dom.btnSplitBundle.classList.remove("hidden");
                if (dom.btnSplitLabel) dom.btnSplitLabel.textContent = "Split Bundle";
                dom.btnSplitBundle.title = "Split multi-layer bundle into standalone maps (Hotkey: U)";
            } else {
                dom.btnSplitBundle.classList.add("hidden");
            }
        }

        if (dom.btnSplitBundleHud) {
            if (isSplitChild) {
                dom.btnSplitBundleHud.classList.remove("hidden");
                if (dom.btnSplitHudText) dom.btnSplitHudText.textContent = "Regroup";
                dom.btnSplitBundleHud.title = "Regroup split candidates back into bundle (Hotkey: U)";
            } else if (isMultiLayer) {
                dom.btnSplitBundleHud.classList.remove("hidden");
                if (dom.btnSplitHudText) dom.btnSplitHudText.textContent = "Split";
                dom.btnSplitBundleHud.title = "Split multi-layer bundle into standalone maps (Hotkey: U)";
            } else {
                dom.btnSplitBundleHud.classList.add("hidden");
            }
        }

        // Show layer deck container
        dom.layerDeck.classList.remove("hidden");

        // Sync active state in Role Selector HUD
        const activeLayer = layers[state.currentLayerIndex] || layers[0];
        const activeRoleCls = getRoleCssClass(activeLayer?.role);
        const roleBadges = dom.roleSelector ? dom.roleSelector.querySelectorAll(".role-badge") : [];
        roleBadges.forEach((b) => {
            if (b.getAttribute("data-role") === activeRoleCls) {
                b.classList.add("active");
            } else {
                b.classList.remove("active");
            }
        });

        // Render layer cards into filmstrip
        if (!dom.layerFilmstrip) return;
        dom.layerFilmstrip.innerHTML = "";

        layers.forEach((l, idx) => {
            const card = document.createElement("div");
            const roleCls = getRoleCssClass(l.role);
            const isUnassigned = roleCls === "unassigned";
            const isExcluded = roleCls === "exclude";
            const isActive = idx === state.currentLayerIndex;

            card.className = `layer-card${isActive ? " active" : ""}${isUnassigned ? " unassigned" : ""}${isExcluded ? " excluded" : ""}`;
            card.setAttribute("role", "tab");
            card.setAttribute("aria-selected", isActive ? "true" : "false");
            card.setAttribute("tabindex", "0");
            card.title = `Layer #${idx}: ${formatRoleName(l.role, l.elevation)}${l.caption ? ` — ${l.caption}` : ""}`;

            const thumbContainer = document.createElement("div");
            thumbContainer.className = "layer-thumb-container";

            const idxBadge = document.createElement("span");
            idxBadge.className = "layer-index-badge";
            idxBadge.textContent = `#${l.index !== undefined ? l.index : idx}`;
            thumbContainer.appendChild(idxBadge);

            const img = document.createElement("img");
            img.className = "layer-thumb-img";
            img.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/${idx}`;
            img.alt = `Layer ${idx}`;
            img.loading = "lazy";
            thumbContainer.appendChild(img);
            card.appendChild(thumbContainer);
            const infoEl = document.createElement("div");
            infoEl.className = "layer-card-info";

            if (l.caption) {
                const captionEl = document.createElement("div");
                captionEl.className = "layer-caption-tooltip";
                captionEl.textContent = l.caption;
                infoEl.appendChild(captionEl);
            }

            const metaEl = document.createElement("div");
            metaEl.className = "layer-meta";
            const pill = document.createElement("span");
            pill.className = `layer-role-pill role-${roleCls}`;
            pill.textContent = formatRoleName(l.role, l.elevation);
            pill.title = "Click to set role (Hotkey: 3)";
            pill.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                enterLabelingMode("role");
            });
            metaEl.appendChild(pill);

            const steppers = document.createElement("div");
            steppers.className = "layer-elevation-steppers";

            const btnDown = document.createElement("button");
            btnDown.type = "button";
            btnDown.className = "btn-elev-stepper";
            btnDown.textContent = "−";
            btnDown.title = "Lower elevation (Alt+Down)";
            btnDown.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                stepLayerElevation(-1, idx);
            });
            steppers.appendChild(btnDown);

            const btnUp = document.createElement("button");
            btnUp.type = "button";
            btnUp.className = "btn-elev-stepper";
            btnUp.textContent = "+";
            btnUp.title = "Raise elevation (Alt+Up)";
            btnUp.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                stepLayerElevation(1, idx);
            });
            steppers.appendChild(btnUp);

            metaEl.appendChild(steppers);
            infoEl.appendChild(metaEl);
            card.appendChild(infoEl);
            card.addEventListener("click", () => {
                setLayer(idx);
            });

            dom.layerFilmstrip.appendChild(card);
        });
    }

    /**
     * Stepper adjustment for active layer floor elevation (Alt+Up / Alt+Down).
     */
    export async function stepLayerElevation(delta, targetIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        const idx = targetIndex !== undefined ? targetIndex : state.currentLayerIndex;
        if (idx < 0 || idx >= layers.length) return;

        const layer = layers[idx];
        const curElev = typeof layer.elevation === "number" ? layer.elevation : 0;
        const newElev = curElev + delta;
        layer.elevation = newElev;

        // Auto-adjust role based on elevation if currently Upper, Basement, Ground, or Unassigned
        const r = (layer.role || "").toLowerCase().replace(/[-_]/g, "");
        if (newElev > 0 && (r === "ground" || r === "basement" || r === "unassigned")) {
            layer.role = "upper_floor";
        } else if (newElev < 0 && (r === "ground" || r === "upperfloor" || r === "upper" || r === "unassigned")) {
            layer.role = "basement";
        } else if (newElev === 0 && (r === "upperfloor" || r === "upper" || r === "basement" || r === "unassigned")) {
            layer.role = "ground";
        }

        candidate.layers = layers;
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Assigns role to current layer with auto-increment/decrement of floor numbers.
     */
    export async function assignRole(roleName, targetIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        const idx = targetIndex !== undefined ? targetIndex : state.currentLayerIndex;
        if (idx < 0 || idx >= layers.length) return;

        const layer = layers[idx];
        const r = roleName.toLowerCase().replace(/[-_]/g, "");

        let elevation = null;
        if (r === "ground") {
            elevation = 0;
        } else if (r === "upperfloor" || r === "upper") {
            // Auto-increment: find highest existing UpperFloor in bundle
            let maxUpper = 0;
            layers.forEach((l, i) => {
                if (i !== idx) {
                    const lr = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    if (lr === "upperfloor" || lr === "upper") {
                        const elev = typeof l.elevation === "number" ? l.elevation : 0;
                        if (elev > maxUpper) maxUpper = elev;
                    }
                }
            });
            elevation = maxUpper + 1;
        } else if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
            // Auto-decrement: find lowest existing Basement in bundle
            let minBasement = 0;
            layers.forEach((l, i) => {
                if (i !== idx) {
                    const lr = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    if (lr === "basement" || lr === "lower" || lr === "lowerfloor" || lr === "cellar") {
                        const elev = typeof l.elevation === "number" ? l.elevation : 0;
                        if (elev < minBasement) minBasement = elev;
                    }
                }
            });
            elevation = minBasement - 1;
        } else {
            // Roof, Variant, Exclude, Unassigned
            elevation = null;
        }

        layer.role = roleName;
        layer.elevation = elevation;
        candidate.layers = layers;
        exitLabelingMode();
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Space hotkey: Quick-toggle current layer between Exclude and assigned/inferred role.
     */
    export async function toggleActiveLayerExclude() {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (!layers || layers.length === 0) return;

        const layer = layers[state.currentLayerIndex];
        if (!layer) return;

        const r = (layer.role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "exclude" || r === "excluded") {
            // Re-include layer
            const prevRole = layer._prevRole || (layer.index === 0 ? "ground" : "upper_floor");
            const prevElev = layer._prevElevation !== undefined ? layer._prevElevation : (prevRole === "ground" ? 0 : 1);
            layer.role = prevRole;
            layer.elevation = prevElev;
            showToast(`Layer #${state.currentLayerIndex} included as ${formatRoleName(prevRole, prevElev)}`, "info");
        } else {
            // Exclude layer
            layer._prevRole = layer.role;
            layer._prevElevation = layer.elevation;
            layer.role = "exclude";
            layer.elevation = null;
            showToast(`Layer #${state.currentLayerIndex} marked as Exclude`, "info");
        }

        candidate.layers = layers;
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Persists updated layer metadata to /api/staging/{id}/layers.
     */
    export async function persistCandidateLayers(candidate) {
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(candidate.id)}/layers`, {
                method: "PATCH",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ layers: candidate.layers }),
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const updated = await res.json();
            candidate.layers = updated.layers;
            renderLayerDeck();
        } catch (err) {
            showToast(`Failed to persist layer metadata: ${err.message}`, "danger");
        }
    }

    /**
     * Decomposes multi-layer bundle into standalone candidates via /api/staging/{id}/split.
     */
    export async function splitCandidate(id) {
        if (state.isSubmitting) return;
        state.isSubmitting = true;
        showLoading(true, "Splitting bundle into standalone maps...");
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(id)}/split`, {
                method: "POST",
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const splitCandidates = await res.json();
            const idx = state.candidates.findIndex((c) => c.id === id);
            if (idx !== -1) {
                state.candidates.splice(idx, 1, ...splitCandidates);
                state.lastSplitParentId = id;
                loadCandidate(idx);
                showToast("Bundle split into standalone maps. Press U or click Undo to regroup.", "info");
            }
        } catch (err) {
            showToast(`Split failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
            showLoading(false);
        }
    }

    /**
     * Recombines split candidates back into single bundle via /api/staging/{parent_id}/regroup.
     */
    export async function regroupCandidate(parentId) {
        if (state.isSubmitting) return;
        state.isSubmitting = true;
        showLoading(true, "Regrouping multi-layer bundle...");
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(parentId)}/regroup`, {
                method: "POST",
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const regroupedCandidate = await res.json();

            // Find all split children belonging to parentId
            const firstIdx = state.candidates.findIndex(
                (c) => c.parent_group_id === parentId || c.id === parentId || c.id.startsWith(`${parentId}_`)
            );
            if (firstIdx !== -1) {
                const count = state.candidates.filter(
                    (c) => c.parent_group_id === parentId || c.id === parentId || c.id.startsWith(`${parentId}_`)
                ).length;
                state.candidates.splice(firstIdx, Math.max(1, count), regroupedCandidate);
                state.lastSplitParentId = null;
                loadCandidate(firstIdx);
                showToast("Regrouped multi-layer bundle", "success");
            }
        } catch (err) {
            showToast(`Regroup failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
            showLoading(false);
        }
    }

    /**
     * Toggles between split bundle and regroup depending on current candidate state.
     */
    export function handleSplitOrRegroup() {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        if (candidate.parent_group_id) {
            regroupCandidate(candidate.parent_group_id);
        } else if (candidate.layers && candidate.layers.length > 1) {
            splitCandidate(candidate.id);
        } else if (state.lastSplitParentId) {
            regroupCandidate(state.lastSplitParentId);
        } else {
            showToast("Candidate is not a multi-layer bundle", "info");
        }
    }
