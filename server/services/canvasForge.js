const sharp = require('sharp');

/**
 * Canvas forging and region processing module for Battlemap Builder.
 * Encapsulates context framing, guide image rendering, mask generation,
 * in-memory render adapter execution, coordinate offset calculations,
 * and seamless soft-edge alpha compositing for region forging.
 */

// Convert base64 data URL to Buffer
function dataUrlToBuffer(dataUrl) {
  if (!dataUrl) return null;
  const base64Data = dataUrl.replace(/^data:image\/\w+;base64,/, '');
  return Buffer.from(base64Data, 'base64');
}

// Convert Buffer to PNG base64 Data URL
function bufferToDataUrl(buffer, mimeType = 'image/png') {
  return `data:${mimeType};base64,${buffer.toString('base64')}`;
}

// Background color for void / expansion areas in context frames
const REGION_VOID_COLOR = { r: 15, g: 20, b: 30 };

/** Black RGB frame carrying `greyBuffer` as its alpha channel, for use with `dest-in`. */
async function greyToAlphaLayer(greyBuffer, width, height) {
  return sharp({ create: { width, height, channels: 3, background: '#000000' } })
    .joinChannel(greyBuffer)
    .png()
    .toBuffer();
}

/**
 * Prepares a focused context frame (default 512px padding around selection), guide image,
 * and binary mask for unified region forging (edit, expand, and hybrid edits).
 */
async function prepareRegionCanvas(originalDataUrl, selection, contextPadding = 512) {
  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const metadata = await sharp(origBuffer).metadata();
  const origW = metadata.width;
  const origH = metadata.height;

  const selX = Math.round(selection.x);
  const selY = Math.round(selection.y);
  const selW = Math.round(selection.width);
  const selH = Math.round(selection.height);

  const minX = Math.min(0, selX);
  const minY = Math.min(0, selY);
  const maxX = Math.max(origW, selX + selW);
  const maxY = Math.max(origH, selY + selH);

  const newW = maxX - minX;
  const newH = maxY - minY;
  const origLeft = -minX;
  const origTop = -minY;
  const selLeft = selX - minX;
  const selTop = selY - minY;

  const contextLeft = Math.max(0, selLeft - contextPadding);
  const contextTop = Math.max(0, selTop - contextPadding);
  const contextRight = Math.min(newW, selLeft + selW + contextPadding);
  const contextBottom = Math.min(newH, selTop + selH + contextPadding);

  const contextW = contextRight - contextLeft;
  const contextH = contextBottom - contextTop;

  // Intersect context crop with existing map [origLeft, origTop, origW, origH]
  const intersectLeft = Math.max(origLeft, contextLeft);
  const intersectTop = Math.max(origTop, contextTop);
  const intersectRight = Math.min(origLeft + origW, contextRight);
  const intersectBottom = Math.min(origTop + origH, contextBottom);

  const hasIntersection = intersectRight > intersectLeft && intersectBottom > intersectTop;

  let contextBuffer;
  if (hasIntersection) {
    const intersectW = intersectRight - intersectLeft;
    const intersectH = intersectBottom - intersectTop;
    const cropX = intersectLeft - origLeft;
    const cropY = intersectTop - origTop;
    const placeX = intersectLeft - contextLeft;
    const placeY = intersectTop - contextTop;

    const mapCrop = await sharp(origBuffer)
      .extract({ left: cropX, top: cropY, width: intersectW, height: intersectH })
      .toBuffer();

    contextBuffer = await sharp({
      create: {
        width: contextW,
        height: contextH,
        channels: 3,
        background: REGION_VOID_COLOR
      }
    })
      .composite([{ input: mapCrop, left: placeX, top: placeY }])
      .png()
      .toBuffer();
  } else {
    contextBuffer = await sharp({
      create: {
        width: contextW,
        height: contextH,
        channels: 3,
        background: REGION_VOID_COLOR
      }
    })
      .png()
      .toBuffer();
  }

  const selOnContext = {
    left: selLeft - contextLeft,
    top: selTop - contextTop,
    width: selW,
    height: selH
  };

  // Guide image: context frame with cyan highlight box (30% opacity, 4px cyan stroke)
  const guideSvg = `<svg width="${contextW}" height="${contextH}" xmlns="http://www.w3.org/2000/svg">
    <rect x="${selOnContext.left}" y="${selOnContext.top}" width="${selOnContext.width}" height="${selOnContext.height}" fill="rgba(34, 211, 238, 0.3)" stroke="#22d3ee" stroke-width="4" />
  </svg>`;

  const guideBuffer = await sharp(contextBuffer)
    .composite([{ input: Buffer.from(guideSvg) }])
    .png()
    .toBuffer();

  // Binary mask: white inside selection, black everywhere else
  const maskSvg = `<svg width="${contextW}" height="${contextH}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${contextW}" height="${contextH}" fill="#000000" />
    <rect x="${selOnContext.left}" y="${selOnContext.top}" width="${selOnContext.width}" height="${selOnContext.height}" fill="#ffffff" />
  </svg>`;

  const maskBuffer = await sharp(Buffer.from(maskSvg))
    .png()
    .toBuffer();

  return {
    contextBuffer,
    guideBuffer,
    maskBuffer,
    dimensions: {
      newW,
      newH,
      origW,
      origH,
      origLeft,
      origTop,
      contextLeft,
      contextTop,
      contextW,
      contextH,
      selOnContext
    }
  };
}

