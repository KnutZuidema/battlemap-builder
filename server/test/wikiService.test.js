const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');

const {
  getWikiRoot,
  resolveSafeWikiPath,
  getWikiTree,
  getWikiArticle
} = require('../services/wikiService');

test('resolveSafeWikiPath rejects traversal and absolute paths with ACCESS_DENIED', () => {
  assert.throws(() => {
    resolveSafeWikiPath('../../etc/passwd');
  }, (err) => {
    return err.code === 'ACCESS_DENIED';
  });

  assert.throws(() => {
    resolveSafeWikiPath('/etc/passwd');
  }, (err) => {
    return err.code === 'ACCESS_DENIED';
  });

  assert.throws(() => {
    resolveSafeWikiPath('Custom/../../outside.md');
  }, (err) => {
    return err.code === 'ACCESS_DENIED';
  });
});

test('getWikiTree and getWikiArticle discover and read articles', async () => {
  const tmpWiki = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-forge-wiki-test-'));
  const originalEnv = process.env.WIKI_PATH;
  process.env.WIKI_PATH = tmpWiki;

  try {
    const placesDir = path.join(tmpWiki, 'Regions', 'Aeloria', 'Places');
    fs.mkdirSync(placesDir, { recursive: true });
    fs.writeFileSync(
      path.join(placesDir, 'HiddenSanctuary.md'),
      '# The Hidden Sanctuary\n\nA quiet refuge carved into stone.',
      'utf8'
    );

    const tree = await getWikiTree();
    assert.strictEqual(tree.ok, true);
    assert.ok(Array.isArray(tree.regions));
    assert.strictEqual(tree.regions.length, 1);
    assert.strictEqual(tree.regions[0].places[0].name, 'The Hidden Sanctuary');

    const article = await getWikiArticle('Regions/Aeloria/Places/HiddenSanctuary.md');
    assert.strictEqual(article.title, 'The Hidden Sanctuary');
    assert.match(article.content, /quiet refuge/);
  } finally {
    if (originalEnv) process.env.WIKI_PATH = originalEnv;
    else delete process.env.WIKI_PATH;
    fs.rmSync(tmpWiki, { recursive: true, force: true });
  }
});
