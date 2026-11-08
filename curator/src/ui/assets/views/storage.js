/**
 * Storage footprint monitor and the staging purge confirmation modals.
 *
 * The stats render has three mutually exclusive states instead of an only-on-success update:
 *
 * - `loading` — the read is in flight and the drawer says so instead of showing figures;
 * - `ready` — the measured figures;
 * - `error` — an explicit, retryable failure message.
 *
 * #292 is why: the request swallowed its own failure in `catch (_) {}`, so the drawer kept
 * rendering the `index.html` fallbacks — `0 B`, `0 files`, `0 B (0)` — next to a
 * `Load storage stats failed` toast. Reporting an unreadable footprint as an empty one is worse
 * than reporting nothing at all.
 *
 * The two state elements are owned by this view: `#storage-stats-loading` and
 * `#storage-stats-error` (with its `#btn-retry-storage-stats` action) are looked up in the markup
 * and, when the modal does not carry them, created on demand — so the state contract lives next to
 * the code that paints it rather than in the page.
 */

import { state } from "../store.js";
import { formatBytes } from "../kit/format.js";
import { fetchStatus, showToast } from "../shell/status.js";
import { stopStageMotion } from "../stage/canvas.js";
import { fetchStagingQueue } from "./triage.js";
import { fetchStorageStats as requestStorageStats, purgeStaging as requestPurgeStaging } from "../requests/storage.js";

/** Text the loading indicator carries while a footprint read is in flight. */
const LOADING_TEXT = "Reading storage footprint…";

const LOADING_ID = "storage-stats-loading";
const ERROR_ID = "storage-stats-error";
const RETRY_ID = "btn-retry-storage-stats";

/**
 * Every figure the drawer and its widget display, so an in-flight or failed read can clear them
 * all rather than leave a number on screen claiming a measurement that was never made.
 */
const FIGURE_IDS = [
    "storage-staging-bytes",
    "storage-staging-files",
    "storage-breakdown-reddit",
    "storage-breakdown-generated",
    "storage-breakdown-root",
    "storage-curated-bytes",
    "storage-curated-files",
    "storage-rejected-bytes",
    "storage-rejected-records",
    "storage-telemetry-bytes",
    "storage-telemetry-files",
    "storage-total-bytes",
    "storage-total-files",
    "drawer-storage-staging",
    "drawer-storage-curated",
];

/**
 * The element for `id`, created inside the storage modal when the markup does not carry it.
 *
 * The modal body is the host, so a created indicator sits directly above the figures it replaces.
 */
function stateElement(id, { tag = "div", className = "" } = {}) {
    const existing = document.getElementById(id);
    if (existing) return existing;

    const created = document.createElement(tag);
    created.id = id;
    created.className = className;
    const host = document.getElementById("storage-modal") || document.body;
    if (!host) return null;
    if (typeof host.prepend === "function") host.prepend(created);
    else host.appendChild(created);
    return created;
}

/** Show or clear the loading indicator; it carries its own label and no children. */
function renderLoading(visible) {
    const el = stateElement(LOADING_ID, { className: "storage-state storage-state-loading hidden" });
    if (!el) return;
    el.textContent = visible ? LOADING_TEXT : "";
    el.classList.toggle("hidden", !visible);
}

/**
 * The retry action the error state offers, bound to a fresh read exactly once.
 *
 * The listener is attached here rather than only in `setupStorageControls`, because a markup that
 * does not carry the control gets it created at error time — and a control that is created must
 * still work.
 */
function retryControl() {
    const retry = stateElement(RETRY_ID, { tag: "button", className: "btn btn-secondary btn-sm" });
    if (!retry) return null;
    if (!retry.__storageRetryBound) {
        retry.type = "button";
        retry.textContent = "Retry";
        retry.addEventListener("click", () => fetchStorageStats());
        retry.__storageRetryBound = true;
    }
    return retry;
}

/**
 * Show or clear the error state: the failure message plus the retry action.
 *
 * `message == null` clears the state entirely, which is what a new read does before it starts.
 */
function renderError(message) {
    const el = stateElement(ERROR_ID, { className: "storage-state storage-state-error hidden" });
    if (!el) return;
    if (message == null) {
        el.textContent = "";
        el.classList.add("hidden");
        return;
    }
    el.textContent = `Load storage stats failed: ${message}`;
    const retry = retryControl();
    if (retry && retry.parentElement !== el) el.appendChild(retry);
    el.classList.remove("hidden");
}

/** Blank every figure and the theme list, so nothing on screen claims an unread footprint. */
function clearStorageFigures() {
    for (const id of FIGURE_IDS) {
        const el = document.getElementById(id);
        if (el) el.textContent = "";
    }
    const themesListEl = document.getElementById("storage-themes-list");
    if (themesListEl) themesListEl.innerHTML = "";
    const reclaimFilesEl = document.getElementById("purge-reclaim-files");
    if (reclaimFilesEl) reclaimFilesEl.textContent = "Reclaim size unavailable until the footprint loads";
}

