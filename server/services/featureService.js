const sharp = require('sharp');
const { parseUsage } = require('./wikiPrompter');
const { providerFailure, describeFailure, summarizeUpstreamError } = require('./failure');
const { dataUrlToBuffer, bufferToDataUrl, forgeCanvasRegion } = require('./canvasForge');
const { createForgeRegionAdapter } = require('./battlemapService');

const OPENROUTER_CHAT_URL = 'https://openrouter.ai/api/v1/chat/completions';
const FEATURE_DETECTION_MODEL = 'openai/gpt-6-luna';
const APPLICATION_TITLE = 'Dungeon Forge';
const REQUEST_TIMEOUT_MS = Number(process.env.OPENROUTER_TIMEOUT_MS) || 120000;

const FEATURE_DEFINITIONS = {
  door: {
    type: 'door',
    stateLabels: ['Closed', 'Open'],
    suggestedAltPrompt: 'Top-down orthographic plan view of the same door swung wide open, revealing the open threshold and floor beyond, matching surrounding masonry and 64px grid scale, strictly gridless.'
  },
  destructible_wall: {
    type: 'destructible_wall',
    stateLabels: ['Intact', 'Broken'],
    suggestedAltPrompt: 'Top-down orthographic plan view of the same wall blasted open into crumbling rubble and broken stones, creating an open breach connecting the rooms, matching surrounding floor and 64px grid scale, strictly gridless.'
  },
  carpet: {
    type: 'carpet',
    stateLabels: ['Covered', 'Revealed'],
    suggestedAltPrompt: 'Top-down orthographic plan view with the rug pulled aside and rolled back to reveal an open iron trapdoor with a ladder descending into darkness, matching surrounding floor and 64px grid scale, strictly gridless.'
  }
};

const SYSTEM_INSTRUCTION = `You are an expert tabletop RPG battlemap vision analysis AI.
Your task is to analyze top-down 2D orthographic battlemaps and identify interactive architectural features and hidden elements grounded in the battlemap's generation prompt and lore context.

MAP SCALE AND SPATIAL RULES:
- The battlemap is rendered at 64 pixels = 5 tabletop feet (a standard 1024x1024 map is 16x16 grid squares).
- Standard doors, secret hatches, and single wall breaches occupy approximately 64x64 to 128x128 pixels (1 to 2 grid squares).

FEATURE TYPES:
You must strictly classify features into ONE of these THREE finite types only:
1. "door" - Any single or double wooden, iron, reinforced, or secret door set in a wall or partition.
2. "destructible_wall" - Any cracked, crumbling, weakened, fragile, or damaged wall section or barricade that can be breached or smashed open.
3. "carpet" - Any floor rug, tapestry on the floor, ornate carpet, or cloth covering that could conceal a secret trapdoor, hatch, or pit beneath it.

COORDINATES:
Provide normalized 2D bounding boxes in "box_2d": [ymin, xmin, ymax, xmax] with coordinates normalized from 0 to 1000 (where 0 is top/left, 1000 is bottom/right).
Enclose the full interactive object (e.g. the door frame, the cracked wall section, or the rug).

RESPONSE FORMAT:
Respond with valid JSON adhering strictly to this schema:
{
  "features": [
    {
      "id": "feat_1",
      "type": "door" | "destructible_wall" | "carpet",
      "label": "Brief descriptive label (e.g. Iron-Banded Dungeon Door)",
      "description": "Visual description of the feature as seen on the map",
      "loreReference": "Connection to the battlemap prompt or wiki lore context",
      "box_2d": [ymin, xmin, ymax, xmax]
    }
  ]
}`;

function getApiKey() {
  const key = process.env.OPENROUTER_API_KEY;
  if (!key || !key.trim()) {
    const err = new Error('OPENROUTER_API_KEY is not configured on the server');
    err.code = 'NO_API_KEY';
    throw err;
  }
  return key.trim();
}

/**
 * Snaps normalized or pixel box coordinates to 64px grid increments
 * with bounds clamping and minimum 64x64 dimension.
 */
