/**
 * Dataset Library grid: view switching, filtering, facet/sort/density controls,
 * multi-select, keyboard navigation, and the draft LoRA dataset export.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { escapeHtml } from "../kit/dom.js";
import { formatRoleName, formatBytes } from "../kit/format.js";
import { resizeCanvas, requestRender } from "../stage/canvas.js";
import {
    openLibraryFullscreen,
    closeLibraryFullscreen,
    toggleFullscreenGridPanel,
    switchFullscreenLayer,
    setFullscreenZoom100,
    fullscreenZoomStep,
    fullscreenFitToScreen,
    startFsPanLoop,
    startFsZoomLoop,
    activeFsPanKeys,
    activeFsZoomKeys,
    loadFullscreenMap,
    requestFullscreenRender,
} from "./library/fullscreen.js";
import {
    openDeleteCuratedModal,
    openBulkDeleteModal,
    closeDeleteCuratedModal,
    confirmDeleteCurated,
    bulkTagSelectedMaps,
    refineSelectedMaps,
    exportSelectedLoraDataset,
} from "./library/bulk.js";
import {
    openLibraryInspector,
    closeLibraryInspector,
    toggleLibraryInspector,
    saveLibraryInspectorChanges,
} from "./inspector.js";
import { showToast } from "../app.js";
import { datasetScopeTheme, datasetScopeQuery } from "../dataset-scope.js";
import { syncReadinessToScope } from "./readiness.js";
import {
    fetchDatasetSnapshots,
    freezeDatasetSnapshot,
    toggleSnapshotsShelf,
} from "./snapshots.js";

// ==========================================================================
// Dataset Library View & LoRA Export (Issue #31)
// ==========================================================================

export function switchView(view) {
    if (state.libraryFullscreenActive && view !== "library") {
        closeLibraryFullscreen();
    }
    if (state.libraryDrawerActive && view !== "library") {
        closeLibraryInspector();
    }
    state.currentView = view;
    if (view === "triage") {
        dom.tabNavTriage?.classList.add("active");
        dom.tabNavLibrary?.classList.remove("active");
        dom.triageViewport?.classList.remove("hidden");
        dom.libraryViewport?.classList.add("hidden");
        resizeCanvas();
        requestRender();
    } else if (view === "library") {
        dom.tabNavLibrary?.classList.add("active");
        dom.tabNavTriage?.classList.remove("active");
        dom.libraryViewport?.classList.remove("hidden");
        dom.triageViewport?.classList.add("hidden");
        fetchLibraryMaps();
        syncReadinessToScope({ audit: true });
    }
}

export async function fetchLibraryMaps() {
    if (!dom.libraryGrid) return;
    dom.libraryGrid.setAttribute("aria-busy", "true");

    try {
        const res = await fetch("/api/curated");
        if (!res.ok) {
            throw new Error(`HTTP ${res.status}: ${res.statusText}`);
        }
        const maps = await res.json();
        state.curatedMaps = maps;

        applyLibraryFilters();
        fetchDatasetSnapshots();
    } catch (err) {
        showToast(`Failed to load dataset library: ${err.message}`, "danger");
    } finally {
        if (dom.libraryGrid) dom.libraryGrid.setAttribute("aria-busy", "false");
    }
}

export function applyLibraryFilters() {
    if (!state.curatedMaps) return;

    const activeTag = (state.libraryActiveTag || "").toLowerCase().trim();
    const query = (state.librarySearchQuery || "").toLowerCase().trim();
    const facet = state.libraryFacet || "all";

    const filtered = state.curatedMaps.filter((map) => {
        const tags = map.tags || [];
        const layerCount = (map.layers && map.layers.length > 0) ? map.layers.length : 1;
        const hasDesc = Boolean(map.description && map.description.trim());

        // 1. Facet filter
        if (facet === "bundles" && layerCount <= 1) {
            return false;
        }
        if (facet === "single" && layerCount > 1) {
            return false;
        }
        if (facet === "has-desc" && !hasDesc) {
            return false;
        }
        if (facet === "missing-desc" && hasDesc) {
            return false;
        }

        // 2. Active tag filter
        if (activeTag && activeTag !== "all") {
            const matchesTag = tags.some(t => t.toLowerCase() === activeTag);
            if (!matchesTag) {
                return false;
            }
        }

        // 3. Search query filter
        if (query) {
            const matchesId = (map.id || "").toLowerCase().includes(query);
            const matchesTag = tags.some(t => t.toLowerCase().includes(query));
            const matchesDesc = (map.description || "").toLowerCase().includes(query);
            if (!matchesId && !matchesTag && !matchesDesc) {
                return false;
            }
        }

        return true;
    });

    sortCuratedMaps(filtered, state.librarySort);
    state.libraryFilteredMaps = filtered;

    // Keep focused index within valid bounds
    if (state.libraryFocusedIndex >= filtered.length) {
        state.libraryFocusedIndex = filtered.length > 0 ? filtered.length - 1 : -1;
    }

    if (dom.libraryCountBadge) {
        dom.libraryCountBadge.textContent = `${filtered.length} Map${filtered.length === 1 ? "" : "s"}`;
    }

    renderLibraryTagPills();
    renderLibraryGrid(filtered);
}

function sortCuratedMaps(maps, sortKey) {
    if (!maps || maps.length === 0) return;
    switch (sortKey) {
        case "date-asc":
            maps.sort((a, b) => {
                const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                if (aTime !== bTime) return aTime - bTime;
                return (a.id || "").localeCompare(b.id || "");
            });
            break;
        case "layers-desc":
            maps.sort((a, b) => {
                const aLayers = (a.layers && a.layers.length > 0) ? a.layers.length : 1;
                const bLayers = (b.layers && b.layers.length > 0) ? b.layers.length : 1;
                if (bLayers !== aLayers) return bLayers - aLayers;
                const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                return bTime - aTime;
            });
            break;
        case "dimensions-desc":
            maps.sort((a, b) => {
                const aDim = (a.dimensions && Array.isArray(a.dimensions) && a.dimensions.length >= 2)
                    ? (a.dimensions[0] * a.dimensions[1]) : 0;
                const bDim = (b.dimensions && Array.isArray(b.dimensions) && b.dimensions.length >= 2)
                    ? (b.dimensions[0] * b.dimensions[1]) : 0;
                if (bDim !== aDim) return bDim - aDim;
                const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                return bTime - aTime;
            });
            break;
        case "id-asc":
            maps.sort((a, b) => {
                return (a.id || "").localeCompare(b.id || "");
            });
            break;
        case "date-desc":
        default:
            maps.sort((a, b) => {
                const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                if (bTime !== aTime) return bTime - aTime;
                return (b.id || "").localeCompare(a.id || "");
            });
            break;
    }
}

function setLibraryDensity(density) {
    if (!["compact", "comfortable", "large"].includes(density)) return;
    state.libraryDensity = density;
    try {
        localStorage.setItem("curator_library_density", density);
    } catch (_) {}

    if (dom.libraryGrid) {
        dom.libraryGrid.classList.remove("density-compact", "density-comfortable", "density-large");
        dom.libraryGrid.classList.add(`density-${density}`);
    }

    const densityButtons = document.querySelectorAll(".btn-density");
    densityButtons.forEach((btn) => {
        btn.classList.toggle("active", btn.dataset.density === density);
    });
}

function setLibraryFacet(facet) {
    state.libraryFacet = facet;
    const facetButtons = document.querySelectorAll(".lib-facet-chip");
    facetButtons.forEach((btn) => {
        btn.classList.toggle("active", (btn.dataset.facet || "all") === facet);
    });
    applyLibraryFilters();
}

export function clearLibraryFilters() {
    state.librarySearchQuery = "";
    if (dom.librarySearchInput) {
        dom.librarySearchInput.value = "";
    }
    state.libraryActiveTag = null;
    setLibraryFacet("all");
    fetchDatasetSnapshots();
    syncReadinessToScope({ audit: true });
}

function toggleCardSelection(id) {
    if (!id) return;
    if (state.librarySelectedIds.has(id)) {
        state.librarySelectedIds.delete(id);
    } else {
        state.librarySelectedIds.add(id);
    }
    updateCardSelectionVisual(id);
    updateLibraryBulkBar();
}

export function updateLibraryBulkBar() {
    const count = state.librarySelectedIds.size;
    if (dom.bulkSelectedCount) {
        dom.bulkSelectedCount.textContent = `${count} Selected`;
    }
    if (dom.libraryBulkBar) {
        if (count > 0) {
            dom.libraryBulkBar.classList.remove("hidden");
        } else {
            dom.libraryBulkBar.classList.add("hidden");
        }
    }
}

export function clearLibrarySelection() {
    state.librarySelectedIds.clear();
    if (dom.libraryGrid) {
        const cards = dom.libraryGrid.querySelectorAll(".curated-card");
        cards.forEach(card => {
            card.classList.remove("selected");
            const checkbox = card.querySelector(".curated-card-checkbox");
            if (checkbox) checkbox.checked = false;
        });
    }
    updateLibraryBulkBar();
}
function updateCardSelectionVisual(id) {
    if (!dom.libraryGrid) return;
    const cards = dom.libraryGrid.querySelectorAll(".curated-card");
    for (const card of cards) {
        if (card.dataset.id === id) {
            const isSelected = state.librarySelectedIds.has(id);
            card.classList.toggle("selected", isSelected);
            const checkbox = card.querySelector(".curated-card-checkbox");
            if (checkbox) checkbox.checked = isSelected;
            break;
        }
    }
}

export function setLibraryFocus(index, scrollIntoView = true) {
    if (index === -1 || !state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) {
        state.libraryFocusedIndex = -1;
        if (dom.libraryGrid) {
            const cards = dom.libraryGrid.querySelectorAll(".curated-card");
            cards.forEach(c => c.classList.remove("focused"));
        }
        return;
    }
    const maxIdx = state.libraryFilteredMaps.length - 1;
    const newIdx = Math.max(0, Math.min(index, maxIdx));
    state.libraryFocusedIndex = newIdx;

    if (!dom.libraryGrid) return;
    const cards = dom.libraryGrid.querySelectorAll(".curated-card");
    cards.forEach((c, idx) => {
        if (idx === newIdx) {
            c.classList.add("focused");
            if (scrollIntoView) {
                c.scrollIntoView({ block: "nearest", behavior: "smooth" });
            }
        } else {
            c.classList.remove("focused");
        }
    });
}

function getGridColumnCount() {
    if (!dom.libraryGrid) return 1;
    const cards = dom.libraryGrid.querySelectorAll(".curated-card");
    if (cards.length < 2) return 1;
    const firstTop = cards[0].getBoundingClientRect().top;
    let cols = 0;
    for (let i = 0; i < cards.length; i++) {
        if (Math.abs(cards[i].getBoundingClientRect().top - firstTop) < 10) {
            cols++;
        } else {
            break;
        }
    }
    return cols > 0 ? cols : 1;
}

export function handleLibraryKeydown(e) {
    if (state.libraryFullscreenActive) {
        // Escape: close grid panel if open, otherwise exit fullscreen
        if (e.key === "Escape") {
            if (state.libraryFullscreenGridPanelOpen) {
                toggleFullscreenGridPanel(false);
            } else {
                closeLibraryFullscreen();
            }
            e.preventDefault();
            return;
        }

        // WASD: Pan continuous loop
        const keyLower = e.key.toLowerCase();
        if (["w", "a", "s", "d"].includes(keyLower) && !e.ctrlKey && !e.metaKey && !e.altKey) {
            activeFsPanKeys[keyLower] = true;
            startFsPanLoop();
            e.preventDefault();
            return;
        }

        // Zoom in / out: [ and ] or - and + / =
        if (e.key === "]" || e.code === "BracketRight") {
            activeFsZoomKeys.in = true;
            startFsZoomLoop();
            e.preventDefault();
            return;
        }
        if (e.key === "[" || e.code === "BracketLeft") {
            activeFsZoomKeys.out = true;
            startFsZoomLoop();
            e.preventDefault();
            return;
        }
        if (e.key === "=" || e.key === "+") {
            fullscreenZoomStep(1.2);
            e.preventDefault();
            return;
        }
        if (e.key === "-" || e.key === "_") {
            fullscreenZoomStep(1 / 1.2);
            e.preventDefault();
            return;
        }

        // Fit to screen: F or 0
        if ((e.key === "f" || e.key === "F" || e.key === "0") && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
            fullscreenFitToScreen();
            e.preventDefault();
            return;
        }

        // Number keys 1-6 switch displayed layer (or 1 sets 100% zoom if single-layer)
        if (["1", "2", "3", "4", "5", "6"].includes(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
            const targetIdx = parseInt(e.key, 10) - 1;
            const layers = (state.libraryFullscreenMap && state.libraryFullscreenMap.layers) ? state.libraryFullscreenMap.layers : [];
            if (layers.length > 1) {
                if (targetIdx < layers.length) {
                    switchFullscreenLayer(targetIdx);
                }
            } else {
                if (e.key === "1") {
                    setFullscreenZoom100();
                }
            }
            e.preventDefault();
            return;
        }

        // Toggle grid: G
        if ((e.key === "g" || e.key === "G") && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
            state.libraryFullscreenGridVisible = !state.libraryFullscreenGridVisible;
            if (dom.btnFsGridToggle) dom.btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
            requestFullscreenRender();
            e.preventDefault();
            return;
        }

        // Toggle grid calibration panel: Shift+G
        if ((e.key === "g" || e.key === "G") && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
            toggleFullscreenGridPanel();
            e.preventDefault();
            return;
        }

        // Navigate maps: J / Right (Next), K / Left (Prev)
        if (e.key === "j" || e.key === "J" || e.key === "ArrowRight") {
            if (state.libraryFilteredMaps && state.libraryFullscreenIndex < state.libraryFilteredMaps.length - 1) {
                loadFullscreenMap(state.libraryFullscreenIndex + 1, 0);
                setLibraryFocus(state.libraryFullscreenIndex, true);
            }
            e.preventDefault();
            return;
        }
        if (e.key === "k" || e.key === "K" || e.key === "ArrowLeft") {
            if (state.libraryFilteredMaps && state.libraryFullscreenIndex > 0) {
                loadFullscreenMap(state.libraryFullscreenIndex - 1, 0);
                setLibraryFocus(state.libraryFullscreenIndex, true);
            }
            e.preventDefault();
            return;
        }

        return;
    }

    // --- Standard Library View Hotkeys (when fullscreen is NOT active) ---
    const activeEl = document.activeElement;
    const tag = activeEl ? activeEl.tagName.toLowerCase() : "";
    const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

    // Deletion modal hotkeys
    if (state.deleteModalOpen) {
        if (e.key === "Escape") {
            closeDeleteCuratedModal();
            e.preventDefault();
            return;
        }
        if (e.key === "Enter" && !isInputFocused) {
            confirmDeleteCurated();
            e.preventDefault();
            return;
        }
        return;
    }

    // Deselect all on Escape if selection is active and drawer is not open
    if (e.key === "Escape" && !state.libraryDrawerActive) {
        if (state.librarySelectedIds.size > 0) {
            clearLibrarySelection();
            e.preventDefault();
            return;
        }
    }
    // Inspector Drawer hotkeys
    if (state.libraryDrawerActive) {
        // Save Changes: Ctrl+S or Cmd+S
        if ((e.ctrlKey || e.metaKey) && (e.key === "s" || e.key === "S")) {
            saveLibraryInspectorChanges();
            e.preventDefault();
            return;
        }

        // Close Drawer: Esc
        if (e.key === "Escape") {
            closeLibraryInspector();
            e.preventDefault();
            return;
        }

        // Toggle Drawer: E (only when not typing in text field)
        if ((e.key === "e" || e.key === "E") && !e.ctrlKey && !e.metaKey && !e.altKey && !isInputFocused) {
            closeLibraryInspector();
            e.preventDefault();
            return;
        }

        // If an input is focused inside the drawer, don't execute other grid navigation hotkeys
        if (isInputFocused) return;
    }

    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isInputFocused) return;

    // Single Delete & Reject hotkey: Delete or Backspace on focused card or active drawer
    if (e.key === "Delete" || e.key === "Backspace") {
        if (state.libraryDrawerActive && state.libraryDrawerMap) {
            openDeleteCuratedModal(state.libraryDrawerMap.id);
            e.preventDefault();
            return;
        }
        if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
            const map = state.libraryFilteredMaps[state.libraryFocusedIndex];
            if (map) {
                openDeleteCuratedModal(map.id);
                e.preventDefault();
                return;
            }
        }
    }

    // Toggle / Open drawer on focused card: E
    if (e.key === "e" || e.key === "E") {
        toggleLibraryInspector();
        e.preventDefault();
        return;
    }

    // Open drawer on Enter or Space on focused card:
    if (e.key === "Enter" || e.key === " ") {
        if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
            openLibraryInspector(state.libraryFilteredMaps[state.libraryFocusedIndex]);
            e.preventDefault();
            return;
        }
    }
    // Open fullscreen with F on focused card:
    if ((e.key === "f" || e.key === "F") && !e.shiftKey) {
        if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
            const targetIdx = state.libraryFocusedIndex >= 0 ? state.libraryFocusedIndex : 0;
            openLibraryFullscreen(targetIdx);
        }
        e.preventDefault();
        return;
    }

    // Density hotkeys: 1, 2, 3
    if (e.key === "1") {
        setLibraryDensity("compact");
        e.preventDefault();
        return;
    }
    if (e.key === "2") {
        setLibraryDensity("comfortable");
        e.preventDefault();
        return;
    }
    if (e.key === "3") {
        setLibraryDensity("large");
        e.preventDefault();
        return;
    }

    // Toggle selection on focused card: X / x
    if (e.key === "x" || e.key === "X") {
        if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
            const map = state.libraryFilteredMaps[state.libraryFocusedIndex];
            if (map) {
                toggleCardSelection(map.id);
            }
        }
        e.preventDefault();
        return;
    }

    // Navigation hotkeys: J/K or ArrowDown/ArrowUp/ArrowLeft/ArrowRight
    if (["j", "J", "ArrowDown", "k", "K", "ArrowUp", "ArrowRight", "ArrowLeft"].includes(e.key)) {
        if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;

        const count = state.libraryFilteredMaps.length;
        const cols = getGridColumnCount();

        if (state.libraryFocusedIndex === -1) {
            setLibraryFocus(0);
            e.preventDefault();
            return;
        }

        let newIdx = state.libraryFocusedIndex;
        if (e.key === "j" || e.key === "J") {
            newIdx = Math.min(count - 1, newIdx + 1);
        } else if (e.key === "k" || e.key === "K") {
            newIdx = Math.max(0, newIdx - 1);
        } else if (e.key === "ArrowRight") {
            newIdx = Math.min(count - 1, newIdx + 1);
        } else if (e.key === "ArrowLeft") {
            newIdx = Math.max(0, newIdx - 1);
        } else if (e.key === "ArrowDown") {
            newIdx = Math.min(count - 1, newIdx + cols);
        } else if (e.key === "ArrowUp") {
            newIdx = Math.max(0, newIdx - cols);
        }

        setLibraryFocus(newIdx);
        e.preventDefault();
        return;
    }
}

function renderLibraryTagPills() {
    if (!dom.libraryTagPills) return;
    dom.libraryTagPills.innerHTML = "";

    // Dynamically calculate unique tags across all curated maps
    const tagMap = new Map();
    for (const map of state.curatedMaps) {
        const tags = map.tags || [];
        for (const t of tags) {
            if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
        }
    }

    const sortedKeys = Array.from(tagMap.keys()).sort();
    const fragment = document.createDocumentFragment();

    // "All" pill
    const allBtn = document.createElement("button");
    allBtn.type = "button";
    allBtn.className = `lib-tag-pill ${(!state.libraryActiveTag || state.libraryActiveTag === "all") ? "active" : ""}`;
    allBtn.textContent = "All";
    allBtn.addEventListener("click", () => {
        state.libraryActiveTag = null;
        applyLibraryFilters();
        fetchDatasetSnapshots();
        syncReadinessToScope({ audit: true });
    });
    fragment.appendChild(allBtn);

    // Individual tag pills
    for (const key of sortedKeys) {
        const displayName = tagMap.get(key);
        const pill = document.createElement("button");
        pill.type = "button";
        const isActive = state.libraryActiveTag && state.libraryActiveTag.toLowerCase() === key;
        pill.className = `lib-tag-pill ${isActive ? "active" : ""}`;
        pill.textContent = displayName;
        pill.dataset.tag = displayName;

        pill.addEventListener("click", () => {
            if (state.libraryActiveTag && state.libraryActiveTag.toLowerCase() === key) {
                state.libraryActiveTag = null;
            } else {
                state.libraryActiveTag = displayName;
            }
            applyLibraryFilters();
            fetchDatasetSnapshots();
            syncReadinessToScope({ audit: true });
        });

        fragment.appendChild(pill);
    }

    dom.libraryTagPills.appendChild(fragment);
}

function renderLibraryGrid(maps) {
    if (!dom.libraryGrid || !dom.libraryEmpty) return;
    dom.libraryGrid.innerHTML = "";

    const hasAnyCuratedMaps = state.curatedMaps && state.curatedMaps.length > 0;
    const hasMatchingMaps = maps && maps.length > 0;

    const showEmpty = !hasAnyCuratedMaps;
    const showFilterEmpty = hasAnyCuratedMaps && !hasMatchingMaps;
    const showGrid = hasAnyCuratedMaps && hasMatchingMaps;

    dom.libraryEmpty.classList.toggle("hidden", !showEmpty);
    if (dom.libraryFilterEmpty) dom.libraryFilterEmpty.classList.toggle("hidden", !showFilterEmpty);
    dom.libraryGrid.classList.toggle("hidden", !showGrid);

    if (!showGrid) return;
    const fragment = document.createDocumentFragment();
    maps.forEach((map, idx) => {
        fragment.appendChild(renderCuratedCard(map, idx));
    });

    dom.libraryGrid.appendChild(fragment);
}

function getShortRoleChip(role, elevation) {
    const r = (role || "").toLowerCase().replace(/[-_]/g, "");
    if (r === "ground") {
        return "G";
    }
    if (r === "upperfloor" || r === "upper") {
        const elev = (typeof elevation === "number" && elevation !== 0) ? Math.abs(elevation) : 1;
        return `U+${elev}`;
    }
    if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
        const elev = (typeof elevation === "number" && elevation !== 0) ? -Math.abs(elevation) : -1;
        return `B${elev}`;
    }
    if (r === "roof") {
        return "Roof";
    }
    if (r === "variant") {
        return "Var";
    }
    return (role || "").slice(0, 3).toUpperCase() || "?";
}

export function getScrubPillRoleClass(role) {
    const r = (role || "").toLowerCase().replace(/[-_]/g, "");
    if (r === "ground") return "role-ground";
    if (r === "upperfloor" || r === "upper") return "role-upper";
    if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "role-basement";
    if (r === "roof") return "role-roof";
    if (r === "variant") return "role-variant";
    return "role-default";
}

function computeElevationSpan(layers) {
    if (!layers || layers.length <= 1) return null;
    let minElev = null;
    let maxElev = null;
    let hasElevations = false;

    for (const l of layers) {
        const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
        let elev = null;
        if (typeof l.elevation === "number") {
            elev = l.elevation;
        } else if (r === "ground") {
            elev = 0;
        } else if (r === "upperfloor" || r === "upper") {
            elev = 1;
        } else if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
            elev = -1;
        }

        if (elev !== null) {
            hasElevations = true;
            if (minElev === null || elev < minElev) minElev = elev;
            if (maxElev === null || elev > maxElev) maxElev = elev;
        }
    }

    if (!hasElevations || minElev === null || maxElev === null || minElev === maxElev) {
        return null;
    }

    const fmt = (n) => (n > 0 ? `+${n}` : n === 0 ? "0" : `−${Math.abs(n)}`);
    return `${fmt(minElev)} to ${fmt(maxElev)}`;
}

function renderCuratedCard(map, index) {
    const card = document.createElement("div");
    const isSelected = state.librarySelectedIds.has(map.id);
    const isFocused = state.libraryFocusedIndex === index;
    card.className = `curated-card${isSelected ? " selected" : ""}${isFocused ? " focused" : ""}`;
    card.dataset.id = map.id;
    const tags = map.tags || [];
    const primaryTheme = tags[0] || "dungeon";
    card.dataset.theme = primaryTheme;

    const thumbUrl = `/api/image/curated/${encodeURIComponent(map.id)}`;
    const layerCount = (map.layers && map.layers.length > 0) ? map.layers.length : 1;
    const layerLabel = `${layerCount} Layer${layerCount === 1 ? "" : "s"}`;
    const isBundle = map.layers && Array.isArray(map.layers) && map.layers.length > 1;

    const elevationSpan = isBundle ? computeElevationSpan(map.layers) : null;
    const elevationBadgeHtml = elevationSpan
        ? `<span class="curated-elevation-badge" title="Elevation span: ${elevationSpan}">${escapeHtml(elevationSpan)}</span>`
        : "";

    let scrubberHtml = "";
    if (isBundle) {
        const groundIndex = map.layers.findIndex(l => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "ground";
        });
        const defaultIndex = groundIndex >= 0 ? groundIndex : 0;

        const pillsHtml = map.layers.map((layer, idx) => {
            const shortRole = getShortRoleChip(layer.role, layer.elevation);
            const roleCls = getScrubPillRoleClass(layer.role);
            const isActive = idx === defaultIndex;
            const fullRole = formatRoleName(layer.role, layer.elevation);
            const elevText = (typeof layer.elevation === "number")
                ? `Elevation: ${layer.elevation >= 0 ? `+${layer.elevation}` : layer.elevation}`
                : "Elevation: 0";
            const tooltipParts = [fullRole, elevText, layer.filename || ""].filter(Boolean);
            const tooltip = tooltipParts.join(" • ");
            const layerIdx = layer.index !== undefined ? layer.index : idx;

            return `<button type="button" class="layer-scrub-pill ${roleCls}${isActive ? " active" : ""}" data-layer-idx="${layerIdx}" title="${escapeHtml(tooltip)}">${escapeHtml(shortRole)}</button>`;
        }).join("");

        scrubberHtml = `<div class="curated-layer-scrubber">${pillsHtml}</div>`;
    }

    const pills = tags.map(
        t => `<span class="curated-tag-pill">${escapeHtml(t)}</span>`
    );

    const tagsHtml = pills.length > 0
        ? pills.join("")
        : `<span class="curated-tag-pill text-muted">no tags</span>`;

    const descriptionText = (map.description && map.description.trim())
        ? map.description.trim()
        : (map.layers && map.layers[0] && map.layers[0].caption
            ? map.layers[0].caption
            : `dungeonforge battlemap, ${primaryTheme}`);

    card.innerHTML = `
        <div class="curated-thumb-box">
            <div class="curated-select-wrapper" title="Toggle selection (X)">
                <input type="checkbox" class="curated-card-checkbox" ${isSelected ? "checked" : ""} aria-label="Select battlemap ${escapeHtml(map.id)}" />
            </div>
            <img class="curated-thumb-img" src="${thumbUrl}" alt="Map ${escapeHtml(map.id)}" loading="lazy">
            <div class="curated-thumb-overlay">
                <span class="curated-theme-badge">${escapeHtml(primaryTheme)}</span>
                <div class="curated-overlay-badges">
                    ${elevationBadgeHtml}
                    <span class="curated-layers-badge">${layerLabel}</span>
                </div>
            </div>
            ${scrubberHtml}
        </div>
        <div class="curated-card-body">
            <div class="curated-header-row">
                <span class="curated-card-id mono" title="${escapeHtml(map.id)}">${escapeHtml(map.id)}</span>
                <div class="curated-card-actions">
                    <button type="button" class="btn-card-edit" title="Edit Metadata &amp; Refine (Hotkey: E or Enter)" aria-label="Edit Map">
                        <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
                        </svg>
                        <span>Edit</span>
                    </button>
                    <button type="button" class="btn-card-fullscreen" title="Inspect Fullscreen (Hotkey: F)" aria-label="Inspect Fullscreen">
                        <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                            <polyline points="15 3 21 3 21 9"/>
                            <polyline points="9 21 3 21 3 15"/>
                            <line x1="21" y1="3" x2="14" y2="10"/>
                            <line x1="3" y1="21" x2="10" y2="14"/>
                        </svg>
                        <span>Inspect</span>
                    </button>
                    <button type="button" class="btn-card-delete" title="Delete &amp; Reject Map (Hotkey: Delete)" aria-label="Delete Map">
                        <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                            <polyline points="3 6 5 6 21 6"/>
                            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                            <line x1="10" y1="11" x2="10" y2="17"/>
                            <line x1="14" y1="11" x2="14" y2="17"/>
                        </svg>
                        <span>Delete</span>
                    </button>
                </div>
            </div>
            <div class="curated-tags-row">
                ${tagsHtml}
            </div>
            <div class="curated-description-box">
                <span class="curated-description-label">DungeonForge Prompt Description</span>
                <div class="curated-description-text">${escapeHtml(descriptionText)}</div>
            </div>
        </div>
    `;

    if (isBundle) {
        const scrubber = card.querySelector(".curated-layer-scrubber");
        const thumbImg = card.querySelector(".curated-thumb-img");

        if (scrubber && thumbImg) {
            const groundIndex = map.layers.findIndex(l => {
                const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                return r === "ground";
            });
            const defaultIndex = groundIndex >= 0 ? groundIndex : 0;
            const pillButtons = scrubber.querySelectorAll(".layer-scrub-pill");

            const activateLayer = (layer, pill) => {
                pillButtons.forEach(p => p.classList.remove("active"));
                pill.classList.add("active");

                const layerIdx = layer.index !== undefined ? layer.index : parseInt(pill.dataset.layerIdx, 10);
                const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

                if (thumbImg.src !== directUrl && !thumbImg.src.endsWith(directUrl)) {
                    thumbImg.src = directUrl;
                }
            };

            pillButtons.forEach((pill, idx) => {
                const layer = map.layers[idx];
                if (!layer) return;

                pill.addEventListener("mouseenter", () => {
                    activateLayer(layer, pill);
                });

                pill.addEventListener("click", (e) => {
                    e.stopPropagation();
                    activateLayer(layer, pill);
                });

                pill.addEventListener("touchstart", (e) => {
                    e.stopPropagation();
                    activateLayer(layer, pill);
                }, { passive: true });
            });

            scrubber.addEventListener("mouseleave", () => {
                pillButtons.forEach((p, idx) => {
                    p.classList.toggle("active", idx === defaultIndex);
                });
                thumbImg.onerror = null;
                thumbImg.src = thumbUrl;
            });
        }
    }

    const selectWrapper = card.querySelector(".curated-select-wrapper");
    const checkbox = card.querySelector(".curated-card-checkbox");
    checkbox?.addEventListener("click", (e) => {
        e.stopPropagation();
    });
    checkbox?.addEventListener("change", (e) => {
        e.stopPropagation();
        toggleCardSelection(map.id);
    });
    selectWrapper?.addEventListener("click", (e) => {
        if (e.target === checkbox) return;
        e.stopPropagation();
        toggleCardSelection(map.id);
    });

    card.addEventListener("click", (e) => {
        if (e.target.closest(".curated-select-wrapper")) return;
        if (e.target.closest(".curated-layer-scrubber")) return;
        if (e.target.closest(".curated-card-actions")) return;
        if (e.shiftKey) {
            toggleCardSelection(map.id);
            return;
        }
        setLibraryFocus(index, false);
    });

    const btnEdit = card.querySelector(".btn-card-edit");
    btnEdit?.addEventListener("click", (e) => {
        e.stopPropagation();
        setLibraryFocus(index, false);
        openLibraryInspector(map);
    });

    const btnInspect = card.querySelector(".btn-card-fullscreen");
    btnInspect?.addEventListener("click", (e) => {
        e.stopPropagation();
        openLibraryFullscreen(index);
    });

    const btnDelete = card.querySelector(".btn-card-delete");
    btnDelete?.addEventListener("click", (e) => {
        e.stopPropagation();
        openDeleteCuratedModal(map.id);
    });
    const thumbBox = card.querySelector(".curated-thumb-box");
    thumbBox?.addEventListener("dblclick", (e) => {
        if (e.target.closest(".curated-select-wrapper")) return;
        if (e.target.closest(".curated-layer-scrubber")) return;
        e.stopPropagation();
        openLibraryFullscreen(index);
    });

    return card;
}

async function exportLoraDataset() {
    if (state.isExporting) return;
    state.isExporting = true;
    if (dom.btnExportLora) dom.btnExportLora.disabled = true;
    if (dom.btnExportLabel) dom.btnExportLabel.textContent = "Packaging ZIP...";

    const tag = state.libraryActiveTag || "all";
    const tagLabel = tag === "all" ? "all maps" : tag;
    showToast(`Packaging LoRA dataset ZIP archive (${tagLabel})...`, "info");

    try {
        const url = `/api/dataset/export${datasetScopeQuery()}`;
        const res = await fetch(url);
        if (!res.ok) {
            throw new Error(`Export failed (HTTP ${res.status}: ${res.statusText})`);
        }

        const blob = await res.blob();
        const filename = "lora-dataset.zip";

        const downloadUrl = window.URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = downloadUrl;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        a.remove();
        window.URL.revokeObjectURL(downloadUrl);

        showToast(`LoRA dataset downloaded (${formatBytes(blob.size)})!`, "success");
    } catch (err) {
        showToast(`Export error: ${err.message}`, "danger");
    } finally {
        state.isExporting = false;
        if (dom.btnExportLora) dom.btnExportLora.disabled = false;
        if (dom.btnExportLabel) dom.btnExportLabel.textContent = "Export LoRA Dataset";
    }
}

export function setupLibraryControls() {
    dom.tabNavTriage?.addEventListener("click", () => switchView("triage"));
    dom.tabNavLibrary?.addEventListener("click", () => switchView("library"));
    dom.btnSwitchToTriage?.addEventListener("click", () => switchView("triage"));
    dom.btnRefreshLibrary?.addEventListener("click", () => fetchLibraryMaps());
    dom.btnExportLora?.addEventListener("click", () => exportLoraDataset());
    dom.btnClearLibraryFilters?.addEventListener("click", () => clearLibraryFilters());
    dom.btnFreezeSnapshot?.addEventListener("click", () => freezeDatasetSnapshot());
    dom.btnRefreshSnapshots?.addEventListener("click", () => fetchDatasetSnapshots());
    dom.btnToggleSnapshots?.addEventListener("click", () => toggleSnapshotsShelf());

    dom.librarySearchInput?.addEventListener("input", (e) => {
        state.librarySearchQuery = e.target.value;
        applyLibraryFilters();
    });

    const facetButtons = document.querySelectorAll(".lib-facet-chip");
    facetButtons.forEach((btn) => {
        btn.addEventListener("click", () => {
            const facet = btn.dataset.facet || "all";
            setLibraryFacet(facet);
        });
    });

    dom.librarySortSelect?.addEventListener("change", (e) => {
        state.librarySort = e.target.value;
        applyLibraryFilters();
    });

    const densityButtons = document.querySelectorAll(".btn-density");
    densityButtons.forEach((btn) => {
        btn.addEventListener("click", () => {
            const density = btn.dataset.density;
            if (density) {
                setLibraryDensity(density);
            }
        });
    });

    setLibraryDensity(state.libraryDensity);
    if (dom.librarySortSelect) {
        dom.librarySortSelect.value = state.librarySort;
    }

    // Delete Modal & Bulk Operations Controls (Issue #137)
    dom.btnCloseDeleteModal?.addEventListener("click", () => closeDeleteCuratedModal());
    dom.btnCancelDelete?.addEventListener("click", () => closeDeleteCuratedModal());
    dom.btnConfirmDelete?.addEventListener("click", () => confirmDeleteCurated());
    dom.deleteCuratedModal?.addEventListener("click", (e) => {
        if (e.target === dom.deleteCuratedModal) {
            closeDeleteCuratedModal();
        }
    });

    dom.btnBulkDelete?.addEventListener("click", () => openBulkDeleteModal());
    dom.btnBulkTag?.addEventListener("click", () => bulkTagSelectedMaps());
    dom.btnBulkRefine?.addEventListener("click", () => refineSelectedMaps());
    dom.btnBulkExport?.addEventListener("click", () => exportSelectedLoraDataset());
    dom.btnBulkDeselect?.addEventListener("click", () => clearLibrarySelection());
}
