import React, { useState, useEffect, useMemo } from 'react';
import {
  X,
  Search,
  BookOpen,
  ChevronRight,
  ChevronDown,
  MapPin,
  Compass,
  Scroll,
  Sparkles,
  Wand2,
  Copy,
  Check,
  ArrowLeft,
  AlertTriangle,
  RotateCcw,
  Shield,
  Layers,
  Flame,
  CornerDownRight,
  ExternalLink
} from 'lucide-react';
import {
  fetchWikiTreeApi,
  fetchWikiArticleApi,
  suggestWikiPromptsApi,
  generateCustomWikiPromptApi
} from '../services/api';
export default function WikiDrawer({
  isOpen,
  onClose,
  onSelectPrompt,
  activeWikiContext = null,
  onSetActiveWikiContext = null
}) {
  const [tree, setTree] = useState(null);
  const [treeLoading, setTreeLoading] = useState(false);
  const [treeError, setTreeError] = useState(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [expandedRegions, setExpandedRegions] = useState({});
  const [adventuresOpen, setAdventuresOpen] = useState(true);

  // Article View state
  const [selectedArticlePath, setSelectedArticlePath] = useState(null);
  const [article, setArticle] = useState(null);
  const [articleLoading, setArticleLoading] = useState(false);
  const [articleError, setArticleError] = useState(null);
  const [activeTab, setActiveTab] = useState('forge'); // 'forge' | 'lore'

  // Prompts & Translation state
  const [customIntent, setCustomIntent] = useState('');
  const [isSuggesting, setIsSuggesting] = useState(false);
  const [isTranslatingCustom, setIsTranslatingCustom] = useState(false);
  const [promptResults, setPromptResults] = useState([]);
  const [lastUsage, setLastUsage] = useState(null);
  const [promptError, setPromptError] = useState(null);
  const [copiedIdx, setCopiedIdx] = useState(null);

  const filteredCustom = useMemo(() => {
    if (!tree?.custom) return [];
    if (!searchQuery.trim()) return tree.custom;
    const q = searchQuery.toLowerCase();
    return tree.custom.filter((c) => c.name.toLowerCase().includes(q) || c.path.toLowerCase().includes(q));
  }, [tree, searchQuery]);

  // Load tree on open if not loaded
  useEffect(() => {
    if (isOpen && !tree && !treeLoading) {
      loadTree();
    }
  }, [isOpen, tree, treeLoading]);

  // Keyboard escape listener
  useEffect(() => {
    const handleKeyDown = (e) => {
      if (e.key === 'Escape' && isOpen) {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  // Load tree function
  const loadTree = async () => {
    setTreeLoading(true);
    setTreeError(null);
    try {
      const data = await fetchWikiTreeApi();
      setTree(data);
      // Default expand first 2 regions
      if (data?.regions?.length) {
        setExpandedRegions({
          [data.regions[0].id]: true,
          [data.regions[1]?.id]: true
        });
      }
    } catch (err) {
      console.error('Failed to load wiki tree:', err);
      setTreeError(err.message || 'Failed to connect to Wiki directory');
    } finally {
      setTreeLoading(false);
    }
  };

  // Load selected article
  const loadArticle = async (path) => {
    setSelectedArticlePath(path);
    setArticleLoading(true);
    setArticleError(null);
    setPromptResults([]);
    setPromptError(null);
    setCustomIntent('');

    try {
      const data = await fetchWikiArticleApi(path);
      setArticle(data);
      setActiveTab('forge');
    } catch (err) {
      console.error('Failed to load wiki article:', err);
      setArticleError(err.message || 'Failed to read article');
    } finally {
      setArticleLoading(false);
    }
  };

  // Toggle region accordion
  const toggleRegion = (id) => {
    setExpandedRegions((prev) => ({
      ...prev,
      [id]: !prev[id]
    }));
  };

  // Filtered regions and adventures based on search
  const { filteredRegions, filteredAdventures } = useMemo(() => {
    if (!tree) return { filteredRegions: [], filteredAdventures: [] };
    const q = searchQuery.trim().toLowerCase();
    if (!q) {
      return {
        filteredRegions: tree.regions || [],
        filteredAdventures: tree.adventures || []
      };
    }

    const reg = (tree.regions || [])
      .map((r) => {
        const matchesRegionName = r.name.toLowerCase().includes(q);
        const matchingPlaces = (r.places || []).filter(
          (p) => matchesRegionName || p.name.toLowerCase().includes(q)
        );
        const matchingSettlements = (r.settlements || []).filter(
          (s) => matchesRegionName || s.name.toLowerCase().includes(q)
        );
        if (matchesRegionName || matchingPlaces.length || matchingSettlements.length) {
          return {
            ...r,
            places: matchingPlaces,
            settlements: matchingSettlements
          };
        }
        return null;
      })
      .filter(Boolean);

    const adv = (tree.adventures || [])
      .map((a) => {
        const matchesAdvName = a.name.toLowerCase().includes(q);
        const matchingChapters = (a.chapters || []).filter(
          (c) => matchesAdvName || c.name.toLowerCase().includes(q)
        );
        if (matchesAdvName || matchingChapters.length) {
          return {
            ...a,
            chapters: matchingChapters
          };
        }
        return null;
      })
      .filter(Boolean);

    return { filteredRegions: reg, filteredAdventures: adv };
  }, [tree, searchQuery]);

  // Handle auto-expanding regions when searching
  useEffect(() => {
    if (searchQuery.trim() && filteredRegions.length) {
      const nextExp = {};
      filteredRegions.forEach((r) => {
        nextExp[r.id] = true;
      });
      setExpandedRegions(nextExp);
      setAdventuresOpen(true);
    }
  }, [searchQuery, filteredRegions]);

  // Suggest 3 concepts
  const handleSuggest = async () => {
    if (!selectedArticlePath) return;
    setIsSuggesting(true);
    setPromptError(null);
    try {
      const res = await suggestWikiPromptsApi(selectedArticlePath);
      setPromptResults(res.suggestions || []);
      setLastUsage(res.usage || null);
    } catch (err) {
      console.error('Failed to suggest prompts:', err);
      setPromptError(err.message || 'Failed to generate battlemap concepts');
    } finally {
      setIsSuggesting(false);
    }
  };

  // Generate custom tailored prompt
  const handleCustomTranslate = async (e) => {
    e?.preventDefault();
    if (!selectedArticlePath || !customIntent.trim()) return;
    setIsTranslatingCustom(true);
    setPromptError(null);
    try {
      const res = await generateCustomWikiPromptApi(selectedArticlePath, customIntent.trim());
      if (res.prompt) {
        setPromptResults((prev) => [res.prompt, ...prev]);
        setLastUsage(res.usage || null);
      }
    } catch (err) {
      console.error('Failed to generate custom prompt:', err);
      setPromptError(err.message || 'Failed to translate custom encounter');
    } finally {
      setIsTranslatingCustom(false);
    }
  };

  // Copy prompt helper
  const handleCopyPrompt = async (text, idx) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedIdx(idx);
      setTimeout(() => setCopiedIdx(null), 2000);
    } catch (err) {
      console.error('Failed to copy prompt:', err);
    }
  };

  // Navigate from wikilinks
  const handleNavigateWikiLink = (target) => {
    if (!tree) return;
    const clean = target.replace(/\\/g, '/');

    // 1. Direct path match
    for (const r of tree.regions) {
      for (const p of r.places) {
        if (p.path.endsWith(clean) || p.name.toLowerCase() === clean.toLowerCase()) {
          loadArticle(p.path);
          return;
        }
      }
      for (const s of r.settlements) {
        if (s.path.endsWith(clean) || s.name.toLowerCase() === clean.toLowerCase()) {
          loadArticle(s.path);
          return;
        }
      }
    }
    for (const a of tree.adventures) {
      if (a.path.endsWith(clean) || a.name.toLowerCase() === clean.toLowerCase()) {
        loadArticle(a.path);
        return;
      }
      for (const c of a.chapters) {
        if (c.path.endsWith(clean) || c.name.toLowerCase() === clean.toLowerCase()) {
          loadArticle(c.path);
          return;
        }
      }
    }
    if (tree.custom) {
      for (const cu of tree.custom) {
        if (cu.path.endsWith(clean) || cu.name.toLowerCase() === clean.toLowerCase()) {
          loadArticle(cu.path);
          return;
        }
      }
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/65 backdrop-blur-sm transition-opacity"
        onClick={onClose}
      />

      {/* Slide-over Drawer Panel */}
      <div className="relative w-full max-w-2xl h-full bg-[#0a0f1d]/95 border-l border-white/10 shadow-2xl flex flex-col backdrop-blur-2xl z-10 overflow-hidden select-text">
        {/* Drawer Header */}
        <header className="h-[3.75rem] px-5 border-b border-white/[0.08] flex items-center justify-between shrink-0 bg-black/30">
          <div className="flex items-center gap-2.5">
            <div className="p-1.5 rounded-lg bg-amber-500/15 border border-amber-500/30 text-amber-300">
              <BookOpen size={17} />
            </div>
            <div>
              <h2 className="text-sm font-bold font-display text-white tracking-wide flex items-center gap-2">
                <span>World Wiki & Battlemaps</span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-amber-500/15 text-amber-300 border border-amber-500/30 font-medium">
                  Umbraen Lore
                </span>
              </h2>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="icon-btn p-1.5 hover:text-white hover:bg-white/10"
              title="Close (Esc)"
            >
              <X size={18} />
            </button>
          </div>
        </header>

        {/* Search Bar & Subnav */}
        <div className="px-5 py-3 border-b border-white/[0.06] bg-black/15 flex items-center gap-3 shrink-0">
          <div className="relative flex-1">
            <Search
              size={14}
              className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-500"
            />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search places, settlements, adventures..."
              className="field text-xs pl-8 pr-7 py-2 bg-black/40 font-sans"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-white"
              >
                <X size={12} />
              </button>
            )}
          </div>

          {selectedArticlePath && (
            <button
              type="button"
              onClick={() => setSelectedArticlePath(null)}
              className="btn btn-ghost px-2.5 py-1.5 text-xs text-slate-300 hover:text-white shrink-0"
              title="Return to region and adventure list"
            >
              <ArrowLeft size={13} />
              <span>Locations</span>
            </button>
          )}
        </div>

        {/* Drawer Body */}
        <div className="flex-1 overflow-y-auto min-h-0">
          {treeLoading ? (
            <div className="p-12 flex flex-col items-center justify-center text-center">
              <div className="w-8 h-8 rounded-full border-2 border-amber-400/30 border-t-amber-400 animate-spin mb-3" />
              <p className="text-xs text-slate-400 font-mono">Scanning campaign wiki locations…</p>
            </div>
          ) : treeError ? (
            <div className="m-5 p-4 rounded-xl border border-rose-500/30 bg-rose-950/30 text-rose-200 text-xs">
              <div className="flex items-center gap-2 font-semibold mb-1">
                <AlertTriangle size={15} className="text-rose-400" />
                <span>Wiki Connection Notice</span>
              </div>
              <p className="text-slate-300 mt-1">{treeError}</p>
              <button
                onClick={loadTree}
                className="btn btn-ghost mt-3 text-xs px-3 py-1 text-slate-200"
              >
                <RotateCcw size={12} />
                <span>Retry Connection</span>
              </button>
            </div>
          ) : selectedArticlePath ? (
            /* =======================================================
               ARTICLE VIEW: Tabs for Forge & Prompts / Lore & Reference
               ======================================================= */
            <div className="p-5 space-y-5">
              {/* Breadcrumb Header */}
              <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-3 border-b border-white/[0.08]">
                <div className="flex items-center gap-2 text-xs text-slate-400 min-w-0">
                  <button
                    onClick={() => setSelectedArticlePath(null)}
                    className="hover:text-amber-300 transition-colors"
                  >
                    Locations
                  </button>
                  <ChevronRight size={12} className="text-slate-600 shrink-0" />
                  {article?.parentRegion && (
                    <>
                      <span className="text-slate-300 truncate">{article.parentRegion.name}</span>
                      <ChevronRight size={12} className="text-slate-600 shrink-0" />
                    </>
                  )}
                  <span className="text-amber-300 font-semibold truncate max-w-[200px]">
                    {article?.title || 'Loading…'}
                  </span>
                </div>

                <div className="flex items-center gap-2 shrink-0">
                  {/* Active Context Toggle Button */}
                  {onSetActiveWikiContext && (
                    <button
                      type="button"
                      onClick={() => {
                        const isAlreadyActive = activeWikiContext?.path === selectedArticlePath;
                        if (isAlreadyActive) {
                          onSetActiveWikiContext(null);
                        } else {
                          onSetActiveWikiContext({ path: selectedArticlePath, title: article?.title });
                        }
                      }}
                      className={`px-2.5 py-1 text-xs font-semibold rounded-lg border transition-all flex items-center gap-1.5 ${
                        activeWikiContext?.path === selectedArticlePath
                          ? 'bg-amber-500/25 text-amber-200 border-amber-500/40 shadow-sm'
                          : 'text-slate-400 hover:text-amber-200 hover:bg-amber-500/10 border-white/[0.08]'
                      }`}
                      title={activeWikiContext?.path === selectedArticlePath ? 'Currently active as map context. Click to unlink.' : 'Add this article as context to the current map'}
                    >
                      <BookOpen size={12} className={activeWikiContext?.path === selectedArticlePath ? 'text-amber-400' : 'text-slate-400'} />
                      <span>{activeWikiContext?.path === selectedArticlePath ? 'Active Context' : 'Add as Map Context'}</span>
                      {activeWikiContext?.path === selectedArticlePath && <Check size={11} className="text-emerald-400" />}
                    </button>
                  )}

                  {/* Tabs Switcher */}
                  <div className="flex items-center bg-black/40 p-1 rounded-xl border border-white/[0.08]">
                    <button
                      onClick={() => setActiveTab('forge')}
                      className={`px-3 py-1 text-xs font-semibold rounded-lg transition-all flex items-center gap-1.5 ${
                        activeTab === 'forge'
                          ? 'bg-amber-500/20 text-amber-300 border border-amber-500/40 shadow-sm'
                          : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      <Wand2 size={12} />
                      <span>Forge</span>
                    </button>
                    <button
                      onClick={() => setActiveTab('lore')}
                      className={`px-3 py-1 text-xs font-semibold rounded-lg transition-all flex items-center gap-1.5 ${
                        activeTab === 'lore'
                          ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40 shadow-sm'
                          : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      <BookOpen size={12} />
                      <span>Lore</span>
                    </button>
                  </div>
                </div>
              </div>

              {articleLoading ? (
                <div className="py-16 text-center">
                  <div className="w-8 h-8 rounded-full border-2 border-amber-400/30 border-t-amber-400 animate-spin mx-auto mb-3" />
                  <p className="text-xs text-slate-400 font-mono">Reading location lore & tactical data…</p>
                </div>
              ) : articleError ? (
                <div className="p-4 rounded-xl border border-rose-500/30 bg-rose-950/30 text-rose-200 text-xs">
                  <div className="flex items-center gap-2 font-semibold mb-1">
                    <AlertTriangle size={15} />
                    <span>Failed to load article</span>
                  </div>
                  <p>{articleError}</p>
                </div>
              ) : activeTab === 'lore' ? (
                /* TAB 2: Lore & Reference */
                <div>
                  <WikiArticleViewer
                    article={article}
                    onNavigateArticle={handleNavigateWikiLink}
                  />
                </div>
              ) : (
                /* TAB 1: Forge & Prompts */
                <div className="space-y-6">
                  {/* Action Banner */}
                  <div className="panel-card p-4 bg-gradient-to-br from-amber-500/10 via-transparent to-transparent border-amber-500/25">
                    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                      <div>
                        <h3 className="text-sm font-bold text-white flex items-center gap-2">
                          <Sparkles size={15} className="text-amber-400" />
                          <span>AI Battlemap Translation</span>
                        </h3>
                        <p className="text-xs text-slate-400 mt-1">
                          Translates tactical rooms and encounters into 2D orthographic prompts.
                        </p>
                      </div>

                      <button
                        onClick={handleSuggest}
                        disabled={isSuggesting || isTranslatingCustom}
                        className="btn btn-primary px-3.5 py-2 text-xs shrink-0"
                      >
                        {isSuggesting ? (
                          <>
                            <Wand2 size={13} className="animate-spin-slow" />
                            <span>Translating Lore…</span>
                          </>
                        ) : (
                          <>
                            <Sparkles size={13} />
                            <span>Suggest 3 Concepts</span>
                          </>
                        )}
                      </button>
                    </div>

                    {/* Custom Intent Translator Form */}
                    <form onSubmit={handleCustomTranslate} className="mt-4 pt-3 border-t border-white/[0.08]">
                      <label className="block text-[11px] font-semibold text-slate-300 uppercase tracking-wider mb-1.5">
                        Specific Room / Encounter Area
                      </label>
                      <div className="flex gap-2">
                        <input
                          type="text"
                          value={customIntent}
                          onChange={(e) => setCustomIntent(e.target.value)}
                          placeholder="e.g. Flooded cellar with broken iron gate leading to sea caves..."
                          className="field text-xs px-3 py-2 bg-black/40 font-sans flex-1"
                        />
                        <button
                          type="submit"
                          disabled={!customIntent.trim() || isTranslatingCustom || isSuggesting}
                          className="btn btn-cyan px-3.5 py-2 text-xs shrink-0"
                        >
                          {isTranslatingCustom ? (
                            <>
                              <Wand2 size={13} className="animate-spin-slow" />
                              <span>Forging…</span>
                            </>
                          ) : (
                            <>
                              <Wand2 size={13} />
                              <span>Translate</span>
                            </>
                          )}
                        </button>
                      </div>
                    </form>
                  </div>

                  {/* Prompt Error Banner */}
                  {promptError && (
                    <div className="p-3.5 rounded-xl border border-rose-500/40 bg-rose-950/40 text-rose-200 text-xs flex items-start gap-2.5">
                      <AlertTriangle size={15} className="text-rose-400 shrink-0 mt-0.5" />
                      <div>
                        <div className="font-semibold text-rose-300">Prompt Generation Error</div>
                        <div className="mt-0.5 text-slate-300">{promptError}</div>
                      </div>
                    </div>
                  )}

                  {/* Results List */}
                  {promptResults.length > 0 && (
                    <div className="space-y-4">
                      <div className="flex items-center justify-between text-xs text-slate-400 pb-1 border-b border-white/[0.06]">
                        <span className="font-semibold text-slate-200">
                          Ready-to-Forge Prompts ({promptResults.length})
                        </span>
                        {lastUsage?.cost ? (
                          <span className="font-mono text-[11px] text-amber-400/90">
                            Est. translation cost: ${lastUsage.cost.toFixed(5)}
                          </span>
                        ) : null}
                      </div>

                      {promptResults.map((item, idx) => (
                        <div
                          key={idx}
                          className="panel-card p-4 border-white/10 hover:border-amber-500/40 transition-all bg-black/30 space-y-3"
                        >
                          {/* Card Header: Title & Badges */}
                          <div className="flex items-start justify-between gap-3">
                            <div>
                              <h4 className="text-sm font-bold text-white flex items-center gap-2">
                                <span>{item.title}</span>
                              </h4>
                              <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
                                <span className="text-[10px] font-semibold px-2 py-0.5 rounded-md bg-amber-500/15 text-amber-300 border border-amber-500/30">
                                  {item.encounterType || 'Encounter'}
                                </span>
                                <span className="text-[10px] font-mono px-2 py-0.5 rounded-md bg-white/5 text-slate-300 border border-white/10">
                                  Grid: {item.gridSize || '20x20'}
                                </span>
                              </div>
                            </div>

                            <button
                              onClick={() =>
                                onSelectPrompt({
                                  title: item.title,
                                  prompt: item.imagePrompt,
                                  usage: lastUsage,
                                  articleTitle: article?.title,
                                  articlePath: selectedArticlePath
                                })
                              }
                              className="btn btn-primary px-3 py-1.5 text-xs font-semibold shrink-0 shadow-lg"
                              title="Populate prompt and set map title for generation"
                            >
                              <Sparkles size={12} />
                              <span>Use Prompt</span>
                            </button>
                          </div>

                          {/* Tactical Highlights */}
                          {item.tacticalHighlights?.length > 0 && (
                            <div className="space-y-1">
                              <span className="text-[10px] font-semibold uppercase tracking-wider text-slate-400">
                                Tactical Highlights
                              </span>
                              <div className="flex flex-wrap gap-1.5">
                                {item.tacticalHighlights.map((hl, hIdx) => (
                                  <span
                                    key={hIdx}
                                    className="text-[11px] px-2 py-0.5 rounded bg-white/5 text-slate-300 border border-white/10"
                                  >
                                    {hl}
                                  </span>
                                ))}
                              </div>
                            </div>
                          )}

                          {/* Image Prompt Box */}
                          <div className="relative">
                            <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 mb-1 flex items-center justify-between">
                              <span>Image Generator Prompt</span>
                              <button
                                type="button"
                                onClick={() => handleCopyPrompt(item.imagePrompt, idx)}
                                className="inline-flex items-center gap-1 text-[10px] text-slate-400 hover:text-white"
                              >
                                {copiedIdx === idx ? (
                                  <>
                                    <Check size={11} className="text-emerald-400" />
                                    <span className="text-emerald-300">Copied</span>
                                  </>
                                ) : (
                                  <>
                                    <Copy size={11} />
                                    <span>Copy Text</span>
                                  </>
                                )}
                              </button>
                            </div>
                            <div className="p-3 rounded-lg bg-black/50 border border-white/10 font-sans text-xs text-slate-300 leading-relaxed max-h-36 overflow-y-auto select-all">
                              {item.imagePrompt}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          ) : (
            /* =======================================================
               NAVIGATION VIEW: Region Accordions & Adventures List
               ======================================================= */
            <div className="p-5 space-y-5">
              {/* Intro Banner */}
              <div className="panel-card p-4 border-amber-500/20 bg-amber-500/5 flex items-start gap-3">
                <Compass className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />
                <div className="text-xs leading-relaxed text-slate-300">
                  <span className="font-semibold text-white">Campaign World Wiki:</span> Browse
                  tactical locations across <strong className="text-amber-300">Umbraen</strong>.
                  Select any place, settlement, or dungeon chapter to view lore, read-aloud notes,
                  and extract ready-to-forge 2D battlemap prompts.
                </div>
              </div>

              {/* Regions Section */}
              <div className="space-y-3">
                <div className="flex items-center justify-between text-xs text-slate-400 font-semibold uppercase tracking-wider">
                  <span>Regions ({filteredRegions.length})</span>
                </div>

                <div className="space-y-2">
                  {filteredRegions.map((region) => {
                    const isExpanded = !!expandedRegions[region.id];
                    const placesCount = region.places?.length || 0;
                    const settlementsCount = region.settlements?.length || 0;
                    const totalLocs = placesCount + settlementsCount;

                    return (
                      <div
                        key={region.id}
                        className="rounded-xl border border-white/[0.08] bg-black/25 overflow-hidden transition-colors hover:border-white/[0.14]"
                      >
                        {/* Region Accordion Header */}
                        <button
                          type="button"
                          onClick={() => toggleRegion(region.id)}
                          className="w-full px-4 py-3 flex items-center justify-between gap-3 text-left hover:bg-white/[0.03] transition-colors"
                        >
                          <div className="flex items-center gap-2.5">
                            {isExpanded ? (
                              <ChevronDown size={15} className="text-amber-400" />
                            ) : (
                              <ChevronRight size={15} className="text-slate-500" />
                            )}
                            <div>
                              <div className="text-sm font-semibold text-white">
                                {region.name}
                              </div>
                              <div className="text-[11px] text-slate-400 flex items-center gap-2 mt-0.5">
                                <span>{placesCount} places</span>
                                <span>·</span>
                                <span>{settlementsCount} settlements</span>
                              </div>
                            </div>
                          </div>

                          <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-white/5 text-slate-300 border border-white/10">
                            {totalLocs} locations
                          </span>
                        </button>

                        {/* Region Content Items */}
                        {isExpanded && (
                          <div className="px-4 pb-3 pt-1 border-t border-white/[0.06] space-y-3 bg-black/15">
                            {/* Places */}
                            {placesCount > 0 && (
                              <div>
                                <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500 mb-1.5">
                                  Places & Landmarks
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                                  {region.places.map((place) => (
                                    <button
                                      key={place.id}
                                      onClick={() => loadArticle(place.path)}
                                      className="text-left px-2.5 py-1.5 rounded-lg border border-white/5 bg-white/[0.02] hover:bg-amber-500/10 hover:border-amber-500/30 transition-all flex items-center justify-between group"
                                    >
                                      <div className="flex items-center gap-2 truncate">
                                        <MapPin
                                          size={12}
                                          className="text-slate-500 group-hover:text-amber-400 shrink-0"
                                        />
                                        <span className="text-xs text-slate-300 group-hover:text-white truncate">
                                          {place.name}
                                        </span>
                                      </div>
                                      <ChevronRight
                                        size={11}
                                        className="text-slate-600 group-hover:text-amber-300 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                      />
                                    </button>
                                  ))}
                                </div>
                              </div>
                            )}

                            {/* Settlements */}
                            {settlementsCount > 0 && (
                              <div>
                                <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500 mb-1.5">
                                  Settlements
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                                  {region.settlements.map((settlement) => (
                                    <button
                                      key={settlement.id}
                                      onClick={() => loadArticle(settlement.path)}
                                      className="text-left px-2.5 py-1.5 rounded-lg border border-white/5 bg-white/[0.02] hover:bg-cyan-500/10 hover:border-cyan-500/30 transition-all flex items-center justify-between group"
                                    >
                                      <div className="flex items-center gap-2 truncate">
                                        <Shield
                                          size={12}
                                          className="text-slate-500 group-hover:text-cyan-400 shrink-0"
                                        />
                                        <span className="text-xs text-slate-300 group-hover:text-white truncate">
                                          {settlement.name}
                                        </span>
                                      </div>
                                      <ChevronRight
                                        size={11}
                                        className="text-slate-600 group-hover:text-cyan-300 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                      />
                                    </button>
                                  ))}
                                </div>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* Adventures Section */}
              {filteredAdventures.length > 0 && (
                <div className="space-y-3 pt-2">
                  <div className="flex items-center justify-between text-xs text-slate-400 font-semibold uppercase tracking-wider">
                    <span>Adventures & Dungeons ({filteredAdventures.length})</span>
                  </div>

                  <div className="space-y-2">
                    {filteredAdventures.map((adv) => {
                      const hasChapters = adv.chapters?.length > 0;
                      return (
                        <div
                          key={adv.id}
                          className="rounded-xl border border-white/[0.08] bg-black/25 overflow-hidden transition-colors hover:border-white/[0.14]"
                        >
                          <div className="px-4 py-3 flex items-center justify-between gap-3">
                            <div className="flex items-center gap-2.5 truncate">
                              <Scroll size={15} className="text-purple-400 shrink-0" />
                              <span className="text-sm font-semibold text-white truncate">
                                {adv.name}
                              </span>
                            </div>

                            {hasChapters ? (
                              <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-purple-500/15 text-purple-300 border border-purple-500/30 shrink-0">
                                {adv.chapters.length} chapters
                              </span>
                            ) : (
                              <button
                                onClick={() => loadArticle(adv.path)}
                                className="btn btn-ghost px-2.5 py-1 text-xs text-purple-300 hover:text-white shrink-0"
                              >
                                <span>Open Module</span>
                                <ChevronRight size={11} />
                              </button>
                            )}
                          </div>

                          {hasChapters && (
                            <div className="px-4 pb-3 pt-1 border-t border-white/[0.06] bg-black/15 space-y-1">
                              {adv.overviewPath && (
                                <button
                                  onClick={() => loadArticle(adv.overviewPath)}
                                  className="w-full text-left px-2.5 py-1.5 rounded-lg border border-purple-500/20 bg-purple-500/10 hover:bg-purple-500/20 text-purple-200 text-xs font-semibold flex items-center justify-between mb-1"
                                >
                                  <span>Overview & Primer</span>
                                  <ChevronRight size={11} />
                                </button>
                              )}
                              <div className="max-h-48 overflow-y-auto space-y-1 pr-1">
                                {adv.chapters.map((chapter) => (
                                  <button
                                    key={chapter.id}
                                    onClick={() => loadArticle(chapter.path)}
                                    className="w-full text-left px-2.5 py-1.5 rounded-lg border border-white/5 bg-white/[0.02] hover:bg-purple-500/10 hover:border-purple-500/30 transition-all flex items-center justify-between group"
                                  >
                                    <span className="text-xs text-slate-300 group-hover:text-white truncate">
                                      {chapter.name}
                                    </span>
                                    <ChevronRight
                                      size={11}
                                      className="text-slate-600 group-hover:text-purple-300 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                                    />
                                  </button>
                                ))}
                              </div>
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Custom Lore Section */}
              {filteredCustom.length > 0 && (
                <div className="space-y-3 pt-2">
                  <div className="flex items-center justify-between text-xs text-slate-400 font-semibold uppercase tracking-wider">
                    <span>Custom Lore & Articles ({filteredCustom.length})</span>
                  </div>

                  <div className="space-y-1.5">
                    {filteredCustom.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        onClick={() => loadArticle(item.path)}
                        className="w-full text-left p-3 rounded-xl border border-white/[0.08] bg-black/25 hover:border-amber-400/40 hover:bg-amber-500/[0.06] transition-all flex items-center justify-between group"
                      >
                        <div className="flex items-center gap-2.5 truncate">
                          <BookOpen size={14} className="text-amber-400 shrink-0" />
                          <span className="text-xs font-semibold text-white group-hover:text-amber-200 truncate">
                            {item.name}
                          </span>
                        </div>
                        <ChevronRight size={13} className="text-slate-600 group-hover:text-amber-300 shrink-0" />
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
