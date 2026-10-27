//! PostgreSQL-backed [`CatalogRepository`] storing domain entities as indexed JSONB documents (ADR-0011).

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::domain::{
    Candidate, CuratedMap, DatasetSnapshot, LoraAdapter, RedditHarvestQuery, RejectionReason,
    RejectionRecord, Theme, TrainingMetricSnapshot, TrainingRun,
};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use crate::ports::harvest_run_ledger::{HarvestRunLedger, HarvestRunOutcome};
use crate::ports::storage_error::StorageError;

fn map_sqlx_err(err: sqlx::Error) -> StorageError {
    StorageError::Io(std::io::Error::other(err.to_string()))
}

/// Classifies a failure to decode a persisted entity as malformed stored data rather than a
/// serialization fault, matching [`super::FilesystemCatalogRepository`] (API 400, not 500).
fn decode_error(err: serde_json::Error) -> StorageError {
    StorageError::InvalidData(format!("Invalid stored entity: {err}"))
}

/// Parses a stored rejection reason, surfacing unknown values as invalid data.
fn parse_rejection_reason(raw: &str) -> Result<RejectionReason, StorageError> {
    raw.parse::<RejectionReason>()
        .map_err(|err| StorageError::InvalidData(format!("Invalid rejection reason: {err}")))
}

/// Decodes one rejection log row, read back in append order by the caller.
fn rejection_from_row(row: &PgRow) -> Result<RejectionRecord, StorageError> {
    let id: String = row.try_get("candidate_id").map_err(map_sqlx_err)?;
    let reason_str: String = row.try_get("reason").map_err(map_sqlx_err)?;
    let reason = parse_rejection_reason(&reason_str)?;
    let notes: Option<String> = row.try_get("notes").map_err(map_sqlx_err)?;
    let timestamp: chrono::DateTime<chrono::Utc> =
        row.try_get("rejected_at").map_err(map_sqlx_err)?;
    Ok(RejectionRecord::new(id, reason, notes, timestamp))
}

/// Truncates a timestamp to PostgreSQL `timestamptz` (microsecond) precision.
///
/// Filesystem manifests keep nanosecond timestamps; the append-only logs are deduplicated on the
/// stored column, so both sides must be compared at the precision PostgreSQL actually keeps.
fn to_micros(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    timestamp - chrono::Duration::nanoseconds(i64::from(timestamp.timestamp_subsec_nanos() % 1_000))
}

/// Conflict policy applied by the bulk import path when a row already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportConflict {
    /// Keep the stored row (`ON CONFLICT DO NOTHING`); the default for repeat migrations.
    SkipExisting,
    /// Replace the stored row (`ON CONFLICT DO UPDATE`); used by `--force`.
    Overwrite,
}

/// Identity of one line in the append-only rejection log.
///
/// The log has a surrogate primary key, so idempotency is keyed on the full record instead: a
/// repeat import of the same rejection is detected and skipped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RejectionKey {
    pub candidate_id: String,
    pub reason: String,
    pub notes: Option<String>,
    pub rejected_at: DateTime<Utc>,
}

impl RejectionKey {
    /// Computes the idempotency key of a filesystem rejection record.
    pub fn from_record(record: &RejectionRecord) -> Self {
        Self {
            candidate_id: record.id.clone(),
            reason: record.reason.as_str().to_string(),
            notes: record.notes.clone(),
            rejected_at: to_micros(record.timestamp),
        }
    }
}

/// Identity of one appended training metric sample.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrainingMetricKey {
    pub run_id: String,
    pub recorded_at: DateTime<Utc>,
    pub step: usize,
}

impl TrainingMetricKey {
    /// Computes the idempotency key of a metric sample read from `metrics.jsonl`.
    pub fn from_sample(run_id: &str, metric: &TrainingMetricSnapshot) -> Self {
        Self {
            run_id: run_id.to_string(),
            recorded_at: to_micros(metric.timestamp),
            step: metric.step,
        }
    }
}

/// Primary-key snapshot used to predict import outcomes without writing (dry runs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogImportIndex {
    pub candidate_ids: HashSet<String>,
    pub curated_ids: HashSet<String>,
    pub dataset_snapshot_ids: HashSet<String>,
    pub training_run_ids: HashSet<String>,
    pub lora_adapter_ids: HashSet<String>,
    pub rejections: HashSet<RejectionKey>,
    pub training_metrics: HashSet<TrainingMetricKey>,
}

