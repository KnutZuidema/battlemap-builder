/**
 * What the Snapshots shelf already did, pinned so converting the shelf to a render target cannot
 * quietly change it: the collapse toggle's class, aria state and label, and the freeze progress
 * panel's phase, counts, bar and per-map detail.
 *
 * These run green today on purpose. Only the delete action's outcome depends on the render
 * target, so it is asserted beside that target in `views-snapshots-render.test.js`.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { dom } from "../assets/dom.js";
import { state } from "../assets/store.js";
import {
    showFreezeProgressPanel,
    toggleSnapshotsShelf,
    updateFreezeProgressUI,
} from "../assets/views/snapshots.js";
import { installDom } from "./support/dom.js";

test("collapse keeps its class, aria state and label", (t) => {
    const ui = installDom();
    dom.snapshotsShelf = ui.document.createElement("div");
    dom.btnToggleSnapshots = ui.document.createElement("button");
    dom.toggleSnapshotsText = ui.document.createElement("span");
    state.snapshotsCollapsed = false;

    t.after(() => {
        for (const name of ["snapshotsShelf", "btnToggleSnapshots", "toggleSnapshotsText"]) {
            delete dom[name];
        }
        state.snapshotsCollapsed = false;
        ui.restore();
    });

    toggleSnapshotsShelf();

    assert.equal(state.snapshotsCollapsed, true);
    assert.equal(dom.snapshotsShelf.classList.contains("collapsed"), true);
    assert.equal(dom.btnToggleSnapshots.getAttribute("aria-expanded"), "false");
    assert.equal(dom.toggleSnapshotsText.textContent, "Expand");

    toggleSnapshotsShelf();

    assert.equal(state.snapshotsCollapsed, false);
    assert.equal(dom.snapshotsShelf.classList.contains("collapsed"), false);
    assert.equal(dom.btnToggleSnapshots.getAttribute("aria-expanded"), "true");
    assert.equal(dom.toggleSnapshotsText.textContent, "Collapse");
});

test("freeze progress paints the panel, the counts and the freeze button label", (t) => {
    const ui = installDom();
    const names = [
        "freezeProgressPanel",
        "freezePhaseBadge",
        "freezeProgressMessage",
        "freezeProgressCounts",
        "freezeProgressPercent",
        "freezeProgressBar",
        "freezeDetailMap",
        "freezeDetailSection",
        "btnFreezeLabel",
    ];
    for (const name of names) dom[name] = ui.document.createElement("div");
    dom.freezeProgressPanel.classList.add("hidden");

    t.after(() => {
        for (const name of names) delete dom[name];
        ui.restore();
    });

    updateFreezeProgressUI({
        phase: "packaging",
        completed_samples: 5,
        total_samples: 10,
        current_map: "map-7",
        current_section: [2, 3],
        message: "Packing sections",
    });

    assert.equal(dom.freezeProgressPanel.classList.contains("hidden"), false, "progress reveals the panel");
    assert.equal(dom.freezePhaseBadge.textContent, "PACKAGING");
    assert.equal(dom.freezePhaseBadge.className, "freeze-phase-badge phase-packaging");
    assert.equal(dom.freezeProgressMessage.textContent, "Packing sections");
    assert.equal(dom.freezeProgressCounts.textContent, "5 / 10 samples");
    assert.equal(dom.freezeProgressPercent.textContent, "(60%)");
    assert.equal(dom.freezeProgressBar.style.width, "60%");
    assert.equal(dom.btnFreezeLabel.textContent, "Freezing (5/10)");
    assert.equal(dom.freezeDetailMap.textContent, "Map: map-7");
    assert.equal(dom.freezeDetailSection.textContent, "Section: (2, 3)");

    showFreezeProgressPanel(false);
    assert.equal(dom.freezeProgressPanel.classList.contains("hidden"), true);
});

test("the audit phase reports map counts and caps its share of the bar", (t) => {
    const ui = installDom();
    const names = [
        "freezeProgressPanel",
        "freezePhaseBadge",
        "freezeProgressMessage",
        "freezeProgressCounts",
        "freezeProgressPercent",
        "freezeProgressBar",
        "freezeDetailMap",
        "freezeDetailSection",
        "btnFreezeLabel",
    ];
    for (const name of names) dom[name] = ui.document.createElement("div");

    t.after(() => {
        for (const name of names) delete dom[name];
        ui.restore();
    });

    updateFreezeProgressUI({ phase: "auditing", completed_maps: 3, total_maps: 6 });

    assert.equal(dom.freezePhaseBadge.textContent, "AUDITING");
    assert.equal(dom.freezeProgressCounts.textContent, "3 / 6 maps");
    assert.equal(dom.freezeProgressPercent.textContent, "(13%)");
    assert.equal(dom.btnFreezeLabel.textContent, "Auditing (3/6)");
    assert.equal(dom.freezeDetailMap.textContent, "", "no map is named when the phase reports none");
});
