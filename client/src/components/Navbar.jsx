import React, { useRef, useState, useEffect } from 'react';
import {
  Shield,
  Download,
  RotateCcw,
  RotateCw,
  RefreshCw,
  Trash2,
  FileJson,
  Upload,
  Terminal,
  Sparkles,
  Zap,
  BookOpen,
  ChevronDown,
  Check,
  Wand2,
  Gamepad2,
  Settings,
  Sliders,
  Layers,
  Grid3X3
} from 'lucide-react';
import CostTracker from './CostTracker';

const DEFAULT_MODELS = [
  {
    id: 'google/gemini-3.1-flash-image',
    label: 'Nano Banana 2 Flash',
    shortLabel: 'Flash',
    badge: 'Quality',
    costHint: '$0.50/M in · $3/M out (~$0.07 per map)',
    note: 'High-fidelity battlemap generation'
  },
  {
    id: 'google/gemini-3.1-flash-lite-image',
    label: 'Nano Banana 2 Flash Lite',
    shortLabel: 'Flash Lite',
    badge: 'Fast',
    costHint: '$0.25/M in · $1.50/M out (~$0.035 per map)',
    note: 'Ultra-fast, lightweight generation'
  }
];
export default function Navbar({
  selectedModel,
  onSelectModel,
  models,
  forgeImageMode = 'highlighted-only',
  onSelectForgeImageMode,
  showGrid = true,
  onToggleGrid,
  onOpenDebugModal,
  debugLogCount,
  onOpenWikiDrawer,
  onExportPng,
  onExportJson,
  onImportJson,
  onClearCanvas,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
  onRetry,
  canRetry,
  isRetrying,
  retryTooltip,
  mapTitle,
  setMapTitle,
  mapCost,
  wikiContext,
  activeMode = 'edit',
  onSetMode,
  onDetectFeatures,
  isDetectingFeatures = false,
  featureCount = 0,
  hasMap = false
}) {
  const fileInputRef = useRef(null);
  const modelDropdownRef = useRef(null);
  const settingsDropdownRef = useRef(null);
  const [isModelDropdownOpen, setIsModelDropdownOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);

  useEffect(() => {
    if (!isSettingsOpen) return;
    const handleClickOutside = (e) => {
      if (settingsDropdownRef.current && !settingsDropdownRef.current.contains(e.target)) {
        setIsSettingsOpen(false);
      }
    };
    const handleKeyDown = (e) => {
      if (e.key === 'Escape') {
        setIsSettingsOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [isSettingsOpen]);
  useEffect(() => {
    if (!isModelDropdownOpen) return;
    const handleClickOutside = (e) => {
      if (modelDropdownRef.current && !modelDropdownRef.current.contains(e.target)) {
        setIsModelDropdownOpen(false);
      }
    };
    const handleKeyDown = (e) => {
      if (e.key === 'Escape') {
        setIsModelDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [isModelDropdownOpen]);
  const handleFileChange = (e) => {
    const file = e.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        try {
          const json = JSON.parse(event.target.result);
          onImportJson(json);
        } catch {
          alert('Invalid map file format');
        }
      };
      reader.readAsText(file);
    }
    e.target.value = '';
  };

  const modelList = (models && models.length > 0) ? models : DEFAULT_MODELS;
  const currentModel = modelList.find(
    (m) => selectedModel === m.id || (m.aliases && m.aliases.includes(selectedModel))
  ) || modelList[0];
  const isCurrentLite = currentModel?.id?.includes('lite');
  const CurrentIcon = isCurrentLite ? Zap : Sparkles;
  return (
    <header className="h-[3.75rem] border-b border-white/[0.07] glass-panel px-3 sm:px-4 flex items-center justify-between gap-2 shrink-0 z-30 relative select-none">
      {/* Brand & Map Title */}
      <div className="flex items-center gap-2.5 sm:gap-3.5 shrink-0">
        <div className="relative shrink-0">
          <div className="absolute inset-0 rounded-xl bg-amber-500/40 blur-lg" />
          <div className="relative w-9 h-9 rounded-xl bg-gradient-to-br from-amber-300 via-amber-500 to-amber-700 flex items-center justify-center shadow-lg shadow-amber-900/40 border border-amber-300/40">
            <Shield className="w-[18px] h-[18px] text-slate-950" strokeWidth={2.4} />
          </div>
        </div>

        <div className="flex items-center gap-3 min-w-0">
          <div className="leading-none hidden sm:block">
            <h1 className="font-display text-[15px] font-bold tracking-[0.02em] bg-gradient-to-r from-amber-200 via-amber-400 to-amber-600 bg-clip-text text-transparent">
              DUNGEON FORGE
            </h1>
            <p className="mt-1 text-[9px] font-mono uppercase tracking-[0.28em] text-slate-500">Battlemap Studio</p>
          </div>

          <span className="hidden 2xl:block w-px h-7 bg-white/[0.08]" />

          <input
            value={mapTitle}
            onChange={(e) => setMapTitle(e.target.value)}
            spellCheck={false}
            aria-label="Map title"
            className="hidden 2xl:block w-36 2xl:w-48 bg-transparent border border-transparent hover:border-white/[0.08] focus:border-amber-400/40 focus:bg-black/30 rounded-lg px-2.5 py-1.5 text-xs font-medium text-slate-200 outline-none transition truncate"
          />
        </div>
      </div>

      {/* Center Controls: Mode Toggle, Feature Detection & Model Picker */}
      <div className="flex items-center gap-2 shrink-0">
        {/* Mode Toggle & Tactical Features Pill */}
        <div className="flex items-center gap-0.5 bg-black/25 p-1 rounded-xl border border-white/[0.07]">
          <button
            type="button"
            onClick={() => onSetMode && onSetMode('edit')}
            title="Edit Mode: Inspect, forge, and adjust interactive map features and regions"
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
              activeMode === 'edit'
                ? 'bg-cyan-500/20 text-cyan-200 border border-cyan-500/40 shadow-sm shadow-cyan-950/50'
                : 'text-slate-400 hover:text-slate-200 hover:bg-white/[0.04] border border-transparent'
            }`}
          >
            <Wand2 className={`w-3.5 h-3.5 ${activeMode === 'edit' ? 'text-cyan-400' : 'text-slate-500'}`} />
            <span>Edit</span>
          </button>

          <button
            type="button"
            onClick={() => onSetMode && onSetMode('play')}
            disabled={!hasMap}
            title={
              !hasMap
                ? 'Generate or load a battlemap to enter Play mode'
                : 'Play Mode: Tactical gameplay mode with seamless interactive feature triggers'
            }
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
              !hasMap
                ? 'text-slate-500 opacity-40 cursor-not-allowed border border-transparent'
                : activeMode === 'play'
                ? 'bg-amber-500/20 text-amber-200 border border-amber-500/40 shadow-sm shadow-amber-950/50 cursor-pointer'
                : 'text-slate-400 hover:text-slate-200 hover:bg-white/[0.04] border border-transparent cursor-pointer'
            }`}
          >
            <Gamepad2 className={`w-3.5 h-3.5 ${activeMode === 'play' ? 'text-amber-400' : 'text-slate-500'}`} />
            <span>Play</span>
          </button>

          <span className="w-px h-3.5 bg-white/[0.08]" />

          <button
            type="button"
            onClick={onDetectFeatures}
            disabled={!hasMap || isDetectingFeatures || activeMode !== 'edit'}
            title={
              !hasMap
                ? 'Generate or load a battlemap first to detect features'
                : activeMode !== 'edit'
                ? 'Switch to Edit mode to detect features'
                : 'Detect doors, destructible walls, and hidden trapdoors using AI Vision'
            }
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${
              !hasMap || activeMode !== 'edit'
                ? 'text-slate-500 opacity-40 cursor-not-allowed border border-transparent'
                : featureCount > 0
                ? 'text-cyan-200 bg-cyan-500/15 hover:bg-cyan-500/25 border border-cyan-500/30 cursor-pointer'
                : 'text-amber-300 bg-amber-500/15 hover:bg-amber-500/25 border border-amber-500/30 cursor-pointer'
            }`}
          >
            <Sparkles className={`w-3.5 h-3.5 ${isDetectingFeatures ? 'animate-spin text-cyan-400' : 'text-amber-400'}`} />
            <span className="hidden sm:inline">
              {isDetectingFeatures ? 'Detecting…' : 'Detect Features'}
            </span>
            {featureCount > 0 && !isDetectingFeatures && (
              <span className="text-[10px] font-mono font-bold bg-cyan-500/20 text-cyan-200 px-1.5 py-0.2 rounded-full border border-cyan-500/40">
                {featureCount}
              </span>
            )}
          </button>
        </div>

        {/* Model Picker Dropdown */}
        <div className="relative select-none shrink-0" ref={modelDropdownRef}>
        <button
          type="button"
          onClick={() => {
            setIsModelDropdownOpen((prev) => !prev);
            setIsSettingsOpen(false);
          }}
          aria-expanded={isModelDropdownOpen}
          title={`Active Model: ${currentModel.label} (${currentModel.id})\nClick to change model`}
          className={`flex items-center gap-2 px-3 py-1.5 rounded-xl border transition-all cursor-pointer ${
            isModelDropdownOpen
              ? 'bg-amber-500/15 border-amber-500/40 text-amber-200 shadow-sm shadow-amber-950/50'
              : 'bg-black/35 hover:bg-black/50 border-white/[0.08] hover:border-amber-500/30 text-slate-200 shadow-inner shadow-black/40'
          }`}
        >
          <div className="flex items-center gap-1.5 min-w-0">
            <CurrentIcon className="w-3.5 h-3.5 text-amber-400 shrink-0" />
            <span className="text-[10px] font-mono text-slate-400 uppercase tracking-wider hidden 2xl:inline">Model:</span>
            <span className="text-xs font-medium text-slate-200 truncate">
              <span className="hidden xl:inline">{currentModel.label}</span>
              <span className="xl:hidden">{currentModel.shortLabel || (isCurrentLite ? 'Flash Lite' : 'Flash')}</span>
            </span>
            {currentModel.badge && (
              <span className="text-[9px] font-mono px-1 py-0.2 rounded border bg-amber-400/20 text-amber-300 border-amber-400/30 hidden 2xl:inline shrink-0">
                {currentModel.badge}
              </span>
            )}
          </div>

          <ChevronDown
            className={`w-3.5 h-3.5 text-slate-400 transition-transform duration-200 shrink-0 ${
              isModelDropdownOpen ? 'rotate-180 text-amber-400' : ''
            }`}
          />
        </button>

        {/* Dropdown Menu */}
        {isModelDropdownOpen && (
          <div
            role="listbox"
            aria-label="Select AI Image Model"
            className="animate-rise absolute left-0 sm:left-1/2 sm:-translate-x-1/2 top-full mt-2 w-72 sm:w-80 p-2 rounded-2xl bg-slate-950/95 backdrop-blur-xl border border-white/[0.1] shadow-2xl shadow-black/90 z-50 text-xs"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-2.5 py-1.5 border-b border-white/[0.08] mb-1.5">
              <span className="text-[10px] font-mono text-slate-400 uppercase tracking-wider">
                Select Model
              </span>
              <span className="text-[10px] font-mono text-amber-400/80">OpenRouter</span>
            </div>

            <div className="space-y-1">
              {modelList.map((m) => {
                const isSelected = selectedModel === m.id || (m.aliases && m.aliases.includes(selectedModel));
                const isLite = m.id.includes('lite');
                const Icon = isLite ? Zap : Sparkles;

                return (
                  <button
                    key={m.id}
                    type="button"
                    role="option"
                    aria-selected={isSelected}
                    onClick={() => {
                      if (onSelectModel) onSelectModel(m.id);
                      setIsModelDropdownOpen(false);
                    }}
                    className={`w-full text-left p-2.5 rounded-xl border transition-all flex items-start gap-2.5 cursor-pointer ${
                      isSelected
                        ? 'bg-amber-500/15 border-amber-500/40 text-amber-100 shadow-sm shadow-amber-950/40'
                        : 'bg-transparent hover:bg-white/[0.05] border-transparent text-slate-300 hover:text-white'
                    }`}
                  >
                    <div
                      className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 mt-0.5 border ${
                        isSelected
                          ? 'bg-amber-500/20 border-amber-400/40 text-amber-400'
                          : 'bg-white/[0.04] border-white/[0.08] text-slate-400'
                      }`}
                    >
                      <Icon className="w-3.5 h-3.5" />
                    </div>

                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between gap-1.5">
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span className="font-semibold text-xs truncate">{m.label}</span>
                          {m.badge && (
                            <span
                              className={`text-[9px] font-mono px-1 py-0.2 rounded border shrink-0 ${
                                isSelected
                                  ? 'bg-amber-400/20 text-amber-300 border-amber-400/30'
                                  : 'bg-white/[0.04] text-slate-500 border-white/[0.06]'
                              }`}
                            >
                              {m.badge}
                            </span>
                          )}
                        </div>
                        {isSelected && <Check className="w-3.5 h-3.5 text-amber-400 shrink-0" />}
                      </div>

                      {m.costHint && (
                        <div className="text-[10px] font-mono text-amber-400/70 mt-1">
                          {m.costHint}
                        </div>
                      )}
                      {m.note && (
                        <div className="text-[10px] text-slate-400 mt-0.5 leading-snug">
                          {m.note}
                        </div>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        )}
        </div>
      </div>

      {/* Actions & Tools */}
      <div className="flex items-center gap-2 shrink-0">
        {/* Persistent Cost Tracker */}
        <div className="hidden 2xl:block">
          <CostTracker mapCost={mapCost} />
        </div>

        <div className="flex items-center gap-0.5 bg-black/25 p-1 rounded-xl border border-white/[0.07]">
          <button onClick={onUndo} disabled={!canUndo} title="Undo (Ctrl+Z)" className="icon-btn p-1.5">
            <RotateCcw className="w-3.5 h-3.5" />
          </button>
          <span className="w-px h-3.5 bg-white/[0.08]" />
          <button onClick={onRedo} disabled={!canRedo} title="Redo (Ctrl+Shift+Z)" className="icon-btn p-1.5">
            <RotateCw className="w-3.5 h-3.5" />
          </button>
          <span className="w-px h-3.5 bg-white/[0.08]" />
          <button
            onClick={onRetry}
            disabled={!canRetry}
            title={retryTooltip || 'Retry last generation (Alt+R)'}
            className={`flex items-center gap-1.5 px-2 py-1 rounded-lg text-xs font-medium transition-all ${
              canRetry
                ? 'text-cyan-300 hover:text-cyan-100 hover:bg-cyan-500/15'
                : 'text-slate-500 opacity-40 cursor-not-allowed'
            }`}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRetrying ? 'animate-spin' : ''}`} />
            <span className="hidden sm:inline">Retry</span>
          </button>
        </div>

        {/* Developer & Lore Tools: Debug Logs & World Wiki */}
        <div className="flex items-center gap-0.5 bg-black/25 p-1 rounded-xl border border-white/[0.07]">
          <button
            onClick={onOpenDebugModal}
            title="Toggle Prompt & Response Debug Window"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-300 hover:text-cyan-200 hover:bg-cyan-500/15 transition-all cursor-pointer"
          >
            <Terminal className="w-3.5 h-3.5 text-cyan-400" />
            <span className="hidden 2xl:inline">Debug Logs</span>
            {debugLogCount > 0 && (
              <span className="text-[10px] font-mono font-bold bg-cyan-500/20 text-cyan-200 px-1.5 rounded-full border border-cyan-500/40">
                {debugLogCount}
              </span>
            )}
          </button>

          <span className="w-px h-3.5 bg-white/[0.08]" />

          <button
            onClick={onOpenWikiDrawer}
            title="Browse D&D World Wiki & Tactical Lore"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-300 hover:text-amber-200 hover:bg-amber-500/15 transition-all cursor-pointer"
          >
            <BookOpen className="w-3.5 h-3.5 text-amber-400" />
            <span className="hidden 2xl:inline">World Wiki</span>
          </button>

          {wikiContext && (
            <>
              <span className="w-px h-3.5 bg-white/[0.08]" />
              <button
                onClick={onOpenWikiDrawer}
                title={`Active Lore Context: ${wikiContext.title} (${wikiContext.path})\nClick to view article in World Wiki`}
                className="flex items-center gap-1.5 px-2 py-0.5 rounded-lg text-xs font-medium bg-amber-500/20 text-amber-200 border border-amber-500/40 hover:bg-amber-500/30 transition-all max-w-[10rem] sm:max-w-[12rem] cursor-pointer"
              >
                <span className="truncate">Context: {wikiContext.title}</span>
              </button>
            </>
          )}
        </div>

        {/* Project & File Actions: Load, Save, Export PNG, Delete */}
        {/* Settings Menu Dropdown */}
        <div className="relative select-none shrink-0" ref={settingsDropdownRef}>
          <input
            type="file"
            ref={fileInputRef}
            onChange={handleFileChange}
            accept=".json"
            style={{ display: 'none' }}
          />

          <button
            type="button"
            onClick={() => {
              setIsSettingsOpen((prev) => !prev);
              setIsModelDropdownOpen(false);
            }}
            aria-haspopup="dialog"
            aria-expanded={isSettingsOpen}
            title="Settings: Forge image modes, grid overlay, project actions"
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl border transition-all cursor-pointer ${
              isSettingsOpen
                ? 'bg-amber-500/20 border-amber-500/40 text-amber-200 shadow-sm shadow-amber-950/50'
                : 'bg-black/35 hover:bg-black/50 border-white/[0.08] hover:border-amber-500/30 text-slate-200 shadow-inner shadow-black/40'
            }`}
          >
            <Settings className={`w-3.5 h-3.5 text-amber-400 transition-transform duration-200 ${isSettingsOpen ? 'rotate-45' : ''}`} />
            <span className="text-xs font-medium">Settings</span>
            <ChevronDown className={`w-3 h-3 text-slate-400 transition-transform duration-200 ${isSettingsOpen ? 'rotate-180 text-amber-400' : ''}`} />
          </button>

          {/* Expandable Settings Popover Dropdown */}
          {isSettingsOpen && (
            <div
              role="dialog"
              aria-label="Studio Settings"
              className="animate-rise absolute right-0 top-full mt-2 w-80 sm:w-96 max-h-[calc(100vh-5rem)] overflow-y-auto p-3.5 rounded-2xl bg-slate-950/95 backdrop-blur-xl border border-white/[0.1] shadow-2xl shadow-black/90 z-50 text-xs text-slate-200 space-y-4"
            >
              {/* Header */}
              <div className="flex items-center justify-between border-b border-white/[0.08] pb-2.5">
                <div className="flex items-center gap-2">
                  <Sliders className="w-4 h-4 text-amber-400" />
                  <span className="font-semibold text-sm text-slate-100 font-display tracking-wide">Studio Settings</span>
                </div>
                <span className="text-[10px] font-mono uppercase tracking-wider text-slate-500">Preferences</span>
              </div>

              {/* Section 1: Region Forging Image Inputs */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
                    <Layers className="w-3.5 h-3.5 text-cyan-400" />
                    <span>Region Forge Image Inputs</span>
                  </div>
                </div>
                <p className="text-[11px] text-slate-400 leading-snug">
                  Configure which visual reference images are sent to the AI model during region edits and canvas expansion.
                </p>

                <div className="space-y-1.5 pt-0.5">
                  {/* Option 1: Highlighted Only */}
                  <button
                    type="button"
                    onClick={() => onSelectForgeImageMode && onSelectForgeImageMode('highlighted-only')}
                    className={`w-full text-left p-2.5 rounded-xl border transition-all flex items-start gap-2.5 cursor-pointer ${
                      forgeImageMode === 'highlighted-only'
                        ? 'bg-cyan-500/15 border-cyan-500/40 text-cyan-100 shadow-sm shadow-cyan-950/40'
                        : 'bg-white/[0.02] hover:bg-white/[0.05] border-white/[0.08] text-slate-300'
                    }`}
                  >
                    <div
                      className={`w-4 h-4 rounded-full mt-0.5 border flex items-center justify-center shrink-0 ${
                        forgeImageMode === 'highlighted-only'
                          ? 'border-cyan-400 bg-cyan-500/20 text-cyan-300'
                          : 'border-slate-600 bg-slate-800'
                      }`}
                    >
                      {forgeImageMode === 'highlighted-only' && <div className="w-1.5 h-1.5 rounded-full bg-cyan-400" />}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between gap-1">
                        <span className="font-semibold text-xs text-white">Highlighted Area Only</span>
                        <span className="text-[9px] font-mono px-1.5 py-0.2 rounded border bg-cyan-400/20 text-cyan-300 border-cyan-400/30">
                          1 Image · Recommended
                        </span>
                      </div>
                      <p className="mt-1 text-[11px] text-slate-400 leading-snug">
                        Sends only the context window with the target area highlighted in cyan tint. Avoids conflicting multi-image directives and lowers token usage.
                      </p>
                    </div>
                  </button>

                  {/* Option 2: All 3 Images */}
                  <button
                    type="button"
                    onClick={() => onSelectForgeImageMode && onSelectForgeImageMode('all-three')}
                    className={`w-full text-left p-2.5 rounded-xl border transition-all flex items-start gap-2.5 cursor-pointer ${
                      forgeImageMode === 'all-three' || forgeImageMode === 'all'
                        ? 'bg-amber-500/15 border-amber-500/40 text-amber-100 shadow-sm shadow-amber-950/40'
                        : 'bg-white/[0.02] hover:bg-white/[0.05] border-white/[0.08] text-slate-300'
                    }`}
                  >
                    <div
                      className={`w-4 h-4 rounded-full mt-0.5 border flex items-center justify-center shrink-0 ${
                        forgeImageMode === 'all-three' || forgeImageMode === 'all'
                          ? 'border-amber-400 bg-amber-500/20 text-amber-300'
                          : 'border-slate-600 bg-slate-800'
                      }`}
                    >
                      {(forgeImageMode === 'all-three' || forgeImageMode === 'all') && (
                        <div className="w-1.5 h-1.5 rounded-full bg-amber-400" />
                      )}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between gap-1">
                        <span className="font-semibold text-xs text-white">All 3 Images</span>
                        <span className="text-[9px] font-mono px-1.5 py-0.2 rounded border bg-amber-400/20 text-amber-300 border-amber-400/30">
                          3 Images · Legacy
                        </span>
                      </div>
                      <p className="mt-1 text-[11px] text-slate-400 leading-snug">
                        Dispatches raw context battlemap, cyan-highlighted guide image, and black/white binary mask.
                      </p>
                    </div>
                  </button>
                </div>
              </div>

              {/* Section 2: Canvas Display */}
              <div className="space-y-2 pt-2 border-t border-white/[0.08]">
                <div className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
                  <Grid3X3 className="w-3.5 h-3.5 text-amber-400" />
                  <span>Canvas & Display</span>
                </div>

                <div className="flex items-center justify-between p-2.5 rounded-xl bg-white/[0.02] border border-white/[0.08]">
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-xs text-white">5ft Tactical Grid</span>
                      <span className="text-[9px] font-mono px-1.5 py-0.2 rounded border bg-slate-800 text-slate-400 border-slate-700">
                        Key: G
                      </span>
                    </div>
                    <p className="text-[11px] text-slate-400 leading-snug mt-0.5">
                      64px = 5ft grid lines overlaid on map
                    </p>
                  </div>

                  <button
                    type="button"
                    onClick={onToggleGrid}
                    className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none ${
                      showGrid ? 'bg-cyan-500' : 'bg-slate-700'
                    }`}
                  >
                    <span
                      className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow-lg ring-0 transition duration-200 ease-in-out ${
                        showGrid ? 'translate-x-4' : 'translate-x-0'
                      }`}
                    />
                  </button>
                </div>
              </div>

              {/* Section 3: Project Actions */}
              <div className="space-y-2 pt-2 border-t border-white/[0.08]">
                <div className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
                  <FileJson className="w-3.5 h-3.5 text-slate-400" />
                  <span>Project Files & Canvas</span>
                </div>

                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      fileInputRef.current?.click();
                      setIsSettingsOpen(false);
                    }}
                    className="flex items-center gap-2 p-2 rounded-xl bg-white/[0.03] hover:bg-white/[0.07] border border-white/[0.08] text-slate-200 text-left transition cursor-pointer"
                  >
                    <Upload className="w-3.5 h-3.5 text-slate-400 shrink-0" />
                    <div className="min-w-0">
                      <div className="font-medium text-xs truncate">Load JSON</div>
                      <div className="text-[10px] text-slate-500 truncate">Import project</div>
                    </div>
                  </button>

                  <button
                    type="button"
                    onClick={() => {
                      onExportJson();
                      setIsSettingsOpen(false);
                    }}
                    disabled={!hasMap}
                    className={`flex items-center gap-2 p-2 rounded-xl border text-left transition ${
                      hasMap
                        ? 'bg-white/[0.03] hover:bg-white/[0.07] border-white/[0.08] text-slate-200 cursor-pointer'
                        : 'bg-white/[0.01] border-white/[0.04] text-slate-600 opacity-50 cursor-not-allowed'
                    }`}
                  >
                    <FileJson className="w-3.5 h-3.5 text-slate-400 shrink-0" />
                    <div className="min-w-0">
                      <div className="font-medium text-xs truncate">Save JSON</div>
                      <div className="text-[10px] text-slate-500 truncate">Export project</div>
                    </div>
                  </button>
                </div>

                <button
                  type="button"
                  onClick={() => {
                    onClearCanvas();
                    setIsSettingsOpen(false);
                  }}
                  disabled={!hasMap}
                  className={`w-full flex items-center justify-between p-2 rounded-xl border transition ${
                    hasMap
                      ? 'bg-rose-500/10 hover:bg-rose-500/20 border-rose-500/30 text-rose-300 cursor-pointer'
                      : 'bg-white/[0.01] border-white/[0.04] text-slate-600 opacity-50 cursor-not-allowed'
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <Trash2 className="w-3.5 h-3.5 text-rose-400 shrink-0" />
                    <span className="font-medium text-xs">New Map (Clear Canvas)</span>
                  </div>
                  <span className="text-[10px] font-mono text-rose-400/70">Reset</span>
                </button>
              </div>

              {/* Section 4: Diagnostics & Telemetry */}
              <div className="space-y-2 pt-2 border-t border-white/[0.08]">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
                    <Terminal className="w-3.5 h-3.5 text-cyan-400" />
                    <span>Telemetry & Debug</span>
                  </div>
                  {debugLogCount > 0 && (
                    <span className="text-[10px] font-mono font-bold bg-cyan-500/20 text-cyan-200 px-1.5 py-0.2 rounded-full border border-cyan-500/40">
                      {debugLogCount} logs
                    </span>
                  )}
                </div>

                <div className="flex items-center justify-between p-2.5 rounded-xl bg-white/[0.02] border border-white/[0.08]">
                  <div>
                    <div className="font-medium text-xs text-white">Debug Logs Inspector</div>
                    <div className="text-[10px] text-slate-400 mt-0.5">
                      {debugLogCount > 0
                        ? `${debugLogCount} request payload${debugLogCount === 1 ? '' : 's'} recorded`
                        : 'View system prompts & API responses'}
                    </div>
                  </div>

                  <button
                    type="button"
                    onClick={() => {
                      onOpenDebugModal();
                      setIsSettingsOpen(false);
                    }}
                    className="px-2.5 py-1 rounded-lg text-xs font-medium text-cyan-200 bg-cyan-500/20 hover:bg-cyan-500/30 border border-cyan-500/40 transition cursor-pointer"
                  >
                    Open Inspector
                  </button>
                </div>

                {mapCost && typeof mapCost.totalCost === 'number' && (
                  <div className="flex items-center justify-between px-2.5 py-1 text-[11px] text-slate-400 font-mono">
                    <span>Session Cost:</span>
                    <span className="text-amber-300 font-bold">${mapCost.totalCost.toFixed(4)}</span>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Export PNG Primary Action */}
        <button
          onClick={onExportPng}
          disabled={!hasMap}
          title="Export Map as PNG"
          className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold shadow-sm transition-all ${
            hasMap
              ? 'bg-gradient-to-r from-amber-400 via-amber-500 to-amber-600 text-slate-950 hover:brightness-105 shadow-amber-950/40 cursor-pointer'
              : 'bg-white/[0.04] text-slate-500 border border-white/[0.06] cursor-not-allowed opacity-50'
          }`}
        >
          <Download className={`w-3.5 h-3.5 ${hasMap ? 'text-slate-950' : 'text-slate-500'}`} strokeWidth={2.4} />
          <span className="hidden sm:inline">Export PNG</span>
          <span className="sm:hidden">Export</span>
        </button>
      </div>
    </header>
  );
}
