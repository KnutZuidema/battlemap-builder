/**
 * Deletion safeguard modal and the multi-select bulk actions: tagging, one-at-a-time
 * AI refinement, and exporting the selected maps as a LoRA dataset.
 */

import { dom } from "../../dom.js";
import { state } from "../../store.js";
import { escapeHtml } from "../../kit/dom.js";
import { formatBytes } from "../../kit/format.js";
import {
    applyLibraryFilters,
    clearLibrarySelection,
    updateLibraryBulkBar,
} from "../library.js";
import {
    closeLibraryInspector,
    renderInspectorLayers,
    renderInspectorThemeChips,
    renderInspectorTagChips,
    updateInspectorDescriptionCounter,
} from "../inspector.js";
import { showToast } from "../../shell/status.js";
import { invalidateDatasetReadiness } from "../readiness.js";
import {
    bulkDeleteCuratedMaps,
    deleteCuratedMap,
    exportSelectedMaps,
    refineCuratedMap,
    saveCuratedMap,
} from "../../requests/library.js";

// ==========================================================================
// Deletion Safeguard Modal & Multi-Select Bulk Operations (Issue #137)
// ==========================================================================

export function openDeleteCuratedModal(mapId) {
    if (!mapId) return;
    state.deleteModalOpen = true;
    state.deleteModalTarget = { type: "single", id: mapId };

    if (dom.deleteCuratedTitle) {
        dom.deleteCuratedTitle.textContent = "Delete & Reject Curated Map";
    }
    if (dom.deleteTargetPreview) {
        dom.deleteTargetPreview.innerHTML = `<span class="delete-target-id mono">${escapeHtml(mapId)}</span>`;
    }
    if (dom.deleteReasonSelect) {
        dom.deleteReasonSelect.value = "unspecified";
    }
    if (dom.deleteNotesInput) {
        dom.deleteNotesInput.value = "";
    }
    if (dom.deleteNotesGroup) {
        dom.deleteNotesGroup.style.display = "";
    }
    if (dom.deleteCuratedModal) {
        dom.deleteCuratedModal.classList.remove("hidden");
    }
}

export function openBulkDeleteModal() {
    if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
    const ids = Array.from(state.librarySelectedIds);
    state.deleteModalOpen = true;
    state.deleteModalTarget = { type: "bulk", ids: ids };

    if (dom.deleteCuratedTitle) {
        dom.deleteCuratedTitle.textContent = "Delete & Reject Selected Maps";
    }
    if (dom.deleteTargetPreview) {
        dom.deleteTargetPreview.innerHTML = `<span class="delete-target-count mono">${ids.length} Curated Map${ids.length === 1 ? "" : "s"} Selected for Permanent Deletion</span>`;
    }
    if (dom.deleteReasonSelect) {
        dom.deleteReasonSelect.value = "unspecified";
    }
    if (dom.deleteNotesInput) {
        dom.deleteNotesInput.value = "";
    }
    if (dom.deleteNotesGroup) {
        dom.deleteNotesGroup.style.display = "none";
    }
    if (dom.deleteCuratedModal) {
        dom.deleteCuratedModal.classList.remove("hidden");
    }
}

export function closeDeleteCuratedModal() {
    state.deleteModalOpen = false;
    state.deleteModalTarget = null;
    if (dom.deleteCuratedModal) {
        dom.deleteCuratedModal.classList.add("hidden");
    }
}

export async function confirmDeleteCurated() {
    if (!state.deleteModalTarget) return;
    if (dom.btnConfirmDelete) dom.btnConfirmDelete.disabled = true;

    const reason = dom.deleteReasonSelect ? dom.deleteReasonSelect.value : "unspecified";
    const notes = dom.deleteNotesInput && dom.deleteNotesInput.value.trim() ? dom.deleteNotesInput.value.trim() : null;

    try {
        if (state.deleteModalTarget.type === "single") {
            const id = state.deleteModalTarget.id;
            try {
                await deleteCuratedMap(id, { reason, notes });
            } catch (_) {
                // The failure policy names the operation; the modal stays open for a retry.
                return;
            }

            // Remove map from state.curatedMaps
            state.curatedMaps = state.curatedMaps.filter(m => m.id !== id);
            if (state.librarySelectedIds.has(id)) {
                state.librarySelectedIds.delete(id);
                updateLibraryBulkBar();
            }

            // Close drawer if open for this map
            if (state.libraryDrawerActive && state.libraryDrawerMap && state.libraryDrawerMap.id === id) {
                closeLibraryInspector();
            }

            const cardEl = dom.libraryGrid ? dom.libraryGrid.querySelector(`.curated-card[data-id="${id}"]`) : null;
            if (cardEl) {
                cardEl.remove();
            }

            applyLibraryFilters();
            closeDeleteCuratedModal();
            invalidateDatasetReadiness();
            showToast(`Deleted curated map ${id} and logged rejection`, "danger");
        } else if (state.deleteModalTarget.type === "bulk") {
            const ids = state.deleteModalTarget.ids;
            let data;
            try {
                data = await bulkDeleteCuratedMaps(ids, { reason });
            } catch (_) {
                // The failure policy names the operation; the modal stays open for a retry.
                return;
            }
            const deletedCount = data?.deleted_count !== undefined ? data.deleted_count : ids.length;

            const idSet = new Set(ids);
            state.curatedMaps = state.curatedMaps.filter(m => !idSet.has(m.id));

            if (state.libraryDrawerActive && state.libraryDrawerMap && idSet.has(state.libraryDrawerMap.id)) {
                closeLibraryInspector();
            }

            ids.forEach(id => {
                const cardEl = dom.libraryGrid ? dom.libraryGrid.querySelector(`.curated-card[data-id="${id}"]`) : null;
                if (cardEl) cardEl.remove();
            });

            clearLibrarySelection();
            applyLibraryFilters();
            closeDeleteCuratedModal();
            invalidateDatasetReadiness();
            showToast(`Deleted ${deletedCount} curated map${deletedCount === 1 ? "" : "s"} and logged rejection`, "danger");
        }
    } catch (_) {
        // The failure policy named the operation; the modal stays open for a retry.
    } finally {
        if (dom.btnConfirmDelete) dom.btnConfirmDelete.disabled = false;
    }
}

