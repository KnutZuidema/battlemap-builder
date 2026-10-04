import React, { useRef } from 'react';
import {
  Shield,
  Download,
  RotateCcw,
  RotateCw,
  Trash2,
  FileJson,
  Upload,
  Terminal,
  Sparkles,
  Zap,
  BookOpen
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
  mapTitle,
  setMapTitle,
  mapCost
}) {
  const fileInputRef = useRef(null);

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

  return (
    <header className="h-[3.75rem] border-b border-white/[0.07] glass-panel px-4 flex items-center justify-between shrink-0 z-30 relative select-none">
      {/* Brand & Map Title */}
      <div className="flex items-center gap-3.5 min-w-0">
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

          <span className="hidden sm:block w-px h-7 bg-white/[0.08]" />

          <input
            value={mapTitle}
            onChange={(e) => setMapTitle(e.target.value)}
            spellCheck={false}
            aria-label="Map title"
            className="min-w-0 max-w-[18rem] bg-transparent border border-transparent hover:border-white/[0.08] focus:border-amber-400/40 focus:bg-black/30 rounded-lg px-2.5 py-1.5 text-xs font-medium text-slate-200 outline-none transition"
          />
        </div>
      </div>

      {/* Model Picker in Top Bar */}
      <div className="flex items-center gap-1.5 bg-black/40 p-1 rounded-xl border border-white/[0.08] shadow-inner shadow-black/40">
        <div className="hidden xl:flex items-center gap-1 px-2 text-[10px] font-mono text-slate-400 uppercase tracking-wider">
          <Sparkles className="w-3 h-3 text-amber-400/80" />
          <span>Model</span>
        </div>

        <div className="flex items-center gap-0.5">
          {modelList.map((m) => {
            const isSelected = selectedModel === m.id || (m.aliases && m.aliases.includes(selectedModel));
            const isLite = m.id.includes('lite');
            const Icon = isLite ? Zap : Sparkles;

            return (
              <button
                key={m.id}
                type="button"
                onClick={() => onSelectModel && onSelectModel(m.id)}
                title={`${m.label} (${m.id})\n${m.costHint || ''}\n${m.note || ''}`}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${
                  isSelected
                    ? 'bg-amber-500/20 text-amber-200 border border-amber-500/40 shadow-sm shadow-amber-950/50'
                    : 'text-slate-400 hover:text-slate-200 hover:bg-white/[0.04] border border-transparent'
                }`}
              >
                <Icon className={`w-3.5 h-3.5 ${isSelected ? 'text-amber-400' : 'text-slate-500'}`} />
                <span className="hidden md:inline">{m.label}</span>
                <span className="md:hidden">{m.shortLabel || (isLite ? 'Flash Lite' : 'Flash')}</span>
                {m.badge && (
                  <span
                    className={`text-[9px] font-mono px-1 py-0.2 rounded border hidden 2xl:inline ${
                      isSelected
                        ? 'bg-amber-400/20 text-amber-300 border-amber-400/30'
                        : 'bg-white/[0.04] text-slate-500 border-white/[0.06]'
                    }`}
                  >
                    {m.badge}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Actions & Tools */}
      <div className="flex items-center gap-2">
        {/* Persistent Cost Tracker */}
        <CostTracker mapCost={mapCost} />

        <span className="w-px h-6 bg-white/[0.08] mx-0.5 hidden sm:block" />

        <div className="flex items-center gap-0.5 bg-black/25 p-1 rounded-xl border border-white/[0.07]">
          <button onClick={onUndo} disabled={!canUndo} title="Undo" className="icon-btn p-1.5">
            <RotateCcw className="w-3.5 h-3.5" />
          </button>
          <span className="w-px h-3.5 bg-white/[0.08]" />
          <button onClick={onRedo} disabled={!canRedo} title="Redo" className="icon-btn p-1.5">
            <RotateCw className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Debug Logs */}
        <button
          onClick={onOpenDebugModal}
          title="Toggle Prompt & Response Debug Window"
          className="btn btn-ghost px-2.5 py-1.5 text-xs"
        >
          <Terminal className="w-3.5 h-3.5 text-cyan-300" />
          <span className="hidden lg:inline">Debug Logs</span>
          {debugLogCount > 0 && (
            <span className="text-[10px] font-mono font-bold bg-cyan-500/20 text-cyan-200 px-1.5 rounded-full border border-cyan-500/40">
              {debugLogCount}
            </span>
          )}
        </button>

        {/* World Wiki Lore & Tactical Browser */}
        <button
          onClick={onOpenWikiDrawer}
          title="Browse D&D World Wiki & Tactical Lore"
          className="btn btn-ghost px-2.5 py-1.5 text-xs text-amber-200/90 hover:text-amber-100 hover:border-amber-500/40 hover:bg-amber-500/10"
        >
          <BookOpen className="w-3.5 h-3.5 text-amber-400" />
          <span className="hidden md:inline">World Wiki</span>
        </button>

        <span className="w-px h-6 bg-white/[0.08] mx-0.5 hidden sm:block" />

        <input
          type="file"
          ref={fileInputRef}
          onChange={handleFileChange}
          accept=".json"
          style={{ display: 'none' }}
        />

        <button onClick={() => fileInputRef.current?.click()} title="Load Map JSON" className="btn btn-ghost px-2.5 py-1.5 text-xs">
          <Upload className="w-3.5 h-3.5" />
          <span className="hidden lg:inline">Load</span>
        </button>

        <button onClick={onExportJson} title="Save Map Project State" className="btn btn-ghost px-2.5 py-1.5 text-xs">
          <FileJson className="w-3.5 h-3.5" />
          <span className="hidden lg:inline">Save</span>
        </button>

        <button onClick={onExportPng} className="btn btn-primary px-3.5 py-2 text-xs">
          <Download className="w-3.5 h-3.5" />
          <span>Export PNG</span>
        </button>

        <button onClick={onClearCanvas} title="New Map" className="icon-btn p-2 bg-black/25 border border-white/[0.07] hover:text-rose-300 hover:border-rose-500/30 hover:bg-rose-950/40">
          <Trash2 className="w-3.5 h-3.5" />
        </button>
      </div>
    </header>
  );
}
