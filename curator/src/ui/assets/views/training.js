/**
 * LoRA Training Runs & Adapter Studio: the drawer, run launch/dispatch/cancel, artifact
 * verification, checkpoint publication, adapter retirement and the status poll
 * (Issue #209, ADR-0009, ADR-0010).
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { escapeHtml } from "../kit/dom.js";
import { formatDuration } from "../kit/format.js";
import { stopPanLoop, stopZoomLoop } from "../stage/canvas.js";
import { showToast } from "../shell/status.js";
import { closeHelpModal } from "../shell/help.js";
import { closeIngestionDrawer } from "./ingestion.js";
import {
    closeBenchmarkDrawer,
    fetchCostStats,
    loadBenchmarkModelCatalog,
} from "./benchmark.js";
import { datasetScopeTheme, datasetScopeQuery } from "../dataset-scope.js";
import { datasetIssueLabel } from "./readiness.js";
import {
    fetchDatasetSnapshots,
    showFreezeProgressPanel,
    updateFreezeProgressUI,
    showFreezeRefusal,
    performFreezeSnapshotWithProgress,
} from "./snapshots.js";

// ==========================================================================
// LoRA Training Runs & Adapter Studio (Issue #209, ADR-0009, ADR-0010)
// ==========================================================================

let trainingPollTimer = null;

export function toggleTrainingDrawer() {
    if (state.trainingDrawerOpen) {
        closeTrainingDrawer();
    } else {
        openTrainingDrawer();
    }
}

export async function openTrainingDrawer(options = {}) {
    stopPanLoop();
    stopZoomLoop();
    if (state.helpModalOpen) closeHelpModal();
    if (state.drawerOpen) closeIngestionDrawer();
    if (state.benchmarkDrawerOpen) closeBenchmarkDrawer();

    state.trainingDrawerOpen = true;
    dom.trainingDrawer?.classList.remove("hidden");

    const theme = datasetScopeTheme();
    if (dom.trainingScopeHint) {
        dom.trainingScopeHint.textContent = theme ? `Target theme scope: '${theme}'` : "Target scope: All themes";
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

    if (options.snapshotId && dom.trainingSnapshotSelect) {
        dom.trainingSnapshotSelect.value = options.snapshotId;
        setLaunchFormCollapsed(false);
    }
}

export function closeTrainingDrawer() {
    stopPanLoop();
    stopZoomLoop();
    state.trainingDrawerOpen = false;
    dom.trainingDrawer?.classList.add("hidden");
    dom.canvas?.focus();
}

function switchTrainingTab(tab) {
    state.trainingActiveTab = tab;
    const isRuns = tab === "runs";
    dom.tabTrainingRuns?.classList.toggle("active", isRuns);
    dom.tabTrainingRuns?.setAttribute("aria-selected", String(isRuns));
    dom.tabTrainingAdapters?.classList.toggle("active", !isRuns);
    dom.tabTrainingAdapters?.setAttribute("aria-selected", String(!isRuns));

    dom.trainingViewRuns?.classList.toggle("hidden", !isRuns);
    dom.trainingViewAdapters?.classList.toggle("hidden", isRuns);

    if (!isRuns) {
        fetchTrainingAdapters();
    }
}

function setLaunchFormCollapsed(collapsed) {
    state.launchFormCollapsed = collapsed;
    if (dom.trainingLaunchForm) {
        dom.trainingLaunchForm.style.display = collapsed ? "none" : "";
    }
    if (dom.btnToggleLaunchText) {
        dom.btnToggleLaunchText.textContent = collapsed ? "Expand" : "Collapse";
    }
}

async function checkTrainingScopeReadiness() {
    if (!dom.trainingReadinessIndicator) return;
    dom.trainingReadinessIndicator.textContent = "Auditing...";
    dom.trainingReadinessIndicator.className = "gate-status mono";

    try {
        const res = await fetch(`/api/dataset/readiness${datasetScopeQuery()}`);
        if (!res.ok) {
            dom.trainingReadinessIndicator.textContent = `Error ${res.status}`;
            dom.trainingReadinessIndicator.className = "gate-status mono has-issues";
            return null;
        }
        const report = await res.json();
        const total = report.maps_total || 0;
        const ready = Boolean(report.ready);
        if (ready) {
            dom.trainingReadinessIndicator.textContent = `Ready (${report.sections_total || 0} sections / ${total} maps)`;
            dom.trainingReadinessIndicator.className = "gate-status mono is-ready";
        } else {
            dom.trainingReadinessIndicator.textContent = `Not Ready (${report.maps_ready || 0}/${total} maps)`;
            dom.trainingReadinessIndicator.className = "gate-status mono has-issues";
        }
        return report;
    } catch (err) {
        dom.trainingReadinessIndicator.textContent = "Offline";
        dom.trainingReadinessIndicator.className = "gate-status mono has-issues";
        return null;
    }
}

export function populateTrainingSnapshotSelect(snapshots) {
    if (!dom.trainingSnapshotSelect) return;
    const currentVal = dom.trainingSnapshotSelect.value;
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
    dom.trainingSnapshotSelect.innerHTML = html;
    if (currentVal && Array.from(dom.trainingSnapshotSelect.options).some(o => o.value === currentVal)) {
        dom.trainingSnapshotSelect.value = currentVal;
    }
}

function showTrainingLaunchError(errorObj) {
    if (!dom.trainingLaunchError) return;
    if (!errorObj) {
        dom.trainingLaunchError.classList.add("hidden");
        dom.trainingLaunchError.innerHTML = "";
        return;
    }

    dom.trainingLaunchError.classList.remove("hidden");
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
    dom.trainingLaunchError.innerHTML = inner;
}

async function startTrainingRun() {
    if (state.isLaunchingRun) return;
    state.isLaunchingRun = true;
    showTrainingLaunchError(null);

    if (dom.btnStartTrainingRun) dom.btnStartTrainingRun.disabled = true;
    if (dom.btnStartRunText) dom.btnStartRunText.textContent = "Launching Run...";

    try {
        let snapshotId = dom.trainingSnapshotSelect?.value || "__freeze_current__";
        const policy = state.trainingSelectedPolicy || "calibration";

        // If user selected to freeze current scope inline:
        if (snapshotId === "__freeze_current__") {
            const scopeTheme = datasetScopeTheme();
            const scopeLabel = scopeTheme ? `theme '${scopeTheme}'` : "all themes";
            showToast(`Auditing readiness & freezing snapshot for ${scopeLabel}...`, "info");
            if (dom.trainingScopeHint) dom.trainingScopeHint.textContent = `Freezing snapshot for ${scopeLabel}...`;
            showFreezeProgressPanel(true);

            try {
                const createdSnapshot = await performFreezeSnapshotWithProgress(scopeTheme, (event, data) => {
                    updateFreezeProgressUI(data);
                    if (dom.trainingScopeHint && data.message) {
                        dom.trainingScopeHint.textContent = data.message;
                    }
                });
                snapshotId = createdSnapshot.id;
                showToast(`Created snapshot '${snapshotId}'! Proceeding to training run...`, "info");
                fetchDatasetSnapshots();
                setTimeout(() => showFreezeProgressPanel(false), 2000);
            } catch (errData) {
                showFreezeProgressPanel(false);
                if (dom.trainingScopeHint) dom.trainingScopeHint.textContent = "Target: Current active theme scope.";
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
        if (dom.btnStartTrainingRun) dom.btnStartTrainingRun.disabled = false;
        if (dom.btnStartRunText) dom.btnStartRunText.textContent = "Launch Training Run";
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

export async function fetchTrainingRuns() {
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
        if (dom.trainingRunsCountTab) dom.trainingRunsCountTab.textContent = String(total);
        if (dom.trainingRunsTotalBadge) dom.trainingRunsTotalBadge.textContent = String(total);

        // Active badge
        const activeCount = state.trainingRuns.filter(r => r.state === "running" || r.state === "queued").length;
        if (dom.trainingRunsActiveBadge) {
            if (activeCount > 0) {
                dom.trainingRunsActiveBadge.textContent = `${activeCount} Active`;
                dom.trainingRunsActiveBadge.classList.remove("hidden");
            } else {
                dom.trainingRunsActiveBadge.classList.add("hidden");
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
        if (dom.trainingAdaptersCountTab) dom.trainingAdaptersCountTab.textContent = String(total);
        if (dom.adaptersCountBadge) dom.adaptersCountBadge.textContent = `${total} Adapter${total === 1 ? "" : "s"}`;

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
    if (!dom.trainingRunsList) return;
    const filter = state.trainingRunsFilter || "all";
    const filtered = state.trainingRuns.filter(r => {
        if (filter === "active") return r.state === "running" || r.state === "queued";
        if (filter === "succeeded") return r.state === "succeeded";
        if (filter === "failed") return r.state === "failed";
        if (filter === "cancelled") return r.state === "cancelled";
        return true;
    });

    if (filtered.length === 0) {
        dom.trainingRunsList.innerHTML = "";
        dom.trainingRunsEmpty?.classList.remove("hidden");
        return;
    }

    dom.trainingRunsEmpty?.classList.add("hidden");
    dom.trainingRunsList.innerHTML = filtered.map(r => {
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
    dom.trainingRunsList.querySelectorAll(".btn-dispatch-run").forEach(btn => {
        btn.addEventListener("click", () => {
            const runId = btn.getAttribute("data-run-id");
            if (runId) dispatchTrainingRun(runId);
        });
    });
    dom.trainingRunsList.querySelectorAll(".btn-cancel-run").forEach(btn => {
        btn.addEventListener("click", () => {
            const runId = btn.getAttribute("data-run-id");
            if (runId) cancelTrainingRun(runId);
        });
    });
    dom.trainingRunsList.querySelectorAll(".btn-verify-run").forEach(btn => {
        btn.addEventListener("click", () => {
            const runId = btn.getAttribute("data-run-id");
            if (runId) verifyTrainingArtifacts(runId);
        });
    });
    dom.trainingRunsList.querySelectorAll(".btn-publish-checkpoint").forEach(btn => {
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
    if (!dom.trainingAdaptersList) return;
    const filter = state.trainingAdaptersFilter || "all";
    const filtered = state.trainingAdapters.filter(a => {
        if (filter === "usable") return !a.retired;
        if (filter === "retired") return Boolean(a.retired);
        return true;
    });

    if (filtered.length === 0) {
        dom.trainingAdaptersList.innerHTML = "";
        dom.trainingAdaptersEmpty?.classList.remove("hidden");
        return;
    }

    dom.trainingAdaptersEmpty?.classList.add("hidden");
    dom.trainingAdaptersList.innerHTML = filtered.map(a => {
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
    dom.trainingAdaptersList.querySelectorAll(".btn-retire-adapter").forEach(btn => {
        btn.addEventListener("click", () => {
            const id = btn.getAttribute("data-id");
            if (id) setTrainingAdapterRetired(id, true);
        });
    });
    dom.trainingAdaptersList.querySelectorAll(".btn-restore-adapter").forEach(btn => {
        btn.addEventListener("click", () => {
            const id = btn.getAttribute("data-id");
            if (id) setTrainingAdapterRetired(id, false);
        });
    });
}

export function setupTrainingControls() {
    dom.btnOpenTrainingDrawer?.addEventListener("click", () => openTrainingDrawer());
    dom.tabNavTraining?.addEventListener("click", () => openTrainingDrawer());
    dom.btnOpenTrainingFromLibrary?.addEventListener("click", () => openTrainingDrawer());
    dom.btnCloseTrainingDrawer?.addEventListener("click", () => closeTrainingDrawer());
    dom.btnCancelTrainingDrawer?.addEventListener("click", () => closeTrainingDrawer());

    dom.trainingDrawer?.addEventListener("click", (e) => {
        if (e.target === dom.trainingDrawer) closeTrainingDrawer();
    });

    dom.tabTrainingRuns?.addEventListener("click", () => switchTrainingTab("runs"));
    dom.tabTrainingAdapters?.addEventListener("click", () => switchTrainingTab("adapters"));

    dom.btnToggleLaunchForm?.addEventListener("click", () => {
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

    dom.btnCheckTrainingReadiness?.addEventListener("click", () => checkTrainingScopeReadiness());
    dom.btnStartTrainingRun?.addEventListener("click", () => startTrainingRun());
    dom.btnRefreshTrainingRuns?.addEventListener("click", () => fetchTrainingRuns());

    dom.trainingRunsFilter?.addEventListener("change", (e) => {
        state.trainingRunsFilter = e.target.value;
        renderTrainingRuns();
    });

    dom.btnRefreshTrainingAdapters?.addEventListener("click", () => fetchTrainingAdapters());
    dom.trainingAdaptersFilter?.addEventListener("change", (e) => {
        state.trainingAdaptersFilter = e.target.value;
        renderTrainingAdapters();
    });
}
