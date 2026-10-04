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
