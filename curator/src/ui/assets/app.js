/**
 * Dungeon Forge — Battlemap Curator Frontend Application
 * High-performance zero-dependency Canvas 2D triage deck, 64px VTT grid overlay,
 * zero-latency in-memory prefetching, and keyboard-driven hotkeys.
 */

(() => {
    "use strict";

    // Application State
    const state = {
        // Queue data
        candidates: [],
        currentIndex: -1,
        currentImage: null,

        // In-memory image cache for 0ms transitions (id -> HTMLImageElement)
        imageCache: new Map(),

        // Curation parameters
        labelingMode: null,
        currentLayerIndex: 0,
        onionSkinActive: false,
        lastSplitParentId: null,
        // Canvas transform
        pan: { x: 0, y: 0 },
        zoom: 1.0,
        isPanning: false,
        dragStart: { x: 0, y: 0 },
        panStart: { x: 0, y: 0 },

        // Visual overlays
        gridVisible: true,
        gridSize: 64,
        gridOffset: { x: 0, y: 0 },
        gridAlignMode: false,
        gridPanelOpen: false,
        isDraggingGrid: false,
        gridDragStart: { x: 0, y: 0 },
        gridOffsetStart: { x: 0, y: 0 },
        // Modal and UI dialog state
        helpModalOpen: false,
        drawerOpen: false,
        isSubmitting: false,
        isHarvesting: false,
        // Candidate ids whose curation request is still in flight server-side.
        pendingCurationIds: new Set(),

        // Benchmark Drawer & Budget Guardrails
        benchmarkDrawerOpen: false,
        budgetGuardModalOpen: false,
        isBenchmarking: false,
        benchmarkAbortController: null,
        benchmarkWindowMinimized: false,
        // Storage Monitor & Purge (Issue #30)
        storageModalOpen: false,
        purgeModalOpen: false,
        isPurging: false,
        storageStats: null,
        // LoRA Training Dataset Readiness audit (Issue #184)
        datasetReadinessModalOpen: false,
        isAuditingDataset: false,
        // Harvesting parameters
        harvestSort: "hot",
        harvestTime: "all",
        // Dataset Library View state (Issue #31 & Issue #132)
        currentView: "triage",
        libraryActiveTag: null,
        librarySearchQuery: "",
        libraryDensity: (function() {
            try {
                const stored = localStorage.getItem("curator_library_density");
                if (stored && ["compact", "comfortable", "large"].includes(stored)) {
                    return stored;
                }
            } catch (_) {}
            return "comfortable";
        })(),
        librarySort: "date-desc",
        libraryFacet: "all",
        libraryFocusedIndex: -1,
        librarySelectedIds: new Set(),
        libraryFilteredMaps: [],
        curatedMaps: [],
        isExporting: false,
        // Interactive Decision Modal (Issue #102)
        decisionModalOpen: false,
        // Fullscreen Stage Overlay state (Issue #134)
        libraryFullscreenActive: false,
        libraryFullscreenMap: null,
        libraryFullscreenIndex: -1,
        libraryFullscreenLayerIndex: 0,
        libraryFullscreenPan: { x: 0, y: 0 },
        libraryFullscreenZoom: 1.0,
        libraryFullscreenIsPanning: false,
        libraryFullscreenPanStart: { x: 0, y: 0 },
        libraryFullscreenGridVisible: true,
        libraryFullscreenGridSize: 64,
        libraryFullscreenGridOffset: { x: 0, y: 0 },
        libraryFullscreenGridPanelOpen: false,
        libraryFullscreenGridAlignMode: false,
        libraryFullscreenIsDraggingGrid: false,
        libraryFullscreenGridDragStart: { x: 0, y: 0 },
        libraryFullscreenGridOffsetStart: { x: 0, y: 0 },
        libraryFullscreenOnionSkinActive: false,
        libraryFullscreenCurrentImage: null,
        libraryFullscreenImageCache: new Map(),
        // Slide-Over Inspector Drawer & Curated Map Metadata Editing (Issue #135 & Issue #136)
        libraryDrawerActive: false,
        libraryDrawerMap: null,
        libraryDraft: { tags: [], description: "", layers: [] },
        libraryDraftDirty: false,
        libraryAiLoading: { suggest: false, describe: false, combo: false },
        // Deletion Safeguard Modal & Bulk Ops (Issue #137)
        deleteModalOpen: false,
        deleteModalTarget: null,
    };

    // Curate Decision Modal Flow State
    const decisionState = {
        isOpen: false,
        candidate: null,
        cardIndex: 0,
        acceptedTags: [],
        cardDecisions: [],
        step: "cards",
    };

    // Constants
    const VTT_GRID_SIZE = 64; // 64px represents standard 5ft D&D VTT square
    const MIN_GRID_SIZE = 16;
    const MAX_GRID_SIZE = 300;
    const MIN_ZOOM = 0.1;
    const MAX_ZOOM = 5.0;
    const PREFETCH_AHEAD = 3;

    const MODEL_PRICES = {
        "google/gemini-3.1-flash-image": 0.07,
        "google/gemini-3.1-flash-lite-image": 0.035,
        "meta/muse-image": 0.01,
        "bytedance-seed/seedream-5-0-flash": 0.018,
    };
    const BUDGET_GUARDRAIL_THRESHOLD = 2.00;
    const CANONICAL_THEMES = [
        "dungeon",
        "cave",
        "tavern",
        "wilderness",
        "ruins",
        "urban",
        "scifi",
        "building",
    ];
    const THEMES = CANONICAL_THEMES;


    // DOM Elements cache
    let canvas, ctx;
    let statusIndicator, statusText;
    let candidateMetaBox, queueCounter, candidateId, candidateDimensions, candidateSource;
    let zoomLevelDisplay, hudPanZoom, hudGridStatus, hudLatticeStatus, hudResolution, hudPrefetchStatus;
    let btnGridToggle, btnFitScreen, btnPrev, btnNext, btnReject, btnCurate;
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
    let btnOnionSkin, btnSplitBundle, btnSplitBundleHud, btnSplitHudText, btnSplitLabel;
    let onionSkinBadge;

    // Benchmark Drawer & Guardrails DOM Elements
    let btnOpenBenchmarkDrawer;
    let costMonitorHud, costMonitorText;
    let benchmarkDrawer, btnCloseBenchmarkDrawer, btnCancelBenchmarkDrawer;
    let btnModelsSelectAll, btnModelsSelectNone, btnPromptsSelectAll, btnPromptsSelectNone;
    let benchmarkEnableCustom, customPromptFields, benchmarkCustomName, benchmarkCustomPrompt, benchmarkEnableReference;
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
    let btnDatasetReadiness, datasetReadinessLabel;
    let datasetReadinessModal, datasetReadinessSummary, datasetReadinessIssues, datasetReadinessIcon;

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
    let btnFsOnionSkin;
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
    let btnBulkDelete, btnBulkTag, btnBulkExport, btnBulkDeselect;
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
        btnOnionSkin = document.getElementById("btn-onion-skin");
        btnSplitBundle = document.getElementById("btn-split-bundle");
        btnSplitBundleHud = document.getElementById("btn-split-bundle-hud");
        btnSplitHudText = document.getElementById("btn-split-hud-text");
        btnSplitLabel = document.getElementById("btn-split-label");
        onionSkinBadge = document.getElementById("onion-skin-badge");

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
        btnDatasetReadiness = document.getElementById("btn-dataset-readiness");
        datasetReadinessLabel = document.getElementById("dataset-readiness-label");
        datasetReadinessModal = document.getElementById("dataset-readiness-modal");
        datasetReadinessSummary = document.getElementById("dataset-readiness-summary");
        datasetReadinessIssues = document.getElementById("dataset-readiness-issues");
        datasetReadinessIcon = document.getElementById("dataset-readiness-icon");

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
        btnFsOnionSkin = document.getElementById("btn-fs-onion-skin");
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
        btnBulkExport = document.getElementById("btn-bulk-export");
        btnBulkDeselect = document.getElementById("btn-bulk-deselect");
        btnInspectorDelete = document.getElementById("btn-inspector-delete");
    }

    // ==========================================================================
    // Canvas 2D Engine & Rendering
    // ==========================================================================

    function setupCanvas() {
        resizeCanvas();
        window.addEventListener("resize", () => {
            resizeCanvas();
            requestRender();
            if (state.libraryFullscreenActive) {
                resizeFullscreenCanvas();
                requestFullscreenRender();
            }
        });
    }

    function resizeCanvas() {
        const container = canvas.parentElement;
        const width = container.clientWidth;
        const height = container.clientHeight;
        const dpr = window.devicePixelRatio || 1;

        if (canvas.width !== width * dpr || canvas.height !== height * dpr) {
            canvas.width = width * dpr;
            canvas.height = height * dpr;
            canvas.style.width = `${width}px`;
            canvas.style.height = `${height}px`;
        }
    }

    let renderPending = false;
    function requestRender() {
        if (!renderPending) {
            renderPending = true;
            requestAnimationFrame(() => {
                renderPending = false;
                render();
            });
        }
    }

    /**
     * Calculates the full-grid lattice dimensions, section counts, and eligibility.
     */
    function calculateLatticeInfo(width, height, cellSize, offsetX, offsetY) {
        const size = Math.max(16, cellSize || 64);
        const ox = ((Math.round(offsetX || 0) % size) + size) % size;
        const oy = ((Math.round(offsetY || 0) % size) + size) % size;
        const cols = Math.floor(Math.max(0, (width || 0) - ox) / size);
        const rows = Math.floor(Math.max(0, (height || 0) - oy) / size);
        const isEligible = cols >= 16 && rows >= 16;
        const kx = isEligible ? Math.ceil(cols / 16) : null;
        const ky = isEligible ? Math.ceil(rows / 16) : null;
        return {
            size,
            ox,
            oy,
            cols,
            rows,
            isEligible,
            kx,
            ky,
            fullW: cols * size,
            fullH: rows * size,
        };
    }

    /**
     * Retrieves Ground layer dimensions for the current triage candidate.
     */
    function getCurrentGroundDimensions() {
        const cand = state.candidates && state.candidates[state.currentIndex];
        if (cand) {
            const layers = getCandidateLayers(cand);
            const ground = layers.find(l => {
                const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                return r === "ground";
            });
            if (ground && ground.dimensions && ground.dimensions[0] > 0 && ground.dimensions[1] > 0) {
                return { width: ground.dimensions[0], height: ground.dimensions[1] };
            }
            const l0 = layers.find(l => l.index === 0);
            if (l0 && l0.dimensions && l0.dimensions[0] > 0 && l0.dimensions[1] > 0) {
                return { width: l0.dimensions[0], height: l0.dimensions[1] };
            }
            if (layers.length > 0 && layers[0].dimensions && layers[0].dimensions[0] > 0 && layers[0].dimensions[1] > 0) {
                return { width: layers[0].dimensions[0], height: layers[0].dimensions[1] };
            }
            if (cand.dimensions && Array.isArray(cand.dimensions) && cand.dimensions[0] > 0 && cand.dimensions[1] > 0) {
                return { width: cand.dimensions[0], height: cand.dimensions[1] };
            }
            if (cand.dimensions && cand.dimensions.width && cand.dimensions.height) {
                return { width: cand.dimensions.width, height: cand.dimensions.height };
            }
        }
        if (state.currentImage && state.currentImage.naturalWidth > 0) {
            return { width: state.currentImage.naturalWidth, height: state.currentImage.naturalHeight };
        }
        return { width: 2048, height: 2048 };
    }

    /**
     * Retrieves Ground layer dimensions for the current fullscreen map.
     */
    function getLibraryFullscreenDimensions() {
        if (state.libraryFullscreenMap) {
            const map = state.libraryFullscreenMap;
            if (map.layers && Array.isArray(map.layers) && map.layers.length > 0) {
                const ground = map.layers.find(l => {
                    const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    return r === "ground";
                });
                if (ground && ground.dimensions && ground.dimensions[0] > 0 && ground.dimensions[1] > 0) {
                    return { width: ground.dimensions[0], height: ground.dimensions[1] };
                }
                const l0 = map.layers.find(l => l.index === 0);
                if (l0 && l0.dimensions && l0.dimensions[0] > 0 && l0.dimensions[1] > 0) {
                    return { width: l0.dimensions[0], height: l0.dimensions[1] };
                }
                if (map.layers[0].dimensions && map.layers[0].dimensions[0] > 0 && map.layers[0].dimensions[1] > 0) {
                    return { width: map.layers[0].dimensions[0], height: map.layers[0].dimensions[1] };
                }
            }
            if (map.dimensions && Array.isArray(map.dimensions) && map.dimensions[0] > 0 && map.dimensions[1] > 0) {
                return { width: map.dimensions[0], height: map.dimensions[1] };
            }
            if (map.dimensions && map.dimensions.width && map.dimensions.height) {
                return { width: map.dimensions.width, height: map.dimensions.height };
            }
        }
        if (state.libraryFullscreenCurrentImage && state.libraryFullscreenCurrentImage.naturalWidth > 0) {
            return {
                width: state.libraryFullscreenCurrentImage.naturalWidth,
                height: state.libraryFullscreenCurrentImage.naturalHeight,
            };
        }
        return { width: 2048, height: 2048 };
    }

    /**
     * High-performance 60fps Canvas 2D render loop.
     * Applies devicePixelRatio scaling, pan/zoom transformation matrix,
     * draws the battlemap, and renders the single-draw-call 64px VTT grid overlay.
     */
    function render() {
        if (!ctx) return;
        const dpr = window.devicePixelRatio || 1;
        const viewWidth = canvas.clientWidth;
        const viewHeight = canvas.clientHeight;

        ctx.save();
        ctx.clearRect(0, 0, canvas.width, canvas.height);

        // Fill background
        ctx.fillStyle = "#070a10";
        ctx.fillRect(0, 0, canvas.width, canvas.height);

        // Normalize devicePixelRatio coordinates
        ctx.scale(dpr, dpr);

        if (state.currentImage && state.currentImage.complete && state.currentImage.naturalWidth > 0) {
            const img = state.currentImage;
            const imgW = img.naturalWidth || img.width;
            const imgH = img.naturalHeight || img.height;

            ctx.save();
            // Apply pan and zoom transformation
            ctx.translate(state.pan.x, state.pan.y);
            ctx.scale(state.zoom, state.zoom);

            // Crisp rendering: disable smoothing when zoomed in past 200% for pixel examination
            ctx.imageSmoothingEnabled = state.zoom < 2.0;
            ctx.imageSmoothingQuality = "high";

            // Onion-skin overlay: draw base Ground / layer 0 image with 50% opacity
            if (state.onionSkinActive && state.currentLayerIndex !== 0 && state.currentIndex >= 0) {
                const curCand = state.candidates[state.currentIndex];
                if (curCand) {
                    const groundKey = `${curCand.id}_layer_0`;
                    const groundImg = state.imageCache.get(groundKey) || state.imageCache.get(curCand.id);
                    if (groundImg && groundImg.complete && groundImg.naturalWidth > 0) {
                        const gW = groundImg.naturalWidth || groundImg.width;
                        const gH = groundImg.naturalHeight || groundImg.height;
                        ctx.save();
                        ctx.globalAlpha = 0.5;
                        ctx.drawImage(groundImg, 0, 0, gW, gH);
                        ctx.restore();
                    }
                }
            }

            // Draw active layer battlemap at origin (0, 0)
            ctx.globalAlpha = 1.0;
            ctx.drawImage(img, 0, 0, imgW, imgH);
            const isCalibrationActive = Boolean(state.gridPanelOpen || state.gridAlignMode || state.isDraggingGrid);
            // Draw crisp 64px VTT grid overlay in image space
            if (state.gridVisible || isCalibrationActive) {
                drawVttGrid(ctx, imgW, imgH, isCalibrationActive);
            }

            // Draw image bounding border
            ctx.beginPath();
            ctx.rect(0, 0, imgW, imgH);
            ctx.lineWidth = Math.max(1.0, 1.5 / state.zoom);
            ctx.strokeStyle = "rgba(56, 189, 248, 0.75)";
            ctx.stroke();

            ctx.restore();
        }

        ctx.restore();
    }

    /**
     * Single-draw-call crisp 64px VTT grid overlay.
     * Evaluates 5ft D&D tactical scale, door alignment, and poché wall consistency.
     */
    function drawVttGrid(c, width, height, isCalibrationActive = false) {
        c.save();
        c.beginPath();
        c.rect(0, 0, width, height);
        c.clip();

        const size = state.gridSize || VTT_GRID_SIZE;
        const ox = ((Math.round(state.gridOffset.x) % size) + size) % size;
        const oy = ((Math.round(state.gridOffset.y) % size) + size) % size;
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

        // Inversely scale line width with zoom so grid remains ~1 screen pixel thin
        c.lineWidth = Math.max(0.5, 1.0 / state.zoom);
        c.strokeStyle = "rgba(56, 189, 248, 0.4)";
        c.stroke();

        // Draw boundary border around active full grid
        if (isCalibrationActive && cols > 0 && rows > 0) {
            c.save();
            c.beginPath();
            c.rect(ox, oy, fullW, fullH);
            c.lineWidth = Math.max(1.5, 2.5 / state.zoom);
            const isEligible = cols >= 16 && rows >= 16;
            c.strokeStyle = isEligible ? "rgba(56, 189, 248, 0.95)" : "rgba(244, 63, 94, 0.95)";
            c.stroke();
            c.restore();
        }

        c.restore();
    }

    /**
     * Fits the current battlemap image centered within the viewport.
     */
    function fitToScreen() {
        if (!state.currentImage) return;
        const img = state.currentImage;
        const imgW = img.naturalWidth || img.width;
        const imgH = img.naturalHeight || img.height;
        const viewW = canvas.clientWidth;
        const viewH = canvas.clientHeight;

        if (!imgW || !imgH || !viewW || !viewH) return;

        const padding = 32;
        const availW = Math.max(20, viewW - padding * 2);
        const availH = Math.max(20, viewH - padding * 2);

        // Fit within viewport bounds, capped at 100% on initial load
        const fitScale = Math.min(availW / imgW, availH / imgH, 1.0);
        state.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, fitScale));

        // Center in viewport
        state.pan.x = Math.round((viewW - imgW * state.zoom) / 2);
        state.pan.y = Math.round((viewH - imgH * state.zoom) / 2);

        requestRender();
        updateHud();
    }

    function zoomStep(factor, clientPoint = null) {
        if (!state.currentImage) return;
        const prevZoom = state.zoom;
        let newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
        newZoom = Math.round(newZoom * 1000) / 1000;

        if (newZoom === prevZoom) return;

        const viewW = canvas.clientWidth;
        const viewH = canvas.clientHeight;
        const focusX = clientPoint ? clientPoint.x : viewW / 2;
        const focusY = clientPoint ? clientPoint.y : viewH / 2;

        // Keep the focal point stationary during zoom:
        state.pan.x = focusX - (focusX - state.pan.x) * (newZoom / prevZoom);
        state.pan.y = focusY - (focusY - state.pan.y) * (newZoom / prevZoom);
        state.zoom = newZoom;

        requestRender();
        updateHud();
    }

    // ==========================================================================
    // Viewport Panning Loop & Hotkey Controls
    // ==========================================================================

    const activePanKeys = { w: false, a: false, s: false, d: false };
    let panAnimationId = null;
    let lastPanTimestamp = 0;
    const PAN_SPEED = 700; // pixels per second

    function panLoop(timestamp) {
        if (!lastPanTimestamp) lastPanTimestamp = timestamp;
        const dt = Math.min((timestamp - lastPanTimestamp) / 1000, 0.1);
        lastPanTimestamp = timestamp;

        let moveX = 0;
        let moveY = 0;
        if (activePanKeys.w) moveY += 1;
        if (activePanKeys.s) moveY -= 1;
        if (activePanKeys.a) moveX += 1;
        if (activePanKeys.d) moveX -= 1;

        if (moveX !== 0 || moveY !== 0) {
            if (moveX !== 0 && moveY !== 0) {
                const invSqrt2 = 0.70710678;
                moveX *= invSqrt2;
                moveY *= invSqrt2;
            }
            if (state.currentImage) {
                state.pan.x += moveX * PAN_SPEED * dt;
                state.pan.y += moveY * PAN_SPEED * dt;
                requestRender();
                updateHud();
            }
            panAnimationId = requestAnimationFrame(panLoop);
        } else {
            panAnimationId = null;
            lastPanTimestamp = 0;
        }
    }

    function startPanLoop() {
        if (!panAnimationId) {
            lastPanTimestamp = performance.now();
            panAnimationId = requestAnimationFrame(panLoop);
        }
    }

    function stopPanLoop() {
        if (panAnimationId) {
            cancelAnimationFrame(panAnimationId);
            panAnimationId = null;
        }
        lastPanTimestamp = 0;
        activePanKeys.w = false;
        activePanKeys.a = false;
        activePanKeys.s = false;
        activePanKeys.d = false;
    }

    function panStep(dx, dy) {
        if (!state.currentImage) return;
        state.pan.x += dx;
        state.pan.y += dy;
        requestRender();
        updateHud();
    }

    // ==========================================================================
    // Viewport Zooming Loop & Hotkey Controls
    // ==========================================================================

    const activeZoomKeys = { in: false, out: false };
    let zoomAnimationId = null;
    let lastZoomTimestamp = 0;
    const ZOOM_SPEED = 2.0;

    function zoomLoop(timestamp) {
        if (!lastZoomTimestamp) lastZoomTimestamp = timestamp;
        const dt = Math.min((timestamp - lastZoomTimestamp) / 1000, 0.1);
        lastZoomTimestamp = timestamp;

        if (activeZoomKeys.in || activeZoomKeys.out) {
            let factor = 1.0;
            if (activeZoomKeys.in && !activeZoomKeys.out) {
                factor = Math.pow(ZOOM_SPEED, dt);
            } else if (activeZoomKeys.out && !activeZoomKeys.in) {
                factor = Math.pow(1 / ZOOM_SPEED, dt);
            }

            if (factor !== 1.0 && state.currentImage) {
                const prevZoom = state.zoom;
                const newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, prevZoom * factor));
                if (newZoom !== prevZoom) {
                    const focusX = canvas.clientWidth / 2;
                    const focusY = canvas.clientHeight / 2;
                    state.pan.x = focusX - (focusX - state.pan.x) * (newZoom / prevZoom);
                    state.pan.y = focusY - (focusY - state.pan.y) * (newZoom / prevZoom);
                    state.zoom = newZoom;
                    requestRender();
                    updateHud();
                }
            }
            zoomAnimationId = requestAnimationFrame(zoomLoop);
        } else {
            zoomAnimationId = null;
            lastZoomTimestamp = 0;
        }
    }

    function startZoomLoop() {
        if (!zoomAnimationId) {
            lastZoomTimestamp = performance.now();
            zoomAnimationId = requestAnimationFrame(zoomLoop);
        }
    }

    function stopZoomLoop() {
        if (zoomAnimationId) {
            cancelAnimationFrame(zoomAnimationId);
            zoomAnimationId = null;
        }
        lastZoomTimestamp = 0;
        activeZoomKeys.in = false;
        activeZoomKeys.out = false;
    }

    // ==========================================================================
    // Pre-fetching & Candidate Queue Management
    // ==========================================================================

    async function fetchStagingQueue() {
        showLoading(true, "Fetching staging queue...");
        try {
            const res = await fetch("/api/staging");
            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }
            const data = await res.json();
            // A candidate whose curation request is still in flight is already gone from the
            // reviewer's local queue; do not resurrect it from the server's staging listing.
            state.candidates = (Array.isArray(data) ? data : []).filter(
                (c) => !state.pendingCurationIds.has(c.id)
            );

            if (state.candidates.length > 0) {
                loadCandidate(0);
            } else {
                state.currentIndex = -1;
                state.currentImage = null;
                showEmptyState(true);
                showLoading(false);
                updateHud();
            }
        } catch (err) {
            showToast(`Failed to load staging queue: ${err.message}`, "danger");
            showLoading(false);
            showEmptyState(true);
        }
    }

    /**
     * Loads candidate by index, leveraging in-memory prefetch cache for 0ms transitions.
     */
    function loadCandidate(index) {
        exitLabelingMode();
        if (index < 0 || index >= state.candidates.length) {
            state.currentIndex = -1;
            state.currentLayerIndex = 0;
            state.currentImage = null;
            requestRender();
            renderLayerDeck();
            updateHud();
            showEmptyState(state.candidates.length === 0);
            return;
        }

        showEmptyState(false);
        state.currentIndex = index;
        state.currentLayerIndex = 0;
        const candidate = state.candidates[index];
        fetchThemeSuggestions(candidate);
        if (candidate.grid) {
            state.gridSize = candidate.grid.cell_size_px ?? 64;
            state.gridOffset = {
                x: candidate.grid.offset_x ?? 0,
                y: candidate.grid.offset_y ?? 0,
            };
        } else {
            state.gridSize = 64;
            state.gridOffset = { x: 0, y: 0 };
        }
        updateGridPanel();
        const cacheKey = `${candidate.id}_layer_0`;

        // Check if layer 0 (or candidate) is already cached in memory
        let cachedImg = state.imageCache.get(cacheKey) || state.imageCache.get(candidate.id);

        if (cachedImg && cachedImg.complete && cachedImg.naturalWidth > 0) {
            // Instant 0ms visual delay transition!
            state.currentImage = cachedImg;
            showLoading(false);
            fitToScreen();
            renderLayerDeck();
            prefetchUpcoming();
            updateHud();
        } else {
            showLoading(true, `Loading candidate #${candidate.id}...`);
            if (!cachedImg) {
                cachedImg = new Image();
                cachedImg.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/0`;
                state.imageCache.set(cacheKey, cachedImg);
                state.imageCache.set(candidate.id, cachedImg);
            }

            cachedImg.onload = () => {
                // Ensure candidate hasn't shifted while loading
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    state.currentImage = cachedImg;
                    showLoading(false);
                    fitToScreen();
                    renderLayerDeck();
                    prefetchUpcoming();
                    updateHud();
                }
            };

            cachedImg.onerror = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    showLoading(false);
                    showToast(`Error loading candidate ${candidate.id}`, "danger");
                }
            };
        }
    }

    /**
     * In-memory pre-fetching of all layers for active candidate, upcoming 3 candidates,
     * and previous 1 candidate for instant, zero-latency transitions upon hotkeys.
     */
    function prefetchUpcoming() {
        let readyCount = 0;

        // Prefetch all layers of current candidate
        if (state.currentIndex >= 0 && state.currentIndex < state.candidates.length) {
            const curCand = state.candidates[state.currentIndex];
            const curLayers = getCandidateLayers(curCand);
            curLayers.forEach((l, idx) => {
                const key = `${curCand.id}_layer_${idx}`;
                let img = state.imageCache.get(key);
                if (!img) {
                    img = new Image();
                    img.src = `/api/image/staging/${encodeURIComponent(curCand.id)}/${idx}`;
                    state.imageCache.set(key, img);
                    img.addEventListener("load", () => updateHud(), { once: true });
                }
                if (img.complete && img.naturalWidth > 0) readyCount++;
            });
            if (!state.imageCache.get(curCand.id)) {
                const l0 = state.imageCache.get(`${curCand.id}_layer_0`);
                if (l0) state.imageCache.set(curCand.id, l0);
            }
        }

        // Prefetch next candidates
        for (let offset = 1; offset <= PREFETCH_AHEAD; offset++) {
            const nextIdx = state.currentIndex + offset;
            if (nextIdx < state.candidates.length) {
                const nextCand = state.candidates[nextIdx];
                fetchThemeSuggestions(nextCand);
                const layers = getCandidateLayers(nextCand);
                layers.forEach((l, idx) => {
                    const key = `${nextCand.id}_layer_${idx}`;
                    let img = state.imageCache.get(key);
                    if (!img) {
                        img = new Image();
                        img.src = `/api/image/staging/${encodeURIComponent(nextCand.id)}/${idx}`;
                        state.imageCache.set(key, img);
                        img.addEventListener("load", () => updateHud(), { once: true });
                    }
                    if (img.complete && img.naturalWidth > 0) readyCount++;
                });
                if (!state.imageCache.get(nextCand.id)) {
                    const l0 = state.imageCache.get(`${nextCand.id}_layer_0`);
                    if (l0) state.imageCache.set(nextCand.id, l0);
                }
            }
        }

        // Prefetch previous 1 candidate
        if (state.currentIndex > 0) {
            const prevCand = state.candidates[state.currentIndex - 1];
            const layers = getCandidateLayers(prevCand);
            layers.forEach((l, idx) => {
                const key = `${prevCand.id}_layer_${idx}`;
                let prevImg = state.imageCache.get(key);
                if (!prevImg) {
                    prevImg = new Image();
                    prevImg.src = `/api/image/staging/${encodeURIComponent(prevCand.id)}/${idx}`;
                    state.imageCache.set(key, prevImg);
                    prevImg.addEventListener("load", () => updateHud(), { once: true });
                }
                if (prevImg.complete && prevImg.naturalWidth > 0) readyCount++;
            });
            if (!state.imageCache.get(prevCand.id)) {
                const l0 = state.imageCache.get(`${prevCand.id}_layer_0`);
                if (l0) state.imageCache.set(prevCand.id, l0);
            }
        }

        return readyCount;
    }

    function navigateNext() {
        if (state.currentIndex < state.candidates.length - 1) {
            loadCandidate(state.currentIndex + 1);
        } else {
            showToast("Reached end of staging queue", "info");
        }
    }

    function navigatePrev() {
        if (state.currentIndex > 0) {
            loadCandidate(state.currentIndex - 1);
        } else {
            showToast("At beginning of staging queue", "info");
        }
    }

    // ==========================================================================
    // Multi-Layer Canvas Triage Deck & Hotkey Controls
    // ==========================================================================

    function getCandidateLayers(candidate) {
        if (!candidate) return [];
        if (candidate.layers && candidate.layers.length > 0) {
            return candidate.layers;
        }
        return [{
            index: 0,
            filename: candidate.filename || `${candidate.id}.png`,
            role: "ground",
            elevation: 0,
            dimensions: candidate.dimensions || [2048, 2048],
        }];
    }

    function formatRoleName(role, elevation) {
        const r = (role || "unassigned").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") return "Ground (0)";
        if (r === "upperfloor" || r === "upper") {
            const elev = typeof elevation === "number" ? elevation : 1;
            return `Upper (${elev >= 0 ? `+${elev}` : elev})`;
        }
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
            const elev = typeof elevation === "number" ? elevation : -1;
            return `Basement (${elev >= 0 ? `+${elev}` : elev})`;
        }
        if (r === "roof") return "Roof";
        if (r === "variant") return "Variant";
        if (r === "exclude" || r === "excluded") return "Exclude";
        return "Unassigned";
    }

    function getRoleCssClass(role) {
        const r = (role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") return "ground";
        if (r === "upperfloor" || r === "upper") return "upper_floor";
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "basement";
        if (r === "roof") return "roof";
        if (r === "variant") return "variant";
        if (r === "exclude" || r === "excluded") return "exclude";
        return "unassigned";
    }

    /**
     * Synchronized in-place layer switching.
     * Preserves state.pan and state.zoom exactly without resetting the viewport!
     */
    function setLayer(layerIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (layerIndex < 0 || layerIndex >= layers.length) return;

        state.currentLayerIndex = layerIndex;
        const cacheKey = `${candidate.id}_layer_${layerIndex}`;
        let cachedImg = state.imageCache.get(cacheKey);

        if (cachedImg && cachedImg.complete && cachedImg.naturalWidth > 0) {
            // Instant 0ms transition with camera pan and zoom preserved!
            state.currentImage = cachedImg;
            requestRender();
            renderLayerDeck();
            updateHud();
        } else {
            showLoading(true, `Loading layer #${layerIndex}...`);
            if (!cachedImg) {
                cachedImg = new Image();
                cachedImg.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/${layerIndex}`;
                state.imageCache.set(cacheKey, cachedImg);
            }
            cachedImg.onload = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id && state.currentLayerIndex === layerIndex) {
                    state.currentImage = cachedImg;
                    showLoading(false);
                    requestRender();
                    renderLayerDeck();
                    updateHud();
                }
            };
            cachedImg.onerror = () => {
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    showLoading(false);
                    showToast(`Error loading layer ${layerIndex}`, "danger");
                }
            };
        }
    }

    /**
     * Cycles to next or previous layer within the current bundle without resetting camera.
     */
    function cycleLayer(delta) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (!layers || layers.length <= 1) return;

        const nextIdx = (state.currentLayerIndex + delta + layers.length) % layers.length;
        setLayer(nextIdx);
    }

    /**
     * Renders the left-side layer deck, filmstrip cards, and role selectors.
     */
    function renderLayerDeck() {
        if (!layerDeck) return;
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            layerDeck.classList.add("hidden");
            if (btnSplitBundleHud) btnSplitBundleHud.classList.add("hidden");
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);

        // Update counter
        if (layerDeckCount) {
            layerDeckCount.textContent = `${state.currentLayerIndex + 1} / ${layers.length}`;
        }

        const isSplitChild = Boolean(candidate.parent_group_id);
        const isMultiLayer = layers.length > 1;

        if (layerDeckBundleTag) {
            if (isSplitChild) {
                layerDeckBundleTag.textContent = "Split Child";
                layerDeckBundleTag.className = "badge badge-bundle";
            } else if (isMultiLayer) {
                layerDeckBundleTag.textContent = "Multi-Layer Bundle";
                layerDeckBundleTag.className = "badge badge-bundle";
            } else {
                layerDeckBundleTag.textContent = "Standalone Map";
                layerDeckBundleTag.className = "badge";
            }
        }

        if (btnSplitBundle) {
            if (isSplitChild) {
                btnSplitBundle.classList.remove("hidden");
                if (btnSplitLabel) btnSplitLabel.textContent = "Regroup Bundle";
                btnSplitBundle.title = "Regroup split candidates back into bundle (Hotkey: U)";
            } else if (isMultiLayer) {
                btnSplitBundle.classList.remove("hidden");
                if (btnSplitLabel) btnSplitLabel.textContent = "Split Bundle";
                btnSplitBundle.title = "Split multi-layer bundle into standalone maps (Hotkey: U)";
            } else {
                btnSplitBundle.classList.add("hidden");
            }
        }

        if (btnSplitBundleHud) {
            if (isSplitChild) {
                btnSplitBundleHud.classList.remove("hidden");
                if (btnSplitHudText) btnSplitHudText.textContent = "Regroup";
                btnSplitBundleHud.title = "Regroup split candidates back into bundle (Hotkey: U)";
            } else if (isMultiLayer) {
                btnSplitBundleHud.classList.remove("hidden");
                if (btnSplitHudText) btnSplitHudText.textContent = "Split";
                btnSplitBundleHud.title = "Split multi-layer bundle into standalone maps (Hotkey: U)";
            } else {
                btnSplitBundleHud.classList.add("hidden");
            }
        }

        // Show layer deck container
        layerDeck.classList.remove("hidden");

        // Sync active state in Role Selector HUD
        const activeLayer = layers[state.currentLayerIndex] || layers[0];
        const activeRoleCls = getRoleCssClass(activeLayer?.role);
        const roleBadges = roleSelector ? roleSelector.querySelectorAll(".role-badge") : [];
        roleBadges.forEach((b) => {
            if (b.getAttribute("data-role") === activeRoleCls) {
                b.classList.add("active");
            } else {
                b.classList.remove("active");
            }
        });

        // Sync Onion-Skin button
        if (btnOnionSkin) {
            if (state.onionSkinActive) {
                btnOnionSkin.classList.add("active");
            } else {
                btnOnionSkin.classList.remove("active");
            }
        }
        if (onionSkinBadge) {
            if (state.onionSkinActive) {
                onionSkinBadge.classList.remove("hidden");
            } else {
                onionSkinBadge.classList.add("hidden");
            }
        }

        // Render layer cards into filmstrip
        if (!layerFilmstrip) return;
        layerFilmstrip.innerHTML = "";

        layers.forEach((l, idx) => {
            const card = document.createElement("div");
            const roleCls = getRoleCssClass(l.role);
            const isUnassigned = roleCls === "unassigned";
            const isExcluded = roleCls === "exclude";
            const isActive = idx === state.currentLayerIndex;

            card.className = `layer-card${isActive ? " active" : ""}${isUnassigned ? " unassigned" : ""}${isExcluded ? " excluded" : ""}`;
            card.setAttribute("role", "tab");
            card.setAttribute("aria-selected", isActive ? "true" : "false");
            card.setAttribute("tabindex", "0");
            card.title = `Layer #${idx}: ${formatRoleName(l.role, l.elevation)}${l.caption ? ` — ${l.caption}` : ""}`;

            const thumbContainer = document.createElement("div");
            thumbContainer.className = "layer-thumb-container";

            const idxBadge = document.createElement("span");
            idxBadge.className = "layer-index-badge";
            idxBadge.textContent = `#${l.index !== undefined ? l.index : idx}`;
            thumbContainer.appendChild(idxBadge);

            const img = document.createElement("img");
            img.className = "layer-thumb-img";
            img.src = `/api/image/staging/${encodeURIComponent(candidate.id)}/${idx}`;
            img.alt = `Layer ${idx}`;
            img.loading = "lazy";
            thumbContainer.appendChild(img);
            card.appendChild(thumbContainer);
            const infoEl = document.createElement("div");
            infoEl.className = "layer-card-info";

            if (l.caption) {
                const captionEl = document.createElement("div");
                captionEl.className = "layer-caption-tooltip";
                captionEl.textContent = l.caption;
                infoEl.appendChild(captionEl);
            }

            const metaEl = document.createElement("div");
            metaEl.className = "layer-meta";
            const pill = document.createElement("span");
            pill.className = `layer-role-pill role-${roleCls}`;
            pill.textContent = formatRoleName(l.role, l.elevation);
            pill.title = "Click to set role (Hotkey: 3)";
            pill.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                enterLabelingMode("role");
            });
            metaEl.appendChild(pill);

            const steppers = document.createElement("div");
            steppers.className = "layer-elevation-steppers";

            const btnDown = document.createElement("button");
            btnDown.type = "button";
            btnDown.className = "btn-elev-stepper";
            btnDown.textContent = "−";
            btnDown.title = "Lower elevation (Alt+Down)";
            btnDown.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                stepLayerElevation(-1, idx);
            });
            steppers.appendChild(btnDown);

            const btnUp = document.createElement("button");
            btnUp.type = "button";
            btnUp.className = "btn-elev-stepper";
            btnUp.textContent = "+";
            btnUp.title = "Raise elevation (Alt+Up)";
            btnUp.addEventListener("click", (e) => {
                e.stopPropagation();
                setLayer(idx);
                stepLayerElevation(1, idx);
            });
            steppers.appendChild(btnUp);

            metaEl.appendChild(steppers);
            infoEl.appendChild(metaEl);
            card.appendChild(infoEl);
            card.addEventListener("click", () => {
                setLayer(idx);
            });

            layerFilmstrip.appendChild(card);
        });
    }

    /**
     * Stepper adjustment for active layer floor elevation (Alt+Up / Alt+Down).
     */
    async function stepLayerElevation(delta, targetIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        const idx = targetIndex !== undefined ? targetIndex : state.currentLayerIndex;
        if (idx < 0 || idx >= layers.length) return;

        const layer = layers[idx];
        const curElev = typeof layer.elevation === "number" ? layer.elevation : 0;
        const newElev = curElev + delta;
        layer.elevation = newElev;

        // Auto-adjust role based on elevation if currently Upper, Basement, Ground, or Unassigned
        const r = (layer.role || "").toLowerCase().replace(/[-_]/g, "");
        if (newElev > 0 && (r === "ground" || r === "basement" || r === "unassigned")) {
            layer.role = "upper_floor";
        } else if (newElev < 0 && (r === "ground" || r === "upperfloor" || r === "upper" || r === "unassigned")) {
            layer.role = "basement";
        } else if (newElev === 0 && (r === "upperfloor" || r === "upper" || r === "basement" || r === "unassigned")) {
            layer.role = "ground";
        }

        candidate.layers = layers;
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Assigns role to current layer with auto-increment/decrement of floor numbers.
     */
    async function assignRole(roleName, targetIndex) {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        const idx = targetIndex !== undefined ? targetIndex : state.currentLayerIndex;
        if (idx < 0 || idx >= layers.length) return;

        const layer = layers[idx];
        const r = roleName.toLowerCase().replace(/[-_]/g, "");

        let elevation = null;
        if (r === "ground") {
            elevation = 0;
        } else if (r === "upperfloor" || r === "upper") {
            // Auto-increment: find highest existing UpperFloor in bundle
            let maxUpper = 0;
            layers.forEach((l, i) => {
                if (i !== idx) {
                    const lr = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    if (lr === "upperfloor" || lr === "upper") {
                        const elev = typeof l.elevation === "number" ? l.elevation : 0;
                        if (elev > maxUpper) maxUpper = elev;
                    }
                }
            });
            elevation = maxUpper + 1;
        } else if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
            // Auto-decrement: find lowest existing Basement in bundle
            let minBasement = 0;
            layers.forEach((l, i) => {
                if (i !== idx) {
                    const lr = (l.role || "").toLowerCase().replace(/[-_]/g, "");
                    if (lr === "basement" || lr === "lower" || lr === "lowerfloor" || lr === "cellar") {
                        const elev = typeof l.elevation === "number" ? l.elevation : 0;
                        if (elev < minBasement) minBasement = elev;
                    }
                }
            });
            elevation = minBasement - 1;
        } else {
            // Roof, Variant, Exclude, Unassigned
            elevation = null;
        }

        layer.role = roleName;
        layer.elevation = elevation;
        candidate.layers = layers;
        exitLabelingMode();
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Space hotkey: Quick-toggle current layer between Exclude and assigned/inferred role.
     */
    async function toggleActiveLayerExclude() {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        const layers = getCandidateLayers(candidate);
        if (!layers || layers.length === 0) return;

        const layer = layers[state.currentLayerIndex];
        if (!layer) return;

        const r = (layer.role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "exclude" || r === "excluded") {
            // Re-include layer
            const prevRole = layer._prevRole || (layer.index === 0 ? "ground" : "upper_floor");
            const prevElev = layer._prevElevation !== undefined ? layer._prevElevation : (prevRole === "ground" ? 0 : 1);
            layer.role = prevRole;
            layer.elevation = prevElev;
            showToast(`Layer #${state.currentLayerIndex} included as ${formatRoleName(prevRole, prevElev)}`, "info");
        } else {
            // Exclude layer
            layer._prevRole = layer.role;
            layer._prevElevation = layer.elevation;
            layer.role = "exclude";
            layer.elevation = null;
            showToast(`Layer #${state.currentLayerIndex} marked as Exclude`, "info");
        }

        candidate.layers = layers;
        renderLayerDeck();
        await persistCandidateLayers(candidate);
    }

    /**
     * Persists updated layer metadata to /api/staging/{id}/layers.
     */
    async function persistCandidateLayers(candidate) {
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(candidate.id)}/layers`, {
                method: "PATCH",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ layers: candidate.layers }),
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const updated = await res.json();
            candidate.layers = updated.layers;
            renderLayerDeck();
        } catch (err) {
            showToast(`Failed to persist layer metadata: ${err.message}`, "danger");
        }
    }

    /**
     * Decomposes multi-layer bundle into standalone candidates via /api/staging/{id}/split.
     */
    async function splitCandidate(id) {
        if (state.isSubmitting) return;
        state.isSubmitting = true;
        showLoading(true, "Splitting bundle into standalone maps...");
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(id)}/split`, {
                method: "POST",
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const splitCandidates = await res.json();
            const idx = state.candidates.findIndex((c) => c.id === id);
            if (idx !== -1) {
                state.candidates.splice(idx, 1, ...splitCandidates);
                state.lastSplitParentId = id;
                loadCandidate(idx);
                showToast("Bundle split into standalone maps. Press U or click Undo to regroup.", "info");
            }
        } catch (err) {
            showToast(`Split failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
            showLoading(false);
        }
    }

    /**
     * Recombines split candidates back into single bundle via /api/staging/{parent_id}/regroup.
     */
    async function regroupCandidate(parentId) {
        if (state.isSubmitting) return;
        state.isSubmitting = true;
        showLoading(true, "Regrouping multi-layer bundle...");
        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(parentId)}/regroup`, {
                method: "POST",
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.message || `HTTP ${res.status}`);
            }
            const regroupedCandidate = await res.json();

            // Find all split children belonging to parentId
            const firstIdx = state.candidates.findIndex(
                (c) => c.parent_group_id === parentId || c.id === parentId || c.id.startsWith(`${parentId}_`)
            );
            if (firstIdx !== -1) {
                const count = state.candidates.filter(
                    (c) => c.parent_group_id === parentId || c.id === parentId || c.id.startsWith(`${parentId}_`)
                ).length;
                state.candidates.splice(firstIdx, Math.max(1, count), regroupedCandidate);
                state.lastSplitParentId = null;
                loadCandidate(firstIdx);
                showToast("Regrouped multi-layer bundle", "success");
            }
        } catch (err) {
            showToast(`Regroup failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
            showLoading(false);
        }
    }

    /**
     * Toggles between split bundle and regroup depending on current candidate state.
     */
    function handleSplitOrRegroup() {
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        const candidate = state.candidates[state.currentIndex];
        if (candidate.parent_group_id) {
            regroupCandidate(candidate.parent_group_id);
        } else if (candidate.layers && candidate.layers.length > 1) {
            splitCandidate(candidate.id);
        } else if (state.lastSplitParentId) {
            regroupCandidate(state.lastSplitParentId);
        } else {
            showToast("Candidate is not a multi-layer bundle", "info");
        }
    }

    /**
     * Toggles 50% opacity Onion-Skin overlay of base Ground layer.
     */
    function toggleOnionSkin() {
        state.onionSkinActive = !state.onionSkinActive;
        if (btnOnionSkin) {
            if (state.onionSkinActive) {
                btnOnionSkin.classList.add("active");
            } else {
                btnOnionSkin.classList.remove("active");
            }
        }
        if (onionSkinBadge) {
            if (state.onionSkinActive) {
                onionSkinBadge.classList.remove("hidden");
            } else {
                onionSkinBadge.classList.add("hidden");
            }
        }
        requestRender();
        showToast(`Onion-Skin overlay: ${state.onionSkinActive ? "Active (50% Ground)" : "Disabled"}`, "info");
    }

    // ==========================================================================
    // Curation & Rejection Actions
    // ==========================================================================

    /**
     * Proactively pre-fetches AI vision theme suggestions for candidate in background.
     * Caches suggestions on candidate._themeSuggestions or flags candidate._suggestionsOffline.
     */
    function fetchThemeSuggestions(candidate) {
        if (!candidate) {
            return Promise.resolve([]);
        }
        if (candidate._themeSuggestions !== undefined) {
            return Promise.resolve(candidate._themeSuggestions);
        }
        if (candidate._fetchPromise) {
            return candidate._fetchPromise;
        }

        candidate._fetchPromise = fetch("/api/staging/" + encodeURIComponent(candidate.id) + "/suggest-themes", {
            method: "POST",
        })
            .then((res) => {
                if (!res.ok) throw new Error(`HTTP ${res.status}`);
                return res.json();
            })
            .then((data) => {
                const suggestions = Array.isArray(data.suggestions) ? data.suggestions : [];
                candidate._themeSuggestions = suggestions;
                candidate._suggestionsOffline = suggestions.length === 0;
                return candidate._themeSuggestions;
            })
            .catch((err) => {
                console.warn(`Vision theme suggestions offline/failed for candidate ${candidate.id}:`, err);
                candidate._themeSuggestions = [];
                candidate._suggestionsOffline = true;
                return [];
            })
            .finally(() => {
                candidate._fetchPromise = null;
            });

        return candidate._fetchPromise;
    }

    let currentDescribeAbortController = null;

    /**
     * Asynchronously requests vision description for candidate.
     * @param {Object} candidate
     * @param {boolean} force - if true, bypasses candidate._description cache
     */
    async function fetchCandidateDescription(candidate, force = false) {
        if (!candidate) return;

        if (currentDescribeAbortController) {
            currentDescribeAbortController.abort();
            currentDescribeAbortController = null;
        }

        if (!force && candidate._description) {
            if (decisionDescriptionInput) {
                decisionDescriptionInput.value = candidate._description;
                decisionDescriptionInput.classList.remove("hidden");
            }
            if (decisionDescriptionLoading) {
                decisionDescriptionLoading.classList.add("hidden");
            }
            if (decisionDescriptionStatus) {
                decisionDescriptionStatus.className = "decision-description-indicator status-dot status-ready";
                decisionDescriptionStatus.title = "Description ready";
            }
            return candidate._description;
        }

        if (decisionDescriptionLoading) {
            decisionDescriptionLoading.classList.remove("hidden");
        }
        if (decisionDescriptionInput) {
            decisionDescriptionInput.classList.add("hidden");
        }
        if (btnRegenerateDescription) {
            btnRegenerateDescription.disabled = true;
        }
        if (decisionDescriptionStatus) {
            decisionDescriptionStatus.className = "decision-description-indicator status-dot status-processing";
            decisionDescriptionStatus.title = "Generating description with Gemini 3.5 Flash Lite...";
        }

        const controller = new AbortController();
        currentDescribeAbortController = controller;

        const payload = {
            tags: decisionState.acceptedTags && decisionState.acceptedTags.length > 0
                ? decisionState.acceptedTags
                : (candidate.tags || []),
        };

        try {
            const res = await fetch(`/api/staging/${encodeURIComponent(candidate.id)}/describe`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
                signal: controller.signal,
            });

            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }

            const data = await res.json();
            const desc = (data && typeof data.description === "string") ? data.description.trim() : "";
            candidate._description = desc;

            if (decisionState.candidate && decisionState.candidate.id === candidate.id) {
                if (decisionDescriptionInput) {
                    decisionDescriptionInput.value = desc;
                    decisionDescriptionInput.classList.remove("hidden");
                }
                if (decisionDescriptionLoading) {
                    decisionDescriptionLoading.classList.add("hidden");
                }
                if (decisionDescriptionStatus) {
                    decisionDescriptionStatus.className = "decision-description-indicator status-dot status-ready";
                    decisionDescriptionStatus.title = "Description generated";
                }
            }
            return desc;
        } catch (err) {
            if (err.name === "AbortError") {
                return;
            }
            console.warn(`Vision description failed for candidate ${candidate.id}:`, err);
            if (decisionState.candidate && decisionState.candidate.id === candidate.id) {
                if (decisionDescriptionInput) {
                    if (!decisionDescriptionInput.value.trim()) {
                        decisionDescriptionInput.value = candidate.description || (candidate.layers && candidate.layers[0]?.caption) || "";
                    }
                    decisionDescriptionInput.classList.remove("hidden");
                }
                if (decisionDescriptionLoading) {
                    decisionDescriptionLoading.classList.add("hidden");
                }
                if (decisionDescriptionStatus) {
                    decisionDescriptionStatus.className = "decision-description-indicator status-dot status-offline";
                    decisionDescriptionStatus.title = "AI vision description unavailable / fallback";
                }
            }
        } finally {
            if (currentDescribeAbortController === controller) {
                currentDescribeAbortController = null;
            }
            if (btnRegenerateDescription) {
                btnRegenerateDescription.disabled = false;
            }
        }
    }
    /**
     * Curate Validation Gate:
     * 1. If candidate has layers and all layers are excluded (role === 'exclude'), block curation.
     * 2. If candidate has any included layer with role unassigned, block curation, focus that layer
     *    in the filmstrip deck, and enter role labeling mode.
     * Returns true if candidate layers pass validation, false otherwise.
     */
    function validateCandidateLayers(candidate) {
        if (!candidate) return false;
        const layers = getCandidateLayers(candidate);

        // 1. If all layers are Exclude, block curation
        const allExcluded = layers.length > 0 && layers.every((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "exclude" || r === "excluded";
        });
        if (allExcluded) {
            showToast("Cannot curate: at least 1 layer must be included", "danger");
            return false;
        }

        // 2. If any included layer has role Unassigned, block curation
        const unassignedIdx = layers.findIndex((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r !== "exclude" && r !== "excluded" && (r === "unassigned" || !r);
        });
        if (unassignedIdx !== -1) {
            const unassignedLayer = layers[unassignedIdx];
            const layerNum = unassignedLayer.index !== undefined ? unassignedLayer.index : unassignedIdx;
            showToast(`Cannot curate: layer ${layerNum} is Unassigned. Please assign a role.`, "danger");
            setLayer(unassignedIdx);
            if (layerFilmstrip && layerFilmstrip.children[unassignedIdx]) {
                const card = layerFilmstrip.children[unassignedIdx];
                card.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
                if (typeof card.focus === "function") {
                    card.focus();
                }
            }
            enterLabelingMode("role");
            return false;
        }
        // 3. Strict Ground layer requirement: candidate must have exactly one Ground layer (role ground)
        const groundCount = layers.filter((l) => {
            const r = (l.role || "").toLowerCase().replace(/[-_]/g, "");
            return r === "ground";
        }).length;
        if (groundCount === 0) {
            showToast("Cannot curate: at least one layer must be assigned as Ground floor before curating.", "danger");
            return false;
        }
        if (groundCount > 1) {
            showToast("Cannot curate: only one layer can be designated as Ground floor.", "danger");
            return false;
        }

        return true;
    }

    /**
     * Opens the interactive curate decision modal with vision suggestion cards
     * and custom tag input workflow.
     */
    async function openCurateDecisionModal() {
        stopPanLoop();
        stopZoomLoop();
        exitLabelingMode();
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        if (!validateCandidateLayers(candidate)) {
            return;
        }

        // 16×16 Full-Grid Lattice Curation Gate
        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (!lattice.isEligible) {
            showToast(`Map has only ${lattice.cols}×${lattice.rows} full Grid Cells; at least 16×16 required for curation`, "danger");
            toggleGridPanel(true);
            return;
        }
        // If suggestions are still being fetched, wait for them
        if (candidate._fetchPromise || (candidate._themeSuggestions === undefined && !candidate._suggestionsOffline)) {
            await fetchThemeSuggestions(candidate);
        }

        decisionState.isOpen = true;
        state.decisionModalOpen = true;
        decisionState.candidate = candidate;
        decisionState.cardIndex = 0;
        decisionState.acceptedTags = [];
        decisionState.cardDecisions = [];

        if (curateDecisionModal) {
            curateDecisionModal.classList.remove("hidden");
        }
        if (decisionValidationError) {
            decisionValidationError.classList.add("hidden");
        }
        if (decisionDescriptionInput) {
            decisionDescriptionInput.value = "";
            decisionDescriptionInput.classList.remove("hidden");
        }
        if (decisionDescriptionLoading) {
            decisionDescriptionLoading.classList.add("hidden");
        }

        const suggestions = candidate._themeSuggestions || [];
        if (suggestions.length > 0) {
            decisionState.step = "cards";
            if (decisionOfflineBadge) decisionOfflineBadge.classList.add("hidden");
            if (decisionOfflineNote) decisionOfflineNote.classList.add("hidden");
            renderDecisionCard();
        } else {
            // No suggestions or offline: directly show custom step with offline indicator
            if (candidate._suggestionsOffline || suggestions.length === 0) {
                if (decisionOfflineBadge) decisionOfflineBadge.classList.remove("hidden");
                if (decisionOfflineNote) decisionOfflineNote.classList.remove("hidden");
            } else {
                if (decisionOfflineBadge) decisionOfflineBadge.classList.add("hidden");
                if (decisionOfflineNote) decisionOfflineNote.classList.add("hidden");
            }
            transitionToCustomStep();
        }
    }

    function closeCurateDecisionModal() {
        stopPanLoop();
        stopZoomLoop();
        if (currentDescribeAbortController) {
            currentDescribeAbortController.abort();
            currentDescribeAbortController = null;
        }
        if (decisionDescriptionInput) {
            decisionDescriptionInput.value = "";
        }
        if (decisionDescriptionLoading) {
            decisionDescriptionLoading.classList.add("hidden");
        }
        decisionState.isOpen = false;
        state.decisionModalOpen = false;
        if (curateDecisionModal) {
            curateDecisionModal.classList.add("hidden");
        }
        decisionState.candidate = null;
        decisionState.cardIndex = 0;
        decisionState.acceptedTags = [];
        decisionState.cardDecisions = [];
        decisionState.step = "cards";
    }

    function renderDecisionCard() {
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) {
            transitionToCustomStep();
            return;
        }

        decisionState.step = "cards";
        if (decisionCardStep) decisionCardStep.classList.remove("hidden");
        if (decisionCustomStep) decisionCustomStep.classList.add("hidden");

        const currentTag = suggestions[decisionState.cardIndex];
        if (decisionCardTag) {
            decisionCardTag.textContent = currentTag;
        }
        if (decisionProgressBadge) {
            decisionProgressBadge.classList.remove("hidden");
            decisionProgressBadge.textContent = `Suggestion ${decisionState.cardIndex + 1} of ${suggestions.length}`;
        }
        if (btnDecisionUndo) {
            btnDecisionUndo.disabled = (decisionState.cardIndex === 0);
        }

        renderAcceptedTags();
    }

    function handleDecisionReject() {
        if (decisionState.step !== "cards") return;
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) return;
        const tag = suggestions[decisionState.cardIndex];

        decisionState.cardDecisions[decisionState.cardIndex] = { suggestion: tag, accepted: false };
        removeAcceptedTag(tag);

        decisionState.cardIndex++;
        renderDecisionCard();
    }

    function handleDecisionAccept() {
        if (decisionState.step !== "cards") return;
        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionState.cardIndex >= suggestions.length) return;
        const tag = suggestions[decisionState.cardIndex];

        decisionState.cardDecisions[decisionState.cardIndex] = { suggestion: tag, accepted: true };
        addAcceptedTag(tag);

        decisionState.cardIndex++;
        renderDecisionCard();
    }

    function handleDecisionUndo() {
        if (decisionState.cardIndex > 0) {
            decisionState.cardIndex--;
            const prev = decisionState.cardDecisions[decisionState.cardIndex];
            if (prev && prev.accepted) {
                removeAcceptedTag(prev.suggestion);
            }
            delete decisionState.cardDecisions[decisionState.cardIndex];
            renderDecisionCard();
        }
    }

    function transitionToCustomStep() {
        decisionState.step = "custom";
        if (decisionCardStep) decisionCardStep.classList.add("hidden");
        if (decisionCustomStep) decisionCustomStep.classList.remove("hidden");

        const suggestions = decisionState.candidate?._themeSuggestions || [];
        if (decisionProgressBadge) {
            if (suggestions.length > 0) {
                decisionProgressBadge.classList.remove("hidden");
                decisionProgressBadge.textContent = "Custom Tags";
            } else {
                decisionProgressBadge.classList.add("hidden");
            }
        }
        if (decisionTagInput) {
            decisionTagInput.value = "";
            setTimeout(() => decisionTagInput.focus(), 50);
        }

        renderAutocompletePills("");
        renderAcceptedTags();

        if (decisionState.candidate) {
            fetchCandidateDescription(decisionState.candidate, false);
        }
    }

    function getKnownDatasetTags() {
        const tagMap = new Map();
        const defaultThemes = ["Dungeon", "Cave", "Tavern", "Wilderness", "Ruins", "Urban", "SciFi", "Building"];
        for (const t of defaultThemes) {
            tagMap.set(t.toLowerCase(), t);
        }

        if (state.curatedMaps && Array.isArray(state.curatedMaps)) {
            for (const map of state.curatedMaps) {
                const tags = map.tags || [];
                for (const t of tags) {
                    if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
                }
            }
        }

        if (state.candidates && Array.isArray(state.candidates)) {
            for (const c of state.candidates) {
                const tags = c.tags || [];
                for (const t of tags) {
                    if (t && t.trim()) tagMap.set(t.trim().toLowerCase(), t.trim());
                }
            }
        }

        return Array.from(tagMap.values());
    }

    function renderAutocompletePills(query) {
        if (!decisionAutocompleteList) return;
        decisionAutocompleteList.innerHTML = "";

        const allTags = getKnownDatasetTags();
        const cleanQuery = (query || "").trim().toLowerCase();

        let matches = [];
        if (cleanQuery.length > 0) {
            matches = allTags.filter((t) => {
                const lower = t.toLowerCase();
                return lower.includes(cleanQuery) && !hasAcceptedTag(lower);
            });
        } else {
            // Show canonical defaults that aren't yet accepted
            const defaults = ["Dungeon", "Cave", "Tavern", "Wilderness", "Ruins", "Urban", "SciFi", "Building"];
            matches = defaults.filter((t) => !hasAcceptedTag(t));
        }

        // Limit to 8 pills to keep modal uncluttered
        const displayMatches = matches.slice(0, 8);
        for (const tag of displayMatches) {
            const pill = document.createElement("button");
            pill.type = "button";
            pill.className = "autocomplete-pill";
            pill.dataset.tag = tag;
            pill.textContent = tag;
            pill.addEventListener("click", () => {
                addAcceptedTag(tag);
                if (decisionTagInput) {
                    decisionTagInput.value = "";
                    decisionTagInput.focus();
                }
                renderAcceptedTags();
                renderAutocompletePills("");
            });
            decisionAutocompleteList.appendChild(pill);
        }
    }

    function hasAcceptedTag(tag) {
        const lower = tag.trim().toLowerCase();
        return decisionState.acceptedTags.some((t) => t.toLowerCase() === lower);
    }

    function addAcceptedTag(tag) {
        const trimmed = tag.trim();
        if (!trimmed) return;
        if (!hasAcceptedTag(trimmed)) {
            decisionState.acceptedTags.push(trimmed);
            if (decisionValidationError) {
                decisionValidationError.classList.add("hidden");
            }
        }
    }

    function removeAcceptedTag(tag) {
        const lower = tag.trim().toLowerCase();
        decisionState.acceptedTags = decisionState.acceptedTags.filter(
            (t) => t.toLowerCase() !== lower
        );
    }

    function renderAcceptedTags() {
        if (!decisionAcceptedTags) return;
        decisionAcceptedTags.innerHTML = "";

         if (decisionState.acceptedTags.length === 0) {
            const hint = document.createElement("span");
            hint.className = "decision-empty-tags-hint";
            hint.textContent = "No tags accepted yet";
            decisionAcceptedTags.appendChild(hint);
            return;
        }

        for (const tag of decisionState.acceptedTags) {
            const chip = document.createElement("span");
            chip.className = "tag-chip";
            chip.dataset.tag = tag;

            const label = document.createElement("span");
            label.textContent = tag;
            chip.appendChild(label);

            const btnRemove = document.createElement("button");
            btnRemove.type = "button";
            btnRemove.className = "btn-remove-tag";
            btnRemove.setAttribute("aria-label", `Remove ${tag}`);
            btnRemove.textContent = "✕";
            btnRemove.addEventListener("click", (e) => {
                e.stopPropagation();
                removeAcceptedTag(tag);
                renderAcceptedTags();
                if (decisionState.step === "custom" && decisionTagInput) {
                    renderAutocompletePills(decisionTagInput.value.trim());
                }
            });
            chip.appendChild(btnRemove);

            decisionAcceptedTags.appendChild(chip);
        }
    }

    function onDecisionTagInput(e) {
        const query = decisionTagInput ? decisionTagInput.value.trim() : "";
        renderAutocompletePills(query);
        if (decisionValidationError) {
            decisionValidationError.classList.add("hidden");
        }
    }

    function onDecisionTagInputKeydown(e) {
        if (e.key === "Enter") {
            e.preventDefault();
            handleCustomTagInputEnter();
            return;
        }

        if (e.key === "Tab") {
            const firstPill = decisionAutocompleteList?.querySelector(".autocomplete-pill");
            if (firstPill && firstPill.dataset.tag) {
                e.preventDefault();
                addAcceptedTag(firstPill.dataset.tag);
                if (decisionTagInput) {
                    decisionTagInput.value = "";
                    decisionTagInput.focus();
                }
                renderAcceptedTags();
                renderAutocompletePills("");
            }
            return;
        }

        if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
            if (decisionTagInput && decisionTagInput.value === "") {
                const suggestions = decisionState.candidate?._themeSuggestions || [];
                if (suggestions.length > 0 && decisionState.cardIndex > 0) {
                    e.preventDefault();
                    handleDecisionUndo();
                }
            }
        }
    }

    function handleCustomTagAdd() {
        if (!decisionTagInput) return;
        const text = decisionTagInput.value.trim();
        if (text) {
            addAcceptedTag(text);
            decisionTagInput.value = "";
            renderAcceptedTags();
            renderAutocompletePills("");
            decisionTagInput.focus();
        } else {
            if (decisionState.acceptedTags.length === 0) {
                triggerDecisionShake();
            } else {
                decisionTagInput.focus();
            }
        }
    }

    function handleCustomTagInputEnter() {
        if (!decisionTagInput) return;
        const text = decisionTagInput.value.trim();
        if (text) {
            addAcceptedTag(text);
            decisionTagInput.value = "";
            renderAcceptedTags();
            renderAutocompletePills("");
            decisionTagInput.focus();
        } else {
            // Empty Enter: finish if acceptedTags.length >= 1, else shake
            if (decisionState.acceptedTags.length >= 1) {
                submitDecisionCuration();
            } else {
                triggerDecisionShake();
            }
        }
    }

    function handleCustomTagSubmit() {
        if (decisionTagInput && decisionTagInput.value.trim()) {
            addAcceptedTag(decisionTagInput.value.trim());
            decisionTagInput.value = "";
            renderAcceptedTags();
        }

        if (decisionState.acceptedTags.length >= 1) {
            submitDecisionCuration();
        } else {
            triggerDecisionShake();
        }
    }

    function triggerDecisionShake() {
        const card = curateDecisionModal?.querySelector(".modal-card") || decisionTagInput;
        if (card) {
            card.classList.remove("shake");
            void card.offsetWidth;
            card.classList.add("shake");
            setTimeout(() => card.classList.remove("shake"), 500);
        }
        if (decisionValidationError) {
            decisionValidationError.textContent = "At least one theme label required";
            decisionValidationError.classList.remove("hidden");
        }
        if (decisionTagInput) {
            decisionTagInput.focus();
        }
    }

    /**
     * Submits the curate decision without blocking the triage loop.
     *
     * The server finishes the curation — strict map-level description generation included —
     * after this function returns, so the queue advances immediately and the outcome arrives
     * as a toast once the response lands. A failed curation puts the candidate back into the
     * local queue: nothing was committed, so it is still listed in staging.
     */
    function submitDecisionCuration() {
        if (decisionState.acceptedTags.length === 0) {
            triggerDecisionShake();
            return;
        }

        const candidate = decisionState.candidate;
        if (!candidate) return;

        const acceptedTags = Array.from(decisionState.acceptedTags);
        const descriptionValue = decisionDescriptionInput ? decisionDescriptionInput.value.trim() : "";
        closeCurateDecisionModal();

        const payload = {
            id: candidate.id,
            tags: acceptedTags,
            layers: candidate.layers && candidate.layers.length > 0 ? candidate.layers : undefined,
            description: descriptionValue || undefined,
        };

        // Optimistically drop the candidate from the local queue and memory cache, then move on:
        // description generation must never stall the reviewer.
        const removedIndex = state.currentIndex;
        state.imageCache.delete(candidate.id);
        if (candidate.layers) {
            candidate.layers.forEach((_, idx) => {
                state.imageCache.delete(`${candidate.id}_layer_${idx}`);
            });
        }
        state.candidates.splice(removedIndex, 1);

        // Maintain index within remaining bounds
        if (state.currentIndex >= state.candidates.length) {
            state.currentIndex = state.candidates.length - 1;
        }

        loadCandidate(state.currentIndex);
        state.pendingCurationIds.add(candidate.id);

        fetch("/api/curate", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
        })
            .then(async (res) => {
                if (!res.ok) {
                    const errJson = await res.json().catch(() => ({}));
                    throw new Error(errJson.error || errJson.message || `HTTP ${res.status}`);
                }
                showToast(`Candidate #${candidate.id} curated`, "success");
            })
            .catch((err) => {
                restoreUncuratedCandidate(candidate, removedIndex);
                showToast(`Curate failed for #${candidate.id}: ${err.message}`, "danger");
            })
            .finally(() => {
                state.pendingCurationIds.delete(candidate.id);
                fetchStatus();
            });
    }

    /**
     * Puts a candidate whose curation request failed back into the local review queue at the
     * position it was curated from. The candidate is still present in staging, so without this
     * the reviewer's queue would silently disagree with the server.
     */
    function restoreUncuratedCandidate(candidate, removedIndex) {
        if (state.candidates.some((c) => c.id === candidate.id)) {
            return; // Queue was refetched meanwhile and already lists the candidate.
        }
        const insertAt = Math.min(Math.max(removedIndex, 0), state.candidates.length);
        state.candidates.splice(insertAt, 0, candidate);
        if (state.currentIndex < 0) {
            state.currentIndex = insertAt;
        } else if (state.currentIndex >= insertAt) {
            state.currentIndex += 1;
        }
        loadCandidate(state.currentIndex);
    }

    async function curateCurrentCandidate() {
        exitLabelingMode();
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        // Curate Validation Gate: validate layers upfront before opening decision modal
        const candidate = state.candidates[state.currentIndex];
        if (!validateCandidateLayers(candidate)) {
            return;
        }

        // 16×16 Full-Grid Lattice Curation Gate
        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (!lattice.isEligible) {
            showToast(`Map has only ${lattice.cols}×${lattice.rows} full Grid Cells; at least 16×16 required for curation`, "danger");
            toggleGridPanel(true);
            return;
        }

        await openCurateDecisionModal();
    }

    function handleDecisionModalKeydown(e) {
        if (e.key === "Escape") {
            closeCurateDecisionModal();
            e.preventDefault();
            return;
        }

        if (decisionState.step === "cards") {
            if (e.key === "ArrowLeft" || e.key === "Left") {
                handleDecisionReject();
                e.preventDefault();
                return;
            }
            if (e.key === "ArrowRight" || e.key === "Right") {
                handleDecisionAccept();
                e.preventDefault();
                return;
            }
            if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
                handleDecisionUndo();
                e.preventDefault();
                return;
            }
            return;
        }

        if (decisionState.step === "custom") {
            if (document.activeElement === decisionDescriptionInput) {
                if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    handleCustomTagSubmit();
                }
                return;
            }

            if (document.activeElement === decisionTagInput) {
                return;
            }

            if (e.key === "Enter") {
                handleCustomTagSubmit();
                e.preventDefault();
                return;
            }
            if (e.key === "Backspace" || e.key === "ArrowUp" || e.key === "Up") {
                handleDecisionUndo();
                e.preventDefault();
                return;
            }
            if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Left" || e.key === "Right") {
                e.preventDefault();
                return;
            }
            return;
        }
    }

    async function rejectCurrentCandidate() {
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        state.isSubmitting = true;

        try {
            const payload = {
                id: candidate.id,
            };

            const res = await fetch("/api/reject", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });

            if (!res.ok) {
                const errJson = await res.json().catch(() => ({}));
                throw new Error(errJson.message || `HTTP ${res.status}`);
            }

            showToast(`Candidate #${candidate.id} rejected`, "danger");

            // Remove rejected item from local queue and cache
            state.imageCache.delete(candidate.id);
            if (candidate.layers) {
                candidate.layers.forEach((_, idx) => {
                    state.imageCache.delete(`${candidate.id}_layer_${idx}`);
                });
            }
            state.candidates.splice(state.currentIndex, 1);

            if (state.currentIndex >= state.candidates.length) {
                state.currentIndex = state.candidates.length - 1;
            }

            loadCandidate(state.currentIndex);
            fetchStatus();
        } catch (err) {
            showToast(`Rejection failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
        }
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
            const sessionSpend = (stats.session_spend_usd || 0).toFixed(2);
            const totalSpend = (stats.total_spend_usd || 0).toFixed(2);
            const totalGens = stats.total_generations || 0;
            const mapWord = totalGens === 1 ? "map" : "maps";
            if (costMonitorText) {
                costMonitorText.textContent = `Session: $${sessionSpend} · Total: $${totalSpend} (${totalGens} ${mapWord} generated)`;
            }
            if (costMonitorHud) {
                costMonitorHud.title = `Session Spend: $${sessionSpend} | Lifetime Spend: $${totalSpend} | Total Maps: ${totalGens}`;
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

    function calculatePreflightMetrics() {
        const models = getSelectedBenchmarkModels();
        const archetypes = getSelectedBenchmarkArchetypes();
        const hasCustom = benchmarkEnableCustom?.checked && (benchmarkCustomPrompt?.value.trim().length > 0 || benchmarkCustomName?.value.trim().length > 0);
        const promptCount = archetypes.length + (hasCustom ? 1 : 0);
        const modelCount = models.length;
        const totalImages = modelCount * promptCount;

        let costPerSet = 0;
        for (const m of models) {
            costPerSet += MODEL_PRICES[m] || 0.05;
        }
        const estimatedCost = promptCount * costPerSet;

        return {
            models,
            archetypes,
            hasCustom,
            modelCount,
            promptCount,
            totalImages,
            estimatedCost,
        };
    }

    function updatePreflightCost() {
        const metrics = calculatePreflightMetrics();
        const { modelCount, promptCount, totalImages, estimatedCost } = metrics;

        const costStr = `$${estimatedCost.toFixed(2)}`;
        if (calcBadgeCost) calcBadgeCost.textContent = costStr;

        if (calcFormulaDisplay) {
            calcFormulaDisplay.textContent = `${modelCount} model${modelCount === 1 ? "" : "s"} × ${promptCount} prompt${promptCount === 1 ? "" : "s"} = ${totalImages} image${totalImages === 1 ? "" : "s"} (~${costStr})`;
        }

        const exceedsGuardrail = estimatedCost > BUDGET_GUARDRAIL_THRESHOLD;
        if (calcWarningNote) {
            calcWarningNote.classList.toggle("hidden", !exceedsGuardrail);
        }

        if (btnStartBenchmarkText) {
            btnStartBenchmarkText.textContent = `Run Benchmark (${costStr})`;
        }

        if (btnStartBenchmark) {
            btnStartBenchmark.disabled = (modelCount === 0 || promptCount === 0 || state.isBenchmarking);
        }
    }

    function toggleBenchmarkDrawer() {
        if (state.benchmarkDrawerOpen) {
            closeBenchmarkDrawer();
        } else {
            openBenchmarkDrawer();
        }
    }

    function openBenchmarkDrawer() {
        stopPanLoop();
        stopZoomLoop();
        if (state.helpModalOpen) closeHelpModal();
        if (state.drawerOpen) closeIngestionDrawer();
        state.benchmarkDrawerOpen = true;
        benchmarkDrawer?.classList.remove("hidden");
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

        document.querySelectorAll('input[name="benchmark-model"]').forEach((cb) => {
            cb.addEventListener("change", () => updatePreflightCost());
        });
        document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => {
            cb.addEventListener("change", () => updatePreflightCost());
        });

        benchmarkEnableCustom?.addEventListener("change", () => {
            customPromptFields?.classList.toggle("hidden", !benchmarkEnableCustom.checked);
            updatePreflightCost();
        });
        benchmarkCustomName?.addEventListener("input", () => updatePreflightCost());
        benchmarkCustomPrompt?.addEventListener("input", () => updatePreflightCost());

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

    function formatBytes(bytes) {
        if (!bytes || bytes === 0) return "0 B";
        const k = 1024;
        const sizes = ["B", "KB", "MB", "GB", "TB"];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        const val = bytes / Math.pow(k, i);
        return (val >= 10 || i === 0 ? val.toFixed(0) : val.toFixed(1)) + " " + sizes[i];
    }

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

    function escapeHtml(str) {
        if (!str) return "";
        return String(str)
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#039;");
    }

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

            // Toggle onion-skin overlay: O
            if ((e.key === "o" || e.key === "O") && !e.ctrlKey && !e.metaKey && !e.altKey) {
                state.libraryFullscreenOnionSkinActive = !state.libraryFullscreenOnionSkinActive;
                if (btnFsOnionSkin) btnFsOnionSkin.classList.toggle("active", state.libraryFullscreenOnionSkinActive);
                requestFullscreenRender();
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
                : `ttrpg battlemap, flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless, ${primaryTheme}`);

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
    // LoRA Training Dataset Readiness Audit (Issue #184)
    // ==========================================================================

    const DATASET_ISSUE_LABELS = {
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

    function renderDatasetReadinessLabel(report) {
        if (!datasetReadinessLabel || !btnDatasetReadiness) return;
        const total = (report && report.maps_total) || 0;
        if (!report || total === 0) {
            datasetReadinessLabel.textContent = "Dataset Readiness";
            btnDatasetReadiness.classList.remove("is-ready", "has-issues");
            btnDatasetReadiness.title = "Audit curated maps against the LoRA Training Dataset contract (16×16 Map Sections, Trigger Phrase captions)";
            return;
        }
        const ready = Boolean(report.ready);
        datasetReadinessLabel.textContent = ready
            ? `Dataset Ready · ${report.sections_total} sections`
            : `Dataset Issues · ${report.maps_ready}/${total} maps`;
        btnDatasetReadiness.classList.toggle("is-ready", ready);
        btnDatasetReadiness.classList.toggle("has-issues", !ready);
        btnDatasetReadiness.title = ready
            ? `All ${total} curated map(s) contribute ${report.sections_total} clean 16×16 Map Sections`
            : `${total - report.maps_ready} of ${total} curated map(s) have dataset issues`;
    }

    function renderDatasetReadinessReport(report) {
        if (!datasetReadinessSummary || !datasetReadinessIssues) return;
        renderDatasetReadinessLabel(report);

        if (datasetReadinessIcon) {
            datasetReadinessIcon.textContent = report && report.ready ? "✓" : "!";
            datasetReadinessIcon.className = report && report.ready
                ? "readiness-icon is-ready"
                : "readiness-icon has-issues";
        }

        const total = (report && report.maps_total) || 0;
        const sections = (report && report.sections_total) || 0;
        const readyMaps = (report && report.maps_ready) || 0;
        const readySections = (report && report.sections_ready) || 0;
        const scope = datasetScopeTheme() || "all themes";

        const stats = [
            ["Maps ready", `${readyMaps} / ${total}`],
            ["Map Sections ready", `${readySections} / ${sections}`],
            ["Scope", scope],
        ];
        datasetReadinessSummary.innerHTML = stats
            .map(([label, value]) => `<div class="readiness-stat"><span class="readiness-stat-label">${escapeHtml(label)}</span><span class="readiness-stat-value mono">${escapeHtml(value)}</span></div>`)
            .join("");

        const flagged = ((report && report.maps) || []).filter(map => (map.issues || []).length > 0);
        if (flagged.length === 0) {
            datasetReadinessIssues.innerHTML = '<div class="readiness-clean">No issues: every curated map in scope exports clean 16×16 Map Section samples.</div>';
            return;
        }

        datasetReadinessIssues.innerHTML = flagged
            .map(map => {
                const rows = map.issues
                    .map(issue => `<li><span class="readiness-issue-code mono">${escapeHtml(datasetIssueLabel(issue.code))}</span> ${escapeHtml(issue.detail)}</li>`)
                    .join("");
                const counts = `${map.ready_sections} / ${map.sections} sections ready`;
                return `<div class="readiness-map-issue"><div class="readiness-map-head"><span class="readiness-map-id mono">${escapeHtml(map.id)}</span><span class="readiness-map-theme">${escapeHtml(map.theme)}</span><span class="readiness-map-count mono">${escapeHtml(counts)}</span></div><ul class="readiness-issue-list">${rows}</ul></div>`;
            })
            .join("");
    }

    async function fetchDatasetReadiness() {
        if (state.isAuditingDataset) return;
        state.isAuditingDataset = true;
        if (btnDatasetReadiness) btnDatasetReadiness.disabled = true;
        try {
            const res = await fetch(`/api/dataset/readiness${datasetScopeQuery()}`);
            if (!res.ok) {
                const errData = await res.json().catch(() => ({}));
                throw new Error(errData.error || `HTTP ${res.status}`);
            }
            renderDatasetReadinessReport(await res.json());
        } catch (err) {
            showToast(`Dataset audit failed: ${err.message}`, "danger");
        } finally {
            state.isAuditingDataset = false;
            if (btnDatasetReadiness) btnDatasetReadiness.disabled = false;
        }
    }

    function openDatasetReadinessModal() {
        stopPanLoop();
        stopZoomLoop();
        state.datasetReadinessModalOpen = true;
        datasetReadinessModal?.classList.remove("hidden");
        fetchDatasetReadiness();
    }

    function closeDatasetReadinessModal() {
        stopPanLoop();
        stopZoomLoop();
        state.datasetReadinessModalOpen = false;
        datasetReadinessModal?.classList.add("hidden");
    }

    function setupDatasetReadinessControls() {
        btnDatasetReadiness?.addEventListener("click", () => openDatasetReadinessModal());
        document.getElementById("btn-close-readiness-modal")?.addEventListener("click", () => closeDatasetReadinessModal());
        document.getElementById("btn-dismiss-readiness")?.addEventListener("click", () => closeDatasetReadinessModal());
        document.getElementById("btn-refresh-readiness")?.addEventListener("click", () => fetchDatasetReadiness());
        datasetReadinessModal?.addEventListener("click", (e) => {
            if (e.target === datasetReadinessModal) closeDatasetReadinessModal();
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
            showToast(`Added tag "${normalizedTag}" to ${successCount} map${successCount === 1 ? "" : "s"}`, "success");
        } catch (err) {
            showToast(`Bulk tag error: ${err.message}`, "danger");
        } finally {
            if (btnBulkTag) btnBulkTag.disabled = false;
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

    function resizeFullscreenCanvas() {
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
    function requestFullscreenRender() {
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
     * onion-skin overlay (50% Ground layer), and single-draw-call 64px VTT grid overlay.
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

            // Onion-skin overlay: draw base Ground layer image with 50% opacity
            if (state.libraryFullscreenOnionSkinActive && state.libraryFullscreenMap) {
                const map = state.libraryFullscreenMap;
                const layers = map.layers || [];
                let groundIdx = layers.findIndex(l => (l.role || "").toLowerCase().replace(/[-_]/g, "") === "ground");
                if (groundIdx < 0 && layers.length > 0) groundIdx = 0;

                if (groundIdx >= 0 && groundIdx !== state.libraryFullscreenLayerIndex) {
                    const gLayer = layers[groundIdx];
                    const gActualIdx = (gLayer && gLayer.index !== undefined) ? gLayer.index : groundIdx;
                    const groundKey = `${map.id}_layer_${gActualIdx}`;
                    const groundImg = state.libraryFullscreenImageCache.get(groundKey);
                    if (groundImg && groundImg.complete && groundImg.naturalWidth > 0) {
                        const gW = groundImg.naturalWidth || groundImg.width;
                        const gH = groundImg.naturalHeight || groundImg.height;
                        c.save();
                        c.globalAlpha = 0.5;
                        c.drawImage(groundImg, 0, 0, gW, gH);
                        c.restore();
                    }
                }
            }

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

        // Prefetch ground layer for onion-skin
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

        if (btnFsOnionSkin) {
            btnFsOnionSkin.classList.toggle("active", state.libraryFullscreenOnionSkinActive);
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
                    await fetch(`/api/curated/${encodeURIComponent(map.id)}`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify(payload),
                    });
                } catch (err) {
                    console.error("Failed to save curated grid config:", err);
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

        btnFsOnionSkin?.addEventListener("click", () => {
            state.libraryFullscreenOnionSkinActive = !state.libraryFullscreenOnionSkinActive;
            if (btnFsOnionSkin) btnFsOnionSkin.classList.toggle("active", state.libraryFullscreenOnionSkinActive);
            requestFullscreenRender();
        });

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

    function normalizeLayerRole(role) {
        const r = (role || "").toLowerCase().replace(/[-_]/g, "");
        if (r === "ground") return "ground";
        if (r === "upperfloor" || r === "upper") return "upper_floor";
        if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") return "basement";
        if (r === "roof") return "roof";
        if (r === "variant") return "variant";
        return "ground";
    }

    function isCanonicalThemeTag(tag) {
        return CANONICAL_THEMES.includes((tag || "").trim().toLowerCase());
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
        btnReject.addEventListener("click", () => rejectCurrentCandidate());

        document.getElementById("btn-help").addEventListener("click", () => toggleHelpModal());
        document.getElementById("btn-close-help-modal").addEventListener("click", () => closeHelpModal());
        document.getElementById("btn-dismiss-help").addEventListener("click", () => closeHelpModal());


        document.getElementById("btn-refresh-staging").addEventListener("click", () => {
            fetchStatus();
            fetchStagingQueue();
        });


        // Filmstrip deck buttons & role badges
        if (btnOnionSkin) {
            btnOnionSkin.addEventListener("click", () => toggleOnionSkin());
        }
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
        setupDatasetReadinessControls();
        setupLibraryControls();
        setupFullscreenControls();
        setupInspectorDrawerControls();
        setupDecisionModalControls();

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
            if (state.datasetReadinessModalOpen) {
                closeDatasetReadinessModal();
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

        // View switching hotkeys: L for library, T for triage
        if (!e.ctrlKey && !e.metaKey && !e.altKey) {
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

        // Onion-skin overlay toggle: O
        if (e.key === "o" || e.key === "O") {
            toggleOnionSkin();
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

    let saveCandidateGridTimeout = null;
    function saveCurrentCandidateGrid() {
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

    function setGridSize(size) {
        const num = Number.isFinite(size) ? size : parseInt(size, 10);
        if (isNaN(num)) return;
        const clamped = Math.max(MIN_GRID_SIZE, Math.min(MAX_GRID_SIZE, Math.round(num)));
        state.gridSize = clamped;
        saveCurrentCandidateGrid();
        updateHud();
        updateGridPanel();
        requestRender();
    }

    function nudgeGridOffset(dx, dy) {
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

    function resetGrid() {
        state.gridSize = 64;
        state.gridOffset = { x: 0, y: 0 };
        saveCurrentCandidateGrid();
        updateHud();
        updateGridPanel();
        requestRender();
        showToast("Grid reset to default 64px (0,0)", "info");
    }

    function toggleGridPanel(force) {
        state.gridPanelOpen = typeof force === "boolean" ? force : !state.gridPanelOpen;
        if (gridAdjustPanel) {
            gridAdjustPanel.classList.toggle("hidden", !state.gridPanelOpen);
        }
        if (btnGridAdjust) {
            btnGridAdjust.classList.toggle("active", state.gridPanelOpen);
        }
        if (state.gridPanelOpen) {
            updateGridPanel();
        }
        requestRender();
    }

    function toggleGridAlignMode(force) {
        state.gridAlignMode = typeof force === "boolean" ? force : !state.gridAlignMode;
        if (btnGridAlignDrag) {
            btnGridAlignDrag.classList.toggle("active", state.gridAlignMode);
        }
        if (canvas) {
            canvas.classList.toggle("grid-aligning", state.gridAlignMode);
        }
        requestRender();
    }

    function updateGridPanel() {
        if (gridSizeInput) {
            gridSizeInput.value = state.gridSize;
        }
        if (gridSizeSlider) {
            gridSizeSlider.value = Math.min(256, Math.max(16, state.gridSize));
        }
        const ox = state.gridOffset ? Math.round(state.gridOffset.x) : 0;
        const oy = state.gridOffset ? Math.round(state.gridOffset.y) : 0;
        if (gridOffsetXVal) {
            gridOffsetXVal.textContent = ox > 0 ? `+${ox}px` : `${ox}px`;
        }
        if (gridOffsetYVal) {
            gridOffsetYVal.textContent = oy > 0 ? `+${oy}px` : `${oy}px`;
        }
        const chips = document.querySelectorAll(".preset-chip");
        chips.forEach((chip) => {
            const sz = parseInt(chip.getAttribute("data-size"), 10);
            chip.classList.toggle("active", sz === state.gridSize);
        });

        const dims = getCurrentGroundDimensions();
        const lattice = calculateLatticeInfo(dims.width, dims.height, state.gridSize, state.gridOffset.x, state.gridOffset.y);
        if (gridLatticeCells) {
            gridLatticeCells.textContent = `${lattice.cols} × ${lattice.rows} cells`;
        }
        if (gridLatticeSections) {
            if (lattice.isEligible) {
                gridLatticeSections.textContent = `${lattice.kx} × ${lattice.ky} sections`;
                gridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-valid";
            } else {
                gridLatticeSections.textContent = "< 16×16 cells";
                gridLatticeSections.className = "grid-lattice-sections-val mono badge-sections badge-sections-warning";
            }
        }
    }

    function toggleGrid() {
        state.gridVisible = !state.gridVisible;
        if (state.gridVisible) {
            btnGridToggle.classList.add("active");
        } else {
            btnGridToggle.classList.remove("active");
        }
        requestRender();
        updateHud();
        const size = state.gridSize || VTT_GRID_SIZE;
        showToast(`${size}px VTT Grid: ${state.gridVisible ? "Enabled" : "Disabled"}`, "info");
    }

    function enterLabelingMode(mode) {
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

    function exitLabelingMode() {
        stopPanLoop();
        stopZoomLoop();
        state.labelingMode = null;
        const roleSelectorEl = document.getElementById("role-selector");
        if (roleSelectorEl) roleSelectorEl.classList.remove("selecting-mode");
    }

    // ==========================================================================
    // UI Helpers & Status Display
    // ==========================================================================

    async function fetchStatus() {
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

    function updateHud() {
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

    function showEmptyState(visible) {
        if (visible) {
            emptyState.classList.remove("hidden");
        } else {
            emptyState.classList.add("hidden");
        }
    }

    function showLoading(visible, text = "Loading...") {
        if (visible) {
            loadingText.textContent = text;
            loadingOverlay.classList.remove("hidden");
        } else {
            loadingOverlay.classList.add("hidden");
        }
    }

    function showToast(message, type = "info") {
        const container = document.getElementById("toast-container");
        if (!container) return;

        const toast = document.createElement("div");
        toast.className = `toast toast-${type}`;
        toast.textContent = message;

        container.appendChild(toast);
        setTimeout(() => {
            if (toast.parentElement) {
                toast.parentElement.removeChild(toast);
            }
        }, 3000);
    }

    // Initialize when DOM is ready
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
