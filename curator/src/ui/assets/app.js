/**
 * Dungeon Forge — Battlemap Curator Frontend Application
 * High-performance zero-dependency Canvas 2D triage deck, 64px VTT grid overlay,
 * zero-latency in-memory prefetching, and keyboard-driven hotkeys.
 */

import { state } from "./store.js";
import {
    VTT_GRID_SIZE,
    calculateLatticeInfo,
} from "./kit/grid-math.js";
import { formatBytes } from "./kit/format.js";
import { dom } from "./dom.js";
import {
    setupCanvas,
    requestRender,
    getCurrentGroundDimensions,
    fitToScreen,
    zoomStep,
    startPanLoop,
    stopPanLoop,
    startZoomLoop,
    stopZoomLoop,
    activePanKeys,
    activeZoomKeys,
} from "./stage/canvas.js";
import {
    saveCurrentCandidateGrid,
    setGridSize,
    nudgeGridOffset,
    resetGrid,
    toggleGridPanel,
    toggleGridAlignMode,
    updateGridPanel,
    toggleGrid,
    enterLabelingMode,
    exitLabelingMode,
} from "./stage/grid-panel.js";
import {
    fetchStagingQueue,
    loadCandidate,
    navigateNext,
    navigatePrev,
    cycleLayer,
    stepLayerElevation,
    assignRole,
    toggleActiveLayerExclude,
    handleSplitOrRegroup,
} from "./views/triage.js";
import {
    fetchCandidateDescription,
    closeCurateDecisionModal,
    handleDecisionReject,
    handleDecisionAccept,
    handleDecisionUndo,
    onDecisionTagInput,
    onDecisionTagInputKeydown,
    handleCustomTagAdd,
    handleCustomTagSubmit,
    curateCurrentCandidate,
    handleDecisionModalKeydown,
    decisionState,
    rejectCurrentCandidate,
    undoRejectCandidate,
} from "./views/curation.js";
import {
    switchView,
    clearLibrarySelection,
    setLibraryFocus,
    handleLibraryKeydown,
    setupLibraryControls,
} from "./views/library.js";
import { closeDeleteCuratedModal } from "./views/library/bulk.js";
import {
    closeLibraryFullscreen,
    toggleFullscreenGridPanel,
    saveFullscreenMapGrid,
    updateFullscreenGridPanel,
    stopFsPanLoop,
    stopFsZoomLoop,
    activeFsPanKeys,
    activeFsZoomKeys,
    setupFullscreenControls,
    requestFullscreenRender,
} from "./views/library/fullscreen.js";
import {
    closeLibraryInspector,
    saveLibraryInspectorChanges,
    setupInspectorDrawerControls,
} from "./views/inspector.js";
import {
    syncReadinessToScope,
    setupReadinessControls,
} from "./views/readiness.js";
import {
    fetchTrainingRuns,
    closeTrainingDrawer,
    toggleTrainingDrawer,
    setupTrainingControls,
} from "./views/training.js";

    // Constants
    const BUDGET_GUARDRAIL_THRESHOLD = 2.00;


    // DOM Elements cache
    let canvas, ctx;
    let statusIndicator, statusText;
    let candidateMetaBox, queueCounter, candidateId, candidateDimensions, candidateSource;
    let zoomLevelDisplay, hudPanZoom, hudGridStatus, hudLatticeStatus, hudResolution, hudPrefetchStatus;
    let btnGridToggle, btnFitScreen, btnPrev, btnNext, btnUndoReject, btnReject, btnCurate;
    // Grid Adjustment DOM Elements
    let btnGridAdjust, gridAdjustPanel, btnCloseGridPanel;
    let btnGridSizeDec, btnGridSizeInc, gridSizeInput, gridSizeSlider;
    let gridOffsetXVal, gridOffsetYVal;
    let gridLatticeCells, gridLatticeSections;
    let btnGridNudgeUp, btnGridNudgeDown, btnGridNudgeLeft, btnGridNudgeRight;
    let btnGridAlignDrag, btnGridReset, btnGridDone;
    let emptyState, loadingOverlay, loadingText;
    let helpModal;
    let ingestionDrawer, drawerPanel, btnOpenDrawer, btnEmptyHarvest, btnCloseDrawer, btnCancelDrawer;
    let harvestSortGroup, harvestTimeGroup, harvestLimitInput, harvestLimitValue, harvestGridlessOnly;
    let btnStartHarvest, harvestProgressContainer, harvestProgressBar, harvestProgressText;

    // Layer Deck & Filmstrip DOM Elements
    let layerDeck, layerDeckCount, layerDeckBundleTag, roleSelector, layerFilmstrip;
    let btnSplitBundle, btnSplitBundleHud, btnSplitHudText, btnSplitLabel;

    // Benchmark Drawer & Guardrails DOM Elements
    let btnOpenBenchmarkDrawer;
    let costMonitorHud, costMonitorText;
    let benchmarkDrawer, btnCloseBenchmarkDrawer, btnCancelBenchmarkDrawer;
    let btnModelsSelectAll, btnModelsSelectNone, btnPromptsSelectAll, btnPromptsSelectNone;
    let benchmarkEnableCustom, customPromptFields, benchmarkCustomName, benchmarkCustomPrompt, benchmarkEnableReference;
    let benchmarkModelsList, benchmarkReferenceWarning;
    // Selectable models served by GET /api/models (ADR-0010); never hardcoded.
    let benchmarkModelCatalog = [];
    let benchmarkEnableCustomLora, benchmarkCustomLoraFields, benchmarkCustomLoraPath, benchmarkCustomLoraScale;
    let calcBadgeCost, calcFormulaDisplay, calcWarningNote;
    let btnStartBenchmark, btnStartBenchmarkText;
    let benchmarkProgressContainer, benchmarkProgressBar, benchmarkProgressText, btnCancelBenchmarkRun;
    let budgetGuardModal, btnCloseBudgetGuard, btnDismissBudgetGuard, btnConfirmBudgetGuard, btnConfirmBudgetGuardText;
    let budgetGuardCostAmount, budgetGuardBreakdown;

    // Floating Benchmark Progress Window & Pill Elements
    let benchmarkProgressWindow, btnBenchmarkWindowMinimize, btnBenchmarkWindowClose;
    let benchmarkWindowStatus, benchmarkWindowCurrentStep, benchmarkWindowProgressStats;
    let benchmarkWindowProgressBar, benchmarkWindowPreviews, benchmarkWindowPreviewCount;
    let benchmarkPreviewsEmpty, benchmarkWindowLogsContainer, benchmarkWindowLogs;
    let btnBenchmarkClearLogs, benchmarkWindowCost, btnBenchmarkWindowCancel;
    let benchmarkWindowPill, benchmarkPillCount, benchmarkPillCost;
    let activeGenerationIndicatorCard;

    // Dataset Library View DOM Elements (Issue #31)
    let tabNavTriage, tabNavLibrary;
    let triageViewport, libraryViewport;
    let librarySearchInput, libraryTagPills, libraryGrid, libraryEmpty, libraryFilterEmpty, libraryCountBadge;
    let librarySortSelect, libraryDensityGroup, libraryFacetFilters;
    let btnExportLora, btnExportLabel, btnRefreshLibrary, btnSwitchToTriage, btnClearLibraryFilters;
    let btnFreezeSnapshot, btnFreezeLabel;
    let freezeProgressPanel, freezePhaseBadge, freezeProgressMessage, freezeProgressCounts, freezeProgressPercent, freezeProgressBar, freezeDetailMap, freezeDetailSection;
    let datasetSnapshotsPanel, snapshotsList, snapshotsEmpty, snapshotsCountBadge;
    let btnRefreshSnapshots, btnToggleSnapshots, toggleSnapshotsText, snapshotsShelf;
    let datasetReadinessPanel, datasetReadinessStatus, datasetReadinessSummary, datasetReadinessIssues;
    let datasetReadinessIcon, btnAuditReadiness, btnAuditReadinessLabel;

    // Training Runs & LoRA Drawer Elements (Issue #209)
    let btnOpenTrainingDrawer, tabNavTraining, btnOpenTrainingFromLibrary;
    let trainingDrawer, trainingDrawerPanel, btnCloseTrainingDrawer, btnCancelTrainingDrawer;
    let tabTrainingRuns, tabTrainingAdapters, trainingViewRuns, trainingViewAdapters;
    let trainingRunsCountTab, trainingAdaptersCountTab, trainingRunsActiveBadge;
    let trainingLaunchCard, btnToggleLaunchForm, btnToggleLaunchText, trainingLaunchForm;
    let trainingSnapshotSelect, trainingPolicyGroup, trainingScopeHint;
    let trainingReadinessIndicator, btnCheckTrainingReadiness, btnStartTrainingRun, btnStartRunText, trainingLaunchError;
    let trainingRunsTotalBadge, trainingRunsFilter, btnRefreshTrainingRuns, trainingRunsList, trainingRunsEmpty;
    let adaptersCountBadge, trainingAdaptersFilter, btnRefreshTrainingAdapters, trainingAdaptersList, trainingAdaptersEmpty;

    // Curate Decision Modal DOM Elements (Issue #102)
    let curateDecisionModal;
    let decisionProgressBadge, decisionOfflineBadge;
    let decisionCardStep, decisionFocalCard, decisionCardTag;
    let btnDecisionReject, btnDecisionUndo, btnDecisionAccept;
    let decisionCustomStep, decisionOfflineNote, decisionTagInput, btnAddCustomTag;
    let decisionDescriptionInput, decisionDescriptionLoading, btnRegenerateDescription, decisionDescriptionStatus;
    let decisionAutocompleteList, decisionValidationError;
    let decisionAcceptedTags;
    let btnCloseDecisionModal, btnCancelDecision, btnFinishDecision;

    // Fullscreen Stage Overlay DOM Elements (Issue #134)
    let libraryFullscreenOverlay, libraryFullscreenCanvas, libraryFullscreenCtx;
    let libraryFullscreenHud;
    let fsMapIdBadge, fsMapDimensions, fsGridStatusBadge, fsMapCounter;
    let btnFsGridToggle, btnFsGridAdjust, btnFsFit;
    let btnFsZoomOut, btnFsZoomIn, fsZoomReadout;
    let btnFsPrevMap, btnFsNextMap, btnFsClose;
    let libraryFullscreenGridPanel, btnFsCloseGridPanel;
    let btnFsGridSizeDec, btnFsGridSizeInc, fsGridSizeInput, fsGridSizeSlider;
    let fsGridOffsetXVal, fsGridOffsetYVal;
    let fsGridLatticeCells, fsGridLatticeSections;
    let btnFsGridNudgeUp, btnFsGridNudgeDown, btnFsGridNudgeLeft, btnFsGridNudgeRight;
    let btnFsGridAlignDrag, btnFsGridReset, btnFsGridDone;
    let libraryFullscreenLayerDeck, fsLayerFilmstripStrip, fsLayerDeckCount;

    // Inspector Drawer DOM Elements (Issue #135 & Issue #136)
    let libraryInspectorDrawer, inspectorMapIdBadge, inspectorUnsavedBadge, btnCloseInspector;
    let inspectorLayerCountBadge, inspectorLayersList;
    let inspectorThemeChips, inspectorThemeInput, btnAddTheme;
    let inspectorTagChips, inspectorTagInput, btnAddTag;
    let inspectorDescCounter, inspectorDescriptionInput;
    let btnInspectorSuggestThemes, btnInspectorDescribe, btnInspectorRunAll;
    let inspectorAiSuggestionsBox, inspectorAiSuggestionsPills, btnAcceptAllSuggestions;
    let btnInspectorFullscreen, btnInspectorDiscard, btnInspectorSave;

    // Delete Curated Modal & Bulk Action Bar DOM Elements (Issue #137)
    let deleteCuratedModal, deleteCuratedTitle, deleteTargetPreview;
    let deleteReasonSelect, deleteNotesInput, deleteNotesGroup;
    let btnCancelDelete, btnConfirmDelete, btnConfirmDeleteText, btnCloseDeleteModal;
    let libraryBulkBar, bulkSelectedCount;
    let btnBulkDelete, btnBulkTag, btnBulkRefine, btnBulkRefineLabel, btnBulkExport, btnBulkDeselect;
    let btnInspectorDelete;

    // ==========================================================================
    // Initialization & Bootstrap
    // ==========================================================================

    function init() {
        cacheDOMElements();
        setupCanvas();
        setupEventListeners();
        updateGridPanel();
        fetchStatus();
        fetchCostStats();
        fetchStagingQueue();
        fetchStorageStats();
        fetchTrainingRuns();
        syncReadinessToScope();
    }

    function cacheDOMElements() {
        canvas = document.getElementById("triage-canvas");
        ctx = canvas.getContext("2d");

        statusIndicator = document.getElementById("status-indicator");
        statusText = document.getElementById("status-text");
        candidateMetaBox = document.getElementById("candidate-meta-box");
        queueCounter = document.getElementById("queue-counter");
        candidateId = document.getElementById("candidate-id");
        candidateDimensions = document.getElementById("candidate-dimensions");
        candidateSource = document.getElementById("candidate-source");

        zoomLevelDisplay = document.getElementById("zoom-level");
        hudPanZoom = document.getElementById("hud-pan-zoom");
        hudGridStatus = document.getElementById("hud-grid-status");
        hudLatticeStatus = document.getElementById("hud-lattice-status");
        hudResolution = document.getElementById("hud-resolution");
        hudPrefetchStatus = document.getElementById("hud-prefetch-status");

        btnGridToggle = document.getElementById("btn-grid-toggle");
        btnGridAdjust = document.getElementById("btn-grid-adjust");
        gridAdjustPanel = document.getElementById("grid-adjust-panel");
        btnCloseGridPanel = document.getElementById("btn-close-grid-panel");
        btnGridSizeDec = document.getElementById("btn-grid-size-dec");
        btnGridSizeInc = document.getElementById("btn-grid-size-inc");
        gridSizeInput = document.getElementById("grid-size-input");
        gridSizeSlider = document.getElementById("grid-size-slider");
        gridOffsetXVal = document.getElementById("grid-offset-x-val");
        gridOffsetYVal = document.getElementById("grid-offset-y-val");
        gridLatticeCells = document.getElementById("grid-lattice-cells");
        gridLatticeSections = document.getElementById("grid-lattice-sections");
        btnGridNudgeUp = document.getElementById("btn-grid-nudge-up");
        btnGridNudgeDown = document.getElementById("btn-grid-nudge-down");
        btnGridNudgeLeft = document.getElementById("btn-grid-nudge-left");
        btnGridNudgeRight = document.getElementById("btn-grid-nudge-right");
        btnGridAlignDrag = document.getElementById("btn-grid-align-drag");
        btnGridReset = document.getElementById("btn-grid-reset");
        btnGridDone = document.getElementById("btn-grid-done");
        btnFitScreen = document.getElementById("btn-fit-screen");
        btnPrev = document.getElementById("btn-prev");
        btnNext = document.getElementById("btn-next");
        btnUndoReject = document.getElementById("btn-undo-reject");
        btnReject = document.getElementById("btn-reject");
        btnCurate = document.getElementById("btn-curate");

        emptyState = document.getElementById("empty-state");
        loadingOverlay = document.getElementById("loading-overlay");
        loadingText = document.getElementById("loading-text");

        layerDeck = document.getElementById("layer-deck");
        layerDeckCount = document.getElementById("layer-deck-count");
        layerDeckBundleTag = document.getElementById("layer-deck-bundle-tag");
        roleSelector = document.getElementById("role-selector");
        layerFilmstrip = document.getElementById("layer-filmstrip");
        btnSplitBundle = document.getElementById("btn-split-bundle");
        btnSplitBundleHud = document.getElementById("btn-split-bundle-hud");
        btnSplitHudText = document.getElementById("btn-split-hud-text");
        btnSplitLabel = document.getElementById("btn-split-label");

        helpModal = document.getElementById("help-modal");

        ingestionDrawer = document.getElementById("ingestion-drawer");
        drawerPanel = document.getElementById("drawer-panel");
        btnOpenDrawer = document.getElementById("btn-open-drawer");
        btnEmptyHarvest = document.getElementById("btn-empty-harvest");
        btnCloseDrawer = document.getElementById("btn-close-drawer");
        btnCancelDrawer = document.getElementById("btn-cancel-drawer");

        harvestSortGroup = document.getElementById("harvest-sort-group");
        harvestTimeGroup = document.getElementById("harvest-time-group");
        harvestLimitInput = document.getElementById("harvest-limit");
        harvestLimitValue = document.getElementById("harvest-limit-value");
        harvestGridlessOnly = document.getElementById("harvest-gridless-only");

        btnStartHarvest = document.getElementById("btn-start-harvest");
        harvestProgressContainer = document.getElementById("harvest-progress-container");
        harvestProgressBar = document.getElementById("harvest-progress-bar");
        harvestProgressText = document.getElementById("harvest-progress-text");

        // Benchmark Drawer & HUD Elements
        btnOpenBenchmarkDrawer = document.getElementById("btn-open-benchmark-drawer");
        costMonitorHud = document.getElementById("cost-monitor-hud");
        costMonitorText = document.getElementById("cost-monitor-text");

        benchmarkDrawer = document.getElementById("benchmark-drawer");
        btnCloseBenchmarkDrawer = document.getElementById("btn-close-benchmark-drawer");
        btnCancelBenchmarkDrawer = document.getElementById("btn-cancel-benchmark-drawer");

        btnModelsSelectAll = document.getElementById("btn-models-select-all");
        btnModelsSelectNone = document.getElementById("btn-models-select-none");
        btnPromptsSelectAll = document.getElementById("btn-prompts-select-all");
        btnPromptsSelectNone = document.getElementById("btn-prompts-select-none");

        benchmarkEnableCustom = document.getElementById("benchmark-enable-custom");
        customPromptFields = document.getElementById("custom-prompt-fields");
        benchmarkCustomName = document.getElementById("benchmark-custom-name");
        benchmarkCustomPrompt = document.getElementById("benchmark-custom-prompt");
        benchmarkEnableReference = document.getElementById("benchmark-enable-reference");
        benchmarkModelsList = document.getElementById("benchmark-models-list");
        benchmarkReferenceWarning = document.getElementById("benchmark-reference-warning");
        benchmarkEnableCustomLora = document.getElementById("benchmark-enable-custom-lora");
        benchmarkCustomLoraFields = document.getElementById("benchmark-custom-lora-fields");
        benchmarkCustomLoraPath = document.getElementById("benchmark-custom-lora-path");
        benchmarkCustomLoraScale = document.getElementById("benchmark-custom-lora-scale");

        calcBadgeCost = document.getElementById("calc-badge-cost");
        calcFormulaDisplay = document.getElementById("calc-formula-display");
        calcWarningNote = document.getElementById("calc-warning-note");

        btnStartBenchmark = document.getElementById("btn-start-benchmark");
        btnStartBenchmarkText = document.getElementById("btn-start-benchmark-text");

        benchmarkProgressContainer = document.getElementById("benchmark-progress-container");
        benchmarkProgressBar = document.getElementById("benchmark-progress-bar");
        benchmarkProgressText = document.getElementById("benchmark-progress-text");
        btnCancelBenchmarkRun = document.getElementById("btn-cancel-benchmark-run");

        budgetGuardModal = document.getElementById("budget-guard-modal");
        btnCloseBudgetGuard = document.getElementById("btn-close-budget-guard");
        btnDismissBudgetGuard = document.getElementById("btn-dismiss-budget-guard");
        btnConfirmBudgetGuard = document.getElementById("btn-confirm-budget-guard");
        btnConfirmBudgetGuardText = document.getElementById("btn-confirm-budget-guard-text");
        budgetGuardCostAmount = document.getElementById("budget-guard-cost-amount");
        budgetGuardBreakdown = document.getElementById("budget-guard-breakdown");

        // Floating Benchmark Progress Window & Pill Elements
        benchmarkProgressWindow = document.getElementById("benchmark-progress-window");
        btnBenchmarkWindowMinimize = document.getElementById("btn-benchmark-window-minimize");
        btnBenchmarkWindowClose = document.getElementById("btn-benchmark-window-close");
        benchmarkWindowStatus = document.getElementById("benchmark-window-status");
        benchmarkWindowCurrentStep = document.getElementById("benchmark-window-current-step");
        benchmarkWindowProgressStats = document.getElementById("benchmark-window-progress-stats");
        benchmarkWindowProgressBar = document.getElementById("benchmark-window-progress-bar");
        benchmarkWindowPreviews = document.getElementById("benchmark-window-previews");
        benchmarkWindowPreviewCount = document.getElementById("benchmark-window-preview-count");
        benchmarkPreviewsEmpty = document.getElementById("benchmark-previews-empty");
        benchmarkWindowLogsContainer = document.getElementById("benchmark-window-logs-container");
        benchmarkWindowLogs = document.getElementById("benchmark-window-logs");
        btnBenchmarkClearLogs = document.getElementById("btn-benchmark-clear-logs");
        benchmarkWindowCost = document.getElementById("benchmark-window-cost");
        btnBenchmarkWindowCancel = document.getElementById("btn-benchmark-window-cancel");
        benchmarkWindowPill = document.getElementById("benchmark-window-pill");
        benchmarkPillCount = document.getElementById("benchmark-pill-count");
        benchmarkPillCost = document.getElementById("benchmark-pill-cost");

        // Dataset Library View Elements
        tabNavTriage = document.getElementById("tab-nav-triage");
        tabNavLibrary = document.getElementById("tab-nav-library");
        triageViewport = document.getElementById("triage-viewport");
        libraryViewport = document.getElementById("library-viewport");
        librarySearchInput = document.getElementById("library-search-input");
        libraryTagPills = document.getElementById("library-tag-pills");
        libraryGrid = document.getElementById("library-grid");
        libraryEmpty = document.getElementById("library-empty");
        libraryFilterEmpty = document.getElementById("library-filter-empty");
        libraryCountBadge = document.getElementById("library-count-badge");
        librarySortSelect = document.getElementById("library-sort-select");
        libraryDensityGroup = document.getElementById("library-density-group");
        libraryFacetFilters = document.getElementById("library-facet-filters");
        btnExportLora = document.getElementById("btn-export-lora");
        btnExportLabel = document.getElementById("btn-export-label");
        btnRefreshLibrary = document.getElementById("btn-refresh-library");
        btnSwitchToTriage = document.getElementById("btn-switch-to-triage");
        btnClearLibraryFilters = document.getElementById("btn-clear-library-filters");
        btnFreezeSnapshot = document.getElementById("btn-freeze-snapshot");
        btnFreezeLabel = document.getElementById("btn-freeze-label");
        freezeProgressPanel = document.getElementById("freeze-progress-panel");
        freezePhaseBadge = document.getElementById("freeze-phase-badge");
        freezeProgressMessage = document.getElementById("freeze-progress-message");
        freezeProgressCounts = document.getElementById("freeze-progress-counts");
        freezeProgressPercent = document.getElementById("freeze-progress-percent");
        freezeProgressBar = document.getElementById("freeze-progress-bar");
        freezeDetailMap = document.getElementById("freeze-detail-map");
        freezeDetailSection = document.getElementById("freeze-detail-section");
        datasetSnapshotsPanel = document.getElementById("dataset-snapshots-panel");
        snapshotsList = document.getElementById("snapshots-list");
        snapshotsEmpty = document.getElementById("snapshots-empty");
        snapshotsCountBadge = document.getElementById("snapshots-count-badge");
        btnRefreshSnapshots = document.getElementById("btn-refresh-snapshots");
        btnToggleSnapshots = document.getElementById("btn-toggle-snapshots");
        toggleSnapshotsText = document.getElementById("toggle-snapshots-text");
        snapshotsShelf = document.getElementById("snapshots-shelf");
        datasetReadinessPanel = document.getElementById("dataset-readiness-panel");
        datasetReadinessStatus = document.getElementById("dataset-readiness-status");
        datasetReadinessSummary = document.getElementById("dataset-readiness-summary");
        datasetReadinessIssues = document.getElementById("dataset-readiness-issues");
        datasetReadinessIcon = document.getElementById("dataset-readiness-icon");
        btnAuditReadiness = document.getElementById("btn-audit-readiness");
        btnAuditReadinessLabel = document.getElementById("btn-audit-readiness-label");

        // Training Runs & LoRA Drawer Elements (Issue #209)
        btnOpenTrainingDrawer = document.getElementById("btn-open-training-drawer");
        tabNavTraining = document.getElementById("tab-nav-training");
        btnOpenTrainingFromLibrary = document.getElementById("btn-open-training-from-library");
        trainingDrawer = document.getElementById("training-drawer");
        trainingDrawerPanel = document.getElementById("training-drawer-panel");
        btnCloseTrainingDrawer = document.getElementById("btn-close-training-drawer");
        btnCancelTrainingDrawer = document.getElementById("btn-cancel-training-drawer");
        tabTrainingRuns = document.getElementById("tab-training-runs");
        tabTrainingAdapters = document.getElementById("tab-training-adapters");
        trainingViewRuns = document.getElementById("training-view-runs");
        trainingViewAdapters = document.getElementById("training-view-adapters");
        trainingRunsCountTab = document.getElementById("training-runs-count-tab");
        trainingAdaptersCountTab = document.getElementById("training-adapters-count-tab");
        trainingRunsActiveBadge = document.getElementById("training-runs-active-badge");
        trainingLaunchCard = document.getElementById("training-launch-card");
        btnToggleLaunchForm = document.getElementById("btn-toggle-launch-form");
        btnToggleLaunchText = document.getElementById("btn-toggle-launch-text");
        trainingLaunchForm = document.getElementById("training-launch-form");
        trainingSnapshotSelect = document.getElementById("training-snapshot-select");
        trainingPolicyGroup = document.querySelector(".training-policy-selector");
        trainingScopeHint = document.getElementById("training-scope-hint");
        trainingReadinessIndicator = document.getElementById("training-readiness-indicator");
        btnCheckTrainingReadiness = document.getElementById("btn-check-training-readiness");
        btnStartTrainingRun = document.getElementById("btn-start-training-run");
        btnStartRunText = document.getElementById("btn-start-run-text");
        trainingLaunchError = document.getElementById("training-launch-error");
        trainingRunsTotalBadge = document.getElementById("training-runs-total-badge");
        trainingRunsFilter = document.getElementById("training-runs-filter");
        btnRefreshTrainingRuns = document.getElementById("btn-refresh-training-runs");
        trainingRunsList = document.getElementById("training-runs-list");
        trainingRunsEmpty = document.getElementById("training-runs-empty");
        adaptersCountBadge = document.getElementById("adapters-count-badge");
        trainingAdaptersFilter = document.getElementById("training-adapters-filter");
        btnRefreshTrainingAdapters = document.getElementById("btn-refresh-training-adapters");
        trainingAdaptersList = document.getElementById("training-adapters-list");
        trainingAdaptersEmpty = document.getElementById("training-adapters-empty");

        // Curate Decision Modal Elements (Issue #102)
        curateDecisionModal = document.getElementById("curate-decision-modal");
        decisionProgressBadge = document.getElementById("decision-progress-badge");
        decisionOfflineBadge = document.getElementById("decision-offline-badge");
        decisionCardStep = document.getElementById("decision-card-step");
        decisionFocalCard = document.getElementById("decision-focal-card");
        decisionCardTag = document.getElementById("decision-card-tag");
        btnDecisionReject = document.getElementById("btn-decision-reject");
        btnDecisionUndo = document.getElementById("btn-decision-undo");
        btnDecisionAccept = document.getElementById("btn-decision-accept");
        decisionCustomStep = document.getElementById("decision-custom-step");
        decisionOfflineNote = document.getElementById("decision-offline-note");
        decisionTagInput = document.getElementById("decision-tag-input");
        btnAddCustomTag = document.getElementById("btn-add-custom-tag");
        decisionAutocompleteList = document.getElementById("decision-autocomplete-list");
        decisionValidationError = document.getElementById("decision-validation-error");
        decisionDescriptionInput = document.getElementById("decision-description-input");
        decisionDescriptionLoading = document.getElementById("decision-description-loading");
        btnRegenerateDescription = document.getElementById("btn-regenerate-description");
        decisionDescriptionStatus = document.getElementById("decision-description-status");
        decisionAcceptedTags = document.getElementById("decision-accepted-tags");
        btnCloseDecisionModal = document.getElementById("btn-close-decision-modal");
        btnCancelDecision = document.getElementById("btn-cancel-decision");
        btnFinishDecision = document.getElementById("btn-finish-decision");

        // Fullscreen Stage Overlay Elements (Issue #134)
        libraryFullscreenOverlay = document.getElementById("library-fullscreen-overlay");
        libraryFullscreenCanvas = document.getElementById("library-fullscreen-canvas");
        libraryFullscreenCtx = libraryFullscreenCanvas ? libraryFullscreenCanvas.getContext("2d") : null;
        libraryFullscreenHud = document.getElementById("library-fullscreen-hud");
        fsMapIdBadge = document.getElementById("fs-map-id-badge");
        fsMapDimensions = document.getElementById("fs-map-dimensions");
        fsGridStatusBadge = document.getElementById("fs-grid-status-badge");
        fsMapCounter = document.getElementById("fs-map-counter");
        btnFsGridToggle = document.getElementById("btn-fs-grid-toggle");
        btnFsGridAdjust = document.getElementById("btn-fs-grid-adjust");
        btnFsFit = document.getElementById("btn-fs-fit");
        btnFsZoomOut = document.getElementById("btn-fs-zoom-out");
        btnFsZoomIn = document.getElementById("btn-fs-zoom-in");
        fsZoomReadout = document.getElementById("fs-zoom-readout");
        btnFsPrevMap = document.getElementById("btn-fs-prev-map");
        btnFsNextMap = document.getElementById("btn-fs-next-map");
        btnFsClose = document.getElementById("btn-fs-close");
        libraryFullscreenGridPanel = document.getElementById("library-fullscreen-grid-panel");
        btnFsCloseGridPanel = document.getElementById("btn-fs-close-grid-panel");
        btnFsGridSizeDec = document.getElementById("btn-fs-grid-size-dec");
        btnFsGridSizeInc = document.getElementById("btn-fs-grid-size-inc");
        fsGridSizeInput = document.getElementById("fs-grid-size-input");
        fsGridSizeSlider = document.getElementById("fs-grid-size-slider");
        fsGridOffsetXVal = document.getElementById("fs-grid-offset-x-val");
        fsGridOffsetYVal = document.getElementById("fs-grid-offset-y-val");
        fsGridLatticeCells = document.getElementById("fs-grid-lattice-cells");
        fsGridLatticeSections = document.getElementById("fs-grid-lattice-sections");
        btnFsGridNudgeUp = document.getElementById("btn-fs-grid-nudge-up");
        btnFsGridNudgeDown = document.getElementById("btn-fs-grid-nudge-down");
        btnFsGridNudgeLeft = document.getElementById("btn-fs-grid-nudge-left");
        btnFsGridNudgeRight = document.getElementById("btn-fs-grid-nudge-right");
        btnFsGridAlignDrag = document.getElementById("btn-fs-grid-align-drag");
        btnFsGridReset = document.getElementById("btn-fs-grid-reset");
        btnFsGridDone = document.getElementById("btn-fs-grid-done");
        libraryFullscreenLayerDeck = document.getElementById("library-fullscreen-layer-deck");
        fsLayerFilmstripStrip = document.getElementById("fs-layer-filmstrip-strip");
        fsLayerDeckCount = document.getElementById("fs-layer-deck-count");

        // Inspector Drawer Elements (Issue #135 & Issue #136)
        libraryInspectorDrawer = document.getElementById("library-inspector-drawer");
        inspectorMapIdBadge = document.getElementById("inspector-map-id-badge");
        inspectorUnsavedBadge = document.getElementById("inspector-unsaved-badge");
        btnCloseInspector = document.getElementById("btn-close-inspector");
        inspectorLayerCountBadge = document.getElementById("inspector-layer-count-badge");
        inspectorLayersList = document.getElementById("inspector-layers-list");
        inspectorThemeChips = document.getElementById("inspector-theme-chips");
        inspectorThemeInput = document.getElementById("inspector-theme-input");
        btnAddTheme = document.getElementById("btn-add-theme");
        inspectorTagChips = document.getElementById("inspector-tag-chips");
        inspectorTagInput = document.getElementById("inspector-tag-input");
        btnAddTag = document.getElementById("btn-add-tag");
        inspectorDescCounter = document.getElementById("inspector-desc-counter");
        inspectorDescriptionInput = document.getElementById("inspector-description-input");
        btnInspectorSuggestThemes = document.getElementById("btn-inspector-suggest-themes");
        btnInspectorDescribe = document.getElementById("btn-inspector-describe");
        btnInspectorRunAll = document.getElementById("btn-inspector-run-all");
        inspectorAiSuggestionsBox = document.getElementById("inspector-ai-suggestions-box");
        inspectorAiSuggestionsPills = document.getElementById("inspector-ai-suggestions-pills");
        btnAcceptAllSuggestions = document.getElementById("btn-accept-all-suggestions");
        btnInspectorFullscreen = document.getElementById("btn-inspector-fullscreen");
        btnInspectorDiscard = document.getElementById("btn-inspector-discard");
        btnInspectorSave = document.getElementById("btn-inspector-save");

        // Delete Curated Modal & Bulk Action Bar Elements (Issue #137)
        deleteCuratedModal = document.getElementById("delete-curated-modal");
        deleteCuratedTitle = document.getElementById("delete-curated-title");
        deleteTargetPreview = document.getElementById("delete-target-preview");
        deleteReasonSelect = document.getElementById("delete-reason-select");
        deleteNotesInput = document.getElementById("delete-notes-input");
        deleteNotesGroup = document.getElementById("delete-notes-group");
        btnCancelDelete = document.getElementById("btn-cancel-delete");
        btnConfirmDelete = document.getElementById("btn-confirm-delete");
        btnConfirmDeleteText = document.getElementById("btn-confirm-delete-text");
        btnCloseDeleteModal = document.getElementById("btn-close-delete-modal");
        libraryBulkBar = document.getElementById("library-bulk-bar");
        bulkSelectedCount = document.getElementById("bulk-selected-count");
        btnBulkDelete = document.getElementById("btn-bulk-delete");
        btnBulkTag = document.getElementById("btn-bulk-tag");
        btnBulkRefine = document.getElementById("btn-bulk-refine");
        btnBulkRefineLabel = document.getElementById("btn-bulk-refine-label");
        btnBulkExport = document.getElementById("btn-bulk-export");
        btnBulkDeselect = document.getElementById("btn-bulk-deselect");
        btnInspectorDelete = document.getElementById("btn-inspector-delete");
        Object.assign(dom, {
        // Only elements read through the registry are published; later view tickets extend
        // this list when they start reading another element via dom.<key>.
        adaptersCountBadge, btnAcceptAllSuggestions, btnAddTag, btnAddTheme, btnAuditReadiness,
        btnAuditReadinessLabel, btnBulkDelete, btnBulkDeselect, btnBulkExport, btnBulkRefine,
        btnBulkRefineLabel, btnBulkTag, btnCancelDelete, btnCancelTrainingDrawer, btnCheckTrainingReadiness,
        btnClearLibraryFilters, btnCloseDeleteModal, btnCloseInspector, btnCloseTrainingDrawer,
        btnConfirmDelete, btnDecisionUndo, btnExportLabel, btnExportLora, btnFreezeLabel, btnFreezeSnapshot,
        btnFsClose, btnFsCloseGridPanel, btnFsFit, btnFsGridAdjust, btnFsGridAlignDrag, btnFsGridDone,
        btnFsGridNudgeDown, btnFsGridNudgeLeft, btnFsGridNudgeRight, btnFsGridNudgeUp, btnFsGridReset,
        btnFsGridSizeDec, btnFsGridSizeInc, btnFsGridToggle, btnFsNextMap, btnFsPrevMap, btnFsZoomIn,
        btnFsZoomOut, btnGridAdjust, btnGridAlignDrag, btnGridToggle, btnInspectorDelete,
        btnInspectorDescribe, btnInspectorDiscard, btnInspectorFullscreen, btnInspectorRunAll,
        btnInspectorSave, btnInspectorSuggestThemes, btnOpenTrainingDrawer, btnOpenTrainingFromLibrary,
        btnRefreshLibrary, btnRefreshSnapshots, btnRefreshTrainingAdapters, btnRefreshTrainingRuns,
        btnRegenerateDescription, btnSplitBundle, btnSplitBundleHud, btnSplitHudText, btnSplitLabel,
        btnStartRunText, btnStartTrainingRun, btnSwitchToTriage, btnToggleLaunchForm, btnToggleLaunchText,
        btnToggleSnapshots, btnUndoReject, bulkSelectedCount, canvas, ctx, curateDecisionModal,
        datasetReadinessIcon, datasetReadinessIssues, datasetReadinessPanel, datasetReadinessStatus,
        datasetReadinessSummary, decisionAcceptedTags, decisionAutocompleteList, decisionCardStep,
        decisionCardTag, decisionCustomStep, decisionDescriptionInput, decisionDescriptionLoading,
        decisionDescriptionStatus, decisionOfflineBadge, decisionOfflineNote, decisionProgressBadge,
        decisionTagInput, decisionValidationError, deleteCuratedModal, deleteCuratedTitle, deleteNotesGroup,
        deleteNotesInput, deleteReasonSelect, deleteTargetPreview, freezeDetailMap, freezeDetailSection,
        freezePhaseBadge, freezeProgressBar, freezeProgressCounts, freezeProgressMessage,
        freezeProgressPanel, freezeProgressPercent, fsGridLatticeCells, fsGridLatticeSections,
        fsGridOffsetXVal, fsGridOffsetYVal, fsGridSizeInput, fsGridSizeSlider, fsGridStatusBadge,
        fsLayerDeckCount, fsLayerFilmstripStrip, fsMapCounter, fsMapDimensions, fsMapIdBadge, fsZoomReadout,
        gridAdjustPanel, gridLatticeCells, gridLatticeSections, gridOffsetXVal, gridOffsetYVal,
        gridSizeInput, gridSizeSlider, inspectorAiSuggestionsBox, inspectorAiSuggestionsPills,
        inspectorDescCounter, inspectorDescriptionInput, inspectorLayerCountBadge, inspectorLayersList,
        inspectorMapIdBadge, inspectorTagChips, inspectorTagInput, inspectorThemeChips, inspectorThemeInput,
        inspectorUnsavedBadge, layerDeck, layerDeckBundleTag, layerDeckCount, layerFilmstrip,
        libraryBulkBar, libraryCountBadge, libraryEmpty, libraryFilterEmpty, libraryFullscreenCanvas,
        libraryFullscreenCtx, libraryFullscreenGridPanel, libraryFullscreenLayerDeck,
        libraryFullscreenOverlay, libraryGrid, libraryInspectorDrawer, librarySearchInput,
        librarySortSelect, libraryTagPills, libraryViewport, roleSelector, snapshotsCountBadge,
        snapshotsEmpty, snapshotsList, snapshotsShelf, tabNavLibrary, tabNavTraining, tabNavTriage,
        tabTrainingAdapters, tabTrainingRuns, toggleSnapshotsText, trainingAdaptersCountTab,
        trainingAdaptersEmpty, trainingAdaptersFilter, trainingAdaptersList, trainingDrawer,
        trainingLaunchError, trainingLaunchForm, trainingReadinessIndicator, trainingRunsActiveBadge,
        trainingRunsCountTab, trainingRunsEmpty, trainingRunsFilter, trainingRunsList,
        trainingRunsTotalBadge, trainingScopeHint, trainingSnapshotSelect, trainingViewAdapters,
        trainingViewRuns, triageViewport
        });
    }

    function toggleHelpModal() {
        stopPanLoop();
        stopZoomLoop();
        state.helpModalOpen = !state.helpModalOpen;
        if (state.helpModalOpen) {
            helpModal.classList.remove("hidden");
        } else {
            helpModal.classList.add("hidden");
            canvas.focus();
        }
    }

    export function closeHelpModal() {
        stopPanLoop();
        stopZoomLoop();
        state.helpModalOpen = false;
        helpModal.classList.add("hidden");
        canvas.focus();
    }

    // ==========================================================================
    // Ingestion Drawer Controls & Live Harvesting
    // ==========================================================================

    function toggleIngestionDrawer() {
        if (state.drawerOpen) {
            closeIngestionDrawer();
        } else {
            openIngestionDrawer();
        }
    }

    function openIngestionDrawer() {
        stopPanLoop();
        stopZoomLoop();
        if (state.helpModalOpen) closeHelpModal();
        state.drawerOpen = true;
        ingestionDrawer?.classList.remove("hidden");
        btnStartHarvest?.focus();
    }

    export function closeIngestionDrawer() {
        stopPanLoop();
        stopZoomLoop();
        state.drawerOpen = false;
        ingestionDrawer?.classList.add("hidden");
        canvas.focus();
    }

    async function startRedditHarvest() {
        if (state.isHarvesting) return;
        state.isHarvesting = true;
        btnStartHarvest.disabled = true;
        harvestProgressContainer.classList.remove("hidden");
        harvestProgressBar.style.width = "5%";
        harvestProgressText.textContent = "Connecting to Reddit harvester...";

        const payload = {
            sort: state.harvestSort,
            time: state.harvestTime,
            limit: parseInt(harvestLimitInput.value, 10),
            gridless_only: harvestGridlessOnly.checked,
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
                harvestProgressBar.style.width = "100%";
                harvestProgressText.textContent = msg;
                showToast(msg, "success");
                await fetchStagingQueue();
                fetchStatus();
            }
        } catch (err) {
            harvestProgressText.textContent = `Harvest failed: ${err.message}`;
            showToast(`Harvest error: ${err.message}`, "danger");
        } finally {
            state.isHarvesting = false;
            btnStartHarvest.disabled = false;
        }
    }

    function handleHarvestEvent(event, data) {
        if (event === "progress") {
            const pct = data.total > 0 ? Math.round((data.processed / data.total) * 100) : 10;
            harvestProgressBar.style.width = `${Math.max(5, Math.min(100, pct))}%`;
            harvestProgressText.textContent = data.message;
        } else if (event === "complete") {
            harvestProgressBar.style.width = "100%";
            harvestProgressText.textContent = data.message;
            showToast(data.message, "success");
            fetchStagingQueue();
            fetchStatus();
        } else if (event === "error") {
            harvestProgressText.textContent = `Error: ${data.error || "Unknown"}`;
            showToast(`Harvest failed: ${data.error || "Unknown"}`, "danger");
        }
    }


    // ==========================================================================
    // AI Benchmark Suite, Dynamic Calculator & Budget Guardrails
    // ==========================================================================

    export async function fetchCostStats() {
        try {
            const res = await fetch("/api/cost/stats");
            if (!res.ok) return;
            const stats = await res.json();
            const allTimeSpend = (stats.all_time_spend != null ? stats.all_time_spend : stats.total_spend_usd || 0).toFixed(2);
            const sessionSpend = (stats.session_spend != null ? stats.session_spend : stats.session_spend_usd || 0).toFixed(2);
            const trainingSpend = (stats.training_spend || 0).toFixed(2);
            const generationSpend = (stats.generation_spend || stats.total_spend_usd || 0).toFixed(2);
            const totalGens = stats.total_generations || 0;
            const mapWord = totalGens === 1 ? "map" : "maps";
            const trainingRunsCount = stats.training_runs || 0;
            const runWord = trainingRunsCount === 1 ? "run" : "runs";

            if (costMonitorText) {
                costMonitorText.textContent = `Session: $${sessionSpend} · Total: $${allTimeSpend} (Train: $${trainingSpend} · ${totalGens} ${mapWord})`;
            }
            if (costMonitorHud) {
                costMonitorHud.title = `Session Spend: $${sessionSpend} | Lifetime Spend: $${allTimeSpend} (Training: $${trainingSpend} across ${trainingRunsCount} ${runWord}, Gen: $${generationSpend} across ${totalGens} ${mapWord})`;
            }
        } catch (_) {
            // Silently ignore telemetry refresh failure
        }
    }

    function getSelectedBenchmarkModels() {
        const checkboxes = document.querySelectorAll('input[name="benchmark-model"]:checked');
        return Array.from(checkboxes).map((cb) => cb.value);
    }

    function getSelectedBenchmarkArchetypes() {
        const checkboxes = document.querySelectorAll('input[name="benchmark-archetype"]:checked');
        return Array.from(checkboxes).map((cb) => cb.value);
    }

    function benchmarkModelById(id) {
        return benchmarkModelCatalog.find((m) => m.id === id) || null;
    }

    // Badge styling is presentation only; identity and price always come from the registry entry.
    function benchmarkModelBadgeClass(model) {
        if (model.provider === "wavespeed") return "badge-wavespeed";
        if (model.id.includes("flash-lite")) return "badge-flash-lite";
        if (model.id.includes("muse")) return "badge-muse";
        if (model.id.includes("seedream")) return "badge-seedream";
        return "badge-flash";
    }

    // Fetches the derived registry; publishing or retiring an adapter shows up on the next open
    // with no restart (ADR-0010).
    export async function loadBenchmarkModelCatalog() {
        try {
            const res = await fetch("/api/models");
            if (!res.ok) throw new Error(`GET /api/models returned ${res.status}`);
            const models = await res.json();
            benchmarkModelCatalog = Array.isArray(models) ? models : [];
        } catch (_) {
            benchmarkModelCatalog = [];
        }
        renderBenchmarkModelChecklist();
    }

    function renderBenchmarkModelChecklist() {
        if (!benchmarkModelsList) return;
        benchmarkModelsList.innerHTML = "";

        if (benchmarkModelCatalog.length === 0) {
            const empty = document.createElement("div");
            empty.className = "check-item-desc mono";
            empty.textContent = "No selectable models reported by the registry.";
            benchmarkModelsList.appendChild(empty);
            return;
        }

        for (const model of benchmarkModelCatalog) {
            const label = document.createElement("label");
            label.className = "check-item-card";

            const checkbox = document.createElement("input");
            checkbox.type = "checkbox";
            checkbox.name = "benchmark-model";
            checkbox.value = model.id;
            // The default selection is the registry's OpenRouter baseline entries (ADR-0010).
            checkbox.checked = model.provider === "openrouter";

            const info = document.createElement("div");
            info.className = "check-item-info";

            const titleRow = document.createElement("div");
            titleRow.className = "check-item-title-row";

            const name = document.createElement("span");
            name.className = "check-item-name";
            name.textContent = model.display_name;

            const badge = document.createElement("span");
            badge.className = `pricing-badge ${benchmarkModelBadgeClass(model)}`;
            badge.textContent = `$${Number(model.estimated_cost_per_image).toFixed(3)}/image`;

            titleRow.appendChild(name);
            titleRow.appendChild(badge);

            const desc = document.createElement("span");
            desc.className = "check-item-desc mono";
            desc.textContent = model.id;

            info.appendChild(titleRow);
            info.appendChild(desc);
            label.appendChild(checkbox);
            label.appendChild(info);
            benchmarkModelsList.appendChild(label);
        }
    }

    function referenceConditioningRequested() {
        return benchmarkEnableReference ? benchmarkEnableReference.checked : true;
    }

    function calculatePreflightMetrics() {
        const models = getSelectedBenchmarkModels();
        const archetypes = getSelectedBenchmarkArchetypes();
        const hasCustom = benchmarkEnableCustom?.checked && (benchmarkCustomPrompt?.value.trim().length > 0 || benchmarkCustomName?.value.trim().length > 0);
        const promptCount = archetypes.length + (hasCustom ? 1 : 0);
        const modelCount = models.length;
        const totalImages = modelCount * promptCount;

        let costPerSet = 0;
        for (const id of models) {
            const model = benchmarkModelById(id);
            costPerSet += model ? Number(model.estimated_cost_per_image) || 0 : 0;
        }
        const estimatedCost = promptCount * costPerSet;

        // Reference conditioning is a capability gate, not a label (ADR-0010): a model whose
        // `supports_reference_image` is false cannot accept an image, so the combination is blocked.
        const referenceIncapableModels = referenceConditioningRequested()
            ? models.filter((id) => {
                const model = benchmarkModelById(id);
                return model ? !model.supports_reference_image : false;
            })
            : [];

        return {
            models,
            archetypes,
            hasCustom,
            modelCount,
            promptCount,
            totalImages,
            estimatedCost,
            referenceIncapableModels,
        };
    }

    function updatePreflightCost() {
        const metrics = calculatePreflightMetrics();
        const { modelCount, promptCount, totalImages, estimatedCost, referenceIncapableModels } = metrics;

        const costStr = `$${estimatedCost.toFixed(2)}`;
        if (calcBadgeCost) calcBadgeCost.textContent = costStr;

        if (calcFormulaDisplay) {
            calcFormulaDisplay.textContent = `${modelCount} model${modelCount === 1 ? "" : "s"} × ${promptCount} prompt${promptCount === 1 ? "" : "s"} = ${totalImages} image${totalImages === 1 ? "" : "s"} (~${costStr})`;
        }

        const exceedsGuardrail = estimatedCost > BUDGET_GUARDRAIL_THRESHOLD;
        if (calcWarningNote) {
            calcWarningNote.classList.toggle("hidden", !exceedsGuardrail);
        }

        const referenceBlocked = referenceIncapableModels.length > 0;
        if (benchmarkReferenceWarning) {
            if (referenceBlocked) {
                benchmarkReferenceWarning.textContent = `⚠️ Reference conditioning is unavailable for: ${referenceIncapableModels.join(", ")}. Turn it off or deselect those models.`;
            }
            benchmarkReferenceWarning.classList.toggle("hidden", !referenceBlocked);
        }

        if (btnStartBenchmarkText) {
            btnStartBenchmarkText.textContent = `Run Benchmark (${costStr})`;
        }

        if (btnStartBenchmark) {
            btnStartBenchmark.disabled = (modelCount === 0 || promptCount === 0 || referenceBlocked || state.isBenchmarking);
        }
    }

    function toggleBenchmarkDrawer() {
        if (state.benchmarkDrawerOpen) {
            closeBenchmarkDrawer();
        } else {
            openBenchmarkDrawer();
        }
    }

    async function openBenchmarkDrawer() {
        stopPanLoop();
        stopZoomLoop();
        if (state.helpModalOpen) closeHelpModal();
        if (state.drawerOpen) closeIngestionDrawer();
        state.benchmarkDrawerOpen = true;
        benchmarkDrawer?.classList.remove("hidden");
        await loadBenchmarkModelCatalog();
        updatePreflightCost();
    }

    export function closeBenchmarkDrawer() {
        stopPanLoop();
        stopZoomLoop();
        state.benchmarkDrawerOpen = false;
        benchmarkDrawer?.classList.add("hidden");
        canvas.focus();
    }

    function openBudgetGuardModal(estimatedCost, modelCount, promptCount, totalImages) {
        stopPanLoop();
        stopZoomLoop();
        state.budgetGuardModalOpen = true;
        const costStr = `$${estimatedCost.toFixed(2)}`;
        if (budgetGuardCostAmount) budgetGuardCostAmount.textContent = costStr;
        if (budgetGuardBreakdown) {
            budgetGuardBreakdown.textContent = `${modelCount} models × ${promptCount} prompts = ${totalImages} images`;
        }
        if (btnConfirmBudgetGuardText) {
            btnConfirmBudgetGuardText.textContent = `Confirm & Run (${costStr})`;
        }
        budgetGuardModal?.classList.remove("hidden");
    }

    function closeBudgetGuardModal() {
        stopPanLoop();
        stopZoomLoop();
        state.budgetGuardModalOpen = false;
        budgetGuardModal?.classList.add("hidden");
    }

    function handleStartBenchmarkClick() {
        const metrics = calculatePreflightMetrics();
        if (metrics.modelCount === 0) {
            showToast("Please select at least 1 model", "danger");
            return;
        }
        if (metrics.promptCount === 0) {
            showToast("Please select at least 1 benchmark archetype or enter a custom prompt", "danger");
            return;
        }
        if (metrics.referenceIncapableModels.length > 0) {
            showToast(
                `Reference conditioning is unavailable for: ${metrics.referenceIncapableModels.join(", ")}`,
                "danger",
            );
            return;
        }

        if (metrics.estimatedCost > BUDGET_GUARDRAIL_THRESHOLD) {
            openBudgetGuardModal(metrics.estimatedCost, metrics.modelCount, metrics.promptCount, metrics.totalImages);
        } else {
            startBenchmarkExecution(metrics);
        }
    }

    // ==========================================================================
    // Floating Benchmark Progress Window & Live Logs
    // ==========================================================================

    function openBenchmarkProgressWindow(metrics) {
        stopPanLoop();
        stopZoomLoop();
        state.benchmarkWindowMinimized = false;
        benchmarkWindowPill?.classList.add("hidden");
        benchmarkProgressWindow?.classList.remove("hidden");

        const total = metrics?.totalImages || 0;
        if (benchmarkWindowStatus) {
            benchmarkWindowStatus.textContent = "Running";
            benchmarkWindowStatus.className = "benchmark-window-badge status-running";
        }
        if (benchmarkWindowCurrentStep) {
            benchmarkWindowCurrentStep.textContent = "Initializing batch...";
        }
        if (benchmarkWindowProgressStats) {
            benchmarkWindowProgressStats.textContent = `0/${total} (0%)`;
        }
        if (benchmarkWindowProgressBar) {
            benchmarkWindowProgressBar.style.width = "0%";
        }
        if (btnBenchmarkWindowClose) {
            btnBenchmarkWindowClose.disabled = true;
        }
        if (btnBenchmarkWindowCancel) {
            btnBenchmarkWindowCancel.disabled = false;
        }
        if (benchmarkWindowCost) {
            benchmarkWindowCost.textContent = "$0.0000";
        }
        if (benchmarkPillCount) {
            benchmarkPillCount.textContent = `0/${total}`;
        }
        if (benchmarkPillCost) {
            benchmarkPillCost.textContent = "$0.00";
        }
        if (benchmarkWindowPreviewCount) {
            benchmarkWindowPreviewCount.textContent = "0 generated";
        }

        // Reset previews strip
        if (benchmarkWindowPreviews) {
            benchmarkWindowPreviews.innerHTML = `<div class="benchmark-previews-empty" id="benchmark-previews-empty">Waiting for first image completion...</div>`;
        }

        // Reset execution logs
        if (benchmarkWindowLogs) {
            benchmarkWindowLogs.innerHTML = "";
        }
        appendBenchmarkLog("INFO", `Initializing benchmark run: ${metrics?.modelCount || 0} model(s) × ${metrics?.promptCount || 0} prompt(s) = ${total} images.`);
    }

    function closeBenchmarkProgressWindow() {
        benchmarkProgressWindow?.classList.add("hidden");
        benchmarkWindowPill?.classList.add("hidden");
    }

    function minimizeBenchmarkWindow() {
        state.benchmarkWindowMinimized = true;
        benchmarkProgressWindow?.classList.add("hidden");
        benchmarkWindowPill?.classList.remove("hidden");
    }

    function expandBenchmarkWindow() {
        state.benchmarkWindowMinimized = false;
        benchmarkWindowPill?.classList.add("hidden");
        benchmarkProgressWindow?.classList.remove("hidden");
    }

    function toggleBenchmarkWindowMinimize() {
        if (state.benchmarkWindowMinimized) {
            expandBenchmarkWindow();
        } else {
            minimizeBenchmarkWindow();
        }
    }

    function appendBenchmarkLog(type, message) {
        if (!benchmarkWindowLogs) return;
        const now = new Date();
        const timeStr = now.toTimeString().split(" ")[0]; // "HH:MM:SS"
        const row = document.createElement("div");
        row.className = `log-entry log-${type.toLowerCase()}`;

        const timeSpan = document.createElement("span");
        timeSpan.className = "log-time";
        timeSpan.textContent = `[${timeStr}]`;

        const tagSpan = document.createElement("span");
        tagSpan.className = `log-tag tag-${type.toLowerCase()}`;
        tagSpan.textContent = `[${type}]`;

        const msgSpan = document.createElement("span");
        msgSpan.className = "log-message";
        msgSpan.textContent = message;

        row.appendChild(timeSpan);
        row.appendChild(tagSpan);
        row.appendChild(msgSpan);

        benchmarkWindowLogs.appendChild(row);
        benchmarkWindowLogs.scrollTop = benchmarkWindowLogs.scrollHeight;
    }

    function addBenchmarkThumbnailPreview(metadata) {
        if (!benchmarkWindowPreviews || !metadata || !metadata.id) return;

        const emptyPlaceholder = document.getElementById("benchmark-previews-empty");
        emptyPlaceholder?.remove();

        const existing = document.getElementById(`preview-card-${metadata.id}`);
        if (existing) return;

        const shortModel = (metadata.model || "").split("/").pop() || metadata.model || "Model";

        const card = document.createElement("div");
        card.id = `preview-card-${metadata.id}`;
        card.className = "benchmark-preview-card preview-card-completed";
        card.title = `Click to load in staging canvas\n${metadata.prompt_name} (${metadata.model})\nLatency: ${(metadata.latency_ms / 1000).toFixed(1)}s | Cost: $${metadata.cost_usd.toFixed(4)}`;

        const img = document.createElement("img");
        img.className = "preview-thumbnail-img";
        img.src = `/api/image/staging/${encodeURIComponent(metadata.id)}`;
        img.alt = metadata.prompt_name || "Benchmark Candidate";
        img.loading = "lazy";

        const overlay = document.createElement("div");
        overlay.className = "preview-card-overlay";

        const modelBadge = document.createElement("span");
        modelBadge.className = "preview-model-badge";
        modelBadge.textContent = shortModel;
        modelBadge.title = metadata.model || "";

        const promptSpan = document.createElement("span");
        promptSpan.className = "preview-prompt-name";
        promptSpan.textContent = metadata.prompt_name || "Scenario";
        promptSpan.title = metadata.prompt_name || "";

        const statsRow = document.createElement("div");
        statsRow.className = "preview-card-stats";

        const latencySpan = document.createElement("span");
        latencySpan.className = "preview-stat-latency mono";
        latencySpan.textContent = `${(metadata.latency_ms / 1000).toFixed(1)}s`;

        const costSpan = document.createElement("span");
        costSpan.className = "preview-stat-cost mono";
        costSpan.textContent = `$${metadata.cost_usd.toFixed(4)}`;

        statsRow.appendChild(latencySpan);
        statsRow.appendChild(costSpan);

        overlay.appendChild(modelBadge);
        overlay.appendChild(promptSpan);
        overlay.appendChild(statsRow);

        card.appendChild(img);
        card.appendChild(overlay);

        card.addEventListener("click", () => {
            focusCandidateById(metadata.id);
        });

        // Insert before active generation indicator card if present
        const activeCard = document.getElementById("benchmark-active-generation-card");
        if (activeCard && activeCard.parentNode === benchmarkWindowPreviews) {
            benchmarkWindowPreviews.insertBefore(card, activeCard);
        } else {
            benchmarkWindowPreviews.appendChild(card);
        }

        // Auto-scroll strip to newest thumbnail
        benchmarkWindowPreviews.scrollLeft = benchmarkWindowPreviews.scrollWidth;
    }

    async function focusCandidateById(id) {
        if (!id) return;
        let idx = state.candidates.findIndex((c) => String(c.id) === String(id));
        if (idx !== -1) {
            loadCandidate(idx);
            showToast(`Loaded candidate #${id} into canvas`, "info");
            return;
        }

        await fetchStagingQueue();
        idx = state.candidates.findIndex((c) => String(c.id) === String(id));
        if (idx !== -1) {
            loadCandidate(idx);
            showToast(`Loaded candidate #${id} into canvas`, "info");
        } else {
            const imgUrl = `/api/image/staging/${encodeURIComponent(id)}`;
            const img = new Image();
            img.src = imgUrl;
            img.onload = () => {
                state.imageCache.set(id, img);
                state.currentImage = img;
                showLoading(false);
                fitToScreen();
                showEmptyState(false);
                updateHud();
                showToast(`Displaying candidate #${id}`, "info");
            };
            img.onerror = () => {
                showToast(`Failed to load image for candidate #${id}`, "danger");
            };
        }
    }

    async function startBenchmarkExecution(metrics) {
        closeBudgetGuardModal();
        if (state.isBenchmarking) return;
        state.isBenchmarking = true;

        // Automatically close the benchmark configuration drawer so user is free to curate
        closeBenchmarkDrawer();
        openBenchmarkProgressWindow(metrics);

        if (btnStartBenchmark) btnStartBenchmark.disabled = true;
        benchmarkProgressContainer?.classList.remove("hidden");
        if (benchmarkProgressBar) benchmarkProgressBar.style.width = "5%";
        if (benchmarkProgressText) benchmarkProgressText.textContent = "Connecting to benchmark runner...";

        state.benchmarkAbortController = new AbortController();

        const customPrompts = [];
        if (metrics.hasCustom) {
            customPrompts.push({
                name: benchmarkCustomName?.value.trim() || "Custom Scenario",
                prompt: benchmarkCustomPrompt?.value.trim() || "",
            });
        }

        const payload = {
            models: metrics.models,
            archetypes: metrics.archetypes,
            custom_prompts: customPrompts,
            use_reference_conditioning: benchmarkEnableReference ? benchmarkEnableReference.checked : true,
        };
        if (benchmarkEnableCustomLora?.checked && benchmarkCustomLoraPath?.value.trim()) {
            payload.lora_path = benchmarkCustomLoraPath.value.trim();
            const scale = parseFloat(benchmarkCustomLoraScale?.value);
            if (!isNaN(scale)) {
                payload.lora_scale = scale;
            }
        }

        try {
            const res = await fetch("/api/benchmark/run", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    "Accept": "text/event-stream",
                },
                body: JSON.stringify(payload),
                signal: state.benchmarkAbortController.signal,
            });

            if (!res.ok) {
                const errJson = await res.json().catch(() => ({}));
                throw new Error(errJson.error || errJson.message || `HTTP ${res.status}`);
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
                    buffer = lines.pop();

                    let currentEvent = "message";
                    for (const line of lines) {
                        const trimmed = line.trim();
                        if (trimmed.startsWith("event:")) {
                            currentEvent = trimmed.slice(6).trim();
                        } else if (trimmed.startsWith("data:")) {
                            const dataStr = trimmed.slice(5).trim();
                            try {
                                const data = JSON.parse(dataStr);
                                handleBenchmarkEvent(currentEvent, data);
                            } catch (_) {}
                        }
                    }
                }
            } else {
                const data = await res.json();
                const msg = `Benchmark complete: ${data.completed}/${data.total} generated ($${(data.total_cost_usd || 0).toFixed(4)})`;
                if (benchmarkProgressBar) benchmarkProgressBar.style.width = "100%";
                if (benchmarkProgressText) benchmarkProgressText.textContent = msg;

                if (benchmarkWindowProgressBar) benchmarkWindowProgressBar.style.width = "100%";
                if (benchmarkWindowProgressStats) benchmarkWindowProgressStats.textContent = `${data.completed}/${data.total} (100%)`;
                if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = msg;
                if (benchmarkWindowStatus) {
                    benchmarkWindowStatus.textContent = "Complete";
                    benchmarkWindowStatus.className = "benchmark-window-badge status-complete";
                }
                if (benchmarkWindowCost) benchmarkWindowCost.textContent = `$${(data.total_cost_usd || 0).toFixed(4)}`;
                if (benchmarkPillCount) benchmarkPillCount.textContent = `${data.completed}/${data.total}`;
                if (benchmarkPillCost) benchmarkPillCost.textContent = `$${(data.total_cost_usd || 0).toFixed(2)}`;
                if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
                if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;

                appendBenchmarkLog("SUCCESS", `Benchmark batch completed! (${data.completed}/${data.total} images)`);
                showToast(msg, "success");
                await fetchStagingQueue();
                fetchStatus();
                fetchCostStats();
            }
        } catch (err) {
            document.getElementById("benchmark-active-generation-card")?.remove();
            if (err.name === "AbortError") {
                if (benchmarkProgressText) benchmarkProgressText.textContent = "Benchmark run cancelled by user.";
                if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = "Benchmark batch cancelled by user.";
                if (benchmarkWindowStatus) {
                    benchmarkWindowStatus.textContent = "Cancelled";
                    benchmarkWindowStatus.className = "benchmark-window-badge status-cancelled";
                }
                appendBenchmarkLog("CANCEL", "Benchmark run cancelled by user.");
                if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
                if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;
                showToast("Benchmark execution cancelled", "info");
            } else {
                if (benchmarkProgressText) benchmarkProgressText.textContent = `Benchmark failed: ${err.message}`;
                if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = `Failed: ${err.message}`;
                if (benchmarkWindowStatus) {
                    benchmarkWindowStatus.textContent = "Error";
                    benchmarkWindowStatus.className = "benchmark-window-badge status-error";
                }
                appendBenchmarkLog("ERROR", `Benchmark failed: ${err.message}`);
                if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
                if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;
                showToast(`Benchmark error: ${err.message}`, "danger");
            }
        } finally {
            state.isBenchmarking = false;
            state.benchmarkAbortController = null;
            document.getElementById("benchmark-active-generation-card")?.remove();
            if (btnStartBenchmark) btnStartBenchmark.disabled = false;
            fetchCostStats();
        }
    }

    function handleBenchmarkEvent(event, data) {
        if (!data) return;
        const phase = data.phase || (event === "complete" ? "complete" : event === "cancelled" ? "cancelled" : event === "error" ? "error" : "progress");

        if (phase === "starting") {
            const total = data.total || 0;
            if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = data.message || "Starting benchmark batch...";
            if (benchmarkWindowProgressStats) benchmarkWindowProgressStats.textContent = `0/${total} (0%)`;
            if (benchmarkPillCount) benchmarkPillCount.textContent = `0/${total}`;
            if (benchmarkProgressText) benchmarkProgressText.textContent = data.message || "Starting benchmark batch...";
            appendBenchmarkLog("INFO", data.message || `Starting benchmark batch (${total} tasks queued)`);
        } else if (phase === "generating") {
            const promptName = data.prompt || "Scenario";
            const modelName = data.model || "Model";
            const shortModel = modelName.split("/").pop() || modelName;

            if (benchmarkWindowCurrentStep) {
                benchmarkWindowCurrentStep.textContent = data.message || `Generating ${promptName} with ${modelName}...`;
            }
            if (benchmarkProgressText) {
                benchmarkProgressText.textContent = data.message || `Generating ${promptName}...`;
            }

            // Remove empty placeholder in previews
            document.getElementById("benchmark-previews-empty")?.remove();

            // Display or update active generation indicator card
            let activeCard = document.getElementById("benchmark-active-generation-card");
            if (!activeCard) {
                activeCard = document.createElement("div");
                activeCard.id = "benchmark-active-generation-card";
                activeCard.className = "benchmark-preview-card preview-card-generating";
                benchmarkWindowPreviews?.appendChild(activeCard);
            }
            activeCard.innerHTML = `
                <div class="preview-spinner-pulse"></div>
                <div class="preview-card-info">
                    <span class="preview-badge-generating">GENERATING</span>
                    <span class="preview-model-badge" title="${modelName}">${shortModel}</span>
                    <span class="preview-prompt-name" title="${promptName}">${promptName}</span>
                </div>
            `;

            appendBenchmarkLog("GEN", data.message || `Generating ${promptName} with ${modelName}...`);
        } else if (phase === "progress") {
            // Remove active generation card
            document.getElementById("benchmark-active-generation-card")?.remove();

            const completed = data.completed || 0;
            const total = data.total || 1;
            const pct = Math.round((completed / total) * 100);
            const spend = data.current_spend || 0;

            if (benchmarkWindowProgressBar) {
                benchmarkWindowProgressBar.style.width = `${Math.min(100, Math.max(5, pct))}%`;
            }
            if (benchmarkProgressBar) {
                benchmarkProgressBar.style.width = `${Math.min(100, Math.max(5, pct))}%`;
            }
            if (benchmarkWindowProgressStats) {
                benchmarkWindowProgressStats.textContent = `${completed}/${total} (${pct}%)`;
            }
            if (benchmarkWindowCurrentStep) {
                benchmarkWindowCurrentStep.textContent = data.message || `Completed ${completed}/${total}`;
            }
            if (benchmarkProgressText) {
                const spendStr = spend ? ` ($${spend.toFixed(4)})` : "";
                benchmarkProgressText.textContent = `${data.message || `Generated ${completed}/${total}`}${spendStr}`;
            }
            if (benchmarkPillCount) {
                benchmarkPillCount.textContent = `${completed}/${total}`;
            }
            if (benchmarkPillCost) {
                benchmarkPillCost.textContent = `$${spend.toFixed(2)}`;
            }
            if (benchmarkWindowCost) {
                benchmarkWindowCost.textContent = `$${spend.toFixed(4)}`;
            }

            if (data.metadata) {
                addBenchmarkThumbnailPreview(data.metadata);
                const latencySec = (data.metadata.latency_ms / 1000).toFixed(1);
                const costStr = data.metadata.cost_usd.toFixed(4);
                appendBenchmarkLog("DONE", `Completed ${data.metadata.prompt_name} (${data.metadata.model}) in ${latencySec}s ($${costStr})`);
                if (benchmarkWindowPreviewCount) {
                    benchmarkWindowPreviewCount.textContent = `${completed} generated`;
                }
            }

            fetchCostStats();
        } else if (phase === "complete") {
            document.getElementById("benchmark-active-generation-card")?.remove();

            const total = data.total || data.completed || 0;
            const completed = data.completed || total;
            const spend = data.current_spend || data.total_cost_usd || 0;

            if (benchmarkWindowProgressBar) benchmarkWindowProgressBar.style.width = "100%";
            if (benchmarkProgressBar) benchmarkProgressBar.style.width = "100%";
            if (benchmarkWindowProgressStats) benchmarkWindowProgressStats.textContent = `${completed}/${total} (100%)`;
            if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = data.message || "Benchmark batch completed!";
            if (benchmarkProgressText) benchmarkProgressText.textContent = data.message || "Benchmark complete!";

            if (benchmarkWindowStatus) {
                benchmarkWindowStatus.textContent = "Complete";
                benchmarkWindowStatus.className = "benchmark-window-badge status-complete";
            }
            if (benchmarkWindowCost) {
                benchmarkWindowCost.textContent = `$${spend.toFixed(4)}`;
            }
            if (benchmarkPillCount) {
                benchmarkPillCount.textContent = `${completed}/${total}`;
            }
            if (benchmarkPillCost) {
                benchmarkPillCost.textContent = `$${spend.toFixed(2)}`;
            }

            if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
            if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;

            appendBenchmarkLog("SUCCESS", `Benchmark batch completed! (${completed}/${total} images, $${spend.toFixed(4)})`);
            showToast(data.message || "Benchmark batch completed!", "success");

            fetchStagingQueue();
            fetchStatus();
            fetchCostStats();
        } else if (phase === "cancelled") {
            document.getElementById("benchmark-active-generation-card")?.remove();

            if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = data.message || "Benchmark run cancelled.";
            if (benchmarkProgressText) benchmarkProgressText.textContent = data.message || "Cancelled";
            if (benchmarkWindowStatus) {
                benchmarkWindowStatus.textContent = "Cancelled";
                benchmarkWindowStatus.className = "benchmark-window-badge status-cancelled";
            }
            if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
            if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;

            appendBenchmarkLog("CANCEL", data.message || "Benchmark batch execution cancelled.");
            showToast(data.message || "Benchmark cancelled", "info");

            fetchStagingQueue();
            fetchStatus();
            fetchCostStats();
        } else if (phase === "error") {
            document.getElementById("benchmark-active-generation-card")?.remove();

            const errMsg = data.message || data.error || "Unknown error";
            if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = `Error: ${errMsg}`;
            if (benchmarkProgressText) benchmarkProgressText.textContent = `Error: ${errMsg}`;
            if (benchmarkWindowStatus) {
                benchmarkWindowStatus.textContent = "Error";
                benchmarkWindowStatus.className = "benchmark-window-badge status-error";
            }
            if (btnBenchmarkWindowClose) btnBenchmarkWindowClose.disabled = false;
            if (btnBenchmarkWindowCancel) btnBenchmarkWindowCancel.disabled = true;

            appendBenchmarkLog("ERROR", `Benchmark error: ${errMsg}`);
            showToast(`Benchmark error: ${errMsg}`, "danger");

            fetchCostStats();
        }
    }

    async function cancelBenchmarkExecution() {
        document.getElementById("benchmark-active-generation-card")?.remove();
        if (state.benchmarkAbortController) {
            state.benchmarkAbortController.abort();
        }
        try {
            await fetch("/api/benchmark/cancel", { method: "POST" });
        } catch (_) {}
        if (benchmarkProgressText) benchmarkProgressText.textContent = "Cancelling benchmark batch...";
        if (benchmarkWindowCurrentStep) benchmarkWindowCurrentStep.textContent = "Cancelling benchmark batch...";
        appendBenchmarkLog("CANCEL", "Cancellation requested by user.");
        showToast("Benchmark batch cancellation requested", "info");
    }
    function setupDrawerControls() {
        btnOpenDrawer?.addEventListener("click", () => openIngestionDrawer());
        btnEmptyHarvest?.addEventListener("click", () => openIngestionDrawer());
        btnCloseDrawer?.addEventListener("click", () => closeIngestionDrawer());
        btnCancelDrawer?.addEventListener("click", () => closeIngestionDrawer());

        ingestionDrawer?.addEventListener("click", (e) => {
            if (e.target === ingestionDrawer) {
                closeIngestionDrawer();
            }
        });

        harvestLimitInput?.addEventListener("input", () => {
            harvestLimitValue.textContent = harvestLimitInput.value;
        });

        const sortBtns = harvestSortGroup?.querySelectorAll(".btn-toggle");
        sortBtns?.forEach((btn) => {
            btn.addEventListener("click", () => {
                const sort = btn.getAttribute("data-sort");
                if (sort) {
                    state.harvestSort = sort;
                    sortBtns.forEach((b) => b.classList.toggle("active", b === btn));
                }
            });
        });

        const timeBtns = harvestTimeGroup?.querySelectorAll(".btn-toggle");
        timeBtns?.forEach((btn) => {
            btn.addEventListener("click", () => {
                const time = btn.getAttribute("data-time");
                if (time) {
                    state.harvestTime = time;
                    timeBtns.forEach((b) => b.classList.toggle("active", b === btn));
                }
            });
        });

        btnStartHarvest?.addEventListener("click", () => startRedditHarvest());

        // Benchmark Drawer Controls
        btnOpenBenchmarkDrawer?.addEventListener("click", () => openBenchmarkDrawer());
        btnCloseBenchmarkDrawer?.addEventListener("click", () => closeBenchmarkDrawer());
        btnCancelBenchmarkDrawer?.addEventListener("click", () => closeBenchmarkDrawer());

        benchmarkDrawer?.addEventListener("click", (e) => {
            if (e.target === benchmarkDrawer) closeBenchmarkDrawer();
        });

        btnModelsSelectAll?.addEventListener("click", () => {
            document.querySelectorAll('input[name="benchmark-model"]').forEach((cb) => (cb.checked = true));
            updatePreflightCost();
        });
        btnModelsSelectNone?.addEventListener("click", () => {
            document.querySelectorAll('input[name="benchmark-model"]').forEach((cb) => (cb.checked = false));
            updatePreflightCost();
        });
        btnPromptsSelectAll?.addEventListener("click", () => {
            document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => (cb.checked = true));
            updatePreflightCost();
        });
        btnPromptsSelectNone?.addEventListener("click", () => {
            document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => (cb.checked = false));
            updatePreflightCost();
        });

        // The model checklist is rendered from the registry after each fetch, so its change events
        // are handled by delegation on the container instead of per-checkbox listeners.
        benchmarkModelsList?.addEventListener("change", (e) => {
            if (e.target && e.target.name === "benchmark-model") updatePreflightCost();
        });
        benchmarkEnableReference?.addEventListener("change", () => updatePreflightCost());
        document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => {
            cb.addEventListener("change", () => updatePreflightCost());
        });

        benchmarkEnableCustom?.addEventListener("change", () => {
            customPromptFields?.classList.toggle("hidden", !benchmarkEnableCustom.checked);
            updatePreflightCost();
        });
        benchmarkCustomName?.addEventListener("input", () => updatePreflightCost());
        benchmarkCustomPrompt?.addEventListener("input", () => updatePreflightCost());

        benchmarkEnableCustomLora?.addEventListener("change", () => {
            benchmarkCustomLoraFields?.classList.toggle("hidden", !benchmarkEnableCustomLora.checked);
        });
        btnStartBenchmark?.addEventListener("click", () => handleStartBenchmarkClick());
        btnCancelBenchmarkRun?.addEventListener("click", () => cancelBenchmarkExecution());

        // Floating Benchmark Progress Window & Pill Controls
        btnBenchmarkWindowMinimize?.addEventListener("click", () => minimizeBenchmarkWindow());
        btnBenchmarkWindowClose?.addEventListener("click", () => closeBenchmarkProgressWindow());
        btnBenchmarkWindowCancel?.addEventListener("click", () => cancelBenchmarkExecution());
        benchmarkWindowPill?.addEventListener("click", () => expandBenchmarkWindow());
        btnBenchmarkClearLogs?.addEventListener("click", () => {
            if (benchmarkWindowLogs) benchmarkWindowLogs.innerHTML = "";
        });

        btnCloseBudgetGuard?.addEventListener("click", () => closeBudgetGuardModal());
        btnDismissBudgetGuard?.addEventListener("click", () => closeBudgetGuardModal());
        budgetGuardModal?.addEventListener("click", (e) => {
            if (e.target === budgetGuardModal) closeBudgetGuardModal();
        });
        btnConfirmBudgetGuard?.addEventListener("click", () => {
            const metrics = calculatePreflightMetrics();
            startBenchmarkExecution(metrics);
        });
    }

    // ==========================================================================
    // Storage Footprint & Quota Monitor (Issue #30)
    // ==========================================================================

    async function fetchStorageStats() {
        try {
            const res = await fetch("/api/storage/stats");
            if (!res.ok) return;
            const data = await res.json();
            state.storageStats = data;
            renderStorageStats(data);
        } catch (_) {
            // Silently ignore background stats fetch failures
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
        stopPanLoop();
        stopZoomLoop();
        state.storageModalOpen = true;
        const modal = document.getElementById("storage-modal");
        modal?.classList.remove("hidden");
        fetchStorageStats();
    }

    function closeStorageModal() {
        stopPanLoop();
        stopZoomLoop();
        state.storageModalOpen = false;
        const modal = document.getElementById("storage-modal");
        modal?.classList.add("hidden");
    }

    function openPurgeModal() {
        stopPanLoop();
        stopZoomLoop();
        state.purgeModalOpen = true;
        const modal = document.getElementById("purge-confirm-modal");
        modal?.classList.remove("hidden");
        const reclaimBytesEl = document.getElementById("purge-reclaim-bytes");
        const reclaimFilesEl = document.getElementById("purge-reclaim-files");
        const currentStaging = state.storageStats?.staging || { bytes: 0, file_count: 0 };
        if (reclaimBytesEl) reclaimBytesEl.textContent = formatBytes(currentStaging.bytes);
        if (reclaimFilesEl) reclaimFilesEl.textContent = `${currentStaging.file_count.toLocaleString()} candidate files will be permanently deleted`;
    }

    function closePurgeModal() {
        stopPanLoop();
        stopZoomLoop();
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
            const res = await fetch("/api/storage/purge-staging", { method: "POST" });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({ error: "Purge failed" }));
                throw new Error(errData.error || "Purge failed");
            }
            const data = await res.json();
            closePurgeModal();
            showToast(`Staging purged: ${data.purged_files} files deleted, ${formatBytes(data.reclaimed_bytes)} reclaimed`, "success");

            // Update UI candidates, status indicator, and storage stats
            await fetchStagingQueue();
            fetchStatus();
            await fetchStorageStats();
        } catch (err) {
            showToast(`Failed to purge staging: ${err.message}`, "danger");
        } finally {
            state.isPurging = false;
            if (confirmBtn) confirmBtn.disabled = false;
            if (confirmText) confirmText.textContent = "Confirm & Purge Staging";
        }
    }

    function setupStorageControls() {
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

    // ==========================================================================
    // UI Event Listeners & Hotkeys
    // ==========================================================================

    function setupDecisionModalControls() {
        if (btnCloseDecisionModal) {
            btnCloseDecisionModal.addEventListener("click", () => closeCurateDecisionModal());
        }
        if (btnCancelDecision) {
            btnCancelDecision.addEventListener("click", () => closeCurateDecisionModal());
        }
        if (btnDecisionReject) {
            btnDecisionReject.addEventListener("click", () => handleDecisionReject());
        }
        if (btnDecisionAccept) {
            btnDecisionAccept.addEventListener("click", () => handleDecisionAccept());
        }
        if (btnDecisionUndo) {
            btnDecisionUndo.addEventListener("click", () => handleDecisionUndo());
        }
        if (btnAddCustomTag) {
            btnAddCustomTag.addEventListener("click", () => handleCustomTagAdd());
        }
        if (btnFinishDecision) {
            btnFinishDecision.addEventListener("click", () => handleCustomTagSubmit());
        }
        if (decisionTagInput) {
            decisionTagInput.addEventListener("input", (e) => onDecisionTagInput(e));
            decisionTagInput.addEventListener("keydown", (e) => onDecisionTagInputKeydown(e));
        }
        if (btnRegenerateDescription) {
            btnRegenerateDescription.addEventListener("click", () => {
                if (decisionState.candidate) {
                    fetchCandidateDescription(decisionState.candidate, true);
                }
            });
        }
        if (decisionDescriptionInput) {
            decisionDescriptionInput.addEventListener("keydown", (e) => {
                if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    handleCustomTagSubmit();
                }
            });
        }
        if (curateDecisionModal) {
            curateDecisionModal.addEventListener("click", (e) => {
                if (e.target === curateDecisionModal) {
                    closeCurateDecisionModal();
                }
            });
        }
    }

    function setupEventListeners() {
        // --- Canvas Mouse Pan Dragging & Dynamic Grid Align Drag ---
        canvas.addEventListener("mousedown", (e) => {
            if ((e.button === 0) && (e.shiftKey || state.gridAlignMode)) {
                state.isDraggingGrid = true;
                state.gridDragStart = { x: e.clientX, y: e.clientY };
                state.gridOffsetStart = { x: state.gridOffset.x, y: state.gridOffset.y };
                canvas.classList.add("grid-dragging");
                e.preventDefault();
                return;
            }

            // Left click (0) or Middle click (1)
            if (e.button === 0 || e.button === 1) {
                state.isPanning = true;
                state.dragStart = { x: e.clientX, y: e.clientY };
                state.panStart = { x: state.pan.x, y: state.pan.y };
                canvas.classList.add("panning");
                e.preventDefault();
            }
        });

        window.addEventListener("mousemove", (e) => {
            if (canvas && !state.isDraggingGrid) {
                canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
            }
            if (libraryFullscreenCanvas?.parentElement && !state.libraryFullscreenIsDraggingGrid) {
                libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", e.shiftKey || state.libraryFullscreenGridAlignMode);
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
                const imgDx = (e.clientX - state.gridDragStart.x) / state.zoom;
                const imgDy = (e.clientY - state.gridDragStart.y) / state.zoom;
                state.gridOffset.x = Math.round(state.gridOffsetStart.x + imgDx);
                state.gridOffset.y = Math.round(state.gridOffsetStart.y + imgDy);
                saveCurrentCandidateGrid();
                requestRender();
                updateHud();
                updateGridPanel();
                return;
            }

            if (!state.isPanning) return;
            const dx = e.clientX - state.dragStart.x;
            const dy = e.clientY - state.dragStart.y;
            state.pan.x = state.panStart.x + dx;
            state.pan.y = state.panStart.y + dy;
            requestRender();
            updateHud();
        });

        window.addEventListener("mouseup", (e) => {
            if (state.libraryFullscreenIsDraggingGrid) {
                state.libraryFullscreenIsDraggingGrid = false;
                saveFullscreenMapGrid();
                libraryFullscreenCanvas?.parentElement?.classList.remove("grid-dragging");
            }
            if (state.libraryFullscreenIsPanning) {
                state.libraryFullscreenIsPanning = false;
                libraryFullscreenCanvas?.parentElement?.classList.remove("panning");
            }
            if (state.isDraggingGrid) {
                state.isDraggingGrid = false;
                canvas.classList.remove("grid-dragging");
            }
            if (state.isPanning) {
                state.isPanning = false;
                canvas.classList.remove("panning");
            }
        });

        // --- Canvas Wheel Zoom ---
        canvas.addEventListener("wheel", (e) => {
            e.preventDefault();
            if (!state.currentImage) return;

            const rect = canvas.getBoundingClientRect();
            const point = {
                x: e.clientX - rect.left,
                y: e.clientY - rect.top,
            };

            const factor = e.deltaY < 0 ? 1.15 : (1 / 1.15);
            zoomStep(factor, point);
        }, { passive: false });

        // --- Toolbar Button Actions ---
        btnGridToggle.addEventListener("click", () => toggleGrid());
        // --- Grid Calibration Panel Event Listeners ---
        if (btnGridAdjust) {
            btnGridAdjust.addEventListener("click", () => toggleGridPanel());
        }
        if (btnCloseGridPanel) {
            btnCloseGridPanel.addEventListener("click", () => toggleGridPanel(false));
        }
        if (btnGridDone) {
            btnGridDone.addEventListener("click", () => toggleGridPanel(false));
        }
        if (btnGridReset) {
            btnGridReset.addEventListener("click", () => resetGrid());
        }
        if (btnGridSizeDec) {
            btnGridSizeDec.addEventListener("click", () => setGridSize(state.gridSize - 1));
        }
        if (btnGridSizeInc) {
            btnGridSizeInc.addEventListener("click", () => setGridSize(state.gridSize + 1));
        }
        if (gridSizeInput) {
            gridSizeInput.addEventListener("input", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setGridSize(val);
            });
            gridSizeInput.addEventListener("change", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setGridSize(val);
            });
        }
        if (gridSizeSlider) {
            gridSizeSlider.addEventListener("input", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setGridSize(val);
            });
        }
        if (btnGridNudgeUp) {
            btnGridNudgeUp.addEventListener("click", () => nudgeGridOffset(0, -1));
        }
        if (btnGridNudgeDown) {
            btnGridNudgeDown.addEventListener("click", () => nudgeGridOffset(0, 1));
        }
        if (btnGridNudgeLeft) {
            btnGridNudgeLeft.addEventListener("click", () => nudgeGridOffset(-1, 0));
        }
        if (btnGridNudgeRight) {
            btnGridNudgeRight.addEventListener("click", () => nudgeGridOffset(1, 0));
        }
        if (btnGridAlignDrag) {
            btnGridAlignDrag.addEventListener("click", () => toggleGridAlignMode());
        }

        const presetChips = document.querySelectorAll(".preset-chip");
        presetChips.forEach((chip) => {
            chip.addEventListener("click", () => {
                const sz = parseInt(chip.getAttribute("data-size"), 10);
                if (!isNaN(sz)) setGridSize(sz);
            });
        });

        btnFitScreen.addEventListener("click", () => fitToScreen());
        document.getElementById("btn-zoom-in").addEventListener("click", () => zoomStep(1.25));
        document.getElementById("btn-zoom-out").addEventListener("click", () => zoomStep(1 / 1.25));

        btnPrev.addEventListener("click", () => navigatePrev());
        btnNext.addEventListener("click", () => navigateNext());
        btnCurate.addEventListener("click", () => curateCurrentCandidate());
        btnUndoReject?.addEventListener("click", () => undoRejectCandidate());
        btnReject.addEventListener("click", () => rejectCurrentCandidate());
        document.getElementById("btn-help").addEventListener("click", () => toggleHelpModal());
        document.getElementById("btn-close-help-modal").addEventListener("click", () => closeHelpModal());
        document.getElementById("btn-dismiss-help").addEventListener("click", () => closeHelpModal());


        document.getElementById("btn-refresh-staging").addEventListener("click", () => {
            fetchStatus();
            fetchStagingQueue();
        });


        // Filmstrip deck buttons & role badges
        if (btnSplitBundle) {
            btnSplitBundle.addEventListener("click", () => handleSplitOrRegroup());
        }
        if (btnSplitBundleHud) {
            btnSplitBundleHud.addEventListener("click", () => handleSplitOrRegroup());
        }

        const roleBadges = document.querySelectorAll(".role-badge");
        roleBadges.forEach((badge) => {
            badge.addEventListener("click", () => {
                const role = badge.getAttribute("data-role");
                if (role) assignRole(role);
            });
        });

        // Modal backdrop click to close
        helpModal.addEventListener("click", (e) => {
            if (e.target === helpModal) closeHelpModal();
        });
        setupDrawerControls();
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
            stopPanLoop();
            stopZoomLoop();
            stopFsPanLoop();
            stopFsZoomLoop();
            if (state.isDraggingGrid) {
                state.isDraggingGrid = false;
                canvas.classList.remove("grid-dragging");
            }
            if (canvas) {
                canvas.classList.toggle("grid-aligning", state.gridAlignMode);
            }
            if (state.libraryFullscreenIsDraggingGrid) {
                state.libraryFullscreenIsDraggingGrid = false;
                libraryFullscreenCanvas?.parentElement?.classList.remove("grid-dragging");
            }
            if (state.libraryFullscreenIsPanning) {
                state.libraryFullscreenIsPanning = false;
                libraryFullscreenCanvas?.parentElement?.classList.remove("panning");
            }
            if (libraryFullscreenCanvas?.parentElement) {
                libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", state.libraryFullscreenGridAlignMode);
            }
        });
        window.addEventListener("focusin", (e) => {
            const tag = e.target?.tagName?.toLowerCase();
            if (tag === "input" || tag === "textarea" || tag === "select") {
                stopPanLoop();
                stopZoomLoop();
                stopFsPanLoop();
                stopFsZoomLoop();
            }
        });
    }
    function handleKeydown(e) {
        if (canvas) {
            canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
        }
        const tag = document.activeElement ? document.activeElement.tagName.toLowerCase() : "";
        const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

        // Global Modal Dismiss: Escape
        if (e.key === "Escape") {
            stopPanLoop();
            stopZoomLoop();
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
            setGridSize(state.gridSize - sizeStep);
            e.preventDefault();
            return;
        }
        if (e.shiftKey && (e.key === "]" || e.key === "}" || e.code === "BracketRight")) {
            const sizeStep = e.altKey ? 5 : 1;
            setGridSize(state.gridSize + sizeStep);
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
                if (!activePanKeys[key]) {
                    activePanKeys[key] = true;
                    startPanLoop();
                }
                e.preventDefault();
                return;
            }
        }

        // Viewport Zooming: [ and ]
        if (!isInputFocused && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
            if (e.key === "]" || e.code === "BracketRight") {
                activeZoomKeys.in = true;
                startZoomLoop();
                e.preventDefault();
                return;
            }
            if (e.key === "[" || e.code === "BracketLeft") {
                activeZoomKeys.out = true;
                startZoomLoop();
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
            zoomStep(1.2);
            e.preventDefault();
            return;
        }
        if (e.key === "-" || e.key === "_") {
            zoomStep(1 / 1.2);
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
        if (canvas) {
            canvas.classList.toggle("grid-aligning", e.shiftKey || state.gridAlignMode);
        }
        if (libraryFullscreenCanvas?.parentElement) {
            libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", e.shiftKey || state.libraryFullscreenGridAlignMode);
        }
        const key = e.key.toLowerCase();
        if (key === "w" || key === "a" || key === "s" || key === "d") {
            activePanKeys[key] = false;
            activeFsPanKeys[key] = false;
            if (!activePanKeys.w && !activePanKeys.a && !activePanKeys.s && !activePanKeys.d) {
                stopPanLoop();
            }
            if (!activeFsPanKeys.w && !activeFsPanKeys.a && !activeFsPanKeys.s && !activeFsPanKeys.d) {
                stopFsPanLoop();
            }
        }
        if (e.key === "]" || e.code === "BracketRight") {
            activeZoomKeys.in = false;
            activeFsZoomKeys.in = false;
            if (!activeZoomKeys.in && !activeZoomKeys.out) {
                stopZoomLoop();
            }
            if (!activeFsZoomKeys.in && !activeFsZoomKeys.out) {
                stopFsZoomLoop();
            }
        }
        if (e.key === "[" || e.code === "BracketLeft") {
            activeZoomKeys.out = false;
            activeFsZoomKeys.out = false;
            if (!activeZoomKeys.in && !activeZoomKeys.out) {
                stopZoomLoop();
            }
            if (!activeFsZoomKeys.in && !activeFsZoomKeys.out) {
                stopFsZoomLoop();
            }
        }
    }

    // ==========================================================================
    // UI Helpers & Status Display
    // ==========================================================================

    export async function fetchStatus() {
        try {
            const res = await fetch("/api/status");
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const data = await res.json();

            statusIndicator.className = "status-pill status-connected";
            statusText.textContent = `Online (${data.staging_count})`;
            statusIndicator.title = `Curator API Online | Staging: ${data.staging_count} | Curated: ${data.curated_count}`;
        } catch (err) {
            statusIndicator.className = "status-pill status-error";
            statusText.textContent = "Offline";
            statusIndicator.title = `API Disconnected: ${err.message}`;
        }
    }

    export function updateHud() {
        const zoomPercent = Math.round(state.zoom * 100);
        zoomLevelDisplay.textContent = `${zoomPercent}%`;
        hudPanZoom.textContent = `Zoom: ${zoomPercent}% (${Math.round(state.pan.x)}, ${Math.round(state.pan.y)})`;

        const size = state.gridSize || VTT_GRID_SIZE;
        const ox = state.gridOffset ? Math.round(state.gridOffset.x) : 0;
        const oy = state.gridOffset ? Math.round(state.gridOffset.y) : 0;
        if (ox !== 0 || oy !== 0) {
            const signX = ox > 0 ? `+${ox}` : `${ox}`;
            const signY = oy > 0 ? `+${oy}` : `${oy}`;
            hudGridStatus.textContent = `Grid: ${size}px (${signX}, ${signY}) [${state.gridVisible ? "ON" : "OFF"}]`;
        } else {
            hudGridStatus.textContent = `Grid: ${size}px [${state.gridVisible ? "ON" : "OFF"}]`;
        }

        if (hudLatticeStatus) {
            const dims = getCurrentGroundDimensions();
            const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
            if (lattice.isEligible) {
                hudLatticeStatus.textContent = `${lattice.cols} × ${lattice.rows} cells (${lattice.kx} × ${lattice.ky} sections)`;
                hudLatticeStatus.classList.remove("hud-warning");
            } else {
                hudLatticeStatus.textContent = `${lattice.cols} × ${lattice.rows} cells (< 16×16 cells)`;
                hudLatticeStatus.classList.add("hud-warning");
            }
        }

        if (state.currentImage && state.currentImage.complete) {
            const imgW = state.currentImage.naturalWidth || state.currentImage.width;
            const imgH = state.currentImage.naturalHeight || state.currentImage.height;
            hudResolution.textContent = `${imgW} × ${imgH} px`;
        } else {
            hudResolution.textContent = "—";
        }

        // Count cached prefetch images
        let readyCount = 0;
        state.imageCache.forEach((img) => {
            if (img && img.complete && img.naturalWidth > 0) readyCount++;
        });
        hudPrefetchStatus.textContent = `Prefetch: ${readyCount} ready`;

        // Update candidate metadata box in topbar
        if (state.currentIndex >= 0 && state.currentIndex < state.candidates.length) {
            const candidate = state.candidates[state.currentIndex];
            candidateMetaBox.classList.remove("hidden");
            queueCounter.textContent = `${state.currentIndex + 1} / ${state.candidates.length}`;
            candidateId.textContent = candidate.id;
            candidateDimensions.textContent = `${candidate.dimensions[0]} × ${candidate.dimensions[1]}`;
            candidateSource.textContent = candidate.source;
            candidateSource.className = `meta-tag candidate-source badge-source source-${candidate.source}`;
        } else {
            candidateMetaBox.classList.add("hidden");
        }
    }

    export function showEmptyState(visible) {
        if (visible) {
            emptyState.classList.remove("hidden");
        } else {
            emptyState.classList.add("hidden");
        }
    }

    export function showLoading(visible, text = "Loading...") {
        if (visible) {
            loadingText.textContent = text;
            loadingOverlay.classList.remove("hidden");
        } else {
            loadingOverlay.classList.add("hidden");
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

    // Initialize when DOM is ready
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
