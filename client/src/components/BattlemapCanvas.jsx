import React, { useRef, useState, useEffect } from 'react';
import { Stage, Layer, Image as KonvaImage, Line, Rect, Circle, Text, Group } from 'react-konva';
import { ZoomIn, ZoomOut, Maximize, Move, Clock, Sparkles, Paintbrush, Plus } from 'lucide-react';
import { formatTimer } from '../utils/format';
import { createSelection, resizeSelection, selectionHandles } from '../utils/outpaintSelection';

/** Rough height of the expansion popover, used to leave room for it when focusing a selection. */
const POPOVER_RESERVE_PX = 240;
const FOCUS_PAD_PX = 40;
/** Screen-pixel size of the border hit areas and grips, kept constant while zooming. */
const HANDLE_HIT_PX = 18;

/** Mask brush radius in map pixels — the only edit-brush control left, now that the sidebar is gone. */
const MASK_BRUSH_RADIUS = 30;

export default function BattlemapCanvas({
  mapImage,
  inpaintMaskLines,
  setInpaintMaskLines,
  onMaskPainted,
  // Outpaint selection: the strip of new canvas, resized by dragging its borders
  outpaintSelection,
  onOutpaintSelectionChange,
  renderOutpaintPopover,
  // Timer & Shimmer Overlay Props
  isGenerating,
  isEditing,
  isExpanding,
  elapsedSeconds,
  generationPhaseText
}) {
  const containerRef = useRef(null);
  const stageRef = useRef(null);
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 });
  const [scale, setScale] = useState(1);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = useState(false);
  const [lastPanPos, setLastPanPos] = useState({ x: 0, y: 0 });
  const [isDrawing, setIsDrawing] = useState(false);
  // Whether the stroke in progress actually laid down mask points, so a stray right-click does not
  // pop the edit modal.
  const strokeDrewMask = useRef(false);
  // Edges of the outpaint selection being dragged, and the cursor to show over its handles.
  const [resizeEdges, setResizeEdges] = useState(null);
  const [handleCursor, setHandleCursor] = useState(null);

  // Loaded map image element
  const [loadedImage, setLoadedImage] = useState(null);

  const isBusy = isGenerating || isEditing || isExpanding;

  // Measure container dimensions
  useEffect(() => {
    const updateSize = () => {
      if (containerRef.current) {
        setDimensions({
          width: containerRef.current.offsetWidth,
          height: containerRef.current.offsetHeight
        });
      }
    };
    updateSize();
    window.addEventListener('resize', updateSize);
    return () => window.removeEventListener('resize', updateSize);
  }, []);

  // Load Map Image when `mapImage` dataUrl changes, and fit it to the viewport
  useEffect(() => {
    if (!mapImage) {
      setLoadedImage(null);
      return;
    }
    const img = new window.Image();
    img.crossOrigin = 'Anonymous';
    img.src = mapImage;
    img.onload = () => {
      setLoadedImage(img);
      if (containerRef.current) {
        const cw = containerRef.current.offsetWidth;
        const ch = containerRef.current.offsetHeight;
        const fit = Math.min((cw * 0.86) / img.width, (ch * 0.86) / img.height, 1.6);
        setScale(fit);
        setPosition({
          x: (cw - img.width * fit) / 2,
          y: (ch - img.height * fit) / 2
        });
      }
    };
  }, [mapImage]);

  // Mouse Wheel Zoom
  const handleWheel = (e) => {
    e.evt.preventDefault();
    const scaleBy = 1.1;
    const stage = stageRef.current;
    if (!stage) return;

    const oldScale = stage.scaleX();
    const pointer = stage.getPointerPosition();

    const mousePointTo = {
      x: (pointer.x - stage.x()) / oldScale,
      y: (pointer.y - stage.y()) / oldScale
    };

    const newScale = e.evt.deltaY < 0 ? oldScale * scaleBy : oldScale / scaleBy;
    const clampedScale = Math.max(0.1, Math.min(5, newScale));

    setScale(clampedScale);
    setPosition({
      x: pointer.x - mousePointTo.x * clampedScale,
      y: pointer.y - mousePointTo.y * clampedScale
    });
  };

  const pointerToMap = () => {
    const stage = stageRef.current;
    const pos = stage && stage.getPointerPosition();
    if (!pos) return null;
    return {
      x: (pos.x - position.x) / scale,
      y: (pos.y - position.y) / scale
    };
  };

  // Mouse Down — left / middle button pans, right button paints an edit mask
  const handleMouseDown = (e) => {
    if (e.evt.button === 0 || e.evt.button === 1) {
      // Clicking an expansion hotspot must stay a click, not start a pan.
      if (e.target && e.target.getParent() && e.target.getParent().attrs.name === 'hotspot') return;
      setIsPanning(true);
      setLastPanPos({ x: e.evt.clientX, y: e.evt.clientY });
      return;
    }

    if (e.evt.button !== 2 || !loadedImage || isBusy) return;

    const point = pointerToMap();
    if (!point) return;

    strokeDrewMask.current = true;
    setIsDrawing(true);
    setInpaintMaskLines([
      ...inpaintMaskLines,
      { radius: MASK_BRUSH_RADIUS, points: [point.x, point.y] }
    ]);
  };

  // Mouse Move
  const handleMouseMove = (e) => {
    if (resizeEdges && outpaintSelection && loadedImage) {
      const point = pointerToMap();
      if (point) {
        onOutpaintSelectionChange(resizeSelection(outpaintSelection, resizeEdges.edges, point, loadedImage.width, loadedImage.height));
      }
      return;
    }

    if (isPanning) {
      const dx = e.evt.clientX - lastPanPos.x;
      const dy = e.evt.clientY - lastPanPos.y;
      setPosition((prev) => ({ x: prev.x + dx, y: prev.y + dy }));
      setLastPanPos({ x: e.evt.clientX, y: e.evt.clientY });
      return;
    }

    if (!isDrawing) return;

    const point = pointerToMap();
    if (!point) return;

    const lastLine = inpaintMaskLines[inpaintMaskLines.length - 1];
    if (!lastLine) return;
    const newLines = inpaintMaskLines.slice(0, -1);
    newLines.push({ ...lastLine, points: lastLine.points.concat([point.x, point.y]) });
    setInpaintMaskLines(newLines);
  };

  // Mouse Up — a finished mask stroke hands off to the prompt modal
  // Global pointer listeners while dragging a resize handle so dragging stays active even outside the stage
  useEffect(() => {
    if (!resizeEdges || !outpaintSelection || !loadedImage) return undefined;

    const onPointerMove = (e) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const point = {
        x: (e.clientX - rect.left - position.x) / scale,
        y: (e.clientY - rect.top - position.y) / scale
      };
      onOutpaintSelectionChange(resizeSelection(outpaintSelection, resizeEdges.edges, point, loadedImage.width, loadedImage.height));
    };

    const onPointerUp = () => {
      setResizeEdges(null);
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, [resizeEdges, outpaintSelection, loadedImage, position, scale, onOutpaintSelectionChange]);

  const handleMouseUp = ({ openPrompt = true } = {}) => {
    const drew = isDrawing && strokeDrewMask.current;
    strokeDrewMask.current = false;
    setIsDrawing(false);
    setIsPanning(false);
    setResizeEdges(null);
    if (drew && openPrompt) onMaskPainted();
  };

  // A clicked hotspot starts a full-side selection and makes sure it (and its popover) is in view.
  const handleRequestOutpaint = (direction) => {
    if (!loadedImage) return;
    const selection = createSelection(direction, loadedImage.width, loadedImage.height);
    onOutpaintSelectionChange(selection);
    focusSelection(selection);
  };

  // Pan/zoom out only as far as needed so the selection — and the popover that sits above a north
  // selection or below a south one — is fully visible. Never zooms in, never moves a view that fits.
  const focusSelection = (selection) => {
    if (!loadedImage || !containerRef.current) return;
    const cw = containerRef.current.offsetWidth;
    const ch = containerRef.current.offsetHeight;
    const needsTop = selection.direction === 'north' ? POPOVER_RESERVE_PX : 0;
    const needsBottom = selection.direction === 'south' ? POPOVER_RESERVE_PX : 0;
    const areaLeft = FOCUS_PAD_PX;
    const areaRight = cw - FOCUS_PAD_PX;
    const areaTop = FOCUS_PAD_PX + needsTop;
    const areaBottom = ch - FOCUS_PAD_PX - needsBottom;

    const x0 = Math.min(0, selection.x);
    const y0 = Math.min(0, selection.y);
    const x1 = Math.max(loadedImage.width, selection.x + selection.width);
    const y1 = Math.max(loadedImage.height, selection.y + selection.height);

    const fits = position.x + x0 * scale >= areaLeft
      && position.x + x1 * scale <= areaRight
      && position.y + y0 * scale >= areaTop
      && position.y + y1 * scale <= areaBottom;
    if (fits) return;

    const fit = Math.min((areaRight - areaLeft) / (x1 - x0), (areaBottom - areaTop) / (y1 - y0));
    const nextScale = Math.max(0.1, Math.min(scale, fit));
    setScale(nextScale);
    setPosition({
      x: areaLeft + ((areaRight - areaLeft) - (x1 - x0) * nextScale) / 2 - x0 * nextScale,
      y: areaTop + ((areaBottom - areaTop) - (y1 - y0) * nextScale) / 2 - y0 * nextScale
    });
  };

  // Recenter / fit view
  const handleResetView = () => {
    if (!loadedImage || !containerRef.current) return;
    const cw = containerRef.current.offsetWidth;
    const ch = containerRef.current.offsetHeight;
    const fit = Math.min((cw * 0.86) / loadedImage.width, (ch * 0.86) / loadedImage.height, 1.6);
    setScale(fit);
    setPosition({
      x: (cw - loadedImage.width * fit) / 2,
      y: (ch - loadedImage.height * fit) / 2
    });
  };

  // Render Generating / Outpainting Target Shimmer Region
  const renderShimmerOverlay = () => {
    if (!isBusy || !loadedImage) return null;

    const mapWidth = loadedImage.width;
    const mapHeight = loadedImage.height;

    let targetX = 0;
    let targetY = 0;
    let targetW = mapWidth;
    let targetH = mapHeight;
    let color = '#f5a524'; // Amber for initial gen

    if (isExpanding) {
      color = '#34d399'; // Emerald for outpainting
      if (outpaintSelection) {
        targetX = outpaintSelection.x;
        targetY = outpaintSelection.y;
        targetW = outpaintSelection.width;
        targetH = outpaintSelection.height;
      }
    } else if (isEditing) {
      color = '#22d3ee'; // Cyan for inpainting
    }

    return (
      <Group x={targetX} y={targetY}>
        <Rect
          width={targetW}
          height={targetH}
          fill={color}
          opacity={0.12}
          stroke={color}
          strokeWidth={3}
          dash={[14, 8]}
          cornerRadius={14}
          perfectDrawEnabled={false}
          listening={false}
        />
        <Rect
          width={targetW}
          height={targetH}
          stroke={color}
          strokeWidth={5}
          opacity={0.5}
          cornerRadius={14}
          perfectDrawEnabled={false}
          listening={false}
        />
      </Group>
    );
  };

  const handleHoverCursor = outpaintSelection && !isBusy ? handleCursor : null;
  const cursor = resizeEdges
    ? resizeEdges.cursor
    : isPanning ? 'grabbing' : isDrawing ? 'crosshair' : (handleHoverCursor || 'default');

  // Where the expansion popover goes: next to the selected strip, on a side that never covers the map.
  const popoverSides = {
    north: { sides: ['above'], align: 'center' },
    south: { sides: ['below'], align: 'center' },
    east: { sides: ['above', 'below'], align: 'start' },
    west: { sides: ['above', 'below'], align: 'end' }
  };
  const outpaintPopover = outpaintSelection && loadedImage && renderOutpaintPopover
    ? renderOutpaintPopover({
      rect: {
        left: position.x + outpaintSelection.x * scale,
        top: position.y + outpaintSelection.y * scale,
        width: outpaintSelection.width * scale,
        height: outpaintSelection.height * scale
      },
      bounds: dimensions,
      ...(popoverSides[outpaintSelection.direction] || popoverSides.east)
    })
    : null;

  return (
    <main
      ref={containerRef}
      className="flex-1 h-full relative overflow-hidden canvas-grid-bg select-none"
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* Zoom / Pan Controls Overlay */}
      <div className="absolute top-4 right-4 z-10 flex items-center gap-1 bg-slate-950/70 backdrop-blur-xl p-1.5 rounded-2xl border border-white/[0.08] shadow-xl shadow-black/40">
        <button onClick={() => setScale((s) => Math.max(0.1, s / 1.2))} className="icon-btn p-2" title="Zoom Out">
          <ZoomOut className="w-4 h-4" />
        </button>
        <span className="min-w-[3.25rem] text-center text-xs font-mono font-bold text-amber-300">
          {Math.round(scale * 100)}%
        </span>
        <button onClick={() => setScale((s) => Math.min(5, s * 1.2))} className="icon-btn p-2" title="Zoom In">
          <ZoomIn className="w-4 h-4" />
        </button>
        <span className="w-px h-4 bg-white/10 mx-0.5" />
        <button onClick={handleResetView} disabled={!loadedImage} className="icon-btn p-2" title="Fit to View">
          <Maximize className="w-4 h-4" />
        </button>
      </div>

      {/* FLOATING GENERATION TIMER BADGE OVERLAY */}
      {isBusy && loadedImage && (
        <div className="absolute top-4 left-1/2 -translate-x-1/2 z-30 bg-slate-950/90 backdrop-blur-xl px-5 py-2.5 rounded-2xl border border-amber-500/40 shadow-2xl shadow-amber-500/10 flex items-center gap-3">
          <div className="p-1.5 rounded-xl bg-amber-500/15 text-amber-300">
            <Sparkles className="w-4 h-4 animate-spin-slow" />
          </div>
          <div>
            <div className="text-[11px] font-bold tracking-wider text-amber-300 uppercase">
              {generationPhaseText || 'FORGING MAP…'}
            </div>
            <div className="text-xs text-slate-300 flex items-center gap-1.5 font-mono">
              <Clock className="w-3.5 h-3.5 text-slate-500" />
              <span>Elapsed <strong className="text-amber-200">{formatTimer(elapsedSeconds)}</strong></span>
            </div>
          </div>
        </div>
      )}

      {/* Canvas Interaction Hints */}
      <div className="absolute bottom-4 left-4 z-10 bg-slate-950/70 backdrop-blur-md px-3 py-1.5 rounded-xl border border-white/[0.07] text-[11px] text-slate-400 flex items-center gap-2">
        <Move className="w-3.5 h-3.5 text-amber-400" />
        <span>Drag to Pan · Scroll to Zoom</span>
      </div>

      {!isBusy && (
        <div className="absolute bottom-4 right-4 z-10 flex items-center gap-2">
          <span className="bg-slate-950/70 backdrop-blur-md px-3 py-1.5 rounded-xl border border-cyan-500/25 text-[11px] text-cyan-200 flex items-center gap-2">
            <Paintbrush className="w-3.5 h-3.5" />
            <span>Right-click drag to mask an edit</span>
          </span>
          <span className="bg-slate-950/70 backdrop-blur-md px-3 py-1.5 rounded-xl border border-emerald-500/25 text-[11px] text-emerald-200 flex items-center gap-2">
            <Plus className="w-3.5 h-3.5" />
            <span>Expand</span>
          </span>
        </div>
      )}

      <Stage
        ref={stageRef}
        width={dimensions.width}
        height={dimensions.height}
        scaleX={scale}
        scaleY={scale}
        x={position.x}
        y={position.y}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={() => handleMouseUp()}
        onMouseLeave={() => handleMouseUp({ openPrompt: false })}
        style={{ cursor }}
      >
        {/* BASE MAP LAYER */}
        <Layer>
          {loadedImage && <KonvaImage image={loadedImage} x={0} y={0} />}
        </Layer>

        {/* SHIMMER & TIMER TARGET REGION OVERLAY */}
        <Layer>{renderShimmerOverlay()}</Layer>

        {/* INPAINT SELECTION MASK LAYER */}
        <Layer>
          {inpaintMaskLines.map((line, i) => (
            <Line
              key={i}
              points={line.points}
              stroke="rgba(34, 211, 238, 0.5)"
              strokeWidth={line.radius * 2}
              tension={0.5}
              lineCap="round"
              lineJoin="round"
              listening={false}
            />
          ))}
        </Layer>

        {/* OUTPAINT SELECTION — drag its borders / outer corners to resize the strip */}
        {loadedImage && !isBusy && outpaintSelection && (
          <Layer>
            <OutpaintSelection
              selection={outpaintSelection}
              scale={scale}
              onResizeStart={setResizeEdges}
              onHandleHover={setHandleCursor}
            />
          </Layer>
        )}

        {/* OUTPAINT EXPANSION HOTSPOTS — the active side is already selected, so it has none */}
        {loadedImage && !isBusy && (
          <Layer>
            {outpaintSelection?.direction !== 'north' && (
              <Hotspot x={loadedImage.width / 2} y={-44} onClick={() => handleRequestOutpaint('north')} />
            )}
            {outpaintSelection?.direction !== 'south' && (
              <Hotspot x={loadedImage.width / 2} y={loadedImage.height + 44} onClick={() => handleRequestOutpaint('south')} />
            )}
            {outpaintSelection?.direction !== 'east' && (
              <Hotspot x={loadedImage.width + 44} y={loadedImage.height / 2} onClick={() => handleRequestOutpaint('east')} />
            )}
            {outpaintSelection?.direction !== 'west' && (
              <Hotspot x={-44} y={loadedImage.height / 2} onClick={() => handleRequestOutpaint('west')} />
            )}
          </Layer>
        )}
      </Stage>

      {/* Expansion prompt: floats beside the selected strip and follows it while panning / zooming */}
      {outpaintPopover}
    </main>
  );
}

