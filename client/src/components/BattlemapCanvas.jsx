import React, { useRef, useState, useEffect } from 'react';
import { Stage, Layer, Image as KonvaImage, Line, Rect, Circle, Text, Group } from 'react-konva';
import { ZoomIn, ZoomOut, Maximize, Move, Clock, Sparkles, Paintbrush, Plus } from 'lucide-react';
import { formatTimer } from '../utils/format';

/** Mask brush radius in map pixels — the only edit-brush control left, now that the sidebar is gone. */
const MASK_BRUSH_RADIUS = 30;

export default function BattlemapCanvas({
  mapImage,
  inpaintMaskLines,
  setInpaintMaskLines,
  onMaskPainted,
  onRequestOutpaint,
  // Timer & Shimmer Overlay Props
  isGenerating,
  isEditing,
  isExpanding,
  outpaintDirection,
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

  // Mouse Down — middle-click / Shift pans, right button paints an edit mask
  const handleMouseDown = (e) => {
    if (e.evt.button === 1 || e.evt.shiftKey) {
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
  const handleMouseUp = ({ openPrompt = true } = {}) => {
    const drew = isDrawing && strokeDrewMask.current;
    strokeDrewMask.current = false;
    setIsDrawing(false);
    setIsPanning(false);
    if (drew && openPrompt) onMaskPainted();
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
      const expandPx = 512;
      switch (outpaintDirection) {
        case 'north':
          targetY = -expandPx;
          targetH = expandPx;
          break;
        case 'south':
          targetY = mapHeight;
          targetH = expandPx;
          break;
        case 'west':
          targetX = -expandPx;
          targetW = expandPx;
          break;
        case 'east':
          targetX = mapWidth;
          targetW = expandPx;
          break;
        case 'all':
          targetX = -expandPx;
          targetY = -expandPx;
          targetW = mapWidth + expandPx * 2;
          targetH = mapHeight + expandPx * 2;
          break;
        default:
          targetX = mapWidth;
          targetW = expandPx;
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

  const cursor = isPanning ? 'grabbing' : isDrawing ? 'crosshair' : 'default';

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
        <span>Shift + Drag or Middle Click to Pan · Scroll to Zoom</span>
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

        {/* OUTPAINT EXPANSION HOTSPOTS */}
        {loadedImage && !isBusy && (
          <Layer>
            <Hotspot x={loadedImage.width / 2} y={-44} onClick={() => onRequestOutpaint('north')} />
            <Hotspot x={loadedImage.width / 2} y={loadedImage.height + 44} onClick={() => onRequestOutpaint('south')} />
            <Hotspot x={loadedImage.width + 44} y={loadedImage.height / 2} onClick={() => onRequestOutpaint('east')} />
            <Hotspot x={-44} y={loadedImage.height / 2} onClick={() => onRequestOutpaint('west')} />
          </Layer>
        )}
      </Stage>
    </main>
  );
}

/** Round + hotspot at a canvas edge: clicking it opens the expansion prompt for that direction. */
function Hotspot({ x, y, onClick }) {
  const [hover, setHover] = React.useState(false);
  return (
    <Group
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
