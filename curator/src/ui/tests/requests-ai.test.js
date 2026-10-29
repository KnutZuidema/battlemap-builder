import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../assets/store.js";
import {
    describeCuratedMap,
    describeStagingCandidate,
    suggestCuratedThemes,
    suggestStagingThemes,
} from "../assets/requests/ai.js";
import { clearRequestFailure } from "../assets/requests/failures.js";
import { fetchThemeSuggestions } from "../assets/views/curation.js";
import { describeInspectorMap } from "../assets/views/inspector.js";
import { installRequestDom, jsonResponse, stubFetch } from "./support/fakes.js";

function setUp(t, handler) {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const ui = installRequestDom();
    const server = stubFetch(handler);
    t.after(() => {
        server.restore();
        clearRequestFailure();
        ui.restore();
    });
    return { ui, server };
}

test("the four AI clients post to their own endpoint and normalise the payload", async (t) => {
    const { server } = setUp(t, async (path) => {
        if (path.endsWith("/suggest-themes")) return jsonResponse({ suggestions: ["cave", "damp"] });
        if (path.startsWith("/api/curated/")) return jsonResponse({ description: "  a damp cave  ", applied: true });
        return jsonResponse({ description: "  a damp cave  " });
    });

    const staging = await describeStagingCandidate("c 1", { tags: ["cave"] });
    assert.deepEqual(staging, { description: "a damp cave" }, "descriptions are trimmed");

    const curatedSuggestions = await suggestCuratedThemes("m1");
    assert.deepEqual(curatedSuggestions, ["cave", "damp"]);

    const curated = await describeCuratedMap("m1", { tags: [], apply: true });
    assert.deepEqual(curated, { description: "a damp cave", applied: true }, "apply reports what the server persisted");

    await suggestStagingThemes("c 1");

    assert.deepEqual(
        server.calls.map((call) => call.path),
        [
            "/api/staging/c%201/describe",
            "/api/curated/m1/suggest-themes",
            "/api/curated/m1/describe",
            "/api/staging/c%201/suggest-themes",
        ],
    );
    assert.deepEqual(JSON.parse(server.calls[0].init.body), { tags: ["cave"] });
    assert.deepEqual(JSON.parse(server.calls[2].init.body), { tags: [], apply: true });
    assert.equal(server.calls[1].init.body, undefined, "a suggestion request sends no body");
});

test("an empty suggestions payload reads as no suggestions", async (t) => {
    setUp(t, async () => jsonResponse({}));
    assert.deepEqual(await suggestStagingThemes("c1"), []);
});

test("the triage prefetch and the decision modal share one suggestion request per candidate", async (t) => {
    const { server } = setUp(t, async () => jsonResponse({ suggestions: ["cave"] }));

    const candidate = { id: "c1" };
    const first = fetchThemeSuggestions(candidate);
    const second = fetchThemeSuggestions(candidate);
    assert.deepEqual(await first, ["cave"]);
    assert.deepEqual(await second, ["cave"]);

    assert.equal(server.calls.length, 1, "the second caller joined the in-flight request");
    assert.deepEqual(candidate._themeSuggestions, ["cave"]);
    assert.equal(candidate._suggestionsOffline, false);

    // A cached candidate answers without a request.
    assert.deepEqual(await fetchThemeSuggestions(candidate), ["cave"]);
    assert.equal(server.calls.length, 1);

    // A different candidate has its own request.
    await fetchThemeSuggestions({ id: "c2" });
    assert.equal(server.calls.length, 2);
    assert.equal(server.calls[1].path, "/api/staging/c2/suggest-themes");
});

test("a failed suggestion request flags the candidate offline after one report", async (t) => {
    const { ui } = setUp(t, async () =>
        jsonResponse({ error: "vision model unavailable", code: "model_offline" }, { status: 503, statusText: "Service Unavailable" }),
    );

    const candidate = { id: "c1" };
    assert.deepEqual(await fetchThemeSuggestions(candidate), []);
    assert.equal(candidate._suggestionsOffline, true);
    assert.equal(ui.toastContainer.children.length, 1);
    assert.equal(ui.statusText.textContent, "Suggest staging themes failed");
});

test("a late describe for the map the drawer left never lands in the open draft", async (t) => {
    let release;
    const { ui, server } = setUp(
        t,
        () =>
            new Promise((resolve) => {
                release = () => resolve(jsonResponse({ description: "old map description" }));
            }),
    );

    state.libraryDrawerActive = true;
    state.libraryDrawerMap = { id: "map-a" };
    state.libraryDraft = { tags: ["dungeon"], description: "" };

    const pending = describeInspectorMap();
    assert.equal(server.calls[0].path, "/api/curated/map-a/describe");

    // The reviewer moves to another map while map-a's describe is still in flight.
    state.libraryDrawerMap = { id: "map-b" };
    state.libraryDraft = { tags: ["cave"], description: "map b draft" };

    release();
    await pending;

    assert.equal(server.calls.length, 1);
    assert.equal(state.libraryDraft.description, "map b draft", "map-a's answer is dropped");
    assert.deepEqual(state.libraryDraft.tags, ["cave"]);
    assert.equal(state.libraryDrawerMap.id, "map-b");
    assert.equal(ui?.toastContainer.children.length ?? 0, 0, "a dropped answer is not reported as applied");
});

test("a failed describe leaves the open draft alone and reports once", async (t) => {
    const { ui } = setUp(t, async () =>
        jsonResponse({ error: "describe model unavailable", code: "model_offline" }, { status: 502, statusText: "Bad Gateway" }),
    );

    state.libraryDrawerActive = true;
    state.libraryDrawerMap = { id: "map-a" };
    state.libraryDraft = { tags: ["dungeon"], description: "keep this draft" };

    await describeInspectorMap();

    assert.equal(state.libraryDraft.description, "keep this draft", "the view did not continue as if successful");
    assert.equal(ui.pill.className, "status-pill status-error");
    assert.equal(ui.statusText.textContent, "Describe curated map failed");
    assert.match(ui.pill.title, /model_offline/);
    assert.equal(ui.toastContainer.children.length, 1);
});
