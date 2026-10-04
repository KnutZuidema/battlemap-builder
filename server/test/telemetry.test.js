const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const { extractImageSources, extractImageSource } = require('../services/providers/openrouterProvider');
const { providerFailure } = require('../services/failure');

test('extractImageSources extracts images from message.images array', () => {
  const payload = {
    choices: [
      {
        message: {
          images: [
            { image_url: { url: 'https://example.com/img1.png' } },
            { data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', mime_type: 'image/png' },
            'https://example.com/img3.png'
          ]
        }
      }
    ]
  };

  const sources = extractImageSources(payload);
  assert.strictEqual(sources.length, 3);
  assert.strictEqual(sources[0], 'https://example.com/img1.png');
  assert.match(sources[1], /^data:image\/png;base64,/);
  assert.strictEqual(sources[2], 'https://example.com/img3.png');

  const first = extractImageSource(payload);
  assert.strictEqual(first, 'https://example.com/img1.png');
});

test('extractImageSources extracts images from message.content parts', () => {
  const payload = {
    choices: [
      {
        message: {
          content: [
            { type: 'text', text: 'Here is your battlemap:' },
            { type: 'image_url', image_url: { url: 'data:image/png;base64,abc123' } }
          ]
        }
      }
    ]
  };

  const sources = extractImageSources(payload);
  assert.strictEqual(sources.length, 1);
  assert.strictEqual(sources[0], 'data:image/png;base64,abc123');
});

test('extractImageSources returns empty array when message has no images', () => {
  const payload = {
    choices: [
      {
        message: {
          content: 'Just text, no images'
        }
      }
    ]
  };

  const sources = extractImageSources(payload);
  assert.deepStrictEqual(sources, []);
  assert.strictEqual(extractImageSource(payload), null);
});

test('providerFailure includes images and outputImages in telemetry', () => {
  const err = providerFailure('Test failure', {
    code: 'TEST_ERROR',
    provider: 'openrouter',
    images: [{ id: 0, label: 'Input 1', dataUrl: 'data:image/png;base64,in' }],
    outputImages: [{ id: 0, label: 'Output 1', dataUrl: 'data:image/png;base64,out' }]
  });

  assert.strictEqual(err.code, 'TEST_ERROR');
  assert.strictEqual(err.telemetry.images.length, 1);
  assert.strictEqual(err.telemetry.outputImages.length, 1);
  assert.strictEqual(err.telemetry.outputImages[0].label, 'Output 1');
});

test('requestCanvas captures direct model output images in debug telemetry', async (t) => {
  const { requestCanvas } = require('../services/providers/openrouterProvider');
  const originalFetch = global.fetch;

  // 1x1 red PNG
  const pngBase64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  
  global.fetch = async (url) => {
    return {
      ok: true,
      status: 200,
      json: async () => ({
        choices: [
          {
            finish_reason: 'stop',
            message: {
              images: [
                { data: pngBase64, mime_type: 'image/png' }
              ]
            }
          }
        ],
        usage: { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 }
      })
    };
  };

  t.after(() => {
    global.fetch = originalFetch;
  });

  const result = await requestCanvas({
    model: 'google/gemini-3.1-flash-image',
    systemInstruction: 'Generate map',
    userPromptText: 'A dungeon room',
    fullPromptPayload: 'Generate map\n\nA dungeon room',
    images: [],
    width: 64,
    height: 64,
    apiKey: 'mock-key'
  });

  assert.ok(result.imageBuffer);
  assert.ok(result.debug);
  assert.strictEqual(result.debug.rawResponseText, '[stop response — 1 image output received]');
  assert.strictEqual(result.debug.outputImages.length, 1);
  assert.strictEqual(result.debug.outputImages[0].id, 0);
  assert.strictEqual(result.debug.outputImages[0].label, 'Model Output 1 (1×1 px)');
  assert.match(result.debug.outputImages[0].dataUrl, /^data:image\/png;base64,/);
});
