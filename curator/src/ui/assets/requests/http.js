/**
 * The private HTTP verb helper for `requests/*`.
 *
 * Every domain function in this directory builds on `requestJson`; views import the domain
 * function they need and never this module, so there is exactly one place that maps a
 * response onto the `RequestError` envelope and exactly one place that reports a failure.
 */

import { clearRequestFailure, reportRequestFailure } from "./failures.js";

/**
 * The failure envelope every request in the UI raises.
 *
 * `status` is the HTTP status when a response arrived, `code` and `debug` are the server's
 * error contract (`{ error, code, debug }`), and `path`/`operation` say what was attempted.
 */
export class RequestError extends Error {
    constructor(message, { status, code, debug, path, operation, cause } = {}) {
        super(message);
        this.name = "RequestError";
        this.status = status;
        this.code = code;
        this.debug = debug;
        this.path = path;
        this.operation = operation;
        if (cause !== undefined) {
            this.cause = cause;
        }
    }
}

function isAbort(error) {
    return Boolean(error) && error.name === "AbortError";
}

/** Report the failure through the one failure policy, then hand the error back to throw. */
function fail(error) {
    reportRequestFailure(error, error.operation);
    return error;
}

/** Read an error body; an unparsable body is not itself a failure worth a second error. */
async function readErrorPayload(response) {
    try {
        const payload = await response.json();
        return payload && typeof payload === "object" ? payload : null;
    } catch (error) {
        if (isAbort(error)) throw error;
        return null;
    }
}

/**
 * Perform one JSON request.
 *
 * A non-OK response becomes a `RequestError` carrying the server's `{ error, code, debug }`
 * (falling back to the status line when the body is not JSON), a network failure becomes a
 * `RequestError` naming the path, and an abort is rethrown untouched: a cancelled request is
 * not a failure and must not raise the status pill.
 */
export async function requestJson(path, { method = "GET", body, signal, operation } = {}) {
    let response;
    try {
        response = await fetch(path, {
            method,
            signal,
            headers: body === undefined ? undefined : { "Content-Type": "application/json" },
            body: body === undefined ? undefined : JSON.stringify(body),
        });
    } catch (error) {
        if (isAbort(error)) throw error;
        const reason = error && error.message ? error.message : String(error);
        throw fail(new RequestError(`Could not reach ${path}: ${reason}`, { path, operation, cause: error }));
    }

    if (!response.ok) {
        const failure = await readErrorPayload(response);
        const statusLine = `HTTP ${response.status}${response.statusText ? ` ${response.statusText}` : ""}`;
        const message = (failure && typeof failure.error === "string" && failure.error) || statusLine;
        throw fail(
            new RequestError(message, {
                status: response.status,
                code: failure?.code,
                debug: failure?.debug,
                path,
                operation,
            }),
        );
    }

    let payload;
    try {
        payload = await response.json();
    } catch (error) {
        if (isAbort(error)) throw error;
        const reason = error && error.message ? error.message : String(error);
        throw fail(new RequestError(`Invalid JSON from ${path}: ${reason}`, { path, operation, cause: error }));
    }

    clearRequestFailure();
    return payload;
}
