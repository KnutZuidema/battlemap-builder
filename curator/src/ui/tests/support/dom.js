/**
 * A tiny DOM for the view unit tests: element, text and fragment nodes with the handful
 * of members `kit/html.js` and a render target use.
 *
 * It is not a DOM implementation — no HTML parsing, no selectors, no layout — and it stays
 * that small on purpose: a test that needs more than this belongs behind a browser harness.
 */

function textNode(text) {
    let value = String(text);
    return {
        nodeType: 3,
        parentNode: null,
        get textContent() {
            return value;
        },
        set textContent(next) {
            value = String(next);
        },
        get childNodes() {
            return [];
        },
        get children() {
            return [];
        },
        get firstChild() {
            return null;
        },
    };
}

function childElements(node) {
    return node.childNodes.filter((child) => child.nodeType === 1);
}

function textOf(node) {
    return node.nodeType === 3 ? node.textContent : node.childNodes.map(textOf).join("");
}

function appendChild(parent, child) {
    if (child.nodeType === 11) {
        for (const grandchild of child.childNodes.slice()) appendChild(parent, grandchild);
        child.childNodes.length = 0;
        return child;
    }
    if (child.parentNode) child.parentNode.removeChild(child);
    child.parentNode = parent;
    parent.childNodes.push(child);
    return child;
}

function insertBefore(parent, child, reference) {
    if (reference === null || reference === undefined) return appendChild(parent, child);
    if (child.parentNode) child.parentNode.removeChild(child);
    const index = parent.childNodes.indexOf(reference);
    if (index === -1) throw new Error("insertBefore: reference is not a child");
    child.parentNode = parent;
    parent.childNodes.splice(index, 0, child);
    return child;
}

function removeChild(parent, child) {
    const index = parent.childNodes.indexOf(child);
    if (index === -1) throw new Error("removeChild: not a child");
    parent.childNodes.splice(index, 1);
    child.parentNode = null;
    return child;
}

function replaceWithText(node, value) {
    const text = value === null || value === undefined ? "" : String(value);
    for (const child of node.childNodes) child.parentNode = null;
    node.childNodes.length = 0;
    if (text === "") return;
    const child = textNode(text);
    child.parentNode = node;
    node.childNodes.push(child);
}

function createElement(tagName, document) {
    const attributes = new Map();
    const listeners = new Map();

    const classNames = () => (attributes.get("class") || "").split(/\s+/).filter(Boolean);
    const writeClassNames = (names) => attributes.set("class", names.join(" "));

    const node = {
        nodeType: 1,
        tagName: String(tagName).toUpperCase(),
        ownerDocument: document,
        parentNode: null,
        childNodes: [],
        attributes,
        style: {},
        className: "",
        value: "",
        checked: false,
        disabled: false,
        scrollTop: 0,

        get parentElement() {
            return this.parentNode;
        },
        get children() {
            return childElements(this);
        },
        get firstChild() {
            return this.childNodes[0] || null;
        },
        get textContent() {
            return this.childNodes.map(textOf).join("");
        },
        set textContent(value) {
            replaceWithText(this, value);
        },

        appendChild(child) {
            return appendChild(this, child);
        },
        insertBefore(child, reference) {
            return insertBefore(this, child, reference);
        },
        removeChild(child) {
            return removeChild(this, child);
        },
        remove() {
            if (this.parentNode) this.parentNode.removeChild(this);
        },

        setAttribute(name, value) {
            attributes.set(String(name), String(value));
        },
        getAttribute(name) {
            const key = String(name);
            return attributes.has(key) ? attributes.get(key) : null;
        },
        removeAttribute(name) {
            attributes.delete(String(name));
        },
        hasAttribute(name) {
            return attributes.has(String(name));
        },

        addEventListener(type, listener) {
            const key = String(type);
            if (!listeners.has(key)) listeners.set(key, []);
            listeners.get(key).push(listener);
        },
        removeEventListener(type, listener) {
            const key = String(type);
            const registered = listeners.get(key) || [];
            const index = registered.indexOf(listener);
            if (index >= 0) registered.splice(index, 1);
        },
        dispatchEvent(event) {
            const dispatched = event && typeof event === "object" ? event : { type: String(event) };
            if (dispatched.target === undefined) dispatched.target = node;
            for (const listener of listeners.get(String(dispatched.type || "")) || []) {
                listener(dispatched);
            }
            return true;
        },

        focus() {
            document.activeElement = node;
        },
        blur() {
            if (document.activeElement === node) document.activeElement = null;
        },

        classList: {
            add(...names) {
                const current = classNames();
                for (const name of names) if (!current.includes(name)) current.push(name);
                writeClassNames(current);
            },
            remove(...names) {
                writeClassNames(classNames().filter((name) => !names.includes(name)));
            },
            toggle(name, force) {
                const on = force === undefined ? !classNames().includes(name) : Boolean(force);
                if (on) node.classList.add(name);
                else node.classList.remove(name);
                return on;
            },
            contains(name) {
                return classNames().includes(name);
            },
        },
    };

    return node;
}

function createFragment(document) {
    return {
        nodeType: 11,
        ownerDocument: document,
        parentNode: null,
        childNodes: [],
        get children() {
            return childElements(this);
        },
        get firstChild() {
            return this.childNodes[0] || null;
        },
        get textContent() {
            return this.childNodes.map(textOf).join("");
        },
        set textContent(value) {
            replaceWithText(this, value);
        },
        appendChild(child) {
            return appendChild(this, child);
        },
        insertBefore(child, reference) {
            return insertBefore(this, child, reference);
        },
        removeChild(child) {
            return removeChild(this, child);
        },
    };
}

/** A fresh document with the three node factories a render target needs. */
export function createDocument() {
    const document = {
        activeElement: null,
        createElement(tagName) {
            return createElement(tagName, document);
        },
        createTextNode(text) {
            return textNode(text);
        },
        createDocumentFragment() {
            return createFragment(document);
        },
    };
    return document;
}

/** Install a fresh document as `globalThis.document`; `restore()` puts the old one back. */
export function installDom() {
    const previous = globalThis.document;
    const document = createDocument();
    globalThis.document = document;
    return {
        document,
        restore() {
            globalThis.document = previous;
        },
    };
}
