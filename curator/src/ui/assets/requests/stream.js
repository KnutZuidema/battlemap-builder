/**
 * The one server-sent-event reader for `requests/*`.
 *
 * Harvesting, benchmarking and freezing all answer with an event stream: a non-OK response is
 * the same failure envelope as any other request, an OK response that turns out to be plain
 * JSON is the one-shot summary those endpoints offer when the caller cannot stream, and a
 * stream is read line by line with each `data:` frame parsed and handed to `onEvent`.
 *
 * The transport is `http.js`'s one `request()` with `Accept: text/event-stream`; this module
 * owns only the framing. What an event means (progress, complete, cancelled, error) belongs
 * to the view that asked for the stream.
 */

import { RequestError, failRequest, isAbort, readJson, request } from "./http.js";

/**
 * Read a response body as server-sent events.
 *
 * Frames are split on newlines, so a chunk boundary in the middle of a line (or of a frame)
 * is buffered until the rest arrives. `event:` names the frame; each `data:` line is JSON.
 * A `data:` line that is not JSON means the stream broke its own contract, so it fails the
 * request rather than being dropped silently.
 */
async function readEvents(response, { path, operation, onEvent }) {
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let eventName = "message";

    try {
        for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });

            const lines = buffer.split("\n");
            buffer = lines.pop();
            for (const line of lines) {
                const trimmed = line.trim();
                if (trimmed === "") {
                    eventName = "message";
                    continue;
                }
                if (trimmed.startsWith("event:")) {
                    eventName = trimmed.slice(6).trim();
                    continue;
                }
                if (!trimmed.startsWith("data:")) continue;

                const raw = trimmed.slice(5).trim();
                if (raw === "") continue;
                let data;
                try {
                    data = JSON.parse(raw);
                } catch (error) {
                    throw failRequest(
                        new RequestError(`Invalid event from ${path}: ${error.message}`, {
                            path,
                            operation,
                            cause: error,
                        }),
                    );
                }
                onEvent?.(eventName, data);
                eventName = "message";
            }
        }
    } catch (error) {
        // The caller is done with this stream: an aborted read, a frame that broke the
        // contract, or a view that threw while handling an event. Stop the body rather than
        // leaving it flowing for a response nobody will read.
        try {
            await reader.cancel();
        } catch (_) {
            // The stream is already gone (aborted or failed); there is nothing left to stop.
        }
        if (isAbort(error)) throw error;
        if (error instanceof RequestError) throw error;
        const reason = error && error.message ? error.message : String(error);
        throw failRequest(new RequestError(`Lost the event stream from ${path}: ${reason}`, { path, operation, cause: error }));
    } finally {
        reader.releaseLock?.();
    }
}

/**
 * Read a 2xx response: an event stream is framed into events, and anything else is the
 * endpoint's one-shot JSON summary (its no-stream mode).
 */
async function consumeEvents(response, { path, operation, onEvent }) {
    const contentType = response.headers?.get?.("content-type") || "";
    if (!contentType.includes("text/event-stream") || !response.body) {
        return readJson(response, path, operation);
    }
    await readEvents(response, { path, operation, onEvent });
    return undefined;
}

/**
 * Perform one request and consume the response as an event stream.
 *
 * Resolves with the response's plain JSON payload when the server answered with a one-shot
 * summary instead of a stream (the endpoint's no-stream mode), and with `undefined` once a
 * stream has been read to its end. The transport is `http.js`'s one `request()`, so a non-OK
 * response becomes a reported `RequestError` before any event is dispatched and an abort
 * propagates untouched, exactly as for every JSON verb.
 *
 * @param {string} path
 * @param {{
 *   method?: string,
 *   body?: unknown,
 *   signal?: AbortSignal,
 *   operation?: string,
 *   onEvent?: (event: string, data: unknown) => void,
 * }} [options]
 * @returns {Promise<unknown>}
 */
export function requestEventStream(path, options = {}) {
    const { method = "POST", body, signal, operation, onEvent } = options;
    return request(
        path,
        { method, body, signal, operation, headers: { Accept: "text/event-stream" } },
        (response) => consumeEvents(response, { path, operation, onEvent }),
    );
}
