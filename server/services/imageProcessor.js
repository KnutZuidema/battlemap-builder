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

/**
 * Creates an expanded canvas image for Outpainting.
 * Extends canvas in direction ('north', 'south', 'east', 'west', 'all')
 * and returns both the padded base image and dimensions.
 */
async function prepareOutpaintCanvas(originalDataUrl, direction = 'east', expandAmount = 512, overlap = 128) {
  const inputBuffer = dataUrlToBuffer(originalDataUrl);
  const metadata = await sharp(inputBuffer).metadata();
  const origW = metadata.width;
  const origH = metadata.height;

  let newW = origW;
  let newH = origH;
  let left = 0;
  let top = 0;

  switch (direction.toLowerCase()) {
    case 'north':
      newH = origH + expandAmount;
      top = expandAmount;
      break;
    case 'south':
      newH = origH + expandAmount;
      top = 0;
      break;
    case 'west':
      newW = origW + expandAmount;
      left = expandAmount;
      break;
    case 'east':
      newW = origW + expandAmount;
      left = 0;
      break;
    case 'all':
      newW = origW + expandAmount * 2;
      newH = origH + expandAmount * 2;
      left = expandAmount;
      top = expandAmount;
      break;
    default:
      newW = origW + expandAmount;
  }

  // Create a canvas with neutral background for the extended area
  const extendedImageBuffer = await sharp({
    create: {
      width: newW,
      height: newH,
      channels: 4,
      background: { r: 15, g: 20, b: 30, alpha: 1 }
    }
  })
    .composite([{ input: inputBuffer, top, left }])
    .png()
    .toBuffer();

  return {
    extendedImage: bufferToDataUrl(extendedImageBuffer),
    dimensions: { width: newW, height: newH, origW, origH, top, left, direction, expandAmount }
  };
}

/**
 * Composites original map onto extended map buffer with soft directional alpha feathering
 * to eliminate stark hard line borders along outpainted edges.
 */
async function featheredOutpaintComposite(origBuffer, extBuffer, top, left, direction = 'east', featherPx = 48) {
  const origMeta = await sharp(origBuffer).metadata();
  const w = origMeta.width;
  const h = origMeta.height;

  // Calculate inset feathering margins depending on seam connection direction
  const topInset = (direction === 'north' || direction === 'all') ? featherPx : 0;
  const bottomInset = (direction === 'south' || direction === 'all') ? featherPx : 0;
  const leftInset = (direction === 'west' || direction === 'all') ? featherPx : 0;
  const rightInset = (direction === 'east' || direction === 'all') ? featherPx : 0;

  // Build SVG mask with blurred rounded rect edge
  const maskSvg = `<svg width="${w}" height="${h}" xmlns="http://www.w3.org/2000/svg">
    <rect x="0" y="0" width="${w}" height="${h}" fill="black" />
    <rect x="${leftInset}" y="${topInset}" width="${Math.max(1, w - leftInset - rightInset)}" height="${Math.max(1, h - topInset - bottomInset)}" fill="white" rx="${Math.max(1, featherPx / 2)}" />
  </svg>`;

  // Render SVG mask and apply Gaussian blur for 100% soft alpha transition
  const maskBuf = await sharp(Buffer.from(maskSvg))
    .blur(Math.max(1, Math.round(featherPx / 2)))
    .png()
    .toBuffer();

  // Apply dest-in to create smooth alpha fade along connection seam
  const featheredOrig = await sharp(origBuffer)
    .composite([{ input: maskBuf, blend: 'dest-in' }])
    .png()
    .toBuffer();

  // Composite soft-edged original map over newly generated extension map
  const finalStitched = await sharp(extBuffer)
    .composite([{ input: featheredOrig, top, left }])
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