/**
 * Composites the model's output buffer back onto the final expanded canvas.
 * Model output is masked to selOnContext with a soft 20px inward feather along preserved map edges,
 * ensuring zero bleed outside the box and smooth seamless internal transitions.
 */
async function compositeRegion(origBuffer, modelBuffer, dimensions, featherPx = 20) {
  const {
    newW,
    newH,
    origW,
    origH,
    origLeft,
    origTop,
    contextLeft,
    contextTop,
    contextW,
    contextH,
    selOnContext
  } = dimensions;

  const resizedModel = await sharp(modelBuffer)
    .resize(contextW, contextH, { fit: 'fill' })
    .flatten({ background: '#000000' })
    .ensureAlpha()
    .toBuffer();

  const feather = Math.max(1, Math.min(featherPx, Math.floor(Math.min(selOnContext.width, selOnContext.height) / 2)));

  const mapCtxLeft = origLeft - contextLeft;
  const mapCtxTop = origTop - contextTop;
  const mapCtxRight = mapCtxLeft + origW;
  const mapCtxBottom = mapCtxTop + origH;

  const overlapLeft = Math.max(selOnContext.left, mapCtxLeft);
  const overlapTop = Math.max(selOnContext.top, mapCtxTop);
  const overlapRight = Math.min(selOnContext.left + selOnContext.width, mapCtxRight);
  const overlapBottom = Math.min(selOnContext.top + selOnContext.height, mapCtxBottom);
  const hasOverlap = overlapRight > overlapLeft && overlapBottom > overlapTop;

  const defs = [];
  const bands = [];

  if (hasOverlap && selOnContext.left >= mapCtxLeft && selOnContext.left < mapCtxRight) {
    defs.push(`<linearGradient id="grad-left" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#000000" />
      <stop offset="100%" stop-color="#ffffff" />
    </linearGradient>`);
    bands.push(`<rect x="${selOnContext.left}" y="${overlapTop}" width="${feather}" height="${overlapBottom - overlapTop}" fill="url(#grad-left)" />`);
  }

  if (hasOverlap && selOnContext.left + selOnContext.width <= mapCtxRight && selOnContext.left + selOnContext.width > mapCtxLeft) {
    defs.push(`<linearGradient id="grad-right" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#ffffff" />
      <stop offset="100%" stop-color="#000000" />
    </linearGradient>`);
    bands.push(`<rect x="${selOnContext.left + selOnContext.width - feather}" y="${overlapTop}" width="${feather}" height="${overlapBottom - overlapTop}" fill="url(#grad-right)" />`);
  }

  if (hasOverlap && selOnContext.top >= mapCtxTop && selOnContext.top < mapCtxBottom) {
    defs.push(`<linearGradient id="grad-top" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#000000" />
      <stop offset="100%" stop-color="#ffffff" />
    </linearGradient>`);
    bands.push(`<rect x="${overlapLeft}" y="${selOnContext.top}" width="${overlapRight - overlapLeft}" height="${feather}" fill="url(#grad-top)" />`);
  }

  if (hasOverlap && selOnContext.top + selOnContext.height <= mapCtxBottom && selOnContext.top + selOnContext.height > mapCtxTop) {
    defs.push(`<linearGradient id="grad-bottom" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#ffffff" />
      <stop offset="100%" stop-color="#000000" />
    </linearGradient>`);
    bands.push(`<rect x="${overlapLeft}" y="${selOnContext.top + selOnContext.height - feather}" width="${overlapRight - overlapLeft}" height="${feather}" fill="url(#grad-bottom)" />`);
  }

  const alphaSvg = `<svg width="${contextW}" height="${contextH}" xmlns="http://www.w3.org/2000/svg">
    <defs>${defs.join('')}</defs>
    <rect width="${contextW}" height="${contextH}" fill="#000000" />
    <rect x="${selOnContext.left}" y="${selOnContext.top}" width="${selOnContext.width}" height="${selOnContext.height}" fill="#ffffff" />
    ${bands.join('\n    ')}
  </svg>`;

  // Strict binary rectangle to guarantee zero bleed outside selOnContext
  const strictClipSvg = `<svg width="${contextW}" height="${contextH}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${contextW}" height="${contextH}" fill="#000000" />
    <rect x="${selOnContext.left}" y="${selOnContext.top}" width="${selOnContext.width}" height="${selOnContext.height}" fill="#ffffff" />
  </svg>`;

  const strictClipBuffer = await sharp(Buffer.from(strictClipSvg)).greyscale().png().toBuffer();

  const alphaGreyBuffer = await sharp(Buffer.from(alphaSvg))
    .greyscale()
    .composite([{ input: await greyToAlphaLayer(strictClipBuffer, contextW, contextH), blend: 'dest-in' }])
    .png()
    .toBuffer();

  const modelAlphaLayer = await greyToAlphaLayer(alphaGreyBuffer, contextW, contextH);

  const maskedModel = await sharp(resizedModel)
    .composite([{ input: modelAlphaLayer, blend: 'dest-in' }])
    .png()
    .toBuffer();

  const finalStitched = await sharp({
    create: {
      width: newW,
      height: newH,
      channels: 4,
      background: { r: 0, g: 0, b: 0, alpha: 0 }
    }
  })
    .composite([
      { input: origBuffer, left: origLeft, top: origTop },
      { input: maskedModel, left: contextLeft, top: contextTop }
    ])
    .png()
    .toBuffer();

  return bufferToDataUrl(finalStitched);
}

