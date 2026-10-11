use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use curator::adapters::reddit::{BackoffConfig, RedditCrawler, DEFAULT_USER_AGENT};
use curator::domain::{GridStatus, RedditHarvestQuery, RedditSort, RedditTimeRange};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

const MOCK_LISTING_JSON: &str = r#"{
    "kind": "Listing",
    "data": {
        "after": "t3_post999",
        "dist": 2,
        "children": [
            {
                "kind": "t3",
                "data": {
                    "id": "single1",
                    "name": "t3_single1",
                    "title": "[25x30] Ruined Abbey [Gridless]",
                    "author": "abbey_mapper",
                    "permalink": "/r/battlemaps/comments/single1/ruined_abbey/",
                    "url": "https://i.redd.it/ruined_abbey.png",
                    "score": 410,
                    "created_utc": 1690500000.0,
                    "preview": {
                        "images": [
                            {
                                "source": {
                                    "url": "https://preview.redd.it/ruined_abbey.png?auto=webp&amp;s=abc",
                                    "width": 2500,
                                    "height": 3000
                                }
                            }
                        ]
                    }
                }
            },
            {
                "kind": "t3",
                "data": {
                    "id": "text_only",
                    "name": "t3_text_only",
                    "title": "Need suggestions for underwater maps",
                    "author": "curious_dm",
                    "permalink": "/r/battlemaps/comments/text_only/suggestions/",
                    "url": "https://reddit.com/r/battlemaps/comments/text_only/suggestions/",
                    "score": 15,
                    "created_utc": 1690550000.0
                }
            }
        ]
    }
}"#;

#[tokio::test]
async fn test_fetch_posts_end_to_end_with_user_agent_and_query_params() {
    let app = Router::new().route(
        "/r/battlemaps/top.json",
        get(|headers: HeaderMap, Query(params): Query<HashMap<String, String>>| async move {
            let ua = headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert_eq!(ua, DEFAULT_USER_AGENT);

            assert_eq!(params.get("t").map(|s| s.as_str()), Some("all"));
            assert_eq!(params.get("limit").map(|s| s.as_str()), Some("25"));

            (StatusCode::OK, MOCK_LISTING_JSON)
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{}", addr);
    let crawler = RedditCrawler::new().with_base_url(base_url);

    let query = RedditHarvestQuery::battlemaps()
        .with_sort(RedditSort::Top)
        .with_time(RedditTimeRange::All)
        .with_limit(25);

    let posts = crawler.fetch_posts(&query).await.expect("successful fetch");
    assert_eq!(posts.len(), 2);

    let p0 = &posts[0];
    assert_eq!(p0.id, "single1");
    assert_eq!(p0.tags.grid_status, GridStatus::Gridless);
    assert_eq!(p0.tags.dimensions, Some((25, 30)));
    assert_eq!(p0.media.len(), 1);
    assert_eq!(p0.media[0].url, "https://i.redd.it/ruined_abbey.png");
    assert_eq!(p0.media[0].width, Some(2500));
    assert_eq!(p0.media[0].height, Some(3000));

    let p1 = &posts[1];
    assert_eq!(p1.id, "text_only");
    assert_eq!(p1.tags.grid_status, GridStatus::Unknown);
    assert_eq!(p1.media.len(), 0);

    // Test harvest_candidates filters out posts without media
    let candidates = crawler
        .harvest_candidates(&query)
        .await
        .expect("successful harvest");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "single1");
}

#[tokio::test]
async fn test_rate_limit_429_backoff_and_recovery() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let app = Router::new().route(
        "/r/battlemaps/hot.json",
        get(move || {
            let attempts = attempts_clone.clone();
            async move {
                let count = attempts.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("Retry-After", "0")],
                        "Rate limited",
                    )
                        .into_response()
                } else {
                    (StatusCode::OK, MOCK_LISTING_JSON).into_response()
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{}", addr);
    let backoff = BackoffConfig::new(Duration::from_millis(10), 3, 2.0);
    let crawler = RedditCrawler::new()
        .with_base_url(base_url)
        .with_backoff(backoff);

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Hot);

    let posts = crawler.fetch_posts(&query).await.expect("retries succeed");
    assert_eq!(posts.len(), 2);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_rate_limit_429_exhausted_retries() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let app = Router::new().route(
        "/r/battlemaps/hot.json",
        get(move || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                (StatusCode::TOO_MANY_REQUESTS, "Still rate limited").into_response()
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{}", addr);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 2, 2.0);
    let crawler = RedditCrawler::new()
        .with_base_url(base_url)
        .with_backoff(backoff);

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Hot);

    let err = crawler
        .fetch_posts(&query)
        .await
        .expect_err("should fail with rate limit");

    match err {
        HarvesterError::RateLimited { retries } => {
            assert_eq!(retries, 2);
        }
        other => panic!("expected HarvesterError::RateLimited, got: {other:?}"),
    }

    assert_eq!(attempts.load(Ordering::SeqCst), 3); // initial + 2 retries
}

#[tokio::test]
async fn test_http_server_error_returns_invalid_response() {
    let app = Router::new().route(
        "/r/battlemaps/hot.json",
        get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Reddit is down").into_response() }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{}", addr);
    let crawler = RedditCrawler::new().with_base_url(base_url);

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Hot);
    let err = crawler.fetch_posts(&query).await.expect_err("should fail");

    match err {
        HarvesterError::InvalidResponse(msg) => {
            assert!(msg.contains("500"));
        }
        other => panic!("expected InvalidResponse, got: {other:?}"),
    }
}
