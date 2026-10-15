const fs = require('fs');
const path = require('path');
const sharp = require('sharp');

/**
 * Returns the resolved directory where maps are stored.
 * Configurable via process.env.MAPS_DIR or defaulting to <projectRoot>/data/maps.
 */
function getMapsDir() {
  const dir = process.env.MAPS_DIR
    ? path.resolve(process.env.MAPS_DIR)
    : path.resolve(__dirname, '../../data/maps');
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  return dir;
}

/**
 * Validates map ID to prevent path traversal or invalid directory names.
 * Accepts only alphanumeric characters, hyphens, and underscores.
 */
function validateMapId(id) {
  if (typeof id !== 'string' || !id || !/^[a-zA-Z0-9_-]+$/.test(id)) {
    const err = new Error(`Invalid map ID: ${id}`);
    err.code = 'INVALID_MAP_ID';
    throw err;
  }
  return id;
}

/**
 * Generates a clean URL/FS-safe ID from title or random slug.
 */
function generateMapId(title) {
  const now = Date.now();
  let slug = '';
  if (typeof title === 'string') {
    slug = title
      .toLowerCase()
      .trim()
      .replace(/[^a-z0-9_-]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }
  if (!slug) {
    slug = Math.random().toString(36).substring(2, 8);
  } else if (slug.length > 32) {
    slug = slug.substring(0, 32).replace(/-+$/, '');
  }
  let finalId = `${now}-${slug}`;
  if (fs.existsSync(path.join(getMapsDir(), finalId))) {
    finalId = `${finalId}-${Math.random().toString(36).substring(2, 6)}`;
  }
  return finalId;
}

/**
 * Extracts a Buffer from a base64 data URL or raw base64 string.
 */
function extractImageBuffer(mapImage) {
  if (Buffer.isBuffer(mapImage)) return mapImage;
  if (typeof mapImage !== 'string' || !mapImage.trim()) {
    const err = new Error('Invalid or missing mapImage');
    err.code = 'INVALID_MAP_DATA';
    throw err;
  }
  const base64Data = mapImage.replace(/^data:image\/\w+;base64,/, '');
  return Buffer.from(base64Data, 'base64');
}

/**
 * Saves a new map to disk.
 *
 * @param {Object} mapData
 * @returns {Promise<Object>} Metadata object including id
 */
async function saveMap(mapData) {
  if (!mapData || typeof mapData !== 'object') {
    const err = new Error('Invalid map data payload');
    err.code = 'INVALID_MAP_DATA';
    throw err;
  }

  let id = mapData.id;
  if (id) {
    validateMapId(id);
  } else {
    id = generateMapId(mapData.title);
  }

  const imageBuffer = extractImageBuffer(mapData.mapImage);
  const mapsDir = getMapsDir();
  const mapDir = path.join(mapsDir, id);
  fs.mkdirSync(mapDir, { recursive: true });

  // 1. Save full-res image
  const imagePath = path.join(mapDir, 'image.png');
  await fs.promises.writeFile(imagePath, imageBuffer);

  // 2. Determine dimensions and generate 256x256 thumbnail
  let dimensions = mapData.dimensions;
  if (!dimensions || typeof dimensions.width !== 'number' || typeof dimensions.height !== 'number') {
    const metadata = await sharp(imageBuffer).metadata();
    dimensions = {
      width: metadata.width || 1024,
      height: metadata.height || 1024
    };
  }

  const thumbPath = path.join(mapDir, 'thumb.png');
  await sharp(imageBuffer)
    .resize(256, 256, { fit: 'cover' })
    .png()
    .toFile(thumbPath);

  // 3. Save map.json atomically
  const nowIso = new Date().toISOString();
  const metadata = {
    id,
    title: typeof mapData.title === 'string' ? mapData.title : 'Untitled Map',
    prompt: typeof mapData.prompt === 'string' ? mapData.prompt : '',
    dimensions,
    createdAt: mapData.createdAt || nowIso,
    updatedAt: mapData.updatedAt || nowIso,
    cost: typeof mapData.cost === 'number' ? mapData.cost : 0,
    features: Array.isArray(mapData.features) ? mapData.features : [],
    wikiContext: mapData.wikiContext || null,
    history: Array.isArray(mapData.history) ? mapData.history : [],
    historyIndex: typeof mapData.historyIndex === 'number' ? mapData.historyIndex : 0,
    version: '2.0'
  };

  const jsonPath = path.join(mapDir, 'map.json');
  const tmpJsonPath = path.join(mapDir, `map.json.tmp.${Date.now()}.${Math.random().toString(36).substring(2, 6)}`);
  await fs.promises.writeFile(tmpJsonPath, JSON.stringify(metadata, null, 2), 'utf-8');
  await fs.promises.rename(tmpJsonPath, jsonPath);

  return metadata;
}

/**
 * Lists all stored maps sorted by updatedAt descending.
 * Attaches thumbnailDataUrl for instant preview.
 *
 * @returns {Promise<Array<Object>>}
 */
async function listMaps() {
  const mapsDir = getMapsDir();
  const entries = await fs.promises.readdir(mapsDir, { withFileTypes: true });
  const maps = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const mapDir = path.join(mapsDir, entry.name);
    const jsonPath = path.join(mapDir, 'map.json');
    if (!fs.existsSync(jsonPath)) continue;

    try {
      const content = await fs.promises.readFile(jsonPath, 'utf-8');
      const meta = JSON.parse(content);

      let thumbnailDataUrl = null;
      const thumbPath = path.join(mapDir, 'thumb.png');
      if (fs.existsSync(thumbPath)) {
        const thumbBuf = await fs.promises.readFile(thumbPath);
        thumbnailDataUrl = `data:image/png;base64,${thumbBuf.toString('base64')}`;
      }

      maps.push({
        id: meta.id || entry.name,
        title: meta.title || '',
        prompt: meta.prompt || '',
        dimensions: meta.dimensions || null,
        createdAt: meta.createdAt || null,
        updatedAt: meta.updatedAt || null,
        cost: typeof meta.cost === 'number' ? meta.cost : 0,
        featureCount: Array.isArray(meta.features) ? meta.features.length : 0,
        thumbnailDataUrl
      });
    } catch (err) {
      // Ignore unparseable or corrupted directories
      continue;
    }
  }

  maps.sort((a, b) => {
    const timeA = a.updatedAt ? new Date(a.updatedAt).getTime() : 0;
    const timeB = b.updatedAt ? new Date(b.updatedAt).getTime() : 0;
    return timeB - timeA;
  });

  return maps;
}

