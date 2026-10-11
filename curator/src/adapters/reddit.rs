use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::domain::{
    HarvestedMedia, ParsedTags, RedditHarvestQuery, RedditPost,
};
use crate::ports::harvester::{HarvesterError, RedditHarvester};

pub const DEFAULT_USER_AGENT: &str =
    "DungeonForge-Curator/1.0 (by /u/dungeon_forge_dev)";
pub const DEFAULT_REDDIT_BASE_URL: &str = "https://www.reddit.com";
pub const DEFAULT_REDDIT_OAUTH_BASE_URL: &str = "https://oauth.reddit.com";
pub const DEFAULT_REDDIT_TOKEN_URL: &str = "https://www.reddit.com/api/v1/access_token";
pub const DEFAULT_REDDIT_PUBLIC_CLIENT_ID: &str = "TWTsqXa53CexlrYGBWaesQ";
pub const DEFAULT_REDDIT_REFRESH_TOKEN_CLIENT_ID: &str = DEFAULT_REDDIT_PUBLIC_CLIENT_ID;
/// OAuth2 client credentials for authenticating against Reddit's API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedditCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl RedditCredentials {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }
}

/// OAuth2 authentication strategy for Reddit's API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedditAuthStrategy {
    /// OAuth2 client credentials flow using client_id and client_secret.
    ClientCredentials(RedditCredentials),
    /// OAuth2 refresh token flow using user grant refresh token and client_id (no secret).
    RefreshToken {
        client_id: String,
        refresh_token: String,
    },
}

/// Cached OAuth2 bearer access token with expiration tracking.
#[derive(Debug, Clone)]
struct CachedToken {
    pub access_token: String,
    pub expires_at: Instant,
}

impl CachedToken {
    fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RedditTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    pub expires_in: u64,
    #[serde(default)]
    pub scope: Option<String>,
}
/// Configuration for exponential backoff on HTTP 429 rate limit responses.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffConfig {
    pub initial_delay: Duration,
    pub max_retries: u32,
    pub backoff_factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(3),
            max_retries: 3,
            backoff_factor: 2.0,
        }
    }
}

impl BackoffConfig {
    pub fn new(initial_delay: Duration, max_retries: u32, backoff_factor: f64) -> Self {
        Self {
            initial_delay,
            max_retries,
            backoff_factor,
        }
    }

    /// Calculate the backoff delay duration for a specific attempt number (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let factor = self.backoff_factor.powi(attempt as i32);
        self.initial_delay.mul_f64(factor)
    }
}

/// Crawler and harvester adapter for public and authenticated Reddit JSON endpoints.
#[derive(Debug, Clone)]
pub struct RedditCrawler {
    client: reqwest::Client,
    base_url: String,
    oauth_base_url: String,
    token_url: String,
    user_agent: String,
    auth_strategy: Option<RedditAuthStrategy>,
    token_cache: Arc<RwLock<Option<CachedToken>>>,
    backoff: BackoffConfig,
}

impl Default for RedditCrawler {
    fn default() -> Self {
        Self::new()
    }
}

