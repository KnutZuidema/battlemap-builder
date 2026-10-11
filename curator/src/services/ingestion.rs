use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::adapters::reddit::{unescape_xml_entities, DEFAULT_USER_AGENT};
use crate::domain::{
    format_source_url, RedditHarvestQuery, RedditPost, RedditSidecarMetadata, Source,
};
use crate::ports::harvester::{HarvesterError, RedditHarvester};
use crate::ports::storage::{StorageError, StorageRepository};

/// Summary of an ingestion execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IngestionSummary {
    pub processed: usize,
    pub downloaded: usize,
    pub skipped: usize,
}

impl IngestionSummary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_downloaded(&mut self) {
        self.processed += 1;
        self.downloaded += 1;
    }

    pub fn record_skipped(&mut self) {
        self.processed += 1;
        self.skipped += 1;
    }
}

/// Outcome of processing a single media item from a Reddit post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestionOutcome {
    Downloaded(String),
    Skipped { id: String, reason: SkipReason },
}

/// Reason for skipping an item during ingestion deduplication checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    Rejected,
    AlreadyInStaging,
    AlreadyCurated,
    NoMedia,
    InvalidMediaIndex,
    NotGridless,
}

/// Progress event emitted during Reddit harvesting pipeline execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionProgress {
    pub phase: String,
    pub message: String,
    pub processed: usize,
    pub downloaded: usize,
    pub skipped: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
}

impl IngestionProgress {
    pub fn new(
        phase: impl Into<String>,
        message: impl Into<String>,
        processed: usize,
        downloaded: usize,
        skipped: usize,
        total: usize,
    ) -> Self {
        Self {
            phase: phase.into(),
            message: message.into(),
            processed,
            downloaded,
            skipped,
            total,
            candidate_id: None,
        }
    }

    pub fn with_candidate_id(mut self, candidate_id: impl Into<String>) -> Self {
        self.candidate_id = Some(candidate_id.into());
        self
    }
}

/// Errors arising during ingestion pipeline operations.
#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    #[error("Harvester error: {0}")]
    Harvester(#[from] HarvesterError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Derives candidate identifier for a media item of a Reddit post.
/// For single-image posts, returns the post ID directly (`abc123`).
/// For multi-image gallery posts, returns `{post_id}_{media_index}` (`abc123_0`).
pub fn candidate_id_for_post(post_id: &str, media_index: usize, total_media: usize) -> String {
    if total_media <= 1 {
        post_id.to_string()
    } else {
        format!("{post_id}_{media_index}")
    }
}

/// Service orchestrating Reddit crawling, deduplication checks, image downloading,
/// and persisting staged candidates with companion sidecar metadata.
#[derive(Debug, Clone)]
pub struct RedditIngestionService<H, S> {
    harvester: H,
    storage: S,
    http_client: reqwest::Client,
}

impl<H, S> RedditIngestionService<H, S> {
    pub fn new(harvester: H, storage: S) -> Self {
        let http_client = reqwest::Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            harvester,
            storage,
            http_client,
        }
    }

    pub fn with_http_client(harvester: H, storage: S, http_client: reqwest::Client) -> Self {
        Self {
            harvester,
            storage,
            http_client,
        }
    }

    pub fn harvester(&self) -> &H {
        &self.harvester
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }
}

impl<H: RedditHarvester, S: StorageRepository> RedditIngestionService<H, S> {
    /// Ingest battlemaps matching a Reddit query.
    pub async fn ingest_query(
        &self,
        query: &RedditHarvestQuery,
    ) -> Result<IngestionSummary, IngestionError> {
        self.ingest_query_filtered(query, false).await
    }

    /// Ingest battlemaps matching a Reddit query with optional gridless-only filtering.
    pub async fn ingest_query_filtered(
        &self,
        query: &RedditHarvestQuery,
        gridless_only: bool,
    ) -> Result<IngestionSummary, IngestionError> {
        let posts = self.harvester.fetch_posts(query).await?;
        self.ingest_posts_filtered(&posts, gridless_only).await
    }

