/**
 * The Dataset Library's readiness audit filter (Issue #287).
 *
 * A stored audit report names the maps flagged per issue code. Selecting a row's "Show N maps"
 * narrows the grid to exactly those maps, composes with the tag/search/facet filters, and is
 * dropped when the scope changes or a new report replaces the selection. Selecting it never
 * sends a request: remediation stays behind its own buttons.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import {
    applyLibraryFilters,
    clearLibraryAuditFilter,
    clearLibraryFilters,
    setupLibraryControls,
    showLibraryAuditFilter,
} from "../assets/views/library.js";
import {
    applyReadinessReport,
    auditFilterMapIds,
    invalidateDatasetReadiness,
    readinessScopeKey,
    renderReadinessPanel,
    setupReadinessControls,
    syncReadinessToScope,
} from "../assets/views/readiness.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { element, installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

const MISSING = "physical_description_missing";
const GRID = "grid_below_minimum";
const GROUND = "ground_layer_missing";

const DOM_NAMES = [
    "libraryTagPills",
    "libraryGrid",
    "libraryEmpty",
    "libraryFilterEmpty",
    "libraryFilterEmptyDesc",
    "libraryCountBadge",
    "libraryAuditFilterChip",
    "libraryAuditFilterLabel",
    "libraryAuditFilterCount",
    "librarySearchInput",
];

function issuesMap(id, ...codes) {
    return { id, issues: codes.map((code) => ({ code })) };
}

function report(maps) {
    return {
        ready: false,
        maps_total: maps.length,
        maps_ready: 0,
        sections_total: maps.length,
        sections_ready: 0,
        maps,
    };
}

/** `map-gone` is flagged but absent from the curated scope: the filter must ignore it. */
function auditedReport() {
    return report([
        issuesMap("map-a", MISSING),
        issuesMap("map-b", MISSING, GRID),
        issuesMap("map-c"),
        issuesMap("map-gone", MISSING, GROUND),
    ]);
}

function curatedMaps() {
    return [
        { id: "map-a", tags: ["cave"], description: "a stone hall", layers: [{ role: "ground", index: 0 }], curated_at: "2026-01-01T00:00:00Z" },
        { id: "map-b", tags: ["crypt"], description: "", layers: [{ role: "ground", index: 0 }], curated_at: "2026-01-02T00:00:00Z" },
        { id: "map-c", tags: ["cave"], description: "mossy floor", layers: [{ role: "ground", index: 0 }], curated_at: "2026-01-03T00:00:00Z" },
    ];
}

/** The ids the grid currently shows, sorted so assertions do not depend on the deck's sort order. */
function visibleIds() {
    return state.libraryFilteredMaps.map((map) => map.id).sort();
}

function setUp(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();
    const stored = new Map();
    const previousStorage = globalThis.localStorage;
    globalThis.localStorage = {
        getItem: (key) => (stored.has(key) ? stored.get(key) : null),
        setItem: (key, value) => stored.set(key, String(value)),
        removeItem: (key) => stored.delete(key),
    };
    DOM_NAMES.forEach((name) => {
        dom[name] = element("div");
    });
    const issues = element("ul");
    let onClick = null;
    issues.addEventListener = (type, handler) => {
        if (type === "click") onClick = handler;
    };
    dom.datasetReadinessIssues = issues;
    dom.datasetReadinessSummary = element("div");
    setupReadinessControls();

    state.currentView = "library";
    state.curatedMaps = curatedMaps();
    state.libraryActiveTag = null;
    state.librarySearchQuery = "";
    state.libraryFacet = "all";
    state.librarySort = "date-desc";
    state.libraryFocusedIndex = -1;
    state.librarySelectedIds = new Set();
    state.libraryFilteredMaps = [];
    state.libraryAuditFilter = null;
    state.readinessReport = auditedReport();
    state.readinessAuditedAt = new Date().toISOString();
    state.readinessScopeKey = "all";
    state.readinessRefusal = null;
    state.readinessAudit = null;
    state.readinessAuditEpoch = -1;
    state.readinessAuditScope = null;
    state.readinessBatch = null;
    state.readinessEpoch = 0;
    applyLibraryFilters();

    t.after(() => {
        clearRequestFailure();
        globalThis.localStorage = previousStorage;
        DOM_NAMES.forEach((name) => delete dom[name]);
        delete dom.btnClearLibraryAuditFilter;
        delete dom.datasetReadinessIssues;
        delete dom.datasetReadinessSummary;
        state.curatedMaps = [];
        state.libraryFilteredMaps = [];
        state.libraryAuditFilter = null;
        state.libraryActiveTag = null;
        state.librarySearchQuery = "";
        state.libraryFacet = "all";
        state.librarySelectedIds = new Set();
        state.readinessReport = null;
        state.readinessScopeKey = null;
        state.snapshots = [];
        state.isFetchingSnapshots = false;
        ui.restore();
    });

    return {
        ui,
        click(target) {
            let prevented = false;
            onClick({
                target,
                preventDefault() {
                    prevented = true;
                },
            });
            return prevented;
        },
    };
}

