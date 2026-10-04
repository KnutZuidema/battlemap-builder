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
  Gamepad2
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
  const [isModelDropdownOpen, setIsModelDropdownOpen] = useState(false);

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
      <div className="flex items-center gap-2.5 sm:gap-3.5 min-w-0">
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

          <span className="hidden lg:block w-px h-7 bg-white/[0.08]" />

          <input
            value={mapTitle}
            onChange={(e) => setMapTitle(e.target.value)}
            spellCheck={false}
            aria-label="Map title"
            className="hidden lg:block w-28 xl:w-48 bg-transparent border border-transparent hover:border-white/[0.08] focus:border-amber-400/40 focus:bg-black/30 rounded-lg px-2.5 py-1.5 text-xs font-medium text-slate-200 outline-none transition truncate"
          />
        </div>
      </div>

      {/* Center Controls: Mode Toggle & Feature Detection */}
      <div className="flex items-center gap-2">
        {hasMap && (
          <div className="flex items-center gap-1 bg-black/40 p-1 rounded-xl border border-white/[0.08] shadow-inner shadow-black/40">
            <button
              type="button"
              onClick={() => onSetMode && onSetMode('edit')}
              title="Edit Mode: Inspect, forge, and adjust interactive map features and regions"
              className={`flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold transition-all ${
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
              title="Play Mode: Tactical gameplay mode with seamless interactive feature triggers"
              className={`flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold transition-all ${
                activeMode === 'play'
                  ? 'bg-amber-500/20 text-amber-200 border border-amber-500/40 shadow-sm shadow-amber-950/50'
                  : 'text-slate-400 hover:text-slate-200 hover:bg-white/[0.04] border border-transparent'
              }`}
            >
              <Gamepad2 className={`w-3.5 h-3.5 ${activeMode === 'play' ? 'text-amber-400' : 'text-slate-500'}`} />
              <span>Play</span>
            </button>
          </div>
        )}

        {hasMap && activeMode === 'edit' && (
          <button
            type="button"
            onClick={onDetectFeatures}
            disabled={isDetectingFeatures}
            title="Detect doors, destructible walls, and hidden trapdoors using AI Vision"
            className={`btn px-3 py-1.5 text-xs flex items-center gap-1.5 ${
              featureCount > 0
                ? 'btn-ghost text-cyan-200 border-cyan-500/30 bg-cyan-500/10 hover:bg-cyan-500/20'
                : 'btn-ghost text-amber-300 border-amber-500/30 bg-amber-500/10 hover:bg-amber-500/20'
            }`}
          >
            <Sparkles className={`w-3.5 h-3.5 ${isDetectingFeatures ? 'animate-spin text-cyan-400' : 'text-amber-400'}`} />
            <span className="hidden sm:inline">
              {isDetectingFeatures ? 'Detecting Features…' : 'Detect Features'}
            </span>
            {featureCount > 0 && !isDetectingFeatures && (
              <span className="text-[10px] font-mono font-bold bg-cyan-500/20 text-cyan-200 px-1.5 py-0.2 rounded-full border border-cyan-500/40">
                {featureCount}
              </span>
            )}
          </button>
        )}
      </div>

      {/* Model Picker Dropdown */}
      <div className="relative select-none shrink-0" ref={modelDropdownRef}>
        <button
          type="button"
          onClick={() => setIsModelDropdownOpen((prev) => !prev)}
          aria-haspopup="listbox"
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

      {/* Actions & Tools */}
      <div className="flex items-center gap-2 shrink-0">
        {/* Persistent Cost Tracker */}
        <div className="hidden lg:block">
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
            <span className="hidden xl:inline">Debug Logs</span>
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
            <span className="hidden xl:inline">World Wiki</span>
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
        <div className="flex items-center gap-0.5 bg-black/25 p-1 rounded-xl border border-white/[0.07]">
          <input
            type="file"
            ref={fileInputRef}
            onChange={handleFileChange}
            accept=".json"
            style={{ display: 'none' }}
          />

          <button
            onClick={() => fileInputRef.current?.click()}
            title="Load Map JSON"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-300 hover:text-white hover:bg-white/[0.06] transition-all cursor-pointer"
          >
            <Upload className="w-3.5 h-3.5 text-slate-400" />
            <span className="hidden xl:inline">Load</span>
          </button>
          <span className="w-px h-3.5 bg-white/[0.08]" />

          <button
            onClick={onExportJson}
            title="Save Map Project State (JSON)"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-300 hover:text-white hover:bg-white/[0.06] transition-all cursor-pointer"
          >
            <FileJson className="w-3.5 h-3.5 text-slate-400" />
            <span className="hidden xl:inline">Save</span>
          </button>

          <span className="w-px h-3.5 bg-white/[0.08]" />

          <button
            onClick={onExportPng}
            title="Export Map as PNG"
            className="flex items-center gap-1.5 px-3 py-1 rounded-lg text-xs font-semibold bg-gradient-to-r from-amber-400 via-amber-500 to-amber-600 text-slate-950 hover:brightness-105 shadow-sm shadow-amber-950/40 transition-all cursor-pointer"
          >
            <Download className="w-3.5 h-3.5 text-slate-950" strokeWidth={2.4} />
            <span className="hidden sm:inline">Export PNG</span>
            <span className="sm:hidden">Export</span>
          </button>

          <span className="w-px h-3.5 bg-white/[0.08]" />

          <button
            onClick={onClearCanvas}
            title="New Map (Clear Canvas)"
            className="flex items-center justify-center p-1.5 rounded-lg text-slate-400 hover:text-rose-300 hover:bg-rose-500/15 transition-all cursor-pointer"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </header>
  );
}
