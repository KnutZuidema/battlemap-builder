import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { battlemapArchiveFilename, triggerBlobDownload } from "../assets/kit/dom.js";
import {
    exportBattlemap,
    exportBattlemaps,
    exportSelectedBattlemaps,
} from "../assets/requests/library.js";
import { RequestError } from "../assets/requests/http.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { exportFullBattlemaps, exportLoraDataset } from "../assets/views/library.js";
import { exportSelectedFullBattlemaps, exportSelectedLoraDataset } from "../assets/views/library/bulk.js";
import { exportInspectorBattlemap } from "../assets/views/inspector.js";
import { exportFullscreenBattlemap } from "../assets/views/library/fullscreen.js";
import { element, installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

function enabledTimers(t) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
}

function blobResponse(content = "PK-ZIP-BYTES", { status = 200, statusText = "OK" } = {}) {
    const blob = new Blob([content], { type: "application/zip" });
    return {
        ok: status >= 200 && status < 300,
        status,
        statusText,
        blob: async () => blob,
        json: async () => {
            throw new SyntaxError("not json");
        },
    };
}

/**
 * Install fake browser download primitives (`document.body`, `URL.createObjectURL` /
 * `revokeObjectURL`, anchor `.click()` / `.remove()`) and export buttons on `dom`.
 */
function installExportHarness(t) {
    enabledTimers(t);
    const ui = installRequestDom();

    const body = element("body");
    const clickedDownloads = [];
    const createdUrls = [];
    const revokedUrls = [];

    const origCreateElement = globalThis.document.createElement;
    globalThis.document.body = body;
    globalThis.document.createElement = (tag) => {
        const el = origCreateElement(tag);
        if (String(tag).toLowerCase() === "a") {
            el.click = () => {
                clickedDownloads.push({ href: el.href, download: el.download, attached: el.parentElement === body });
            };
            el.remove = () => {
                el.parentElement?.removeChild(el);
            };
        }
        return el;
    };

    const prevWindow = globalThis.window;
    let urlSeq = 0;
    const fakeUrlApi = {
        createObjectURL(blob) {
            const url = `blob:test/${++urlSeq}`;
            createdUrls.push({ url, size: blob.size, type: blob.type });
            return url;
        },
        revokeObjectURL(url) {
            revokedUrls.push(url);
        },
    };
    globalThis.window = { URL: fakeUrlApi };

    const exportDomKeys = [
        "btnExportLora",
        "btnExportLabel",
        "btnExportBattlemaps",
        "btnExportBattlemapsLabel",
        "btnBulkExport",
        "btnBulkExportBattlemaps",
        "btnBulkExportBattlemapsLabel",
        "btnInspectorExport",
        "btnInspectorExportLabel",
        "btnFsExport",
        "btnFsExportLabel",
    ];
    for (const key of exportDomKeys) {
        dom[key] = element(key.startsWith("btn") && !key.endsWith("Label") ? "button" : "span");
    }
    dom.btnExportLabel.textContent = "Export LoRA Dataset";
    dom.btnExportBattlemapsLabel.textContent = "Export Battlemaps";
    dom.btnBulkExportBattlemapsLabel.textContent = "Export Battlemaps (ZIP)";
    dom.btnInspectorExportLabel.textContent = "Export Battlemap";
    dom.btnFsExportLabel.textContent = "Export";

    const prevState = {
        libraryActiveTag: state.libraryActiveTag,
        librarySelectedIds: state.librarySelectedIds,
        libraryDrawerMap: state.libraryDrawerMap,
        libraryFullscreenMap: state.libraryFullscreenMap,
        isExporting: state.isExporting,
    };
    state.libraryActiveTag = null;
    state.librarySelectedIds = new Set();
    state.libraryDrawerMap = null;
    state.libraryFullscreenMap = null;
    state.isExporting = false;

    t.after(() => {
        globalThis.window = prevWindow;
        for (const key of exportDomKeys) delete dom[key];
        state.libraryActiveTag = prevState.libraryActiveTag;
        state.librarySelectedIds = prevState.librarySelectedIds;
        state.libraryDrawerMap = prevState.libraryDrawerMap;
        state.libraryFullscreenMap = prevState.libraryFullscreenMap;
        state.isExporting = prevState.isExporting;
        clearRequestFailure();
        ui.restore();
    });

    return { ui, body, clickedDownloads, createdUrls, revokedUrls };
}

