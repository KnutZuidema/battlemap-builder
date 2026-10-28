import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../assets/store.js";
import {
    CANONICAL_THEMES,
    getKnownDatasetTags,
    isCanonicalThemeTag,
    normalizeLayerRole,
} from "../assets/kit/tags.js";

const DEFAULT_TAGS = [
    "Dungeon",
    "Cave",
    "Tavern",
    "Wilderness",
    "Ruins",
    "Urban",
    "SciFi",
    "Building",
];

test("the canonical theme list is the single source of theme tags", () => {
    assert.deepEqual(CANONICAL_THEMES, [
        "dungeon",
        "cave",
        "tavern",
        "wilderness",
        "ruins",
        "urban",
        "scifi",
        "building",
    ]);
});

test("canonical theme tags are accepted case- and whitespace-insensitively", () => {
    assert.equal(isCanonicalThemeTag("Dungeon"), true);
    assert.equal(isCanonicalThemeTag("  CAVE "), true);
    assert.equal(isCanonicalThemeTag("sciFi"), true);
});

test("unknown, empty and missing theme tags are rejected", () => {
    assert.equal(isCanonicalThemeTag("spaceship"), false);
    assert.equal(isCanonicalThemeTag("dungeon-crypt"), false);
    assert.equal(isCanonicalThemeTag(""), false);
    assert.equal(isCanonicalThemeTag(null), false);
});

test("layer roles normalise to the canonical role names", () => {
    assert.equal(normalizeLayerRole("Ground"), "ground");
    assert.equal(normalizeLayerRole("upper-floor"), "upper_floor");
    assert.equal(normalizeLayerRole("UPPER"), "upper_floor");
    assert.equal(normalizeLayerRole("CELLAR"), "basement");
    assert.equal(normalizeLayerRole("roof"), "roof");
    assert.equal(normalizeLayerRole("variant"), "variant");
});

test("unknown or missing layer roles fall back to ground", () => {
    assert.equal(normalizeLayerRole("mezzanine"), "ground");
    assert.equal(normalizeLayerRole(""), "ground");
    assert.equal(normalizeLayerRole(undefined), "ground");
});

test("known dataset tags merge defaults with curated map and candidate tags", () => {
    state.curatedMaps = [{ tags: ["Tavern", " swamp "] }];
    state.candidates = [{ tags: ["SWAMP", "Ruins", "   "] }];
    try {
        // Curated-map tags are stored first, so a later candidate tag overwrites the label
        // but keeps the original insertion slot.
        assert.deepEqual(getKnownDatasetTags(), [...DEFAULT_TAGS, "SWAMP"]);
    } finally {
        state.curatedMaps = [];
        state.candidates = [];
    }
});

test("known dataset tags fall back to the default vocabulary", () => {
    state.curatedMaps = [];
    state.candidates = [];
    assert.deepEqual(getKnownDatasetTags(), DEFAULT_TAGS);
});
