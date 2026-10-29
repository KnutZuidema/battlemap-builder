/**
 * Frozen Dataset Snapshots: the snapshot shelf, streaming freeze progress, freeze status
 * polling and snapshot deletion (Issue #216).
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { escapeHtml } from "../kit/dom.js";
import { formatBytes } from "../kit/format.js";
import { showToast } from "../shell/status.js";
import { datasetScopeTheme } from "../dataset-scope.js";
import {
    deleteDatasetSnapshot as requestDeleteSnapshot,
    fetchDatasetSnapshots as requestDatasetSnapshots,
    fetchFreezeStatus,
    freezeDatasetSnapshot as requestFreezeSnapshot,
} from "../requests/snapshots.js";
import {
    applyReadinessReport,
    clearReadinessCache,
    readinessScopeKey,
    renderReadinessPanel,
    revealReadinessPanel,
} from "./readiness.js";
import {
    openTrainingDrawer,
    populateTrainingSnapshotSelect,
} from "./training.js";

// ==========================================================================
// Frozen Dataset Snapshots (Issue #216)
// ==========================================================================

/**
 * A refused freeze carries the full audit, so the drawer renders it and the operator fixes the
 * flagged maps in place. No separate modal: the readiness block outlives the refusal.
 *
 * `toast` is off when the refusal arrived as a non-OK response: the failure policy has already
 * raised the pill and toasted the operation, and a second toast would say the same thing.
 */
export function showFreezeRefusal(refusal, scopeTheme, { toast = true } = {}) {
    const readiness = refusal && refusal.readiness;
    const error = (refusal && refusal.error) || refusal?.message || "Dataset is not ready";
    // The refusal audited the scope that was frozen, which the operator may have left since:
    // store it under that scope, never under whichever tag happens to be active now.
    const scopeKey = (scopeTheme || "").toLowerCase() || "all";
    if (readiness && Array.isArray(readiness.maps)) {
        applyReadinessReport(scopeKey, readiness, new Date().toISOString());
    } else if (scopeKey === readinessScopeKey()) {
        // The refusal already invalidated the frozen scope, which is still the one on screen.
        clearReadinessCache();
        state.readinessReport = null;
        state.readinessAuditedAt = null;
    }
    if (scopeKey !== readinessScopeKey()) {
        if (toast) showToast(`Freeze refused: ${error}`, "danger");
        return;
    }
    state.readinessRefusal = { error, blocking: (refusal && refusal.blocking_issues) || refusal?.payload?.blocking_issues || [] };
    renderReadinessPanel();
    revealReadinessPanel();
    if (toast) showToast(`Freeze refused: ${error}`, "danger");
}

export async function fetchDatasetSnapshots() {
    if (state.isFetchingSnapshots) return;
    state.isFetchingSnapshots = true;
    try {
        const snapshots = await requestDatasetSnapshots();
        state.snapshots = snapshots;
        renderDatasetSnapshots(snapshots);
        populateTrainingSnapshotSelect(snapshots);
        checkActiveFreezeStatus();
    } catch (_) {
        // The request module reported it; the shelf keeps the snapshots it already has.
    } finally {
        state.isFetchingSnapshots = false;
    }
}

function renderDatasetSnapshots(snapshots) {
    if (!dom.snapshotsList || !dom.snapshotsCountBadge) return;
    const total = (snapshots && snapshots.length) || 0;
    dom.snapshotsCountBadge.textContent = `${total} Snapshot${total === 1 ? "" : "s"}`;

    if (!snapshots || snapshots.length === 0) {
        dom.snapshotsList.innerHTML = "";
        dom.snapshotsEmpty?.classList.remove("hidden");
        return;
    }

    dom.snapshotsEmpty?.classList.add("hidden");
    dom.snapshotsList.innerHTML = snapshots.map(s => {
        const id = escapeHtml(s.id);
        const themeLabel = s.theme ? escapeHtml(s.theme) : "All Themes";
        const shaHead = s.sha256 ? escapeHtml(s.sha256.substring(0, 12)) : "";
        const shaFull = escapeHtml(s.sha256 || "");
        const counts = `${s.samples_count} sample${s.samples_count === 1 ? "" : "s"} (${s.maps_count} map${s.maps_count === 1 ? "" : "s"})`;
        const sizeStr = formatBytes(s.size_bytes);
        const dateStr = s.created_at ? new Date(s.created_at).toLocaleString() : "";
        const downloadUrl = `/api/training/datasets/${encodeURIComponent(s.id)}/archive`;

        return `
            <div class="snapshot-card" data-id="${id}">
                <div class="snapshot-card-head">
                    <span class="snapshot-id mono" title="${id}">${id}</span>
                    <span class="snapshot-theme-badge">${themeLabel}</span>
                </div>
                <div class="snapshot-meta-row">
                    <span class="snapshot-sha256 mono" title="SHA-256: ${shaFull}">SHA: ${shaHead}…</span>
                    <span class="snapshot-counts mono">${counts}</span>
                    <span class="snapshot-size mono">${sizeStr}</span>
                </div>
                <div class="snapshot-meta-row">
                    <span class="snapshot-time mono">${escapeHtml(dateStr)}</span>
                </div>
                <div class="snapshot-card-actions">
                    <button type="button" class="btn btn-primary btn-xs btn-train-snapshot" data-id="${id}" title="Launch Training Run on this snapshot">
                        Train LoRA
                    </button>
                    <a href="${downloadUrl}" download="${id}.zip" class="btn btn-secondary btn-xs" title="Download ZIP Archive">
                        Download ZIP
                    </a>
                    <button type="button" class="btn btn-danger btn-xs btn-delete-snapshot" data-id="${id}" title="Delete Snapshot">
                        Delete
                    </button>
                </div>
            </div>
        `;
    }).join("");

    dom.snapshotsList.querySelectorAll(".btn-delete-snapshot").forEach(btn => {
        btn.addEventListener("click", () => {
            const id = btn.getAttribute("data-id");
            if (id) deleteDatasetSnapshot(id);
        });
    });
    dom.snapshotsList.querySelectorAll(".btn-train-snapshot").forEach(btn => {
        btn.addEventListener("click", () => {
            const id = btn.getAttribute("data-id");
            if (id) openTrainingDrawer({ snapshotId: id });
        });
    });
    populateTrainingSnapshotSelect(snapshots);
}

