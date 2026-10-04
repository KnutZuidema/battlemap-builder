import React, { useRef, useState, useEffect, useCallback } from 'react';
import { Stage, Layer, Image as KonvaImage, Rect, Group, Shape } from 'react-konva';
import { ZoomIn, ZoomOut, Maximize, Move, Clock, Sparkles, Wand2, X, Crop, RefreshCw, Grid } from 'lucide-react';
import { formatTimer } from '../utils/format';
import {
  REGION_HANDLES,
  REGION_GRID_SIZE,
  createRegionSelection,
  resizeRegionSelection,
  moveRegionSelection,
  describeRegion
} from '../utils/regionSelection';

export default function BattlemapCanvas({
  mapImage,
  regionSelection,
  onRegionSelectionChange,
  onOpenRegionPrompt,
  renderRegionPopover,
  isGenerating,
  isForgingRegion,
  elapsedSeconds,
  generationPhaseText,
  retryContext,
  onRetry,
  isRetryPillDismissed,
  onDismissRetryPill,
  showGrid = true,
  onToggleGrid
}) {
  const containerRef = useRef(null);
  const stageRef = useRef(null);
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 });
  const [scale, setScale] = useState(1);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = useState(false);
  const [lastPanPos, setLastPanPos] = useState({ x: 0, y: 0 });

  // Loaded map image element
  const [loadedImage, setLoadedImage] = useState(null);

  // Active handle being dragged for resizing { id, edges, cursor }
  const [resizeHandle, setResizeHandle] = useState(null);
  // Whether the interior of the box is being dragged
  const [isMovingBox, setIsMovingBox] = useState(false);
  // Pointer starting position in map space for move / right-drag
  const dragStartPointRef = useRef(null);
  const selectionAtDragStartRef = useRef(null);
  // Right-drag selection drafting
  const [isRightDragging, setIsRightDragging] = useState(false);

  // Cursor state
  const [hoverCursor, setHoverCursor] = useState(null);

  // Local grid visibility fallback
  const [localShowGrid, setLocalShowGrid] = useState(true);
  const effectiveShowGrid = typeof showGrid === 'boolean' ? showGrid : localShowGrid;
  const handleToggleGrid = useCallback(() => {
    if (onToggleGrid) {
      onToggleGrid();
    } else {
      setLocalShowGrid((prev) => !prev);
    }
  }, [onToggleGrid]);

  const isBusy = isGenerating || isForgingRegion;

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

  const pointerToMap = useCallback(() => {
    const stage = stageRef.current;
    const pos = stage && stage.getPointerPosition();
    if (!pos) return null;
    return {
      x: (pos.x - position.x) / scale,
      y: (pos.y - position.y) / scale
    };
  }, [position, scale]);

  // Mouse Down — left / middle button pans canvas, right button initiates region selection
  const handleMouseDown = (e) => {
    if (e.evt.button === 0 || e.evt.button === 1) {
      setIsPanning(true);
      setLastPanPos({ x: e.evt.clientX, y: e.evt.clientY });
      return;
    }

    if (e.evt.button === 2 && loadedImage && !isBusy) {
      if (onDismissRetryPill) onDismissRetryPill();
      const point = pointerToMap();
      if (!point) return;

      dragStartPointRef.current = point;
      selectionAtDragStartRef.current = null;
      setIsRightDragging(true);

      const initialDraft = createRegionSelection(
        { x: point.x, y: point.y, width: REGION_GRID_SIZE, height: REGION_GRID_SIZE },
        loadedImage.width,
        loadedImage.height
      );
      onRegionSelectionChange(initialDraft);
    }
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

    if (isRightDragging && loadedImage && dragStartPointRef.current) {
      const point = pointerToMap();
      if (!point) return;

      const start = dragStartPointRef.current;
      const draft = createRegionSelection(
        {
          x: Math.min(start.x, point.x),
          y: Math.min(start.y, point.y),
          width: Math.abs(point.x - start.x),
          height: Math.abs(point.y - start.y)
        },
        loadedImage.width,
        loadedImage.height
      );
      onRegionSelectionChange(draft);
    }
  };

  const handleMouseUp = () => {
    setIsPanning(false);
    if (isRightDragging) {
      setIsRightDragging(false);
      dragStartPointRef.current = null;
    }
  };


  // Global mouseup listener so dragging/panning is cleanly released even outside canvas
  useEffect(() => {
    const onWindowMouseUp = () => {
      setIsPanning(false);
      setIsRightDragging(false);
      dragStartPointRef.current = null;
    };
    window.addEventListener('mouseup', onWindowMouseUp);
    return () => window.removeEventListener('mouseup', onWindowMouseUp);
  }, []);

  // Global pointer listeners while dragging handles or moving box so interactions stay active even outside stage
  useEffect(() => {
    if ((!resizeHandle && !isMovingBox) || !regionSelection || !loadedImage) return undefined;

    const onPointerMove = (e) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const pointer = {
        x: (e.clientX - rect.left - position.x) / scale,
        y: (e.clientY - rect.top - position.y) / scale
      };

      if (resizeHandle) {
        const next = resizeRegionSelection(
          regionSelection,
          resizeHandle.edges,
          pointer,
          loadedImage.width,
          loadedImage.height
        );
        onRegionSelectionChange(next);
      } else if (isMovingBox && dragStartPointRef.current && selectionAtDragStartRef.current) {
        const delta = {
          x: pointer.x - dragStartPointRef.current.x,
          y: pointer.y - dragStartPointRef.current.y
        };
        const next = moveRegionSelection(
          selectionAtDragStartRef.current,
          delta,
          loadedImage.width,
          loadedImage.height
        );
        onRegionSelectionChange(next);
      }
    };

    const onPointerUp = () => {
      setResizeHandle(null);
      setIsMovingBox(false);
      dragStartPointRef.current = null;
      selectionAtDragStartRef.current = null;
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, [resizeHandle, isMovingBox, regionSelection, loadedImage, position, scale, onRegionSelectionChange]);

  // Escape key clears region selection; 'g'/'G' toggles 5ft grid overlay
  useEffect(() => {
    const onKeyDown = (e) => {
      const activeTag = document.activeElement ? document.activeElement.tagName : '';
      const isInput = activeTag === 'INPUT' || activeTag === 'TEXTAREA' || document.activeElement?.isContentEditable;
      if (isInput) return;

      if ((e.key === 'g' || e.key === 'G') && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        handleToggleGrid();
      } else if (e.key === 'Escape' && regionSelection && !isBusy) {
        onRegionSelectionChange(null);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [regionSelection, isBusy, onRegionSelectionChange, handleToggleGrid]);

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

  // Render Generating / Forging Target Shimmer Region
  const renderShimmerOverlay = () => {
    if (!isBusy || !loadedImage) return null;

    const mapWidth = loadedImage.width;
    const mapHeight = loadedImage.height;

    let targetX = 0;
    let targetY = 0;
    let targetW = mapWidth;
    let targetH = mapHeight;
    let color = '#f5a524'; // Amber for initial generation

    const isBaseMap = mapImage === retryContext?.originalDataUrl;
    const activeRetryRegion = isBaseMap ? retryContext?.selection : retryContext?.resultRegion;
    const activeForgeRegion = regionSelection || activeRetryRegion;

    if (isForgingRegion && activeForgeRegion) {
      targetX = activeForgeRegion.x;
      targetY = activeForgeRegion.y;
      targetW = activeForgeRegion.width;
      targetH = activeForgeRegion.height;
      color = '#22d3ee'; // Cyan for region forging
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

  // Determine cursor
  const cursor = resizeHandle
    ? resizeHandle.cursor
    : isMovingBox
    ? 'move'
    : isPanning
    ? 'grabbing'
    : isRightDragging
    ? 'crosshair'
    : hoverCursor || 'default';

  // Description details for badge
  const regionDesc = regionSelection && loadedImage
    ? describeRegion(regionSelection, loadedImage.width, loadedImage.height)
    : null;

  // Screen coordinates for floating action pill and popover
  const screenLeft = regionSelection ? position.x + regionSelection.x * scale : 0;
  const screenTop = regionSelection ? position.y + regionSelection.y * scale : 0;
  const screenWidth = regionSelection ? regionSelection.width * scale : 0;
  const screenHeight = regionSelection ? regionSelection.height * scale : 0;

  const pillLeft = screenLeft + screenWidth / 2;
  const pillIsAbove = screenTop > 65;
  const pillTop = pillIsAbove ? screenTop - 12 : screenTop + screenHeight + 12;

  // Screen coordinates for floating retry pill on last forged region
  const isBaseMap = mapImage === retryContext?.originalDataUrl;
  const activeRetryRegion = isBaseMap ? retryContext?.selection : retryContext?.resultRegion;
  const showRetryPill = !isBusy && !regionSelection && !isRightDragging && !isRetryPillDismissed && loadedImage && retryContext?.type === 'FORGE_REGION' && activeRetryRegion;

  const retryScreenLeft = activeRetryRegion ? position.x + activeRetryRegion.x * scale : 0;
  const retryScreenTop = activeRetryRegion ? position.y + activeRetryRegion.y * scale : 0;
  const retryScreenWidth = activeRetryRegion ? activeRetryRegion.width * scale : 0;
  const retryScreenHeight = activeRetryRegion ? activeRetryRegion.height * scale : 0;

  const retryPillLeft = retryScreenLeft + retryScreenWidth / 2;
  const retryPillIsAbove = retryScreenTop > 65;
  const retryPillTop = retryPillIsAbove ? retryScreenTop - 12 : retryScreenTop + retryScreenHeight + 12;

  const regionPopover = regionSelection && loadedImage && renderRegionPopover
    ? renderRegionPopover({
        rect: {
          left: screenLeft,
          top: screenTop,
          width: screenWidth,
          height: screenHeight
        },
        bounds: dimensions,
        sides: ['above', 'below'],
        align: 'center'
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
        <span className="w-px h-4 bg-white/10 mx-0.5" />
        <button
          onClick={handleToggleGrid}
          className={`icon-btn p-2 transition-colors ${
            effectiveShowGrid
              ? 'text-cyan-300 bg-cyan-500/15 border border-cyan-500/30'
              : 'text-slate-400 hover:text-slate-200'
          }`}
          title="Toggle 5ft Grid (G)"
        >
          <Grid className="w-4 h-4" />
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
            <Crop className="w-3.5 h-3.5" />
            <span>Right-drag to select region · Drag box/handles to adjust</span>
          </span>
        </div>
      )}

      {/* FLOATING ACTION PILL ON SELECTION BOX */}
      {regionSelection && loadedImage && !isBusy && (
        <div
          className="absolute z-20 flex items-center gap-2 bg-slate-950/90 backdrop-blur-xl px-3 py-1.5 rounded-2xl border border-cyan-500/40 shadow-2xl shadow-cyan-500/15 pointer-events-auto transform -translate-x-1/2"
          style={{
            left: pillLeft,
            top: pillTop,
            transform: `translate(-50%, ${pillIsAbove ? '-100%' : '0'})`
          }}
        >
          <span className="text-xs font-mono font-bold text-cyan-300 px-1">
            {regionDesc?.details || `${regionSelection.width} × ${regionSelection.height} px`}
          </span>
          <button
            onClick={onOpenRegionPrompt}
            className="btn-primary flex items-center gap-1.5 px-3 py-1 text-xs rounded-xl font-semibold shadow-lg shadow-cyan-500/25"
          >
            <Wand2 className="w-3.5 h-3.5" />
            <span>Forge Region</span>
          </button>
          <button
            onClick={() => onRegionSelectionChange(null)}
            className="icon-btn p-1 text-slate-400 hover:text-white rounded-lg"
            title="Clear selection (Esc)"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}

      {/* FLOATING RETRY PILL FOR LAST FORGED REGION */}
      {showRetryPill && (
        <div
          className="absolute z-20 flex items-center gap-2 bg-slate-950/90 backdrop-blur-xl px-3 py-1.5 rounded-2xl border border-cyan-500/40 shadow-2xl shadow-cyan-500/15 pointer-events-auto transform -translate-x-1/2 animate-rise"
          style={{
            left: retryPillLeft,
            top: retryPillTop,
            transform: `translate(-50%, ${retryPillIsAbove ? '-100%' : '0'})`
          }}
        >
          <div className="flex items-center gap-1.5 text-xs font-mono font-bold text-cyan-300 px-1">
            <Sparkles className="w-3.5 h-3.5 text-cyan-400" />
            <span>{retryContext.label ? `${retryContext.label} forged` : 'Region forged'}</span>
          </div>
          <button
            onClick={onRetry}
            className="btn-cyan flex items-center gap-1.5 px-3 py-1 text-xs rounded-xl font-semibold shadow-lg shadow-cyan-500/25 hover:brightness-110 active:scale-95 transition"
            title={`Retry generation for this region (${retryContext.prompt || 'seamless terrain'})`}
          >
            <RefreshCw className="w-3.5 h-3.5" />
            <span>Retry</span>
          </button>
          <button
            onClick={onDismissRetryPill}
            className="icon-btn p-1 text-slate-400 hover:text-white rounded-lg"
            title="Dismiss"
          >
            <X className="w-3.5 h-3.5" />
          </button>
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
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        style={{ cursor }}
      >
        {/* BASE MAP LAYER */}
        <Layer>
          {loadedImage && <KonvaImage image={loadedImage} x={0} y={0} />}
          {loadedImage && effectiveShowGrid && (
            <GridOverlay width={loadedImage.width} height={loadedImage.height} />
          )}
        </Layer>

        {/* SHIMMER & TIMER TARGET REGION OVERLAY */}
        <Layer>{renderShimmerOverlay()}</Layer>

        {/* REGION SELECTION BOX LAYER */}
        {loadedImage && !isBusy && regionSelection && (
          <Layer>
            <RegionSelectionBox
              selection={regionSelection}
              scale={scale}
              onResizeStart={(handle) => {
                setResizeHandle(handle);
              }}
              onMoveStart={() => {
                const point = pointerToMap();
                if (point) {
                  dragStartPointRef.current = point;
                  selectionAtDragStartRef.current = { ...regionSelection };
                  setIsMovingBox(true);
                }
              }}
              onHoverCursor={setHoverCursor}
            />
          </Layer>
        )}
      </Stage>

      {/* Floating Prompt Popover when open */}
      {regionPopover}
    </main>
  );
}

/**
 * 64px Grid-Snapped Selection Box with interior dragging and 8 resize handles.
 */
function RegionSelectionBox({
  selection,
  scale,
  onResizeStart,
  onMoveStart,
  onHoverCursor
}) {
  const { x, y, width, height } = selection;
  const hit = 18 / scale;
  const px = 1 / scale;

  return (
    <Group>
      {/* Dashed selection rectangle */}
      <Rect
        x={x}
        y={y}
        width={width}
        height={height}
        fill="rgba(34, 211, 238, 0.12)"
        stroke="#22d3ee"
        strokeWidth={2}
        strokeScaleEnabled={false}
        dash={[10, 6]}
        perfectDrawEnabled={false}
        listening={false}
      />

      {/* Interior move drag area */}
      <Rect
        x={x}
        y={y}
        width={width}
        height={height}
        fill="rgba(0,0,0,0.001)"
        perfectDrawEnabled={false}
        onMouseDown={(e) => {
          if (e.evt.button !== 0) return;
          e.cancelBubble = true;
          onMoveStart();
        }}
        onMouseEnter={() => onHoverCursor('move')}
        onMouseLeave={() => onHoverCursor(null)}
      />

      {/* 8 Resize Handles */}
      {REGION_HANDLES.map((handle) => {
        const isCorner = handle.edges.length === 2;
        let hx, hy, hw, hh;
        let gx, gy, gw, gh;

        if (isCorner) {
          const cx = handle.edges.includes('left') ? x : x + width;
          const cy = handle.edges.includes('top') ? y : y + height;
          hx = cx - hit * 0.8;
          hy = cy - hit * 0.8;
          hw = hit * 1.6;
          hh = hit * 1.6;
          gx = cx - 6 * px;
          gy = cy - 6 * px;
          gw = 12 * px;
          gh = 12 * px;
        } else {
          const edge = handle.edges[0];
          if (edge === 'left') {
            hx = x - hit / 2;
            hy = y;
            hw = hit;
            hh = height;
            gx = x - 3 * px;
            gy = y + height / 2 - 20 * px;
            gw = 6 * px;
            gh = 40 * px;
          } else if (edge === 'right') {
            hx = x + width - hit / 2;
            hy = y;
            hw = hit;
            hh = height;
            gx = x + width - 3 * px;
            gy = y + height / 2 - 20 * px;
            gw = 6 * px;
            gh = 40 * px;
          } else if (edge === 'top') {
            hx = x;
            hy = y - hit / 2;
            hw = width;
            hh = hit;
            gx = x + width / 2 - 20 * px;
            gy = y - 3 * px;
            gw = 40 * px;
            gh = 6 * px;
          } else {
            hx = x;
            hy = y + height - hit / 2;
            hw = width;
            hh = hit;
            gx = x + width / 2 - 20 * px;
            gy = y + height - 3 * px;
            gw = 40 * px;
            gh = 6 * px;
          }
        }

        return (
          <Group
            key={handle.id}
            onMouseDown={(e) => {
              if (e.evt.button !== 0) return;
              e.cancelBubble = true;
              onResizeStart(handle);
            }}
            onMouseEnter={() => onHoverCursor(handle.cursor)}
            onMouseLeave={() => onHoverCursor(null)}
          >
            <Rect x={hx} y={hy} width={hw} height={hh} fill="rgba(0,0,0,0.001)" />
            <Rect
              x={gx}
              y={gy}
              width={gw}
              height={gh}
              fill="#22d3ee"
              stroke="rgba(255,255,255,0.9)"
              strokeWidth={1}
              strokeScaleEnabled={false}
              cornerRadius={isCorner ? 3 * px : 2 * px}
              perfectDrawEnabled={false}
              listening={false}
            />
          </Group>
        );
      })}
    </Group>
  );
}

/**
 * 5ft (64px) non-interactive Grid Overlay rendered via a single Konva Shape draw call.
 */
function GridOverlay({ width, height }) {
  const drawGrid = useCallback((context, shape) => {
    context.beginPath();
    for (let x = 0; x <= width; x += REGION_GRID_SIZE) {
      context.moveTo(x, 0);
      context.lineTo(x, height);
    }
    for (let y = 0; y <= height; y += REGION_GRID_SIZE) {
      context.moveTo(0, y);
      context.lineTo(width, y);
    }
    context.fillStrokeShape(shape);
  }, [width, height]);

  return (
    <Shape
      sceneFunc={drawGrid}
      stroke="rgba(255, 255, 255, 0.15)"
      strokeWidth={1}
      strokeScaleEnabled={false}
      perfectDrawEnabled={false}
      listening={false}
    />
  );
}