    /// Ingest battlemaps matching a Reddit query with real-time progress updates.
    pub async fn ingest_query_progress<F>(
        &self,
        query: &RedditHarvestQuery,
        gridless_only: bool,
        mut on_progress: F,
    ) -> Result<IngestionSummary, IngestionError>
    where
        F: FnMut(IngestionProgress) + Send,
    {
        on_progress(IngestionProgress::new(
            "fetching",
            format!(
                "Fetching posts from r/{} (sort: {}, limit: {})...",
                query.subreddit, query.sort, query.limit
            ),
            0,
            0,
            0,
            0,
        ));

        let posts = self.harvester.fetch_posts(query).await?;
        let total_items: usize = posts
            .iter()
            .map(|p| if p.media.is_empty() { 1 } else { p.media.len() })
            .sum();

        if posts.is_empty() {
            let summary = IngestionSummary::default();
            on_progress(IngestionProgress::new(
                "complete",
                format!("No posts found in r/{}", query.subreddit),
                0,
                0,
                0,
                0,
            ));
            return Ok(summary);
        }

        let mut summary = IngestionSummary::default();

        for post in &posts {
            if post.media.is_empty() {
                summary.record_skipped();
                on_progress(
                    IngestionProgress::new(
                        "skipped",
                        format!(
                            "Harvesting: {}/{} maps downloaded ({} duplicates skipped)...",
                            summary.downloaded, total_items, summary.skipped
                        ),
                        summary.processed,
                        summary.downloaded,
                        summary.skipped,
                        total_items,
                    )
                    .with_candidate_id(post.id.clone()),
                );
                continue;
            }

            for idx in 0..post.media.len() {
                match self.ingest_media_filtered(post, idx, gridless_only).await? {
                    IngestionOutcome::Downloaded(cid) => {
                        summary.record_downloaded();
                        on_progress(
                            IngestionProgress::new(
                                "downloaded",
                                format!(
                                    "Harvesting: {}/{} maps downloaded ({} duplicates skipped)...",
                                    summary.downloaded, total_items, summary.skipped
                                ),
                                summary.processed,
                                summary.downloaded,
                                summary.skipped,
                                total_items,
                            )
                            .with_candidate_id(cid),
                        );
                    }
                    IngestionOutcome::Skipped { id, reason: _ } => {
                        summary.record_skipped();
                        on_progress(
                            IngestionProgress::new(
                                "skipped",
                                format!(
                                    "Harvesting: {}/{} maps downloaded ({} duplicates skipped)...",
                                    summary.downloaded, total_items, summary.skipped
                                ),
                                summary.processed,
                                summary.downloaded,
                                summary.skipped,
                                total_items,
                            )
                            .with_candidate_id(id),
                        );
                    }
                }
            }
        }

        on_progress(IngestionProgress::new(
            "complete",
            format!(
                "Harvest complete: {} maps downloaded ({} duplicates skipped)...",
                summary.downloaded, summary.skipped
            ),
            summary.processed,
            summary.downloaded,
            summary.skipped,
            total_items,
        ));

        Ok(summary)
    }

    /// Ingest a batch of Reddit posts without filter.
    pub async fn ingest_posts(
        &self,
        posts: &[RedditPost],
    ) -> Result<IngestionSummary, IngestionError> {
        self.ingest_posts_filtered(posts, false).await
    }

    /// Ingest a batch of Reddit posts with optional gridless-only filter.
    pub async fn ingest_posts_filtered(
        &self,
        posts: &[RedditPost],
        gridless_only: bool,
    ) -> Result<IngestionSummary, IngestionError> {
        let mut summary = IngestionSummary::default();
        for post in posts {
            let post_summary = self.ingest_post_filtered(post, gridless_only).await?;
            summary.processed += post_summary.processed;
            summary.downloaded += post_summary.downloaded;
            summary.skipped += post_summary.skipped;
        }
        Ok(summary)
    }

