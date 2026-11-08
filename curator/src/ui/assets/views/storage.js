/**
 * Storage footprint monitor and the staging purge confirmation modals.
 *
 * The stats render has three explicit phases instead of an only-on-success update:
 *
 * - `loading` — the modal shows a loading indicator, because a scan over a deployed bucket is a
 *   real wait and the markup's fallback numbers are not a footprint.
 * - `ready` — the figures, from the last successful read.
 * - `error` — an explicit failure message with a retry action.
 *
 * Issue #292 is why: the request swallowed its own failure, so the modal kept rendering the
 * `index.html` fallbacks — `0 B`, `0 files`, `0 B (0)` — next to a `Load storage stats failed`
 * toast. Misreporting an unreadable footprint as an empty one is worse than reporting nothing.
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
 * Render the modal's loading/error state.
 *
 * The figures keep whatever the last successful read produced; the indicators only add the
 * "this is not the current footprint" or "there is no footprint to show" message, so a stale
 * drawer is labelled stale instead of silently reading as current.
 */
function renderStorageStatsState() {
    const phase = state.storageStatsPhase;
    const loadingEl = document.getElementById("storage-stats-loading");
    const errorEl = document.getElementById("storage-stats-error");
    const errorTextEl = document.getElementById("storage-stats-error-text");
    const statsEl = document.getElementById("storage-stats-content");

    if (loadingEl) loadingEl.classList.toggle("hidden", phase !== "loading");
    if (errorEl) errorEl.classList.toggle("hidden", phase !== "error");
    if (errorTextEl && phase === "error") {
        const detail = state.storageStatsError ? `: ${state.storageStatsError}` : "";
        errorTextEl.textContent = `Storage footprint is unavailable${detail}`;
    }
    // The cards carry real measurements or nothing: a failed first read hides them instead of
    // letting them show the markup's placeholder zeros.
    if (statsEl) {
        const showFigures = phase === "ready";
        statsEl.classList.toggle("hidden", !showFigures);
        statsEl.classList.toggle("is-stale", false);
    }
    const refreshBtn = document.getElementById("btn-refresh-storage");
    if (refreshBtn) refreshBtn.disabled = phase === "loading";
}

/**
 * Read the footprint and render it.
 *
 * `quiet` is for the boot prefetch: the drawer is not open yet, so a failure raises the pill and
 * toasts once through the request module without also painting the modal's error panel. An
 * explicit open or refresh passes `quiet: false` and names the state it shows. Either way the
 * phase is recorded, so the modal never presents placeholder zeros as a measurement.
 */
export async function fetchStorageStats({ quiet = false } = {}) {
    setStorageStatsPhase("loading");
    try {
        const data = await requestStorageStats();
        state.storageStats = data;
        setStorageStatsPhase("ready");
        renderStorageStats(data);
        return data;
    } catch (error) {
        if (error && error.name === "AbortError") {
            // Cancelled, not failed: leave the last phase and figures alone.
            setStorageStatsPhase(state.storageStats ? "ready" : "idle");
            return null;
        }
        setStorageStatsPhase("error", {
            error: error && error.message ? error.message : String(error),
        });
        // The request module already raised the pill and toasted; the modal clears the placeholder
        // figures so nothing on screen claims a footprint that was never read.
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
 * Every figure the modal and the drawer widget display, for one successful read.
 *
 * The cards are rendered here rather than left at their markup defaults, so the numbers on
 * screen always come from a server answer.
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
    // Opening the drawer is an explicit read: it shows the loading indicator and, if it fails,
    // says so. The request joins any in-flight scan instead of starting a second one.
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
