const sharp = require('sharp');

/**
 * Image processing utilities for Battlemap Builder
 * Handles canvas padding for outpainting, soft-edge alpha feathering,
 * mask overlaying for inpainting, and map stitching.
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


const OUTPAINT_MIN_DEPTH_PX = 16;
const OUTPAINT_MAX_DEPTH_PX = 2048;
const OUTPAINT_DEFAULT_DEPTH_PX = 512;
// What the model is asked to fill vs. the part of the new frame the user did not select.
const OUTPAINT_FILL_COLOR = { r: 15, g: 20, b: 30 };
const OUTPAINT_VOID_COLOR = { r: 255, g: 0, b: 255 };

const clampInt = (value, min, max, fallback) => {
  const n = Math.round(Number(value));
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, n));
};

/** Greyscale rendering of an SVG (white = opaque), ready to become an alpha channel. */
async function svgToGrey(svg, blurSigma = 0) {
  let pipeline = sharp(Buffer.from(svg)).flatten({ background: '#ffffff' }).greyscale();
  if (blurSigma > 0) pipeline = pipeline.blur(blurSigma);
  return pipeline.png().toBuffer();
}

/** Black RGB frame carrying `greyBuffer` as its alpha channel, for use with `dest-in`. */
async function greyToAlphaLayer(greyBuffer, width, height) {
  return sharp({ create: { width, height, channels: 3, background: '#000000' } })
    .joinChannel(greyBuffer)
    .png()
    .toBuffer();
}

/**
 * Creates an expanded canvas image for Outpainting.
 * Returns the padded base image, cyan guide image, binary mask image, and geometry dimensions.
 */
async function prepareOutpaintCanvas(originalDataUrl, direction = 'east', expandAmount = OUTPAINT_DEFAULT_DEPTH_PX, span = null) {
  const inputBuffer = dataUrlToBuffer(originalDataUrl);
  const metadata = await sharp(inputBuffer).metadata();
  const origW = metadata.width;
  const origH = metadata.height;

  const requested = String(direction || 'east').toLowerCase();
  const dir = ['north', 'south', 'east', 'west', 'all'].includes(requested) ? requested : 'east';
  const depth = clampInt(expandAmount, OUTPAINT_MIN_DEPTH_PX, OUTPAINT_MAX_DEPTH_PX, OUTPAINT_DEFAULT_DEPTH_PX);

  const alongHorizontalEdge = dir === 'north' || dir === 'south';
  const edgeLength = alongHorizontalEdge || dir === 'all' ? origW : origH;
  let spanStart = 0;
  let spanEnd = edgeLength;
  if (span && dir !== 'all') {
    const start = clampInt(span.start, 0, edgeLength - 1, 0);
    const end = clampInt(span.end, start + 1, edgeLength, edgeLength);
    spanStart = start;
    spanEnd = end;
  }
  const spanLength = spanEnd - spanStart;
  const partial = spanLength < edgeLength;

  let newW = origW;
  let newH = origH;
  let left = 0;
  let top = 0;
  let region = null; // the strip the model must fill, in new-canvas coordinates

  switch (dir) {
    case 'north':
      newH = origH + depth;
      top = depth;
      region = { left: spanStart, top: 0, width: spanLength, height: depth };
      break;
    case 'south':
      newH = origH + depth;
      region = { left: spanStart, top: origH, width: spanLength, height: depth };
      break;
    case 'west':
      newW = origW + depth;
      left = depth;
      region = { left: 0, top: spanStart, width: depth, height: spanLength };
      break;
    case 'east':
      newW = origW + depth;
      region = { left: origW, top: spanStart, width: depth, height: spanLength };
      break;
    case 'all':
      newW = origW + depth * 2;
      newH = origH + depth * 2;
      left = depth;
      top = depth;
      region = { left: 0, top: 0, width: newW, height: newH };
      break;
  }

  // 1. Base extended canvas: dark neutral background + original map at top,left + dark fill in region
  const fillLayer = region ? [{
    input: await sharp({
      create: { width: region.width, height: region.height, channels: 3, background: OUTPAINT_FILL_COLOR }
    }).png().toBuffer(),
    top: region.top,
    left: region.left
  }] : [];

  const extendedImageBuffer = await sharp({
    create: {
      width: newW,
      height: newH,
      channels: 4,
      background: { r: 10, g: 13, b: 20, alpha: 1 }
    }
  })
    .composite([
      ...fillLayer,
      { input: inputBuffer, top, left }
    ])
    .png()
    .toBuffer();

  // 2. Guide image: extended canvas with cyan tint inside region
  let guideImageBuffer = extendedImageBuffer;
  if (region) {
    const cyanLayer = await sharp({
      create: { width: region.width, height: region.height, channels: 4, background: { r: 34, g: 211, b: 238, alpha: 0.45 } }
    }).png().toBuffer();
    guideImageBuffer = await sharp(extendedImageBuffer)
      .composite([{ input: cyanLayer, top: region.top, left: region.left }])
      .png()
      .toBuffer();
  }

  // 3. Binary mask image: WHITE inside region ONLY, BLACK everywhere else
  const maskLayers = region ? [{
    input: await sharp({
      create: { width: region.width, height: region.height, channels: 3, background: { r: 255, g: 255, b: 255 } }
    }).png().toBuffer(),
    top: region.top,
    left: region.left
  }] : [{
    input: await sharp({
      create: { width: newW, height: newH, channels: 3, background: { r: 255, g: 255, b: 255 } }
    }).png().toBuffer(),
    top: 0,
    left: 0
  }];

  const maskImageBuffer = await sharp({
    create: {
      width: newW,
      height: newH,
      channels: 3,
      background: { r: 0, g: 0, b: 0 }
    }
  })
    .composite(maskLayers)
    .png()
    .toBuffer();

  return {
    extendedImage: bufferToDataUrl(extendedImageBuffer),
    guideImage: bufferToDataUrl(guideImageBuffer),
    maskImage: bufferToDataUrl(maskImageBuffer),
    dimensions: {
      width: newW,
      height: newH,
      origW,
      origH,
      top,
      left,
      direction: dir,
      expandAmount: depth,
      span: { start: spanStart, end: spanEnd, length: edgeLength },
      region,
      hasVoid: partial
    }
  };
}