impl RedditCrawler {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .build()
                .expect("valid reqwest client"),
            base_url: DEFAULT_REDDIT_BASE_URL.to_string(),
            oauth_base_url: DEFAULT_REDDIT_OAUTH_BASE_URL.to_string(),
            token_url: DEFAULT_REDDIT_TOKEN_URL.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            auth_strategy: None,
            token_cache: Arc::new(RwLock::new(None)),
            backoff: BackoffConfig::default(),
        }
    }

    /// Create a `RedditCrawler` configured from environment variables:
    /// - `REDDIT_REFRESH_TOKEN`: OAuth2 refresh token (takes precedence over client credentials;
    ///   uses `REDDIT_CLIENT_ID` if set, otherwise defaults to `DEFAULT_REDDIT_PUBLIC_CLIENT_ID`)
    /// - `REDDIT_CLIENT_ID` & `REDDIT_CLIENT_SECRET`: OAuth2 client credentials
    /// - `REDDIT_USER_AGENT`: custom User-Agent header
    /// - `REDDIT_BASE_URL`: override unauthenticated base URL
    /// - `REDDIT_OAUTH_BASE_URL`: override OAuth base URL
    /// - `REDDIT_TOKEN_URL`: override OAuth token endpoint URL
    pub fn from_env() -> Self {
        let mut crawler = Self::new();

        let refresh_token = std::env::var("REDDIT_REFRESH_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let client_id = std::env::var("REDDIT_CLIENT_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let client_secret = std::env::var("REDDIT_CLIENT_SECRET")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(token) = refresh_token {
            let cid = client_id.unwrap_or_else(|| DEFAULT_REDDIT_PUBLIC_CLIENT_ID.to_string());
            crawler = crawler.with_refresh_token(cid, token);
        } else if let (Some(cid), Some(csec)) = (client_id, client_secret) {
            crawler = crawler.with_credentials(RedditCredentials::new(cid, csec));
        }

        if let Ok(user_agent) = std::env::var("REDDIT_USER_AGENT") {
            let user_agent = user_agent.trim();
            if !user_agent.is_empty() {
                crawler = crawler.with_user_agent(user_agent);
            }
        }

        if let Ok(base_url) = std::env::var("REDDIT_BASE_URL") {
            let base_url = base_url.trim();
            if !base_url.is_empty() {
                crawler = crawler.with_base_url(base_url);
            }
        }

        if let Ok(oauth_base_url) = std::env::var("REDDIT_OAUTH_BASE_URL") {
            let oauth_base_url = oauth_base_url.trim();
            if !oauth_base_url.is_empty() {
                crawler = crawler.with_oauth_base_url(oauth_base_url);
            }
        }

        if let Ok(token_url) = std::env::var("REDDIT_TOKEN_URL") {
            let token_url = token_url.trim();
            if !token_url.is_empty() {
                crawler = crawler.with_token_url(token_url);
            }
        }

        crawler
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_oauth_base_url(mut self, oauth_base_url: impl Into<String>) -> Self {
        self.oauth_base_url = oauth_base_url.into();
        self
    }

    pub fn with_token_url(mut self, token_url: impl Into<String>) -> Self {
        self.token_url = token_url.into();
        self
    }

    pub fn with_credentials(mut self, credentials: RedditCredentials) -> Self {
        self.auth_strategy = Some(RedditAuthStrategy::ClientCredentials(credentials));
        self
    }

    pub fn with_client_credentials(
        mut self,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        self.auth_strategy = Some(RedditAuthStrategy::ClientCredentials(RedditCredentials::new(
            client_id,
            client_secret,
        )));
        self
    }

    pub fn with_auth_strategy(mut self, auth_strategy: RedditAuthStrategy) -> Self {
        self.auth_strategy = Some(auth_strategy);
        self
    }

    pub fn with_refresh_token(
        mut self,
        client_id: impl Into<String>,
        refresh_token: impl Into<String>,
    ) -> Self {
        self.auth_strategy = Some(RedditAuthStrategy::RefreshToken {
            client_id: client_id.into(),
            refresh_token: refresh_token.into(),
        });
        self
    }

    pub fn with_refresh_token_auth(self, refresh_token: impl Into<String>) -> Self {
        self.with_refresh_token(DEFAULT_REDDIT_PUBLIC_CLIENT_ID, refresh_token)
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn with_backoff(mut self, backoff: BackoffConfig) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn oauth_base_url(&self) -> &str {
        &self.oauth_base_url
    }

    pub fn token_url(&self) -> &str {
        &self.token_url
    }

    pub fn credentials(&self) -> Option<&RedditCredentials> {
        match &self.auth_strategy {
            Some(RedditAuthStrategy::ClientCredentials(creds)) => Some(creds),
            _ => None,
        }
    }

    pub fn auth_strategy(&self) -> Option<&RedditAuthStrategy> {
        self.auth_strategy.as_ref()
    }

    pub fn refresh_token(&self) -> Option<&str> {
        match &self.auth_strategy {
            Some(RedditAuthStrategy::RefreshToken { refresh_token, .. }) => Some(refresh_token),
            _ => None,
        }
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub fn backoff(&self) -> &BackoffConfig {
        &self.backoff
    }

    /// Returns the effective base URL: `oauth_base_url` when credentials are configured,
    /// otherwise `base_url`.
    pub fn effective_base_url(&self) -> &str {
        if self.auth_strategy.is_some() {
            &self.oauth_base_url
        } else {
            &self.base_url
        }
    }

    /// Obtains an OAuth2 bearer access token using client credentials, reusing the cached token if valid.
    pub async fn get_or_refresh_token(&self) -> Result<String, HarvesterError> {
        let auth = self.auth_strategy.as_ref().ok_or_else(|| {
            HarvesterError::InvalidResponse(
                "Cannot obtain Reddit OAuth token: client credentials or refresh token authentication is not configured".to_string(),
            )
        })?;

        // Fast path: check read lock
        {
            let cache = self.token_cache.read().await;
            if let Some(cached) = &*cache {
                if cached.is_valid() {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        // Slow path: acquire write lock
        let mut cache = self.token_cache.write().await;
        if let Some(cached) = &*cache {
            if cached.is_valid() {
                return Ok(cached.access_token.clone());
            }
        }

        let new_token = self.fetch_access_token(auth).await?;
        let token_str = new_token.access_token.clone();
        *cache = Some(new_token);
        Ok(token_str)
    }

    /// Check whether a valid OAuth token is currently cached in memory.
    pub async fn has_cached_token(&self) -> bool {
        let cache = self.token_cache.read().await;
        cache.as_ref().map(|t| t.is_valid()).unwrap_or(false)
    }

    async fn fetch_access_token(
        &self,
        auth: &RedditAuthStrategy,
    ) -> Result<CachedToken, HarvesterError> {
        info!(url = %self.token_url, "Requesting Reddit OAuth2 access token");

        let req = match auth {
            RedditAuthStrategy::ClientCredentials(creds) => self
                .client
                .post(&self.token_url)
                .basic_auth(&creds.client_id, Some(&creds.client_secret))
                .header(reqwest::header::USER_AGENT, &self.user_agent)
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&[("grant_type", "client_credentials")]),
            RedditAuthStrategy::RefreshToken {
                client_id,
                refresh_token,
            } => self
                .client
                .post(&self.token_url)
                .basic_auth(client_id, Some(""))
                .header(reqwest::header::USER_AGENT, &self.user_agent)
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                ]),
        };

        let res = req.send().await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(HarvesterError::InvalidResponse(format!(
                "Failed to obtain Reddit OAuth access token (HTTP {status}) for URL {}: {body}",
                self.token_url
            )));
        }

        let token_resp: RedditTokenResponse = res.json().await?;
        info!(
            "Reddit OAuth2 access token acquired successfully (expires in {}s)",
            token_resp.expires_in
        );

        const TOKEN_SAFETY_BUFFER_SECS: u64 = 60;
        let ttl = if token_resp.expires_in > TOKEN_SAFETY_BUFFER_SECS {
            Duration::from_secs(token_resp.expires_in - TOKEN_SAFETY_BUFFER_SECS)
        } else {
            Duration::from_secs(token_resp.expires_in)
        };
        let expires_at = Instant::now() + ttl;

        Ok(CachedToken {
            access_token: token_resp.access_token,
            expires_at,
        })
    }

    /// Construct the full target URL for querying Reddit listings.
    pub fn build_url(&self, query: &RedditHarvestQuery) -> String {
        let sub = query
            .subreddit
            .trim_start_matches("r/")
            .trim_start_matches('/');
        let sort = query.sort.as_str();
        let limit = query.limit.clamp(1, 100);
        let base = self.effective_base_url();

        let mut url = if let Some(time) = query.time {
            format!(
                "{}/r/{}/{}.json?t={}&limit={}",
                base.trim_end_matches('/'),
                sub,
                sort,
                time.as_str(),
                limit
            )
        } else {
            format!(
                "{}/r/{}/{}.json?limit={}",
                base.trim_end_matches('/'),
                sub,
                sort,
                limit
            )
        };

        if let Some(after) = &query.after {
            url.push_str(&format!("&after={after}"));
        }

        url
    }

    /// Parse Reddit listing response JSON string into domain `RedditPost` items.
    pub fn parse_listing_json(json_str: &str) -> Result<Vec<RedditPost>, HarvesterError> {
        let payload: RedditListingPayload = serde_json::from_str(json_str)?;
        let data = match payload.data {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        let mut posts = Vec::with_capacity(data.children.len());
        for child in data.children {
            let raw = child.data;
            let tags = ParsedTags::parse_title(&raw.title);
            let media = extract_media_from_raw(&raw);

            let created_utc = raw
                .created_utc
                .and_then(|ts| {
                    let secs = ts as i64;
                    let nsecs = ((ts - secs as f64) * 1_000_000_000.0) as u32;
                    DateTime::from_timestamp(secs, nsecs)
                })
                .unwrap_or_else(Utc::now);

            let permalink = match &raw.permalink {
                Some(p) if p.starts_with('/') => format!("https://www.reddit.com{p}"),
                Some(p) => p.clone(),
                None => format!("https://www.reddit.com/r/battlemaps/comments/{}/", raw.id),
            };

            let name = raw.name.unwrap_or_else(|| format!("t3_{}", raw.id));
            let url = raw.url.unwrap_or_else(|| permalink.clone());
            let author = raw.author.unwrap_or_else(|| "[deleted]".to_string());
            let score = raw.score.unwrap_or(0);

            posts.push(RedditPost {
                id: raw.id,
                name,
                title: raw.title,
                author,
                permalink,
                url,
                score,
                created_utc,
                media,
                tags,
            });
        }

        Ok(posts)
    }
}

/// Unescape XML/HTML character entities often embedded by Reddit in image URLs (e.g. `&amp;` -> `&`).
pub fn unescape_xml_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Check if a URL points directly to an uncompressed image based on file extension or trusted image host.
pub fn is_direct_image_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    if path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".webp")
    {
        return true;
    }
    if lower.contains("i.redd.it/") || lower.contains("i.imgur.com/") {
        return true;
    }
    false
}

fn extract_media_from_raw(post: &RedditPostRawData) -> Vec<HarvestedMedia> {
    let mut media = Vec::new();

    // 1. Galleries via media_metadata
    if let Some(media_metadata) = &post.media_metadata {
        if !media_metadata.is_empty() {
            let mut seen_ids = std::collections::HashSet::new();

            // First follow gallery_data ordering and captions if available
            if let Some(gallery_data) = &post.gallery_data {
                for item in &gallery_data.items {
                    if let Some(meta) = media_metadata.get(&item.media_id) {
                        seen_ids.insert(&item.media_id);
                        if let Some(item_media) =
                            extract_media_from_item(meta, item.caption.as_deref())
                        {
                            media.push(item_media);
                        }
                    }
                }
            }

            // Also check any items in media_metadata not listed in gallery_data
            let mut remaining_keys: Vec<&String> = media_metadata
                .keys()
                .filter(|k| !seen_ids.contains(k))
                .collect();
            remaining_keys.sort(); // deterministic ordering
            for key in remaining_keys {
                if let Some(meta) = media_metadata.get(key) {
                    if let Some(item_media) = extract_media_from_item(meta, None) {
                        media.push(item_media);
                    }
                }
            }
        }
    }

    // 2. Direct single image post if media is still empty
    if media.is_empty() {
        if let Some(url_str) = &post.url {
            if is_direct_image_url(url_str) {
                let unescaped_url = unescape_xml_entities(url_str);
                let (width, height) = extract_dimensions_from_preview(post);
                media.push(HarvestedMedia {
                    url: unescaped_url,
                    width,
                    height,
                    caption: None,
                });
            }
        }
    }

    // 3. Fallback to preview image source if media is still empty
    if media.is_empty() {
        if let Some(preview) = &post.preview {
            if let Some(images) = &preview.images {
                if let Some(first_img) = images.first() {
                    if let Some(source) = &first_img.source {
                        if let Some(url_str) = &source.url {
                            let unescaped_url = unescape_xml_entities(url_str);
                            media.push(HarvestedMedia {
                                url: unescaped_url,
                                width: source.width,
                                height: source.height,
                                caption: None,
                            });
                        }
                    }
                }
            }
        }
    }

    media
}

fn extract_media_from_item(
    meta: &RedditRawMediaItem,
    caption: Option<&str>,
) -> Option<HarvestedMedia> {
    // Check uncompressed source 's' first
    if let Some(s) = &meta.s {
        if let Some(url_str) = s.u.as_ref().or(s.gif.as_ref()) {
            return Some(HarvestedMedia {
                url: unescape_xml_entities(url_str),
                width: s.x,
                height: s.y,
                caption: caption.map(|c| c.to_string()),
            });
        }
    }

    // Fallback to highest-resolution preview in 'p' if 's' is missing url
    if let Some(previews) = &meta.p {
        if let Some(best) = previews
            .iter()
            .max_by_key(|p| p.x.unwrap_or(0) * p.y.unwrap_or(0))
        {
            if let Some(url_str) = best.u.as_ref().or(best.gif.as_ref()) {
                return Some(HarvestedMedia {
                    url: unescape_xml_entities(url_str),
                    width: best.x,
                    height: best.y,
                    caption: caption.map(|c| c.to_string()),
                });
            }
        }
    }

    None
}

fn extract_dimensions_from_preview(post: &RedditPostRawData) -> (Option<u32>, Option<u32>) {
    post.preview
        .as_ref()
        .and_then(|p| p.images.as_ref())
        .and_then(|imgs| imgs.first())
        .and_then(|img| img.source.as_ref())
        .map(|s| (s.width, s.height))
        .unwrap_or((None, None))
}

#[async_trait]
impl RedditHarvester for RedditCrawler {
    async fn fetch_posts(
        &self,
        query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        let token = if self.auth_strategy.is_some() {
            Some(self.get_or_refresh_token().await?)
        } else {
            None
        };

        let url = self.build_url(query);
        let mut delay = self.backoff.initial_delay;
        let mut attempts = 0;

        loop {
            info!(
                url = %url,
                authenticated = self.auth_strategy.is_some(),
                attempt = attempts,
                "Dispatching Reddit API request"
            );

            let mut req = self
                .client
                .get(&url)
                .header(reqwest::header::USER_AGENT, &self.user_agent)
                .header(reqwest::header::ACCEPT, "application/json");

            if let Some(tok) = &token {
                req = req.bearer_auth(tok);
            }

            let res = req.send().await?;
            let status = res.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if attempts >= self.backoff.max_retries {
                    warn!(
                        retries = attempts,
                        "Exceeded max retries after HTTP 429 rate limit"
                    );
                    return Err(HarvesterError::RateLimited { retries: attempts });
                }

                // Check Retry-After header if supplied by server
                let wait_time =
                    if let Some(retry_after) = res.headers().get(reqwest::header::RETRY_AFTER) {
                        if let Ok(secs) = retry_after.to_str().unwrap_or("").parse::<u64>() {
                            Duration::from_secs(secs).max(delay)
                        } else {
                            delay
                        }
                    } else {
                        delay
                    };

                warn!(
                    attempt = attempts,
                    wait_secs = ?wait_time,
                    "Received HTTP 429 Too Many Requests; backing off"
                );

                tokio::time::sleep(wait_time).await;
                delay = delay.mul_f64(self.backoff.backoff_factor);
                attempts += 1;
                continue;
            }

            if status == reqwest::StatusCode::FORBIDDEN {
                let msg = match &self.auth_strategy {
                    Some(RedditAuthStrategy::RefreshToken { .. }) => {
                        format!(
                            "Reddit HTTP error 403 Forbidden for URL {url}: Access denied. \
                            Please verify your REDDIT_REFRESH_TOKEN, REDDIT_CLIENT_ID, and REDDIT_USER_AGENT configuration."
                        )
                    }
                    Some(RedditAuthStrategy::ClientCredentials(_)) => {
                        format!(
                            "Reddit HTTP error 403 Forbidden for URL {url}: Access denied. \
                            Please verify your REDDIT_CLIENT_ID, REDDIT_CLIENT_SECRET, and REDDIT_USER_AGENT configuration."
                        )
                    }
                    None => {
                        format!(
                            "Reddit HTTP error 403 Forbidden for URL {url}: Reddit blocks unauthenticated requests to public JSON endpoints. \
                            Please configure REDDIT_CLIENT_ID and REDDIT_CLIENT_SECRET in .env to authenticate via OAuth2 client credentials."
                        )
                    }
                };
                return Err(HarvesterError::InvalidResponse(msg));
            }

            if status == reqwest::StatusCode::UNAUTHORIZED {
                let msg = match &self.auth_strategy {
                    Some(RedditAuthStrategy::RefreshToken { .. }) => {
                        format!(
                            "Reddit HTTP error 401 Unauthorized for URL {url}: Invalid or expired OAuth2 credentials. \
                            Please check REDDIT_REFRESH_TOKEN in .env."
                        )
                    }
                    Some(RedditAuthStrategy::ClientCredentials(_)) => {
                        format!(
                            "Reddit HTTP error 401 Unauthorized for URL {url}: Invalid or expired OAuth2 credentials. \
                            Please check REDDIT_CLIENT_ID and REDDIT_CLIENT_SECRET in .env."
                        )
                    }
                    None => {
                        format!(
                            "Reddit HTTP error 401 Unauthorized for URL {url}: Invalid or expired OAuth2 credentials."
                        )
                    }
                };
                return Err(HarvesterError::InvalidResponse(msg));
            }

            if !status.is_success() {
                return Err(HarvesterError::InvalidResponse(format!(
                    "Reddit HTTP error {status} for URL {url}"
                )));
            }

            let body = res.text().await?;
            return Self::parse_listing_json(&body);
        }
    }
}

