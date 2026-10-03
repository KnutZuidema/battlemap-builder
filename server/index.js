const express = require('express');
const cors = require('cors');
const path = require('path');
const dotenv = require('dotenv');
const { prepareInpaintCanvas } = require('./services/imageProcessor');
const { generateBattlemap, inpaintBattlemap, outpaintBattlemap } = require('./services/battlemapService');
const { isProviderConfigured, MODEL_CATALOG, DEFAULT_MODEL_ID } = require('./services/modelCatalog');

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

app.listen(PORT, () => {
  console.log(`⚔️ Dungeon Forge server running on http://localhost:${PORT}`);
});