/**
 * Loads a single map by ID, including its full-res mapImage as a data URL.
 *
 * @param {string} id
 * @returns {Promise<Object>}
 */
async function getMap(id) {
  validateMapId(id);
  const mapsDir = getMapsDir();
  const mapDir = path.join(mapsDir, id);
  const jsonPath = path.join(mapDir, 'map.json');

  if (!fs.existsSync(mapDir) || !fs.existsSync(jsonPath)) {
    const err = new Error(`Map not found: ${id}`);
    err.code = 'MAP_NOT_FOUND';
    throw err;
  }

  const content = await fs.promises.readFile(jsonPath, 'utf-8');
  const mapData = JSON.parse(content);

  const imagePath = path.join(mapDir, 'image.png');
  if (fs.existsSync(imagePath)) {
    const imageBuf = await fs.promises.readFile(imagePath);
    mapData.mapImage = `data:image/png;base64,${imageBuf.toString('base64')}`;
  } else {
    mapData.mapImage = null;
  }

  return mapData;
}

/**
 * Updates an existing map's metadata or image.
 *
 * @param {string} id
 * @param {Object} updates
 * @returns {Promise<Object>}
 */
async function updateMap(id, updates = {}) {
  validateMapId(id);
  const mapsDir = getMapsDir();
  const mapDir = path.join(mapsDir, id);
  const jsonPath = path.join(mapDir, 'map.json');

  if (!fs.existsSync(mapDir) || !fs.existsSync(jsonPath)) {
    const err = new Error(`Map not found: ${id}`);
    err.code = 'MAP_NOT_FOUND';
    throw err;
  }

  const content = await fs.promises.readFile(jsonPath, 'utf-8');
  const existing = JSON.parse(content);

  let updatedMapImage = null;
  if (updates.mapImage) {
    const imageBuffer = extractImageBuffer(updates.mapImage);
    const imagePath = path.join(mapDir, 'image.png');
    await fs.promises.writeFile(imagePath, imageBuffer);

    const thumbPath = path.join(mapDir, 'thumb.png');
    await sharp(imageBuffer)
      .resize(256, 256, { fit: 'cover' })
      .png()
      .toFile(thumbPath);

    if (!updates.dimensions) {
      const meta = await sharp(imageBuffer).metadata();
      existing.dimensions = {
        width: meta.width || existing.dimensions?.width || 1024,
        height: meta.height || existing.dimensions?.height || 1024
      };
    }
    updatedMapImage = updates.mapImage;
  }

  if (updates.title !== undefined) existing.title = updates.title;
  if (updates.prompt !== undefined) existing.prompt = updates.prompt;
  if (updates.dimensions !== undefined) existing.dimensions = updates.dimensions;
  if (updates.features !== undefined) existing.features = updates.features;
  if (updates.cost !== undefined) existing.cost = updates.cost;
  if (updates.history !== undefined) existing.history = updates.history;
  if (updates.historyIndex !== undefined) existing.historyIndex = updates.historyIndex;
  if (updates.wikiContext !== undefined) existing.wikiContext = updates.wikiContext;

  existing.updatedAt = new Date().toISOString();

  const tmpJsonPath = path.join(mapDir, `map.json.tmp.${Date.now()}.${Math.random().toString(36).substring(2, 6)}`);
  await fs.promises.writeFile(tmpJsonPath, JSON.stringify(existing, null, 2), 'utf-8');
  await fs.promises.rename(tmpJsonPath, jsonPath);

  const result = { ...existing };
  if (updatedMapImage) {
    result.mapImage = updatedMapImage;
  } else {
    const imagePath = path.join(mapDir, 'image.png');
    if (fs.existsSync(imagePath)) {
      const imageBuf = await fs.promises.readFile(imagePath);
      result.mapImage = `data:image/png;base64,${imageBuf.toString('base64')}`;
    }
  }

  return result;
}

