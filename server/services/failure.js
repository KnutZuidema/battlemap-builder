/**
 * One failure vocabulary shared by every map provider.
 *
 * Every failing path throws an Error whose `code` drives the HTTP status and whose `telemetry`
 * (prompts, model attempt trail, raw response) is forwarded to the client, so the error banner and
 * the debug log always describe the attempt that actually failed — never a generic status.
 */

const PROVIDER_LABELS = {
  openrouter: 'OpenRouter'
};

const NO_KEY_MESSAGES = {
  openrouter: 'OPENROUTER_API_KEY is empty in server/.env (or the server environment), so no map model can run. Set the key and restart the server.'
};

function providerLabel(provider) {
  return PROVIDER_LABELS[provider] || provider || 'Model provider';
}

function noApiKeyMessage(provider) {
  return NO_KEY_MESSAGES[provider] || `${providerLabel(provider)} API key is not configured. Set it in server/.env and restart the server.`;
}

/**
 * Flattens a raw upstream failure (SDK error, REST error body, network error) into one factual
 * line. SDK errors wrap a JSON body inside `message`, so the body is unwrapped when parseable.
 */
function summarizeUpstreamError(err) {
  const raw = (err && err.message) || String(err || 'Unknown error');
  try {
    const parsed = JSON.parse(raw);
    const apiError = parsed.error || parsed;
    return `${apiError.status ? `${apiError.status}: ` : ''}${apiError.message || raw}`;
  } catch {
    return raw.split('\n')[0];
  }
}

/**
 * Failures another model attempt cannot fix: dead key, blocked key, billing, rate/credit exhaustion.
 */
function isFatalUpstreamError(summary) {
  return /API_KEY_SERVICE_BLOCKED|RESOURCE_EXHAUSTED|prepayment credits|API key not valid|API_KEY_INVALID|insufficient_quota|No auth credentials|401|402|403|Unauthorized|Key limit exceeded|more credits/i.test(summary);
}

/**
 * Translates a raw provider failure into one actionable, user-facing sentence.
 */
function describeFailure(provider, detail) {
  const label = providerLabel(provider);

  if (provider === 'openrouter') {
    if (/401|Unauthorized|No auth credentials/i.test(detail)) {
      return 'OpenRouter rejected the API key (401 unauthorized): check the OPENROUTER_API_KEY value in server/.env or the environment.';
    }
    if (/402|insufficient|more credits|credit/i.test(detail)) {
      return `OpenRouter reported insufficient credits: top up the account at https://openrouter.ai/credits. Provider said: ${detail}`;
    }
    if (/404|No endpoints found|not a valid model/i.test(detail)) {
      return `OpenRouter has no usable endpoint for this model under the account's guardrail/privacy settings — it may have been renamed, retired, or excluded by a zero-data-retention policy (check https://openrouter.ai/settings/privacy). Provider said: ${detail}`;
    }
  }

  if (/fetch failed|ENOTFOUND|ECONNREFUSED|socket hang up|network|timeout|aborted/i.test(detail)) {
    return `Could not reach the ${label} API (network error): ${detail}`;
  }
  if (/^model returned no image/.test(detail)) {
    return `${label} responded but produced no image — ${detail}`;
  }
  return `${label} request failed: ${detail}`;
}

/**
 * Single error shape every failure path throws. `code` drives the HTTP status; `telemetry` is what
 * the API route forwards to the client.
 */
function providerFailure(message, {
  code = 'MODEL_FAILED',
  provider = null,
  attempts = [],
  rawResponseText = '',
  systemInstruction,
  userPrompt,
  fullPrompt,
  images = [],
  outputImages = [],
  usage = null
} = {}) {
  const err = new Error(message);
  err.code = code;
  err.telemetry = {
    provider,
    systemInstruction,
    userPrompt,
    fullPrompt,
    images,
    outputImages,
    rawResponseText,
    failureReason: message,
    attempts,
    usage
  };
  return err;
}

module.exports = {
  providerLabel,
  noApiKeyMessage,
  summarizeUpstreamError,
  isFatalUpstreamError,
  describeFailure,
  providerFailure
};