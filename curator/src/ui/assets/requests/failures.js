/**
 * The one request-failure policy.
 *
 * A failed request raises the connection pill into its error state with the operation named
 * in `statusText` and the failure code in the pill title, and fires exactly one toast per
 * failure transition. Failures are tracked per operation: repeating the same operation's
 * failure while it is already recorded is a retry of one failing request, not a second event,
 * so a background poll that fails every tick reports itself once. A different operation failing
 * is its own transition.
 *
 * A success clears the failure of the operation that succeeded. The pill keeps naming whatever
 * still fails: a poll's failing request does not go quiet just because another request in the
 * same tick answered. `clearRequestFailure()` with no operation clears everything, which is what
 * the server-status probe does — the API answering its health check forgets the recorded
 * failures, and the next one is a new transition.
 */

import { dom } from "../dom.js";
import { showToast } from "../shell/status.js";

/** operation -> { code, error } for every failure not yet recovered from. */
const failures = new Map();

/** The pill as it was before the first unrecovered failure, so recovery restores it. */
let previousPill = null;

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

function restorePill(previous) {
    if (!previous || !dom.statusIndicator) return;
    dom.statusIndicator.className = previous.className;
    dom.statusIndicator.title = previous.title;
    if (dom.statusText) {
        dom.statusText.textContent = previous.text;
    }
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

/** Paint the most recently recorded failure, or the pre-failure pill when none is left. */
function repaint() {
    if (failures.size === 0) {
        const previous = previousPill;
        previousPill = null;
        restorePill(previous);
        return;
    }
    const [name, entry] = [...failures].pop();
    paintFailure(name, entry.code, entry.error);
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
    const isNewTransition = !failures.has(name);

    if (failures.size === 0) {
        previousPill = snapshotPill();
    }
    // Re-inserting keeps the map in failure order, so the repaint picks the newest failure.
    failures.delete(name);
    failures.set(name, { code, error });

    paintFailure(name, code, error);
    if (isNewTransition) {
        showToast(`${name} failed (${code}): ${failureMessage(error)}`, "danger");
    }
}

/**
 * A request succeeded: forget `operation`'s failure, or every failure when no operation is
 * given (the status probe's liveness answer).
 */
export function clearRequestFailure(operation) {
    if (failures.size === 0) return;
    if (operation === undefined) {
        failures.clear();
    } else if (!failures.delete(operation)) {
        return;
    }
    repaint();
}
