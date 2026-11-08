/**
 * The Snapshots shelf render target: the frozen-snapshot shelf renders from its state, keyed by
 * snapshot id, without replacing the nodes the curator is already using.
 *
 * The same state arrives again whenever a freeze finishes or a listing refresh lands, so the
 * assertions here are about what survives that second render: the focused card action, the
 * list's scroll offset, the card and its action nodes, and the download link the browser is
 * mid-navigation on.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { renderDatasetSnapshots } from "../assets/views/snapshots.js";
import { allByClass, byClass, byTag, contains, installDom } from "./support/dom.js";

function snapshot(overrides = {}) {
    return {
        id: "snap-a",
        theme: "crypt",
        sha256: "abcdef0123456789abcdef",
        samples_count: 12,
        maps_count: 3,
        size_bytes: 2048,
        created_at: "2026-10-01T12:00:00Z",
        ...overrides,
    };
}

function setUp(t) {
    const ui = installDom();
    dom.snapshotsShelf = ui.document.createElement("div");
    dom.snapshotsCountBadge = ui.document.createElement("span");
    state.snapshots = [];
    state.isFetchingSnapshots = false;

    t.after(() => {
        delete dom.snapshotsShelf;
        delete dom.snapshotsCountBadge;
        state.snapshots = [];
        state.isFetchingSnapshots = false;
        ui.restore();
    });

    return {
        document: ui.document,
        shelf: dom.snapshotsShelf,
        badge: dom.snapshotsCountBadge,
        list: () => byClass(dom.snapshotsShelf, "snapshots-list"),
        empty: () => byClass(dom.snapshotsShelf, "snapshots-empty"),
        cards: () => allByClass(dom.snapshotsShelf, "snapshot-card"),
    };
}

/** The nodes a shelf card is asserted through, located by the classes its stylesheet uses. */
function parts(card) {
    return {
        id: byClass(card, "snapshot-id"),
        theme: byClass(card, "snapshot-theme-badge"),
        sha: byClass(card, "snapshot-sha256"),
        counts: byClass(card, "snapshot-counts"),
        size: byClass(card, "snapshot-size"),
        time: byClass(card, "snapshot-time"),
        train: byClass(card, "btn-train-snapshot"),
        download: byTag(card, "a"),
        remove: byClass(card, "btn-delete-snapshot"),
    };
}

async function settle() {
    for (let index = 0; index < 4; index += 1) {
        await new Promise((resolve) => setImmediate(resolve));
    }
}