/**
 * The strip of new canvas the user is about to generate. The edge on the map is fixed; the other
 * borders and the two outer corners are grabbed to set how deep the strip goes and how much of the
 * map's side it covers. Hit areas and grips keep a constant on-screen size while zooming.
 */
function OutpaintSelection({ selection, scale, onResizeStart, onHandleHover }) {
  const { x, y, width, height, direction } = selection;
  const hit = HANDLE_HIT_PX / scale;
  const px = 1 / scale;

  const hitRect = (handle) => {
    if (handle.edges.length === 2) {
      const cx = handle.edges.includes('left') ? x : x + width;
      const cy = handle.edges.includes('top') ? y : y + height;
      return { x: cx - hit * 0.8, y: cy - hit * 0.8, width: hit * 1.6, height: hit * 1.6 };
    }
    switch (handle.edges[0]) {
      case 'left': return { x: x - hit / 2, y, width: hit, height };
      case 'right': return { x: x + width - hit / 2, y, width: hit, height };
      case 'top': return { x, y: y - hit / 2, width, height: hit };
      default: return { x, y: y + height - hit / 2, width, height: hit };
    }
  };

  const grip = (handle) => {
    if (handle.edges.length === 2) {
      const cx = handle.edges.includes('left') ? x : x + width;
      const cy = handle.edges.includes('top') ? y : y + height;
      return { x: cx - 6 * px, y: cy - 6 * px, width: 12 * px, height: 12 * px };
    }
    const edge = handle.edges[0];
    const vertical = edge === 'left' || edge === 'right';
    const w = (vertical ? 6 : 40) * px;
    const h = (vertical ? 40 : 6) * px;
    const cx = edge === 'left' ? x : edge === 'right' ? x + width : x + width / 2;
    const cy = edge === 'top' ? y : edge === 'bottom' ? y + height : y + height / 2;
    return { x: cx - w / 2, y: cy - h / 2, width: w, height: h };
  };

  // Single-edge handles first, so the corner handles sit on top of them where the two meet.
  const handles = selectionHandles(direction).sort((a, b) => a.edges.length - b.edges.length);

  return (
    <Group>
      <Rect
        x={x}
        y={y}
        width={width}
        height={height}
        fill="rgba(52, 211, 153, 0.14)"
        stroke="#34d399"
        strokeWidth={2}
        strokeScaleEnabled={false}
        dash={[10, 6]}
        perfectDrawEnabled={false}
        listening={false}
      />
      <Text
        x={x + 10 * px}
        y={y + 8 * px}
        text={`${Math.round(width)} × ${Math.round(height)} px`}
        fontSize={12 * px}
        fontFamily="monospace"
        fontStyle="bold"
        fill="#a7f3d0"
        listening={false}
      />
      {handles.map((handle) => (
        <Group
          key={handle.id}
          onMouseDown={(e) => {
            if (e.evt.button !== 0) return;
            e.cancelBubble = true;
            onResizeStart(handle);
          }}
          onMouseEnter={() => onHandleHover(handle.cursor)}
          onMouseLeave={() => onHandleHover(null)}
        >
          <Rect {...hitRect(handle)} fill="rgba(0,0,0,0.01)" />
          <Rect
            {...grip(handle)}
            fill="#34d399"
            stroke="rgba(255,255,255,0.85)"
            strokeWidth={1}
            strokeScaleEnabled={false}
            cornerRadius={3 * px}
            perfectDrawEnabled={false}
            listening={false}
          />
        </Group>
      ))}
    </Group>
  );
}

/** Round + hotspot at a canvas edge: clicking it opens the expansion prompt for that direction. */
function Hotspot({ x, y, onClick }) {
  const [hover, setHover] = React.useState(false);
  return (
    <Group
      name="hotspot"
      x={x}
      y={y}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      {/* Konva's attr is `radius`, not `r` — with `r` the circle stays zero-radius: invisible, no hit
          region, and a 0-sized offscreen buffer canvas that throws. perfectDraw off keeps the shadow
          out of that buffer path. */}
      <Circle
        radius={hover ? 26 : 22}
        fill={hover ? 'rgba(16,185,129,0.95)' : 'rgba(5,150,105,0.95)'}
        stroke="rgba(255,255,255,0.7)"
        strokeWidth={1.5}
        shadowColor="#10b981"
        shadowBlur={hover ? 24 : 10}
        perfectDrawEnabled={false}
      />
      <Text text="+" fontSize={26} fill="#ffffff" x={-8} y={-14} fontStyle="bold" listening={false} />
    </Group>
  );
}