/**
 * Read the footprint and render it.
 *
 * The request joins any scan already in flight (`requests/storage.js`), so the boot prefetch, a
 * drawer open, a refresh and a retry never overlap.
 */
export async function fetchStorageStats() {
    state.storageStatsPhase = "loading";
    state.storageStatsError = null;
    renderError(null);
    renderLoading(true);
    clearStorageFigures();

    try {
        const data = await requestStorageStats();
        state.storageStats = data;
        state.storageStatsPhase = "ready";
        renderLoading(false);
        renderStorageStats(data);
        return data;
    } catch (error) {
        if (error && error.name === "AbortError") {
            // Cancelled, not failed: restore the state the last settled read left behind.
            state.storageStatsPhase = state.storageStats ? "ready" : "idle";
            renderLoading(false);
            if (state.storageStats) renderStorageStats(state.storageStats);
            return null;
        }
        state.storageStatsPhase = "error";
        state.storageStatsError = error && error.message ? error.message : String(error);
        renderLoading(false);
        renderError(state.storageStatsError);
        // The request module already raised the status pill and toasted the failure.
        return null;
    }
}

function renderStorageStats(stats) {
    if (!stats) return;

    // 1. Staging Card
    const stagingBytesEl = document.getElementById("storage-staging-bytes");
    const stagingFilesEl = document.getElementById("storage-staging-files");
    if (stagingBytesEl) stagingBytesEl.textContent = formatBytes(stats.staging.bytes);
    if (stagingFilesEl) stagingFilesEl.textContent = `${stats.staging.file_count.toLocaleString()} files`;

    const redditBreakdownEl = document.getElementById("storage-breakdown-reddit");
    const genBreakdownEl = document.getElementById("storage-breakdown-generated");
    const rootBreakdownEl = document.getElementById("storage-breakdown-root");

    const redditStats = stats.staging_breakdown?.reddit || { bytes: 0, file_count: 0 };
    const genStats = stats.staging_breakdown?.generated || { bytes: 0, file_count: 0 };
    const rootStats = stats.staging_breakdown?.root || { bytes: 0, file_count: 0 };

    if (redditBreakdownEl) redditBreakdownEl.textContent = `${formatBytes(redditStats.bytes)} (${redditStats.file_count})`;
    if (genBreakdownEl) genBreakdownEl.textContent = `${formatBytes(genStats.bytes)} (${genStats.file_count})`;
    if (rootBreakdownEl) rootBreakdownEl.textContent = `${formatBytes(rootStats.bytes)} (${rootStats.file_count})`;

    // 2. Curated Card
    const curatedBytesEl = document.getElementById("storage-curated-bytes");
    const curatedFilesEl = document.getElementById("storage-curated-files");
    if (curatedBytesEl) curatedBytesEl.textContent = formatBytes(stats.curated.bytes);
    if (curatedFilesEl) curatedFilesEl.textContent = `${stats.curated.file_count.toLocaleString()} files`;

    const themesListEl = document.getElementById("storage-themes-list");
    if (themesListEl && stats.curated_by_theme) {
        themesListEl.innerHTML = "";
        const sortedThemes = Object.entries(stats.curated_by_theme).sort((a, b) => b[1].bytes - a[1].bytes);
        for (const [theme, themeStats] of sortedThemes) {
            const item = document.createElement("div");
            item.className = "breakdown-item";
            const capitalized = theme.charAt(0).toUpperCase() + theme.slice(1);
            item.innerHTML = `<span>${capitalized}:</span> <span class="mono">${formatBytes(themeStats.bytes)} (${themeStats.map_count} maps)</span>`;
            themesListEl.appendChild(item);
        }
    }

    // 3. Rejected Card
    const rejectedBytesEl = document.getElementById("storage-rejected-bytes");
    const rejectedRecordsEl = document.getElementById("storage-rejected-records");
    if (rejectedBytesEl) rejectedBytesEl.textContent = formatBytes(stats.rejected.bytes);
    if (rejectedRecordsEl) rejectedRecordsEl.textContent = `${stats.rejected.file_count.toLocaleString()} records`;

    // 4. Telemetry Card
    const telemetryBytesEl = document.getElementById("storage-telemetry-bytes");
    const telemetryFilesEl = document.getElementById("storage-telemetry-files");
    if (telemetryBytesEl) telemetryBytesEl.textContent = formatBytes(stats.telemetry.bytes);
    if (telemetryFilesEl) telemetryFilesEl.textContent = `${stats.telemetry.file_count.toLocaleString()} files`;

    // 5. Total Footprint
    const totalBytesEl = document.getElementById("storage-total-bytes");
    const totalFilesEl = document.getElementById("storage-total-files");
    if (totalBytesEl) totalBytesEl.textContent = formatBytes(stats.total_bytes);
    if (totalFilesEl) totalFilesEl.textContent = `(${stats.total_files.toLocaleString()} items)`;

    // 6. Drawer Widget Update
    const drawerStagingEl = document.getElementById("drawer-storage-staging");
    const drawerCuratedEl = document.getElementById("drawer-storage-curated");
    if (drawerStagingEl) drawerStagingEl.textContent = `${formatBytes(stats.staging.bytes)} (${stats.staging.file_count} files)`;
    if (drawerCuratedEl) drawerCuratedEl.textContent = `${formatBytes(stats.curated.bytes)} (${stats.curated.file_count} files)`;
}

