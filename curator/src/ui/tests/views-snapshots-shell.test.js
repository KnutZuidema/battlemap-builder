/**
 * The Snapshots shelf's shell contract.
 *
 * A converted view owns its subtree, so the shell keeps only the mount root as an id and the
 * element registry keeps only the nodes outside that subtree. A converted view that leaves its
 * inner ids in the shell is two owners of one subtree; an unconverted view keeps its ids until
 * it is converted (ADR-0012).
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const shellMarkup = readFileSync(new URL("../assets/index.html", import.meta.url), "utf8");
const registry = readFileSync(new URL("../assets/dom.js", import.meta.url), "utf8");

test("the shell keeps the shelf mount root and hands its subtree to the view", () => {
    assert.match(shellMarkup, /id="snapshots-shelf"/, "the view mounts on the shell's shelf root");
    assert.doesNotMatch(
        shellMarkup,
        /id="snapshots-list"/,
        "the converted view creates the list, so the shell must not also own one",
    );
    assert.doesNotMatch(
        shellMarkup,
        /id="snapshots-empty"/,
        "the converted view creates the empty notice, so the shell must not also own one",
    );
});

test("the element registry keeps the shell chrome and drops the converted subtree", () => {
    assert.match(
        registry,
        /dom\.snapshotsShelf = document\.getElementById\("snapshots-shelf"\)/,
        "the view mounts through the root the registry hands it",
    );
    assert.match(
        registry,
        /dom\.snapshotsCountBadge = document\.getElementById\("snapshots-count-badge"\)/,
        "the count badge is shell chrome outside the shelf, so the view still writes it",
    );
    assert.doesNotMatch(registry, /dom\.snapshotsList/, "the list belongs to the view");
    assert.doesNotMatch(registry, /dom\.snapshotsEmpty/, "the empty notice belongs to the view");
});

test("an unconverted view keeps its shell ids until it is converted", () => {
    // The library grid is issue #269's conversion, so its markup id and registry entry stay.
    assert.match(shellMarkup, /id="library-grid"/);
    assert.match(registry, /dom\.libraryGrid = document\.getElementById\("library-grid"\)/);
});
