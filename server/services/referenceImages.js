const fs = require('fs');
const path = require('path');
const sharp = require('sharp');

/**
 * Catalog of curated 2D orthographic plan-view battlemap reference images.
 * Used for few-shot visual conditioning so image models render flat sliced poché walls
 * rather than 3D/isometric high-angle perspective.
 */
const REFERENCE_DEFINITIONS = [
  {
    id: 'stone-dungeon',
    filename: 'battlemap-reference-4.jpeg',
    label: 'Perspective Reference: 2D Stone Dungeon Plan View',
    keywords: [
      'dungeon', 'crypt', 'tomb', 'cave', 'cavern', 'cell', 'prison',
      'stone', 'underground', 'vault', 'catacomb', 'temple', 'sewer',
      'mine', 'labyrinth', 'shrine', 'altar', 'corridor'
    ]
  },
  {
    id: 'ruins-courtyard',
    // Curated ungridded reference: 1024x1024 px, 1:1 square, 2D top-down plan view, strictly ungridded.
    filename: 'battlemap-reference-3.png',
    fallbackFilename: 'battlemap-reference-4.jpeg',
    label: 'Perspective Reference: 2D Ruins & Courtyard Plan View',
    keywords: [
      'ruin', 'ruins', 'courtyard', 'river', 'bridge', 'stream', 'outdoor',
      'forest', 'garden', 'trees', 'road', 'path', 'plaza', 'swamp',
      'camp', 'nature', 'water', 'waterfall', 'cliff'
    ]
  },
  {
    id: 'tavern-inn',
    filename: 'battlemap-reference-1.jpeg',
    label: 'Perspective Reference: 2D Tavern & Rooms Plan View',
    keywords: [
      'tavern', 'inn', 'bar', 'pub', 'bed', 'bedroom', 'room', 'house',
      'timber', 'wood', 'furniture', 'kitchen', 'cabin', 'cottage', 'innkeeper'
    ]
  },
  {
    id: 'grand-hall-portal',
    filename: 'battlemap-reference-2.jpg',
    label: 'Perspective Reference: 2D Grand Hall & Portal Plan View',
    keywords: [
      'portal', 'hall', 'great hall', 'market', 'shop', 'chasm', 'pit',
      'arena', 'keep', 'fortress', 'mansion', 'castle', 'throne'
    ]
  }
];

// In-memory cache for processed reference image buffers
const bufferCache = new Map();

/**
 * Resolves the directory holding the reference images.
 * Checks project root `data/` first, then `server/assets/references/`.
 */
function getReferencesDir() {
  const candidates = [
    path.resolve(__dirname, '../../data'),
    path.resolve(__dirname, '../assets/references')
  ];

  for (const dir of candidates) {
    if (fs.existsSync(dir)) {
      return dir;
    }
  }

  return candidates[0];
}

/**
 * Loads, crops (if needed), resizes to target dimension (default 512x512) and
 * caches the reference image buffer in JPEG format for efficient API payload delivery.
 */
async function loadReferenceBuffer(def, targetSize = 512) {
  const cacheKey = `${def.id}-${targetSize}`;
  if (bufferCache.has(cacheKey)) {
    return bufferCache.get(cacheKey);
  }

  const dir = getReferencesDir();
  let filePath = path.join(dir, def.filename);
  if (!fs.existsSync(filePath) && def.fallbackFilename) {
    filePath = path.join(dir, def.fallbackFilename);
  }

  if (!fs.existsSync(filePath)) {
    console.warn(`[ReferenceImages] File not found: ${filePath}`);
    return null;
  }

  try {
    const rawBuffer = await fs.promises.readFile(filePath);
    const optimizedBuffer = await sharp(rawBuffer)
      .resize(targetSize, targetSize, { fit: 'cover' })
      .jpeg({ quality: 85, mozjpeg: true })
      .toBuffer();

    const item = {
      id: def.id,
      label: def.label,
      mimeType: 'image/jpeg',
      buffer: optimizedBuffer
    };

    bufferCache.set(cacheKey, item);
    return item;
  } catch (err) {
    console.error(`[ReferenceImages] Failed to load ${filePath}:`, err.message);
    return null;
  }
}

/**
 * Matches user prompt against reference keywords and returns the most relevant reference(s).
 *
 * @param {string} prompt - User request text
 * @param {Object} options
 * @param {number} [options.count=1] - Number of reference images to return (typically 1 or 2)
 * @param {number} [options.targetSize=512] - Pixel size for the reference image buffer
 * @returns {Promise<Array<{ label: string, mimeType: string, buffer: Buffer }>>}
 */
async function getReferenceImagesForPrompt(prompt, options = {}) {
  const { count = 1, targetSize = 512 } = options;
  const normalized = typeof prompt === 'string' ? prompt.toLowerCase() : '';

  // Score each definition by keyword occurrences in prompt
  const scored = REFERENCE_DEFINITIONS.map(def => {
    let score = 0;
    for (const kw of def.keywords) {
      // Word boundary match
      const regex = new RegExp(`\\b${kw}\\b`, 'i');
      if (regex.test(normalized)) {
        score += 1;
      }
    }
    return { def, score };
  });

  // Sort descending by score; break ties with default order (stone-dungeon is first)
  scored.sort((a, b) => b.score - a.score);

  const selectedDefs = [];
  for (let i = 0; i < Math.min(count, scored.length); i++) {
    selectedDefs.push(scored[i].def);
  }

  const images = [];
  for (const def of selectedDefs) {
    const loaded = await loadReferenceBuffer(def, targetSize);
    if (loaded) {
      images.push({
        label: loaded.label,
        mimeType: loaded.mimeType,
        buffer: loaded.buffer
      });
    }
  }

  return images;
}

module.exports = {
  REFERENCE_DEFINITIONS,
  getReferencesDir,
  loadReferenceBuffer,
  getReferenceImagesForPrompt
};