/**
 * Forges a region on a battlemap (edit, expand, or hybrid).
 * Encapsulates the 11-field dimensions record, context framing, guide/mask generation,
 * in-memory render adapter execution, origin coordinate shift offset calculation,
 * and 20px inward seam feathering.
 *
 * @param {Object|string} optionsOrDataUrl - Options object { originalDataUrl, selection, renderAdapter, contextPadding, featherPx } or originalDataUrl string
 * @param {Object} [selectionArg] - Selection if called positionally
 * @param {Function} [renderAdapterArg] - Render adapter callback if called positionally
 * @param {number} [contextPaddingArg] - Context padding if called positionally
 * @param {number} [featherPxArg] - Feather pixels if called positionally
 * @returns {Promise<{ imageDataUrl: string, offset: { dx: number, dy: number }, debug?: any, usage?: any, cost?: number|null }>}
 */
async function forgeCanvasRegion(optionsOrDataUrl, selectionArg, renderAdapterArg, contextPaddingArg, featherPxArg) {
  let originalDataUrl;
  let selection;
  let renderAdapter;
  let contextPadding = 512;
  let featherPx = 20;

  if (
    typeof optionsOrDataUrl === 'object' &&
    optionsOrDataUrl !== null &&
    !Buffer.isBuffer(optionsOrDataUrl) &&
    ('originalDataUrl' in optionsOrDataUrl || 'selection' in optionsOrDataUrl)
  ) {
    originalDataUrl = optionsOrDataUrl.originalDataUrl;
    selection = optionsOrDataUrl.selection;
    renderAdapter = optionsOrDataUrl.renderAdapter;
    if (typeof optionsOrDataUrl.contextPadding === 'number') {
      contextPadding = optionsOrDataUrl.contextPadding;
    }
    if (typeof optionsOrDataUrl.featherPx === 'number') {
      featherPx = optionsOrDataUrl.featherPx;
    }
  } else {
    originalDataUrl = optionsOrDataUrl;
    selection = selectionArg;
    renderAdapter = renderAdapterArg;
    if (typeof contextPaddingArg === 'number') {
      contextPadding = contextPaddingArg;
    }
    if (typeof featherPxArg === 'number') {
      featherPx = featherPxArg;
    }
  }

  if (!originalDataUrl || !selection) {
    const err = new Error('originalDataUrl and selection are required for forgeCanvasRegion');
    err.code = 'INVALID_PARAMETERS';
    throw err;
  }

  const prepared = await prepareRegionCanvas(originalDataUrl, selection, contextPadding);
  const { dimensions, contextBuffer, guideBuffer, maskBuffer } = prepared;
  const { contextW, contextH, selOnContext } = dimensions;

  // Calculate origin coordinate shift offset { dx, dy } when expanding North or West into Void:
  // - When expanding West (selection.x < 0), dx = -minX (positive pixel shift of old map origin)
  // - When expanding North (selection.y < 0), dy = -minY (positive pixel shift of old map origin)
  // - When not expanding North/West, dx = 0, dy = 0
  const selX = Math.round(selection.x);
  const selY = Math.round(selection.y);
  const minX = Math.min(0, selX);
  const minY = Math.min(0, selY);
  const dx = selX < 0 ? -minX : 0;
  const dy = selY < 0 ? -minY : 0;
  const offset = { dx, dy };

  let adapterResult;
  if (typeof renderAdapter === 'function') {
    adapterResult = await renderAdapter({
      contextBuffer,
      guideBuffer,
      maskBuffer,
      contextW,
      contextH,
      selOnContext
    });
  } else {
    adapterResult = { imageBuffer: contextBuffer };
  }

  let modelBuffer;
  let debug = null;
  let usage = null;
  let cost = null;

  if (Buffer.isBuffer(adapterResult)) {
    modelBuffer = adapterResult;
  } else if (adapterResult && (Buffer.isBuffer(adapterResult.imageBuffer) || typeof adapterResult.imageBuffer === 'string')) {
    modelBuffer = Buffer.isBuffer(adapterResult.imageBuffer)
      ? adapterResult.imageBuffer
      : dataUrlToBuffer(adapterResult.imageBuffer);
    debug = adapterResult.debug || null;
    usage = adapterResult.usage || adapterResult.debug?.usage || null;
    cost = adapterResult.cost ?? adapterResult.debug?.usage?.cost ?? null;
  } else {
    throw new Error('renderAdapter must return a Buffer or an object containing imageBuffer');
  }

  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const finalStitchedDataUrl = await compositeRegion(origBuffer, modelBuffer, dimensions, featherPx);

  return {
    imageDataUrl: finalStitchedDataUrl,
    offset,
    debug,
    usage,
    cost
  };
}