export function toggleSnapshotsShelf() {
    state.snapshotsCollapsed = !state.snapshotsCollapsed;
    if (dom.snapshotsShelf) {
        dom.snapshotsShelf.classList.toggle("collapsed", state.snapshotsCollapsed);
    }
    if (dom.btnToggleSnapshots) {
        dom.btnToggleSnapshots.setAttribute("aria-expanded", String(!state.snapshotsCollapsed));
    }
    if (dom.toggleSnapshotsText) {
        dom.toggleSnapshotsText.textContent = state.snapshotsCollapsed ? "Expand" : "Collapse";
    }
}

export function showFreezeProgressPanel(show) {
    if (!dom.freezeProgressPanel) return;
    if (show) {
        dom.freezeProgressPanel.classList.remove("hidden");
    } else {
        dom.freezeProgressPanel.classList.add("hidden");
    }
}

export function updateFreezeProgressUI(progress) {
    if (!progress) return;
    showFreezeProgressPanel(true);

    const phase = (progress.phase || "starting").toLowerCase();
    if (dom.freezePhaseBadge) {
        dom.freezePhaseBadge.textContent = phase.toUpperCase();
        dom.freezePhaseBadge.className = `freeze-phase-badge phase-${phase}`;
    }

    if (dom.freezeProgressMessage) {
        dom.freezeProgressMessage.textContent = progress.message || "Processing dataset snapshot...";
    }

    let pct = 0;
    let countsText = "";
    if (phase === "starting") {
        pct = 5;
        countsText = "Initializing...";
    } else if (phase === "auditing") {
        const completed = progress.completed_maps || 0;
        const total = progress.total_maps || 1;
        pct = Math.min(25, Math.round((completed / Math.max(total, 1)) * 25));
        countsText = `${completed} / ${total} maps`;
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = `Auditing (${completed}/${total})`;
    } else if (phase === "packaging") {
        const completed = progress.completed_samples || 0;
        const total = progress.total_samples || 1;
        pct = 25 + Math.min(70, Math.round((completed / Math.max(total, 1)) * 70));
        countsText = `${completed} / ${total} samples`;
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = `Freezing (${completed}/${total})`;
    } else if (phase === "finalizing") {
        pct = 95;
        countsText = "Saving artifact...";
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Finalizing...";
    } else if (phase === "complete") {
        pct = 100;
        countsText = "Complete";
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Complete!";
    } else if (phase === "error") {
        countsText = "Error";
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Freeze Snapshot";
    }

    if (dom.freezeProgressBar) {
        dom.freezeProgressBar.style.width = `${pct}%`;
    }
    if (dom.freezeProgressCounts) {
        dom.freezeProgressCounts.textContent = countsText;
    }
    if (dom.freezeProgressPercent) {
        dom.freezeProgressPercent.textContent = `(${pct}%)`;
    }

    if (dom.freezeDetailMap) {
        dom.freezeDetailMap.textContent = progress.current_map ? `Map: ${progress.current_map}` : "";
    }
    if (dom.freezeDetailSection) {
        dom.freezeDetailSection.textContent = progress.current_section
            ? `Section: (${progress.current_section[0]}, ${progress.current_section[1]})`
            : "";
    }
}

let freezePollingTimer = null;
function startFreezeStatusPolling() {
    if (freezePollingTimer) return;
    freezePollingTimer = setInterval(async () => {
        let data;
        try {
            data = await fetchFreezeStatus();
        } catch (_) {
            // The request module names the failed operation on the pill, and toasts the
            // transition once: a freeze-status check that fails every 1.5s is one failure,
            // not one per attempt.
            return;
        }
        if (data.in_progress && data.progress) {
            state.isFreezingSnapshot = true;
            if (dom.btnFreezeSnapshot) dom.btnFreezeSnapshot.disabled = true;
            updateFreezeProgressUI(data.progress);
        } else {
            stopFreezeStatusPolling();
            state.isFreezingSnapshot = false;
            if (dom.btnFreezeSnapshot) dom.btnFreezeSnapshot.disabled = false;
            if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Freeze Snapshot";
            showFreezeProgressPanel(false);
            fetchDatasetSnapshots();
        }
    }, 1500);
}