/**
 * Deletes a map folder and all of its contents.
 *
 * @param {string} id
 * @returns {Promise<{ ok: boolean, id: string }>}
 */
async function deleteMap(id) {
  validateMapId(id);
  const mapsDir = getMapsDir();
  const mapDir = path.join(mapsDir, id);

  if (!fs.existsSync(mapDir)) {
    const err = new Error(`Map not found: ${id}`);
    err.code = 'MAP_NOT_FOUND';
    throw err;
  }

  fs.rmSync(mapDir, { recursive: true, force: true });
  return { ok: true, id };
}

/**
 * Duplicates an existing map into a new directory with a copy title and new ID.
 *
 * @param {string} id
 * @returns {Promise<Object>} New map metadata
 */
async function duplicateMap(id) {
  validateMapId(id);
  const mapsDir = getMapsDir();
  const srcDir = path.join(mapsDir, id);
  const srcJsonPath = path.join(srcDir, 'map.json');

  if (!fs.existsSync(srcDir) || !fs.existsSync(srcJsonPath)) {
    const err = new Error(`Map not found: ${id}`);
    err.code = 'MAP_NOT_FOUND';
    throw err;
  }

  const content = await fs.promises.readFile(srcJsonPath, 'utf-8');
  const existing = JSON.parse(content);

  const newTitle = `${existing.title || 'Untitled Map'} (Copy)`;
  const newId = generateMapId(newTitle);
  const destDir = path.join(mapsDir, newId);
  fs.mkdirSync(destDir, { recursive: true });

  const srcImage = path.join(srcDir, 'image.png');
  const destImage = path.join(destDir, 'image.png');
  if (fs.existsSync(srcImage)) {
    await fs.promises.copyFile(srcImage, destImage);
  }

  const srcThumb = path.join(srcDir, 'thumb.png');
  const destThumb = path.join(destDir, 'thumb.png');
  if (fs.existsSync(srcThumb)) {
    await fs.promises.copyFile(srcThumb, destThumb);
  }

  const nowIso = new Date().toISOString();
  const newMetadata = {
    ...existing,
    id: newId,
    title: newTitle,
    createdAt: nowIso,
    updatedAt: nowIso
  };

  const destJsonPath = path.join(destDir, 'map.json');
  const tmpJsonPath = path.join(destDir, `map.json.tmp.${Date.now()}.${Math.random().toString(36).substring(2, 6)}`);
  await fs.promises.writeFile(tmpJsonPath, JSON.stringify(newMetadata, null, 2), 'utf-8');
  await fs.promises.rename(tmpJsonPath, destJsonPath);

  return newMetadata;
}

module.exports = {
  getMapsDir,
  validateMapId,
  generateMapId,
  extractImageBuffer,
  saveMap,
  listMaps,
  getMap,
  updateMap,
  deleteMap,
  duplicateMap
};
