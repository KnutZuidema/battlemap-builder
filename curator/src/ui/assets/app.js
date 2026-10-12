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
        selectedTheme: "dungeon",
        selectedRating: 4,
        selectedReason: "GridBurnedIn",
        labelingMode: null,
        // Canvas transform
        pan: { x: 0, y: 0 },
        zoom: 1.0,
        isPanning: false,
        dragStart: { x: 0, y: 0 },
        panStart: { x: 0, y: 0 },

        // Visual overlays
        gridVisible: true,

        // Modal and UI dialog state
        rejectionModalOpen: false,
        helpModalOpen: false,
        drawerOpen: false,
        isSubmitting: false,
        isHarvesting: false,

        // Benchmark Drawer & Budget Guardrails
        benchmarkDrawerOpen: false,
        budgetGuardModalOpen: false,
        isBenchmarking: false,
        benchmarkAbortController: null,
        benchmarkWindowMinimized: false,
        // Harvesting parameters
        harvestSort: "hot",
        harvestTime: "all",
    };

    // Constants
    const VTT_GRID_SIZE = 64; // 64px represents standard 5ft D&D VTT square
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
    const THEMES = [
        "dungeon",
        "cave",
        "tavern",
        "wilderness",
        "ruins",
        "urban",
        "scifi",
        "building",
    ];

    const REJECTION_REASONS = [
        "GridBurnedIn",
        "IsometricTilt",
        "BlurryLowRes",
        "BadGeometry",
        "Irrelevant",
    ];

    // DOM Elements cache
    let canvas, ctx;
    let statusIndicator, statusText;
    let candidateMetaBox, queueCounter, candidateId, candidateDimensions, candidateSource;
    let zoomLevelDisplay, hudPanZoom, hudGridStatus, hudResolution, hudPrefetchStatus;
    let btnGridToggle, btnFitScreen, btnPrev, btnNext, btnReject, btnCurate;
    let emptyState, loadingOverlay, loadingText;
    let rejectionModal, rejectionNotesInput, helpModal;
    let ingestionDrawer, drawerPanel, btnOpenDrawer, btnEmptyHarvest, btnCloseDrawer, btnCancelDrawer;
    let harvestSortGroup, harvestTimeGroup, harvestLimitInput, harvestLimitValue, harvestGridlessOnly;
    let btnStartHarvest, harvestProgressContainer, harvestProgressBar, harvestProgressText;

    // Benchmark Drawer & Guardrails DOM Elements
    let btnOpenBenchmarkDrawer;
    let costMonitorHud, costMonitorText;
    let benchmarkDrawer, btnCloseBenchmarkDrawer, btnCancelBenchmarkDrawer;
    let btnModelsSelectAll, btnModelsSelectNone, btnPromptsSelectAll, btnPromptsSelectNone;
    let benchmarkEnableCustom, customPromptFields, benchmarkCustomName, benchmarkCustomPrompt;
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

    // ==========================================================================
    // Initialization & Bootstrap
    // ==========================================================================

    function init() {
        cacheDOMElements();
        setupCanvas();
        setupEventListeners();
        fetchStatus();
        fetchCostStats();
        fetchStagingQueue();
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
        hudResolution = document.getElementById("hud-resolution");
        hudPrefetchStatus = document.getElementById("hud-prefetch-status");

        btnGridToggle = document.getElementById("btn-grid-toggle");
        btnFitScreen = document.getElementById("btn-fit-screen");
        btnPrev = document.getElementById("btn-prev");
        btnNext = document.getElementById("btn-next");
        btnReject = document.getElementById("btn-reject");
        btnCurate = document.getElementById("btn-curate");

        emptyState = document.getElementById("empty-state");
        loadingOverlay = document.getElementById("loading-overlay");
        loadingText = document.getElementById("loading-text");

        rejectionModal = document.getElementById("rejection-modal");
        rejectionNotesInput = document.getElementById("rejection-notes");
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
    }

    // ==========================================================================
    // Canvas 2D Engine & Rendering
    // ==========================================================================

    function setupCanvas() {
        resizeCanvas();
        window.addEventListener("resize", () => {
            resizeCanvas();
            requestRender();
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

            // Draw battlemap at origin (0, 0)
            ctx.drawImage(img, 0, 0, imgW, imgH);

            // Draw crisp 64px VTT grid overlay in image space
            if (state.gridVisible) {
                drawVttGrid(ctx, imgW, imgH);
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
    function drawVttGrid(c, width, height) {
        c.beginPath();

        // Vertical 64px grid lines
        for (let x = 0; x <= width; x += VTT_GRID_SIZE) {
            c.moveTo(x, 0);
            c.lineTo(x, height);
        }

        // Horizontal 64px grid lines
        for (let y = 0; y <= height; y += VTT_GRID_SIZE) {
            c.moveTo(0, y);
            c.lineTo(width, y);
        }

        // Inversely scale line width with zoom so grid remains ~1 screen pixel thin
        c.lineWidth = Math.max(0.5, 1.0 / state.zoom);
        c.strokeStyle = "rgba(56, 189, 248, 0.4)";
        c.stroke();
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
            state.candidates = Array.isArray(data) ? data : [];

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
            state.currentImage = null;
            requestRender();
            updateHud();
            showEmptyState(state.candidates.length === 0);
            return;
        }

        showEmptyState(false);
        state.currentIndex = index;
        const candidate = state.candidates[index];
        const imageUrl = `/api/image/staging/${encodeURIComponent(candidate.id)}`;

        // Check if image is already cached in memory
        let cachedImg = state.imageCache.get(candidate.id);

        if (cachedImg && cachedImg.complete && cachedImg.naturalWidth > 0) {
            // Instant 0ms visual delay transition!
            state.currentImage = cachedImg;
            showLoading(false);
            fitToScreen();
            prefetchUpcoming();
            updateHud();
        } else {
            showLoading(true, `Loading candidate #${candidate.id}...`);
            if (!cachedImg) {
                cachedImg = new Image();
                cachedImg.src = imageUrl;
                state.imageCache.set(candidate.id, cachedImg);
            }

            cachedImg.onload = () => {
                // Ensure candidate hasn't shifted while loading
                if (state.candidates[state.currentIndex]?.id === candidate.id) {
                    state.currentImage = cachedImg;
                    showLoading(false);
                    fitToScreen();
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
     * In-memory pre-fetching of the next 3 candidate images and prior 1 image
     * for instant, zero-latency transitions upon J/K hotkeys.
     */
    function prefetchUpcoming() {
        let readyCount = 0;

        // Prefetch next 3 candidates
        for (let offset = 1; offset <= PREFETCH_AHEAD; offset++) {
            const nextIdx = state.currentIndex + offset;
            if (nextIdx < state.candidates.length) {
                const nextCand = state.candidates[nextIdx];
                let img = state.imageCache.get(nextCand.id);
                if (!img) {
                    img = new Image();
                    img.src = `/api/image/staging/${encodeURIComponent(nextCand.id)}`;
                    state.imageCache.set(nextCand.id, img);
                }
                if (img.complete && img.naturalWidth > 0) {
                    readyCount++;
                } else {
                    img.addEventListener("load", () => updateHud(), { once: true });
                }
            }
        }

        // Prefetch previous 1 candidate
        if (state.currentIndex > 0) {
            const prevCand = state.candidates[state.currentIndex - 1];
            let prevImg = state.imageCache.get(prevCand.id);
            if (!prevImg) {
                prevImg = new Image();
                prevImg.src = `/api/image/staging/${encodeURIComponent(prevCand.id)}`;
                state.imageCache.set(prevCand.id, prevImg);
            }
            if (prevImg.complete && prevImg.naturalWidth > 0) {
                readyCount++;
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
    // Curation & Rejection Actions
    // ==========================================================================

    async function curateCurrentCandidate() {
        exitLabelingMode();
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        state.isSubmitting = true;
        btnCurate.disabled = true;

        try {
            const payload = {
                id: candidate.id,
                theme: state.selectedTheme,
                rating: state.selectedRating,
                tags: [],
            };

            const res = await fetch("/api/curate", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload),
            });

            if (!res.ok) {
                const errJson = await res.json().catch(() => ({}));
                throw new Error(errJson.message || `HTTP ${res.status}`);
            }

            const themeName = state.selectedTheme.charAt(0).toUpperCase() + state.selectedTheme.slice(1);
            showToast(`Candidate #${candidate.id} curated to ${themeName} (★${state.selectedRating})`, "success");

            // Remove curated item from local queue and memory cache
            state.imageCache.delete(candidate.id);
            state.candidates.splice(state.currentIndex, 1);

            // Maintain index within remaining bounds
            if (state.currentIndex >= state.candidates.length) {
                state.currentIndex = state.candidates.length - 1;
            }

            loadCandidate(state.currentIndex);
            fetchStatus();
        } catch (err) {
            showToast(`Curate failed: ${err.message}`, "danger");
        } finally {
            state.isSubmitting = false;
            btnCurate.disabled = false;
        }
    }

    async function confirmRejection() {
        if (state.isSubmitting || state.currentIndex < 0 || state.currentIndex >= state.candidates.length) {
            return;
        }

        const candidate = state.candidates[state.currentIndex];
        const notes = rejectionNotesInput.value.trim();
        state.isSubmitting = true;

        try {
            const payload = {
                id: candidate.id,
                reason: state.selectedReason,
                notes: notes || undefined,
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

            closeRejectionModal();
            showToast(`Candidate #${candidate.id} rejected (${state.selectedReason})`, "danger");

            // Remove rejected item from local queue and cache
            state.imageCache.delete(candidate.id);
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

    function openRejectionModal() {
        stopPanLoop();
        exitLabelingMode();
        if (state.currentIndex < 0 || state.currentIndex >= state.candidates.length) return;
        state.rejectionModalOpen = true;
        rejectionNotesInput.value = "";
        rejectionModal.classList.remove("hidden");
        updateRejectionReasonChips();
        const activeChip = rejectionModal.querySelector(".reason-chip.active");
        if (activeChip) {
            activeChip.focus();
        }
    }

    function closeRejectionModal() {
        stopPanLoop();
        state.rejectionModalOpen = false;
        rejectionModal.classList.add("hidden");
        canvas.focus();
    }

    function selectRejectionReason(reason) {
        state.selectedReason = reason;
        updateRejectionReasonChips();
    }

    function updateRejectionReasonChips() {
        const chips = rejectionModal.querySelectorAll(".reason-chip");
        chips.forEach((chip) => {
            const reason = chip.getAttribute("data-reason");
            if (reason === state.selectedReason) {
                chip.classList.add("active");
            } else {
                chip.classList.remove("active");
            }
        });
    }

    function toggleHelpModal() {
        stopPanLoop();
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
        if (state.rejectionModalOpen) closeRejectionModal();
        if (state.helpModalOpen) closeHelpModal();
        state.drawerOpen = true;
        ingestionDrawer?.classList.remove("hidden");
        btnStartHarvest?.focus();
    }

    function closeIngestionDrawer() {
        stopPanLoop();
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
        if (state.rejectionModalOpen) closeRejectionModal();
        if (state.helpModalOpen) closeHelpModal();
        if (state.drawerOpen) closeIngestionDrawer();
        state.benchmarkDrawerOpen = true;
        benchmarkDrawer?.classList.remove("hidden");
        updatePreflightCost();
    }

    function closeBenchmarkDrawer() {
        stopPanLoop();
        state.benchmarkDrawerOpen = false;
        benchmarkDrawer?.classList.add("hidden");
        canvas.focus();
    }

    function openBudgetGuardModal(estimatedCost, modelCount, promptCount, totalImages) {
        stopPanLoop();
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

            appendBenchmarkLog("GEN", `Generating ${promptName} with ${modelName}...`);
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
    // UI Event Listeners & Hotkeys
    // ==========================================================================

    function setupEventListeners() {
        // --- Canvas Mouse Pan Dragging ---
        canvas.addEventListener("mousedown", (e) => {
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
            if (!state.isPanning) return;
            const dx = e.clientX - state.dragStart.x;
            const dy = e.clientY - state.dragStart.y;
            state.pan.x = state.panStart.x + dx;
            state.pan.y = state.panStart.y + dy;
            requestRender();
            updateHud();
        });

        window.addEventListener("mouseup", (e) => {
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
        btnFitScreen.addEventListener("click", () => fitToScreen());
        document.getElementById("btn-zoom-in").addEventListener("click", () => zoomStep(1.25));
        document.getElementById("btn-zoom-out").addEventListener("click", () => zoomStep(1 / 1.25));

        btnPrev.addEventListener("click", () => navigatePrev());
        btnNext.addEventListener("click", () => navigateNext());
        btnCurate.addEventListener("click", () => curateCurrentCandidate());
        btnReject.addEventListener("click", () => openRejectionModal());

        document.getElementById("btn-help").addEventListener("click", () => toggleHelpModal());
        document.getElementById("btn-close-help-modal").addEventListener("click", () => closeHelpModal());
        document.getElementById("btn-dismiss-help").addEventListener("click", () => closeHelpModal());

        document.getElementById("btn-close-reject-modal").addEventListener("click", () => closeRejectionModal());
        document.getElementById("btn-cancel-reject").addEventListener("click", () => closeRejectionModal());
        document.getElementById("btn-confirm-reject").addEventListener("click", () => confirmRejection());

        document.getElementById("btn-refresh-staging").addEventListener("click", () => {
            fetchStatus();
            fetchStagingQueue();
        });

        // Rejection reasons chip clicks
        const reasonChips = rejectionModal.querySelectorAll(".reason-chip");
        reasonChips.forEach((chip) => {
            chip.addEventListener("click", () => {
                const reason = chip.getAttribute("data-reason");
                if (reason) selectRejectionReason(reason);
            });
        });

        // Theme badges click
        const themeBadges = document.querySelectorAll(".theme-badge");
        themeBadges.forEach((badge) => {
            badge.addEventListener("click", () => {
                const theme = badge.getAttribute("data-theme");
                if (theme) setTheme(theme);
            });
        });

        // Rating stars click
        const starBtns = document.querySelectorAll(".star-btn");
        starBtns.forEach((btn) => {
            btn.addEventListener("click", () => {
                const rating = parseInt(btn.getAttribute("data-rating"), 10);
                if (rating) setRating(rating);
            });
        });

        // Modal backdrop click to close
        rejectionModal.addEventListener("click", (e) => {
            if (e.target === rejectionModal) closeRejectionModal();
        });
        helpModal.addEventListener("click", (e) => {
            if (e.target === helpModal) closeHelpModal();
        });
        setupDrawerControls();


        // --- Keyboard Hotkey Dispatcher ---
        window.addEventListener("keydown", handleKeydown);
        window.addEventListener("keyup", handleKeyup);
        window.addEventListener("blur", stopPanLoop);
        window.addEventListener("focusin", (e) => {
            const tag = e.target?.tagName?.toLowerCase();
            if (tag === "input" || tag === "textarea" || tag === "select") {
                stopPanLoop();
            }
        });
    }

    function handleKeydown(e) {
        const tag = document.activeElement ? document.activeElement.tagName.toLowerCase() : "";
        const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

        // Global Modal Dismiss: Escape
        if (e.key === "Escape") {
            stopPanLoop();
            if (state.labelingMode) {
                exitLabelingMode();
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
            if (state.rejectionModalOpen) {
                closeRejectionModal();
                e.preventDefault();
                return;
            }
            if (state.helpModalOpen) {
                closeHelpModal();
                e.preventDefault();
                return;
            }
            openRejectionModal();
            e.preventDefault();
            return;
        }

        // Hotkeys inside Rejection Modal
        if (state.rejectionModalOpen) {
            if (e.key === "Enter") {
                confirmRejection();
                e.preventDefault();
                return;
            }
            if (!isInputFocused && ["1", "2", "3", "4", "5"].includes(e.key)) {
                const reason = REJECTION_REASONS[parseInt(e.key, 10) - 1];
                if (reason) {
                    selectRejectionReason(reason);
                    e.preventDefault();
                }
                return;
            }
            return;
        }

        // Ignore triage hotkeys if typing in an input
        if (isInputFocused) return;

        // Secondary selection handling for active labeling mode
        if (state.labelingMode === "theme") {
            if (["1", "2", "3", "4", "5", "6", "7", "8", "9"].includes(e.key)) {
                const idx = parseInt(e.key, 10) - 1;
                if (idx >= 0 && idx < THEMES.length) {
                    setTheme(THEMES[idx]);
                }
                exitLabelingMode();
                e.preventDefault();
                return;
            }
        }

        if (state.labelingMode === "rating") {
            if (["1", "2", "3", "4", "5"].includes(e.key)) {
                const rating = parseInt(e.key, 10);
                setRating(rating);
                exitLabelingMode();
                e.preventDefault();
                return;
            }
            if (["6", "7", "8", "9"].includes(e.key)) {
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

        // Two-step labeling mode triggers: 1 (Theme), 2 (Rating)
        if (e.key === "1" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
            enterLabelingMode("theme");
            e.preventDefault();
            return;
        }
        if (e.key === "2" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
            enterLabelingMode("rating");
            e.preventDefault();
            return;
        }
        // 64px VTT Grid toggle: G
        if (e.key === "g" || e.key === "G") {
            toggleGrid();
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

        // Zoom: + or = or ] / - or _ or [
        if (e.key === "+" || e.key === "=" || e.key === "]") {
            zoomStep(1.2);
            e.preventDefault();
            return;
        }
        if (e.key === "-" || e.key === "_" || e.key === "[") {
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
            openRejectionModal();
            e.preventDefault();
            return;
        }
    }

    function handleKeyup(e) {
        const key = e.key.toLowerCase();
        if (key === "w" || key === "a" || key === "s" || key === "d") {
            activePanKeys[key] = false;
            if (!activePanKeys.w && !activePanKeys.a && !activePanKeys.s && !activePanKeys.d) {
                stopPanLoop();
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
        showToast(`64px VTT Grid: ${state.gridVisible ? "Enabled" : "Disabled"}`, "info");
    }

    function enterLabelingMode(mode) {
        stopPanLoop();
        state.labelingMode = mode;
        const themeSelector = document.getElementById("theme-selector");
        const ratingSelector = document.getElementById("rating-selector");
        if (themeSelector) themeSelector.classList.remove("selecting-mode");
        if (ratingSelector) ratingSelector.classList.remove("selecting-mode");

        if (mode === "theme" && themeSelector) {
            themeSelector.classList.add("selecting-mode");
            showToast("Select Theme: press 1-8 (Esc to cancel)", "info");
        } else if (mode === "rating" && ratingSelector) {
            ratingSelector.classList.add("selecting-mode");
            showToast("Select Rating: press 1-5 (Esc to cancel)", "info");
        }
    }

    function exitLabelingMode() {
        stopPanLoop();
        state.labelingMode = null;
        const themeSelector = document.getElementById("theme-selector");
        const ratingSelector = document.getElementById("rating-selector");
        if (themeSelector) themeSelector.classList.remove("selecting-mode");
        if (ratingSelector) ratingSelector.classList.remove("selecting-mode");
    }

    function setTheme(theme) {
        exitLabelingMode();
        state.selectedTheme = theme;
        const badges = document.querySelectorAll(".theme-badge");
        badges.forEach((b) => {
            if (b.getAttribute("data-theme") === theme) {
                b.classList.add("active");
            } else {
                b.classList.remove("active");
            }
        });
        const capitalized = theme.charAt(0).toUpperCase() + theme.slice(1);
        showToast(`Theme: ${capitalized}`, "info");
    }
    function setRating(rating) {
        exitLabelingMode();
        state.selectedRating = rating;
        const starBtns = document.querySelectorAll(".star-btn");
        starBtns.forEach((btn) => {
            const r = parseInt(btn.getAttribute("data-rating"), 10);
            if (r <= rating) {
                btn.classList.add("active");
            } else {
                btn.classList.remove("active");
            }
        });
        showToast(`Rating: ${rating} Star${rating > 1 ? "s" : ""}`, "info");
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

        hudGridStatus.textContent = `Grid: 64px [${state.gridVisible ? "ON" : "OFF"}]`;

        if (state.currentImage && state.currentImage.complete) {
            const imgW = state.currentImage.naturalWidth || state.currentImage.width;
            const imgH = state.currentImage.naturalHeight || state.currentImage.height;
            hudResolution.textContent = `${imgW} × ${imgH} px`;
        } else {
            hudResolution.textContent = "—";
        }

        // Count cached prefetch images
        let readyCount = 0;
        for (let i = 1; i <= PREFETCH_AHEAD; i++) {
            const idx = state.currentIndex + i;
            if (idx < state.candidates.length) {
                const cand = state.candidates[idx];
                const img = state.imageCache.get(cand.id);
                if (img && img.complete && img.naturalWidth > 0) readyCount++;
            }
        }
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
