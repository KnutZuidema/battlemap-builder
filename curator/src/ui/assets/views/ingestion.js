/**
 * Ingestion drawer: Reddit harvest configuration, the live harvest event
 * stream, and the staging drawer's open/close state.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { closeHelpModal } from "../shell/help.js";
import { fetchStatus, showToast } from "../shell/status.js";
import { stopPanLoop, stopZoomLoop } from "../stage/canvas.js";
import { fetchStagingQueue } from "./triage.js";

export function toggleIngestionDrawer() {
    if (state.drawerOpen) {
        closeIngestionDrawer();
    } else {
        openIngestionDrawer();
    }
}

export function openIngestionDrawer() {
    stopPanLoop();
    stopZoomLoop();
    if (state.helpModalOpen) closeHelpModal();
    state.drawerOpen = true;
    dom.ingestionDrawer?.classList.remove("hidden");
    dom.btnStartHarvest?.focus();
}

export function closeIngestionDrawer() {
    stopPanLoop();
    stopZoomLoop();
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
        const res = await fetch("/api/harvest/reddit", {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                "Accept": "text/event-stream",
            },
            body: JSON.stringify(payload),
        });

        if (!res.ok) {
            const errJson = await res.json().catch(() => ({}));
            throw new Error(errJson.error || `HTTP ${res.status}`);
        }

        const contentType = res.headers.get("content-type") || "";

        if (contentType.includes("text/event-stream") && res.body) {
            const reader = res.body.getReader();
            const decoder = new TextDecoder();
            let buffer = "";

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                buffer += decoder.decode(value, { stream: true });

                const lines = buffer.split("\n");
                buffer = lines.pop(); // preserve trailing partial line

                let currentEvent = "message";
                for (const line of lines) {
                    const trimmed = line.trim();
                    if (trimmed.startsWith("event:")) {
                        currentEvent = trimmed.slice(6).trim();
                    } else if (trimmed.startsWith("data:")) {
                        const dataStr = trimmed.slice(5).trim();
                        try {
                            const data = JSON.parse(dataStr);
                            handleHarvestEvent(currentEvent, data);
                        } catch (_) {
                            // Ignore json parse errors on malformed lines
                        }
                    }
                }
            }
        } else {
            const data = await res.json();
            const msg = `Harvest complete: ${data.downloaded} downloaded, ${data.skipped} skipped.`;
            dom.harvestProgressBar.style.width = "100%";
            dom.harvestProgressText.textContent = msg;
            showToast(msg, "success");
            await fetchStagingQueue();
            fetchStatus();
        }
    } catch (err) {
        dom.harvestProgressText.textContent = `Harvest failed: ${err.message}`;
        showToast(`Harvest error: ${err.message}`, "danger");
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
