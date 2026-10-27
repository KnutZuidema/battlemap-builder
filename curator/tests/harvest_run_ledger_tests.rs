//! Integration tests for the Reddit harvest run ledger (ADR-0011, issue #243).
//!
//! The PostgreSQL tests short-circuit when `DATABASE_URL` is unset, so CI without a database still
//! passes. Set `DATABASE_URL` to run the ledger assertions against a live database; the embedded
//! migrations are applied first and every row this suite writes is removed afterwards.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use curator::adapters::{
    InMemoryBlobStorage, InMemoryCatalogRepository, NoopHarvestRunLedger, PostgresCatalogRepository,
};
use curator::domain::{ParsedTags, RedditHarvestQuery, RedditPost};
use curator::ports::harvest_run_ledger::{HarvestRunLedger, HarvestRunOutcome};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::services::{IngestionProgress, RedditIngestionService};
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

/// Per-test subreddit marker. Keeps concurrent tests isolated inside one shared database.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("it-243-{prefix}-{}-{nanos}-{sequence}", std::process::id())
}

/// Removes every ledger row this suite wrote for `prefix`.
async fn cleanup(repository: &PostgresCatalogRepository, prefix: &str) {
    sqlx::query("DELETE FROM harvest_runs WHERE subreddit LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(repository.pool())
        .await
        .expect("cleanup harvest_runs");
}

/// One decoded `harvest_runs` row.
#[derive(Debug)]
struct LedgerRow {
    finished_at: Option<DateTime<Utc>>,
    subreddit: String,
    sort: String,
    posts_scanned: i32,
    downloaded: i32,
    skipped: i32,
    error: Option<String>,
}

impl LedgerRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        Self {
            finished_at: row.try_get("finished_at").expect("finished_at"),
            subreddit: row.try_get("subreddit").expect("subreddit"),
            sort: row.try_get("sort").expect("sort"),
            posts_scanned: row.try_get("posts_scanned").expect("posts_scanned"),
            downloaded: row.try_get("downloaded").expect("downloaded"),
            skipped: row.try_get("skipped").expect("skipped"),
            error: row.try_get("error").expect("error"),
        }
    }
}

async fn run_row(repository: &PostgresCatalogRepository, run_id: &str) -> LedgerRow {
    let row = sqlx::query(
        "SELECT finished_at, subreddit, sort, posts_scanned, downloaded, skipped, error \
         FROM harvest_runs WHERE id = $1::uuid",
    )
    .bind(run_id)
    .fetch_one(repository.pool())
    .await
    .expect("fetch harvest run by id");
    LedgerRow::from_row(&row)
}

async fn latest_run_row(repository: &PostgresCatalogRepository, subreddit: &str) -> LedgerRow {
    let row = sqlx::query(
        "SELECT finished_at, subreddit, sort, posts_scanned, downloaded, skipped, error \
         FROM harvest_runs WHERE subreddit = $1 ORDER BY started_at DESC, id DESC LIMIT 1",
    )
    .bind(subreddit)
    .fetch_one(repository.pool())
    .await
    .expect("fetch latest harvest run by subreddit");
    LedgerRow::from_row(&row)
}

/// Post carrying no media: ingestion skips it without any network access.
fn post_without_media(id: &str) -> RedditPost {
    RedditPost {
        id: id.to_string(),
        name: format!("t3_{id}"),
        title: "[30x30] Quiet Courtyard".to_string(),
        author: "mapper".to_string(),
        permalink: format!("/r/battlemaps/comments/{id}/"),
        url: format!("https://i.redd.it/{id}.png"),
        score: 1,
        created_utc: Utc::now(),
        media: Vec::new(),
        tags: ParsedTags::parse_title("[30x30] Quiet Courtyard"),
    }
}

/// Harvester returning fixed posts.
struct MockHarvester {
    posts: Vec<RedditPost>,
}

#[async_trait]
impl RedditHarvester for MockHarvester {
    async fn fetch_posts(
        &self,
        _query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        Ok(self.posts.clone())
    }
}

/// Harvester whose fetch always fails, exercising the errored-run path.
struct FailingHarvester;

#[async_trait]
impl RedditHarvester for FailingHarvester {
    async fn fetch_posts(
        &self,
        _query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        Err(HarvesterError::InvalidResponse(
            "reddit unavailable".to_string(),
        ))
    }
}

#[tokio::test]
async fn harvesting_without_a_ledger_still_succeeds() {
    let service = RedditIngestionService::new(
        MockHarvester {
            posts: vec![post_without_media("no-ledger-1")],
        },
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
        Arc::new(NoopHarvestRunLedger),
    );
    let query = RedditHarvestQuery::default();

    let (summary, run_id) = service
        .ingest_query_with_run(&query, false)
        .await
        .expect("harvest without ledger");

    assert_eq!(summary.processed, 1);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.downloaded, 0);
    assert_eq!(run_id, None, "no ledger means no run id");

    let mut events: Vec<IngestionProgress> = Vec::new();
    service
        .ingest_query_progress(&query, false, |progress| events.push(progress))
        .await
        .expect("progress harvest without ledger");

    let complete = events.last().expect("complete event");
    assert_eq!(complete.phase, "complete");
    assert_eq!(complete.run_id, None);
}

