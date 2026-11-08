/**
 * The node helper: elements and fragments built from DOM construction only.
 *
 * Every assertion reads the shim's node tree, so a helper that interpolated a child into markup
 * would fail here rather than pass on a string comparison. The shim refuses `innerHTML`, which
 * makes accepting user text without escaping the only way these tests can pass.
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

test("h builds one element whose string props become attributes", (t) => {
    withDom(t);

    const element = h("button", {
        type: "button",
        class: "btn btn-danger btn-xs btn-delete-snapshot",
        title: 'Delete "snapshot" <one>',
        "aria-label": "Delete Snapshot",
        "data-id": "snap-1",
    });

    assert.equal(element.tagName, "BUTTON");
    assert.equal(element.getAttribute("type"), "button");
    assert.match(element.getAttribute("class"), /\bbtn-delete-snapshot\b/);
    assert.equal(element.getAttribute("data-id"), "snap-1");
    assert.equal(
        element.getAttribute("title"),
        'Delete "snapshot" <one>',
        "an attribute value is never escaped or re-parsed",
    );
    assert.equal(element.childNodes.length, 0, "props are not children");
});

test("h nests the element children it is given", (t) => {
    withDom(t);

    const element = h("div", { class: "outer" }, h("span", { class: "inner" }, "hi"));

    assert.equal(element.children.length, 1);
    assert.equal(element.children[0].tagName, "SPAN");
    assert.equal(element.children[0].getAttribute("class"), "inner");
    assert.equal(element.textContent, "hi");
});

test("a text child becomes a text node, never markup", (t) => {
    const ui = withDom(t);

    const element = h("p", null, "<script>alert(1)</script>", " & ", 5);

    assert.equal(element.textContent, "<script>alert(1)</script> & 5");
    assert.equal(element.children.length, 0, "no element is created from the string");
    assert.equal(element.childNodes.length, 3);
    assert.equal(element.childNodes[0].nodeType, 3);
    assert.equal(ui.document.innerHTMLWrites, 0, "nothing is parsed as HTML");
});

test("h registers on* props as listeners for that event only", (t) => {
    withDom(t);

    let clicks = 0;
    const element = h("button", { onclick: () => clicks++ }, "go");

    element.dispatchEvent({ type: "click" });
    assert.equal(clicks, 1);

    element.dispatchEvent({ type: "change" });
    assert.equal(clicks, 1, "only the named event is bound");
});

test("h keeps node children by identity, flattens arrays and skips nullish children", (t) => {
    withDom(t);

    const span = h("span", null, "x");
    const element = h("div", null, [span, [h("b", null, "y")]], "tail", null, undefined, false);

    assert.equal(element.children[0], span, "a node child is appended as it is, not copied");
    assert.equal(element.children.length, 2);
    assert.equal(element.textContent, "xytail");
});

test("h treats an omitted props argument as no props", (t) => {
    withDom(t);

    const element = h("div");

    assert.equal(element.tagName, "DIV");
    assert.equal(element.childNodes.length, 0);
    assert.equal(element.getAttribute("class"), null);
});

test("fragment builds a DocumentFragment with the same child rules", (t) => {
    const ui = withDom(t);

    const span = h("span", null, "b");
    const node = fragment("a", null, span, ["c"], 1);

    assert.equal(node.nodeType, 11);
    assert.equal(node.textContent, "abc1");
    assert.equal(node.childNodes.length, 4);
    assert.equal(node.children[0], span);

    const host = ui.document.createElement("div");
    host.appendChild(node);
    assert.equal(host.childNodes.length, 4, "appending the fragment moves its children into place");
    assert.equal(node.childNodes.length, 0);
    assert.equal(host.textContent, "abc1");
});

test("no helper path assigns innerHTML, even for hostile interpolated text", (t) => {
    const ui = withDom(t);

    const tree = h(
        "div",
        { class: "card", "data-id": "</div><img src=x onerror=alert(1)>" },
        h("span", null, "<img src=x>"),
        // A node child of a fragment keeps its identity; the hostile string is that node's text.
        fragment(h("b", null, "<b>")),
    );

    assert.equal(ui.document.innerHTMLWrites, 0);
    assert.equal(tree.children.length, 2);
    assert.equal(tree.children[1].tagName, "B");
    assert.equal(tree.children[1].textContent, "<b>");
});
