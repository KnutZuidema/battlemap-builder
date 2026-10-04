const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const {
  REFERENCE_DEFINITIONS,
  getReferenceImagesForPrompt
} = require('../services/referenceImages');
const { IMAGE_PROMPTS, buildBattlemapPrompt } = require('../services/prompts');

test('REFERENCE_DEFINITIONS contains curated battlemap references', () => {
  assert.strictEqual(REFERENCE_DEFINITIONS.length, 4);
  const ids = REFERENCE_DEFINITIONS.map(d => d.id);
  assert.ok(ids.includes('stone-dungeon'));
  assert.ok(ids.includes('ruins-courtyard'));
  assert.ok(ids.includes('tavern-inn'));
  assert.ok(ids.includes('grand-hall-portal'));
});

test('getReferenceImagesForPrompt selects matching reference based on keywords', async () => {
  const tavernImages = await getReferenceImagesForPrompt('A noisy tavern with a hearth and wooden tables');
  assert.strictEqual(tavernImages.length, 1);
  assert.match(tavernImages[0].label, /Tavern/);
  assert.strictEqual(tavernImages[0].mimeType, 'image/jpeg');

  const ruinsImages = await getReferenceImagesForPrompt('Ancient mossy ruins beside a rushing river');
  assert.strictEqual(ruinsImages.length, 1);
  assert.match(ruinsImages[0].label, /Ruins/);

  const dungeonImages = await getReferenceImagesForPrompt('A dark underground crypt with stone sarcophagi');
  assert.strictEqual(dungeonImages.length, 1);
  assert.match(dungeonImages[0].label, /Stone Dungeon/);

  // Default when prompt has no specific keywords
  const defaultImages = await getReferenceImagesForPrompt('A mysterious place');
  assert.strictEqual(defaultImages.length, 1);
  assert.ok(defaultImages[0].buffer.length > 0);

  // Verify buffer is valid image and resized to 512x512
  const meta = await sharp(defaultImages[0].buffer).metadata();
  assert.strictEqual(meta.width, 512);
  assert.strictEqual(meta.height, 512);
  assert.strictEqual(meta.format, 'jpeg');
});

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

  const prompt = buildBattlemapPrompt('A grand library');
  assert.match(prompt, /poché/i);
  assert.match(prompt, /architectural floor plan/i);
  assert.match(prompt, /gridless/i);
  assert.match(prompt, /64px = 5ft/i);
});
