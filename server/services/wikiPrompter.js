const { getWikiArticle } = require('./wikiService');

const OPENROUTER_CHAT_URL = 'https://openrouter.ai/api/v1/chat/completions';
const PRIMARY_MODEL = 'openai/gpt-6-luna';
const FALLBACK_MODEL = 'google/gemini-2.5-flash';
const APPLICATION_TITLE = 'Dungeon Forge';
const REQUEST_TIMEOUT_MS = Number(process.env.OPENROUTER_TIMEOUT_MS) || 120000;

const SYSTEM_INSTRUCTION = `You are an expert tabletop RPG lore cartographer and encounter designer specializing in the world of Umbraen.
Your task is to analyze wiki location lore and describe rich, tangible, and tactically evocative map details for tabletop battlemaps.

CORE OBJECTIVE:
Focus strictly on the physical environment and tangible map details drawn directly from the wiki lore:
- Architectural structures, construction materials, masonry, woodwork, pillars, doors, and furnishings.
- Environmental hazards, terrain obstacles, water, elevation differences, and tactical cover.
- Specific room identities, chambers, passages, and DM secrets described in the location lore.
- Visible lighting sources (braziers, torches, glowing moss, luminescence) and atmospheric floor textures.

DO NOT INCLUDE:
- Camera angles, perspective, projection, or rendering directives (such as "top-down", "2D flat view", "orthographic", or "poché") — the image generation system prompt handles camera and perspective.
- Meta-prompts, art style instructions (e.g. "digital art", "hand-painted"), UI labels, numbers, grid lines, or game statistics.

You must respond in JSON format conforming strictly to the requested schema.`;

function getApiKey() {
  const key = process.env.OPENROUTER_API_KEY;
  if (!key || !key.trim()) {
    const err = new Error('OPENROUTER_API_KEY is not configured on the server');
    err.code = 'NO_API_KEY';
    throw err;
  }
  return key.trim();
}

function parseUsage(payload, fallbackCostPerMIn = 0.15, fallbackCostPerMOut = 0.60) {
  const usage = payload?.usage;
  if (!usage) {
    return { promptTokens: 0, completionTokens: 0, totalTokens: 0, cost: 0 };
  }

  const promptTokens = typeof usage.prompt_tokens === 'number' ? usage.prompt_tokens : 0;
  const completionTokens = typeof usage.completion_tokens === 'number' ? usage.completion_tokens : 0;
  const totalTokens = typeof usage.total_tokens === 'number'
    ? usage.total_tokens
    : (promptTokens + completionTokens);

  let cost = 0;
  if (typeof usage.cost === 'number') {
    cost = usage.cost;
  } else if (typeof usage.cost_details?.upstream_inference_cost === 'number') {
    cost = usage.cost_details.upstream_inference_cost;
  } else {
    cost = (promptTokens / 1_000_000 * fallbackCostPerMIn) + (completionTokens / 1_000_000 * fallbackCostPerMOut);
  }

  return {
    promptTokens,
    completionTokens,
    totalTokens,
    cost
  };
}

async function callOpenRouterCompletion({ model, messages, apiKey }) {
  const body = {
    model,
    messages,
    response_format: { type: 'json_object' }
  };

  const res = await fetch(OPENROUTER_CHAT_URL, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Content-Type': 'application/json',
      'X-Title': APPLICATION_TITLE
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
  });

  const payload = await res.json().catch(() => null);

  if (!res.ok) {
    const detail = payload && payload.error
      ? `${payload.error.code || payload.error.type || res.status}: ${payload.error.message || JSON.stringify(payload.error)}`
      : `HTTP ${res.status} from OpenRouter`;
    const err = new Error(detail);
    err.status = res.status;
    err.payload = payload;
    throw err;
  }

  if (!payload || !payload.choices || !payload.choices[0]?.message?.content) {
    throw new Error('Received unreadable completion response from OpenRouter');
  }

  return payload;
}

