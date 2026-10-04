const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');

const {
  getWikiRoot,
  resolveSafeWikiPath,
  createWikiArticle,
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

test('createWikiArticle validates required parameters', async () => {
  await assert.rejects(async () => {
    await createWikiArticle({});
  }, (err) => err.code === 'INVALID_PARAMETERS');

  await assert.rejects(async () => {
    await createWikiArticle({ relPath: 'Custom/Test.md' });
  }, (err) => err.code === 'INVALID_PARAMETERS');

  await assert.rejects(async () => {
    await createWikiArticle({ content: 'Hello' });
  }, (err) => err.code === 'INVALID_PARAMETERS');
});

test('createWikiArticle and getWikiTree with Custom directory', async () => {
  const tmpWiki = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-forge-wiki-test-'));
  const originalEnv = process.env.WIKI_PATH;
  process.env.WIKI_PATH = tmpWiki;

  try {
    // Create custom article
    const res = await createWikiArticle({
      relPath: 'Custom/SecretCave.md',
      content: '# The Secret Cave\n\nA damp cavern hidden behind moss.',
      title: 'The Secret Cave'
    });

    assert.strictEqual(res.ok, true);
    assert.strictEqual(res.path, 'Custom/SecretCave.md');
    assert.strictEqual(res.title, 'The Secret Cave');

    // Verify file exists on disk
    const onDisk = fs.readFileSync(path.join(tmpWiki, 'Custom/SecretCave.md'), 'utf8');
    assert.match(onDisk, /The Secret Cave/);

    // Cannot overwrite without overwrite=true
    await assert.rejects(async () => {
      await createWikiArticle({
        relPath: 'Custom/SecretCave.md',
        content: '# Overwritten'
      });
    }, (err) => err.code === 'ARTICLE_ALREADY_EXISTS');

    // Can overwrite with overwrite=true
    const overwriteRes = await createWikiArticle({
      relPath: 'Custom/SecretCave.md',
      content: '# The Updated Secret Cave\n\nUpdated text.',
      overwrite: true
    });
    assert.strictEqual(overwriteRes.title, 'The Updated Secret Cave');

    // Test getWikiTree finds Custom article
    const tree = await getWikiTree();
    assert.strictEqual(tree.ok, true);
    assert.ok(Array.isArray(tree.custom));
    assert.strictEqual(tree.custom.length, 1);
    assert.strictEqual(tree.custom[0].name, 'The Updated Secret Cave');
    assert.strictEqual(tree.custom[0].path, 'Custom/SecretCave.md');
    assert.strictEqual(tree.custom[0].type, 'custom');

    // Test getWikiArticle loads the created custom article
    const article = await getWikiArticle('Custom/SecretCave.md');
    assert.strictEqual(article.title, 'The Updated Secret Cave');
    assert.match(article.content, /Updated text/);
  } finally {
    if (originalEnv) process.env.WIKI_PATH = originalEnv;
    else delete process.env.WIKI_PATH;
    fs.rmSync(tmpWiki, { recursive: true, force: true });
  }
});
