const sharp = require('sharp');
const {
  dataUrlToBuffer,
  bufferToDataUrl,
  prepareOutpaintCanvas,
  featheredOutpaintComposite,
  buildInpaintGuide,
  compositeInpaintRegion
} = require('./imageProcessor');
const {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildInpaintPrompt,
  buildOutpaintPrompt
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
async function generateBattlemap({ prompt, width = 1024, height = 1024 }) {
  const entry = configuredModel();
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
    debug: result.debug
  };
}

/**
 * Inpainting / detail modification: the model receives the map, the tinted guide and the binary mask,
 * and only the masked bounding box of its render is composited back — every pixel outside it keeps
 * its original bytes. The original map is never silently returned as if the edit had happened.
 */
async function inpaintBattlemap({ originalDataUrl, maskDataUrl, prompt }) {
  const entry = configuredModel();

  if (!entry.vision) {
    throw providerFailure(
      `The configured model ${entry.label} (${entry.id}) cannot receive images, so it cannot run a masked detail edit. Point DEFAULT_MODEL_ID in server/services/modelCatalog.js at a vision-capable model.`,
      { code: 'MODEL_NOT_VISION_CAPABLE', provider: entry.provider }
    );
  }

  const systemInstruction = IMAGE_PROMPTS.INPAINT;
  const userPromptText = buildInpaintPrompt(prompt);
  const fullPromptPayload = `${systemInstruction}\n\n[USER INPAINT REQUEST]\n${userPromptText}`;

  const origBuf = dataUrlToBuffer(originalDataUrl);
  const maskBuf = dataUrlToBuffer(maskDataUrl);
  const { width, height } = await sharp(origBuf).metadata();

  const guideBuffer = await buildInpaintGuide(origBuf, maskBuf, width, height);
  const maskPngBuffer = await sharp(maskBuf)
    .resize(width, height, { fit: 'fill' })
    .greyscale()
    .png()
    .toBuffer();

  const result = await runModel(entry, {
    systemInstruction,
    userPromptText: prompt,
    fullPromptPayload,
    images: [
      { label: `Image 1 — current battlemap at ${width}x${height} (authoritative pixels)`, buffer: origBuf },
      { label: 'Image 2 — the same battlemap with the editable region tinted red', buffer: guideBuffer },
      { label: 'Image 3 — binary mask of the editable region, white = edit, black = preserve', buffer: maskPngBuffer }
    ],
    width,
    height
  });

  const imageDataUrl = await compositeInpaintRegion(origBuf, result.imageBuffer, maskBuf, width, height, INPAINT_FEATHER_PX);

  return { imageDataUrl, debug: result.debug };
}

/**
 * Outpainting / canvas extension. The model receives the padded canvas so it can see what has to be
 * filled; the original map is feathered over the seam, and the canvas is never padded with
 * procedurally generated terrain.
 */
async function outpaintBattlemap({ originalDataUrl, direction = 'east', extensionPrompt = '', expandAmount = 512, span = null }) {
  const entry = configuredModel();
  const systemInstruction = IMAGE_PROMPTS.OUTPAINT;

  const prepared = await prepareOutpaintCanvas(originalDataUrl, direction, expandAmount, span);
  const { width: newW, height: newH, region } = prepared.dimensions;
  // The canvas processor normalises the side and clamps the span; use its view from here on.
  const resolvedDirection = prepared.dimensions.direction;

  const userPromptText = extensionPrompt || `Expand canvas towards ${resolvedDirection.toUpperCase()}`;
  const fullPromptPayload = `${systemInstruction}\n\n[OUTPAINT DIRECTION & USER REQUEST]\n${buildOutpaintPrompt({
    direction: resolvedDirection,
    extensionPrompt,
    width: newW,
    height: newH,
    region
  })}`;

  const images = [
    {
      label: `Extended battlemap canvas (${newW}x${newH} px) containing the existing map and empty dark expansion region`,
      buffer: dataUrlToBuffer(prepared.extendedImage)
    },
    {
      label: 'Extended canvas with the selected expansion box highlighted in cyan tint',
      buffer: dataUrlToBuffer(prepared.guideImage)
    },
    {
      label: 'Binary mask of the expansion box (WHITE = area to generate new map terrain inside, BLACK = keep strictly unchanged/blank)',
      buffer: dataUrlToBuffer(prepared.maskImage)
    }
  ];

  const result = await runModel(entry, {
    systemInstruction,
    userPromptText,
    fullPromptPayload,
    images,
    width: newW,
    height: newH
  });

  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const finalStitchedDataUrl = await featheredOutpaintComposite(origBuffer, result.imageBuffer, prepared.dimensions, 48);

  return {
    imageDataUrl: finalStitchedDataUrl,
    dimensions: prepared.dimensions,
    debug: result.debug
  };
}

module.exports = {
  generateBattlemap,
  inpaintBattlemap,
  outpaintBattlemap
};