async function executeWithFallback({ messages, apiKey }) {
  let modelUsed = PRIMARY_MODEL;
  let payload = null;
  const attempts = [];

  try {
    payload = await callOpenRouterCompletion({ model: PRIMARY_MODEL, messages, apiKey });
    attempts.push({ model: PRIMARY_MODEL, status: 'success' });
  } catch (err) {
    attempts.push({ model: PRIMARY_MODEL, status: 'failed', error: err.message });
    console.warn(`[WikiPrompter] Primary model ${PRIMARY_MODEL} failed: ${err.message}. Retrying with fallback ${FALLBACK_MODEL}...`);
    try {
      modelUsed = FALLBACK_MODEL;
      payload = await callOpenRouterCompletion({ model: FALLBACK_MODEL, messages, apiKey });
      attempts.push({ model: FALLBACK_MODEL, status: 'success' });
    } catch (fallbackErr) {
      attempts.push({ model: FALLBACK_MODEL, status: 'failed', error: fallbackErr.message });
      const finalErr = new Error(`Prompt generation failed: ${fallbackErr.message}`);
      finalErr.code = 'PROMPT_GENERATION_FAILED';
      finalErr.telemetry = {
        attempts,
        lastError: fallbackErr.message
      };
      throw finalErr;
    }
  }

  return { payload, modelUsed, attempts };
}

/**
 * Suggest 3 battlemap concepts based on the wiki article lore
 */
async function suggestBattlemaps({ articlePath }) {
  const apiKey = getApiKey();
  const article = await getWikiArticle(articlePath);

  const regionLoreContext = article.parentRegion?.contentExcerpt
    ? `\nParent Region (${article.parentRegion.name}) Background Lore:\n${article.parentRegion.contentExcerpt}`
    : '';

  const userPayload = `Location Article: ${article.title}
Path: ${article.path}
${regionLoreContext}

Article Content:
${article.content.slice(0, 5000)}

Analyze this location and suggest 3 distinct battlemap encounter concepts (e.g. key chambers, courtyards, entrances, or hazard zones).
Respond with JSON matching:
{
  "suggestions": [
    {
      "title": "Descriptive Location / Room Name",
      "encounterType": "Interior / Dungeon / Coastal / Urban / Wilderness",
      "gridSize": "20x20 squares",
      "tacticalHighlights": [
        "Tactical cover feature",
        "Hazard or terrain obstacle",
        "Chokepoint or flank opportunity"
      ],
      "imagePrompt": "Evocative 60-120 word description of physical room architecture, materials, furnishings, hazards, and lighting based on the wiki lore..."
    }
  ]
}`;

  const messages = [
    { role: 'system', content: SYSTEM_INSTRUCTION },
    { role: 'user', content: userPayload }
  ];

  const { payload, modelUsed } = await executeWithFallback({ messages, apiKey });
  const rawText = payload.choices[0].message.content;

  let parsed = null;
  try {
    parsed = JSON.parse(rawText);
  } catch (parseErr) {
    const err = new Error(`Failed to parse battlemap suggestions JSON: ${parseErr.message}`);
    err.code = 'PROMPT_PARSE_ERROR';
    err.telemetry = { rawText };
    throw err;
  }

  const suggestions = Array.isArray(parsed.suggestions)
    ? parsed.suggestions
    : (parsed.items || []);

  const normalized = suggestions.map((s, idx) => ({
    title: s.title || `Encounter Area ${idx + 1}`,
    encounterType: s.encounterType || 'Interior',
    gridSize: s.gridSize || '20x20 squares',
    tacticalHighlights: Array.isArray(s.tacticalHighlights) ? s.tacticalHighlights : [],
    imagePrompt: s.imagePrompt || ''
  }));

  const usage = parseUsage(payload);

  return {
    suggestions: normalized,
    model: modelUsed,
    usage
  };
}

/**
 * Generate a single custom tailored battlemap prompt based on user's specific room/intent
 */
