/**
 * Node construction for the views.
 *
 * `h` builds one element and `fragment` builds a `DocumentFragment`, both from DOM
 * construction only: children become nodes, so interpolated data is text and is never
 * parsed as HTML. There is no `innerHTML` path and therefore nothing to escape.
 */

/**
 * Build an element.
 *
 * `props` is `null`/omitted for no props. A string key becomes an attribute (`class`,
 * `title`, `href`, `download`, `type`, `value`, `role`, `aria-*`, `data-*`, …); a
 * function under an `on<event>` key registers an `addEventListener` for the lowercased
 * event name; `dataset` sets `data-*` attributes; `style` assigns onto `element.style`.
 *
 * `children` accept nodes, strings and numbers (the latter two become text nodes);
 * arrays are flattened recursively; `null`, `undefined` and `false` are skipped.
 */
export function h(tag, props, ...children) {
    const element = document.createElement(tag);
    applyProps(element, props);
    appendChildren(element, children);
    return element;
}

/** Build a `DocumentFragment` with the same child rules as `h`. */
export function fragment(...children) {
    const node = document.createDocumentFragment();
    appendChildren(node, children);
    return node;
}

function applyProps(element, props) {
    if (!props) return;

    for (const [key, value] of Object.entries(props)) {
        if (value === null || value === undefined || value === false) continue;

        if (key === "dataset") {
            for (const [name, dataValue] of Object.entries(value)) {
                if (dataValue === null || dataValue === undefined || dataValue === false) continue;
                element.setAttribute(`data-${name}`, String(dataValue));
            }
            continue;
        }

        if (key === "style") {
            for (const [name, styleValue] of Object.entries(value)) {
                element.style[name] = styleValue;
            }
            continue;
        }

        if (typeof value === "function" && key.length > 2 && key.startsWith("on")) {
            element.addEventListener(key.slice(2).toLowerCase(), value);
            continue;
        }

        element.setAttribute(key, value === true ? "" : String(value));
    }
}

function appendChildren(parent, children) {
    for (const child of children) {
        if (child === null || child === undefined || child === false) continue;

        if (Array.isArray(child)) {
            appendChildren(parent, child);
            continue;
        }

        if (typeof child === "string" || typeof child === "number") {
            parent.appendChild(document.createTextNode(String(child)));
            continue;
        }

        parent.appendChild(child);
    }
}
