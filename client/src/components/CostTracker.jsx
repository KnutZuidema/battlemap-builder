import React, { useState } from 'react';
import { Coins, ChevronDown, ArrowDownLeft, ArrowUpRight, Hash } from 'lucide-react';
import { formatCost, formatTokens } from '../utils/format';

export default function CostTracker({ mapCost }) {
  const [isOpen, setIsOpen] = useState(false);

  const cost = mapCost || {};
  const totalCost = Number(cost.totalCost) || 0;
  const promptTokens = Number(cost.promptTokens) || 0;
  const completionTokens = Number(cost.completionTokens) || 0;
  const totalTokens = Number(cost.totalTokens) || (promptTokens + completionTokens);
  const promptCost = Number(cost.promptCost) || 0;
  const completionCost = Number(cost.completionCost) || 0;
  const requestsCount = Number(cost.requestsCount) || 0;

  return (
    <div className="relative select-none" onMouseLeave={() => setIsOpen(false)}>
      <button
        type="button"
        onClick={() => setIsOpen((prev) => !prev)}
        onMouseEnter={() => setIsOpen(true)}
        aria-label="Map Generation Cost Tracker"
        title="Total cost of this battlemap so far (click to inspect input and output costs)"
        className={`flex items-center gap-2 px-2.5 py-1.5 rounded-xl border transition-all ${
          isOpen
            ? 'bg-amber-500/20 border-amber-500/40 text-amber-200 shadow-sm shadow-amber-950/50'
            : 'bg-black/35 hover:bg-black/50 border-white/[0.08] hover:border-amber-400/30 text-slate-300 hover:text-amber-200'
        }`}
      >
        <div className="flex items-center justify-center w-5 h-5 rounded-lg bg-amber-500/15 border border-amber-400/30 text-amber-400">
          <Coins className="w-3 h-3" />
        </div>

        <div className="flex flex-col items-start leading-none">
          <span className="text-[8px] font-mono uppercase tracking-wider text-slate-400">Map Cost</span>
          <span className="text-xs font-mono font-bold text-amber-300">
            {formatCost(totalCost, 4)}
          </span>
        </div>

        <ChevronDown className={`w-3 h-3 text-slate-500 transition-transform ${isOpen ? 'rotate-180 text-amber-400' : ''}`} />
      </button>

      {/* Popover Breakdown Card */}
      {isOpen && (
        <div className="animate-rise absolute right-0 top-full mt-2 w-72 p-3.5 rounded-2xl bg-slate-950/95 backdrop-blur-xl border border-white/[0.1] shadow-2xl shadow-black/80 z-50 text-xs">
          {/* Header */}
          <div className="flex items-center justify-between border-b border-white/[0.08] pb-2.5 mb-2.5">
            <div className="flex items-center gap-1.5 text-amber-300 font-semibold">
              <Coins className="w-4 h-4 text-amber-400" />
              <span>Map Cost Tracker</span>
            </div>
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-amber-500/10 border border-amber-500/30 text-amber-300 font-bold">
              {requestsCount} {requestsCount === 1 ? 'call' : 'calls'}
            </span>
          </div>

          <p className="text-[11px] text-slate-400 mb-3 leading-relaxed">
            Cumulative cost for this battlemap so far, including both input and output token costs.
          </p>

          {/* Token & Cost Breakdown Rows */}
          <div className="space-y-2 font-mono">
            {/* Input Tokens */}
            <div className="p-2 rounded-xl bg-white/[0.03] border border-white/[0.05] flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <ArrowDownLeft className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
                <div>
                  <div className="text-[10px] text-slate-400">Input Tokens</div>
                  <div className="text-xs text-slate-200">{formatTokens(promptTokens)}</div>
                </div>
              </div>
              <div className="text-right">
                <div className="text-[10px] text-slate-500">Input Cost</div>
                <div className="text-xs font-bold text-cyan-300">{formatCost(promptCost, 5)}</div>
              </div>
            </div>

            {/* Output Tokens */}
            <div className="p-2 rounded-xl bg-white/[0.03] border border-white/[0.05] flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <ArrowUpRight className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                <div>
                  <div className="text-[10px] text-slate-400">Output Tokens</div>
                  <div className="text-xs text-slate-200">{formatTokens(completionTokens)}</div>
                </div>
              </div>
              <div className="text-right">
                <div className="text-[10px] text-slate-500">Output Cost</div>
                <div className="text-xs font-bold text-emerald-300">{formatCost(completionCost, 5)}</div>
              </div>
            </div>

            {/* Total Cost Summary */}
            <div className="pt-2 border-t border-white/[0.08] flex items-center justify-between text-slate-200">
              <div className="flex items-center gap-1 text-[11px] text-slate-400 font-sans">
                <Hash className="w-3 h-3 text-slate-500" />
                <span>Total:</span>
                <span className="font-mono text-slate-300">{formatTokens(totalTokens)} tok</span>
              </div>
              <div className="text-right">
                <span className="text-[10px] text-slate-400 mr-1.5">Total Cost:</span>
                <span className="text-sm font-bold text-amber-400">{formatCost(totalCost, 4)}</span>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
