/**
 * LoRA Training Dataset Readiness: the theme-scoped audit cache, the per-issue-kind summary the
 * snapshots drawer renders, and the one-click remediation it offers (Issues #184, #245).
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { escapeHtml } from "../kit/dom.js";
import { formatReadinessAge } from "../kit/format.js";
import {
    fetchLibraryMaps,
    switchView,
    clearLibraryFilters,
    applyLibraryFilters,
} from "./library.js";
import {
    openLibraryFullscreen,
    toggleFullscreenGridPanel,
} from "./library/fullscreen.js";
import { openLibraryInspector } from "./inspector.js";
import { showToast } from "../shell/status.js";
import { datasetScopeTheme, datasetScopeQuery } from "../dataset-scope.js";
import { toggleSnapshotsShelf } from "./snapshots.js";

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

export function datasetIssueLabel(code) {
    return DATASET_ISSUE_LABELS[code] || code;
}

/** The one-click remediation a flagged code offers, or null when no single click resolves it. */
function datasetIssueFix(code) {
    if (READINESS_DESCRIPTION_CODES.includes(code)) return { action: "describe", label: "Fix descriptions" };
    if (READINESS_GRID_CODES.includes(code)) return { action: "grid", label: "Edit grid" };
    if (READINESS_GROUND_CODES.includes(code)) return { action: "inspect", label: "Inspect bundle" };
    return null;
}

export function readinessScopeKey() {
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

export function clearReadinessCache() {
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

export function renderReadinessPanel() {
    if (!dom.datasetReadinessSummary || !dom.datasetReadinessIssues) return;
    const report = state.readinessReport;
    const refusal = state.readinessRefusal;
    const ready = Boolean(report && report.ready);

    if (dom.datasetReadinessStatus) {
        dom.datasetReadinessStatus.textContent = readinessStatusText(report, refusal);
        dom.datasetReadinessStatus.classList.toggle("is-ready", ready && !refusal);
        dom.datasetReadinessStatus.classList.toggle("has-issues", Boolean(refusal) || Boolean(report && !ready));
    }
    if (dom.datasetReadinessIcon) {
        dom.datasetReadinessIcon.textContent = refusal ? "✕" : ready ? "✓" : report ? "!" : "◍";
        dom.datasetReadinessIcon.className = refusal || (report && !ready)
            ? "readiness-icon has-issues"
            : ready
                ? "readiness-icon is-ready"
                : "readiness-icon";
    }
    dom.datasetReadinessSummary.innerHTML = readinessStatsHtml(report, state.readinessAuditedAt);
    dom.datasetReadinessIssues.innerHTML = readinessIssuesHtml(report, refusal);
    if (dom.btnAuditReadinessLabel) {
        dom.btnAuditReadinessLabel.textContent = state.readinessAudit ? "Auditing…" : report ? "Re-audit" : "Audit";
    }
}

/** Show the audit stored for the active scope; audit only when the cache has nothing. */
export function syncReadinessToScope({ audit = false } = {}) {
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

export function applyReadinessReport(scopeKey, report, auditedAt) {
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
    if (dom.btnAuditReadiness) dom.btnAuditReadiness.disabled = true;
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
        if (dom.btnAuditReadiness) dom.btnAuditReadiness.disabled = false;
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
    return dom.datasetReadinessIssues ? [...dom.datasetReadinessIssues.querySelectorAll("[data-fix]")] : [];
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
export function revealReadinessPanel() {
    if (state.currentView !== "library") switchView("library");
    if (state.snapshotsCollapsed) toggleSnapshotsShelf();
    dom.datasetReadinessPanel?.classList.add("is-refused");
    dom.datasetReadinessPanel?.scrollIntoView({ behavior: "smooth", block: "center" });
    setTimeout(() => dom.datasetReadinessPanel?.classList.remove("is-refused"), 3000);
}

export function setupReadinessControls() {
    dom.btnAuditReadiness?.addEventListener("click", () => auditDatasetReadiness({ force: true }));
    dom.datasetReadinessIssues?.addEventListener("click", (e) => {
        const button = e.target.closest("[data-fix]");
        if (!button) return;
        e.preventDefault();
        fixDatasetIssueKind(button.dataset.issueCode);
    });
}
