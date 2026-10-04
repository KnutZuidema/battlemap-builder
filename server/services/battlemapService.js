const sharp = require('sharp');
const {
  dataUrlToBuffer,
  bufferToDataUrl,
  measureMaskBounds,
  prepareRegionCanvas,
  compositeRegion
} = require('./imageProcessor');
const {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildInpaintPrompt,
  buildOutpaintPrompt,
  buildForgeRegionPrompt
} = require('./prompts');
const {
  providerKeyName,
  isProviderConfigured,
  configuredModel
} = require('./modelCatalog');
const { providerFailure, describeFailure, noApiKeyMessage } = require('./failure');
const openrouterProvider = require('./providers/openrouterProvider');

/**
 * Orchestrates map generation, inpainting and outpainting across providers.
 *
 * The service owns everything provider-agnostic: which model the backend is configured to run,
 * whether it can see images at all, the canvas compositing that keeps edits inside their mask, and
 * the error shape every failure is reported with. Providers only turn a payload into an image.
 *
 * There is no procedural fallback: when the configured model produces nothing, the request fails and
 * the thrown error carries the telemetry the UI needs to explain exactly what happened.
 */

const PROVIDERS = {
  openrouter: openrouterProvider
};

/**
 * Width of the soft alpha ramp inside the mask seam when the model render is composited back.
 */
const INPAINT_FEATHER_PX = 6;

/**
 * One dispatch point for every mode: checks the provider key, calls the provider, and restates any
 * raw upstream failure as an actionable sentence plus the attempt trail.
 */
async function runModel(entry, { systemInstruction, userPromptText, fullPromptPayload, images = [], width, height }) {
  const provider = PROVIDERS[entry.provider];

  if (!provider) {
    throw providerFailure(`Provider "${entry.provider}" is not wired into this server build.`, {
      code: 'UNKNOWN_PROVIDER',
      provider: entry.provider
    });
  }

  if (!isProviderConfigured(entry.provider)) {
    console.error(`[BattlemapService] ${noApiKeyMessage(entry.provider)}`);
    throw providerFailure(noApiKeyMessage(entry.provider), {
      code: 'NO_API_KEY',
      provider: entry.provider,
      systemInstruction,
      userPrompt: userPromptText,
      fullPrompt: fullPromptPayload
    });
  }

  try {
    console.log(`[BattlemapService] ${entry.id} (${entry.provider}) → ${width}x${height}, ${images.length} image input(s)`);
    const result = await provider.requestCanvas({
      model: entry.id,
      systemInstruction,
      userPromptText,
      fullPromptPayload,
      images,
      width,
      height,
      apiKey: process.env[providerKeyName(entry.provider)]
    });

    return {
      imageBuffer: result.imageBuffer,
      debug: { ...result.debug, provider: entry.provider, model: entry.id }
    };
  } catch (err) {
    if (err.code === 'NO_API_KEY') throw err;

    const failure = describeFailure(entry.provider, err.message);
    console.error(`[BattlemapService] ${entry.id} failed: ${failure}`);

    throw providerFailure(failure, {
      code: err.code || 'MODEL_FAILED',
      provider: entry.provider,
      attempts: err.attempts || [],
      rawResponseText: err.rawResponseText || '',
      usage: err.usage || null,
      systemInstruction,
      userPrompt: userPromptText,
      fullPrompt: fullPromptPayload
    });
  }
}

/**
 * Initial map generation from a text prompt with the backend-configured model. Throws when the model
 * produces no map — the caller must surface the failure rather than hand the user something the
 * model never made.
 */
async function generateBattlemap({ prompt, width = 1024, height = 1024, model }) {
  const entry = configuredModel(model);
  const systemInstruction = IMAGE_PROMPTS.GENERATE;
  const userPromptText = buildBattlemapPrompt(prompt);
  const fullPromptPayload = `${systemInstruction}\n\n[USER REQUEST]\n${userPromptText}`;

  const result = await runModel(entry, {
    systemInstruction,
    userPromptText: prompt,
    fullPromptPayload,
    images: [],
    width,
    height
  });

  return {
    imageDataUrl: bufferToDataUrl(result.imageBuffer),
    prompt,
    debug: result.debug,
    usage: result.debug?.usage || null,
    cost: result.debug?.usage?.cost ?? null
  };
}

