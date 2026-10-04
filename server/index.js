const express = require('express');
const cors = require('cors');
const path = require('path');
const dotenv = require('dotenv');
const { prepareInpaintCanvas } = require('./services/imageProcessor');
const { generateBattlemap, inpaintBattlemap, outpaintBattlemap, forgeRegion } = require('./services/battlemapService');
const { isProviderConfigured, MODEL_CATALOG, DEFAULT_MODEL_ID } = require('./services/modelCatalog');
const { getWikiTree, getWikiArticle } = require('./services/wikiService');
const { suggestBattlemaps, generateCustomBattlemapPrompt } = require('./services/wikiPrompter');

// The server config lives next to this file; `process.cwd()` is the repo root under `npm run dev`.
dotenv.config({ path: path.join(__dirname, '.env') });

const app = express();
const PORT = process.env.PORT || 3001;

// CORS setup
app.use(cors());

// High payload size limit for base64 canvas images
app.use(express.json({ limit: '50mb' }));
app.use(express.urlencoded({ limit: '50mb', extended: true }));

// Healthcheck endpoint — reports which provider keys the server can actually use
app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    timestamp: new Date().toISOString(),
    providers: {
      openrouter: isProviderConfigured('openrouter')
    }
  });
});

// Models endpoint — returns selectable battlemap models and capabilities
app.get('/api/models', (req, res) => {
  res.json({
    models: MODEL_CATALOG.map((m) => ({
      id: m.id,
      label: m.label,
      shortLabel: m.shortLabel,
      provider: m.provider,
      vision: m.vision,
      costHint: m.costHint,
      note: m.note
    })),
    defaultModelId: DEFAULT_MODEL_ID
  });
});

// ============================================================================
// Wiki Endpoints: Lore navigation & Battlemap prompt translation
// ============================================================================

/**
 * Returns curated hierarchy of wiki regions (places & settlements) and adventures
 */
app.get('/api/wiki/tree', async (req, res) => {
  try {
    const tree = await getWikiTree();
    res.json(tree);
  } catch (err) {
    console.error('[API /api/wiki/tree error]', err.message);
    const status = err.code === 'WIKI_NOT_FOUND' ? 404 : 500;
    res.status(status).json({
      error: err.message,
      code: err.code || 'WIKI_TREE_FAILED'
    });
  }
});

/**
 * Returns a single wiki article markdown along with parent region excerpt
 */
app.get('/api/wiki/article', async (req, res) => {
  try {
    const articlePath = req.query.path;
    if (!articlePath) {
      return res.status(400).json({ error: 'Missing path parameter', code: 'INVALID_PATH' });
    }
    const article = await getWikiArticle(articlePath);
    res.json(article);
  } catch (err) {
    console.error('[API /api/wiki/article error]', err.message);
    const status = err.code === 'ACCESS_DENIED' ? 403
      : (err.code === 'WIKI_ARTICLE_NOT_FOUND' ? 404 : 500);
    res.status(status).json({
      error: err.message,
      code: err.code || 'WIKI_ARTICLE_FAILED'
    });
  }
});

/**
 * Generates 3 tactical battlemap prompt suggestions for an article
 */
app.post('/api/wiki/prompts/suggest', async (req, res) => {
  try {
    const { path: articlePath } = req.body;
    if (!articlePath) {
      return res.status(400).json({ error: 'Missing path in request body', code: 'INVALID_PATH' });
    }
    console.log(`[API /wiki/prompts/suggest] Suggesting battlemaps for "${articlePath}"`);
    const result = await suggestBattlemaps({ articlePath });
    res.json(result);
  } catch (err) {
    console.error('[API /api/wiki/prompts/suggest error]', err.message);
    const status = err.code === 'NO_API_KEY' ? 503
      : (err.code === 'WIKI_ARTICLE_NOT_FOUND' || err.code === 'INVALID_PATH' ? 404 : 502);
    res.status(status).json({
      error: err.message,
      code: err.code || 'PROMPT_SUGGESTION_FAILED',
      debug: err.telemetry || null
    });
  }
});

/**
 * Generates a single custom battlemap prompt for an article based on user's room/encounter intent
 */