/**
 * Deletes a rectangular region from the map image, setting the pixels inside
 * the selection rectangle to transparent.
 *
 * @param {string} originalDataUrl - The map image as a base64 data URL
 * @param {{x: number, y: number, width: number, height: number}} selection - Bounding box in map coordinates
 * @returns {Promise<string>} The updated image data URL with the region cleared
 */
async function deleteRegion(originalDataUrl, selection) {
  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const metadata = await sharp(origBuffer).metadata();
  const { width, height } = metadata;

  const selX = Math.round(selection.x);
  const selY = Math.round(selection.y);
  const selW = Math.round(selection.width);
  const selH = Math.round(selection.height);

  const intersectLeft = Math.max(0, selX);
  const intersectTop = Math.max(0, selY);
  const intersectRight = Math.min(width, selX + selW);
  const intersectBottom = Math.min(height, selY + selH);

  if (intersectRight <= intersectLeft || intersectBottom <= intersectTop) {
    return originalDataUrl;
  }

  const maskW = intersectRight - intersectLeft;
  const maskH = intersectBottom - intersectTop;

  const maskSvg = `<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
    <rect width="${width}" height="${height}" fill="#ffffff" />
    <rect x="${intersectLeft}" y="${intersectTop}" width="${maskW}" height="${maskH}" fill="#000000" />
  </svg>`;

  const maskBuffer = await sharp(Buffer.from(maskSvg)).greyscale().png().toBuffer();
  const maskAlphaLayer = await greyToAlphaLayer(maskBuffer, width, height);

  const resultBuffer = await sharp(origBuffer)
    .ensureAlpha()
    .composite([{ input: maskAlphaLayer, blend: 'dest-in' }])
    .png()
    .toBuffer();

  return bufferToDataUrl(resultBuffer);
}

module.exports = {
  dataUrlToBuffer,
  bufferToDataUrl,
  forgeCanvasRegion,
  deleteRegion,
  REGION_VOID_COLOR,
  prepareRegionCanvas,
  compositeRegion
};
