/**
 * Connection status, HUD readouts, the loading overlay and toast notifications.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE, calculateLatticeInfo } from "../kit/grid-math.js";
import { getCurrentGroundDimensions } from "../kit/layers.js";

export async function fetchStatus() {
    try {
        const res = await fetch("/api/status");
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();

        dom.statusIndicator.className = "status-pill status-connected";
        dom.statusText.textContent = `Online (${data.staging_count})`;
        dom.statusIndicator.title = `Curator API Online | Staging: ${data.staging_count} | Curated: ${data.curated_count}`;
    } catch (err) {
        dom.statusIndicator.className = "status-pill status-error";
        dom.statusText.textContent = "Offline";
        dom.statusIndicator.title = `API Disconnected: ${err.message}`;
    }
}

export function updateHud() {
    const zoomPercent = Math.round(state.zoom * 100);
    dom.zoomLevelDisplay.textContent = `${zoomPercent}%`;
    dom.hudPanZoom.textContent = `Zoom: ${zoomPercent}% (${Math.round(state.pan.x)}, ${Math.round(state.pan.y)})`;

    const size = state.gridSize || VTT_GRID_SIZE;
    const ox = state.gridOffset ? Math.round(state.gridOffset.x) : 0;
    const oy = state.gridOffset ? Math.round(state.gridOffset.y) : 0;
    if (ox !== 0 || oy !== 0) {
        const signX = ox > 0 ? `+${ox}` : `${ox}`;
        const signY = oy > 0 ? `+${oy}` : `${oy}`;
        dom.hudGridStatus.textContent = `Grid: ${size}px (${signX}, ${signY}) [${state.gridVisible ? "ON" : "OFF"}]`;
    } else {
        dom.hudGridStatus.textContent = `Grid: ${size}px [${state.gridVisible ? "ON" : "OFF"}]`;
    }

    if (dom.hudLatticeStatus) {
        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (lattice.isEligible) {
            dom.hudLatticeStatus.textContent = `${lattice.cols} × ${lattice.rows} cells (${lattice.kx} × ${lattice.ky} sections)`;
            dom.hudLatticeStatus.classList.remove("hud-warning");
        } else {
            dom.hudLatticeStatus.textContent = `${lattice.cols} × ${lattice.rows} cells (< 16×16 cells)`;
            dom.hudLatticeStatus.classList.add("hud-warning");
        }
    }

    if (state.currentImage && state.currentImage.complete) {
        const imgW = state.currentImage.naturalWidth || state.currentImage.width;
        const imgH = state.currentImage.naturalHeight || state.currentImage.height;
        dom.hudResolution.textContent = `${imgW} × ${imgH} px`;
    } else {
        dom.hudResolution.textContent = "—";
    }

    // Count cached prefetch images
    let readyCount = 0;
    state.imageCache.forEach((img) => {
        if (img && img.complete && img.naturalWidth > 0) readyCount++;
    });
    dom.hudPrefetchStatus.textContent = `Prefetch: ${readyCount} ready`;

    // Update candidate metadata box in topbar
    if (state.currentIndex >= 0 && state.currentIndex < state.candidates.length) {
        const candidate = state.candidates[state.currentIndex];
        dom.candidateMetaBox.classList.remove("hidden");
        dom.queueCounter.textContent = `${state.currentIndex + 1} / ${state.candidates.length}`;
        dom.candidateId.textContent = candidate.id;
        dom.candidateDimensions.textContent = `${candidate.dimensions[0]} × ${candidate.dimensions[1]}`;
        dom.candidateSource.textContent = candidate.source;
        dom.candidateSource.className = `meta-tag candidate-source badge-source source-${candidate.source}`;
    } else {
        dom.candidateMetaBox.classList.add("hidden");
    }
}

export function showEmptyState(visible) {
    if (visible) {
        dom.emptyState.classList.remove("hidden");
    } else {
        dom.emptyState.classList.add("hidden");
    }
}

export function showLoading(visible, text = "Loading...") {
    if (visible) {
        dom.loadingText.textContent = text;
        dom.loadingOverlay.classList.remove("hidden");
    } else {
        dom.loadingOverlay.classList.add("hidden");
    }
}

export function showToast(message, type = "info", action = null) {
    const container = document.getElementById("toast-container");
    if (!container) return;

    const toast = document.createElement("div");
    toast.className = `toast toast-${type}`;

    const textSpan = document.createElement("span");
    textSpan.className = "toast-message";
    textSpan.textContent = message;
    toast.appendChild(textSpan);

    if (action && typeof action.onClick === "function") {
        const actionBtn = document.createElement("button");
        actionBtn.type = "button";
        actionBtn.className = "toast-action";
        actionBtn.textContent = action.label || "Undo";
        actionBtn.addEventListener("click", (e) => {
            e.stopPropagation();
            if (toast.parentElement) {
                toast.parentElement.removeChild(toast);
            }
            action.onClick();
        });
        toast.appendChild(actionBtn);
    }

    container.appendChild(toast);
    const duration = action ? 5000 : 3000;
    setTimeout(() => {
        if (toast.parentElement) {
            toast.parentElement.removeChild(toast);
        }
    }, duration);
}
