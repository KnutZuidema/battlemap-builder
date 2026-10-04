const { getWikiArticle } = require('./wikiService');

const OPENROUTER_CHAT_URL = 'https://openrouter.ai/api/v1/chat/completions';
const PRIMARY_MODEL = 'openai/gpt-6-luna';
const FALLBACK_MODEL = 'google/gemini-2.5-flash';
const APPLICATION_TITLE = 'Dungeon Forge';
const REQUEST_TIMEOUT_MS = Number(process.env.OPENROUTER_TIMEOUT_MS) || 120000;

const SYSTEM_INSTRUCTION = `You are an expert AI prompt engineer specializing in 2D orthographic tabletop battlemaps for D&D 5e encounters in the world of Umbraen.
Your task is to analyze wiki location lore and generate evocative, tactically rich image prompts for a top-down battlemap generator.

CRITICAL BATTLEMAP GENERATION RULES:
- Perspective: Strictly top-down orthographic view (bird's-eye view, 2D flat projection).
- Grid compatibility: 5ft square grid layout, clear scale (e.g. 20x20 squares, 30x30 squares).
- No text or UI: Never mention labels, numbers, borders, UI elements, or words in the image prompt.
- Tactical features: Explicitly incorporate cover (crates, pillars, walls), environmental hazards (deep water, fire, traps), and chokepoints.
- Art style: Hand-painted fantasy realism, rich textures (stone, water, wood), cinematic directional lighting from above.
- Prompt length: 100-180 words, dense with visual and spatial detail.

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
      "imagePrompt": "Detailed 100-180 word top-down orthographic battlemap image prompt..."
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
    "imagePrompt": "Detailed 100-180 word top-down orthographic battlemap image prompt..."
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

module.exports = {
  PRIMARY_MODEL,
  FALLBACK_MODEL,
  suggestBattlemaps,
  generateCustomBattlemapPrompt
};
