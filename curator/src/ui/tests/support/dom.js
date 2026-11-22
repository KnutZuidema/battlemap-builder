/**
 * A tiny DOM for the render-target tests: element, text and fragment nodes carrying exactly
 * the members a view's render target and the node helper use.
 *
 * It is deliberately not a DOM implementation — no HTML parsing, no selectors — and every
 * element refuses `innerHTML`. A render target that builds markup instead of nodes throws here
 * rather than passing a string comparison.
 */

function textNode(value) {
    let text = String(value);
    return {
        nodeType: 3,
        parentNode: null,
        childNodes: [],
        get children() {
            return [];
        },
        get firstChild() {
            return null;
        },
        get lastChild() {
            return null;
        },
        get textContent() {
            return text;
        },
        set textContent(next) {
            text = String(next);
        },
    };
}

function textOf(node) {
    return node.nodeType === 3 ? node.textContent : node.childNodes.map(textOf).join("");
}

function elementChildren(node) {
    return node.childNodes.filter((child) => child.nodeType === 1);
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
    if (child.nodeType === 11) {
        for (const grandchild of child.childNodes.slice()) insertBefore(parent, grandchild, reference);
        return child;
    }
    if (child.parentNode) child.parentNode.removeChild(child);
    const index = parent.childNodes.indexOf(reference);
    if (index === -1) throw new Error("insertBefore: the reference is not a child of this node");
    child.parentNode = parent;
    parent.childNodes.splice(index, 0, child);
    return child;
}

function removeChild(parent, child) {
    const index = parent.childNodes.indexOf(child);
    if (index === -1) throw new Error("removeChild: the node is not a child of this node");
    parent.childNodes.splice(index, 1);
    child.parentNode = null;
    return child;
}

function replaceWithText(node, value) {
    for (const child of node.childNodes) child.parentNode = null;
    node.childNodes.length = 0;
    const text = value === null || value === undefined ? "" : String(value);
    if (text === "") return;
    const child = textNode(text);
    child.parentNode = node;
    node.childNodes.push(child);
}

function replaceChildren(node, children) {
    for (const child of node.childNodes) child.parentNode = null;
    node.childNodes.length = 0;
    for (const child of children) appendChild(node, child);
}

function createElement(tagName, document) {
    const attributes = new Map();
    const listeners = new Map();

    const readClasses = () => (attributes.get("class") || "").split(/\s+/).filter(Boolean);
    const writeClasses = (names) => attributes.set("class", names.join(" "));

    const node = {
        nodeType: 1,
        tagName: String(tagName).toUpperCase(),
        ownerDocument: document,
        parentNode: null,
        childNodes: [],
        style: {},
        scrollTop: 0,

        get parentElement() {
            return this.parentNode;
        },
        get children() {
            return elementChildren(this);
        },
        get firstChild() {
            return this.childNodes[0] || null;
        },
        get lastChild() {
            return this.childNodes[this.childNodes.length - 1] || null;
        },
        get textContent() {
            return this.childNodes.map(textOf).join("");
        },
        set textContent(value) {
            replaceWithText(this, value);
        },

        get className() {
            return attributes.get("class") || "";
        },
        set className(value) {
            attributes.set("class", String(value));
        },

        get innerHTML() {
            return "";
        },
        set innerHTML(_value) {
            document.innerHTMLWrites += 1;
            throw new Error(
                `innerHTML assigned on <${String(tagName).toLowerCase()}>: render targets build nodes, never markup`,
            );
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
        replaceChildren(...children) {
            replaceChildren(this, children);
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
        hasAttribute(name) {
            return attributes.has(String(name));
        },
        removeAttribute(name) {
            attributes.delete(String(name));
        },

        addEventListener(type, listener) {
            const key = String(type);
            if (!listeners.has(key)) listeners.set(key, []);
            listeners.get(key).push(listener);
        },
        removeEventListener(type, listener) {
            const registered = listeners.get(String(type)) || [];
            const index = registered.indexOf(listener);
            if (index >= 0) registered.splice(index, 1);
        },
        dispatchEvent(event) {
            const dispatched = event && typeof event === "object" ? event : { type: String(event) };
            if (dispatched.target === undefined) dispatched.target = node;
            for (const listener of (listeners.get(String(dispatched.type || "")) || []).slice()) {
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

        querySelector() {
            return null;
        },
        querySelectorAll() {
            return [];
        },

        classList: {
            add(...names) {
                const current = readClasses();
                for (const name of names) if (!current.includes(name)) current.push(name);
                writeClasses(current);
            },
            remove(...names) {
                writeClasses(readClasses().filter((name) => !names.includes(name)));
            },
            toggle(name, force) {
                const on = force === undefined ? !readClasses().includes(name) : Boolean(force);
                if (on) node.classList.add(name);
                else node.classList.remove(name);
                return on;
            },
            contains(name) {
                return readClasses().includes(name);
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
            return elementChildren(this);
        },
        get firstChild() {
            return this.childNodes[0] || null;
        },
        get lastChild() {
            return this.childNodes[this.childNodes.length - 1] || null;
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
        replaceChildren(...children) {
            replaceChildren(this, children);
        },
    };
}

/** A fresh document with the node factories a render target needs and an innerHTML tripwire. */
export function createDocument() {
    const byId = new Map();
    const document = {
        readyState: "complete",
        activeElement: null,
        /** How many times a test's code path assigned `innerHTML` (each assignment throws). */
        innerHTMLWrites: 0,
        createElement(tagName) {
            return createElement(tagName, document);
        },
        createTextNode(value) {
            return textNode(value);
        },
        createDocumentFragment() {
            return createFragment(document);
        },
        getElementById(id) {
            const key = String(id);
            let found = byId.get(key);
            if (!found) {
                found = createElement("div", document);
                byId.set(key, found);
            }
            return found;
        },
        querySelector() {
            return null;
        },
        querySelectorAll() {
            return [];
        },
        addEventListener() {},
        removeEventListener() {},
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

/** Every node below `root`, `root` included, in document order. */
export function allNodes(root) {
    const found = [];
    const walk = (node) => {
        found.push(node);
        for (const child of node.childNodes || []) walk(child);
    };
    walk(root);
    return found;
}

/** Every element below `root` carrying `name` in its class list, in document order. */
export function allByClass(root, name) {
    return allNodes(root).filter((node) => node.nodeType === 1 && node.classList.contains(name));
}

/** The first element below `root` carrying `name` in its class list, or `null`. */
export function byClass(root, name) {
    return allByClass(root, name)[0] || null;
}

/** The first element below `root` with `tagName`, or `null`. */
export function byTag(root, tagName) {
    const wanted = String(tagName).toUpperCase();
    return allNodes(root).find((node) => node.nodeType === 1 && node.tagName === wanted) || null;
}

/** Whether `node` is `root` or sits inside it: the "still on screen" check. */
export function contains(root, node) {
    return node !== null && node !== undefined && allNodes(root).includes(node);
}
