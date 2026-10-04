const test = require('node:test');
const assert = require('node:assert');
const { suggestRegionForging } = require('../services/wikiPrompter');

test('suggestRegionForging validates required parameters', async () => {
  await assert.rejects(async () => {
    await suggestRegionForging({});
  }, (err) => err.code === 'INVALID_PATH');

  await assert.rejects(async () => {
    await suggestRegionForging({ articlePath: 'Regions/Aeloria.md' });
  }, (err) => err.code === 'INVALID_SELECTION');

  await assert.rejects(async () => {
    await suggestRegionForging({
      articlePath: 'Regions/Aeloria.md',
      selection: { x: 'bad', y: 0, width: 100, height: 100 }
    });
  }, (err) => err.code === 'INVALID_SELECTION');
});
