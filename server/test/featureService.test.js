const test = require('node:test');
const assert = require('node:assert');
const {
  snapBoxTo64,
  normalizeFeatures,
  FEATURE_DEFINITIONS,
  detectFeatures
} = require('../services/featureService');

test('snapBoxTo64 snaps 0-1000 coordinates to 64px grid increments', () => {
  // [ymin, xmin, ymax, xmax]
  // 500/1000 * 1024 = 512, 100/1000 * 1024 = 102.4 -> Math.round(102.4 / 64) * 64 = 128
  const box = snapBoxTo64([100, 200, 250, 400], 1024, 1024);
  assert.strictEqual(box.x % 64, 0);
  assert.strictEqual(box.y % 64, 0);
  assert.strictEqual(box.width % 64, 0);
  assert.strictEqual(box.height % 64, 0);
  assert.ok(box.width >= 64);
  assert.ok(box.height >= 64);
});

test('snapBoxTo64 enforces minimum 64x64 and canvas bounds clamping', () => {
  const tinyBox = snapBoxTo64([0, 0, 10, 10], 1024, 1024);
  assert.strictEqual(tinyBox.x, 0);
  assert.strictEqual(tinyBox.y, 0);
  assert.strictEqual(tinyBox.width, 64);
  assert.strictEqual(tinyBox.height, 64);

  const edgeBox = snapBoxTo64([980, 980, 1000, 1000], 1024, 1024);
  assert.ok(edgeBox.x + edgeBox.width <= 1024);
  assert.ok(edgeBox.y + edgeBox.height <= 1024);
});

test('normalizeFeatures strictly allows only door, destructible_wall, and carpet', () => {
  const raw = [
    {
      id: 'd1',
      type: 'door',
      label: 'Oak Door',
      box_2d: [100, 100, 200, 200]
    },
    {
      id: 'w1',
      type: 'destructible_wall',
      label: 'Cracked Masonry',
      box_2d: [300, 300, 400, 400]
    },
    {
      id: 'c1',
      type: 'carpet',
      label: 'Ornate Rug',
      box_2d: [500, 500, 600, 600]
    },
    {
      id: 'bad1',
      type: 'treasure_chest',
      label: 'Gold Chest',
      box_2d: [700, 700, 800, 800]
    }
  ];

  const features = normalizeFeatures(raw, 1024, 1024);
  assert.strictEqual(features.length, 3);

  const types = features.map(f => f.type);
  assert.deepStrictEqual(types, ['door', 'destructible_wall', 'carpet']);

  for (const feat of features) {
    assert.strictEqual(feat.currentState, 0);
    assert.strictEqual(feat.states.length, 2);
    assert.strictEqual(feat.states[0].stateIndex, 0);
    assert.strictEqual(feat.states[1].stateIndex, 1);
    assert.strictEqual(feat.states[0].patchDataUrl, null);
    assert.strictEqual(feat.states[1].patchDataUrl, null);
    assert.ok(feat.suggestedAltPrompt.length > 0);
  }

  assert.deepStrictEqual(features[0].states.map(s => s.label), FEATURE_DEFINITIONS.door.stateLabels);
  assert.deepStrictEqual(features[1].states.map(s => s.label), FEATURE_DEFINITIONS.destructible_wall.stateLabels);
  assert.deepStrictEqual(features[2].states.map(s => s.label), FEATURE_DEFINITIONS.carpet.stateLabels);
});

test('detectFeatures rejects missing mapDataUrl', async () => {
  await assert.rejects(async () => {
    await detectFeatures({});
  }, (err) => err.code === 'INVALID_PARAMETERS');
});