// ---------------------------------------------------------------------------
// Serde helper models for deserializing raw Reddit API responses
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RedditListingPayload {
    #[serde(default)]
    pub data: Option<RedditListingPayloadData>,
}

#[derive(Debug, Deserialize)]
struct RedditListingPayloadData {
    #[serde(default)]
    pub children: Vec<RedditPostEnvelope>,
}

#[derive(Debug, Deserialize)]
struct RedditPostEnvelope {
    pub data: RedditPostRawData,
}

#[derive(Debug, Deserialize)]
struct RedditPostRawData {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub score: Option<i32>,
    #[serde(default)]
    pub created_utc: Option<f64>,
    #[serde(default)]
    pub media_metadata: Option<HashMap<String, RedditRawMediaItem>>,
    #[serde(default)]
    pub gallery_data: Option<RedditRawGalleryData>,
    #[serde(default)]
    pub preview: Option<RedditRawPreview>,
}

#[derive(Debug, Deserialize)]
struct RedditRawGalleryData {
    #[serde(default)]
    pub items: Vec<RedditRawGalleryItem>,
}

#[derive(Debug, Deserialize)]
struct RedditRawGalleryItem {
    pub media_id: String,
    #[serde(default)]
    pub caption: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RedditRawMediaItem {
    #[serde(default)]
    pub s: Option<RedditRawMediaSource>,
    #[serde(default)]
    pub p: Option<Vec<RedditRawMediaSource>>,
}

#[derive(Debug, Deserialize)]
struct RedditRawMediaSource {
    #[serde(default)]
    pub x: Option<u32>,
    #[serde(default)]
    pub y: Option<u32>,
    #[serde(default)]
    pub u: Option<String>,
    #[serde(default)]
    pub gif: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RedditRawPreview {
    #[serde(default)]
    pub images: Option<Vec<RedditRawPreviewImage>>,
}

#[derive(Debug, Deserialize)]
struct RedditRawPreviewImage {
    #[serde(default)]
    pub source: Option<RedditRawPreviewSource>,
}

#[derive(Debug, Deserialize)]
struct RedditRawPreviewSource {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GridStatus, RedditSort, RedditTimeRange};

