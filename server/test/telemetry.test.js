const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const { extractImageSources, extractImageSource, selectAspectRatio } = require('../services/providers/openrouterProvider');
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

test('selectAspectRatio maps dimensions to closest supported aspect ratios', () => {
  assert.strictEqual(selectAspectRatio(1024, 1024), '1:1');
  assert.strictEqual(selectAspectRatio(null, null), '1:1');
  assert.strictEqual(selectAspectRatio(1920, 1080), '16:9');
  assert.strictEqual(selectAspectRatio(1080, 1920), '9:16');
  assert.strictEqual(selectAspectRatio(1024, 768), '4:3');
  assert.strictEqual(selectAspectRatio(768, 1024), '3:4');
  assert.strictEqual(selectAspectRatio(1200, 800), '3:2');
  assert.strictEqual(selectAspectRatio(800, 1200), '2:3');
  assert.strictEqual(selectAspectRatio(1000, 800), '5:4');
  assert.strictEqual(selectAspectRatio(800, 1000), '4:5');
  assert.strictEqual(selectAspectRatio(2560, 1080), '21:9');
});

test('requestCanvas sends image_config with aspect_ratio in request body and debug telemetry', async (t) => {
  const { requestCanvas } = require('../services/providers/openrouterProvider');
  const originalFetch = global.fetch;
  const pngBase64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  let capturedBody = null;

  global.fetch = async (url, opts) => {
    capturedBody = JSON.parse(opts.body);
    return {
      ok: true,
      status: 200,
      json: async () => ({
        choices: [
          {
            finish_reason: 'stop',
            message: {
              images: [{ data: pngBase64, mime_type: 'image/png' }]
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
    width: 1024,
    height: 1024,
    apiKey: 'mock-key'
  });

  assert.ok(capturedBody);
  assert.deepStrictEqual(capturedBody.image_config, { aspect_ratio: '1:1' });
  assert.deepStrictEqual(result.debug.imageConfig, { aspect_ratio: '1:1' });
});

test('buildInputReferences selects only visual context for the images API and requestCanvas logs the adaptation', async (t) => {
  const { buildInputReferences, requestCanvas } = require('../services/providers/openrouterProvider');
  const buffer = Buffer.from('x');
  const images = [
    { role: 'context', label: 'Context window', buffer },
    { role: 'guide', label: 'Highlight guide', buffer },
    { role: 'mask', label: 'Region mask', buffer },
    { label: 'Perspective Reference', buffer }
  ];

  const refs = buildInputReferences(images, 'meta/muse-image');
  assert.strictEqual(refs.length, 2, 'context + reference images only');

  // Role, not label text, decides selection: a "mask"-worded context label is still sent.
  assert.strictEqual(
    buildInputReferences([{ role: 'context', label: 'Cyan mask lookalike label', buffer }], 'meta/muse-image').length,
    1
  );

  const originalFetch = global.fetch;
  const pngBase64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  const bodies = [];
  global.fetch = async (url, opts) => {
    bodies.push(JSON.parse(opts.body));
    return {
      ok: true,
      status: 200,
      json: async () => ({
        data: [{ b64_json: pngBase64 }],
        choices: [{ finish_reason: 'stop', message: { images: [{ data: pngBase64, mime_type: 'image/png' }] } }]
      })
    };
  };
  t.after(() => { global.fetch = originalFetch; });

  const imagesResult = await requestCanvas({
    model: 'meta/muse-image', systemInstruction: 's', userPromptText: 'u', fullPromptPayload: 'p',
    images, width: 64, height: 64, apiKey: 'k'
  });
  assert.strictEqual(bodies[0].input_references.length, 2);
  const c = imagesResult.debug.conditioning;
  assert.strictEqual(c.apiType, 'images');
  assert.strictEqual(c.mode, 'visual-context-only');
  assert.strictEqual(c.adapted, true);
  assert.deepStrictEqual(c.dropped.map((d) => d.role), ['guide', 'mask']);
  assert.deepStrictEqual(c.selected.map((d) => d.role), ['context', 'reference']);
  assert.match(c.explanation, /accepts only context\/reference/);

  const chatResult = await requestCanvas({
    model: 'google/gemini-3.1-flash-image', systemInstruction: 's', userPromptText: 'u', fullPromptPayload: 'p',
    images, width: 64, height: 64, apiKey: 'k'
  });
  assert.strictEqual(chatResult.debug.conditioning.adapted, false);
  assert.strictEqual(chatResult.debug.conditioning.selected.length, 4);
});

test('requestCanvas rejects instead of generating unconditioned when an images-API model keeps no conditioning image', async (t) => {
  const { requestCanvas } = require('../services/providers/openrouterProvider');
  const buffer = Buffer.from('x');
  const originalFetch = global.fetch;
  let fetched = false;
  global.fetch = async () => { fetched = true; throw new Error('must not be called'); };
  t.after(() => { global.fetch = originalFetch; });

  await assert.rejects(
    requestCanvas({
      model: 'meta/muse-image', systemInstruction: 's', userPromptText: 'u', fullPromptPayload: 'p',
      images: [{ role: 'guide', label: 'Highlight guide', buffer }, { role: 'mask', label: 'Region mask', buffer }],
      width: 64, height: 64, apiKey: 'k'
    }),
    (err) => {
      assert.strictEqual(err.code, 'MODEL_CAPABILITY_UNSUPPORTED');
      assert.deepStrictEqual(err.telemetry.images, []);
      assert.deepStrictEqual(err.telemetry.conditioning.selected, []);
      assert.deepStrictEqual(err.telemetry.conditioning.dropped.map((d) => d.role), ['guide', 'mask']);
      assert.strictEqual(err.telemetry.conditioning.adapted, true);
      return true;
    }
  );
  assert.strictEqual(fetched, false, 'no request is sent without conditioning');
});
