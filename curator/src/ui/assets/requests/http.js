/**
 * The private HTTP verb helpers for `requests/*`.
 *
 * Every domain function in this directory builds on `requestJson`, `requestBlob` or
 * `requestEventStream`; views import the domain function they need and never this module, so
 * there is exactly one place that maps a response onto the `RequestError` envelope and exactly
 * one place that reports a failure.
 */

import { clearRequestFailure, reportRequestFailure } from "./failures.js";

/**
 * The failure envelope every request in the UI raises.
 *
 * `status` is the HTTP status when a response arrived, `code` and `debug` are the server's
 * error contract (`{ error, code, debug }`), `payload` is that whole body (an endpoint adds
 * `blocking_issues`, `readiness`, `active_run_id` there for its own refusals), and
 * `path`/`operation` say what was attempted.
 */
export class RequestError extends Error {
    constructor(message, { status, code, debug, path, operation, payload, cause } = {}) {
        super(message);
        this.name = "RequestError";
        this.status = status;
        this.code = code;
        this.debug = debug;
        this.path = path;
        this.operation = operation;
        this.payload = payload;
        if (cause !== undefined) {
            this.cause = cause;
        }
    }
}

function isAbort(error) {
    return Boolean(error) && error.name === "AbortError";
}

/** Report the failure through the one failure policy, then hand the error back to throw. */
export function failRequest(error) {
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
 * Map a non-OK response onto the failure envelope: the server's `{ error, code, debug }` when
 * the body carries it, the status line otherwise. Reported through the one failure policy.
 */
export async function responseFailure(response, path, operation) {
    const failure = await readErrorPayload(response);
    const statusLine = `HTTP ${response.status}${response.statusText ? ` ${response.statusText}` : ""}`;
    const message = (failure && typeof failure.error === "string" && failure.error) || statusLine;
    return failRequest(
        new RequestError(message, {
            status: response.status,
            code: failure?.code,
            debug: failure?.debug,
            path,
            operation,
            payload: failure,
        }),
    );
}

/**
 * Read a 2xx body as JSON.
 *
 * A successful request may carry no payload: `DELETE /api/curated/{id}` answers 204, and a 200
 * with an empty body is equally a success. Only a body that is present but unparsable is an
 * invalid-JSON failure.
 */
export async function readJson(response, path, operation) {
    let payload = null;
    try {
        const text = await response.text();
        if (text) payload = JSON.parse(text);
    } catch (error) {
        if (isAbort(error)) throw error;
        const reason = error && error.message ? error.message : String(error);
        throw failRequest(new RequestError(`Invalid JSON from ${path}: ${reason}`, { path, operation, cause: error }));
    }
    return payload;
}

/**
 * Perform one request and read the response body with `consume` (default: JSON).
 *
 * A non-OK response becomes a `RequestError` carrying the server's error contract, a network
 * failure becomes a `RequestError` naming the path, and an abort is rethrown untouched: a
 * cancelled request is not a failure and must not raise the status pill. A success clears the
 * failure the policy reports.
 */
async function request(path, { method = "GET", body, signal, operation, headers }, consume) {
    let response;
    try {
        response = await fetch(path, {
            method,
            signal,
            headers: body === undefined
                ? headers
                : { ...headers, "Content-Type": "application/json" },
            body: body === undefined ? undefined : JSON.stringify(body),
        });
    } catch (error) {
        if (isAbort(error)) throw error;
        const reason = error && error.message ? error.message : String(error);
        throw failRequest(new RequestError(`Could not reach ${path}: ${reason}`, { path, operation, cause: error }));
    }

    if (!response.ok) {
        throw await responseFailure(response, path, operation);
    }

    const payload = await consume(response);
    clearRequestFailure(operation);
    return payload;
}

/** Perform one JSON request and resolve with the parsed payload (or `null` for no body). */
export function requestJson(path, options = {}) {
    return request(path, options, (response) => readJson(response, path, options.operation));
}

/** Perform one request whose 2xx body is a binary archive, resolving with a `Blob`. */
export function requestBlob(path, options = {}) {
    return request(path, options, async (response) => {
        try {
            return await response.blob();
        } catch (error) {
            if (isAbort(error)) throw error;
            const reason = error && error.message ? error.message : String(error);
            throw failRequest(
                new RequestError(`Could not read the archive from ${path}: ${reason}`, {
                    path,
                    operation: options.operation,
                    cause: error,
                }),
            );
        }
    });
}
