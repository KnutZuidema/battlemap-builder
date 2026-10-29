/**
 * Test fakes for the request-module tests: a DOM stand-in for the status pill and the toast
 * container, and a fetch stub that records what a client asked for.
 */

import { dom } from "../../assets/dom.js";

/** A detached element with the handful of members the UI writes to. */
export function element(tag = "div") {
    const classes = new Set();
    return {
        tagName: String(tag).toUpperCase(),
        className: "",
        textContent: "",
        title: "",
        value: "",
        innerHTML: "",
        children: [],
        parentElement: null,
        dataset: {},
        style: {},
        width: 0,
        height: 0,
        clientWidth: 800,
        clientHeight: 600,
        classList: {
            add: (...names) => names.forEach((name) => classes.add(name)),
            remove: (...names) => names.forEach((name) => classes.delete(name)),
            toggle(name, force) {
                const on = force === undefined ? !classes.has(name) : Boolean(force);
                if (on) classes.add(name);
                else classes.delete(name);
            },
            contains: (name) => classes.has(name),
        },
        appendChild(child) {
            this.children.push(child);
            child.parentElement = this;
            return child;
        },
        removeChild(child) {
            const index = this.children.indexOf(child);
            if (index >= 0) this.children.splice(index, 1);
            child.parentElement = null;
            return child;
        },
        addEventListener() {},
        removeEventListener() {},
        querySelector: () => null,
        querySelectorAll: () => [],
        setAttribute() {},
        getAttribute: () => null,
        closest: () => null,
        focus() {},
        scrollIntoView() {},
        getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
        getContext: () => null,
    };
}

/**
 * Install `document` plus the status pill the failure policy paints, and hand back the pieces
 * a test asserts on. `restore()` takes it all back down.
 */
export function installRequestDom() {
    const previousDocument = globalThis.document;

    const toastContainer = element("div");
    const byId = new Map([["toast-container", toastContainer]]);
    globalThis.document = {
        readyState: "complete",
        activeElement: null,
        getElementById(id) {
            let found = byId.get(id);
            if (!found) {
                found = element("div");
                byId.set(id, found);
            }
            return found;
        },
        createElement: (tag) => element(tag),
        createDocumentFragment: () => element("#fragment"),
        querySelector: () => null,
        querySelectorAll: () => [],
        addEventListener() {},
    };

    const pill = element("span");
    pill.className = "status-pill status-connected";
    pill.title = "Curator API Online | Staging: 2 | Curated: 5";
    const statusText = element("span");
    statusText.textContent = "Online (2)";
    dom.statusIndicator = pill;
    dom.statusText = statusText;

    return {
        pill,
        statusText,
        toastContainer,
        toastMessages: () => toastContainer.children.map((toast) => toast.children[0]?.textContent ?? ""),
        restore() {
            globalThis.document = previousDocument;
            delete dom.statusIndicator;
            delete dom.statusText;
        },
    };
}

/**
 * Replace `fetch` with `handler(path, init)`, recording every call. `restore()` puts the real
 * fetch back.
 */
export function stubFetch(handler) {
    const original = globalThis.fetch;
    const calls = [];
    globalThis.fetch = async (path, init) => {
        calls.push({ path, init });
        return handler(path, init);
    };
    return {
        calls,
        restore() {
            globalThis.fetch = original;
        },
    };
}

/** A JSON `Response`-like object for the fetch stub. */
export function jsonResponse(payload, { status = 200, statusText = "OK" } = {}) {
    const body = JSON.stringify(payload);
    return {
        ok: status >= 200 && status < 300,
        status,
        statusText,
        json: async () => payload,
        text: async () => body,
    };
}

/** A 2xx response with no body, as `DELETE` endpoints answer (204, or an empty 200). */
export function emptyResponse({ status = 204, statusText = "No Content" } = {}) {
    return {
        ok: true,
        status,
        statusText,
        json: async () => {
            // A real Response rejects on an empty body; the helper must not hide that.
            throw new SyntaxError("Unexpected end of JSON input");
        },
        text: async () => "",
    };
}

/**
 * A `text/event-stream` `Response`-like object that yields `chunks` in order.
 *
 * Each chunk is the exact text one network read delivers, so a test can split a frame in the
 * middle of a line and check the reader buffers it back together. `cancelled` records whether
 * the reader stopped the body.
 */
export function eventStreamResponse(chunks, { status = 200, statusText = "OK" } = {}) {
    const bytes = chunks.map((chunk) => new TextEncoder().encode(chunk));
    const state = { cancelled: false, read: 0 };
    return {
        ok: status >= 200 && status < 300,
        status,
        statusText,
        headers: {
            get: (name) => (String(name).toLowerCase() === "content-type" ? "text/event-stream" : null),
        },
        body: {
            getReader() {
                return {
                    async read() {
                        if (state.read < bytes.length) {
                            return { done: false, value: bytes[state.read++] };
                        }
                        return { done: true, value: undefined };
                    },
                    async cancel() {
                        state.cancelled = true;
                    },
                    releaseLock() {},
                };
            },
        },
        stream: state,
    };
}
