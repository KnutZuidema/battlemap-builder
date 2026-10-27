//! Integration tests for the disk-to-PostgreSQL catalog migration (`migrate-disk-to-postgres`).
//!
//! Database-backed tests short-circuit when `DATABASE_URL` is unset, so CI without a database
//! still passes. Set `DATABASE_URL` (for example
//! `postgres://curator:curator@127.0.0.1:5432/curator_test`) to exercise the import path; the
//! embedded migrations are applied before each test and every row is namespaced with a per-test
//! prefix so concurrent tests can share one database.
//!
//! Discovery and reporting are pure and always run.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use curator::adapters::postgres_catalog_repository::PostgresCatalogRepository;
use curator::domain::{
    BenchmarkMetadata, Candidate, CuratedMap, DatasetSnapshot, LoraAdapter, RejectionReason,
    RejectionRecord, Source, Theme, TrainingMetricSnapshot, TrainingRecipe, TrainingRun,
    TrainingRunState,
};
use curator::ports::catalog_repository::{CatalogRepository, CuratedFilter};
use curator::services::catalog_migration::{
    discover_catalog_entities, run_disk_to_postgres_migration, DiskToPostgresOptions,
    DiskToPostgresReport, EntityMigrationCounts, MigrationFailure, ENTITY_CURATED_MAPS,
};

const ENV_VAR: &str = "DATABASE_URL";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

// ─────────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────────

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent dir");
    fs::write(path, serde_json::to_vec_pretty(value).expect("serialize fixture")).expect("write fixture");
}

fn sample_candidate(id: &str) -> Candidate {
    let mut candidate = Candidate::new(id, Source::Reddit, "crypt.png", (30, 20), Utc::now());
    candidate.title = Some("Crypt Entrance".to_string());
    candidate.author = Some("cartographer".to_string());
    candidate
}

fn sample_curated(id: &str) -> CuratedMap {
    CuratedMap::with_now(id, Theme::Dungeon, vec!["crypt".to_string(), "stone".to_string()], (30, 20))
}

fn sample_snapshot(id: &str) -> DatasetSnapshot {
    DatasetSnapshot::new(
        id,
        Some(Theme::Dungeon),
        Utc::now(),
        "sha256-digest",
        4096,
        1,
        8,
        "battlemap",
        format!("artifacts/datasets/{id}.zip"),
        Some(format!("https://r2.example.com/{id}.zip")),
    )
}