    #[test]
    fn test_backoff_config_delay_calculation() {
        let config = BackoffConfig::new(Duration::from_secs(3), 3, 2.0);
        assert_eq!(config.delay_for_attempt(0), Duration::from_secs(3));
        assert_eq!(config.delay_for_attempt(1), Duration::from_secs(6));
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(12));
    }

    #[test]
    fn test_build_url() {
        let crawler = RedditCrawler::new();

        let q1 = RedditHarvestQuery::battlemaps()
            .with_sort(RedditSort::Hot)
            .with_time(RedditTimeRange::All)
            .with_limit(25);
        assert_eq!(
            crawler.build_url(&q1),
            "https://www.reddit.com/r/battlemaps/hot.json?t=all&limit=25"
        );

        let q2 = RedditHarvestQuery::new()
            .with_subreddit("r/dndmaps")
            .with_sort(RedditSort::Top)
            .with_time(RedditTimeRange::Month)
            .with_limit(50)
            .with_after("t3_12345");
        assert_eq!(
            crawler.build_url(&q2),
            "https://www.reddit.com/r/dndmaps/top.json?t=month&limit=50&after=t3_12345"
        );
    }

    #[test]
    fn test_build_url_authenticated() {
        let crawler = RedditCrawler::new()
            .with_credentials(RedditCredentials::new("id", "secret"));

        let q1 = RedditHarvestQuery::battlemaps()
            .with_sort(RedditSort::Hot)
            .with_time(RedditTimeRange::All)
            .with_limit(25);
        assert_eq!(
            crawler.build_url(&q1),
            "https://oauth.reddit.com/r/battlemaps/hot.json?t=all&limit=25"
        );
    }

