import React, { useEffect, useRef } from 'react';
import { X, Wand2, Command } from 'lucide-react';

const TONES = {
  cyan: {
    tile: 'text-cyan-300 bg-cyan-500/12 border-cyan-500/25',
    glow: 'from-cyan-500/25',
    field: 'field-cyan',
    button: 'btn-cyan'
  },
  emerald: {
    tile: 'text-emerald-300 bg-emerald-500/12 border-emerald-500/25',
    glow: 'from-emerald-500/25',
    field: 'field-emerald',
    button: 'btn-emerald'
  }
};

/**
 * One prompt modal for both canvas actions: the canvas decides *where* (a painted mask, a clicked
 * edge hotspot) and this collects the *what*. Escape closes, Cmd/Ctrl+Enter submits.
 */
export default function ActionModal({
  open,
  tone = 'cyan',
  icon: Icon,
  title,
  subtitle,
  chip,
  placeholder,
  value,
  onChange,
  onSubmit,
  onClose,
  busy = false,
  busyLabel = 'Working…',
  confirmLabel = 'Apply',
  hint,
  allowEmpty = false
}) {
  const textareaRef = useRef(null);
  const t = TONES[tone] || TONES.cyan;
  const canSubmit = !busy && (allowEmpty || Boolean(value && value.trim()));

  useEffect(() => {
    if (!open || busy) return;
    textareaRef.current?.focus();
  }, [open, busy]);

  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (e) => {
      if (e.key === 'Escape' && !busy) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [open, busy, onClose]);

  if (!open) return null;

  const submit = () => {
    if (!canSubmit) return;
    onSubmit();
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-slate-950/70 backdrop-blur-md px-4 animate-fade"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div className="animate-rise panel-card relative w-full max-w-lg overflow-hidden rounded-3xl shadow-2xl shadow-black/60">
        <div className={`pointer-events-none absolute -top-24 left-1/2 h-48 w-[26rem] -translate-x-1/2 rounded-full bg-gradient-to-b ${t.glow} to-transparent blur-3xl`} />

        <div className="relative p-6 space-y-4">
          <div className="flex items-start gap-3">
            <span className={`grid place-items-center w-9 h-9 rounded-xl border shrink-0 ${t.tile}`}>
              <Icon className="w-4 h-4" />
            </span>
            <div className="min-w-0 flex-1">
              <h3 className="font-display text-base font-bold text-slate-100 flex items-center gap-2 flex-wrap">
                <span>{title}</span>
                {chip && (
                  <span className="text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-md bg-white/[0.06] border border-white/[0.1] text-slate-300">
                    {chip}
                  </span>
                )}
              </h3>
              {subtitle && <p className="mt-1 text-xs leading-relaxed text-slate-400">{subtitle}</p>}
            </div>
            <button
              onClick={onClose}
              disabled={busy}
              title="Close"
              className="icon-btn p-1.5 shrink-0"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          <textarea
            ref={textareaRef}
            rows={4}
            value={value}
            disabled={busy}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                e.preventDefault();
                submit();
              }
            }}
            placeholder={placeholder}
            className={`field ${t.field} text-sm p-3.5 leading-relaxed resize-none font-sans`}
          />

          <div className="flex items-center justify-between gap-3">
            <p className="text-[10px] font-mono text-slate-500 flex items-center gap-1.5 min-w-0">
              {hint && <span className="truncate">{hint}</span>}
              <span className="hidden sm:flex items-center gap-1 shrink-0">
                <Command className="w-3 h-3" />+Enter
              </span>
            </p>
            <div className="flex items-center gap-2 shrink-0">
              <button onClick={onClose} disabled={busy} className="btn btn-ghost px-3.5 py-2 text-xs">
                Cancel
              </button>
              <button onClick={submit} disabled={!canSubmit} className={`btn ${t.button} px-4 py-2 text-xs`}>
                {busy ? (
                  <>
                    <Wand2 className="w-4 h-4 animate-spin-slow" />
                    <span>{busyLabel}</span>
                  </>
                ) : (
                  <>
                    <Icon className="w-4 h-4" />
                    <span>{confirmLabel}</span>
                  </>
                )}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