async function generateCustomBattlemapPrompt({ articlePath, userIntent }) {
  const apiKey = getApiKey();
  const article = await getWikiArticle(articlePath);

  if (!userIntent || !userIntent.trim()) {
    const err = new Error('Missing userIntent for custom battlemap generation');
    err.code = 'INVALID_INTENT';
    throw err;
  }

  const regionLoreContext = article.parentRegion?.contentExcerpt
    ? `\nParent Region (${article.parentRegion.name}) Background Lore:\n${article.parentRegion.contentExcerpt}`
    : '';

  const userPayload = `Location Article: ${article.title}
Path: ${article.path}
${regionLoreContext}

Specific Encounter / Area Requested:
${userIntent.trim()}

Article Content:
${article.content.slice(0, 5000)}

Generate a tailored battlemap prompt focused specifically on the requested encounter or area.
Respond with JSON matching:
{
  "prompt": {
    "title": "Descriptive Location / Room Name",
    "encounterType": "Interior / Dungeon / Coastal / Urban / Wilderness",
    "gridSize": "20x20 squares",
    "tacticalHighlights": [
      "Tactical cover feature",
      "Hazard or terrain obstacle",
      "Chokepoint or flank opportunity"
    ],
    "imagePrompt": "Evocative 60-120 word description of physical room architecture, materials, furnishings, hazards, and lighting based on the wiki lore..."
  }
}`;

  const messages = [
    { role: 'system', content: SYSTEM_INSTRUCTION },
    { role: 'user', content: userPayload }
  ];

  const { payload, modelUsed } = await executeWithFallback({ messages, apiKey });
  const rawText = payload.choices[0].message.content;

  let parsed = null;
  try {
    parsed = JSON.parse(rawText);
  } catch (parseErr) {
    const err = new Error(`Failed to parse custom battlemap prompt JSON: ${parseErr.message}`);
    err.code = 'PROMPT_PARSE_ERROR';
    err.telemetry = { rawText };
    throw err;
  }

  const promptObj = parsed.prompt || parsed;
  const prompt = {
    title: promptObj.title || article.title,
    encounterType: promptObj.encounterType || 'Interior',
    gridSize: promptObj.gridSize || '20x20 squares',
    tacticalHighlights: Array.isArray(promptObj.tacticalHighlights) ? promptObj.tacticalHighlights : [],
    imagePrompt: promptObj.imagePrompt || ''
  };

  const usage = parseUsage(payload);

  return {
    prompt,
    model: modelUsed,
    usage
  };
}
/**
 * Suggest lore-grounded forge concepts for a selected region on the canvas
 */