/**
 * Builds the final stitched map: model output is masked strictly inside the selected expansion region
 * (with soft outer edges), and composited with the original map (feathered along the seam).
 */
async function featheredOutpaintComposite(origBuffer, extBuffer, dimensions, featherPx = 48, outerFeatherPx = 12) {
  const { width: newW, height: newH, top, left, direction, span, region } = dimensions;
  const origMeta = await sharp(origBuffer).metadata();
  const w = origMeta.width;
  const h = origMeta.height;
  const feather = Math.max(1, Math.min(featherPx, Math.floor(Math.min(w, h) / 2)));
  const segmentLength = span ? span.end - span.start : (direction === 'north' || direction === 'south' ? w : h);

  // 1. Build mask for model output: white inside region + featherPx seam overlap into orig map
  let seamLeft = region ? region.left : 0;
  let seamTop = region ? region.top : 0;
  let seamWidth = region ? region.width : newW;
  let seamHeight = region ? region.height : newH;

  if (region) {
    switch (direction) {
      case 'east':
        seamLeft -= feather;
        seamWidth += feather;
        break;
      case 'west':
        seamWidth += feather;
        break;
      case 'north':
        seamHeight += feather;
        break;
      case 'south':
        seamTop -= feather;
        seamHeight += feather;
        break;
    }
  }

  const modelMaskSvg = `<svg width="${newW}" height="${newH}" xmlns="http://www.w3.org/2000/svg">
    <rect x="0" y="0" width="${newW}" height="${newH}" fill="black" />
    <rect x="${seamLeft}" y="${seamTop}" width="${seamWidth}" height="${seamHeight}" fill="white" rx="${outerFeatherPx}" />
  </svg>`;

  const modelMaskBuffer = await sharp(Buffer.from(modelMaskSvg))
    .blur(Math.max(1, outerFeatherPx))
    .png()
    .toBuffer();

  const modelAlphaLayer = await greyToAlphaLayer(modelMaskBuffer, newW, newH);

  const maskedModelLayer = await sharp(extBuffer)
    .resize(newW, newH, { fit: 'fill' })
    .ensureAlpha()
    .composite([{ input: modelAlphaLayer, blend: 'dest-in' }])
    .png()
    .toBuffer();

  // 2. Feather original map along the seam (only across selected span)
  const spanStart = span ? span.start : 0;
  const bands = [];
  switch (direction) {
    case 'east': bands.push({ x: w - feather, y: spanStart, width: feather, height: segmentLength }); break;
    case 'west': bands.push({ x: 0, y: spanStart, width: feather, height: segmentLength }); break;
    case 'north': bands.push({ x: spanStart, y: 0, width: segmentLength, height: feather }); break;
    case 'south': bands.push({ x: spanStart, y: h - feather, width: segmentLength, height: feather }); break;
    default:
      bands.push(
        { x: 0, y: 0, width: w, height: feather },
        { x: 0, y: h - feather, width: w, height: feather },
        { x: 0, y: 0, width: feather, height: h },
        { x: w - feather, y: 0, width: feather, height: h }
      );
  }

  const origFeatherSvg = `<svg width="${w}" height="${h}" xmlns="http://www.w3.org/2000/svg">
    <rect x="0" y="0" width="${w}" height="${h}" fill="white" />
    ${bands.map((b) => `<rect x="${b.x}" y="${b.y}" width="${b.width}" height="${b.height}" fill="black" />`).join('\n    ')}
  </svg>`;

  const origFeatherBuffer = await svgToGrey(origFeatherSvg, Math.max(1, Math.round(feather / 2)));
  const origAlphaLayer = await greyToAlphaLayer(origFeatherBuffer, w, h);

  const featheredOrig = await sharp(origBuffer)
    .ensureAlpha()
    .composite([{ input: origAlphaLayer, blend: 'dest-in' }])
    .png()
    .toBuffer();

  // 3. Composite feathered original over masked model layer
  const finalStitched = await sharp({
    create: { width: newW, height: newH, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } }
  })
    .composite([
      { input: maskedModelLayer, top: 0, left: 0 },
      { input: featheredOrig, top, left }
    ])
    .png()
    .toBuffer();

  return bufferToDataUrl(finalStitched);
}


