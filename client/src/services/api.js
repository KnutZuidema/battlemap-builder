/**
 * API client service for communicating with Battlemap Builder Express Backend
 */

const API_BASE = '';

/**
 * Posts JSON to a backend route. Failures reject with an Error carrying the server's `code` and
 * `debug` telemetry, so the UI can show why the selected model produced nothing rather than a
 * bare status.
 */
async function postJson(path, body) {
  const res = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  });

  const data = await res.json().catch(() => null);

  if (!res.ok) {
    const err = new Error((data && data.error) || `Server error ${res.status}`);
    err.code = (data && data.code) || 'REQUEST_FAILED';
    err.debug = (data && data.debug) || null;
    throw err;
  }

  return data;
}

/**
 * Fetches JSON from a backend route.
 */
async function getJson(path) {
  const res = await fetch(`${API_BASE}${path}`);
  const data = await res.json().catch(() => null);

  if (!res.ok) {
    const err = new Error((data && data.error) || `Server error ${res.status}`);
    err.code = (data && data.code) || 'REQUEST_FAILED';
    err.debug = (data && data.debug) || null;
    throw err;
  }

  return data;
}

export async function checkServerHealth() {
  try {
    const res = await fetch(`${API_BASE}/api/health`);
    if (!res.ok) throw new Error('Healthcheck failed');
    return await res.json();
  } catch (err) {
    return { status: 'error', error: err.message };
  }
}

export async function fetchModelsApi() {
  try {
    const res = await fetch(`${API_BASE}/api/models`);
    if (!res.ok) throw new Error('Failed to fetch models');
    return await res.json();
  } catch (err) {
    return null;
  }
}

/** Request battlemap generation, optionally specifying a model */
export async function generateBattlemapApi({ prompt, width, height, model }) {
  return postJson('/api/generate', { prompt, width, height, model });
}

/** Request unified region forging (edit, expand, or hybrid) */
export async function forgeRegionApi({ originalDataUrl, selection, prompt, model }) {
  return postJson('/api/forge-region', { originalDataUrl, selection, prompt, model });
}

/**
 * Wiki endpoints for lore navigation and tactical prompt translation
 */
export async function fetchWikiTreeApi() {
  return getJson('/api/wiki/tree');
}

export async function fetchWikiArticleApi(articlePath) {
  return getJson(`/api/wiki/article?path=${encodeURIComponent(articlePath)}`);
}

export async function suggestWikiPromptsApi(articlePath) {
  return postJson('/api/wiki/prompts/suggest', { path: articlePath });
}

export async function generateCustomWikiPromptApi(articlePath, userIntent) {
  return postJson('/api/wiki/prompts/custom', { path: articlePath, userIntent });
}

export async function suggestWikiRegionForgeApi({ articlePath, selection, mapDimensions, mapTitle, basePrompt, userIntent }) {
  return postJson('/api/wiki/regions/suggest', {
    articlePath,
    selection,
    mapDimensions,
    mapTitle,
    basePrompt,
    userIntent
  });
}