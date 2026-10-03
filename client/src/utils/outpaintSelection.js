/**
 * Geometry of the outpaint selection: the strip of new canvas attached to one side of the map.
 *
 * A selection is `{ direction, x, y, width, height }` in map pixels (0,0 = the map's top-left, so the
 * strip of a north/west selection has negative coordinates). The edge touching the map is fixed; the
 * outer edge sets how far the map grows and the two side edges set how much of that side is covered.
 */

export const OUTPAINT_DEFAULT_DEPTH = 512;
export const OUTPAINT_MIN_DEPTH = 64;
export const OUTPAINT_MAX_DEPTH = 1024;
export const OUTPAINT_MIN_SPAN = 64;

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));

/** The edge that sits on the map and therefore cannot be dragged. */
const INNER_EDGE = { east: 'left', west: 'right', north: 'bottom', south: 'top' };

const EDGE_CURSOR = { left: 'ew-resize', right: 'ew-resize', top: 'ns-resize', bottom: 'ns-resize' };

/** A full-side selection, `OUTPAINT_DEFAULT_DEPTH` px deep. */
export function createSelection(direction, mapWidth, mapHeight) {
  const depth = OUTPAINT_DEFAULT_DEPTH;
  switch (direction) {
    case 'north': return { direction, x: 0, y: -depth, width: mapWidth, height: depth };
    case 'south': return { direction, x: 0, y: mapHeight, width: mapWidth, height: depth };
    case 'west': return { direction, x: -depth, y: 0, width: depth, height: mapHeight };
    default: return { direction: 'east', x: mapWidth, y: 0, width: depth, height: mapHeight };
  }
}

/** Draggable handles for a selection: its three free edges and the two outer corners. */
export function selectionHandles(direction) {
  const inner = INNER_EDGE[direction] || 'left';
  const free = ['left', 'right', 'top', 'bottom'].filter((edge) => edge !== inner);
  const handles = free.map((edge) => ({ id: edge, edges: [edge], cursor: EDGE_CURSOR[edge] }));
  const horizontal = free.filter((edge) => edge === 'left' || edge === 'right');
  const vertical = free.filter((edge) => edge === 'top' || edge === 'bottom');
  horizontal.forEach((h) => vertical.forEach((v) => {
    const diagonalNwSe = (h === 'left') === (v === 'top');
    handles.push({ id: `${v}-${h}`, edges: [v, h], cursor: diagonalNwSe ? 'nwse-resize' : 'nesw-resize' });
  }));
  return handles;
}

/** Moves the given edges of `selection` to the pointer (map pixels), within the allowed limits. */
export function resizeSelection(selection, edges, point, mapWidth, mapHeight) {
  const { direction } = selection;
  let left = selection.x;
  let right = selection.x + selection.width;
  let top = selection.y;
  let bottom = selection.y + selection.height;
  const minSpanX = Math.min(OUTPAINT_MIN_SPAN, mapWidth);
  const minSpanY = Math.min(OUTPAINT_MIN_SPAN, mapHeight);
  const px = Math.round(point.x);
  const py = Math.round(point.y);
  const alongX = direction === 'north' || direction === 'south';

  edges.forEach((edge) => {
    switch (edge) {
      case 'left':
        if (direction === 'west') left = clamp(px, -OUTPAINT_MAX_DEPTH, -OUTPAINT_MIN_DEPTH);
        else if (alongX) left = clamp(px, 0, right - minSpanX);
        break;
      case 'right':
        if (direction === 'east') right = clamp(px, mapWidth + OUTPAINT_MIN_DEPTH, mapWidth + OUTPAINT_MAX_DEPTH);
        else if (alongX) right = clamp(px, left + minSpanX, mapWidth);
        break;
      case 'top':
        if (direction === 'north') top = clamp(py, -OUTPAINT_MAX_DEPTH, -OUTPAINT_MIN_DEPTH);
        else if (!alongX) top = clamp(py, 0, bottom - minSpanY);
        break;
      case 'bottom':
        if (direction === 'south') bottom = clamp(py, mapHeight + OUTPAINT_MIN_DEPTH, mapHeight + OUTPAINT_MAX_DEPTH);
        else if (!alongX) bottom = clamp(py, top + minSpanY, mapHeight);
        break;
      default:
    }
  });

  return { direction, x: left, y: top, width: right - left, height: bottom - top };
}

/** The selection as the outpaint API takes it: how deep, and which stretch of the edge. */
export function selectionToOutpaintParams(selection) {
  const alongX = selection.direction === 'north' || selection.direction === 'south';
  return alongX
    ? { direction: selection.direction, expandAmount: selection.height, spanStart: selection.x, spanEnd: selection.x + selection.width }
    : { direction: selection.direction, expandAmount: selection.width, spanStart: selection.y, spanEnd: selection.y + selection.height };
}

/** Short human label, e.g. "east · 512 × 300 px". */
export function describeSelection(selection) {
  return `${selection.direction} · ${selection.width} × ${selection.height} px`;
}
