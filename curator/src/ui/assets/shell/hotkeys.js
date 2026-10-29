/**
 * Global event wiring: stage pointer/wheel handlers, toolbar bindings, the
 * drawer and modal control setup, and the keyboard hotkey dispatcher.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { closeHelpModal, toggleHelpModal } from "./help.js";
import { fetchStatus } from "./status.js";
import { fitToScreen, getTriageViewport, stopStageMotion } from "../stage/canvas.js";
import {
    enterLabelingMode,
    exitLabelingMode,
    nudgeGridOffset,
    resetGrid,
    setGridSize,
    toggleGrid,
    toggleGridAlignMode,
    toggleGridPanel,
    updateGridPanel,
} from "./grid-panel.js";
import {
    closeBenchmarkDrawer,
    closeBudgetGuardModal,
    setupBenchmarkControls,
    toggleBenchmarkDrawer,
} from "../views/benchmark.js";
import {
    closeCurateDecisionModal,
    curateCurrentCandidate,
    handleDecisionModalKeydown,
    rejectCurrentCandidate,
    setupDecisionModalControls,
    undoRejectCandidate,
} from "../views/curation.js";
import { closeIngestionDrawer, setupDrawerControls, toggleIngestionDrawer } from "../views/ingestion.js";
import {
    closeLibraryInspector,
    saveLibraryInspectorChanges,
    setupInspectorDrawerControls,
} from "../views/inspector.js";
import {
    clearLibrarySelection,
    handleLibraryKeydown,
    setLibraryFocus,
    setupLibraryControls,
    switchView,
} from "../views/library.js";
import { setupReadinessControls } from "../views/readiness.js";
import { closePurgeModal, closeStorageModal, setupStorageControls } from "../views/storage.js";
import {
    closeTrainingDrawer,
    setupTrainingControls,
    toggleTrainingDrawer,
} from "../views/training.js";
import {
    assignRole,
    cycleLayer,
    fetchStagingQueue,
    handleSplitOrRegroup,
    navigateNext,
    navigatePrev,
    stepLayerElevation,
    toggleActiveLayerExclude,
} from "../views/triage.js";
import { closeDeleteCuratedModal } from "../views/library/bulk.js";
import {
    activeFsPanKeys,
    activeFsZoomKeys,
    closeLibraryFullscreen,
    requestFullscreenRender,
    saveFullscreenMapGrid,
    setupFullscreenControls,
    stopFsPanLoop,
    stopFsZoomLoop,
    toggleFullscreenGridPanel,
    updateFullscreenGridPanel,
} from "../views/library/fullscreen.js";

export function setupEventListeners() {
    // --- Canvas Mouse Pan Dragging & Dynamic Grid Align Drag ---
    dom.canvas.addEventListener("mousedown", (e) => {
        if ((e.button === 0) && (e.shiftKey || state.gridAlignMode)) {
            state.isDraggingGrid = true;
            state.gridDragStart = { x: e.clientX, y: e.clientY };
            const offset = getTriageViewport().getView().gridOffset;
            state.gridDragOrigin = { x: offset.x, y: offset.y };
            dom.canvas.classList.add("grid-dragging");
            e.preventDefault();
            return;
        }

        // Left click (0) or Middle click (1)
        if (e.button === 0 || e.button === 1) {
            state.isPanning = true;
            state.dragStart = { x: e.clientX, y: e.clientY };
            dom.canvas.classList.add("panning");
            e.preventDefault();
        }
    });

    window.addEventListener("mousemove", (e) => {
        if (dom.canvas && !state.isDraggingGrid) {
            dom.canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
        }
        if (dom.libraryFullscreenCanvas?.parentElement && !state.libraryFullscreenIsDraggingGrid) {
            dom.libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", e.shiftKey || state.libraryFullscreenGridAlignMode);
        }

        if (state.libraryFullscreenActive) {
            if (state.libraryFullscreenIsDraggingGrid) {
                const imgDx = (e.clientX - state.libraryFullscreenGridDragStart.x) / state.libraryFullscreenZoom;
                const imgDy = (e.clientY - state.libraryFullscreenGridDragStart.y) / state.libraryFullscreenZoom;
                state.libraryFullscreenGridOffset.x = Math.round(state.libraryFullscreenGridOffsetStart.x + imgDx);
                state.libraryFullscreenGridOffset.y = Math.round(state.libraryFullscreenGridOffsetStart.y + imgDy);
                saveFullscreenMapGrid();
                updateFullscreenGridPanel();
                requestFullscreenRender();
                return;
            }

            if (state.libraryFullscreenIsPanning) {
                state.libraryFullscreenPan.x = e.clientX - state.libraryFullscreenPanStart.x;
                state.libraryFullscreenPan.y = e.clientY - state.libraryFullscreenPanStart.y;
                requestFullscreenRender();
                return;
            }
        }

        if (state.isDraggingGrid) {
            const viewport = getTriageViewport();
            const zoom = viewport.getView().zoom;
            const imgDx = (e.clientX - state.gridDragStart.x) / zoom;
            const imgDy = (e.clientY - state.gridDragStart.y) / zoom;
            viewport.setGridOffset({
                x: Math.round(state.gridDragOrigin.x + imgDx),
                y: Math.round(state.gridDragOrigin.y + imgDy),
            });
            updateGridPanel();
            return;
        }

        if (!state.isPanning) return;
        getTriageViewport().panBy(e.clientX - state.dragStart.x, e.clientY - state.dragStart.y);
        state.dragStart = { x: e.clientX, y: e.clientY };
    });

    window.addEventListener("mouseup", (e) => {
        if (state.libraryFullscreenIsDraggingGrid) {
            state.libraryFullscreenIsDraggingGrid = false;
            saveFullscreenMapGrid();
            dom.libraryFullscreenCanvas?.parentElement?.classList.remove("grid-dragging");
        }
        if (state.libraryFullscreenIsPanning) {
            state.libraryFullscreenIsPanning = false;
            dom.libraryFullscreenCanvas?.parentElement?.classList.remove("panning");
        }
        if (state.isDraggingGrid) {
            state.isDraggingGrid = false;
            dom.canvas.classList.remove("grid-dragging");
        }
        if (state.isPanning) {
            state.isPanning = false;
            dom.canvas.classList.remove("panning");
        }
    });

    // --- Canvas Wheel Zoom ---
    dom.canvas.addEventListener("wheel", (e) => {
        e.preventDefault();
        if (!state.currentImage) return;

        const rect = dom.canvas.getBoundingClientRect();
        const point = {
            x: e.clientX - rect.left,
            y: e.clientY - rect.top,
        };

        const factor = e.deltaY < 0 ? 1.15 : (1 / 1.15);
        getTriageViewport().zoomBy(factor, point);
    }, { passive: false });

    // --- Toolbar Button Actions ---
    dom.btnGridToggle.addEventListener("click", () => toggleGrid());
    // --- Grid Calibration Panel Event Listeners ---
    if (dom.btnGridAdjust) {
        dom.btnGridAdjust.addEventListener("click", () => toggleGridPanel());
    }
    if (dom.btnCloseGridPanel) {
        dom.btnCloseGridPanel.addEventListener("click", () => toggleGridPanel(false));
    }
    if (dom.btnGridDone) {
        dom.btnGridDone.addEventListener("click", () => toggleGridPanel(false));
    }
    if (dom.btnGridReset) {
        dom.btnGridReset.addEventListener("click", () => resetGrid());
    }
    if (dom.btnGridSizeDec) {
        dom.btnGridSizeDec.addEventListener("click", () => setGridSize(getTriageViewport().getView().gridSize - 1));
    }
    if (dom.btnGridSizeInc) {
        dom.btnGridSizeInc.addEventListener("click", () => setGridSize(getTriageViewport().getView().gridSize + 1));
    }
    if (dom.gridSizeInput) {
        dom.gridSizeInput.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setGridSize(val);
        });
        dom.gridSizeInput.addEventListener("change", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setGridSize(val);
        });
    }
    if (dom.gridSizeSlider) {
        dom.gridSizeSlider.addEventListener("input", (e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val)) setGridSize(val);
        });
    }
    if (dom.btnGridNudgeUp) {
        dom.btnGridNudgeUp.addEventListener("click", () => nudgeGridOffset(0, -1));
    }
    if (dom.btnGridNudgeDown) {
        dom.btnGridNudgeDown.addEventListener("click", () => nudgeGridOffset(0, 1));
    }
    if (dom.btnGridNudgeLeft) {
        dom.btnGridNudgeLeft.addEventListener("click", () => nudgeGridOffset(-1, 0));
    }
    if (dom.btnGridNudgeRight) {
        dom.btnGridNudgeRight.addEventListener("click", () => nudgeGridOffset(1, 0));
    }
    if (dom.btnGridAlignDrag) {
        dom.btnGridAlignDrag.addEventListener("click", () => toggleGridAlignMode());
    }

    const presetChips = document.querySelectorAll(".preset-chip");
    presetChips.forEach((chip) => {
        chip.addEventListener("click", () => {
            const sz = parseInt(chip.getAttribute("data-size"), 10);
            if (!isNaN(sz)) setGridSize(sz);
        });
    });

    dom.btnFitScreen.addEventListener("click", () => fitToScreen());
    document.getElementById("btn-zoom-in").addEventListener("click", () => getTriageViewport().zoomBy(1.25));
    document.getElementById("btn-zoom-out").addEventListener("click", () => getTriageViewport().zoomBy(1 / 1.25));

    dom.btnPrev.addEventListener("click", () => navigatePrev());
    dom.btnNext.addEventListener("click", () => navigateNext());
    dom.btnCurate.addEventListener("click", () => curateCurrentCandidate());
    dom.btnUndoReject?.addEventListener("click", () => undoRejectCandidate());
    dom.btnReject.addEventListener("click", () => rejectCurrentCandidate());
    document.getElementById("btn-help").addEventListener("click", () => toggleHelpModal());
    document.getElementById("btn-close-help-modal").addEventListener("click", () => closeHelpModal());
    document.getElementById("btn-dismiss-help").addEventListener("click", () => closeHelpModal());


    document.getElementById("btn-refresh-staging").addEventListener("click", () => {
        fetchStatus();
        fetchStagingQueue();
    });


    // Filmstrip deck buttons & role badges
    if (dom.btnSplitBundle) {
        dom.btnSplitBundle.addEventListener("click", () => handleSplitOrRegroup());
    }
    if (dom.btnSplitBundleHud) {
        dom.btnSplitBundleHud.addEventListener("click", () => handleSplitOrRegroup());
    }

    const roleBadges = document.querySelectorAll(".role-badge");
    roleBadges.forEach((badge) => {
        badge.addEventListener("click", () => {
            const role = badge.getAttribute("data-role");
            if (role) assignRole(role);
        });
    });

    // Modal backdrop click to close
    dom.helpModal.addEventListener("click", (e) => {
        if (e.target === dom.helpModal) closeHelpModal();
    });
    setupDrawerControls();
    setupBenchmarkControls();
    setupStorageControls();
    setupReadinessControls();
    setupLibraryControls();
    setupFullscreenControls();
    setupInspectorDrawerControls();
    setupDecisionModalControls();
    setupTrainingControls();

    // --- Keyboard Hotkey Dispatcher ---
    window.addEventListener("keydown", handleKeydown);
    window.addEventListener("keyup", handleKeyup);
    window.addEventListener("blur", () => {
        stopStageMotion();
        stopFsPanLoop();
        stopFsZoomLoop();
        if (state.isDraggingGrid) {
            state.isDraggingGrid = false;
            dom.canvas.classList.remove("grid-dragging");
        }
        if (dom.canvas) {
            dom.canvas.classList.toggle("grid-aligning", state.gridAlignMode);
        }
        if (state.libraryFullscreenIsDraggingGrid) {
            state.libraryFullscreenIsDraggingGrid = false;
            dom.libraryFullscreenCanvas?.parentElement?.classList.remove("grid-dragging");
        }
        if (state.libraryFullscreenIsPanning) {
            state.libraryFullscreenIsPanning = false;
            dom.libraryFullscreenCanvas?.parentElement?.classList.remove("panning");
        }
        if (dom.libraryFullscreenCanvas?.parentElement) {
            dom.libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", state.libraryFullscreenGridAlignMode);
        }
    });
    window.addEventListener("focusin", (e) => {
        const tag = e.target?.tagName?.toLowerCase();
        if (tag === "input" || tag === "textarea" || tag === "select") {
            stopStageMotion();
            stopFsPanLoop();
            stopFsZoomLoop();
        }
    });
}

function handleKeydown(e) {
    if (dom.canvas) {
        dom.canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
    }
    const tag = document.activeElement ? document.activeElement.tagName.toLowerCase() : "";
    const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

    // Global Modal Dismiss: Escape
    if (e.key === "Escape") {
        stopStageMotion();
        if (state.gridPanelOpen) {
            toggleGridPanel(false);
            e.preventDefault();
            return;
        }
        if (state.labelingMode) {
            exitLabelingMode();
            e.preventDefault();
            return;
        }
        if (state.purgeModalOpen) {
            closePurgeModal();
            e.preventDefault();
            return;
        }
        if (state.storageModalOpen) {
            closeStorageModal();
            e.preventDefault();
            return;
        }
        if (state.budgetGuardModalOpen) {
            closeBudgetGuardModal();
            e.preventDefault();
            return;
        }
        if (state.trainingDrawerOpen) {
            closeTrainingDrawer();
            e.preventDefault();
            return;
        }
        if (state.benchmarkDrawerOpen) {
            closeBenchmarkDrawer();
            e.preventDefault();
            return;
        }
        if (state.drawerOpen) {
            closeIngestionDrawer();
            e.preventDefault();
            return;
        }
        if (state.decisionModalOpen) {
            closeCurateDecisionModal();
            e.preventDefault();
            return;
        }
        if (state.helpModalOpen) {
            closeHelpModal();
            e.preventDefault();
            return;
        }
        if (state.currentView === "library") {
            if (state.deleteModalOpen) {
                closeDeleteCuratedModal();
                e.preventDefault();
                return;
            }
            if (state.libraryFullscreenActive) {
                if (state.libraryFullscreenGridPanelOpen) {
                    toggleFullscreenGridPanel(false);
                } else {
                    closeLibraryFullscreen();
                }
                e.preventDefault();
                return;
            }
            if (state.libraryDrawerActive) {
                closeLibraryInspector();
                e.preventDefault();
                return;
            }
            if (state.librarySelectedIds.size > 0) {
                clearLibrarySelection();
                e.preventDefault();
                return;
            }
            const activeEl = document.activeElement;
            if (isInputFocused) {
                activeEl.blur();
            } else if (state.libraryFocusedIndex !== -1) {
                setLibraryFocus(-1);
            }
            e.preventDefault();
            return;
        }
        rejectCurrentCandidate();
        e.preventDefault();
        return;
    }
    // Hotkeys inside Curate Decision Modal (Issue #102)
    if (state.decisionModalOpen) {
        handleDecisionModalKeydown(e);
        return;
    }

    // Inspector Drawer Save Changes (Ctrl+S / Cmd+S)
    if (state.libraryDrawerActive && (e.ctrlKey || e.metaKey) && (e.key === "s" || e.key === "S")) {
        saveLibraryInspectorChanges();
        e.preventDefault();
        return;
    }


    // Ignore triage hotkeys if typing in an input
    if (isInputFocused) return;

    // View switching & training drawer hotkeys: R for training, L for library, T for triage
    if (!e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.key === "r" || e.key === "R") {
            toggleTrainingDrawer();
            e.preventDefault();
            return;
        }
        if (e.key === "l" || e.key === "L") {
            switchView("library");
            e.preventDefault();
            return;
        }
        if (e.key === "t" || e.key === "T") {
            switchView("triage");
            e.preventDefault();
            return;
        }
    }

    // Suppress triage controls when in Library view; delegate to library hotkeys
    if (state.currentView === "library") {
        handleLibraryKeydown(e);
        return;
    }


    // Secondary selection handling for active labeling mode

    if (state.labelingMode === "role") {
        const roleKeys = {
            "1": "ground",
            "2": "upper_floor",
            "3": "basement",
            "4": "roof",
            "5": "variant",
            "6": "exclude",
        };
        if (roleKeys[e.key]) {
            assignRole(roleKeys[e.key]);
            e.preventDefault();
            return;
        }
        if (["7", "8", "9"].includes(e.key)) {
            e.preventDefault();
            return;
        }
    }

    // Ingestion drawer hotkey: H
    if (e.key === "h" || e.key === "H") {
        toggleIngestionDrawer();
        e.preventDefault();
        return;
    }

    // Benchmark drawer hotkey: B
    if (e.key === "b" || e.key === "B") {
        toggleBenchmarkDrawer();
        e.preventDefault();
        return;
    }

    // Help modal hotkey: ?
    if (e.key === "?" || (e.shiftKey && e.key === "/")) {
        toggleHelpModal();
        e.preventDefault();
        return;
    }

    // Two-step labeling mode trigger: 3 (Layer Role)
    if (e.key === "3" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
        enterLabelingMode("role");
        e.preventDefault();
        return;
    }
    // Dynamic Grid Calibration Panel: Shift + G
    if (e.shiftKey && (e.key === "g" || e.key === "G")) {
        toggleGridPanel();
        e.preventDefault();
        return;
    }

    // 64px VTT Grid toggle: G (unmodified)
    if ((e.key === "g" || e.key === "G") && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
        toggleGrid();
        e.preventDefault();
        return;
    }

    // Shift + Arrow keys: Nudge Grid Offset
    if (e.shiftKey && !e.ctrlKey && !e.metaKey) {
        const step = e.altKey ? 5 : 1;
        if (e.key === "ArrowUp" || e.key === "Up") {
            nudgeGridOffset(0, -step);
            e.preventDefault();
            return;
        }
        if (e.key === "ArrowDown" || e.key === "Down") {
            nudgeGridOffset(0, step);
            e.preventDefault();
            return;
        }
        if (e.key === "ArrowLeft" || e.key === "Left") {
            nudgeGridOffset(-step, 0);
            e.preventDefault();
            return;
        }
        if (e.key === "ArrowRight" || e.key === "Right") {
            nudgeGridOffset(step, 0);
            e.preventDefault();
            return;
        }
    }

    // Shift + [ / ]: Adjust Grid Size
    if (e.shiftKey && (e.key === "[" || e.key === "{" || e.code === "BracketLeft")) {
        const sizeStep = e.altKey ? 5 : 1;
        setGridSize(getTriageViewport().getView().gridSize - sizeStep);
        e.preventDefault();
        return;
    }
    if (e.shiftKey && (e.key === "]" || e.key === "}" || e.code === "BracketRight")) {
        const sizeStep = e.altKey ? 5 : 1;
        setGridSize(getTriageViewport().getView().gridSize + sizeStep);
        e.preventDefault();
        return;
    }

    // Fit to screen: F
    if (e.key === "f" || e.key === "F") {
        fitToScreen();
        e.preventDefault();
        return;
    }

    // Viewport Panning: WASD
    if (!isInputFocused && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const key = e.key.toLowerCase();
        if (key === "w" || key === "a" || key === "s" || key === "d") {
            getTriageViewport().pressPanKey(key);
            e.preventDefault();
            return;
        }
    }

    // Viewport Zooming: [ and ]
    if (!isInputFocused && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.key === "]" || e.code === "BracketRight") {
            getTriageViewport().setZoomKey("in", true);
            e.preventDefault();
            return;
        }
        if (e.key === "[" || e.code === "BracketLeft") {
            getTriageViewport().setZoomKey("out", true);
            e.preventDefault();
            return;
        }
    }

    // Layer Navigation: Tab / Shift+Tab or Up / Down arrows
    if (e.key === "Tab") {
        cycleLayer(e.shiftKey ? -1 : 1);
        e.preventDefault();
        return;
    }
    if (e.altKey && (e.key === "ArrowUp" || e.key === "Up")) {
        stepLayerElevation(1);
        e.preventDefault();
        return;
    }
    if (e.altKey && (e.key === "ArrowDown" || e.key === "Down")) {
        stepLayerElevation(-1);
        e.preventDefault();
        return;
    }
    if (!e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowUp" || e.key === "Up")) {
        cycleLayer(-1);
        e.preventDefault();
        return;
    }
    if (!e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowDown" || e.key === "Down")) {
        cycleLayer(1);
        e.preventDefault();
        return;
    }

    // Space hotkey: Quick toggle Exclude on active layer
    if (e.key === " " || e.code === "Space") {
        toggleActiveLayerExclude();
        e.preventDefault();
        return;
    }

    // Split / Regroup bundle: U or Ctrl+Z
    if ((e.key === "u" || e.key === "U") && !e.ctrlKey && !e.metaKey && !e.altKey) {
        handleSplitOrRegroup();
        e.preventDefault();
        return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === "z" || e.key === "Z")) {
        const currentCand = state.candidates[state.currentIndex];
        if (currentCand?.parent_group_id || state.lastSplitParentId) {
            handleSplitOrRegroup();
            e.preventDefault();
            return;
        }
    }

    // Zoom: + or = / - or _
    if (e.key === "+" || e.key === "=") {
        getTriageViewport().zoomBy(1.2);
        e.preventDefault();
        return;
    }
    if (e.key === "-" || e.key === "_") {
        getTriageViewport().zoomBy(1 / 1.2);
        e.preventDefault();
        return;
    }

    // Navigation: Next (J or Right)
    if (e.key === "j" || e.key === "J" || e.key === "ArrowRight") {
        navigateNext();
        e.preventDefault();
        return;
    }

    // Navigation: Prev (K or Left)
    if (e.key === "k" || e.key === "K" || e.key === "ArrowLeft") {
        navigatePrev();
        e.preventDefault();
        return;
    }

    // Curate action: Enter
    if (e.key === "Enter") {
        curateCurrentCandidate();
        e.preventDefault();
        return;
    }

    // Reject action: X or Delete
    if (e.key === "x" || e.key === "X" || e.key === "Delete") {
        rejectCurrentCandidate();
        e.preventDefault();
        return;
    }

    // Undo candidate rejection: Z or Ctrl+Z / Cmd+Z (without Alt)
    if (!e.altKey && (e.key === "z" || e.key === "Z")) {
        undoRejectCandidate();
        e.preventDefault();
        return;
    }
}

function handleKeyup(e) {
    if (dom.canvas) {
        dom.canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
    }
    if (dom.libraryFullscreenCanvas?.parentElement) {
        dom.libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", e.shiftKey || state.libraryFullscreenGridAlignMode);
    }
    const key = e.key.toLowerCase();
    if (key === "w" || key === "a" || key === "s" || key === "d") {
        getTriageViewport().releasePanKey(key);
        activeFsPanKeys[key] = false;
        if (!activeFsPanKeys.w && !activeFsPanKeys.a && !activeFsPanKeys.s && !activeFsPanKeys.d) {
            stopFsPanLoop();
        }
    }
    if (e.key === "]" || e.code === "BracketRight") {
        getTriageViewport().setZoomKey("in", false);
        activeFsZoomKeys.in = false;
        if (!activeFsZoomKeys.in && !activeFsZoomKeys.out) {
            stopFsZoomLoop();
        }
    }
    if (e.key === "[" || e.code === "BracketLeft") {
        getTriageViewport().setZoomKey("out", false);
        activeFsZoomKeys.out = false;
        if (!activeFsZoomKeys.in && !activeFsZoomKeys.out) {
            stopFsZoomLoop();
        }
    }
}
