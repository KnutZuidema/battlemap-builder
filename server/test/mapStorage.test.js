const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');
const sharp = require('sharp');

// Set temporary directory for MAPS_DIR before requiring mapStorage
const tmpMapsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'map-storage-unit-'));
process.env.MAPS_DIR = tmpMapsDir;

const {
  getMapsDir,
  validateMapId,
  generateMapId,
  saveMap,
  listMaps,
  getMap,
  updateMap,
  deleteMap,
  duplicateMap
} = require('../services/mapStorage');

async function createTestImage(width = 100, height = 100) {
  const buf = await sharp({
    create: {
      width,
      height,
      channels: 4,
      background: { r: 50, g: 120, b: 200, alpha: 1 }
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

test('validateMapId: permits valid alphanumeric IDs and hyphens/underscores', () => {
  assert.strictEqual(validateMapId('dungeon-123_abc'), 'dungeon-123_abc');
  assert.strictEqual(validateMapId('MAP01'), 'MAP01');
  assert.strictEqual(validateMapId('simple'), 'simple');
});

test('validateMapId: rejects path traversal and invalid characters with INVALID_MAP_ID', () => {
  const invalidIds = ['..', '../foo', 'foo/bar', 'foo\\bar', 'foo..bar', 'foo bar', 'foo@bar', '', null, undefined, 123];
  for (const invalid of invalidIds) {
    assert.throws(
      () => validateMapId(invalid),
      (err) => err.code === 'INVALID_MAP_ID',
      `Expected INVALID_MAP_ID for '${invalid}'`
    );
  }
});

test('saveMap: saves a map with explicit ID, creates image, thumbnail and metadata', async () => {
  const testImg = await createTestImage(300, 300);
  const mapData = {
    id: 'test-crypt-map',
    title: 'The Sunken Crypt',
    prompt: 'An ancient sunken crypt with water pooling on stone tiles',
    mapImage: testImg,
    features: [{ id: 'f1', type: 'door', box: { x: 10, y: 10, width: 20, height: 20 } }],
    cost: 0.04,
    wikiContext: 'Faerun Underdark',
    history: ['prompt 1'],
    historyIndex: 0,
    dimensions: { width: 300, height: 300 }
  };

  const saved = await saveMap(mapData);
  assert.strictEqual(saved.id, 'test-crypt-map');
  assert.strictEqual(saved.title, 'The Sunken Crypt');
  assert.strictEqual(saved.version, '2.0');
  assert.strictEqual(saved.cost, 0.04);
  assert.strictEqual(saved.features.length, 1);
  assert.deepStrictEqual(saved.dimensions, { width: 300, height: 300 });

  // Verify files on disk
  const mapDir = path.join(tmpMapsDir, 'test-crypt-map');
  assert.ok(fs.existsSync(path.join(mapDir, 'image.png')), 'image.png must exist');
  assert.ok(fs.existsSync(path.join(mapDir, 'thumb.png')), 'thumb.png must exist');
  assert.ok(fs.existsSync(path.join(mapDir, 'map.json')), 'map.json must exist');

  // Verify thumb.png is resized to 256x256
  const thumbMeta = await sharp(path.join(mapDir, 'thumb.png')).metadata();
  assert.strictEqual(thumbMeta.width, 256);
  assert.strictEqual(thumbMeta.height, 256);
});

test('saveMap: generates ID when not provided and resolves dimensions via sharp', async () => {
  const testImg = await createTestImage(180, 240);
  const saved = await saveMap({
    title: 'Goblin Hideout',
    prompt: 'A dark cave with goblin totems',
    mapImage: testImg
  });

  assert.ok(saved.id.includes('goblin-hideout'), `Generated ID '${saved.id}' should include title slug`);
  assert.strictEqual(saved.dimensions.width, 180);
  assert.strictEqual(saved.dimensions.height, 240);
  assert.strictEqual(saved.version, '2.0');
});

test('saveMap: throws INVALID_MAP_DATA when mapImage is missing', async () => {
  await assert.rejects(
    async () => saveMap({ title: 'No Image' }),
    (err) => err.code === 'INVALID_MAP_DATA'
  );
});

test('getMap: retrieves full map with mapImage data URL', async () => {
  const map = await getMap('test-crypt-map');
  assert.strictEqual(map.id, 'test-crypt-map');
  assert.strictEqual(map.title, 'The Sunken Crypt');
  assert.ok(typeof map.mapImage === 'string' && map.mapImage.startsWith('data:image/png;base64,'));
  assert.strictEqual(map.features.length, 1);
});

test('getMap: throws MAP_NOT_FOUND when map does not exist', async () => {
  await assert.rejects(
    async () => getMap('non-existent-id'),
    (err) => err.code === 'MAP_NOT_FOUND'
  );
});

test('getMap: throws INVALID_MAP_ID on path traversal', async () => {
  await assert.rejects(
    async () => getMap('../../secret'),
    (err) => err.code === 'INVALID_MAP_ID'
  );
});

test('listMaps: returns summaries with thumbnailDataUrl sorted by updatedAt desc', async () => {
  const maps = await listMaps();
  assert.ok(Array.isArray(maps));
  assert.ok(maps.length >= 2);

  const crypt = maps.find((m) => m.id === 'test-crypt-map');
  assert.ok(crypt, 'test-crypt-map should be listed');
  assert.strictEqual(crypt.title, 'The Sunken Crypt');
  assert.strictEqual(crypt.featureCount, 1);
  assert.ok(crypt.thumbnailDataUrl.startsWith('data:image/png;base64,'));
  assert.strictEqual(crypt.mapImage, undefined, 'Summary should not leak full mapImage');

  // Verify sort order
  for (let i = 0; i < maps.length - 1; i++) {
    const timeA = new Date(maps[i].updatedAt).getTime();
    const timeB = new Date(maps[i + 1].updatedAt).getTime();
    assert.ok(timeA >= timeB, 'Maps should be sorted by updatedAt desc');
  }
});

test('updateMap: updates metadata and regenerates image/thumbnail when mapImage provided', async () => {
  const newImg = await createTestImage(400, 400);
  const updated = await updateMap('test-crypt-map', {
    title: 'The Flooded Crypt',
    cost: 0.08,
    mapImage: newImg,
    features: [
      { id: 'f1', type: 'door' },
      { id: 'f2', type: 'chest' }
    ]
  });

  assert.strictEqual(updated.id, 'test-crypt-map');
  assert.strictEqual(updated.title, 'The Flooded Crypt');
  assert.strictEqual(updated.cost, 0.08);
  assert.strictEqual(updated.features.length, 2);
  assert.deepStrictEqual(updated.dimensions, { width: 400, height: 400 });

  // Verify persisted on disk
  const reloaded = await getMap('test-crypt-map');
  assert.strictEqual(reloaded.title, 'The Flooded Crypt');
  assert.strictEqual(reloaded.features.length, 2);
});

test('updateMap: throws MAP_NOT_FOUND when updating non-existent map', async () => {
  await assert.rejects(
    async () => updateMap('no-such-map', { title: 'New' }),
    (err) => err.code === 'MAP_NOT_FOUND'
  );
});

test('duplicateMap: creates a new map copy with title (Copy) and copies images', async () => {
  const copy = await duplicateMap('test-crypt-map');
  assert.notStrictEqual(copy.id, 'test-crypt-map');
  assert.strictEqual(copy.title, 'The Flooded Crypt (Copy)');
  assert.strictEqual(copy.features.length, 2);

  // Check files exist in new directory
  const copyDir = path.join(tmpMapsDir, copy.id);
  assert.ok(fs.existsSync(path.join(copyDir, 'image.png')));
  assert.ok(fs.existsSync(path.join(copyDir, 'thumb.png')));
  assert.ok(fs.existsSync(path.join(copyDir, 'map.json')));

  // Ensure full getMap works on duplicate
  const fullCopy = await getMap(copy.id);
  assert.strictEqual(fullCopy.title, 'The Flooded Crypt (Copy)');
  assert.ok(fullCopy.mapImage.startsWith('data:image/png;base64,'));
});

test('duplicateMap: throws MAP_NOT_FOUND when source does not exist', async () => {
  await assert.rejects(
    async () => duplicateMap('missing-map'),
    (err) => err.code === 'MAP_NOT_FOUND'
  );
});

test('deleteMap: removes map directory completely', async () => {
  const res = await deleteMap('test-crypt-map');
  assert.deepStrictEqual(res, { ok: true, id: 'test-crypt-map' });

  const mapDir = path.join(tmpMapsDir, 'test-crypt-map');
  assert.ok(!fs.existsSync(mapDir), 'Directory should be deleted');

  await assert.rejects(
    async () => getMap('test-crypt-map'),
    (err) => err.code === 'MAP_NOT_FOUND'
  );
});

test('deleteMap: throws MAP_NOT_FOUND when deleting already deleted or missing map', async () => {
  await assert.rejects(
    async () => deleteMap('test-crypt-map'),
    (err) => err.code === 'MAP_NOT_FOUND'
  );
});
