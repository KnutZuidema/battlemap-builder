/**
 * Progressive library rendering contract (Issue #285): the grid paints one bounded batch, the
 * sentinel appends the next one, filter/search changes restart the batch, and targeting a card
 * beyond the batch renders through it before focus or an overlay acts on it.
 *
 * The test drives the exported render entry points on a small DOM stand-in. jsdom is not used
 * anywhere in this harness, so the fake models exactly the members `library.js` touches.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import {
    LIBRARY_RENDER_BATCH,
    LIBRARY_SEARCH_DEBOUNCE_MS,
    applyLibraryFilters,
    appendLibraryBatch,
    clearLibraryAuditFilter,
    fetchLibraryMaps,
    renderLibraryThrough,
    setLibraryFocus,
    setupLibraryControls,
    switchView,
} from "../assets/views/library.js";

class FakeNode {
    constructor(tag = "div") {
        this.tagName = String(tag).toUpperCase();
        this.children = [];
        this.dataset = {};
        this.style = {};
        this.textContent = "";
        this.title = "";
        this.value = "";
        this.hidden = false;
        this.attributes = {};
        this.listeners = new Map();
        this.parentElement = null;
        this._className = "";
        this._classes = new Set();
        this._innerHTML = "";

        const classes = this._classes;
        this.classList = {
            add: (...names) => names.forEach((name) => classes.add(name)),
            remove: (...names) => names.forEach((name) => classes.delete(name)),
            toggle: (name, force) => {
                const on = force === undefined ? !classes.has(name) : Boolean(force);
                if (on) classes.add(name);
                else classes.delete(name);
                return on;
            },
            contains: (name) => classes.has(name),
        };
    }

    set className(value) {
        this._className = String(value);
        this._classes.clear();
        for (const name of String(value).split(/\s+/)) {
            if (name) this._classes.add(name);
        }
    }

    get className() {
        return this._className;
    }

    set innerHTML(value) {
        this._innerHTML = String(value);
        if (value === "") this.children.length = 0;
    }

    get innerHTML() {
        return this._innerHTML;
    }

    appendChild(child) {
        if (child.tagName === "#FRAGMENT") {
            const moved = [...child.children];
            child.children.length = 0;
            for (const node of moved) this.appendChild(node);
            return child;
        }
        if (child.parentElement) child.parentElement.removeChild(child);
        child.parentElement = this;
        this.children.push(child);
        return child;
    }

    removeChild(child) {
        const index = this.children.indexOf(child);
        if (index >= 0) this.children.splice(index, 1);
        child.parentElement = null;
        return child;
    }

    remove() {
        this.parentElement?.removeChild(this);
    }

    addEventListener(type, handler) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(handler);
    }

    removeEventListener() {}

    dispatchEvent(event) {
        for (const handler of this.listeners.get(event.type) || []) handler(event);
        return true;
    }

    setAttribute(name, value) {
        this.attributes[name] = String(value);
    }

    getAttribute(name) {
        return this.attributes[name] ?? null;
    }

    removeAttribute(name) {
        delete this.attributes[name];
    }

    insertAdjacentElement() {}

    querySelector() {
        return new FakeNode("div");
    }

    querySelectorAll(selector) {
        if (selector === ".curated-card") {
            return this.children.filter((child) => child.classList.contains("curated-card"));
        }
        return [];
    }

    closest() {
        return null;
    }

    focus() {}

    scrollIntoView() {}

    getBoundingClientRect() {
        return { left: 0, top: 0, width: 800, height: 600 };
    }
}

function makeMaps(count) {
    return Array.from({ length: count }, (_, index) => ({
        id: `map-${String(index).padStart(3, "0")}`,
        tags: ["dungeon"],
        description: `Battlemap ${index}`,
        dimensions: [512, 512],
        curated_at: new Date(2026, 0, 1, 0, 0, index).toISOString(),
        layers: [
            { index: 0, role: "ground", elevation: 0, filename: "0_ground.png", dimensions: [512, 512] },
        ],
    }));
}

let intersectionObservers = [];

class FakeIntersectionObserver {
    constructor(callback, options) {
        this.callback = callback;
        this.options = options;
        intersectionObservers.push(this);
    }

    observe(node) {
        this.node = node;
    }

    disconnect() {
        this.disconnected = true;
    }
}

function installDom() {
    globalThis.IntersectionObserver = FakeIntersectionObserver;
    intersectionObservers = [];
    globalThis.document = {
        activeElement: { tagName: "BODY" },
        getElementById: () => null,
        createElement: (tag) => new FakeNode(tag),
        createDocumentFragment: () => new FakeNode("#fragment"),
        querySelector: () => null,
        querySelectorAll: () => [],
        addEventListener() {},
    };
    globalThis.window = { addEventListener() {} };

    dom.libraryGrid = new FakeNode("div");
    dom.libraryEmpty = new FakeNode("div");
    dom.libraryFilterEmpty = new FakeNode("div");
    dom.libraryCountBadge = new FakeNode("span");
    dom.libraryTagPills = new FakeNode("div");
    dom.librarySearchInput = new FakeNode("input");
}

function resetState(maps) {
    state.currentView = "library";
    state.curatedMaps = maps;
    state.libraryActiveTag = null;
    state.librarySearchQuery = "";
    state.libraryFacet = "all";
    state.librarySort = "date-desc";
    state.libraryFocusedIndex = -1;
    state.librarySelectedIds = new Set();
    state.libraryFilteredMaps = [];
    state.libraryAuditFilter = null;
}

function mount(maps) {
    installDom();
    resetState(maps);
}

function cards() {
    return dom.libraryGrid.children.filter((child) => child.classList.contains("curated-card"));
}

function sentinel() {
    return dom.libraryGrid.children.find((child) => child.classList.contains("library-grid-sentinel")) || null;
}

test("the first library render is bounded to one batch", () => {
    mount(makeMaps(137));

    applyLibraryFilters();

    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "only the first batch is built");
    assert.ok(sentinel(), "a sentinel marks the end of the rendered batch");
    assert.equal(
        cards()[0].dataset.id,
        state.libraryFilteredMaps[0].id,
        "the batch starts at the first filtered map",
    );
});

test("appending the next batch extends the grid and drops the sentinel at the end", () => {
    mount(makeMaps(90));
    applyLibraryFilters();

    assert.equal(appendLibraryBatch(), LIBRARY_RENDER_BATCH);
    assert.equal(cards().length, 80);
    assert.ok(sentinel());

    assert.equal(appendLibraryBatch(), 10);
    assert.equal(cards().length, 90);
    assert.equal(sentinel(), null, "nothing left to append, so the sentinel is torn down");
    assert.equal(appendLibraryBatch(), 0);
});

test("the sentinel observer appends a batch when it enters the viewport", () => {
    mount(makeMaps(80));
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH);

    const current = intersectionObservers[intersectionObservers.length - 1];
    assert.equal(current.node, sentinel());
    current.callback([{ isIntersecting: true }]);

    assert.equal(cards().length, 80);
    assert.equal(sentinel(), null);
});

test("a search, tag or sort change resets the rendered batch", () => {
    mount(makeMaps(120));
    applyLibraryFilters();
    appendLibraryBatch();
    appendLibraryBatch();
    assert.equal(cards().length, 120);

    state.librarySearchQuery = "battlemap";
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "a search starts from the first batch");

    appendLibraryBatch();
    state.libraryActiveTag = "dungeon";
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "a tag filter starts from the first batch");

    appendLibraryBatch();
    state.librarySort = "id-asc";
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "a sort change starts from the first batch");
});

test("a readiness audit filter change resets the rendered batch", () => {
    mount(makeMaps(120));
    applyLibraryFilters();
    appendLibraryBatch();
    assert.equal(cards().length, 80);

    // A strict subset: dropping the audit predicate would keep all 120 maps and leave this
    // test at 40 → 80 → 40, so the narrowed deck and the 70-card append are the assertion.
    state.libraryAuditFilter = {
        code: "physical_description_missing",
        ids: new Set(state.curatedMaps.slice(0, 70).map((map) => map.id)),
    };
    applyLibraryFilters();
    assert.equal(state.libraryFilteredMaps.length, 70, "the audit filter narrows the deck");
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "showing the audit filter starts from the first batch");

    appendLibraryBatch();
    assert.equal(cards().length, 70, "appending stops at the narrowed deck");
    clearLibraryAuditFilter();
    assert.equal(state.libraryAuditFilter, null);
    assert.equal(state.libraryFilteredMaps.length, 120, "clearing the audit filter restores the deck");
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "clearing the audit filter starts from the first batch");
});

test("targeting an index past the batch renders through it before acting", () => {
    mount(makeMaps(200));
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH);

    renderLibraryThrough(119);
    assert.equal(cards().length, 120, "the target card and every card before it are rendered");
    assert.ok(cards().some((card) => card.dataset.id === state.libraryFilteredMaps[119].id));

    setLibraryFocus(170, false);
    assert.equal(cards().length, 171);
    assert.equal(state.libraryFocusedIndex, 170);

    const focused = cards().filter((card) => card.classList.contains("focused"));
    assert.equal(focused.length, 1, "exactly the targeted card is focused");
    assert.equal(focused[0].dataset.id, state.libraryFilteredMaps[170].id);
});

test("the search input is debounced, applies the final query, and never fires after leaving", (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });

    mount(makeMaps(120));
    applyLibraryFilters();
    setupLibraryControls();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH);
    appendLibraryBatch();
    assert.equal(cards().length, 80);

    const built = cards()[0];
    dom.librarySearchInput.dispatchEvent({ type: "input", target: { value: "battlemap" } });

    assert.equal(state.librarySearchQuery, "", "a keystroke does not touch the committed query");
    assert.equal(cards()[0], built, "a keystroke does not rebuild the grid");

    t.mock.timers.tick(LIBRARY_SEARCH_DEBOUNCE_MS - 1);
    assert.equal(cards()[0], built, "the grid still waits for the pause");

    t.mock.timers.tick(1);
    assert.equal(state.librarySearchQuery, "battlemap");
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "the applied query restarts the batch");

    // A query scheduled just before the view closes is committed, but the grid is not rebuilt.
    const rebuilt = cards()[0];
    dom.librarySearchInput.dispatchEvent({ type: "input", target: { value: "battlemap 2" } });
    switchView("triage");

    assert.equal(state.librarySearchQuery, "battlemap 2", "the final query is not swallowed");
    t.mock.timers.tick(LIBRARY_SEARCH_DEBOUNCE_MS * 2);
    assert.equal(cards()[0], rebuilt, "no render fires after the library view closed");
});

test("the sentinel and observer survive a failed refresh on view re-entry", async () => {
    mount(makeMaps(120));
    applyLibraryFilters();
    assert.equal(cards().length, LIBRARY_RENDER_BATCH);
    assert.ok(sentinel());
    const observer = intersectionObservers[intersectionObservers.length - 1];
    assert.equal(observer.node, sentinel());

    const originalFetch = globalThis.fetch;
    globalThis.fetch = async () => ({
        ok: false,
        status: 503,
        statusText: "Service Unavailable",
        json: async () => ({ error: "curated listing unavailable" }),
        text: async () => JSON.stringify({ error: "curated listing unavailable" }),
    });

    try {
        switchView("triage");
        assert.ok(sentinel(), "leaving the library keeps the sentinel");
        switchView("library");
        // switchView fires the refresh without awaiting it; await the same failure explicitly.
        await fetchLibraryMaps();
        await new Promise((resolve) => setImmediate(resolve));
    } finally {
        globalThis.fetch = originalFetch;
    }

    assert.equal(state.currentView, "library");
    assert.equal(cards().length, LIBRARY_RENDER_BATCH, "the failed refresh keeps the rendered batch");
    assert.equal(sentinel(), observer.node, "the failed refresh keeps the sentinel in place");
    assert.equal(observer.disconnected, undefined, "the observer was never disconnected");

    observer.callback([{ isIntersecting: true }]);
    assert.equal(cards().length, 80, "the retained observer still appends the next batch");
    assert.ok(sentinel(), "a fresh sentinel is placed for the remaining cards");
});

test("a refresh preserves a focus beyond the first batch", async () => {
    mount(makeMaps(120));
    applyLibraryFilters();
    setLibraryFocus(85, false);
    assert.equal(cards().length, 86);

    const originalFetch = globalThis.fetch;
    globalThis.fetch = async (path) => {
        const url = String(path);
        let body = "[]";
        if (url === "/api/curated") {
            body = JSON.stringify(state.curatedMaps);
        } else if (url.includes("/freeze/status")) {
            body = JSON.stringify({ in_progress: false });
        }
        return { ok: true, status: 200, statusText: "OK", text: async () => body };
    };

    try {
        await fetchLibraryMaps();
        // Let the un-awaited snapshots shelf requests settle under the stub as well.
        await new Promise((resolve) => setImmediate(resolve));
    } finally {
        globalThis.fetch = originalFetch;
    }

    assert.equal(state.libraryFocusedIndex, 85);
    assert.equal(cards().length, 86, "the refresh renders through the focused target again");
    const focused = cards().filter((card) => card.classList.contains("focused"));
    assert.equal(focused.length, 1, "exactly the preserved target is focused");
    assert.equal(focused[0].dataset.id, state.libraryFilteredMaps[85].id);
});