function stopFreezeStatusPolling() {
    if (freezePollingTimer) {
        clearInterval(freezePollingTimer);
        freezePollingTimer = null;
    }
}

async function checkActiveFreezeStatus() {
    let data;
    try {
        data = await fetchFreezeStatus();
    } catch (_) {
        // Reported by the request module; the shelf keeps its current freeze state.
        return;
    }
    if (data.in_progress && data.progress) {
        state.isFreezingSnapshot = true;
        if (dom.btnFreezeSnapshot) dom.btnFreezeSnapshot.disabled = true;
        updateFreezeProgressUI(data.progress);
        startFreezeStatusPolling();
    }
}

export async function performFreezeSnapshotWithProgress(scopeTheme, onProgress) {
    let finalSnapshot = null;
    let refusal = null;

    const summary = await requestFreezeSnapshot(scopeTheme, {
        onEvent(event, data) {
            if (event === "progress") {
                onProgress?.("progress", data);
            } else if (event === "complete") {
                finalSnapshot = data.snapshot || data;
                onProgress?.("complete", data);
            } else if (event === "error") {
                // The stream answered 200 and reported the failure inside it, so the failure
                // policy never saw a transport error: the refusal is the view's to render.
                refusal = data;
                onProgress?.("error", data);
            }
        },
    });

    if (refusal) throw refusal;
    if (summary) {
        // A non-stream answer is the finished snapshot, which is also the one-shot summary.
        onProgress?.("complete", { snapshot: summary });
        return summary;
    }
    if (!finalSnapshot) {
        throw new Error("Freezing completed without receiving snapshot metadata");
    }
    return finalSnapshot;
}

export async function freezeDatasetSnapshot() {
    if (state.isFreezingSnapshot) return;
    state.isFreezingSnapshot = true;
    if (dom.btnFreezeSnapshot) dom.btnFreezeSnapshot.disabled = true;
    if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Freezing...";

    const scopeTheme = datasetScopeTheme();
    const scopeLabel = scopeTheme ? `theme '${scopeTheme}'` : "all themes";
    showToast(`Auditing readiness & freezing snapshot (${scopeLabel})...`, "info");
    showFreezeProgressPanel(true);
    updateFreezeProgressUI({
        phase: "starting",
        completed_maps: 0,
        total_maps: 0,
        completed_samples: 0,
        total_samples: 0,
        message: `Auditing readiness & freezing snapshot (${scopeLabel})...`,
    });

    try {
        const snapshot = await performFreezeSnapshotWithProgress(scopeTheme, (event, data) => {
            if (event === "progress") {
                updateFreezeProgressUI(data);
            } else if (event === "complete") {
                updateFreezeProgressUI({
                    phase: "complete",
                    completed_maps: data.completed_maps,
                    total_maps: data.total_maps,
                    completed_samples: data.completed_samples,
                    total_samples: data.total_samples,
                    message: `Snapshot created: ${data.snapshot?.id || ""}`,
                    snapshot: data.snapshot,
                });
            }
        });

        showToast(`Frozen Dataset Snapshot '${snapshot.id}' created (${snapshot.samples_count} samples, ${snapshot.maps_count} maps)!`, "success");
        setTimeout(() => {
            showFreezeProgressPanel(false);
        }, 2500);
        fetchDatasetSnapshots();
    } catch (errData) {
        showFreezeProgressPanel(false);
        // A `RequestError` was already raised by the failure policy: only a refusal the server
        // reported inside a 200 stream still needs its own toast here.
        const reported = errData?.name === "RequestError";
        if (errData?.status === 400 || errData?.status === 422 || errData?.code === "DATASET_NOT_READY") {
            showFreezeRefusal(errData, scopeTheme, { toast: !reported });
            return;
        }
        if (errData?.status === 409) {
            // The policy names the refusal; polling picks the progress back up.
            startFreezeStatusPolling();
            return;
        }
        if (reported) return;
        showToast(`Freeze error: ${errData?.message || errData?.error || "Unknown error"}`, "danger");
    } finally {
        state.isFreezingSnapshot = false;
        if (dom.btnFreezeSnapshot) dom.btnFreezeSnapshot.disabled = false;
        if (dom.btnFreezeLabel) dom.btnFreezeLabel.textContent = "Freeze Snapshot";
    }
}

async function deleteDatasetSnapshot(id) {
    if (!id) return;
    if (!confirm(`Are you sure you want to permanently delete frozen dataset snapshot '${id}'? This will remove its archive from blob storage.`)) {
        return;
    }
    try {
        await requestDeleteSnapshot(id);
        showToast(`Dataset snapshot '${id}' deleted`, "info");
        fetchDatasetSnapshots();
    } catch (_) {
        // The failure policy names the operation and carries the server's reason, including the
        // 409 a training run still using this snapshot answers with; the shelf keeps the card.
    }
}