fn sample_run(id: &str, snapshot_id: &str) -> TrainingRun {
    let recipe = TrainingRecipe::calibration();
    TrainingRun {
        id: id.to_string(),
        remote_job_id: Some(format!("{id}-job")),
        state: TrainingRunState::Running,
        dataset_snapshot_id: snapshot_id.to_string(),
        dataset_digest: "dataset-digest".to_string(),
        dataset_url: None,
        recipe: recipe.clone(),
        recipe_digest: recipe.digest(),
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        last_polled_at: None,
        last_poll_error: None,
        consecutive_poll_failures: 0,
        poll_backoff_seconds: 0,
        step: 2,
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

fn sample_metric(step: usize, timestamp: chrono::DateTime<Utc>) -> TrainingMetricSnapshot {
    TrainingMetricSnapshot {
        timestamp,
        step,
        total_steps: 100,
        elapsed_seconds: step as f64,
        cost_usd: 0.01,
        state: TrainingRunState::Running,
    }
}

fn sample_adapter(id: &str, run: &TrainingRun) -> LoraAdapter {
    LoraAdapter {
        id: id.to_string(),
        run_id: run.id.clone(),
        step: 10,
        scale: 0.75,
        base_model: run.recipe.base_model.clone(),
        trigger_phrase: run.recipe.trigger_phrase.clone(),
        artifact_key: format!("artifacts/adapters/{id}.safetensors"),
        artifact_url: format!("https://r2.example.com/adapters/{id}.safetensors"),
        sha256: Some("adapter-sha".to_string()),
        size_bytes: Some(2048),
        recipe_digest: run.recipe.digest(),
        dataset_snapshot_id: run.dataset_snapshot_id.clone(),
        dataset_digest: run.dataset_digest.clone(),
        created_at: Utc::now(),
        retired: false,
        retired_at: None,
    }
}

fn sample_rejection(id: &str, reason: RejectionReason, seconds_ago: i64) -> RejectionRecord {
    RejectionRecord::new(
        id,
        reason,
        Some("reviewer note".to_string()),
        Utc::now() - Duration::seconds(seconds_ago),
    )
}

/// Entity set written to a temporary data directory using the filesystem catalog layout.
struct Fixture {
    candidate: Candidate,
    curated: CuratedMap,
    snapshot: DatasetSnapshot,
    run: TrainingRun,
    adapter: LoraAdapter,
    metrics: Vec<TrainingMetricSnapshot>,
    rejections: Vec<RejectionRecord>,
}

/// Writes one of every catalog entity under `root` using
/// [`curator::adapters::FilesystemCatalogRepository`]'s on-disk conventions.
fn populate_filesystem_catalog(root: &Path, prefix: &str) -> Fixture {
    let candidate = sample_candidate(&format!("{prefix}-candidate"));
    let curated = sample_curated(&format!("{prefix}-curated"));
    let snapshot = sample_snapshot(&format!("{prefix}-snapshot"));
    let run = sample_run(&format!("{prefix}-run"), &snapshot.id);
    let adapter = sample_adapter(&format!("{prefix}-adapter"), &run);
    let metrics = vec![
        sample_metric(1, Utc::now() - Duration::seconds(2)),
        sample_metric(2, Utc::now() - Duration::seconds(1)),
    ];
    let rejections = vec![
        sample_rejection(&format!("{prefix}-rej-a"), RejectionReason::GridBurnedIn, 3),
        sample_rejection(&format!("{prefix}-rej-b"), RejectionReason::Irrelevant, 2),
    ];

    write_json(
        &root.join("staging").join(&candidate.id).join("manifest.json"),
        &candidate,
    );
    write_json(
        &root.join("curated").join(&curated.id).join("manifest.json"),
        &curated,
    );
    write_json(
        &root.join("datasets").join(&snapshot.id).join("snapshot.json"),
        &snapshot,
    );
    write_json(&root.join("training").join(&run.id).join("run.json"), &run);
    let metrics_log: String = metrics
        .iter()
        .map(|metric| serde_json::to_string(metric).expect("serialize metric"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        root.join("training").join(&run.id).join("metrics.jsonl"),
        format!("{metrics_log}\n"),
    )
    .expect("write metrics");
    write_json(
        &root.join("adapters").join(&adapter.id).join("adapter.json"),
        &adapter,
    );
    write_json(&root.join("rejected").join("rejections.json"), &rejections);

    Fixture {
        candidate,
        curated,
        snapshot,
        run,
        adapter,
        metrics,
        rejections,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Database helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Connects to `DATABASE_URL` and applies migrations, returning `None` when it is not configured.
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

/// Per-test identifier prefix, keeping concurrent tests isolated inside one shared database.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("it-{prefix}-{}-{nanos}-{sequence}", std::process::id())
}

/// Removes every row this test may have written.
async fn cleanup(repository: &PostgresCatalogRepository, prefix: &str) {
    let like = format!("{prefix}%");
    for statement in [
        "DELETE FROM training_metrics WHERE run_id LIKE $1",
        "DELETE FROM lora_adapters WHERE id LIKE $1",
        "DELETE FROM dataset_snapshots WHERE id LIKE $1",
        "DELETE FROM training_runs WHERE id LIKE $1",
        "DELETE FROM rejections WHERE candidate_id LIKE $1",
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

async fn count_rows(repository: &PostgresCatalogRepository, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(id)
        .fetch_one(repository.pool())
        .await
        .expect("count rows")
}

fn assert_no_failures(report: &DiskToPostgresReport) {
    assert!(
        !report.has_failures(),
        "migration reported failures:\n{}",
        report.render_summary()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Discovery and reporting (no database required)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn discovery_reads_the_filesystem_catalog_layout_in_append_order() {
    let dir = tempfile::tempdir().expect("temp dir");
    let fixture = populate_filesystem_catalog(dir.path(), "it-discovery");

    let discovery = discover_catalog_entities(dir.path());

    assert!(discovery.failures.is_empty(), "{:?}", discovery.failures);
    assert_eq!(discovery.candidates.len(), 1);
    assert_eq!(discovery.candidates[0].value, fixture.candidate);
    assert_eq!(discovery.curated_maps.len(), 1);
    assert_eq!(discovery.curated_maps[0].value.tags, fixture.curated.tags);
    assert_eq!(discovery.dataset_snapshots.len(), 1);
    assert_eq!(discovery.training_runs.len(), 1);
    assert_eq!(discovery.training_metrics.len(), 2);
    assert_eq!(discovery.training_metrics[0].run_id, fixture.run.id);
    assert_eq!(discovery.training_metrics[0].value.step, 1);
    assert_eq!(discovery.training_metrics[1].value.step, 2);
    assert_eq!(discovery.lora_adapters.len(), 1);
    let rejection_ids: Vec<&str> = discovery
        .rejections
        .iter()
        .map(|item| item.value.id.as_str())
        .collect();
    assert_eq!(
        rejection_ids,
        fixture.rejections.iter().map(|record| record.id.as_str()).collect::<Vec<_>>(),
        "rejection append order must be preserved"
    );
}

#[test]
fn discovery_converts_legacy_generated_benchmark_manifests_to_candidates() {
    let dir = tempfile::tempdir().expect("temp dir");
    let metadata = BenchmarkMetadata::new(
        "it-legacy-bench",
        "openrouter/model",
        "prompt",
        "a prompt",
        None,
        None,
        1200,
        0.01,
    );
    write_json(
        &dir.path().join("staging/it-legacy-bench/manifest.json"),
        &metadata,
    );

    let discovery = discover_catalog_entities(dir.path());

    assert!(discovery.failures.is_empty(), "{:?}", discovery.failures);
    assert_eq!(discovery.candidates.len(), 1);
    assert_eq!(discovery.candidates[0].value.id, "it-legacy-bench");
    assert_eq!(discovery.candidates[0].value.source, Source::Generated);
}

#[test]
fn discovery_records_malformed_manifests_as_failures() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join("curated/it-broken")).expect("create dir");
    fs::write(
        dir.path().join("curated/it-broken/manifest.json"),
        b"{not json",
    )
    .expect("write broken manifest");

    let discovery = discover_catalog_entities(dir.path());

    assert!(discovery.curated_maps.is_empty());
    assert_eq!(discovery.failures.len(), 1);
    let failure = &discovery.failures[0];
    assert_eq!(failure.entity, ENTITY_CURATED_MAPS);
    assert!(
        failure.path.ends_with("curated/it-broken/manifest.json"),
        "{}",
        failure.path
    );
    assert!(failure.reason.contains("invalid JSON"), "{}", failure.reason);
}

#[test]
fn discovery_reads_legacy_jsonl_rejections_before_the_canonical_log() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_json(
        &dir.path().join("rejected/rejections.json"),
        &vec![sample_rejection("it-legacy-d", RejectionReason::GridBurnedIn, 60)],
    );
    let first = serde_json::to_string(&sample_rejection(
        "it-legacy-a",
        RejectionReason::Irrelevant,
        3,
    ))
    .expect("serialize");
    let third = serde_json::to_string(&sample_rejection(
        "it-legacy-c",
        RejectionReason::BlurryLowRes,
        1,
    ))
    .expect("serialize");
    fs::write(
        dir.path().join("rejections.jsonl"),
        format!("{first}\n{{not json}}\n{third}\n"),
    )
    .expect("write jsonl");

    let discovery = discover_catalog_entities(dir.path());
    let ids: Vec<&str> = discovery
        .rejections
        .iter()
        .map(|item| item.value.id.as_str())
        .collect();

    assert_eq!(
        ids,
        vec!["it-legacy-a", "it-legacy-c", "it-legacy-d"],
        "the pre-cutover jsonl log must precede the canonical array"
    );
    assert_eq!(discovery.failures.len(), 1);
    assert!(discovery.failures[0].reason.contains("line 2"));
}

#[test]
fn discovery_of_a_missing_data_dir_is_empty() {
    let discovery = discover_catalog_entities(Path::new("/nonexistent/curator/data"));
    assert!(discovery.failures.is_empty());
    assert!(discovery.candidates.is_empty());
    assert!(discovery.curated_maps.is_empty());
    assert!(discovery.rejections.is_empty());
    assert!(discovery.training_metrics.is_empty());
}

#[test]
fn summary_reports_per_entity_counts_and_failure_details() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join("curated/it-summary")).expect("create dir");
    fs::write(dir.path().join("curated/it-summary/manifest.json"), b"[]").expect("write");

    let discovery = discover_catalog_entities(dir.path());
    let mut report = DiskToPostgresReport {
        completed_at: "2026-10-27T00:00:00Z".to_string(),
        dry_run: true,
        force: false,
        candidates: EntityMigrationCounts::default(),
        curated_maps: EntityMigrationCounts::default(),
        rejections: EntityMigrationCounts::default(),
        dataset_snapshots: EntityMigrationCounts::default(),
        training_runs: EntityMigrationCounts::default(),
        training_metrics: EntityMigrationCounts::default(),
        lora_adapters: EntityMigrationCounts::default(),
        failures: discovery.failures,
    };
    report.candidates.record_import(true);
    report.rejections.record_skip();
    report.failures.push(MigrationFailure::new(
        ENTITY_CURATED_MAPS,
        "data/curated/it-summary/manifest.json",
        "import failed",
    ));
    report.curated_maps.record_failure();

    assert!(report.has_failures());
    let summary = report.render_summary();
    assert!(summary.contains("COMPLETED WITH FAILURES"));
    assert!(summary.contains("candidates"));
    assert!(summary.contains("curated_maps"));
    assert!(summary.contains("data/curated/it-summary/manifest.json"));
    assert!(summary.contains("import failed"));
    assert_eq!(report.failures.len(), 2);
    assert_eq!(report.totals().migrated, 1);
    assert_eq!(report.totals().skipped, 1);
    assert_eq!(report.totals().failed, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Migration against PostgreSQL (skipped without DATABASE_URL)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn migration_imports_every_filesystem_entity_kind() {
    let Some(repository) = repository().await else {
        eprintln!("skipping migration_imports_every_filesystem_entity_kind: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-import");
    let dir = tempfile::tempdir().expect("temp dir");
    let fixture = populate_filesystem_catalog(dir.path(), &prefix);

    let report = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("run migration");

    assert_no_failures(&report);
    assert!(!report.dry_run);
    assert_eq!(report.candidates.migrated, 1);
    assert_eq!(report.curated_maps.migrated, 1);
    assert_eq!(report.rejections.migrated, 2);
    assert_eq!(report.dataset_snapshots.migrated, 1);
    assert_eq!(report.training_runs.migrated, 1);
    assert_eq!(report.training_metrics.migrated, 2);
    assert_eq!(report.lora_adapters.migrated, 1);
    assert_eq!(report.totals().skipped, 0);
    assert_eq!(report.totals().migrated, 9);

    let candidate = repository
        .get_candidate(&fixture.candidate.id)
        .await
        .expect("read candidate");
    assert_eq!(candidate, fixture.candidate);

    let curated = repository
        .get_curated(&fixture.curated.id)
        .await
        .expect("read curated map");
    assert_eq!(curated, fixture.curated, "tags must round-trip");
    let themed = repository
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Dungeon),
            ..CuratedFilter::default()
        })
        .await
        .expect("list dungeon maps");
    assert!(
        themed.iter().any(|map| map.id == fixture.curated.id),
        "theme column must be derived from tags during import"
    );

    let records = repository.rejection_records().await.expect("read rejections");
    let imported_rejections: Vec<&str> = records
        .iter()
        .filter(|record| record.id.starts_with(&prefix))
        .map(|record| record.id.as_str())
        .collect();
    assert_eq!(
        imported_rejections,
        fixture
            .rejections
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        "rejections must keep their append order"
    );

    let snapshots = repository
        .list_dataset_snapshots(Some(&Theme::Dungeon))
        .await
        .expect("list snapshots");
    assert_eq!(
        snapshots
            .iter()
            .find(|snapshot| snapshot.id == fixture.snapshot.id)
            .expect("imported snapshot"),
        &fixture.snapshot
    );

    let runs = repository.list_training_runs().await.expect("list runs");
    assert_eq!(
        runs.iter()
            .find(|run| run.id == fixture.run.id)
            .expect("imported run"),
        &fixture.run
    );

    let metrics = repository
        .list_training_metrics(&fixture.run.id)
        .await
        .expect("list metrics");
    assert_eq!(
        metrics.iter().map(|metric| metric.step).collect::<Vec<_>>(),
        fixture.metrics.iter().map(|metric| metric.step).collect::<Vec<_>>()
    );

    let adapters = repository.list_lora_adapters().await.expect("list adapters");
    assert_eq!(
        adapters
            .iter()
            .find(|adapter| adapter.id == fixture.adapter.id)
            .expect("imported adapter"),
        &fixture.adapter
    );

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn second_migration_run_skips_every_existing_entity() {
    let Some(repository) = repository().await else {
        eprintln!("skipping second_migration_run_skips_every_existing_entity: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-idempotent");
    let dir = tempfile::tempdir().expect("temp dir");
    populate_filesystem_catalog(dir.path(), &prefix);

    let first = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("first migration");
    assert_no_failures(&first);

    let second = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("second migration");

    assert_no_failures(&second);
    assert_eq!(second.totals().migrated, 0, "{}", second.render_summary());
    assert_eq!(second.totals().skipped, first.totals().migrated);
    assert_eq!(second.candidates.migrated, 0);
    assert_eq!(second.candidates.skipped, 1);
    assert_eq!(second.curated_maps.skipped, 1);
    assert_eq!(second.rejections.migrated, 0, "append-only log must not duplicate");
    assert_eq!(second.rejections.skipped, 2);
    assert_eq!(second.dataset_snapshots.skipped, 1);
    assert_eq!(second.training_runs.skipped, 1);
    assert_eq!(second.training_metrics.migrated, 0);
    assert_eq!(second.training_metrics.skipped, 2);
    assert_eq!(second.lora_adapters.skipped, 1);

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn dry_run_reports_work_without_writing() {
    let Some(repository) = repository().await else {
        eprintln!("skipping dry_run_reports_work_without_writing: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-dry");
    let dir = tempfile::tempdir().expect("temp dir");
    let fixture = populate_filesystem_catalog(dir.path(), &prefix);

    let report = run_disk_to_postgres_migration(
        dir.path(),
        &repository,
        DiskToPostgresOptions {
            force: false,
            dry_run: true,
        },
    )
    .await
    .expect("dry run");

    assert!(report.dry_run);
    assert_no_failures(&report);
    assert_eq!(report.candidates.migrated, 1);
    assert_eq!(report.curated_maps.migrated, 1);
    assert_eq!(report.rejections.migrated, 2);
    assert_eq!(report.training_metrics.migrated, 2);
    assert_eq!(report.totals().skipped, 0);

    assert_eq!(
        count_rows(
            &repository,
            "SELECT COUNT(*) FROM candidates WHERE id = $1",
            &fixture.candidate.id,
        )
        .await,
        0
    );
    assert_eq!(
        count_rows(
            &repository,
            "SELECT COUNT(*) FROM curated_maps WHERE id = $1",
            &fixture.curated.id,
        )
        .await,
        0
    );
    assert_eq!(
        count_rows(
            &repository,
            "SELECT COUNT(*) FROM training_metrics WHERE run_id = $1",
            &fixture.run.id,
        )
        .await,
        0
    );
    assert_eq!(
        count_rows(
            &repository,
            "SELECT COUNT(*) FROM rejections WHERE candidate_id = $1",
            &fixture.rejections[0].id,
        )
        .await,
        0
    );

    let index = repository.import_index().await.expect("import index");
    assert!(!index.candidate_ids.contains(&fixture.candidate.id));
    assert!(!index.curated_ids.contains(&fixture.curated.id));
    assert!(!index.training_run_ids.contains(&fixture.run.id));
    assert!(!index.dataset_snapshot_ids.contains(&fixture.snapshot.id));
    assert!(!index.lora_adapter_ids.contains(&fixture.adapter.id));

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn dry_run_with_force_previews_the_overwrites() {
    let Some(repository) = repository().await else {
        eprintln!("skipping dry_run_with_force_previews_the_overwrites: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-dry-force");
    let dir = tempfile::tempdir().expect("temp dir");
    let fixture = populate_filesystem_catalog(dir.path(), &prefix);

    let seeded = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("initial migration");
    assert_no_failures(&seeded);

    let mut renamed = fixture.candidate.clone();
    renamed.title = Some("Previewed Rename".to_string());
    write_json(
        &dir.path()
            .join("staging")
            .join(&renamed.id)
            .join("manifest.json"),
        &renamed,
    );

    let preview = run_disk_to_postgres_migration(
        dir.path(),
        &repository,
        DiskToPostgresOptions {
            force: true,
            dry_run: true,
        },
    )
    .await
    .expect("forced dry run");

    assert!(preview.dry_run);
    assert!(preview.force);
    assert_no_failures(&preview);
    // Existing primary-key rows would be overwritten, so the preview counts them as migrated.
    assert_eq!(preview.candidates.migrated, 1);
    assert_eq!(preview.curated_maps.migrated, 1);
    assert_eq!(preview.dataset_snapshots.migrated, 1);
    assert_eq!(preview.training_runs.migrated, 1);
    assert_eq!(preview.lora_adapters.migrated, 1);
    // The append-only logs ignore --force and stay deduplicated.
    assert_eq!(preview.rejections.migrated, 0);
    assert_eq!(preview.rejections.skipped, 2);
    assert_eq!(preview.training_metrics.migrated, 0);
    assert_eq!(preview.training_metrics.skipped, 2);
    assert_eq!(preview.totals().skipped, 4);

    assert_eq!(
        repository
            .get_candidate(&fixture.candidate.id)
            .await
            .expect("read previewed candidate")
            .title,
        fixture.candidate.title,
        "a dry run must not write"
    );

    let forced = run_disk_to_postgres_migration(
        dir.path(),
        &repository,
        DiskToPostgresOptions {
            force: true,
            dry_run: false,
        },
    )
    .await
    .expect("forced migration");
    assert_eq!(forced.totals(), preview.totals());
    assert_eq!(
        repository
            .get_candidate(&fixture.candidate.id)
            .await
            .expect("read forced candidate")
            .title,
        renamed.title
    );

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn migration_reports_malformed_manifests_and_continues() {
    let Some(repository) = repository().await else {
        eprintln!("skipping migration_reports_malformed_manifests_and_continues: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-broken");
    let dir = tempfile::tempdir().expect("temp dir");
    let fixture = populate_filesystem_catalog(dir.path(), &prefix);
    let broken = dir
        .path()
        .join("staging")
        .join(format!("{prefix}-broken"))
        .join("manifest.json");
    fs::create_dir_all(broken.parent().expect("parent")).expect("create dir");
    fs::write(&broken, b"{not json").expect("write broken manifest");

    let report = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("migration tolerates malformed manifests");

    assert!(report.has_failures());
    assert_eq!(report.candidates.migrated, 1);
    assert_eq!(report.candidates.failed, 1);
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0].path.ends_with(&format!("{prefix}-broken/manifest.json")),
        "{}",
        report.failures[0].path
    );
    assert!(report
        .failures[0]
        .reason
        .contains("neither a Candidate nor a legacy BenchmarkMetadata"));
    assert!(report.render_summary().contains("COMPLETED WITH FAILURES"));

    assert!(repository
        .get_candidate(&fixture.candidate.id)
        .await
        .is_ok());

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn force_reimports_and_overwrites_stored_rows() {
    let Some(repository) = repository().await else {
        eprintln!("skipping force_reimports_and_overwrites_stored_rows: {ENV_VAR} not set");
        return;
    };
    let prefix = unique("diskpg-force");
    let dir = tempfile::tempdir().expect("temp dir");
    let mut fixture = populate_filesystem_catalog(dir.path(), &prefix);

    let first = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("initial migration");
    assert_no_failures(&first);
    assert_eq!(first.candidates.migrated, 1);

    let skipped = run_disk_to_postgres_migration(dir.path(), &repository, DiskToPostgresOptions::default())
        .await
        .expect("repeat migration");
    assert_eq!(skipped.candidates.migrated, 0);
    assert_eq!(skipped.candidates.skipped, 1);

    let mut renamed = fixture.candidate.clone();
    renamed.title = Some("Renamed Crypt".to_string());
    write_json(
        &dir.path()
            .join("staging")
            .join(&renamed.id)
            .join("manifest.json"),
        &renamed,
    );
    fixture.candidate = renamed;

    let forced = run_disk_to_postgres_migration(
        dir.path(),
        &repository,
        DiskToPostgresOptions {
            force: true,
            dry_run: false,
        },
    )
    .await
    .expect("forced migration");

    assert_no_failures(&forced);
    assert!(forced.force);
    assert_eq!(forced.candidates.migrated, 1);
    assert_eq!(forced.candidates.skipped, 0);
    assert_eq!(forced.curated_maps.migrated, 1);
    assert_eq!(forced.training_runs.migrated, 1);

    assert_eq!(
        repository
            .get_candidate(&fixture.candidate.id)
            .await
            .expect("read candidate"),
        fixture.candidate,
        "--force must overwrite the stored row"
    );

    assert_eq!(forced.rejections.migrated, 0, "append-only log stays deduplicated");
    assert_eq!(forced.rejections.skipped, 2);
    assert_eq!(
        count_rows(
            &repository,
            "SELECT COUNT(*) FROM rejections WHERE candidate_id = $1",
            &fixture.rejections[0].id,
        )
        .await,
        1
    );

    cleanup(&repository, &prefix).await;
}
