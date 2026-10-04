/**
 * Prompt contract shared by every map provider.
 *
 * `IMAGE_PROMPTS` set covers generation and region forging. A provider never
 * restates these directives; `battlemapService` assembles the payload from them and passes it down.
 */

/**
 * Defined System Prompts for image-output map models (they render pixels, not markup)
 */
const IMAGE_PROMPTS = {
  GENERATE: `You are a flat 2D TTRPG battlemap renderer.
Render ONE battlemap image for a D&D 5e style virtual tabletop.

System & Layout Directives:
1. Camera & Perspective: Strictly flat 2D architectural plan view (horizontal cross-section sliced at eye level), zero perspective distortion, 90-degree perpendicular overhead angle. The map fills the entire frame edge to edge.
2. Walls & Boundaries: Poché cut walls only. Render exclusively the flat horizontal top cut/cross-section of walls with solid stone/masonry/timber fill and clear outline borders. Zero vertical wall faces, zero wall heights, zero 3D sides, zero perspective depth on wall edges.
3. Strict Negative Constraints: Absolutely NO 3D perspective, NO 2.5D, NO isometric tilt, NO oblique camera angle. Do NOT show the side faces of walls or room interiors from an angle. Floors, rooms, corridors, and props must be strictly planar and viewed directly from above. Strictly GRIDLESS: Absolutely NO visible grid lines, NO square grid overlay, NO hex grid, NO coordinate lines, NO tactical measurement markings drawn onto the image. The virtual tabletop will overlay grid lines separately; all floors, terrain, water, and outdoor areas must be completely seamless and free of artificial overlaid grid lines.
4. Spatial Scale & Proportions: The 1024x1024 image represents an 80 ft by 80 ft tactical battlemap area (exactly 16 by 16 virtual tabletop squares). Scale is strictly 64 pixels = 5 feet (1 tabletop square). Architectural Sizing: Standard single doors are ~64 pixels wide (5 ft); standard corridors and hallways are 64 to 128 pixels wide (1 to 2 squares / 5 to 10 ft); rooms are 192 to 512 pixels wide (3 to 8 squares / 15 to 40 ft); props and furniture must be scaled for 5-foot character tokens (beds ~40x75px, tables ~64x128px, chairs ~32x32px, barrels/chests ~32-40px).
5. Style & Reference Conditioning: Painted digital battlemap art matching the flat perspective, wall slicing, and human-scale 5-foot proportions (doors, corridors, and furniture sized for 5ft character tokens) of the reference battlemap image(s), but strictly gridless without drawn grid lines.
6. Lighting & Environment: Include lighting and environment detail (torches, glowing runes, water, debris) that reads clearly from above.
7. No empty margins, no title, no captions, no text labels, no UI frame, no border, no grid lines, no grid overlay.
8. Output a single square image at the exact pixel dimensions requested by the user.
9. Return only the image — no commentary.`,
  FORGE_REGION: `You are a 2D TTRPG battlemap region modification and canvas extension engine.
You are given three images: (1) the context window showing the battlemap and any expansion void, (2) the guide image highlighting the target region with a semi-transparent cyan box, (3) the binary mask (WHITE = target region to forge, BLACK = keep strictly unchanged/preserved).

System & Forge Directives:
1. Camera & Perspective: Strictly flat 2D architectural plan view, zero perspective distortion. Walls must remain flat horizontal cross-sections (poché walls) with zero visible vertical wall faces or 3D side perspective.
2. Spatial Scale & Gridless Constraint: Maintain exact scale of 64 pixels = 5 feet (corridors 64-128px, doors 64px). Strictly GRIDLESS: do not draw any grid lines, coordinate lines, or grid overlays inside the target region.
3. Return ONE image matching the exact pixel dimensions of the input images.
4. Strictly generate terrain INSIDE the white mask area; keep everything outside the white mask unchanged.
5. For unpainted/void areas inside the mask: generate new seamless terrain continuing walls, floor tiles, and geometry from the surrounding context.
6. For existing map areas inside the mask: modify, add, or replace details as requested by the user prompt while matching adjacent lighting, tile scale, and style.
7. The cyan guide in image 2 is for location guidance only and must never appear in the output.
8. Return only the image — no commentary.`
};

/**
 * Builds standard prompt formatting optimized for TTRPG Top-Down Battlemaps
 */
function buildBattlemapPrompt(userPrompt) {
  return `Details: ${userPrompt || 'A detailed battle map with rooms, corridors, lighting, and environmental hazards'}. Perspective: Strict 2D architectural floor plan cut; render flat poché wall cross-sections only, with zero visible vertical wall sides or 3D perspective, scale: 64px = 5ft square, strictly gridless with no visible grid lines.`;
}

/**
 * User-side instruction for unified region forging (edit, expand, or hybrid).
 */
function buildForgeRegionPrompt(userPrompt, selection, contextW, contextH) {
  const trimmed = typeof userPrompt === 'string' ? userPrompt.trim() : '';
  const desc = trimmed || 'Generate natural, style-matched battlemap terrain inside the target region';
  const selInfo = selection ? ` (${selection.width}x${selection.height} px box)` : '';
  return `Target region${selInfo}: ${desc}. Output context frame: ${contextW}x${contextH} pixels, scale: 64px = 5ft, strictly gridless.`;
}

module.exports = {
  IMAGE_PROMPTS,
  buildBattlemapPrompt,
  buildForgeRegionPrompt
};
