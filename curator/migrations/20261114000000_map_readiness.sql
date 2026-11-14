-- Persisted per-map LoRA readiness audits.
--
-- One row per curated map: the audit is reused while `fingerprint` still matches the map
-- (dimensions, grid, sections, description, layers), and re-run once the map changes. Rows are
-- deleted with their curated map. `data` is the serialized `MapReadinessRecord` document.
CREATE TABLE IF NOT EXISTS map_readiness (
    id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    audited_at TIMESTAMPTZ NOT NULL,
    data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_map_readiness_audited ON map_readiness(audited_at);
