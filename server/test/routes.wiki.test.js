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

  const tmpWiki = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-forge-wiki-route-test-'));
  process.env.WIKI_PATH = tmpWiki;
  const placesDir = path.join(tmpWiki, 'Regions', 'Aeloria', 'Places');
  fs.mkdirSync(placesDir, { recursive: true });
  fs.writeFileSync(
    path.join(placesDir, "Storm's Eye Temple.md"),
    "# Storm's Eye Temple\n\nAncient stone temple on a flooded crag.",
    'utf8'
  );

test('GET /api/wiki/tree returns regions, adventures, and custom sections', async () => {
  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/wiki/tree`);
    assert.strictEqual(res.status, 200);
    const data = await res.json();
    assert.strictEqual(data.ok, true);
    assert.ok(Array.isArray(data.regions));
    assert.ok(Array.isArray(data.adventures));
    assert.ok(Array.isArray(data.custom));
  } finally {
    server.close();
  }
});

test('POST /api/wiki/regions/suggest generates tactical suggestions with live key', async (t) => {
  if (!process.env.OPENROUTER_API_KEY) {
    t.skip('Skipping live region suggest test: OPENROUTER_API_KEY not configured');
    return;
  }

  const { server, baseUrl } = await startTestServer(app);
  try {
    const res = await fetch(`${baseUrl}/api/wiki/regions/suggest`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        articlePath: "Regions/Aeloria/Places/Storm's Eye Temple.md",
        selection: { x: 1024, y: 0, width: 512, height: 512 },
        mapDimensions: { width: 1024, height: 1024 },
        mapTitle: "Storm's Eye Temple",
        basePrompt: "Flooded stone temple"
      })
    });

    assert.strictEqual(res.status, 200);
    const data = await res.json();
    assert.ok(Array.isArray(data.suggestions));
    assert.ok(data.suggestions.length >= 1);
    const first = data.suggestions[0];
    assert.ok(first.title);
    assert.match(first.category, /East expansion/i);
    assert.ok(first.forgePrompt);
    assert.ok(Array.isArray(first.tacticalHighlights));
  } finally {
    server.close();
  }
});
