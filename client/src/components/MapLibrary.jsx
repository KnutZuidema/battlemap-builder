import React, { useState, useEffect, useMemo, useCallback } from 'react';
import {
  Search,
  Plus,
  RefreshCw,
  Layers,
  Sparkles,
  Gamepad2,
  Copy,
  Trash2,
  Compass,
  X,
  Clock,
  ArrowUpDown,
  Wand2,
  ArrowLeft,
  Loader2,
  AlertCircle
} from 'lucide-react';
import { fetchMapsApi, duplicateMapApi, deleteMapApi } from '../services/api';
import { formatCost } from '../utils/format';

/**
 * Formats ISO timestamp into relative human readable time string.
 */
function formatRelativeTime(dateString) {
  if (!dateString) return 'Unknown';
  const date = new Date(dateString);
  if (isNaN(date.getTime())) return 'Unknown';

  const now = new Date();
  const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);

  if (diffInSeconds < 60) {
    return 'Just now';
  }
  const diffInMinutes = Math.floor(diffInSeconds / 60);
  if (diffInMinutes < 60) {
    return `${diffInMinutes}m ago`;
  }
  const diffInHours = Math.floor(diffInMinutes / 60);
  if (diffInHours < 24) {
    return `${diffInHours}h ago`;
  }
  const diffInDays = Math.floor(diffInHours / 24);
  if (diffInDays === 1) {
    return 'Yesterday';
  }
  if (diffInDays < 30) {
    return `${diffInDays}d ago`;
  }
  return date.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: date.getFullYear() !== now.getFullYear() ? 'numeric' : undefined
  });
}

/**
 * Extracts total numeric cost from either a flat number or cost telemetry object.
 */
function extractTotalCost(cost) {
  if (typeof cost === 'number' && !isNaN(cost)) return cost;
  if (cost && typeof cost.totalCost === 'number' && !isNaN(cost.totalCost)) return cost.totalCost;
  return 0;
}

/**
 * Dedicated thumbnail renderer with fallback image on error or missing data.
 */
function MapThumbnail({ thumbnailDataUrl, title }) {
  const [hasError, setHasError] = useState(false);

  if (!thumbnailDataUrl || hasError) {
    return (
      <div className="w-full h-full flex flex-col items-center justify-center bg-slate-900/90 text-slate-500 canvas-grid-bg select-none">
        <Compass className="w-10 h-10 text-slate-600 mb-2 stroke-[1.5]" />
        <span className="text-xs font-mono text-slate-500">No Preview</span>
      </div>
    );
  }

  return (
    <img
      src={thumbnailDataUrl}
      alt={title || 'Battlemap thumbnail'}
      className="w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
      loading="lazy"
      onError={() => setHasError(true)}
    />
  );
}

/**
 * MapLibrary - Responsive gallery view for browsing, filtering, and managing saved battlemaps.
 */