/**
 * Prepares inpainting composite by blending user mask onto original image.
 */
async function prepareInpaintCanvas(originalDataUrl, maskDataUrl) {
  const origBuffer = dataUrlToBuffer(originalDataUrl);
  const maskBuffer = dataUrlToBuffer(maskDataUrl);

  const origMeta = await sharp(origBuffer).metadata();

  const resizedMask = await sharp(maskBuffer)
    .resize(origMeta.width, origMeta.height, { fit: 'fill' })
    .toBuffer();

  return {
    originalBuffer: origBuffer,
    maskBuffer: resizedMask,
    originalDataUrl: bufferToDataUrl(origBuffer),
    maskDataUrl: bufferToDataUrl(resizedMask),
    width: origMeta.width,
    height: origMeta.height
  };
}

/**
 * Bounding box of the painted (non-black) mask pixels at the target resolution,
 * or null when the mask is empty. Bounds the region any model edit may touch.
 */
async function measureMaskBounds(maskBuffer, width, height, threshold = 16) {
  const { data, info } = await sharp(maskBuffer)
    .resize(width, height, { fit: 'fill' })
    .greyscale()
    .raw()
    .toBuffer({ resolveWithObject: true });

  const w = info.width;
  const h = info.height;
  let left = w;
  let top = h;
  let right = -1;
  let bottom = -1;

  for (let y = 0; y < h; y++) {
    const row = y * w;
    for (let x = 0; x < w; x++) {
      if (data[row + x] > threshold) {
        if (x < left) left = x;
        if (x > right) right = x;
        if (y < top) top = y;
        if (y > bottom) bottom = y;
      }
    }
  }

  if (right < 0) return null;
  return { left, top, width: right - left + 1, height: bottom - top + 1 };
}

/**
 * Builds the visual mask guide sent to Gemini: the source map with the painted region tinted, so
 * the model sees which pixels it may rewrite (the tint itself must never appear in its output).
 */
async function buildInpaintGuide(originalBuffer, maskBuffer, width, height, tint = { r: 255, g: 32, b: 32 }, opacity = 0.6) {
  const alpha = await sharp(maskBuffer)
    .resize(width, height, { fit: 'fill' })
    .greyscale()
    .linear(opacity, 0)
    .png()
    .toBuffer();

  const tintLayer = await sharp({
    create: { width, height, channels: 3, background: tint }
  })
    .joinChannel(alpha)
    .png()
    .toBuffer();

  return sharp(originalBuffer)
    .resize(width, height, { fit: 'fill' })
    .flatten({ background: '#000000' })
    .composite([{ input: tintLayer, blend: 'over' }])
    .png()
    .toBuffer();
}

/**
 * Composites the model's inpainted render back onto the original through the painted mask.
 * The patch is clipped to the mask bounding box, so every pixel outside it keeps its original
 * bytes; inside it the feathered mask alpha decides how much of the model output shows through.
 */
async function compositeInpaintRegion(originalBuffer, editedBuffer, maskBuffer, width, height, featherPx = 6) {
  const bounds = await measureMaskBounds(maskBuffer, width, height);

  if (!bounds) {
    return bufferToDataUrl(await sharp(originalBuffer).png().toBuffer());
  }

  let alphaFull = await sharp(maskBuffer)
    .resize(width, height, { fit: 'fill' })
    .greyscale()
    .png()
    .toBuffer();

  // Materialise the blur before cropping so the feather is not recomputed against cropped edges.
  if (featherPx > 0) {
    alphaFull = await sharp(alphaFull).blur(featherPx).png().toBuffer();
  }

  const alphaPatch = await sharp(alphaFull).extract(bounds).png().toBuffer();

  // Materialise the resize/flatten first so `extract` can never be applied at the source resolution.
  const editedFull = await sharp(editedBuffer)
    .resize(width, height, { fit: 'fill' })
    .flatten({ background: '#000000' })
    .png()
    .toBuffer();

  const editedPatch = await sharp(editedFull).extract(bounds).png().toBuffer();

  const patch = await sharp(editedPatch).joinChannel(alphaPatch).png().toBuffer();

  const composited = await sharp(originalBuffer)
    .composite([{ input: patch, top: bounds.top, left: bounds.left }])
    .png()
    .toBuffer();

  return bufferToDataUrl(composited);
}

module.exports = {
  dataUrlToBuffer,
  bufferToDataUrl,
  prepareOutpaintCanvas,
  featheredOutpaintComposite,
  prepareInpaintCanvas,
  measureMaskBounds,
  buildInpaintGuide,
  compositeInpaintRegion
};