async function suggestRegionForging({ articlePath, selection, mapDimensions, mapTitle, basePrompt, userIntent }) {
  if (!articlePath || typeof articlePath !== 'string' || !articlePath.trim()) {
    const err = new Error('Missing articlePath for region forge suggestions');
    err.code = 'INVALID_PATH';
    throw err;
  }
  if (!selection || typeof selection.x !== 'number' || typeof selection.y !== 'number' ||
      typeof selection.width !== 'number' || typeof selection.height !== 'number') {
    const err = new Error('Invalid selection coordinates for region forge suggestions');
    err.code = 'INVALID_SELECTION';
    throw err;
  }

  const apiKey = getApiKey();
  const article = await getWikiArticle(articlePath);

  const mapW = (mapDimensions && typeof mapDimensions.width === 'number' && mapDimensions.width > 0) ? mapDimensions.width : 1024;
  const mapH = (mapDimensions && typeof mapDimensions.height === 'number' && mapDimensions.height > 0) ? mapDimensions.height : 1024;

  let relationship = 'Interior area (Edit)';
  if (selection.x >= mapW) {
    relationship = 'East expansion';
  } else if (selection.x + selection.width <= 0) {
    relationship = 'West expansion';
  } else if (selection.y >= mapH) {
    relationship = 'South expansion';
  } else if (selection.y + selection.height <= 0) {
    relationship = 'North expansion';
  } else if (selection.x < 0 || selection.y < 0 || selection.x + selection.width > mapW || selection.y + selection.height > mapH) {
    relationship = 'Boundary extension (Hybrid)';
  }

  const gridSquaresX = Math.max(1, Math.round(selection.width / 64));
  const gridSquaresY = Math.max(1, Math.round(selection.height / 64));
  const dimensionFeetX = gridSquaresX * 5;
  const dimensionFeetY = gridSquaresY * 5;

  const regionLoreContext = article.parentRegion?.contentExcerpt
    ? `\nParent Region (${article.parentRegion.name}) Background Lore:\n${article.parentRegion.contentExcerpt}`
    : '';

  const userPayload = `Location Article: ${article.title}
Path: ${article.path}
${regionLoreContext}

Current Map Context:
- Map Title: ${mapTitle || article.title}
- Canvas Dimensions: ${mapW}x${mapH} px
- Base Map Prompt: ${basePrompt || 'Top-down fantasy tabletop battlemap'}
- Selected Region: x=${selection.x}, y=${selection.y}, w=${selection.width}, h=${selection.height}
- Spatial Relationship: ${relationship}
- Region Scale: ${gridSquaresX}x${gridSquaresY} grid squares (${dimensionFeetX}ft x ${dimensionFeetY}ft in 5ft/sq D&D scale)
${userIntent ? `- User Intent / Steering: ${userIntent}` : ''}

Article Lore & Layout:
${article.content.slice(0, 5000)}

TASK:
Analyze the selected canvas region in relation to the location lore and current battlemap.
Provide 3 distinct suggestions grounded in this article's layout, navigation graph, adjacent rooms, environmental hazards, and tactical features.

DIRECTIVES:
1. Focus strictly on physical map details, architecture, materials, and terrain drawn from the wiki lore. Do NOT specify perspective, camera angles, or poché walls (the renderer handles camera and perspective).
2. If this is an EXPANSION (North, South, East, West, or Boundary):
   - Identify adjacent chambers, connecting corridors, exterior courtyards, subterranean passages, or natural terrain described in the wiki article that logically attach at this boundary.
   - Describe doorway connections, floor transitions, and architectural continuity based on the location lore.
3. If this is an INTERIOR EDIT:
   - Identify specific furnishings, tactical cover, altars, traps, elevated daises, or hazards from the article lore that belong in this room area.
4. If User Intent is provided, incorporate and prioritize it.
5. Provide dense, vivid 40-70 word descriptions for "forgePrompt" detailing physical architecture, materials, furnishings, lighting, and environmental terrain.
Respond strictly in JSON matching this schema:
{
  "suggestions": [
    {
      "title": "Room / Feature Name",
      "category": "${relationship}",
      "description": "Short 1-2 sentence description of lore & purpose from the wiki",
      "tacticalHighlights": [
        "Tactical cover or feature",
        "Environmental hazard or obstacle",
        "Tactical note or DM secret"
      ],
      "forgePrompt": "Dense 40-70 word description of architectural elements, furnishings, materials, lighting, and terrain features based on the wiki lore."
    }
  ]
}`;

  const messages = [
    { role: 'system', content: SYSTEM_INSTRUCTION },
    { role: 'user', content: userPayload }
  ];

  const { payload, modelUsed } = await executeWithFallback({ messages, apiKey });
  const rawText = payload.choices[0].message.content;

  let parsed = null;
  try {
    parsed = JSON.parse(rawText);
  } catch (parseErr) {
    const err = new Error(`Failed to parse region forge suggestions JSON: ${parseErr.message}`);
    err.code = 'PROMPT_PARSE_ERROR';
    err.telemetry = { rawText };
    throw err;
  }

  const suggestions = Array.isArray(parsed.suggestions)
    ? parsed.suggestions
    : (parsed.items || []);

  const normalized = suggestions.slice(0, 3).map((s, idx) => ({
    title: (typeof s.title === 'string' && s.title.trim()) || `Feature ${idx + 1}`,
    category: (typeof s.category === 'string' && s.category.trim()) || relationship,
    description: (typeof s.description === 'string' && s.description.trim()) || '',
    tacticalHighlights: Array.isArray(s.tacticalHighlights)
      ? s.tacticalHighlights.map(h => String(h).trim()).filter(Boolean)
      : [],
    forgePrompt: (typeof s.forgePrompt === 'string' && s.forgePrompt.trim()) || ''
  }));

  const usage = parseUsage(payload);

  return {
    suggestions: normalized,
    model: modelUsed,
    usage
  };
}


module.exports = {
  PRIMARY_MODEL,
  FALLBACK_MODEL,
  suggestBattlemaps,
  generateCustomBattlemapPrompt,
  suggestRegionForging,
  parseUsage
};
