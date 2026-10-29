/**
 * Ingestion drawer: Reddit harvest configuration, the live harvest event
 * stream, the staging drawer's open/close state, and its control bindings.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { closeHelpModal } from "../shell/help.js";
import { fetchStatus, showToast } from "../shell/status.js";
import { stopStageMotion } from "../stage/canvas.js";
import { fetchStagingQueue } from "./triage.js";
import { harvestReddit } from "../requests/harvest.js";

export function toggleIngestionDrawer() {
    if (state.drawerOpen) {
        closeIngestionDrawer();
    } else {
        openIngestionDrawer();
    }
}

export function openIngestionDrawer() {
    stopStageMotion();
    if (state.helpModalOpen) closeHelpModal();
    state.drawerOpen = true;
    dom.ingestionDrawer?.classList.remove("hidden");
    dom.btnStartHarvest?.focus();
}

export function closeIngestionDrawer() {
    stopStageMotion();
    state.drawerOpen = false;
    dom.ingestionDrawer?.classList.add("hidden");
    dom.canvas.focus();
}

export async function startRedditHarvest() {
    if (state.isHarvesting) return;
    state.isHarvesting = true;
    dom.btnStartHarvest.disabled = true;
    dom.harvestProgressContainer.classList.remove("hidden");
    dom.harvestProgressBar.style.width = "5%";
    dom.harvestProgressText.textContent = "Connecting to Reddit harvester...";

    const payload = {
        sort: state.harvestSort,
        time: state.harvestTime,
        limit: parseInt(dom.harvestLimitInput.value, 10),
        gridless_only: dom.harvestGridlessOnly.checked,
    };

    try {
        const summary = await harvestReddit(payload, { onEvent: handleHarvestEvent });

        if (summary) {
            const msg = `Harvest complete: ${summary.downloaded} downloaded, ${summary.skipped} skipped.`;
            dom.harvestProgressBar.style.width = "100%";
            dom.harvestProgressText.textContent = msg;
            showToast(msg, "success");
            await fetchStagingQueue();
            fetchStatus();
        }
    } catch (_) {
        // The failure policy names the operation; a harvest that failed mid-stream already
        // reported its own `error` event through the drawer.
        dom.harvestProgressText.textContent = "Harvest failed";
    } finally {
        state.isHarvesting = false;
        dom.btnStartHarvest.disabled = false;
    }
}

function handleHarvestEvent(event, data) {
    if (event === "progress") {
        const pct = data.total > 0 ? Math.round((data.processed / data.total) * 100) : 10;
        dom.harvestProgressBar.style.width = `${Math.max(5, Math.min(100, pct))}%`;
        dom.harvestProgressText.textContent = data.message;
    } else if (event === "complete") {
        dom.harvestProgressBar.style.width = "100%";
        dom.harvestProgressText.textContent = data.message;
        showToast(data.message, "success");
        fetchStagingQueue();
        fetchStatus();
    } else if (event === "error") {
        dom.harvestProgressText.textContent = `Error: ${data.error || "Unknown"}`;
        showToast(`Harvest failed: ${data.error || "Unknown"}`, "danger");
    }
}

export function setupDrawerControls() {
    dom.btnOpenDrawer?.addEventListener("click", () => openIngestionDrawer());
    dom.btnEmptyHarvest?.addEventListener("click", () => openIngestionDrawer());
    dom.btnCloseDrawer?.addEventListener("click", () => closeIngestionDrawer());
    dom.btnCancelDrawer?.addEventListener("click", () => closeIngestionDrawer());

    dom.ingestionDrawer?.addEventListener("click", (e) => {
        if (e.target === dom.ingestionDrawer) {
            closeIngestionDrawer();
        }
    });

    dom.harvestLimitInput?.addEventListener("input", () => {
        dom.harvestLimitValue.textContent = dom.harvestLimitInput.value;
    });

    const sortBtns = dom.harvestSortGroup?.querySelectorAll(".btn-toggle");
    sortBtns?.forEach((btn) => {
        btn.addEventListener("click", () => {
            const sort = btn.getAttribute("data-sort");
            if (sort) {
                state.harvestSort = sort;
                sortBtns.forEach((b) => b.classList.toggle("active", b === btn));
            }
        });
    });

    const timeBtns = dom.harvestTimeGroup?.querySelectorAll(".btn-toggle");
    timeBtns?.forEach((btn) => {
        btn.addEventListener("click", () => {
            const time = btn.getAttribute("data-time");
            if (time) {
                state.harvestTime = time;
                timeBtns.forEach((b) => b.classList.toggle("active", b === btn));
            }
        });
    });

    dom.btnStartHarvest?.addEventListener("click", () => startRedditHarvest());
}
