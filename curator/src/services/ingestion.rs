use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

pub use crate::adapters::reddit::BackoffConfig;
use crate::adapters::reddit::{
    parse_rate_limit_delay, unescape_xml_entities, DEFAULT_USER_AGENT,
};
use crate::domain::{Candidate, GridConfig, RedditHarvestQuery, RedditPost};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::harvester::{HarvesterError, RedditHarvester};
use crate::ports::storage_error::StorageError;
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
    DownloadFailed,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
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
            wait_seconds: None,
            attempt: None,
            max_retries: None,
        }
    }

    pub fn with_candidate_id(mut self, candidate_id: impl Into<String>) -> Self {
        self.candidate_id = Some(candidate_id.into());
        self
    }

    pub fn with_rate_limit(mut self, wait_seconds: u64, attempt: u32, max_retries: u32) -> Self {
        self.wait_seconds = Some(wait_seconds);
        self.attempt = Some(attempt);
        self.max_retries = Some(max_retries);
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
/// and persisting staged candidates with companion layer blobs.
#[derive(Debug, Clone)]
pub struct RedditIngestionService<H, C, B> {
    harvester: H,
    catalog: C,
    blobs: B,
    http_client: reqwest::Client,
    backoff: BackoffConfig,
}

impl<H, C, B> RedditIngestionService<H, C, B> {
    pub fn new(harvester: H, catalog: C, blobs: B) -> Self {
        let http_client = reqwest::Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            harvester,
            catalog,
            blobs,
            http_client,
            backoff: Self::backoff_from_env(),
        }
    }

    fn backoff_from_env() -> BackoffConfig {
        let mut backoff = BackoffConfig::default();

        if let Ok(initial_secs_str) = std::env::var("REDDIT_BACKOFF_INITIAL_SECS") {
            if let Ok(secs) = initial_secs_str.trim().parse::<u64>() {
                backoff.initial_delay = Duration::from_secs(secs);
            }
        }

        if let Ok(max_retries_str) = std::env::var("REDDIT_BACKOFF_MAX_RETRIES") {
            if let Ok(retries) = max_retries_str.trim().parse::<u32>() {
                backoff.max_retries = retries;
            }
        }

        if let Ok(factor_str) = std::env::var("REDDIT_BACKOFF_FACTOR") {
            if let Ok(factor) = factor_str.trim().parse::<f64>() {
                backoff.backoff_factor = factor;
            }
        }

        backoff
    }

    pub fn with_http_client(harvester: H, catalog: C, blobs: B, http_client: reqwest::Client) -> Self {
        let mut service = Self::new(harvester, catalog, blobs);
        service.http_client = http_client;
        service
    }

    pub fn with_backoff(mut self, backoff: BackoffConfig) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn harvester(&self) -> &H {
        &self.harvester
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub fn backoff(&self) -> &BackoffConfig {
        &self.backoff
    }
}


impl<H: RedditHarvester, C: CatalogRepository, B: BlobStorage> RedditIngestionService<H, C, B> {
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
        info!(
            subreddit = %query.subreddit,
            sort = %query.sort,
            limit = query.limit,
            gridless_only = gridless_only,
            "Starting Reddit harvest"
        );
        let posts = self.harvester.fetch_posts(query).await?;
        info!(
            subreddit = %query.subreddit,
            count = posts.len(),
            "Fetched raw posts from Reddit"
        );
        let summary = self.ingest_posts_filtered(&posts, gridless_only).await?;
        info!(
            downloaded = summary.downloaded,
            skipped = summary.skipped,
            processed = summary.processed,
            errors = 0,
            "Reddit harvest completed"
        );
        Ok(summary)
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
        info!(
            subreddit = %query.subreddit,
            sort = %query.sort,
            limit = query.limit,
            gridless_only = gridless_only,
            "Starting Reddit harvest"
        );
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

        let posts = match self.harvester.fetch_posts(query).await {
            Ok(posts) => {
                info!(
                    subreddit = %query.subreddit,
                    count = posts.len(),
                    "Fetched raw posts from Reddit"
                );
                posts
            }
            Err(e) => {
                error!(error = %e, "Failed to fetch posts from Reddit");
                return Err(e.into());
            }
        };
        let total_items = posts.len();

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
            let mut rate_limit_hook =
                |cid: &str, wait_time: Duration, attempt: u32, max_retries: u32| {
                    let wait_secs =
                        if wait_time.as_secs() == 0 && wait_time.subsec_millis() == 0 {
                            0
                        } else {
                            wait_time.as_secs().max(1)
                        };
                    on_progress(
                        IngestionProgress::new(
                            "rate_limited",
                            format!(
                                "Rate limited downloading candidate {cid}; waiting {wait_secs}s (attempt {attempt}/{max_retries})..."
                            ),
                            summary.processed,
                            summary.downloaded,
                            summary.skipped,
                            total_items,
                        )
                        .with_candidate_id(cid)
                        .with_rate_limit(wait_secs, attempt, max_retries),
                    );
                };

            let outcome = match self
                .ingest_post_filtered_inner(
                    post,
                    gridless_only,
                    Some(&mut rate_limit_hook),
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    warn!(
                        post_id = %post.id,
                        error = %e,
                        "Bundle ingestion failed, skipping candidate"
                    );
                    IngestionOutcome::Skipped {
                        id: post.id.clone(),
                        reason: SkipReason::DownloadFailed,
                    }
                }
            };

            match outcome {
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
        info!(
            downloaded = summary.downloaded,
            skipped = summary.skipped,
            processed = summary.processed,
            errors = 0,
            "Reddit harvest completed"
        );

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
        match self
            .ingest_post_filtered_inner(
                post,
                gridless_only,
                None::<fn(&str, Duration, u32, u32)>,
            )
            .await
        {
            Ok(IngestionOutcome::Downloaded(_)) => summary.record_downloaded(),
            Ok(IngestionOutcome::Skipped { .. }) => summary.record_skipped(),
            Err(e) => {
                warn!(
                    post_id = %post.id,
                    error = %e,
                    "Bundle ingestion failed, skipping post"
                );
                summary.record_skipped();
            }
        }
        Ok(summary)
    }

    /// Ingest a Reddit post as a multi-layer map bundle without filter.
    pub async fn ingest_post_bundle(
        &self,
        post: &RedditPost,
    ) -> Result<IngestionOutcome, IngestionError> {
        self.ingest_post_bundle_filtered(post, false).await
    }

    /// Ingest a Reddit post as a multi-layer map bundle with optional gridless-only filter.
    pub async fn ingest_post_bundle_filtered(
        &self,
        post: &RedditPost,
        gridless_only: bool,
    ) -> Result<IngestionOutcome, IngestionError> {
        self.ingest_post_filtered_inner(
            post,
            gridless_only,
            None::<fn(&str, Duration, u32, u32)>,
        )
        .await
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
        if media_index >= post.media.len() {
            return Ok(IngestionOutcome::Skipped {
                id: format!("{}_{}", post.id, media_index),
                reason: SkipReason::InvalidMediaIndex,
            });
        }
        self.ingest_post_filtered_inner(
            post,
            gridless_only,
            None::<fn(&str, Duration, u32, u32)>,
        )
        .await
    }

    async fn check_deduplication(&self, post: &RedditPost) -> Result<Option<SkipReason>, StorageError> {
        let child_0 = format!("{}_0", post.id);

        // 1. Check if post or any split child was rejected
        if self.catalog.is_rejected(&post.id).await? || self.catalog.is_rejected(&child_0).await? {
            return Ok(Some(SkipReason::Rejected));
        }
        for idx in 1..post.media.len() {
            let child = format!("{}_{idx}", post.id);
            if self.catalog.is_rejected(&child).await? {
                return Ok(Some(SkipReason::Rejected));
            }
        }

        // 2. Check if post or any split child is in staging
        if self.catalog.get_candidate(&post.id).await.is_ok()
            || self.catalog.get_candidate(&child_0).await.is_ok()
        {
            return Ok(Some(SkipReason::AlreadyInStaging));
        }
        for idx in 1..post.media.len() {
            let child = format!("{}_{idx}", post.id);
            if self.catalog.get_candidate(&child).await.is_ok() {
                return Ok(Some(SkipReason::AlreadyInStaging));
            }
        }

        // 3. Check if post or any split child is in curated
        if self.catalog.get_curated(&post.id).await.is_ok()
            || self.catalog.get_curated(&child_0).await.is_ok()
        {
            return Ok(Some(SkipReason::AlreadyCurated));
        }
        for idx in 1..post.media.len() {
            let child = format!("{}_{idx}", post.id);
            if self.catalog.get_curated(&child).await.is_ok() {
                return Ok(Some(SkipReason::AlreadyCurated));
            }
        }

        Ok(None)
    }

    async fn download_media_bytes<Hook>(
        &self,
        post_id: &str,
        media_url: &str,
        mut on_rate_limit: Option<&mut Hook>,
    ) -> Result<Vec<u8>, SkipReason>
    where
        Hook: FnMut(&str, Duration, u32, u32) + Send,
    {
        let download_url = unescape_xml_entities(media_url);
        let mut attempts: u32 = 0;
        let mut delay = self.backoff.initial_delay;

        loop {
            let response = match self.http_client.get(&download_url).send().await {
                Ok(res) => res,
                Err(err) => {
                    warn!(
                        post_id = %post_id,
                        media_url = %media_url,
                        error = %err,
                        "Failed to request image download"
                    );
                    return Err(SkipReason::DownloadFailed);
                }
            };

            let status = response.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if attempts >= self.backoff.max_retries {
                    warn!(
                        post_id = %post_id,
                        media_url = %media_url,
                        retries = attempts,
                        "Exceeded max retries downloading image after HTTP 429 rate limit"
                    );
                    return Err(SkipReason::DownloadFailed);
                }

                let wait_time = parse_rate_limit_delay(response.headers(), delay);
                let attempt_num = attempts + 1;
                let wait_secs = if wait_time.as_secs() == 0 && wait_time.subsec_millis() == 0 {
                    0
                } else {
                    wait_time.as_secs().max(1)
                };

                warn!(
                    post_id = %post_id,
                    media_url = %media_url,
                    attempt = attempt_num,
                    max_retries = self.backoff.max_retries,
                    wait_secs = wait_secs,
                    "Received HTTP 429 Too Many Requests while downloading media; backing off"
                );

                if let Some(hook) = on_rate_limit.as_deref_mut() {
                    hook(post_id, wait_time, attempt_num, self.backoff.max_retries);
                }

                tokio::time::sleep(wait_time).await;
                delay = delay.mul_f64(self.backoff.backoff_factor);
                attempts += 1;
                continue;
            }

            if !status.is_success() {
                warn!(
                    post_id = %post_id,
                    media_url = %media_url,
                    status = %status,
                    "Non-success HTTP status encountered while downloading image"
                );
                return Err(SkipReason::DownloadFailed);
            }

            match response.bytes().await {
                Ok(bytes) => return Ok(bytes.to_vec()),
                Err(err) => {
                    warn!(
                        post_id = %post_id,
                        media_url = %media_url,
                        error = %err,
                        "Failed to read image response bytes"
                    );
                    return Err(SkipReason::DownloadFailed);
                }
            }
        }
    }

    /// Normalizes a freshly assembled candidate exactly as the first persistence of a staging
    /// bundle always did: layer dimensions are measured from the downloaded bytes whenever the
    /// media metadata carried no usable width/height, candidate dimensions fall back to the first
    /// layer, and the grid is auto-seeded from the parsed lattice while the candidate still carries
    /// the default grid. Only ever applied to a genuinely new candidate (the deduplication check
    /// ran first), so a later save of the same candidate keeps whatever grid it was given.
    fn normalize_new_candidate(post: &RedditPost, layer_bytes: &[Vec<u8>]) -> Candidate {
        let mut candidate = post.to_bundle_candidate();
        let candidate_dimensions = candidate.dimensions;
        for (index, bytes) in layer_bytes.iter().enumerate() {
            let Some(layer) = candidate.layers.get_mut(index) else {
                break;
            };
            if layer.dimensions == (0, 0) {
                layer.dimensions = image::load_from_memory(bytes)
                    .map(|img| (img.width(), img.height()))
                    .unwrap_or(candidate_dimensions);
            }
        }
        if candidate.dimensions == (0, 0) {
            if let Some(first) = candidate.layers.first() {
                candidate.dimensions = first.dimensions;
            }
        }
        if candidate.grid == GridConfig::default() {
            if let Some(parsed) = candidate.parsed_dimensions {
                if candidate.dimensions != (0, 0) {
                    candidate.grid = GridConfig::auto_seed(parsed, candidate.dimensions);
                }
            }
        }
        candidate
    }

    async fn ingest_post_filtered_inner<Hook>(
        &self,
        post: &RedditPost,
        gridless_only: bool,
        mut on_rate_limit: Option<Hook>,
    ) -> Result<IngestionOutcome, IngestionError>
    where
        Hook: FnMut(&str, Duration, u32, u32) + Send,
    {
        // 0. Filter if post has no media
        if post.media.is_empty() {
            info!(
                post_id = %post.id,
                reason = "no_media",
                "Skipping post: no media"
            );
            return Ok(IngestionOutcome::Skipped {
                id: post.id.clone(),
                reason: SkipReason::NoMedia,
            });
        }

        // 1. Filter if gridless only requested and post is not gridless
        if gridless_only && !post.tags.is_gridless() {
            info!(
                post_id = %post.id,
                reason = "not_gridless",
                "Skipping post: not gridless"
            );
            return Ok(IngestionOutcome::Skipped {
                id: post.id.clone(),
                reason: SkipReason::NotGridless,
            });
        }

        // 2. Check deduplication against storage
        if let Some(reason) = self.check_deduplication(post).await? {
            info!(
                post_id = %post.id,
                reason = ?reason,
                "Skipping post: deduplication check hit"
            );
            return Ok(IngestionOutcome::Skipped {
                id: post.id.clone(),
                reason,
            });
        }

        // 3. Download all media items for the post
        let mut layer_bytes_list: Vec<Vec<u8>> = Vec::with_capacity(post.media.len());
        for media in &post.media {
            match self
                .download_media_bytes(&post.id, &media.url, on_rate_limit.as_mut())
                .await
            {
                Ok(bytes) => layer_bytes_list.push(bytes),
                Err(reason) => {
                    warn!(
                        post_id = %post.id,
                        media_url = %media.url,
                        reason = ?reason,
                        "Failed to download post media item; skipping bundle"
                    );
                    return Ok(IngestionOutcome::Skipped {
                        id: post.id.clone(),
                        reason,
                    });
                }
            }
        }

        // 4. Assemble the candidate bundle, normalized the way the first persistence of a staging
        //    bundle always was
        let candidate = Self::normalize_new_candidate(post, &layer_bytes_list);

        // 5. Write one blob per layer in index order, then the manifest last: a candidate must
        //    never become discoverable in the catalog before the images it describes, or a failed
        //    blob write would leave it visible and permanently skipped as already in staging
        let mut written_keys: Vec<BlobKey> = Vec::with_capacity(layer_bytes_list.len());
        for (index, bytes) in layer_bytes_list.iter().enumerate() {
            let key = BlobKey::StagingLayer {
                id: candidate.id.clone(),
                index,
            };
            if let Err(err) = self.blobs.write_blob(&key, bytes).await {
                for written in &written_keys {
                    let _ = self.blobs.delete_blob(written).await;
                }
                return Err(err.into());
            }
            written_keys.push(key);
        }
        self.catalog.save_candidate(&candidate).await?;

        info!(
            post_id = %post.id,
            title = %post.title,
            layer_count = layer_bytes_list.len(),
            "Downloaded and staged candidate bundle"
        );

        Ok(IngestionOutcome::Downloaded(post.id.clone()))
    }
}
