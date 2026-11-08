/**
 * Dungeon Forge — Battlemap Curator Frontend Application
 *
 * Bootstrap only: the entry point wires the module graph together and dispatches
 * the first data fetches. Every feature lives in the module that owns it.
 */

import { initializeDom } from "./dom.js";
import { getTriageViewport, setupCanvas } from "./stage/canvas.js";
import { updateGridPanel } from "./shell/grid-panel.js";
import { setupEventListeners } from "./shell/hotkeys.js";
import { fetchStatus, updateHud } from "./shell/status.js";
import { fetchCostStats } from "./views/benchmark.js";
import { syncReadinessToScope } from "./views/readiness.js";
import { fetchStorageStats } from "./views/storage.js";
import { fetchTrainingRuns } from "./views/training.js";
import { fetchStagingQueue } from "./views/triage.js";

function init() {
    initializeDom();
    setupCanvas();
    getTriageViewport().subscribe(updateHud);
    setupEventListeners();
    updateGridPanel();
    fetchStatus();
    fetchCostStats();
    fetchStagingQueue();
    // Prefetch only: the drawer is closed, so a failure must not paint its error panel. The read
    // is deduplicated through the request tracker, so opening the drawer while this is still in
    // flight joins it instead of starting a second scan of the bucket.
    fetchStorageStats({ quiet: true });
    fetchTrainingRuns();
    syncReadinessToScope();
}

// Initialize when DOM is ready
if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
} else {
    init();
}
