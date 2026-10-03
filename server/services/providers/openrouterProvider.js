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

/**
 * POSTs one chat completion and returns the parsed body, turning any HTTP failure into an Error
 * carrying OpenRouter's own message so the failure summary stays factual.
 */
async function postCompletion({ apiKey, model, systemInstruction, fullPromptPayload, images }) {
  const body = {
    model,
    modalities: ['image', 'text'],
    messages: [
      { role: 'system', content: systemInstruction },
      { role: 'user', content: buildUserContent(fullPromptPayload, images) }
    ]
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
function extractImageSource(payload) {
  const message = payload.choices && payload.choices[0] && payload.choices[0].message;
  if (!message) return null;

  const candidates = [];
  if (Array.isArray(message.images)) candidates.push(...message.images);
  if (Array.isArray(message.content)) candidates.push(...message.content);

  for (const candidate of candidates) {
    if (!candidate) continue;
    const url = candidate.image_url && (candidate.image_url.url || candidate.image_url);
    if (typeof url === 'string' && url.length > 0) return url;
    if (typeof candidate.data === 'string' && candidate.data.length > 0) {
      return `data:${candidate.mime_type || candidate.media_type || 'image/png'};base64,${candidate.data}`;
    }
  }
  return null;
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

  const payload = await postCompletion({ apiKey, model, systemInstruction, fullPromptPayload, images });

  const usage = readUsage(payload, model);
  const finishReason = readFinishReason(payload);
  const source = extractImageSource(payload);
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
      images: telemetryImages
    };
    throw err;
  }

  const imageBuffer = await sharp(await sourceToBuffer(source))
    .resize(width, height, { fit: 'fill' })
    .png()
    .toBuffer();

  return {
    imageBuffer,
    debug: {
      modelUsed: model,
      rawResponseText: rawText || `[${finishReason || 'image'} response — image bytes omitted from telemetry]`,
      attempts: [{ model, outcome: 'ok', finishReason, responseChars: rawText.length, usage }],
      usage,
      systemInstruction,
      userPrompt: userPromptText,
      fullPrompt: fullPromptPayload,
      images: telemetryImages
    }
  };
}

module.exports = { requestCanvas };
