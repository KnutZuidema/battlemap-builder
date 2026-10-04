/**
 * Backend-side configuration of the map-generation models. The client never names a model: every
 * request runs `DEFAULT_MODEL_ID`, so switching engines is a change here and nowhere else.
 *
 * Every entry produces a raster battlemap image directly (`image` output): the model renders the
 * pixels itself and the returned bytes are resized to the requested canvas size. There is no SVG
 * contract any more, so no model returns markup that has to be rasterised locally.
 *
 * `vision` decides whether the model can receive images; only vision models can run the INPAINT
 * flow, which hands the model the map, a tinted guide and the binary mask. Every current entry is
 * vision-capable; the flag stays part of the contract so the guard keeps holding if one is not.
 *
 * `costHint` values are per-million-token OpenRouter list prices observed 2026-09-24, plus the
 * measured per-image cost of the probe call made for each entry.
 */

const PROVIDER_KEYS = {
  openrouter: 'OPENROUTER_API_KEY'
};

/**
 * The model every request runs. The model actually used is always named in the response telemetry.
 */
const DEFAULT_MODEL_ID = 'google/gemini-3.1-flash-image';

const MODEL_CATALOG = [
  {
    id: 'google/gemini-3.1-flash-image',
    label: 'Nano Banana 2 Flash',
    shortLabel: 'Flash',
    aliases: ['nanobanana 2 flash', 'nanobanana-2-flash', 'gemini-3.1-flash-image', 'flash'],
    provider: 'openrouter',
    vision: true,
    pricing: {
      inputPerMillion: 0.50,
      outputPerMillion: 3.00,
      imageOutput: 0.00006
    },
    costHint: '$0.50/M in · $3/M out (~$0.07 per map, edit or expansion)',
    note: 'Google: Nano Banana 2 (Gemini 3.1 Flash Image). Fast, high-quality painted battlemap generation; supports mask edits.'
  },
  {
    id: 'google/gemini-3.1-flash-lite-image',
    label: 'Nano Banana 2 Flash Lite',
    shortLabel: 'Flash Lite',
    aliases: ['nanobanana 2 flash lite', 'nanobanana-2-flash-lite', 'gemini-3.1-flash-lite-image', 'flash-lite', 'lite'],
    provider: 'openrouter',
    vision: true,
    pricing: {
      inputPerMillion: 0.25,
      outputPerMillion: 1.50,
      imageOutput: 0.00003
    },
    costHint: '$0.25/M in · $1.50/M out (~$0.035 per map, edit or expansion)',
    note: 'Google: Nano Banana 2 Lite (Gemini 3.1 Flash Lite Image). Ultra-fast, lightweight image generation; supports mask edits.'
  }
];

function providerKeyName(provider) {
  return PROVIDER_KEYS[provider] || null;
}

/**
 * A provider counts as configured when its key is present in the server environment; the value is
 * never forwarded to the client.
 */
function isProviderConfigured(provider) {
  const keyName = providerKeyName(provider);
  const value = keyName ? process.env[keyName] : null;
  return Boolean(value && value.trim().length > 5);
}

function findModel(requestedId) {
  if (!requestedId) return null;
  const norm = String(requestedId).trim().toLowerCase();
  return (
    MODEL_CATALOG.find((entry) => {
      if (entry.id.toLowerCase() === norm) return true;
      if (entry.label.toLowerCase() === norm) return true;
      if (entry.shortLabel && entry.shortLabel.toLowerCase() === norm) return true;
      if (entry.aliases && entry.aliases.some((alias) => alias.toLowerCase() === norm)) return true;
      return false;
    }) ||
    MODEL_CATALOG.find((entry) => {
      if (norm.includes('lite') && entry.id.includes('lite')) return true;
      if (!norm.includes('lite') && (norm.includes('flash') || norm.includes('banana')) && !entry.id.includes('lite')) return true;
      return false;
    }) ||
    null
  );
}

/**
 * Returns the catalog entry to run: matches `requestedModelId` if provided, otherwise `DEFAULT_MODEL_ID`.
 */
function configuredModel(requestedModelId) {
  if (requestedModelId) {
    const matched = findModel(requestedModelId);
    if (matched) return matched;
  }
  const declared = MODEL_CATALOG.find((entry) => entry.id === DEFAULT_MODEL_ID) || MODEL_CATALOG[0];
  if (isProviderConfigured(declared.provider)) return declared;
  return MODEL_CATALOG.find((entry) => isProviderConfigured(entry.provider)) || declared;
}

function getModelPricing(modelId) {
  const model = findModel(modelId) || configuredModel(modelId);
  return model?.pricing || {
    inputPerMillion: 0.50,
    outputPerMillion: 3.00,
    imageOutput: 0.00006
  };
}

module.exports = {
  DEFAULT_MODEL_ID,
  MODEL_CATALOG,
  providerKeyName,
  isProviderConfigured,
  configuredModel,
  findModel,
  getModelPricing
};
