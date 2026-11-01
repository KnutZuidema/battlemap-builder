/**
 * Library card Thumbnail regression: cards load the derived Thumbnail route, and an edited
 * map's card gets a per-edit `?v=` cache-buster while an unedited map keeps the plain URL.
 *
 * The test drives the exported filter/render entry point on stub DOM nodes, so it observes the
 * URL the card actually emits instead of reaching into the module's private state.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import { applyLibraryFilters } from "../assets/views/library.js";

function fakeClassList() {
    const names = new Set();
    return {
        add: (...next) => next.forEach((name) => names.add(name)),
        remove: (...next) => next.forEach((name) => names.delete(name)),
        toggle: (name, force) => {
            const on = force === undefined ? !names.has(name) : Boolean(force);
            if (on) names.add(name);
            else names.delete(name);
            return on;
        },
        contains: (name) => names.has(name),
    };
}

function fakeElement() {
    return {
        className: "",
        innerHTML: "",
        textContent: "",
        title: "",
        value: "",
        hidden: false,
        dataset: {},
        style: {},
        children: [],
        classList: fakeClassList(),
        appendChild(child) {
            this.children.push(child);
        },
        addEventListener() {},
        removeEventListener() {},
        setAttribute() {},
        getAttribute: () => null,
        removeAttribute() {},
        insertAdjacentElement() {},
        focus() {},
        scrollIntoView() {},
        closest: () => null,
        querySelector: () => fakeElement(),
        querySelectorAll: () => [],
    };
}

globalThis.document = {
    getElementById: () => null,
    querySelectorAll: () => [],
    createElement: () => fakeElement(),
    createDocumentFragment: () => fakeElement(),
    activeElement: { tagName: "BODY", blur() {} },
};
globalThis.window = { addEventListener() {} };

function mountLibraryDom() {
    dom.libraryGrid = fakeElement();
    dom.libraryEmpty = fakeElement();
    dom.libraryFilterEmpty = fakeElement();
    dom.libraryCountBadge = fakeElement();
    dom.libraryTagPills = fakeElement();
}

/** The card node rendered for the single fixture map. */
function renderedCard() {
    const fragments = dom.libraryGrid.children;
    const fragment = fragments[fragments.length - 1];
    return fragment.children[fragment.children.length - 1];
}

function twoLayerMap(description = "Original description") {
    return {
        id: "thumb-card",
        tags: ["dungeon"],
        description,
        dimensions: [512, 512],
        layers: [
            { index: 0, role: "ground", elevation: 0, filename: "0_ground.png", dimensions: [512, 512] },
            { index: 1, role: "upper_floor", elevation: 1, filename: "1_upper_1.png", dimensions: [512, 512] },
        ],
    };
}

test("an edited map's card emits a cache-busted Thumbnail URL, an untouched card does not", () => {
    mountLibraryDom();
    state.libraryActiveTag = null;
    state.librarySearchQuery = "";
    state.libraryFacet = "all";
    state.curatedMaps = [twoLayerMap()];

    applyLibraryFilters();
    assert.match(
        renderedCard().innerHTML,
        /src="\/api\/thumb\/curated\/thumb-card"/,
        "a fresh card loads the derived Thumbnail route",
    );
    assert.doesNotMatch(
        renderedCard().innerHTML,
        /\?v=/,
        "an untouched card keeps the plain URL",
    );

    // Filtering or sorting re-renders the same map object; that is not an edit.
    applyLibraryFilters();
    assert.doesNotMatch(
        renderedCard().innerHTML,
        /\?v=/,
        "a re-render of the same map object must not bust the cache",
    );

    // Every edit path replaces the map object in `state.curatedMaps` before re-rendering.
    state.curatedMaps[0] = { ...state.curatedMaps[0], description: "Edited description" };
    applyLibraryFilters();
    assert.match(
        renderedCard().innerHTML,
        /src="\/api\/thumb\/curated\/thumb-card\?v=\d+"/,
        "the edited card refetches its Thumbnail instead of the browser-cached JPEG",
    );
});