test("the audit filter derives its map ids from the stored report, per code", (t) => {
    setUp(t);
    assert.deepEqual([...auditFilterMapIds(MISSING)], ["map-a", "map-b", "map-gone"]);
    assert.deepEqual([...auditFilterMapIds(GRID)], ["map-b"]);
    assert.deepEqual([...auditFilterMapIds(GROUND)], ["map-gone"]);
    assert.equal(auditFilterMapIds("unknown_code").size, 0);
});

test("selecting an issue kind narrows the grid and composes with tag, search and facet", (t) => {
    setUp(t);

    showLibraryAuditFilter(MISSING);
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);
    assert.equal(state.libraryAuditFilter.code, MISSING);
    assert.equal(dom.libraryCountBadge.textContent, "2 of 3 Maps");
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), false);
    assert.equal(dom.libraryAuditFilterLabel.textContent, "Physical Description missing");
    assert.equal(dom.libraryAuditFilterCount.textContent, "2 maps");

    // Tag filter ANDs on top.
    state.libraryActiveTag = "crypt";
    applyLibraryFilters();
    assert.deepEqual(visibleIds(), ["map-b"]);

    // Search ANDs on top of the audit filter.
    state.libraryActiveTag = null;
    state.librarySearchQuery = "map-a";
    applyLibraryFilters();
    assert.deepEqual(visibleIds(), ["map-a"]);

    // Facet ANDs on top too.
    state.librarySearchQuery = "";
    state.libraryFacet = "missing-desc";
    applyLibraryFilters();
    assert.deepEqual(visibleIds(), ["map-b"]);

    // Clearing the audit filter leaves the facet filter in charge.
    clearLibraryAuditFilter();
    assert.equal(state.libraryAuditFilter, null);
    assert.equal(state.libraryFacet, "missing-desc");
    assert.deepEqual(visibleIds(), ["map-b"]);
    assert.equal(dom.libraryCountBadge.textContent, "1 Map");
});

test("clearing the audit filter restores the full scope and hides the chip", (t) => {
    setUp(t);
    showLibraryAuditFilter(MISSING);
    assert.equal(visibleIds().length, 2);

    clearLibraryAuditFilter();
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryCountBadge.textContent, "3 Maps");
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), true);
});

test("a scope change drops the audit filter instead of leaving a stale selection", (t) => {
    setUp(t);
    showLibraryAuditFilter(MISSING);
    assert.equal(state.libraryAuditFilter.code, MISSING);

    state.libraryActiveTag = "cave";
    syncReadinessToScope();

    assert.equal(readinessScopeKey(), "cave");
    assert.equal(state.libraryAuditFilter, null);
    assert.deepEqual(visibleIds(), ["map-a", "map-c"]);
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), true);
});

test("a new report applied to the active scope replaces the audit selection", (t) => {
    setUp(t);
    showLibraryAuditFilter(MISSING);

    const fresh = report([issuesMap("map-a"), issuesMap("map-b"), issuesMap("map-c", MISSING)]);
    applyReadinessReport(readinessScopeKey(), fresh, new Date().toISOString());

    assert.equal(state.libraryAuditFilter, null);
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryCountBadge.textContent, "3 Maps");
});

