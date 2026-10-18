const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const { forgeCanvasRegion, bufferToDataUrl } = require('../services/canvasForge');

async function createTestImage(width = 128, height = 128, color = { r: 60, g: 60, b: 60 }) {
  const buf = await sharp({
    create: { width, height, channels: 3, background: color }
  }).png().toBuffer();
  return bufferToDataUrl(buf);
}

/**
 * Feature translation function matching App.jsx implementation
 */
function translateFeatures(features, offset) {
  const dx = offset?.dx || 0;
  const dy = offset?.dy || 0;
  if (dx === 0 && dy === 0) {
    return features;
  }
  return features.map((f) => ({
    ...f,
    box: f.box ? { ...f.box, x: f.box.x + dx, y: f.box.y + dy } : f.box,
    ...(typeof f.x === 'number' ? { x: f.x + dx } : {}),
    ...(typeof f.y === 'number' ? { y: f.y + dy } : {})
  }));
}

test('translateFeatures preserves coordinates when dx=0 and dy=0', () => {
  const initialFeatures = [
    { id: 'feat_1', type: 'door', box: { x: 100, y: 150, width: 64, height: 64 } },
    { id: 'feat_2', type: 'chest', box: { x: 300, y: 400, width: 64, height: 64 }, x: 300, y: 400 }
  ];

  const translated = translateFeatures(initialFeatures, { dx: 0, dy: 0 });
  assert.strictEqual(translated, initialFeatures);
});

test('translateFeatures shifts box and optional x/y coordinates when expanding West (dx > 0)', () => {
  const initialFeatures = [
    { id: 'feat_door', type: 'door', box: { x: 128, y: 64, width: 64, height: 64 } },
    { id: 'feat_point', type: 'trap', box: { x: 256, y: 128, width: 64, height: 64 }, x: 256, y: 128 }
  ];

  const translated = translateFeatures(initialFeatures, { dx: 64, dy: 0 });

  assert.deepStrictEqual(translated[0].box, { x: 192, y: 64, width: 64, height: 64 });
  assert.deepStrictEqual(translated[1].box, { x: 320, y: 128, width: 64, height: 64 });
  assert.strictEqual(translated[1].x, 320);
  assert.strictEqual(translated[1].y, 128);
});

test('translateFeatures shifts box and optional x/y coordinates when expanding North (dy > 0)', () => {
  const initialFeatures = [
    { id: 'feat_door', type: 'door', box: { x: 128, y: 64, width: 64, height: 64 } },
    { id: 'feat_point', type: 'trap', box: { x: 256, y: 128, width: 64, height: 64 }, x: 256, y: 128 }
  ];

  const translated = translateFeatures(initialFeatures, { dx: 0, dy: 128 });

  assert.deepStrictEqual(translated[0].box, { x: 128, y: 192, width: 64, height: 64 });
  assert.deepStrictEqual(translated[1].box, { x: 256, y: 256, width: 64, height: 64 });
  assert.strictEqual(translated[1].x, 256);
  assert.strictEqual(translated[1].y, 256);
});

test('translateFeatures shifts both axes when expanding North-West into Void', () => {
  const initialFeatures = [
    { id: 'feat_altar', type: 'altar', box: { x: 200, y: 300, width: 128, height: 64 }, x: 200, y: 300 }
  ];

  const translated = translateFeatures(initialFeatures, { dx: 64, dy: 64 });

  assert.deepStrictEqual(translated[0].box, { x: 264, y: 364, width: 128, height: 64 });
  assert.strictEqual(translated[0].x, 264);
  assert.strictEqual(translated[0].y, 364);
});

test('history stack restores mapImage and features synchronously on undo and redo', () => {
  let history = [];
  let historyIndex = -1;

  const pushHistoryState = (newMapImage, newFeatures) => {
    const newHistory = history.slice(0, historyIndex + 1);
    newHistory.push({ mapImage: newMapImage, features: newFeatures });
    history = newHistory;
    historyIndex = newHistory.length - 1;
  };

  // Step 1: Initial map generated
  const img0 = 'data:image/png;base64,MAP0';
  pushHistoryState(img0, []);
  assert.strictEqual(history.length, 1);
  assert.strictEqual(historyIndex, 0);

  // Step 2: Features detected
  const feat0 = [{ id: 'door1', box: { x: 100, y: 100, width: 64, height: 64 } }];
  pushHistoryState(img0, feat0);
  assert.strictEqual(history.length, 2);
  assert.strictEqual(historyIndex, 1);

  // Step 3: North-West expansion
  const img1 = 'data:image/png;base64,MAP1';
  const feat1 = translateFeatures(feat0, { dx: 64, dy: 64 });
  pushHistoryState(img1, feat1);
  assert.strictEqual(history.length, 3);
  assert.strictEqual(historyIndex, 2);

  // Verify state before undo
  assert.deepStrictEqual(history[historyIndex].features[0].box, { x: 164, y: 164, width: 64, height: 64 });
  assert.strictEqual(history[historyIndex].mapImage, img1);

  // Step 4: Undo expansion
  historyIndex -= 1;
  const undoState = history[historyIndex];
  assert.strictEqual(undoState.mapImage, img0);
  assert.deepStrictEqual(undoState.features[0].box, { x: 100, y: 100, width: 64, height: 64 });

  // Step 5: Redo expansion
  historyIndex += 1;
  const redoState = history[historyIndex];
  assert.strictEqual(redoState.mapImage, img1);
  assert.deepStrictEqual(redoState.features[0].box, { x: 164, y: 164, width: 64, height: 64 });
});

test('end-to-end: forgeCanvasRegion Void expansion offset aligns interactive features with expanded canvas', async () => {
  const originalWidth = 256;
  const originalHeight = 256;
  const originalDataUrl = await createTestImage(originalWidth, originalHeight);

  // Feature placed at (64, 64) in the original 256x256 map
  const feature = {
    id: 'chest_1',
    type: 'chest',
    box: { x: 64, y: 64, width: 64, height: 64 }
  };

  // Expand West by 64px (selection.x = -64)
  const selection = { x: -64, y: 0, width: 64, height: 128 };
  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  // Verify positive dx offset computed by canvasForge
  assert.strictEqual(res.offset.dx, 64);
  assert.strictEqual(res.offset.dy, 0);

  // Translate feature
  const translated = translateFeatures([feature], res.offset);

  // The original image was placed at x = dx (64) in the new canvas
  // The feature is now at x = 64 + 64 = 128, exactly aligned with the shifted map content
  assert.deepStrictEqual(translated[0].box, {
    x: 128,
    y: 64,
    width: 64,
    height: 64
  });
});