    #[test]
    fn test_build_url_authenticated_refresh_token() {
        let crawler = RedditCrawler::new().with_refresh_token_auth("mock_refresh_token");

        let q1 = RedditHarvestQuery::battlemaps()
            .with_sort(RedditSort::Hot)
            .with_time(RedditTimeRange::All)
            .with_limit(25);
        assert_eq!(
            crawler.build_url(&q1),
            "https://oauth.reddit.com/r/battlemaps/hot.json?t=all&limit=25"
        );
        assert_eq!(
            crawler.auth_strategy(),
            Some(&RedditAuthStrategy::RefreshToken {
                client_id: DEFAULT_REDDIT_PUBLIC_CLIENT_ID.to_string(),
                refresh_token: "mock_refresh_token".to_string(),
            })
        );
        assert_eq!(crawler.refresh_token(), Some("mock_refresh_token"));
        assert_eq!(crawler.credentials(), None);
        assert_eq!(crawler.effective_base_url(), DEFAULT_REDDIT_OAUTH_BASE_URL);
    }

    #[test]
    fn test_crawler_refresh_token_custom_client_id() {
        let crawler = RedditCrawler::new()
            .with_refresh_token("custom_client_id", "mock_refresh_token");

        assert_eq!(
            crawler.auth_strategy(),
            Some(&RedditAuthStrategy::RefreshToken {
                client_id: "custom_client_id".to_string(),
                refresh_token: "mock_refresh_token".to_string(),
            })
        );
        assert_eq!(crawler.refresh_token(), Some("mock_refresh_token"));
        assert_eq!(crawler.credentials(), None);
        assert_eq!(crawler.effective_base_url(), DEFAULT_REDDIT_OAUTH_BASE_URL);
    }

