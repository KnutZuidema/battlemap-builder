/**
 * Grid Configuration panel: cell size, offset nudging, alignment mode, and the
 * full-grid lattice readout for the triage candidate.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE, MIN_GRID_SIZE, MAX_GRID_SIZE, calculateLatticeInfo } from "../kit/grid-math.js";
import { requestRender, stopPanLoop, stopZoomLoop } from "./canvas.js";
import { getCurrentGroundDimensions } from "../kit/layers.js";
import { showToast, updateHud } from "../shell/status.js";

let saveCandidateGridTimeout = null;
export function saveCurrentCandidateGrid() {
    if (state.currentIndex >= 0 && state.currentIndex < state.candidates.length) {
        const cand = state.candidates[state.currentIndex];
        if (cand && cand.id) {
            const payload = {
                cell_size_px: state.gridSize,
                offset_x: state.gridOffset.x,
                offset_y: state.gridOffset.y,
            };
            cand.grid = { ...payload };

            clearTimeout(saveCandidateGridTimeout);
            saveCandidateGridTimeout = setTimeout(async () => {
                try {
                    await fetch(`/api/staging/${encodeURIComponent(cand.id)}/grid`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify(payload),
                    });
                } catch (err) {
                    console.error("Failed to save staging candidate grid config:", err);
                }
            }, 300);
        }
    }
}

export function setGridSize(size) {
    const num = Number.isFinite(size) ? size : parseInt(size, 10);
    if (isNaN(num)) return;
    const clamped = Math.max(MIN_GRID_SIZE, Math.min(MAX_GRID_SIZE, Math.round(num)));
    state.gridSize = clamped;
    saveCurrentCandidateGrid();
    updateHud();
    updateGridPanel();
    requestRender();
}

export function nudgeGridOffset(dx, dy) {
    state.gridOffset.x = Math.round(state.gridOffset.x + dx);
    state.gridOffset.y = Math.round(state.gridOffset.y + dy);
    saveCurrentCandidateGrid();
    updateHud();
    updateGridPanel();
    requestRender();
}

function setGridOffset(x, y) {
    state.gridOffset.x = Math.round(x);
    state.gridOffset.y = Math.round(y);
    saveCurrentCandidateGrid();
    updateHud();
    updateGridPanel();
    requestRender();
}

export function resetGrid() {
    state.gridSize = 64;
    state.gridOffset = { x: 0, y: 0 };
    saveCurrentCandidateGrid();
    updateHud();
    updateGridPanel();
    requestRender();
    showToast("Grid reset to default 64px (0,0)", "info");
}

export function toggleGridPanel(force) {
    state.gridPanelOpen = typeof force === "boolean" ? force : !state.gridPanelOpen;
    if (dom.gridAdjustPanel) {
        dom.gridAdjustPanel.classList.toggle("hidden", !state.gridPanelOpen);
    }
    if (dom.btnGridAdjust) {
        dom.btnGridAdjust.classList.toggle("active", state.gridPanelOpen);
    }
    if (state.gridPanelOpen) {
        updateGridPanel();
    }
    requestRender();
}

export function toggleGridAlignMode(force) {
    state.gridAlignMode = typeof force === "boolean" ? force : !state.gridAlignMode;
    if (dom.btnGridAlignDrag) {
        dom.btnGridAlignDrag.classList.toggle("active", state.gridAlignMode);
    }
    if (dom.canvas) {
        dom.canvas.classList.toggle("grid-aligning", state.gridAlignMode);
    }
    requestRender();
}

export function updateGridPanel() {
    if (dom.gridSizeInput) {
        dom.gridSizeInput.value = state.gridSize;
    }
    if (dom.gridSizeSlider) {
        dom.gridSizeSlider.value = Math.min(256, Math.max(16, state.gridSize));
    }
    const ox = state.gridOffset ? Math.round(state.gridOffset.x) : 0;
    const oy = state.gridOffset ? Math.round(state.gridOffset.y) : 0;
    if (dom.gridOffsetXVal) {
        dom.gridOffsetXVal.textContent = ox > 0 ? `+${ox}px` : `${ox}px`;
    }
    if (dom.gridOffsetYVal) {
        dom.gridOffsetYVal.textContent = oy > 0 ? `+${oy}px` : `${oy}px`;
    }
    const chips = document.querySelectorAll(".preset-chip");
    chips.forEach((chip) => {
        const sz = parseInt(chip.getAttribute("data-size"), 10);
        chip.classList.toggle("active", sz === state.gridSize);
    });

    const dims = getCurrentGroundDimensions();
    const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
    if (dom.gridLatticeCells) {
        dom.gridLatticeCells.textContent = `${lattice.cols} × ${lattice.rows} cells`;
    }
    if (dom.gridLatticeSections) {
        if (lattice.isEligible) {
            dom.gridLatticeSections.textContent = `${lattice.kx} × ${lattice.ky} sections`;
            dom.gridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-valid";
        } else {
            dom.gridLatticeSections.textContent = "< 16×16 cells";
            dom.gridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-warning";
        }
    }
}

export function toggleGrid() {
    state.gridVisible = !state.gridVisible;
    if (state.gridVisible) {
        dom.btnGridToggle.classList.add("active");
    } else {
        dom.btnGridToggle.classList.remove("active");
    }
    requestRender();
    updateHud();
    const size = state.gridSize || VTT_GRID_SIZE;
    showToast(`${size}px VTT Grid: ${state.gridVisible ? "Enabled" : "Disabled"}`, "info");
}

export function enterLabelingMode(mode) {
    stopPanLoop();
    stopZoomLoop();
    state.labelingMode = mode;
    const roleSelectorEl = document.getElementById("role-selector");
    if (roleSelectorEl) roleSelectorEl.classList.remove("selecting-mode");

    if (mode === "role" && roleSelectorEl) {
        roleSelectorEl.classList.add("selecting-mode");
        showToast("Assign Layer Role: 1 Ground, 2 Upper, 3 Basement, 4 Roof, 5 Variant, 6 Exclude (Esc to cancel)", "info");
    }
}

export function exitLabelingMode() {
    stopPanLoop();
    stopZoomLoop();
    state.labelingMode = null;
    const roleSelectorEl = document.getElementById("role-selector");
    if (roleSelectorEl) roleSelectorEl.classList.remove("selecting-mode");
}
