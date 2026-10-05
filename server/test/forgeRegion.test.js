const test = require('node:test');
const assert = require('node:assert');
const sharp = require('sharp');
const { forgeRegion } = require('../services/battlemapService');
const openrouterProvider = require('../services/providers/openrouterProvider');

test('forgeRegion sends only 1 highlighted image when forgeImageMode is highlighted-only (default)', async (t) => {
  const originalRequestCanvas = openrouterProvider.requestCanvas;

  // Create a minimal 128x128 PNG data URL for test
  const mapBuffer = await sharp({
    create: {
      width: 128,
      height: 128,
      channels: 3,
      background: { r: 50, g: 50, b: 50 }
    }
  }).png().toBuffer();
  const originalDataUrl = `data:image/png;base64,${mapBuffer.toString('base64')}`;

  let capturedArgs = null;
  openrouterProvider.requestCanvas = async (args) => {
    capturedArgs = args;
    // Return a dummy 128x128 image
    const outBuffer = await sharp({
      create: {
        width: args.width,
        height: args.height,
        channels: 3,
        background: { r: 100, g: 100, b: 100 }
      }
    }).png().toBuffer();
    return {
      imageBuffer: outBuffer,
      debug: { usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 } }
    };
  };

  t.after(() => {
    openrouterProvider.requestCanvas = originalRequestCanvas;
  });

  const res = await forgeRegion({
    originalDataUrl,
    selection: { x: 32, y: 32, width: 64, height: 64 },
    prompt: 'Add an ancient altar',
    forgeImageMode: 'highlighted-only'
  });

  assert.ok(res.imageDataUrl);
  assert.ok(capturedArgs);
  assert.strictEqual(capturedArgs.images.length, 1);
  assert.match(capturedArgs.images[0].label, /highlighted/i);
  assert.match(capturedArgs.systemInstruction, /ONE image/i);
});

test('forgeRegion sends all 3 images when forgeImageMode is all-three', async (t) => {
  const originalRequestCanvas = openrouterProvider.requestCanvas;

  const mapBuffer = await sharp({
    create: {
      width: 128,
      height: 128,
      channels: 3,
      background: { r: 50, g: 50, b: 50 }
    }
  }).png().toBuffer();
  const originalDataUrl = `data:image/png;base64,${mapBuffer.toString('base64')}`;

  let capturedArgs = null;
  openrouterProvider.requestCanvas = async (args) => {
    capturedArgs = args;
    const outBuffer = await sharp({
      create: {
        width: args.width,
        height: args.height,
        channels: 3,
        background: { r: 100, g: 100, b: 100 }
      }
    }).png().toBuffer();
    return {
      imageBuffer: outBuffer,
      debug: { usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 } }
    };
  };

  t.after(() => {
    openrouterProvider.requestCanvas = originalRequestCanvas;
  });

  const res = await forgeRegion({
    originalDataUrl,
    selection: { x: 32, y: 32, width: 64, height: 64 },
    prompt: 'Add an ancient altar',
    forgeImageMode: 'all-three'
  });

  assert.ok(res.imageDataUrl);
  assert.ok(capturedArgs);
  assert.strictEqual(capturedArgs.images.length, 3);
  assert.match(capturedArgs.images[0].label, /context window/i);
  assert.match(capturedArgs.images[1].label, /cyan tint/i);
  assert.match(capturedArgs.images[2].label, /binary mask/i);
  assert.match(capturedArgs.systemInstruction, /three images/i);
});