app.post('/api/wiki/prompts/custom', async (req, res) => {
  try {
    const { path: articlePath, userIntent } = req.body;
    if (!articlePath || !userIntent) {
      return res.status(400).json({
        error: 'Missing path or userIntent in request body',
        code: 'INVALID_PARAMETERS'
      });
    }
    console.log(`[API /wiki/prompts/custom] Generating custom battlemap for "${articlePath}": "${userIntent}"`);
    const result = await generateCustomBattlemapPrompt({ articlePath, userIntent });
    res.json(result);
  } catch (err) {
    console.error('[API /api/wiki/prompts/custom error]', err.message);
    const status = err.code === 'NO_API_KEY' ? 503
      : (err.code === 'WIKI_ARTICLE_NOT_FOUND' || err.code === 'INVALID_PATH' ? 404 : 502);
    res.status(status).json({
      error: err.message,
      code: err.code || 'PROMPT_CUSTOM_FAILED',
      debug: err.telemetry || null
    });
  }
});

/**
 * No map producer has a fallback: failures carry `telemetry` (prompts, model attempt trail, raw
 * response) which is forwarded so the client can show the real cause instead of a generic error.
 * A missing provider key is a server misconfiguration (503), an unusable model request is a bad
 * request (400), and an upstream failure is a bad gateway (502).
 */
function sendGenerationError(res, err) {
  const code = err.code || 'REQUEST_FAILED';
  console.error(`[API Error ${code}]`, err.message);

  const status = code === 'NO_API_KEY'
    ? 503
    : (code === 'UNKNOWN_PROVIDER' || code === 'MODEL_NOT_VISION_CAPABLE' ? 400 : 502);

  res.status(status).json({
    error: err.message,
    code,
    debug: err.telemetry || null
  });
}

/**
 * Initial map generation from a text prompt
 */
app.post('/api/generate', async (req, res) => {
  try {
    const { prompt, width, height, model } = req.body;
    console.log(`[API /generate] Generating map: prompt="${prompt}" model="${model || 'default'}"`);

    const result = await generateBattlemap({
      prompt,
      width: parseInt(width) || 1024,
      height: parseInt(height) || 1024,
      model
    });

    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

/**
 * Inpainting route to update a specific area of the battlemap
 */
app.post('/api/inpaint', async (req, res) => {
  try {
    const { originalDataUrl, maskDataUrl, prompt, model } = req.body;

    if (!originalDataUrl || !maskDataUrl) {
      return res.status(400).json({ error: 'originalDataUrl and maskDataUrl are required' });
    }

    console.log(`[API /inpaint] Updating map detail: prompt="${prompt}" model="${model || 'default'}"`);

    const processed = await prepareInpaintCanvas(originalDataUrl, maskDataUrl);
    const result = await inpaintBattlemap({
      originalDataUrl: processed.originalDataUrl,
      maskDataUrl: processed.maskDataUrl,
      prompt,
      model
    });

    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

/**
 * Outpainting route to extend battlemap canvas outward
 */
app.post('/api/outpaint', async (req, res) => {
  try {
    const { originalDataUrl, direction, extensionPrompt, expandAmount, spanStart, spanEnd, model } = req.body;

    if (!originalDataUrl || !direction) {
      return res.status(400).json({ error: 'originalDataUrl and direction are required' });
    }

    // spanStart/spanEnd (px along the expanded edge) limit the new strip to part of that side
    const hasSpan = Number.isFinite(Number(spanStart)) && Number.isFinite(Number(spanEnd)) && spanStart !== null && spanEnd !== null;

    console.log(`[API /outpaint] Extending canvas ${direction.toUpperCase()}: prompt="${extensionPrompt}" model="${model || 'default'}"${hasSpan ? ` span=${spanStart}..${spanEnd}` : ''}`);

    const result = await outpaintBattlemap({
      originalDataUrl,
      direction,
      extensionPrompt,
      expandAmount: parseInt(expandAmount) || 512,
      span: hasSpan ? { start: Number(spanStart), end: Number(spanEnd) } : null,
      model
    });

    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

/**
 * Unified region modification route (inpainting, outpainting, and hybrid edits)
 */
app.post('/api/forge-region', async (req, res) => {
  try {
    const { originalDataUrl, selection, prompt, model } = req.body;

    if (!originalDataUrl || !selection) {
      return res.status(400).json({ error: 'originalDataUrl and selection are required' });
    }

    console.log(`[API /forge-region] Forging region (${selection.width}x${selection.height} at ${selection.x},${selection.y}): prompt="${prompt}" model="${model || 'default'}"`);

    const result = await forgeRegion({
      originalDataUrl,
      selection,
      prompt,
      model
    });

    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

app.listen(PORT, () => {
  console.log(`⚔️ Dungeon Forge server running on http://localhost:${PORT}`);
});
