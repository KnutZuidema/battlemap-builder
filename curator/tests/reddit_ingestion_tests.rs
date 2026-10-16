use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use async_trait::async_trait;
use axum::routing::get;
use axum::Router;
use chrono::Utc;

use curator::adapters::filesystem_storage::FilesystemStorage;
use curator::adapters::reddit::BackoffConfig;
use curator::domain::{
    CuratedMap, HarvestedMedia, LayerRole, ParsedTags, RedditHarvestQuery, RedditPost,
    RejectionReason, Source, Theme,
};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::ports::storage::StorageRepository;
use curator::services::{
    IngestionOutcome, IngestionProgress, RedditIngestionService, SkipReason,
};

/// Mock harvester that returns pre-configured Reddit posts.
#[derive(Clone, Default)]
struct MockHarvester {
    posts: Vec<RedditPost>,
}

impl MockHarvester {
    fn new(posts: Vec<RedditPost>) -> Self {
        Self { posts }
    }
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

/// Helper to generate a minimal valid 1x1 PNG image.
fn create_test_png() -> Vec<u8> {
    let img = image::RgbaImage::new(1, 1);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
    bytes
}

#[tokio::test]
async fn test_ingestion_deduplication_against_rejections() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    // Log a rejection for post_rejected
    storage
        .log_rejection("post_rejected", RejectionReason::GridBurnedIn, Some("Visible grid"))
        .unwrap();

    let post = RedditPost {
        id: "post_rejected".into(),
        name: "t3_post_rejected".into(),
        title: "[30x30] Ruined Keep".into(),
        author: "mapper_bob".into(),
        permalink: "/r/battlemaps/comments/post_rejected/".into(),
        url: "http://127.0.0.1/fake.png".into(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new("http://127.0.0.1/fake.png")],
        tags: ParsedTags::parse_title("[30x30] Ruined Keep"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let outcome = service.ingest_media(&post, 0).await.unwrap();
    assert_eq!(
        outcome,
        IngestionOutcome::Skipped {
            id: "post_rejected".into(),
            reason: SkipReason::Rejected,
        }
    );

    let summary = service.ingest_post(&post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 0);
    assert_eq!(summary.skipped, 1);
}

#[tokio::test]
async fn test_ingestion_deduplication_against_staging() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    // Pre-populate an item in staging/reddit
    let post = RedditPost {
        id: "staged_map".into(),
        name: "t3_staged_map".into(),
        title: "[40x40] Castle Courtyard [No Grid]".into(),
        author: "castle_builder".into(),
        permalink: "/r/battlemaps/comments/staged_map/".into(),
        url: "http://127.0.0.1/castle.png".into(),
        score: 250,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new("http://127.0.0.1/castle.png")],
        tags: ParsedTags::parse_title("[40x40] Castle Courtyard [No Grid]"),
    };

    let sidecar = post.to_sidecar_metadata(0).unwrap();
    storage.save_staging_reddit(&sidecar, &create_test_png()).unwrap();

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let outcome = service.ingest_media(&post, 0).await.unwrap();
    assert_eq!(
        outcome,
        IngestionOutcome::Skipped {
            id: "staged_map".into(),
            reason: SkipReason::AlreadyInStaging,
        }
    );
}

#[tokio::test]
async fn test_ingestion_deduplication_against_curated() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    // Save a curated map
    let curated = CuratedMap::with_now(
        "curated_map_1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (30, 30),
    );
    storage.save_curated(&curated, &create_test_png()).unwrap();

    let post = RedditPost {
        id: "curated_map_1".into(),
        name: "t3_curated_map_1".into(),
        title: "[30x30] Deep Dungeon [Gridless]".into(),
        author: "dungeon_guy".into(),
        permalink: "/r/battlemaps/comments/curated_map_1/".into(),
        url: "http://127.0.0.1/deep.png".into(),
        score: 500,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new("http://127.0.0.1/deep.png")],
        tags: ParsedTags::parse_title("[30x30] Deep Dungeon [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let outcome = service.ingest_media(&post, 0).await.unwrap();
    assert_eq!(
        outcome,
        IngestionOutcome::Skipped {
            id: "curated_map_1".into(),
            reason: SkipReason::AlreadyCurated,
        }
    );
}

