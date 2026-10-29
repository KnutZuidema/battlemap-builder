/**
 * Storage footprint monitor and the staging purge confirmation modals.
 */

import { state } from "../store.js";
import { formatBytes } from "../kit/format.js";
import { fetchStatus, showToast } from "../shell/status.js";
import { stopStageMotion } from "../stage/canvas.js";
import { fetchStagingQueue } from "./triage.js";
import { fetchStorageStats as requestStorageStats, purgeStaging as requestPurgeStaging } from "../requests/storage.js";

export async function fetchStorageStats() {
    try {
        const data = await requestStorageStats();
        state.storageStats = data;
        renderStorageStats(data);
    } catch (_) {
        // The request module reported it; the drawer keeps the last known footprint.
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
    const currentStaging = state.storageStats?.staging || { bytes: 0, file_count: 0 };
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
