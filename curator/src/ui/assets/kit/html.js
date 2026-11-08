/**
 * Node construction for the views.
 *
 * `h` builds one element and `fragment` builds a `DocumentFragment`, both out of DOM
 * construction only: children become nodes, so interpolated data is text and is never parsed
 * as HTML. There is no `innerHTML` path here and therefore nothing to escape.
 */

/**
 * Build an element.
 *
 * `props` is `null`/omitted for no props. A prop becomes an attribute verbatim, so a `title`,
 * `data-*` or `href` carrying quotes or angle brackets is stored exactly as given; a function
 * under an `on<event>` key registers an `addEventListener` for that event only.
 *
 * `children` accept nodes, strings and numbers (the latter two become text nodes); arrays are
 * flattened recursively; `null`, `undefined` and `false` are skipped.
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

    for (const [name, value] of Object.entries(props)) {
        if (value === null || value === undefined || value === false) continue;

        if (typeof value === "function" && name.length > 2 && name.startsWith("on")) {
            element.addEventListener(name.slice(2).toLowerCase(), value);
            continue;
        }

        // A boolean attribute is present or absent, never the string "true".
        element.setAttribute(name, value === true ? "" : String(value));
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