test("ids missing from the curated scope are ignored, and an empty intersection explains itself", (t) => {
    setUp(t);

    showLibraryAuditFilter(MISSING);
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);

    // Only `map-gone` carries this code; it is not in the curated library.
    showLibraryAuditFilter(GROUND);
    assert.equal(state.libraryAuditFilter.code, GROUND);
    assert.equal(state.libraryAuditFilter.ids.has("map-gone"), true);
    assert.deepEqual(visibleIds(), []);
    assert.equal(dom.libraryFilterEmpty.classList.contains("hidden"), false);
    assert.match(dom.libraryFilterEmptyDesc.textContent, /Ground layer missing/);
    assert.equal(dom.libraryCountBadge.textContent, "0 of 3 Maps");
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), false);
    assert.equal(dom.libraryAuditFilterCount.textContent, "0 maps");

    // Dismissing the chip restores the scope (the wired empty-state clear is covered below).
    clearLibraryAuditFilter();
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryFilterEmpty.classList.contains("hidden"), true);
    assert.equal(dom.libraryFilterEmptyDesc.textContent, "No battlemaps match the current filter criteria.");
});

test("a curated map that loads after the selection re-enters the filter", (t) => {
    setUp(t);
    // The library fetch is still in flight when a cached report's row is clicked.
    state.curatedMaps = [];
    showLibraryAuditFilter(MISSING);
    assert.deepEqual(visibleIds(), []);

    state.curatedMaps = curatedMaps();
    applyLibraryFilters();
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);
});

test("every issue row offers Show N maps, including codes without a one-click fix", (t) => {
    setUp(t);
    state.readinessReport = report([
        issuesMap("map-a", MISSING),
        issuesMap("map-b", MISSING, GRID),
        issuesMap("map-c", "empty_dataset"),
    ]);

    renderReadinessPanel();
    const html = dom.datasetReadinessIssues.innerHTML;

    assert.match(html, /data-show-maps="physical_description_missing"/);
    assert.match(html, /data-show-maps="grid_below_minimum"/);
    assert.match(html, /data-show-maps="empty_dataset"/);
    assert.match(html, /Show 2 maps/);
    assert.match(html, /Show 1 map</);
    assert.equal((html.match(/data-show-maps=/g) || []).length, 3);
    assert.equal((html.match(/data-fix=/g) || []).length, 2);

    const rows = html.split("<li ").slice(1);
    const noFixRow = rows.find((row) => row.includes('data-show-maps="empty_dataset"'));
    assert.ok(noFixRow, "the code without a fix is still rendered");
    assert.ok(!noFixRow.includes("data-fix="));
});

test("a freeze refusal without a report keeps the action-less fallback rows", (t) => {
    setUp(t);
    state.readinessReport = null;
    state.readinessRefusal = { error: "Dataset is not ready", blocking: [{ code: MISSING, count: 2 }] };

    renderReadinessPanel();
    const html = dom.datasetReadinessIssues.innerHTML;

    assert.ok(!html.includes("data-show-maps"));
    assert.match(html, /2 findings/);
});

test("the delegated Show N maps action filters the grid and sends no request", async (t) => {
    const { click } = setUp(t);
    const server = stubFetch(() => jsonResponse({}));
    t.after(() => server.restore());

    const prevented = click({
        closest: (selector) => (selector === "[data-show-maps]" ? { dataset: { showMaps: MISSING } } : null),
    });
    for (let i = 0; i < 3; i += 1) await new Promise((resolve) => setImmediate(resolve));

    assert.equal(prevented, true);
    assert.equal(state.libraryAuditFilter.code, MISSING);
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);
    assert.equal(state.readinessBatch, null);
    assert.equal(server.calls.length, 0);
    assert.ok(!server.calls.some((call) => call.path.includes("/describe")));
});

