/**
 * Frozen Dataset Snapshots: the snapshot shelf, streaming freeze progress, freeze status
 * polling and snapshot deletion (Issue #216).
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { h } from "../kit/html.js";
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
    // A refusal arrives either as an in-stream `error` frame (the fields sit on the frame) or
    // as a non-OK response, where `RequestError.payload` carries the same body.
    const readiness = refusal?.readiness ?? refusal?.payload?.readiness;
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

/**
 * The shelf render target's own nodes. Created once per root, then reused: the list and
 * the empty notice never move, and a card is replaced only when its snapshot id leaves the
 * list. Everything below the root belongs to this module.
 */
const shelf = {
    root: null,
    list: null,
    empty: null,
    cards: new Map(),
};

function attachShelf(root) {
    if (shelf.root !== root) {
        shelf.root = root;
        shelf.list = null;
        shelf.empty = null;
        shelf.cards = new Map();
        while (root.firstChild) root.removeChild(root.firstChild);
    }
    if (shelf.list) return;

    shelf.list = h("div", { class: "snapshots-list" });
    shelf.empty = h(
        "div",
        { class: "snapshots-empty hidden" },
        h(
            "p",
            null,
            "No frozen snapshots for this theme scope yet. Click ",
            h("strong", null, "Freeze Snapshot"),
            " to create one.",
        ),
    );
    root.appendChild(shelf.list);
    root.appendChild(shelf.empty);
}

function createSnapshotCard(snapshot) {
    const id = snapshot.id;
    const nodes = {
        id: h("span", { class: "snapshot-id mono", title: id }, id),
        theme: h("span", { class: "snapshot-theme-badge" }),
        sha: h("span", { class: "snapshot-sha256 mono" }),
        counts: h("span", { class: "snapshot-counts mono" }),
        size: h("span", { class: "snapshot-size mono" }),
        time: h("span", { class: "snapshot-time mono" }),
        download: h(
            "a",
            {
                class: "btn btn-secondary btn-xs",
                href: `/api/training/datasets/${encodeURIComponent(id)}/archive`,
                download: `${id}.zip`,
                title: "Download ZIP Archive",
            },
            "Download ZIP",
        ),
    };

    nodes.train = h(
        "button",
        {
            type: "button",
            class: "btn btn-primary btn-xs btn-train-snapshot",
            dataset: { id },
            title: "Launch Training Run on this snapshot",
            onclick: () => openTrainingDrawer({ snapshotId: id }),
        },
        "Train LoRA",
    );
    nodes.delete = h(
        "button",
        {
            type: "button",
            class: "btn btn-danger btn-xs btn-delete-snapshot",
            dataset: { id },
            title: "Delete Snapshot",
            onclick: () => deleteDatasetSnapshot(id),
        },
        "Delete",
    );

    const card = h(
        "div",
        { class: "snapshot-card", dataset: { id } },
        h("div", { class: "snapshot-card-head" }, nodes.id, nodes.theme),
        h("div", { class: "snapshot-meta-row" }, nodes.sha, nodes.counts, nodes.size),
        h("div", { class: "snapshot-meta-row" }, nodes.time),
        h("div", { class: "snapshot-card-actions" }, nodes.train, nodes.download, nodes.delete),
    );

    return { id, card, nodes };
}

function setText(node, text) {
    if (node.textContent !== text) node.textContent = text;
}

function setAttribute(node, name, value) {
    if (node.getAttribute(name) !== value) node.setAttribute(name, value);
}

/** Write the card's current data and nothing else: unchanged nodes are left untouched. */
function updateSnapshotCard(record, snapshot) {
    const id = snapshot.id;
    const themeLabel = snapshot.theme ? snapshot.theme : "All Themes";
    const shaHead = snapshot.sha256 ? snapshot.sha256.substring(0, 12) : "";
    const shaFull = snapshot.sha256 || "";
    const counts = `${snapshot.samples_count} sample${snapshot.samples_count === 1 ? "" : "s"} (${snapshot.maps_count} map${snapshot.maps_count === 1 ? "" : "s"})`;
    const sizeStr = formatBytes(snapshot.size_bytes);
    const dateStr = snapshot.created_at ? new Date(snapshot.created_at).toLocaleString() : "";

    setText(record.nodes.id, id);
    setAttribute(record.nodes.id, "title", id);
    setText(record.nodes.theme, themeLabel);
    setText(record.nodes.sha, `SHA: ${shaHead}…`);
    setAttribute(record.nodes.sha, "title", `SHA-256: ${shaFull}`);
    setText(record.nodes.counts, counts);
    setText(record.nodes.size, sizeStr);
    setText(record.nodes.time, dateStr);
    setAttribute(record.nodes.download, "href", `/api/training/datasets/${encodeURIComponent(id)}/archive`);
    setAttribute(record.nodes.download, "download", `${id}.zip`);
}

/**
 * Render the snapshot shelf from its state.
 *
 * Keyed by snapshot id and in place: a re-render keeps every surviving card's nodes, so a
 * scroll position, a focused button or an in-flight interaction inside the shelf survives
 * the freeze-progress refresh that triggered it.
 */
export function renderDatasetSnapshots(snapshots) {
    const list = Array.isArray(snapshots) ? snapshots : [];
    const total = list.length;
    if (dom.snapshotsCountBadge) {
        dom.snapshotsCountBadge.textContent = `${total} Snapshot${total === 1 ? "" : "s"}`;
    }

    const root = dom.snapshotsShelf;
    if (!root) return;
    attachShelf(root);

    const wanted = new Set(list.map((snapshot) => snapshot.id));
    for (const [id, record] of shelf.cards) {
        if (wanted.has(id)) continue;
        record.card.remove();
        shelf.cards.delete(id);
    }

    let index = 0;
    for (const snapshot of list) {
        let record = shelf.cards.get(snapshot.id);
        if (!record) {
            record = createSnapshotCard(snapshot);
            shelf.cards.set(snapshot.id, record);
        }
        updateSnapshotCard(record, snapshot);

        const current = shelf.list.childNodes[index] || null;
        if (current !== record.card) shelf.list.insertBefore(record.card, current);
        index += 1;
    }

    shelf.empty.classList.toggle("hidden", total > 0);
    populateTrainingSnapshotSelect(list);
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
