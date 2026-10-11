use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;

use async_trait::async_trait;
use axum::routing::get;
use axum::Router;
use chrono::Utc;

use curator::adapters::filesystem_storage::FilesystemStorage;
use curator::domain::{
    CuratedMap, HarvestedMedia, ParsedTags, RedditHarvestQuery, RedditPost, RejectionReason,
    Source, Theme,
};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::ports::storage::StorageRepository;
use curator::services::{
    candidate_id_for_post, IngestionOutcome, IngestionProgress, RedditIngestionService, SkipReason,
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
        4,
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
    let image_file = reddit_staging.join("harvest_1.png");
    let sidecar_file = reddit_staging.join("harvest_1.json");

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
        5,
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
            HarvestedMedia::new(img_url.clone()),
            HarvestedMedia::new(img_url.clone()),
        ],
        tags: ParsedTags::parse_title("[20x20] Forest Encounter [Gridless]"),
    };

    assert_eq!(candidate_id_for_post(&gallery_post.id, 0, 2), "gallery_post_0");
    assert_eq!(candidate_id_for_post(&gallery_post.id, 1, 2), "gallery_post_1");

    let harvester = MockHarvester::new(vec![gallery_post.clone()]);
    let service = RedditIngestionService::new(harvester, storage.clone());

    let summary = service.ingest_post(&gallery_post).await.unwrap();
    assert_eq!(summary.processed, 2);
    assert_eq!(summary.downloaded, 2);
    assert_eq!(summary.skipped, 0);

    let reddit_staging = storage.staging_dir().join("reddit");
    assert!(reddit_staging.join("gallery_post_0.png").is_file());
    assert!(reddit_staging.join("gallery_post_0.json").is_file());
    assert!(reddit_staging.join("gallery_post_1.png").is_file());
    assert!(reddit_staging.join("gallery_post_1.json").is_file());

    // Reject gallery_post_1
    storage
        .reject_staging("gallery_post_1", RejectionReason::BlurryLowRes, Some("Blurry variant"))
        .unwrap();

    assert!(!reddit_staging.join("gallery_post_1.png").exists());
    assert!(!reddit_staging.join("gallery_post_1.json").exists());
    assert!(storage.is_rejected("gallery_post_1").unwrap());

    // Subsequent harvest skips both (item 0 in staging, item 1 rejected)
    let re_outcome_0 = service.ingest_media(&gallery_post, 0).await.unwrap();
    assert_eq!(
        re_outcome_0,
        IngestionOutcome::Skipped {
            id: "gallery_post_0".into(),
            reason: SkipReason::AlreadyInStaging,
        }
    );

    let re_outcome_1 = service.ingest_media(&gallery_post, 1).await.unwrap();
    assert_eq!(
        re_outcome_1,
        IngestionOutcome::Skipped {
            id: "gallery_post_1".into(),
            reason: SkipReason::Rejected,
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
