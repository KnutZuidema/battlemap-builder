/**
 * Dungeon Forge Curator — Training Run & LoRA Prototype Script
 *
 * Implements three distinct UI variants answering Issue #189:
 * 1. Variant A: "Unified Slide-Over Training Drawer & Persistent Selection Bar"
 * 2. Variant B: "Dedicated Top-Level 'Training Workbench' View & Floating Progress Window"
 * 3. Variant C: "Modal-Centric Wizard & Top-Center Dynamic Island HUD"
 *
 * Plus:
 * - Floating bottom switcher bar (<- / ->, keyboard arrows, ?variant= URL sync)
 * - Derived ModelRegistry & Benchmark Drawer integration across all variants
 * - Interactive simulation state (live steps, real-time cost, streaming logs, checkpoint sample grids, adapter publication)
 */

(function () {
    "use strict";

    // --------------------------------------------------------------------------
    // Simulated SVG Battlemap Sample Grid Generator (Procedural Thumbnails)
    // --------------------------------------------------------------------------
    function createSampleSvg(title, step, accentColor = "#f59e0b") {
        return `data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">
            <rect width="256" height="256" fill="%230b1320"/>
            <defs>
                <pattern id="grid_${step}" width="16" height="16" patternUnits="userSpaceOnUse">
                    <path d="M 16 0 L 0 0 0 16" fill="none" stroke="%231e293b" stroke-width="0.75"/>
                </pattern>
            </defs>
            <rect width="256" height="256" fill="url(%23grid_${step})"/>
            <!-- Dungeon Walls (Poché) -->
            <path d="M 20 20 L 236 20 L 236 70 L 190 70 L 190 140 L 236 140 L 236 236 L 20 236 Z" fill="%23151f32" stroke="%23334155" stroke-width="2"/>
            <!-- Inner Chamber -->
            <rect x="50" y="50" width="110" height="140" fill="%230f172a" stroke="%23475569" stroke-width="1.5"/>
            <!-- Pillars -->
            <circle cx="75" cy="80" r="8" fill="%23334155" stroke="${encodeURIComponent(accentColor)}" stroke-width="1"/>
            <circle cx="135" cy="80" r="8" fill="%23334155" stroke="${encodeURIComponent(accentColor)}" stroke-width="1"/>
            <circle cx="75" cy="160" r="8" fill="%23334155" stroke="${encodeURIComponent(accentColor)}" stroke-width="1"/>
            <circle cx="135" cy="160" r="8" fill="%23334155" stroke="${encodeURIComponent(accentColor)}" stroke-width="1"/>
            <!-- Central Feature / Altar -->
            <rect x="92" y="110" width="26" height="36" rx="3" fill="%231e293b" stroke="${encodeURIComponent(accentColor)}" stroke-width="1.5"/>
            <!-- Banner / Step Stamp -->
            <rect x="8" y="8" width="90" height="20" rx="4" fill="rgba(0,0,0,0.75)"/>
            <text x="14" y="22" fill="%23f8fafc" font-family="sans-serif" font-size="10" font-weight="bold">${title}</text>
            <rect x="180" y="228" width="68" height="18" rx="3" fill="rgba(0,0,0,0.8)"/>
            <text x="186" y="241" fill="${encodeURIComponent(accentColor)}" font-family="monospace" font-size="9">s${step}</text>
        </svg>`;
    }

    // --------------------------------------------------------------------------
    // In-Memory Simulated State Store
    // --------------------------------------------------------------------------
    const protoState = {
        variant: "A", // 'A' | 'B' | 'C'
        inspectorOpen: false,

        // Mock Curated Maps for Dataset Library Selection
        curatedMaps: [
            { id: "cm_01_crypt", title: "Ancient Granite Crypt", theme: "stone-dungeon", sections: 4, width: 2048, height: 2048, groundFile: "0_ground.png", ready: true },
            { id: "cm_02_tavern", title: "Wayfarer Tavern & Cellar", theme: "tavern", sections: 2, width: 1536, height: 1024, groundFile: "0_ground.png", ready: true },
            { id: "cm_03_ruins", title: "Sunken Citadel Ruins", theme: "ruins", sections: 6, width: 2560, height: 2048, groundFile: "0_ground.png", ready: true },
            { id: "cm_04_cavern", title: "Bioluminescent Grotto", theme: "underdark", sections: 4, width: 2048, height: 2048, groundFile: "0_ground.png", ready: true },
            { id: "cm_05_sewers", title: "Iron Sluice Cistern", theme: "sewer", sections: 4, width: 2048, height: 2048, groundFile: "0_ground.png", ready: true },
            { id: "cm_06_throne", title: "Grand Obsidian Throne", theme: "stone-dungeon", sections: 6, width: 2560, height: 2560, groundFile: "0_ground.png", ready: true },
            { id: "cm_07_ambush", title: "Riverside Gorge Crossing", theme: "wilderness", sections: 4, width: 2048, height: 2048, groundFile: "0_ground.png", ready: true },
            { id: "cm_08_library", title: "Arcane Script Vault", theme: "dungeon", sections: 4, width: 2048, height: 2048, groundFile: "0_ground.png", ready: true },
        ],

        // Selected Map IDs for Training Scope
        selectedMapIds: new Set(["cm_01_crypt", "cm_03_ruins", "cm_05_sewers", "cm_06_throne"]),

        // Training Runs
        runs: [
            {
                id: "run_20261024_01",
                baseModel: "black-forest-labs/FLUX.1-dev",
                triggerPhrase: "dungeonforge battlemap",
                datasetSnapshotId: "snap_stone_dungeon_v1",
                sectionCount: 20,
                state: "succeeded", // 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'
                steps: 1500,
                totalSteps: 1500,
                loss: 0.0642,
                cost: 1.25,
                startedAt: "2026-10-24T09:15:00Z",
                completedAt: "2026-10-24T09:56:42Z",
                checkpoints: [
                    { step: 500, loss: 0.138, sampleUrl: createSampleSvg("Step 500 Sample", 500, "#f59e0b"), r2Verified: true },
                    { step: 1000, loss: 0.092, sampleUrl: createSampleSvg("Step 1000 Sample", 1000, "#f59e0b"), r2Verified: true },
                    { step: 1500, loss: 0.064, sampleUrl: createSampleSvg("Step 1500 Final", 1500, "#10b981"), r2Verified: true, published: true }
                ],
                logs: [
                    "[2026-10-24 09:15:02] [modal] Provisioning L40S GPU node in us-east...",
                    "[2026-10-24 09:15:18] [ai-toolkit] Loaded Base Model black-forest-labs/FLUX.1-dev (Rank 32, Alpha 16)",
                    "[2026-10-24 09:15:22] [ai-toolkit] Unpacked 20 16×16 Map Section crops (1024×1024 @ 64px = 1 cell)",
                    "[2026-10-24 09:28:40] [checkpoint] Step 500 reached. Loss: 0.138. Uploaded adapter to R2.",
                    "[2026-10-24 09:42:15] [checkpoint] Step 1000 reached. Loss: 0.092. Uploaded adapter to R2.",
                    "[2026-10-24 09:56:40] [checkpoint] Step 1500 completed. Loss: 0.064. Artifact verification: PASSED.",
                    "[2026-10-24 09:56:42] [curator] Training run run_20261024_01 marked SUCCEEDED. Total spend: $1.25."
                ]
            },
            {
                id: "run_20261024_02",
                baseModel: "black-forest-labs/FLUX.1-dev",
                triggerPhrase: "dungeonforge battlemap",
                datasetSnapshotId: "snap_crypt_ruins_v2",
                sectionCount: 18,
                state: "running",
                steps: 840,
                totalSteps: 1500,
                loss: 0.0842,
                cost: 0.70,
                startedAt: "2026-10-24T10:45:00Z",
                completedAt: null,
                checkpoints: [
                    { step: 500, loss: 0.141, sampleUrl: createSampleSvg("Step 500 Sample", 500, "#f59e0b"), r2Verified: true }
                ],
                logs: [
                    "[2026-10-24 10:45:04] [modal] Job dispatched to L40S tier (Node modal-l40s-8821).",
                    "[2026-10-24 10:45:20] [ai-toolkit] Initialized Prodigy optimizer (lr=1.0, d_coef=1.0).",
                    "[2026-10-24 10:45:24] [ai-toolkit] Dataset verified: 18 sections with Trigger Phrase prefix.",
                    "[2026-10-24 10:58:12] [checkpoint] Step 500 checkpoint saved to R2 artifacts prefix.",
                    "[2026-10-24 11:05:44] [step 800/1500] step_loss=0.0862 avg_loss=0.0842 time_per_step=1.52s",
                    "[2026-10-24 11:06:50] [step 840/1500] step_loss=0.0831 avg_loss=0.0842 vram_used=42.1GB"
                ]
            }
        ],

        // Published LoRA Adapters (Derived Model Registry entries per ADR-0010)
        adapters: [
            {
                id: "dungeonforge-flux-s1500",
                runId: "run_20261024_01",
                step: 1500,
                scale: 1.0,
                baseModel: "FLUX.1-dev",
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                r2Status: "verified",
                retired: false,
                createdAt: "2026-10-24T09:58:00Z",
                engines: [
                    {
                        id: "dungeonforge-flux-s1500",
                        engine: "text-to-image-lora",
                        name: "DungeonForge FLUX LoRA (s1500 · T2I)",
                        pricePerImage: 0.015,
                        hasVision: false,
                        supportsReferenceImage: false,
                        provider: "wavespeed"
                    },
                    {
                        id: "dungeonforge-flux-s1500-edit",
                        engine: "edit-lora",
                        name: "DungeonForge FLUX LoRA Edit (s1500 · Reference-Capable)",
                        pricePerImage: 0.021,
                        hasVision: true,
                        supportsReferenceImage: true,
                        provider: "wavespeed"
                    }
                ]
            }
        ]
    };

    // Calculate total sections selected
    function getSelectedSectionsCount() {
        let total = 0;
        for (const m of protoState.curatedMaps) {
            if (protoState.selectedMapIds.has(m.id)) {
                total += m.sections;
            }
        }
        return total;
    }

    // --------------------------------------------------------------------------
    // Simulation Engine Ticker (Live Training Progress Simulation)
    // --------------------------------------------------------------------------
    let simInterval = null;

    function startSimulationTicker() {
        clearInterval(simInterval);
        simInterval = setInterval(() => {
            const activeRun = protoState.runs.find(r => r.state === "running");
            if (!activeRun) return;

            // Increment steps
            if (activeRun.steps < activeRun.totalSteps) {
                activeRun.steps += 10;
                activeRun.loss = Math.max(0.045, +(activeRun.loss - 0.0003).toFixed(4));
                activeRun.cost = +(activeRun.cost + 0.0083).toFixed(4);

                // Add log line every 50 steps
                if (activeRun.steps % 50 === 0) {
                    const now = new Date().toTimeString().split(" ")[0];
                    activeRun.logs.push(`[${now}] [step ${activeRun.steps}/${activeRun.totalSteps}] loss=${activeRun.loss} gpu_temp=68C vram=42.3GB`);
                    if (activeRun.logs.length > 50) activeRun.logs.shift();
                }

                // Check for step 1000 checkpoint
                if (activeRun.steps === 1000 && !activeRun.checkpoints.find(c => c.step === 1000)) {
                    activeRun.checkpoints.push({
                        step: 1000,
                        loss: activeRun.loss,
                        sampleUrl: createSampleSvg("Step 1000 Sample", 1000, "#f59e0b"),
                        r2Verified: true
                    });
                    activeRun.logs.push(`[checkpoint] Checkpoint step 1000 saved and verified in R2.`);
                }

                // Update active UI elements if mounted
                updateActiveRunViews();
            } else {
                // Completed run
                activeRun.state = "succeeded";
                activeRun.completedAt = new Date().toISOString();
                if (!activeRun.checkpoints.find(c => c.step === activeRun.totalSteps)) {
                    activeRun.checkpoints.push({
                        step: activeRun.totalSteps,
                        loss: activeRun.loss,
                        sampleUrl: createSampleSvg("Step 1500 Final", activeRun.totalSteps, "#10b981"),
                        r2Verified: true,
                        published: false
                    });
                }
                activeRun.logs.push(`[curator] Run completed successfully! 3 checkpoints verified.`);
                updateActiveRunViews();
                showProtoToast(`Training Run ${activeRun.id} completed! Checkpoints ready to evaluate.`);
            }
        }, 3000);
    }

    // Step simulation manually by +100 steps
    function stepSimulationForward() {
        const activeRun = protoState.runs.find(r => r.state === "running");
        if (!activeRun) {
            showProtoToast("No active running job to advance. Starting a new run...", "info");
            dispatchNewSimRun();
            return;
        }
        activeRun.steps = Math.min(activeRun.totalSteps, activeRun.steps + 100);
        activeRun.cost = +(activeRun.cost + 0.08).toFixed(4);
        activeRun.loss = Math.max(0.045, +(activeRun.loss - 0.003).toFixed(4));
        const now = new Date().toTimeString().split(" ")[0];
        activeRun.logs.push(`[${now}] [FAST-FORWARD +100] step=${activeRun.steps}/${activeRun.totalSteps} loss=${activeRun.loss}`);

        if (activeRun.steps >= 1000 && !activeRun.checkpoints.find(c => c.step === 1000)) {
            activeRun.checkpoints.push({
                step: 1000,
                loss: 0.092,
                sampleUrl: createSampleSvg("Step 1000 Sample", 1000, "#f59e0b"),
                r2Verified: true
            });
        }
        if (activeRun.steps >= activeRun.totalSteps) {
            activeRun.state = "succeeded";
            activeRun.completedAt = new Date().toISOString();
            if (!activeRun.checkpoints.find(c => c.step === activeRun.totalSteps)) {
                activeRun.checkpoints.push({
                    step: activeRun.totalSteps,
                    loss: activeRun.loss,
                    sampleUrl: createSampleSvg("Step 1500 Final", activeRun.totalSteps, "#10b981"),
                    r2Verified: true,
                    published: false
                });
            }
        }
        updateActiveRunViews();
        showProtoToast(`Advanced active run to step ${activeRun.steps}/${activeRun.totalSteps}`, "success");
    }

    // Dispatch a new simulated training run
    function dispatchNewSimRun(recipe = {}) {
        // Enforce ADR-0009: one active run serialized
        const running = protoState.runs.find(r => r.state === "running");
        if (running) {
            showProtoToast("Conflict: Another run is currently running. Serialized execution enforced (ADR-0009).", "warning");
            return false;
        }

        const runId = "run_" + new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14);
        const sectionCount = getSelectedSectionsCount() || 20;
        const totalSteps = recipe.steps || 1500;

        const newRun = {
            id: runId,
            baseModel: recipe.baseModel || "black-forest-labs/FLUX.1-dev",
            triggerPhrase: recipe.triggerPhrase || "dungeonforge battlemap",
            datasetSnapshotId: "snap_" + runId,
            sectionCount: sectionCount,
            state: "running",
            steps: 0,
            totalSteps: totalSteps,
            loss: 0.285,
            cost: 0.05,
            startedAt: new Date().toISOString(),
            completedAt: null,
            checkpoints: [],
            logs: [
                `[${new Date().toTimeString().split(" ")[0]}] [modal] Job ${runId} queued on Modal Cloud (L40S tier).`,
                `[${new Date().toTimeString().split(" ")[0]}] [curator] Frozen Dataset Snapshot snap_${runId} (${sectionCount} sections) verified in R2.`,
                `[${new Date().toTimeString().split(" ")[0]}] [ai-toolkit] Initializing training recipe: ${totalSteps} steps, Rank 32, Alpha 16...`
            ]
        };

        protoState.runs.unshift(newRun);
        updateActiveRunViews();
        showProtoToast(`Dispatched Training Run ${runId} (${sectionCount} sections · ${totalSteps} steps)!`, "success");
        return true;
    }

    // Publish a LoRA Adapter from a Checkpoint
    function publishAdapterFromCheckpoint(runId, step, scale = 1.0) {
        const run = protoState.runs.find(r => r.id === runId);
        if (!run) return;
        const cp = run.checkpoints.find(c => c.step === step);
        if (!cp) return;

        const adapterId = `${run.id}-s${step}`;
        const existing = protoState.adapters.find(a => a.id === adapterId);
        if (existing) {
            showProtoToast(`Adapter ${adapterId} is already published in the catalog.`, "info");
            return;
        }

        cp.published = true;
        const newAdapter = {
            id: adapterId,
            runId: run.id,
            step: step,
            scale: scale,
            baseModel: "FLUX.1-dev",
            sha256: "a1b2c3d4e5f67890" + Math.random().toString(16).slice(2, 10),
            r2Status: "verified",
            retired: false,
            createdAt: new Date().toISOString(),
            engines: [
                {
                    id: adapterId,
                    engine: "text-to-image-lora",
                    name: `DungeonForge FLUX LoRA (${adapterId} · T2I)`,
                    pricePerImage: 0.015,
                    hasVision: false,
                    supportsReferenceImage: false,
                    provider: "wavespeed"
                },
                {
                    id: `${adapterId}-edit`,
                    engine: "edit-lora",
                    name: `DungeonForge FLUX LoRA Edit (${adapterId} · Reference-Capable)`,
                    pricePerImage: 0.021,
                    hasVision: true,
                    supportsReferenceImage: true,
                    provider: "wavespeed"
                }
            ]
        };

        protoState.adapters.unshift(newAdapter);
        renderBenchmarkDrawerModels();
        updateActiveRunViews();
        showProtoToast(`Published LoRA Adapter ${adapterId} to Catalog & Benchmark Drawer!`, "success");
    }

    // Toggle Retire / Restore Adapter (ADR-0009 / ADR-0010)
    function toggleRetireAdapter(adapterId) {
        const ad = protoState.adapters.find(a => a.id === adapterId);
        if (!ad) return;
        ad.retired = !ad.retired;
        renderBenchmarkDrawerModels();
        updateActiveRunViews();
        showProtoToast(`Adapter ${adapterId} ${ad.retired ? "soft-retired (hidden from benchmark)" : "restored as active"}`, "info");
    }

    // Reset simulation state
    function resetSimState() {
        protoState.runs = [
            {
                id: "run_20261024_01",
                baseModel: "black-forest-labs/FLUX.1-dev",
                triggerPhrase: "dungeonforge battlemap",
                datasetSnapshotId: "snap_stone_dungeon_v1",
                sectionCount: 20,
                state: "succeeded",
                steps: 1500,
                totalSteps: 1500,
                loss: 0.0642,
                cost: 1.25,
                startedAt: "2026-10-24T09:15:00Z",
                completedAt: "2026-10-24T09:56:42Z",
                checkpoints: [
                    { step: 500, loss: 0.138, sampleUrl: createSampleSvg("Step 500 Sample", 500, "#f59e0b"), r2Verified: true },
                    { step: 1000, loss: 0.092, sampleUrl: createSampleSvg("Step 1000 Sample", 1000, "#f59e0b"), r2Verified: true },
                    { step: 1500, loss: 0.064, sampleUrl: createSampleSvg("Step 1500 Final", 1500, "#10b981"), r2Verified: true, published: true }
                ],
                logs: [
                    "[2026-10-24 09:15:02] [modal] Provisioning L40S GPU node in us-east...",
                    "[2026-10-24 09:15:18] [ai-toolkit] Loaded Base Model black-forest-labs/FLUX.1-dev",
                    "[2026-10-24 09:56:40] [checkpoint] Step 1500 completed. Artifact verification: PASSED."
                ]
            }
        ];
        protoState.selectedMapIds = new Set(["cm_01_crypt", "cm_03_ruins", "cm_05_sewers", "cm_06_throne"]);
        updateActiveRunViews();
        renderBenchmarkDrawerModels();
        showProtoToast("Prototype state reset to baseline.", "info");
    }

    // Helper Toast notification
    function showProtoToast(msg, type = "info") {
        const container = document.getElementById("toast-container");
        if (!container) return;
        const toast = document.createElement("div");
        toast.className = `toast toast-${type === "warning" ? "danger" : type}`;
        toast.innerHTML = `<span style="margin-right:6px">⚡</span><span>${msg}</span>`;
        container.appendChild(toast);
        setTimeout(() => toast.remove(), 4000);
    }

    // --------------------------------------------------------------------------
    // Floating Prototype Switcher Bar (Bottom-Center)
    // --------------------------------------------------------------------------
    const VARIANTS = [
        { key: "A", name: "Slide-Over Drawer & Selection Bar", desc: "Unified right-hand drawer and sticky multi-select bar in Library" },
        { key: "B", name: "Training Workbench & Floating HUD", desc: "Top-level 3-column view with live node charts & floating window" },
        { key: "C", name: "Modal Wizard & Dynamic Island HUD", desc: "3-step configuration modal and sleek top-center Dynamic Island" }
    ];

    function renderPrototypeSwitcher() {
        let bar = document.getElementById("proto-switcher-bar");
        if (!bar) {
            bar = document.createElement("div");
            bar.id = "proto-switcher-bar";
            bar.className = "proto-switcher-bar";
            document.body.appendChild(bar);
        }

        const currentVar = VARIANTS.find(v => v.key === protoState.variant) || VARIANTS[0];

        bar.innerHTML = `
            <button type="button" id="proto-btn-prev" class="proto-switcher-nav-btn" title="Previous Variant (Left Arrow)">◀</button>
            <div class="proto-variant-label" title="${currentVar.desc}">
                <span class="proto-variant-key">Variant ${currentVar.key}</span>
                <span class="proto-variant-name">${currentVar.name}</span>
            </div>
            <button type="button" id="proto-btn-next" class="proto-switcher-nav-btn" title="Next Variant (Right Arrow)">▶</button>
            <div class="proto-switcher-divider"></div>
            <button type="button" id="proto-btn-step" class="proto-switcher-tool-btn" title="Advance running simulation by +100 steps">
                <span>▶▶ +100 Steps</span>
            </button>
            <button type="button" id="proto-btn-inspector" class="proto-switcher-tool-btn" title="View live prototype state JSON">
                <span>State Inspector</span>
            </button>
            <button type="button" id="proto-btn-reset" class="proto-switcher-tool-btn" title="Reset simulation state">
                <span>↺ Reset</span>
            </button>
        `;

        document.getElementById("proto-btn-prev")?.addEventListener("click", () => cycleVariant(-1));
        document.getElementById("proto-btn-next")?.addEventListener("click", () => cycleVariant(1));
        document.getElementById("proto-btn-step")?.addEventListener("click", () => stepSimulationForward());
        document.getElementById("proto-btn-inspector")?.addEventListener("click", () => toggleStateInspector());
        document.getElementById("proto-btn-reset")?.addEventListener("click", () => resetSimState());
    }

    function cycleVariant(delta) {
        const idx = VARIANTS.findIndex(v => v.key === protoState.variant);
        const nextIdx = (idx + delta + VARIANTS.length) % VARIANTS.length;
        setVariant(VARIANTS[nextIdx].key);
    }

    function setVariant(varKey) {
        if (!VARIANTS.find(v => v.key === varKey)) return;
        protoState.variant = varKey;

        // Update URL search param
        const url = new URL(window.location.href);
        url.searchParams.set("variant", varKey);
        window.history.replaceState({}, "", url.toString());

        // Teardown previous variant DOM
        teardownVariants();

        // Render new variant DOM
        renderPrototypeSwitcher();
        if (varKey === "A") initVariantA();
        else if (varKey === "B") initVariantB();
        else if (varKey === "C") initVariantC();

        renderBenchmarkDrawerModels();
        showProtoToast(`Switched to Variant ${varKey}: ${VARIANTS.find(v => v.key === varKey)?.name}`, "info");
    }

    function teardownVariants() {
        // Teardown Variant A
        document.getElementById("btn-open-training-drawer")?.remove();
        document.getElementById("vA-selection-bar")?.remove();
        document.getElementById("vA-drawer")?.remove();
        document.querySelectorAll(".proto-card-checkbox").forEach(el => el.remove());

        // Teardown Variant B
        document.getElementById("tab-nav-training-vB")?.remove();
        document.getElementById("vB-workbench-view")?.remove();
        document.getElementById("vB-library-stage-banner")?.remove();
        document.getElementById("vB-floating-window")?.remove();
        document.getElementById("vB-floating-pill")?.remove();

        // Teardown Variant C
        document.getElementById("vC-dynamic-island")?.remove();
        document.getElementById("vC-island-dropdown")?.remove();
        document.getElementById("vC-launch-fab")?.remove();
        document.getElementById("vC-wizard-modal")?.remove();
        document.getElementById("vC-eval-modal")?.remove();
    }

    // --------------------------------------------------------------------------
    // State Inspector Modal
    // --------------------------------------------------------------------------
    function toggleStateInspector() {
        let modal = document.getElementById("proto-state-inspector");
        if (modal) {
            modal.remove();
            return;
        }

        modal = document.createElement("div");
        modal.id = "proto-state-inspector";
        modal.className = "proto-inspector-modal";
        document.body.appendChild(modal);

        const activeRun = protoState.runs.find(r => r.state === "running");

        modal.innerHTML = `
            <div class="proto-inspector-header">
                <div class="proto-inspector-title">
                    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M12 2v20M2 12h20"/>
                    </svg>
                    <span>Curator Training State Inspector (Issue #189)</span>
                </div>
                <button type="button" class="btn-close" id="btn-close-state-inspector">×</button>
            </div>
            <div class="proto-inspector-body">
                <div>
                    <div class="proto-inspector-section-title">Current Summary</div>
                    <div style="display:flex; gap:12px; font-size:12px;">
                        <span class="badge badge-lora">Active Variant: ${protoState.variant}</span>
                        <span class="badge ${activeRun ? "badge-status-running" : "badge-curator"}">
                            ${activeRun ? `Running: ${activeRun.id} (${activeRun.steps}/${activeRun.totalSteps})` : "No Active Run"}
                        </span>
                        <span class="badge badge-count">Published Adapters: ${protoState.adapters.length}</span>
                        <span class="badge badge-source">Selected Maps: ${protoState.selectedMapIds.size} (${getSelectedSectionsCount()} sections)</span>
                    </div>
                </div>
                <div>
                    <div class="proto-inspector-section-title">Derived Model Registry (ADR-0010)</div>
                    <div class="proto-inspector-json">${JSON.stringify(protoState.adapters, null, 2)}</div>
                </div>
                <div>
                    <div class="proto-inspector-section-title">Training Runs History (ADR-0009)</div>
                    <div class="proto-inspector-json">${JSON.stringify(protoState.runs, null, 2)}</div>
                </div>
            </div>
        `;

        document.getElementById("btn-close-state-inspector")?.addEventListener("click", () => modal.remove());
    }

    // --------------------------------------------------------------------------
    // VARIANT A: Unified Slide-Over Training Drawer & Selection Bar
    // --------------------------------------------------------------------------
    function initVariantA() {
        // 1. Top HUD Button
        const brandSection = document.querySelector(".hud-brand");
        if (brandSection && !document.getElementById("btn-open-training-drawer")) {
            const btn = document.createElement("button");
            btn.type = "button";
            btn.id = "btn-open-training-drawer";
            btn.className = "vA-hud-btn-training";
            btn.title = "Open Training Runs & LoRA Adapter Drawer (Hotkey: R)";

            const activeRun = protoState.runs.find(r => r.state === "running");
            btn.innerHTML = `
                <span class="${activeRun ? "pulse-dot" : ""}"></span>
                <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2">
                    <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
                </svg>
                <span>Training</span>
                <span class="badge badge-count mono" id="vA-hud-run-badge">${activeRun ? `${Math.round(activeRun.steps / activeRun.totalSteps * 100)}%` : protoState.runs.length}</span>
            `;
            btn.addEventListener("click", () => toggleVariantADrawer());
            brandSection.appendChild(btn);
        }

        // 2. Selection checkboxes on curated cards in Library
        renderLibraryCardCheckboxes();

        // 3. Sticky bottom Selection Bar in Library
        renderVariantASelectionBar();
    }

    function toggleVariantADrawer(targetTab = "new-run") {
        let drawer = document.getElementById("vA-drawer");
        if (drawer) {
            drawer.remove();
            return;
        }

        drawer = document.createElement("aside");
        drawer.id = "vA-drawer";
        drawer.className = "vA-drawer";
        document.body.appendChild(drawer);

        renderVariantADrawerContent(drawer, targetTab);
    }

    function renderVariantADrawerContent(drawer, activeTab = "new-run") {
        const activeRun = protoState.runs.find(r => r.state === "running");
        const sectionsCount = getSelectedSectionsCount() || 20;

        drawer.innerHTML = `
            <div class="vA-drawer-header">
                <div style="display:flex; align-items:center; gap:8px;">
                    <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="#fbbf24" stroke-width="2">
                        <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
                    </svg>
                    <h3 style="font-size:15px; font-weight:700; color:#f8fafc;">Training Runs & LoRA Studio</h3>
                </div>
                <button type="button" class="btn-close" id="vA-btn-close-drawer">×</button>
            </div>

            <div class="vA-drawer-tabs">
                <button type="button" class="vA-drawer-tab-btn ${activeTab === "new-run" ? "active" : ""}" data-tab="new-run">New Run</button>
                <button type="button" class="vA-drawer-tab-btn ${activeTab === "active-run" ? "active" : ""}" data-tab="active-run">
                    Active Runs ${activeRun ? '<span class="pulse-dot" style="display:inline-block; vertical-align:middle; margin-left:4px;"></span>' : ""}
                </button>
                <button type="button" class="vA-drawer-tab-btn ${activeTab === "adapters" ? "active" : ""}" data-tab="adapters">LoRA Catalog (${protoState.adapters.length})</button>
            </div>

            <div class="vA-drawer-body" id="vA-drawer-body">
                <!-- Tab contents populated below -->
            </div>
        `;

        document.getElementById("vA-btn-close-drawer")?.addEventListener("click", () => drawer.remove());
        drawer.querySelectorAll(".vA-drawer-tab-btn").forEach(btn => {
            btn.addEventListener("click", (e) => {
                drawer.querySelectorAll(".vA-drawer-tab-btn").forEach(b => b.classList.remove("active"));
                btn.classList.add("active");
                renderVariantATab(btn.dataset.tab);
            });
        });

        renderVariantATab(activeTab);
    }

    function renderVariantATab(tab) {
        const body = document.getElementById("vA-drawer-body");
        if (!body) return;

        if (tab === "new-run") {
            const sectionsCount = getSelectedSectionsCount() || 20;
            const mapsCount = protoState.selectedMapIds.size || 6;

            body.innerHTML = `
                <!-- Scope Card -->
                <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:10px; padding:12px;">
                    <div style="display:flex; justify-content:space-between; align-items:center;">
                        <span style="font-weight:700; font-size:13px; color:#f8fafc;">Dataset Scope & Readiness</span>
                        <span class="badge badge-status-succeeded">✓ 100% Ready</span>
                    </div>
                    <p style="font-size:12px; color:var(--text-muted); margin-top:4px;">
                        Selected: <strong style="color:#f8fafc;">${mapsCount} Curated Maps</strong> contributing <strong style="color:#fbbf24;">${sectionsCount} Map Sections (16×16)</strong>.
                    </p>
                    <div style="display:flex; gap:8px; margin-top:8px; font-size:11px;">
                        <span class="badge badge-source">Base Model: FLUX.1-dev</span>
                        <span class="badge badge-count">Resolution: 1024×1024</span>
                        <span class="badge badge-lora">Trigger: dungeonforge battlemap</span>
                    </div>
                </div>

                <!-- Recipe Presets -->
                <div>
                    <label style="font-size:12px; font-weight:700; color:#f8fafc;">Training Recipe Preset</label>
                    <div class="vA-preset-chips">
                        <div class="vA-preset-chip active" data-steps="1500" data-cost="1.25">
                            <span class="vA-preset-name">Standard</span>
                            <span class="vA-preset-meta">1500 steps · ~$1.25</span>
                        </div>
                        <div class="vA-preset-chip" data-steps="500" data-cost="0.42">
                            <span class="vA-preset-name">Quick Test</span>
                            <span class="vA-preset-meta">500 steps · ~$0.42</span>
                        </div>
                        <div class="vA-preset-chip" data-steps="3000" data-cost="2.50">
                            <span class="vA-preset-name">Deep Precision</span>
                            <span class="vA-preset-meta">3000 steps · ~$2.50</span>
                        </div>
                    </div>
                </div>

                <!-- Hyperparameters Form -->
                <div style="background:rgba(255,255,255,0.02); border:1px solid var(--border-subtle); border-radius:10px; padding:12px; display:flex; flex-direction:column; gap:10px;">
                    <div style="display:flex; justify-content:space-between; font-size:12px;">
                        <span>LoRA Rank: <strong style="color:#f8fafc;">32</strong></span>
                        <span>LoRA Alpha: <strong style="color:#f8fafc;">16</strong></span>
                        <span>Optimizer: <strong style="color:#f8fafc;">Prodigy</strong></span>
                    </div>
                    <div style="font-size:11.5px; color:var(--text-muted);">
                        Trigger Phrase: <code class="mono" style="color:#38bdf8;">dungeonforge battlemap</code> (applied once per ADR-0008)
                    </div>
                </div>

                <!-- Launch Button -->
                <button type="button" id="vA-btn-dispatch" class="btn btn-primary btn-block" style="padding:12px; font-size:14px; font-weight:700; background:linear-gradient(135deg, #f59e0b, #d97706); color:#0b0f17; border:none;">
                    ⚡ Dispatch Training Run ($1.25)
                </button>
                <span style="font-size:11px; text-align:center; color:var(--text-dim);">
                    Dispatches to Modal Cloud (L40S tier). Serialized run lock enforced.
                </span>
            `;

            body.querySelectorAll(".vA-preset-chip").forEach(chip => {
                chip.addEventListener("click", () => {
                    body.querySelectorAll(".vA-preset-chip").forEach(c => c.classList.remove("active"));
                    chip.classList.add("active");
                    const btn = document.getElementById("vA-btn-dispatch");
                    if (btn) btn.innerText = `⚡ Dispatch Training Run ($${chip.dataset.cost})`;
                });
            });

            document.getElementById("vA-btn-dispatch")?.addEventListener("click", () => {
                const activePreset = body.querySelector(".vA-preset-chip.active");
                const steps = activePreset ? parseInt(activePreset.dataset.steps, 10) : 1500;
                const ok = dispatchNewSimRun({ steps });
                if (ok) renderVariantATab("active-run");
            });

        } else if (tab === "active-run") {
            const activeRun = protoState.runs.find(r => r.state === "running") || protoState.runs[0];
            if (!activeRun) {
                body.innerHTML = `<div class="empty-card" style="padding:24px; text-align:center;">No active or historical training runs.</div>`;
                return;
            }

            const pct = Math.round((activeRun.steps / activeRun.totalSteps) * 100);

            body.innerHTML = `
                <!-- Run Header Status -->
                <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:10px; padding:12px;">
                    <div style="display:flex; justify-content:space-between; align-items:center;">
                        <span class="mono" style="font-weight:700; font-size:13px; color:#f8fafc;">${activeRun.id}</span>
                        <span class="badge ${activeRun.state === "running" ? "badge-status-running" : "badge-status-succeeded"}">
                            ${activeRun.state.toUpperCase()}
                        </span>
                    </div>
                    <div style="margin-top:10px;">
                        <div style="display:flex; justify-content:space-between; font-size:12px; margin-bottom:4px;">
                            <span>Progress (${pct}%)</span>
                            <span class="mono">${activeRun.steps} / ${activeRun.totalSteps} steps</span>
                        </div>
                        <div class="progress-bar-track" style="height:6px; background:#1e293b; border-radius:3px; overflow:hidden;">
                            <div class="progress-bar-fill" style="width:${pct}%; height:100%; background:linear-gradient(90deg, #f59e0b, #10b981);"></div>
                        </div>
                    </div>
                    <div style="display:flex; justify-content:space-between; margin-top:10px; font-size:11.5px; color:var(--text-muted);">
                        <span>Cost: <strong class="mono" style="color:#f8fafc;">$${activeRun.cost.toFixed(2)}</strong></span>
                        <span>Loss: <strong class="mono" style="color:#a5f3fc;">${activeRun.loss}</strong></span>
                        <span>Base: <strong style="color:#f8fafc;">FLUX.1-dev</strong></span>
                    </div>
                </div>

                <!-- Checkpoints Section -->
                <div>
                    <div style="font-size:12px; font-weight:700; color:#f8fafc; margin-bottom:8px;">
                        Checkpoints & Sample Grids (${activeRun.checkpoints.length})
                    </div>
                    <div class="proto-checkpoint-grid">
                        ${activeRun.checkpoints.map(cp => `
                            <div class="proto-checkpoint-card">
                                <div class="proto-checkpoint-thumb">
                                    <img src="${cp.sampleUrl}" alt="Step ${cp.step}" style="width:100%; height:100%; object-fit:cover;"/>
                                </div>
                                <div class="proto-checkpoint-footer">
                                    <div style="display:flex; justify-content:space-between; font-size:11px;">
                                        <span class="mono" style="font-weight:700;">s${cp.step}</span>
                                        <span class="badge badge-source" style="font-size:9px;">loss ${cp.loss}</span>
                                    </div>
                                    ${cp.published ?
                                        `<span class="badge badge-status-succeeded" style="font-size:9px; text-align:center;">Published ✓</span>` :
                                        `<button type="button" class="btn btn-secondary btn-sm vA-btn-publish" data-run="${activeRun.id}" data-step="${cp.step}" style="font-size:10px; padding:2px 4px;">Publish Adapter</button>`
                                    }
                                </div>
                            </div>
                        `).join("")}
                    </div>
                </div>

                <!-- Streaming Log Terminal -->
                <div>
                    <div style="display:flex; justify-content:space-between; font-size:12px; font-weight:700; color:#f8fafc; margin-bottom:6px;">
                        <span>Execution Logs</span>
                        <span class="badge badge-source" style="font-size:9px;">Modal L40S</span>
                    </div>
                    <div class="proto-log-terminal">
                        ${activeRun.logs.map(line => `<div class="proto-log-line ${line.includes("Loss") || line.includes("step") ? "highlight" : ""}">${line}</div>`).join("")}
                    </div>
                </div>
            `;

            body.querySelectorAll(".vA-btn-publish").forEach(btn => {
                btn.addEventListener("click", () => {
                    publishAdapterFromCheckpoint(btn.dataset.run, parseInt(btn.dataset.step, 10));
                    renderVariantATab("active-run");
                });
            });

        } else if (tab === "adapters") {
            body.innerHTML = `
                <div style="display:flex; justify-content:space-between; align-items:center;">
                    <span style="font-weight:700; font-size:13px; color:#f8fafc;">Published LoRA Adapters (${protoState.adapters.length})</span>
                    <span style="font-size:11px; color:var(--text-muted);">WaveSpeed & Modal Serving</span>
                </div>

                <div style="display:flex; flex-direction:column; gap:10px;">
                    ${protoState.adapters.map(ad => `
                        <div style="background:rgba(255,255,255,0.03); border:1px solid ${ad.retired ? "var(--border-subtle)" : "rgba(245,158,11,0.4)"}; border-radius:10px; padding:12px; opacity:${ad.retired ? 0.6 : 1};">
                            <div style="display:flex; justify-content:space-between; align-items:flex-start;">
                                <div>
                                    <div style="display:flex; align-items:center; gap:6px;">
                                        <span class="badge badge-lora">LoRA</span>
                                        <span class="mono" style="font-weight:700; color:#f8fafc; font-size:13px;">${ad.id}</span>
                                    </div>
                                    <div style="font-size:11px; color:var(--text-muted); margin-top:4px;">
                                        Base: ${ad.baseModel} · Step: ${ad.step} · Scale: ${ad.scale}
                                    </div>
                                </div>
                                <span class="badge ${ad.retired ? "badge-source" : "badge-status-succeeded"}">
                                    ${ad.retired ? "Retired" : "Active"}
                                </span>
                            </div>

                            <div style="margin-top:8px; display:flex; gap:6px;">
                                ${ad.engines.map(eng => `
                                    <span class="badge badge-engine">
                                        ${eng.engine} ($${eng.pricePerImage}/map)
                                    </span>
                                `).join("")}
                            </div>

                            <div style="display:flex; justify-content:space-between; align-items:center; margin-top:10px; border-top:1px solid var(--border-subtle); padding-top:8px;">
                                <span class="mono" style="font-size:10px; color:var(--text-dim);" title="SHA-256 Checksum">
                                    sha256:${ad.sha256.slice(0, 12)}...
                                </span>
                                <button type="button" class="btn btn-ghost btn-sm vA-btn-toggle-retire" data-id="${ad.id}" style="font-size:11px;">
                                    ${ad.retired ? "Restore Adapter" : "Retire Adapter"}
                                </button>
                            </div>
                        </div>
                    `).join("")}
                </div>
            `;

            body.querySelectorAll(".vA-btn-toggle-retire").forEach(btn => {
                btn.addEventListener("click", () => {
                    toggleRetireAdapter(btn.dataset.id);
                    renderVariantATab("adapters");
                });
            });
        }
    }

    function renderVariantASelectionBar() {
        let bar = document.getElementById("vA-selection-bar");
        const libraryViewport = document.getElementById("library-viewport");
        if (!libraryViewport) return;

        if (!bar) {
            bar = document.createElement("div");
            bar.id = "vA-selection-bar";
            bar.className = "vA-selection-bar";
            libraryViewport.appendChild(bar);
        }

        const mapsCount = protoState.selectedMapIds.size;
        const sectionsCount = getSelectedSectionsCount();

        bar.innerHTML = `
            <div class="vA-selection-meta">
                <span class="vA-selection-count">
                    ⚡ <span style="color:#fbbf24;">${mapsCount} Maps</span> Selected
                    <span style="font-weight:400; color:var(--text-muted); font-size:12.5px;">(${sectionsCount} Map Sections · 16×16)</span>
                </span>
                <span class="badge badge-status-succeeded" style="font-size:11px;">Readiness: 100% Pass</span>
            </div>
            <div class="vA-selection-actions">
                <button type="button" id="vA-btn-select-all" class="btn btn-secondary btn-sm">Select All (${protoState.curatedMaps.length})</button>
                <button type="button" id="vA-btn-clear-selection" class="btn btn-ghost btn-sm">Clear</button>
                <button type="button" id="vA-btn-open-training-bar" class="btn btn-primary btn-sm" style="background:linear-gradient(135deg, #f59e0b, #d97706); color:#0b0f17; font-weight:700;">
                    Configure & Train (${sectionsCount} Sections) →
                </button>
            </div>
        `;

        document.getElementById("vA-btn-select-all")?.addEventListener("click", () => {
            protoState.curatedMaps.forEach(m => protoState.selectedMapIds.add(m.id));
            renderLibraryCardCheckboxes();
            renderVariantASelectionBar();
        });

        document.getElementById("vA-btn-clear-selection")?.addEventListener("click", () => {
            protoState.selectedMapIds.clear();
            renderLibraryCardCheckboxes();
            renderVariantASelectionBar();
        });

        document.getElementById("vA-btn-open-training-bar")?.addEventListener("click", () => {
            toggleVariantADrawer("new-run");
        });
    }

    function renderLibraryCardCheckboxes() {
        const grid = document.getElementById("library-grid");
        if (!grid) return;

        grid.querySelectorAll(".curated-card").forEach(card => {
            const mapId = card.dataset.mapId || card.getAttribute("data-id");
            if (!mapId) return;

            let cb = card.querySelector(".proto-card-checkbox");
            if (!cb) {
                cb = document.createElement("input");
                cb.type = "checkbox";
                cb.className = "proto-card-checkbox";
                cb.style.cssText = "position:absolute; top:8px; left:8px; z-index:20; width:18px; height:18px; cursor:pointer;";
                card.style.position = "relative";
                card.appendChild(cb);
            }

            cb.checked = protoState.selectedMapIds.has(mapId);
            cb.onchange = (e) => {
                if (e.target.checked) protoState.selectedMapIds.add(mapId);
                else protoState.selectedMapIds.delete(mapId);
                if (protoState.variant === "A") renderVariantASelectionBar();
                else if (protoState.variant === "C") renderVariantCLaunchFab();
            };
        });
    }

    // --------------------------------------------------------------------------
    // VARIANT B: Dedicated Top-Level Training Workbench & Floating Progress
    // --------------------------------------------------------------------------
    function initVariantB() {
        // 1. Navigation Switcher 3rd Tab
        const nav = document.querySelector(".nav-switcher");
        if (nav && !document.getElementById("tab-nav-training-vB")) {
            const btn = document.createElement("button");
            btn.type = "button";
            btn.id = "tab-nav-training-vB";
            btn.className = "nav-switcher-btn";
            btn.title = "Training Workbench (Hotkey: W)";

            const activeRun = protoState.runs.find(r => r.state === "running");
            btn.innerHTML = `
                <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
                    <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
                </svg>
                <span>Training Workbench</span>
                ${activeRun ? '<span class="pulse-dot" style="display:inline-block; vertical-align:middle; margin-left:4px;"></span>' : ""}
            `;

            btn.addEventListener("click", () => switchViewToWorkbench());
            nav.appendChild(btn);
        }

        // 2. Inline Stage Banner in Dataset Library
        renderVariantBStageBanner();

        // 3. Floating progress window if active run
        renderVariantBFloatingWindow();
    }

    function switchViewToWorkbench() {
        // Toggle active nav tab
        document.querySelectorAll(".nav-switcher-btn").forEach(b => b.classList.remove("active"));
        document.getElementById("tab-nav-training-vB")?.classList.add("active");

        // Hide other viewports
        document.getElementById("triage-viewport")?.classList.add("hidden");
        document.getElementById("library-viewport")?.classList.add("hidden");

        // Mount or unhide Workbench viewport
        let view = document.getElementById("vB-workbench-view");
        if (!view) {
            view = document.createElement("section");
            view.id = "vB-workbench-view";
            view.className = "vB-workbench-view";
            document.getElementById("app")?.appendChild(view);
        }
        view.classList.remove("hidden");

        renderVariantBWorkbench(view);
    }

    function renderVariantBWorkbench(view) {
        const sectionsCount = getSelectedSectionsCount() || 20;
        const activeRun = protoState.runs.find(r => r.state === "running") || protoState.runs[0];
        const pct = activeRun ? Math.round((activeRun.steps / activeRun.totalSteps) * 100) : 0;

        view.innerHTML = `
            <!-- Column 1: Dataset Snapshot & 16x16 Section Inspector -->
            <div class="vB-col vB-col-dataset">
                <div class="vB-col-header">
                    <span class="vB-col-title">
                        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#38bdf8" stroke-width="2">
                            <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
                            <line x1="3" y1="9" x2="21" y2="9"/>
                            <line x1="9" y1="21" x2="9" y2="9"/>
                        </svg>
                        <span>Dataset Snapshot (${sectionsCount} Sections)</span>
                    </span>
                    <span class="badge badge-status-succeeded">Audit: 100% Valid</span>
                </div>
                <div class="vB-col-content">
                    <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:10px; font-size:12px;">
                        <div style="font-weight:700; color:#f8fafc;">Snapshot: snap_stone_dungeon_v1</div>
                        <div style="color:var(--text-muted); margin-top:2px;">Format: 1024×1024 PNGs · Uniform 64px = 1 Cell · Zero Letterbox</div>
                        <div style="color:#fbbf24; margin-top:4px;">Prefix: "dungeonforge battlemap, " applied once</div>
                    </div>

                    <div style="font-size:11.5px; font-weight:700; text-transform:uppercase; color:var(--text-muted);">
                        16×16 Grid Cell Crops Preview
                    </div>
                    <div class="vB-section-grid">
                        ${Array.from({ length: 12 }).map((_, i) => `
                            <div class="vB-section-tile">
                                <img src="${createSampleSvg(`Tile ${i+1}`, 1000, "#38bdf8")}" style="width:100%; height:100%; object-fit:cover;"/>
                                <span class="vB-section-badge">1024px</span>
                            </div>
                        `).join("")}
                    </div>

                    <button type="button" class="btn btn-secondary btn-sm" id="vB-btn-re-freeze">
                        Freeze Immutable Snapshot in R2
                    </button>
                </div>
            </div>

            <!-- Column 2: Hyperparameter Matrix & Provisioning -->
            <div class="vB-col vB-col-recipe">
                <div class="vB-col-header">
                    <span class="vB-col-title">
                        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#fbbf24" stroke-width="2">
                            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
                        </svg>
                        <span>Hyperparameter Matrix & Node Specs</span>
                    </span>
                    <span class="badge badge-source">Modal L40S</span>
                </div>
                <div class="vB-col-content">
                    <!-- Node Specs -->
                    <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:12px; font-size:12px;">
                        <div style="display:flex; justify-content:space-between;">
                            <span>Hardware Tier: <strong style="color:#f8fafc;">NVIDIA L40S (48GB)</strong></span>
                            <span>Rate: <strong style="color:#34d399;">$0.031 / min</strong></span>
                        </div>
                        <div style="margin-top:6px; color:var(--text-muted); font-size:11.5px;">
                            Training Engine: <code>ai-toolkit</code> via Modal FastAPI Orchestrator
                        </div>
                    </div>

                    <!-- Sliders & Inputs -->
                    <div style="display:flex; flex-direction:column; gap:12px;">
                        <div>
                            <div style="display:flex; justify-content:space-between; font-size:12px;">
                                <span>Training Steps</span>
                                <span class="mono" id="vB-val-steps" style="color:#fbbf24; font-weight:700;">1500 steps</span>
                            </div>
                            <input type="range" id="vB-slider-steps" min="500" max="3000" step="250" value="1500" style="width:100%; margin-top:4px;"/>
                        </div>

                        <div>
                            <div style="display:flex; justify-content:space-between; font-size:12px;">
                                <span>LoRA Rank & Alpha</span>
                                <span class="mono" style="color:#a5f3fc;">Rank 32 / Alpha 16</span>
                            </div>
                        </div>

                        <div>
                            <label style="font-size:12px; color:var(--text-secondary);">Optimizer</label>
                            <select class="library-select" style="width:100%; margin-top:4px;">
                                <option value="prodigy" selected>Prodigy (Adaptive lr=1.0)</option>
                                <option value="adamw">AdamW (lr=1e-4)</option>
                            </select>
                        </div>

                        <div>
                            <label style="font-size:12px; color:var(--text-secondary);">Base Model</label>
                            <select class="library-select" style="width:100%; margin-top:4px;">
                                <option value="black-forest-labs/FLUX.1-dev" selected>black-forest-labs/FLUX.1-dev (12B DiT)</option>
                            </select>
                        </div>
                    </div>

                    <!-- Cost Calculation Box -->
                    <div style="background:rgba(245,158,11,0.08); border:1px solid rgba(245,158,11,0.3); border-radius:8px; padding:12px; margin-top:auto;">
                        <div style="display:flex; justify-content:space-between; font-size:13px; font-weight:700;">
                            <span>Projected Run Cost</span>
                            <span class="mono" id="vB-val-cost" style="color:#fbbf24;">$1.25</span>
                        </div>
                        <div style="font-size:11px; color:var(--text-muted); margin-top:2px;">
                            Est. Wall Time: ~41 min · Break-even after ~35 generated maps
                        </div>
                        <button type="button" id="vB-btn-dispatch" class="btn btn-primary btn-block" style="margin-top:10px; background:#f59e0b; color:#0b0f17; font-weight:700;">
                            Dispatch Training Run to Modal
                        </button>
                    </div>
                </div>
            </div>

            <!-- Column 3: Active Node Telemetry & Adapters -->
            <div class="vB-col vB-col-telemetry">
                <div class="vB-col-header">
                    <span class="vB-col-title">
                        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#10b981" stroke-width="2">
                            <polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>
                        </svg>
                        <span>Node Telemetry & Adapters</span>
                    </span>
                    <span class="badge ${activeRun?.state === "running" ? "badge-status-running" : "badge-status-succeeded"}">
                        ${activeRun ? activeRun.state.toUpperCase() : "IDLE"}
                    </span>
                </div>
                <div class="vB-col-content">
                    ${activeRun ? `
                        <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:12px;">
                            <div style="display:flex; justify-content:space-between; font-size:12.5px;">
                                <span class="mono" style="font-weight:700;">${activeRun.id}</span>
                                <span class="mono" style="color:#fbbf24;">${activeRun.steps}/${activeRun.totalSteps} (${pct}%)</span>
                            </div>
                            <div class="progress-bar-track" style="height:6px; background:#1e293b; border-radius:3px; overflow:hidden; margin-top:8px;">
                                <div class="progress-bar-fill" style="width:${pct}%; height:100%; background:#10b981;"></div>
                            </div>
                            <div style="display:flex; justify-content:space-between; margin-top:8px; font-size:11.5px; color:var(--text-muted);">
                                <span>Spend: <strong class="mono" style="color:#f8fafc;">$${activeRun.cost.toFixed(2)}</strong></span>
                                <span>Loss: <strong class="mono" style="color:#38bdf8;">${activeRun.loss}</strong></span>
                                <span>VRAM: <strong class="mono" style="color:#f8fafc;">42.1 / 48 GB</strong></span>
                            </div>
                        </div>

                        <!-- Checkpoint Cards -->
                        <div>
                            <span style="font-size:12px; font-weight:700; color:#f8fafc;">Checkpoints</span>
                            <div class="proto-checkpoint-grid" style="margin-top:6px;">
                                ${activeRun.checkpoints.map(cp => `
                                    <div class="proto-checkpoint-card">
                                        <div class="proto-checkpoint-thumb">
                                            <img src="${cp.sampleUrl}" alt="Step ${cp.step}" style="width:100%; height:100%; object-fit:cover;"/>
                                        </div>
                                        <div class="proto-checkpoint-footer">
                                            <span class="mono" style="font-size:10.5px; font-weight:700;">s${cp.step}</span>
                                            ${cp.published ?
                                                `<span class="badge badge-status-succeeded" style="font-size:9px;">Published</span>` :
                                                `<button type="button" class="btn btn-secondary btn-sm vB-btn-publish" data-run="${activeRun.id}" data-step="${cp.step}" style="font-size:9.5px; padding:2px 4px;">Publish</button>`
                                            }
                                        </div>
                                    </div>
                                `).join("")}
                            </div>
                        </div>

                        <!-- Terminal Log -->
                        <div class="proto-log-terminal" style="max-height:140px;">
                            ${activeRun.logs.map(l => `<div class="proto-log-line">${l}</div>`).join("")}
                        </div>
                    ` : `
                        <div class="empty-card" style="padding:20px; text-align:center;">No training run running.</div>
                    `}

                    <!-- Published Adapters Table -->
                    <div style="margin-top:8px;">
                        <span style="font-size:12px; font-weight:700; color:#f8fafc;">Catalog Adapters (${protoState.adapters.length})</span>
                        <div style="display:flex; flex-direction:column; gap:6px; margin-top:6px;">
                            ${protoState.adapters.map(ad => `
                                <div style="display:flex; justify-content:space-between; align-items:center; background:rgba(255,255,255,0.02); border:1px solid var(--border-subtle); border-radius:6px; padding:6px 10px; font-size:11.5px;">
                                    <div>
                                        <span class="mono" style="font-weight:700; color:#fbbf24;">${ad.id}</span>
                                        <span style="color:var(--text-muted); margin-left:6px;">${ad.baseModel} · s${ad.step}</span>
                                    </div>
                                    <button type="button" class="btn btn-ghost btn-sm vB-btn-retire" data-id="${ad.id}" style="font-size:10.5px;">
                                        ${ad.retired ? "Restore" : "Retire"}
                                    </button>
                                </div>
                            `).join("")}
                        </div>
                    </div>
                </div>
            </div>
        `;

        // Bind events in Workbench
        document.getElementById("vB-slider-steps")?.addEventListener("input", (e) => {
            const steps = parseInt(e.target.value, 10);
            document.getElementById("vB-val-steps").innerText = `${steps} steps`;
            const cost = (steps / 1500 * 1.25).toFixed(2);
            document.getElementById("vB-val-cost").innerText = `$${cost}`;
        });

        document.getElementById("vB-btn-dispatch")?.addEventListener("click", () => {
            const steps = parseInt(document.getElementById("vB-slider-steps").value, 10);
            dispatchNewSimRun({ steps });
            renderVariantBWorkbench(view);
        });

        view.querySelectorAll(".vB-btn-publish").forEach(btn => {
            btn.addEventListener("click", () => {
                publishAdapterFromCheckpoint(btn.dataset.run, parseInt(btn.dataset.step, 10));
                renderVariantBWorkbench(view);
            });
        });

        view.querySelectorAll(".vB-btn-retire").forEach(btn => {
            btn.addEventListener("click", () => {
                toggleRetireAdapter(btn.dataset.id);
                renderVariantBWorkbench(view);
            });
        });
    }

    function renderVariantBStageBanner() {
        const toolbar = document.querySelector(".library-toolbar");
        if (!toolbar || document.getElementById("vB-library-stage-banner")) return;

        const banner = document.createElement("div");
        banner.id = "vB-library-stage-banner";
        banner.style.cssText = "display:flex; align-items:center; justify-content:space-between; width:100%; padding:8px 16px; background:rgba(245,158,11,0.08); border-bottom:1px solid rgba(245,158,11,0.25); font-size:12.5px;";

        const sectionsCount = getSelectedSectionsCount() || 20;
        banner.innerHTML = `
            <div>
                <span style="font-weight:700; color:#fbbf24;">⚡ LoRA Training Scope:</span>
                <span style="color:#f8fafc; margin-left:6px;">${protoState.selectedMapIds.size} Maps · ${sectionsCount} Map Sections</span>
                <span class="badge badge-status-succeeded" style="margin-left:8px;">Ready for FLUX.1-dev</span>
            </div>
            <button type="button" id="vB-btn-stage-now" class="btn btn-secondary btn-sm" style="border-color:var(--accent-amber); color:#fbbf24;">
                Open in Training Workbench →
            </button>
        `;

        toolbar.after(banner);
        document.getElementById("vB-btn-stage-now")?.addEventListener("click", () => switchViewToWorkbench());
    }

    function renderVariantBFloatingWindow() {
        let win = document.getElementById("vB-floating-window");
        const activeRun = protoState.runs.find(r => r.state === "running");

        if (!activeRun) {
            win?.remove();
            return;
        }

        if (!win) {
            win = document.createElement("div");
            win.id = "vB-floating-window";
            win.className = "vB-floating-window";
            document.body.appendChild(win);
        }

        const pct = Math.round((activeRun.steps / activeRun.totalSteps) * 100);

        win.innerHTML = `
            <div class="vB-floating-header">
                <div style="display:flex; align-items:center; gap:6px;">
                    <span class="pulse-dot"></span>
                    <span style="font-size:12px; font-weight:700; color:#fbbf24;">Modal Training: ${activeRun.id}</span>
                </div>
                <button type="button" class="btn-close" id="vB-btn-close-floating">×</button>
            </div>
            <div style="padding:10px 14px; font-size:12px;">
                <div style="display:flex; justify-content:space-between; margin-bottom:4px;">
                    <span>Step ${activeRun.steps} of ${activeRun.totalSteps}</span>
                    <span class="mono" style="color:#10b981;">${pct}%</span>
                </div>
                <div class="progress-bar-track" style="height:5px; background:#1e293b; border-radius:2px; overflow:hidden;">
                    <div class="progress-bar-fill" style="width:${pct}%; height:100%; background:#f59e0b;"></div>
                </div>
                <div style="display:flex; justify-content:space-between; margin-top:8px; font-size:11px; color:var(--text-muted);">
                    <span>Spend: $${activeRun.cost.toFixed(2)}</span>
                    <button type="button" id="vB-btn-open-from-float" class="btn-link" style="color:#38bdf8;">View Workbench →</button>
                </div>
            </div>
        `;

        document.getElementById("vB-btn-close-floating")?.addEventListener("click", () => win.remove());
        document.getElementById("vB-btn-open-from-float")?.addEventListener("click", () => switchViewToWorkbench());
    }

    // --------------------------------------------------------------------------
    // VARIANT C: Modal-Centric Wizard & Top-Center Dynamic Island HUD
    // --------------------------------------------------------------------------
    function initVariantC() {
        // 1. Dynamic Island at Top Center
        renderVariantCDynamicIsland();

        // 2. Floating Action Button in Dataset Library
        renderVariantCLaunchFab();

        // 3. Checkboxes in Library
        renderLibraryCardCheckboxes();
    }

    function renderVariantCDynamicIsland() {
        let island = document.getElementById("vC-dynamic-island");
        const activeRun = protoState.runs.find(r => r.state === "running");

        if (!island) {
            island = document.createElement("div");
            island.id = "vC-dynamic-island";
            island.className = "vC-dynamic-island";
            document.body.appendChild(island);
        }

        if (activeRun) {
            const pct = Math.round((activeRun.steps / activeRun.totalSteps) * 100);
            island.className = "vC-dynamic-island";
            island.innerHTML = `
                <div style="display:flex; align-items:center; gap:6px;">
                    <span class="pulse-dot"></span>
                    <span style="font-weight:700; color:#fbbf24; font-size:11.5px;">FLUX LoRA</span>
                </div>
                <span style="color:rgba(255,255,255,0.2);">|</span>
                <span class="mono" style="font-size:11.5px; color:#f8fafc;">${activeRun.steps}/${activeRun.totalSteps}</span>
                <span class="badge badge-source" style="font-size:10px;">${pct}%</span>
                <span class="mono" style="font-size:11px; color:#34d399;">$${activeRun.cost.toFixed(2)}</span>
            `;
            island.onclick = () => toggleVariantCIslandDropdown(activeRun);
        } else {
            const lastRun = protoState.runs[0];
            island.className = "vC-dynamic-island completed";
            island.innerHTML = `
                <span style="color:#10b981;">✓</span>
                <span style="font-size:11.5px; font-weight:600; color:#f8fafc;">Training Idle</span>
                <span class="badge badge-lora" style="font-size:9.5px;">${protoState.adapters.length} Adapters</span>
            `;
            island.onclick = () => toggleVariantCIslandDropdown(lastRun);
        }
    }

    function toggleVariantCIslandDropdown(run) {
        let dd = document.getElementById("vC-island-dropdown");
        if (dd) {
            dd.remove();
            return;
        }

        dd = document.createElement("div");
        dd.id = "vC-island-dropdown";
        dd.className = "vC-island-dropdown";
        document.body.appendChild(dd);

        if (!run) {
            dd.innerHTML = `<div>No recent runs. Launch training from the Library view!</div>`;
            return;
        }

        const pct = Math.round((run.steps / run.totalSteps) * 100);

        dd.innerHTML = `
            <div style="display:flex; justify-content:space-between; align-items:center;">
                <span class="mono" style="font-weight:700; color:#f8fafc;">${run.id}</span>
                <span class="badge ${run.state === "running" ? "badge-status-running" : "badge-status-succeeded"}">
                    ${run.state.toUpperCase()}
                </span>
            </div>
            <div style="margin-top:4px;">
                <div style="display:flex; justify-content:space-between; font-size:11px; margin-bottom:2px;">
                    <span>Step ${run.steps} of ${run.totalSteps}</span>
                    <span class="mono">${pct}%</span>
                </div>
                <div class="progress-bar-track" style="height:4px; background:#1e293b; border-radius:2px; overflow:hidden;">
                    <div class="progress-bar-fill" style="width:${pct}%; height:100%; background:#f59e0b;"></div>
                </div>
            </div>
            <div class="proto-log-terminal" style="max-height:90px; font-size:10px;">
                ${run.logs.slice(-3).map(l => `<div class="proto-log-line">${l}</div>`).join("")}
            </div>
            <div style="display:flex; justify-content:space-between; align-items:center; margin-top:6px;">
                <span style="font-size:11.5px; color:var(--text-muted);">Spend: $${run.cost.toFixed(2)}</span>
                <div style="display:flex; gap:6px;">
                    <button type="button" class="btn btn-secondary btn-sm" id="vC-btn-eval-checkpoints" style="font-size:11px;">
                        Evaluate Checkpoints (${run.checkpoints.length})
                    </button>
                    <button type="button" class="btn btn-ghost btn-sm" id="vC-btn-close-dd" style="font-size:11px;">Close</button>
                </div>
            </div>
        `;

        document.getElementById("vC-btn-close-dd")?.addEventListener("click", () => dd.remove());
        document.getElementById("vC-btn-eval-checkpoints")?.addEventListener("click", () => {
            dd.remove();
            openVariantCEvaluationModal(run);
        });
    }

    function openVariantCEvaluationModal(run) {
        let modal = document.getElementById("vC-eval-modal");
        if (modal) modal.remove();

        modal = document.createElement("div");
        modal.id = "vC-eval-modal";
        modal.className = "proto-inspector-modal";
        document.body.appendChild(modal);

        modal.innerHTML = `
            <div class="proto-inspector-header">
                <div class="proto-inspector-title">
                    <span>Evaluate & Publish Checkpoints: ${run.id}</span>
                </div>
                <button type="button" class="btn-close" id="vC-btn-close-eval">×</button>
            </div>
            <div class="proto-inspector-body">
                <p style="color:var(--text-secondary); font-size:12px;">
                    Inspect orthographic sample grids generated at training checkpoint intervals. Choose a checkpoint to publish as an active LoRA Adapter.
                </p>
                <div class="proto-checkpoint-grid" style="grid-template-columns:repeat(3, 1fr);">
                    ${run.checkpoints.map(cp => `
                        <div class="proto-checkpoint-card">
                            <div class="proto-checkpoint-thumb">
                                <img src="${cp.sampleUrl}" alt="Step ${cp.step}" style="width:100%; height:100%; object-fit:cover;"/>
                            </div>
                            <div class="proto-checkpoint-footer">
                                <span class="mono" style="font-weight:700;">Step ${cp.step}</span>
                                <span style="font-size:10px; color:var(--text-muted);">Loss: ${cp.loss}</span>
                                ${cp.published ?
                                    `<span class="badge badge-status-succeeded" style="text-align:center;">Published ✓</span>` :
                                    `<button type="button" class="btn btn-primary btn-sm vC-btn-publish-cp" data-step="${cp.step}" style="margin-top:4px; font-size:10px; background:#f59e0b; color:#0b0f17; font-weight:700;">
                                        Publish Adapter
                                    </button>`
                                }
                            </div>
                        </div>
                    `).join("")}
                </div>
            </div>
        `;

        document.getElementById("vC-btn-close-eval")?.addEventListener("click", () => modal.remove());
        modal.querySelectorAll(".vC-btn-publish-cp").forEach(btn => {
            btn.addEventListener("click", () => {
                publishAdapterFromCheckpoint(run.id, parseInt(btn.dataset.step, 10));
                modal.remove();
            });
        });
    }

    function renderVariantCLaunchFab() {
        let fab = document.getElementById("vC-launch-fab");
        const libraryViewport = document.getElementById("library-viewport");
        if (!libraryViewport) return;

        if (!fab) {
            fab = document.createElement("button");
            fab.id = "vC-launch-fab";
            fab.className = "vC-launch-fab";
            document.body.appendChild(fab);
        }

        const sectionsCount = getSelectedSectionsCount() || 20;
        fab.innerHTML = `
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.5">
                <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
            </svg>
            <span>Launch Training (${sectionsCount} Sections)</span>
        `;

        fab.onclick = () => openVariantCWizardModal();
    }

    function openVariantCWizardModal() {
        let modal = document.getElementById("vC-wizard-modal");
        if (modal) modal.remove();

        modal = document.createElement("div");
        modal.id = "vC-wizard-modal";
        modal.className = "vC-wizard-modal";
        document.body.appendChild(modal);

        let step = 1;
        const sectionsCount = getSelectedSectionsCount() || 20;

        function renderWizardStep() {
            modal.innerHTML = `
                <div class="vC-wizard-stepper">
                    <div class="vC-wizard-step ${step === 1 ? "active" : (step > 1 ? "completed" : "")}">
                        <span class="vC-step-number">1</span>
                        <span>Scope & Audit</span>
                    </div>
                    <div class="vC-wizard-step ${step === 2 ? "active" : (step > 2 ? "completed" : "")}">
                        <span class="vC-step-number">2</span>
                        <span>Recipe & Intensity</span>
                    </div>
                    <div class="vC-wizard-step ${step === 3 ? "active" : ""}">
                        <span class="vC-step-number">3</span>
                        <span>Review & Dispatch</span>
                    </div>
                    <button type="button" class="btn-close" id="vC-btn-close-wizard" style="margin-left:auto;">×</button>
                </div>

                <div style="padding:20px; overflow-y:auto; display:flex; flex-direction:column; gap:16px;">
                    ${step === 1 ? `
                        <div>
                            <h4 style="font-size:14px; font-weight:700; color:#f8fafc;">Step 1: Dataset Scope Audit</h4>
                            <p style="font-size:12px; color:var(--text-muted); margin-top:2px;">
                                Auditing selected curated maps against the 16×16 section contract (1024×1024 crops with perspective-neutral descriptions).
                            </p>
                        </div>
                        <div style="display:flex; gap:10px;">
                            <div style="flex:1; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:10px;">
                                <div style="font-size:11px; color:var(--text-muted);">Curated Maps</div>
                                <div style="font-size:18px; font-weight:700; color:#f8fafc;">${protoState.selectedMapIds.size}</div>
                            </div>
                            <div style="flex:1; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:10px;">
                                <div style="font-size:11px; color:var(--text-muted);">16×16 Sections</div>
                                <div style="font-size:18px; font-weight:700; color:#fbbf24;">${sectionsCount}</div>
                            </div>
                            <div style="flex:1; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:10px;">
                                <div style="font-size:11px; color:var(--text-muted);">Audit Status</div>
                                <div style="font-size:14px; font-weight:700; color:#34d399; margin-top:4px;">✓ 100% Passed</div>
                            </div>
                        </div>
                        <div style="display:flex; justify-content:flex-end; margin-top:10px;">
                            <button type="button" class="btn btn-primary" id="vC-btn-step1-next">Continue to Recipe →</button>
                        </div>
                    ` : step === 2 ? `
                        <div>
                            <h4 style="font-size:14px; font-weight:700; color:#f8fafc;">Step 2: Training Intensity & Recipe</h4>
                            <p style="font-size:12px; color:var(--text-muted); margin-top:2px;">
                                Select compute intensity preset. Modal cloud GPU runner bills per-second on L40S hardware.
                            </p>
                        </div>
                        <div style="display:flex; flex-direction:column; gap:8px;">
                            <label style="display:flex; align-items:center; gap:12px; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:12px; cursor:pointer;">
                                <input type="radio" name="vC-intensity" value="500" data-cost="0.42"/>
                                <div>
                                    <div style="font-weight:700; color:#f8fafc;">Quick Experiment (500 steps)</div>
                                    <div style="font-size:11px; color:var(--text-muted);">~14 mins wall time · Est. Cost: $0.42</div>
                                </div>
                            </label>
                            <label style="display:flex; align-items:center; gap:12px; background:rgba(245,158,11,0.12); border:1px solid var(--accent-amber); border-radius:8px; padding:12px; cursor:pointer;">
                                <input type="radio" name="vC-intensity" value="1500" data-cost="1.25" checked/>
                                <div>
                                    <div style="font-weight:700; color:#fbbf24;">Balanced Baseline (1500 steps) — Recommended</div>
                                    <div style="font-size:11px; color:var(--text-muted);">~41 mins wall time · Est. Cost: $1.25 · Optimal Poled masonry & poché walls</div>
                                </div>
                            </label>
                            <label style="display:flex; align-items:center; gap:12px; background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:12px; cursor:pointer;">
                                <input type="radio" name="vC-intensity" value="3000" data-cost="2.50"/>
                                <div>
                                    <div style="font-weight:700; color:#f8fafc;">High Precision (3000 steps)</div>
                                    <div style="font-size:11px; color:var(--text-muted);">~82 mins wall time · Est. Cost: $2.50</div>
                                </div>
                            </label>
                        </div>
                        <div style="display:flex; justify-content:space-between; margin-top:10px;">
                            <button type="button" class="btn btn-secondary" id="vC-btn-step2-prev">← Back</button>
                            <button type="button" class="btn btn-primary" id="vC-btn-step2-next">Review & Launch →</button>
                        </div>
                    ` : `
                        <div>
                            <h4 style="font-size:14px; font-weight:700; color:#f8fafc;">Step 3: Review & Dispatch</h4>
                            <p style="font-size:12px; color:var(--text-muted); margin-top:2px;">
                                Confirm parameters before freezing dataset snapshot in R2 and initiating Modal GPU provisioning.
                            </p>
                        </div>
                        <div style="background:rgba(255,255,255,0.03); border:1px solid var(--border-subtle); border-radius:8px; padding:12px; font-size:12px; display:flex; flex-direction:column; gap:6px;">
                            <div>Base Model: <strong style="color:#f8fafc;">black-forest-labs/FLUX.1-dev</strong></div>
                            <div>Dataset Size: <strong style="color:#fbbf24;">${sectionsCount} sections (1024×1024)</strong></div>
                            <div>Trigger Phrase: <code class="mono" style="color:#38bdf8;">dungeonforge battlemap</code></div>
                            <div>Total Steps: <strong style="color:#f8fafc;">1500 steps</strong></div>
                            <div>Projected Spend: <strong style="color:#34d399;">$1.25</strong></div>
                        </div>
                        <div style="display:flex; justify-content:space-between; margin-top:10px;">
                            <button type="button" class="btn btn-secondary" id="vC-btn-step3-prev">← Back</button>
                            <button type="button" class="btn btn-primary" id="vC-btn-step3-dispatch" style="background:#f59e0b; color:#0b0f17; font-weight:700;">
                                ⚡ Confirm & Dispatch Run
                            </button>
                        </div>
                    `}
                </div>
            `;

            document.getElementById("vC-btn-close-wizard")?.addEventListener("click", () => modal.remove());
            document.getElementById("vC-btn-step1-next")?.addEventListener("click", () => { step = 2; renderWizardStep(); });
            document.getElementById("vC-btn-step2-prev")?.addEventListener("click", () => { step = 1; renderWizardStep(); });
            document.getElementById("vC-btn-step2-next")?.addEventListener("click", () => { step = 3; renderWizardStep(); });
            document.getElementById("vC-btn-step3-prev")?.addEventListener("click", () => { step = 2; renderWizardStep(); });
            document.getElementById("vC-btn-step3-dispatch")?.addEventListener("click", () => {
                dispatchNewSimRun({ steps: 1500 });
                modal.remove();
                renderVariantCDynamicIsland();
            });
        }

        renderWizardStep();
    }

    // --------------------------------------------------------------------------
    // Benchmark Drawer Integration (Question 3 of Issue #189)
    // --------------------------------------------------------------------------
    function renderBenchmarkDrawerModels() {
        const list = document.getElementById("benchmark-models-list");
        if (!list) return;

        // Keep standard baseline OpenRouter models
        const baselines = [
            { id: "google/gemini-3.1-flash-image", name: "Google Gemini 3.1 Flash Image", price: "$0.070 / image", desc: "Default high-fidelity battlemap model", checked: true },
            { id: "google/gemini-3.1-flash-lite-image", name: "Google Gemini 3.1 Flash Lite", price: "$0.035 / image", desc: "Low-cost high-throughput generation", checked: true },
            { id: "meta/muse-image", name: "Meta Muse Image", price: "$0.020 / image", desc: "Fast baseline generation", checked: false },
            { id: "bytedance-seed/seedream-5-0-flash", name: "ByteDance Seedream 5.0 Flash", price: "$0.030 / image", desc: "High-resolution architectural balance", checked: false }
        ];

        // Active published LoRA adapters
        const activeAdapters = protoState.adapters.filter(a => !a.retired);

        if (protoState.variant === "A") {
            // Variant A: Explicit Grouped Sections
            list.innerHTML = `
                <div class="proto-benchmark-group-title">
                    <span>Pretrained Baselines (OpenRouter)</span>
                    <span class="badge badge-source">${baselines.length} models</span>
                </div>
                ${baselines.map(m => createBaselineCheckCard(m)).join("")}

                <div class="proto-benchmark-group-title" style="margin-top:12px;">
                    <span>Trained LoRA Adapters (WaveSpeed / Modal)</span>
                    <span class="badge badge-lora">${activeAdapters.length} adapters</span>
                </div>
                ${activeAdapters.length ? activeAdapters.map(ad => `
                    <div style="background:rgba(245,158,11,0.04); border:1px solid rgba(245,158,11,0.3); border-radius:8px; padding:10px; margin-bottom:8px;">
                        <div style="display:flex; justify-content:space-between; align-items:center;">
                            <span class="mono" style="font-weight:700; color:#fbbf24; font-size:12.5px;">${ad.id}</span>
                            <span class="badge badge-lora">FLUX.1-dev</span>
                        </div>
                        <div style="display:flex; flex-direction:column; gap:4px; margin-top:8px;">
                            ${ad.engines.map(eng => `
                                <label class="check-item-card" style="margin:0; padding:6px 8px;">
                                    <input type="checkbox" name="benchmark-model" value="${eng.id}" checked>
                                    <div class="check-item-info">
                                        <div class="check-item-title-row">
                                            <span class="check-item-name" style="font-size:12px;">${eng.name}</span>
                                            <span class="check-item-price mono" style="font-size:11px;">$${eng.pricePerImage}/map</span>
                                        </div>
                                        <span class="check-item-desc" style="font-size:10.5px;">
                                            ${eng.supportsReferenceImage ? "✓ Supports Reference Conditioning (Edit Engine)" : "Text-to-Image only (no reference)"}
                                        </span>
                                    </div>
                                </label>
                            `).join("")}
                        </div>
                    </div>
                `).join("") : `<div style="font-size:11.5px; color:var(--text-muted); padding:6px;">No published adapters in catalog yet.</div>`}
            `;

        } else if (protoState.variant === "B") {
            // Variant B: Flat Unified List with Filter Pills & Inline Engine Mode Toggle
            list.innerHTML = `
                <div style="display:flex; gap:6px; margin-bottom:10px;">
                    <button type="button" class="lib-facet-chip active" id="vB-bench-filter-all">All (${baselines.length + activeAdapters.length})</button>
                    <button type="button" class="lib-facet-chip" id="vB-bench-filter-base">Base Models (${baselines.length})</button>
                    <button type="button" class="lib-facet-chip" id="vB-bench-filter-lora">LoRA (${activeAdapters.length})</button>
                </div>

                ${baselines.map(m => createBaselineCheckCard(m)).join("")}

                ${activeAdapters.map(ad => `
                    <label class="check-item-card proto-lora-model-card">
                        <input type="checkbox" name="benchmark-model" value="${ad.engines[0].id}" checked>
                        <div class="check-item-info">
                            <div class="check-item-title-row">
                                <div style="display:flex; align-items:center; gap:6px;">
                                    <span class="badge badge-lora">LoRA</span>
                                    <span class="check-item-name">${ad.id}</span>
                                </div>
                                <span class="check-item-price mono">$0.015 - $0.021</span>
                            </div>
                            <div class="proto-lora-engine-toggle">
                                <span class="proto-engine-option active" data-engine="t2i">T2I ($0.015)</span>
                                <span class="proto-engine-option" data-engine="edit">Edit / Reference ($0.021)</span>
                            </div>
                            <div style="font-size:10.5px; color:var(--text-muted); margin-top:4px;">
                                Base: ${ad.baseModel} · s${ad.step} · Scale: 1.0
                            </div>
                        </div>
                    </label>
                `).join("")}
            `;

        } else if (protoState.variant === "C") {
            // Variant C: Segmented Tabs with Interactive LoRA Scale Slider & 1-Click Comparative Preset
            list.innerHTML = `
                <div class="vA-drawer-tabs" style="margin:0 0 10px 0;">
                    <button type="button" class="vA-drawer-tab-btn active" id="vC-bench-tab-std">Standard Baselines (${baselines.length})</button>
                    <button type="button" class="vA-drawer-tab-btn" id="vC-bench-tab-lora">Custom LoRA Adapters (${activeAdapters.length})</button>
                </div>

                <div id="vC-bench-panel-std">
                    ${baselines.map(m => createBaselineCheckCard(m)).join("")}
                </div>

                <div id="vC-bench-panel-lora" class="hidden">
                    <!-- 1-Click Comparison Action -->
                    <button type="button" id="vC-btn-compare-gemini" class="btn btn-secondary btn-block btn-sm" style="margin-bottom:10px; border-color:var(--accent-amber); color:#fbbf24;">
                        ⚡ 1-Click Pair: Compare Top LoRA vs Gemini 3.1 Flash
                    </button>

                    ${activeAdapters.map(ad => `
                        <div style="background:rgba(245,158,11,0.04); border:1px solid rgba(245,158,11,0.3); border-radius:8px; padding:10px; margin-bottom:8px;">
                            <label style="display:flex; align-items:center; gap:8px; cursor:pointer;">
                                <input type="checkbox" name="benchmark-model" value="${ad.id}" checked>
                                <span style="font-weight:700; color:#fbbf24; font-size:12.5px;">${ad.id}</span>
                            </label>
                            <div class="proto-lora-scale-slider-container">
                                <span>LoRA Scale:</span>
                                <input type="range" class="vC-scale-slider" min="0.2" max="1.5" step="0.05" value="1.0" style="flex:1;"/>
                                <span class="mono vC-scale-val" style="color:#a5f3fc; font-weight:700;">1.00</span>
                            </div>
                            <div style="display:flex; gap:12px; margin-top:8px; font-size:11px;">
                                <label style="display:flex; align-items:center; gap:4px; cursor:pointer;">
                                    <input type="radio" name="vC_engine_${ad.id}" value="t2i" checked/> Text-to-Image ($0.015)
                                </label>
                                <label style="display:flex; align-items:center; gap:4px; cursor:pointer;">
                                    <input type="radio" name="vC_engine_${ad.id}" value="edit"/> Edit / Ref ($0.021)
                                </label>
                            </div>
                        </div>
                    `).join("")}
                </div>
            `;

            const tabStd = document.getElementById("vC-bench-tab-std");
            const tabLora = document.getElementById("vC-bench-tab-lora");
            const panelStd = document.getElementById("vC-bench-panel-std");
            const panelLora = document.getElementById("vC-bench-panel-lora");

            tabStd?.addEventListener("click", () => {
                tabStd.classList.add("active");
                tabLora.classList.remove("active");
                panelStd.classList.remove("hidden");
                panelLora.classList.add("hidden");
            });

            tabLora?.addEventListener("click", () => {
                tabLora.classList.add("active");
                tabStd.classList.remove("active");
                panelLora.classList.remove("hidden");
                panelStd.classList.add("hidden");
            });

            document.getElementById("vC-btn-compare-gemini")?.addEventListener("click", () => {
                showProtoToast("Configured benchmark pair: Gemini 3.1 Flash Image + DungeonForge FLUX LoRA (s1500)", "success");
            });

            list.querySelectorAll(".vC-scale-slider").forEach(slider => {
                slider.addEventListener("input", (e) => {
                    slider.parentElement.querySelector(".vC-scale-val").innerText = parseFloat(e.target.value).toFixed(2);
                });
            });
        }
    }

    function createBaselineCheckCard(m) {
        return `
            <label class="check-item-card">
                <input type="checkbox" name="benchmark-model" value="${m.id}" ${m.checked ? "checked" : ""}>
                <div class="check-item-info">
                    <div class="check-item-title-row">
                        <span class="check-item-name">${m.name}</span>
                        <span class="check-item-price mono">${m.price}</span>
                    </div>
                    <span class="check-item-desc">${m.desc}</span>
                </div>
            </label>
        `;
    }

    function updateActiveRunViews() {
        const activeRun = protoState.runs.find(r => r.state === "running");

        // Variant A update
        if (protoState.variant === "A") {
            const badge = document.getElementById("vA-hud-run-badge");
            if (badge) {
                badge.innerText = activeRun ? `${Math.round(activeRun.steps / activeRun.totalSteps * 100)}%` : protoState.runs.length;
            }
            if (document.getElementById("vA-drawer")) {
                const activeTabBtn = document.querySelector(".vA-drawer-tab-btn.active");
                if (activeTabBtn) renderVariantATab(activeTabBtn.dataset.tab);
            }
            renderVariantASelectionBar();
        }

        // Variant B update
        if (protoState.variant === "B") {
            const wb = document.getElementById("vB-workbench-view");
            if (wb && !wb.classList.contains("hidden")) {
                renderVariantBWorkbench(wb);
            }
            renderVariantBFloatingWindow();
        }

        // Variant C update
        if (protoState.variant === "C") {
            renderVariantCDynamicIsland();
            renderVariantCLaunchFab();
        }
    }

    // --------------------------------------------------------------------------
    // Keyboard Navigation for Prototype Switcher (Left / Right Arrows)
    // --------------------------------------------------------------------------
    function handlePrototypeKeydown(e) {
        // Do not intercept if user is typing in form controls
        const tag = e.target?.tagName?.toLowerCase();
        if (tag === "input" || tag === "textarea" || tag === "select" || e.target?.isContentEditable) {
            return;
        }

        if (e.key === "ArrowLeft") {
            cycleVariant(-1);
            e.preventDefault();
        } else if (e.key === "ArrowRight") {
            cycleVariant(1);
            e.preventDefault();
        } else if (e.key === "r" || e.key === "R") {
            if (protoState.variant === "A") {
                toggleVariantADrawer();
                e.preventDefault();
            }
        } else if (e.key === "w" || e.key === "W") {
            if (protoState.variant === "B") {
                switchViewToWorkbench();
                e.preventDefault();
            }
        }
    }

    // --------------------------------------------------------------------------
    // Bootstrap & Initialization
    // --------------------------------------------------------------------------
    function init() {
        // Read URL param ?variant=
        const urlParams = new URLSearchParams(window.location.search);
        const variantParam = urlParams.get("variant")?.toUpperCase();
        if (variantParam && VARIANTS.find(v => v.key === variantParam)) {
            protoState.variant = variantParam;
        }

        // Mount switcher bar
        renderPrototypeSwitcher();

        // Mount active variant
        if (protoState.variant === "A") initVariantA();
        else if (protoState.variant === "B") initVariantB();
        else if (protoState.variant === "C") initVariantC();

        // Mount Benchmark Drawer integration
        renderBenchmarkDrawerModels();

        // Start live ticker
        startSimulationTicker();

        // Listen for keyboard arrow shortcuts
        window.addEventListener("keydown", handlePrototypeKeydown);

        // When switching tabs to Library, ensure checkboxes/selection bars update
        const tabNavLibrary = document.getElementById("tab-nav-library");
        tabNavLibrary?.addEventListener("click", () => {
            setTimeout(() => {
                if (protoState.variant === "A") {
                    renderLibraryCardCheckboxes();
                    renderVariantASelectionBar();
                } else if (protoState.variant === "B") {
                    renderVariantBStageBanner();
                } else if (protoState.variant === "C") {
                    renderLibraryCardCheckboxes();
                    renderVariantCLaunchFab();
                }
            }, 300);
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
