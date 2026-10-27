//! Integration tests for [`PostgresCatalogRepository`] against a live PostgreSQL database.
//!
//! Every test short-circuits when `DATABASE_URL` is unset, so CI without a database still passes.
//! Set `DATABASE_URL` (for example `postgres://curator:curator@127.0.0.1:5432/curator_test`) to run
//! the suite; the embedded migrations are applied before each test.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use curator::adapters::postgres_catalog_repository::{
    CatalogBackend, PostgresCatalogRepository,
};
use curator::domain::{
    Candidate, CuratedMap, DatasetSnapshot, LoraAdapter, RejectionReason, Source, Theme,
    TrainingMetricSnapshot, TrainingRecipe, TrainingRun, TrainingRunState,
};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use curator::ports::storage_error::StorageError;
use sqlx::Row;

const ENV_VAR: &str = "DATABASE_URL";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Connects to the database named by `DATABASE_URL`, returning `None` when it is not configured.
async fn repository() -> Option<PostgresCatalogRepository> {
    let url = match std::env::var(ENV_VAR) {
        Ok(url) if !url.trim().is_empty() => url,
        _ => return None,
    };
    let repository = PostgresCatalogRepository::connect(url.trim())
        .await
        .expect("connect to DATABASE_URL");
    repository.run_migrations().await.expect("apply migrations");
    Some(repository)
}

/// Per-test identifier prefix. Keeps concurrent tests isolated inside one shared database.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("it-{prefix}-{}-{nanos}-{sequence}", std::process::id())
}

/// Removes every row this test may have written, so the shared database stays clean.
async fn cleanup(repository: &PostgresCatalogRepository, prefix: &str) {
    let like = format!("{prefix}%");
    for statement in [
        "DELETE FROM training_metrics WHERE run_id LIKE $1",
        "DELETE FROM lora_adapters WHERE id LIKE $1",
        "DELETE FROM dataset_snapshots WHERE id LIKE $1",
        "DELETE FROM training_runs WHERE id LIKE $1",
        "DELETE FROM trashed_candidates WHERE id LIKE $1",
        "DELETE FROM rejections WHERE id LIKE $1",
        "DELETE FROM curated_maps WHERE id LIKE $1",
        "DELETE FROM candidates WHERE id LIKE $1",
    ] {
        sqlx::query(statement)
            .bind(&like)
            .execute(repository.pool())
            .await
            .expect("cleanup statement");
    }
}

fn sample_run(id: &str, snapshot_id: &str, created_at: chrono::DateTime<Utc>) -> TrainingRun {
    let recipe = TrainingRecipe::calibration();
    TrainingRun {
        id: id.to_string(),
        remote_job_id: Some(format!("{id}-job")),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot_id.to_string(),
        dataset_digest: "digest".to_string(),
        dataset_url: Some(format!("https://r2.example.com/{id}.zip")),
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at,
        started_at: Some(created_at),
        finished_at: None,
        last_polled_at: None,
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 10,
        total_steps: 100,
        elapsed_seconds: 12.5,
        cost_usd: 0.05,
        checkpoints: Vec::new(),
        published_adapter_ids: Vec::new(),
        error: None,
        failure_code: None,
        metrics: Vec::new(),
    }
}

fn metric(step: usize, timestamp: chrono::DateTime<Utc>) -> TrainingMetricSnapshot {
    TrainingMetricSnapshot {
        timestamp,
        step,
        total_steps: 100,
        elapsed_seconds: step as f64,
        cost_usd: 0.01,
        state: TrainingRunState::Running,
    }
}