test("battlemapArchiveFilename sanitizes IDs to [A-Za-z0-9._-] with battlemap fallback", () => {
    assert.equal(battlemapArchiveFilename("fort_01-v2.0"), "fort_01-v2.0.zip");
    assert.equal(battlemapArchiveFilename("map with spaces/quotes\""), "mapwithspacesquotes.zip");
    assert.equal(battlemapArchiveFilename("!!!"), "battlemap.zip");
    assert.equal(battlemapArchiveFilename("..."), "battlemap.zip");
    assert.equal(battlemapArchiveFilename("..hidden"), "hidden.zip");
    assert.equal(battlemapArchiveFilename(""), "battlemap.zip");
});

test("triggerBlobDownload attaches, clicks, removes the anchor and revokes the object URL", (t) => {
    const h = installExportHarness(t);
    const blob = new Blob(["zip-content"], { type: "application/zip" });

    triggerBlobDownload(blob, "citadel.zip");

    assert.equal(h.createdUrls.length, 1);
    assert.equal(h.createdUrls[0].size, 11);
    assert.deepEqual(h.clickedDownloads, [
        { href: "blob:test/1", download: "citadel.zip", attached: true },
    ]);
    assert.equal(h.body.children.length, 0, "anchor is removed after click");
    assert.deepEqual(h.revokedUrls, ["blob:test/1"]);
});

test("request helpers route all three Battlemap export endpoints through requestBlob", async (t) => {
    installExportHarness(t);
    const server = stubFetch(async () => blobResponse("ZIP"));
    t.after(() => server.restore());

    // Unfiltered scope
    state.libraryActiveTag = null;
    const allBlob = await exportBattlemaps();
    assert.equal(allBlob.size, 3);
    assert.equal(server.calls[0].path, "/api/curated/export");
    assert.equal(server.calls[0].init.method, "GET");

    // Canonical theme scope
    state.libraryActiveTag = "Dungeon";
    await exportBattlemaps();
    assert.equal(server.calls[1].path, "/api/curated/export?theme=dungeon");

    // Non-canonical tag falls back to unscoped export
    state.libraryActiveTag = "custom-tag";
    await exportBattlemaps();
    assert.equal(server.calls[2].path, "/api/curated/export");

    // Selected export
    await exportSelectedBattlemaps(["m2", "m1"]);
    assert.equal(server.calls[3].path, "/api/curated/export-selected");
    assert.equal(server.calls[3].init.method, "POST");
    assert.deepEqual(JSON.parse(server.calls[3].init.body), { ids: ["m2", "m1"] });

    // Single-map export (URL-encoded ID)
    await exportBattlemap("map/special 1");
    assert.equal(server.calls[4].path, "/api/curated/map%2Fspecial%201/export");
    assert.equal(server.calls[4].init.method, "GET");
});

test("exportBattlemap maps a 404 through the single failure policy", async (t) => {
    const h = installExportHarness(t);
    const server = stubFetch(async () =>
        jsonResponse({ error: "Curated map 'ghost' not found" }, { status: 404, statusText: "Not Found" }),
    );
    t.after(() => server.restore());

    await assert.rejects(exportBattlemap("ghost"), (err) => {
        assert.ok(err instanceof RequestError);
        assert.equal(err.status, 404);
        assert.equal(err.message, "Curated map 'ghost' not found");
        assert.equal(err.operation, "Export battlemap");
        return true;
    });

    assert.equal(h.ui.pill.className, "status-pill status-error");
    assert.equal(h.ui.toastContainer.children.length, 1);
});