function openStorageModal() {
    stopStageMotion();
    state.storageModalOpen = true;
    const modal = document.getElementById("storage-modal");
    modal?.classList.remove("hidden");
    fetchStorageStats();
}

export function closeStorageModal() {
    stopStageMotion();
    state.storageModalOpen = false;
    const modal = document.getElementById("storage-modal");
    modal?.classList.add("hidden");
}

function openPurgeModal() {
    stopStageMotion();
    state.purgeModalOpen = true;
    const modal = document.getElementById("purge-confirm-modal");
    modal?.classList.remove("hidden");
    const reclaimBytesEl = document.getElementById("purge-reclaim-bytes");
    const reclaimFilesEl = document.getElementById("purge-reclaim-files");
    const currentStaging = state.storageStats?.staging || null;
    if (!currentStaging) {
        // Nothing was read, so there is no reclaim figure to quote: `0 B` would be a guess about
        // bytes that are about to be deleted for good.
        if (reclaimBytesEl) reclaimBytesEl.textContent = "—";
        if (reclaimFilesEl) reclaimFilesEl.textContent = "Reclaim size unavailable until the footprint loads";
        return;
    }
    if (reclaimBytesEl) reclaimBytesEl.textContent = formatBytes(currentStaging.bytes);
    if (reclaimFilesEl) reclaimFilesEl.textContent = `${currentStaging.file_count.toLocaleString()} candidate files will be permanently deleted`;
}

export function closePurgeModal() {
    stopStageMotion();
    state.purgeModalOpen = false;
    const modal = document.getElementById("purge-confirm-modal");
    modal?.classList.add("hidden");
}

async function purgeStaging() {
    if (state.isPurging) return;
    state.isPurging = true;
    const confirmBtn = document.getElementById("btn-confirm-purge");
    const confirmText = document.getElementById("btn-confirm-purge-text");
    if (confirmBtn) confirmBtn.disabled = true;
    if (confirmText) confirmText.textContent = "Purging...";

    try {
        const data = await requestPurgeStaging();
        closePurgeModal();
        showToast(`Staging purged: ${data.purged_files} files deleted, ${formatBytes(data.reclaimed_bytes)} reclaimed`, "success");

        // Update UI candidates, status indicator, and storage stats
        await fetchStagingQueue();
        fetchStatus();
        await fetchStorageStats();
    } catch (_) {
        // The request module reported it; the confirmation modal stays open for a retry.
    } finally {
        state.isPurging = false;
        if (confirmBtn) confirmBtn.disabled = false;
        if (confirmText) confirmText.textContent = "Confirm & Purge Staging";
    }
}

export function setupStorageControls() {
    document.getElementById("btn-open-storage-modal")?.addEventListener("click", () => openStorageModal());
    document.getElementById("btn-close-storage-modal")?.addEventListener("click", () => closeStorageModal());
    document.getElementById("btn-dismiss-storage-modal")?.addEventListener("click", () => closeStorageModal());
    document.getElementById("btn-refresh-storage")?.addEventListener("click", () => fetchStorageStats());
    document.getElementById("btn-drawer-open-storage")?.addEventListener("click", () => openStorageModal());
    // The retry action lives in the error state, so it is bound wherever that state is painted.
    retryControl();

    document.getElementById("btn-open-purge-confirm")?.addEventListener("click", () => openPurgeModal());
    document.getElementById("btn-close-purge-modal")?.addEventListener("click", () => closePurgeModal());
    document.getElementById("btn-cancel-purge")?.addEventListener("click", () => closePurgeModal());
    document.getElementById("btn-confirm-purge")?.addEventListener("click", () => purgeStaging());

    const storageModal = document.getElementById("storage-modal");
    storageModal?.addEventListener("click", (e) => {
        if (e.target === storageModal) closeStorageModal();
    });

    const purgeModal = document.getElementById("purge-confirm-modal");
    purgeModal?.addEventListener("click", (e) => {
        if (e.target === purgeModal) closePurgeModal();
    });
}
