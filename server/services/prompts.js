/**
 * Prompt contract shared by every map provider.
 *
 * Map models render raster battlemap pixels directly — the reply IS the image — so a single
 * `IMAGE_PROMPTS` set covers generation, mask edits and canvas extension. A provider never
 * restates these directives; `battlemapService` assembles the payload from them and passes it down.
 */

/**
 * Defined System Prompts for image-output map models (they render pixels, not markup)
 */
const IMAGE_PROMPTS = {
  GENERATE: `You are a top-down 2D TTRPG battlemap renderer.
Render ONE battlemap image for a D&D 5e style virtual tabletop.

System & Layout Directives:
1. Camera: perfectly flat orthographic top-down view, zero perspective distortion, the map fills the entire frame edge to edge.
2. Style: painted digital battlemap art, crisp readable floor tiles, walls, doors and props at a consistent 5 ft square scale.
3. Include lighting and environment detail (torches, glowing runes, water, debris) that reads clearly from above.
4. No empty margins, no title, no captions, no text labels, no UI frame, no border.
5. Output a single square image at the exact pixel dimensions requested by the user.
6. Return only the image — no commentary.`,

  INPAINT: `You are a 2D TTRPG battlemap inpainting engine.
You are given three images of the same battlemap: (1) the current map, (2) the same map with the editable region tinted red, (3) the binary mask of that region (white = edit, black = preserve).

System & Editing Directives:
1. Return ONE image at exactly the same pixel dimensions as the supplied images.
2. Reproduce everything outside the white mask region unchanged — same layout, geometry, lighting and colours.
3. Inside the masked region, replace or add the requested objects (props, hazards, doors, altars, lighting elements).
4. Preserve room geometry, floor tile scale, wall thickness and perspective so the edit aligns with adjacent tiles.
5. Match the colour balance and lighting intensity of the surrounding tiles.
6. The red tint in image 2 is a guide only — it must never appear in the output.
7. Return only the image — no commentary.`,

  OUTPAINT: `You are a 2D TTRPG battlemap canvas extension engine.
You are given one battlemap frame containing a flat, empty dark region where the canvas is being extended.

System & Extension Directives:
1. Fill the empty region with terrain that continues seamlessly from the existing map: same palette, tile scale, wall thickness and lighting direction.
2. Extend hallways, rooms, or outdoor terrain logically in the requested direction.
3. Keep every pixel of the existing map unchanged.
4. Blend the seam softly — no hard cut line along the boundary.
5. Output ONE image at exactly the pixel dimensions requested by the user — only the image, no commentary.`
};

/**
 * Builds standard prompt formatting optimized for TTRPG Top-Down Battlemaps
 */
function buildBattlemapPrompt(userPrompt) {
  return `Details: ${userPrompt || 'A detailed battle map with rooms, corridors, lighting, and environmental hazards'}.`;
}

/**
 * Short user-side instruction for an inpaint call; the mask images carry the "what/where" detail.
 */
function buildInpaintPrompt(userPrompt) {
  const trimmed = typeof userPrompt === 'string' ? userPrompt.trim() : '';
  return trimmed || 'Add natural, style-matched detail inside the masked region';
}

/**
 * User-side instruction for an outpaint call. Image models pick their own output resolution, so the
 * target pixel size is stated explicitly — the stitch back onto the original canvas needs exact dimensions.
 */
function buildOutpaintPrompt({ direction, extensionPrompt, width, height }) {
  const base = `Extend this top-down TTRPG battlemap towards the ${direction.toUpperCase()}. Extended area features: ${extensionPrompt || 'continue room/corridor layout with matching floor tiles and walls'}`;
  return `${base}. Output canvas: ${width}x${height} pixels.`;
}

module.exports = {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildInpaintPrompt,
  buildOutpaintPrompt
};
