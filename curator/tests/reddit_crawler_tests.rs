use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use base64::prelude::*;
use curator::adapters::reddit::{
    parse_rate_limit_delay, BackoffConfig, RedditAuthStrategy, RedditCrawler,
    RedditCredentials, DEFAULT_REDDIT_BASE_URL, DEFAULT_REDDIT_OAUTH_BASE_URL,
    DEFAULT_REDDIT_PUBLIC_CLIENT_ID, DEFAULT_REDDIT_TOKEN_URL, DEFAULT_USER_AGENT,
    MAX_RATE_LIMIT_DELAY,
};
use curator::domain::{GridStatus, RedditHarvestQuery, RedditSort, RedditTimeRange};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
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

#[tokio::test]
async fn test_oauth_token_retrieval_and_caching() {
    let token_requests = Arc::new(AtomicU32::new(0));
    let token_requests_clone = token_requests.clone();

    let app = Router::new().route(
        "/api/v1/access_token",
        post(move |headers: HeaderMap, body: String| {
            let token_requests = token_requests_clone.clone();
            async move {
                token_requests.fetch_add(1, Ordering::SeqCst);

                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert!(
                    auth.starts_with("Basic "),
                    "expected Basic auth, got {auth}"
                );

                let ua = headers
                    .get(axum::http::header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(ua, DEFAULT_USER_AGENT);

                let accept = headers
                    .get(axum::http::header::ACCEPT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(accept, "application/json");

                assert!(
                    body.contains("grant_type=client_credentials"),
                    "expected grant_type=client_credentials in body, got: {body}"
                );

                (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    r#"{"access_token":"mock_oauth_token_123","token_type":"bearer","expires_in":3600,"scope":"*"}"#,
                )
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let token_url = format!("http://{addr}/api/v1/access_token");
    let crawler = RedditCrawler::new()
        .with_credentials(RedditCredentials::new("test_client_id", "test_client_secret"))
        .with_token_url(token_url);

    assert!(!crawler.has_cached_token().await);

    // First call: retrieves token from mock server
    let token1 = crawler
        .get_or_refresh_token()
        .await
        .expect("token should be obtained");
    assert_eq!(token1, "mock_oauth_token_123");
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
    assert!(crawler.has_cached_token().await);

    // Second call: should use cached token and NOT issue another HTTP request
    let token2 = crawler
        .get_or_refresh_token()
        .await
        .expect("cached token should be returned");
    assert_eq!(token2, "mock_oauth_token_123");
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_authenticated_fetch_posts_with_bearer_token() {
    let app = Router::new()
        .route(
            "/api/v1/access_token",
            post(|| async {
                (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    r#"{"access_token":"bearer_secret_xyz","token_type":"bearer","expires_in":3600}"#,
                )
            }),
        )
        .route(
            "/r/battlemaps/top.json",
            get(|headers: HeaderMap| async move {
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(auth, "Bearer bearer_secret_xyz");

                let accept = headers
                    .get(axum::http::header::ACCEPT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(accept, "application/json");

                (StatusCode::OK, MOCK_LISTING_JSON)
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let crawler = RedditCrawler::new()
        .with_credentials(RedditCredentials::new("client_id", "client_secret"))
        .with_oauth_base_url(format!("http://{addr}"))
        .with_token_url(format!("http://{addr}/api/v1/access_token"));

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Top);
    let posts = crawler
        .fetch_posts(&query)
        .await
        .expect("authenticated fetch succeeds");

    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].id, "single1");
    assert!(crawler.has_cached_token().await);
}

#[tokio::test]
async fn test_refresh_token_retrieval_formatting_and_caching() {
    let token_requests = Arc::new(AtomicU32::new(0));
    let token_requests_clone = token_requests.clone();

    let app = Router::new().route(
        "/api/v1/access_token",
        post(move |headers: HeaderMap, body: String| {
            let token_requests = token_requests_clone.clone();
            async move {
                token_requests.fetch_add(1, Ordering::SeqCst);

                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert!(
                    auth.starts_with("Basic "),
                    "expected Basic auth header, got {auth}"
                );
                let basic_b64 = auth.strip_prefix("Basic ").unwrap();
                let decoded = String::from_utf8(
                    BASE64_STANDARD.decode(basic_b64).expect("valid base64"),
                )
                .expect("valid utf-8 credentials");
                assert_eq!(
                    decoded, "custom_client_123:",
                    "expected Basic auth containing client_id and empty password"
                );

                let content_type = headers
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert!(
                    content_type.contains("application/x-www-form-urlencoded"),
                    "expected application/x-www-form-urlencoded content-type, got: {content_type}"
                );

                let ua = headers
                    .get(axum::http::header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(ua, DEFAULT_USER_AGENT);

                let accept = headers
                    .get(axum::http::header::ACCEPT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(accept, "application/json");

                assert!(
                    body.contains("grant_type=refresh_token"),
                    "expected grant_type=refresh_token in body, got: {body}"
                );
                assert!(
                    body.contains("refresh_token=mock_refresh_token_secret"),
                    "expected refresh_token parameter in body, got: {body}"
                );

                (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    r#"{"access_token":"mock_rt_access_token_777","token_type":"bearer","expires_in":3600,"scope":"read"}"#,
                )
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let token_url = format!("http://{addr}/api/v1/access_token");
    let crawler = RedditCrawler::new()
        .with_refresh_token("custom_client_123", "mock_refresh_token_secret")
        .with_token_url(token_url);

    assert!(!crawler.has_cached_token().await);

    // First call: retrieves token from mock server
    let token1 = crawler
        .get_or_refresh_token()
        .await
        .expect("token should be obtained via refresh token");
    assert_eq!(token1, "mock_rt_access_token_777");
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
    assert!(crawler.has_cached_token().await);

    // Second call: should use cached token and NOT issue another HTTP request
    let token2 = crawler
        .get_or_refresh_token()
        .await
        .expect("cached token should be returned");
    assert_eq!(token2, "mock_rt_access_token_777");
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_authenticated_fetch_posts_with_refresh_token() {
    let token_requests = Arc::new(AtomicU32::new(0));
    let token_requests_clone = token_requests.clone();

    let app = Router::new()
        .route(
            "/api/v1/access_token",
            post(move |headers: HeaderMap, body: String| {
                let token_requests = token_requests_clone.clone();
                async move {
                    token_requests.fetch_add(1, Ordering::SeqCst);

                    let auth = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    assert!(auth.starts_with("Basic "));
                    let basic_b64 = auth.strip_prefix("Basic ").unwrap();
                    let decoded = String::from_utf8(
                        BASE64_STANDARD.decode(basic_b64).expect("valid base64"),
                    )
                    .unwrap();
                    assert_eq!(decoded, format!("{DEFAULT_REDDIT_PUBLIC_CLIENT_ID}:"));

                    assert!(body.contains("grant_type=refresh_token"));
                    assert!(body.contains("refresh_token=mock_user_rt_999"));

                    (
                        StatusCode::OK,
                        [("content-type", "application/json")],
                        r#"{"access_token":"bearer_from_rt_xyz","token_type":"bearer","expires_in":3600}"#,
                    )
                }
            }),
        )
        .route(
            "/r/battlemaps/top.json",
            get(|headers: HeaderMap| async move {
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(auth, "Bearer bearer_from_rt_xyz");

                let accept = headers
                    .get(axum::http::header::ACCEPT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(accept, "application/json");

                (StatusCode::OK, MOCK_LISTING_JSON)
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let crawler = RedditCrawler::new()
        .with_refresh_token_auth("mock_user_rt_999")
        .with_oauth_base_url(format!("http://{addr}"))
        .with_token_url(format!("http://{addr}/api/v1/access_token"));

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Top);
    let posts = crawler
        .fetch_posts(&query)
        .await
        .expect("harvest via refresh token succeeds");

    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].id, "single1");
    assert!(crawler.has_cached_token().await);
    assert_eq!(token_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_403_forbidden_diagnostic_error_unauthenticated() {
    let app = Router::new().route(
        "/r/battlemaps/top.json",
        get(|| async {
            (
                StatusCode::FORBIDDEN,
                "Blocked by Reddit edge security WAF",
            )
                .into_response()
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let crawler = RedditCrawler::new().with_base_url(format!("http://{addr}"));
    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Top);
    let err = crawler
        .fetch_posts(&query)
        .await
        .expect_err("should return 403 error");

    assert!(err.is_forbidden());
    match err {
        HarvesterError::InvalidResponse(msg) => {
            assert!(msg.contains("403 Forbidden"), "msg: {msg}");
            assert!(msg.contains("REDDIT_CLIENT_ID"), "msg: {msg}");
            assert!(msg.contains("REDDIT_CLIENT_SECRET"), "msg: {msg}");
            assert!(msg.contains(".env"), "msg: {msg}");
            assert!(msg.contains("unauthenticated"), "msg: {msg}");
        }
        other => panic!("expected HarvesterError::InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_403_forbidden_diagnostic_error_authenticated() {
    let app = Router::new()
        .route(
            "/api/v1/access_token",
            post(|| async {
                (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    r#"{"access_token":"token_123","token_type":"bearer","expires_in":3600}"#,
                )
            }),
        )
        .route(
            "/r/battlemaps/top.json",
            get(|| async {
                (StatusCode::FORBIDDEN, "Access Denied").into_response()
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let crawler = RedditCrawler::new()
        .with_credentials(RedditCredentials::new("client_id", "client_secret"))
        .with_oauth_base_url(format!("http://{addr}"))
        .with_token_url(format!("http://{addr}/api/v1/access_token"));

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Top);
    let err = crawler
        .fetch_posts(&query)
        .await
        .expect_err("should return 403 error");

    assert!(err.is_forbidden());
    match err {
        HarvesterError::InvalidResponse(msg) => {
            assert!(msg.contains("403 Forbidden"), "msg: {msg}");
            assert!(msg.contains("Access denied"), "msg: {msg}");
            assert!(msg.contains("REDDIT_CLIENT_ID"), "msg: {msg}");
        }
        other => panic!("expected HarvesterError::InvalidResponse, got {other:?}"),
    }
}

static ENV_TEST_MUTEX: Mutex<()> = Mutex::const_new(());

#[tokio::test]
async fn test_crawler_builder_and_environment_variable_configuration() {
    // 1. Builder configuration
    let creds = RedditCredentials::new("builder_client", "builder_secret");
    let crawler = RedditCrawler::new()
        .with_credentials(creds.clone())
        .with_user_agent("CustomBot/2.0")
        .with_base_url("https://custom.reddit.com")
        .with_oauth_base_url("https://custom-oauth.reddit.com")
        .with_token_url("https://custom.reddit.com/token");

    assert_eq!(crawler.credentials(), Some(&creds));
    assert_eq!(crawler.user_agent(), "CustomBot/2.0");
    assert_eq!(crawler.base_url(), "https://custom.reddit.com");
    assert_eq!(crawler.oauth_base_url(), "https://custom-oauth.reddit.com");
    assert_eq!(crawler.token_url(), "https://custom.reddit.com/token");
    assert_eq!(
        crawler.effective_base_url(),
        "https://custom-oauth.reddit.com"
    );

    // 2. Convenience client credentials builder
    let crawler2 = RedditCrawler::new()
        .with_client_credentials("id2", "sec2");
    assert_eq!(
        crawler2.credentials(),
        Some(&RedditCredentials::new("id2", "sec2"))
    );

    // 3. from_env configuration
    let _guard = ENV_TEST_MUTEX.lock().await;
    std::env::set_var("REDDIT_CLIENT_ID", "env_client_id");
    std::env::set_var("REDDIT_CLIENT_SECRET", "env_client_secret");
    std::env::set_var("REDDIT_USER_AGENT", "EnvBot/1.0");
    std::env::set_var("REDDIT_BASE_URL", "https://env.reddit.com");
    std::env::set_var("REDDIT_OAUTH_BASE_URL", "https://env-oauth.reddit.com");
    std::env::set_var("REDDIT_TOKEN_URL", "https://env.reddit.com/api/token");

    let env_crawler = RedditCrawler::from_env();
    assert_eq!(
        env_crawler.credentials(),
        Some(&RedditCredentials::new("env_client_id", "env_client_secret"))
    );
    assert_eq!(env_crawler.user_agent(), "EnvBot/1.0");
    assert_eq!(env_crawler.base_url(), "https://env.reddit.com");
    assert_eq!(env_crawler.oauth_base_url(), "https://env-oauth.reddit.com");
    assert_eq!(env_crawler.token_url(), "https://env.reddit.com/api/token");
    assert_eq!(
        env_crawler.effective_base_url(),
        "https://env-oauth.reddit.com"
    );

    // Clean up env vars
    std::env::remove_var("REDDIT_CLIENT_ID");
    std::env::remove_var("REDDIT_CLIENT_SECRET");
    std::env::remove_var("REDDIT_USER_AGENT");
    std::env::remove_var("REDDIT_BASE_URL");
    std::env::remove_var("REDDIT_OAUTH_BASE_URL");
    std::env::remove_var("REDDIT_TOKEN_URL");

    // Verify clean default state of RedditCrawler::new()
    let default_crawler = RedditCrawler::new();
    assert_eq!(default_crawler.credentials(), None);
    assert_eq!(default_crawler.user_agent(), DEFAULT_USER_AGENT);
    assert_eq!(default_crawler.base_url(), DEFAULT_REDDIT_BASE_URL);
    assert_eq!(
        default_crawler.oauth_base_url(),
        DEFAULT_REDDIT_OAUTH_BASE_URL
    );
    assert_eq!(default_crawler.token_url(), DEFAULT_REDDIT_TOKEN_URL);
    assert_eq!(
        default_crawler.effective_base_url(),
        DEFAULT_REDDIT_BASE_URL
    );
}

#[tokio::test]
async fn test_refresh_token_builder_and_from_env_configuration() {
    // 1. Builder configuration with custom client ID
    let crawler1 = RedditCrawler::new()
        .with_refresh_token("builder_client_id", "builder_refresh_token");

    assert_eq!(
        crawler1.auth_strategy(),
        Some(&RedditAuthStrategy::RefreshToken {
            client_id: "builder_client_id".to_string(),
            refresh_token: "builder_refresh_token".to_string(),
        })
    );
    assert_eq!(crawler1.refresh_token(), Some("builder_refresh_token"));
    assert_eq!(crawler1.credentials(), None);
    assert_eq!(
        crawler1.effective_base_url(),
        DEFAULT_REDDIT_OAUTH_BASE_URL
    );

    // 2. Convenience builder defaulting to public client ID
    let crawler2 = RedditCrawler::new()
        .with_refresh_token_auth("default_refresh_token");

    assert_eq!(
        crawler2.auth_strategy(),
        Some(&RedditAuthStrategy::RefreshToken {
            client_id: DEFAULT_REDDIT_PUBLIC_CLIENT_ID.to_string(),
            refresh_token: "default_refresh_token".to_string(),
        })
    );
    assert_eq!(crawler2.refresh_token(), Some("default_refresh_token"));
    assert_eq!(crawler2.credentials(), None);
    assert_eq!(
        crawler2.effective_base_url(),
        DEFAULT_REDDIT_OAUTH_BASE_URL
    );

    // 3. from_env with REDDIT_REFRESH_TOKEN alone (default client ID)
    let _guard = ENV_TEST_MUTEX.lock().await;
    std::env::set_var("REDDIT_REFRESH_TOKEN", "env_user_rt");
    let env_crawler1 = RedditCrawler::from_env();
    assert_eq!(
        env_crawler1.auth_strategy(),
        Some(&RedditAuthStrategy::RefreshToken {
            client_id: DEFAULT_REDDIT_PUBLIC_CLIENT_ID.to_string(),
            refresh_token: "env_user_rt".to_string(),
        })
    );
    assert_eq!(env_crawler1.refresh_token(), Some("env_user_rt"));
    assert_eq!(env_crawler1.credentials(), None);
    assert_eq!(
        env_crawler1.effective_base_url(),
        DEFAULT_REDDIT_OAUTH_BASE_URL
    );

    // 4. from_env with REDDIT_REFRESH_TOKEN and custom REDDIT_CLIENT_ID
    std::env::set_var("REDDIT_CLIENT_ID", "env_custom_cid");
    let env_crawler2 = RedditCrawler::from_env();
    assert_eq!(
        env_crawler2.auth_strategy(),
        Some(&RedditAuthStrategy::RefreshToken {
            client_id: "env_custom_cid".to_string(),
            refresh_token: "env_user_rt".to_string(),
        })
    );
    assert_eq!(env_crawler2.refresh_token(), Some("env_user_rt"));

    // 5. REDDIT_REFRESH_TOKEN takes precedence over REDDIT_CLIENT_SECRET
    std::env::set_var("REDDIT_CLIENT_SECRET", "unused_client_secret");
    let env_crawler3 = RedditCrawler::from_env();
    assert_eq!(
        env_crawler3.auth_strategy(),
        Some(&RedditAuthStrategy::RefreshToken {
            client_id: "env_custom_cid".to_string(),
            refresh_token: "env_user_rt".to_string(),
        })
    );

    // 6. If REDDIT_REFRESH_TOKEN is empty string, falls back to client credentials
    std::env::set_var("REDDIT_REFRESH_TOKEN", "   ");
    std::env::set_var("REDDIT_CLIENT_ID", "fallback_client_id");
    std::env::set_var("REDDIT_CLIENT_SECRET", "fallback_secret");
    let env_crawler4 = RedditCrawler::from_env();
    assert_eq!(
        env_crawler4.credentials(),
        Some(&RedditCredentials::new("fallback_client_id", "fallback_secret"))
    );

    // Clean up
    std::env::remove_var("REDDIT_REFRESH_TOKEN");
    std::env::remove_var("REDDIT_CLIENT_ID");
    std::env::remove_var("REDDIT_CLIENT_SECRET");
}

#[test]
fn test_parse_rate_limit_delay_scenarios() {
    use reqwest::header::{HeaderMap, HeaderValue};

    let fallback = Duration::from_secs(3);

    // 1. Neither header present -> fallback delay returned
    let headers = HeaderMap::new();
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(3));

    // 2. Retry-After with integer seconds
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("10"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(10));

    // When Retry-After is smaller than fallback, max(header, fallback) is taken
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("1"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(3));

    // 3. Retry-After with float seconds
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("4.2"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(5));

    // 4. X-Ratelimit-Reset with integer seconds
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("45"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(45));

    // Standard casing
    let mut headers = HeaderMap::new();
    headers.insert("X-Ratelimit-Reset", HeaderValue::from_static("20"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(20));

    // 5. X-Ratelimit-Reset with float seconds (rounded up using .ceil() as u64)
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("14.2"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(15));

    let mut headers = HeaderMap::new();
    headers.insert("X-Ratelimit-Reset", HeaderValue::from_static("0.1"));
    assert_eq!(parse_rate_limit_delay(&headers, Duration::from_secs(0)), Duration::from_secs(1));

    // 6. Both Retry-After and X-Ratelimit-Reset present (takes the max of both and fallback)
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("10"));
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("25.3"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(26));

    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("50"));
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("12"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(50));

    // 7. Clamped to upper bound of 300 seconds (5 minutes)
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("9999"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(300));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), MAX_RATE_LIMIT_DELAY);

    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("1200.5"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(300));

    // Fallback itself exceeding 300s is also clamped
    let headers = HeaderMap::new();
    assert_eq!(
        parse_rate_limit_delay(&headers, Duration::from_secs(600)),
        Duration::from_secs(300)
    );

    // 8. Invalid or negative headers are ignored, falling back safely
    let mut headers = HeaderMap::new();
    headers.insert("Retry-After", HeaderValue::from_static("invalid_delay"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(3));

    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("-10.5"));
    assert_eq!(parse_rate_limit_delay(&headers, fallback), Duration::from_secs(3));
}

#[tokio::test]
async fn test_fetch_access_token_429_backoff_and_recovery() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let app = Router::new().route(
        "/api/v1/access_token",
        post(move || {
            let attempts = attempts_clone.clone();
            async move {
                let count = attempts.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("Retry-After", "0"), ("X-Ratelimit-Reset", "0.01")],
                        "Rate limited",
                    )
                        .into_response()
                } else {
                    let resp = serde_json::json!({
                        "access_token": "token_recovered_after_429",
                        "token_type": "bearer",
                        "expires_in": 3600
                    });
                    (StatusCode::OK, resp.to_string()).into_response()
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let token_url = format!("http://{}/api/v1/access_token", addr);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 3, 2.0);
    let crawler = RedditCrawler::new()
        .with_token_url(token_url)
        .with_client_credentials("test_id", "test_secret")
        .with_backoff(backoff);

    let token = crawler
        .get_or_refresh_token()
        .await
        .expect("OAuth token acquired after retry");
    assert_eq!(token, "token_recovered_after_429");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_fetch_access_token_429_exhausted_retries() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let app = Router::new().route(
        "/api/v1/access_token",
        post(move || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", "0")],
                    "Always rate limited",
                )
                    .into_response()
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let token_url = format!("http://{}/api/v1/access_token", addr);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 2, 2.0);
    let crawler = RedditCrawler::new()
        .with_token_url(token_url)
        .with_client_credentials("test_id", "test_secret")
        .with_backoff(backoff);

    let err = crawler
        .get_or_refresh_token()
        .await
        .expect_err("should exhaust retries");
    match err {
        HarvesterError::RateLimited { retries } => {
            assert_eq!(retries, 2);
        }
        other => panic!("expected HarvesterError::RateLimited, got: {other:?}"),
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 3); // initial + 2 retries
}

#[tokio::test]
async fn test_fetch_posts_rate_limit_with_x_ratelimit_reset_header() {
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let app = Router::new().route(
        "/r/battlemaps/hot.json",
        get(move || {
            let attempts = attempts_clone.clone();
            async move {
                let count = attempts.fetch_add(1, Ordering::SeqCst);
                if count < 1 {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("X-Ratelimit-Reset", "0.01")],
                        "Rate limited with reset header",
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
    let backoff = BackoffConfig::new(Duration::from_millis(5), 3, 2.0);
    let crawler = RedditCrawler::new()
        .with_base_url(base_url)
        .with_backoff(backoff);

    let query = RedditHarvestQuery::battlemaps().with_sort(RedditSort::Hot);

    let posts = crawler
        .fetch_posts(&query)
        .await
        .expect("succeeds after backoff driven by x-ratelimit-reset");
    assert_eq!(posts.len(), 2);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_backoff_config_from_env_configuration() {
    let _guard = ENV_TEST_MUTEX.lock().await;

    // 1. Defaults when environment variables are not set
    std::env::remove_var("REDDIT_BACKOFF_INITIAL_SECS");
    std::env::remove_var("REDDIT_BACKOFF_MAX_RETRIES");
    std::env::remove_var("REDDIT_BACKOFF_FACTOR");

    let default_crawler = RedditCrawler::from_env();
    assert_eq!(
        default_crawler.backoff().initial_delay,
        Duration::from_secs(3)
    );
    assert_eq!(default_crawler.backoff().max_retries, 5);
    assert_eq!(default_crawler.backoff().backoff_factor, 2.0);

    // 2. Custom environment variable configuration
    std::env::set_var("REDDIT_BACKOFF_INITIAL_SECS", "10");
    std::env::set_var("REDDIT_BACKOFF_MAX_RETRIES", "7");
    std::env::set_var("REDDIT_BACKOFF_FACTOR", "2.5");

    let custom_crawler = RedditCrawler::from_env();
    assert_eq!(
        custom_crawler.backoff().initial_delay,
        Duration::from_secs(10)
    );
    assert_eq!(custom_crawler.backoff().max_retries, 7);
    assert_eq!(custom_crawler.backoff().backoff_factor, 2.5);

    // Clean up
    std::env::remove_var("REDDIT_BACKOFF_INITIAL_SECS");
    std::env::remove_var("REDDIT_BACKOFF_MAX_RETRIES");
    std::env::remove_var("REDDIT_BACKOFF_FACTOR");
}
