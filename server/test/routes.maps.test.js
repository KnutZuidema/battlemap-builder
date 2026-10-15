const test = require('node:test');
const assert = require('node:assert');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const sharp = require('sharp');

// Set isolated temporary directory for routes tests
const tmpMapsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'routes-maps-test-'));
process.env.MAPS_DIR = tmpMapsDir;

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

async function createTestImage(width = 120, height = 120) {
  const buf = await sharp({
    create: {
      width,
      height,
      channels: 4,
      background: { r: 80, g: 160, b: 240, alpha: 1 }
    }
  }).png().toBuffer();
  return `data:image/png;base64,${buf.toString('base64')}`;
}

test.after(() => {
  try {
    fs.rmSync(tmpMapsDir, { recursive: true, force: true });
  } catch (err) {
    // Ignore cleanup error
  }
});

test('Maps API: full lifecycle (POST, GET list, GET id, PUT, DUPLICATE, DELETE)', async () => {
  const { server, baseUrl } = await startTestServer(app);

  try {
    const testImg = await createTestImage(200, 200);

    // 1. GET /api/maps initially empty
    const initRes = await fetch(`${baseUrl}/api/maps`);
    assert.strictEqual(initRes.status, 200);
    const initData = await initRes.json();
    assert.strictEqual(initData.ok, true);
    assert.deepStrictEqual(initData.maps, []);

    // 2. POST /api/maps: create map
    const createRes = await fetch(`${baseUrl}/api/maps`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        id: 'dragon-lair',
        title: "Red Dragon's Lair",
        prompt: 'Volcanic cavern with molten lava streams',
        mapImage: testImg,
        cost: 0.05,
        features: [{ id: 'f1', type: 'lava-pool' }]
      })
    });
    assert.strictEqual(createRes.status, 201);
    const createData = await createRes.json();
    assert.strictEqual(createData.ok, true);
    assert.strictEqual(createData.map.id, 'dragon-lair');
    assert.strictEqual(createData.map.title, "Red Dragon's Lair");

    // 3. GET /api/maps: list maps
    const listRes = await fetch(`${baseUrl}/api/maps`);
    assert.strictEqual(listRes.status, 200);
    const listData = await listRes.json();
    assert.strictEqual(listData.ok, true);
    assert.strictEqual(listData.maps.length, 1);
    assert.strictEqual(listData.maps[0].id, 'dragon-lair');
    assert.strictEqual(listData.maps[0].featureCount, 1);
    assert.ok(listData.maps[0].thumbnailDataUrl.startsWith('data:image/png;base64,'));

    // 4. GET /api/maps/:id: fetch full map
    const getRes = await fetch(`${baseUrl}/api/maps/dragon-lair`);
    assert.strictEqual(getRes.status, 200);
    const getData = await getRes.json();
    assert.strictEqual(getData.ok, true);
    assert.strictEqual(getData.map.id, 'dragon-lair');
    assert.strictEqual(getData.map.title, "Red Dragon's Lair");
    assert.ok(getData.map.mapImage.startsWith('data:image/png;base64,'));

    // 5. PUT /api/maps/:id: update map
    const putRes = await fetch(`${baseUrl}/api/maps/dragon-lair`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        title: "Awakened Red Dragon's Lair",
        cost: 0.10
      })
    });
    assert.strictEqual(putRes.status, 200);
    const putData = await putRes.json();
    assert.strictEqual(putData.ok, true);
    assert.strictEqual(putData.map.title, "Awakened Red Dragon's Lair");
    assert.strictEqual(putData.map.cost, 0.10);

    // Verify change persisted
    const verifyGet = await fetch(`${baseUrl}/api/maps/dragon-lair`);
    const verifyData = await verifyGet.json();
    assert.strictEqual(verifyData.map.title, "Awakened Red Dragon's Lair");

    // 6. POST /api/maps/:id/duplicate: duplicate map
    const dupRes = await fetch(`${baseUrl}/api/maps/dragon-lair/duplicate`, {
      method: 'POST'
    });
    assert.strictEqual(dupRes.status, 201);
    const dupData = await dupRes.json();
    assert.strictEqual(dupData.ok, true);
    assert.strictEqual(dupData.map.title, "Awakened Red Dragon's Lair (Copy)");
    assert.notStrictEqual(dupData.map.id, 'dragon-lair');

    // List should now have 2 maps
    const listAfterDup = await (await fetch(`${baseUrl}/api/maps`)).json();
    assert.strictEqual(listAfterDup.maps.length, 2);

    // 7. DELETE /api/maps/:id: delete original map
    const delRes = await fetch(`${baseUrl}/api/maps/dragon-lair`, {
      method: 'DELETE'
    });
    assert.strictEqual(delRes.status, 200);
    const delData = await delRes.json();
    assert.strictEqual(delData.ok, true);
    assert.strictEqual(delData.id, 'dragon-lair');

    // GET deleted map should return 404
    const getDeleted = await fetch(`${baseUrl}/api/maps/dragon-lair`);
    assert.strictEqual(getDeleted.status, 404);
    const deletedErr = await getDeleted.json();
    assert.strictEqual(deletedErr.ok, false);
    assert.strictEqual(deletedErr.code, 'MAP_NOT_FOUND');

    // Duplicate map should still exist
    const getDup = await fetch(`${baseUrl}/api/maps/${dupData.map.id}`);
    assert.strictEqual(getDup.status, 200);
  } finally {
    server.close();
  }
});

test('Maps API error handling: validates inputs and handles not found', async () => {
  const { server, baseUrl } = await startTestServer(app);

  try {
    // 1. POST /api/maps with missing mapImage -> 400
    const invalidPost = await fetch(`${baseUrl}/api/maps`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: 'No Image Map' })
    });
    assert.strictEqual(invalidPost.status, 400);
    const postErr = await invalidPost.json();
    assert.strictEqual(postErr.ok, false);
    assert.strictEqual(postErr.code, 'INVALID_MAP_DATA');

    // 2. GET /api/maps/non-existent -> 404
    const notFoundGet = await fetch(`${baseUrl}/api/maps/unknown-map-123`);
    assert.strictEqual(notFoundGet.status, 404);
    const notFoundData = await notFoundGet.json();
    assert.strictEqual(notFoundData.ok, false);
    assert.strictEqual(notFoundData.code, 'MAP_NOT_FOUND');

    // 3. GET /api/maps/invalid..path -> 400
    const traversalGet = await fetch(`${baseUrl}/api/maps/..%2Fsecret`);
    assert.strictEqual(traversalGet.status, 400);
    const traversalData = await traversalGet.json();
    assert.strictEqual(traversalData.ok, false);
    assert.strictEqual(traversalData.code, 'INVALID_MAP_ID');

    // 4. PUT /api/maps/non-existent -> 404
    const putNotFound = await fetch(`${baseUrl}/api/maps/unknown-map-123`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: 'New Title' })
    });
    assert.strictEqual(putNotFound.status, 404);

    // 5. DELETE /api/maps/non-existent -> 404
    const delNotFound = await fetch(`${baseUrl}/api/maps/unknown-map-123`, {
      method: 'DELETE'
    });
    assert.strictEqual(delNotFound.status, 404);

    // 6. POST /api/maps/non-existent/duplicate -> 404
    const dupNotFound = await fetch(`${baseUrl}/api/maps/unknown-map-123/duplicate`, {
      method: 'POST'
    });
    assert.strictEqual(dupNotFound.status, 404);
  } finally {
    server.close();
  }
});
