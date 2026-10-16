import React, { useState, useEffect, useRef } from 'react';
import confetti from 'canvas-confetti';
import { AlertTriangle, X, Terminal, Wand2, RefreshCw } from 'lucide-react';
import Navbar from './components/Navbar';
import PromptHero from './components/PromptHero';
import BattlemapCanvas from './components/BattlemapCanvas';
import ActionModal from './components/ActionModal';
import DebugModal from './components/DebugModal';
import WikiDrawer from './components/WikiDrawer';
import FeatureInspector from './components/FeatureInspector';
import PlayFeatureCard from './components/PlayFeatureCard';
import MapLibrary from './components/MapLibrary';
import {
  checkServerHealth,
  fetchModelsApi,
  generateBattlemapApi,
  forgeRegionApi,
  deleteRegionApi,
  detectFeaturesApi,
  forgeFeatureStateApi,
  fetchMapByIdApi,
  saveMapApi,
  updateMapApi
} from './services/api';
import { describeRegion, getRegionInCanvasCoordinates, deleteRegionClient } from './utils/regionSelection';

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

  // View Navigation & Map Session Persistence State
  const [currentView, setCurrentView] = useState('studio'); // 'studio' | 'library'
  const [currentMapId, setCurrentMapId] = useState(null);
  const [isSavingMap, setIsSavingMap] = useState(false);
  const [saveStatus, setSaveStatus] = useState('idle'); // 'idle' | 'saving' | 'saved'
  const saveTimeoutRef = useRef(null);

  useEffect(() => {
    return () => {
      clearTimeout(saveTimeoutRef.current);
    };
  }, []);
  const [showGrid, setShowGrid] = useState(true);

  // Selected Image Gen Model — defaults to Nano Banana 2 Flash, persisted in localStorage
  const [selectedModel, setSelectedModel] = useState(() => {
    try {
      return localStorage.getItem('dungeon_forge_selected_model') || 'google/gemini-3.1-flash-image';
    } catch {
      return 'google/gemini-3.1-flash-image';
    }
  });
  // Configurable Forge Region Image Inputs mode — defaults to 'highlighted-only', persisted in localStorage
  const [forgeImageMode, setForgeImageMode] = useState(() => {
    try {
      return localStorage.getItem('dungeon_forge_image_mode') || 'highlighted-only';
    } catch {
      return 'highlighted-only';
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

  const handleSelectForgeImageMode = (mode) => {
    setForgeImageMode(mode);
    try {
      localStorage.setItem('dungeon_forge_image_mode', mode);
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
  const [mapSessionId, setMapSessionId] = useState(0);

  // Interactive Features & Active Mode State
  const [activeMode, setActiveMode] = useState('edit'); // 'edit' | 'play'
  const [features, setFeatures] = useState([]);
  const [selectedFeatureId, setSelectedFeatureId] = useState(null);
  const [isDetectingFeatures, setIsDetectingFeatures] = useState(false);
  const [isForgingFeatureState, setIsForgingFeatureState] = useState(false);

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
    FORGE_REGION: 'Region forge',
    DELETE_REGION: 'Region delete',
    FEATURE_DETECTION: 'Feature detection',
    FORGE_FEATURE_STATE: 'Feature state forge'
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
      imageConfig: telemetry.imageConfig || null,
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
      imageConfig: telemetry.imageConfig || null,
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
  const pushHistoryState = (newMapImage, newFeatures = features) => {
    let baseHistory = history;
    let baseIndex = historyIndex;
    // Defensive check: if history was empty but a base map exists, seed it
    if (baseHistory.length === 0 && mapImage && mapImage !== newMapImage) {
      baseHistory = [{ mapImage, features }];
      baseIndex = 0;
    }
    const newHistory = baseHistory.slice(0, baseIndex + 1);
    newHistory.push({ mapImage: newMapImage, features: newFeatures });
    setHistory(newHistory);
    setHistoryIndex(newHistory.length - 1);
  };

  const handleUndo = () => {
    if (isBusy || historyIndex <= 0) return;
    const prev = history[historyIndex - 1];
    setMapImage(prev.mapImage);
    if (Array.isArray(prev.features)) {
      setFeatures(prev.features);
    }
    setHistoryIndex(historyIndex - 1);
    setRegionSelection(null);
    setIsRegionModalOpen(false);
    setSelectedFeatureId(null);
  };

  const handleRedo = () => {
    if (isBusy || historyIndex >= history.length - 1) return;
    const next = history[historyIndex + 1];
    setMapImage(next.mapImage);
    if (Array.isArray(next.features)) {
      setFeatures(next.features);
    }
    setHistoryIndex(historyIndex + 1);
    setRegionSelection(null);
    setIsRegionModalOpen(false);
    setSelectedFeatureId(null);
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
      setFeatures([]);
      setSelectedFeatureId(null);
      pushHistoryState(res.imageDataUrl, []);
      addMapCost(res.usage || res.debug?.usage, true);
      pushDebugLog('GENERATE', prompt, res, startTimeMs);
      confetti({ particleCount: 55, spread: 65, origin: { y: 0.8 } });

      // Automatically persist newly forged map to disk library
      try {
        const initialCost = res.usage || res.debug?.usage;
        const initialMapCost = initialCost ? {
          totalCost: typeof initialCost.cost === 'number' ? initialCost.cost : ((Number(initialCost.promptCost) || 0) + (Number(initialCost.completionCost) || 0)),
          promptTokens: Number(initialCost.promptTokens) || 0,
          completionTokens: Number(initialCost.completionTokens) || 0,
          totalTokens: Number(initialCost.totalTokens) || 0,
          promptCost: Number(initialCost.promptCost) || 0,
          completionCost: Number(initialCost.completionCost) || 0,
          requestsCount: 1
        } : DEFAULT_MAP_COST;

        const saveRes = await saveMapApi({
          title: mapTitle || 'Dungeon Hallways Map',
          prompt,
          mapImage: res.imageDataUrl,
          features: [],
          cost: initialMapCost,
          wikiContext: wikiContext || null,
          history: [{ mapImage: res.imageDataUrl, features: [] }],
          historyIndex: 0,
          dimensions: { width: 1024, height: 1024 }
        });
        if (saveRes?.map?.id) {
          setCurrentMapId(saveRes.map.id);
        }
      } catch (saveErr) {
        console.warn('Auto-save newly generated battlemap failed:', saveErr);
      }
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
      forgeImageMode,
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
        model: selectedModel,
        forgeImageMode
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

  // Handle Region Deletion
  const handleDeleteRegion = async () => {
    if (!mapImage || !regionSelection) return;
    const startTimeMs = Date.now();
    const regionInfo = describeRegion(regionSelection, 1024, 1024);
    const label = regionInfo ? regionInfo.label : 'Region';

    try {
      let nextDataUrl;
      try {
        const res = await deleteRegionApi({
          originalDataUrl: mapImage,
          selection: regionSelection
        });
        nextDataUrl = res.imageDataUrl;
      } catch (apiErr) {
        console.warn('Backend delete region failed, falling back to client-side canvas:', apiErr.message);
        nextDataUrl = await deleteRegionClient(mapImage, regionSelection);
      }

      // Filter out features whose center is inside the deleted region
      const updatedFeatures = features.filter((f) => {
        if (!f.box) return true;
        const centerX = f.box.x + f.box.width / 2;
        const centerY = f.box.y + f.box.height / 2;
        const isInside = (
          centerX >= regionSelection.x &&
          centerX <= regionSelection.x + regionSelection.width &&
          centerY >= regionSelection.y &&
          centerY <= regionSelection.y + regionSelection.height
        );
        return !isInside;
      });

      if (selectedFeatureId && !updatedFeatures.some((f) => f.id === selectedFeatureId)) {
        setSelectedFeatureId(null);
      }

      setMapImage(nextDataUrl);
      setFeatures(updatedFeatures);
      pushHistoryState(nextDataUrl, updatedFeatures);
      pushDebugLog('DELETE_REGION', `Deleted ${label}`, { debug: { action: 'DELETE_REGION', selection: regionSelection } }, startTimeMs);
      setRegionSelection(null);
      setIsRegionModalOpen(false);
      setIsRetryPillDismissed(true);
    } catch (err) {
      reportFailure('DELETE_REGION', `Delete ${label}`, err, startTimeMs);
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
          model: selectedModel || retryContext.model,
          forgeImageMode: retryContext.forgeImageMode || forgeImageMode
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
  const handleSaveMapRef = useRef(handleSaveMapToDisk);
  const handleToggleViewRef = useRef(() => {
    setCurrentView((prev) => (prev === 'library' ? 'studio' : 'library'));
  });

  useEffect(() => {
    handleRetryRef.current = handleRetry;
    handleUndoRef.current = handleUndo;
    handleRedoRef.current = handleRedo;
    handleSaveMapRef.current = handleSaveMapToDisk;
    handleToggleViewRef.current = () => {
      setCurrentView((prev) => (prev === 'library' ? 'studio' : 'library'));
    };
  });
  useEffect(() => {
    const handleKeyDown = (e) => {
      // Save to disk: Ctrl+S or Cmd+S
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        handleSaveMapRef.current?.();
        return;
      }

      const isInput = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (isInput) return;

      // Toggle Library / Studio view: L
      if (!e.metaKey && !e.ctrlKey && !e.altKey && e.key.toLowerCase() === 'l') {
        e.preventDefault();
        handleToggleViewRef.current?.();
        return;
      }

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

    if (!showGrid) {
      link.href = mapImage;
      link.click();
      return;
    }

    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = img.naturalWidth || img.width;
      canvas.height = img.naturalHeight || img.height;
      const ctx = canvas.getContext('2d');
      if (!ctx) {
        link.href = mapImage;
        link.click();
        return;
      }
      ctx.drawImage(img, 0, 0);

      // Draw 64px grid overlay with subtle lines
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.25)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let x = 0; x <= canvas.width; x += 64) {
        ctx.moveTo(x, 0);
        ctx.lineTo(x, canvas.height);
      }
      for (let y = 0; y <= canvas.height; y += 64) {
        ctx.moveTo(0, y);
        ctx.lineTo(canvas.width, y);
      }
      ctx.stroke();

      link.href = canvas.toDataURL('image/png');
      link.click();
    };
    img.onerror = () => {
      link.href = mapImage;
      link.click();
    };
    img.src = mapImage;
  };

  // Export Project JSON State
  const handleExportJson = () => {
    const projectData = {
      title: mapTitle,
      mapImage,
      features: features || [],
      history: history.length > 0 ? history : (mapImage ? [{ mapImage, features }] : []),
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
      setMapSessionId((prev) => prev + 1);
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
      if (Array.isArray(data.features)) {
        setFeatures(data.features);
      } else {
        setFeatures([]);
      }
      setSelectedFeatureId(null);
      setActiveMode('edit');
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

  // Load saved battlemap from library
  const handleSelectMapFromLibrary = async (mapId, targetMode = 'edit') => {
    try {
      const res = await fetchMapByIdApi(mapId);
      const mapData = res?.map || res;
      if (!mapData || !mapData.mapImage) {
        throw new Error('Invalid map payload received from server');
      }

      setMapImage(mapData.mapImage);
      setFeatures(mapData.features || []);
      setMapTitle(mapData.title || 'Untitled Map');
      setPrompt(mapData.prompt || '');
      setWikiContext(mapData.wikiContext || null);

      if (mapData.cost && typeof mapData.cost === 'object') {
        const restored = {
          totalCost: Number(mapData.cost.totalCost ?? 0),
          promptTokens: Number(mapData.cost.promptTokens ?? 0),
          completionTokens: Number(mapData.cost.completionTokens ?? 0),
          totalTokens: Number(mapData.cost.totalTokens ?? 0),
          promptCost: Number(mapData.cost.promptCost ?? 0),
          completionCost: Number(mapData.cost.completionCost ?? 0),
          requestsCount: Number(mapData.cost.requestsCount ?? 1)
        };
        setMapCost(restored);
        try {
          localStorage.setItem('dungeon_forge_map_cost', JSON.stringify(restored));
        } catch {}
      } else if (typeof mapData.cost === 'number') {
        const restored = {
          ...DEFAULT_MAP_COST,
          totalCost: mapData.cost,
          requestsCount: 1
        };
        setMapCost(restored);
        try {
          localStorage.setItem('dungeon_forge_map_cost', JSON.stringify(restored));
        } catch {}
      } else {
        setMapCost(DEFAULT_MAP_COST);
        try {
          localStorage.removeItem('dungeon_forge_map_cost');
        } catch {}
      }

      if (Array.isArray(mapData.history) && mapData.history.length > 0) {
        setHistory(mapData.history);
        setHistoryIndex(
          typeof mapData.historyIndex === 'number' && mapData.historyIndex >= 0
            ? mapData.historyIndex
            : mapData.history.length - 1
        );
      } else {
        setHistory([{ mapImage: mapData.mapImage, features: mapData.features || [] }]);
        setHistoryIndex(0);
      }

      setCurrentMapId(mapId);
      setActiveMode(targetMode === 'play' ? 'play' : 'edit');
      setCurrentView('studio');
      setMapSessionId((prev) => prev + 1);

      // Reset selection and inspector overlays
      setRegionSelection(null);
      setRegionPrompt('');
      setIsRegionModalOpen(false);
      setSelectedFeatureId(null);
      setRetryContext(null);
      setIsRetryPillDismissed(false);
    } catch (err) {
      console.error('Failed to load map from disk library:', err);
      alert(`Failed to load map: ${err.message || 'Unknown error'}`);
    }
  };

  // Save or update active map to disk library
  const handleSaveMapToDisk = async () => {
    if (!mapImage || isSavingMap) return;

    setIsSavingMap(true);
    setSaveStatus('saving');
    clearTimeout(saveTimeoutRef.current);

    const payload = {
      title: mapTitle || 'Untitled Map',
      prompt,
      mapImage,
      features: features || [],
      cost: mapCost,
      wikiContext: wikiContext || null,
      history: history.length > 0 ? history : [{ mapImage, features }],
      historyIndex: history.length > 0 ? historyIndex : 0,
      dimensions: { width: 1024, height: 1024 }
    };

    try {
      if (currentMapId) {
        await updateMapApi(currentMapId, payload);
      } else {
        const res = await saveMapApi(payload);
        if (res?.map?.id) {
          setCurrentMapId(res.map.id);
        }
      }

      setSaveStatus('saved');
      saveTimeoutRef.current = setTimeout(() => {
        setSaveStatus('idle');
      }, 3000);
    } catch (err) {
      console.error('Failed to save map to disk:', err);
      setSaveStatus('idle');
      alert(`Failed to save map: ${err.message || 'Unknown error'}`);
    } finally {
      setIsSavingMap(false);
    }
  };

  // Create a brand new map: reset canvas, ID, and return to studio edit mode
  const handleCreateNewMap = () => {
    setMapImage(null);
    setCurrentMapId(null);
    setFeatures([]);
    setSelectedFeatureId(null);
    setMapTitle('Dungeon Hallways Map');
    setPrompt('Flooded stone dungeon room with ancient pillars, glowing torches, and a central boss altar');
    setHistory([]);
    setHistoryIndex(-1);
    setRegionSelection(null);
    setRegionPrompt('');
    setIsRegionModalOpen(false);
    setRetryContext(null);
    setIsRetryPillDismissed(false);
    setMapCost(DEFAULT_MAP_COST);
    setWikiContext(null);
    try {
      localStorage.removeItem('dungeon_forge_map_cost');
    } catch {
      // ignore
    }
    setMapSessionId((prev) => prev + 1);
    setCurrentView('studio');
    setActiveMode('edit');
  };

  // Start over: back to the prompt with no map
  const handleClearCanvas = () => {
    if (window.confirm('Start a new map? This clears the current one.')) {
      handleCreateNewMap();
    }
  };

  // Set Edit or Play mode
  const handleSetMode = (mode) => {
    setActiveMode(mode);
    if (mode === 'play') {
      setRegionSelection(null);
      setIsRegionModalOpen(false);
    }
  };

  // Detect interactive features via AI Vision
  const handleDetectFeatures = async () => {
    if (!mapImage || isBusy || isDetectingFeatures) return;
    const startTimeMs = Date.now();
    setIsDetectingFeatures(true);
    setFailure(null);

    try {
      const res = await detectFeaturesApi({
        mapDataUrl: mapImage,
        prompt,
        wikiContext,
        mapDimensions: { width: 1024, height: 1024 }
      });

      const detected = res.features || [];
      setFeatures(detected);
      if (detected.length > 0) {
        setSelectedFeatureId(detected[0].id);
      }
      pushHistoryState(mapImage, detected);
      if (res.usage) {
        addMapCost(res.usage, false);
      }
      pushDebugLog('FEATURE_DETECTION', `Detect features for: "${prompt}"`, res, startTimeMs);
      confetti({ particleCount: 45, spread: 60, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('FEATURE_DETECTION', `Feature detection: "${prompt}"`, err, startTimeMs);
    } finally {
      setIsDetectingFeatures(false);
    }
  };

  // Toggle feature state between default (0) and alternate (1)
  const handleToggleFeatureState = (featureId) => {
    setFeatures((prev) => {
      const next = prev.map((f) => {
        if (f.id === featureId) {
          const nextState = f.currentState === 0 ? 1 : 0;
          return { ...f, currentState: nextState };
        }
        return f;
      });
      pushHistoryState(mapImage, next);
      return next;
    });
  };

  // Forge alternate state for a feature within its bounding box
  const handleForgeFeatureState = async (featureId, customPrompt) => {
    const target = features.find((f) => f.id === featureId);
    if (!target || !mapImage || isForgingFeatureState) return;

    const startTimeMs = Date.now();
    setIsForgingFeatureState(true);
    setFailure(null);

    const forgePromptText = customPrompt || target.suggestedAltPrompt;

    try {
      const res = await forgeFeatureStateApi({
        originalDataUrl: mapImage,
        box: target.box,
        prompt: forgePromptText,
        model: selectedModel,
        forgeImageMode
      });

      const nextFeatures = features.map((f) => {
        if (f.id === featureId) {
          const updatedStates = [...f.states];
          updatedStates[1] = {
            ...updatedStates[1],
            patchDataUrl: res.patchDataUrl
          };
          return {
            ...f,
            states: updatedStates,
            currentState: 1 // Automatically activate newly forged state
          };
        }
        return f;
      });

      setFeatures(nextFeatures);
      pushHistoryState(mapImage, nextFeatures);
      if (res.usage) {
        addMapCost(res.usage, false);
      }
      pushDebugLog('FORGE_FEATURE_STATE', `${target.label}: ${forgePromptText}`, res, startTimeMs);
      confetti({ particleCount: 45, spread: 60, origin: { y: 0.8 } });
    } catch (err) {
      reportFailure('FORGE_FEATURE_STATE', `${target.label}: ${forgePromptText}`, err, startTimeMs);
    } finally {
      setIsForgingFeatureState(false);
    }
  };

  // Update feature properties (e.g. label)
  const handleUpdateFeature = (featureId, updates) => {
    setFeatures((prev) => {
      const next = prev.map((f) => (f.id === featureId ? { ...f, ...updates } : f));
      pushHistoryState(mapImage, next);
      return next;
    });
  };

  // Delete a feature
  const handleDeleteFeature = (featureId) => {
    setFeatures((prev) => {
      const next = prev.filter((f) => f.id !== featureId);
      pushHistoryState(mapImage, next);
      return next;
    });
    if (selectedFeatureId === featureId) {
      setSelectedFeatureId(null);
    }
  };

  // Update feature box (e.g. while dragging/resizing handles in edit mode)
  const handleUpdateFeatureBox = (featureId, nextBox) => {
    setFeatures((prev) => prev.map((f) => (f.id === featureId ? { ...f, box: nextBox } : f)));
  };

  return (
    <div className="app-shell flex flex-col h-screen text-slate-100 font-sans overflow-hidden">
      {/* Top Header Navigation */}
      <Navbar
        selectedModel={selectedModel}
        onSelectModel={handleSelectModel}
        models={availableModels}
        forgeImageMode={forgeImageMode}
        onSelectForgeImageMode={handleSelectForgeImageMode}
        showGrid={showGrid}
        onToggleGrid={() => setShowGrid((prev) => !prev)}
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
        activeMode={activeMode}
        onSetMode={handleSetMode}
        onDetectFeatures={handleDetectFeatures}
        isDetectingFeatures={isDetectingFeatures}
        featureCount={features.length}
        hasMap={Boolean(mapImage)}
        currentView={currentView}
        onSetView={setCurrentView}
        onSaveMap={handleSaveMapToDisk}
        isSavingMap={isSavingMap}
        saveStatus={saveStatus}
        currentMapId={currentMapId}
      />
      {/* Workspace: the prompt while there is no map, the canvas once there is, or the map library */}
      <div className="flex flex-1 overflow-hidden relative z-10">
        {currentView === 'library' ? (
          <MapLibrary
            onSelectMap={handleSelectMapFromLibrary}
            onCreateNewMap={handleCreateNewMap}
            currentMapId={currentMapId}
            onClose={() => setCurrentView('studio')}
          />
        ) : mapImage ? (
          <BattlemapCanvas
            key={mapSessionId}
            mapImage={mapImage}
            showGrid={showGrid}
            onToggleGrid={() => setShowGrid((prev) => !prev)}
            regionSelection={regionSelection}
            onRegionSelectionChange={(sel) => {
              setRegionSelection(sel);
              if (sel) setIsRetryPillDismissed(true);
            }}
            onOpenRegionPrompt={() => setIsRegionModalOpen(true)}
            onDeleteRegion={handleDeleteRegion}
            renderRegionPopover={(anchor) => (
              <ActionModal
                open={isRegionModalOpen}
                anchor={anchor}
                regionSelection={regionSelection}
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
            activeMode={activeMode}
            features={features}
            selectedFeatureId={selectedFeatureId}
            onSelectFeature={setSelectedFeatureId}
            onToggleFeatureState={handleToggleFeatureState}
            onUpdateFeatureBox={handleUpdateFeatureBox}
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
            onOpenLibrary={() => setCurrentView('library')}
          />
        )}

        {/* Edit Mode: Feature Inspector Modal */}
        {currentView === 'studio' && mapImage && activeMode === 'edit' && selectedFeatureId && (
          <FeatureInspector
            feature={features.find((f) => f.id === selectedFeatureId)}
            onClose={() => setSelectedFeatureId(null)}
            onUpdate={handleUpdateFeature}
            onToggleState={handleToggleFeatureState}
            onForgeState={handleForgeFeatureState}
            onDelete={handleDeleteFeature}
            isForgingState={isForgingFeatureState}
            onOpenWikiArticle={() => setIsWikiDrawerOpen(true)}
          />
        )}

        {/* Play Mode: Tactical Feature Card */}
        {currentView === 'studio' && mapImage && activeMode === 'play' && selectedFeatureId && (
          <PlayFeatureCard
            feature={features.find((f) => f.id === selectedFeatureId)}
            onClose={() => setSelectedFeatureId(null)}
            onToggleState={handleToggleFeatureState}
            onSwitchToEdit={(featureId) => {
              setActiveMode('edit');
              setSelectedFeatureId(featureId);
            }}
            onOpenWikiArticle={() => setIsWikiDrawerOpen(true)}
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