    /// Ingest all media items from a single Reddit post without filter.
    pub async fn ingest_post(&self, post: &RedditPost) -> Result<IngestionSummary, IngestionError> {
        self.ingest_post_filtered(post, false).await
    }

    /// Ingest all media items from a single Reddit post with optional gridless-only filter.
    pub async fn ingest_post_filtered(
        &self,
        post: &RedditPost,
        gridless_only: bool,
    ) -> Result<IngestionSummary, IngestionError> {
        let mut summary = IngestionSummary::default();
        if post.media.is_empty() {
            summary.record_skipped();
            return Ok(summary);
        }

        for idx in 0..post.media.len() {
            match self.ingest_media_filtered(post, idx, gridless_only).await? {
                IngestionOutcome::Downloaded(_) => summary.record_downloaded(),
                IngestionOutcome::Skipped { .. } => summary.record_skipped(),
            }
        }

        Ok(summary)
    }

    /// Ingest a specific media item from a Reddit post without filter.
    pub async fn ingest_media(
        &self,
        post: &RedditPost,
        media_index: usize,
    ) -> Result<IngestionOutcome, IngestionError> {
        self.ingest_media_filtered(post, media_index, false).await
    }

    /// Ingest a specific media item from a Reddit post with optional gridless-only filter.
    pub async fn ingest_media_filtered(
        &self,
        post: &RedditPost,
        media_index: usize,
        gridless_only: bool,
    ) -> Result<IngestionOutcome, IngestionError> {
        let media = match post.media.get(media_index) {
            Some(m) => m,
            None => {
                return Ok(IngestionOutcome::Skipped {
                    id: format!("{}_{}", post.id, media_index),
                    reason: SkipReason::InvalidMediaIndex,
                });
            }
        };

        let candidate_id = candidate_id_for_post(&post.id, media_index, post.media.len());

        // 0. Filter if gridless only requested and post is not gridless
        if gridless_only && !post.tags.is_gridless() {
            return Ok(IngestionOutcome::Skipped {
                id: candidate_id,
                reason: SkipReason::NotGridless,
            });
        }

        // 1. Check if candidate ID or post base ID has been rejected
        if self.storage.is_rejected(&candidate_id)? || self.storage.is_rejected(&post.id)? {
            return Ok(IngestionOutcome::Skipped {
                id: candidate_id,
                reason: SkipReason::Rejected,
            });
        }

        // 2. Check if candidate ID or post base ID is already in staging
        if self.storage.is_in_staging(&candidate_id)? || self.storage.is_in_staging(&post.id)? {
            return Ok(IngestionOutcome::Skipped {
                id: candidate_id,
                reason: SkipReason::AlreadyInStaging,
            });
        }

        // 3. Check if candidate ID or post base ID is already in curated
        if self.storage.is_curated(&candidate_id)? || self.storage.is_curated(&post.id)? {
            return Ok(IngestionOutcome::Skipped {
                id: candidate_id,
                reason: SkipReason::AlreadyCurated,
            });
        }

        // 4. Download image bytes
        let download_url = unescape_xml_entities(&media.url);
        let response = self.http_client.get(&download_url).send().await?;
        if !response.status().is_success() {
            return Err(IngestionError::Http(
                response.error_for_status().unwrap_err(),
            ));
        }

        let image_bytes = response.bytes().await?;

        // 5. Build sidecar metadata according to Issue #25 schema
        let source_url = format_source_url(&post.permalink, &media.url);
        let sidecar = RedditSidecarMetadata {
            id: candidate_id.clone(),
            source: Source::Reddit,
            source_url,
            title: post.title.clone(),
            author: post.author.clone(),
            parsed_gridless: post.tags.is_gridless(),
            parsed_dimensions: post.tags.dimensions,
            created_at: Utc::now(),
        };

        // 6. Persist to data/staging/reddit/{reddit_id}.png and {reddit_id}.json
        self.storage.save_staging_reddit(&sidecar, &image_bytes)?;

        Ok(IngestionOutcome::Downloaded(candidate_id))
    }
}