fn sample_snapshot(
    id: &str,
    theme: Option<Theme>,
    created_at: chrono::DateTime<Utc>,
) -> DatasetSnapshot {
    DatasetSnapshot::new(
        id,
        theme,
        created_at,
        "sha256",
        1024,
        3,
        30,
        "battlemap",
        format!("datasets/{id}.zip"),
        Some(format!("https://r2.example.com/datasets/{id}.zip")),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend resolution (no database required)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn backend_resolver_prefers_postgres_only_for_non_empty_database_url() {
    assert_eq!(CatalogBackend::resolve(None), CatalogBackend::Filesystem);
    assert_eq!(
        CatalogBackend::resolve(Some("")),
        CatalogBackend::Filesystem
    );
    assert_eq!(
        CatalogBackend::resolve(Some("   ")),
        CatalogBackend::Filesystem
    );
    assert_eq!(
        CatalogBackend::resolve(Some(" postgres://curator@localhost/curator ")),
        CatalogBackend::Postgres("postgres://curator@localhost/curator".to_string())
    );
}

#[tokio::test]
async fn filesystem_fallback_catalog_serves_the_same_port_contract() {
    // With `DATABASE_URL` unset the resolver selects the filesystem backend, which must keep
    // serving the full port contract used by Postgres when a database is configured.
    let catalog = curator::adapters::FilesystemCatalogRepository::new(
        tempfile::tempdir().expect("tempdir").path(),
    );

    let group = unique("fallback-group");
    let mut first = Candidate::new(
        format!("{group}-a"),
        Source::Reddit,
        "a.png",
        (10, 10),
        Utc::now(),
    );
    first.parent_group_id = Some(group.clone());
    let second = Candidate::new(
        format!("{group}-b"),
        Source::Generated,
        "b.png",
        (10, 10),
        Utc::now(),
    );

    catalog.save_candidate(&first).await.expect("save candidate");
    catalog.save_candidate(&second).await.expect("save candidate");
    assert_eq!(
        catalog.get_candidate(&first.id).await.expect("get candidate"),
        first
    );

    let listed = catalog
        .list_candidates(&CandidateFilter {
            source: Some(Source::Reddit),
            parent_group_id: Some(group.clone()),
            ..CandidateFilter::default()
        })
        .await
        .expect("list candidates");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, first.id);

    catalog
        .delete_candidate(&first.id)
        .await
        .expect("delete candidate");
    catalog
        .delete_candidate(&first.id)
        .await
        .expect("delete candidate is idempotent");
    assert!(matches!(
        catalog.get_candidate(&first.id).await,
        Err(StorageError::NotFound(_))
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn migrations_create_indexed_hybrid_schema() {
    let Some(repository) = repository().await else {
        eprintln!("skipping migrations_create_indexed_hybrid_schema: {ENV_VAR} not set");
        return;
    };

    let rows =
        sqlx::query("SELECT indexname, indexdef FROM pg_indexes WHERE schemaname = 'public'")
            .fetch_all(repository.pool())
            .await
            .expect("read pg_indexes");

    let definition = |name: &str| {
        rows.iter()
            .find(|row| {
                row.try_get::<String, _>("indexname")
                    .expect("indexname column")
                    == name
            })
            .map(|row| row.try_get::<String, _>("indexdef").expect("indexdef column"))
            .unwrap_or_else(|| panic!("missing index {name}"))
    };

    assert!(
        definition("idx_curated_tags").to_lowercase().contains("gin"),
        "curated map tags must be GIN indexed"
    );
    assert!(
        definition("idx_candidates_source").contains("source"),
        "candidates.source must be btree indexed"
    );
    assert!(
        definition("idx_candidates_parent").contains("parent_group_id"),
        "candidates.parent_group_id must be btree indexed"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Candidates
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn candidates_support_crud_filters_pagination_and_upserts() {
    let Some(repository) = repository().await else {
        eprintln!("skipping candidates test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("candidates");
    let group = format!("{prefix}-group");

    let mut first = Candidate::new(
        format!("{prefix}-a"),
        Source::Reddit,
        "a.png",
        (10, 10),
        Utc::now(),
    );
    first.parent_group_id = Some(group.clone());
    let mut second = Candidate::new(
        format!("{prefix}-b"),
        Source::Reddit,
        "b.png",
        (10, 10),
        Utc::now(),
    );
    second.parent_group_id = Some(group.clone());
    let mut third = Candidate::new(
        format!("{prefix}-c"),
        Source::Generated,
        "c.png",
        (10, 10),
        Utc::now(),
    );
    third.parent_group_id = Some(group.clone());

    for candidate in [&first, &second, &third] {
        repository.save_candidate(candidate).await.expect("save candidate");
    }

    assert_eq!(
        repository.get_candidate(&first.id).await.expect("get candidate"),
        first
    );

    let all_in_group = repository
        .list_candidates(&CandidateFilter {
            parent_group_id: Some(group.clone()),
            ..CandidateFilter::default()
        })
        .await
        .expect("list by parent group");
    let ids: Vec<&str> = all_in_group.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![first.id.as_str(), second.id.as_str(), third.id.as_str()],
        "list must be ordered by id"
    );

    let reddit_in_group = repository
        .list_candidates(&CandidateFilter {
            source: Some(Source::Reddit),
            parent_group_id: Some(group.clone()),
            ..CandidateFilter::default()
        })
        .await
        .expect("list by source and parent group");
    assert_eq!(reddit_in_group.len(), 2);
    assert!(reddit_in_group.iter().all(|c| c.source == Source::Reddit));

    let paginated = repository
        .list_candidates(&CandidateFilter {
            parent_group_id: Some(group.clone()),
            limit: Some(1),
            offset: Some(1),
            ..CandidateFilter::default()
        })
        .await
        .expect("paginated list");
    assert_eq!(paginated.len(), 1);
    assert_eq!(paginated[0].id, second.id);

    // Saving the same id again upserts instead of duplicating.
    let mut updated = first.clone();
    updated.title = Some("Updated title".to_string());
    repository.save_candidate(&updated).await.expect("upsert candidate");
    assert_eq!(
        repository.get_candidate(&first.id).await.expect("get upserted"),
        updated
    );
    let after_upsert = repository
        .list_candidates(&CandidateFilter {
            parent_group_id: Some(group.clone()),
            ..CandidateFilter::default()
        })
        .await
        .expect("list after upsert");
    assert_eq!(after_upsert.len(), 3);

    repository
        .delete_candidate(&first.id)
        .await
        .expect("delete candidate");
    assert!(matches!(
        repository.get_candidate(&first.id).await,
        Err(StorageError::NotFound(_))
    ));
    repository
        .delete_candidate(&first.id)
        .await
        .expect("second delete is idempotent");

    cleanup(&repository, &prefix).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Curated maps
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn curated_maps_support_theme_tag_id_filters_and_pagination() {
    let Some(repository) = repository().await else {
        eprintln!("skipping curated test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("curated");
    let marker_tag = format!("{prefix}-tag");

    let dungeon = CuratedMap::with_now(
        format!("{prefix}-a"),
        Theme::Dungeon,
        vec![marker_tag.clone()],
        (20, 20),
    );
    let tavern = CuratedMap::with_now(
        format!("{prefix}-b"),
        Theme::Tavern,
        vec![marker_tag.clone()],
        (20, 20),
    );
    // Theme membership is derived from tags, including synonyms such as "structure".
    let building = CuratedMap::with_now(
        format!("{prefix}-c"),
        Theme::Building,
        Vec::new(),
        (20, 20),
    );
    let mut building = building;
    building.tags = vec!["structure".to_string()];

    for map in [&dungeon, &tavern, &building] {
        repository.save_curated(map).await.expect("save curated");
    }

    let dungeon_matches = repository
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Dungeon),
            tags: vec![marker_tag.clone()],
            ..CuratedFilter::default()
        })
        .await
        .expect("filter by theme and tag");
    let ids: Vec<&str> = dungeon_matches.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec![dungeon.id.as_str()]);

    let building_matches = repository
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Building),
            ..CuratedFilter::default()
        })
        .await
        .expect("filter by synonym-derived theme");
    assert!(
        building_matches.iter().any(|m| m.id == building.id),
        "tag `structure` must resolve to the Building theme"
    );

    let tagged = repository
        .list_curated(&CuratedFilter {
            tags: vec![marker_tag.clone()],
            ..CuratedFilter::default()
        })
        .await
        .expect("filter by tag");
    let ids: Vec<&str> = tagged.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec![dungeon.id.as_str(), tavern.id.as_str()]);

    let by_ids = repository
        .list_curated(&CuratedFilter {
            ids: Some(vec![dungeon.id.clone(), building.id.clone()]),
            ..CuratedFilter::default()
        })
        .await
        .expect("filter by ids");
    let ids: Vec<&str> = by_ids.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec![dungeon.id.as_str(), building.id.as_str()]);

    let page = repository
        .list_curated(&CuratedFilter {
            tags: vec![marker_tag.clone()],
            limit: Some(1),
            offset: Some(1),
            ..CuratedFilter::default()
        })
        .await
        .expect("paginated curated list");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, tavern.id);

    let mut updated = dungeon.clone();
    updated.description = Some("A deeper dungeon".to_string());
    repository.save_curated(&updated).await.expect("upsert curated");
    assert_eq!(
        repository
            .get_curated(&dungeon.id)
            .await
            .expect("get upserted curated")
            .description,
        Some("A deeper dungeon".to_string())
    );

    repository
        .delete_curated(&tavern.id)
        .await
        .expect("delete curated");
    assert!(matches!(
        repository.get_curated(&tavern.id).await,
        Err(StorageError::NotFound(_))
    ));
    repository
        .delete_curated(&tavern.id)
        .await
        .expect("second delete is idempotent");

    cleanup(&repository, &prefix).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Rejection log
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rejections_support_logging_lookup_removal_and_latest() {
    let Some(repository) = repository().await else {
        eprintln!("skipping rejections test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("rejection");
    let first_id = format!("{prefix}-a");
    let second_id = format!("{prefix}-b");

    assert!(!repository.is_rejected(&first_id).await.expect("is_rejected"));

    repository
        .log_rejection(&first_id, RejectionReason::GridBurnedIn, Some("grid"))
        .await
        .expect("log rejection");
    repository
        .log_rejection(&second_id, RejectionReason::Irrelevant, None)
        .await
        .expect("log rejection");

    assert!(repository.is_rejected(&first_id).await.expect("is_rejected"));
    assert!(repository.is_rejected(&second_id).await.expect("is_rejected"));

    let records = repository.rejection_records().await.expect("records");
    let ours: Vec<_> = records
        .iter()
        .filter(|record| record.id.starts_with(&prefix))
        .collect();
    assert_eq!(ours.len(), 2);
    assert_eq!(ours[0].id, first_id);
    assert_eq!(ours[1].id, second_id);
    assert_eq!(ours[0].reason, RejectionReason::GridBurnedIn);
    assert_eq!(ours[0].notes.as_deref(), Some("grid"));

    let latest = repository
        .latest_rejection()
        .await
        .expect("latest rejection")
        .expect("some rejection exists");
    assert!(latest.timestamp >= ours[1].timestamp);

    // Logging the same id again upserts the row rather than appending a duplicate.
    repository
        .log_rejection(&first_id, RejectionReason::BadGeometry, None)
        .await
        .expect("re-log rejection");
    let records = repository.rejection_records().await.expect("records");
    let refreshed: Vec<_> = records
        .iter()
        .filter(|record| record.id == first_id)
        .collect();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].reason, RejectionReason::BadGeometry);
    assert_eq!(refreshed[0].notes, None);

    repository
        .remove_rejection(&second_id)
        .await
        .expect("remove rejection");
    assert!(!repository.is_rejected(&second_id).await.expect("is_rejected"));
    repository
        .remove_rejection(&second_id)
        .await
        .expect("second removal is idempotent");

    cleanup(&repository, &prefix).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Trash
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn trashed_candidates_round_trip_and_delete_idempotently() {
    let Some(repository) = repository().await else {
        eprintln!("skipping trash test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("trash");
    let candidate = Candidate::new(
        format!("{prefix}-a"),
        Source::Manual,
        "trash.png",
        (8, 8),
        Utc::now(),
    );

    repository
        .save_trashed_candidate(&candidate)
        .await
        .expect("save trashed");
    assert_eq!(
        repository
            .get_trashed_candidate(&candidate.id)
            .await
            .expect("get trashed"),
        candidate
    );

    repository
        .delete_trashed_candidate(&candidate.id)
        .await
        .expect("delete trashed");
    assert!(matches!(
        repository.get_trashed_candidate(&candidate.id).await,
        Err(StorageError::NotFound(_))
    ));
    repository
        .delete_trashed_candidate(&candidate.id)
        .await
        .expect("second delete is idempotent");

    cleanup(&repository, &prefix).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Dataset snapshots
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dataset_snapshots_filter_by_theme_sort_newest_first_and_track_references() {
    let Some(repository) = repository().await else {
        eprintln!("skipping snapshots test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("snapshot");
    let older = sample_snapshot(
        &format!("{prefix}-a"),
        Some(Theme::Dungeon),
        Utc::now() - Duration::seconds(60),
    );
    let newer = sample_snapshot(
        &format!("{prefix}-b"),
        Some(Theme::Tavern),
        Utc::now(),
    );
    let untagged = sample_snapshot(&format!("{prefix}-c"), None, Utc::now() - Duration::seconds(120));

    for snapshot in [&older, &newer, &untagged] {
        repository
            .save_dataset_snapshot(snapshot)
            .await
            .expect("save snapshot");
    }
    assert_eq!(
        repository
            .get_dataset_snapshot(&older.id)
            .await
            .expect("get snapshot"),
        older
    );

    let dungeon_snapshots = repository
        .list_dataset_snapshots(Some(&Theme::Dungeon))
        .await
        .expect("list by theme");
    assert!(dungeon_snapshots.iter().any(|s| s.id == older.id));
    assert!(!dungeon_snapshots.iter().any(|s| s.id == newer.id));

    let all = repository.list_dataset_snapshots(None).await.expect("list all");
    let newer_position = all
        .iter()
        .position(|s| s.id == newer.id)
        .expect("newer present");
    let older_position = all
        .iter()
        .position(|s| s.id == older.id)
        .expect("older present");
    assert!(
        newer_position < older_position,
        "snapshots must be ordered newest first"
    );

    assert!(!repository
        .is_dataset_snapshot_referenced(&older.id)
        .await
        .expect("unreferenced check"));
    let run = sample_run(&format!("{prefix}-run"), &older.id, Utc::now());
    repository.save_training_run(&run).await.expect("save run");
    assert!(repository
        .is_dataset_snapshot_referenced(&older.id)
        .await
        .expect("referenced check"));

    repository
        .delete_dataset_snapshot(&untagged.id)
        .await
        .expect("delete snapshot");
    assert!(matches!(
        repository.get_dataset_snapshot(&untagged.id).await,
        Err(StorageError::NotFound(_))
    ));
    repository
        .delete_dataset_snapshot(&untagged.id)
        .await
        .expect("second delete is idempotent");

    cleanup(&repository, &prefix).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Training runs, metrics, and LoRA adapters
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn training_runs_metrics_and_adapters_round_trip_with_ordering() {
    let Some(repository) = repository().await else {
        eprintln!("skipping training test: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("training");
    let snapshot_id = format!("{prefix}-snapshot");
    let older_run_id = format!("{prefix}-run-a");
    let newer_run_id = format!("{prefix}-run-b");
    let older_run = sample_run(&older_run_id, &snapshot_id, Utc::now() - Duration::seconds(60));
    let mut newer_run = sample_run(&newer_run_id, &snapshot_id, Utc::now());
    newer_run.state = TrainingRunState::Succeeded;

    repository.save_training_run(&older_run).await.expect("save run");
    repository.save_training_run(&newer_run).await.expect("save run");

    let runs = repository.list_training_runs().await.expect("list runs");
    let newer_position = runs
        .iter()
        .position(|run| run.id == newer_run_id)
        .expect("newer run present");
    let older_position = runs
        .iter()
        .position(|run| run.id == older_run_id)
        .expect("older run present");
    assert!(newer_position < older_position, "runs must be newest first");
    assert_eq!(
        repository.get_training_run(&newer_run_id).await.expect("get run").state,
        TrainingRunState::Succeeded
    );
    assert!(matches!(
        repository.get_training_run(&format!("{prefix}-missing")).await,
        Err(StorageError::NotFound(_))
    ));

    let base = Utc::now();
    for (index, step) in [10usize, 20, 30].into_iter().enumerate() {
        let offset = i64::try_from(index).expect("small index");
        repository
            .append_training_metric(
                &older_run_id,
                &metric(step, base + Duration::seconds(offset)),
            )
            .await
            .expect("append metric");
    }
    let metrics = repository
        .list_training_metrics(&older_run_id)
        .await
        .expect("list metrics");
    let steps: Vec<usize> = metrics.iter().map(|metric| metric.step).collect();
    assert_eq!(steps, vec![10, 20, 30]);

    // Metrics are an append-only log: appending for an unpersisted run is allowed, matching the
    // filesystem and in-memory adapters.
    let orphan_run_id = format!("{prefix}-orphan-run");
    repository
        .append_training_metric(&orphan_run_id, &metric(5, base))
        .await
        .expect("append metric for unpersisted run");
    assert_eq!(
        repository
            .list_training_metrics(&orphan_run_id)
            .await
            .expect("list orphan metrics")
            .len(),
        1
    );
    assert!(repository
        .list_training_metrics(&format!("{prefix}-none"))
        .await
        .expect("list missing metrics")
        .is_empty());

    let recipe = TrainingRecipe::calibration();
    let make_adapter = |suffix: &str, created_at: chrono::DateTime<Utc>| LoraAdapter {
        id: format!("{prefix}-adapter-{suffix}"),
        run_id: older_run_id.clone(),
        step: 10,
        scale: 0.75,
        base_model: recipe.base_model.clone(),
        trigger_phrase: recipe.trigger_phrase.clone(),
        artifact_key: format!("artifacts/{prefix}/{suffix}.safetensors"),
        artifact_url: format!("https://r2.example.com/{prefix}/{suffix}.safetensors"),
        sha256: Some("sha".to_string()),
        size_bytes: Some(512),
        recipe_digest: recipe.digest(),
        dataset_snapshot_id: snapshot_id.clone(),
        dataset_digest: "digest".to_string(),
        created_at,
        retired: false,
        retired_at: None,
    };
    let older_adapter = make_adapter("a", Utc::now() - Duration::seconds(60));
    let newer_adapter = make_adapter("b", Utc::now());
    repository
        .save_lora_adapter(&older_adapter)
        .await
        .expect("save adapter");
    repository
        .save_lora_adapter(&newer_adapter)
        .await
        .expect("save adapter");
    assert_eq!(
        repository
            .get_lora_adapter(&newer_adapter.id)
            .await
            .expect("get adapter"),
        newer_adapter
    );

    let adapters = repository.list_lora_adapters().await.expect("list adapters");
    let newer_position = adapters
        .iter()
        .position(|adapter| adapter.id == newer_adapter.id)
        .expect("newer adapter present");
    let older_position = adapters
        .iter()
        .position(|adapter| adapter.id == older_adapter.id)
        .expect("older adapter present");
    assert!(
        newer_position < older_position,
        "adapters must be newest first"
    );

    let mut retired = older_adapter.clone();
    retired.retired = true;
    retired.retired_at = Some(Utc::now());
    repository
        .save_lora_adapter(&retired)
        .await
        .expect("upsert adapter");
    assert!(repository
        .get_lora_adapter(&older_adapter.id)
        .await
        .expect("get upserted adapter")
        .retired);
    assert!(matches!(
        repository.get_lora_adapter(&format!("{prefix}-missing")).await,
        Err(StorageError::NotFound(_))
    ));

    cleanup(&repository, &prefix).await;
}
