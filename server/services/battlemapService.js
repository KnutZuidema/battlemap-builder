const sharp = require('sharp');
const {
  dataUrlToBuffer,
  bufferToDataUrl,
  prepareRegionCanvas,
  compositeRegion,
  deleteRegion: processDeleteRegion
} = require('./imageProcessor');
const {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildForgeRegionPrompt,
  getForgeRegionSystemPrompt
} = require('./prompts');
const {
  providerKeyName,
  isProviderConfigured,
  configuredModel
} = require('./modelCatalog');
const { providerFailure, describeFailure, noApiKeyMessage } = require('./failure');
const openrouterProvider = require('./providers/openrouterProvider');
const { getReferenceImagesForPrompt } = require('./referenceImages');

/**
 * Orchestrates map generation and region forging across providers.
 *
 * The service owns everything provider-agnostic: which model the backend is configured to run,
 * whether it can see images at all, the canvas compositing that keeps edits inside their target region, and
 * the error shape every failure is reported with. Providers only turn a payload into an image.
 *
 * There is no procedural fallback: when the configured model produces nothing, the request fails and
 * the thrown error carries the telemetry the UI needs to explain exactly what happened.
 */

const PROVIDERS = {
  openrouter: openrouterProvider
};

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
      apiKey: process.env[providerKeyName(entry.provider)],
      apiType: entry.apiType
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
      fullPrompt: fullPromptPayload,
      images: err.telemetry?.images || [],
      outputImages: err.telemetry?.outputImages || []
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

  // If the model is vision-capable, attach reference image(s) for visual perspective conditioning
  const referenceImages = entry.vision
    ? await getReferenceImagesForPrompt(prompt, { count: 1 })
    : [];

  const result = await runModel(entry, {
    systemInstruction,
    userPromptText: prompt,
    fullPromptPayload,
    images: referenceImages,
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
async function forgeRegion({ originalDataUrl, selection, prompt, model, forgeImageMode = 'highlighted-only' }) {
  const entry = configuredModel(model);

  if (!entry.vision) {
    throw providerFailure(
      `The configured model ${entry.label} (${entry.id}) cannot receive images, so it cannot run a region edit. Point DEFAULT_MODEL_ID in server/services/modelCatalog.js at a vision-capable model.`,
      { code: 'MODEL_NOT_VISION_CAPABLE', provider: entry.provider }
    );
  }

  const isAllThree = forgeImageMode === 'all-three' || forgeImageMode === 'all';
  const systemInstruction = getForgeRegionSystemPrompt(forgeImageMode);
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

  const images = isAllThree
    ? [
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
      ]
    : [
        {
          label: `Context window with target region highlighted in cyan tint (${contextW}x${contextH} px)`,
          buffer: guideBuffer
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
 * Forges an alternate state for an interactive feature within its bounding box,
 * then extracts and returns the cropped overlay patch.
 */
async function forgeFeatureState({ originalDataUrl, box, prompt, model, forgeImageMode }) {
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

  const compositeResult = await forgeRegion({
    originalDataUrl,
    selection,
    prompt,
    model,
    forgeImageMode
  });
  const compBuffer = dataUrlToBuffer(compositeResult.imageDataUrl);
  const meta = await sharp(compBuffer).metadata();
  const extractLeft = Math.max(0, Math.min((compositeResult.dimensions?.origLeft || 0) + selection.x, meta.width - 1));
  const extractTop = Math.max(0, Math.min((compositeResult.dimensions?.origTop || 0) + selection.y, meta.height - 1));
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

  const patchDataUrl = bufferToDataUrl(patchBuffer);

  return {
    patchDataUrl,
    dimensions: compositeResult.dimensions,
    debug: compositeResult.debug,
    usage: compositeResult.usage,
    cost: compositeResult.cost
  };
}

/**
 * Deletes a rectangular region from the battlemap, clearing it to transparent.
 */
async function deleteRegion({ originalDataUrl, selection }) {
  if (!originalDataUrl || !selection) {
    const err = new Error('originalDataUrl and selection are required');
    err.code = 'INVALID_PARAMETERS';
    throw err;
  }
  const imageDataUrl = await processDeleteRegion(originalDataUrl, selection);
  return {
    imageDataUrl,
    selection
  };
}

module.exports = {
  generateBattlemap,
  forgeRegion,
  forgeFeatureState,
  deleteRegion
};