    #[test]
    fn test_unescape_xml_entities() {
        let input = "https://preview.redd.it/item.png?width=2000&amp;format=png&amp;auto=webp&amp;s=abc123";
        let unescaped = unescape_xml_entities(input);
        assert_eq!(
            unescaped,
            "https://preview.redd.it/item.png?width=2000&format=png&auto=webp&s=abc123"
        );
    }

    #[test]
    fn test_is_direct_image_url() {
        assert!(is_direct_image_url("https://i.redd.it/abcdef.png"));
        assert!(is_direct_image_url("https://i.redd.it/abcdef.jpg"));
        assert!(is_direct_image_url("https://i.redd.it/abcdef.jpeg"));
        assert!(is_direct_image_url("https://i.redd.it/abcdef.webp"));
        assert!(is_direct_image_url("https://i.imgur.com/abcdef.png"));
        assert!(is_direct_image_url("https://i.redd.it/unnamed_image"));
        assert!(is_direct_image_url("https://site.com/map.png?width=2000"));
        assert!(!is_direct_image_url("https://reddit.com/r/battlemaps/comments/123/"));
        assert!(!is_direct_image_url("https://youtube.com/watch?v=123"));
    }

    #[test]
    fn test_parse_listing_json_single_image_post() {
        let json_payload = r#"{
            "kind": "Listing",
            "data": {
                "after": "t3_post2",
                "children": [
                    {
                        "kind": "t3",
                        "data": {
                            "id": "post1",
                            "name": "t3_post1",
                            "title": "[30x40] Ancient Forest Shrine [Gridless]",
                            "author": "battlemapper",
                            "permalink": "/r/battlemaps/comments/post1/ancient_forest_shrine/",
                            "url": "https://i.redd.it/forest_shrine.png",
                            "score": 320,
                            "created_utc": 1690000000.0,
                            "preview": {
                                "images": [
                                    {
                                        "source": {
                                            "url": "https://preview.redd.it/forest_shrine.png?auto=webp&amp;s=xyz",
                                            "width": 3000,
                                            "height": 4000
                                        }
                                    }
                                ]
                            }
                        }
                    }
                ]
            }
        }"#;

