import React from 'react';
import {
  X,
  DoorClosed,
  DoorOpen,
  Hammer,
  Layers,
  BookOpen,
  Sparkles,
  Wand2,
  RotateCcw
} from 'lucide-react';

const TYPE_CONFIG = {
  door: {
    label: 'Doorway',
    iconClosed: DoorClosed,
    iconOpen: DoorOpen,
    color: 'text-cyan-400 bg-cyan-500/10 border-cyan-500/30'
  },
  destructible_wall: {
    label: 'Destructible Wall',
    iconClosed: Hammer,
    iconOpen: Hammer,
    color: 'text-amber-400 bg-amber-500/10 border-amber-500/30'
  },
  carpet: {
    label: 'Hidden Floor Rug',
    iconClosed: Layers,
    iconOpen: Layers,
    color: 'text-emerald-400 bg-emerald-500/10 border-emerald-500/30'
  }
};

export default function PlayFeatureCard({
  feature,
  onClose,
  onToggleState,
  onSwitchToEdit,
  _onOpenWikiArticle
}) {
  if (!feature) return null;

  const config = TYPE_CONFIG[feature.type] || {
    label: feature.type,
    iconClosed: Sparkles,
    iconOpen: Sparkles,
    color: 'text-violet-400 bg-violet-500/10 border-violet-500/30'
  };

  const isState2Active = feature.currentState === 1;
  const activeStateObj = feature.states[feature.currentState] || feature.states[0];
  const hasState2Patch = Boolean(feature.states?.[1]?.patchDataUrl);
  const TypeIcon = isState2Active ? config.iconOpen : config.iconClosed;

  return (
    <div
      className="absolute bottom-8 left-1/2 -translate-x-1/2 z-30 w-96 glass-panel border border-slate-700/60 shadow-2xl rounded-2xl p-5 flex flex-col gap-3.5 text-slate-100 animate-in fade-in slide-in-from-bottom-4 duration-200"
      style={{
        background: 'rgba(10, 15, 29, 0.90)',
        backdropFilter: 'blur(24px)'
      }}
    >
      {/* Header */}
      <div className="flex items-start justify-between gap-3 border-b border-white/8 pb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span
              className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-semibold rounded-full border ${config.color}`}
            >
              <TypeIcon className="w-3.5 h-3.5" />
              {config.label}
            </span>
            <span
              className={`inline-flex items-center px-2 py-0.5 text-[11px] font-semibold rounded-full border ${
                isState2Active
                  ? 'text-amber-300 bg-amber-500/12 border-amber-500/30'
                  : 'text-cyan-300 bg-cyan-500/12 border-cyan-500/30'
              }`}
            >
              {activeStateObj?.label || (isState2Active ? 'State 2' : 'State 1')}
            </span>
          </div>
          <h3 className="font-display font-bold text-base text-white tracking-tight truncate">
            {feature.label}
          </h3>
        </div>

        <button
          type="button"
          onClick={onClose}
          className="p-1.5 rounded-lg text-slate-400 hover:text-white hover:bg-white/8 transition-colors"
          title="Close card"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Lore details */}
      {(feature.loreReference || feature.description) && (
        <div className="rounded-xl bg-slate-900/50 border border-white/6 p-3 flex flex-col gap-1.5">
          {feature.loreReference && (
            <div className="flex items-start gap-2 text-xs text-amber-200/90">
              <BookOpen className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
              <span className="leading-relaxed font-medium">{feature.loreReference}</span>
            </div>
          )}
          {feature.description && (
            <p className="text-xs text-slate-400 leading-relaxed pl-6">
              {feature.description}
            </p>
          )}
        </div>
      )}

      {/* Interactive Trigger */}
      <div className="flex items-center justify-between gap-3 pt-1">
        {hasState2Patch ? (
          <button
            type="button"
            onClick={() => onToggleState?.(feature.id)}
            className={`flex-1 btn py-2.5 px-4 text-xs flex items-center justify-center gap-2 ${
              isState2Active ? 'btn-ghost text-slate-200 hover:text-white' : 'btn-primary'
            }`}
          >
            <RotateCcw className="w-4 h-4" />
            <span>
              {isState2Active
                ? `Restore ${feature.states[0]?.label || 'Default'}`
                : `Trigger ${feature.states[1]?.label || 'Alternate'}`}
            </span>
          </button>
        ) : (
          <div className="flex-1 flex flex-col gap-2">
            <div className="text-xs text-amber-300/80 bg-amber-500/10 border border-amber-500/20 rounded-xl p-2.5 text-center">
              State 2 ({feature.states[1]?.label}) hasn't been forged yet.
            </div>
            {onSwitchToEdit && (
              <button
                type="button"
                onClick={() => onSwitchToEdit(feature.id)}
                className="btn btn-ghost py-2 px-3 text-xs flex items-center justify-center gap-1.5 text-cyan-300 hover:text-cyan-200"
              >
                <Wand2 className="w-3.5 h-3.5" />
                <span>Switch to Edit Mode to Forge</span>
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