#[tokio::test]
async fn ledger_records_run_start_and_completion() {
    let Some(repository) = repository().await else {
        eprintln!("Skipping ledger_records_run_start_and_completion: {ENV_VAR} is not set");
        return;
    };
    let prefix = unique("direct");
    let query = RedditHarvestQuery::default().with_subreddit(&prefix);
    let ledger: Arc<dyn HarvestRunLedger> = Arc::new(repository.clone());

    let run_id = ledger
        .start_run(&query)
        .await
        .expect("record run start")
        .expect("ledger row id");

    let started = run_row(&repository, &run_id).await;
    assert_eq!(started.subreddit, prefix);
    assert_eq!(started.sort, "hot");
    assert!(
        started.finished_at.is_none(),
        "a started run is not finished yet"
    );
    assert_eq!(started.posts_scanned, 0);

    ledger
        .finish_run(Some(&run_id), &HarvestRunOutcome::success(4, 3, 1))
        .await
        .expect("record run completion");

    let finished = run_row(&repository, &run_id).await;
    assert!(finished.finished_at.is_some());
    assert_eq!(finished.posts_scanned, 4);
    assert_eq!(finished.downloaded, 3);
    assert_eq!(finished.skipped, 1);
    assert_eq!(finished.error, None);

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn successful_harvest_persists_its_ledger_entry() {
    let Some(repository) = repository().await else {
        eprintln!("Skipping successful_harvest_persists_its_ledger_entry: {ENV_VAR} is not set");
        return;
    };
    let prefix = unique("success");
    let query = RedditHarvestQuery::default().with_subreddit(&prefix);

    let service = RedditIngestionService::new(
        MockHarvester {
            posts: vec![post_without_media("ledger-success-1")],
        },
        Arc::new(repository.clone()),
        Arc::new(InMemoryBlobStorage::new()),
        Arc::new(repository.clone()),
    );

    let (summary, run_id) = service
        .ingest_query_with_run(&query, false)
        .await
        .expect("successful harvest");
    let run_id = run_id.expect("ledger run id in the harvest result");

    assert_eq!(summary.processed, 1);
    assert_eq!(summary.skipped, 1);

    let row = run_row(&repository, &run_id).await;
    assert_eq!(row.subreddit, prefix);
    assert_eq!(row.sort, "hot");
    assert!(row.finished_at.is_some());
    assert_eq!(row.posts_scanned, 1);
    assert_eq!(row.downloaded, 0);
    assert_eq!(row.skipped, 1);
    assert_eq!(row.error, None);

    // The streaming path carries the run id on the terminal `complete` event.
    let mut events: Vec<IngestionProgress> = Vec::new();
    service
        .ingest_query_progress(&query, false, |progress| events.push(progress))
        .await
        .expect("streaming harvest");
    let complete = events.last().expect("complete event");
    assert_eq!(complete.phase, "complete");
    let streamed_run_id = complete
        .run_id
        .clone()
        .expect("run id on the completion event");
    let streamed = run_row(&repository, &streamed_run_id).await;
    assert!(streamed.finished_at.is_some());
    assert_eq!(streamed.posts_scanned, 1);

    cleanup(&repository, &prefix).await;
}

#[tokio::test]
async fn errored_harvest_records_the_failure() {
    let Some(repository) = repository().await else {
        eprintln!("Skipping errored_harvest_records_the_failure: {ENV_VAR} is not set");
        return;
    };
    let prefix = unique("error");
    let query = RedditHarvestQuery::default().with_subreddit(&prefix);

    let service = RedditIngestionService::new(
        FailingHarvester,
        Arc::new(repository.clone()),
        Arc::new(InMemoryBlobStorage::new()),
        Arc::new(repository.clone()),
    );

    let error = service
        .ingest_query_with_run(&query, false)
        .await
        .expect_err("failing harvester must fail the run");
    assert!(error.to_string().contains("reddit unavailable"));

    let row = latest_run_row(&repository, &prefix).await;
    assert!(
        row.finished_at.is_some(),
        "a failed run is still marked finished"
    );
    assert_eq!(row.posts_scanned, 0);
    assert_eq!(row.downloaded, 0);
    assert_eq!(row.skipped, 0);
    let recorded = row.error.expect("failure recorded on the ledger row");
    assert!(recorded.contains("reddit unavailable"), "recorded: {recorded}");

    cleanup(&repository, &prefix).await;
}
