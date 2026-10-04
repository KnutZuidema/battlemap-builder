const fs = require('fs');
const path = require('path');

const WIKI_DEFAULT_PATH = path.join(process.env.HOME || '/home/knut', 'Documents/D&D campaign/Wiki');

function getWikiRoot() {
  return process.env.WIKI_PATH ? path.resolve(process.env.WIKI_PATH) : WIKI_DEFAULT_PATH;
}

function resolveSafeWikiPath(relPath) {
  if (!relPath || typeof relPath !== 'string' || !relPath.trim()) {
    const err = new Error('Invalid path: path must be a non-empty string');
    err.code = 'INVALID_PATH';
    throw err;
  }
  const root = getWikiRoot();
  if (path.isAbsolute(relPath)) {
    const err = new Error(`Access denied: path traversal detected for "${relPath}"`);
    err.code = 'ACCESS_DENIED';
    throw err;
  }
  const resolved = path.resolve(root, relPath);
  const relative = path.relative(root, resolved);
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    const err = new Error(`Access denied: path traversal detected for "${relPath}"`);
    err.code = 'ACCESS_DENIED';
    throw err;
  }
  return resolved;
}

function slugify(text) {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '');
}

function extractMarkdownTitle(content, defaultTitle) {
  if (!content) return defaultTitle;
  const fmMatch = content.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (fmMatch) {
    const titleMatch = fmMatch[1].match(/^title:\s*["']?([^"'\r\n]+)["']?/m);
    if (titleMatch && titleMatch[1].trim()) return titleMatch[1].trim();
  }
  const headingMatch = content.match(/^#\s+([^\r\n#]+)/m);
  if (headingMatch && headingMatch[1].trim()) return headingMatch[1].trim();
  return defaultTitle;
}

async function getWikiTree() {
  const root = getWikiRoot();
  if (!fs.existsSync(root)) {
    const err = new Error(`Wiki directory not found at ${root}`);
    err.code = 'WIKI_NOT_FOUND';
    throw err;
  }

  // 1. Scan Regions
  const regionsDir = path.join(root, 'Regions');
  const regions = [];

  if (fs.existsSync(regionsDir)) {
    const regionEntries = await fs.promises.readdir(regionsDir, { withFileTypes: true });
    for (const entry of regionEntries) {
      if (!entry.isDirectory()) continue;
      const regionName = entry.name;
      const regionPath = path.join(regionsDir, regionName);

      // Scan Places
      const places = [];
      const placesDir = path.join(regionPath, 'Places');
      if (fs.existsSync(placesDir)) {
        const placeFiles = await fs.promises.readdir(placesDir, { withFileTypes: true });
        for (const file of placeFiles) {
          if (file.isFile() && file.name.endsWith('.md')) {
            const filePath = path.join(placesDir, file.name);
            const content = await fs.promises.readFile(filePath, 'utf8').catch(() => '');
            const title = extractMarkdownTitle(content, file.name.replace(/\.md$/, ''));
            const relPath = path.relative(root, filePath);
            places.push({
              id: slugify(`${regionName}-place-${file.name}`),
              name: title,
              path: relPath,
              type: 'place'
            });
          }
        }
        places.sort((a, b) => a.name.localeCompare(b.name));
      }

      // Scan Settlements
      const settlements = [];
      const settlementsDir = path.join(regionPath, 'Settlements');
      if (fs.existsSync(settlementsDir)) {
        const settlementFiles = await fs.promises.readdir(settlementsDir, { withFileTypes: true });
        for (const file of settlementFiles) {
          if (file.isFile() && file.name.endsWith('.md')) {
            const filePath = path.join(settlementsDir, file.name);
            const content = await fs.promises.readFile(filePath, 'utf8').catch(() => '');
            const title = extractMarkdownTitle(content, file.name.replace(/\.md$/, ''));
            const relPath = path.relative(root, filePath);
            settlements.push({
              id: slugify(`${regionName}-settlement-${file.name}`),
              name: title,
              path: relPath,
              type: 'settlement'
            });
          }
        }
        settlements.sort((a, b) => a.name.localeCompare(b.name));
      }

      regions.push({
        id: slugify(`region-${regionName}`),
        name: regionName,
        path: `Regions/${regionName}`,
        places,
        settlements
      });
    }
    regions.sort((a, b) => a.name.localeCompare(b.name));
  }

  // 2. Scan Adventures
  const adventuresDir = path.join(root, 'Adventures');
  const adventures = [];

  async function getMarkdownFilesRecursively(dir) {
    let files = [];
    const entries = await fs.promises.readdir(dir, { withFileTypes: true });
    for (const item of entries) {
      const full = path.join(dir, item.name);
      if (item.isDirectory()) {
        const sub = await getMarkdownFilesRecursively(full);
        files = files.concat(sub);
      } else if (item.isFile() && item.name.endsWith('.md')) {
        files.push(full);
      }
    }
    return files;
  }

  if (fs.existsSync(adventuresDir)) {
    const advEntries = await fs.promises.readdir(adventuresDir, { withFileTypes: true });
    const advMap = new Map();

    for (const entry of advEntries) {
      if (entry.isDirectory()) {
        const dirName = entry.name;
        const fullDir = path.join(adventuresDir, dirName);
        const mdFiles = await getMarkdownFilesRecursively(fullDir);
        const chapters = [];
        for (const f of mdFiles) {
          const content = await fs.promises.readFile(f, 'utf8').catch(() => '');
          const defaultName = path.basename(f, '.md');
          const title = extractMarkdownTitle(content, defaultName);
          const relPath = path.relative(root, f);
          chapters.push({
            id: slugify(`adv-${dirName}-${path.basename(f)}`),
            name: title,
            path: relPath
          });
        }
        chapters.sort((a, b) => a.path.localeCompare(b.path));

        const existing = advMap.get(dirName) || {};
        advMap.set(dirName, {
          ...existing,
          id: slugify(`adventure-${dirName}`),
          name: dirName,
          path: `Adventures/${dirName}`,
          chapters
        });
      } else if (entry.isFile() && entry.name.endsWith('.md')) {
        const rawName = entry.name.replace(/\.md$/, '');
        const filePath = path.join(adventuresDir, entry.name);
        const content = await fs.promises.readFile(filePath, 'utf8').catch(() => '');
        const title = extractMarkdownTitle(content, rawName);
        const relPath = path.relative(root, filePath);

        if (advMap.has(rawName)) {
          const existing = advMap.get(rawName);
          existing.overviewPath = relPath;
        } else {
          advMap.set(rawName, {
            id: slugify(`adventure-${rawName}`),
            name: title,
            path: relPath,
            chapters: []
          });
        }
      }
    }

    for (const val of advMap.values()) {
      adventures.push(val);
    }
    adventures.sort((a, b) => a.name.localeCompare(b.name));
  }

  // 3. Scan Custom
  const customDir = path.join(root, 'Custom');
  const custom = [];

  if (fs.existsSync(customDir)) {
    const customFiles = await getMarkdownFilesRecursively(customDir);
    for (const f of customFiles) {
      const content = await fs.promises.readFile(f, 'utf8').catch(() => '');
      const defaultName = path.basename(f, '.md');
      const title = extractMarkdownTitle(content, defaultName);
      const relPath = path.relative(root, f).replace(/\\/g, '/');
      custom.push({
        id: slugify(`custom-${path.relative(customDir, f).replace(/\.md$/, '')}`),
        name: title,
        path: relPath,
        type: 'custom'
      });
    }
    custom.sort((a, b) => a.name.localeCompare(b.name));
  }

  return {
    ok: true,
    root,
    regions,
    adventures,
    custom
  };
}

async function getWikiArticle(relPath) {
  if (!relPath) {
    const err = new Error('Missing article path');
    err.code = 'INVALID_PATH';
    throw err;
  }

  const safePath = resolveSafeWikiPath(relPath);
  if (!fs.existsSync(safePath)) {
    const err = new Error(`Wiki article not found at ${relPath}`);
    err.code = 'WIKI_ARTICLE_NOT_FOUND';
    throw err;
  }

  const content = await fs.promises.readFile(safePath, 'utf8');
  const defaultTitle = path.basename(relPath, '.md');
  const title = extractMarkdownTitle(content, defaultTitle);

  // Check parent region lore if under Regions/<RegionName>/
  let parentRegion = null;
  const normalized = relPath.replace(/\\/g, '/');
  const regionMatch = normalized.match(/^Regions\/([^/]+)\//);
  if (regionMatch) {
    const regionName = regionMatch[1];
    const regionFile = path.join(getWikiRoot(), 'Regions', `${regionName}.md`);
    let contentExcerpt = null;
    if (fs.existsSync(regionFile)) {
      const regionLore = await fs.promises.readFile(regionFile, 'utf8').catch(() => '');
      contentExcerpt = regionLore.slice(0, 2000);
    }
    parentRegion = {
      name: regionName,
      contentExcerpt
    };
  }

  return {
    title,
    path: relPath,
    content,
    parentRegion
  };
}
module.exports = {
  getWikiRoot,
  resolveSafeWikiPath,
  extractMarkdownTitle,
  getWikiTree,
  getWikiArticle
};
