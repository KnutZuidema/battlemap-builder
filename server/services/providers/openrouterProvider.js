const sharp = require('sharp');
const { getModelPricing } = require('../modelCatalog');

/**
 * OpenRouter provider — one HTTP surface (`/api/v1/chat/completions`) drives every map request.
 *
 * Image models (e.g. google/gemini-3.1-flash-image) return raster pixels in `message.images` or in
 * image parts of `message.content`; those bytes are resized to the exact canvas size the stitchers
 * downstream expect (fill, so no map content is cropped away).
 */

const OPENROUTER_CHAT_URL = 'https://openrouter.ai/api/v1/chat/completions';
const APPLICATION_TITLE = 'Dungeon Forge';
const REQUEST_TIMEOUT_MS = Number(process.env.OPENROUTER_TIMEOUT_MS) || 300000;

function buildUserContent(fullPromptPayload, images) {
  if (!images.length) return fullPromptPayload;

  const parts = [{ type: 'text', text: fullPromptPayload }];
  for (const image of images) {
    parts.push({ type: 'text', text: `${image.label}:` });
    parts.push({
      type: 'image_url',
      image_url: { url: `data:${image.mimeType || 'image/png'};base64,${image.buffer.toString('base64')}` }
    });
  }
  return parts;
}

const SUPPORTED_ASPECT_RATIOS = [
  { id: '1:8', ratio: 1 / 8 },
  { id: '1:4', ratio: 1 / 4 },
  { id: '9:16', ratio: 9 / 16 },
  { id: '2:3', ratio: 2 / 3 },
  { id: '3:4', ratio: 3 / 4 },
  { id: '4:5', ratio: 4 / 5 },
  { id: '1:1', ratio: 1.0 },
  { id: '5:4', ratio: 5 / 4 },
  { id: '4:3', ratio: 4 / 3 },
  { id: '3:2', ratio: 3 / 2 },
  { id: '16:9', ratio: 16 / 9 },
  { id: '21:9', ratio: 21 / 9 },
  { id: '4:1', ratio: 4 / 1 },
  { id: '8:1', ratio: 8 / 1 }
];

/**
 * Selects the closest supported aspect ratio for a given width and height using logarithmic ratio distance.
 */
function selectAspectRatio(width, height) {
  if (!width || !height || width === height) return '1:1';
  const targetRatio = width / height;
  let best = SUPPORTED_ASPECT_RATIOS[0];
  let minDiff = Math.abs(Math.log(targetRatio / best.ratio));
  for (let i = 1; i < SUPPORTED_ASPECT_RATIOS.length; i++) {
    const diff = Math.abs(Math.log(targetRatio / SUPPORTED_ASPECT_RATIOS[i].ratio));
    if (diff < minDiff) {
      minDiff = diff;
      best = SUPPORTED_ASPECT_RATIOS[i];
    }
  }
  return best.id;
}

/**
 * POSTs one chat completion and returns the parsed body, turning any HTTP failure into an Error
 * carrying OpenRouter's own message so the failure summary stays factual.
 */
async function postCompletion({ apiKey, model, systemInstruction, fullPromptPayload, images, imageConfig }) {
  const body = {
    model,
    modalities: ['image', 'text'],
    messages: [
      { role: 'system', content: systemInstruction },
      { role: 'user', content: buildUserContent(fullPromptPayload, images) }
    ]
  };

  if (imageConfig && Object.keys(imageConfig).length > 0) {
    body.image_config = imageConfig;
  }
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
    throw err;
  }

  if (!payload) throw new Error(`HTTP ${res.status} from OpenRouter with an unreadable body`);
  return payload;
}

