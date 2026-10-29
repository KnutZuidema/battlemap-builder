/**
 * The Reddit harvest request.
 *
 * The server streams one event per harvested candidate (`progress`) and a final (`complete` or
 * `error`) frame; a caller that cannot stream gets the harvest summary in one JSON answer,
 * which this resolves with.
 */

import { requestEventStream } from "./stream.js";

/**
 * Harvest battlemaps from a subreddit.
 *
 * @param {{ sort: string, time: string, limit: number, gridless_only: boolean }} payload
 * @param {{ onEvent?: (event: string, data: unknown) => void, signal?: AbortSignal }} [options]
 * @returns {Promise<unknown>} the one-shot summary, or `undefined` for a streamed harvest
 */
export function harvestReddit(payload, { onEvent, signal } = {}) {
    return requestEventStream("/api/harvest/reddit", {
        method: "POST",
        body: payload,
        signal,
        onEvent,
        operation: "Harvest Reddit maps",
    });
}
