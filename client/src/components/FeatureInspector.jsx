import React, { useState, useEffect } from 'react';
import {
  X,
  DoorClosed,
  Hammer,
  Layers,
  Wand2,
  Trash2,
  BookOpen,
  Eye,
  RefreshCw,
  Check,
  Edit3,
  Sparkles
} from 'lucide-react';

const TYPE_CONFIG = {
  door: {
    label: 'Doorway',
    icon: DoorClosed,
    color: 'text-cyan-400 bg-cyan-500/10 border-cyan-500/30'
  },
  destructible_wall: {
    label: 'Destructible Wall',
    icon: Hammer,
    color: 'text-amber-400 bg-amber-500/10 border-amber-500/30'
  },
  carpet: {
    label: 'Hidden Floor Rug',
    icon: Layers,
    color: 'text-emerald-400 bg-emerald-500/10 border-emerald-500/30'
  }
};

export default function FeatureInspector({
  feature,
  onClose,
  onUpdate,
  onToggleState,
  onForgeState,
  onDelete,
  isForgingState = false,
  _onOpenWikiArticle = null
}) {
  const [isEditingLabel, setIsEditingLabel] = useState(false);
  const [labelValue, setLabelValue] = useState(feature?.label || '');
  const [altPrompt, setAltPrompt] = useState(feature?.suggestedAltPrompt || '');
  const [showReforgePrompt, setShowReforgePrompt] = useState(false);

  useEffect(() => {
    if (feature) {
      setLabelValue(feature.label || '');
      setAltPrompt(feature.suggestedAltPrompt || '');
    }
  }, [feature?.id, feature?.label, feature?.suggestedAltPrompt]);

  if (!feature) return null;
  const config = TYPE_CONFIG[feature.type] || {
    label: feature.type,
    icon: Sparkles,
    color: 'text-violet-400 bg-violet-500/10 border-violet-500/30'
  };
  const TypeIcon = config.icon;

  const hasState2Patch = Boolean(feature.states?.[1]?.patchDataUrl);
  const isState2Active = feature.currentState === 1;

  const handleSaveLabel = () => {
    const trimmed = labelValue.trim();
    if (trimmed && trimmed !== feature.label) {
      onUpdate?.(feature.id, { label: trimmed });
    }
    setIsEditingLabel(false);
  };

  const handleForge = () => {
    if (!altPrompt.trim() || isForgingState) return;
    onForgeState?.(feature.id, altPrompt.trim());
  };

  return (
    <div
      className="absolute top-20 right-6 z-30 w-96 max-h-[calc(100vh-6.5rem)] overflow-y-auto glass-panel border border-slate-700/60 shadow-2xl rounded-2xl p-5 flex flex-col gap-4 text-slate-100 animate-in fade-in slide-in-from-right-4 duration-200"
      style={{
        background: 'rgba(10, 15, 29, 0.88)',
        backdropFilter: 'blur(24px)'
      }}
    >
      {/* Top Header */}
      <div className="flex items-start justify-between gap-3 border-b border-white/8 pb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1.5">
            <span
              className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-semibold rounded-full border ${config.color}`}
            >
              <TypeIcon className="w-3.5 h-3.5" />
              {config.label}
            </span>
            <span className="text-[11px] font-mono text-slate-400">
              {feature.box.width}×{feature.box.height}px
            </span>
          </div>

          {isEditingLabel ? (
            <div className="flex items-center gap-2 mt-1">
              <input
                type="text"
                value={labelValue}
                onChange={(e) => setLabelValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleSaveLabel();
                  if (e.key === 'Escape') setIsEditingLabel(false);
                }}
                autoFocus
                className="w-full bg-slate-900/80 border border-cyan-500/50 rounded-lg px-2.5 py-1 text-sm text-white focus:outline-none focus:ring-1 focus:ring-cyan-500"
              />
              <button
                type="button"
                onClick={handleSaveLabel}
                className="p-1 rounded-md bg-cyan-500/20 text-cyan-300 hover:bg-cyan-500/30"
                title="Save label"
              >
                <Check className="w-4 h-4" />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2 group cursor-pointer" onClick={() => setIsEditingLabel(true)}>
              <h3 className="font-display font-semibold text-base text-white tracking-tight truncate">
                {feature.label}
              </h3>
              <Edit3 className="w-3.5 h-3.5 text-slate-400 opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>
          )}
        </div>

        <button
          type="button"
          onClick={onClose}
          className="p-1.5 rounded-lg text-slate-400 hover:text-white hover:bg-white/8 transition-colors"
          title="Close inspector"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Description / Lore connection */}
      {(feature.loreReference || feature.description) && (
        <div className="rounded-xl bg-slate-900/50 border border-white/6 p-3 flex flex-col gap-1.5">
          {feature.loreReference && (
            <div className="flex items-start gap-2 text-xs text-amber-300/90">
              <BookOpen className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
              <span className="leading-relaxed">{feature.loreReference}</span>
            </div>
          )}
          {feature.description && (
            <p className="text-xs text-slate-400 leading-relaxed pl-6">
              {feature.description}
            </p>
          )}
        </div>
      )}

      {/* Feature States Controller */}
      <div className="flex flex-col gap-2.5">
        <div className="flex items-center justify-between text-xs font-semibold text-slate-300">
          <span>INTERACTIVE STATES</span>
          <span className="text-[11px] font-mono text-cyan-400">
            Active: {feature.states[feature.currentState]?.label}
          </span>
        </div>

        {/* State 1: Default Base */}
        <div
          className={`rounded-xl border p-3 flex items-center justify-between transition-all ${
            !isState2Active
              ? 'bg-cyan-500/10 border-cyan-500/40 text-cyan-200 shadow-sm shadow-cyan-950'
              : 'bg-white/2 border-white/6 text-slate-400 hover:border-white/12'
          }`}
        >
          <div className="flex items-center gap-2.5">
            <div
              className={`w-2 h-2 rounded-full ${
                !isState2Active ? 'bg-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.8)]' : 'bg-slate-600'
              }`}
            />
            <div>
              <div className="text-xs font-semibold text-slate-200">
                State 1: {feature.states[0]?.label || 'Default'}
              </div>
              <div className="text-[11px] text-slate-400">Base battlemap surface</div>
            </div>
          </div>
          <button
            type="button"
            onClick={() => onToggleState?.(feature.id)}
            disabled={!isState2Active}
            className={`px-2.5 py-1 text-xs rounded-lg font-medium transition-colors ${
              !isState2Active
                ? 'bg-cyan-500/20 text-cyan-300 cursor-default'
                : 'bg-white/6 text-slate-300 hover:bg-white/12 hover:text-white'
            }`}
          >
            {!isState2Active ? 'Active' : 'Preview'}
          </button>
        </div>

        {/* State 2: Alternate Forged Patch */}
        <div
          className={`rounded-xl border p-3 flex flex-col gap-2.5 transition-all ${
            isState2Active
              ? 'bg-amber-500/10 border-amber-500/40 text-amber-200 shadow-sm shadow-amber-950'
              : 'bg-white/2 border-white/6 text-slate-400 hover:border-white/12'
          }`}
        >
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div
                className={`w-2 h-2 rounded-full ${
                  isState2Active ? 'bg-amber-400 shadow-[0_0_8px_rgba(245,158,11,0.8)]' : 'bg-slate-600'
                }`}
              />
              <div>
                <div className="text-xs font-semibold text-slate-200">
                  State 2: {feature.states[1]?.label || 'Alternate'}
                </div>
                <div className="text-[11px] text-slate-400">
                  {hasState2Patch ? 'Forged patch ready' : 'Not forged yet'}
                </div>
              </div>
            </div>

            {hasState2Patch ? (
              <button
                type="button"
                onClick={() => onToggleState?.(feature.id)}
                className={`px-2.5 py-1 text-xs rounded-lg font-medium transition-colors ${
                  isState2Active
                    ? 'bg-amber-500/20 text-amber-300 cursor-default'
                    : 'bg-white/6 text-slate-300 hover:bg-white/12 hover:text-white'
                }`}
              >
                {isState2Active ? 'Active' : 'Preview'}
              </button>
            ) : (
              <span className="text-[11px] font-medium text-amber-400/80 bg-amber-500/10 px-2 py-0.5 rounded-full border border-amber-500/20">
                Needs Forge
              </span>
            )}
          </div>

          {/* If Patch exists, show thumbnail preview and re-forge toggle */}
          {hasState2Patch && (
            <div className="flex items-center gap-3 pt-2 border-t border-white/6">
              <div className="w-14 h-14 rounded-lg overflow-hidden border border-amber-500/30 bg-slate-900 shrink-0">
                <img
                  src={feature.states[1].patchDataUrl}
                  alt="State 2 preview"
                  className="w-full h-full object-cover"
                />
              </div>
              <div className="flex-1 min-w-0 flex flex-col gap-1">
                <span className="text-xs text-slate-300 truncate">Alternate visual patch</span>
                <button
                  type="button"
                  onClick={() => setShowReforgePrompt(!showReforgePrompt)}
                  className="text-xs text-amber-400 hover:text-amber-300 underline underline-offset-2 text-left"
                >
                  {showReforgePrompt ? 'Hide re-forge prompt' : 'Change prompt & re-forge'}
                </button>
              </div>
            </div>
          )}

          {/* Prompt input if not forged OR if re-forging */}
          {(!hasState2Patch || showReforgePrompt) && (
            <div className="flex flex-col gap-2 pt-2 border-t border-white/6">
              <label className="text-[11px] font-semibold text-slate-300">
                State 2 Forging Prompt:
              </label>
              <textarea
                value={altPrompt}
                onChange={(e) => setAltPrompt(e.target.value)}
                placeholder="Describe how the feature looks in its alternate state..."
                rows={3}
                className="w-full bg-slate-900/90 border border-slate-700 rounded-xl p-2.5 text-xs text-slate-200 placeholder:text-slate-500 focus:outline-none focus:border-amber-500/60 focus:ring-1 focus:ring-amber-500/40 resize-none font-sans"
              />
              <button
                type="button"
                onClick={handleForge}
                disabled={!altPrompt.trim() || isForgingState}
                className="btn btn-primary py-2 px-3 text-xs w-full flex items-center justify-center gap-2"
              >
                {isForgingState ? (
                  <>
                    <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                    <span>Forging Alternate State...</span>
                  </>
                ) : (
                  <>
                    <Wand2 className="w-3.5 h-3.5" />
                    <span>{hasState2Patch ? 'Re-forge State 2 Patch' : 'Forge State 2 Patch'}</span>
                  </>
                )}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Bottom Footer Actions */}
      <div className="flex items-center justify-between border-t border-white/8 pt-3 mt-1">
        <button
          type="button"
          onClick={() => onToggleState?.(feature.id)}
          className="btn btn-ghost py-1.5 px-3 text-xs flex items-center gap-1.5 text-slate-300 hover:text-white"
          title="Toggle preview between State 1 and State 2"
        >
          <Eye className="w-3.5 h-3.5 text-cyan-400" />
          <span>Toggle State ({isState2Active ? 'State 2' : 'State 1'})</span>
        </button>

        <button
          type="button"
          onClick={() => onDelete?.(feature.id)}
          className="p-1.5 rounded-lg text-rose-400/80 hover:text-rose-300 hover:bg-rose-500/10 transition-colors"
          title="Delete feature"
        >
          <Trash2 className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}
