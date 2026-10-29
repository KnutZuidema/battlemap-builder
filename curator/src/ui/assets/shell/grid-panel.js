/**
 * Grid Configuration panel: cell size, offset nudging, alignment mode, and the
 * full-grid lattice readout for the triage candidate.
 *
 * The display state itself lives in the triage Stage Viewport; this module only
 * drives it and mirrors it into the panel DOM.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { VTT_GRID_SIZE, MIN_GRID_SIZE, calculateLatticeInfo, formatLatticePanel } from "../kit/grid-math.js";
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
    const viewport = getTriageViewport();
    const next = typeof force === "boolean" ? force : !viewport.getView().gridPanelOpen;
    viewport.setGridPanelOpen(next);
    if (dom.gridAdjustPanel) {
        dom.gridAdjustPanel.classList.toggle("hidden", !next);
    }
    if (dom.btnGridAdjust) {
        dom.btnGridAdjust.classList.toggle("active", next);
    }
    if (next) {
        updateGridPanel();
    }
    requestRender();
}

export function toggleGridAlignMode(force) {
    const viewport = getTriageViewport();
    const next = typeof force === "boolean" ? force : !viewport.getView().gridAlignMode;
    viewport.setGridAlignMode(next);
    if (dom.btnGridAlignDrag) {
        dom.btnGridAlignDrag.classList.toggle("active", next);
    }
    if (dom.canvas) {
        dom.canvas.classList.toggle("grid-aligning", next);
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
    const readout = formatLatticePanel(lattice);
    if (dom.gridLatticeCells) {
        dom.gridLatticeCells.textContent = readout.cells;
    }
    if (dom.gridLatticeSections) {
        dom.gridLatticeSections.textContent = readout.sections;
        dom.gridLatticeSections.className = lattice.isEligible
            ? "grid-lattice-sections-val mono badge-sections badge-sections-valid"
            : "grid-lattice-sections-val mono badge-sections badge-sections-warning";
    }
}

export function toggleGrid() {
    const viewport = getTriageViewport();
    const visible = !viewport.getView().gridVisible;
    viewport.setGridVisible(visible);
    if (visible) {
        dom.btnGridToggle.classList.add("active");
    } else {
        dom.btnGridToggle.classList.remove("active");
    }
    requestRender();
    updateHud();
    const size = viewport.getView().gridSize || VTT_GRID_SIZE;
    showToast(`${size}px VTT Grid: ${visible ? "Enabled" : "Disabled"}`, "info");
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