function readUsage(payload, model) {
  const usage = payload && payload.usage;
  if (!usage) return null;

  const promptTokens = typeof usage.prompt_tokens === 'number' ? usage.prompt_tokens : null;
  const completionTokens = typeof usage.completion_tokens === 'number' ? usage.completion_tokens : null;
  const totalTokens = typeof usage.total_tokens === 'number'
    ? usage.total_tokens
    : ((promptTokens ?? 0) + (completionTokens ?? 0));

  const reasoningTokens = (usage.completion_tokens_details && usage.completion_tokens_details.reasoning_tokens) ?? null;
  const imageTokens = (usage.completion_tokens_details && usage.completion_tokens_details.image_tokens) ?? null;

  const costDetails = usage.cost_details || {};
  let promptCost = typeof costDetails.upstream_inference_prompt_cost === 'number'
    ? costDetails.upstream_inference_prompt_cost
    : null;
  let completionCost = typeof costDetails.upstream_inference_completions_cost === 'number'
    ? costDetails.upstream_inference_completions_cost
    : null;
  let totalCost = typeof usage.cost === 'number'
    ? usage.cost
    : (typeof costDetails.upstream_inference_cost === 'number' ? costDetails.upstream_inference_cost : null);

  // Fallback to calculation if OpenRouter omitted costs
  const pricing = getModelPricing(model);
  if (pricing) {
    if (promptCost === null && promptTokens !== null) {
      promptCost = (promptTokens / 1_000_000) * pricing.inputPerMillion;
    }
    if (completionCost === null && completionTokens !== null) {
      completionCost = (completionTokens / 1_000_000) * pricing.outputPerMillion;
    }
    if (totalCost === null && (promptCost !== null || completionCost !== null)) {
      totalCost = (promptCost || 0) + (completionCost || 0);
    }
  }

  // If totalCost is known but promptCost or completionCost is missing
  if (totalCost !== null) {
    if (promptCost === null && completionCost !== null) {
      promptCost = Math.max(0, totalCost - completionCost);
    } else if (completionCost === null && promptCost !== null) {
      completionCost = Math.max(0, totalCost - promptCost);
    } else if (promptCost === null && completionCost === null) {
      if (promptTokens && completionTokens) {
        const inRate = pricing?.inputPerMillion || 0.50;
        const outRate = pricing?.outputPerMillion || 3.00;
        const estPrompt = (promptTokens / 1_000_000) * inRate;
        const estCompletion = (completionTokens / 1_000_000) * outRate;
        const estTotal = estPrompt + estCompletion;
        if (estTotal > 0) {
          promptCost = (estPrompt / estTotal) * totalCost;
          completionCost = (estCompletion / estTotal) * totalCost;
        } else {
          promptCost = 0;
          completionCost = totalCost;
        }
      } else {
        promptCost = 0;
        completionCost = totalCost;
      }
    }
  }

  return {
    promptTokens,
    completionTokens,
    totalTokens,
    reasoningTokens,
    imageTokens,
    promptCost: promptCost !== null ? Number(promptCost) : null,
    completionCost: completionCost !== null ? Number(completionCost) : null,
    cost: totalCost !== null ? Number(totalCost) : null
  };
}

function readFinishReason(payload) {
  return (payload.choices && payload.choices[0] && payload.choices[0].finish_reason) || null;
}

/**
 * Image-out models return either `message.images[]` or image parts inside `message.content`.
 */
function extractImageSources(payload) {
  const message = payload.choices && payload.choices[0] && payload.choices[0].message;
  if (!message) return [];

  const candidates = [];
  if (Array.isArray(message.images)) candidates.push(...message.images);
  if (Array.isArray(message.content)) candidates.push(...message.content);

  const sources = [];
  for (const candidate of candidates) {
    if (!candidate) continue;
    if (typeof candidate === 'string' && candidate.length > 0) {
      sources.push(candidate);
      continue;
    }
    const url = candidate.image_url && (candidate.image_url.url || candidate.image_url);
    if (typeof url === 'string' && url.length > 0) {
      sources.push(url);
      continue;
    }
    if (typeof candidate.url === 'string' && candidate.url.length > 0) {
      sources.push(candidate.url);
      continue;
    }
    if (typeof candidate.data === 'string' && candidate.data.length > 0) {
      sources.push(`data:${candidate.mime_type || candidate.media_type || 'image/png'};base64,${candidate.data}`);
      continue;
    }
    if (candidate.inline_data && typeof candidate.inline_data.data === 'string' && candidate.inline_data.data.length > 0) {
      sources.push(`data:${candidate.inline_data.mime_type || 'image/png'};base64,${candidate.inline_data.data}`);
      continue;
    }
    if (candidate.source && typeof candidate.source.data === 'string' && candidate.source.data.length > 0) {
      sources.push(`data:${candidate.source.media_type || 'image/png'};base64,${candidate.source.data}`);
      continue;
    }
  }
  return sources;
}

