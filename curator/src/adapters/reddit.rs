use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

use crate::domain::{
    HarvestedMedia, ParsedTags, RedditHarvestQuery, RedditPost,
};
use crate::ports::harvester::{HarvesterError, RedditHarvester};

pub const DEFAULT_USER_AGENT: &str =
    "DungeonForge-Curator/1.0 (by /u/dungeon_forge_dev)";
pub const DEFAULT_REDDIT_BASE_URL: &str = "https://www.reddit.com";

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

/// Crawler and harvester adapter for public Reddit JSON endpoints.
#[derive(Debug, Clone)]
pub struct RedditCrawler {
    client: reqwest::Client,
    base_url: String,
    user_agent: String,
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
            user_agent: DEFAULT_USER_AGENT.to_string(),
            backoff: BackoffConfig::default(),
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
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

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub fn backoff(&self) -> &BackoffConfig {
        &self.backoff
    }

    /// Construct the full target URL for querying Reddit listings.
    pub fn build_url(&self, query: &RedditHarvestQuery) -> String {
        let sub = query
            .subreddit
            .trim_start_matches("r/")
            .trim_start_matches('/');
        let sort = query.sort.as_str();
        let limit = query.limit.clamp(1, 100);

        let mut url = if let Some(time) = query.time {
            format!(
                "{}/r/{}/{}.json?t={}&limit={}",
                self.base_url.trim_end_matches('/'),
                sub,
                sort,
                time.as_str(),
                limit
            )
        } else {
            format!(
                "{}/r/{}/{}.json?limit={}",
                self.base_url.trim_end_matches('/'),
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
        let url = self.build_url(query);
        let mut delay = self.backoff.initial_delay;
        let mut attempts = 0;

        loop {
            debug!(attempt = attempts, url = %url, "Sending Reddit HTTP request");

            let res = self
                .client
                .get(&url)
                .header(reqwest::header::USER_AGENT, &self.user_agent)
                .send()
                .await?;

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
