/**
 * `views/snapshots.js` render target: the shelf renders from its snapshot state, keyed by
 * id, without replacing surviving nodes.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { renderDatasetSnapshots } from "../assets/views/snapshots.js";
import { installDom } from "./support/dom.js";

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

    t.after(() => {
        delete dom.snapshotsShelf;
        delete dom.snapshotsCountBadge;
        ui.restore();
    });

    return {
        document: ui.document,
        shelf: dom.snapshotsShelf,
        badge: dom.snapshotsCountBadge,
        list: () => dom.snapshotsShelf.children[0],
        empty: () => dom.snapshotsShelf.children[1],
        cards: () => dom.snapshotsShelf.children[0].children,
    };
}

/** The nodes a shelf test writes assertions against, by the card's layout contract. */
function parts(card) {
    return {
        id: card.children[0].children[0],
        theme: card.children[0].children[1],
        sha: card.children[1].children[0],
        counts: card.children[1].children[1],
        size: card.children[1].children[2],
        time: card.children[2].children[0],
        train: card.children[3].children[0],
        download: card.children[3].children[1],
        remove: card.children[3].children[2],
    };
}

test("the first render builds one card per snapshot with its text", (t) => {
    const ui = setUp(t);
    const date = new Date("2026-10-01T12:00:00Z").toLocaleString();

    renderDatasetSnapshots([
        snapshot(),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    assert.equal(ui.badge.textContent, "2 Snapshots");
    assert.equal(ui.shelf.children.length, 2, "the view owns the list and the empty notice");
    assert.equal(ui.list().getAttribute("class"), "snapshots-list");
    assert.equal(ui.empty().getAttribute("class"), "snapshots-empty hidden");
    assert.equal(ui.cards().length, 2);

    const first = parts(ui.cards()[0]);
    assert.equal(ui.cards()[0].getAttribute("data-id"), "snap-a");
    assert.equal(first.id.textContent, "snap-a");
    assert.equal(first.id.getAttribute("title"), "snap-a");
    assert.equal(first.theme.textContent, "crypt");
    assert.equal(first.sha.textContent, "SHA: abcdef012345…");
    assert.equal(first.sha.getAttribute("title"), "SHA-256: abcdef0123456789abcdef");
    assert.equal(first.counts.textContent, "12 samples (3 maps)");
    assert.equal(first.size.textContent, "2.0 KB");
    assert.equal(first.time.textContent, date);
    assert.equal(first.train.textContent, "Train LoRA");
    assert.equal(first.train.getAttribute("data-id"), "snap-a");
    assert.equal(first.download.textContent, "Download ZIP");
    assert.equal(first.download.getAttribute("href"), "/api/training/datasets/snap-a/archive");
    assert.equal(first.download.getAttribute("download"), "snap-a.zip");
    assert.equal(first.remove.textContent, "Delete");
    assert.equal(first.remove.getAttribute("data-id"), "snap-a");

    const second = parts(ui.cards()[1]);
    assert.equal(second.id.textContent, "snap-b");
    assert.equal(second.theme.textContent, "All Themes");
    assert.equal(second.counts.textContent, "1 sample (1 map)");
    assert.equal(second.size.textContent, "512 B");
});

test("a re-render updates only the changed card's text and keeps node identity", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([
        snapshot(),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    const firstCard = ui.cards()[0];
    const secondCard = ui.cards()[1];
    const firstCounts = parts(firstCard).counts;
    const secondCounts = parts(secondCard).counts;
    const secondCountsText = secondCounts.firstChild;
    const secondSize = parts(secondCard).size;

    renderDatasetSnapshots([
        snapshot({ samples_count: 13 }),
        snapshot({ id: "snap-b", theme: null, samples_count: 1, maps_count: 1, size_bytes: 512 }),
    ]);

    assert.equal(ui.cards()[0], firstCard, "the surviving card keeps its node");
    assert.equal(ui.cards()[1], secondCard, "the untouched card keeps its node");
    assert.equal(parts(firstCard).counts, firstCounts, "the changed span is written, not replaced");
    assert.equal(firstCounts.textContent, "13 samples (3 maps)");
    assert.equal(secondCounts.firstChild, secondCountsText, "unchanged text is not rewritten");
    assert.equal(parts(secondCard).size, secondSize);
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
    assert.equal(ui.cards()[0].getAttribute("data-id"), "snap-c");
    assert.equal(ui.cards()[1], secondCard, "reordering moves the surviving node, it does not rebuild it");
});

test("a progress re-render keeps scroll position, focus and card identity", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([snapshot(), snapshot({ id: "snap-b", theme: null })]);
    const firstCard = ui.cards()[0];
    const trainButton = parts(firstCard).train;

    ui.list().scrollTop = 240;
    trainButton.focus();

    // What the freeze-progress refresh does: render the same state again.
    renderDatasetSnapshots([snapshot(), snapshot({ id: "snap-b", theme: null })]);

    assert.equal(ui.list().scrollTop, 240);
    assert.equal(ui.document.activeElement, trainButton);
    assert.equal(ui.cards()[0], firstCard);
});

test("snapshot fields land as text, not markup", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([
        snapshot({ id: "snap-a", theme: "<script>alert(1)</script>", sha256: "<b>bad</b>" }),
    ]);

    const card = ui.cards()[0];
    assert.equal(parts(card).theme.textContent, "<script>alert(1)</script>");
    assert.equal(parts(card).theme.children.length, 0);
    assert.equal(parts(card).sha.getAttribute("title"), "SHA-256: <b>bad</b>");
    assert.equal(parts(card).sha.textContent, "SHA: <b>bad</b>…");
});

test("the empty state and count badge follow the list", (t) => {
    const ui = setUp(t);

    renderDatasetSnapshots([]);
    assert.equal(ui.badge.textContent, "0 Snapshots");
    assert.equal(ui.cards().length, 0);
    assert.equal(ui.empty().classList.contains("hidden"), false);

    renderDatasetSnapshots([snapshot()]);
    assert.equal(ui.badge.textContent, "1 Snapshot");
    assert.equal(ui.empty().classList.contains("hidden"), true);
    assert.equal(ui.list().children.length, 1);
});