function snapBoxTo64(box2d, canvasWidth = 1024, canvasHeight = 1024) {
  if (!Array.isArray(box2d) || box2d.length < 4) {
    return { x: 0, y: 0, width: 64, height: 64 };
  }

  const [rawYmin, rawXmin, rawYmax, rawXmax] = box2d.map(Number);
  const ymin = Number.isFinite(rawYmin) ? rawYmin : 0;
  const xmin = Number.isFinite(rawXmin) ? rawXmin : 0;
  const ymax = Number.isFinite(rawYmax) ? rawYmax : 0;
  const xmax = Number.isFinite(rawXmax) ? rawXmax : 0;

  const maxCoord = Math.max(ymin, xmin, ymax, xmax);
  const scale = maxCoord <= 1.01 ? 1 : (maxCoord <= 1001 ? 1000 : canvasWidth);

  const normYmin = Math.max(0, Math.min(1, ymin / scale));
  const normXmin = Math.max(0, Math.min(1, xmin / scale));
  const normYmax = Math.max(normYmin, Math.min(1, ymax / scale));
  const normXmax = Math.max(normXmin, Math.min(1, xmax / scale));

  const rawX = normXmin * canvasWidth;
  const rawY = normYmin * canvasHeight;
  const rawW = (normXmax - normXmin) * canvasWidth;
  const rawH = (normYmax - normYmin) * canvasHeight;

  let x = Math.round(rawX / 64) * 64;
  let y = Math.round(rawY / 64) * 64;
  let width = Math.max(64, Math.round(rawW / 64) * 64);
  let height = Math.max(64, Math.round(rawH / 64) * 64);

  // Clamp within canvas boundaries
  x = Math.max(0, Math.min(x, canvasWidth - 64));
  y = Math.max(0, Math.min(y, canvasHeight - 64));

  if (x + width > canvasWidth) {
    width = Math.max(64, canvasWidth - x);
  }
  if (y + height > canvasHeight) {
    height = Math.max(64, canvasHeight - y);
  }

  return { x, y, width, height };
}

/**
 * Normalizes and formats detected features from the model payload.
 */
function normalizeFeatures(rawFeatures, canvasWidth = 1024, canvasHeight = 1024) {
  if (!Array.isArray(rawFeatures)) return [];

  const validTypes = new Set(['door', 'destructible_wall', 'carpet']);
  const normalized = [];

  for (let i = 0; i < rawFeatures.length; i++) {
    const item = rawFeatures[i];
    if (!item || typeof item !== 'object') continue;

    let type = typeof item.type === 'string' ? item.type.toLowerCase().trim() : '';
    // Normalize aliases if any
    if (type === 'destructible' || type === 'cracked_wall' || type === 'damaged_wall' || type === 'wall') {
      type = 'destructible_wall';
    } else if (type === 'rug' || type === 'trapdoor' || type === 'hatch') {
      type = 'carpet';
    }

    if (!validTypes.has(type)) {
      continue;
    }

    const def = FEATURE_DEFINITIONS[type];
    const snappedBox = snapBoxTo64(item.box_2d, canvasWidth, canvasHeight);
    const id = typeof item.id === 'string' && item.id.trim()
      ? item.id.trim()
      : `feat_${type}_${i + 1}`;

    const label = typeof item.label === 'string' && item.label.trim()
      ? item.label.trim()
      : (type === 'door' ? 'Doorway' : (type === 'destructible_wall' ? 'Weakened Wall' : 'Floor Rug'));

    const description = typeof item.description === 'string' ? item.description.trim() : '';
    const loreReference = typeof item.loreReference === 'string' ? item.loreReference.trim() : '';

    normalized.push({
      id,
      type,
      label,
      description,
      loreReference,
      box: snappedBox,
      suggestedAltPrompt: def.suggestedAltPrompt,
      currentState: 0,
      states: [
        { stateIndex: 0, label: def.stateLabels[0], patchDataUrl: null },
        { stateIndex: 1, label: def.stateLabels[1], patchDataUrl: null }
      ]
    });
  }

  return normalized;
}

/**
 * Detects interactive features (doors, destructible walls, carpets) on a battlemap.
 */
