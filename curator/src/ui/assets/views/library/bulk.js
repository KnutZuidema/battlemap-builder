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
    showToast,
    invalidateDatasetReadiness,
} from "../../app.js";

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
            const res = await fetch(`/api/curated/${encodeURIComponent(id)}`, {
                method: "DELETE",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    reason: reason,
                    notes: notes,
                }),
            });
            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
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
            const res = await fetch("/api/curated/bulk-delete", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    ids: ids,
                    reason: reason,
                }),
            });
            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
            }

            const data = await res.json();
            const deletedCount = data.deleted_count !== undefined ? data.deleted_count : ids.length;

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
    } catch (err) {
        showToast(`Deletion failed: ${err.message}`, "danger");
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

            const res = await fetch(`/api/curated/${encodeURIComponent(id)}`, {
                method: "PATCH",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ tags: currentTags }),
            });

            if (res.ok) {
                const updated = await res.json();
                if (map) {
                    map.tags = updated.tags;
                }
                successCount++;
            }
        }

        applyLibraryFilters();
        if (successCount > 0) invalidateDatasetReadiness();
        showToast(`Added tag "${normalizedTag}" to ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
    } catch (err) {
        showToast(`Bulk tag error: ${err.message}`, "danger");
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
    const failures = [];
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
                const res = await fetch(`/api/curated/${encodeURIComponent(id)}/refine`, {
                    method: "POST",
                });
                if (!res.ok) {
                    const errData = await res.json().catch(() => ({}));
                    throw new Error(errData.error || `HTTP ${res.status}`);
                }
                const updated = await res.json();

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
            } catch (err) {
                failures.push({ id, message: err.message });
                console.error(`AI refine failed for #${id}:`, err);
            }
        }

        if (successCount > 0) {
            applyLibraryFilters();
            invalidateDatasetReadiness();
        }

        if (failures.length === 0) {
            showToast(`AI-refined ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
        } else {
            const failedIds = failures.map(f => `#${f.id}`).join(", ");
            showToast(
                `AI refine failed for ${failures.length} of ${total} maps (${failedIds}): ${failures[0].message}`,
                "danger"
            );
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
        const payload = { ids: Array.from(state.librarySelectedIds) };
        const res = await fetch("/api/dataset/export-selected", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
        });
        if (!res.ok) {
            const errText = await res.text();
            throw new Error(`Export failed (HTTP ${res.status}: ${errText || res.statusText}`);
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

        showToast(`Downloaded LoRA dataset (${formatBytes(blob.size)}) for ${count} map${count === 1 ? "" : "s"}!`, "success");
    } catch (err) {
        showToast(`Export error: ${err.message}`, "danger");
    } finally {
        state.isExporting = false;
        if (dom.btnBulkExport) dom.btnBulkExport.disabled = false;
    }
}
