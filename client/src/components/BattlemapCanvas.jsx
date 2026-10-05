import React, { useRef, useState, useEffect, useCallback } from 'react';
import { Stage, Layer, Image as KonvaImage, Rect, Group, Shape, Text as KonvaText } from 'react-konva';
import { ZoomIn, ZoomOut, Maximize, Move, Clock, Sparkles, Wand2, X, Crop, RefreshCw, Grid, Gamepad2, Trash2 } from 'lucide-react';
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
  onDeleteRegion,
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
  onToggleGrid,
  activeMode = 'edit',
  features = [],
  selectedFeatureId = null,
  onSelectFeature,
  onToggleFeatureState,
  onUpdateFeatureBox
}) {
  const containerRef = useRef(null);
  const stageRef = useRef(null);
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 });
  const [scale, setScale] = useState(1);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = useState(false);
  const [lastPanPos, setLastPanPos] = useState({ x: 0, y: 0 });
  const hasFittedRef = useRef(false);
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
  // Interactive feature dragging & hover states
  const [featureResizeHandle, setFeatureResizeHandle] = useState(null);
  const [isMovingFeature, setIsMovingFeature] = useState(false);
  const featureDragStartRef = useRef(null);
  const featureBoxAtStartRef = useRef(null);
  const [hoveredFeatureId, setHoveredFeatureId] = useState(null);
  const selectedFeature = features.find((f) => f.id === selectedFeatureId);


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
        const cw = containerRef.current.offsetWidth;
        const ch = containerRef.current.offsetHeight;
        setDimensions({
          width: cw,
          height: ch
        });
        if (!hasFittedRef.current && loadedImage && cw > 0 && ch > 0) {
          hasFittedRef.current = true;
          const fit = Math.min((cw * 0.86) / loadedImage.width, (ch * 0.86) / loadedImage.height, 1.6);
          setScale(fit);
          setPosition({
            x: (cw - loadedImage.width * fit) / 2,
            y: (ch - loadedImage.height * fit) / 2
          });
        }
      }
    };
    updateSize();
    window.addEventListener('resize', updateSize);
    return () => window.removeEventListener('resize', updateSize);
  }, [loadedImage]);

  // Load Map Image when `mapImage` dataUrl changes, and fit it to the viewport on initial load
  useEffect(() => {
    if (!mapImage) {
      setLoadedImage(null);
      hasFittedRef.current = false;
      return;
    }
    const img = new window.Image();
    img.crossOrigin = 'Anonymous';
    img.src = mapImage;
    img.onload = () => {
      setLoadedImage(img);
      if (!hasFittedRef.current && containerRef.current) {
        const cw = containerRef.current.offsetWidth;
        const ch = containerRef.current.offsetHeight;
        if (cw > 0 && ch > 0) {
          hasFittedRef.current = true;
          const fit = Math.min((cw * 0.86) / img.width, (ch * 0.86) / img.height, 1.6);
          setScale(fit);
          setPosition({
            x: (cw - img.width * fit) / 2,
            y: (ch - img.height * fit) / 2
          });
        }
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
    // Deselect feature if left-clicking on empty background in Edit mode
    if (e.evt.button === 0 && selectedFeatureId) {
      onSelectFeature?.(null);
    }

    if (e.evt.button === 0 || e.evt.button === 1) {
      setIsPanning(true);
      setLastPanPos({ x: e.evt.clientX, y: e.evt.clientY });
      return;
    }

    if (activeMode === 'play') {
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

    if (activeMode === 'play') {
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

  // Global pointer listeners while dragging handles or moving box/feature
  useEffect(() => {
    const isInteractingWithFeature = (featureResizeHandle || isMovingFeature) && selectedFeature;
    const isInteractingWithRegion = (resizeHandle || isMovingBox) && regionSelection;
    if ((!isInteractingWithFeature && !isInteractingWithRegion) || !loadedImage) return undefined;

    const onPointerMove = (e) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const pointer = {
        x: (e.clientX - rect.left - position.x) / scale,
        y: (e.clientY - rect.top - position.y) / scale
      };

      if (featureResizeHandle && selectedFeature) {
        const next = resizeRegionSelection(
          selectedFeature.box,
          featureResizeHandle.edges,
          pointer,
          loadedImage.width,
          loadedImage.height
        );
        onUpdateFeatureBox?.(selectedFeature.id, next);
      } else if (isMovingFeature && featureDragStartRef.current && featureBoxAtStartRef.current && selectedFeature) {
        const delta = {
          x: pointer.x - featureDragStartRef.current.x,
          y: pointer.y - featureDragStartRef.current.y
        };
        const next = moveRegionSelection(
          featureBoxAtStartRef.current,
          delta,
          loadedImage.width,
          loadedImage.height
        );
        onUpdateFeatureBox?.(selectedFeature.id, next);
      } else if (resizeHandle) {
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

      setFeatureResizeHandle(null);
      setIsMovingFeature(false);
      featureDragStartRef.current = null;
      featureBoxAtStartRef.current = null;
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, [
    resizeHandle,
    isMovingBox,
    regionSelection,
    loadedImage,
    position,
    scale,
    onRegionSelectionChange,
    featureResizeHandle,
    isMovingFeature,
    selectedFeature,
    onUpdateFeatureBox
  ]);

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
      } else if ((e.key === 'Delete' || e.key === 'Backspace') && regionSelection && !isBusy && activeMode === 'edit') {
        e.preventDefault();
        onDeleteRegion?.();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [regionSelection, isBusy, activeMode, onDeleteRegion, onRegionSelectionChange, handleToggleGrid]);

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
  const cursor = (featureResizeHandle ? featureResizeHandle.cursor : null)
    || (isMovingFeature ? 'move' : null)
    || (resizeHandle ? resizeHandle.cursor : null)
    || (isMovingBox ? 'move' : null)
    || (isPanning ? 'grabbing' : null)
    || (isRightDragging ? 'crosshair' : null)
    || hoverCursor
    || 'default';

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

      {!isBusy && activeMode === 'edit' && (
        <div className="absolute bottom-4 right-4 z-10 flex items-center gap-2">
          <span className="bg-slate-950/70 backdrop-blur-md px-3 py-1.5 rounded-xl border border-cyan-500/25 text-[11px] text-cyan-200 flex items-center gap-2">
            <Crop className="w-3.5 h-3.5" />
            <span>Right-drag to select region · Drag box/handles to adjust</span>
          </span>
        </div>
      )}
      {!isBusy && activeMode === 'play' && (
        <div className="absolute bottom-4 right-4 z-10 flex items-center gap-2">
          <span className="bg-slate-950/70 backdrop-blur-md px-3 py-1.5 rounded-xl border border-amber-500/25 text-[11px] text-amber-200 flex items-center gap-2">
            <Gamepad2 className="w-3.5 h-3.5" />
            <span>Play Mode · Click interactive elements to trigger</span>
          </span>
        </div>
      )}

      {/* FLOATING ACTION PILL ON SELECTION BOX (Edit mode only) */}
      {regionSelection && loadedImage && !isBusy && activeMode === 'edit' && (
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
            onClick={onDeleteRegion}
            className="flex items-center gap-1.5 px-3 py-1 text-xs rounded-xl font-semibold bg-rose-500/20 hover:bg-rose-500/30 text-rose-300 hover:text-white border border-rose-500/40 shadow-lg shadow-rose-950/50 hover:brightness-110 active:scale-95 transition cursor-pointer"
            title="Delete Region (Del)"
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span>Delete Region</span>
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
      {showRetryPill && activeMode === 'edit' && (
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
        {/* BASE MAP LAYER & ALTERNATE FEATURE PATCHES */}
        <Layer>
          {loadedImage && <KonvaImage image={loadedImage} x={0} y={0} />}
          {features.map((feature) => {
            if (feature.currentState === 1 && feature.states?.[1]?.patchDataUrl) {
              return <FeaturePatchImage key={`patch-${feature.id}`} feature={feature} />;
            }
            return null;
          })}
          {loadedImage && effectiveShowGrid && (
            <GridOverlay width={loadedImage.width} height={loadedImage.height} />
          )}
        </Layer>

        {/* SHIMMER & TIMER TARGET REGION OVERLAY */}
        <Layer>{renderShimmerOverlay()}</Layer>

        {/* REGION SELECTION BOX LAYER (Edit mode only) */}
        {loadedImage && !isBusy && regionSelection && activeMode === 'edit' && (
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

        {/* INTERACTIVE FEATURES LAYER */}
        {loadedImage && (
          <Layer>
            {/* Edit Mode: Dashed boxes, badges, and handles */}
            {activeMode === 'edit' && features.map((feature) => {
              const isSelected = feature.id === selectedFeatureId;
              const typeColor = feature.type === 'door'
                ? '#22d3ee'
                : (feature.type === 'destructible_wall' ? '#f59e0b' : '#34d399');
              const px = 1 / scale;

              return (
                <Group key={`feat-edit-${feature.id}`}>
                  {/* Feature Bounding Box */}
                  <Rect
                    x={feature.box.x}
                    y={feature.box.y}
                    width={feature.box.width}
                    height={feature.box.height}
                    stroke={isSelected ? '#22d3ee' : typeColor}
                    strokeWidth={isSelected ? 2.5 * px : 1.5 * px}
                    strokeScaleEnabled={false}
                    dash={isSelected ? undefined : [6, 4]}
                    fill={isSelected ? 'rgba(34, 211, 238, 0.16)' : 'rgba(6, 182, 212, 0.08)'}
                    perfectDrawEnabled={false}
                    onClick={(e) => {
                      e.cancelBubble = true;
                      onSelectFeature?.(feature.id);
                    }}
                    onMouseEnter={() => setHoverCursor('pointer')}
                    onMouseLeave={() => setHoverCursor(null)}
                  />

                  {/* Feature Tag Badge */}
                  <Group x={feature.box.x} y={Math.max(0, feature.box.y - 20 * px)}>
                    <Rect
                      width={Math.min(feature.box.width, 100 * px)}
                      height={18 * px}
                      fill="rgba(10, 15, 29, 0.9)"
                      stroke={isSelected ? '#22d3ee' : 'rgba(255, 255, 255, 0.2)'}
                      strokeWidth={1 * px}
                      cornerRadius={4 * px}
                      perfectDrawEnabled={false}
                    />
                    <KonvaText
                      x={4 * px}
                      y={3 * px}
                      text={feature.label || feature.type}
                      fontSize={10 * px}
                      fill="#ffffff"
                      fontStyle="bold"
                      width={Math.min(feature.box.width - 8 * px, 92 * px)}
                      ellipsis={true}
                      wrap="none"
                      perfectDrawEnabled={false}
                    />
                  </Group>

                  {/* Resize & move handles for selected feature */}
                  {isSelected && (
                    <SelectedFeatureHandles
                      box={feature.box}
                      scale={scale}
                      onResizeStart={(handle) => setFeatureResizeHandle(handle)}
                      onMoveStart={() => {
                        const point = pointerToMap();
                        if (point) {
                          featureDragStartRef.current = point;
                          featureBoxAtStartRef.current = { ...feature.box };
                          setIsMovingFeature(true);
                        }
                      }}
                      onHoverCursor={setHoverCursor}
                    />
                  )}
                </Group>
              );
            })}

            {/* Play Mode: Immersive hoverable & clickable areas */}
            {activeMode === 'play' && features.map((feature) => {
              const isHovered = hoveredFeatureId === feature.id;
              const px = 1 / scale;

              return (
                <Group key={`feat-play-${feature.id}`}>
                  <Rect
                    x={feature.box.x}
                    y={feature.box.y}
                    width={feature.box.width}
                    height={feature.box.height}
                    fill="rgba(0, 0, 0, 0.001)"
                    stroke={isHovered ? 'rgba(245, 158, 11, 0.9)' : undefined}
                    strokeWidth={isHovered ? 2.5 * px : 0}
                    cornerRadius={4 * px}
                    perfectDrawEnabled={false}
                    onClick={(e) => {
                      e.cancelBubble = true;
                      if (feature.states?.[1]?.patchDataUrl) {
                        onToggleFeatureState?.(feature.id);
                      }
                      onSelectFeature?.(feature.id);
                    }}
                    onMouseEnter={() => {
                      setHoveredFeatureId(feature.id);
                      setHoverCursor('pointer');
                    }}
                    onMouseLeave={() => {
                      if (hoveredFeatureId === feature.id) setHoveredFeatureId(null);
                      setHoverCursor(null);
                    }}
                  />
                </Group>
              );
            })}
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

/**
 * Reactive patch image overlay for alternate feature states
 */
function FeaturePatchImage({ feature }) {
  const [patchImg, setPatchImg] = useState(null);
  const patchUrl = feature.states?.[1]?.patchDataUrl;

  useEffect(() => {
    if (!patchUrl) {
      setPatchImg(null);
      return;
    }
    const img = new window.Image();
    img.crossOrigin = 'Anonymous';
    img.src = patchUrl;
    img.onload = () => setPatchImg(img);
  }, [patchUrl]);

  if (!patchImg) return null;

  return (
    <KonvaImage
      image={patchImg}
      x={feature.box.x}
      y={feature.box.y}
      width={feature.box.width}
      height={feature.box.height}
    />
  );
}

/**
 * 64px Grid-Snapped Selected Feature Handles with interior dragging and 8 resize handles
 */
function SelectedFeatureHandles({ box, scale, onResizeStart, onMoveStart, onHoverCursor }) {
  const { x, y, width, height } = box;
  const hit = 18 / scale;
  const px = 1 / scale;

  return (
    <Group>
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
          gx = cx - 5 * px;
          gy = cy - 5 * px;
          gw = 10 * px;
          gh = 10 * px;
        } else {
          const edge = handle.edges[0];
          if (edge === 'left') {
            hx = x - hit / 2;
            hy = y;
            hw = hit;
            hh = height;
            gx = x - 2.5 * px;
            gy = y + height / 2 - 16 * px;
            gw = 5 * px;
            gh = 32 * px;
          } else if (edge === 'right') {
            hx = x + width - hit / 2;
            hy = y;
            hw = hit;
            hh = height;
            gx = x + width - 2.5 * px;
            gy = y + height / 2 - 16 * px;
            gw = 5 * px;
            gh = 32 * px;
          } else if (edge === 'top') {
            hx = x;
            hy = y - hit / 2;
            hw = width;
            hh = hit;
            gx = x + width / 2 - 16 * px;
            gy = y - 2.5 * px;
            gw = 32 * px;
            gh = 5 * px;
          } else {
            hx = x;
            hy = y + height - hit / 2;
            hw = width;
            hh = hit;
            gx = x + width / 2 - 16 * px;
            gy = y + height - 2.5 * px;
            gw = 32 * px;
            gh = 5 * px;
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
            <Rect
              x={hx}
              y={hy}
              width={hw}
              height={hh}
              fill="rgba(0,0,0,0.001)"
              perfectDrawEnabled={false}
            />
            <Rect
              x={gx}
              y={gy}
              width={gw}
              height={gh}
              fill="#22d3ee"
              stroke="#04222a"
              strokeWidth={1 * px}
              cornerRadius={2 * px}
              perfectDrawEnabled={false}
              listening={false}
            />
          </Group>
        );
      })}
    </Group>
  );
}