#[tokio::test]
async fn test_end_to_end_ingestion_download_and_staging_persistence() {
    // 1. Spawn a mock image server
    let png_bytes = create_test_png();
    let png_clone = png_bytes.clone();

    let app = Router::new().route(
        "/image.png",
        get(move || {
            let data = png_clone.clone();
            async move { (axum::http::StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let image_url = format!("http://{addr}/image.png");

    // 2. Setup storage and harvester
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post = RedditPost {
        id: "harvest_1".into(),
        name: "t3_harvest_1".into(),
        title: "[50x50] Crystal Spire [No Grid]".into(),
        author: "spire_architect".into(),
        permalink: "https://www.reddit.com/r/battlemaps/comments/harvest_1/crystal_spire/".into(),
        url: image_url.clone(),
        score: 320,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[50x50] Crystal Spire [No Grid]"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    // 3. Run ingest_posts
    let summary = service.ingest_posts(&[post.clone()]).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    // 4. Verify files on disk in staging/reddit/
    let reddit_staging = storage.staging_dir().join("reddit");
    let bundle_dir = reddit_staging.join("harvest_1");
    let image_file = if bundle_dir.join("0.png").is_file() {
        bundle_dir.join("0.png")
    } else {
        reddit_staging.join("harvest_1.png")
    };
    let sidecar_file = if bundle_dir.join("manifest.json").is_file() {
        bundle_dir.join("manifest.json")
    } else {
        reddit_staging.join("harvest_1.json")
    };

    assert!(image_file.is_file(), "image file must exist");
    assert!(sidecar_file.is_file(), "sidecar json must exist");
    let saved_bytes = std::fs::read(&image_file).unwrap();
    assert_eq!(saved_bytes, png_bytes);

    // 5. Verify sidecar JSON matches Issue #25 schema
    let sidecar_content = std::fs::read_to_string(&sidecar_file).unwrap();
    let sidecar_value: serde_json::Value = serde_json::from_str(&sidecar_content).unwrap();

    assert_eq!(sidecar_value["id"], "harvest_1");
    assert_eq!(sidecar_value["source"], "reddit");
    assert!(sidecar_value["source_url"].as_str().unwrap().contains(&image_url));
    assert_eq!(sidecar_value["title"], "[50x50] Crystal Spire [No Grid]");
    assert_eq!(sidecar_value["author"], "spire_architect");
    assert_eq!(sidecar_value["parsed_gridless"], true);
    assert_eq!(sidecar_value["parsed_dimensions"], serde_json::json!([50, 50]));
    assert!(sidecar_value["created_at"].is_string());

    // 6. Verify list_staging() discovers and deserializes the staged candidate
    let staged_candidates = storage.list_staging().unwrap();
    assert_eq!(staged_candidates.len(), 1);
    let cand = &staged_candidates[0];
    assert_eq!(cand.id, "harvest_1");
    assert_eq!(cand.source, Source::Reddit);
    assert_eq!(cand.dimensions, (50, 50));
    assert_eq!(cand.parsed_gridless, Some(true));
    assert_eq!(cand.author.as_deref(), Some("spire_architect"));

    // 7. Verify repeated harvest skips previously downloaded candidate
    let second_summary = service.ingest_posts(&[post.clone()]).await.unwrap();
    assert_eq!(second_summary.processed, 1);
    assert_eq!(second_summary.downloaded, 0);
    assert_eq!(second_summary.skipped, 1);

    // 8. Verify curate_staging moves the candidate from staging/reddit to curated/
    let curated = CuratedMap::with_now(
        "harvest_1",
        Theme::SciFi,
        vec!["crystal".into(), "spire".into()],
        (50, 50),
    );
    storage.curate_staging(&curated).unwrap();

    assert!(!image_file.exists(), "image moved from staging");
    assert!(!sidecar_file.exists(), "sidecar deleted from staging");
    assert!(storage.is_curated("harvest_1").unwrap());

    let curated_bytes = storage.get_curated_image(&Theme::SciFi, "harvest_1").unwrap();
    assert_eq!(curated_bytes, png_bytes);

    // 9. Third harvest now skips due to AlreadyCurated
    let third_outcome = service.ingest_media(&post, 0).await.unwrap();
    assert_eq!(
        third_outcome,
        IngestionOutcome::Skipped {
            id: "harvest_1".into(),
            reason: SkipReason::AlreadyCurated,
        }
    );
}

#[tokio::test]
async fn test_multi_image_gallery_ingestion() {
    let png_bytes = create_test_png();
    let png_clone = png_bytes.clone();

    let app = Router::new().route(
        "/gallery.png",
        get(move || {
            let data = png_clone.clone();
            async move { (axum::http::StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let img_url = format!("http://{addr}/gallery.png");

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let gallery_post = RedditPost {
        id: "gallery_post".into(),
        name: "t3_gallery_post".into(),
        title: "[20x20] Forest Encounter [Gridless]".into(),
        author: "druid_mapper".into(),
        permalink: "https://www.reddit.com/r/battlemaps/comments/gallery_post/".into(),
        url: img_url.clone(),
        score: 150,
        created_utc: Utc::now(),
        media: vec![
            HarvestedMedia::new(img_url.clone()).with_caption("Ground Floor"),
            HarvestedMedia::new(img_url.clone()).with_caption("Upper Floor"),
        ],
        tags: ParsedTags::parse_title("[20x20] Forest Encounter [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![gallery_post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let summary = service.ingest_post(&gallery_post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    let reddit_staging = storage.staging_dir().join("reddit");
    let bundle_dir = reddit_staging.join("gallery_post");
    assert!(bundle_dir.join("0.png").is_file());
    assert!(bundle_dir.join("1.png").is_file());
    assert!(bundle_dir.join("manifest.json").is_file());

    // Verify staged candidate metadata and layers
    let staged = storage.list_staging().unwrap();
    assert_eq!(staged.len(), 1);
    let cand = &staged[0];
    assert_eq!(cand.id, "gallery_post");
    assert_eq!(cand.source, Source::Reddit);
    assert_eq!(cand.dimensions, (20, 20));
    assert_eq!(cand.parsed_gridless, Some(true));
    assert_eq!(cand.layers.len(), 2);

    assert_eq!(cand.layers[0].index, 0);
    assert_eq!(cand.layers[0].filename, "0.png");
    assert_eq!(cand.layers[0].role, LayerRole::Ground);
    assert_eq!(cand.layers[0].elevation, Some(0));
    assert_eq!(cand.layers[0].caption.as_deref(), Some("Ground Floor"));

    assert_eq!(cand.layers[1].index, 1);
    assert_eq!(cand.layers[1].filename, "1.png");
    assert_eq!(cand.layers[1].role, LayerRole::UpperFloor);
    assert_eq!(cand.layers[1].elevation, Some(1));
    assert_eq!(cand.layers[1].caption.as_deref(), Some("Upper Floor"));

    // Subsequent harvest skips because already in staging
    let re_summary = service.ingest_post(&gallery_post).await.unwrap();
    assert_eq!(re_summary.processed, 1);
    assert_eq!(re_summary.downloaded, 0);
    assert_eq!(re_summary.skipped, 1);

    let re_outcome = service.ingest_media(&gallery_post, 0).await.unwrap();
    assert_eq!(
        re_outcome,
        IngestionOutcome::Skipped {
            id: "gallery_post".into(),
            reason: SkipReason::AlreadyInStaging,
        }
    );

    // Reject gallery_post
    storage
        .reject_staging("gallery_post", RejectionReason::BlurryLowRes, Some("Blurry variant"))
        .unwrap();

    assert!(!bundle_dir.exists());
    assert!(storage.is_rejected("gallery_post").unwrap());

    // Subsequent harvest skips because rejected
    let re_outcome_rej = service.ingest_media(&gallery_post, 0).await.unwrap();
    assert_eq!(
        re_outcome_rej,
        IngestionOutcome::Skipped {
            id: "gallery_post".into(),
            reason: SkipReason::Rejected,
        }
    );
}

#[tokio::test]
async fn test_split_post_deduplication() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    // Case 1: Split child gallery_split_0 exists in staging
    let split_child_post = RedditPost {
        id: "gallery_split".into(),
        name: "t3_gallery_split".into(),
        title: "[30x30] Forest Split [Gridless]".into(),
        author: "druid_mapper".into(),
        permalink: "https://www.reddit.com/r/battlemaps/comments/gallery_split/".into(),
        url: "http://127.0.0.1/fake.png".into(),
        score: 150,
        created_utc: Utc::now(),
        media: vec![
            HarvestedMedia::new("http://127.0.0.1/fake0.png"),
            HarvestedMedia::new("http://127.0.0.1/fake1.png"),
        ],
        tags: ParsedTags::parse_title("[30x30] Forest Split [Gridless]"),
    };

    // Pre-populate split child "gallery_split_0" into staging
    let child_sidecar = split_child_post.to_sidecar_metadata(0).unwrap();
    assert_eq!(child_sidecar.id, "gallery_split_0");
    storage
        .save_staging_reddit(&child_sidecar, &create_test_png())
        .unwrap();
    assert!(storage.is_in_staging("gallery_split_0").unwrap());

    let harvester = MockHarvester::new(vec![split_child_post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    // Harvest should skip gallery_split because child gallery_split_0 is already in staging
    let summary = service.ingest_post(&split_child_post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 0);
    assert_eq!(summary.skipped, 1);

    let outcome = service.ingest_media(&split_child_post, 0).await.unwrap();
    assert_eq!(
        outcome,
        IngestionOutcome::Skipped {
            id: "gallery_split".into(),
            reason: SkipReason::AlreadyInStaging,
        }
    );

    // Case 2: Split child rejected in rejections.json
    storage
        .log_rejection(
            "gallery_rejected_0",
            RejectionReason::GridBurnedIn,
            Some("Burned grid on layer 0"),
        )
        .unwrap();
    assert!(storage.is_rejected("gallery_rejected_0").unwrap());

    let split_rejected_post = RedditPost {
        id: "gallery_rejected".into(),
        name: "t3_gallery_rejected".into(),
        title: "[25x25] Ruined Tower".into(),
        author: "mapper".into(),
        permalink: "/comments/gallery_rejected/".into(),
        url: "http://127.0.0.1/rej.png".into(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![
            HarvestedMedia::new("http://127.0.0.1/rej0.png"),
            HarvestedMedia::new("http://127.0.0.1/rej1.png"),
        ],
        tags: ParsedTags::parse_title("[25x25] Ruined Tower"),
    };

    let outcome_rej = service
        .ingest_media(&split_rejected_post, 0)
        .await
        .unwrap();
    assert_eq!(
        outcome_rej,
        IngestionOutcome::Skipped {
            id: "gallery_rejected".into(),
            reason: SkipReason::Rejected,
        }
    );

    // Case 3: Split child curated
    let curated = CuratedMap::with_now(
        "gallery_curated_0",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (25, 25),
    );
    storage.save_curated(&curated, &create_test_png()).unwrap();
    assert!(storage.is_curated("gallery_curated_0").unwrap());

    let split_curated_post = RedditPost {
        id: "gallery_curated".into(),
        name: "t3_gallery_curated".into(),
        title: "[25x25] Curated Dungeon".into(),
        author: "mapper".into(),
        permalink: "/comments/gallery_curated/".into(),
        url: "http://127.0.0.1/cur.png".into(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![
            HarvestedMedia::new("http://127.0.0.1/cur0.png"),
            HarvestedMedia::new("http://127.0.0.1/cur1.png"),
        ],
        tags: ParsedTags::parse_title("[25x25] Curated Dungeon"),
    };

    let outcome_cur = service
        .ingest_media(&split_curated_post, 0)
        .await
        .unwrap();
    assert_eq!(
        outcome_cur,
        IngestionOutcome::Skipped {
            id: "gallery_curated".into(),
            reason: SkipReason::AlreadyCurated,
        }
    );
}

#[tokio::test]
async fn test_ingest_query_filtered_gridless_only() {
    let png_bytes = create_test_png();
    let app = Router::new().route(
        "/image.png",
        get(move || {
            let data = png_bytes.clone();
            async move { (axum::http::StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let image_url = format!("http://{addr}/image.png");

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post_gridless = RedditPost {
        id: "gridless_1".into(),
        name: "t3_gridless_1".into(),
        title: "[30x30] Forest Clearing [Gridless]".into(),
        author: "cartographer".into(),
        permalink: "/r/battlemaps/comments/gridless_1/".into(),
        url: image_url.clone(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[30x30] Forest Clearing [Gridless]"),
    };

    let post_gridded = RedditPost {
        id: "gridded_1".into(),
        name: "t3_gridded_1".into(),
        title: "[30x30] Forest Clearing [Gridded]".into(),
        author: "cartographer".into(),
        permalink: "/r/battlemaps/comments/gridded_1/".into(),
        url: image_url.clone(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[30x30] Forest Clearing [Gridded]"),
    };

    let post_unknown = RedditPost {
        id: "unknown_1".into(),
        name: "t3_unknown_1".into(),
        title: "Dungeon Map".into(),
        author: "cartographer".into(),
        permalink: "/r/battlemaps/comments/unknown_1/".into(),
        url: image_url.clone(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("Dungeon Map"),
    };

    let harvester = MockHarvester::new(vec![post_gridless, post_gridded, post_unknown]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let query = RedditHarvestQuery::default();
    let summary = service.ingest_query_filtered(&query, true).await.unwrap();

    assert_eq!(summary.processed, 3);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 2);

    // Only gridless candidate should be in staging
    assert!(storage.is_in_staging("gridless_1").unwrap());
    assert!(!storage.is_in_staging("gridded_1").unwrap());
    assert!(!storage.is_in_staging("unknown_1").unwrap());
}

#[tokio::test]
async fn test_ingest_query_progress_events() {
    let png_bytes = create_test_png();
    let app = Router::new().route(
        "/image.png",
        get(move || {
            let data = png_bytes.clone();
            async move { (axum::http::StatusCode::OK, [("content-type", "image/png")], data) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let image_url = format!("http://{addr}/image.png");

    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post1 = RedditPost {
        id: "prog_1".into(),
        name: "t3_prog_1".into(),
        title: "Map 1 [Gridless]".into(),
        author: "cartographer".into(),
        permalink: "/r/battlemaps/comments/prog_1/".into(),
        url: image_url.clone(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("Map 1 [Gridless]"),
    };

    let post2 = RedditPost {
        id: "prog_2".into(),
        name: "t3_prog_2".into(),
        title: "Map 2".into(),
        author: "cartographer".into(),
        permalink: "/r/battlemaps/comments/prog_2/".into(),
        url: image_url.clone(),
        score: 100,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("Map 2"),
    };

    let harvester = MockHarvester::new(vec![post1, post2]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let query = RedditHarvestQuery::default();
    let mut events: Vec<IngestionProgress> = Vec::new();

    let summary = service
        .ingest_query_progress(&query, true, |progress| {
            events.push(progress);
        })
        .await
        .unwrap();

    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 1);
    assert!(!events.is_empty());
    assert_eq!(events[0].phase, "fetching");
    assert_eq!(events.last().unwrap().phase, "complete");
}

#[tokio::test]
async fn test_media_download_429_backoff_and_recovery() {
    let png_bytes = create_test_png();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let app = Router::new().route(
        "/recoverable.png",
        get(move || {
            let data = png_bytes.clone();
            let c = counter_clone.clone();
            async move {
                let count = c.fetch_add(1, Ordering::SeqCst);
                if count == 0 {
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "0"), ("content-type", "text/plain")],
                        vec![],
                    )
                } else if count == 1 {
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("x-ratelimit-reset", "0"), ("content-type", "text/plain")],
                        vec![],
                    )
                } else {
                    (
                        axum::http::StatusCode::OK,
                        [("retry-after", "0"), ("content-type", "image/png")],
                        data,
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let image_url = format!("http://{addr}/recoverable.png");
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post = RedditPost {
        id: "recover_post".into(),
        name: "t3_recover_post".into(),
        title: "[20x20] Ambush Pass [Gridless]".into(),
        author: "ranger".into(),
        permalink: "/r/battlemaps/comments/recover_post/".into(),
        url: image_url.clone(),
        score: 120,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[20x20] Ambush Pass [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 3, 1.5);
    let service = RedditIngestionService::new(harvester, storage.clone()).with_backoff(backoff);

    assert_eq!(service.backoff().max_retries, 3);

    let summary = service.ingest_post(&post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    // Verify file exists on disk
    let staged_bundle = storage.staging_dir().join("reddit").join("recover_post");
    let staged_file = storage.staging_dir().join("reddit").join("recover_post.png");
    assert!(staged_bundle.join("0.png").is_file() || staged_file.is_file());
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_media_download_429_exhausted_retries_continues_batch() {
    let png_bytes = create_test_png();
    let png_clone = png_bytes.clone();
    let fail_counter = Arc::new(AtomicUsize::new(0));
    let fail_counter_clone = fail_counter.clone();

    let app = Router::new()
        .route(
            "/always_429.png",
            get(move || {
                let c = fail_counter_clone.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "0"), ("content-type", "text/plain")],
                        vec![],
                    )
                }
            }),
        )
        .route(
            "/ok.png",
            get(move || {
                let data = png_clone.clone();
                async move {
                    (
                        axum::http::StatusCode::OK,
                        [("content-type", "image/png")],
                        data,
                    )
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let fail_url = format!("http://{addr}/always_429.png");
    let ok_url = format!("http://{addr}/ok.png");
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post1_fail = RedditPost {
        id: "post_1_fail".into(),
        name: "t3_post_1_fail".into(),
        title: "[25x25] Dragon Lair [Gridless]".into(),
        author: "dragon_master".into(),
        permalink: "/r/battlemaps/comments/post_1_fail/".into(),
        url: fail_url.clone(),
        score: 300,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(fail_url.clone())],
        tags: ParsedTags::parse_title("[25x25] Dragon Lair [Gridless]"),
    };

    let post2_ok = RedditPost {
        id: "post_2_ok".into(),
        name: "t3_post_2_ok".into(),
        title: "[30x30] Goblin Camp [Gridless]".into(),
        author: "goblin_king".into(),
        permalink: "/r/battlemaps/comments/post_2_ok/".into(),
        url: ok_url.clone(),
        score: 200,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(ok_url.clone())],
        tags: ParsedTags::parse_title("[30x30] Goblin Camp [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![post1_fail.clone(), post2_ok.clone()]);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 2, 1.5);
    let service = RedditIngestionService::new(harvester, storage.clone()).with_backoff(backoff);

    // First verify ingest_media for post1_fail directly returns SkipReason::DownloadFailed
    let outcome = service.ingest_media(&post1_fail, 0).await.unwrap();
    assert_eq!(
        outcome,
        IngestionOutcome::Skipped {
            id: "post_1_fail".into(),
            reason: SkipReason::DownloadFailed,
        }
    );

    // Now verify batch execution of both posts continues
    let summary = service
        .ingest_posts(&[post1_fail.clone(), post2_ok.clone()])
        .await
        .unwrap();

    assert_eq!(summary.processed, 2);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 1);

    // Staging must NOT have post_1_fail, but MUST have post_2_ok
    let reddit_staging = storage.staging_dir().join("reddit");
    assert!(!reddit_staging.join("post_1_fail").exists());
    assert!(!reddit_staging.join("post_1_fail.png").exists());
    assert!(!reddit_staging.join("post_1_fail.json").exists());
    assert!(reddit_staging.join("post_2_ok").join("0.png").is_file() || reddit_staging.join("post_2_ok.png").is_file());
    assert!(reddit_staging.join("post_2_ok").join("manifest.json").is_file() || reddit_staging.join("post_2_ok.json").is_file());
}

#[tokio::test]
async fn test_rate_limit_progress_events_emitted_during_backoff() {
    let png_bytes = create_test_png();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let app = Router::new().route(
        "/rate_limit_progress.png",
        get(move || {
            let data = png_bytes.clone();
            let c = counter_clone.clone();
            async move {
                let count = c.fetch_add(1, Ordering::SeqCst);
                if count == 0 {
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "1"), ("content-type", "text/plain")],
                        vec![],
                    )
                } else {
                    (
                        axum::http::StatusCode::OK,
                        [("retry-after", "0"), ("content-type", "image/png")],
                        data,
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let image_url = format!("http://{addr}/rate_limit_progress.png");
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));

    let post = RedditPost {
        id: "rl_post".into(),
        name: "t3_rl_post".into(),
        title: "[40x40] Sunken Temple [Gridless]".into(),
        author: "priest".into(),
        permalink: "/r/battlemaps/comments/rl_post/".into(),
        url: image_url.clone(),
        score: 180,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("[40x40] Sunken Temple [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![post]);
    let backoff = BackoffConfig::new(Duration::from_millis(5), 3, 2.0);
    let service = RedditIngestionService::new(harvester, storage).with_backoff(backoff);

    let query = RedditHarvestQuery::default();
    let mut events: Vec<IngestionProgress> = Vec::new();

    let summary = service
        .ingest_query_progress(&query, false, |progress| {
            events.push(progress);
        })
        .await
        .unwrap();

    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    let rl_event = events
        .iter()
        .find(|e| e.phase == "rate_limited")
        .expect("Must have emitted a rate_limited progress event");

    assert_eq!(rl_event.candidate_id.as_deref(), Some("rl_post"));
    assert_eq!(rl_event.attempt, Some(1));
    assert_eq!(rl_event.max_retries, Some(3));
    assert!(rl_event.message.contains("Rate limited"));
    assert!(rl_event.wait_seconds.is_some());
    assert_eq!(events.last().unwrap().phase, "complete");
}

#[test]
fn test_reddit_ingestion_service_backoff_config() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(dir.path()));
    let harvester = MockHarvester::default();
    let service = RedditIngestionService::new(harvester, storage.clone());

    assert_eq!(service.backoff().initial_delay, Duration::from_secs(3));
    assert_eq!(service.backoff().max_retries, 5);
    assert_eq!(service.backoff().backoff_factor, 2.0);

    let custom = BackoffConfig::new(Duration::from_secs(1), 2, 1.5);
    let service = service.with_backoff(custom.clone());
    assert_eq!(service.backoff(), &custom);
}
