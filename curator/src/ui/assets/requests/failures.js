/**
 * The one request-failure policy.
 *
 * A failed request raises the connection pill into its error state with the operation named
 * in `statusText` and the failure code in the pill title, and fires exactly one toast per
 * failure transition. Repeating the same operation's failure while it is already reported is
 * a retry of one failing request, not a second event; a different operation, or a success,
 * starts a new transition.
 */

import { dom } from "../dom.js";
import { showToast } from "../shell/status.js";

/** The failure the pill currently reports, or null when the last request succeeded. */
let activeFailure = null;

function isAbort(error) {
    return Boolean(error) && error.name === "AbortError";
}

function failureCode(error) {
    if (error && error.code) return error.code;
    if (error && error.status != null) return `HTTP ${error.status}`;
    return (error && error.name) || "error";
}

function failureMessage(error) {
    return (error && typeof error.message === "string" && error.message) || "Unknown error";
}

/** The pill as it was before the failure, so a success can put it back unchanged. */
function snapshotPill() {
    if (!dom.statusIndicator) return null;
    return {
        className: dom.statusIndicator.className,
        text: dom.statusText ? dom.statusText.textContent : "",
        title: dom.statusIndicator.title || "",
    };
}

function paintFailure(operation, code, error) {
    if (!dom.statusIndicator) return;
    dom.statusIndicator.className = "status-pill status-error";
    if (dom.statusText) {
        dom.statusText.textContent = `${operation} failed`;
    }
    const debug = error && error.debug ? ` — ${error.debug}` : "";
    dom.statusIndicator.title = `${operation} failed (${code}): ${failureMessage(error)}${debug}`;
}

/**
 * Record one request failure: error state on the pill, and one toast for the transition.
 *
 * An abort is not a failure: a superseded or cancelled request leaves the pill alone.
 */
export function reportRequestFailure(error, operation) {
    if (isAbort(error)) return;

    const name = operation || (error && error.operation) || "Request";
    const code = failureCode(error);
    const isNewTransition = !activeFailure || activeFailure.operation !== name;

    if (!activeFailure) {
        activeFailure = { operation: name, previous: snapshotPill() };
    } else {
        activeFailure.operation = name;
    }

    paintFailure(name, code, error);
    if (isNewTransition) {
        showToast(`${name} failed (${code}): ${failureMessage(error)}`, "danger");
    }
}

/** The last request succeeded: forget the failure and put the pill back. */
export function clearRequestFailure() {
    if (!activeFailure) return;
    const previous = activeFailure.previous;
    activeFailure = null;
    if (!previous || !dom.statusIndicator) return;
    dom.statusIndicator.className = previous.className;
    dom.statusIndicator.title = previous.title;
    if (dom.statusText) {
        dom.statusText.textContent = previous.text;
    }
}
