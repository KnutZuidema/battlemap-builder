/**
 * Storage footprint monitor and the staging purge confirmation modals.
 *
 * The stats render has three explicit phases instead of an only-on-success update:
 *
 * - `loading` — the drawer shows a loading indicator, because a scan over a deployed bucket is a
 *   real wait and the markup's fallback numbers are not a footprint.
 * - `ready` — the figures, from the last successful read.
 * - `error` — an explicit failure message with a retry action.
 *
 * Issue #292 is why: the request swallowed its own failure, so the drawer kept rendering the
 * `index.html` fallbacks — `0 B`, `0 files`, `0 B (0)` — next to a `Load storage stats failed`
 * toast. Misreporting an unreadable footprint as an empty one is worse than reporting nothing.
 *
 * The phase is recorded whether or not the drawer is open, but the drawer's own indicators are
 * painted only while it is: the boot prefetch runs with it closed, and a failure there belongs on
 * the status pill the request module already paints.
 */

import { state } from "../store.js";
import { formatBytes } from "../kit/format.js";
import { fetchStatus, showToast } from "../shell/status.js";
import { stopStageMotion } from "../stage/canvas.js";
import { fetchStagingQueue } from "./triage.js";
import { fetchStorageStats as requestStorageStats, purgeStaging as requestPurgeStaging } from "../requests/storage.js";

/** Phase of the last stats read: `idle` (never asked), `loading`, `ready` or `error`. */
function setStorageStatsPhase(phase, { error = null } = {}) {
    state.storageStatsPhase = phase;
    state.storageStatsError = error;
    renderStorageStatsState();
}

/**
 * Paint the drawer's loading/error indicators from the recorded phase.
 *
 * The cards are shown only for `ready`, so a failed or in-flight read never leaves the markup's
 * placeholder figures on screen claiming a measurement. The indicators are gated on the drawer
 * being open, so a boot failure does not repaint a drawer nobody opened — opening it re-renders
 * and then reports the recorded failure.
 */
function renderStorageStatsState() {
    const phase = state.storageStatsPhase;
    const drawerOpen = Boolean(state.storageModalOpen);
    const loadingEl = document.getElementById("storage-stats-loading");
    const errorEl = document.getElementById("storage-stats-error");
    const errorTextEl = document.getElementById("storage-stats-error-text");
    const statsEl = document.getElementById("storage-stats-content");

    if (loadingEl) loadingEl.classList.toggle("hidden", !(drawerOpen && phase === "loading"));
    if (errorEl) errorEl.classList.toggle("hidden", !(drawerOpen && phase === "error"));
    if (errorTextEl && phase === "error") {
        const detail = state.storageStatsError ? `: ${state.storageStatsError}` : "";
        errorTextEl.textContent = `Storage footprint is unavailable${detail}`;
    }
    if (statsEl) statsEl.classList.toggle("hidden", phase !== "ready");
    const refreshBtn = document.getElementById("btn-refresh-storage");
    if (refreshBtn) refreshBtn.disabled = phase === "loading";
}

/**
 * Read the footprint and render it.
 *
 * The request joins any scan already in flight (`requests/storage.js`), so the boot prefetch, a
 * drawer open and a refresh never overlap. The phase is always recorded; the drawer's indicators
 * are painted only while it is open (see [`renderStorageStatsState`]).
 */
export async function fetchStorageStats() {
    setStorageStatsPhase("loading");
    try {
        const data = await requestStorageStats();
        state.storageStats = data;
        setStorageStatsPhase("ready");
        renderStorageStats(data);
        return data;
    } catch (error) {
        if (error && error.name === "AbortError") {
            // Cancelled, not failed: restore the phase the last settled read left behind.
            setStorageStatsPhase(state.storageStats ? "ready" : "idle");
            return null;
        }
        setStorageStatsPhase("error", {
            error: error && error.message ? error.message : String(error),
        });
        // The request module already raised the pill and toasted; clearing the markup's placeholder
        // figures keeps anything on screen from claiming a footprint that was never read.
        renderStorageStatsFallback();
        return null;
    }
}

/**
 * Clear the placeholder figures the markup ships with, so a failed read never renders as a
 * measured `0 B` footprint.
 */
function renderStorageStatsFallback() {
    for (const id of [
        "storage-staging-bytes",
        "storage-curated-bytes",
        "storage-rejected-bytes",
        "storage-telemetry-bytes",
        "storage-total-bytes",
    ]) {
        const el = document.getElementById(id);
        if (el) el.textContent = "—";
    }
    for (const id of ["storage-staging-files", "storage-curated-files", "storage-telemetry-files"]) {
        const el = document.getElementById(id);
        if (el) el.textContent = "—";
    }
    const recordsEl = document.getElementById("storage-rejected-records");
    if (recordsEl) recordsEl.textContent = "—";
    const totalFilesEl = document.getElementById("storage-total-files");
    if (totalFilesEl) totalFilesEl.textContent = "(unavailable)";
    for (const id of ["storage-breakdown-reddit", "storage-breakdown-generated", "storage-breakdown-root"]) {
        const el = document.getElementById(id);
        if (el) el.textContent = "—";
    }
    const themesListEl = document.getElementById("storage-themes-list");
    if (themesListEl) themesListEl.innerHTML = "";
    for (const id of ["drawer-storage-staging", "drawer-storage-curated"]) {
        const el = document.getElementById(id);
        if (el) el.textContent = "—";
    }
    // A failed read says nothing about the staging footprint, so the purge confirmation must not
    // promise a reclaim figure it does not have.
    const reclaimBytesEl = document.getElementById("purge-reclaim-bytes");
    if (reclaimBytesEl) reclaimBytesEl.textContent = "—";
    const reclaimFilesEl = document.getElementById("purge-reclaim-files");
    if (reclaimFilesEl) reclaimFilesEl.textContent = "Reclaim size unavailable until the footprint loads";
}

/**
 * Every figure the drawer and its widget display, for one successful read.
 *
 * The figures are rendered here rather than left at their markup defaults, so what is on screen
 * always comes from a server answer.
 */
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
    // Paint whatever the last read left behind — the loading indicator, the recorded error, or the
    // figures — before the explicit read below joins or starts a scan.
    renderStorageStatsState();
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
        // Nothing was read, so there is no reclaim figure to quote: a `0 B` promise would be a
        // guess about bytes that are about to be deleted for good.
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
    document.getElementById("btn-retry-storage")?.addEventListener("click", () => fetchStorageStats());
    document.getElementById("btn-drawer-open-storage")?.addEventListener("click", () => openStorageModal());

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
