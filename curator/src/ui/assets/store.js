/**
 * Shared application state for the Curator UI.
 *
 * One mutable object, imported by every module that reads or writes UI state.
 * Fields are grouped by feature; move a field only when the feature moves.
 */

// Application State
export const state = {
    // Queue data
    candidates: [],
    currentIndex: -1,
    currentImage: null,

    // In-memory image cache for 0ms transitions (id -> HTMLImageElement)
    imageCache: new Map(),

    // Curation parameters
    labelingMode: null,
    currentLayerIndex: 0,
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
    // Rejection undo stack tracking { candidate, index }
    rejectedStack: [],
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
    // LoRA Training Dataset Readiness audit (Issues #184, #245): cached in localStorage per
    // theme scope and rendered inside the Frozen Dataset Snapshots drawer.
    readinessScopeKey: null,
    readinessReport: null,
    readinessAuditedAt: null,
    readinessRefusal: null,
    readinessAudit: null,
    // The scope and generation the in-flight audit was started for: a write bumps the epoch and
    // a scope switch changes the key, and neither may join an audit that predates it.
    readinessAuditEpoch: -1,
    readinessAuditScope: null,
    readinessBatch: null,
    readinessEpoch: 0,
    describeRequests: new Map(),
    // Frozen Dataset Snapshots (Issue #216)
    isFreezingSnapshot: false,
    isFetchingSnapshots: false,
    snapshotsCollapsed: false,
    snapshots: [],
    // LoRA Training Runs & Adapters (Issue #209, ADR-0009, ADR-0010)
    trainingDrawerOpen: false,
    trainingActiveTab: "runs", // 'runs' | 'adapters'
    trainingRuns: [],
    trainingAdapters: [],
    isFetchingRuns: false,
    isFetchingAdapters: false,
    isLaunchingRun: false,
    trainingSelectedPolicy: "calibration",
    trainingRunsFilter: "all",
    trainingAdaptersFilter: "all",
    selectedSnapshotForRun: "",
    launchFormCollapsed: false,
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
    isBulkRefining: false,
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
