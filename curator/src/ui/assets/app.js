/**
 * Dungeon Forge — Battlemap Curator Frontend Application
 * High-performance zero-dependency Canvas 2D triage deck, 64px VTT grid overlay,
 * zero-latency in-memory prefetching, and keyboard-driven hotkeys.
 */

import { state } from "./store.js";
import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    MAX_GRID_SIZE,
    MIN_ZOOM,
    MAX_ZOOM,
    calculateLatticeInfo,
} from "./kit/grid-math.js";
import {
    formatRoleName,
    formatBytes,
    formatReadinessAge,
    formatDuration,
} from "./kit/format.js";
import {
    CANONICAL_THEMES,
    isCanonicalThemeTag,
    normalizeLayerRole,
} from "./kit/tags.js";
import { escapeHtml } from "./kit/dom.js";
import { dom } from "./dom.js";
import {
    setupCanvas,
    resizeCanvas,
    requestRender,
    getCurrentGroundDimensions,
    getLibraryFullscreenDimensions,
    fitToScreen,
    zoomStep,
    startPanLoop,
    stopPanLoop,
    startZoomLoop,
    stopZoomLoop,
    activePanKeys,
    activeZoomKeys,
    PAN_SPEED,
    ZOOM_SPEED,
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
    let trainingPollTimer = null;

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
        btnDecisionUndo, btnGridAdjust, btnGridAlignDrag, btnGridToggle, btnRegenerateDescription,
        btnSplitBundle, btnSplitBundleHud, btnSplitHudText, btnSplitLabel, btnUndoReject,
        canvas, ctx, curateDecisionModal, decisionAcceptedTags, decisionAutocompleteList,
        decisionCardStep, decisionCardTag, decisionCustomStep, decisionDescriptionInput, decisionDescriptionLoading,
        decisionDescriptionStatus, decisionOfflineBadge, decisionOfflineNote, decisionProgressBadge, decisionTagInput,
        decisionValidationError, gridAdjustPanel, gridLatticeCells, gridLatticeSections, gridOffsetXVal,
        gridOffsetYVal, gridSizeInput, gridSizeSlider, layerDeck, layerDeckBundleTag,
        layerDeckCount, layerFilmstrip, roleSelector,
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

    function closeHelpModal() {
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

    function closeIngestionDrawer() {
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

    async function fetchCostStats() {
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
    async function loadBenchmarkModelCatalog() {
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

    function closeBenchmarkDrawer() {
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
    // Dataset Library View & LoRA Export (Issue #31)
    // ==========================================================================

    function switchView(view) {
        if (state.libraryFullscreenActive && view !== "library") {
            closeLibraryFullscreen();
        }
        if (state.libraryDrawerActive && view !== "library") {
            closeLibraryInspector();
        }
        state.currentView = view;
        if (view === "triage") {
            tabNavTriage?.classList.add("active");
            tabNavLibrary?.classList.remove("active");
            triageViewport?.classList.remove("hidden");
            libraryViewport?.classList.add("hidden");
            resizeCanvas();
            requestRender();
        } else if (view === "library") {
            tabNavLibrary?.classList.add("active");
            tabNavTriage?.classList.remove("active");
            libraryViewport?.classList.remove("hidden");
            triageViewport?.classList.add("hidden");
            fetchLibraryMaps();
            syncReadinessToScope({ audit: true });
        }
    }

    async function fetchLibraryMaps() {
        if (!libraryGrid) return;
        libraryGrid.setAttribute("aria-busy", "true");

        try {
            const res = await fetch("/api/curated");
            if (!res.ok) {
                throw new Error(`HTTP ${res.status}: ${res.statusText}`);
            }
            const maps = await res.json();
            state.curatedMaps = maps;

            applyLibraryFilters();
            fetchDatasetSnapshots();
        } catch (err) {
            showToast(`Failed to load dataset library: ${err.message}`, "danger");
        } finally {
            if (libraryGrid) libraryGrid.setAttribute("aria-busy", "false");
        }
    }

    function applyLibraryFilters() {
        if (!state.curatedMaps) return;

        const activeTag = (state.libraryActiveTag || "").toLowerCase().trim();
        const query = (state.librarySearchQuery || "").toLowerCase().trim();
        const facet = state.libraryFacet || "all";

        const filtered = state.curatedMaps.filter((map) => {
            const tags = map.tags || [];
            const layerCount = (map.layers && map.layers.length > 0) ? map.layers.length : 1;
            const hasDesc = Boolean(map.description && map.description.trim());

            // 1. Facet filter
            if (facet === "bundles" && layerCount <= 1) {
                return false;
            }
            if (facet === "single" && layerCount > 1) {
                return false;
            }
            if (facet === "has-desc" && !hasDesc) {
                return false;
            }
            if (facet === "missing-desc" && hasDesc) {
                return false;
            }

            // 2. Active tag filter
            if (activeTag && activeTag !== "all") {
                const matchesTag = tags.some(t => t.toLowerCase() === activeTag);
                if (!matchesTag) {
                    return false;
                }
            }

            // 3. Search query filter
            if (query) {
                const matchesId = (map.id || "").toLowerCase().includes(query);
                const matchesTag = tags.some(t => t.toLowerCase().includes(query));
                const matchesDesc = (map.description || "").toLowerCase().includes(query);
                if (!matchesId && !matchesTag && !matchesDesc) {
                    return false;
                }
            }

            return true;
        });

        sortCuratedMaps(filtered, state.librarySort);
        state.libraryFilteredMaps = filtered;

        // Keep focused index within valid bounds
        if (state.libraryFocusedIndex >= filtered.length) {
            state.libraryFocusedIndex = filtered.length > 0 ? filtered.length - 1 : -1;
        }

        if (libraryCountBadge) {
            libraryCountBadge.textContent = `${filtered.length} Map${filtered.length === 1 ? "" : "s"}`;
        }

        renderLibraryTagPills();
        renderLibraryGrid(filtered);
    }

    function sortCuratedMaps(maps, sortKey) {
        if (!maps || maps.length === 0) return;
        switch (sortKey) {
            case "date-asc":
                maps.sort((a, b) => {
                    const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                    const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                    if (aTime !== bTime) return aTime - bTime;
                    return (a.id || "").localeCompare(b.id || "");
                });
                break;
            case "layers-desc":
                maps.sort((a, b) => {
                    const aLayers = (a.layers && a.layers.length > 0) ? a.layers.length : 1;
                    const bLayers = (b.layers && b.layers.length > 0) ? b.layers.length : 1;
                    if (bLayers !== aLayers) return bLayers - aLayers;
                    const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                    const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                    return bTime - aTime;
                });
                break;
            case "dimensions-desc":
                maps.sort((a, b) => {
                    const aDim = (a.dimensions && Array.isArray(a.dimensions) && a.dimensions.length >= 2)
                        ? (a.dimensions[0] * a.dimensions[1]) : 0;
                    const bDim = (b.dimensions && Array.isArray(b.dimensions) && b.dimensions.length >= 2)
                        ? (b.dimensions[0] * b.dimensions[1]) : 0;
                    if (bDim !== aDim) return bDim - aDim;
                    const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                    const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                    return bTime - aTime;
                });
                break;
            case "id-asc":
                maps.sort((a, b) => {
                    return (a.id || "").localeCompare(b.id || "");
                });
                break;
            case "date-desc":
            default:
                maps.sort((a, b) => {
                    const aTime = a.curated_at ? new Date(a.curated_at).getTime() : 0;
                    const bTime = b.curated_at ? new Date(b.curated_at).getTime() : 0;
                    if (bTime !== aTime) return bTime - aTime;
                    return (b.id || "").localeCompare(a.id || "");
                });
                break;
        }
    }

    function setLibraryDensity(density) {
        if (!["compact", "comfortable", "large"].includes(density)) return;
        state.libraryDensity = density;
        try {
            localStorage.setItem("curator_library_density", density);
        } catch (_) {}

        if (libraryGrid) {
            libraryGrid.classList.remove("density-compact", "density-comfortable", "density-large");
            libraryGrid.classList.add(`density-${density}`);
        }

        const densityButtons = document.querySelectorAll(".btn-density");
        densityButtons.forEach((btn) => {
            btn.classList.toggle("active", btn.dataset.density === density);
        });
    }

    function setLibraryFacet(facet) {
        state.libraryFacet = facet;
        const facetButtons = document.querySelectorAll(".lib-facet-chip");
        facetButtons.forEach((btn) => {
            btn.classList.toggle("active", (btn.dataset.facet || "all") === facet);
        });
        applyLibraryFilters();
    }

    function clearLibraryFilters() {
        state.librarySearchQuery = "";
        if (librarySearchInput) {
            librarySearchInput.value = "";
        }
        state.libraryActiveTag = null;
        setLibraryFacet("all");
        fetchDatasetSnapshots();
        syncReadinessToScope({ audit: true });
    }

    function toggleCardSelection(id) {
        if (!id) return;
        if (state.librarySelectedIds.has(id)) {
            state.librarySelectedIds.delete(id);
        } else {
            state.librarySelectedIds.add(id);
        }
        updateCardSelectionVisual(id);
        updateLibraryBulkBar();
    }

    function updateLibraryBulkBar() {
        const count = state.librarySelectedIds.size;
        if (bulkSelectedCount) {
            bulkSelectedCount.textContent = `${count} Selected`;
        }
        if (libraryBulkBar) {
            if (count > 0) {
                libraryBulkBar.classList.remove("hidden");
            } else {
                libraryBulkBar.classList.add("hidden");
            }
        }
    }

    function clearLibrarySelection() {
        state.librarySelectedIds.clear();
        if (libraryGrid) {
            const cards = libraryGrid.querySelectorAll(".curated-card");
            cards.forEach(card => {
                card.classList.remove("selected");
                const checkbox = card.querySelector(".curated-card-checkbox");
                if (checkbox) checkbox.checked = false;
            });
        }
        updateLibraryBulkBar();
    }
    function updateCardSelectionVisual(id) {
        if (!libraryGrid) return;
        const cards = libraryGrid.querySelectorAll(".curated-card");
        for (const card of cards) {
            if (card.dataset.id === id) {
                const isSelected = state.librarySelectedIds.has(id);
                card.classList.toggle("selected", isSelected);
                const checkbox = card.querySelector(".curated-card-checkbox");
                if (checkbox) checkbox.checked = isSelected;
                break;
            }
        }
    }

    function setLibraryFocus(index, scrollIntoView = true) {
        if (index === -1 || !state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) {
            state.libraryFocusedIndex = -1;
            if (libraryGrid) {
                const cards = libraryGrid.querySelectorAll(".curated-card");
                cards.forEach(c => c.classList.remove("focused"));
            }
            return;
        }
        const maxIdx = state.libraryFilteredMaps.length - 1;
        const newIdx = Math.max(0, Math.min(index, maxIdx));
        state.libraryFocusedIndex = newIdx;

        if (!libraryGrid) return;
        const cards = libraryGrid.querySelectorAll(".curated-card");
        cards.forEach((c, idx) => {
            if (idx === newIdx) {
                c.classList.add("focused");
                if (scrollIntoView) {
                    c.scrollIntoView({ block: "nearest", behavior: "smooth" });
                }
            } else {
                c.classList.remove("focused");
            }
        });
    }

    function getGridColumnCount() {
        if (!libraryGrid) return 1;
        const cards = libraryGrid.querySelectorAll(".curated-card");
        if (cards.length < 2) return 1;
        const firstTop = cards[0].getBoundingClientRect().top;
        let cols = 0;
        for (let i = 0; i < cards.length; i++) {
            if (Math.abs(cards[i].getBoundingClientRect().top - firstTop) < 10) {
                cols++;
            } else {
                break;
            }
        }
        return cols > 0 ? cols : 1;
    }

    function handleLibraryKeydown(e) {
        if (state.libraryFullscreenActive) {
            // Escape: close grid panel if open, otherwise exit fullscreen
            if (e.key === "Escape") {
                if (state.libraryFullscreenGridPanelOpen) {
                    toggleFullscreenGridPanel(false);
                } else {
                    closeLibraryFullscreen();
                }
                e.preventDefault();
                return;
            }

            // WASD: Pan continuous loop
            const keyLower = e.key.toLowerCase();
            if (["w", "a", "s", "d"].includes(keyLower) && !e.ctrlKey && !e.metaKey && !e.altKey) {
                activeFsPanKeys[keyLower] = true;
                startFsPanLoop();
                e.preventDefault();
                return;
            }

            // Zoom in / out: [ and ] or - and + / =
            if (e.key === "]" || e.code === "BracketRight") {
                activeFsZoomKeys.in = true;
                startFsZoomLoop();
                e.preventDefault();
                return;
            }
            if (e.key === "[" || e.code === "BracketLeft") {
                activeFsZoomKeys.out = true;
                startFsZoomLoop();
                e.preventDefault();
                return;
            }
            if (e.key === "=" || e.key === "+") {
                fullscreenZoomStep(1.2);
                e.preventDefault();
                return;
            }
            if (e.key === "-" || e.key === "_") {
                fullscreenZoomStep(1 / 1.2);
                e.preventDefault();
                return;
            }

            // Fit to screen: F or 0
            if ((e.key === "f" || e.key === "F" || e.key === "0") && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
                fullscreenFitToScreen();
                e.preventDefault();
                return;
            }

            // Number keys 1-6 switch displayed layer (or 1 sets 100% zoom if single-layer)
            if (["1", "2", "3", "4", "5", "6"].includes(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
                const targetIdx = parseInt(e.key, 10) - 1;
                const layers = (state.libraryFullscreenMap && state.libraryFullscreenMap.layers) ? state.libraryFullscreenMap.layers : [];
                if (layers.length > 1) {
                    if (targetIdx < layers.length) {
                        switchFullscreenLayer(targetIdx);
                    }
                } else {
                    if (e.key === "1") {
                        setFullscreenZoom100();
                    }
                }
                e.preventDefault();
                return;
            }

            // Toggle grid: G
            if ((e.key === "g" || e.key === "G") && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
                state.libraryFullscreenGridVisible = !state.libraryFullscreenGridVisible;
                if (btnFsGridToggle) btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
                requestFullscreenRender();
                e.preventDefault();
                return;
            }

            // Toggle grid calibration panel: Shift+G
            if ((e.key === "g" || e.key === "G") && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
                toggleFullscreenGridPanel();
                e.preventDefault();
                return;
            }

            // Navigate maps: J / Right (Next), K / Left (Prev)
            if (e.key === "j" || e.key === "J" || e.key === "ArrowRight") {
                if (state.libraryFilteredMaps && state.libraryFullscreenIndex < state.libraryFilteredMaps.length - 1) {
                    loadFullscreenMap(state.libraryFullscreenIndex + 1, 0);
                    setLibraryFocus(state.libraryFullscreenIndex, true);
                }
                e.preventDefault();
                return;
            }
            if (e.key === "k" || e.key === "K" || e.key === "ArrowLeft") {
                if (state.libraryFilteredMaps && state.libraryFullscreenIndex > 0) {
                    loadFullscreenMap(state.libraryFullscreenIndex - 1, 0);
                    setLibraryFocus(state.libraryFullscreenIndex, true);
                }
                e.preventDefault();
                return;
            }

            return;
        }

        // --- Standard Library View Hotkeys (when fullscreen is NOT active) ---
        const activeEl = document.activeElement;
        const tag = activeEl ? activeEl.tagName.toLowerCase() : "";
        const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

        // Deletion modal hotkeys
        if (state.deleteModalOpen) {
            if (e.key === "Escape") {
                closeDeleteCuratedModal();
                e.preventDefault();
                return;
            }
            if (e.key === "Enter" && !isInputFocused) {
                confirmDeleteCurated();
                e.preventDefault();
                return;
            }
            return;
        }

        // Deselect all on Escape if selection is active and drawer is not open
        if (e.key === "Escape" && !state.libraryDrawerActive) {
            if (state.librarySelectedIds.size > 0) {
                clearLibrarySelection();
                e.preventDefault();
                return;
            }
        }
        // Inspector Drawer hotkeys
        if (state.libraryDrawerActive) {
            // Save Changes: Ctrl+S or Cmd+S
            if ((e.ctrlKey || e.metaKey) && (e.key === "s" || e.key === "S")) {
                saveLibraryInspectorChanges();
                e.preventDefault();
                return;
            }

            // Close Drawer: Esc
            if (e.key === "Escape") {
                closeLibraryInspector();
                e.preventDefault();
                return;
            }

            // Toggle Drawer: E (only when not typing in text field)
            if ((e.key === "e" || e.key === "E") && !e.ctrlKey && !e.metaKey && !e.altKey && !isInputFocused) {
                closeLibraryInspector();
                e.preventDefault();
                return;
            }

            // If an input is focused inside the drawer, don't execute other grid navigation hotkeys
            if (isInputFocused) return;
        }

        if (e.ctrlKey || e.metaKey || e.altKey) return;
        if (isInputFocused) return;

        // Single Delete & Reject hotkey: Delete or Backspace on focused card or active drawer
        if (e.key === "Delete" || e.key === "Backspace") {
            if (state.libraryDrawerActive && state.libraryDrawerMap) {
                openDeleteCuratedModal(state.libraryDrawerMap.id);
                e.preventDefault();
                return;
            }
            if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
                const map = state.libraryFilteredMaps[state.libraryFocusedIndex];
                if (map) {
                    openDeleteCuratedModal(map.id);
                    e.preventDefault();
                    return;
                }
            }
        }

        // Toggle / Open drawer on focused card: E
        if (e.key === "e" || e.key === "E") {
            toggleLibraryInspector();
            e.preventDefault();
            return;
        }

        // Open drawer on Enter or Space on focused card:
        if (e.key === "Enter" || e.key === " ") {
            if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
                openLibraryInspector(state.libraryFilteredMaps[state.libraryFocusedIndex]);
                e.preventDefault();
                return;
            }
        }
        // Open fullscreen with F on focused card:
        if ((e.key === "f" || e.key === "F") && !e.shiftKey) {
            if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
                const targetIdx = state.libraryFocusedIndex >= 0 ? state.libraryFocusedIndex : 0;
                openLibraryFullscreen(targetIdx);
            }
            e.preventDefault();
            return;
        }

        // Density hotkeys: 1, 2, 3
        if (e.key === "1") {
            setLibraryDensity("compact");
            e.preventDefault();
            return;
        }
        if (e.key === "2") {
            setLibraryDensity("comfortable");
            e.preventDefault();
            return;
        }
        if (e.key === "3") {
            setLibraryDensity("large");
            e.preventDefault();
            return;
        }

        // Toggle selection on focused card: X / x
        if (e.key === "x" || e.key === "X") {
            if (state.libraryFilteredMaps && state.libraryFocusedIndex >= 0 && state.libraryFocusedIndex < state.libraryFilteredMaps.length) {
                const map = state.libraryFilteredMaps[state.libraryFocusedIndex];
                if (map) {
                    toggleCardSelection(map.id);
                }
            }
            e.preventDefault();
            return;
        }

        // Navigation hotkeys: J/K or ArrowDown/ArrowUp/ArrowLeft/ArrowRight
        if (["j", "J", "ArrowDown", "k", "K", "ArrowUp", "ArrowRight", "ArrowLeft"].includes(e.key)) {
            if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;

            const count = state.libraryFilteredMaps.length;
            const cols = getGridColumnCount();

            if (state.libraryFocusedIndex === -1) {
                setLibraryFocus(0);
                e.preventDefault();
                return;
            }

            let newIdx = state.libraryFocusedIndex;
            if (e.key === "j" || e.key === "J") {
                newIdx = Math.min(count - 1, newIdx + 1);
            } else if (e.key === "k" || e.key === "K") {
                newIdx = Math.max(0, newIdx - 1);
            } else if (e.key === "ArrowRight") {
                newIdx = Math.min(count - 1, newIdx + 1);
            } else if (e.key === "ArrowLeft") {
                newIdx = Math.max(0, newIdx - 1);
            } else if (e.key === "ArrowDown") {
                newIdx = Math.min(count - 1, newIdx + cols);
            } else if (e.key === "ArrowUp") {
                newIdx = Math.max(0, newIdx - cols);
            }

            setLibraryFocus(newIdx);
            e.preventDefault();
            return;
        }
    }

    function renderLibraryTagPills() {
        if (!libraryTagPills) return;
        libraryTagPills.innerHTML = "";

        // Dynamically calculate unique tags across all curated maps
        const tagMap = new Map();
        for (const map of state.curatedMaps) {
            const tags = map.tags || [];
            for (const t of tags) {
                if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
            }
        }

        const sortedKeys = Array.from(tagMap.keys()).sort();
        const fragment = document.createDocumentFragment();

        // "All" pill
        const allBtn = document.createElement("button");
        allBtn.type = "button";
        allBtn.className = `lib-tag-pill ${(!state.libraryActiveTag || state.libraryActiveTag === "all") ? "active" : ""}`;
        allBtn.textContent = "All";
        allBtn.addEventListener("click", () => {
            state.libraryActiveTag = null;
            applyLibraryFilters();
            fetchDatasetSnapshots();
            syncReadinessToScope({ audit: true });
        });
        fragment.appendChild(allBtn);

        // Individual tag pills
        for (const key of sortedKeys) {
            const displayName = tagMap.get(key);
            const pill = document.createElement("button");
            pill.type = "button";
            const isActive = state.libraryActiveTag && state.libraryActiveTag.toLowerCase() === key;
            pill.className = `lib-tag-pill ${isActive ? "active" : ""}`;
            pill.textContent = displayName;
            pill.dataset.tag = displayName;

            pill.addEventListener("click", () => {
                if (state.libraryActiveTag && state.libraryActiveTag.toLowerCase() === key) {
                    state.libraryActiveTag = null;
                } else {
                    state.libraryActiveTag = displayName;
                }
                applyLibraryFilters();
                fetchDatasetSnapshots();
                syncReadinessToScope({ audit: true });
            });

            fragment.appendChild(pill);
        }

        libraryTagPills.appendChild(fragment);
    }

    function renderLibraryGrid(maps) {
        if (!libraryGrid || !libraryEmpty) return;
        libraryGrid.innerHTML = "";

        const hasAnyCuratedMaps = state.curatedMaps && state.curatedMaps.length > 0;
        const hasMatchingMaps = maps && maps.length > 0;

        const showEmpty = !hasAnyCuratedMaps;
        const showFilterEmpty = hasAnyCuratedMaps && !hasMatchingMaps;
        const showGrid = hasAnyCuratedMaps && hasMatchingMaps;

        libraryEmpty.classList.toggle("hidden", !showEmpty);
        if (libraryFilterEmpty) libraryFilterEmpty.classList.toggle("hidden", !showFilterEmpty);
        libraryGrid.classList.toggle("hidden", !showGrid);

        if (!showGrid) return;
        const fragment = document.createDocumentFragment();
        maps.forEach((map, idx) => {
            fragment.appendChild(renderCuratedCard(map, idx));
        });

        libraryGrid.appendChild(fragment);
    }

    function getShortRoleChip(role, elevation) {
        const r = (role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") {
            return "G";
        }
        if (r === "upperfloor" || r === "upper") {
            const elev = (typeof elevation === "number" && elevation !== 0) ? Math.abs(elevation) : 1;
            return `U+${elev}`;
        }
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
            const elev = (typeof elevation === "number" && elevation !== 0) ? -Math.abs(elevation) : -1;
            return `B${elev}`;
        }
        if (r === "roof") {
            return "Roof";
        }
        if (r === "variant") {
            return "Var";
        }
        return (role || "").slice(0, 3).toUpperCase() || "?";
    }

    function getScrubPillRoleClass(role) {
        const r = (role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") return "role-ground";
        if (r === "upperfloor" || r === "upper") return "role-upper";
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "role-basement";
        if (r === "roof") return "role-roof";
        if (r === "variant") return "role-variant";
        return "role-default";
    }

    function computeElevationSpan(layers) {
        if (!layers || layers.length <= 1) return null;
        let minElev = null;
        let maxElev = null;
        let hasElevations = false;

        for (const l of layers) {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            let elev = null;
            if (typeof l.elevation === "number") {
                elev = l.elevation;
            } else if (r === "ground") {
                elev = 0;
            } else if (r === "upperfloor" || r === "upper") {
                elev = 1;
            } else if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
                elev = -1;
            }

            if (elev !== null) {
                hasElevations = true;
                if (minElev === null || elev < minElev) minElev = elev;
                if (maxElev === null || elev > maxElev) maxElev = elev;
            }
        }

        if (!hasElevations || minElev === null || maxElev === null || minElev === maxElev) {
            return null;
        }

        const fmt = (n) => (n > 0 ? `+${n}` : n === 0 ? "0" : `−${Math.abs(n)}`);
        return `${fmt(minElev)} to ${fmt(maxElev)}`;
    }

    function renderCuratedCard(map, index) {
        const card = document.createElement("div");
        const isSelected = state.librarySelectedIds.has(map.id);
        const isFocused = state.libraryFocusedIndex === index;
        card.className = `curated-card${isSelected ? " selected" : ""}${isFocused ? " focused" : ""}`;
        card.dataset.id = map.id;
        const tags = map.tags || [];
        const primaryTheme = tags[0] || "dungeon";
        card.dataset.theme = primaryTheme;

        const thumbUrl = `/api/image/curated/${encodeURIComponent(map.id)}`;
        const layerCount = (map.layers && map.layers.length > 0) ? map.layers.length : 1;
        const layerLabel = `${layerCount} Layer${layerCount === 1 ? "" : "s"}`;
        const isBundle = map.layers && Array.isArray(map.layers) && map.layers.length > 1;

        const elevationSpan = isBundle ? computeElevationSpan(map.layers) : null;
        const elevationBadgeHtml = elevationSpan
            ? `<span class="curated-elevation-badge" title="Elevation span: ${elevationSpan}">${escapeHtml(elevationSpan)}</span>`
            : "";

        let scrubberHtml = "";
        if (isBundle) {
            const groundIndex = map.layers.findIndex(l => {
                const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                return r === "ground";
            });
            const defaultIndex = groundIndex >= 0 ? groundIndex : 0;

            const pillsHtml = map.layers.map((layer, idx) => {
                const shortRole = getShortRoleChip(layer.role, layer.elevation);
                const roleCls = getScrubPillRoleClass(layer.role);
                const isActive = idx === defaultIndex;
                const fullRole = formatRoleName(layer.role, layer.elevation);
                const elevText = (typeof layer.elevation === "number")
                    ? `Elevation: ${layer.elevation >= 0 ? `+${layer.elevation}` : layer.elevation}`
                    : "Elevation: 0";
                const tooltipParts = [fullRole, elevText, layer.filename || ""].filter(Boolean);
                const tooltip = tooltipParts.join(" • ");
                const layerIdx = layer.index !== undefined ? layer.index : idx;

                return `<button type="button" class="layer-scrub-pill ${roleCls}${isActive ? " active" : ""}" data-layer-idx="${layerIdx}" title="${escapeHtml(tooltip)}">${escapeHtml(shortRole)}</button>`;
            }).join("");

            scrubberHtml = `<div class="curated-layer-scrubber">${pillsHtml}</div>`;
        }

        const pills = tags.map(
            t => `<span class="curated-tag-pill">${escapeHtml(t)}</span>`
        );

        const tagsHtml = pills.length > 0
            ? pills.join("")
            : `<span class="curated-tag-pill text-muted">no tags</span>`;

        const descriptionText = (map.description && map.description.trim())
            ? map.description.trim()
            : (map.layers && map.layers[0] && map.layers[0].caption
                ? map.layers[0].caption
                : `dungeonforge battlemap, ${primaryTheme}`);

        card.innerHTML = `
            <div class="curated-thumb-box">
                <div class="curated-select-wrapper" title="Toggle selection (X)">
                    <input type="checkbox" class="curated-card-checkbox" ${isSelected ? "checked" : ""} aria-label="Select battlemap ${escapeHtml(map.id)}" />
                </div>
                <img class="curated-thumb-img" src="${thumbUrl}" alt="Map ${escapeHtml(map.id)}" loading="lazy">
                <div class="curated-thumb-overlay">
                    <span class="curated-theme-badge">${escapeHtml(primaryTheme)}</span>
                    <div class="curated-overlay-badges">
                        ${elevationBadgeHtml}
                        <span class="curated-layers-badge">${layerLabel}</span>
                    </div>
                </div>
                ${scrubberHtml}
            </div>
            <div class="curated-card-body">
                <div class="curated-header-row">
                    <span class="curated-card-id mono" title="${escapeHtml(map.id)}">${escapeHtml(map.id)}</span>
                    <div class="curated-card-actions">
                        <button type="button" class="btn-card-edit" title="Edit Metadata &amp; Refine (Hotkey: E or Enter)" aria-label="Edit Map">
                            <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                                <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
                            </svg>
                            <span>Edit</span>
                        </button>
                        <button type="button" class="btn-card-fullscreen" title="Inspect Fullscreen (Hotkey: F)" aria-label="Inspect Fullscreen">
                            <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                                <polyline points="15 3 21 3 21 9"/>
                                <polyline points="9 21 3 21 3 15"/>
                                <line x1="21" y1="3" x2="14" y2="10"/>
                                <line x1="3" y1="21" x2="10" y2="14"/>
                            </svg>
                            <span>Inspect</span>
                        </button>
                        <button type="button" class="btn-card-delete" title="Delete &amp; Reject Map (Hotkey: Delete)" aria-label="Delete Map">
                            <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                                <polyline points="3 6 5 6 21 6"/>
                                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                                <line x1="10" y1="11" x2="10" y2="17"/>
                                <line x1="14" y1="11" x2="14" y2="17"/>
                            </svg>
                            <span>Delete</span>
                        </button>
                    </div>
                </div>
                <div class="curated-tags-row">
                    ${tagsHtml}
                </div>
                <div class="curated-description-box">
                    <span class="curated-description-label">DungeonForge Prompt Description</span>
                    <div class="curated-description-text">${escapeHtml(descriptionText)}</div>
                </div>
            </div>
        `;

        if (isBundle) {
            const scrubber = card.querySelector(".curated-layer-scrubber");
            const thumbImg = card.querySelector(".curated-thumb-img");

            if (scrubber && thumbImg) {
                const groundIndex = map.layers.findIndex(l => {
                    const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    return r === "ground";
                });
                const defaultIndex = groundIndex >= 0 ? groundIndex : 0;
                const pillButtons = scrubber.querySelectorAll(".layer-scrub-pill");

                const activateLayer = (layer, pill) => {
                    pillButtons.forEach(p => p.classList.remove("active"));
                    pill.classList.add("active");

                    const layerIdx = layer.index !== undefined ? layer.index : parseInt(pill.dataset.layerIdx, 10);
                    const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

                    if (thumbImg.src !== directUrl && !thumbImg.src.endsWith(directUrl)) {
                        thumbImg.src = directUrl;
                    }
                };

                pillButtons.forEach((pill, idx) => {
                    const layer = map.layers[idx];
                    if (!layer) return;

                    pill.addEventListener("mouseenter", () => {
                        activateLayer(layer, pill);
                    });

                    pill.addEventListener("click", (e) => {
                        e.stopPropagation();
                        activateLayer(layer, pill);
                    });

                    pill.addEventListener("touchstart", (e) => {
                        e.stopPropagation();
                        activateLayer(layer, pill);
                    }, { passive: true });
                });

                scrubber.addEventListener("mouseleave", () => {
                    pillButtons.forEach((p, idx) => {
                        p.classList.toggle("active", idx === defaultIndex);
                    });
                    thumbImg.onerror = null;
                    thumbImg.src = thumbUrl;
                });
            }
        }

        const selectWrapper = card.querySelector(".curated-select-wrapper");
        const checkbox = card.querySelector(".curated-card-checkbox");
        checkbox?.addEventListener("click", (e) => {
            e.stopPropagation();
        });
        checkbox?.addEventListener("change", (e) => {
            e.stopPropagation();
            toggleCardSelection(map.id);
        });
        selectWrapper?.addEventListener("click", (e) => {
            if (e.target === checkbox) return;
            e.stopPropagation();
            toggleCardSelection(map.id);
        });

        card.addEventListener("click", (e) => {
            if (e.target.closest(".curated-select-wrapper")) return;
            if (e.target.closest(".curated-layer-scrubber")) return;
            if (e.target.closest(".curated-card-actions")) return;
            if (e.shiftKey) {
                toggleCardSelection(map.id);
                return;
            }
            setLibraryFocus(index, false);
        });

        const btnEdit = card.querySelector(".btn-card-edit");
        btnEdit?.addEventListener("click", (e) => {
            e.stopPropagation();
            setLibraryFocus(index, false);
            openLibraryInspector(map);
        });

        const btnInspect = card.querySelector(".btn-card-fullscreen");
        btnInspect?.addEventListener("click", (e) => {
            e.stopPropagation();
            openLibraryFullscreen(index);
        });

        const btnDelete = card.querySelector(".btn-card-delete");
        btnDelete?.addEventListener("click", (e) => {
            e.stopPropagation();
            openDeleteCuratedModal(map.id);
        });
        const thumbBox = card.querySelector(".curated-thumb-box");
        thumbBox?.addEventListener("dblclick", (e) => {
            if (e.target.closest(".curated-select-wrapper")) return;
            if (e.target.closest(".curated-layer-scrubber")) return;
            e.stopPropagation();
            openLibraryFullscreen(index);
        });

        return card;
    }

    /** Active library tag when it is a theme the dataset endpoints can scope to, else null. */
    function datasetScopeTheme() {
        const tag = (state.libraryActiveTag || "").toLowerCase();
        return CANONICAL_THEMES.includes(tag) ? tag : null;
    }

    /** Theme query for the current library tag filter, mirroring the export scope exactly. */
    function datasetScopeQuery() {
        const theme = datasetScopeTheme();
        return theme ? `?theme=${encodeURIComponent(theme)}` : "";
    }

    async function exportLoraDataset() {
        if (state.isExporting) return;
        state.isExporting = true;
        if (btnExportLora) btnExportLora.disabled = true;
        if (btnExportLabel) btnExportLabel.textContent = "Packaging ZIP...";

        const tag = state.libraryActiveTag || "all";
        const tagLabel = tag === "all" ? "all maps" : tag;
        showToast(`Packaging LoRA dataset ZIP archive (${tagLabel})...`, "info");

        try {
            const url = `/api/dataset/export${datasetScopeQuery()}`;
            const res = await fetch(url);
            if (!res.ok) {
                throw new Error(`Export failed (HTTP ${res.status}: ${res.statusText})`);
            }

            const blob = await res.blob();
            const filename = "lora-dataset.zip";

            const downloadUrl = window.URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = downloadUrl;
            a.download = filename;
            document.body.appendChild(a);
            a.click();
            a.remove();
            window.URL.revokeObjectURL(downloadUrl);

            showToast(`LoRA dataset downloaded (${formatBytes(blob.size)})!`, "success");
        } catch (err) {
            showToast(`Export error: ${err.message}`, "danger");
        } finally {
            state.isExporting = false;
            if (btnExportLora) btnExportLora.disabled = false;
            if (btnExportLabel) btnExportLabel.textContent = "Export LoRA Dataset";
        }
    }

    // ==========================================================================
    // LoRA Training Dataset Readiness (Issues #184, #245)
    // ==========================================================================
    //
    // The audit is rendered inside the Frozen Dataset Snapshots drawer, summarized by issue kind,
    // and cached per theme scope so a page reload or a view switch never re-reads every layer.

    const READINESS_CACHE_KEY = "curator_dataset_readiness";
    const READINESS_DESCRIPTION_CODES = [
        "physical_description_missing",
        "physical_description_not_neutral",
    ];
    const READINESS_GRID_CODES = ["grid_below_minimum", "section_unresolvable"];
    const READINESS_GROUND_CODES = ["ground_layer_missing", "ground_image_unreadable"];

    const DATASET_ISSUE_LABELS = {
        empty_dataset: "Empty dataset scope",
        ground_layer_missing: "Ground layer missing",
        ground_image_unreadable: "Ground layer unreadable",
        grid_below_minimum: "Grid below 16×16 cells",
        section_unresolvable: "Map Section unresolvable",
        physical_description_missing: "Physical Description missing",
        physical_description_not_neutral: "Physical Description not neutral",
    };

    function datasetIssueLabel(code) {
        return DATASET_ISSUE_LABELS[code] || code;
    }

    /** The one-click remediation a flagged code offers, or null when no single click resolves it. */
    function datasetIssueFix(code) {
        if (READINESS_DESCRIPTION_CODES.includes(code)) return { action: "describe", label: "Fix descriptions" };
        if (READINESS_GRID_CODES.includes(code)) return { action: "grid", label: "Edit grid" };
        if (READINESS_GROUND_CODES.includes(code)) return { action: "inspect", label: "Inspect bundle" };
        return null;
    }

    function readinessScopeKey() {
        return datasetScopeTheme() || "all";
    }

    function readinessScopeLabel() {
        const theme = datasetScopeTheme();
        return theme ? `theme '${theme}'` : "all themes";
    }

    /** Persisted audits keyed by theme scope, so an unchanged scope needs no re-audit. */
    function readReadinessCache() {
        try {
            const raw = localStorage.getItem(READINESS_CACHE_KEY);
            if (!raw) return {};
            const parsed = JSON.parse(raw);
            return parsed && typeof parsed === "object" ? parsed : {};
        } catch (_) {
            return {};
        }
    }

    function cachedReadiness(scopeKey = readinessScopeKey()) {
        const entry = readReadinessCache()[scopeKey];
        if (!entry || !entry.report || typeof entry.report !== "object") return null;
        return { report: entry.report, auditedAt: entry.audited_at || null };
    }

    function saveReadinessCache(scopeKey, report, auditedAt) {
        try {
            const cache = readReadinessCache();
            cache[scopeKey] = { audited_at: auditedAt, report };
            localStorage.setItem(READINESS_CACHE_KEY, JSON.stringify(cache));
        } catch (_) {
            // A full or blocked storage quota must not break the audit itself.
        }
    }

    function clearReadinessCache() {
        try {
            localStorage.removeItem(READINESS_CACHE_KEY);
        } catch (_) {}
    }

    /** Codes aggregated across maps: one row per issue kind, never one row per failed map. */
    function aggregateDatasetIssues(report) {
        const byCode = new Map();
        ((report && report.maps) || []).forEach((map) => {
            (map.issues || []).forEach((issue) => {
                const entry = byCode.get(issue.code) || { code: issue.code, occurrences: 0, maps: new Set() };
                entry.occurrences += 1;
                entry.maps.add(map.id);
                byCode.set(issue.code, entry);
            });
        });
        return [...byCode.values()]
            .map((entry) => ({ code: entry.code, occurrences: entry.occurrences, maps: [...entry.maps].sort() }))
            .sort((a, b) => b.maps.length - a.maps.length || a.code.localeCompare(b.code));
    }

    /** Maps flagged with one issue code, taken from the same aggregation the panel renders. */
    function mapsWithIssue(code, report = state.readinessReport) {
        return aggregateDatasetIssues(report).find((entry) => entry.code === code)?.maps || [];
    }

    function descriptionMapIds(report = state.readinessReport) {
        return [
            ...new Set(
                aggregateDatasetIssues(report)
                    .filter((entry) => READINESS_DESCRIPTION_CODES.includes(entry.code))
                    .flatMap((entry) => entry.maps),
            ),
        ];
    }

    function readinessStatusText(report, refusal) {
        if (refusal) return "Freeze refused";
        if (!report) return "Not audited";
        if ((report.maps_total || 0) === 0) return "No curated maps in scope";
        if (report.ready) return `Ready · ${report.sections_total || 0} sections`;
        const blocked = (report.maps_total || 0) - (report.maps_ready || 0);
        return `${blocked} of ${report.maps_total || 0} maps blocked`;
    }

    function readinessStatsHtml(report, auditedAt) {
        const stats = [
            ["Maps ready", report ? `${report.maps_ready || 0} / ${report.maps_total || 0}` : "—"],
            ["Sections ready", report ? `${report.sections_ready || 0} / ${report.sections_total || 0}` : "—"],
            ["Scope", readinessScopeLabel()],
            ["Last audit", auditedAt ? formatReadinessAge(auditedAt) : "never"],
        ];
        return stats
            .map(([label, value]) => `<div class="readiness-stat"><span class="readiness-stat-label">${escapeHtml(label)}</span><span class="readiness-stat-value mono">${escapeHtml(value)}</span></div>`)
            .join("");
    }

    function readinessIssueRowHtml(entry) {
        const fix = datasetIssueFix(entry.code);
        const maps = `${entry.maps.length} ${entry.maps.length === 1 ? "map" : "maps"}`;
        const findings = entry.occurrences > entry.maps.length ? ` · ${entry.occurrences} findings` : "";
        const title = entry.maps.join(", ");
        const button = fix
            ? `<button type="button" class="btn btn-secondary btn-xs readiness-action-btn" data-fix="${escapeHtml(fix.action)}" data-issue-code="${escapeHtml(entry.code)}" data-label="${escapeHtml(fix.label)}" title="${escapeHtml(title)}"><span class="readiness-action-label">${escapeHtml(fix.label)}</span><span class="readiness-action-spinner hidden"></span></button>`
            : "";
        return `<li class="readiness-issue-row" title="${escapeHtml(title)}"><div class="readiness-issue-text"><span class="readiness-issue-code">${escapeHtml(datasetIssueLabel(entry.code))}</span><span class="readiness-issue-count mono">${escapeHtml(maps + findings)}</span></div>${button ? `<div class="readiness-issue-actions">${button}</div>` : ""}</li>`;
    }

    function readinessIssuesHtml(report, refusal) {
        if (!report) {
            const blocking = (refusal && refusal.blocking) || [];
            const note = refusal && refusal.error
                ? `<p class="readiness-refusal-note">${escapeHtml(refusal.error)}</p>`
                : "";
            if (blocking.length > 0) {
                return `${note}<ul class="readiness-issue-list">${blocking.map((issue) => `<li class="readiness-issue-row"><div class="readiness-issue-text"><span class="readiness-issue-code">${escapeHtml(datasetIssueLabel(issue.code))}</span><span class="readiness-issue-count mono">${escapeHtml(issue.count > 0 ? `${issue.count} findings` : "no curated maps in scope")}</span></div></li>`).join("")}</ul>`;
            }
            return `${note}<div class="readiness-empty">No audit has been stored for ${escapeHtml(readinessScopeLabel())} yet. Run <strong>Audit</strong>, or freeze a snapshot — a refusal carries the full audit.</div>`;
        }
        if ((report.maps_total || 0) === 0) {
            return `<div class="readiness-empty">No curated maps in this scope: an empty dataset is not ready to train on. Curate at least one battlemap with 16×16 Map Sections, then audit again.</div>`;
        }
        const entries = aggregateDatasetIssues(report);
        if (entries.length === 0) {
            // A stored report may carry no map details (older shape, hand-edited cache); without
            // them a false "clean" claim would sit next to a non-ready status line.
            return report.ready
                ? `<div class="readiness-clean">No issues: every curated map in scope exports clean 16×16 Map Section samples.</div>`
                : `<div class="readiness-empty">The stored audit for ${escapeHtml(readinessScopeLabel())} reports no per-map details but is not ready. Run <strong>Audit</strong> again.</div>`;
        }
        return `<ul class="readiness-issue-list">${entries.map(readinessIssueRowHtml).join("")}</ul>`;
    }

    function renderReadinessPanel() {
        if (!datasetReadinessSummary || !datasetReadinessIssues) return;
        const report = state.readinessReport;
        const refusal = state.readinessRefusal;
        const ready = Boolean(report && report.ready);

        if (datasetReadinessStatus) {
            datasetReadinessStatus.textContent = readinessStatusText(report, refusal);
            datasetReadinessStatus.classList.toggle("is-ready", ready && !refusal);
            datasetReadinessStatus.classList.toggle("has-issues", Boolean(refusal) || Boolean(report && !ready));
        }
        if (datasetReadinessIcon) {
            datasetReadinessIcon.textContent = refusal ? "✕" : ready ? "✓" : report ? "!" : "◍";
            datasetReadinessIcon.className = refusal || (report && !ready)
                ? "readiness-icon has-issues"
                : ready
                    ? "readiness-icon is-ready"
                    : "readiness-icon";
        }
        datasetReadinessSummary.innerHTML = readinessStatsHtml(report, state.readinessAuditedAt);
        datasetReadinessIssues.innerHTML = readinessIssuesHtml(report, refusal);
        if (btnAuditReadinessLabel) {
            btnAuditReadinessLabel.textContent = state.readinessAudit ? "Auditing…" : report ? "Re-audit" : "Audit";
        }
    }

    /** Show the audit stored for the active scope; audit only when the cache has nothing. */
    function syncReadinessToScope({ audit = false } = {}) {
        const scopeKey = readinessScopeKey();
        const cached = cachedReadiness(scopeKey);
        const scopeChanged = state.readinessScopeKey !== scopeKey;
        state.readinessScopeKey = scopeKey;
        state.readinessReport = cached ? cached.report : null;
        state.readinessAuditedAt = cached ? cached.auditedAt : null;
        // A refusal describes the scope that produced it; switching themes drops it.
        if (scopeChanged) state.readinessRefusal = null;
        renderReadinessPanel();
        // A newly selected scope must be audited even while another scope's audit is running: the
        // new audit queues behind the stale one and the stale response is discarded.
        if (audit && !cached) {
            auditDatasetReadiness();
        }
    }

    function applyReadinessReport(scopeKey, report, auditedAt) {
        saveReadinessCache(scopeKey, report, auditedAt);
        if (scopeKey !== readinessScopeKey()) return;
        state.readinessScopeKey = scopeKey;
        state.readinessReport = report;
        state.readinessAuditedAt = auditedAt;
        state.readinessRefusal = null;
        renderReadinessPanel();
    }

    /** Render the cached report when it exists; only an explicit force or a mutation re-reads layers. */
    async function auditDatasetReadiness({ force = false } = {}) {
        let scopeKey = readinessScopeKey();
        if (!force) {
            const cached = cachedReadiness(scopeKey);
            if (cached) {
                state.readinessReport = cached.report;
                state.readinessAuditedAt = cached.auditedAt;
                state.readinessRefusal = null;
                renderReadinessPanel();
                return cached.report;
            }
        }
        // An audit is only reusable for the scope and generation it started in: a curated write
        // bumps the epoch and a scope switch changes the key, so either one must not join a request
        // whose report predates it. Let the stale audit settle, then read the layers again — unless
        // another caller already queued the fresh audit, in which case join that one.
        while (state.readinessAudit) {
            if (state.readinessAuditScope === readinessScopeKey() && state.readinessAuditEpoch === state.readinessEpoch) {
                return state.readinessAudit;
            }
            await state.readinessAudit;
        }
        // Waiting may have spanned a scope switch: audit the scope that is active now.
        scopeKey = readinessScopeKey();

        const epoch = state.readinessEpoch;
        // The in-flight promise is the only "auditing" flag: the panel derives its label and the
        // button its disabled state from it, and it is cleared once the request has settled.
        if (btnAuditReadiness) btnAuditReadiness.disabled = true;
        const run = (async () => {
            try {
                const res = await fetch(`/api/dataset/readiness${datasetScopeQuery()}`);
                if (!res.ok) {
                    const errData = await res.json().catch(() => ({}));
                    throw new Error(errData.error || `HTTP ${res.status}`);
                }
                const report = await res.json();
                // A write or a scope switch while the GET ran makes this report stale: never cache
                // it, and never let it overwrite the panel that now describes something else.
                if (epoch !== state.readinessEpoch || scopeKey !== readinessScopeKey()) return null;
                applyReadinessReport(scopeKey, report, new Date().toISOString());
                return report;
            } catch (err) {
                showToast(`Dataset audit failed: ${err.message}`, "danger");
                return null;
            }
        })();
        state.readinessAudit = run;
        state.readinessAuditEpoch = epoch;
        state.readinessAuditScope = scopeKey;
        // A newer audit may already own the flag; only the settling one clears it.
        const settle = () => {
            if (state.readinessAudit !== run) return;
            state.readinessAudit = null;
            if (btnAuditReadiness) btnAuditReadiness.disabled = false;
            renderReadinessPanel();
        };
        run.then(settle, settle);
        renderReadinessPanel();
        return run;
    }

    /** A written curated bundle invalidates every cached scope. */
    export function invalidateDatasetReadiness() {
        clearReadinessCache();
        // Any audit already in flight read the pre-write store, so it must be neither joined nor
        // cached: the bump makes a fresh audit start behind it.
        state.readinessEpoch += 1;
        state.readinessRefusal = null;
        state.readinessScopeKey = readinessScopeKey();
        state.readinessReport = null;
        state.readinessAuditedAt = null;
        renderReadinessPanel();
        if (state.currentView === "library") {
            // The cache was just cleared, so this cannot short-circuit on a stale report.
            auditDatasetReadiness();
        }
    }

    // --------------------------------------------------------------------------
    // Remediation (Issue #245)
    // --------------------------------------------------------------------------

    /** Regenerate the map's Physical Description and persist it with its Map Sections. */
    async function applyDatasetDescription(mapId) {
        const res = await fetch(`/api/curated/${encodeURIComponent(mapId)}/describe`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ apply: true }),
        });
        if (!res.ok) {
            const errData = await res.json().catch(() => ({}));
            throw new Error(errData.error || `HTTP ${res.status}`);
        }
        const payload = await res.json();
        if (payload.applied !== true) {
            throw new Error("Description was generated but not persisted");
        }
        return payload;
    }

    /**
     * One regeneration request per map: a second click during an in-flight
     * `POST /api/curated/{id}/describe` reuses that promise instead of skipping the map.
     */
    function applyDescriptionOnce(mapId) {
        const inFlight = state.describeRequests.get(mapId);
        if (inFlight) return inFlight;
        const request = applyDatasetDescription(mapId).finally(() => state.describeRequests.delete(mapId));
        state.describeRequests.set(mapId, request);
        return request;
    }

    function readinessActionButtons() {
        return datasetReadinessIssues ? [...datasetReadinessIssues.querySelectorAll("[data-fix]")] : [];
    }

    function setReadinessActionsBusy(busy) {
        readinessActionButtons().forEach((button) => {
            button.disabled = busy;
            const isDescription = button.dataset.fix === "describe";
            button.classList.toggle("is-busy", busy && isDescription);
            button.querySelector(".readiness-action-spinner")?.classList.toggle("hidden", !(busy && isDescription));
            const label = button.querySelector(".readiness-action-label");
            if (!label) return;
            if (!busy) {
                label.textContent = button.dataset.label || label.textContent;
                return;
            }
            label.textContent = isDescription && state.readinessBatch
                ? `Regenerating ${state.readinessBatch.done}/${state.readinessBatch.total}…`
                : "Working…";
        });
    }

    /**
     * Say what a batch actually did: how many requests it sent, how many it reused from a click
     * that was already in flight, and which maps failed and why. Never a bare "Fixed 0".
     */
    function reportBatchDescriptionOutcome(sent, reused, failed) {
        const parts = [];
        if (sent > 0) parts.push(`Fixed ${sent} description${sent === 1 ? "" : "s"}`);
        if (reused > 0) parts.push(`reused ${reused} in-flight regeneration${reused === 1 ? "" : "s"}`);
        if (failed.length > 0) parts.push(`${failed.length} failed: ${failed.join(", ")}`);
        const message = parts.join("; ");
        const level = failed.length > 0 ? "danger" : sent > 0 ? "success" : "info";
        showToast(message.charAt(0).toUpperCase() + message.slice(1), level);
    }

    /** Regenerate every description-flagged map once, then re-audit the scope. */
    async function fixDatasetDescriptions() {
        if (state.readinessBatch) {
            showToast("Description regeneration is already running", "info");
            return;
        }
        if (!state.readinessReport) {
            showToast("No stored audit for this scope yet — auditing before regenerating", "info");
            const report = await auditDatasetReadiness({ force: true });
            if (!report) return;
        }
        const mapIds = descriptionMapIds();
        if (mapIds.length === 0) {
            showToast("No map in this scope has a description issue", "info");
            return;
        }

        state.readinessBatch = { total: mapIds.length, done: 0 };
        setReadinessActionsBusy(true);
        const failed = [];
        let sent = 0;
        let reused = 0;
        try {
            for (const mapId of mapIds) {
                state.readinessBatch.done += 1;
                setReadinessActionsBusy(true);
                // Snapshot before awaiting: a request that settles must not count as reused.
                const joinedInFlight = state.describeRequests.has(mapId);
                try {
                    await applyDescriptionOnce(mapId);
                    if (joinedInFlight) reused += 1;
                    else sent += 1;
                } catch (err) {
                    failed.push(`${mapId} (${err.message})`);
                    console.error(`Description remediation failed for ${mapId}:`, err);
                }
            }
        } finally {
            state.readinessBatch = null;
            setReadinessActionsBusy(false);
        }

        reportBatchDescriptionOutcome(sent, reused, failed);
        invalidateDatasetReadiness();
        const report = await auditDatasetReadiness();
        const remaining = descriptionMapIds(report);
        if (remaining.length > 0) {
            showToast(`${remaining.length} map(s) still fail the description audit`, "warning");
        }
    }

    /** Run the fix a flagged issue kind offers: descriptions, the grid editor, or the inspector. */
    async function fixDatasetIssueKind(code) {
        const fix = datasetIssueFix(code);
        if (!fix) return;
        if (fix.action === "describe") {
            await fixDatasetDescriptions();
            return;
        }
        const targets = mapsWithIssue(code);
        if (targets.length === 0) {
            showToast(`No map reports ${datasetIssueLabel(code)} any more; auditing again`, "info");
            await auditDatasetReadiness({ force: true });
            return;
        }
        if (targets.length > 1) {
            const noun = fix.action === "grid" ? "a grid fix" : "a bundle fix";
            showToast(`${targets.length} maps need ${noun}: opening ${targets[0]} first`, "info");
        }
        if (fix.action === "grid") {
            await openGridCalibrationForMap(targets[0]);
            return;
        }
        await openCuratedInspectorById(targets[0]);
    }

    /** Load the library scope, switch to it, and return the index of `mapId` in the filtered deck. */
    async function focusLibraryMap(mapId) {
        if (!state.curatedMaps || !state.curatedMaps.some(map => map.id === mapId)) {
            await fetchLibraryMaps();
        }
        switchView("library");
        let index = (state.libraryFilteredMaps || []).findIndex(map => map.id === mapId);
        if (index < 0) {
            clearLibraryFilters();
            applyLibraryFilters();
            index = (state.libraryFilteredMaps || []).findIndex(map => map.id === mapId);
        }
        return index;
    }

    /** Open the fullscreen grid calibration editor for a flagged curated map. */
    async function openGridCalibrationForMap(mapId) {
        const index = await focusLibraryMap(mapId);
        if (index < 0) {
            showToast(`Curated map '${mapId}' is not in the library scope`, "danger");
            return;
        }
        openLibraryFullscreen(index);
        toggleFullscreenGridPanel(true);
    }

    /** Open the curated inspector drawer, where a defective bundle can be inspected or deleted. */
    async function openCuratedInspectorById(mapId) {
        const index = await focusLibraryMap(mapId);
        if (index < 0) {
            showToast(`Curated map '${mapId}' is not in the library scope`, "danger");
            return;
        }
        openLibraryInspector(state.libraryFilteredMaps[index]);
    }

    /** Reveal the readiness block after a refusal: the drawer owns the audit, not a modal. */
    function revealReadinessPanel() {
        if (state.currentView !== "library") switchView("library");
        if (state.snapshotsCollapsed) toggleSnapshotsShelf();
        datasetReadinessPanel?.classList.add("is-refused");
        datasetReadinessPanel?.scrollIntoView({ behavior: "smooth", block: "center" });
        setTimeout(() => datasetReadinessPanel?.classList.remove("is-refused"), 3000);
    }

    function setupReadinessControls() {
        btnAuditReadiness?.addEventListener("click", () => auditDatasetReadiness({ force: true }));
        datasetReadinessIssues?.addEventListener("click", (e) => {
            const button = e.target.closest("[data-fix]");
            if (!button) return;
            e.preventDefault();
            fixDatasetIssueKind(button.dataset.issueCode);
        });
    }

    // ==========================================================================
    // Frozen Dataset Snapshots (Issue #216)
    // ==========================================================================

    /**
     * A refused freeze carries the full audit, so the drawer renders it and the operator fixes the
     * flagged maps in place. No separate modal: the readiness block outlives the refusal.
     */
    function showFreezeRefusal(refusal, scopeTheme) {
        const readiness = refusal && refusal.readiness;
        const error = (refusal && refusal.error) || "Dataset is not ready";
        // The refusal audited the scope that was frozen, which the operator may have left since:
        // store it under that scope, never under whichever tag happens to be active now.
        const scopeKey = (scopeTheme || "").toLowerCase() || "all";
        if (readiness && Array.isArray(readiness.maps)) {
            applyReadinessReport(scopeKey, readiness, new Date().toISOString());
        } else if (scopeKey === readinessScopeKey()) {
            // The refusal already invalidated the frozen scope, which is still the one on screen.
            clearReadinessCache();
            state.readinessReport = null;
            state.readinessAuditedAt = null;
        }
        if (scopeKey !== readinessScopeKey()) {
            showToast(`Freeze refused: ${error}`, "danger");
            return;
        }
        state.readinessRefusal = { error, blocking: (refusal && refusal.blocking_issues) || [] };
        renderReadinessPanel();
        revealReadinessPanel();
        showToast(`Freeze refused: ${error}`, "danger");
    }

    async function fetchDatasetSnapshots() {
        if (state.isFetchingSnapshots) return;
        state.isFetchingSnapshots = true;
        try {
            const res = await fetch(`/api/training/datasets${datasetScopeQuery()}`);
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            const snapshots = await res.json();
            state.snapshots = snapshots;
            renderDatasetSnapshots(snapshots);
            populateTrainingSnapshotSelect(snapshots);
            checkActiveFreezeStatus();
        } catch (err) {
            console.error("Failed to fetch dataset snapshots:", err);
        } finally {
            state.isFetchingSnapshots = false;
        }
    }

    function renderDatasetSnapshots(snapshots) {
        if (!snapshotsList || !snapshotsCountBadge) return;
        const total = (snapshots && snapshots.length) || 0;
        snapshotsCountBadge.textContent = `${total} Snapshot${total === 1 ? "" : "s"}`;

        if (!snapshots || snapshots.length === 0) {
            snapshotsList.innerHTML = "";
            snapshotsEmpty?.classList.remove("hidden");
            return;
        }

        snapshotsEmpty?.classList.add("hidden");
        snapshotsList.innerHTML = snapshots.map(s => {
            const id = escapeHtml(s.id);
            const themeLabel = s.theme ? escapeHtml(s.theme) : "All Themes";
            const shaHead = s.sha256 ? escapeHtml(s.sha256.substring(0, 12)) : "";
            const shaFull = escapeHtml(s.sha256 || "");
            const counts = `${s.samples_count} sample${s.samples_count === 1 ? "" : "s"} (${s.maps_count} map${s.maps_count === 1 ? "" : "s"})`;
            const sizeStr = formatBytes(s.size_bytes);
            const dateStr = s.created_at ? new Date(s.created_at).toLocaleString() : "";
            const downloadUrl = `/api/training/datasets/${encodeURIComponent(s.id)}/archive`;

            return `
                <div class="snapshot-card" data-id="${id}">
                    <div class="snapshot-card-head">
                        <span class="snapshot-id mono" title="${id}">${id}</span>
                        <span class="snapshot-theme-badge">${themeLabel}</span>
                    </div>
                    <div class="snapshot-meta-row">
                        <span class="snapshot-sha256 mono" title="SHA-256: ${shaFull}">SHA: ${shaHead}…</span>
                        <span class="snapshot-counts mono">${counts}</span>
                        <span class="snapshot-size mono">${sizeStr}</span>
                    </div>
                    <div class="snapshot-meta-row">
                        <span class="snapshot-time mono">${escapeHtml(dateStr)}</span>
                    </div>
                    <div class="snapshot-card-actions">
                        <button type="button" class="btn btn-primary btn-xs btn-train-snapshot" data-id="${id}" title="Launch Training Run on this snapshot">
                            Train LoRA
                        </button>
                        <a href="${downloadUrl}" download="${id}.zip" class="btn btn-secondary btn-xs" title="Download ZIP Archive">
                            Download ZIP
                        </a>
                        <button type="button" class="btn btn-danger btn-xs btn-delete-snapshot" data-id="${id}" title="Delete Snapshot">
                            Delete
                        </button>
                    </div>
                </div>
            `;
        }).join("");

        snapshotsList.querySelectorAll(".btn-delete-snapshot").forEach(btn => {
            btn.addEventListener("click", () => {
                const id = btn.getAttribute("data-id");
                if (id) deleteDatasetSnapshot(id);
            });
        });
        snapshotsList.querySelectorAll(".btn-train-snapshot").forEach(btn => {
            btn.addEventListener("click", () => {
                const id = btn.getAttribute("data-id");
                if (id) openTrainingDrawer({ snapshotId: id });
            });
        });
        populateTrainingSnapshotSelect(snapshots);
    }

    function toggleSnapshotsShelf() {
        state.snapshotsCollapsed = !state.snapshotsCollapsed;
        if (snapshotsShelf) {
            snapshotsShelf.classList.toggle("collapsed", state.snapshotsCollapsed);
        }
        if (btnToggleSnapshots) {
            btnToggleSnapshots.setAttribute("aria-expanded", String(!state.snapshotsCollapsed));
        }
        if (toggleSnapshotsText) {
            toggleSnapshotsText.textContent = state.snapshotsCollapsed ? "Expand" : "Collapse";
        }
    }

    function showFreezeProgressPanel(show) {
        if (!freezeProgressPanel) return;
        if (show) {
            freezeProgressPanel.classList.remove("hidden");
        } else {
            freezeProgressPanel.classList.add("hidden");
        }
    }

    function updateFreezeProgressUI(progress) {
        if (!progress) return;
        showFreezeProgressPanel(true);

        const phase = (progress.phase || "starting").toLowerCase();
        if (freezePhaseBadge) {
            freezePhaseBadge.textContent = phase.toUpperCase();
            freezePhaseBadge.className = `freeze-phase-badge phase-${phase}`;
        }

        if (freezeProgressMessage) {
            freezeProgressMessage.textContent = progress.message || "Processing dataset snapshot...";
        }

        let pct = 0;
        let countsText = "";
        if (phase === "starting") {
            pct = 5;
            countsText = "Initializing...";
        } else if (phase === "auditing") {
            const completed = progress.completed_maps || 0;
            const total = progress.total_maps || 1;
            pct = Math.min(25, Math.round((completed / Math.max(total, 1)) * 25));
            countsText = `${completed} / ${total} maps`;
            if (btnFreezeLabel) btnFreezeLabel.textContent = `Auditing (${completed}/${total})`;
        } else if (phase === "packaging") {
            const completed = progress.completed_samples || 0;
            const total = progress.total_samples || 1;
            pct = 25 + Math.min(70, Math.round((completed / Math.max(total, 1)) * 70));
            countsText = `${completed} / ${total} samples`;
            if (btnFreezeLabel) btnFreezeLabel.textContent = `Freezing (${completed}/${total})`;
        } else if (phase === "finalizing") {
            pct = 95;
            countsText = "Saving artifact...";
            if (btnFreezeLabel) btnFreezeLabel.textContent = "Finalizing...";
        } else if (phase === "complete") {
            pct = 100;
            countsText = "Complete";
            if (btnFreezeLabel) btnFreezeLabel.textContent = "Complete!";
        } else if (phase === "error") {
            countsText = "Error";
            if (btnFreezeLabel) btnFreezeLabel.textContent = "Freeze Snapshot";
        }

        if (freezeProgressBar) {
            freezeProgressBar.style.width = `${pct}%`;
        }
        if (freezeProgressCounts) {
            freezeProgressCounts.textContent = countsText;
        }
        if (freezeProgressPercent) {
            freezeProgressPercent.textContent = `(${pct}%)`;
        }

        if (freezeDetailMap) {
            freezeDetailMap.textContent = progress.current_map ? `Map: ${progress.current_map}` : "";
        }
        if (freezeDetailSection) {
            freezeDetailSection.textContent = progress.current_section
                ? `Section: (${progress.current_section[0]}, ${progress.current_section[1]})`
                : "";
        }
    }

    let freezePollingTimer = null;
    function startFreezeStatusPolling() {
        if (freezePollingTimer) return;
        freezePollingTimer = setInterval(async () => {
            try {
                const res = await fetch("/api/training/datasets/freeze/status");
                if (!res.ok) return;
                const data = await res.json();
                if (data.in_progress && data.progress) {
                    state.isFreezingSnapshot = true;
                    if (btnFreezeSnapshot) btnFreezeSnapshot.disabled = true;
                    updateFreezeProgressUI(data.progress);
                } else {
                    stopFreezeStatusPolling();
                    state.isFreezingSnapshot = false;
                    if (btnFreezeSnapshot) btnFreezeSnapshot.disabled = false;
                    if (btnFreezeLabel) btnFreezeLabel.textContent = "Freeze Snapshot";
                    showFreezeProgressPanel(false);
                    fetchDatasetSnapshots();
                }
            } catch (_) {}
        }, 1500);
    }

    function stopFreezeStatusPolling() {
        if (freezePollingTimer) {
            clearInterval(freezePollingTimer);
            freezePollingTimer = null;
        }
    }

    async function checkActiveFreezeStatus() {
        try {
            const res = await fetch("/api/training/datasets/freeze/status");
            if (!res.ok) return;
            const data = await res.json();
            if (data.in_progress && data.progress) {
                state.isFreezingSnapshot = true;
                if (btnFreezeSnapshot) btnFreezeSnapshot.disabled = true;
                updateFreezeProgressUI(data.progress);
                startFreezeStatusPolling();
            }
        } catch (_) {}
    }

    async function performFreezeSnapshotWithProgress(scopeTheme, onProgress) {
        const body = scopeTheme ? { theme: scopeTheme } : {};
        const query = scopeTheme ? `?theme=${encodeURIComponent(scopeTheme)}` : "";
        const res = await fetch(`/api/training/datasets${query}`, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                "Accept": "text/event-stream",
            },
            body: JSON.stringify(body),
        });

        if (!res.ok) {
            const errData = await res.json().catch(() => ({}));
            errData.status = res.status;
            throw errData;
        }

        const contentType = res.headers.get("content-type") || "";
        if (contentType.includes("text/event-stream") && res.body) {
            const reader = res.body.getReader();
            const decoder = new TextDecoder();
            let buffer = "";
            let finalSnapshot = null;
            let currentEvent = "message";

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                buffer += decoder.decode(value, { stream: true });

                const lines = buffer.split("\n");
                buffer = lines.pop();

                for (const line of lines) {
                    const trimmed = line.trim();
                    if (trimmed.startsWith("event:")) {
                        currentEvent = trimmed.slice(6).trim();
                    } else if (trimmed.startsWith("data:")) {
                        const dataStr = trimmed.slice(5).trim();
                        try {
                            const data = JSON.parse(dataStr);
                            if (currentEvent === "progress") {
                                onProgress?.("progress", data);
                            } else if (currentEvent === "complete") {
                                finalSnapshot = data.snapshot || data;
                                onProgress?.("complete", data);
                            } else if (currentEvent === "error") {
                                onProgress?.("error", data);
                                throw data;
                            }
                        } catch (e) {
                            if (currentEvent === "error") throw e;
                        }
                        currentEvent = "message";
                    }
                }
            }

            if (!finalSnapshot) {
                throw new Error("Freezing completed without receiving snapshot metadata");
            }
            return finalSnapshot;
        } else {
            const snapshot = await res.json();
            onProgress?.("complete", { snapshot });
            return snapshot;
        }
    }

    async function freezeDatasetSnapshot() {
        if (state.isFreezingSnapshot) return;
        state.isFreezingSnapshot = true;
        if (btnFreezeSnapshot) btnFreezeSnapshot.disabled = true;
        if (btnFreezeLabel) btnFreezeLabel.textContent = "Freezing...";

        const scopeTheme = datasetScopeTheme();
        const scopeLabel = scopeTheme ? `theme '${scopeTheme}'` : "all themes";
        showToast(`Auditing readiness & freezing snapshot (${scopeLabel})...`, "info");
        showFreezeProgressPanel(true);
        updateFreezeProgressUI({
            phase: "starting",
            completed_maps: 0,
            total_maps: 0,
            completed_samples: 0,
            total_samples: 0,
            message: `Auditing readiness & freezing snapshot (${scopeLabel})...`,
        });

        try {
            const snapshot = await performFreezeSnapshotWithProgress(scopeTheme, (event, data) => {
                if (event === "progress") {
                    updateFreezeProgressUI(data);
                } else if (event === "complete") {
                    updateFreezeProgressUI({
                        phase: "complete",
                        completed_maps: data.completed_maps,
                        total_maps: data.total_maps,
                        completed_samples: data.completed_samples,
                        total_samples: data.total_samples,
                        message: `Snapshot created: ${data.snapshot?.id || ""}`,
                        snapshot: data.snapshot,
                    });
                }
            });

            showToast(`Frozen Dataset Snapshot '${snapshot.id}' created (${snapshot.samples_count} samples, ${snapshot.maps_count} maps)!`, "success");
            setTimeout(() => {
                showFreezeProgressPanel(false);
            }, 2500);
            fetchDatasetSnapshots();
        } catch (errData) {
            showFreezeProgressPanel(false);
            if (errData?.status === 400 || errData?.status === 422 || errData?.code === "DATASET_NOT_READY") {
                showFreezeRefusal(errData, scopeTheme);
                return;
            }
            if (errData?.status === 409) {
                showToast(errData.error || "A dataset snapshot freeze is already in progress", "warning");
                startFreezeStatusPolling();
                return;
            }
            showToast(`Freeze error: ${errData?.message || errData?.error || "Unknown error"}`, "danger");
        } finally {
            state.isFreezingSnapshot = false;
            if (btnFreezeSnapshot) btnFreezeSnapshot.disabled = false;
            if (btnFreezeLabel) btnFreezeLabel.textContent = "Freeze Snapshot";
        }
    }

    async function deleteDatasetSnapshot(id) {
        if (!id) return;
        if (!confirm(`Are you sure you want to permanently delete frozen dataset snapshot '${id}'? This will remove its archive from blob storage.`)) {
            return;
        }
        try {
            const res = await fetch(`/api/training/datasets/${encodeURIComponent(id)}`, {
                method: "DELETE",
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                if (res.status === 409 || errData.code === "SNAPSHOT_IN_USE") {
                    showToast(`Cannot delete snapshot: ${errData.error || "Snapshot is in use by one or more training runs"}`, "danger");
                    return;
                }
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            showToast(`Dataset snapshot '${id}' deleted`, "info");
            fetchDatasetSnapshots();
        } catch (err) {
            showToast(`Failed to delete snapshot: ${err.message}`, "danger");
        }
    }

    // ==========================================================================
    // LoRA Training Runs & Adapter Studio (Issue #209, ADR-0009, ADR-0010)
    // ==========================================================================

    function toggleTrainingDrawer() {
        if (state.trainingDrawerOpen) {
            closeTrainingDrawer();
        } else {
            openTrainingDrawer();
        }
    }

    async function openTrainingDrawer(options = {}) {
        stopPanLoop();
        stopZoomLoop();
        if (state.helpModalOpen) closeHelpModal();
        if (state.drawerOpen) closeIngestionDrawer();
        if (state.benchmarkDrawerOpen) closeBenchmarkDrawer();

        state.trainingDrawerOpen = true;
        trainingDrawer?.classList.remove("hidden");

        const theme = datasetScopeTheme();
        if (trainingScopeHint) {
            trainingScopeHint.textContent = theme ? `Target theme scope: '${theme}'` : "Target scope: All themes";
        }

        if (options.tab) {
            switchTrainingTab(options.tab);
        }

        await Promise.all([
            fetchTrainingRuns(),
            fetchTrainingAdapters(),
            fetchDatasetSnapshots(),
            checkTrainingScopeReadiness(),
        ]);

        if (options.snapshotId && trainingSnapshotSelect) {
            trainingSnapshotSelect.value = options.snapshotId;
            setLaunchFormCollapsed(false);
        }
    }

    function closeTrainingDrawer() {
        stopPanLoop();
        stopZoomLoop();
        state.trainingDrawerOpen = false;
        trainingDrawer?.classList.add("hidden");
        canvas?.focus();
    }

    function switchTrainingTab(tab) {
        state.trainingActiveTab = tab;
        const isRuns = tab === "runs";
        tabTrainingRuns?.classList.toggle("active", isRuns);
        tabTrainingRuns?.setAttribute("aria-selected", String(isRuns));
        tabTrainingAdapters?.classList.toggle("active", !isRuns);
        tabTrainingAdapters?.setAttribute("aria-selected", String(!isRuns));

        trainingViewRuns?.classList.toggle("hidden", !isRuns);
        trainingViewAdapters?.classList.toggle("hidden", isRuns);

        if (!isRuns) {
            fetchTrainingAdapters();
        }
    }

    function setLaunchFormCollapsed(collapsed) {
        state.launchFormCollapsed = collapsed;
        if (trainingLaunchForm) {
            trainingLaunchForm.style.display = collapsed ? "none" : "";
        }
        if (btnToggleLaunchText) {
            btnToggleLaunchText.textContent = collapsed ? "Expand" : "Collapse";
        }
    }

    async function checkTrainingScopeReadiness() {
        if (!trainingReadinessIndicator) return;
        trainingReadinessIndicator.textContent = "Auditing...";
        trainingReadinessIndicator.className = "gate-status mono";

        try {
            const res = await fetch(`/api/dataset/readiness${datasetScopeQuery()}`);
            if (!res.ok) {
                trainingReadinessIndicator.textContent = `Error ${res.status}`;
                trainingReadinessIndicator.className = "gate-status mono has-issues";
                return null;
            }
            const report = await res.json();
            const total = report.maps_total || 0;
            const ready = Boolean(report.ready);
            if (ready) {
                trainingReadinessIndicator.textContent = `Ready (${report.sections_total || 0} sections / ${total} maps)`;
                trainingReadinessIndicator.className = "gate-status mono is-ready";
            } else {
                trainingReadinessIndicator.textContent = `Not Ready (${report.maps_ready || 0}/${total} maps)`;
                trainingReadinessIndicator.className = "gate-status mono has-issues";
            }
            return report;
        } catch (err) {
            trainingReadinessIndicator.textContent = "Offline";
            trainingReadinessIndicator.className = "gate-status mono has-issues";
            return null;
        }
    }

    function populateTrainingSnapshotSelect(snapshots) {
        if (!trainingSnapshotSelect) return;
        const currentVal = trainingSnapshotSelect.value;
        const currentTheme = datasetScopeTheme();
        const themeLabel = currentTheme ? `Theme '${currentTheme}'` : "All Themes";

        let html = `<option value="__freeze_current__">⚡ Freeze Current Scope Now (${themeLabel})</option>`;
        if (Array.isArray(snapshots) && snapshots.length > 0) {
            for (const s of snapshots) {
                const sTheme = s.theme ? s.theme : "All Themes";
                const shaShort = s.sha256 ? s.sha256.substring(0, 8) : "";
                html += `<option value="${escapeHtml(s.id)}">Snapshot: ${escapeHtml(s.id)} (${escapeHtml(sTheme)}, ${s.samples_count} samples, SHA: ${shaShort}…)</option>`;
            }
        }
        trainingSnapshotSelect.innerHTML = html;
        if (currentVal && Array.from(trainingSnapshotSelect.options).some(o => o.value === currentVal)) {
            trainingSnapshotSelect.value = currentVal;
        }
    }

    function showTrainingLaunchError(errorObj) {
        if (!trainingLaunchError) return;
        if (!errorObj) {
            trainingLaunchError.classList.add("hidden");
            trainingLaunchError.innerHTML = "";
            return;
        }

        trainingLaunchError.classList.remove("hidden");
        const code = errorObj.code || "ERROR";
        const message = errorObj.error || errorObj.message || "Failed to launch training run";
        const blocking = errorObj.blocking_issues || [];

        let inner = `<div style="font-weight: 700; margin-bottom: 0.25rem;">[${escapeHtml(code)}] ${escapeHtml(message)}</div>`;
        if (blocking.length > 0) {
            inner += `<div style="font-size: 0.72rem; margin-top: 0.35rem; color: #fca5a5;">Actionable remediations required:</div>`;
            inner += `<ul style="margin: 0.25rem 0 0 1rem; padding: 0;">`;
            for (const b of blocking) {
                const bCode = datasetIssueLabel ? datasetIssueLabel(b.code) : b.code;
                inner += `<li><strong>${escapeHtml(bCode)}:</strong> ${escapeHtml(b.remediation || b.code)}</li>`;
            }
            inner += `</ul>`;
        }
        trainingLaunchError.innerHTML = inner;
    }

    async function startTrainingRun() {
        if (state.isLaunchingRun) return;
        state.isLaunchingRun = true;
        showTrainingLaunchError(null);

        if (btnStartTrainingRun) btnStartTrainingRun.disabled = true;
        if (btnStartRunText) btnStartRunText.textContent = "Launching Run...";

        try {
            let snapshotId = trainingSnapshotSelect?.value || "__freeze_current__";
            const policy = state.trainingSelectedPolicy || "calibration";

            // If user selected to freeze current scope inline:
            if (snapshotId === "__freeze_current__") {
                const scopeTheme = datasetScopeTheme();
                const scopeLabel = scopeTheme ? `theme '${scopeTheme}'` : "all themes";
                showToast(`Auditing readiness & freezing snapshot for ${scopeLabel}...`, "info");
                if (trainingScopeHint) trainingScopeHint.textContent = `Freezing snapshot for ${scopeLabel}...`;
                showFreezeProgressPanel(true);

                try {
                    const createdSnapshot = await performFreezeSnapshotWithProgress(scopeTheme, (event, data) => {
                        updateFreezeProgressUI(data);
                        if (trainingScopeHint && data.message) {
                            trainingScopeHint.textContent = data.message;
                        }
                    });
                    snapshotId = createdSnapshot.id;
                    showToast(`Created snapshot '${snapshotId}'! Proceeding to training run...`, "info");
                    fetchDatasetSnapshots();
                    setTimeout(() => showFreezeProgressPanel(false), 2000);
                } catch (errData) {
                    showFreezeProgressPanel(false);
                    if (trainingScopeHint) trainingScopeHint.textContent = "Target: Current active theme scope.";
                    if (errData?.status === 422 || errData?.code === "DATASET_NOT_READY") {
                        showTrainingLaunchError(errData);
                        showFreezeRefusal(errData, scopeTheme);
                        return;
                    }
                    throw new Error(errData?.error || errData?.message || "Failed to freeze snapshot");
                }
            }

            // Create the queued training run
            const runRes = await fetch("/api/training/runs", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    dataset_snapshot_id: snapshotId,
                    policy: policy,
                }),
            });

            if (!runRes.ok) {
                const errData = await runRes.json().catch(() => ({}));
                showTrainingLaunchError(errData);
                throw new Error(errData.error || `HTTP ${runRes.status}`);
            }

            const queuedRun = await runRes.json();
            showToast(`Created training run '${queuedRun.id}' in queued state. Dispatching to runner...`, "info");

            // Attempt immediate dispatch
            const dispatchRes = await fetch(`/api/training/runs/${encodeURIComponent(queuedRun.id)}/dispatch`, {
                method: "POST",
            });

            if (!dispatchRes.ok) {
                const errData = await dispatchRes.json().catch(() => ({}));
                showTrainingLaunchError(errData);
                if (errData.code === "RUN_ALREADY_ACTIVE") {
                    showToast(`Run queued as '${queuedRun.id}', but dispatch was refused: another run '${errData.active_run_id}' is active.`, "warning");
                } else if (errData.code === "MODAL_NOT_CONFIGURED") {
                    showToast(`Run queued as '${queuedRun.id}', but runner is unconfigured: ${errData.error}`, "warning");
                } else {
                    showToast(`Run queued as '${queuedRun.id}', but dispatch failed: ${errData.error || dispatchRes.status}`, "warning");
                }
            } else {
                const activeRun = await dispatchRes.json();
                showToast(`Training run '${activeRun.id}' dispatched successfully to Modal GPU runner!`, "success");
            }

            await fetchTrainingRuns();
            checkAndScheduleTrainingPoll();
        } catch (err) {
            showToast(`Run launch failed: ${err.message}`, "danger");
        } finally {
            state.isLaunchingRun = false;
            if (btnStartTrainingRun) btnStartTrainingRun.disabled = false;
            if (btnStartRunText) btnStartRunText.textContent = "Launch Training Run";
        }
    }

    async function dispatchTrainingRun(runId) {
        if (!runId) return;
        showToast(`Dispatching training run '${runId}'...`, "info");
        try {
            const res = await fetch(`/api/training/runs/${encodeURIComponent(runId)}/dispatch`, {
                method: "POST",
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                if (errData.code === "RUN_ALREADY_ACTIVE") {
                    showToast(`Dispatch refused: another run '${errData.active_run_id}' is currently running.`, "danger");
                } else if (errData.code === "MODAL_NOT_CONFIGURED") {
                    showToast(`Dispatch refused: ${errData.error}`, "danger");
                } else {
                    showToast(`Dispatch error: ${errData.error || `HTTP ${res.status}`}`, "danger");
                }
                return;
            }
            const updated = await res.json();
            showToast(`Training run '${updated.id}' dispatched!`, "success");
            await fetchTrainingRuns();
            checkAndScheduleTrainingPoll();
        } catch (err) {
            showToast(`Failed to dispatch run: ${err.message}`, "danger");
        }
    }

    async function cancelTrainingRun(runId) {
        if (!runId) return;
        if (!confirm(`Cancel training run '${runId}'? Any in-flight GPU execution will be terminated.`)) {
            return;
        }

        try {
            const res = await fetch(`/api/training/runs/${encodeURIComponent(runId)}/cancel`, {
                method: "POST",
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }

            // Immediately reflect cancelled state in UI
            const localRun = state.trainingRuns.find(r => r.id === runId);
            if (localRun) {
                localRun.state = "cancelled";
                renderTrainingRuns();
            }

            showToast(`Training run '${runId}' cancelled.`, "info");
            await fetchTrainingRuns();
            checkAndScheduleTrainingPoll();
        } catch (err) {
            showToast(`Failed to cancel run: ${err.message}`, "danger");
        }
    }

    async function verifyTrainingArtifacts(runId) {
        if (!runId) return;
        showToast(`Verifying storage artifacts for run '${runId}'...`, "info");
        try {
            const res = await fetch(`/api/training/runs/${encodeURIComponent(runId)}/verify`, {
                method: "POST",
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            const updated = await res.json();
            showToast(`Artifact verification complete for run '${runId}'!`, "success");
            await fetchTrainingRuns();
        } catch (err) {
            showToast(`Artifact verification failed: ${err.message}`, "danger");
        }
    }

    async function publishTrainingCheckpoint(runId, step, scale) {
        if (!runId) return;
        const scaleVal = (scale != null && !isNaN(scale) && scale > 0) ? Number(scale) : 0.75;
        showToast(`Publishing adapter for run '${runId}' at step ${step} (scale ${scaleVal})...`, "info");

        try {
            const res = await fetch(`/api/training/runs/${encodeURIComponent(runId)}/publish`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ step: Number(step), scale: scaleVal }),
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            const adapter = await res.json();
            showToast(`LoRA Adapter '${adapter.id}' published! Available in Benchmark Drawer.`, "success");

            await Promise.all([
                fetchTrainingRuns(),
                fetchTrainingAdapters(),
                loadBenchmarkModelCatalog(),
            ]);
        } catch (err) {
            showToast(`Failed to publish adapter: ${err.message}`, "danger");
        }
    }

    async function setTrainingAdapterRetired(adapterId, retired) {
        if (!adapterId) return;
        const verb = retired ? "Retiring" : "Restoring";
        showToast(`${verb} adapter '${adapterId}'...`, "info");

        try {
            const res = await fetch(`/api/training/adapters/${encodeURIComponent(adapterId)}/retire`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ retired: Boolean(retired) }),
            });
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            const adapter = await res.json();
            const msg = adapter.retired
                ? `LoRA Adapter '${adapter.id}' retired (hidden from Benchmark Drawer).`
                : `LoRA Adapter '${adapter.id}' restored (selectable in Benchmark Drawer).`;
            showToast(msg, "success");

            await Promise.all([
                fetchTrainingAdapters(),
                loadBenchmarkModelCatalog(),
            ]);
        } catch (err) {
            showToast(`Failed to update adapter: ${err.message}`, "danger");
        }
    }

    async function fetchTrainingRuns() {
        if (state.isFetchingRuns) return;
        state.isFetchingRuns = true;

        try {
            const res = await fetch("/api/training/runs");
            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }
            const runs = await res.json();
            state.trainingRuns = Array.isArray(runs) ? runs : [];

            const total = state.trainingRuns.length;
            if (trainingRunsCountTab) trainingRunsCountTab.textContent = String(total);
            if (trainingRunsTotalBadge) trainingRunsTotalBadge.textContent = String(total);

            // Active badge
            const activeCount = state.trainingRuns.filter(r => r.state === "running" || r.state === "queued").length;
            if (trainingRunsActiveBadge) {
                if (activeCount > 0) {
                    trainingRunsActiveBadge.textContent = `${activeCount} Active`;
                    trainingRunsActiveBadge.classList.remove("hidden");
                } else {
                    trainingRunsActiveBadge.classList.add("hidden");
                }
            }

            renderTrainingRuns();
            checkAndScheduleTrainingPoll();
        } catch (err) {
            console.error("Failed to fetch training runs:", err);
        } finally {
            state.isFetchingRuns = false;
        }
    }

    async function fetchTrainingAdapters() {
        if (state.isFetchingAdapters) return;
        state.isFetchingAdapters = true;

        try {
            const res = await fetch("/api/training/adapters");
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const adapters = await res.json();
            state.trainingAdapters = Array.isArray(adapters) ? adapters : [];

            const total = state.trainingAdapters.length;
            if (trainingAdaptersCountTab) trainingAdaptersCountTab.textContent = String(total);
            if (adaptersCountBadge) adaptersCountBadge.textContent = `${total} Adapter${total === 1 ? "" : "s"}`;

            renderTrainingAdapters();
        } catch (err) {
            console.error("Failed to fetch training adapters:", err);
        } finally {
            state.isFetchingAdapters = false;
        }
    }

    function checkAndScheduleTrainingPoll() {
        const hasActive = state.trainingRuns.some(r => r.state === "running" || r.state === "queued");
        if (hasActive) {
            if (!trainingPollTimer) {
                trainingPollTimer = setInterval(pollTrainingState, 3000);
            }
        } else {
            if (trainingPollTimer) {
                clearInterval(trainingPollTimer);
                trainingPollTimer = null;
            }
        }
    }

    async function pollTrainingState() {
        try {
            await fetchTrainingRuns();
            await fetchCostStats();
        } catch (_) {}
    }

    function renderTrainingRuns() {
        if (!trainingRunsList) return;
        const filter = state.trainingRunsFilter || "all";
        const filtered = state.trainingRuns.filter(r => {
            if (filter === "active") return r.state === "running" || r.state === "queued";
            if (filter === "succeeded") return r.state === "succeeded";
            if (filter === "failed") return r.state === "failed";
            if (filter === "cancelled") return r.state === "cancelled";
            return true;
        });

        if (filtered.length === 0) {
            trainingRunsList.innerHTML = "";
            trainingRunsEmpty?.classList.remove("hidden");
            return;
        }

        trainingRunsEmpty?.classList.add("hidden");
        trainingRunsList.innerHTML = filtered.map(r => {
            const id = escapeHtml(r.id);
            const stateLower = (r.state || "queued").toLowerCase();
            const badgeClass = `badge-status-${stateLower}`;
            const policy = r.recipe?.policy || "calibration";
            const steps = r.step || 0;
            const totalSteps = r.total_steps || (policy === "full" ? 4000 : 1500);
            const pct = Math.min(100, Math.round((steps / (totalSteps || 1)) * 100));
            const costStr = Number(r.cost_usd || 0).toFixed(2);
            const elapsedStr = formatDuration(r.elapsed_seconds || 0);
            const snapId = escapeHtml(r.dataset_snapshot_id || "—");
            const snapDigest = r.dataset_digest ? escapeHtml(r.dataset_digest.substring(0, 10)) : "—";
            const isQueued = stateLower === "queued";
            const isRunning = stateLower === "running";
            const isSucceeded = stateLower === "succeeded";
            const isFailed = stateLower === "failed";

            let errorHtml = "";
            if (isFailed && (r.error || r.failure_code)) {
                const codePrefix = r.failure_code ? `[${escapeHtml(r.failure_code)}] ` : "";
                errorHtml = `<div class="run-error-box">${codePrefix}${escapeHtml(r.error || "Training run failed")}</div>`;
            }
            let pollErrorHtml = "";
            if (r.last_poll_error) {
                pollErrorHtml = `<div class="run-poll-error-pill">⚠️ Transient Poll Notice: ${escapeHtml(r.last_poll_error)}</div>`;
            }

            let checkpointsHtml = "";
            if (Array.isArray(r.checkpoints) && r.checkpoints.length > 0) {
                const cpCards = r.checkpoints.map(cp => {
                    const cpStep = cp.step;
                    const vStatus = (cp.artifact_verification?.status || "pending").toLowerCase();
                    const vClass = `badge-verif-${vStatus}`;
                    const thumbHtml = cp.sample_grid_url
                        ? `<a href="${escapeHtml(cp.sample_grid_url)}" target="_blank" rel="noopener noreferrer" title="View full sample grid"><img src="${escapeHtml(cp.sample_grid_url)}" class="checkpoint-sample-thumb" alt="Checkpoint step ${cpStep} sample grid" /></a>`
                        : `<div class="checkpoint-no-sample">No Sample Grid</div>`;

                    const canonicalAdapterId = `${r.id}-s${cpStep}`;
                    const isPublished = (r.published_adapter_ids || []).includes(canonicalAdapterId) ||
                        state.trainingAdapters.some(a => a.id === canonicalAdapterId);

                    let publishHtml = "";
                    if (isPublished) {
                        publishHtml = `<span class="badge badge-published" title="Adapter published to catalog">Published (${escapeHtml(canonicalAdapterId)})</span>`;
                    } else {
                        publishHtml = `
                            <div class="checkpoint-publish-form">
                                <label class="scale-label">
                                    <span>Scale:</span>
                                    <input type="number" min="0.1" max="2.0" step="0.05" value="0.75" class="checkpoint-scale-input" id="scale-input-${id}-${cpStep}">
                                </label>
                                <button type="button" class="btn btn-primary btn-xs btn-publish-checkpoint" data-run-id="${id}" data-step="${cpStep}">
                                    Publish Adapter
                                </button>
                            </div>
                        `;
                    }

                    return `
                        <div class="checkpoint-card">
                            <div class="checkpoint-head">
                                <span class="checkpoint-step mono">Step ${cpStep}</span>
                                <span class="${vClass}">${vStatus}</span>
                            </div>
                            ${thumbHtml}
                            ${publishHtml}
                        </div>
                    `;
                }).join("");

                checkpointsHtml = `
                    <div class="run-checkpoints-section">
                        <div class="checkpoints-header">Checkpoints (${r.checkpoints.length})</div>
                        <div class="checkpoints-grid">${cpCards}</div>
                    </div>
                `;
            }

            let actionBtns = "";
            if (isQueued) {
                actionBtns += `<button type="button" class="btn btn-primary btn-xs btn-dispatch-run" data-run-id="${id}">Dispatch Run</button>`;
                actionBtns += `<button type="button" class="btn btn-danger btn-xs btn-cancel-run" data-run-id="${id}">Cancel Run</button>`;
            } else if (isRunning) {
                actionBtns += `<button type="button" class="btn btn-danger btn-xs btn-cancel-run" data-run-id="${id}">Cancel Run</button>`;
            }
            if (isSucceeded || (r.checkpoints && r.checkpoints.length > 0)) {
                actionBtns += `<button type="button" class="btn btn-secondary btn-xs btn-verify-run" data-run-id="${id}">Verify Artifacts</button>`;
            }

            return `
                <div class="training-run-card" data-id="${id}">
                    <div class="run-head-row">
                        <div class="run-id-wrap">
                            <span class="run-id mono" title="${id}">${id}</span>
                            <span class="policy-badge mono">${escapeHtml(policy)} (${totalSteps} steps)</span>
                        </div>
                        <div class="run-badges-wrap">
                            <span class="${badgeClass}">${stateLower}</span>
                        </div>
                    </div>

                    <div class="run-progress-row">
                        <div class="run-progress-track">
                            <div class="run-progress-bar" style="width: ${pct}%;"></div>
                        </div>
                        <div class="run-progress-labels">
                            <span class="mono">${steps} / ${totalSteps} steps (${pct}%)</span>
                            <span class="mono">Outcome: ${stateLower}</span>
                        </div>
                    </div>

                    <div class="run-telemetry-grid">
                        <div class="telemetry-cell">
                            <span class="telemetry-label">Elapsed</span>
                            <span class="telemetry-val mono">${elapsedStr}</span>
                        </div>
                        <div class="telemetry-cell">
                            <span class="telemetry-label">Cost (USD)</span>
                            <span class="telemetry-val mono">$${costStr}</span>
                        </div>
                        <div class="telemetry-cell">
                            <span class="telemetry-label">Snapshot</span>
                            <span class="telemetry-val mono" title="${snapId}">${snapId}</span>
                        </div>
                        <div class="telemetry-cell">
                            <span class="telemetry-label">Dataset Digest</span>
                            <span class="telemetry-val mono" title="${r.dataset_digest || ''}">${snapDigest}…</span>
                        </div>
                    </div>

                    ${errorHtml}
                    ${pollErrorHtml}
                    ${checkpointsHtml}

                    <div class="run-actions-row">
                        ${actionBtns}
                    </div>
                </div>
            `;
        }).join("");

        // Attach run action event listeners
        trainingRunsList.querySelectorAll(".btn-dispatch-run").forEach(btn => {
            btn.addEventListener("click", () => {
                const runId = btn.getAttribute("data-run-id");
                if (runId) dispatchTrainingRun(runId);
            });
        });
        trainingRunsList.querySelectorAll(".btn-cancel-run").forEach(btn => {
            btn.addEventListener("click", () => {
                const runId = btn.getAttribute("data-run-id");
                if (runId) cancelTrainingRun(runId);
            });
        });
        trainingRunsList.querySelectorAll(".btn-verify-run").forEach(btn => {
            btn.addEventListener("click", () => {
                const runId = btn.getAttribute("data-run-id");
                if (runId) verifyTrainingArtifacts(runId);
            });
        });
        trainingRunsList.querySelectorAll(".btn-publish-checkpoint").forEach(btn => {
            btn.addEventListener("click", () => {
                const runId = btn.getAttribute("data-run-id");
                const step = btn.getAttribute("data-step");
                const scaleEl = document.getElementById(`scale-input-${runId}-${step}`);
                const scale = scaleEl ? Number(scaleEl.value) : 0.75;
                if (runId && step) publishTrainingCheckpoint(runId, step, scale);
            });
        });
    }

    function renderTrainingAdapters() {
        if (!trainingAdaptersList) return;
        const filter = state.trainingAdaptersFilter || "all";
        const filtered = state.trainingAdapters.filter(a => {
            if (filter === "usable") return !a.retired;
            if (filter === "retired") return Boolean(a.retired);
            return true;
        });

        if (filtered.length === 0) {
            trainingAdaptersList.innerHTML = "";
            trainingAdaptersEmpty?.classList.remove("hidden");
            return;
        }

        trainingAdaptersEmpty?.classList.add("hidden");
        trainingAdaptersList.innerHTML = filtered.map(a => {
            const id = escapeHtml(a.id);
            const isRetired = Boolean(a.retired);
            const statusBadge = isRetired
                ? `<span class="badge badge-retired">Retired</span>`
                : `<span class="badge badge-active">Active / Usable</span>`;
            const toggleBtn = isRetired
                ? `<button type="button" class="btn btn-secondary btn-xs btn-restore-adapter" data-id="${id}">Restore</button>`
                : `<button type="button" class="btn btn-danger btn-xs btn-retire-adapter" data-id="${id}">Retire</button>`;

            const baseModel = escapeHtml(a.base_model || "flux-3-image-klein-base");
            const phrase = escapeHtml(a.trigger_phrase || "dungeonforge battlemap");
            const scale = Number(a.scale != null ? a.scale : 0.75).toFixed(2);
            const runId = escapeHtml(a.run_id || "—");
            const step = a.step || 0;
            const snapId = escapeHtml(a.dataset_snapshot_id || "—");
            const shaHead = a.sha256 ? escapeHtml(a.sha256.substring(0, 12)) : "—";

            const benchNote = isRetired
                ? `<div class="adapter-benchmark-pill is-retired">Excluded from Benchmark Drawer model selection (retired)</div>`
                : `<div class="adapter-benchmark-pill">Selectable in Benchmark Drawer as <strong>${id}</strong> (T2I) and <strong>${id}-edit</strong> (Edit)</div>`;

            return `
                <div class="training-adapter-card" data-id="${id}">
                    <div class="adapter-head-row">
                        <div class="adapter-id-wrap">
                            <span class="adapter-id mono">${id}</span>
                            ${statusBadge}
                        </div>
                        <div class="adapter-actions">
                            ${toggleBtn}
                        </div>
                    </div>

                    <div class="adapter-meta-grid">
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">Base Model:</span>
                            <span class="adapter-meta-val mono">${baseModel}</span>
                        </div>
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">Trigger:</span>
                            <span class="adapter-meta-val">"${phrase}"</span>
                        </div>
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">Scale:</span>
                            <span class="adapter-meta-val mono">${scale}</span>
                        </div>
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">Run / Step:</span>
                            <span class="adapter-meta-val mono">${runId} (s${step})</span>
                        </div>
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">Snapshot:</span>
                            <span class="adapter-meta-val mono">${snapId}</span>
                        </div>
                        <div class="adapter-meta-item">
                            <span class="adapter-meta-label">SHA-256:</span>
                            <span class="adapter-meta-val mono" title="${a.sha256 || ''}">${shaHead}…</span>
                        </div>
                    </div>

                    ${benchNote}
                </div>
            `;
        }).join("");

        // Attach retire/restore listeners
        trainingAdaptersList.querySelectorAll(".btn-retire-adapter").forEach(btn => {
            btn.addEventListener("click", () => {
                const id = btn.getAttribute("data-id");
                if (id) setTrainingAdapterRetired(id, true);
            });
        });
        trainingAdaptersList.querySelectorAll(".btn-restore-adapter").forEach(btn => {
            btn.addEventListener("click", () => {
                const id = btn.getAttribute("data-id");
                if (id) setTrainingAdapterRetired(id, false);
            });
        });
    }

    function setupTrainingControls() {
        btnOpenTrainingDrawer?.addEventListener("click", () => openTrainingDrawer());
        tabNavTraining?.addEventListener("click", () => openTrainingDrawer());
        btnOpenTrainingFromLibrary?.addEventListener("click", () => openTrainingDrawer());
        btnCloseTrainingDrawer?.addEventListener("click", () => closeTrainingDrawer());
        btnCancelTrainingDrawer?.addEventListener("click", () => closeTrainingDrawer());

        trainingDrawer?.addEventListener("click", (e) => {
            if (e.target === trainingDrawer) closeTrainingDrawer();
        });

        tabTrainingRuns?.addEventListener("click", () => switchTrainingTab("runs"));
        tabTrainingAdapters?.addEventListener("click", () => switchTrainingTab("adapters"));

        btnToggleLaunchForm?.addEventListener("click", () => {
            setLaunchFormCollapsed(!state.launchFormCollapsed);
        });

        const policyOptions = document.querySelectorAll(".policy-option");
        policyOptions.forEach(opt => {
            opt.addEventListener("click", () => {
                const radio = opt.querySelector('input[type="radio"]');
                if (radio) {
                    radio.checked = true;
                    state.trainingSelectedPolicy = radio.value;
                    policyOptions.forEach(o => o.classList.toggle("active", o === opt));
                }
            });
        });

        btnCheckTrainingReadiness?.addEventListener("click", () => checkTrainingScopeReadiness());
        btnStartTrainingRun?.addEventListener("click", () => startTrainingRun());
        btnRefreshTrainingRuns?.addEventListener("click", () => fetchTrainingRuns());

        trainingRunsFilter?.addEventListener("change", (e) => {
            state.trainingRunsFilter = e.target.value;
            renderTrainingRuns();
        });

        btnRefreshTrainingAdapters?.addEventListener("click", () => fetchTrainingAdapters());
        trainingAdaptersFilter?.addEventListener("change", (e) => {
            state.trainingAdaptersFilter = e.target.value;
            renderTrainingAdapters();
        });
    }

    // ==========================================================================
    // Deletion Safeguard Modal & Multi-Select Bulk Operations (Issue #137)
    // ==========================================================================

    function openDeleteCuratedModal(mapId) {
        if (!mapId) return;
        state.deleteModalOpen = true;
        state.deleteModalTarget = { type: "single", id: mapId };

        if (deleteCuratedTitle) {
            deleteCuratedTitle.textContent = "Delete & Reject Curated Map";
        }
        if (deleteTargetPreview) {
            deleteTargetPreview.innerHTML = `<span class="delete-target-id mono">${escapeHtml(mapId)}</span>`;
        }
        if (deleteReasonSelect) {
            deleteReasonSelect.value = "unspecified";
        }
        if (deleteNotesInput) {
            deleteNotesInput.value = "";
        }
        if (deleteNotesGroup) {
            deleteNotesGroup.style.display = "";
        }
        if (deleteCuratedModal) {
            deleteCuratedModal.classList.remove("hidden");
        }
    }

    function openBulkDeleteModal() {
        if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
        const ids = Array.from(state.librarySelectedIds);
        state.deleteModalOpen = true;
        state.deleteModalTarget = { type: "bulk", ids: ids };

        if (deleteCuratedTitle) {
            deleteCuratedTitle.textContent = "Delete & Reject Selected Maps";
        }
        if (deleteTargetPreview) {
            deleteTargetPreview.innerHTML = `<span class="delete-target-count mono">${ids.length} Curated Map${ids.length === 1 ? "" : "s"} Selected for Permanent Deletion</span>`;
        }
        if (deleteReasonSelect) {
            deleteReasonSelect.value = "unspecified";
        }
        if (deleteNotesInput) {
            deleteNotesInput.value = "";
        }
        if (deleteNotesGroup) {
            deleteNotesGroup.style.display = "none";
        }
        if (deleteCuratedModal) {
            deleteCuratedModal.classList.remove("hidden");
        }
    }

    function closeDeleteCuratedModal() {
        state.deleteModalOpen = false;
        state.deleteModalTarget = null;
        if (deleteCuratedModal) {
            deleteCuratedModal.classList.add("hidden");
        }
    }

    async function confirmDeleteCurated() {
        if (!state.deleteModalTarget) return;
        if (btnConfirmDelete) btnConfirmDelete.disabled = true;

        const reason = deleteReasonSelect ? deleteReasonSelect.value : "unspecified";
        const notes = deleteNotesInput && deleteNotesInput.value.trim() ? deleteNotesInput.value.trim() : null;

        try {
            if (state.deleteModalTarget.type === "single") {
                const id = state.deleteModalTarget.id;
                const res = await fetch(`/api/curated/${encodeURIComponent(id)}`, {
                    method: "DELETE",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({
                        reason: reason,
                        notes: notes,
                    }),
                });
                if (!res.ok) {
                    const errText = await res.text();
                    throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
                }

                // Remove map from state.curatedMaps
                state.curatedMaps = state.curatedMaps.filter(m => m.id !== id);
                if (state.librarySelectedIds.has(id)) {
                    state.librarySelectedIds.delete(id);
                    updateLibraryBulkBar();
                }

                // Close drawer if open for this map
                if (state.libraryDrawerActive && state.libraryDrawerMap && state.libraryDrawerMap.id === id) {
                    closeLibraryInspector();
                }

                const cardEl = libraryGrid ? libraryGrid.querySelector(`.curated-card[data-id="${id}"]`) : null;
                if (cardEl) {
                    cardEl.remove();
                }

                applyLibraryFilters();
                closeDeleteCuratedModal();
                invalidateDatasetReadiness();
                showToast(`Deleted curated map ${id} and logged rejection`, "danger");
            } else if (state.deleteModalTarget.type === "bulk") {
                const ids = state.deleteModalTarget.ids;
                const res = await fetch("/api/curated/bulk-delete", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({
                        ids: ids,
                        reason: reason,
                    }),
                });
                if (!res.ok) {
                    const errText = await res.text();
                    throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
                }

                const data = await res.json();
                const deletedCount = data.deleted_count !== undefined ? data.deleted_count : ids.length;

                const idSet = new Set(ids);
                state.curatedMaps = state.curatedMaps.filter(m => !idSet.has(m.id));

                if (state.libraryDrawerActive && state.libraryDrawerMap && idSet.has(state.libraryDrawerMap.id)) {
                    closeLibraryInspector();
                }

                ids.forEach(id => {
                    const cardEl = libraryGrid ? libraryGrid.querySelector(`.curated-card[data-id="${id}"]`) : null;
                    if (cardEl) cardEl.remove();
                });

                clearLibrarySelection();
                applyLibraryFilters();
                closeDeleteCuratedModal();
                invalidateDatasetReadiness();
                showToast(`Deleted ${deletedCount} curated map${deletedCount === 1 ? "" : "s"} and logged rejection`, "danger");
            }
        } catch (err) {
            showToast(`Deletion failed: ${err.message}`, "danger");
        } finally {
            if (btnConfirmDelete) btnConfirmDelete.disabled = false;
        }
    }

    async function bulkTagSelectedMaps() {
        if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
        const ids = Array.from(state.librarySelectedIds);

        const tagInput = window.prompt(`Enter tag to add to ${ids.length} selected map${ids.length === 1 ? "" : "s"}:`);
        if (!tagInput) return;
        const normalizedTag = tagInput.trim().toLowerCase().replace(/^#+/, "");
        if (!normalizedTag) return;

        if (btnBulkTag) btnBulkTag.disabled = true;

        let successCount = 0;
        try {
            for (const id of ids) {
                const map = state.curatedMaps.find(m => m.id === id);
                const currentTags = map && Array.isArray(map.tags) ? [...map.tags] : [];
                if (!currentTags.map(t => t.toLowerCase()).includes(normalizedTag)) {
                    currentTags.push(normalizedTag);
                }

                const res = await fetch(`/api/curated/${encodeURIComponent(id)}`, {
                    method: "PATCH",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ tags: currentTags }),
                });

                if (res.ok) {
                    const updated = await res.json();
                    if (map) {
                        map.tags = updated.tags;
                    }
                    successCount++;
                }
            }

            applyLibraryFilters();
            if (successCount > 0) invalidateDatasetReadiness();
            showToast(`Added tag "${normalizedTag}" to ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
        } catch (err) {
            showToast(`Bulk tag error: ${err.message}`, "danger");
        } finally {
            if (btnBulkTag) btnBulkTag.disabled = false;
        }
    }

    async function refineSelectedMaps() {
        if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
        if (state.isBulkRefining) return;

        // Snapshot the selection: the loop outlives a selection change, and every map is refined
        // one at a time so the progress label and the readiness audit never interleave.
        const ids = Array.from(state.librarySelectedIds);
        const total = ids.length;
        const failures = [];
        let successCount = 0;

        state.isBulkRefining = true;
        if (btnBulkRefine) btnBulkRefine.disabled = true;
        const setLabel = (text) => {
            if (btnBulkRefineLabel) btnBulkRefineLabel.textContent = text;
        };
        setLabel(`Refining 0/${total}…`);

        try {
            for (let i = 0; i < total; i++) {
                const id = ids[i];
                setLabel(`Refining ${i + 1}/${total}…`);
                try {
                    const res = await fetch(`/api/curated/${encodeURIComponent(id)}/refine`, {
                        method: "POST",
                    });
                    if (!res.ok) {
                        const errData = await res.json().catch(() => ({}));
                        throw new Error(errData.error || `HTTP ${res.status}`);
                    }
                    const updated = await res.json();

                    const idx = state.curatedMaps.findIndex(m => m.id === id);
                    if (idx !== -1) state.curatedMaps[idx] = updated;

                    if (state.libraryDrawerActive && state.libraryDrawerMap && state.libraryDrawerMap.id === id) {
                        state.libraryDrawerMap = updated;
                        // Unsaved operator edits win: only an untouched draft picks up the
                        // refined tags, description and layers.
                        if (!state.libraryDraftDirty) {
                            state.libraryDraft = {
                                tags: Array.isArray(updated.tags) ? [...updated.tags] : [],
                                description: updated.description || "",
                                layers: (updated.layers && updated.layers.length > 0)
                                    ? updated.layers.map(l => ({ ...l }))
                                    : state.libraryDraft.layers,
                            };
                            renderInspectorLayers();
                            renderInspectorThemeChips();
                            renderInspectorTagChips();
                            if (inspectorDescriptionInput) {
                                inspectorDescriptionInput.value = state.libraryDraft.description;
                            }
                            updateInspectorDescriptionCounter(state.libraryDraft.description);
                        }
                    }

                    successCount++;
                } catch (err) {
                    failures.push({ id, message: err.message });
                    console.error(`AI refine failed for #${id}:`, err);
                }
            }

            if (successCount > 0) {
                applyLibraryFilters();
                invalidateDatasetReadiness();
            }

            if (failures.length === 0) {
                showToast(`AI-refined ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
            } else {
                const failedIds = failures.map(f => `#${f.id}`).join(", ");
                showToast(
                    `AI refine failed for ${failures.length} of ${total} maps (${failedIds}): ${failures[0].message}`,
                    "danger"
                );
            }
        } finally {
            state.isBulkRefining = false;
            if (btnBulkRefine) btnBulkRefine.disabled = false;
            setLabel("AI Refine Selected");
        }
    }

    async function exportSelectedLoraDataset() {
        if (!state.librarySelectedIds || state.librarySelectedIds.size === 0) return;
        if (state.isExporting) return;
        state.isExporting = true;
        if (btnBulkExport) btnBulkExport.disabled = true;

        const count = state.librarySelectedIds.size;
        showToast(`Packaging ${count} selected maps into LoRA dataset ZIP...`, "info");

        try {
            const payload = { ids: Array.from(state.librarySelectedIds) };
            const res = await fetch("/api/dataset/export-selected", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });
            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`Export failed (HTTP ${res.status}: ${errText || res.statusText}`);
            }

            const blob = await res.blob();
            const filename = "lora-dataset.zip";

            const downloadUrl = window.URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = downloadUrl;
            a.download = filename;
            document.body.appendChild(a);
            a.click();
            a.remove();
            window.URL.revokeObjectURL(downloadUrl);

            showToast(`Downloaded LoRA dataset (${formatBytes(blob.size)}) for ${count} map${count === 1 ? "" : "s"}!`, "success");
        } catch (err) {
            showToast(`Export error: ${err.message}`, "danger");
        } finally {
            state.isExporting = false;
            if (btnBulkExport) btnBulkExport.disabled = false;
        }
    }

    // ==========================================================================
    // Fullscreen Stage Overlay with Reused 2D Canvas & HUD Controls (Issue #134)
    // ==========================================================================

    export function resizeFullscreenCanvas() {
        if (!libraryFullscreenCanvas || !libraryFullscreenCanvas.parentElement) return;
        const container = libraryFullscreenCanvas.parentElement;
        const width = container.clientWidth;
        const height = container.clientHeight;
        const dpr = window.devicePixelRatio || 1;

        if (libraryFullscreenCanvas.width !== width * dpr || libraryFullscreenCanvas.height !== height * dpr) {
            libraryFullscreenCanvas.width = width * dpr;
            libraryFullscreenCanvas.height = height * dpr;
            libraryFullscreenCanvas.style.width = `${width}px`;
            libraryFullscreenCanvas.style.height = `${height}px`;
        }
    }

    let fsRenderPending = false;
    export function requestFullscreenRender() {
        if (!fsRenderPending) {
            fsRenderPending = true;
            requestAnimationFrame(() => {
                fsRenderPending = false;
                renderFullscreenCanvas();
            });
        }
    }

    /**
     * High-performance 60fps Canvas 2D render loop for the Fullscreen Stage Overlay.
     * Applies devicePixelRatio scaling, pan/zoom transformation matrix,
     * renders crisp pixel examination for zoom > 200%,
     * and single-draw-call 64px VTT grid overlay.
     */
    function renderFullscreenCanvas() {
        if (!libraryFullscreenCtx || !libraryFullscreenCanvas) return;
        const c = libraryFullscreenCtx;
        const dpr = window.devicePixelRatio || 1;

        c.save();
        c.clearRect(0, 0, libraryFullscreenCanvas.width, libraryFullscreenCanvas.height);

        // Fill background
        c.fillStyle = "#070a10";
        c.fillRect(0, 0, libraryFullscreenCanvas.width, libraryFullscreenCanvas.height);

        // Normalize devicePixelRatio coordinates
        c.scale(dpr, dpr);

        if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.complete && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
            const img = state.libraryFullscreenCurrentImage;
            const imgW = img.naturalWidth || img.width;
            const imgH = img.naturalHeight || img.height;

            c.save();
            // Apply pan and zoom transformation
            c.translate(state.libraryFullscreenPan.x, state.libraryFullscreenPan.y);
            c.scale(state.libraryFullscreenZoom, state.libraryFullscreenZoom);

            // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
            c.imageSmoothingEnabled = state.libraryFullscreenZoom < 2.0;
            c.imageSmoothingQuality = "high";

            // Draw active layer battlemap at origin (0, 0)
            c.globalAlpha = 1.0;
            c.drawImage(img, 0, 0, imgW, imgH);

            const isFsCalibrationActive = Boolean(
                state.libraryFullscreenGridPanelOpen ||
                state.libraryFullscreenGridAlignMode ||
                state.libraryFullscreenIsDraggingGrid
            );
            // Draw crisp 64px VTT grid overlay in image space
            if (state.libraryFullscreenGridVisible || isFsCalibrationActive) {
                drawFullscreenVttGrid(c, imgW, imgH, isFsCalibrationActive);
            }

            // Draw image bounding border
            c.beginPath();
            c.rect(0, 0, imgW, imgH);
            c.lineWidth = Math.max(1.0, 1.5 / state.libraryFullscreenZoom);
            c.strokeStyle = "rgba(56, 189, 248, 0.75)";
            c.stroke();

            c.restore();
        }

        c.restore();
    }

    /**
     * Single-draw-call crisp 64px VTT grid overlay for Fullscreen Stage.
     */
    function drawFullscreenVttGrid(c, width, height, isCalibrationActive = false) {
        c.save();
        c.beginPath();
        c.rect(0, 0, width, height);
        c.clip();

        const size = state.libraryFullscreenGridSize || VTT_GRID_SIZE;
        const ox = ((Math.round(state.libraryFullscreenGridOffset.x) % size) + size) % size;
        const oy = ((Math.round(state.libraryFullscreenGridOffset.y) % size) + size) % size;
        const cols = Math.floor(Math.max(0, width - ox) / size);
        const rows = Math.floor(Math.max(0, height - oy) / size);
        const fullW = cols * size;
        const fullH = rows * size;

        // When calibration panel or align mode is active:
        // Shade/dim discarded partial edge strips outside the active full-grid lattice [ox, oy, fullW, fullH]
        if (isCalibrationActive && (cols > 0 || rows > 0)) {
            c.save();
            c.beginPath();
            c.rect(0, 0, width, height);
            c.rect(ox, oy, fullW, fullH);
            c.fillStyle = "rgba(10, 15, 30, 0.65)";
            c.fill("evenodd");
            c.restore();
        }

        c.beginPath();

        // Vertical grid lines
        for (let x = ox; x <= width; x += size) {
            c.moveTo(x, 0);
            c.lineTo(x, height);
        }

        // Horizontal grid lines
        for (let y = oy; y <= height; y += size) {
            c.moveTo(0, y);
            c.lineTo(width, y);
        }

        c.lineWidth = Math.max(0.5, 1.0 / state.libraryFullscreenZoom);
        c.strokeStyle = "rgba(56, 189, 248, 0.4)";
        c.stroke();

        // Draw boundary border around active full grid
        if (isCalibrationActive && cols > 0 && rows > 0) {
            c.save();
            c.beginPath();
            c.rect(ox, oy, fullW, fullH);
            c.lineWidth = Math.max(1.5, 2.5 / state.libraryFullscreenZoom);
            const isEligible = cols >= 16 && rows >= 16;
            c.strokeStyle = isEligible ? "rgba(56, 189, 248, 0.95)" : "rgba(244, 63, 94, 0.95)";
            c.stroke();
            c.restore();
        }

        c.restore();
    }

    function fullscreenFitToScreen() {
        if (!state.libraryFullscreenCurrentImage || !libraryFullscreenCanvas) return;
        const img = state.libraryFullscreenCurrentImage;
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;
        const viewW = libraryFullscreenCanvas.clientWidth;
        const viewH = libraryFullscreenCanvas.clientHeight;

        if (!imgW || !imgH || !viewW || !viewH) return;

        const padding = 40;
        const availW = Math.max(20, viewW - padding * 2);
        const availH = Math.max(20, viewH - padding * 2);

        const fitScale = Math.min(availW / imgW, availH / imgH, 1.0);
        state.libraryFullscreenZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, fitScale));

        state.libraryFullscreenPan.x = Math.round((viewW - imgW * state.libraryFullscreenZoom) / 2);
        state.libraryFullscreenPan.y = Math.round((viewH - imgH * state.libraryFullscreenZoom) / 2);

        updateFullscreenHud();
        requestFullscreenRender();
    }

    function fullscreenZoomStep(factor, clientPoint = null) {
        if (!state.libraryFullscreenCurrentImage || !libraryFullscreenCanvas) return;
        const prevZoom = state.libraryFullscreenZoom;
        let newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
        newZoom = Math.round(newZoom * 1000) / 1000;

        if (newZoom === prevZoom) return;

        const viewW = libraryFullscreenCanvas.clientWidth;
        const viewH = libraryFullscreenCanvas.clientHeight;
        const focusX = clientPoint ? clientPoint.x : viewW / 2;
        const focusY = clientPoint ? clientPoint.y : viewH / 2;

        state.libraryFullscreenPan.x = focusX - (focusX - state.libraryFullscreenPan.x) * (newZoom / prevZoom);
        state.libraryFullscreenPan.y = focusY - (focusY - state.libraryFullscreenPan.y) * (newZoom / prevZoom);
        state.libraryFullscreenZoom = newZoom;

        updateFullscreenHud();
        requestFullscreenRender();
    }

    function setFullscreenZoom100() {
        if (!state.libraryFullscreenCurrentImage || !libraryFullscreenCanvas) return;
        const prevZoom = state.libraryFullscreenZoom;
        const newZoom = 1.0;
        if (newZoom === prevZoom) return;

        const viewW = libraryFullscreenCanvas.clientWidth;
        const viewH = libraryFullscreenCanvas.clientHeight;
        const focusX = viewW / 2;
        const focusY = viewH / 2;

        state.libraryFullscreenPan.x = focusX - (focusX - state.libraryFullscreenPan.x) * (newZoom / prevZoom);
        state.libraryFullscreenPan.y = focusY - (focusY - state.libraryFullscreenPan.y) * (newZoom / prevZoom);
        state.libraryFullscreenZoom = newZoom;

        updateFullscreenHud();
        requestFullscreenRender();
    }

    // --- Fullscreen Continuous Smooth WASD Pan Loop ---
    const activeFsPanKeys = { w: false, a: false, s: false, d: false };
    let fsPanAnimationId = null;
    let lastFsPanTimestamp = 0;

    function fsPanLoop(timestamp) {
        if (!lastFsPanTimestamp) lastFsPanTimestamp = timestamp;
        const dt = Math.min((timestamp - lastFsPanTimestamp) / 1000, 0.1);
        lastFsPanTimestamp = timestamp;

        let moveX = 0;
        let moveY = 0;
        if (activeFsPanKeys.w) moveY += 1;
        if (activeFsPanKeys.s) moveY -= 1;
        if (activeFsPanKeys.a) moveX += 1;
        if (activeFsPanKeys.d) moveX -= 1;

        if (moveX !== 0 || moveY !== 0) {
            if (moveX !== 0 && moveY !== 0) {
                const invSqrt2 = 0.70710678;
                moveX *= invSqrt2;
                moveY *= invSqrt2;
            }
            if (state.libraryFullscreenCurrentImage) {
                state.libraryFullscreenPan.x += moveX * PAN_SPEED * dt;
                state.libraryFullscreenPan.y += moveY * PAN_SPEED * dt;
                requestFullscreenRender();
            }
            fsPanAnimationId = requestAnimationFrame(fsPanLoop);
        } else {
            fsPanAnimationId = null;
            lastFsPanTimestamp = 0;
        }
    }

    function startFsPanLoop() {
        if (!fsPanAnimationId) {
            lastFsPanTimestamp = performance.now();
            fsPanAnimationId = requestAnimationFrame(fsPanLoop);
        }
    }

    function stopFsPanLoop() {
        if (fsPanAnimationId) {
            cancelAnimationFrame(fsPanAnimationId);
            fsPanAnimationId = null;
        }
        lastFsPanTimestamp = 0;
        activeFsPanKeys.w = false;
        activeFsPanKeys.a = false;
        activeFsPanKeys.s = false;
        activeFsPanKeys.d = false;
    }

    // --- Fullscreen Continuous Smooth Zoom Loop ---
    const activeFsZoomKeys = { in: false, out: false };
    let fsZoomAnimationId = null;
    let lastFsZoomTimestamp = 0;

    function fsZoomLoop(timestamp) {
        if (!lastFsZoomTimestamp) lastFsZoomTimestamp = timestamp;
        const dt = Math.min((timestamp - lastFsZoomTimestamp) / 1000, 0.1);
        lastFsZoomTimestamp = timestamp;

        if (activeFsZoomKeys.in || activeFsZoomKeys.out) {
            let factor = 1.0;
            if (activeFsZoomKeys.in && !activeFsZoomKeys.out) {
                factor = Math.pow(ZOOM_SPEED, dt);
            } else if (activeFsZoomKeys.out && !activeFsZoomKeys.in) {
                factor = Math.pow(1 / ZOOM_SPEED, dt);
            }

            if (factor !== 1.0 && state.libraryFullscreenCurrentImage) {
                fullscreenZoomStep(factor);
            }
            fsZoomAnimationId = requestAnimationFrame(fsZoomLoop);
        } else {
            fsZoomAnimationId = null;
            lastFsZoomTimestamp = 0;
        }
    }

    function startFsZoomLoop() {
        if (!fsZoomAnimationId) {
            lastFsZoomTimestamp = performance.now();
            fsZoomAnimationId = requestAnimationFrame(fsZoomLoop);
        }
    }

    function stopFsZoomLoop() {
        if (fsZoomAnimationId) {
            cancelAnimationFrame(fsZoomAnimationId);
            fsZoomAnimationId = null;
        }
        lastFsZoomTimestamp = 0;
        activeFsZoomKeys.in = false;
        activeFsZoomKeys.out = false;
    }

    // --- Fullscreen Map & Layer Loading ---
    function loadFullscreenMap(index, layerIndex = 0) {
        if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;
        const clampedIndex = Math.max(0, Math.min(index, state.libraryFilteredMaps.length - 1));
        const map = state.libraryFilteredMaps[clampedIndex];
        if (!map) return;

        state.libraryFullscreenIndex = clampedIndex;
        state.libraryFullscreenMap = map;

        // Restore or initialize grid settings for this curated map from the persisted grid
        if (map.grid) {
            state.libraryFullscreenGridSize = map.grid.cell_size_px ?? 64;
            state.libraryFullscreenGridOffset = {
                x: map.grid.offset_x ?? 0,
                y: map.grid.offset_y ?? 0,
            };
        } else {
            state.libraryFullscreenGridSize = 64;
            state.libraryFullscreenGridOffset = { x: 0, y: 0 };
        }
        updateFullscreenGridPanel();

        const layers = map.layers && map.layers.length > 0 ? map.layers : [{ index: 0, role: "ground" }];
        const validLayerIndex = Math.max(0, Math.min(layerIndex, layers.length - 1));
        state.libraryFullscreenLayerIndex = validLayerIndex;

        const layerObj = layers[validLayerIndex];
        const actualLayerIdx = (layerObj && layerObj.index !== undefined) ? layerObj.index : validLayerIndex;
        const cacheKey = `${map.id}_layer_${actualLayerIdx}`;

        const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${actualLayerIdx}`;

        let img = state.libraryFullscreenImageCache.get(cacheKey);
        if (img && img.complete && img.naturalWidth > 0) {
            state.libraryFullscreenCurrentImage = img;
            fullscreenFitToScreen();
            renderFullscreenLayerDeck();
            updateFullscreenHud();
        } else {
            if (!img) {
                img = new Image();
                img.src = directUrl;
                state.libraryFullscreenImageCache.set(cacheKey, img);
            }
            img.onload = () => {
                if (state.libraryFullscreenMap && state.libraryFullscreenMap.id === map.id && state.libraryFullscreenLayerIndex === validLayerIndex) {
                    state.libraryFullscreenCurrentImage = img;
                    fullscreenFitToScreen();
                    renderFullscreenLayerDeck();
                    updateFullscreenHud();
                }
            };
            if (img.complete && img.naturalWidth > 0) {
                state.libraryFullscreenCurrentImage = img;
                fullscreenFitToScreen();
                renderFullscreenLayerDeck();
                updateFullscreenHud();
            }
        }

        // Prefetch ground layer
        let groundIdx = layers.findIndex(l => (l.role || "").toLowerCase().replace(/[-_]/g, "") === "ground");
        if (groundIdx < 0 && layers.length > 0) groundIdx = 0;
        if (groundIdx >= 0 && groundIdx !== validLayerIndex) {
            const gLayer = layers[groundIdx];
            const gActualIdx = (gLayer && gLayer.index !== undefined) ? gLayer.index : groundIdx;
            const gKey = `${map.id}_layer_${gActualIdx}`;
            if (!state.libraryFullscreenImageCache.get(gKey)) {
                const gImg = new Image();
                gImg.src = `/api/image/curated/${encodeURIComponent(map.id)}/${gActualIdx}`;
                state.libraryFullscreenImageCache.set(gKey, gImg);
            }
        }

        // Prefetch other layers of this map
        layers.forEach((l, idx) => {
            const lIdx = l.index !== undefined ? l.index : idx;
            const k = `${map.id}_layer_${lIdx}`;
            if (!state.libraryFullscreenImageCache.get(k)) {
                const pImg = new Image();
                pImg.src = `/api/image/curated/${encodeURIComponent(map.id)}/${lIdx}`;
                state.libraryFullscreenImageCache.set(k, pImg);
            }
        });

        // Prefetch adjacent maps
        [-1, 1, 2].forEach(offset => {
            const targetIdx = clampedIndex + offset;
            if (targetIdx >= 0 && targetIdx < state.libraryFilteredMaps.length) {
                const adjMap = state.libraryFilteredMaps[targetIdx];
                const adjKey = `${adjMap.id}_layer_0`;
                if (!state.libraryFullscreenImageCache.get(adjKey)) {
                    const adjImg = new Image();
                    adjImg.src = `/api/image/curated/${encodeURIComponent(adjMap.id)}/0`;
                    state.libraryFullscreenImageCache.set(adjKey, adjImg);
                }
            }
        });

        updateFullscreenHud();
        renderFullscreenLayerDeck();
    }

    function updateFullscreenHud() {
        if (!state.libraryFullscreenActive || !state.libraryFullscreenMap) return;
        const map = state.libraryFullscreenMap;

        if (fsMapIdBadge) {
            fsMapIdBadge.textContent = map.id;
            fsMapIdBadge.title = `Curated Map ID: ${map.id}`;
        }

        if (fsMapDimensions) {
            let dimText = "";
            if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
                dimText = `${state.libraryFullscreenCurrentImage.naturalWidth} × ${state.libraryFullscreenCurrentImage.naturalHeight}`;
            } else if (map.dimensions && map.dimensions.width && map.dimensions.height) {
                dimText = `${map.dimensions.width} × ${map.dimensions.height}`;
            } else {
                dimText = "2048 × 2048";
            }
            fsMapDimensions.textContent = dimText;
        }

        if (fsMapCounter) {
            const total = state.libraryFilteredMaps ? state.libraryFilteredMaps.length : 1;
            const current = (state.libraryFullscreenIndex >= 0 ? state.libraryFullscreenIndex : 0) + 1;
            fsMapCounter.textContent = `Map ${current} of ${total}`;
        }

        if (fsZoomReadout) {
            const pct = Math.round((state.libraryFullscreenZoom || 1.0) * 100);
            fsZoomReadout.textContent = `${pct}%`;
        }

        if (btnFsGridToggle) {
            btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
        }

        if (btnFsGridAdjust) {
            btnFsGridAdjust.classList.toggle("active", state.libraryFullscreenGridPanelOpen);
        }

        if (btnFsPrevMap) {
            btnFsPrevMap.disabled = state.libraryFullscreenIndex <= 0;
        }
        if (btnFsNextMap) {
            btnFsNextMap.disabled = !state.libraryFilteredMaps || state.libraryFullscreenIndex >= state.libraryFilteredMaps.length - 1;
        }
        if (fsGridStatusBadge) {
            const fsDims = getLibraryFullscreenDimensions();
            const fsLattice = calculateLatticeInfo(
                fsDims.width,
                fsDims.height,
                state.libraryFullscreenGridSize,
                state.libraryFullscreenGridOffset.x,
                state.libraryFullscreenGridOffset.y
            );
            if (fsLattice.isEligible) {
                fsGridStatusBadge.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells (${fsLattice.kx} × ${fsLattice.ky} sections)`;
                fsGridStatusBadge.classList.remove("hud-badge-warning");
            } else {
                fsGridStatusBadge.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells (< 16×16 cells)`;
                fsGridStatusBadge.classList.add("hud-badge-warning");
            }
        }
    }

    function renderFullscreenLayerDeck() {
        if (!libraryFullscreenLayerDeck || !fsLayerFilmstripStrip) return;
        const map = state.libraryFullscreenMap;

        if (!map || !map.layers || map.layers.length <= 1) {
            libraryFullscreenLayerDeck.classList.add("hidden");
            return;
        }

        libraryFullscreenLayerDeck.classList.remove("hidden");
        const total = map.layers.length;
        if (fsLayerDeckCount) {
            fsLayerDeckCount.textContent = `(${state.libraryFullscreenLayerIndex + 1} / ${total})`;
        }

        fsLayerFilmstripStrip.innerHTML = "";
        const fragment = document.createDocumentFragment();

        map.layers.forEach((layer, idx) => {
            const card = document.createElement("div");
            const isActive = idx === state.libraryFullscreenLayerIndex;
            card.className = `fs-layer-card${isActive ? " active" : ""}`;
            card.dataset.index = idx;

            const layerIdx = layer.index !== undefined ? layer.index : idx;
            const thumbUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

            const role = (layer.role || "ground").toLowerCase().replace(/[-_]/g, "");
            let roleClass = "fs-role-ground";
            let roleName = "Ground";
            if (role === "upperfloor" || role === "upper") {
                roleClass = "fs-role-upper";
                roleName = "Upper";
            } else if (role === "basement" || role === "lower" || role === "cellar") {
                roleClass = "fs-role-basement";
                roleName = "Basement";
            } else if (role === "roof") {
                roleClass = "fs-role-roof";
                roleName = "Roof";
            } else if (role === "variant") {
                roleClass = "fs-role-variant";
                roleName = "Variant";
            }

            const elevText = typeof layer.elevation === "number"
                ? (layer.elevation > 0 ? `+${layer.elevation}` : `${layer.elevation}`)
                : (role === "ground" ? "0" : (role === "upperfloor" || role === "upper" ? "+1" : (role === "basement" ? "-1" : "0")));

            const hotkeyKbd = idx < 6 ? `<div class="fs-layer-card-key"><kbd>${idx + 1}</kbd></div>` : "";

            card.innerHTML = `
                <div class="fs-layer-card-thumb-wrap">
                    ${hotkeyKbd}
                    <img class="fs-layer-card-img" src="${thumbUrl}" alt="Layer ${idx + 1}" loading="lazy">
                </div>
                <div class="fs-layer-card-meta">
                    <span class="fs-role-chip ${roleClass}">${escapeHtml(roleName)}</span>
                    <span class="fs-elevation-badge mono">${escapeHtml(elevText)}</span>
                </div>
            `;

            card.addEventListener("click", () => {
                switchFullscreenLayer(idx);
            });

            fragment.appendChild(card);
        });

        fsLayerFilmstripStrip.appendChild(fragment);
    }

    function switchFullscreenLayer(layerIndex) {
        if (!state.libraryFullscreenMap) return;
        const layers = state.libraryFullscreenMap.layers || [];
        if (layerIndex < 0 || layerIndex >= layers.length) return;

        state.libraryFullscreenLayerIndex = layerIndex;
        loadFullscreenMap(state.libraryFullscreenIndex, layerIndex);
    }

    // --- Fullscreen Grid Calibration Controls ---
    function toggleFullscreenGridPanel(force) {
        state.libraryFullscreenGridPanelOpen = typeof force === "boolean" ? force : !state.libraryFullscreenGridPanelOpen;
        if (libraryFullscreenGridPanel) {
            libraryFullscreenGridPanel.classList.toggle("hidden", !state.libraryFullscreenGridPanelOpen);
        }
        if (btnFsGridAdjust) {
            btnFsGridAdjust.classList.toggle("active", state.libraryFullscreenGridPanelOpen);
        }
        if (state.libraryFullscreenGridPanelOpen) {
            updateFullscreenGridPanel();
        }
        requestFullscreenRender();
    }

    function updateFullscreenGridPanel() {
        if (!fsGridSizeInput || !fsGridSizeSlider) return;
        const size = state.libraryFullscreenGridSize || 64;
        fsGridSizeInput.value = size;
        fsGridSizeSlider.value = Math.min(256, Math.max(16, size));

        if (fsGridOffsetXVal) fsGridOffsetXVal.textContent = Math.round(state.libraryFullscreenGridOffset.x);
        if (fsGridOffsetYVal) fsGridOffsetYVal.textContent = Math.round(state.libraryFullscreenGridOffset.y);

        const chips = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
        chips.forEach(chip => {
            const chipSize = parseInt(chip.dataset.size, 10);
            chip.classList.toggle("active", chipSize === size);
        });

        const fsDims = getLibraryFullscreenDimensions();
        const fsLattice = calculateLatticeInfo(
            fsDims.width,
            fsDims.height,
            state.libraryFullscreenGridSize,
            state.libraryFullscreenGridOffset.x,
            state.libraryFullscreenGridOffset.y
        );
        if (fsGridLatticeCells) {
            fsGridLatticeCells.textContent = `${fsLattice.cols} × ${fsLattice.rows} cells`;
        }
        if (fsGridLatticeSections) {
            if (fsLattice.isEligible) {
                fsGridLatticeSections.textContent = `${fsLattice.kx} × ${fsLattice.ky} sections`;
                fsGridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-valid";
            } else {
                fsGridLatticeSections.textContent = "< 16×16 cells";
                fsGridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-warning";
            }
        }
    }

    let saveFullscreenGridTimeout = null;
    function saveFullscreenMapGrid() {
        if (state.libraryFullscreenMap && state.libraryFullscreenMap.id) {
            const map = state.libraryFullscreenMap;
            const payload = {
                grid: {
                    cell_size_px: state.libraryFullscreenGridSize,
                    offset_x: state.libraryFullscreenGridOffset.x,
                    offset_y: state.libraryFullscreenGridOffset.y,
                },
            };
            map.grid = { ...payload.grid };

            clearTimeout(saveFullscreenGridTimeout);
            saveFullscreenGridTimeout = setTimeout(async () => {
                try {
                    const res = await fetch(`/api/curated/${encodeURIComponent(map.id)}`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify(payload),
                    });
                    if (!res.ok) {
                        const errData = await res.json().catch(() => ({}));
                        throw new Error(errData.error || `HTTP ${res.status}`);
                    }
                    // The regrid recomputes Map Section descriptions server-side, so the cached
                    // audit this map was flagged in no longer describes the bundle.
                    invalidateDatasetReadiness();
                } catch (err) {
                    showToast(`Grid not saved: ${err.message}`, "danger");
                }
            }, 300);
        }
    }

    function setFullscreenGridSize(size) {
        const num = Number.isFinite(size) ? size : parseInt(size, 10);
        if (isNaN(num)) return;
        const clamped = Math.max(MIN_GRID_SIZE, Math.min(MAX_GRID_SIZE, Math.round(num)));
        state.libraryFullscreenGridSize = clamped;
        saveFullscreenMapGrid();
        updateFullscreenGridPanel();
        updateFullscreenHud();
        requestFullscreenRender();
    }

    function nudgeFullscreenGridOffset(dx, dy) {
        state.libraryFullscreenGridOffset.x = Math.round(state.libraryFullscreenGridOffset.x + dx);
        state.libraryFullscreenGridOffset.y = Math.round(state.libraryFullscreenGridOffset.y + dy);
        saveFullscreenMapGrid();
        updateFullscreenGridPanel();
        updateFullscreenHud();
        requestFullscreenRender();
    }

    function resetFullscreenGrid() {
        state.libraryFullscreenGridSize = 64;
        state.libraryFullscreenGridOffset = { x: 0, y: 0 };
        saveFullscreenMapGrid();
        updateFullscreenGridPanel();
        updateFullscreenHud();
        requestFullscreenRender();
    }

    function toggleFullscreenGridAlignMode(force) {
        state.libraryFullscreenGridAlignMode = typeof force === "boolean" ? force : !state.libraryFullscreenGridAlignMode;
        if (btnFsGridAlignDrag) {
            btnFsGridAlignDrag.classList.toggle("active", state.libraryFullscreenGridAlignMode);
        }
        if (libraryFullscreenCanvas && libraryFullscreenCanvas.parentElement) {
            libraryFullscreenCanvas.parentElement.classList.toggle("grid-aligning", state.libraryFullscreenGridAlignMode);
        }
        requestFullscreenRender();
    }

    // --- Fullscreen Open & Close Lifecycle ---
    function openLibraryFullscreen(index) {
        if (!state.libraryFilteredMaps || state.libraryFilteredMaps.length === 0) return;
        const clampedIndex = Math.max(0, Math.min(index, state.libraryFilteredMaps.length - 1));

        state.libraryFullscreenActive = true;
        if (libraryFullscreenOverlay) {
            libraryFullscreenOverlay.classList.remove("hidden");
        }

        setLibraryFocus(clampedIndex, false);
        resizeFullscreenCanvas();
        loadFullscreenMap(clampedIndex, 0);

        if (libraryFullscreenCanvas) {
            libraryFullscreenCanvas.focus();
        }
    }

    function closeLibraryFullscreen() {
        if (!state.libraryFullscreenActive) return;

        stopFsPanLoop();
        stopFsZoomLoop();

        state.libraryFullscreenActive = false;
        if (libraryFullscreenOverlay) {
            libraryFullscreenOverlay.classList.add("hidden");
        }

        toggleFullscreenGridPanel(false);
        toggleFullscreenGridAlignMode(false);

        // Restore focus on card in library view and scroll into view smoothly
        if (state.libraryFullscreenIndex >= 0) {
            setLibraryFocus(state.libraryFullscreenIndex, true);
        }
    }

    function setupFullscreenControls() {
        if (!libraryFullscreenCanvas) return;

        // Canvas Mouse Pan Dragging & Dynamic Grid Align Drag
        libraryFullscreenCanvas.addEventListener("mousedown", (e) => {
            if (e.button === 0 && (e.shiftKey || state.libraryFullscreenGridAlignMode)) {
                state.libraryFullscreenIsDraggingGrid = true;
                state.libraryFullscreenGridDragStart = { x: e.clientX, y: e.clientY };
                state.libraryFullscreenGridOffsetStart = { x: state.libraryFullscreenGridOffset.x, y: state.libraryFullscreenGridOffset.y };
                libraryFullscreenCanvas.parentElement?.classList.add("grid-dragging");
                e.preventDefault();
                return;
            }

            if (e.button === 0 || e.button === 1) {
                state.libraryFullscreenIsPanning = true;
                state.libraryFullscreenPanStart = {
                    x: e.clientX - state.libraryFullscreenPan.x,
                    y: e.clientY - state.libraryFullscreenPan.y,
                };
                libraryFullscreenCanvas.parentElement?.classList.add("panning");
                e.preventDefault();
            }
        });

        // Wheel Zoom
        libraryFullscreenCanvas.addEventListener("wheel", (e) => {
            e.preventDefault();
            if (!state.libraryFullscreenCurrentImage) return;

            const rect = libraryFullscreenCanvas.getBoundingClientRect();
            const point = {
                x: e.clientX - rect.left,
                y: e.clientY - rect.top,
            };

            const factor = e.deltaY < 0 ? 1.15 : (1 / 1.15);
            fullscreenZoomStep(factor, point);
        }, { passive: false });

        // HUD Buttons
        btnFsGridToggle?.addEventListener("click", () => {
            state.libraryFullscreenGridVisible = !state.libraryFullscreenGridVisible;
            if (btnFsGridToggle) btnFsGridToggle.classList.toggle("active", state.libraryFullscreenGridVisible);
            requestFullscreenRender();
        });

        btnFsGridAdjust?.addEventListener("click", () => toggleFullscreenGridPanel());
        btnFsFit?.addEventListener("click", () => fullscreenFitToScreen());
        btnFsZoomOut?.addEventListener("click", () => fullscreenZoomStep(1 / 1.2));
        btnFsZoomIn?.addEventListener("click", () => fullscreenZoomStep(1.2));

        btnFsPrevMap?.addEventListener("click", () => {
            if (state.libraryFullscreenIndex > 0) {
                loadFullscreenMap(state.libraryFullscreenIndex - 1, 0);
                setLibraryFocus(state.libraryFullscreenIndex, true);
            }
        });

        btnFsNextMap?.addEventListener("click", () => {
            if (state.libraryFilteredMaps && state.libraryFullscreenIndex < state.libraryFilteredMaps.length - 1) {
                loadFullscreenMap(state.libraryFullscreenIndex + 1, 0);
                setLibraryFocus(state.libraryFullscreenIndex, true);
            }
        });

        btnFsClose?.addEventListener("click", () => closeLibraryFullscreen());

        // Fullscreen Grid Panel Controls
        btnFsCloseGridPanel?.addEventListener("click", () => toggleFullscreenGridPanel(false));
        btnFsGridDone?.addEventListener("click", () => toggleFullscreenGridPanel(false));
        btnFsGridReset?.addEventListener("click", () => resetFullscreenGrid());

        btnFsGridSizeDec?.addEventListener("click", () => setFullscreenGridSize(state.libraryFullscreenGridSize - 1));
        btnFsGridSizeInc?.addEventListener("click", () => setFullscreenGridSize(state.libraryFullscreenGridSize + 1));

        if (fsGridSizeInput) {
            fsGridSizeInput.addEventListener("input", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setFullscreenGridSize(val);
            });
            fsGridSizeInput.addEventListener("change", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setFullscreenGridSize(val);
            });
        }

        if (fsGridSizeSlider) {
            fsGridSizeSlider.addEventListener("input", (e) => {
                const val = parseInt(e.target.value, 10);
                if (!isNaN(val)) setFullscreenGridSize(val);
            });
        }

        const fsPresets = document.querySelectorAll("#fs-grid-preset-chips .preset-chip");
        fsPresets.forEach(chip => {
            chip.addEventListener("click", () => {
                const size = parseInt(chip.dataset.size, 10);
                if (!isNaN(size)) setFullscreenGridSize(size);
            });
        });

        btnFsGridNudgeUp?.addEventListener("click", () => nudgeFullscreenGridOffset(0, -1));
        btnFsGridNudgeDown?.addEventListener("click", () => nudgeFullscreenGridOffset(0, 1));
        btnFsGridNudgeLeft?.addEventListener("click", () => nudgeFullscreenGridOffset(-1, 0));
        btnFsGridNudgeRight?.addEventListener("click", () => nudgeFullscreenGridOffset(1, 0));

        btnFsGridAlignDrag?.addEventListener("click", () => toggleFullscreenGridAlignMode());
    }

    // ==========================================================================
    // Slide-Over Inspector Drawer & Refinement Engine (Issue #135 & Issue #136)
    // ==========================================================================

    function openLibraryInspector(map) {
        if (!map) return;
        state.libraryDrawerActive = true;
        state.libraryDrawerMap = map;

        // Populates draft state from selected map (deep copy)
        const tagsList = map.tags ? [...map.tags] : [];
        const descriptionText = map.description || "";
        const layersList = (map.layers && map.layers.length > 0)
            ? map.layers.map(l => ({ ...l }))
            : [{
                index: 0,
                filename: "0_ground.png",
                role: "ground",
                elevation: 0,
                caption: null,
                dimensions: map.dimensions || [1024, 1024],
            }];

        state.libraryDraft = {
            tags: tagsList,
            description: descriptionText,
            layers: layersList,
        };
        state.libraryDraftDirty = false;

        if (inspectorMapIdBadge) {
            inspectorMapIdBadge.textContent = map.id || "—";
            inspectorMapIdBadge.title = map.id || "";
        }

        updateInspectorDirtyVisual(false);
        renderInspectorLayers();
        renderInspectorThemeChips();
        renderInspectorTagChips();

        if (inspectorDescriptionInput) {
            inspectorDescriptionInput.value = state.libraryDraft.description;
        }
        updateInspectorDescriptionCounter(state.libraryDraft.description);

        if (inspectorAiSuggestionsBox) {
            inspectorAiSuggestionsBox.classList.add("hidden");
        }
        if (inspectorAiSuggestionsPills) {
            inspectorAiSuggestionsPills.innerHTML = "";
        }

        if (libraryInspectorDrawer) {
            libraryInspectorDrawer.classList.remove("hidden");
        }
    }

    function closeLibraryInspector() {
        if (!state.libraryDrawerActive) return;
        state.libraryDrawerActive = false;
        if (libraryInspectorDrawer) {
            libraryInspectorDrawer.classList.add("hidden");
        }
    }

    function toggleLibraryInspector() {
        if (state.libraryDrawerActive) {
            closeLibraryInspector();
        } else {
            if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
                const targetIdx = state.libraryFocusedIndex >= 0 ? state.libraryFocusedIndex : 0;
                openLibraryInspector(state.libraryFilteredMaps[targetIdx]);
            }
        }
    }

    function markLibraryDraftDirty(dirty = true) {
        state.libraryDraftDirty = dirty;
        updateInspectorDirtyVisual(dirty);
    }

    function updateInspectorDirtyVisual(dirty) {
        if (inspectorUnsavedBadge) {
            inspectorUnsavedBadge.classList.toggle("hidden", !dirty);
        }
        if (btnInspectorSave) {
            btnInspectorSave.classList.toggle("dirty", dirty);
        }
    }

    function renderInspectorLayers() {
        if (!inspectorLayersList || !state.libraryDrawerMap) return;
        inspectorLayersList.innerHTML = "";

        const layers = state.libraryDraft.layers || [];
        if (inspectorLayerCountBadge) {
            inspectorLayerCountBadge.textContent = `${layers.length} Layer${layers.length === 1 ? "" : "s"}`;
        }

        const map = state.libraryDrawerMap;

        const fragment = document.createDocumentFragment();

        layers.forEach((layer, idx) => {
            const card = document.createElement("div");
            card.className = "inspector-layer-card";
            card.dataset.layerIndex = idx;

            const layerIdx = layer.index !== undefined ? layer.index : idx;
            const directUrl = `/api/image/curated/${encodeURIComponent(map.id)}/${layerIdx}`;

            const roleCls = getScrubPillRoleClass(layer.role);
            const roleName = formatRoleName(layer.role, layer.elevation);
            const normalizedRole = normalizeLayerRole(layer.role);
            const elevText = (typeof layer.elevation === "number")
                ? `${layer.elevation >= 0 ? `+${layer.elevation}` : layer.elevation}`
                : "0";
            const dimsText = (layer.dimensions && Array.isArray(layer.dimensions) && layer.dimensions.length >= 2)
                ? `${layer.dimensions[0]} × ${layer.dimensions[1]}`
                : "—";

            card.innerHTML = `
                <div class="inspector-layer-thumb-box">
                    <img class="inspector-layer-thumb" src="${directUrl}" alt="Layer ${layerIdx}" loading="lazy" />
                </div>
                <div class="inspector-layer-details">
                    <div class="inspector-layer-meta-row">
                        <span class="inspector-layer-filename mono" title="${escapeHtml(layer.filename || `Layer ${layerIdx}`)}">${escapeHtml(layer.filename || `Layer ${layerIdx}`)}</span>
                        <div class="inspector-layer-badges">
                            <span class="inspector-role-badge ${roleCls}">${escapeHtml(roleName)}</span>
                            <span class="inspector-elev-badge mono">${escapeHtml(elevText)}</span>
                            <span class="inspector-dim-badge mono">${escapeHtml(dimsText)}</span>
                        </div>
                    </div>
                    <div class="inspector-layer-controls-row">
                        <div class="inspector-control-group" style="flex: 1;">
                            <label class="inspector-control-label" for="role-sel-${idx}">Role</label>
                            <select id="role-sel-${idx}" class="inspector-role-select" aria-label="Layer role">
                                <option value="ground" ${normalizedRole === "ground" ? "selected" : ""}>Ground</option>
                                <option value="upper_floor" ${normalizedRole === "upper_floor" ? "selected" : ""}>UpperFloor</option>
                                <option value="basement" ${normalizedRole === "basement" ? "selected" : ""}>Basement</option>
                                <option value="roof" ${normalizedRole === "roof" ? "selected" : ""}>Roof</option>
                                <option value="variant" ${normalizedRole === "variant" ? "selected" : ""}>Variant</option>
                            </select>
                        </div>
                        <div class="inspector-control-group">
                            <label class="inspector-control-label" for="elev-inp-${idx}">Elev</label>
                            <input type="number" id="elev-inp-${idx}" class="inspector-elev-input mono" min="-10" max="10" step="1" value="${typeof layer.elevation === "number" ? layer.elevation : (normalizedRole === "ground" ? 0 : 1)}" aria-label="Elevation level" />
                        </div>
                    </div>
                </div>
            `;

            const roleSelect = card.querySelector(".inspector-role-select");
            const elevInput = card.querySelector(".inspector-elev-input");
            const roleBadge = card.querySelector(".inspector-role-badge");
            const elevBadge = card.querySelector(".inspector-elev-badge");

            roleSelect?.addEventListener("change", (e) => {
                const newRole = e.target.value;
                layer.role = newRole;

                // Adjust elevation default if switching roles
                if (newRole === "ground") {
                    layer.elevation = 0;
                    if (elevInput) elevInput.value = "0";
                } else if (newRole === "upper_floor") {
                    if (typeof layer.elevation !== "number" || layer.elevation <= 0) {
                        layer.elevation = 1;
                        if (elevInput) elevInput.value = "1";
                    }
                } else if (newRole === "basement") {
                    if (typeof layer.elevation !== "number" || layer.elevation >= 0) {
                        layer.elevation = -1;
                        if (elevInput) elevInput.value = "-1";
                    }
                }

                if (roleBadge) {
                    roleBadge.className = `inspector-role-badge ${getScrubPillRoleClass(newRole)}`;
                    roleBadge.textContent = formatRoleName(newRole, layer.elevation);
                }
                if (elevBadge) {
                    elevBadge.textContent = layer.elevation >= 0 ? `+${layer.elevation}` : `${layer.elevation}`;
                }

                markLibraryDraftDirty();
            });

            elevInput?.addEventListener("input", (e) => {
                let val = parseInt(e.target.value, 10);
                if (isNaN(val)) val = 0;
                val = Math.max(-10, Math.min(10, val));
                layer.elevation = val;

                if (elevBadge) {
                    elevBadge.textContent = val >= 0 ? `+${val}` : `${val}`;
                }
                if (roleBadge) {
                    roleBadge.textContent = formatRoleName(layer.role, val);
                }

                markLibraryDraftDirty();
            });

            fragment.appendChild(card);
        });

        inspectorLayersList.appendChild(fragment);
    }

    function renderInspectorThemeChips() {
        if (!inspectorThemeChips) return;
        inspectorThemeChips.innerHTML = "";

        const themes = (state.libraryDraft.tags || []).filter(isCanonicalThemeTag);
        themes.forEach((theme) => {
            const chip = document.createElement("span");
            chip.className = "chip chip-theme";
            chip.innerHTML = `${escapeHtml(theme)}<button type="button" class="btn-chip-remove" aria-label="Remove theme ${escapeHtml(theme)}" title="Remove">×</button>`;

            const btnRemove = chip.querySelector(".btn-chip-remove");
            btnRemove?.addEventListener("click", (e) => {
                e.stopPropagation();
                removeInspectorTheme(theme);
            });

            inspectorThemeChips.appendChild(chip);
        });
    }

    function addInspectorTheme(themeStr) {
        if (!themeStr) return;
        const normalized = themeStr.trim().toLowerCase();
        if (!normalized) return;

        if (!state.libraryDraft.tags) {
            state.libraryDraft.tags = [];
        }

        if (!state.libraryDraft.tags.some(t => t.toLowerCase() === normalized)) {
            state.libraryDraft.tags.push(normalized);
            markLibraryDraftDirty();
            renderInspectorThemeChips();
            renderInspectorTagChips();
        }

        if (inspectorThemeInput) {
            inspectorThemeInput.value = "";
        }
    }

    function removeInspectorTheme(themeStr) {
        removeInspectorTag(themeStr);
    }

    function renderInspectorTagChips() {
        if (!inspectorTagChips) return;
        inspectorTagChips.innerHTML = "";

        const tags = (state.libraryDraft.tags || []).filter(t => !isCanonicalThemeTag(t));
        tags.forEach((tag) => {
            const chip = document.createElement("span");
            chip.className = "chip chip-tag";
            chip.innerHTML = `${escapeHtml(tag)}<button type="button" class="btn-chip-remove" aria-label="Remove tag ${escapeHtml(tag)}" title="Remove">×</button>`;

            const btnRemove = chip.querySelector(".btn-chip-remove");
            btnRemove?.addEventListener("click", (e) => {
                e.stopPropagation();
                removeInspectorTag(tag);
            });

            inspectorTagChips.appendChild(chip);
        });
    }

    function addInspectorTag(tagStr) {
        if (!tagStr) return;
        const normalized = tagStr.trim().toLowerCase();
        if (!normalized) return;

        if (!state.libraryDraft.tags) {
            state.libraryDraft.tags = [];
        }

        if (!state.libraryDraft.tags.some(t => t.toLowerCase() === normalized)) {
            state.libraryDraft.tags.push(normalized);
            markLibraryDraftDirty();
            renderInspectorThemeChips();
            renderInspectorTagChips();
        }

        if (inspectorTagInput) {
            inspectorTagInput.value = "";
        }
    }

    function removeInspectorTag(tagStr) {
        if (!state.libraryDraft.tags) return;
        const idx = state.libraryDraft.tags.findIndex(t => t.toLowerCase() === tagStr.toLowerCase());
        if (idx !== -1) {
            state.libraryDraft.tags.splice(idx, 1);
            markLibraryDraftDirty();
            renderInspectorThemeChips();
            renderInspectorTagChips();
        }
    }

    function updateInspectorDescriptionCounter(text) {
        if (!inspectorDescCounter) return;
        const trimmed = (text || "").trim();
        const wordCount = trimmed ? trimmed.split(/\s+/).length : 0;
        const charCount = (text || "").length;

        inspectorDescCounter.textContent = `${wordCount} word${wordCount === 1 ? "" : "s"} · ${charCount} char${charCount === 1 ? "" : "s"}`;

        inspectorDescCounter.classList.remove("counter-optimal", "counter-suboptimal");
        if (wordCount >= 40 && wordCount <= 90) {
            inspectorDescCounter.classList.add("counter-optimal");
        } else if (wordCount > 0) {
            inspectorDescCounter.classList.add("counter-suboptimal");
        }
    }

    async function saveLibraryInspectorChanges() {
        if (!state.libraryDrawerMap || !state.libraryDrawerActive) return;

        const mapId = state.libraryDrawerMap.id;
        if (!mapId) return;

        if (btnInspectorSave) {
            btnInspectorSave.disabled = true;
            btnInspectorSave.classList.add("loading");
        }

        try {
            const payload = {
                tags: state.libraryDraft.tags,
                description: state.libraryDraft.description,
                layers: state.libraryDraft.layers.map((l, idx) => ({
                    index: l.index !== undefined ? l.index : idx,
                    filename: l.filename || `layer_${idx}.png`,
                    role: l.role || "ground",
                    elevation: typeof l.elevation === "number" ? l.elevation : (l.role === "ground" ? 0 : 1),
                    caption: l.caption || null,
                    source_url: l.source_url || null,
                    dimensions: (l.dimensions && Array.isArray(l.dimensions) && l.dimensions.length >= 2) ? l.dimensions : (state.libraryDrawerMap.dimensions || [1024, 1024]),
                })),
            };

            const res = await fetch(`/api/curated/${encodeURIComponent(mapId)}`, {
                method: "PATCH",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });

            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
            }

            const updatedMap = await res.json();

            // Update in state.curatedMaps
            if (state.curatedMaps && Array.isArray(state.curatedMaps)) {
                const idx = state.curatedMaps.findIndex(m => m.id === mapId);
                if (idx !== -1) {
                    state.curatedMaps[idx] = updatedMap;
                } else {
                    state.curatedMaps.unshift(updatedMap);
                }
            }

            state.libraryDrawerMap = updatedMap;
            state.libraryDraft = {
                tags: [...(updatedMap.tags || [])],
                description: updatedMap.description || "",
                layers: updatedMap.layers ? updatedMap.layers.map(l => ({ ...l })) : [...state.libraryDraft.layers],
            };

            // Tags, description and layer roles all feed the audit, so the stored report is stale.
            invalidateDatasetReadiness();
            markLibraryDraftDirty(false);
            applyLibraryFilters();
            showToast(`Saved changes for map ${mapId}`, "success");
        } catch (err) {
            showToast(`Failed to save changes: ${err.message}`, "danger");
        } finally {
            if (btnInspectorSave) {
                btnInspectorSave.disabled = false;
                btnInspectorSave.classList.remove("loading");
            }
        }
    }

    function discardLibraryInspectorChanges() {
        if (!state.libraryDrawerMap) return;

        const map = state.libraryDrawerMap;
        const tagsList = map.tags ? [...map.tags] : [];
        const descriptionText = map.description || "";
        const layersList = (map.layers && map.layers.length > 0)
            ? map.layers.map(l => ({ ...l }))
            : [{
                index: 0,
                filename: "0_ground.png",
                role: "ground",
                elevation: 0,
                caption: null,
                dimensions: map.dimensions || [1024, 1024],
            }];

        state.libraryDraft = {
            tags: tagsList,
            description: descriptionText,
            layers: layersList,
        };
        markLibraryDraftDirty(false);

        renderInspectorLayers();
        renderInspectorThemeChips();
        renderInspectorTagChips();
        if (inspectorDescriptionInput) {
            inspectorDescriptionInput.value = descriptionText;
        }
        updateInspectorDescriptionCounter(descriptionText);
        showToast("Discarded unsaved changes", "info");
    }

    // --- AI Refinements (Issue #136) ---

    async function suggestInspectorThemes() {
        if (!state.libraryDrawerMap) return;
        const mapId = state.libraryDrawerMap.id;

        const spinner = btnInspectorSuggestThemes?.querySelector(".ai-spinner");
        if (btnInspectorSuggestThemes) {
            btnInspectorSuggestThemes.disabled = true;
            spinner?.classList.remove("hidden");
        }

        try {
            const res = await fetch(`/api/curated/${encodeURIComponent(mapId)}/suggest-themes`, {
                method: "POST",
            });
            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
            }

            const data = await res.json();
            const suggestions = data.suggestions || [];

            renderInspectorAiSuggestions(suggestions);
            if (suggestions.length > 0) {
                showToast(`Generated ${suggestions.length} theme/tag suggestions`, "info");
            } else {
                showToast("No new theme suggestions generated", "info");
            }
        } catch (err) {
            showToast(`Theme suggestion failed: ${err.message}`, "danger");
        } finally {
            if (btnInspectorSuggestThemes) {
                btnInspectorSuggestThemes.disabled = false;
                spinner?.classList.add("hidden");
            }
        }
    }

    function renderInspectorAiSuggestions(suggestions) {
        if (!inspectorAiSuggestionsBox || !inspectorAiSuggestionsPills) return;

        if (!suggestions || suggestions.length === 0) {
            inspectorAiSuggestionsBox.classList.add("hidden");
            inspectorAiSuggestionsPills.innerHTML = "";
            return;
        }

        inspectorAiSuggestionsBox.classList.remove("hidden");
        inspectorAiSuggestionsPills.innerHTML = "";

        const fragment = document.createDocumentFragment();

        suggestions.forEach((sug) => {
            const pill = document.createElement("button");
            pill.type = "button";
            pill.className = "ai-suggestion-pill";
            pill.innerHTML = `<span>${escapeHtml(sug)}</span><span class="pill-add-icon">+</span>`;
            pill.title = `Add "${sug}" to themes/tags`;

            pill.addEventListener("click", () => {
                addSuggestedItem(sug);
                pill.classList.add("added");
            });

            fragment.appendChild(pill);
        });

        inspectorAiSuggestionsPills.appendChild(fragment);
    }

    function addSuggestedItem(sug) {
        if (!sug) return;
        const normalized = sug.trim().toLowerCase();
        if (CANONICAL_THEMES.includes(normalized)) {
            addInspectorTheme(normalized);
        } else {
            addInspectorTag(normalized);
        }
    }

    async function describeInspectorMap() {
        if (!state.libraryDrawerMap) return;
        const mapId = state.libraryDrawerMap.id;

        const spinner = btnInspectorDescribe?.querySelector(".ai-spinner");
        if (btnInspectorDescribe) {
            btnInspectorDescribe.disabled = true;
            spinner?.classList.remove("hidden");
        }

        try {
            const payload = {
                tags: state.libraryDraft.tags,
            };

            const res = await fetch(`/api/curated/${encodeURIComponent(mapId)}/describe`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });

            if (!res.ok) {
                const errText = await res.text();
                throw new Error(`HTTP ${res.status}: ${errText || res.statusText}`);
            }

            const data = await res.json();
            const description = data.description || "";

            state.libraryDraft.description = description;
            if (inspectorDescriptionInput) {
                inspectorDescriptionInput.value = description;
            }
            updateInspectorDescriptionCounter(description);
            markLibraryDraftDirty();
            showToast("Generated physical description", "success");
        } catch (err) {
            showToast(`Description generation failed: ${err.message}`, "danger");
        } finally {
            if (btnInspectorDescribe) {
                btnInspectorDescribe.disabled = false;
                spinner?.classList.add("hidden");
            }
        }
    }

    async function runAllInspectorAiRefinements() {
        if (!state.libraryDrawerMap) return;
        const mapId = state.libraryDrawerMap.id;

        const comboSpinner = btnInspectorRunAll?.querySelector(".ai-spinner");
        if (btnInspectorRunAll) {
            btnInspectorRunAll.disabled = true;
            comboSpinner?.classList.remove("hidden");
        }

        try {
            // Step 1: Suggest themes & tags
            const themesRes = await fetch(`/api/curated/${encodeURIComponent(mapId)}/suggest-themes`, {
                method: "POST",
            });
            if (themesRes.ok) {
                const data = await themesRes.json();
                const suggestions = data.suggestions || [];
                renderInspectorAiSuggestions(suggestions);

                // Auto-add canonical themes and up to 3 relevant tags
                suggestions.forEach((sug) => {
                    const norm = sug.trim().toLowerCase();
                    if (CANONICAL_THEMES.includes(norm)) {
                        addInspectorTheme(norm);
                    }
                });
            }

            // Step 2: Generate description using updated draft tags
            const describePayload = {
                tags: state.libraryDraft.tags,
            };
            const descRes = await fetch(`/api/curated/${encodeURIComponent(mapId)}/describe`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(describePayload),
            });
            if (!descRes.ok) {
                const errText = await descRes.text();
                throw new Error(`Description step failed: ${errText || descRes.statusText}`);
            }

            const descData = await descRes.json();
            const description = descData.description || "";
            state.libraryDraft.description = description;
            if (inspectorDescriptionInput) {
                inspectorDescriptionInput.value = description;
            }
            updateInspectorDescriptionCounter(description);
            markLibraryDraftDirty();

            showToast("All AI refinements applied successfully", "success");
        } catch (err) {
            showToast(`AI Refinement failed: ${err.message}`, "danger");
        } finally {
            if (btnInspectorRunAll) {
                btnInspectorRunAll.disabled = false;
                comboSpinner?.classList.add("hidden");
            }
        }
    }

    function setupInspectorDrawerControls() {
        btnCloseInspector?.addEventListener("click", () => closeLibraryInspector());
        btnInspectorDiscard?.addEventListener("click", () => discardLibraryInspectorChanges());
        btnInspectorSave?.addEventListener("click", () => saveLibraryInspectorChanges());

        btnInspectorSuggestThemes?.addEventListener("click", () => suggestInspectorThemes());
        btnInspectorDescribe?.addEventListener("click", () => describeInspectorMap());
        btnInspectorRunAll?.addEventListener("click", () => runAllInspectorAiRefinements());

        btnAcceptAllSuggestions?.addEventListener("click", () => {
            if (!inspectorAiSuggestionsPills) return;
            const pills = inspectorAiSuggestionsPills.querySelectorAll(".ai-suggestion-pill:not(.added)");
            pills.forEach((p) => {
                const text = p.querySelector("span")?.textContent || "";
                if (text) addSuggestedItem(text);
                p.classList.add("added");
            });
            showToast("Added all suggestions", "info");
        });

        btnAddTheme?.addEventListener("click", () => {
            if (inspectorThemeInput) {
                addInspectorTheme(inspectorThemeInput.value);
            }
        });

        inspectorThemeInput?.addEventListener("keydown", (e) => {
            if (e.key === "Enter") {
                e.preventDefault();
                addInspectorTheme(inspectorThemeInput.value);
            } else if (e.key === "Backspace" && !inspectorThemeInput.value) {
                const themeTags = (state.libraryDraft.tags || []).filter(isCanonicalThemeTag);
                if (themeTags.length > 0) {
                    removeInspectorTheme(themeTags[themeTags.length - 1]);
                }
            }
        });

        btnAddTag?.addEventListener("click", () => {
            if (inspectorTagInput) {
                addInspectorTag(inspectorTagInput.value);
            }
        });

        inspectorTagInput?.addEventListener("keydown", (e) => {
            if (e.key === "Enter") {
                e.preventDefault();
                addInspectorTag(inspectorTagInput.value);
            } else if (e.key === "Backspace" && !inspectorTagInput.value) {
                if (state.libraryDraft.tags && state.libraryDraft.tags.length > 0) {
                    removeInspectorTag(state.libraryDraft.tags[state.libraryDraft.tags.length - 1]);
                }
            }
        });

        inspectorDescriptionInput?.addEventListener("input", (e) => {
            state.libraryDraft.description = e.target.value;
            updateInspectorDescriptionCounter(e.target.value);
            markLibraryDraftDirty();
        });

        btnInspectorFullscreen?.addEventListener("click", () => {
            if (!state.libraryDrawerMap) return;
            const mapId = state.libraryDrawerMap.id;
            let targetIdx = 0;
            if (state.libraryFilteredMaps && state.libraryFilteredMaps.length > 0) {
                const found = state.libraryFilteredMaps.findIndex(m => m.id === mapId);
                if (found !== -1) targetIdx = found;
            }
            openLibraryFullscreen(targetIdx);
        });

        btnInspectorDelete?.addEventListener("click", () => {
            if (state.libraryDrawerMap) {
                openDeleteCuratedModal(state.libraryDrawerMap.id);
            }
        });
    }


    function setupLibraryControls() {
        tabNavTriage?.addEventListener("click", () => switchView("triage"));
        tabNavLibrary?.addEventListener("click", () => switchView("library"));
        btnSwitchToTriage?.addEventListener("click", () => switchView("triage"));
        btnRefreshLibrary?.addEventListener("click", () => fetchLibraryMaps());
        btnExportLora?.addEventListener("click", () => exportLoraDataset());
        btnClearLibraryFilters?.addEventListener("click", () => clearLibraryFilters());
        btnFreezeSnapshot?.addEventListener("click", () => freezeDatasetSnapshot());
        btnRefreshSnapshots?.addEventListener("click", () => fetchDatasetSnapshots());
        btnToggleSnapshots?.addEventListener("click", () => toggleSnapshotsShelf());

        librarySearchInput?.addEventListener("input", (e) => {
            state.librarySearchQuery = e.target.value;
            applyLibraryFilters();
        });

        const facetButtons = document.querySelectorAll(".lib-facet-chip");
        facetButtons.forEach((btn) => {
            btn.addEventListener("click", () => {
                const facet = btn.dataset.facet || "all";
                setLibraryFacet(facet);
            });
        });

        librarySortSelect?.addEventListener("change", (e) => {
            state.librarySort = e.target.value;
            applyLibraryFilters();
        });

        const densityButtons = document.querySelectorAll(".btn-density");
        densityButtons.forEach((btn) => {
            btn.addEventListener("click", () => {
                const density = btn.dataset.density;
                if (density) {
                    setLibraryDensity(density);
                }
            });
        });

        setLibraryDensity(state.libraryDensity);
        if (librarySortSelect) {
            librarySortSelect.value = state.librarySort;
        }

        // Delete Modal & Bulk Operations Controls (Issue #137)
        btnCloseDeleteModal?.addEventListener("click", () => closeDeleteCuratedModal());
        btnCancelDelete?.addEventListener("click", () => closeDeleteCuratedModal());
        btnConfirmDelete?.addEventListener("click", () => confirmDeleteCurated());
        deleteCuratedModal?.addEventListener("click", (e) => {
            if (e.target === deleteCuratedModal) {
                closeDeleteCuratedModal();
            }
        });

        btnBulkDelete?.addEventListener("click", () => openBulkDeleteModal());
        btnBulkTag?.addEventListener("click", () => bulkTagSelectedMaps());
        btnBulkRefine?.addEventListener("click", () => refineSelectedMaps());
        btnBulkExport?.addEventListener("click", () => exportSelectedLoraDataset());
        btnBulkDeselect?.addEventListener("click", () => clearLibrarySelection());
    }
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

    // ==========================================================================
    // UI Event Listeners & Hotkeys
    // ==========================================================================

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
