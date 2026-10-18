use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use tempfile::tempdir;

use curator::ports::StorageRepository;
use curator::adapters::FilesystemStorage;
use curator::api::create_router;
use curator::api::routes::DescribeResponse;
use curator::api::state::AppState;
use curator::domain::{Candidate, Source};
use curator::services::description::{
    DescriptionError, DescriptionGenerator, DescriptionService,
};

/// Mock generator that can be configured to succeed, fail, or return empty text.
struct MockDescriptionGenerator {
    result: Result<String, DescriptionError>,
    invoked: AtomicBool,
}

impl MockDescriptionGenerator {
    fn succeeding(text: impl Into<String>) -> Self {
        Self {
            result: Ok(text.into()),
            invoked: AtomicBool::new(false),
        }
    }

    fn failing(err: DescriptionError) -> Self {
        Self {
            result: Err(err),
            invoked: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl DescriptionGenerator for MockDescriptionGenerator {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _mime_type: &str,
        _tags: &[String],
        _context: Option<&str>,
    ) -> Result<String, DescriptionError> {
        self.invoked.store(true, Ordering::SeqCst);
        self.result.clone()
    }
}

fn create_test_png() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A,
        0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A',
        0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
        0x00, 0x00, 0x00, b'I', b'E', b'N', b'D',
        0xAE, 0x42, 0x60, 0x82,
    ]
}

#[tokio::test]
async fn test_description_service_success() {
    let mock = Arc::new(MockDescriptionGenerator::succeeding(
        "Ancient stone tomb with mossy sarcophagi and flickering torchlight.",
    ));
    let service = DescriptionService::new(Some(mock.clone()));

    let image = create_test_png();
    let tags = vec!["crypt".to_string(), "tomb".to_string()];
    let desc = service
        .describe(&image, "image/png", &tags, Some("dungeon"))
        .await
        .expect("description generation should succeed");

    assert_eq!(
        desc,
        "Ancient stone tomb with mossy sarcophagi and flickering torchlight."
    );
    assert!(mock.invoked.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_description_service_unconfigured_strictly_fails() {
    let service = DescriptionService::new(None);

    let image = create_test_png();
    let tags = vec!["dungeon".to_string()];
    let err = service
        .describe(&image, "image/png", &tags, None)
        .await
        .expect_err("unconfigured generator must return DescriptionError::NotConfigured");

    assert!(
        matches!(err, DescriptionError::NotConfigured(_)),
        "expected NotConfigured, got {err:?}"
    );
}

#[tokio::test]
async fn test_description_service_generation_failure_propagates() {
    let mock = Arc::new(MockDescriptionGenerator::failing(
        DescriptionError::GenerationFailed("upstream 500 error".into()),
    ));
    let service = DescriptionService::new(Some(mock.clone()));

    let image = create_test_png();
    let tags = vec!["forest".to_string()];
    let err = service
        .describe(&image, "image/png", &tags, None)
        .await
        .expect_err("generation failure must strictly propagate");

    match err {
        DescriptionError::GenerationFailed(msg) => {
            assert!(msg.contains("upstream 500 error"));
        }
        other => panic!("expected GenerationFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_description_service_empty_description_yields_error() {
    let mock = Arc::new(MockDescriptionGenerator::succeeding("   \n\t  "));
    let service = DescriptionService::new(Some(mock));

    let image = create_test_png();
    let tags = vec!["cave".to_string()];
    let err = service
        .describe(&image, "image/png", &tags, None)
        .await
        .expect_err("empty description text must fail with EmptyDescription");

    assert!(matches!(err, DescriptionError::EmptyDescription));
}

#[tokio::test]
async fn test_api_staging_describe_strictly_fails_when_unconfigured() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let cand = Candidate::with_now(
        "cand_unconfigured_desc",
        Source::Reddit,
        "cand_unconfigured_desc.png",
        (1024, 1024),
    );
    storage.save_staging(&cand, &create_test_png()).unwrap();

    // AppState without OpenRouter / DescriptionService generator
    let state = AppState::new(storage)
        .with_description_service(Arc::new(DescriptionService::new(None)));
    let app = create_router(state);

    let req = Request::builder()
        .uri("/api/staging/cand_unconfigured_desc/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "tags": ["crypt"] }).to_string()))
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    // Must NOT return 200 OK with synthetic fallback
    assert_ne!(
        resp.status(),
        StatusCode::OK,
        "must not return 200 OK when description service is unconfigured"
    );
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_api_staging_describe_strictly_fails_when_upstream_errors() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let cand = Candidate::with_now(
        "cand_failing_desc",
        Source::Reddit,
        "cand_failing_desc.png",
        (1024, 1024),
    );
    storage.save_staging(&cand, &create_test_png()).unwrap();

    let mock_gen = Arc::new(MockDescriptionGenerator::failing(
        DescriptionError::GenerationFailed("rate limit reached".into()),
    ));
    let service = Arc::new(DescriptionService::new(Some(mock_gen)));
    let state = AppState::new(storage).with_description_service(service);
    let app = create_router(state);

    let req = Request::builder()
        .uri("/api/staging/cand_failing_desc/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "tags": ["lava"] }).to_string()))
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_GATEWAY,
        "must return 502 Bad Gateway on upstream failure without fallback"
    );
}

#[tokio::test]
async fn test_api_staging_describe_succeeds_with_configured_service() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let cand = Candidate::with_now(
        "cand_success_desc",
        Source::Reddit,
        "cand_success_desc.png",
        (1024, 1024),
    );
    storage.save_staging(&cand, &create_test_png()).unwrap();

    let mock_gen = Arc::new(MockDescriptionGenerator::succeeding(
        "Granite dungeon corridor with barred iron portcullis and damp flagstones.",
    ));
    let service = Arc::new(DescriptionService::new(Some(mock_gen)));
    let state = AppState::new(storage).with_description_service(service);
    let app = create_router(state);

    let req = Request::builder()
        .uri("/api/staging/cand_success_desc/describe")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "tags": ["dungeon", "corridor"] }).to_string()))
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: DescribeResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(res.id, "cand_success_desc");
    assert_eq!(
        res.description,
        "Granite dungeon corridor with barred iron portcullis and damp flagstones."
    );
}