/// Renders the `ON CONFLICT` tail shared by every primary-key import statement.
fn conflict_clause(conflict: ImportConflict, updates: &str) -> String {
    match conflict {
        ImportConflict::SkipExisting => "ON CONFLICT (id) DO NOTHING".to_string(),
        ImportConflict::Overwrite => format!("ON CONFLICT (id) DO UPDATE SET {updates}"),
    }
}

/// Collects the `id` column of every row into a set.
fn collect_ids(rows: Vec<PgRow>) -> Result<HashSet<String>, StorageError> {
    rows.into_iter()
        .map(|row| row.try_get::<String, _>("id").map_err(map_sqlx_err))
        .collect()
}

/// Whether a query failed because the table has not been created yet (SQLSTATE `42P01`).
fn is_undefined_table(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|db_err| db_err.code())
        .map(|code| code == "42P01")
        .unwrap_or(false)
}

/// Resolved choice of catalog repository backend based on configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogBackend {
    Filesystem,
    Postgres(String),
}

impl CatalogBackend {
    /// Selects the backend from environment variables.
    ///
    /// - If `DATABASE_URL` is set and non-empty, returns [`CatalogBackend::Postgres`].
    /// - Otherwise, defaults to [`CatalogBackend::Filesystem`].
    pub fn from_env() -> Self {
        Self::resolve(std::env::var("DATABASE_URL").ok().as_deref())
    }

    /// Pure resolver behind [`CatalogBackend::from_env`].
    pub fn resolve(database_url: Option<&str>) -> Self {
        match database_url.map(str::trim) {
            Some(url) if !url.is_empty() => CatalogBackend::Postgres(url.to_string()),
            _ => CatalogBackend::Filesystem,
        }
    }
}

/// [`CatalogRepository`] adapter persisting entities in PostgreSQL.
#[derive(Debug, Clone)]
pub struct PostgresCatalogRepository {
    pool: PgPool,
}

