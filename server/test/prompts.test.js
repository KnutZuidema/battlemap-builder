const test = require('node:test');
const assert = require('node:assert');
const { IMAGE_PROMPTS, buildBattlemapPrompt, getForgeRegionSystemPrompt } = require('../services/prompts');

test('IMAGE_PROMPTS enforce flat 2D architectural plan view, poché wall cross-sections, and gridless scale', () => {
  assert.match(IMAGE_PROMPTS.GENERATE, /architectural plan view/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /poché/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /zero vertical wall faces/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /reference battlemap image/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /gridless/i);
  assert.doesNotMatch(IMAGE_PROMPTS.GENERATE, /grid readability/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /64 pixels = 5 feet/i);
  assert.match(IMAGE_PROMPTS.GENERATE, /16 by 16/i);

  assert.match(IMAGE_PROMPTS.FORGE_REGION, /architectural plan view/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION, /poché/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION, /gridless/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION, /64 pixels = 5 feet/i);

  assert.match(IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED, /architectural plan view/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED, /poché/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED, /gridless/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED, /64 pixels = 5 feet/i);
  assert.match(IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED, /ONE image/i);

  assert.strictEqual(getForgeRegionSystemPrompt('highlighted-only'), IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED);
  assert.strictEqual(getForgeRegionSystemPrompt('all-three'), IMAGE_PROMPTS.FORGE_REGION);
  assert.strictEqual(getForgeRegionSystemPrompt(), IMAGE_PROMPTS.FORGE_REGION_HIGHLIGHTED);
  const prompt = buildBattlemapPrompt('A grand library');
  assert.match(prompt, /poché/i);
  assert.match(prompt, /architectural floor plan/i);
  assert.match(prompt, /gridless/i);
  assert.match(prompt, /64px = 5ft/i);
});
