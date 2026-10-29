/**
 * `kit/html.js`: node construction without HTML strings.
 *
 * Every assertion reads the shim's node tree, so a helper that interpolated a child into
 * markup would fail here rather than pass on a string comparison.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { fragment, h } from "../assets/kit/html.js";
import { installDom } from "./support/dom.js";

function withDom(t) {
    const ui = installDom();
    t.after(() => ui.restore());
    return ui;
}

test("h nests elements", (t) => {
    withDom(t);

    const element = h("div", { class: "outer" }, h("span", { class: "inner" }, "hi"));

    assert.equal(element.tagName, "DIV");
    assert.equal(element.getAttribute("class"), "outer");
    assert.equal(element.children.length, 1);
    assert.equal(element.children[0].tagName, "SPAN");
    assert.equal(element.children[0].getAttribute("class"), "inner");
    assert.equal(element.textContent, "hi");
});

test("h string props become attributes", (t) => {
    withDom(t);

    const element = h("button", {
        type: "button",
        title: "Launch",
        role: "button",
        "aria-label": "Train LoRA",
        "data-id": "snap-1",
        disabled: true,
    });

    assert.equal(element.getAttribute("type"), "button");
    assert.equal(element.getAttribute("title"), "Launch");
    assert.equal(element.getAttribute("role"), "button");
    assert.equal(element.getAttribute("aria-label"), "Train LoRA");
    assert.equal(element.getAttribute("data-id"), "snap-1");
    assert.equal(element.getAttribute("disabled"), "");
});

test("h dataset and style objects write data attributes and inline styles", (t) => {
    withDom(t);

    const element = h("div", { dataset: { id: "snap-1", role: "shelf" }, style: { width: "40%" } });

    assert.equal(element.getAttribute("data-id"), "snap-1");
    assert.equal(element.getAttribute("data-role"), "shelf");
    assert.equal(element.style.width, "40%");
});

test("h registers on* props as event listeners", (t) => {
    withDom(t);

    let clicks = 0;
    const element = h("button", { onclick: () => clicks++ }, "go");

    element.dispatchEvent({ type: "click" });
    assert.equal(clicks, 1);

    element.dispatchEvent({ type: "change" });
    assert.equal(clicks, 1);
});

test("interpolated text lands as text, never as markup", (t) => {
    withDom(t);

    const element = h("p", null, "<script>alert(1)</script>", " & ", 5);

    assert.equal(element.textContent, "<script>alert(1)</script> & 5");
    assert.equal(element.children.length, 0, "no element is created from the string");
    assert.equal(element.childNodes.length, 3);
    assert.equal(element.childNodes[0].nodeType, 3);
});

test("h appends node children, flattens arrays and keeps node identity", (t) => {
    withDom(t);

    const span = h("span", null, "x");
    const element = h("div", null, [span, [h("b", null, "y")]], "tail");

    assert.equal(element.children[0], span);
    assert.equal(element.children.length, 2);
    assert.equal(element.textContent, "xytail");
});

test("h skips null, undefined and false children", (t) => {
    withDom(t);

    const element = h("p", null, null, undefined, false, "kept");

    assert.equal(element.childNodes.length, 1);
    assert.equal(element.textContent, "kept");
});

test("h treats an omitted props argument as no props", (t) => {
    withDom(t);

    const element = h("div");
    assert.equal(element.attributes.size, 0);
    assert.equal(element.childNodes.length, 0);
});

test("fragment builds a DocumentFragment with the same child rules", (t) => {
    withDom(t);

    const span = h("span", null, "b");
    const node = fragment("a", null, span, ["c"], 1);

    assert.equal(node.nodeType, 11);
    assert.equal(node.textContent, "abc1");
    assert.equal(node.children[0], span);
    assert.equal(node.childNodes.length, 4);
});
