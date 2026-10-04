import React, { useState, useEffect, useRef } from 'react';
import confetti from 'canvas-confetti';
import { AlertTriangle, X, Terminal, Wand2, RefreshCw } from 'lucide-react';
import Navbar from './components/Navbar';
import PromptHero from './components/PromptHero';
import BattlemapCanvas from './components/BattlemapCanvas';
import ActionModal from './components/ActionModal';
import DebugModal from './components/DebugModal';
import WikiDrawer from './components/WikiDrawer';
import {
  checkServerHealth,
  fetchModelsApi,
  generateBattlemapApi,
  forgeRegionApi
} from './services/api';
import { describeRegion, getRegionInCanvasCoordinates } from './utils/regionSelection';

const PROVIDER_LABELS = { openrouter: 'OpenRouter' };

const DEFAULT_MAP_COST = {
  totalCost: 0,
  promptTokens: 0,
  completionTokens: 0,
  totalTokens: 0,
  promptCost: 0,
  completionCost: 0,
  requestsCount: 0
};

// Shown in the inspector when a request failed before any prompt payload existed (missing key,
// backend unreachable) — never a fabricated prompt.
const NO_TELEMETRY_PROMPT = 'Not captured — the server returned no prompt telemetry for this request.';

export default function App() {
  const [isDebugModalOpen, setIsDebugModalOpen] = useState(false);
  const [isWikiDrawerOpen, setIsWikiDrawerOpen] = useState(false);
  const [wikiContext, setWikiContext] = useState(null);
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

  // Persistent Map Cost Tracking State (including input and output token costs)
  const [mapCost, setMapCost] = useState(() => {
    try {
      const saved = localStorage.getItem('dungeon_forge_map_cost');
      if (saved) {
        const parsed = JSON.parse(saved);
        if (parsed && typeof parsed.totalCost === 'number') {
          return {
            totalCost: Number(parsed.totalCost) || 0,
            promptTokens: Number(parsed.promptTokens) || 0,
            completionTokens: Number(parsed.completionTokens) || 0,
            totalTokens: Number(parsed.totalTokens) || 0,
            promptCost: Number(parsed.promptCost) || 0,
            completionCost: Number(parsed.completionCost) || 0,
            requestsCount: Number(parsed.requestsCount) || 0
          };
        }
      }
    } catch {
      // ignore
    }
    return DEFAULT_MAP_COST;
  });

  const addMapCost = (usage, isNewMap = false) => {
    if (!usage) return;
    const pTokens = Number(usage.promptTokens) || 0;
    const cTokens = Number(usage.completionTokens) || 0;
    const tTokens = Number(usage.totalTokens) || (pTokens + cTokens);
    const pCost = Number(usage.promptCost) || 0;
    const cCost = Number(usage.completionCost) || 0;
    const callCost = typeof usage.cost === 'number' ? usage.cost : (pCost + cCost);

    setMapCost((prev) => {
      const base = isNewMap ? DEFAULT_MAP_COST : prev;
      const updated = {
        totalCost: (Number(base.totalCost) || 0) + callCost,
        promptTokens: (Number(base.promptTokens) || 0) + pTokens,
        completionTokens: (Number(base.completionTokens) || 0) + cTokens,
        totalTokens: (Number(base.totalTokens) || 0) + tTokens,
        promptCost: (Number(base.promptCost) || 0) + pCost,
        completionCost: (Number(base.completionCost) || 0) + cCost,
        requestsCount: (Number(base.requestsCount) || 0) + 1
      };
      try {
        localStorage.setItem('dungeon_forge_map_cost', JSON.stringify(updated));
      } catch {
        // ignore
      }
      return updated;
    });
  };
  const handleSelectWikiPrompt = ({ title, prompt: newPrompt, usage, articleTitle, articlePath }) => {
    if (newPrompt) setPrompt(newPrompt);
    if (title) setMapTitle(articleTitle ? `${articleTitle}: ${title}` : title);
    if (usage) addMapCost(usage);
    if (articlePath) {
      setWikiContext({ path: articlePath, title: articleTitle || title });
    }
    setIsWikiDrawerOpen(false);
  };
  // Map & Canvas State
  const [mapImage, setMapImage] = useState(null);

  // AI Generator State
  const [prompt, setPrompt] = useState('Flooded stone dungeon room with ancient pillars, glowing torches, and a central boss altar');
  const [isGenerating, setIsGenerating] = useState(false);

  // Unified Region Selection State (64px grid)
  const [regionSelection, setRegionSelection] = useState(null);
  const [regionPrompt, setRegionPrompt] = useState('');
  const [isRegionModalOpen, setIsRegionModalOpen] = useState(false);
  const [isForgingRegion, setIsForgingRegion] = useState(false);

  // Retry Mechanism State for regenerating last map generation or region forge
  const [retryContext, setRetryContext] = useState(null);
  const [isRetrying, setIsRetrying] = useState(false);
  const [isRetryPillDismissed, setIsRetryPillDismissed] = useState(false);

  // Generation Timer & Target Region Overlay State
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [generationPhaseText, setGenerationPhaseText] = useState('');

  // History Stack for Undo / Redo
  const [history, setHistory] = useState([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  // Live Timer Ticker effect when generation/edit/expand is active
  const isBusy = isGenerating || isForgingRegion;

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
    setRegionSelection(null);
    setIsRegionModalOpen(false);
  }, [mapImage]);

  // Loud, dismissible failure report (generation / region forging)
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
    FORGE_REGION: 'Region forge'
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
      outputImages: telemetry.outputImages || [],
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
      outputImages: telemetry.outputImages || [],
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
    setRegionSelection(null);
    setIsRegionModalOpen(false);
  };

  const handleRedo = () => {
    if (isBusy || historyIndex >= history.length - 1) return;
    setMapImage(history[historyIndex + 1].mapImage);
    setHistoryIndex(historyIndex + 1);
    setRegionSelection(null);
    setIsRegionModalOpen(false);
  };
  // Handle Initial Map Generation
  const handleGenerateMap = async () => {
    const startTimeMs = Date.now();
    const currentContext = {
      type: 'GENERATE',
      prompt,
      width: 1024,
      height: 1024,
      model: selectedModel,
      label: 'Initial map'
    };
    setRetryContext(currentContext);
    setIsRetryPillDismissed(true);

    setIsGenerating(true);
    setGenerationPhaseText('RENDERING MAP...');
    setFailure(null);
    try {
      const res = await generateBattlemapApi({
        prompt,
        width: 1024,
        height: 1024,
        model: selectedModel
      });

      setMapImage(res.imageDataUrl);
      pushHistoryState(res.imageDataUrl);
      addMapCost(res.usage || res.debug?.usage, true);
      pushDebugLog('GENERATE', prompt, res, startTimeMs);
      confetti({ particleCount: 55, spread: 65, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('GENERATE', prompt, err, startTimeMs);
    } finally {
      setIsGenerating(false);
      setGenerationPhaseText('');
    }
  };

  // Handle Unified Region Forging
  const handleApplyRegion = async () => {
    if (!mapImage || !regionSelection) return;
    const startTimeMs = Date.now();
    const regionInfo = describeRegion(regionSelection, 1024, 1024);
    const label = regionInfo ? regionInfo.label : 'Region';
    const resultRegion = getRegionInCanvasCoordinates(regionSelection);

    const currentContext = {
      type: 'FORGE_REGION',
      originalDataUrl: mapImage,
      selection: { ...regionSelection },
      prompt: regionPrompt,
      model: selectedModel,
      label,
      resultRegion
    };
    setRetryContext(currentContext);
    setIsRetryPillDismissed(false);

    setIsForgingRegion(true);
    setGenerationPhaseText('FORGING REGION...');
    setFailure(null);

    try {
      const res = await forgeRegionApi({
        originalDataUrl: mapImage,
        selection: regionSelection,
        prompt: regionPrompt,
        model: selectedModel
      });

      setMapImage(res.imageDataUrl);
      setRegionPrompt('');
      pushHistoryState(res.imageDataUrl);
      addMapCost(res.usage || res.debug?.usage, false);
      pushDebugLog('FORGE_REGION', `${label}: ${regionPrompt || 'seamless terrain'}`, res, startTimeMs);
      confetti({ particleCount: 50, spread: 60, origin: { y: 0.8 } });
      setRegionSelection(null);
      setIsRegionModalOpen(false);
      setIsRetryPillDismissed(false);
    } catch (err) {
      reportFailure('FORGE_REGION', `${label}: ${regionPrompt}`, err, startTimeMs);
    } finally {
      setIsForgingRegion(false);
      setGenerationPhaseText('');
    }
  };

  // Retry the last generation or region forge with a simple button click
  const handleRetry = async () => {
    if (!retryContext || isBusy) return;
    const startTimeMs = Date.now();
    setIsRetrying(true);
    setFailure(null);

    if (retryContext.type === 'FORGE_REGION') {
      const { originalDataUrl, selection, prompt: savedPrompt, label } = retryContext;
      setIsForgingRegion(true);
      setGenerationPhaseText('RETRYING REGION FORGE...');

      try {
        const res = await forgeRegionApi({
          originalDataUrl,
          selection,
          prompt: savedPrompt,
          model: selectedModel || retryContext.model
        });

        setMapImage(res.imageDataUrl);
        pushHistoryState(res.imageDataUrl);
        addMapCost(res.usage || res.debug?.usage, false);
        pushDebugLog('FORGE_REGION', `${label} (retry): ${savedPrompt || 'seamless terrain'}`, res, startTimeMs);
        confetti({ particleCount: 50, spread: 60, origin: { y: 0.8 } });
        setRegionSelection(null);
        setIsRegionModalOpen(false);
        setIsRetryPillDismissed(false);
      } catch (err) {
        reportFailure('FORGE_REGION', `${label} (retry): ${savedPrompt}`, err, startTimeMs);
      } finally {
        setIsForgingRegion(false);
        setIsRetrying(false);
        setGenerationPhaseText('');
      }
    } else if (retryContext.type === 'GENERATE') {
      const { prompt: savedPrompt, width, height } = retryContext;
      setIsGenerating(true);
      setGenerationPhaseText('RETRYING MAP GENERATION...');

      try {
        const res = await generateBattlemapApi({
          prompt: savedPrompt,
          width: width || 1024,
          height: height || 1024,
          model: selectedModel || retryContext.model
        });

        setMapImage(res.imageDataUrl);
        pushHistoryState(res.imageDataUrl);
        addMapCost(res.usage || res.debug?.usage, true);
        pushDebugLog('GENERATE', `Retry: ${savedPrompt}`, res, startTimeMs);
        confetti({ particleCount: 55, spread: 65, origin: { y: 0.8 } });
      } catch (err) {
        reportFailure('GENERATE', `Retry: ${savedPrompt}`, err, startTimeMs);
      } finally {
        setIsGenerating(false);
        setIsRetrying(false);
        setGenerationPhaseText('');
      }
    }
  };

  // Keep refs for keydown shortcuts
  const handleRetryRef = useRef(handleRetry);
  const handleUndoRef = useRef(handleUndo);
  const handleRedoRef = useRef(handleRedo);

  useEffect(() => {
    handleRetryRef.current = handleRetry;
    handleUndoRef.current = handleUndo;
    handleRedoRef.current = handleRedo;
  });
  useEffect(() => {
    const handleKeyDown = (e) => {
      const isInput = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (isInput) return;

      // Undo: Ctrl+Z or Cmd+Z (without Shift)
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z' && !e.shiftKey) {
        e.preventDefault();
        handleUndoRef.current?.();
        return;
      }

      // Redo: Ctrl+Shift+Z, Cmd+Shift+Z, or Ctrl+Y / Cmd+Y
      if ((e.metaKey || e.ctrlKey) && ((e.key.toLowerCase() === 'z' && e.shiftKey) || e.key.toLowerCase() === 'y')) {
        e.preventDefault();
        handleRedoRef.current?.();
        return;
      }

      // Retry: Alt+R
      if (e.altKey && e.key.toLowerCase() === 'r') {
        e.preventDefault();
        handleRetryRef.current?.();
        return;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const canRetry = Boolean(retryContext) && !isBusy;
  const retryTooltip = retryContext
    ? retryContext.type === 'FORGE_REGION'
      ? `Retry region forge: "${retryContext.prompt || 'seamless terrain'}" (Alt+R)`
      : `Retry map generation: "${retryContext.prompt}" (Alt+R)`
    : 'No generation to retry';

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
      cost: mapCost,
      totalCost: mapCost.totalCost,
      wikiContext: wikiContext || null,
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
      setRegionSelection(null);
      setRegionPrompt('');
      setIsRegionModalOpen(false);
      setRetryContext(null);
      setIsRetryPillDismissed(false);
    }
    if (data.title) setMapTitle(data.title);
    setWikiContext(data.wikiContext || null);
    // Restore cost from save model
    if (data.cost && typeof data.cost === 'object') {
      const restored = {
        totalCost: Number(data.cost.totalCost ?? data.totalCost ?? 0),
        promptTokens: Number(data.cost.promptTokens ?? 0),
        completionTokens: Number(data.cost.completionTokens ?? 0),
        totalTokens: Number(data.cost.totalTokens ?? 0),
        promptCost: Number(data.cost.promptCost ?? 0),
        completionCost: Number(data.cost.completionCost ?? 0),
        requestsCount: Number(data.cost.requestsCount ?? 1)
      };
      setMapCost(restored);
      try {
        localStorage.setItem('dungeon_forge_map_cost', JSON.stringify(restored));
      } catch {
        // ignore
      }
    } else if (typeof data.totalCost === 'number') {
      const restored = {
        ...DEFAULT_MAP_COST,
        totalCost: data.totalCost,
        requestsCount: 1
      };
      setMapCost(restored);
      try {
        localStorage.setItem('dungeon_forge_map_cost', JSON.stringify(restored));
      } catch {
        // ignore
      }
    }
  };

  // Start over: back to the prompt with no map
  const handleClearCanvas = () => {
    if (window.confirm('Start a new map? This clears the current one.')) {
      setMapImage(null);
      setRegionSelection(null);
      setRegionPrompt('');
      setIsRegionModalOpen(false);
      setHistory([]);
      setHistoryIndex(-1);
      setRetryContext(null);
      setIsRetryPillDismissed(false);
      setMapCost(DEFAULT_MAP_COST);
      setWikiContext(null);
      try {
        localStorage.removeItem('dungeon_forge_map_cost');
      } catch {
        // ignore
      }
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
        onOpenWikiDrawer={() => setIsWikiDrawerOpen(true)}
        onExportPng={handleExportPng}
        onExportJson={handleExportJson}
        onImportJson={handleImportJson}
        onClearCanvas={handleClearCanvas}
        onUndo={handleUndo}
        onRedo={handleRedo}
        canUndo={historyIndex > 0 && !isBusy}
        canRedo={historyIndex < history.length - 1 && !isBusy}
        onRetry={handleRetry}
        canRetry={canRetry}
        isRetrying={isRetrying}
        retryTooltip={retryTooltip}
        mapTitle={mapTitle}
        setMapTitle={setMapTitle}
        mapCost={mapCost}
        wikiContext={wikiContext}
      />
      {/* Workspace: the prompt while there is no map, the canvas once there is */}
      <div className="flex flex-1 overflow-hidden relative z-10">
        {mapImage ? (
          <BattlemapCanvas
            mapImage={mapImage}
            regionSelection={regionSelection}
            onRegionSelectionChange={(sel) => {
              setRegionSelection(sel);
              if (sel) setIsRetryPillDismissed(true);
            }}
            onOpenRegionPrompt={() => setIsRegionModalOpen(true)}
            renderRegionPopover={(anchor) => (
              <ActionModal
                open={isRegionModalOpen}
                anchor={anchor}
                tone="cyan"
                icon={Wand2}
                title="Forge Region"
                subtitle="Modify details or extend map boundaries on a 64px grid."
                chip={regionSelection && describeRegion(regionSelection, 1024, 1024)?.details}
                placeholder="e.g. Continue the corridor into a dark cavern chamber with glowing crystals..."
                value={regionPrompt}
                onChange={setRegionPrompt}
                onSubmit={handleApplyRegion}
                onClose={() => setIsRegionModalOpen(false)}
                busy={isForgingRegion}
                busyLabel="Forging Region…"
                confirmLabel="Forge Region"
                allowEmpty
                wikiContext={wikiContext}
                onOpenWikiDrawer={() => setIsWikiDrawerOpen(true)}
                mapDimensions={{ width: 1024, height: 1024 }}
                mapTitle={mapTitle}
                basePrompt={prompt}
              />
            )}
            // Timer & Shimmer Props
            isGenerating={isGenerating}
            isForgingRegion={isForgingRegion}
            elapsedSeconds={elapsedSeconds}
            generationPhaseText={generationPhaseText}
            // Retry Props
            retryContext={retryContext}
            onRetry={handleRetry}
            isRetryPillDismissed={isRetryPillDismissed}
            onDismissRetryPill={() => setIsRetryPillDismissed(true)}
          />
        ) : (
          <PromptHero
            prompt={prompt}
            setPrompt={setPrompt}
            onGenerate={handleGenerateMap}
            isGenerating={isGenerating}
            phaseText={generationPhaseText}
            elapsedSeconds={elapsedSeconds}
            onOpenWikiDrawer={() => setIsWikiDrawerOpen(true)}
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
              {canRetry && (
                <button
                  onClick={handleRetry}
                  className="btn px-3 py-1.5 text-[11px] bg-cyan-500/20 border border-cyan-500/40 text-cyan-200 hover:bg-cyan-500/30"
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isRetrying ? 'animate-spin' : ''}`} />
                  <span>Retry Request</span>
                </button>
              )}
              <button
                onClick={() => setFailure(null)}
                className="btn px-3 py-1.5 text-[11px] bg-rose-900/60 border border-rose-700/60 text-rose-100 hover:bg-rose-900"
              >
                Dismiss
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Debug Telemetry Modal */}
      <DebugModal
        isOpen={isDebugModalOpen}
        onClose={() => setIsDebugModalOpen(false)}
        debugLogs={debugLogs}
        onClearLogs={() => setDebugLogs([])}
      />

      {/* World Wiki & Battlemap Prompt Drawer */}
      <WikiDrawer
        isOpen={isWikiDrawerOpen}
        onClose={() => setIsWikiDrawerOpen(false)}
        onSelectPrompt={handleSelectWikiPrompt}
        activeWikiContext={wikiContext}
        onSetActiveWikiContext={setWikiContext}
      />
    </div>
  );
}
