const express = require('express');
const cors = require('cors');
const path = require('path');
const dotenv = require('dotenv');
const { generateBattlemap, forgeRegion, forgeFeatureState } = require('./services/battlemapService');
const { deleteRegion } = require('./services/imageProcessor');
const { detectFeatures } = require('./services/featureService');
const { isProviderConfigured, MODEL_CATALOG, DEFAULT_MODEL_ID } = require('./services/modelCatalog');
const { getWikiTree, getWikiArticle } = require('./services/wikiService');
const { suggestBattlemaps, generateCustomBattlemapPrompt, suggestRegionForging } = require('./services/wikiPrompter');
const {
  listMaps,
  getMap,
  saveMap,
  updateMap,
  deleteMap,
  duplicateMap
} = require('./services/mapStorage');

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
 * Generates tactical region forging suggestions for a selected canvas region
 */
app.post('/api/wiki/regions/suggest', async (req, res) => {
  try {
    const { articlePath, selection, mapDimensions, mapTitle, basePrompt, userIntent } = req.body;
    if (!articlePath || !selection) {
      return res.status(400).json({
        error: 'Missing articlePath or selection in request body',
        code: 'INVALID_PARAMETERS'
      });
    }
    console.log(`[API /wiki/regions/suggest] Suggesting region forging for "${articlePath}"`);
    const result = await suggestRegionForging({
      articlePath,
      selection,
      mapDimensions,
      mapTitle,
      basePrompt,
      userIntent
    });
    res.json(result);
  } catch (err) {
    console.error('[API /api/wiki/regions/suggest error]', err.message);
    const status = err.code === 'NO_API_KEY' ? 503
      : (err.code === 'WIKI_ARTICLE_NOT_FOUND' ? 404
      : (err.code === 'INVALID_PATH' || err.code === 'INVALID_SELECTION' || err.code === 'INVALID_PARAMETERS' ? 400 : 502));
    res.status(status).json({
      error: err.message,
      code: err.code || 'PROMPT_SUGGESTION_FAILED',
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
 * Unified region modification route (edit details, canvas extension, and hybrid region forging)
 */
app.post('/api/forge-region', async (req, res) => {
  try {
    const { originalDataUrl, selection, prompt, model, forgeImageMode } = req.body;

    if (!originalDataUrl || !selection) {
      return res.status(400).json({ error: 'originalDataUrl and selection are required' });
    }

    console.log(`[API /forge-region] Forging region (${selection.width}x${selection.height} at ${selection.x},${selection.y}): prompt="${prompt}" model="${model || 'default'}" mode="${forgeImageMode || 'highlighted-only'}"`);

    const result = await forgeRegion({
      originalDataUrl,
      selection,
      prompt,
      model,
      forgeImageMode
    });
    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

/**
 * Deletes a rectangular region from the map image, setting the pixels inside the selection to transparent.
 */
app.post('/api/delete-region', async (req, res) => {
  try {
    const { originalDataUrl, selection } = req.body;
    if (!originalDataUrl || !selection) {
      return res.status(400).json({
        error: 'originalDataUrl and selection are required',
        code: 'INVALID_PARAMETERS'
      });
    }

    console.log(`[API /delete-region] Deleting region (${selection.width}x${selection.height} at ${selection.x},${selection.y})`);

    const imageDataUrl = await deleteRegion(originalDataUrl, selection);
    res.json({ imageDataUrl, selection });
  } catch (err) {
    console.error('[API /api/delete-region error]', err.message);
    res.status(500).json({
      error: err.message,
      code: err.code || 'DELETE_REGION_FAILED'
    });
  }
});
/**
 * Detects interactive features (doors, destructible walls, carpets) on a battlemap
 */
app.post('/api/features/detect', async (req, res) => {
  try {
    const { mapDataUrl, prompt, wikiContext, mapDimensions } = req.body;
    if (!mapDataUrl) {
      return res.status(400).json({ error: 'mapDataUrl is required', code: 'INVALID_PARAMETERS' });
    }
    console.log(`[API /features/detect] Detecting interactive features for prompt="${prompt || 'Tabletop battlemap'}"`);
    const result = await detectFeatures({ mapDataUrl, prompt, wikiContext, mapDimensions });
    res.json(result);
  } catch (err) {
    console.error('[API /api/features/detect error]', err.message);
    const status = err.code === 'NO_API_KEY' ? 503
      : (err.code === 'INVALID_PARAMETERS' ? 400 : 502);
    res.status(status).json({
      error: err.message,
      code: err.code || 'FEATURE_DETECTION_FAILED',
      debug: err.telemetry || null
    });
  }
});

/**
 * Forges an alternate state for an interactive feature and returns the overlay patch
 */
app.post('/api/features/forge-state', async (req, res) => {
  try {
    const { originalDataUrl, box, prompt, model, forgeImageMode } = req.body;
    if (!originalDataUrl || !box) {
      return res.status(400).json({ error: 'originalDataUrl and box are required', code: 'INVALID_PARAMETERS' });
    }
    console.log(`[API /features/forge-state] Forging feature state (${box.width}x${box.height} at ${box.x},${box.y}): prompt="${prompt}" model="${model || 'default'}" mode="${forgeImageMode || 'highlighted-only'}"`);
    const result = await forgeFeatureState({
      originalDataUrl,
      box,
      prompt,
      model,
      forgeImageMode
    });
    res.json(result);
  } catch (err) {
    sendGenerationError(res, err);
  }
});

// ============================================================================
// Persistent Map Storage Endpoints
// ============================================================================

function handleMapError(res, err) {
  let status = 500;
  if (err.code === 'MAP_NOT_FOUND') {
    status = 404;
  } else if (err.code === 'INVALID_MAP_ID' || err.code === 'INVALID_PARAMETERS' || err.code === 'INVALID_MAP_DATA') {
    status = 400;
  }
  return res.status(status).json({
    ok: false,
    error: err.message,
    code: err.code || 'STORAGE_ERROR'
  });
}

app.get('/api/maps', async (req, res) => {
  try {
    const maps = await listMaps();
    res.json({ ok: true, maps });
  } catch (err) {
    handleMapError(res, err);
  }
});

app.post('/api/maps', async (req, res) => {
  try {
    const map = await saveMap(req.body);
    res.status(201).json({ ok: true, map });
  } catch (err) {
    handleMapError(res, err);
  }
});

app.get('/api/maps/:id', async (req, res) => {
  try {
    const map = await getMap(req.params.id);
    res.json({ ok: true, map });
  } catch (err) {
    handleMapError(res, err);
  }
});

app.put('/api/maps/:id', async (req, res) => {
  try {
    const map = await updateMap(req.params.id, req.body);
    res.json({ ok: true, map });
  } catch (err) {
    handleMapError(res, err);
  }
});

app.delete('/api/maps/:id', async (req, res) => {
  try {
    const result = await deleteMap(req.params.id);
    res.json({ ok: true, ...result });
  } catch (err) {
    handleMapError(res, err);
  }
});

app.post('/api/maps/:id/duplicate', async (req, res) => {
  try {
    const map = await duplicateMap(req.params.id);
    res.status(201).json({ ok: true, map });
  } catch (err) {
    handleMapError(res, err);
  }
});


if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`⚔️ Dungeon Forge server running on http://localhost:${PORT}`);
  });
}

module.exports = app;