test("the first render builds the list, the empty notice and one card per snapshot", (t) => {
    const ui = setUp(t);
    const date = new Date("2026-10-01T12:00:00Z").toLocaleString();

    renderDatasetSnapshots([
        snapshot(),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    assert.equal(ui.badge.textContent, "2 Snapshots", "the shell badge counts the rendered shelf");
    assert.equal(allByClass(ui.shelf, "snapshots-list").length, 1, "the view owns exactly one list");
    assert.equal(allByClass(ui.shelf, "snapshots-empty").length, 1, "the view owns exactly one empty notice");
    assert.equal(ui.empty().classList.contains("hidden"), true, "a populated shelf hides the empty notice");
    assert.equal(ui.cards().length, 2);

    const first = parts(ui.cards()[0]);
    for (const [name, node] of Object.entries(first)) {
        assert.ok(node, `the card carries its ${name} node`);
    }
    assert.equal(first.id.textContent, "snap-a");
    assert.equal(first.id.getAttribute("title"), "snap-a");
    assert.equal(first.theme.textContent, "crypt");
    assert.equal(first.sha.textContent, "SHA: abcdef012345…");
    assert.equal(first.sha.getAttribute("title"), "SHA-256: abcdef0123456789abcdef");
    assert.equal(first.counts.textContent, "12 samples (3 maps)");
    assert.equal(first.size.textContent, "2.0 KB");
    assert.equal(first.time.textContent, date);
    assert.equal(first.train.textContent, "Train LoRA");
    assert.equal(first.download.textContent, "Download ZIP");
    assert.equal(first.download.getAttribute("href"), "/api/training/datasets/snap-a/archive");
    assert.equal(first.download.getAttribute("download"), "snap-a.zip");
    assert.equal(first.remove.textContent, "Delete");

    const second = parts(ui.cards()[1]);
    assert.equal(second.id.textContent, "snap-b");
    assert.equal(second.theme.textContent, "All Themes");
    assert.equal(second.counts.textContent, "1 sample (1 map)");
    assert.equal(second.size.textContent, "512 B");
});

test("a re-render keeps every surviving card and its action nodes", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([
        snapshot(),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    const list = ui.list();
    const firstCard = ui.cards()[0];
    const secondCard = ui.cards()[1];
    const first = parts(firstCard);
    const second = parts(secondCard);

    renderDatasetSnapshots([
        snapshot({ samples_count: 13 }),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    assert.equal(ui.list(), list, "the list is created once per shelf root, not per render");
    assert.equal(ui.cards()[0], firstCard, "the changed card keeps its node");
    assert.equal(ui.cards()[1], secondCard, "the untouched card keeps its node");
    assert.equal(parts(firstCard).counts, first.counts, "the changed span is written, not replaced");
    assert.equal(first.counts.textContent, "13 samples (3 maps)");

    assert.equal(parts(secondCard).train, second.train, "the train action is not rebuilt");
    assert.equal(parts(secondCard).remove, second.remove, "the delete action is not rebuilt");
    assert.equal(
        parts(secondCard).download,
        second.download,
        "the download link keeps its element, so an in-flight download is not cancelled",
    );
    assert.equal(parts(secondCard).download.firstChild, second.download.firstChild, "its label text survives too");
});

test("a re-render keeps focus and scroll position inside the shelf", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([snapshot(), snapshot({ id: "snap-b", theme: null })]);

    const list = ui.list();
    const firstCard = ui.cards()[0];
    const train = parts(firstCard).train;

    list.scrollTop = 240;
    train.focus();
    assert.equal(ui.document.activeElement, train);

    // What a freeze-progress refresh does: render the same state again.
    renderDatasetSnapshots([snapshot(), snapshot({ id: "snap-b", theme: null })]);

    assert.equal(ui.list().scrollTop, 240, "the shelf list keeps its scroll offset");
    assert.equal(ui.cards()[0], firstCard, "the card keeps its node");
    assert.equal(ui.document.activeElement, train, "the focused card action keeps focus");
    assert.ok(contains(list, ui.document.activeElement), "the focused node is still on screen");
});

test("a removed snapshot loses its card and a new one gains one", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([snapshot(), snapshot({ id: "snap-b", theme: null })]);
    const firstCard = ui.cards()[0];
    const secondCard = ui.cards()[1];

    renderDatasetSnapshots([snapshot({ id: "snap-b", theme: null })]);

    assert.equal(ui.cards().length, 1);
    assert.equal(ui.cards()[0], secondCard);
    assert.equal(firstCard.parentNode, null, "the removed card is detached");
    assert.equal(ui.badge.textContent, "1 Snapshot");

    renderDatasetSnapshots([snapshot({ id: "snap-c" }), snapshot({ id: "snap-b", theme: null })]);

    assert.equal(ui.cards().length, 2);
    assert.equal(parts(ui.cards()[0]).id.textContent, "snap-c");
    assert.equal(ui.cards()[1], secondCard, "reordering moves the surviving node, it does not rebuild it");
});

test("snapshot fields land as text, not markup", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([
        snapshot({ theme: "<script>alert(1)</script>", sha256: "<b>bad</b>" }),
    ]);

    const card = parts(ui.cards()[0]);
    assert.equal(card.theme.textContent, "<script>alert(1)</script>");
    assert.equal(card.theme.children.length, 0, "an interpolated theme is a text node");
    assert.equal(card.sha.getAttribute("title"), "SHA-256: <b>bad</b>");
    assert.equal(card.sha.textContent, "SHA: <b>bad</b>…");
    assert.equal(ui.document.innerHTMLWrites, 0, "the shelf never builds markup");
});

test("the empty state and count badge follow the list", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([]);

    assert.equal(ui.badge.textContent, "0 Snapshots");
    assert.equal(ui.cards().length, 0);
    assert.equal(ui.empty().classList.contains("hidden"), false, "an empty list shows the empty notice");
    assert.match(ui.empty().textContent, /No frozen snapshots for this theme scope yet/);
    assert.equal(byTag(ui.empty(), "strong").textContent, "Freeze Snapshot");

    renderDatasetSnapshots([snapshot()]);

    assert.equal(ui.badge.textContent, "1 Snapshot");
    assert.equal(ui.empty().classList.contains("hidden"), true);
    assert.equal(ui.list().children.length, 1);
});

test("the rendered delete action confirms, deletes and refreshes the shelf", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = setUp(t);

    let remaining = [snapshot()];
    const calls = [];
    const originalFetch = globalThis.fetch;
    const originalConfirm = globalThis.confirm;

    globalThis.fetch = async (path, init) => {
        const method = (init && init.method) || "GET";
        calls.push({ path: String(path), method });
        if (method === "DELETE") {
            remaining = [];
            return {
                ok: true,
                status: 204,
                statusText: "No Content",
                text: async () => "",
                json: async () => {
                    throw new SyntaxError("Unexpected end of JSON input");
                },
            };
        }
        if (String(path).includes("/freeze/status")) {
            return {
                ok: true,
                status: 200,
                statusText: "OK",
                text: async () => JSON.stringify({ in_progress: false }),
            };
        }
        return { ok: true, status: 200, statusText: "OK", text: async () => JSON.stringify(remaining) };
    };
    globalThis.confirm = () => true;

    t.after(() => {
        globalThis.fetch = originalFetch;
        globalThis.confirm = originalConfirm;
    });

    renderDatasetSnapshots([snapshot()]);
    const card = ui.cards()[0];
    const remove = parts(card).remove;
    assert.ok(remove, "the card carries its delete action");

    remove.dispatchEvent({ type: "click", target: remove });
    await settle();

    assert.equal(calls[0].method, "DELETE", "the first request is the delete");
    assert.equal(calls[0].path, "/api/training/datasets/snap-a");
    assert.ok(
        calls.some((call) => call.method === "GET" && call.path.startsWith("/api/training/datasets")),
        "the shelf refreshes its listing after a delete",
    );
    assert.equal(card.parentNode, null, "the deleted snapshot's card is detached");
    assert.equal(ui.cards().length, 0);
    assert.equal(ui.badge.textContent, "0 Snapshots");
});

test("a declined delete confirmation keeps the card and sends nothing", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = setUp(t);

    const calls = [];
    const originalFetch = globalThis.fetch;
    const originalConfirm = globalThis.confirm;
    globalThis.fetch = async (path, init) => {
        calls.push({ path: String(path), method: (init && init.method) || "GET" });
        return { ok: true, status: 200, statusText: "OK", text: async () => "[]" };
    };
    globalThis.confirm = () => false;

    t.after(() => {
        globalThis.fetch = originalFetch;
        globalThis.confirm = originalConfirm;
    });

    renderDatasetSnapshots([snapshot()]);
    const card = ui.cards()[0];
    const remove = parts(card).remove;

    remove.dispatchEvent({ type: "click", target: remove });
    await settle();

    assert.equal(calls.length, 0, "a declined confirmation issues no request");
    assert.equal(ui.cards()[0], card);
    assert.equal(card.parentNode, ui.list());
});
