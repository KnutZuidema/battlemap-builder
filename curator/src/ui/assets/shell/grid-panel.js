/**
 * Grid Configuration panel: cell size, offset nudging, alignment mode, and the
 * full-grid lattice readout for the triage candidate.
 *
 * The display state itself lives in the triage Stage Viewport; this module only
 * drives it and mirrors it into the panel DOM.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE, MIN_GRID_SIZE, calculateLatticeInfo } from "../kit/grid-math.js";
import { getTriageViewport, requestRender, stopStageMotion } from "../stage/canvas.js";
import { getCurrentGroundDimensions } from "../kit/layers.js";
import { showToast, updateHud } from "./status.js";

export function setGridSize(size) {
    getTriageViewport().setGridSize(size);
    updateHud();
    updateGridPanel();
}

export function nudgeGridOffset(dx, dy) {
    getTriageViewport().nudgeGrid(dx, dy);
    updateHud();
    updateGridPanel();
}

export function resetGrid() {
    getTriageViewport().resetGrid();
    updateHud();
    updateGridPanel();
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
    const grid = getTriageViewport().getView();
    if (dom.gridSizeInput) {
        dom.gridSizeInput.value = grid.gridSize;
    }
    if (dom.gridSizeSlider) {
        dom.gridSizeSlider.value = Math.min(256, Math.max(MIN_GRID_SIZE, grid.gridSize));
    }
    const ox = Math.round(grid.gridOffset.x);
    const oy = Math.round(grid.gridOffset.y);
    if (dom.gridOffsetXVal) {
        dom.gridOffsetXVal.textContent = ox > 0 ? `+${ox}px` : `${ox}px`;
    }
    if (dom.gridOffsetYVal) {
        dom.gridOffsetYVal.textContent = oy > 0 ? `+${oy}px` : `${oy}px`;
    }
    const chips = document.querySelectorAll(".preset-chip");
    chips.forEach((chip) => {
        const sz = parseInt(chip.getAttribute("data-size"), 10);
        chip.classList.toggle("active", sz === grid.gridSize);
    });

    const dims = getCurrentGroundDimensions();
    const lattice = calculateLatticeInfo(dims.width, dims.height, grid.gridSize, grid.gridOffset.x, grid.gridOffset.y);
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
    const size = getTriageViewport().getView().gridSize || VTT_GRID_SIZE;
    showToast(`${size}px VTT Grid: ${state.gridVisible ? "Enabled" : "Disabled"}`, "info");
}

export function enterLabelingMode(mode) {
    stopStageMotion();
    state.labelingMode = mode;
    const roleSelectorEl = document.getElementById("role-selector");
    if (roleSelectorEl) roleSelectorEl.classList.remove("selecting-mode");

    if (mode === "role" && roleSelectorEl) {
        roleSelectorEl.classList.add("selecting-mode");
        showToast("Assign Layer Role: 1 Ground, 2 Upper, 3 Basement, 4 Roof, 5 Variant, 6 Exclude (Esc to cancel)", "info");
    }
}

export function exitLabelingMode() {
    stopStageMotion();
    state.labelingMode = null;
    const roleSelectorEl = document.getElementById("role-selector");
    if (roleSelectorEl) roleSelectorEl.classList.remove("selecting-mode");
}
