const test = require('node:test');
const assert = require('node:assert');
const http = require('http');
const app = require('../index');

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

test('POST /api/features/detect returns 400 when mapDataUrl is missing', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/features/detect`, {
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

test('POST /api/features/forge-state returns 400 when originalDataUrl or box is missing', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/features/forge-state`, {
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

test('POST /api/features/:id/forge-state returns 400 when originalDataUrl or box is missing', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/features/door-1/forge-state`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ originalDataUrl: 'data:image/png;base64,AAAA' })
    });
    assert.strictEqual(res.status, 400);
    const data = await res.json();
    assert.strictEqual(data.code, 'INVALID_PARAMETERS');
  } finally {
    server.close();
  }
});

for (const routePath of ['/api/features/forge-state', '/api/features/door-1/forge-state']) {
  test(`POST ${routePath} invokes featureService.forgeFeatureState and returns the patch`, async (t) => {
    const sharp = require('sharp');
    const { bufferToDataUrl } = require('../services/canvasForge');
    const pngBase64 = (await sharp({
      create: { width: 64, height: 64, channels: 3, background: { r: 200, g: 20, b: 20 } }
    }).png().toBuffer()).toString('base64');
    const mapDataUrl = bufferToDataUrl(await sharp({
      create: { width: 128, height: 128, channels: 3, background: { r: 50, g: 50, b: 50 } }
    }).png().toBuffer());

    const originalFetch = global.fetch;
    const originalKey = process.env.OPENROUTER_API_KEY;
    const upstreamBodies = [];
    process.env.OPENROUTER_API_KEY = 'test-key';
    global.fetch = async (url, opts) => {
      if (String(url).includes('openrouter.ai')) {
        upstreamBodies.push(JSON.parse(opts.body));
        return {
          ok: true,
          status: 200,
          json: async () => ({
            choices: [{ finish_reason: 'stop', message: { images: [{ data: pngBase64, mime_type: 'image/png' }] } }],
            usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 }
          })
        };
      }
      return originalFetch(url, opts);
    };
    t.after(() => {
      global.fetch = originalFetch;
      if (originalKey === undefined) delete process.env.OPENROUTER_API_KEY;
      else process.env.OPENROUTER_API_KEY = originalKey;
    });

    const { server, baseUrl } = await startTestServer(app);
    try {
      const res = await fetch(`${baseUrl}${routePath}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          originalDataUrl: mapDataUrl,
          box: { x: 32, y: 32, width: 64, height: 64 },
          prompt: 'Open the door'
        })
      });
      assert.strictEqual(res.status, 200);
      const data = await res.json();
      assert.ok(data.patchDataUrl.startsWith('data:image/png;base64,'));
      assert.deepStrictEqual(data.offset, { dx: 0, dy: 0 });
      assert.strictEqual(upstreamBodies.length, 1, 'provider was invoked once through featureService');
    } finally {
      server.close();
    }
  });
}
