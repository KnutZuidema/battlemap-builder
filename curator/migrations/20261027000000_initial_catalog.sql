-- Initial PostgreSQL catalog schema for Curator domain entities

-- 1. Candidates (Staging Queue)
CREATE TABLE IF NOT EXISTS candidates (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL DEFAULT 'reddit',
    parent_group_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_candidates_source ON candidates(source);
CREATE INDEX IF NOT EXISTS idx_candidates_parent ON candidates(parent_group_id);

-- 2. Curated Maps
CREATE TABLE IF NOT EXISTS curated_maps (
    id TEXT PRIMARY KEY,
    tags TEXT[] NOT NULL DEFAULT '{}',
    themes TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_curated_tags ON curated_maps USING GIN(tags);
CREATE INDEX IF NOT EXISTS idx_curated_themes ON curated_maps USING GIN(themes);

-- 3. Rejections Log
CREATE TABLE IF NOT EXISTS rejections (
    id TEXT PRIMARY KEY,
    reason TEXT NOT NULL,
    notes TEXT,
    rejected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_rejections_rejected_at ON rejections(rejected_at);

-- 4. Trashed Candidates
CREATE TABLE IF NOT EXISTS trashed_candidates (
    id TEXT PRIMARY KEY,
    trashed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);

-- 5. Dataset Snapshots
CREATE TABLE IF NOT EXISTS dataset_snapshots (
    id TEXT PRIMARY KEY,
    theme TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dataset_snapshots_theme ON dataset_snapshots(theme);
CREATE INDEX IF NOT EXISTS idx_dataset_snapshots_created ON dataset_snapshots(created_at DESC);

-- 6. Training Runs
CREATE TABLE IF NOT EXISTS training_runs (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    dataset_snapshot_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_training_runs_snapshot ON training_runs(dataset_snapshot_id);
CREATE INDEX IF NOT EXISTS idx_training_runs_created ON training_runs(created_at DESC);

-- 7. Training Metrics
-- Metrics are an append-only log keyed by run id. No foreign key: the port contract permits
-- appending metrics for a run that has not been persisted yet (parity with the filesystem and
-- in-memory adapters).
CREATE TABLE IF NOT EXISTS training_metrics (
    id BIGSERIAL PRIMARY KEY,
    run_id TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_training_metrics_run_id ON training_metrics(run_id);

-- 8. LoRA Adapters
CREATE TABLE IF NOT EXISTS lora_adapters (
    id TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lora_adapters_created ON lora_adapters(created_at DESC);

-- 9. Harvest Runs Ledger
CREATE TABLE IF NOT EXISTS harvest_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    subreddit TEXT NOT NULL DEFAULT 'battlemaps',
    sort TEXT NOT NULL DEFAULT 'hot',
    posts_scanned INT NOT NULL DEFAULT 0,
    downloaded INT NOT NULL DEFAULT 0,
    skipped INT NOT NULL DEFAULT 0,
    error TEXT
);
CREATE INDEX IF NOT EXISTS idx_harvest_runs_started ON harvest_runs(started_at DESC);