impl PostgresCatalogRepository {
    /// Connects to PostgreSQL with standard pool configuration.
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(map_sqlx_err)?;
        Ok(Self { pool })
    }

    /// Reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Runs embedded schema migrations against the connected database.
    pub async fn run_migrations(&self) -> Result<(), StorageError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|err| {
                StorageError::InvalidData(format!("catalog migrations failed: {err}"))
            })?;
        Ok(())
    }

    /// Fetches import-index rows, treating a not-yet-migrated schema as empty.
    ///
    /// `--dry-run` deliberately does not run migrations, so a fresh database must still produce a
    /// report ("everything would be imported") instead of a missing-relation error.
    async fn fetch_import_rows(&self, sql: &str) -> Result<Vec<PgRow>, StorageError> {
        match sqlx::query(sql).fetch_all(&self.pool).await {
            Ok(rows) => Ok(rows),
            Err(err) if is_undefined_table(&err) => Ok(Vec::new()),
            Err(err) => Err(map_sqlx_err(err)),
        }
    }

    /// Loads every catalog key the disk import path compares against.
    ///
    /// Used to report `--dry-run` outcomes without writing anything.
    pub async fn import_index(&self) -> Result<CatalogImportIndex, StorageError> {
        let candidate_rows = self.fetch_import_rows("SELECT id FROM candidates").await?;
        let curated_rows = self.fetch_import_rows("SELECT id FROM curated_maps").await?;
        let snapshot_rows = self
            .fetch_import_rows("SELECT id FROM dataset_snapshots")
            .await?;
        let run_rows = self.fetch_import_rows("SELECT id FROM training_runs").await?;
        let adapter_rows = self.fetch_import_rows("SELECT id FROM lora_adapters").await?;

        let rejection_rows = self
            .fetch_import_rows("SELECT candidate_id, reason, notes, rejected_at FROM rejections")
            .await?;
        let mut rejections = HashSet::with_capacity(rejection_rows.len());
        for row in &rejection_rows {
            rejections.insert(RejectionKey {
                candidate_id: row.try_get("candidate_id").map_err(map_sqlx_err)?,
                reason: row.try_get("reason").map_err(map_sqlx_err)?,
                notes: row.try_get("notes").map_err(map_sqlx_err)?,
                rejected_at: row.try_get("rejected_at").map_err(map_sqlx_err)?,
            });
        }

        let metric_rows = self
            .fetch_import_rows(
                "SELECT run_id, recorded_at, (data->>'step') AS step FROM training_metrics",
            )
            .await?;
        let mut training_metrics = HashSet::with_capacity(metric_rows.len());
        for row in &metric_rows {
            let step_raw: Option<String> = row.try_get("step").map_err(map_sqlx_err)?;
            training_metrics.insert(TrainingMetricKey {
                run_id: row.try_get("run_id").map_err(map_sqlx_err)?,
                recorded_at: row.try_get("recorded_at").map_err(map_sqlx_err)?,
                step: step_raw
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0),
            });
        }

        Ok(CatalogImportIndex {
            candidate_ids: collect_ids(candidate_rows)?,
            curated_ids: collect_ids(curated_rows)?,
            dataset_snapshot_ids: collect_ids(snapshot_rows)?,
            training_run_ids: collect_ids(run_rows)?,
            lora_adapter_ids: collect_ids(adapter_rows)?,
            rejections,
            training_metrics,
        })
    }

    /// Writes one candidate, returning `false` when `conflict` kept the existing row.
    ///
    /// Both `save_candidate` and `import_candidate` go through this statement, so a stored entity
    /// always matches the `Candidate` written to it, exactly like the filesystem adapter, whose
    /// `save` rewrites the whole manifest.
    async fn insert_candidate(
        &self,
        candidate: &Candidate,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        let source_str = candidate.source.to_string();
        let data = serde_json::to_value(candidate)?;
        let sql = format!(
            r#"
            INSERT INTO candidates (id, source, parent_group_id, created_at, data)
            VALUES ($1, $2, $3, $4, $5)
            {}
            "#,
            conflict_clause(
                conflict,
                "source = EXCLUDED.source, \
                 parent_group_id = EXCLUDED.parent_group_id, \
                 created_at = EXCLUDED.created_at, \
                 data = EXCLUDED.data"
            )
        );
        let result = sqlx::query(&sql)
            .bind(&candidate.id)
            .bind(&source_str)
            .bind(candidate.parent_group_id.as_deref())
            .bind(candidate.created_at)
            .bind(data)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Writes one curated map, returning `false` when `conflict` kept the existing row.
    ///
    /// Shares [`Self::insert_candidate`]'s statement semantics, `curated_at` included.
    async fn insert_curated(
        &self,
        map: &CuratedMap,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        let tags: Vec<String> = map.tags.clone();
        let themes: Vec<String> = tags
            .iter()
            .filter_map(|tag| tag.parse::<Theme>().ok())
            .map(|theme| theme.as_str().to_string())
            .collect();
        let data = serde_json::to_value(map)?;
        let sql = format!(
            r#"
            INSERT INTO curated_maps (id, tags, themes, created_at, data)
            VALUES ($1, $2, $3, $4, $5)
            {}
            "#,
            conflict_clause(
                conflict,
                "tags = EXCLUDED.tags, \
                 themes = EXCLUDED.themes, \
                 created_at = EXCLUDED.created_at, \
                 data = EXCLUDED.data"
            )
        );
        let result = sqlx::query(&sql)
            .bind(&map.id)
            .bind(&tags)
            .bind(&themes)
            .bind(map.curated_at)
            .bind(data)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Writes one dataset snapshot, returning `false` when `conflict` kept the existing row.
    ///
    /// Shares [`Self::insert_candidate`]'s statement semantics.
    async fn insert_dataset_snapshot(
        &self,
        snapshot: &DatasetSnapshot,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        let theme_str = snapshot.theme.as_ref().map(|t| t.as_str().to_string());
        let data = serde_json::to_value(snapshot)?;
        let sql = format!(
            r#"
            INSERT INTO dataset_snapshots (id, theme, created_at, data)
            VALUES ($1, $2, $3, $4)
            {}
            "#,
            conflict_clause(
                conflict,
                "theme = EXCLUDED.theme, \
                 created_at = EXCLUDED.created_at, \
                 data = EXCLUDED.data"
            )
        );
        let result = sqlx::query(&sql)
            .bind(&snapshot.id)
            .bind(theme_str)
            .bind(snapshot.created_at)
            .bind(data)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Writes one training run, returning `false` when `conflict` kept the existing row.
    ///
    /// Shares [`Self::insert_candidate`]'s statement semantics.
    async fn insert_training_run(
        &self,
        run: &TrainingRun,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        let data = serde_json::to_value(run)?;
        let sql = format!(
            r#"
            INSERT INTO training_runs (id, dataset_snapshot_id, created_at, data)
            VALUES ($1, $2, $3, $4)
            {}
            "#,
            conflict_clause(
                conflict,
                "dataset_snapshot_id = EXCLUDED.dataset_snapshot_id, \
                 created_at = EXCLUDED.created_at, \
                 data = EXCLUDED.data"
            )
        );
        let result = sqlx::query(&sql)
            .bind(&run.id)
            .bind(&run.dataset_snapshot_id)
            .bind(run.created_at)
            .bind(data)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Writes one LoRA adapter, returning `false` when `conflict` kept the existing row.
    ///
    /// Shares [`Self::insert_candidate`]'s statement semantics.
    async fn insert_lora_adapter(
        &self,
        adapter: &LoraAdapter,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        let data = serde_json::to_value(adapter)?;
        let sql = format!(
            r#"
            INSERT INTO lora_adapters (id, created_at, data)
            VALUES ($1, $2, $3)
            {}
            "#,
            conflict_clause(
                conflict,
                "created_at = EXCLUDED.created_at, data = EXCLUDED.data"
            )
        );
        let result = sqlx::query(&sql)
            .bind(&adapter.id)
            .bind(adapter.created_at)
            .bind(data)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Imports one candidate manifest, returning `false` when an existing row was kept.
    pub async fn import_candidate(
        &self,
        candidate: &Candidate,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        self.insert_candidate(candidate, conflict).await
    }

    /// Imports one curated map manifest, returning `false` when an existing row was kept.
    pub async fn import_curated(
        &self,
        map: &CuratedMap,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        self.insert_curated(map, conflict).await
    }

    /// Imports one dataset snapshot manifest, returning `false` when an existing row was kept.
    pub async fn import_dataset_snapshot(
        &self,
        snapshot: &DatasetSnapshot,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        self.insert_dataset_snapshot(snapshot, conflict).await
    }

    /// Imports one training run manifest, returning `false` when an existing row was kept.
    pub async fn import_training_run(
        &self,
        run: &TrainingRun,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        self.insert_training_run(run, conflict).await
    }

    /// Imports one LoRA adapter manifest, returning `false` when an existing row was kept.
    pub async fn import_lora_adapter(
        &self,
        adapter: &LoraAdapter,
        conflict: ImportConflict,
    ) -> Result<bool, StorageError> {
        self.insert_lora_adapter(adapter, conflict).await
    }

    /// Appends one rejection record, preserving its original timestamp and order.
    ///
    /// Returns `false` when an identical record (same candidate, reason, notes, and timestamp) is
    /// already stored, so repeated imports never duplicate the append-only log.
    pub async fn import_rejection(
        &self,
        record: &RejectionRecord,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT INTO rejections (candidate_id, reason, notes, rejected_at)
            SELECT $1::text, $2::text, $3::text, $4::timestamptz
            WHERE NOT EXISTS (
                SELECT 1 FROM rejections
                WHERE candidate_id = $1::text
                  AND reason = $2::text
                  AND notes IS NOT DISTINCT FROM $3::text
                  AND rejected_at = $4::timestamptz
            )
            "#,
        )
        .bind(&record.id)
        .bind(record.reason.as_str())
        .bind(record.notes.as_deref())
        .bind(to_micros(record.timestamp))
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Appends one training metric sample, deduplicating identical samples on re-import.
    pub async fn import_training_metric(
        &self,
        run_id: &str,
        metric: &TrainingMetricSnapshot,
    ) -> Result<bool, StorageError> {
        let data = serde_json::to_value(metric)?;
        let result = sqlx::query(
            r#"
            INSERT INTO training_metrics (run_id, data, recorded_at)
            SELECT $1::text, $2::jsonb, $3::timestamptz
            WHERE NOT EXISTS (
                SELECT 1 FROM training_metrics
                WHERE run_id = $1::text
                  AND recorded_at = $3::timestamptz
                  AND data = $2::jsonb
            )
            "#,
        )
        .bind(run_id)
        .bind(data)
        .bind(to_micros(metric.timestamp))
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }
}

#[async_trait]
impl CatalogRepository for PostgresCatalogRepository {
    async fn get_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        let row = sqlx::query("SELECT data FROM candidates WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("Candidate {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn save_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        self.insert_candidate(candidate, ImportConflict::Overwrite)
            .await?;
        Ok(())
    }

    async fn list_candidates(&self, filter: &CandidateFilter) -> Result<Vec<Candidate>, StorageError> {
        let mut builder = sqlx::QueryBuilder::new("SELECT data FROM candidates WHERE 1=1");
        if let Some(source) = &filter.source {
            builder.push(" AND source = ").push_bind(source.to_string());
        }
        if let Some(parent) = &filter.parent_group_id {
            builder.push(" AND parent_group_id = ").push_bind(parent);
        }
        builder.push(" ORDER BY id ASC");
        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }
        if let Some(offset) = filter.offset {
            builder.push(" OFFSET ").push_bind(offset as i64);
        }

        let rows = builder.build().fetch_all(&self.pool).await.map_err(map_sqlx_err)?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            candidates.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(candidates)
    }

    async fn delete_candidate(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM candidates WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn get_curated(&self, id: &str) -> Result<CuratedMap, StorageError> {
        let row = sqlx::query("SELECT data FROM curated_maps WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("Curated map {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn save_curated(&self, map: &CuratedMap) -> Result<(), StorageError> {
        self.insert_curated(map, ImportConflict::Overwrite).await?;
        Ok(())
    }

    async fn list_curated(&self, filter: &CuratedFilter) -> Result<Vec<CuratedMap>, StorageError> {
        let mut builder = sqlx::QueryBuilder::new("SELECT data FROM curated_maps WHERE 1=1");
        if let Some(theme) = &filter.theme {
            let theme_tag = vec![theme.as_str().to_string()];
            builder.push(" AND themes @> ").push_bind(theme_tag);
        }
        if !filter.tags.is_empty() {
            builder.push(" AND tags @> ").push_bind(filter.tags.clone());
        }
        if let Some(ids) = &filter.ids {
            builder.push(" AND id = ANY(").push_bind(ids.clone()).push(")");
        }
        builder.push(" ORDER BY id ASC");
        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }
        if let Some(offset) = filter.offset {
            builder.push(" OFFSET ").push_bind(offset as i64);
        }

        let rows = builder.build().fetch_all(&self.pool).await.map_err(map_sqlx_err)?;
        let mut maps = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            maps.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(maps)
    }

    async fn delete_curated(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM curated_maps WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn rejection_records(&self) -> Result<Vec<RejectionRecord>, StorageError> {
        let rows = sqlx::query(
            "SELECT candidate_id, reason, notes, rejected_at FROM rejections ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_err)?;

        let mut records = Vec::with_capacity(rows.len());
        for row in &rows {
            records.push(rejection_from_row(row)?);
        }
        Ok(records)
    }

    async fn is_rejected(&self, id: &str) -> Result<bool, StorageError> {
        let row =
            sqlx::query("SELECT EXISTS(SELECT 1 FROM rejections WHERE candidate_id = $1) as exists")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(map_sqlx_err)?;
        let exists: bool = row.try_get("exists").map_err(map_sqlx_err)?;
        Ok(exists)
    }

    async fn log_rejection(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let reason_str = reason.as_str();
        sqlx::query(
            r#"
            INSERT INTO rejections (candidate_id, reason, notes, rejected_at)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(id)
        .bind(reason_str)
        .bind(notes)
        .bind(chrono::Utc::now())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn remove_rejection(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM rejections WHERE candidate_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn latest_rejection(&self) -> Result<Option<RejectionRecord>, StorageError> {
        let row = sqlx::query(
            "SELECT candidate_id, reason, notes, rejected_at FROM rejections ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_err)?;

        row.as_ref().map(rejection_from_row).transpose()
    }

    async fn save_trashed_candidate(&self, candidate: &Candidate) -> Result<(), StorageError> {
        let data = serde_json::to_value(candidate)?;
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO trashed_candidates (id, trashed_at, data)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO UPDATE SET
                trashed_at = EXCLUDED.trashed_at,
                data = EXCLUDED.data
            "#,
        )
        .bind(&candidate.id)
        .bind(now)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn get_trashed_candidate(&self, id: &str) -> Result<Candidate, StorageError> {
        let row = sqlx::query("SELECT data FROM trashed_candidates WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("Trashed candidate {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn delete_trashed_candidate(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM trashed_candidates WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn get_dataset_snapshot(&self, id: &str) -> Result<DatasetSnapshot, StorageError> {
        let row = sqlx::query("SELECT data FROM dataset_snapshots WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("Dataset snapshot {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn save_dataset_snapshot(&self, snapshot: &DatasetSnapshot) -> Result<(), StorageError> {
        self.insert_dataset_snapshot(snapshot, ImportConflict::Overwrite)
            .await?;
        Ok(())
    }

    async fn list_dataset_snapshots(
        &self,
        theme: Option<&Theme>,
    ) -> Result<Vec<DatasetSnapshot>, StorageError> {
        let mut builder = sqlx::QueryBuilder::new("SELECT data FROM dataset_snapshots WHERE 1=1");
        if let Some(theme) = theme {
            builder.push(" AND theme = ").push_bind(theme.as_str());
        }
        builder.push(" ORDER BY created_at DESC, id ASC");

        let rows = builder.build().fetch_all(&self.pool).await.map_err(map_sqlx_err)?;
        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            snapshots.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(snapshots)
    }

    async fn delete_dataset_snapshot(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM dataset_snapshots WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn is_dataset_snapshot_referenced(&self, id: &str) -> Result<bool, StorageError> {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM training_runs WHERE dataset_snapshot_id = $1) as exists")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
        let exists: bool = row.try_get("exists").map_err(map_sqlx_err)?;
        Ok(exists)
    }

    async fn get_training_run(&self, id: &str) -> Result<TrainingRun, StorageError> {
        let row = sqlx::query("SELECT data FROM training_runs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("Training run {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn save_training_run(&self, run: &TrainingRun) -> Result<(), StorageError> {
        self.insert_training_run(run, ImportConflict::Overwrite)
            .await?;
        Ok(())
    }

    async fn list_training_runs(&self) -> Result<Vec<TrainingRun>, StorageError> {
        let rows = sqlx::query("SELECT data FROM training_runs ORDER BY created_at DESC, id ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let mut runs = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            runs.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(runs)
    }

    async fn append_training_metric(
        &self,
        run_id: &str,
        metric: &TrainingMetricSnapshot,
    ) -> Result<(), StorageError> {
        let data = serde_json::to_value(metric)?;
        sqlx::query(
            r#"
            INSERT INTO training_metrics (run_id, data, recorded_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(run_id)
        .bind(data)
        .bind(metric.timestamp)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
        Ok(())
    }

    async fn list_training_metrics(
        &self,
        run_id: &str,
    ) -> Result<Vec<TrainingMetricSnapshot>, StorageError> {
        let rows = sqlx::query(
            "SELECT data FROM training_metrics WHERE run_id = $1 ORDER BY recorded_at ASC, id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_err)?;

        let mut metrics = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            metrics.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(metrics)
    }

    async fn get_lora_adapter(&self, id: &str) -> Result<LoraAdapter, StorageError> {
        let row = sqlx::query("SELECT data FROM lora_adapters WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let row = row.ok_or_else(|| StorageError::NotFound(format!("LoRA adapter {id} not found")))?;
        let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
        serde_json::from_value(data).map_err(decode_error)
    }

    async fn save_lora_adapter(&self, adapter: &LoraAdapter) -> Result<(), StorageError> {
        self.insert_lora_adapter(adapter, ImportConflict::Overwrite)
            .await?;
        Ok(())
    }

    async fn list_lora_adapters(&self) -> Result<Vec<LoraAdapter>, StorageError> {
        let rows = sqlx::query("SELECT data FROM lora_adapters ORDER BY created_at DESC, id ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_err)?;

        let mut adapters = Vec::with_capacity(rows.len());
        for row in rows {
            let data: serde_json::Value = row.try_get("data").map_err(map_sqlx_err)?;
            adapters.push(serde_json::from_value(data).map_err(decode_error)?);
        }
        Ok(adapters)
    }
}

/// Clamps a harvest counter into the `INT` range of the `harvest_runs` ledger columns.
fn ledger_counter(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[async_trait]
impl HarvestRunLedger for PostgresCatalogRepository {
    async fn start_run(&self, query: &RedditHarvestQuery) -> Result<Option<String>, StorageError> {
        let row = sqlx::query(
            "INSERT INTO harvest_runs (subreddit, sort) VALUES ($1, $2) RETURNING id::text",
        )
        .bind(&query.subreddit)
        .bind(query.sort.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_err)?;

        row.try_get("id").map(Some).map_err(map_sqlx_err)
    }

    async fn finish_run(
        &self,
        run_id: Option<&str>,
        outcome: &HarvestRunOutcome,
    ) -> Result<(), StorageError> {
        let Some(run_id) = run_id else {
            return Ok(());
        };

        sqlx::query(
            "UPDATE harvest_runs \
             SET finished_at = NOW(), posts_scanned = $2, downloaded = $3, skipped = $4, error = $5 \
             WHERE id = $1::uuid",
        )
        .bind(run_id)
        .bind(ledger_counter(outcome.posts_scanned))
        .bind(ledger_counter(outcome.downloaded))
        .bind(ledger_counter(outcome.skipped))
        .bind(outcome.error.as_deref())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;

        Ok(())
    }
}

