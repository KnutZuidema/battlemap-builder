import test from "node:test";
import assert from "node:assert/strict";

import {
    formatRoleName,
    formatBytes,
    formatReadinessAge,
    formatDuration,
} from "../assets/kit/format.js";

test("formatBytes switches units at the 1024 boundary", () => {
    assert.equal(formatBytes(0), "0 B");
    assert.equal(formatBytes(1023), "1023 B");
    assert.equal(formatBytes(1024), "1.0 KB");
    assert.equal(formatBytes(1536), "1.5 KB");
    assert.equal(formatBytes(1e6), "977 KB");
});

test("formatBytes drops the decimal once the mantissa reaches ten", () => {
    assert.equal(formatBytes(10 * 1024), "10 KB");
    assert.equal(formatBytes(1024 * 1024), "1.0 MB");
});

test("formatDuration renders seconds, minutes and hours", () => {
    assert.equal(formatDuration(0), "0s");
    assert.equal(formatDuration(59), "59s");
    assert.equal(formatDuration(60), "1m 0s");
    assert.equal(formatDuration(3661), "1h 1m 1s");
    assert.equal(formatDuration(-5), "0s");
});

test("formatReadinessAge reports past and immediate timestamps", () => {
    const now = Date.now();
    assert.equal(formatReadinessAge(new Date(now).toISOString()), "just now");
    assert.equal(formatReadinessAge(new Date(now - 5 * 60 * 1000).toISOString()), "5 min ago");
    assert.equal(formatReadinessAge(new Date(now - 2 * 60 * 60 * 1000).toISOString()), "2 h ago");
    assert.equal(formatReadinessAge(new Date(now - 3 * 24 * 60 * 60 * 1000).toISOString()), "3 d ago");
});

test("formatReadinessAge handles missing and unparseable timestamps", () => {
    assert.equal(formatReadinessAge(null), "never");
    assert.equal(formatReadinessAge(""), "never");
    assert.equal(formatReadinessAge("not a timestamp"), "just now");
});

test("formatRoleName labels layer roles with their elevation", () => {
    assert.equal(formatRoleName("ground", 0), "Ground (0)");
    assert.equal(formatRoleName("upper_floor", 2), "Upper (+2)");
    assert.equal(formatRoleName("upper", undefined), "Upper (+1)");
    assert.equal(formatRoleName("upper", -2), "Upper (-2)");
    assert.equal(formatRoleName("basement", undefined), "Basement (-1)");
    assert.equal(formatRoleName("cellar", 1), "Basement (+1)");
});

test("formatRoleName handles non-elevated and unknown roles", () => {
    assert.equal(formatRoleName("roof", 3), "Roof");
    assert.equal(formatRoleName("variant"), "Variant");
    assert.equal(formatRoleName("exclude"), "Exclude");
    assert.equal(formatRoleName("excluded"), "Exclude");
    assert.equal(formatRoleName("nonsense"), "Unassigned");
    assert.equal(formatRoleName(null), "Unassigned");
});