/** Let the requests `clearLibraryFilters()` starts settle before asserting on their effects. */
async function settle() {
    for (let i = 0; i < 10; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

test("an existing selection survives a cache-hit render without auditing", async (t) => {
    setUp(t);
    const server = stubFetch(() => jsonResponse({}));
    t.after(() => server.restore());

    showLibraryAuditFilter(MISSING);
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);

    // The active scope now has its report stored, so the next sync renders from the cache.
    globalThis.localStorage.setItem("curator_dataset_readiness", JSON.stringify({
        all: { audited_at: new Date().toISOString(), report: auditedReport() },
    }));

    syncReadinessToScope({ audit: true });
    await settle();

    assert.equal(state.libraryAuditFilter.code, MISSING, "a cache-hit render keeps the selection");
    assert.deepEqual([...state.libraryAuditFilter.ids].sort(), [...auditFilterMapIds(MISSING)].sort());
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), false);
    assert.equal(dom.libraryCountBadge.textContent, "2 of 3 Maps");
    assert.deepEqual(state.readinessReport, auditedReport());
    assert.ok(
        !server.calls.some((call) => call.path === "/api/dataset/readiness"),
        "a cached report is rendered, never re-audited",
    );
});

test("Clear Filters drops the audit selection and restores the grid", async (t) => {
    setUp(t);
    const server = stubFetch((path) => {
        if (path === "/api/dataset/readiness") return jsonResponse(auditedReport());
        return jsonResponse([]);
    });
    t.after(() => server.restore());

    showLibraryAuditFilter(MISSING);
    assert.deepEqual(visibleIds(), ["map-a", "map-b"]);

    clearLibraryFilters();

    // The selection and the grid are restored before the re-audit it also starts lands.
    assert.equal(state.libraryAuditFilter, null);
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryCountBadge.textContent, "3 Maps");
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), true);

    await settle();
    assert.equal(state.libraryAuditFilter, null, "the re-applied report does not resurrect the selection");
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
});

test("the chip's remove button clears the audit filter", (t) => {
    setUp(t);
    showLibraryAuditFilter(MISSING);

    let removeListener = null;
    dom.btnClearLibraryAuditFilter = element("button");
    dom.btnClearLibraryAuditFilter.addEventListener = (type, handler) => {
        if (type === "click") removeListener = handler;
    };
    setupLibraryControls();

    assert.equal(typeof removeListener, "function", "setupLibraryControls wires the chip's remove button");
    removeListener();

    assert.equal(state.libraryAuditFilter, null);
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), true);
    assert.equal(dom.libraryCountBadge.textContent, "3 Maps");
});

test("the filtered badge pluralises the scope total", (t) => {
    setUp(t);
    state.curatedMaps = curatedMaps().filter((map) => map.id === "map-a");

    showLibraryAuditFilter(MISSING);

    assert.deepEqual(visibleIds(), ["map-a"]);
    assert.equal(dom.libraryCountBadge.textContent, "1 of 1 Map");
});

test("discarding the report drops the audit filter instead of leaving it over Not audited", async (t) => {
    setUp(t);
    const server = stubFetch((path) => {
        // The re-audit that invalidation starts fails: no new report replaces the discarded one.
        if (path === "/api/dataset/readiness") {
            return jsonResponse({ error: "audit unavailable" }, { status: 503, statusText: "Service Unavailable" });
        }
        return jsonResponse([]);
    });
    t.after(() => server.restore());

    showLibraryAuditFilter(MISSING);
    assert.equal(state.libraryAuditFilter.code, MISSING);

    invalidateDatasetReadiness();

    assert.equal(state.readinessReport, null);
    assert.equal(state.libraryAuditFilter, null);
    assert.deepEqual(visibleIds(), ["map-a", "map-b", "map-c"]);
    assert.equal(dom.libraryAuditFilterChip.classList.contains("hidden"), true);
    assert.equal(dom.libraryCountBadge.textContent, "3 Maps");

    await settle();
    assert.equal(state.readinessReport, null);
    assert.equal(state.libraryAuditFilter, null, "a failed audit leaves the discarded selection dropped");
});
