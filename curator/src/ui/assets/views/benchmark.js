/**
 * AI benchmark suite: registry-backed model selection, preflight cost
 * calculation, the budget guard, and the floating progress window with live
 * logs, previews and cancellation.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { closeHelpModal } from "../shell/help.js";
import { fetchStatus, showEmptyState, showLoading, showToast, updateHud } from "../shell/status.js";
import { fitToScreen, stopPanLoop, stopZoomLoop } from "../stage/canvas.js";
import { closeIngestionDrawer } from "./ingestion.js";
import { fetchStagingQueue, loadCandidate } from "./triage.js";

// Guardrail for the one-click run: above this estimate the budget modal confirms first.
const BUDGET_GUARDRAIL_THRESHOLD = 2.00;

// Selectable models served by GET /api/models (ADR-0010); never hardcoded.
let benchmarkModelCatalog = [];

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

        if (dom.costMonitorText) {
            dom.costMonitorText.textContent = `Session: $${sessionSpend} · Total: $${allTimeSpend} (Train: $${trainingSpend} · ${totalGens} ${mapWord})`;
        }
        if (dom.costMonitorHud) {
            dom.costMonitorHud.title = `Session Spend: $${sessionSpend} | Lifetime Spend: $${allTimeSpend} (Training: $${trainingSpend} across ${trainingRunsCount} ${runWord}, Gen: $${generationSpend} across ${totalGens} ${mapWord})`;
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
    if (!dom.benchmarkModelsList) return;
    dom.benchmarkModelsList.innerHTML = "";

    if (benchmarkModelCatalog.length === 0) {
        const empty = document.createElement("div");
        empty.className = "check-item-desc mono";
        empty.textContent = "No selectable models reported by the registry.";
        dom.benchmarkModelsList.appendChild(empty);
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
        dom.benchmarkModelsList.appendChild(label);
    }
}

function referenceConditioningRequested() {
    return dom.benchmarkEnableReference ? dom.benchmarkEnableReference.checked : true;
}

function calculatePreflightMetrics() {
    const models = getSelectedBenchmarkModels();
    const archetypes = getSelectedBenchmarkArchetypes();
    const hasCustom = dom.benchmarkEnableCustom?.checked && (dom.benchmarkCustomPrompt?.value.trim().length > 0 || dom.benchmarkCustomName?.value.trim().length > 0);
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
    if (dom.calcBadgeCost) dom.calcBadgeCost.textContent = costStr;

    if (dom.calcFormulaDisplay) {
        dom.calcFormulaDisplay.textContent = `${modelCount} model${modelCount === 1 ? "" : "s"} × ${promptCount} prompt${promptCount === 1 ? "" : "s"} = ${totalImages} image${totalImages === 1 ? "" : "s"} (~${costStr})`;
    }

    const exceedsGuardrail = estimatedCost > BUDGET_GUARDRAIL_THRESHOLD;
    if (dom.calcWarningNote) {
        dom.calcWarningNote.classList.toggle("hidden", !exceedsGuardrail);
    }

    const referenceBlocked = referenceIncapableModels.length > 0;
    if (dom.benchmarkReferenceWarning) {
        if (referenceBlocked) {
            dom.benchmarkReferenceWarning.textContent = `⚠️ Reference conditioning is unavailable for: ${referenceIncapableModels.join(", ")}. Turn it off or deselect those models.`;
        }
        dom.benchmarkReferenceWarning.classList.toggle("hidden", !referenceBlocked);
    }

    if (dom.btnStartBenchmarkText) {
        dom.btnStartBenchmarkText.textContent = `Run Benchmark (${costStr})`;
    }

    if (dom.btnStartBenchmark) {
        dom.btnStartBenchmark.disabled = (modelCount === 0 || promptCount === 0 || referenceBlocked || state.isBenchmarking);
    }
}

export function toggleBenchmarkDrawer() {
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
    dom.benchmarkDrawer?.classList.remove("hidden");
    await loadBenchmarkModelCatalog();
    updatePreflightCost();
}

export function closeBenchmarkDrawer() {
    stopPanLoop();
    stopZoomLoop();
    state.benchmarkDrawerOpen = false;
    dom.benchmarkDrawer?.classList.add("hidden");
    dom.canvas.focus();
}

function openBudgetGuardModal(estimatedCost, modelCount, promptCount, totalImages) {
    stopPanLoop();
    stopZoomLoop();
    state.budgetGuardModalOpen = true;
    const costStr = `$${estimatedCost.toFixed(2)}`;
    if (dom.budgetGuardCostAmount) dom.budgetGuardCostAmount.textContent = costStr;
    if (dom.budgetGuardBreakdown) {
        dom.budgetGuardBreakdown.textContent = `${modelCount} models × ${promptCount} prompts = ${totalImages} images`;
    }
    if (dom.btnConfirmBudgetGuardText) {
        dom.btnConfirmBudgetGuardText.textContent = `Confirm & Run (${costStr})`;
    }
    dom.budgetGuardModal?.classList.remove("hidden");
}

export function closeBudgetGuardModal() {
    stopPanLoop();
    stopZoomLoop();
    state.budgetGuardModalOpen = false;
    dom.budgetGuardModal?.classList.add("hidden");
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

function openBenchmarkProgressWindow(metrics) {
    stopPanLoop();
    stopZoomLoop();
    state.benchmarkWindowMinimized = false;
    dom.benchmarkWindowPill?.classList.add("hidden");
    dom.benchmarkProgressWindow?.classList.remove("hidden");

    const total = metrics?.totalImages || 0;
    if (dom.benchmarkWindowStatus) {
        dom.benchmarkWindowStatus.textContent = "Running";
        dom.benchmarkWindowStatus.className = "benchmark-window-badge status-running";
    }
    if (dom.benchmarkWindowCurrentStep) {
        dom.benchmarkWindowCurrentStep.textContent = "Initializing batch...";
    }
    if (dom.benchmarkWindowProgressStats) {
        dom.benchmarkWindowProgressStats.textContent = `0/${total} (0%)`;
    }
    if (dom.benchmarkWindowProgressBar) {
        dom.benchmarkWindowProgressBar.style.width = "0%";
    }
    if (dom.btnBenchmarkWindowClose) {
        dom.btnBenchmarkWindowClose.disabled = true;
    }
    if (dom.btnBenchmarkWindowCancel) {
        dom.btnBenchmarkWindowCancel.disabled = false;
    }
    if (dom.benchmarkWindowCost) {
        dom.benchmarkWindowCost.textContent = "$0.0000";
    }
    if (dom.benchmarkPillCount) {
        dom.benchmarkPillCount.textContent = `0/${total}`;
    }
    if (dom.benchmarkPillCost) {
        dom.benchmarkPillCost.textContent = "$0.00";
    }
    if (dom.benchmarkWindowPreviewCount) {
        dom.benchmarkWindowPreviewCount.textContent = "0 generated";
    }

    // Reset previews strip
    if (dom.benchmarkWindowPreviews) {
        dom.benchmarkWindowPreviews.innerHTML = `<div class="benchmark-previews-empty" id="benchmark-previews-empty">Waiting for first image completion...</div>`;
    }

    // Reset execution logs
    if (dom.benchmarkWindowLogs) {
        dom.benchmarkWindowLogs.innerHTML = "";
    }
    appendBenchmarkLog("INFO", `Initializing benchmark run: ${metrics?.modelCount || 0} model(s) × ${metrics?.promptCount || 0} prompt(s) = ${total} images.`);
}

function closeBenchmarkProgressWindow() {
    dom.benchmarkProgressWindow?.classList.add("hidden");
    dom.benchmarkWindowPill?.classList.add("hidden");
}

function minimizeBenchmarkWindow() {
    state.benchmarkWindowMinimized = true;
    dom.benchmarkProgressWindow?.classList.add("hidden");
    dom.benchmarkWindowPill?.classList.remove("hidden");
}

function expandBenchmarkWindow() {
    state.benchmarkWindowMinimized = false;
    dom.benchmarkWindowPill?.classList.add("hidden");
    dom.benchmarkProgressWindow?.classList.remove("hidden");
}

function toggleBenchmarkWindowMinimize() {
    if (state.benchmarkWindowMinimized) {
        expandBenchmarkWindow();
    } else {
        minimizeBenchmarkWindow();
    }
}

function appendBenchmarkLog(type, message) {
    if (!dom.benchmarkWindowLogs) return;
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

    dom.benchmarkWindowLogs.appendChild(row);
    dom.benchmarkWindowLogs.scrollTop = dom.benchmarkWindowLogs.scrollHeight;
}

function addBenchmarkThumbnailPreview(metadata) {
    if (!dom.benchmarkWindowPreviews || !metadata || !metadata.id) return;

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
    if (activeCard && activeCard.parentNode === dom.benchmarkWindowPreviews) {
        dom.benchmarkWindowPreviews.insertBefore(card, activeCard);
    } else {
        dom.benchmarkWindowPreviews.appendChild(card);
    }

    // Auto-scroll strip to newest thumbnail
    dom.benchmarkWindowPreviews.scrollLeft = dom.benchmarkWindowPreviews.scrollWidth;
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

    if (dom.btnStartBenchmark) dom.btnStartBenchmark.disabled = true;
    dom.benchmarkProgressContainer?.classList.remove("hidden");
    if (dom.benchmarkProgressBar) dom.benchmarkProgressBar.style.width = "5%";
    if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = "Connecting to benchmark runner...";

    state.benchmarkAbortController = new AbortController();

    const customPrompts = [];
    if (metrics.hasCustom) {
        customPrompts.push({
            name: dom.benchmarkCustomName?.value.trim() || "Custom Scenario",
            prompt: dom.benchmarkCustomPrompt?.value.trim() || "",
        });
    }

    const payload = {
        models: metrics.models,
        archetypes: metrics.archetypes,
        custom_prompts: customPrompts,
        use_reference_conditioning: dom.benchmarkEnableReference ? dom.benchmarkEnableReference.checked : true,
    };
    if (dom.benchmarkEnableCustomLora?.checked && dom.benchmarkCustomLoraPath?.value.trim()) {
        payload.lora_path = dom.benchmarkCustomLoraPath.value.trim();
        const scale = parseFloat(dom.benchmarkCustomLoraScale?.value);
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
            if (dom.benchmarkProgressBar) dom.benchmarkProgressBar.style.width = "100%";
            if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = msg;

            if (dom.benchmarkWindowProgressBar) dom.benchmarkWindowProgressBar.style.width = "100%";
            if (dom.benchmarkWindowProgressStats) dom.benchmarkWindowProgressStats.textContent = `${data.completed}/${data.total} (100%)`;
            if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = msg;
            if (dom.benchmarkWindowStatus) {
                dom.benchmarkWindowStatus.textContent = "Complete";
                dom.benchmarkWindowStatus.className = "benchmark-window-badge status-complete";
            }
            if (dom.benchmarkWindowCost) dom.benchmarkWindowCost.textContent = `$${(data.total_cost_usd || 0).toFixed(4)}`;
            if (dom.benchmarkPillCount) dom.benchmarkPillCount.textContent = `${data.completed}/${data.total}`;
            if (dom.benchmarkPillCost) dom.benchmarkPillCost.textContent = `$${(data.total_cost_usd || 0).toFixed(2)}`;
            if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
            if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;

            appendBenchmarkLog("SUCCESS", `Benchmark batch completed! (${data.completed}/${data.total} images)`);
            showToast(msg, "success");
            await fetchStagingQueue();
            fetchStatus();
            fetchCostStats();
        }
    } catch (err) {
        document.getElementById("benchmark-active-generation-card")?.remove();
        if (err.name === "AbortError") {
            if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = "Benchmark run cancelled by user.";
            if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = "Benchmark batch cancelled by user.";
            if (dom.benchmarkWindowStatus) {
                dom.benchmarkWindowStatus.textContent = "Cancelled";
                dom.benchmarkWindowStatus.className = "benchmark-window-badge status-cancelled";
            }
            appendBenchmarkLog("CANCEL", "Benchmark run cancelled by user.");
            if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
            if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;
            showToast("Benchmark execution cancelled", "info");
        } else {
            if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = `Benchmark failed: ${err.message}`;
            if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = `Failed: ${err.message}`;
            if (dom.benchmarkWindowStatus) {
                dom.benchmarkWindowStatus.textContent = "Error";
                dom.benchmarkWindowStatus.className = "benchmark-window-badge status-error";
            }
            appendBenchmarkLog("ERROR", `Benchmark failed: ${err.message}`);
            if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
            if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;
            showToast(`Benchmark error: ${err.message}`, "danger");
        }
    } finally {
        state.isBenchmarking = false;
        state.benchmarkAbortController = null;
        document.getElementById("benchmark-active-generation-card")?.remove();
        if (dom.btnStartBenchmark) dom.btnStartBenchmark.disabled = false;
        fetchCostStats();
    }
}

function handleBenchmarkEvent(event, data) {
    if (!data) return;
    const phase = data.phase || (event === "complete" ? "complete" : event === "cancelled" ? "cancelled" : event === "error" ? "error" : "progress");

    if (phase === "starting") {
        const total = data.total || 0;
        if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = data.message || "Starting benchmark batch...";
        if (dom.benchmarkWindowProgressStats) dom.benchmarkWindowProgressStats.textContent = `0/${total} (0%)`;
        if (dom.benchmarkPillCount) dom.benchmarkPillCount.textContent = `0/${total}`;
        if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = data.message || "Starting benchmark batch...";
        appendBenchmarkLog("INFO", data.message || `Starting benchmark batch (${total} tasks queued)`);
    } else if (phase === "generating") {
        const promptName = data.prompt || "Scenario";
        const modelName = data.model || "Model";
        const shortModel = modelName.split("/").pop() || modelName;

        if (dom.benchmarkWindowCurrentStep) {
            dom.benchmarkWindowCurrentStep.textContent = data.message || `Generating ${promptName} with ${modelName}...`;
        }
        if (dom.benchmarkProgressText) {
            dom.benchmarkProgressText.textContent = data.message || `Generating ${promptName}...`;
        }

        // Remove empty placeholder in previews
        document.getElementById("benchmark-previews-empty")?.remove();

        // Display or update active generation indicator card
        let activeCard = document.getElementById("benchmark-active-generation-card");
        if (!activeCard) {
            activeCard = document.createElement("div");
            activeCard.id = "benchmark-active-generation-card";
            activeCard.className = "benchmark-preview-card preview-card-generating";
            dom.benchmarkWindowPreviews?.appendChild(activeCard);
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

        if (dom.benchmarkWindowProgressBar) {
            dom.benchmarkWindowProgressBar.style.width = `${Math.min(100, Math.max(5, pct))}%`;
        }
        if (dom.benchmarkProgressBar) {
            dom.benchmarkProgressBar.style.width = `${Math.min(100, Math.max(5, pct))}%`;
        }
        if (dom.benchmarkWindowProgressStats) {
            dom.benchmarkWindowProgressStats.textContent = `${completed}/${total} (${pct}%)`;
        }
        if (dom.benchmarkWindowCurrentStep) {
            dom.benchmarkWindowCurrentStep.textContent = data.message || `Completed ${completed}/${total}`;
        }
        if (dom.benchmarkProgressText) {
            const spendStr = spend ? ` ($${spend.toFixed(4)})` : "";
            dom.benchmarkProgressText.textContent = `${data.message || `Generated ${completed}/${total}`}${spendStr}`;
        }
        if (dom.benchmarkPillCount) {
            dom.benchmarkPillCount.textContent = `${completed}/${total}`;
        }
        if (dom.benchmarkPillCost) {
            dom.benchmarkPillCost.textContent = `$${spend.toFixed(2)}`;
        }
        if (dom.benchmarkWindowCost) {
            dom.benchmarkWindowCost.textContent = `$${spend.toFixed(4)}`;
        }

        if (data.metadata) {
            addBenchmarkThumbnailPreview(data.metadata);
            const latencySec = (data.metadata.latency_ms / 1000).toFixed(1);
            const costStr = data.metadata.cost_usd.toFixed(4);
            appendBenchmarkLog("DONE", `Completed ${data.metadata.prompt_name} (${data.metadata.model}) in ${latencySec}s ($${costStr})`);
            if (dom.benchmarkWindowPreviewCount) {
                dom.benchmarkWindowPreviewCount.textContent = `${completed} generated`;
            }
        }

        fetchCostStats();
    } else if (phase === "complete") {
        document.getElementById("benchmark-active-generation-card")?.remove();

        const total = data.total || data.completed || 0;
        const completed = data.completed || total;
        const spend = data.current_spend || data.total_cost_usd || 0;

        if (dom.benchmarkWindowProgressBar) dom.benchmarkWindowProgressBar.style.width = "100%";
        if (dom.benchmarkProgressBar) dom.benchmarkProgressBar.style.width = "100%";
        if (dom.benchmarkWindowProgressStats) dom.benchmarkWindowProgressStats.textContent = `${completed}/${total} (100%)`;
        if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = data.message || "Benchmark batch completed!";
        if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = data.message || "Benchmark complete!";

        if (dom.benchmarkWindowStatus) {
            dom.benchmarkWindowStatus.textContent = "Complete";
            dom.benchmarkWindowStatus.className = "benchmark-window-badge status-complete";
        }
        if (dom.benchmarkWindowCost) {
            dom.benchmarkWindowCost.textContent = `$${spend.toFixed(4)}`;
        }
        if (dom.benchmarkPillCount) {
            dom.benchmarkPillCount.textContent = `${completed}/${total}`;
        }
        if (dom.benchmarkPillCost) {
            dom.benchmarkPillCost.textContent = `$${spend.toFixed(2)}`;
        }

        if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
        if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;

        appendBenchmarkLog("SUCCESS", `Benchmark batch completed! (${completed}/${total} images, $${spend.toFixed(4)})`);
        showToast(data.message || "Benchmark batch completed!", "success");

        fetchStagingQueue();
        fetchStatus();
        fetchCostStats();
    } else if (phase === "cancelled") {
        document.getElementById("benchmark-active-generation-card")?.remove();

        if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = data.message || "Benchmark run cancelled.";
        if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = data.message || "Cancelled";
        if (dom.benchmarkWindowStatus) {
            dom.benchmarkWindowStatus.textContent = "Cancelled";
            dom.benchmarkWindowStatus.className = "benchmark-window-badge status-cancelled";
        }
        if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
        if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;

        appendBenchmarkLog("CANCEL", data.message || "Benchmark batch execution cancelled.");
        showToast(data.message || "Benchmark cancelled", "info");

        fetchStagingQueue();
        fetchStatus();
        fetchCostStats();
    } else if (phase === "error") {
        document.getElementById("benchmark-active-generation-card")?.remove();

        const errMsg = data.message || data.error || "Unknown error";
        if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = `Error: ${errMsg}`;
        if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = `Error: ${errMsg}`;
        if (dom.benchmarkWindowStatus) {
            dom.benchmarkWindowStatus.textContent = "Error";
            dom.benchmarkWindowStatus.className = "benchmark-window-badge status-error";
        }
        if (dom.btnBenchmarkWindowClose) dom.btnBenchmarkWindowClose.disabled = false;
        if (dom.btnBenchmarkWindowCancel) dom.btnBenchmarkWindowCancel.disabled = true;

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
    if (dom.benchmarkProgressText) dom.benchmarkProgressText.textContent = "Cancelling benchmark batch...";
    if (dom.benchmarkWindowCurrentStep) dom.benchmarkWindowCurrentStep.textContent = "Cancelling benchmark batch...";
    appendBenchmarkLog("CANCEL", "Cancellation requested by user.");
    showToast("Benchmark batch cancellation requested", "info");
}

export function setupBenchmarkControls() {
    dom.btnOpenBenchmarkDrawer?.addEventListener("click", () => openBenchmarkDrawer());
    dom.btnCloseBenchmarkDrawer?.addEventListener("click", () => closeBenchmarkDrawer());
    dom.btnCancelBenchmarkDrawer?.addEventListener("click", () => closeBenchmarkDrawer());

    dom.benchmarkDrawer?.addEventListener("click", (e) => {
        if (e.target === dom.benchmarkDrawer) closeBenchmarkDrawer();
    });

    dom.btnModelsSelectAll?.addEventListener("click", () => {
        document.querySelectorAll('input[name="benchmark-model"]').forEach((cb) => (cb.checked = true));
        updatePreflightCost();
    });
    dom.btnModelsSelectNone?.addEventListener("click", () => {
        document.querySelectorAll('input[name="benchmark-model"]').forEach((cb) => (cb.checked = false));
        updatePreflightCost();
    });
    dom.btnPromptsSelectAll?.addEventListener("click", () => {
        document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => (cb.checked = true));
        updatePreflightCost();
    });
    dom.btnPromptsSelectNone?.addEventListener("click", () => {
        document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => (cb.checked = false));
        updatePreflightCost();
    });

    // The model checklist is rendered from the registry after each fetch, so its change events
    // are handled by delegation on the container instead of per-checkbox listeners.
    dom.benchmarkModelsList?.addEventListener("change", (e) => {
        if (e.target && e.target.name === "benchmark-model") updatePreflightCost();
    });
    dom.benchmarkEnableReference?.addEventListener("change", () => updatePreflightCost());
    document.querySelectorAll('input[name="benchmark-archetype"]').forEach((cb) => {
        cb.addEventListener("change", () => updatePreflightCost());
    });

    dom.benchmarkEnableCustom?.addEventListener("change", () => {
        dom.customPromptFields?.classList.toggle("hidden", !dom.benchmarkEnableCustom.checked);
        updatePreflightCost();
    });
    dom.benchmarkCustomName?.addEventListener("input", () => updatePreflightCost());
    dom.benchmarkCustomPrompt?.addEventListener("input", () => updatePreflightCost());

    dom.benchmarkEnableCustomLora?.addEventListener("change", () => {
        dom.benchmarkCustomLoraFields?.classList.toggle("hidden", !dom.benchmarkEnableCustomLora.checked);
    });
    dom.btnStartBenchmark?.addEventListener("click", () => handleStartBenchmarkClick());
    dom.btnCancelBenchmarkRun?.addEventListener("click", () => cancelBenchmarkExecution());

    // Floating Benchmark Progress Window & Pill Controls
    dom.btnBenchmarkWindowMinimize?.addEventListener("click", () => minimizeBenchmarkWindow());
    dom.btnBenchmarkWindowClose?.addEventListener("click", () => closeBenchmarkProgressWindow());
    dom.btnBenchmarkWindowCancel?.addEventListener("click", () => cancelBenchmarkExecution());
    dom.benchmarkWindowPill?.addEventListener("click", () => expandBenchmarkWindow());
    dom.btnBenchmarkClearLogs?.addEventListener("click", () => {
        if (dom.benchmarkWindowLogs) dom.benchmarkWindowLogs.innerHTML = "";
    });

    dom.btnCloseBudgetGuard?.addEventListener("click", () => closeBudgetGuardModal());
    dom.btnDismissBudgetGuard?.addEventListener("click", () => closeBudgetGuardModal());
    dom.budgetGuardModal?.addEventListener("click", (e) => {
        if (e.target === dom.budgetGuardModal) closeBudgetGuardModal();
    });
    dom.btnConfirmBudgetGuard?.addEventListener("click", () => {
        const metrics = calculatePreflightMetrics();
        startBenchmarkExecution(metrics);
    });
}
