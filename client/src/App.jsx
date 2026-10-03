import React, { useState, useEffect } from 'react';
import confetti from 'canvas-confetti';
import { AlertTriangle, X, Terminal, Paintbrush, Expand } from 'lucide-react';
import Navbar from './components/Navbar';
import PromptHero from './components/PromptHero';
import BattlemapCanvas from './components/BattlemapCanvas';
import ActionModal from './components/ActionModal';
import DebugModal from './components/DebugModal';
import {
  checkServerHealth,
  fetchModelsApi,
  generateBattlemapApi,
  inpaintBattlemapApi,
  outpaintBattlemapApi
} from './services/api';
import { describeSelection, selectionToOutpaintParams } from './utils/outpaintSelection';

const PROVIDER_LABELS = { openrouter: 'OpenRouter' };

// Shown in the inspector when a request failed before any prompt payload existed (missing key,
// backend unreachable) — never a fabricated prompt.
const NO_TELEMETRY_PROMPT = 'Not captured — the server returned no prompt telemetry for this request.';

export default function App() {
  const [isDebugModalOpen, setIsDebugModalOpen] = useState(false);

  // Debug Log History State
  const [debugLogs, setDebugLogs] = useState([]);

  const [mapTitle, setMapTitle] = useState('Dungeon Hallways Map');

  // Selected Image Gen Model — defaults to Nano Banana 2 Flash, persisted in localStorage
  const [selectedModel, setSelectedModel] = useState(() => {
    try {
      return localStorage.getItem('dungeon_forge_selected_model') || 'google/gemini-3.1-flash-image';
    } catch {
      return 'google/gemini-3.1-flash-image';
    }
  });
  const [availableModels, setAvailableModels] = useState([]);

  useEffect(() => {
    fetchModelsApi().then((data) => {
      if (data?.models?.length) {
        setAvailableModels(data.models);
      }
    });
  }, []);

  const handleSelectModel = (modelId) => {
    setSelectedModel(modelId);
    try {
      localStorage.setItem('dungeon_forge_selected_model', modelId);
    } catch {
      // ignore
    }
  };
  // Map & Canvas State
  const [mapImage, setMapImage] = useState(null);

  // AI Generator State
  const [prompt, setPrompt] = useState('Flooded stone dungeon room with ancient pillars, glowing torches, and a central boss altar');
  const [isGenerating, setIsGenerating] = useState(false);

  // Inpaint Detail Edit State — the mask is painted straight onto the canvas with a right-drag, and a
  // finished stroke opens the prompt modal.
  const [inpaintMaskLines, setInpaintMaskLines] = useState([]);
  const [inpaintPrompt, setInpaintPrompt] = useState('');
  const [isInpaintModalOpen, setIsInpaintModalOpen] = useState(false);
  const [isEditing, setIsEditing] = useState(false);

  // Outpaint Canvas Extension State — a clicked edge hotspot creates a resizable selection (the strip
  // of new canvas), and the prompt popover floats next to it
  const [outpaintSelection, setOutpaintSelection] = useState(null);
  const [outpaintPrompt, setOutpaintPrompt] = useState('Continue hallway into a dark cavern chamber');
  const [isExpanding, setIsExpanding] = useState(false);

  // Generation Timer & Target Region Overlay State
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [generationPhaseText, setGenerationPhaseText] = useState('');

  // History Stack for Undo / Redo
  const [history, setHistory] = useState([]);
  const [historyIndex, setHistoryIndex] = useState(-1);

  // Live Timer Ticker effect when generation/edit/expand is active
  const isBusy = isGenerating || isEditing || isExpanding;

  useEffect(() => {
    let interval = null;
    if (isBusy) {
      setElapsedSeconds(0);
      interval = setInterval(() => {
        setElapsedSeconds((prev) => prev + 1);
      }, 1000);
    } else {
      setElapsedSeconds(0);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [isBusy]);

  // The selection is in the current map's pixel space, so it dies with the map (undo/redo, new map)
  useEffect(() => {
    setOutpaintSelection(null);
  }, [mapImage]);

  // Loud, dismissible failure report (generation / inpaint / outpaint)
  const [failure, setFailure] = useState(null);

  // Warn loudly on load when the server has no provider key: every request would fail anyway
  useEffect(() => {
    checkServerHealth().then((health) => {
      if (health && health.providers && !health.providers.openrouter) {
        setFailure((prev) => prev || {
          type: 'GENERATE',
          code: 'NO_API_KEY',
          title: 'No model provider key configured',
          message: 'server/.env has no OPENROUTER_API_KEY (and none is in the server environment), so every generation, edit and expansion will fail. Add the key and restart the server.',
          attempts: []
        });
      }
    });
  }, []);

  // Human-readable names for the failure banner headline
  const FAILURE_LABELS = {
    GENERATE: 'Map generation',
    INPAINT: 'Detail edit',
    OUTPAINT: 'Canvas expansion'
  };

  // Push new telemetry entry to debug logs
  const pushDebugLog = (type, userPrompt, resData, startTimeMs) => {
    const durationMs = Date.now() - startTimeMs;
    const telemetry = resData.debug || {};
    const sysPrompt = telemetry.systemInstruction || NO_TELEMETRY_PROMPT;
    const usrPrompt = telemetry.userPrompt || userPrompt || 'Standard Battlemap Request';
    const fullPayload = telemetry.fullPrompt || NO_TELEMETRY_PROMPT;

    setDebugLogs((prev) => [{
      id: Date.now(),
      type,
      status: 'ok',
      prompt: usrPrompt,
      systemInstruction: sysPrompt,
      userPrompt: usrPrompt,
      fullPrompt: fullPayload,
      images: telemetry.images || [],
      rawResponseText: telemetry.rawResponseText || '',
      modelUsed: telemetry.modelUsed || telemetry.model || 'Unknown (no telemetry received)',
      provider: telemetry.provider || null,
      usage: telemetry.usage || null,
      failureReason: telemetry.failureReason || null,
      attempts: telemetry.attempts || [],
      durationMs,
      timestamp: new Date().toISOString()
    }, ...prev]);
  };

  // Records a failed model attempt in the debug logs and raises the loud on-screen error banner
  const reportFailure = (type, userPrompt, err, startTimeMs) => {
    const telemetry = err.debug || {};
    const attempts = telemetry.attempts || [];
    const lastAttempt = attempts.length ? attempts[attempts.length - 1] : null;
    const usrPrompt = telemetry.userPrompt || userPrompt || 'Standard Battlemap Request';
    const message = err.message === 'Failed to fetch'
      ? 'Could not reach the backend API (is `npm run dev` running? The server must be up on port 3001).'
      : err.message;
    const providerLabel = PROVIDER_LABELS[telemetry.provider] || 'Model provider';

    setDebugLogs((prev) => [{
      id: Date.now(),
      type,
      status: 'failed',
      prompt: usrPrompt,
      systemInstruction: telemetry.systemInstruction || NO_TELEMETRY_PROMPT,
      userPrompt: usrPrompt,
      fullPrompt: telemetry.fullPrompt || NO_TELEMETRY_PROMPT,
      images: telemetry.images || [],
      rawResponseText: telemetry.rawResponseText || '',
      modelUsed: lastAttempt ? lastAttempt.model : (telemetry.model || 'no model reached'),
      provider: telemetry.provider || null,
      usage: telemetry.usage || null,
      failureReason: message,
      attempts,
      durationMs: Date.now() - startTimeMs,
      timestamp: new Date().toISOString()
    }, ...prev]);

    setFailure({
      type,
      code: err.code || 'REQUEST_FAILED',
      title: err.code === 'NO_API_KEY'
        ? `${providerLabel} API key not configured`
        : `${FAILURE_LABELS[type] || 'Map request'} failed`,
      message,
      attempts,
      provider: telemetry.provider || null,
      model: telemetry.model || null
    });
  };

  // Save current state to history
  const pushHistoryState = (newMapImage) => {
    let baseHistory = history;
    let baseIndex = historyIndex;
    // Defensive check: if history was empty but a base map exists, seed it
    if (baseHistory.length === 0 && mapImage && mapImage !== newMapImage) {
      baseHistory = [{ mapImage }];
      baseIndex = 0;
    }
    const newHistory = baseHistory.slice(0, baseIndex + 1);
    newHistory.push({ mapImage: newMapImage });
    setHistory(newHistory);
    setHistoryIndex(newHistory.length - 1);
  };

  const handleUndo = () => {
    if (isBusy || historyIndex <= 0) return;
    setMapImage(history[historyIndex - 1].mapImage);
    setHistoryIndex(historyIndex - 1);
    setInpaintMaskLines([]);
    setIsInpaintModalOpen(false);
  };

  const handleRedo = () => {
    if (isBusy || historyIndex >= history.length - 1) return;
    setMapImage(history[historyIndex + 1].mapImage);
    setHistoryIndex(historyIndex + 1);
    setInpaintMaskLines([]);
    setIsInpaintModalOpen(false);
  };

  // Convert the painted mask strokes to the binary mask the edit call takes. The strokes are in map
  // pixels, so the mask must be exactly the map's size — an expanded map is no longer 1024x1024, and a
  // fixed-size mask would be stretched onto the wrong region (and clip strokes past 1024).
  const getInpaintMaskDataUrl = async () => {
    if (inpaintMaskLines.length === 0 || !mapImage) return null;
    const { width, height } = await new Promise((resolve, reject) => {
      const img = new window.Image();
      img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => reject(new Error('Could not read the map size for the edit mask'));
      img.src = mapImage;
    });
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    ctx.strokeStyle = '#ffffff';
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    inpaintMaskLines.forEach((line) => {
      ctx.lineWidth = line.radius * 2;
      ctx.beginPath();
      const pts = line.points;
      ctx.moveTo(pts[0], pts[1]);
      for (let i = 2; i < pts.length; i += 2) {
        ctx.lineTo(pts[i], pts[i + 1]);
      }
      ctx.stroke();
    });

    return canvas.toDataURL('image/png');
  };

  // Handle Initial Map Generation
  const handleGenerateMap = async () => {
    const startTimeMs = Date.now();
    setIsGenerating(true);
    setGenerationPhaseText('RENDERING MAP...');
    try {
      const res = await generateBattlemapApi({
        prompt,
        width: 1024,
        height: 1024,
        model: selectedModel
      });

      setMapImage(res.imageDataUrl);
      pushHistoryState(res.imageDataUrl);
      pushDebugLog('GENERATE', prompt, res, startTimeMs);
      confetti({ particleCount: 55, spread: 65, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('GENERATE', prompt, err, startTimeMs);
    } finally {
      setIsGenerating(false);
      setGenerationPhaseText('');
    }
  };

  // Handle Detail Inpainting Edit
  const handleApplyInpaint = async () => {
    const maskDataUrl = await getInpaintMaskDataUrl();
    if (!mapImage || !maskDataUrl) {
      setIsInpaintModalOpen(false);
      setInpaintMaskLines([]);
      return;
    }

    const startTimeMs = Date.now();
    setIsEditing(true);
    setGenerationPhaseText('EDITING MASK REGION...');
    try {
      const res = await inpaintBattlemapApi({
        originalDataUrl: mapImage,
        maskDataUrl,
        prompt: inpaintPrompt,
        model: selectedModel
      });

      setMapImage(res.imageDataUrl);
      setInpaintPrompt('');
      pushHistoryState(res.imageDataUrl);
      pushDebugLog('INPAINT', inpaintPrompt, res, startTimeMs);
      confetti({ particleCount: 40, spread: 50, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('INPAINT', inpaintPrompt, err, startTimeMs);
    } finally {
      setIsEditing(false);
      setGenerationPhaseText('');
      setIsInpaintModalOpen(false);
      setInpaintMaskLines([]);
    }
  };

  // Handle Canvas Outpainting Extension
  const handleApplyOutpaint = async () => {
    const selection = outpaintSelection;
    if (!mapImage || !selection) return;
    const { direction, expandAmount, spanStart, spanEnd } = selectionToOutpaintParams(selection);
    const label = `${direction.toUpperCase()} ${selection.width}×${selection.height}`;

    const startTimeMs = Date.now();
    setIsExpanding(true);
    setGenerationPhaseText(`EXPANDING MAP ${direction.toUpperCase()}...`);

    try {
      const res = await outpaintBattlemapApi({
        originalDataUrl: mapImage,
        direction,
        extensionPrompt: outpaintPrompt,
        expandAmount,
        spanStart,
        spanEnd,
        model: selectedModel
      });

      setMapImage(res.imageDataUrl);
      pushHistoryState(res.imageDataUrl);
      pushDebugLog('OUTPAINT', `${label}: ${outpaintPrompt}`, res, startTimeMs);
      confetti({ particleCount: 60, spread: 70, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('OUTPAINT', `${label}: ${outpaintPrompt}`, err, startTimeMs);
    } finally {
      setIsExpanding(false);
      setGenerationPhaseText('');
      setOutpaintSelection(null);
    }
  };

  // Export Map to PNG
  const handleExportPng = () => {
    if (!mapImage) return;
    const link = document.createElement('a');
    link.download = `${mapTitle.toLowerCase().replace(/\s+/g, '-')}-${Date.now()}.png`;
    link.href = mapImage;
    link.click();
  };

  // Export Project JSON State
  const handleExportJson = () => {
    const projectData = {
      title: mapTitle,
      mapImage,
      history: history.length > 0 ? history : (mapImage ? [{ mapImage }] : []),
      historyIndex: history.length > 0 ? historyIndex : (mapImage ? 0 : -1),
      version: '2.0'
    };
    const blob = new Blob([JSON.stringify(projectData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.download = `${mapTitle.toLowerCase().replace(/\s+/g, '-')}-project.json`;
    link.href = url;
    link.click();
  };

  // Import Project JSON State
  const handleImportJson = (data) => {
    if (!data) return;
    if (data.mapImage) {
      setMapImage(data.mapImage);
      const hasValidHistory = Array.isArray(data.history)
        && data.history.length > 0
        && data.history.every((item) => item && typeof item.mapImage === 'string');

      if (hasValidHistory) {
        setHistory(data.history);
        const validIndex = typeof data.historyIndex === 'number'
          && data.historyIndex >= 0
          && data.historyIndex < data.history.length
            ? data.historyIndex
            : data.history.length - 1;
        setHistoryIndex(validIndex);
      } else {
        setHistory([{ mapImage: data.mapImage }]);
        setHistoryIndex(0);
      }
      setInpaintMaskLines([]);
      setInpaintPrompt('');
      setOutpaintSelection(null);
    }
    if (data.title) setMapTitle(data.title);
  };

  // Start over: back to the prompt with no map
  const handleClearCanvas = () => {
    if (window.confirm('Start a new map? This clears the current one.')) {
      setMapImage(null);
      setInpaintMaskLines([]);
      setInpaintPrompt('');
      setHistory([]);
      setHistoryIndex(-1);
      setOutpaintSelection(null);
    }
  };

  return (
    <div className="app-shell flex flex-col h-screen text-slate-100 font-sans overflow-hidden">
      {/* Top Header Navigation */}
      <Navbar
        selectedModel={selectedModel}
        onSelectModel={handleSelectModel}
        models={availableModels}
        onOpenDebugModal={() => setIsDebugModalOpen(true)}
        debugLogCount={debugLogs.length}
        onExportPng={handleExportPng}
        onExportJson={handleExportJson}
        onImportJson={handleImportJson}
        onClearCanvas={handleClearCanvas}
        onUndo={handleUndo}
        onRedo={handleRedo}
        canUndo={historyIndex > 0 && !isBusy}
        canRedo={historyIndex < history.length - 1 && !isBusy}
        mapTitle={mapTitle}
        setMapTitle={setMapTitle}
      />

      {/* Workspace: the prompt while there is no map, the canvas once there is */}
      <div className="flex flex-1 overflow-hidden relative z-10">
        {mapImage ? (
          <BattlemapCanvas
            mapImage={mapImage}
            inpaintMaskLines={inpaintMaskLines}
            setInpaintMaskLines={setInpaintMaskLines}
            onMaskPainted={() => {
              setOutpaintSelection(null);
              setIsInpaintModalOpen(true);
            }}
            outpaintSelection={outpaintSelection}
            onOutpaintSelectionChange={(selection) => {
              // One canvas action at a time: starting an expansion abandons a pending mask edit
              if (selection) {
                setIsInpaintModalOpen(false);
                setInpaintMaskLines([]);
              }
              setOutpaintSelection(selection);
            }}
            renderOutpaintPopover={(anchor) => (
              <ActionModal
                open
                anchor={anchor}
                tone="emerald"
                icon={Expand}
                title="Expand the canvas"
                chip={describeSelection(outpaintSelection)}
                placeholder="e.g. Continue the corridor into a dark cavern chamber..."
                value={outpaintPrompt}
                onChange={setOutpaintPrompt}
                onSubmit={handleApplyOutpaint}
                onClose={() => setOutpaintSelection(null)}
                busy={isExpanding}
                busyLabel="Expanding…"
                confirmLabel="Expand Map"
                allowEmpty
              />
            )}
            // Timer & Shimmer Props
            isGenerating={isGenerating}
            isEditing={isEditing}
            isExpanding={isExpanding}
            elapsedSeconds={elapsedSeconds}
            generationPhaseText={generationPhaseText}
          />
        ) : (
          <PromptHero
            prompt={prompt}
            setPrompt={setPrompt}
            onGenerate={handleGenerateMap}
            isGenerating={isGenerating}
            phaseText={generationPhaseText}
            elapsedSeconds={elapsedSeconds}
          />
        )}

        {/* Loud failure banner — no silent fallback exists, so the cause must be visible */}
        {failure && (
          <div className="animate-rise absolute top-5 left-1/2 -translate-x-1/2 z-40 w-[min(46rem,calc(100%-2rem))] rounded-2xl border border-rose-600/50 bg-rose-950/85 backdrop-blur-xl shadow-2xl shadow-rose-950/60 p-4 select-text">
            <div className="flex items-start gap-3">
              <div className="p-2 rounded-xl bg-rose-500/15 border border-rose-500/40 text-rose-300 shrink-0">
                <AlertTriangle className="w-5 h-5" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="text-sm font-bold text-rose-50">{failure.title}</h3>
                  <span className="text-[10px] font-mono uppercase px-1.5 py-0.5 rounded-md bg-rose-900/70 border border-rose-700/60 text-rose-200">
                    {failure.code}
                  </span>
                  {failure.model && (
                    <span className="text-[10px] font-mono px-1.5 py-0.5 rounded-md bg-rose-900/40 border border-rose-700/50 text-rose-200/90">
                      {failure.provider ? `${PROVIDER_LABELS[failure.provider] || failure.provider} · ` : ''}{failure.model}
                    </span>
                  )}
                </div>
                <p className="text-xs text-rose-100/90 mt-1.5 leading-relaxed whitespace-pre-wrap break-words">
                  {failure.message}
                </p>
                <p className="text-[11px] text-rose-200/55 mt-1.5">
                  Nothing was generated — the configured model was the only map source and no substitute was applied.
                </p>
                {failure.attempts.length > 0 && (
                  <ul className="mt-2 font-mono text-[11px] text-rose-200/80 space-y-0.5">
                    {failure.attempts.map((a, idx) => (
                      <li key={idx}>
                        • {a.model}: {a.outcome}
                        {a.outcome === 'error'
                          ? ` — ${a.error}`
                          : ` (finish=${a.finishReason || 'unknown'}, ${a.responseChars} chars)`}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              <button
                onClick={() => setFailure(null)}
                title="Dismiss"
                className="icon-btn p-1.5 hover:bg-rose-900/60 hover:text-white shrink-0"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="flex items-center gap-2 mt-3 pl-11">
              <button
                onClick={() => setIsDebugModalOpen(true)}
                className="btn btn-ghost px-3 py-1.5 text-[11px]"
              >
                <Terminal className="w-3.5 h-3.5" />
                <span>Inspect Failed Request Logs</span>
              </button>
              <button
                onClick={() => setFailure(null)}
                className="btn px-3 py-1.5 text-[11px] bg-rose-900/60 border border-rose-700/60 text-rose-100 hover:bg-rose-900"
              >
                Dismiss
              </button>
            </div>
          </div>
        )}

        {/* Masked-edit prompt — the mask is already on the canvas when this opens */}
        <ActionModal
          open={isInpaintModalOpen}
          tone="cyan"
          icon={Paintbrush}
          title="Edit the masked region"
          subtitle="The area you masked will be repainted. Everything outside it keeps its exact pixels."
          chip="Mask edit"
          placeholder="e.g. Place a glowing magical altar surrounded by candles..."
          value={inpaintPrompt}
          onChange={setInpaintPrompt}
          onSubmit={handleApplyInpaint}
          onClose={() => {
            setIsInpaintModalOpen(false);
            setInpaintMaskLines([]);
          }}
          busy={isEditing}
          busyLabel="Editing…"
          confirmLabel="Apply Edit"
          hint="Right-click drag to adjust the mask"
        />
      </div>

      {/* Debug Telemetry Modal */}
      <DebugModal
        isOpen={isDebugModalOpen}
        onClose={() => setIsDebugModalOpen(false)}
        debugLogs={debugLogs}
        onClearLogs={() => setDebugLogs([])}
      />
    </div>
  );
}
