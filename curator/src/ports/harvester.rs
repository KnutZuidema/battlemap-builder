use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{RedditHarvestQuery, RedditPost};

/// Errors arising from harvesting battlemap posts from external providers.
#[derive(Debug, Error)]
pub enum HarvesterError {
    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Rate limited (HTTP 429) after {retries} retries")]
    RateLimited { retries: u32 },

    #[error("Invalid response from provider: {0}")]
    InvalidResponse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Port defining operations for harvesting battlemaps from Reddit.
#[async_trait]
pub trait RedditHarvester: Send + Sync {
    /// Fetch posts matching the harvest query from Reddit.
    async fn fetch_posts(
        &self,
        query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError>;

    /// Fetch and filter posts that contain at least one valid harvested media image.
    async fn harvest_candidates(
        &self,
        query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        let posts = self.fetch_posts(query).await?;
        Ok(posts.into_iter().filter(|p| p.has_media()).collect())
    }
}

#[async_trait]
impl<T: ?Sized + RedditHarvester> RedditHarvester for std::sync::Arc<T> {
    async fn fetch_posts(
        &self,
        query: &RedditHarvestQuery,
    ) -> Result<Vec<RedditPost>, HarvesterError> {
        (**self).fetch_posts(query).await
    }
}