export async function bulkTagSelectedMaps() {
    if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
    const ids = Array.from(state.librarySelectedIds);

    const tagInput = window.prompt(`Enter tag to add to ${ids.length} selected map${ids.length === 1 ? "" : "s"}:`);
    if (!tagInput) return;
    const normalizedTag = tagInput.trim().toLowerCase().replace(/^#+/, "");
    if (!normalizedTag) return;

    if (dom.btnBulkTag) dom.btnBulkTag.disabled = true;

    let successCount = 0;
    try {
        for (const id of ids) {
            const map = state.curatedMaps.find(m => m.id === id);
            const currentTags = map && Array.isArray(map.tags) ? [...map.tags] : [];
            if (!currentTags.map(t => t.toLowerCase()).includes(normalizedTag)) {
                currentTags.push(normalizedTag);
            }

            try {
                const updated = await saveCuratedMap(id, { tags: currentTags });
                if (map) {
                    map.tags = updated.tags;
                }
                successCount++;
            } catch (_) {
                // The failure policy names the operation for the batch; the rest still tag.
            }
        }

        applyLibraryFilters();
        if (successCount > 0) invalidateDatasetReadiness();
        if (successCount > 0) {
            showToast(`Added tag "${normalizedTag}" to ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
        }
    } finally {
        if (dom.btnBulkTag) dom.btnBulkTag.disabled = false;
    }
}

export async function refineSelectedMaps() {
    if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
    if (state.isBulkRefining) return;

    // Snapshot the selection: the loop outlives a selection change, and every map is refined
    // one at a time so the progress label and the readiness audit never interleave.
    const ids = Array.from(state.librarySelectedIds);
    const total = ids.length;
    let successCount = 0;

    state.isBulkRefining = true;
    if (dom.btnBulkRefine) dom.btnBulkRefine.disabled = true;
    const setLabel = (text) => {
        if (dom.btnBulkRefineLabel) dom.btnBulkRefineLabel.textContent = text;
    };
    setLabel(`Refining 0/${total}…`);

    try {
        for (let i = 0; i < total; i++) {
            const id = ids[i];
            setLabel(`Refining ${i + 1}/${total}…`);
            try {
                const updated = await refineCuratedMap(id);

                const idx = state.curatedMaps.findIndex(m => m.id === id);
                if (idx !== -1) state.curatedMaps[idx] = updated;

                if (state.libraryDrawerActive && state.libraryDrawerMap && state.libraryDrawerMap.id === id) {
                    state.libraryDrawerMap = updated;
                    // Unsaved operator edits win: only an untouched draft picks up the
                    // refined tags, description and layers.
                    if (!state.libraryDraftDirty) {
                        state.libraryDraft = {
                            tags: Array.isArray(updated.tags) ? [...updated.tags] : [],
                            description: updated.description || "",
                            layers: (updated.layers && updated.layers.length > 0)
                                ? updated.layers.map(l => ({ ...l }))
                                : state.libraryDraft.layers,
                        };
                        renderInspectorLayers();
                        renderInspectorThemeChips();
                        renderInspectorTagChips();
                        if (dom.inspectorDescriptionInput) {
                            dom.inspectorDescriptionInput.value = state.libraryDraft.description;
                        }
                        updateInspectorDescriptionCounter(state.libraryDraft.description);
                    }
                }

                successCount++;
            } catch (_) {
                // The failure policy names the operation on the first failed map and stays
                // quiet for the retries; the label keeps counting through the batch.
            }
        }

        if (successCount > 0) {
            applyLibraryFilters();
            invalidateDatasetReadiness();
        }

        if (successCount > 0) {
            showToast(`AI-refined ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
        }
    } finally {
        state.isBulkRefining = false;
        if (dom.btnBulkRefine) dom.btnBulkRefine.disabled = false;
        setLabel("AI Refine Selected");
    }
}

export async function exportSelectedLoraDataset() {
    if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
    if (state.isExporting) return;
    state.isExporting = true;
    if (dom.btnBulkExport) dom.btnBulkExport.disabled = true;

    const count = state.librarySelectedIds.size;
    showToast(`Packaging ${count} selected maps into LoRA dataset ZIP...`, "info");

    try {
        const blob = await exportSelectedMaps(Array.from(state.librarySelectedIds));
        const filename = "lora-dataset.zip";

        const downloadUrl = window.URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = downloadUrl;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        a.remove();
        window.URL.revokeObjectURL(downloadUrl);

        showToast(`Downloaded LoRA dataset (${formatBytes(blob.size)}) for ${count} map${count === 1 ? "" : "s"}!`, "success");
    } catch (_) {
        // The failure policy names the operation and carries the server's reason.
    } finally {
        state.isExporting = false;
        if (dom.btnBulkExport) dom.btnBulkExport.disabled = false;
    }
}
