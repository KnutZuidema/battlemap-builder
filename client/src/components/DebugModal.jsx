import React, { useState } from 'react';
import { Terminal, Copy, Check, Trash2, X, Code2, Sparkles, Clock, MessageSquare, Layers, AlertTriangle } from 'lucide-react';

const PROVIDER_LABELS = { gemini: 'Gemini', openrouter: 'OpenRouter' };

export default function DebugModal({ isOpen, onClose, debugLogs, onClearLogs }) {
  const [selectedLogId, setSelectedLogId] = useState(null);
  const [copiedField, setCopiedField] = useState(null);

  if (!isOpen) return null;

  const activeLog = debugLogs.find((l) => l.id === selectedLogId) || debugLogs[0];

  const handleCopy = (text, fieldName) => {
    if (!text) return;
    navigator.clipboard.writeText(text);
    setCopiedField(fieldName);
    setTimeout(() => setCopiedField(null), 1500);
  };

  return (
    <div className="fixed inset-0 bg-slate-950/80 backdrop-blur-md flex items-center justify-center z-50 p-4 select-none">
      <div className="animate-rise bg-[#0a0e1a] border border-white/[0.09] rounded-2xl w-full max-w-5xl h-[88vh] shadow-2xl shadow-black/60 overflow-hidden flex flex-col">
        {/* Header */}
        <div className="px-6 py-3.5 border-b border-slate-800 flex items-center justify-between bg-slate-950/60">
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-lg bg-cyan-500/10 border border-cyan-500/30 text-cyan-400">
              <Terminal className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-base font-bold text-slate-100 flex items-center gap-2 font-display">
                <span>Prompt & Telemetry Inspector</span>
                <span className="text-xs font-mono font-normal text-slate-400 bg-slate-800 px-2 py-0.5 rounded-full">
                  {debugLogs.length} {debugLogs.length === 1 ? 'log' : 'logs'}
                </span>
              </h3>
              <p className="text-[11px] text-slate-400">Inspect defined system prompts, user input requests, full payloads, and model responses</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={onClearLogs}
              title="Clear Log History"
              className="p-1.5 px-2.5 rounded-lg bg-rose-950/30 border border-rose-800/40 text-rose-300 hover:bg-rose-900/50 text-xs font-medium flex items-center gap-1.5 transition"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>Clear</span>
            </button>
            <button
              onClick={onClose}
              className="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-white transition"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Main Content Area: Left Navigation + Right Detail Inspector */}
        <div className="flex-1 flex overflow-hidden">
          {/* Left Logs Navigation */}
          <div className="w-64 border-r border-slate-800 bg-slate-950/40 overflow-y-auto p-2 space-y-1 shrink-0">
            {debugLogs.length === 0 ? (
              <div className="p-4 text-center text-xs text-slate-500 italic">
                No generation logs recorded yet. Click "Generate Map" to capture telemetry!
              </div>
            ) : (
              debugLogs.map((log) => {
                const isSelected = (activeLog && activeLog.id === log.id);
                return (
                  <div
                    key={log.id}
                    onClick={() => setSelectedLogId(log.id)}
                    className={`p-2.5 rounded-xl border cursor-pointer transition ${
                      isSelected 
                        ? 'bg-amber-500/15 border-amber-500/40 text-amber-300 shadow-sm' 
                        : 'bg-slate-900/40 border-slate-800/60 text-slate-400 hover:bg-slate-800/60 hover:text-slate-200'
                    }`}
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="text-[10px] font-bold font-mono uppercase px-1.5 py-0.5 rounded bg-slate-800 text-slate-300">
                        {log.type}
                      </span>
                      <span className="text-[10px] font-mono text-slate-400 flex items-center gap-1">
                        <Clock className="w-3 h-3" />
                        {log.durationMs ? `${(log.durationMs / 1000).toFixed(2)}s` : 'fast'}
                      </span>
                    </div>
                    <div className="text-xs font-semibold text-slate-200 truncate">{log.prompt || 'Map Generation'}</div>
                    <div className="text-[10px] text-slate-500 mt-1 flex items-center justify-between font-mono">
                      <span className={log.status === 'failed' ? 'text-rose-400' : 'text-emerald-400'}>
                        {log.status === 'failed' ? 'FAILED · ' : ''}{log.modelUsed || 'unknown model'}
                      </span>
                      <span>{new Date(log.timestamp).toLocaleTimeString()}</span>
                    </div>
                  </div>
                );
              })
            )}
          </div>

          {/* Right Log Inspector */}
          <div className="flex-1 overflow-y-auto p-5 space-y-4 bg-slate-900/50 flex flex-col">
            {activeLog ? (
              <>
                {/* Meta summary pill */}
                <div className="flex flex-wrap items-center gap-3 p-3 rounded-xl bg-slate-950/70 border border-slate-800 text-xs shrink-0">
                  <div>
                    <span className="text-slate-500">Model:</span>{' '}
                    <strong className="text-amber-400 font-mono">{activeLog.modelUsed}</strong>
                  </div>
                  <div className="w-px h-4 bg-slate-800"></div>
                  <div>
                    <span className="text-slate-500">Latency:</span>{' '}
                    <strong className="text-cyan-400 font-mono">{activeLog.durationMs ? `${(activeLog.durationMs / 1000).toFixed(2)}s` : 'N/A'}</strong>
                  </div>
                  <div className="w-px h-4 bg-slate-800"></div>
                  <div>
                    <span className="text-slate-500">Action:</span>{' '}
                    <span className="text-emerald-400 font-mono font-bold">{activeLog.type}</span>
                  </div>
                  <div className="w-px h-4 bg-slate-800"></div>
                  <div>
                    <span className="text-slate-500">Timestamp:</span>{' '}
                    <span className="text-slate-300 font-mono">{new Date(activeLog.timestamp).toLocaleString()}</span>
                  </div>
                  <div className="w-px h-4 bg-slate-800"></div>
                  <div>
                    <span className="text-slate-500">Provider:</span>{' '}
                    <strong className={`font-mono ${activeLog.status === 'failed' ? 'text-rose-400' : 'text-emerald-400'}`}>
                      {activeLog.status === 'failed'
                        ? `FAILED (${PROVIDER_LABELS[activeLog.provider] || 'no map produced'})`
                        : (PROVIDER_LABELS[activeLog.provider] || 'UNKNOWN')}
                    </strong>
                  </div>
                  {activeLog.usage && (
                    <>
                      <div className="w-px h-4 bg-slate-800"></div>
                      <div>
                        <span className="text-slate-500">Tokens:</span>{' '}
                        <span className="text-slate-300 font-mono">
                          {activeLog.usage.promptTokens ?? '?'} in / {activeLog.usage.completionTokens ?? '?'} out
                        </span>
                      </div>
                      {typeof activeLog.usage.cost === 'number' && (
                        <>
                          <div className="w-px h-4 bg-slate-800"></div>
                          <div>
                            <span className="text-slate-500">Cost:</span>{' '}
                            <span className="text-slate-300 font-mono">${activeLog.usage.cost.toFixed(4)}</span>
                          </div>
                        </>
                      )}
                    </>
                  )}
                </div>

                {/* Failure reason + model attempt trail — a failed request never yields an image */}
                {activeLog.status === 'failed' && (
                  <div className="p-3 rounded-xl bg-rose-950/20 border border-rose-900/40 text-xs shrink-0 space-y-1.5">
                    <div className="flex items-center gap-1.5 font-bold text-rose-300">
                      <AlertTriangle className="w-3.5 h-3.5" />
                      <span>Model produced no map — request failed</span>
                    </div>
                    {activeLog.failureReason && (
                      <div className="font-mono text-rose-200/90 whitespace-pre-wrap break-words">{activeLog.failureReason}</div>
                    )}
                    {activeLog.attempts && activeLog.attempts.length > 0 && (
                      <ul className="font-mono text-[11px] text-slate-400 space-y-0.5">
                        {activeLog.attempts.map((a, idx) => (
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
                )}

                {/* Section 1: Defined System Prompt */}
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between">
                    <label className="text-xs font-bold text-amber-400 uppercase tracking-wider flex items-center gap-1.5">
                      <Sparkles className="w-3.5 h-3.5" />
                      <span>1. System Prompt (Defined Instructions)</span>
                    </label>
                    <button
                      onClick={() => handleCopy(activeLog.systemInstruction, 'sys')}
                      className="text-[11px] text-slate-400 hover:text-amber-400 flex items-center gap-1 transition"
                    >
                      {copiedField === 'sys' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copiedField === 'sys' ? 'Copied!' : 'Copy System Prompt'}</span>
                    </button>
                  </div>
                  <pre className="p-3.5 rounded-xl bg-slate-950 border border-slate-800/90 text-xs font-mono text-amber-200/90 whitespace-pre-wrap leading-relaxed max-h-40 overflow-y-auto select-text">
                    {activeLog.systemInstruction || 'No system prompt defined'}
                  </pre>
                </div>

                {/* Section 2: User Request Prompt */}
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between">
                    <label className="text-xs font-bold text-slate-300 uppercase tracking-wider flex items-center gap-1.5">
                      <MessageSquare className="w-3.5 h-3.5 text-cyan-400" />
                      <span>2. User Request Prompt</span>
                    </label>
                    <button
                      onClick={() => handleCopy(activeLog.userPrompt || activeLog.prompt, 'user')}
                      className="text-[11px] text-slate-400 hover:text-cyan-400 flex items-center gap-1 transition"
                    >
                      {copiedField === 'user' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copiedField === 'user' ? 'Copied!' : 'Copy User Prompt'}</span>
                    </button>
                  </div>
                  <pre className="p-3 rounded-xl bg-slate-950 border border-slate-800 text-xs font-mono text-slate-200 whitespace-pre-wrap leading-relaxed max-h-28 overflow-y-auto select-text">
                    {activeLog.userPrompt || activeLog.prompt || 'Standard battlemap prompt'}
                  </pre>
                </div>

                {/* Section 3: Full Payload Sent to Model */}
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between">
                    <label className="text-xs font-bold text-purple-300 uppercase tracking-wider flex items-center gap-1.5">
                      <Layers className="w-3.5 h-3.5 text-purple-400" />
                      <span>3. Full Combined Payload Sent to Model API</span>
                    </label>
                    <button
                      onClick={() => handleCopy(activeLog.fullPrompt || `${activeLog.systemInstruction}\n\n${activeLog.prompt}`, 'full')}
                      className="text-[11px] text-slate-400 hover:text-purple-300 flex items-center gap-1 transition"
                    >
                      {copiedField === 'full' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copiedField === 'full' ? 'Copied!' : 'Copy Full Payload'}</span>
                    </button>
                  </div>
                  <pre className="p-3 rounded-xl bg-slate-950 border border-slate-800 text-xs font-mono text-purple-200/90 whitespace-pre-wrap leading-relaxed max-h-36 overflow-y-auto select-text">
                    {activeLog.fullPrompt || `${activeLog.systemInstruction}\n\n[USER REQUEST]\n${activeLog.prompt}`}
                  </pre>
                </div>

                {/* Section 4: Raw Model Response Received */}
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between">
                    <label className="text-xs font-bold text-emerald-400 uppercase tracking-wider flex items-center gap-1.5">
                      <Code2 className="w-3.5 h-3.5 text-emerald-400" />
                      <span>4. Raw Model Response Received</span>
                    </label>
                    <button
                      onClick={() => handleCopy(activeLog.rawResponseText, 'response')}
                      className="text-[11px] text-slate-400 hover:text-emerald-400 flex items-center gap-1 transition"
                    >
                      {copiedField === 'response' ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                      <span>{copiedField === 'response' ? 'Copied!' : 'Copy Response'}</span>
                    </button>
                  </div>
                  <pre className="p-3.5 rounded-xl bg-slate-950 border border-slate-800 text-xs font-mono text-emerald-300/90 whitespace-pre-wrap leading-relaxed max-h-48 overflow-y-auto select-text">
                    {activeLog.rawResponseText || 'No raw output returned'}
                  </pre>
                </div>
              </>
            ) : (
              <div className="h-full flex items-center justify-center text-slate-500 text-xs italic">
                Select a log on the left to inspect defined prompts and model responses.
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

