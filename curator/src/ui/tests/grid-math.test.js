import test from "node:test";
import assert from "node:assert/strict";

import {
    VTT_GRID_SIZE,
    MIN_GRID_SIZE,
    MAX_GRID_SIZE,
    MIN_ZOOM,
    MAX_ZOOM,
    calculateLatticeInfo,
} from "../assets/kit/grid-math.js";

test("exposes the VTT grid and zoom bounds", () => {
    assert.equal(VTT_GRID_SIZE, 64);
    assert.equal(MIN_GRID_SIZE, 16);
    assert.equal(MAX_GRID_SIZE, 300);
    assert.equal(MIN_ZOOM, 0.1);
    assert.equal(MAX_ZOOM, 5.0);
});

test("aligned 2048px ground layer yields a 32x32 lattice split into four sections", () => {
    assert.deepEqual(calculateLatticeInfo(2048, 2048, 64, 0, 0), {
        size: 64,
        ox: 0,
        oy: 0,
        cols: 32,
        rows: 32,
        isEligible: true,
        kx: 2,
        ky: 2,
        fullW: 2048,
        fullH: 2048,
    });
});

test("non-square canvas with fractional offsets keeps whole cells only", () => {
    assert.deepEqual(calculateLatticeInfo(2048, 1600, 64, 32, 16), {
        size: 64,
        ox: 32,
        oy: 16,
        cols: 31,
        rows: 24,
        isEligible: true,
        kx: 2,
        ky: 2,
        fullW: 1984,
        fullH: 1536,
    });
});

test("a one-pixel offset shifts the lattice and can drop eligibility", () => {
    const aligned = calculateLatticeInfo(1024, 1024, 64, 0, 0);
    assert.equal(aligned.cols, 16);
    assert.equal(aligned.rows, 16);
    assert.equal(aligned.isEligible, true);
    assert.equal(aligned.kx, 1);
    assert.equal(aligned.fullW, 1024);

    const shifted = calculateLatticeInfo(1024, 1024, 64, 1, 0);
    assert.equal(shifted.ox, 1);
    assert.equal(shifted.cols, 15);
    assert.equal(shifted.rows, 16);
    assert.equal(shifted.isEligible, false);
    assert.equal(shifted.kx, null);
    assert.equal(shifted.ky, null);
    assert.equal(shifted.fullW, 960);
    assert.equal(shifted.fullH, 1024);
});

test("offsets wrap into the cell modulo, including negatives", () => {
    const wrapped = calculateLatticeInfo(1024, 1024, 64, -64, 96);
    assert.equal(wrapped.ox, 0);
    assert.equal(wrapped.oy, 32);
    assert.equal(wrapped.cols, 16);
    assert.equal(wrapped.rows, 15);
    assert.equal(wrapped.isEligible, false);
    assert.equal(wrapped.fullW, 1024);
    assert.equal(wrapped.fullH, 960);
});

test("zero-size input yields an empty, ineligible lattice", () => {
    assert.deepEqual(calculateLatticeInfo(0, 0, 64, 0, 0), {
        size: 64,
        ox: 0,
        oy: 0,
        cols: 0,
        rows: 0,
        isEligible: false,
        kx: null,
        ky: null,
        fullW: 0,
        fullH: 0,
    });
});

test("cell size is clamped to the 16px minimum and defaults to 64", () => {
    const clamped = calculateLatticeInfo(1024, 1024, 4, 0, 0);
    assert.equal(clamped.size, 16);
    assert.equal(clamped.cols, 64);
    assert.equal(clamped.rows, 64);
    assert.equal(clamped.kx, 4);
    assert.equal(clamped.fullW, 1024);

    const defaulted = calculateLatticeInfo(512, 512, 0, 0, 0);
    assert.equal(defaulted.size, 64);
    assert.equal(defaulted.cols, 8);
    assert.equal(defaulted.rows, 8);
    assert.equal(defaulted.isEligible, false);
});
