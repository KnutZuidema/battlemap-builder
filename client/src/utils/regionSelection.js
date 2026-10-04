/**
 * Unified Region Selection Geometry & Math for edit, expansion, and hybrid region forging.
 *
 * All coordinates are in map pixel space (0,0 is the original map's top-left corner).
 * Negative x/y means expanding north/west; x > mapWidth or y > mapHeight means expanding east/south.
 */

export const REGION_GRID_SIZE = 64;
export const REGION_MIN_SIZE = 64;
export const REGION_MAX_SIZE = 1024;
export const CONTEXT_PADDING = 512;

export const REGION_HANDLES = [
  { id: 'top-left', edges: ['top', 'left'], cursor: 'nwse-resize' },
  { id: 'top', edges: ['top'], cursor: 'ns-resize' },
  { id: 'top-right', edges: ['top', 'right'], cursor: 'nesw-resize' },
  { id: 'right', edges: ['right'], cursor: 'ew-resize' },
  { id: 'bottom-right', edges: ['bottom', 'right'], cursor: 'nwse-resize' },
  { id: 'bottom', edges: ['bottom'], cursor: 'ns-resize' },
  { id: 'bottom-left', edges: ['bottom', 'left'], cursor: 'nesw-resize' },
  { id: 'left', edges: ['left'], cursor: 'ew-resize' },
];

/** Snaps a pixel value to the nearest multiple of gridSize. */
export function snapToGrid(val, gridSize = REGION_GRID_SIZE) {
  return Math.round(val / gridSize) * gridSize;
}

/**
 * Creates and normalizes a region selection box.
 * Ensures width and height are positive multiples of gridSize (>= gridSize, <= REGION_MAX_SIZE).
 * Clamps coordinates so the rectangle borders or overlaps the original map area [0, 0, mapWidth, mapHeight].
 */
export function createRegionSelection(rect, mapWidth, mapHeight, gridSize = REGION_GRID_SIZE) {
  let x = rect.x ?? 0;
  let y = rect.y ?? 0;
  let width = rect.width ?? gridSize;
  let height = rect.height ?? gridSize;

  // Handle negative dimensions from drag in reverse direction
  if (width < 0) {
    x += width;
    width = -width;
  }
  if (height < 0) {
    y += height;
    height = -height;
  }

  // Snap dimensions to grid and enforce min/max
  width = Math.min(REGION_MAX_SIZE, Math.max(gridSize, snapToGrid(width, gridSize)));
  height = Math.min(REGION_MAX_SIZE, Math.max(gridSize, snapToGrid(height, gridSize)));

  // Snap position to grid
  x = snapToGrid(x, gridSize);
  y = snapToGrid(y, gridSize);

  // Clamp position to ensure rectangle borders or overlaps the map:
  // x + width >= 0 and x <= mapWidth
  // y + height >= 0 and y <= mapHeight
  x = Math.max(-width, Math.min(mapWidth, x));
  y = Math.max(-height, Math.min(mapHeight, y));

  return { x, y, width, height };
}

/**
 * Resizes active edges of a region selection towards a pointer position in map coordinates.
 */
export function resizeRegionSelection(selection, edges, pointer, mapWidth, mapHeight, gridSize = REGION_GRID_SIZE) {
  let left = selection.x;
  let right = selection.x + selection.width;
  let top = selection.y;
  let bottom = selection.y + selection.height;

  const px = snapToGrid(pointer.x, gridSize);
  const py = snapToGrid(pointer.y, gridSize);

  const edgeSet = new Set(edges);

  if (edgeSet.has('left')) {
    left = Math.min(right - gridSize, Math.max(right - REGION_MAX_SIZE, px));
  }
  if (edgeSet.has('right')) {
    right = Math.max(left + gridSize, Math.min(left + REGION_MAX_SIZE, px));
  }
  if (edgeSet.has('top')) {
    top = Math.min(bottom - gridSize, Math.max(bottom - REGION_MAX_SIZE, py));
  }
  if (edgeSet.has('bottom')) {
    bottom = Math.max(top + gridSize, Math.min(top + REGION_MAX_SIZE, py));
  }

  return createRegionSelection(
    { x: left, y: top, width: right - left, height: bottom - top },
    mapWidth,
    mapHeight,
    gridSize
  );
}

/**
 * Moves region selection by delta while preserving dimensions and map adjacency.
 */
export function moveRegionSelection(selection, delta, mapWidth, mapHeight, gridSize = REGION_GRID_SIZE) {
  const newX = snapToGrid(selection.x + delta.x, gridSize);
  const newY = snapToGrid(selection.y + delta.y, gridSize);

  return createRegionSelection(
    { x: newX, y: newY, width: selection.width, height: selection.height },
    mapWidth,
    mapHeight,
    gridSize
  );
}

/**
 * Categorizes region as 'Edit', 'Expand', or 'Hybrid' and formats descriptive labels.
 */
export function describeRegion(selection, mapWidth, mapHeight) {
  if (!selection) return null;

  const isInside =
    selection.x >= 0 &&
    selection.y >= 0 &&
    selection.x + selection.width <= mapWidth &&
    selection.y + selection.height <= mapHeight;

  const isOutside =
    selection.x + selection.width <= 0 ||
    selection.x >= mapWidth ||
    selection.y + selection.height <= 0 ||
    selection.y >= mapHeight;

  const type = isInside ? 'Edit' : isOutside ? 'Expand' : 'Hybrid';
  const dimensions = `${selection.width} × ${selection.height} px`;
  const label = `${type} · ${dimensions}`;
  const details = `${dimensions} · ${REGION_GRID_SIZE}px Grid`;

  return {
    type,
    label,
    dimensions,
    details,
  };
}

/**
 * Computes context bounding box in expanded canvas coordinates.
 */
export function computeContextWindow(selection, mapWidth, mapHeight, padding = CONTEXT_PADDING) {
  const minX = Math.min(0, selection.x);
  const minY = Math.min(0, selection.y);
  const maxX = Math.max(mapWidth, selection.x + selection.width);
  const maxY = Math.max(mapHeight, selection.y + selection.height);

  const canvasWidth = maxX - minX;
  const canvasHeight = maxY - minY;

  const selCanvasX = selection.x - minX;
  const selCanvasY = selection.y - minY;

  const contextX = Math.max(0, selCanvasX - padding);
  const contextY = Math.max(0, selCanvasY - padding);
  const contextRight = Math.min(canvasWidth, selCanvasX + selection.width + padding);
  const contextBottom = Math.min(canvasHeight, selCanvasY + selection.height + padding);

  const contextWidth = contextRight - contextX;
  const contextHeight = contextBottom - contextY;

  return {
    canvasWidth,
    canvasHeight,
    mapOffset: { x: -minX, y: -minY },
    contextRect: { x: contextX, y: contextY, width: contextWidth, height: contextHeight },
    selectionOnContext: {
      x: selCanvasX - contextX,
      y: selCanvasY - contextY,
      width: selection.width,
      height: selection.height,
    },
  };
}