        let posts = RedditCrawler::parse_listing_json(json_payload).expect("valid parse");
        assert_eq!(posts.len(), 1);

        let p = &posts[0];
        assert_eq!(p.id, "post1");
        assert_eq!(p.name, "t3_post1");
        assert_eq!(p.author, "battlemapper");
        assert_eq!(p.score, 320);
        assert_eq!(p.permalink, "https://www.reddit.com/r/battlemaps/comments/post1/ancient_forest_shrine/");
        assert_eq!(p.tags.grid_status, GridStatus::Gridless);
        assert_eq!(p.tags.dimensions, Some((30, 40)));

        assert_eq!(p.media.len(), 1);
        let m = &p.media[0];
        assert_eq!(m.url, "https://i.redd.it/forest_shrine.png");
        assert_eq!(m.width, Some(3000));
        assert_eq!(m.height, Some(4000));
    }

    #[test]
    fn test_parse_listing_json_gallery_post() {
        let json_payload = r#"{
            "kind": "Listing",
            "data": {
                "children": [
                    {
                        "kind": "t3",
                        "data": {
                            "id": "gallery1",
                            "name": "t3_gallery1",
                            "title": "Sunken Dungeon [40x60]",
                            "author": "dungeon_artist",
                            "permalink": "/r/battlemaps/comments/gallery1/sunken_dungeon/",
                            "url": "https://reddit.com/gallery/gallery1",
                            "score": 500,
                            "created_utc": 1691000000.0,
                            "gallery_data": {
                                "items": [
                                    { "media_id": "img_day", "caption": "Day Variant" },
                                    { "media_id": "img_night", "caption": "Night Variant" }
                                ]
                            },
                            "media_metadata": {
                                "img_night": {
                                    "status": "valid",
                                    "e": "Image",
                                    "m": "image/png",
                                    "s": {
                                        "x": 4000,
                                        "y": 6000,
                                        "u": "https://preview.redd.it/img_night.png?width=4000&amp;format=png&amp;auto=webp&amp;s=night_sig"
                                    }
                                },
                                "img_day": {
                                    "status": "valid",
                                    "e": "Image",
                                    "m": "image/png",
                                    "s": {
                                        "x": 4000,
                                        "y": 6000,
                                        "u": "https://preview.redd.it/img_day.png?width=4000&amp;format=png&amp;auto=webp&amp;s=day_sig"
                                    }
                                }
                            }
                        }
                    }
                ]
            }
        }"#;

        let posts = RedditCrawler::parse_listing_json(json_payload).expect("valid parse");
        assert_eq!(posts.len(), 1);

        let p = &posts[0];
        assert_eq!(p.id, "gallery1");
        assert_eq!(p.tags.grid_status, GridStatus::Gridded);
        assert_eq!(p.tags.dimensions, Some((40, 60)));

        assert_eq!(p.media.len(), 2);
        // Ordered according to gallery_data: day first, then night
        let day = &p.media[0];
        assert_eq!(
            day.url,
            "https://preview.redd.it/img_day.png?width=4000&format=png&auto=webp&s=day_sig"
        );
        assert_eq!(day.width, Some(4000));
        assert_eq!(day.height, Some(6000));
        assert_eq!(day.caption, Some("Day Variant".to_string()));

        let night = &p.media[1];
        assert_eq!(
            night.url,
            "https://preview.redd.it/img_night.png?width=4000&format=png&auto=webp&s=night_sig"
        );
        assert_eq!(night.width, Some(4000));
        assert_eq!(night.height, Some(6000));
        assert_eq!(night.caption, Some("Night Variant".to_string()));
    }
}