test('detectFeatures rejects missing OPENROUTER_API_KEY', async () => {
  const oldKey = process.env.OPENROUTER_API_KEY;
  delete process.env.OPENROUTER_API_KEY;
  try {
    await assert.rejects(async () => {
      await detectFeatures({ mapDataUrl: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==' });
    }, (err) => err.code === 'NO_API_KEY');
  } finally {
    if (oldKey) process.env.OPENROUTER_API_KEY = oldKey;
  }
});

test('forgeFeatureState rejects missing originalDataUrl or box with INVALID_PARAMETERS', async () => {
  const { forgeFeatureState } = require('../services/featureService');

  await assert.rejects(
    () => forgeFeatureState({ box: { x: 0, y: 0, width: 64, height: 64 } }),
    (err) => err.code === 'INVALID_PARAMETERS'
  );
  await assert.rejects(
    () => forgeFeatureState({ originalDataUrl: 'data:image/png;base64,AAAA' }),
    (err) => err.code === 'INVALID_PARAMETERS'
  );
});

test('forgeFeatureState extracts the bounding box patch using the composite offset via an in-memory renderAdapter', async () => {
  const sharp = require('sharp');
  const { forgeFeatureState } = require('../services/featureService');
  const { bufferToDataUrl, dataUrlToBuffer } = require('../services/canvasForge');

  const mapBuffer = await sharp({
    create: { width: 128, height: 128, channels: 3, background: { r: 50, g: 50, b: 50 } }
  }).png().toBuffer();
  const originalDataUrl = bufferToDataUrl(mapBuffer);
  const box = { x: 32.4, y: 32.2, width: 63.6, height: 64 };
  let adapterCalled = false;

  const res = await forgeFeatureState({
    originalDataUrl,
    box,
    prompt: 'Swing the door open',
    renderAdapter: async ({ contextW, contextH }) => {
      adapterCalled = true;
      const imageBuffer = await sharp({
        create: { width: contextW, height: contextH, channels: 3, background: { r: 200, g: 20, b: 20 } }
      }).png().toBuffer();
      return { imageBuffer, debug: { injected: true }, usage: { total_tokens: 7 }, cost: 0.001 };
    }
  });

  assert.strictEqual(adapterCalled, true);
  assert.ok(res.patchDataUrl.startsWith('data:image/png;base64,'));
  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
  assert.deepStrictEqual(res.debug, { injected: true });
  assert.strictEqual(res.usage.total_tokens, 7);
  assert.strictEqual(res.cost, 0.001);

  const patchMeta = await sharp(dataUrlToBuffer(res.patchDataUrl)).metadata();
  assert.strictEqual(patchMeta.width, 64);
  assert.strictEqual(patchMeta.height, 64);
  const { data, info } = await sharp(dataUrlToBuffer(res.patchDataUrl)).raw().toBuffer({ resolveWithObject: true });
  const center = (32 * info.width + 32) * info.channels;
  assert.ok(data[center] > 150 && data[center + 1] < 80, 'patch centre carries the forged (red) pixels');
});

test('forgeFeatureState shifts the patch by the composite offset when expanding west/north', async () => {
  const sharp = require('sharp');
  const { forgeFeatureState } = require('../services/featureService');
  const { bufferToDataUrl, dataUrlToBuffer } = require('../services/canvasForge');

  const mapBuffer = await sharp({
    create: { width: 128, height: 128, channels: 3, background: { r: 50, g: 50, b: 50 } }
  }).png().toBuffer();

  const res = await forgeFeatureState({
    originalDataUrl: bufferToDataUrl(mapBuffer),
    box: { x: -64, y: 0, width: 64, height: 64 },
    renderAdapter: async ({ contextW, contextH }) => ({
      imageBuffer: await sharp({
        create: { width: contextW, height: contextH, channels: 3, background: { r: 10, g: 200, b: 10 } }
      }).png().toBuffer()
    })
  });

  assert.deepStrictEqual(res.offset, { dx: 64, dy: 0 });
  const patchMeta = await sharp(dataUrlToBuffer(res.patchDataUrl)).metadata();
  assert.strictEqual(patchMeta.width, 64);
  assert.strictEqual(patchMeta.height, 64);
});
