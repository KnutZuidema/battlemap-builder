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
    label: 'Gemini 3.1 Flash Image',
    provider: 'openrouter',
    vision: true,
    costHint: '$0.50/M in · $3/M out (~$0.07 per map, edit or expansion)',
    note: 'True image render: measured ~12 s generation, ~14 s masked edit, ~13 s expansion. Painted battlemap quality; supports mask edits.'
  },
  {
    id: 'google/gemini-3-pro-image',
    label: 'Gemini 3 Pro Image',
    provider: 'openrouter',
    vision: true,
    costHint: '$2/M in · $12/M out (~$0.14 per map)',
    note: 'Highest-fidelity Google image model: generation measured ~23 s / ~$0.14. Shares the verified image contract with Flash Image; mask edits and expansion were probed on Flash Image only.'
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

/**
 * The catalog entry every request runs: `DEFAULT_MODEL_ID` when its provider key is present,
 * otherwise the first entry whose key is present — a keyless provider must not turn a plain request
 * into a NO_API_KEY failure while a usable model exists.
 */
function configuredModel() {
  const declared = MODEL_CATALOG.find((entry) => entry.id === DEFAULT_MODEL_ID) || MODEL_CATALOG[0];
  if (isProviderConfigured(declared.provider)) return declared;
  return MODEL_CATALOG.find((entry) => isProviderConfigured(entry.provider)) || declared;
}

module.exports = {
  DEFAULT_MODEL_ID,
  providerKeyName,
  isProviderConfigured,
  configuredModel
};
