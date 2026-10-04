import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { X, Wand2, Command } from 'lucide-react';

const TONES = {
  cyan: {
    tile: 'text-cyan-300 bg-cyan-500/12 border-cyan-500/25',
    glow: 'from-cyan-500/25',
    field: 'field-cyan',
    button: 'btn-cyan',
    edge: 'border-cyan-500/35',
    arrow: 'border-cyan-500/35'
  },
  emerald: {
    tile: 'text-emerald-300 bg-emerald-500/12 border-emerald-500/25',
    glow: 'from-emerald-500/25',
    field: 'field-emerald',
    button: 'btn-emerald',
    edge: 'border-emerald-500/35',
    arrow: 'border-emerald-500/35'
  }
};

const POPOVER_WIDTH = 340;
const POPOVER_MARGIN = 12;
const POPOVER_GAP = 14;

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));

/**
 * Places the popover relative to the anchored region (all coordinates are in the canvas container's
 * pixel space). It takes the first of the anchor's acceptable sides ('above' / 'below') that fits in
 * the container, else sits inside the region, so it never leaves the visible canvas.
 */
function placePopover(anchor, size) {
  const { rect, bounds, sides = ['above', 'below'], align = 'center' } = anchor;
  const { w, h } = size;

  let left;
  if (align === 'start') left = rect.left;
  else if (align === 'end') left = rect.left + rect.width - w;
  else left = rect.left + rect.width / 2 - w / 2;
  left = clamp(left, POPOVER_MARGIN, Math.max(POPOVER_MARGIN, bounds.width - POPOVER_MARGIN - w));

  const aboveTop = rect.top - POPOVER_GAP - h;
  const belowTop = rect.top + rect.height + POPOVER_GAP;
  const fits = {
    above: aboveTop >= POPOVER_MARGIN,
    below: belowTop + h <= bounds.height - POPOVER_MARGIN
  };
  const placement = sides.find((side) => fits[side]) || 'inside';

  let top;
  if (placement === 'above') top = aboveTop;
  else if (placement === 'below') top = belowTop;
  else top = clamp(rect.top + POPOVER_MARGIN, POPOVER_MARGIN, Math.max(POPOVER_MARGIN, bounds.height - POPOVER_MARGIN - h));

  // The arrow points at the visible part of the region.
  const visibleLeft = Math.max(rect.left, 0);
  const visibleRight = Math.min(rect.left + rect.width, bounds.width);
  const targetX = visibleRight > visibleLeft ? (visibleLeft + visibleRight) / 2 : rect.left + rect.width / 2;
  const arrowX = clamp(targetX - left, 22, w - 22);

  return { left, top, placement, arrowX };
}

/**
 * Prompt UI for region forging: the canvas decides *where* (the selected region)
 * and this collects the *what*. Escape closes, Cmd/Ctrl+Enter submits.
 *
 * Without `anchor` it is a centered modal over a dimmed backdrop. With `anchor` it is a compact
 * popover that floats beside a region of the canvas — no backdrop, so the map stays fully visible —
 * and follows that region as the canvas is panned or zoomed.
 * anchor = { rect: {left, top, width, height}, bounds: {width, height}, sides?: ('above'|'below')[], align?: 'center'|'start'|'end' }
 */
export default function ActionModal({
  open,
  tone = 'cyan',
  icon: Icon,
  title = 'Forge Region',
  subtitle = 'Modify details or extend map boundaries on a 64px grid',
  chip,
  placeholder,
  value,
  onChange,
  onSubmit,
  onClose,
  busy = false,
  busyLabel = 'Working…',
  confirmLabel = 'Forge Region',
  hint,
  allowEmpty = false,
  anchor = null
}) {
  const textareaRef = useRef(null);
  const popoverRef = useRef(null);
  const [popoverSize, setPopoverSize] = useState({ w: POPOVER_WIDTH, h: 220 });
  const t = TONES[tone] || TONES.cyan;
  const canSubmit = !busy && (allowEmpty || Boolean(value && value.trim()));
  const floating = Boolean(anchor);

  const popoverWidth = anchor ? Math.min(POPOVER_WIDTH, Math.max(240, anchor.bounds.width - POPOVER_MARGIN * 2)) : POPOVER_WIDTH;

  useEffect(() => {
    if (!open || busy) return;
    textareaRef.current?.focus({ preventScroll: true });
  }, [open, busy]);

  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (e) => {
      if (e.key === 'Escape' && !busy) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [open, busy, onClose]);

  // Measure the popover so it can be placed above/below the region without guessing its height.
  useLayoutEffect(() => {
    if (!floating || !open || !popoverRef.current) return;
    const h = popoverRef.current.offsetHeight;
    setPopoverSize((prev) => (Math.abs(prev.h - h) > 0.5 || Math.abs(prev.w - popoverWidth) > 0.5 ? { w: popoverWidth, h } : prev));
  });

  if (!open) return null;

  const submit = () => {
    if (!canSubmit) return;
    onSubmit();
  };

  const body = (
    <div className={`relative ${floating ? 'p-4 space-y-3' : 'p-6 space-y-4'}`}>
      <div className="flex items-start gap-3">
        <span className={`grid place-items-center ${floating ? 'w-8 h-8' : 'w-9 h-9'} rounded-xl border shrink-0 ${t.tile}`}>
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
          {subtitle && !floating && <p className="mt-1 text-xs leading-relaxed text-slate-400">{subtitle}</p>}
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
        rows={floating ? 3 : 4}
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
          {hint && !floating && <span className="truncate">{hint}</span>}
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
  );

  if (floating) {
    const { left, top, placement, arrowX } = placePopover(anchor, { w: popoverWidth, h: popoverSize.h });
    return (
      <div
        ref={popoverRef}
        role="dialog"
        aria-label={title}
        className={`absolute z-40 rounded-2xl border ${t.edge} bg-slate-950/92 backdrop-blur-xl shadow-2xl shadow-black/60 animate-fade select-text`}
        style={{ left, top, width: popoverWidth }}
        onMouseDown={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.stopPropagation()}
      >
        {placement !== 'inside' && (
          <span
            className={`pointer-events-none absolute w-3 h-3 rotate-45 bg-slate-950 ${t.arrow} ${placement === 'above' ? 'border-b border-r -bottom-1.5' : 'border-t border-l -top-1.5'}`}
            style={{ left: arrowX - 6 }}
          />
        )}
        {body}
      </div>
    );
  }

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-slate-950/70 backdrop-blur-md px-4 animate-fade"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div className="animate-rise panel-card relative w-full max-w-lg overflow-hidden rounded-3xl shadow-2xl shadow-black/60">
        <div className={`pointer-events-none absolute -top-24 left-1/2 h-48 w-[26rem] -translate-x-1/2 rounded-full bg-gradient-to-b ${t.glow} to-transparent blur-3xl`} />
        {body}
      </div>
    </div>
  );
}