test("library toolbar exportFullBattlemaps respects theme scope, guards overlap, and downloads ZIP", async (t) => {
    const h = installExportHarness(t);
    state.libraryActiveTag = "cave";

    let resolveFetch;
    const server = stubFetch(
        () =>
            new Promise((resolve) => {
                resolveFetch = resolve;
            }),
    );
    t.after(() => server.restore());

    const first = exportFullBattlemaps();
    assert.equal(state.isExporting, true);
    assert.equal(dom.btnExportBattlemaps.disabled, true);
    assert.equal(dom.btnExportLora.disabled, true);
    assert.equal(dom.btnExportBattlemapsLabel.textContent, "Packaging ZIP...");

    // Overlapping export calls must be ignored while in flight
    await exportFullBattlemaps();
    await exportLoraDataset();
    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].path, "/api/curated/export?theme=cave");

    resolveFetch(blobResponse("CAVE-ZIP-DATA"));
    await first;

    assert.equal(state.isExporting, false);
    assert.equal(dom.btnExportBattlemaps.disabled, false);
    assert.equal(dom.btnExportLora.disabled, false);
    assert.equal(dom.btnExportBattlemapsLabel.textContent, "Export Battlemaps");
    assert.equal(h.clickedDownloads.length, 1);
    assert.equal(h.clickedDownloads[0].download, "battlemaps-cave.zip");
    assert.equal(h.ui.toastMessages().length, 2, "info + success toast");
});

test("bulk bar exportSelectedFullBattlemaps sends selected IDs and downloads battlemaps.zip", async (t) => {
    const h = installExportHarness(t);
    const server = stubFetch(async () => blobResponse("SELECTED-MAPS-ZIP"));
    t.after(() => server.restore());

    // Empty selection is a no-op
    await exportSelectedFullBattlemaps();
    assert.equal(server.calls.length, 0);

    state.librarySelectedIds = new Set(["map_b", "map_a"]);
    await exportSelectedFullBattlemaps();

    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].path, "/api/curated/export-selected");
    assert.deepEqual(JSON.parse(server.calls[0].init.body), { ids: ["map_b", "map_a"] });
    assert.equal(h.clickedDownloads.length, 1);
    assert.equal(h.clickedDownloads[0].download, "battlemaps.zip");
    assert.equal(dom.btnBulkExportBattlemaps.disabled, false);
    assert.equal(dom.btnBulkExportBattlemapsLabel.textContent, "Export Battlemaps (ZIP)");

    // Bulk LoRA export still works via shared triggerBlobDownload
    await exportSelectedLoraDataset();
    assert.equal(server.calls[1].path, "/api/dataset/export-selected");
    assert.equal(h.clickedDownloads[1].download, "lora-dataset.zip");
});

test("inspector drawer exportInspectorBattlemap downloads the open map and restores controls on error", async (t) => {
    const h = installExportHarness(t);
    let failNext = false;
    const server = stubFetch(async () =>
        failNext
            ? jsonResponse({ error: "Layer blob missing" }, { status: 404, statusText: "Not Found" })
            : blobResponse("SINGLE-MAP-ZIP"),
    );
    t.after(() => server.restore());

    // No open map -> no-op
    await exportInspectorBattlemap();
    assert.equal(server.calls.length, 0);

    state.libraryDrawerMap = { id: "keep_01" };
    await exportInspectorBattlemap();

    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].path, "/api/curated/keep_01/export");
    assert.equal(h.clickedDownloads.length, 1);
    assert.equal(h.clickedDownloads[0].download, "keep_01.zip");
    assert.equal(state.isExporting, false);
    assert.equal(dom.btnInspectorExport.disabled, false);
    assert.equal(dom.btnInspectorExportLabel.textContent, "Export Battlemap");

    // Error path still unlocks export buttons
    failNext = true;
    await exportInspectorBattlemap();
    assert.equal(state.isExporting, false);
    assert.equal(dom.btnInspectorExport.disabled, false);
    assert.equal(dom.btnInspectorExportLabel.textContent, "Export Battlemap");
    assert.equal(h.clickedDownloads.length, 1, "no download triggered on failure");
});

test("fullscreen stage HUD exportFullscreenBattlemap downloads the active map", async (t) => {
    const h = installExportHarness(t);
    const server = stubFetch(async () => blobResponse("FS-MAP-ZIP"));
    t.after(() => server.restore());

    // No active fullscreen map -> no-op
    await exportFullscreenBattlemap();
    assert.equal(server.calls.length, 0);

    state.libraryFullscreenMap = { id: "tower_floor_2" };
    await exportFullscreenBattlemap();

    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].path, "/api/curated/tower_floor_2/export");
    assert.equal(h.clickedDownloads.length, 1);
    assert.equal(h.clickedDownloads[0].download, "tower_floor_2.zip");
    assert.equal(state.isExporting, false);
    assert.equal(dom.btnFsExport.disabled, false);
    assert.equal(dom.btnFsExportLabel.textContent, "Export");
});
