import React, { useEffect, useRef } from 'react';
import { Sparkles, Crop, Wand2, Layers, Clock } from 'lucide-react';
import { formatTimer } from '../utils/format';

/**
 * The one thing shown before a map exists: a prompt. Everything else is a canvas affordance, so the
 * new-map screen is the textarea, the generate button, and nothing else to configure.
 */
export default function PromptHero({ prompt, setPrompt, onGenerate, isGenerating, phaseText, elapsedSeconds }) {
  const textareaRef = useRef(null);

  useEffect(() => {
    if (!isGenerating) textareaRef.current?.focus();
  }, [isGenerating]);

  const handleKeyDown = (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      if (!isGenerating && prompt.trim()) onGenerate();
    }
  };

  if (isGenerating) {
    return (
      <main className="flex-1 h-full relative overflow-hidden canvas-grid-bg grid place-items-center px-6 select-none">
        <div className="animate-rise flex flex-col items-center gap-5 text-center">
          <div className="relative grid place-items-center w-20 h-20">
            <div className="absolute inset-0 rounded-full border border-amber-400/30 animate-spin-slow border-t-amber-300" />
            <div className="absolute inset-3 rounded-full bg-amber-500/10 blur-md animate-breathe" />
            <Sparkles className="w-7 h-7 text-amber-300 animate-breathe" />
          </div>
          <div>
            <p className="font-display text-sm font-semibold text-amber-200 tracking-wide">
              {phaseText || 'FORGING YOUR BATTLEMAP'}
            </p>
            <p className="mt-1.5 font-mono text-xs text-slate-400 flex items-center justify-center gap-1.5">
              <Clock className="w-3.5 h-3.5" />
              <span>Elapsed {formatTimer(elapsedSeconds)} · this can take a moment</span>
            </p>
          </div>
        </div>
      </main>
    );
  }

  return (
    <main className="flex-1 h-full relative overflow-hidden canvas-grid-bg grid place-items-center px-6 select-none">
      <div className="animate-rise w-full max-w-2xl">
        <div className="relative mx-auto mb-6 w-16 h-16 animate-float">
          <div className="absolute inset-0 rounded-2xl bg-amber-500/25 blur-xl" />
          <div className="relative grid place-items-center w-16 h-16 rounded-2xl border border-amber-400/30 bg-gradient-to-br from-amber-400/20 to-transparent">
            <Layers className="w-7 h-7 text-amber-300" />
          </div>
        </div>

        <h2 className="font-display text-center text-2xl font-bold text-slate-100">Forge your first battlemap</h2>
        <p className="mt-2 text-center text-sm leading-relaxed text-slate-400">
          Describe the scene. The model paints a top-down map ready for the table.
        </p>

        <div className="mt-7 panel-card p-4 backdrop-blur-xl">
          <textarea
            ref={textareaRef}
            rows={4}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="e.g. Flooded stone dungeon hallway with glowing magic runes, mossy columns, and a treasure chest..."
            className="field text-sm p-3.5 leading-relaxed resize-none font-sans"
          />
          <div className="mt-3 flex items-center justify-between gap-3">
            <p className="text-[10px] font-mono text-slate-500">Engine: server-configured image model</p>
            <button
              onClick={onGenerate}
              disabled={isGenerating || !prompt.trim()}
              className="btn btn-primary px-5 py-2.5 text-xs"
            >
              {isGenerating ? (
                <>
                  <Wand2 className="w-4 h-4 animate-spin-slow" />
                  <span>Rendering…</span>
                </>
              ) : (
                <>
                  <Sparkles className="w-4 h-4" />
                  <span>Generate Map</span>
                </>
              )}
            </button>
          </div>
        </div>

        <div className="mt-6 grid grid-cols-3 gap-2 text-left">
          {[
            { icon: Sparkles, label: 'Describe', desc: 'Text prompt' },
            { icon: Crop, label: 'Select', desc: 'Right-drag 64px box' },
            { icon: Wand2, label: 'Forge', desc: 'Inpaint or expand' }
          ].map((step, i) => {
            const Icon = step.icon;
            return (
              <div key={step.label} className="rounded-xl border border-white/[0.07] bg-white/[0.03] px-3 py-2.5">
                <Icon className="w-4 h-4 text-amber-300 mb-1.5" />
                <div className="text-[11px] font-semibold text-slate-200">{i + 1}. {step.label}</div>
                <div className="text-[10px] text-slate-500">{step.desc}</div>
              </div>
            );
          })}
        </div>
      </div>
    </main>
  );
}
