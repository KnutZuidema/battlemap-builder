const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const { forgeCanvasRegion, bufferToDataUrl } = require('../services/canvasForge');
const { forgeRegion } = require('../services/battlemapService');

async function createTestImage(width = 128, height = 128, color = { r: 50, g: 50, b: 50 }) {
  const buf = await sharp({
    create: { width, height, channels: 3, background: color }
  }).png().toBuffer();
  return bufferToDataUrl(buf);
}

test('in-memory renderAdapter seam receives context buffers and dimensions without monkey-patching', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 32, y: 32, width: 64, height: 64 };
  let capturedPayload = null;

  const mockAdapter = async (payload) => {
    capturedPayload = payload;
    return {
      imageBuffer: payload.contextBuffer,
      debug: { note: 'in-memory-adapter-test' },
      usage: { total_tokens: 42, cost: 0.005 },
      cost: 0.005
    };
  };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: mockAdapter
  });

  assert.ok(res.imageDataUrl.startsWith('data:image/png;base64,'));
  assert.strictEqual(res.dimensions, undefined, 'dimensions record must be encapsulated');
  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
  assert.deepStrictEqual(res.debug, { note: 'in-memory-adapter-test' });
  assert.strictEqual(res.usage.total_tokens, 42);
  assert.strictEqual(res.cost, 0.005);

  assert.ok(capturedPayload);
  assert.ok(Buffer.isBuffer(capturedPayload.contextBuffer));
  assert.ok(Buffer.isBuffer(capturedPayload.guideBuffer));
  assert.ok(Buffer.isBuffer(capturedPayload.maskBuffer));
  assert.strictEqual(typeof capturedPayload.contextW, 'number');
  assert.strictEqual(typeof capturedPayload.contextH, 'number');
  assert.strictEqual(capturedPayload.selOnContext.width, 64);
  assert.strictEqual(capturedPayload.selOnContext.height, 64);
});

test('forgeCanvasRegion calculates offset { dx: 0, dy: 0 } for internal edit', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 16, y: 16, width: 32, height: 32 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
  assert.strictEqual(res.dimensions, undefined);
});

test('forgeCanvasRegion calculates positive dx offset when expanding West into Void (selection.x < 0)', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: -64, y: 16, width: 64, height: 32 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  assert.deepStrictEqual(res.offset, { dx: 64, dy: 0 });
  assert.strictEqual(res.dimensions, undefined);
});

test('forgeCanvasRegion calculates positive dy offset when expanding North into Void (selection.y < 0)', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 16, y: -48, width: 32, height: 48 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  assert.deepStrictEqual(res.offset, { dx: 0, dy: 48 });
  assert.strictEqual(res.dimensions, undefined);
});

test('forgeCanvasRegion calculates positive dx and dy when expanding North and West into Void', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: -32, y: -64, width: 64, height: 64 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  assert.deepStrictEqual(res.offset, { dx: 32, dy: 64 });
  assert.strictEqual(res.dimensions, undefined);
});

test('forgeCanvasRegion calculates offset { dx: 0, dy: 0 } when expanding East and South into Void', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 128, y: 128, width: 64, height: 64 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => ({ imageBuffer: contextBuffer })
  });

  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
  assert.strictEqual(res.dimensions, undefined);
});

test('forgeRegion in battlemapService delegates to forgeCanvasRegion with in-memory renderAdapter', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: -64, y: 32, width: 64, height: 64 };
  let adapterCalled = false;

  const res = await forgeRegion({
    originalDataUrl,
    selection,
    prompt: 'Expand the dungeon westwards',
    renderAdapter: async (payload) => {
      adapterCalled = true;
      return {
        imageBuffer: payload.contextBuffer,
        debug: { delegated: true },
        usage: { prompt_tokens: 15, completion_tokens: 25, total_tokens: 40, cost: 0.002 },
        cost: 0.002
      };
    }
  });

  assert.strictEqual(adapterCalled, true);
  assert.ok(res.imageDataUrl.startsWith('data:image/png;base64,'));
  assert.deepStrictEqual(res.offset, { dx: 64, dy: 0 });
  assert.strictEqual(res.dimensions, undefined, 'forgeRegion must not expose dimensions');
  assert.deepStrictEqual(res.debug, { delegated: true });
  assert.strictEqual(res.usage.total_tokens, 40);
  assert.strictEqual(res.cost, 0.002);
});

test('forgeCanvasRegion accepts raw Buffer directly from renderAdapter', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 0, y: 0, width: 64, height: 64 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: async ({ contextBuffer }) => contextBuffer
  });

  assert.ok(res.imageDataUrl);
  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
});

test('forgeCanvasRegion defaults to in-memory pass-through when renderAdapter is omitted', async () => {
  const originalDataUrl = await createTestImage(128, 128);
  const selection = { x: 0, y: 0, width: 64, height: 64 };

  const res = await forgeCanvasRegion({
    originalDataUrl,
    selection
  });

  assert.ok(res.imageDataUrl);
  assert.deepStrictEqual(res.offset, { dx: 0, dy: 0 });
});
