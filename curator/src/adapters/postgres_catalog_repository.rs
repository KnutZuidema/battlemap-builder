//! PostgreSQL-backed [`CatalogRepository`] storing domain entities as indexed JSONB documents (ADR-0011).

use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::domain::{
    Candidate, CuratedMap, DatasetSnapshot, LoraAdapter, RejectionReason, RejectionRecord,
    Theme, TrainingMetricSnapshot, TrainingRun,
};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
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
        let source_str = candidate.source.to_string();
        let data = serde_json::to_value(candidate)?;
        sqlx::query(
            r#"
            INSERT INTO candidates (id, source, parent_group_id, created_at, data)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                source = EXCLUDED.source,
                parent_group_id = EXCLUDED.parent_group_id,
                created_at = EXCLUDED.created_at,
                data = EXCLUDED.data
            "#,
        )
        .bind(&candidate.id)
        .bind(&source_str)
        .bind(candidate.parent_group_id.as_deref())
        .bind(candidate.created_at)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
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
        let tags: Vec<String> = map.tags.clone();
        let themes: Vec<String> = tags
            .iter()
            .filter_map(|tag| tag.parse::<Theme>().ok())
            .map(|theme| theme.as_str().to_string())
            .collect();
        let data = serde_json::to_value(map)?;
        sqlx::query(
            r#"
            INSERT INTO curated_maps (id, tags, themes, created_at, data)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                tags = EXCLUDED.tags,
                themes = EXCLUDED.themes,
                data = EXCLUDED.data
            "#,
        )
        .bind(&map.id)
        .bind(&tags)
        .bind(&themes)
        .bind(map.curated_at)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
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
        let theme_str = snapshot.theme.as_ref().map(|t| t.as_str().to_string());
        let data = serde_json::to_value(snapshot)?;
        sqlx::query(
            r#"
            INSERT INTO dataset_snapshots (id, theme, created_at, data)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET
                theme = EXCLUDED.theme,
                created_at = EXCLUDED.created_at,
                data = EXCLUDED.data
            "#,
        )
        .bind(&snapshot.id)
        .bind(theme_str)
        .bind(snapshot.created_at)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
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
        let data = serde_json::to_value(run)?;
        sqlx::query(
            r#"
            INSERT INTO training_runs (id, dataset_snapshot_id, created_at, data)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET
                dataset_snapshot_id = EXCLUDED.dataset_snapshot_id,
                created_at = EXCLUDED.created_at,
                data = EXCLUDED.data
            "#,
        )
        .bind(&run.id)
        .bind(&run.dataset_snapshot_id)
        .bind(run.created_at)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
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
        let data = serde_json::to_value(adapter)?;
        sqlx::query(
            r#"
            INSERT INTO lora_adapters (id, created_at, data)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO UPDATE SET
                created_at = EXCLUDED.created_at,
                data = EXCLUDED.data
            "#,
        )
        .bind(&adapter.id)
        .bind(adapter.created_at)
        .bind(data)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_err)?;
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