/**
 * Unified region modification and canvas extension:
 * Extracts a focused context window around the selection, runs the vision model
 * with context/guide/mask images, and composites the render back with zero bleed outside
 * the selection and a soft 20px inward seam feather against preserved map tiles.
 */
async function forgeRegion({ originalDataUrl, selection, prompt, model }) {
  const entry = configuredModel(model);

  if (!entry.vision) {
    throw providerFailure(
      `The configured model ${entry.label} (${entry.id}) cannot receive images, so it cannot run a region edit. Point DEFAULT_MODEL_ID in server/services/modelCatalog.js at a vision-capable model.`,
      { code: 'MODEL_NOT_VISION_CAPABLE', provider: entry.provider }
    );
  }

  const systemInstruction = IMAGE_PROMPTS.FORGE_REGION;
  const prepared = await prepareRegionCanvas(originalDataUrl, selection, 512);
  const { dimensions, contextBuffer, guideBuffer, maskBuffer } = prepared;
  const { contextW, contextH } = dimensions;

  const userPromptText = prompt || 'Generate seamless, style-matched battlemap terrain in the target region';
  const fullPromptPayload = `${systemInstruction}\n\n[USER REGION FORGE REQUEST]\n${buildForgeRegionPrompt(
    prompt,
    selection,
    contextW,
    contextH
  )}`;

  const images = [
    {
      label: `Image 1 — Focused battlemap context window (${contextW}x${contextH} px)`,
      buffer: contextBuffer
    },
    {
      label: 'Image 2 — Context window with target region highlighted in cyan tint',
      buffer: guideBuffer
    },
    {
      label: 'Image 3 — Binary mask (WHITE = target region to forge, BLACK = preserve strictly unchanged)',
      buffer: maskBuffer
    }
  ];

  const result = await runModel(entry, {
    systemInstruction,
    userPromptText,
    fullPromptPayload,
    images,
    width: contextW,
    height: contextH
  });

  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const finalStitchedDataUrl = await compositeRegion(origBuffer, result.imageBuffer, dimensions, 20);

  return {
    imageDataUrl: finalStitchedDataUrl,
    dimensions,
    debug: result.debug,
    usage: result.debug?.usage || null,
    cost: result.debug?.usage?.cost ?? null
  };
}

/**
 * Inpainting / detail modification (delegates to forgeRegion).
 */
async function inpaintBattlemap({ originalDataUrl, maskDataUrl, prompt, model }) {
  const origBuf = dataUrlToBuffer(originalDataUrl);
  const maskBuf = dataUrlToBuffer(maskDataUrl);
  const { width, height } = await sharp(origBuf).metadata();
  const bounds = await measureMaskBounds(maskBuf, width, height);
  const selection = bounds || { x: 0, y: 0, width, height };
  return forgeRegion({ originalDataUrl, selection, prompt, model });
}

/**
 * Outpainting / canvas extension (delegates to forgeRegion).
 */
async function outpaintBattlemap({ originalDataUrl, direction = 'east', extensionPrompt = '', expandAmount = 512, span = null, model }) {
  const origBuf = dataUrlToBuffer(originalDataUrl);
  const { width: origW, height: origH } = await sharp(origBuf).metadata();
  const dir = String(direction || 'east').toLowerCase();
  const depth = Number(expandAmount) || 512;

  let selection;
  if (dir === 'north') {
    const sStart = span ? span.start : 0;
    const sLen = span ? (span.end - span.start) : origW;
    selection = { x: sStart, y: -depth, width: sLen, height: depth };
  } else if (dir === 'south') {
    const sStart = span ? span.start : 0;
    const sLen = span ? (span.end - span.start) : origW;
    selection = { x: sStart, y: origH, width: sLen, height: depth };
  } else if (dir === 'west') {
    const sStart = span ? span.start : 0;
    const sLen = span ? (span.end - span.start) : origH;
    selection = { x: -depth, y: sStart, width: depth, height: sLen };
  } else {
    const sStart = span ? span.start : 0;
    const sLen = span ? (span.end - span.start) : origH;
    selection = { x: origW, y: sStart, width: depth, height: sLen };
  }

  return forgeRegion({ originalDataUrl, selection, prompt: extensionPrompt, model });
}

module.exports = {
  generateBattlemap,
  inpaintBattlemap,
  outpaintBattlemap,
  forgeRegion
};
