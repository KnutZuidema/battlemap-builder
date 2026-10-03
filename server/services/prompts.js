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
6. The red tint in image 2 is a guide only — it must never appear in the output (unless the request itself asks for something red).
7. The mask only marks where the edit may happen: draw the requested object at a natural 5 ft-grid scale and let the surrounding floor continue around it, rather than stretching it to fill the mask shape.
8. Return only the image — no commentary.`,

  OUTPAINT: `You are a 2D TTRPG battlemap canvas extension engine.
You are given three images: (1) the extended canvas containing the existing map and an empty dark expansion box, (2) the guide image highlighting the expansion region in cyan tint, (3) the binary mask (WHITE = area to generate new map terrain inside, BLACK = keep strictly unchanged/blank).

System & Extension Directives:
1. Return ONE image at exactly the pixel dimensions requested by the user.
2. Fill ONLY the white mask region with top-down battlemap terrain that continues seamlessly from the adjacent map tiles: matching floor tile grid scale, wall thickness, geometry, palette, and lighting direction.
3. Keep everything outside the white mask region black / blank / unchanged. Do NOT generate map terrain outside the white mask region.
4. Align floor grid lines, corridors, walls, and environmental features naturally with the existing map along the open seam.
5. Return only the image — no commentary.`,

  FORGE_REGION: `You are a 2D TTRPG battlemap inpainting and canvas extension engine.
You are given three images: (1) the context window showing the battlemap and any expansion void, (2) the guide image highlighting the target region with a semi-transparent cyan box, (3) the binary mask (WHITE = target region to forge, BLACK = keep strictly unchanged/preserved).

System & Forge Directives:
1. Camera: perfectly flat orthographic top-down 2D view, zero perspective distortion.
2. Return ONE image matching the exact pixel dimensions of the input images.
3. Strictly generate terrain INSIDE the white mask area; keep everything outside the white mask unchanged.
4. For unpainted/void areas inside the mask: generate new seamless terrain continuing walls, floor tiles, and geometry from the surrounding context.
5. For existing map areas inside the mask: modify, add, or replace details as requested by the user prompt while matching adjacent lighting, tile scale, and style.
6. The cyan guide in image 2 is for location guidance only and must never appear in the output.
7. Return only the image — no commentary.`
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
 * User-side instruction for an outpaint call.
 */
function buildOutpaintPrompt({ direction, extensionPrompt, width, height, region }) {
  const base = `Extend this top-down TTRPG battlemap towards the ${direction.toUpperCase()}. Extended area features: ${extensionPrompt || 'continue room/corridor layout with matching floor tiles and walls'}`;
  const boxInfo = region ? ` Paint new map terrain strictly inside the white mask region (${region.width}x${region.height} px box). Do NOT draw map details outside the white mask area.` : '';
  return `${base}.${boxInfo} Output canvas: ${width}x${height} pixels.`;
}
/**
 * User-side instruction for unified region forging (inpaint, outpaint, or hybrid).
 */
function buildForgeRegionPrompt(userPrompt, selection, contextW, contextH) {
  const trimmed = typeof userPrompt === 'string' ? userPrompt.trim() : '';
  const desc = trimmed || 'Generate natural, style-matched battlemap terrain inside the target region';
  const selInfo = selection ? ` (${selection.width}x${selection.height} px box)` : '';
  return `Target region${selInfo}: ${desc}. Output context frame: ${contextW}x${contextH} pixels.`;
}

module.exports = {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildInpaintPrompt,
  buildOutpaintPrompt,
  buildForgeRegionPrompt
};