export default function MapLibrary({
  onSelectMap,
  onCreateNewMap,
  currentMapId,
  onClose
}) {
  const [maps, setMaps] = useState([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [sortBy, setSortBy] = useState('newest');
  const [actionInProgress, setActionInProgress] = useState(null);

  // Fetch maps from backend
  const loadMaps = useCallback(async (isSilent = false) => {
    if (!isSilent) {
      setIsLoading(true);
    }
    setError(null);
    try {
      const data = await fetchMapsApi();
      if (data && Array.isArray(data.maps)) {
        setMaps(data.maps);
      } else {
        setMaps([]);
      }
    } catch (err) {
      setError(err.message || 'Failed to load battlemaps from server');
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function init() {
      try {
        const data = await fetchMapsApi();
        if (!cancelled) {
          if (data && Array.isArray(data.maps)) {
            setMaps(data.maps);
          } else {
            setMaps([]);
          }
        }
      } catch (err) {
        if (!cancelled) {
          setError(err.message || 'Failed to load battlemaps from server');
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }
    init();
    return () => {
      cancelled = true;
    };
  }, []);

  // Duplicate action
  const handleDuplicate = async (map) => {
    if (actionInProgress) return;
    setActionInProgress({ id: map.id, type: 'duplicate' });
    setError(null);
    try {
      const res = await duplicateMapApi(map.id);
      if (res?.ok) {
        await loadMaps(true);
      }
    } catch (err) {
      setError(`Failed to duplicate "${map.title || 'map'}": ${err.message}`);
    } finally {
      setActionInProgress(null);
    }
  };

  // Delete action with confirmation
  const handleDelete = async (map) => {
    if (actionInProgress) return;
    const mapName = map.title || 'Untitled Map';
    const confirmed = window.confirm(`Are you sure you want to delete "${mapName}"? This action cannot be undone.`);
    if (!confirmed) return;

    setActionInProgress({ id: map.id, type: 'delete' });
    setError(null);
    try {
      const res = await deleteMapApi(map.id);
      if (res?.ok) {
        setMaps((prev) => prev.filter((m) => m.id !== map.id));
      }
    } catch (err) {
      setError(`Failed to delete "${mapName}": ${err.message}`);
    } finally {
      setActionInProgress(null);
    }
  };

  // Filtering & Sorting
  const filteredMaps = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();

    const filtered = maps.filter((map) => {
      if (!q) return true;
      const titleMatch = map.title && map.title.toLowerCase().includes(q);
      const promptMatch = map.prompt && map.prompt.toLowerCase().includes(q);
      return Boolean(titleMatch || promptMatch);
    });

    return filtered.sort((a, b) => {
      switch (sortBy) {
        case 'newest': {
          const timeA = a.updatedAt ? new Date(a.updatedAt).getTime() : (a.createdAt ? new Date(a.createdAt).getTime() : 0);
          const timeB = b.updatedAt ? new Date(b.updatedAt).getTime() : (b.createdAt ? new Date(b.createdAt).getTime() : 0);
          return timeB - timeA;
        }
        case 'oldest': {
          const timeA = a.updatedAt ? new Date(a.updatedAt).getTime() : (a.createdAt ? new Date(a.createdAt).getTime() : 0);
          const timeB = b.updatedAt ? new Date(b.updatedAt).getTime() : (b.createdAt ? new Date(b.createdAt).getTime() : 0);
          return timeA - timeB;
        }
        case 'title': {
          return (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
        }
        case 'cost': {
          return extractTotalCost(b.cost) - extractTotalCost(a.cost);
        }
        default:
          return 0;
      }
    });
  }, [maps, searchQuery, sortBy]);

  return (
    <div className="flex-1 h-full flex flex-col overflow-hidden bg-slate-950 text-slate-100 select-none">
      {/* Top Header Bar */}
      <header className="shrink-0 px-6 py-4 border-b border-white/10 glass-panel flex flex-col md:flex-row md:items-center justify-between gap-4 z-20">
        <div className="flex items-center gap-3">
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="btn btn-ghost text-xs px-2.5 py-1.5 flex items-center gap-1.5 text-slate-300 hover:text-white"
              title="Return to Studio"
            >
              <ArrowLeft className="w-4 h-4" />
              <span>Studio</span>
            </button>
          )}

          <div className="flex items-center gap-2.5">
            <div className="w-9 h-9 rounded-xl bg-amber-500/10 border border-amber-500/25 grid place-items-center">
              <Layers className="w-5 h-5 text-amber-400" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h1 className="font-display text-lg font-bold text-slate-100 tracking-tight">
                  Battlemap Library
                </h1>
                {!isLoading && (
                  <span className="px-2 py-0.5 text-xs font-mono font-medium rounded-full bg-slate-800 text-slate-300 border border-slate-700">
                    {maps.length}
                  </span>
                )}
              </div>
              <p className="text-xs text-slate-400 hidden sm:block">
                Browse, search, duplicate, and launch your saved maps
              </p>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-2.5 self-end md:self-auto">
          <button
            type="button"
            onClick={() => loadMaps(false)}
            disabled={isLoading}
            className="btn btn-ghost text-xs py-2 px-3 flex items-center gap-1.5 border border-slate-700/80 hover:border-slate-600"
            title="Refresh map library"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin text-amber-400' : ''}`} />
            <span className="hidden sm:inline">Refresh</span>
          </button>

          {onCreateNewMap && (
            <button
              type="button"
              onClick={onCreateNewMap}
              className="btn btn-primary text-xs py-2 px-3.5 flex items-center gap-1.5 shadow-lg shadow-amber-500/10"
              title="Create new battlemap"
            >
              <Plus className="w-4 h-4" />
              <span>New Map</span>
            </button>
          )}
        </div>
      </header>

      {/* Filter and Search Toolbar */}
      <section className="shrink-0 px-6 py-3 border-b border-white/5 bg-slate-900/60 flex flex-col sm:flex-row items-center justify-between gap-3 z-10">
        <div className="relative w-full sm:w-80">
          <Search className="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search by title or prompt..."
            className="field text-xs pl-9 pr-8 py-2 w-full rounded-lg"
          />
          {searchQuery && (
            <button
              type="button"
              onClick={() => setSearchQuery('')}
              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-200"
              title="Clear search"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>

        <div className="flex items-center gap-2.5 w-full sm:w-auto justify-between sm:justify-end">
          <div className="flex items-center gap-1.5 text-xs text-slate-400">
            <ArrowUpDown className="w-3.5 h-3.5 text-slate-500" />
            <span className="hidden md:inline">Sort:</span>
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value)}
              className="field text-xs py-1.5 px-2.5 rounded-lg bg-slate-900 border border-slate-700/80 text-slate-200 focus:border-amber-400/60"
            >
              <option value="newest">Newest First</option>
              <option value="oldest">Oldest First</option>
              <option value="title">Title (A–Z)</option>
              <option value="cost">Highest Cost</option>
            </select>
          </div>

          {!isLoading && maps.length > 0 && (
            <span className="text-[11px] font-mono text-slate-500 hidden sm:inline">
              Showing {filteredMaps.length} of {maps.length}
            </span>
          )}
        </div>
      </section>

      {/* Error Banner */}
      {error && (
        <div className="shrink-0 mx-6 mt-4 p-3 rounded-xl bg-red-950/60 border border-red-500/30 text-red-200 text-xs flex items-center justify-between gap-3 animate-fade">
          <div className="flex items-center gap-2 min-w-0">
            <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />
            <span className="truncate">{error}</span>
          </div>
          <button
            type="button"
            onClick={() => setError(null)}
            className="text-red-400 hover:text-red-200 text-xs px-2 py-0.5"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Main Content Gallery */}
      <main className="flex-1 overflow-y-auto px-6 py-6">
        {isLoading ? (
          /* Loading Skeletons */
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
            {Array.from({ length: 8 }).map((_, i) => (
              <div key={i} className="panel-card overflow-hidden animate-pulse flex flex-col">
                <div className="aspect-square w-full bg-slate-800/60" />
                <div className="p-4 flex flex-col gap-3">
                  <div className="flex justify-between items-center gap-2">
                    <div className="h-4 bg-slate-700/60 rounded w-2/3" />
                    <div className="h-3 bg-slate-700/40 rounded w-1/4" />
                  </div>
                  <div className="space-y-1.5">
                    <div className="h-3 bg-slate-700/30 rounded w-full" />
                    <div className="h-3 bg-slate-700/30 rounded w-4/5" />
                  </div>
                  <div className="pt-3 border-t border-white/5 flex justify-between items-center mt-2">
                    <div className="h-7 bg-slate-700/50 rounded w-28" />
                    <div className="flex gap-1.5">
                      <div className="h-7 w-7 bg-slate-700/40 rounded-lg" />
                      <div className="h-7 w-7 bg-slate-700/40 rounded-lg" />
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        ) : maps.length === 0 ? (
          /* Empty State: No maps at all */
          <div className="panel-card p-12 text-center max-w-lg mx-auto my-12 flex flex-col items-center gap-4">
            <div className="w-16 h-16 rounded-2xl bg-amber-500/10 border border-amber-500/20 grid place-items-center mb-1">
              <Compass className="w-8 h-8 text-amber-400 animate-float" />
            </div>
            <h3 className="font-display text-xl font-bold text-slate-100">
              Your Map Library is Empty
            </h3>
            <p className="text-slate-400 text-sm leading-relaxed max-w-sm">
              You haven't saved any battlemaps yet. Generate your first map in the studio to start building your tactical library.
            </p>
            {onCreateNewMap && (
              <button
                type="button"
                onClick={onCreateNewMap}
                className="btn btn-primary text-sm px-5 py-2.5 mt-2 flex items-center gap-2 shadow-lg shadow-amber-500/15"
              >
                <Plus className="w-4 h-4" />
                <span>Create Your First Map</span>
              </button>
            )}
          </div>
        ) : filteredMaps.length === 0 ? (
          /* Empty State: Search returned 0 results */
          <div className="panel-card p-10 text-center max-w-md mx-auto my-12 flex flex-col items-center gap-3">
            <div className="w-12 h-12 rounded-xl bg-slate-800/60 border border-slate-700/50 grid place-items-center">
              <Search className="w-6 h-6 text-slate-400" />
            </div>
            <h3 className="font-display text-lg font-bold text-slate-200">
              No battlemaps match
            </h3>
            <p className="text-slate-400 text-sm">
              No battlemaps match <span className="text-amber-300 font-medium">"{searchQuery}"</span>
            </p>
            <button
              type="button"
              onClick={() => setSearchQuery('')}
              className="btn btn-ghost text-xs px-3.5 py-1.5 mt-1 border border-slate-700 hover:border-slate-600"
            >
              Clear search
            </button>
          </div>
        ) : (
          /* Grid of Map Cards */
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
            {filteredMaps.map((map) => {
              const isActive = currentMapId && map.id === currentMapId;
              const costNum = extractTotalCost(map.cost);
              const isDuplicating = actionInProgress?.id === map.id && actionInProgress.type === 'duplicate';
              const isDeleting = actionInProgress?.id === map.id && actionInProgress.type === 'delete';
              const isBusy = Boolean(actionInProgress && actionInProgress.id === map.id);

              return (
                <div
                  key={map.id}
                  className={`group panel-card overflow-hidden flex flex-col transition-all duration-200 ${
                    isActive ? 'ring-2 ring-cyan-500/50 border-cyan-500/40 shadow-cyan-950/30' : ''
                  }`}
                >
                  {/* Square thumbnail container with badges */}
                  <div className="relative aspect-square w-full overflow-hidden bg-slate-950">
                    <MapThumbnail
                      thumbnailDataUrl={map.thumbnailDataUrl}
                      title={map.title}
                    />

                    {/* Gradient vignette overlay */}
                    <div className="absolute inset-0 bg-gradient-to-t from-slate-950/80 via-transparent to-slate-950/40 pointer-events-none" />

                    {/* Top-left: Active in Studio badge */}
                    {isActive && (
                      <div className="absolute top-2.5 left-2.5 z-10 pointer-events-none">
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-semibold rounded-full bg-cyan-500/20 text-cyan-300 border border-cyan-400/40 shadow-lg backdrop-blur-md">
                          <span className="w-1.5 h-1.5 rounded-full bg-cyan-400 animate-pulse" />
                          Active in Studio
                        </span>
                      </div>
                    )}

                    {/* Top-right: Dimensions badge */}
                    <div className="absolute top-2.5 right-2.5 z-10 pointer-events-none">
                      <span className="inline-flex items-center px-2 py-0.5 text-[11px] font-mono font-medium rounded-md bg-slate-900/80 text-slate-300 border border-slate-700/60 shadow-md backdrop-blur-md">
                        {map.dimensions?.width || 1024}×{map.dimensions?.height || 1024}
                      </span>
                    </div>

                    {/* Bottom-left: Feature count badge */}
                    {map.featureCount > 0 && (
                      <div className="absolute bottom-2.5 left-2.5 z-10 pointer-events-none">
                        <span className="inline-flex items-center gap-1.5 px-2 py-0.5 text-xs font-medium rounded-md bg-violet-950/80 text-violet-300 border border-violet-700/50 shadow-md backdrop-blur-md">
                          <Sparkles className="w-3.5 h-3.5 text-violet-400" />
                          {map.featureCount} {map.featureCount === 1 ? 'feature' : 'features'}
                        </span>
                      </div>
                    )}

                    {/* Bottom-right: Cost badge */}
                    {costNum > 0 && (
                      <div className="absolute bottom-2.5 right-2.5 z-10 pointer-events-none">
                        <span className="inline-flex items-center px-2 py-0.5 text-xs font-mono font-semibold rounded-md bg-amber-950/80 text-amber-300 border border-amber-600/50 shadow-md backdrop-blur-md">
                          {costNum >= 0.01 ? `$${costNum.toFixed(2)}` : formatCost(costNum, 4)}
                        </span>
                      </div>
                    )}
                  </div>

                  {/* Card Details */}
                  <div className="p-4 flex flex-col flex-1 gap-2.5">
                    <div className="flex items-start justify-between gap-2">
                      <h3
                        className="font-display font-bold text-slate-100 text-sm truncate flex-1"
                        title={map.title || 'Untitled Map'}
                      >
                        {map.title || 'Untitled Map'}
                      </h3>
                      <span className="text-[11px] text-slate-400 font-medium shrink-0 flex items-center gap-1">
                        <Clock className="w-3 h-3 text-slate-500" />
                        {formatRelativeTime(map.updatedAt || map.createdAt)}
                      </span>
                    </div>

                    {/* Prompt Preview Snippet */}
                    <p className="text-slate-400 text-xs line-clamp-2 leading-relaxed flex-1 min-h-[2.5rem]">
                      {map.prompt ? (
                        map.prompt
                      ) : (
                        <span className="italic text-slate-500">No prompt description</span>
                      )}
                    </p>

                    {/* Card Actions */}
                    <div className="pt-3 border-t border-white/5 flex items-center justify-between gap-2 mt-auto">
                      <div className="flex items-center gap-1.5 flex-1 min-w-0">
                        {/* Primary: Open in Studio */}
                        <button
                          type="button"
                          onClick={() => onSelectMap?.(map.id, 'edit')}
                          className="btn btn-primary text-xs py-1.5 px-3 flex-1 flex items-center justify-center gap-1.5"
                          title="Open in Studio"
                        >
                          <Wand2 className="w-3.5 h-3.5" />
                          <span className="truncate">Open in Studio</span>
                        </button>

                        {/* Play Tactical Mode */}
                        <button
                          type="button"
                          onClick={() => onSelectMap?.(map.id, 'play')}
                          className="btn btn-cyan text-xs py-1.5 px-2.5 flex items-center justify-center gap-1"
                          title="Play Tactical Mode"
                        >
                          <Gamepad2 className="w-3.5 h-3.5" />
                          <span className="hidden xl:inline">Play</span>
                        </button>
                      </div>

                      {/* Secondary Actions: Duplicate & Delete */}
                      <div className="flex items-center gap-1 shrink-0">
                        <button
                          type="button"
                          disabled={isBusy}
                          onClick={() => handleDuplicate(map)}
                          className="icon-btn p-1.5 text-slate-400 hover:text-amber-300 hover:bg-amber-500/10 rounded-lg border border-transparent hover:border-amber-500/20 transition disabled:opacity-40"
                          title="Duplicate map"
                        >
                          {isDuplicating ? (
                            <Loader2 className="w-3.5 h-3.5 animate-spin text-amber-400" />
                          ) : (
                            <Copy className="w-3.5 h-3.5" />
                          )}
                        </button>
                        <button
                          type="button"
                          disabled={isBusy}
                          onClick={() => handleDelete(map)}
                          className="icon-btn p-1.5 text-slate-400 hover:text-red-400 hover:bg-red-500/10 rounded-lg border border-transparent hover:border-red-500/20 transition disabled:opacity-40"
                          title="Delete map"
                        >
                          {isDeleting ? (
                            <Loader2 className="w-3.5 h-3.5 animate-spin text-red-400" />
                          ) : (
                            <Trash2 className="w-3.5 h-3.5" />
                          )}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </main>
    </div>
  );
}
