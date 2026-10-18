const test = require('node:test');
const assert = require('node:assert');
const http = require('http');
const sharp = require('sharp');
const app = require('../index');
const { deleteRegion: imageProcessorDeleteRegion, bufferToDataUrl } = require('../services/imageProcessor');

function startTestServer(serverApp) {
  return new Promise((resolve) => {
    const server = http.createServer(serverApp);
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port;
      resolve({
        server,
        baseUrl: `http://127.0.0.1:${port}`
      });
    });
  });
}

async function createTestImage(width = 128, height = 128, color = { r: 255, g: 100, b: 50, alpha: 1 }) {
  const buf = await sharp({
    create: { width, height, channels: 4, background: color }
  }).png().toBuffer();
  return bufferToDataUrl(buf);
}

test('imageProcessor deleteRegion sets selected region pixels to transparent', async () => {
  const dataUrl = await createTestImage(128, 128);
  const selection = { x: 32, y: 32, width: 64, height: 64 };

  const resultDataUrl = await imageProcessorDeleteRegion(dataUrl, selection);
  assert.ok(resultDataUrl.startsWith('data:image/png;base64,'));

  const base64 = resultDataUrl.replace(/^data:image\/png;base64,/, '');
  const resultBuf = Buffer.from(base64, 'base64');

  const { data, info } = await sharp(resultBuf).raw().toBuffer({ resolveWithObject: true });
  assert.strictEqual(info.width, 128);
  assert.strictEqual(info.height, 128);
  assert.strictEqual(info.channels, 4);

  // Check pixel at (10, 10): untouched, alpha should be 255
  const outsideIdx = (10 * 128 + 10) * 4;
  assert.strictEqual(data[outsideIdx + 3], 255);
  assert.strictEqual(data[outsideIdx], 255);
  assert.strictEqual(data[outsideIdx + 1], 100);
  assert.strictEqual(data[outsideIdx + 2], 50);

  // Check pixel at (40, 40): inside deleted region, alpha should be 0
  const insideIdx = (40 * 128 + 40) * 4;
  assert.strictEqual(data[insideIdx + 3], 0);

  // Check corner pixels of selection
  const insideCorner = (32 * 128 + 32) * 4;
  assert.strictEqual(data[insideCorner + 3], 0);
});

test('imageProcessor deleteRegion returns original image if selection has no intersection', async () => {
  const dataUrl = await createTestImage(64, 64);
  const selection = { x: 100, y: 100, width: 64, height: 64 };

  const resultDataUrl = await imageProcessorDeleteRegion(dataUrl, selection);
  assert.strictEqual(resultDataUrl, dataUrl);
});


test('POST /api/delete-region returns 400 when parameters are missing', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/delete-region`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({})
    });
    assert.strictEqual(res.status, 400);
    const data = await res.json();
    assert.strictEqual(data.code, 'INVALID_PARAMETERS');
  } finally {
    server.close();
  }
});

test('POST /api/delete-region successfully clears region', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const dataUrl = await createTestImage(64, 64);
    const selection = { x: 16, y: 16, width: 32, height: 32 };

    const res = await fetch(`${baseUrl}/api/delete-region`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ originalDataUrl: dataUrl, selection })
    });
    assert.strictEqual(res.status, 200);
    const data = await res.json();
    assert.ok(data.imageDataUrl);
    assert.deepStrictEqual(data.selection, selection);
  } finally {
    server.close();
  }
});