function extractImageSource(payload) {
  const sources = extractImageSources(payload);
  return sources[0] || null;
}

async function sourceToBuffer(source) {
  if (source.startsWith('data:')) {
    const base64 = source.slice(source.indexOf(',') + 1);
    return Buffer.from(base64, 'base64');
  }

  const res = await fetch(source, { signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS) });
  if (!res.ok) throw new Error(`HTTP ${res.status} while downloading the generated image`);
  return Buffer.from(await res.arrayBuffer());
}

/**
 * Runs the image contract: ask for raster output, take the first returned image, and force it to
 * the exact canvas size the stitchers downstream expect.
 */
async function requestCanvas({ model, systemInstruction, userPromptText, fullPromptPayload, images = [], width, height, apiKey }) {
  const telemetryImages = images.map((img, idx) => ({
    id: idx,
    label: img.label || `Image ${idx + 1}`,
    dataUrl: `data:${img.mimeType || 'image/png'};base64,${img.buffer.toString('base64')}`
  }));

  const aspectRatio = selectAspectRatio(width, height);
  const imageConfig = { aspect_ratio: aspectRatio };

  const payload = await postCompletion({
    apiKey,
    model,
    systemInstruction,
    fullPromptPayload,
    images,
    imageConfig
  });

  const usage = readUsage(payload, model);
  const finishReason = readFinishReason(payload);
  const sources = extractImageSources(payload);
  const source = sources[0] || null;
  const rawText = (payload.choices && payload.choices[0] && typeof payload.choices[0].message.content === 'string')
    ? payload.choices[0].message.content
    : '';

  if (!source) {
    const attempts = [{ model, outcome: 'no_image', finishReason, responseChars: rawText.length, usage }];
    const err = new Error(`model returned no image (finish=${finishReason || 'unknown'}, ${rawText.length} chars of text)`);
    err.attempts = attempts;
    err.rawResponseText = rawText;
    err.usage = usage;
    err.telemetry = {
      modelUsed: model,
      rawResponseText: rawText,
      attempts,
      usage,
      systemInstruction,
      userPrompt: userPromptText,
      fullPrompt: fullPromptPayload,
      images: telemetryImages,
      outputImages: [],
      imageConfig
    };
    throw err;
  }

  const rawImageBuffer = await sourceToBuffer(source);
  const imageBuffer = await sharp(rawImageBuffer)
    .resize(width, height, { fit: 'fill' })
    .png()
    .toBuffer();

  const outputImages = await Promise.all(sources.map(async (src, idx) => {
    const buf = (idx === 0) ? rawImageBuffer : await sourceToBuffer(src);
    const meta = await sharp(buf).metadata().catch(() => null);
    const mime = (meta && meta.format) ? `image/${meta.format}` : 'image/png';
    const dataUrl = src.startsWith('data:') ? src : `data:${mime};base64,${buf.toString('base64')}`;
    const dim = meta?.width && meta?.height ? ` (${meta.width}×${meta.height} px)` : '';
    return {
      id: idx,
      label: `Model Output ${idx + 1}${dim}`,
      dataUrl
    };
  }));

  const fallbackResponseText = outputImages.length > 0
    ? `[${finishReason || 'stop'} response — ${outputImages.length} image output${outputImages.length === 1 ? '' : 's'} received]`
    : `[${finishReason || 'stop'} response]`;

  return {
    imageBuffer,
    debug: {
      modelUsed: model,
      rawResponseText: rawText || fallbackResponseText,
      attempts: [{ model, outcome: 'ok', finishReason, responseChars: rawText.length, usage }],
      usage,
      systemInstruction,
      userPrompt: userPromptText,
      fullPrompt: fullPromptPayload,
      images: telemetryImages,
      outputImages,
      imageConfig
    }
  };
}

module.exports = {
  extractImageSources,
  extractImageSource,
  sourceToBuffer,
  postCompletion,
  requestCanvas,
  selectAspectRatio,
  SUPPORTED_ASPECT_RATIOS
};
