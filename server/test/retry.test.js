const test = require('node:test');
const assert = require('node:assert');

test('getRegionInCanvasCoordinates correctly maps coordinates in post-forge canvas', async () => {
  const { getRegionInCanvasCoordinates } = await import('../../client/src/utils/regionSelection.js');

  // Interior edit
  const interior = { x: 256, y: 128, width: 512, height: 512 };
  assert.deepStrictEqual(getRegionInCanvasCoordinates(interior), {
    x: 256,
    y: 128,
    width: 512,
    height: 512
  });

  // Left expansion (x = -256)
  const leftExpand = { x: -256, y: 0, width: 256, height: 1024 };
  assert.deepStrictEqual(getRegionInCanvasCoordinates(leftExpand), {
    x: 0,
    y: 0,
    width: 256,
    height: 1024
  });

  // Top expansion (y = -128)
  const topExpand = { x: 128, y: -128, width: 512, height: 128 };
  assert.deepStrictEqual(getRegionInCanvasCoordinates(topExpand), {
    x: 128,
    y: 0,
    width: 512,
    height: 128
  });

  // Top-left expansion (x = -64, y = -128)
  const topLeftExpand = { x: -64, y: -128, width: 256, height: 256 };
  assert.deepStrictEqual(getRegionInCanvasCoordinates(topLeftExpand), {
    x: 0,
    y: 0,
    width: 256,
    height: 256
  });

  // Right expansion (x = 1024, inside expanded canvas at 1024)
  const rightExpand = { x: 1024, y: 0, width: 256, height: 1024 };
  assert.deepStrictEqual(getRegionInCanvasCoordinates(rightExpand), {
    x: 1024,
    y: 0,
    width: 256,
    height: 1024
  });

  // Null check
  assert.strictEqual(getRegionInCanvasCoordinates(null), null);
});
