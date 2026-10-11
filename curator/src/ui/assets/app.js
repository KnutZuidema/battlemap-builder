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
        isSubmitting: false,
    };

    // Constants
    const VTT_GRID_SIZE = 64; // 64px represents standard 5ft D&D VTT square
    const MIN_ZOOM = 0.1;
    const MAX_ZOOM = 5.0;
    const PREFETCH_AHEAD = 3;

    const THEMES = [
        "dungeon",
        "cave",
        "tavern",
        "wilderness",
        "ruins",
        "urban",
        "scifi",
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

    // ==========================================================================
    // Initialization & Bootstrap
    // ==========================================================================

    function init() {
        cacheDOMElements();
        setupCanvas();
        setupEventListeners();
        fetchStatus();
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
        state.helpModalOpen = !state.helpModalOpen;
        if (state.helpModalOpen) {
            helpModal.classList.remove("hidden");
        } else {
            helpModal.classList.add("hidden");
            canvas.focus();
        }
    }

    function closeHelpModal() {
        state.helpModalOpen = false;
        helpModal.classList.add("hidden");
        canvas.focus();
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

        // --- Keyboard Hotkey Dispatcher ---
        window.addEventListener("keydown", handleKeydown);
    }

    function handleKeydown(e) {
        const tag = document.activeElement ? document.activeElement.tagName.toLowerCase() : "";
        const isInputFocused = tag === "input" || tag === "textarea" || tag === "select";

        // Global Modal Dismiss: Escape
        if (e.key === "Escape") {
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

        // Help modal hotkey: ?
        if (e.key === "?" || (e.shiftKey && e.key === "/")) {
            toggleHelpModal();
            e.preventDefault();
            return;
        }

        // Quality rating hotkeys: Shift + 1-5
        if (e.shiftKey && ["1", "2", "3", "4", "5"].includes(e.key)) {
            const rating = parseInt(e.key, 10);
            setRating(rating);
            e.preventDefault();
            return;
        }

        // Theme selection hotkeys: 1-7
        if (!e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && ["1", "2", "3", "4", "5", "6", "7"].includes(e.key)) {
            const idx = parseInt(e.key, 10) - 1;
            const theme = THEMES[idx];
            if (theme) {
                setTheme(theme);
                e.preventDefault();
            }
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

        // Curate action: A or Enter
        if (e.key === "a" || e.key === "A" || e.key === "Enter") {
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

    function setTheme(theme) {
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
