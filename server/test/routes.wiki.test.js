const test = require('node:test');
const assert = require('node:assert');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
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

test('POST /api/wiki/regions/suggest validation', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/wiki/regions/suggest`, {
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

test('POST /api/wiki/article validations and lifecycle', async () => {
  const tmpWiki = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-forge-route-wiki-test-'));
  const originalEnv = process.env.WIKI_PATH;
  process.env.WIKI_PATH = tmpWiki;

  const { server, baseUrl } = await startTestServer(app);
  try {
    // 1. Missing params -> 400
    const resMissing = await fetch(`${baseUrl}/api/wiki/article`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({})
    });
    assert.strictEqual(resMissing.status, 400);
    const dataMissing = await resMissing.json();
    assert.strictEqual(dataMissing.code, 'INVALID_PARAMETERS');

    // 2. Path traversal -> 403
    const resTraversal = await fetch(`${baseUrl}/api/wiki/article`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        relPath: '../../etc/passwd',
        content: 'evil content'
      })
    });
    assert.strictEqual(resTraversal.status, 403);
    const dataTraversal = await resTraversal.json();
    assert.strictEqual(dataTraversal.code, 'ACCESS_DENIED');

    // 3. Valid creation -> 200
    const resCreate = await fetch(`${baseUrl}/api/wiki/article`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        relPath: 'Custom/TestLocation.md',
        content: '# Test Location\n\nA mystical testing sanctuary.',
        title: 'Test Location'
      })
    });
    assert.strictEqual(resCreate.status, 200);
    const dataCreate = await resCreate.json();
    assert.strictEqual(dataCreate.ok, true);
    assert.strictEqual(dataCreate.article.path, 'Custom/TestLocation.md');
    assert.strictEqual(dataCreate.article.title, 'Test Location');

    // 4. Duplicate without overwrite -> 409
    const resDup = await fetch(`${baseUrl}/api/wiki/article`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        relPath: 'Custom/TestLocation.md',
        content: '# Duplicate'
      })
    });
    assert.strictEqual(resDup.status, 409);
    const dataDup = await resDup.json();
    assert.strictEqual(dataDup.code, 'ARTICLE_ALREADY_EXISTS');
  } finally {
    process.env.WIKI_PATH = originalEnv;
    server.close();
    fs.rmSync(tmpWiki, { recursive: true, force: true });
  }
});