async function detectFeatures({ mapDataUrl, prompt = '', wikiContext = null, mapDimensions = null }) {
  if (!mapDataUrl || typeof mapDataUrl !== 'string') {
    const err = new Error('mapDataUrl is required for feature detection');
    err.code = 'INVALID_PARAMETERS';
    throw err;
  }

  const apiKey = getApiKey();
  const canvasWidth = mapDimensions?.width || 1024;
  const canvasHeight = mapDimensions?.height || 1024;

  const userPromptText = `Analyze this 2D battlemap and locate all doors, destructible/cracked walls, and floor carpets/rugs.\n` +
    `Generation prompt: "${prompt || 'Tabletop battlemap'}"\n` +
    (wikiContext ? `Wiki Lore Context: ${JSON.stringify(wikiContext)}\n` : '') +
    `Return all detected features in the required JSON format.`;

  const messages = [
    {
      role: 'system',
      content: SYSTEM_INSTRUCTION
    },
    {
      role: 'user',
      content: [
        {
          type: 'text',
          text: userPromptText
        },
        {
          type: 'image_url',
          image_url: {
            url: mapDataUrl
          }
        }
      ]
    }
  ];

  const attempts = [];
  let payload = null;
  let rawResponseText = '';

  try {
    const res = await fetch(OPENROUTER_CHAT_URL, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${apiKey}`,
        'Content-Type': 'application/json',
        'X-Title': APPLICATION_TITLE
      },
      body: JSON.stringify({
        model: FEATURE_DETECTION_MODEL,
        messages,
        response_format: { type: 'json_object' }
      }),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
    });

    payload = await res.json().catch(() => null);

    if (!res.ok) {
      const summary = payload?.error?.message || payload?.error || `HTTP ${res.status}`;
      const err = new Error(`OpenRouter feature detection failed: ${summary}`);
      err.status = res.status;
      err.payload = payload;
      throw err;
    }

    rawResponseText = payload?.choices?.[0]?.message?.content || '';
    if (!rawResponseText) {
      throw new Error('OpenRouter response contained no choices or empty message content');
    }

    attempts.push({ model: FEATURE_DETECTION_MODEL, status: 'success' });
  } catch (err) {
    attempts.push({ model: FEATURE_DETECTION_MODEL, status: 'failed', error: err.message });
    const summary = summarizeUpstreamError(err);
    const failureMsg = describeFailure('openrouter', summary);
    throw providerFailure(failureMsg, {
      code: 'FEATURE_DETECTION_FAILED',
      provider: 'openrouter',
      attempts,
      rawResponseText,
      systemInstruction: SYSTEM_INSTRUCTION,
      userPrompt: userPromptText,
      fullPrompt: `[${FEATURE_DETECTION_MODEL}] ${userPromptText}`,
      images: [mapDataUrl.slice(0, 128) + '...'],
      usage: parseUsage(payload)
    });
  }

  let parsed = null;
  try {
    parsed = JSON.parse(rawResponseText);
  } catch (parseErr) {
    throw providerFailure(`Failed to parse feature detection JSON response: ${parseErr.message}`, {
      code: 'FEATURE_DETECTION_FAILED',
      provider: 'openrouter',
      attempts,
      rawResponseText,
      systemInstruction: SYSTEM_INSTRUCTION,
      userPrompt: userPromptText,
      fullPrompt: `[${FEATURE_DETECTION_MODEL}] ${userPromptText}`,
      usage: parseUsage(payload)
    });
  }

  const features = normalizeFeatures(parsed.features || parsed, canvasWidth, canvasHeight);
  const usage = parseUsage(payload);

  return {
    features,
    model: FEATURE_DETECTION_MODEL,
    usage,
    cost: usage.cost,
    debug: {
      model: FEATURE_DETECTION_MODEL,
      systemInstruction: SYSTEM_INSTRUCTION,
      userPrompt: userPromptText,
      rawResponseText,
      attempts,
      usage
    }
  };
}

/**
 * Forges an alternate state for an interactive feature within its bounding box, then extracts the
 * cropped overlay patch from the composited map. Canvas composition is delegated to
 * `canvasForge.forgeCanvasRegion`; `renderAdapter` may be injected, otherwise the configured model
 * runner adapter is built.
 */
async function forgeFeatureState({ originalDataUrl, box, prompt, model, forgeImageMode = 'highlighted-only', renderAdapter }) {
  if (!originalDataUrl || !box) {
    const err = new Error('originalDataUrl and box are required for forgeFeatureState');
    err.code = 'INVALID_PARAMETERS';
    throw err;
  }

  const selection = {
    x: Math.round(box.x),
    y: Math.round(box.y),
    width: Math.round(box.width),
    height: Math.round(box.height)
  };

  const adapter = renderAdapter || createForgeRegionAdapter({ selection, prompt, model, forgeImageMode });
  const compositeResult = await forgeCanvasRegion({
    originalDataUrl,
    selection,
    renderAdapter: adapter
  });

  const compBuffer = dataUrlToBuffer(compositeResult.imageDataUrl);
  const meta = await sharp(compBuffer).metadata();
  const dx = compositeResult.offset?.dx || 0;
  const dy = compositeResult.offset?.dy || 0;
  const extractLeft = Math.max(0, Math.min(dx + selection.x, meta.width - 1));
  const extractTop = Math.max(0, Math.min(dy + selection.y, meta.height - 1));
  const extractWidth = Math.min(selection.width, meta.width - extractLeft);
  const extractHeight = Math.min(selection.height, meta.height - extractTop);

  const patchBuffer = await sharp(compBuffer)
    .extract({
      left: extractLeft,
      top: extractTop,
      width: Math.max(1, extractWidth),
      height: Math.max(1, extractHeight)
    })
    .png()
    .toBuffer();

  return {
    patchDataUrl: bufferToDataUrl(patchBuffer),
    offset: compositeResult.offset,
    debug: compositeResult.debug,
    usage: compositeResult.usage,
    cost: compositeResult.cost
  };
}

module.exports = {
  FEATURE_DETECTION_MODEL,
  FEATURE_DEFINITIONS,
  SYSTEM_INSTRUCTION,
  snapBoxTo64,
  normalizeFeatures,
  detectFeatures,
  forgeFeatureState
};
