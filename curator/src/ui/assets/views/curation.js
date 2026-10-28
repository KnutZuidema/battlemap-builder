/**
 * Curate decision modal, accepted-tag workflow, and the reject / undo actions.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { calculateLatticeInfo } from "../kit/grid-math.js";
import { getKnownDatasetTags } from "../kit/tags.js";
import { decisionState, fetchStatus, invalidateDatasetReadiness, showToast } from "../app.js";
import { getCurrentGroundDimensions, stopPanLoop, stopZoomLoop } from "../stage/canvas.js";
import { enterLabelingMode, exitLabelingMode, toggleGridPanel } from "../stage/grid-panel.js";
import { fetchStagingQueue, getCandidateLayers, loadCandidate, setLayer } from "./triage.js";

    // ==========================================================================
    // Curation & Rejection Actions
    // ==========================================================================

    /**
     * Proactively pre-fetches AI vision theme suggestions for candidate in background.
     * Caches suggestions on candidate._themeSuggestions or flags candidate._suggestionsOffline.
     */
    export function fetchThemeSuggestions(candidate) {
        if (!candidate) {
            return Promise.resolve([]);
        }
        if (candidate._themeSuggestions !== undefined) {
            return Promise.resolve(candidate._themeSuggestions);
        }
        if (candidate._fetchPromise) {
            return candidate._fetchPromise;
        }

        candidate._fetchPromise = fetch("/api/staging/" + encodeURIComponent(candidate.id) + "/suggest-themes", {
            method: "POST",
        })
            .then((res) => {
                if (!res.ok) throw new Error(`HTTP ${res.status}`);
                return res.json();
            })
            .then((data) => {
                const suggestions = Array.isArray(data.suggestions) ? data.suggestions : [];
                candidate._themeSuggestions = suggestions;
                candidate._suggestionsOffline = suggestions.length === 0;
                return candidate._themeSuggestions;
            })
            .catch((err) => {
                console.warn(`Vision theme suggestions offline/failed for candidate ${candidate.id}:`, err);
                candidate._themeSuggestions = [];
                candidate._suggestionsOffline = true;
                return [];
            })
            .finally(() => {
                candidate._fetchPromise = null;
            });

        return candidate._fetchPromise;
    }

    let currentDescribeAbortController = null;

    /**
     * Asynchronously requests vision description for candidate.
     * @param {Object} candidate
     * @param {boolean} force - if true, bypasses candidate._description cache
     */
    export async function fetchCandidateDescription(candidate, force = false) {
        if (!candidate) return;

        if (currentDescribeAbortController) {
            currentDescribeAbortController.abort();
            currentDescribeAbortController = null;
        }

        if (!force && candidate._description) {
            if (dom.decisionDescriptionInput) {
                dom.decisionDescriptionInput.value = candidate._description;
                dom.decisionDescriptionInput.classList.remove("hidden");
            }
            if (dom.decisionDescriptionLoading) {
                dom.decisionDescriptionLoading.classList.add("hidden");
            }
            if (dom.decisionDescriptionStatus) {
                dom.decisionDescriptionStatus.className = "decision-description-indicator status-dot status-ready";
                dom.decisionDescriptionStatus.title = "Description ready";
            }
            return candidate._description;
        }

        if (dom.decisionDescriptionLoading) {
            dom.decisionDescriptionLoading.classList.remove("hidden");
        }
        if (dom.decisionDescriptionInput) {
            dom.decisionDescriptionInput.classList.add("hidden");
        }
        if (dom.btnRegenerateDescription) {
            dom.btnRegenerateDescription.disabled = true;
        }
        if (dom.decisionDescriptionStatus) {
            dom.decisionDescriptionStatus.className = "decision-description-indicator status-dot status-processing";
            dom.decisionDescriptionStatus.title = "Generating description with Gemini 3.5 Flash Lite...";
        }

        const controller = new AbortController();
        currentDescribeAbortController = controller;

        const payload = {
            tags: decisionState.acceptedTags && decisionState.acceptedTags.length > 0
                ? decisionState.acceptedTags
                : (candidate.tags || []),
        };

        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(candidate.id)}/describe`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
                signal: controller.signal,
            });

            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }

            const data = await res.json();
            const desc = (data && typeof data.description === "string") ? data.description.trim() : "";
            candidate._description = desc;

            if (decisionState.candidate && decisionState.candidate.id === candidate.id) {
                if (dom.decisionDescriptionInput) {
                    dom.decisionDescriptionInput.value = desc;
                    dom.decisionDescriptionInput.classList.remove("hidden");
                }
                if (dom.decisionDescriptionLoading) {
                    dom.decisionDescriptionLoading.classList.add("hidden");
                }
                if (dom.decisionDescriptionStatus) {
                    dom.decisionDescriptionStatus.className = "decision-description-indicator status-dot status-ready";
                    dom.decisionDescriptionStatus.title = "Description generated";
                }
            }
            return desc;
        } catch (err) {
            if (err.name === "AbortError") {
                return;
            }
            console.warn(`Vision description failed for candidate ${candidate.id}:`, err);
            if (decisionState.candidate && decisionState.candidate.id === candidate.id) {
                if (dom.decisionDescriptionInput) {
                    if (!dom.decisionDescriptionInput.value.trim()) {
                        dom.decisionDescriptionInput.value = candidate.description || (candidate.layers && candidate.layers[0]?.caption) || "";
                    }
                    dom.decisionDescriptionInput.classList.remove("hidden");
                }
                if (dom.decisionDescriptionLoading) {
                    dom.decisionDescriptionLoading.classList.add("hidden");
                }
                if (dom.decisionDescriptionStatus) {
                    dom.decisionDescriptionStatus.className = "decision-description-indicator status-dot status-offline";
                    dom.decisionDescriptionStatus.title = "AI vision description unavailable / fallback";
                }
            }
        } finally {
            if (currentDescribeAbortController === controller) {
                currentDescribeAbortController = null;
            }
            if (dom.btnRegenerateDescription) {
                dom.btnRegenerateDescription.disabled = false;
            }
        }
    }
    /**
     * Curate Validation Gate:
     * 1. If candidate has layers and all layers are excluded (role === 'exclude'), block curation.
     * 2. If candidate has any included layer with role unassigned, block curation, focus that layer
     *    in the filmstrip deck, and enter role labeling mode.
     * Returns true if candidate layers pass validation, false otherwise.
     */
    export function validateCandidateLayers(candidate) {
        if (!candidate) return false;
        const layers = getCandidateLayers(candidate);

        // 1. If all layers are Exclude, block curation
        const allExcluded = layers.length > 0 && layers.every((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "exclude" || r === "excluded";
        });
        if (allExcluded) {
            showToast("Cannot curate: at least 1 layer must be included", "danger");
            return false;
        }

        // 2. If any included layer has role Unassigned, block curation
        const unassignedIdx = layers.findIndex((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r !== "exclude" && r !== "excluded" && (r === "unassigned" || !r);
        });
        if (unassignedIdx !== -1) {
            const unassignedLayer = layers[unassignedIdx];
            const layerNum = unassignedLayer.index !== undefined ? unassignedLayer.index : unassignedIdx;
            showToast(`Cannot curate: layer ${layerNum} is Unassigned. Please assign a role.`, "danger");
            setLayer(unassignedIdx);
            if (dom.layerFilmstrip && dom.layerFilmstrip.children[unassignedIdx]) {
                const card = dom.layerFilmstrip.children[unassignedIdx];
                card.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
                if (typeof card.focus === "function") {
                    card.focus();
                }
            }
            enterLabelingMode("role");
            return false;
        }
        // 3. Strict Ground layer requirement: candidate must have exactly one Ground layer (role ground)
        const groundCount = layers.filter((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "ground";
        }).length;
        if (groundCount === 0) {
            showToast("Cannot curate: at least one layer must be assigned as Ground floor before curating.", "danger");
            return false;
        }
        if (groundCount > 1) {
            showToast("Cannot curate: only one layer can be designated as Ground floor.", "danger");
            return false;
        }

        return true;
    }

    /**
     * Opens the interactive curate decision modal with vision suggestion cards
     * and custom tag input workflow.
     */
    export async function openCurateDecisionModal() {
        stopPanLoop();
        stopZoomLoop();
        exitLabelingMode();
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        if (!validateCandidateLayers(candidate)) {
            return;
        }

        // 16×16 Full-Grid Lattice Curation Gate
        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (!lattice.isEligible) {
            showToast(`Map has only ${lattice.cols}×${lattice.rows} full Grid Cells; at least 16×16 required for curation`, "danger");
            toggleGridPanel(true);
            return;
        }
        // If suggestions are still being fetched, wait for them
        if (candidate._fetchPromise || (candidate._themeSuggestions === undefined && !candidate._suggestionsOffline)) {
            await fetchThemeSuggestions(candidate);
        }

        decisionState.isOpen = true;
        state.decisionModalOpen = true;
        decisionState.candidate = candidate;
        decisionState.cardIndex = 0;
        decisionState.acceptedTags = [];
        decisionState.cardDecisions = [];

        if (dom.curateDecisionModal) {
            dom.curateDecisionModal.classList.remove("hidden");
        }
        if (dom.decisionValidationError) {
            dom.decisionValidationError.classList.add("hidden");
        }
        if (dom.decisionDescriptionInput) {
            dom.decisionDescriptionInput.value = "";
            dom.decisionDescriptionInput.classList.remove("hidden");
        }
        if (dom.decisionDescriptionLoading) {
            dom.decisionDescriptionLoading.classList.add("hidden");
        }

        const suggestions = candidate._themeSuggestions || [];
        if (suggestions.length > 0) {
            decisionState.step = "cards";
            if (dom.decisionOfflineBadge) dom.decisionOfflineBadge.classList.add("hidden");
            if (dom.decisionOfflineNote) dom.decisionOfflineNote.classList.add("hidden");
            renderDecisionCard();
        } else {
            // No suggestions or offline: directly show custom step with offline indicator
            if (candidate._suggestionsOffline || suggestions.length === 0) {
                if (dom.decisionOfflineBadge) dom.decisionOfflineBadge.classList.remove("hidden");
                if (dom.decisionOfflineNote) dom.decisionOfflineNote.classList.remove("hidden");
            } else {
                if (dom.decisionOfflineBadge) dom.decisionOfflineBadge.classList.add("hidden");
                if (dom.decisionOfflineNote) dom.decisionOfflineNote.classList.add("hidden");
            }
            transitionToCustomStep();
        }
    }

    export function closeCurateDecisionModal() {
        stopPanLoop();
        stopZoomLoop();
        if (currentDescribeAbortController) {
            currentDescribeAbortController.abort();
            currentDescribeAbortController = null;
        }
        if (dom.decisionDescriptionInput) {
            dom.decisionDescriptionInput.value = "";
        }
        if (dom.decisionDescriptionLoading) {
            dom.decisionDescriptionLoading.classList.add("hidden");
        }
        decisionState.isOpen = false;
        state.decisionModalOpen = false;
        if (dom.curateDecisionModal) {
            dom.curateDecisionModal.classList.add("hidden");
        }
        decisionState.candidate = null;
        decisionState.cardIndex = 0;
        decisionState.acceptedTags = [];
        decisionState.cardDecisions = [];
        decisionState.step = "cards";
    }

    export function renderDecisionCard() {
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) {
            transitionToCustomStep();
            return;
        }

        decisionState.step = "cards";
        if (dom.decisionCardStep) dom.decisionCardStep.classList.remove("hidden");
        if (dom.decisionCustomStep) dom.decisionCustomStep.classList.add("hidden");

        const currentTag = suggestions[decisionState.cardIndex];
        if (dom.decisionCardTag) {
            dom.decisionCardTag.textContent = currentTag;
        }
        if (dom.decisionProgressBadge) {
            dom.decisionProgressBadge.classList.remove("hidden");
            dom.decisionProgressBadge.textContent = `Suggestion ${decisionState.cardIndex + 1} of ${suggestions.length}`;
        }
        if (dom.btnDecisionUndo) {
            dom.btnDecisionUndo.disabled = (decisionState.cardIndex === 0);
        }

        renderAcceptedTags();
    }

    export function handleDecisionReject() {
        if (decisionState.step !== "cards") return;
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) return;
        const tag = suggestions[decisionState.cardIndex];

        decisionState.cardDecisions[decisionState.cardIndex] = { suggestion: tag, accepted: false };
        removeAcceptedTag(tag);

        decisionState.cardIndex++;
        renderDecisionCard();
    }

    export function handleDecisionAccept() {
        if (decisionState.step !== "cards") return;
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) return;
        const tag = suggestions[decisionState.cardIndex];

        decisionState.cardDecisions[decisionState.cardIndex] = { suggestion: tag, accepted: true };
        addAcceptedTag(tag);

        decisionState.cardIndex++;
        renderDecisionCard();
    }

    export function handleDecisionUndo() {
        if (decisionState.cardIndex > 0) {
            decisionState.cardIndex--;
            const prev = decisionState.cardDecisions[decisionState.cardIndex];
            if (prev && prev.accepted) {
                removeAcceptedTag(prev.suggestion);
            }
            delete decisionState.cardDecisions[decisionState.cardIndex];
            renderDecisionCard();
        }
    }

    export function transitionToCustomStep() {
        decisionState.step = "custom";
        if (dom.decisionCardStep) dom.decisionCardStep.classList.add("hidden");
        if (dom.decisionCustomStep) dom.decisionCustomStep.classList.remove("hidden");

        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (dom.decisionProgressBadge) {
            if (suggestions.length > 0) {
                dom.decisionProgressBadge.classList.remove("hidden");
                dom.decisionProgressBadge.textContent = "Custom Tags";
            } else {
                dom.decisionProgressBadge.classList.add("hidden");
            }
        }
        if (dom.decisionTagInput) {
            dom.decisionTagInput.value = "";
            setTimeout(() => dom.decisionTagInput.focus(), 50);
        }

        renderAutocompletePills("");
        renderAcceptedTags();

        if (decisionState.candidate) {
            fetchCandidateDescription(decisionState.candidate, false);
        }
    }

    export function renderAutocompletePills(query) {
        if (!dom.decisionAutocompleteList) return;
        dom.decisionAutocompleteList.innerHTML = "";

        const allTags = getKnownDatasetTags();
        const cleanQuery = (query || "").trim().toLowerCase();

        let matches = [];
        if (cleanQuery.length > 0) {
            matches = allTags.filter((t) => {
                const lower = t.toLowerCase();
                return lower.includes(cleanQuery) && !hasAcceptedTag(lower);
            });
        } else {
            // Show canonical defaults that aren't yet accepted
            const defaults = ["Dungeon", "Cave", "Tavern", "Wilderness", "Ruins", "Urban", "SciFi", "Building"];
            matches = defaults.filter((t) => !hasAcceptedTag(t));
        }

        // Limit to 8 pills to keep modal uncluttered
        const displayMatches = matches.slice(0, 8);
        for (const tag of displayMatches) {
            const pill = document.createElement("button");
            pill.type = "button";
            pill.className = "autocomplete-pill";
            pill.dataset.tag = tag;
            pill.textContent = tag;
            pill.addEventListener("click", () => {
                addAcceptedTag(tag);
                if (dom.decisionTagInput) {
                    dom.decisionTagInput.value = "";
                    dom.decisionTagInput.focus();
                }
                renderAcceptedTags();
                renderAutocompletePills("");
            });
            dom.decisionAutocompleteList.appendChild(pill);
        }
    }

    export function hasAcceptedTag(tag) {
        const lower = tag.trim().toLowerCase();
        return decisionState.acceptedTags.some((t) => t.toLowerCase() === lower);
    }

    export function addAcceptedTag(tag) {
        const trimmed = tag.trim();
        if (!trimmed) return;
        if (!hasAcceptedTag(trimmed)) {
            decisionState.acceptedTags.push(trimmed);
            if (dom.decisionValidationError) {
                dom.decisionValidationError.classList.add("hidden");
            }
        }
    }

    export function removeAcceptedTag(tag) {
        const lower = tag.trim().toLowerCase();
        decisionState.acceptedTags = decisionState.acceptedTags.filter(
            (t) => t.toLowerCase() !== lower
        );
    }

    export function renderAcceptedTags() {
        if (!dom.decisionAcceptedTags) return;
        dom.decisionAcceptedTags.innerHTML = "";

         if (decisionState.acceptedTags.length === 0) {
            const hint = document.createElement("span");
            hint.className = "decision-empty-tags-hint";
            hint.textContent = "No tags accepted yet";
            dom.decisionAcceptedTags.appendChild(hint);
            return;
        }

        for (const tag of decisionState.acceptedTags) {
            const chip = document.createElement("span");
            chip.className = "tag-chip";
            chip.dataset.tag = tag;

            const label = document.createElement("span");
            label.textContent = tag;
            chip.appendChild(label);

            const btnRemove = document.createElement("button");
            btnRemove.type = "button";
            btnRemove.className = "btn-remove-tag";
            btnRemove.setAttribute("aria-label", `Remove ${tag}`);
            btnRemove.textContent = "✕";
            btnRemove.addEventListener("click", (e) => {
                e.stopPropagation();
                removeAcceptedTag(tag);
                renderAcceptedTags();
                if (decisionState.step === "custom" && dom.decisionTagInput) {
                    renderAutocompletePills(dom.decisionTagInput.value.trim());
                }
            });
            chip.appendChild(btnRemove);

            dom.decisionAcceptedTags.appendChild(chip);
        }
    }

    export function onDecisionTagInput(e) {
        const query = dom.decisionTagInput ? dom.decisionTagInput.value.trim() : "";
        renderAutocompletePills(query);
        if (dom.decisionValidationError) {
            dom.decisionValidationError.classList.add("hidden");
        }
    }

    export function onDecisionTagInputKeydown(e) {
        if (e.key === "Enter") {
            e.preventDefault();
            handleCustomTagInputEnter();
            return;
        }

        if (e.key === "Tab") {
            const firstPill = dom.decisionAutocompleteList?.querySelector(".autocomplete-pill");
            if (firstPill && firstPill.dataset.tag) {
                e.preventDefault();
                addAcceptedTag(firstPill.dataset.tag);
                if (dom.decisionTagInput) {
                    dom.decisionTagInput.value = "";
                    dom.decisionTagInput.focus();
                }
                renderAcceptedTags();
                renderAutocompletePills("");
            }
            return;
        }

        if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
            if (dom.decisionTagInput && dom.decisionTagInput.value === "") {
                const suggestions = decisionState.candidate?._themeSuggestions || [];
                if (suggestions.length > 0 && decisionState.cardIndex > 0) {
                    e.preventDefault();
                    handleDecisionUndo();
                }
            }
        }
    }

    export function handleCustomTagAdd() {
        if (!dom.decisionTagInput) return;
        const text = dom.decisionTagInput.value.trim();
        if (text) {
            addAcceptedTag(text);
            dom.decisionTagInput.value = "";
            renderAcceptedTags();
            renderAutocompletePills("");
            dom.decisionTagInput.focus();
        } else {
            if (decisionState.acceptedTags.length === 0) {
                triggerDecisionShake();
            } else {
                dom.decisionTagInput.focus();
            }
        }
    }

    export function handleCustomTagInputEnter() {
        if (!dom.decisionTagInput) return;
        const text = dom.decisionTagInput.value.trim();
        if (text) {
            addAcceptedTag(text);
            dom.decisionTagInput.value = "";
            renderAcceptedTags();
            renderAutocompletePills("");
            dom.decisionTagInput.focus();
        } else {
            // Empty Enter: finish if acceptedTags.length >= 1, else shake
            if (decisionState.acceptedTags.length >= 1) {
                submitDecisionCuration();
            } else {
                triggerDecisionShake();
            }
        }
    }

    export function handleCustomTagSubmit() {
        if (dom.decisionTagInput && dom.decisionTagInput.value.trim()) {
            addAcceptedTag(dom.decisionTagInput.value.trim());
            dom.decisionTagInput.value = "";
            renderAcceptedTags();
        }

        if (decisionState.acceptedTags.length >= 1) {
            submitDecisionCuration();
        } else {
            triggerDecisionShake();
        }
    }

    export function triggerDecisionShake() {
        const card = dom.curateDecisionModal?.querySelector(".modal-card") || dom.decisionTagInput;
        if (card) {
            card.classList.remove("shake");
            void card.offsetWidth;
            card.classList.add("shake");
            setTimeout(() => card.classList.remove("shake"), 500);
        }
        if (dom.decisionValidationError) {
            dom.decisionValidationError.textContent = "At least one theme label required";
            dom.decisionValidationError.classList.remove("hidden");
        }
        if (dom.decisionTagInput) {
            dom.decisionTagInput.focus();
        }
    }

    /**
     * Submits the curate decision without blocking the triage loop.
     *
     * The server finishes the curation — strict map-level description generation included —
     * after this function returns, so the queue advances immediately and the outcome arrives
     * as a toast once the response lands. A failed curation puts the candidate back into the
     * local queue: nothing was committed, so it is still listed in staging.
     */
    export function submitDecisionCuration() {
        if (decisionState.acceptedTags.length === 0) {
            triggerDecisionShake();
            return;
        }

        const candidate = decisionState.candidate;
        if (!candidate) return;

        const acceptedTags = Array.from(decisionState.acceptedTags);
        const descriptionValue = dom.decisionDescriptionInput ? dom.decisionDescriptionInput.value.trim() : "";
        closeCurateDecisionModal();

        const payload = {
            id: candidate.id,
            tags: acceptedTags,
            layers: candidate.layers && candidate.layers.length > 0 ? candidate.layers : undefined,
            description: descriptionValue || undefined,
        };

        // Optimistically drop the candidate from the local queue and memory cache, then move on:
        // description generation must never stall the reviewer.
        const removedIndex = state.currentIndex;
        state.imageCache.delete(candidate.id);
        if (candidate.layers) {
            candidate.layers.forEach((_, idx) => {
                state.imageCache.delete(`${candidate.id}_layer_${idx}`);
            });
        }
        state.candidates.splice(removedIndex, 1);

        // Maintain index within remaining bounds
        if (state.currentIndex >= state.candidates.length) {
            state.currentIndex = state.candidates.length - 1;
        }

        loadCandidate(state.currentIndex);
        state.pendingCurationIds.add(candidate.id);

        fetch("/api/curate", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
        })
            .then(async (res) => {
                if (!res.ok) {
                    const errJson = await res.json().catch(() => ({}));
                    throw new Error(errJson.error || errJson.message || `HTTP ${res.status}`);
                }
                showToast(`Candidate #${candidate.id} curated`, "success");
                invalidateDatasetReadiness();
            })
            .catch((err) => {
                restoreUncuratedCandidate(candidate, removedIndex);
                showToast(`Curate failed for #${candidate.id}: ${err.message}`, "danger");
            })
            .finally(() => {
                state.pendingCurationIds.delete(candidate.id);
                fetchStatus();
            });
    }

    /**
     * Puts a candidate whose curation request failed back into the local review queue at the
     * position it was curated from. The candidate is still present in staging, so without this
     * the reviewer's queue would silently disagree with the server.
     */
    export function restoreUncuratedCandidate(candidate, removedIndex) {
        if (state.candidates.some((c) => c.id === candidate.id)) {
            return; // Queue was refetched meanwhile and already lists the candidate.
        }
        const insertAt = Math.min(Math.max(removedIndex, 0), state.candidates.length);
        state.candidates.splice(insertAt, 0, candidate);
        if (state.currentIndex < 0) {
            state.currentIndex = insertAt;
        } else if (state.currentIndex >= insertAt) {
            state.currentIndex += 1;
        }
        loadCandidate(state.currentIndex);
    }

    export async function curateCurrentCandidate() {
        exitLabelingMode();
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        // Curate Validation Gate: validate layers upfront before opening decision modal
        const candidate = state.candidates[state.currentIndex];
        if (!validateCandidateLayers(candidate)) {
            return;
        }

        // 16×16 Full-Grid Lattice Curation Gate
        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (!lattice.isEligible) {
            showToast(`Map has only ${lattice.cols}×${lattice.rows} full Grid Cells; at least 16×16 required for curation`, "danger");
            toggleGridPanel(true);
            return;
        }

        await openCurateDecisionModal();
    }

    export function handleDecisionModalKeydown(e) {
        if (e.key === "Escape") {
            closeCurateDecisionModal();
            e.preventDefault();
            return;
        }

        if (decisionState.step === "cards") {
            if (e.key === "ArrowLeft" || e.key === "Left") {
                handleDecisionReject();
                e.preventDefault();
                return;
            }
            if (e.key === "ArrowRight" || e.key === "Right") {
                handleDecisionAccept();
                e.preventDefault();
                return;
            }
            if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
                handleDecisionUndo();
                e.preventDefault();
                return;
            }
            return;
        }

        if (decisionState.step === "custom") {
            if (document.activeElement === dom.decisionDescriptionInput) {
                if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    handleCustomTagSubmit();
                }
                return;
            }

            if (document.activeElement === dom.decisionTagInput) {
                return;
            }

            if (e.key === "Enter") {
                handleCustomTagSubmit();
                e.preventDefault();
                return;
            }
            if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
                handleDecisionUndo();
                e.preventDefault();
                return;
            }
            if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Left" || e.key === "Right") {
                e.preventDefault();
                return;
            }
            return;
        }
    }

    export function updateUndoRejectButtonState() {
        if (dom.btnUndoReject) {
            dom.btnUndoReject.disabled = !state.rejectedStack || state.rejectedStack.length === 0;
        }
    }

    export async function rejectCurrentCandidate() {
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        const removedIndex = state.currentIndex;
        state.isSubmitting = true;

        try {
            const payload = {
                id: candidate.id,
            };

            const res = await fetch("/api/reject", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });

            if (!res.ok) {
                const errJson = await res.json().catch(() => ({}));
                throw new Error(errJson.message || `HTTP ${res.status}`);
            }

            state.rejectedStack.push({ candidate, index: removedIndex });
            updateUndoRejectButtonState();

            showToast(`Candidate #${candidate.id} rejected`, "danger", {
                label: "Undo",
                onClick: () => undoRejectCandidate(candidate.id),
            });

            // Remove rejected item from local queue and cache
            state.imageCache.delete(candidate.id);
            if (candidate.layers) {
                candidate.layers.forEach((_, idx) => {
                    state.imageCache.delete(`${candidate.id}_layer_${idx}`);
                });
            }
            state.candidates.splice(state.currentIndex, 1);

            if (state.currentIndex >= state.candidates.length) {
                state.currentIndex = state.candidates.length - 1;
            }

            loadCandidate(state.currentIndex);
            fetchStatus();
        } catch (err) {
            showToast(`Rejection failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
        }
    }

    export async function undoRejectCandidate(candidateId = null) {
        if (state.isSubmitting) return;

        let stackEntry = null;
        if (candidateId) {
            const idx = state.rejectedStack.findIndex((e) => e.candidate.id === candidateId);
            if (idx >= 0) {
                stackEntry = state.rejectedStack.splice(idx, 1)[0];
            }
        } else if (state.rejectedStack.length > 0) {
            stackEntry = state.rejectedStack.pop();
        }

        updateUndoRejectButtonState();

        const targetId = candidateId || (stackEntry ? stackEntry.candidate.id : null);
        if (!targetId) {
            showToast("No rejected candidate to undo", "info");
            return;
        }

        state.isSubmitting = true;
        try {
            const res = await fetch("/api/reject/undo", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ id: targetId }),
            });

            if (!res.ok) {
                const errJson = await res.json().catch(() => ({}));
                throw new Error(errJson.error || errJson.message || `HTTP ${res.status}`);
            }

            const data = await res.json();
            const restoredCandidate = data.candidate || (stackEntry ? stackEntry.candidate : null);

            if (restoredCandidate) {
                const insertAt = stackEntry
                    ? Math.min(Math.max(stackEntry.index, 0), state.candidates.length)
                    : state.currentIndex >= 0
                    ? state.currentIndex
                    : 0;

                if (!state.candidates.some((c) => c.id === restoredCandidate.id)) {
                    state.candidates.splice(insertAt, 0, restoredCandidate);
                }
                const targetIndex = state.candidates.findIndex((c) => c.id === restoredCandidate.id);
                if (targetIndex >= 0) {
                    state.currentIndex = targetIndex;
                    loadCandidate(state.currentIndex);
                }
                showToast(`Restored candidate #${restoredCandidate.id} to staging queue`, "success");
            } else {
                await fetchStagingQueue();
                showToast(`Restored candidate #${targetId} to staging queue`, "success");
            }
            fetchStatus();
        } catch (err) {
            if (stackEntry) {
                state.rejectedStack.push(stackEntry);
                updateUndoRejectButtonState();
            }
            showToast(`Undo rejection failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
        }
    }
