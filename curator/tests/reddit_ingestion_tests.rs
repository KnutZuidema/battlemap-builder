use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use async_trait::async_trait;
use axum::routing::get;
use axum::Router;
use chrono::Utc;

use curator::adapters::reddit::BackoffConfig;
use curator::adapters::{
    FilesystemBlobStorage, FilesystemCatalogRepository, InMemoryBlobStorage,
    InMemoryCatalogRepository,
};
use curator::domain::{
    CuratedMap, GridConfig, HarvestedMedia, LayerRole, ParsedTags, RedditHarvestQuery, RedditPost,
    RejectionReason, Source, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CatalogRepository, CandidateFilter};
use curator::ports::harvester::{HarvesterError, RedditHarvester};
use curator::ports::storage_error::StorageError;
use curator::services::curation::{CurationService, CurationWorkflow};
use curator::services::description::{DescriptionError, DescriptionGenerator, DescriptionService};
use curator::services::{
    IngestionOutcome, IngestionProgress, RedditIngestionService, SkipReason,
};

/// Fresh in-memory catalog and blob ports: no filesystem access, no temp dirs.
fn in_memory_ports() -> (Arc<InMemoryCatalogRepository>, Arc<InMemoryBlobStorage>) {
    (
        Arc::new(InMemoryCatalogRepository::new()),
        Arc::new(InMemoryBlobStorage::new()),
    )
}

/// Constant description generator: curation needs *a* physical description, never a live model.
struct ConstantDescriptionGen;

#[async_trait]
impl DescriptionGenerator for ConstantDescriptionGen {
    async fn generate_description(
        &self,
        _image_bytes: &[u8],
        _filename: Option<&str>,
        _tags: &[String],
        _title: Option<&str>,
    ) -> Result<String, DescriptionError> {
        Ok("Constant test description".to_string())
    }
}

/// Production curation workflow over filesystem-backed ports sharing `dir` with the rest of the
/// suite, so curation and rejection move the real staging artifacts of the ingested candidates.
fn curation_service(dir: &tempfile::TempDir) -> CurationService {
    CurationService::new(
        Arc::new(FilesystemCatalogRepository::new(dir.path())),
        Arc::new(FilesystemBlobStorage::new(dir.path())),
        Arc::new(DescriptionService::new(Some(Arc::new(
            ConstantDescriptionGen,
        )))),
    )
}

/// Spawns a one-route mock image server serving `png_bytes` and returns the served URL.
async fn spawn_image_server(path: &str, png_bytes: Vec<u8>) -> String {
    let app = Router::new().route(
        path,
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

    format!("http://{addr}{path}")
}

/// Canonical staging layer address written by the ingestion pipeline.
fn staging_layer(id: &str, index: usize) -> BlobKey {
    BlobKey::StagingLayer {
        id: id.to_string(),
        index,
    }
}

/// [`BlobStorage`] double over an in-memory store whose writes fail from `fail_writes_from` on.
struct FlakyBlobStorage {
    inner: InMemoryBlobStorage,
    fail_writes_from: AtomicUsize,
    write_calls: AtomicUsize,
}

impl FlakyBlobStorage {
    fn failing_from(write_index: usize) -> Self {
        Self {
            inner: InMemoryBlobStorage::new(),
            fail_writes_from: AtomicUsize::new(write_index),
            write_calls: AtomicUsize::new(0),
        }
    }

    /// Lets every later write through, simulating the transient failure going away.
    fn stop_failing(&self) {
        self.fail_writes_from.store(usize::MAX, Ordering::SeqCst);
    }
}

#[async_trait]
impl BlobStorage for FlakyBlobStorage {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        self.inner.read_blob(key).await
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        let call = self.write_calls.fetch_add(1, Ordering::SeqCst);
        if call >= self.fail_writes_from.load(Ordering::SeqCst) {
            return Err(StorageError::InvalidData(
                "simulated blob write failure".into(),
            ));
        }
        self.inner.write_blob(key, bytes).await
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        self.inner.delete_blob(key).await
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        self.inner.exists_blob(key).await
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.inner.blob_size(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        self.inner.list_prefix(prefix).await
    }
}

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

/// Helper to generate a valid PNG image of the given size.
fn create_test_png_sized(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(width, height);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
    bytes
}

/// Helper to generate a minimal valid 1x1 PNG image.
fn create_test_png() -> Vec<u8> {
    create_test_png_sized(1, 1)
}

#[tokio::test]
async fn test_ingestion_deduplication_against_rejections() {
    let (catalog, blobs) = in_memory_ports();

    // Log a rejection for post_rejected
    catalog
        .log_rejection("post_rejected", RejectionReason::GridBurnedIn, Some("Visible grid"))
        .await
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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

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
    let (catalog, blobs) = in_memory_ports();

    // Pre-populate the candidate and its layer blob in staging
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

    catalog.save_candidate(&post.to_bundle_candidate()).await.unwrap();
    blobs
        .write_blob(&staging_layer("staged_map", 0), &create_test_png())
        .await
        .unwrap();

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

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
    let (catalog, blobs) = in_memory_ports();

    // Save a curated map
    let curated = CuratedMap::with_now(
        "curated_map_1",
        Theme::Dungeon,
        vec!["dungeon".into()],
        (30, 30),
    );
    catalog.save_curated(&curated).await.unwrap();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

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
    // 1. Spawn a mock image server serving a 512x512 map
    let png_bytes = create_test_png_sized(512, 512);
    let image_url = spawn_image_server("/image.png", png_bytes.clone()).await;

    // 2. Filesystem-backed ports sharing one root with the production curation store, so step 8
    //    moves the candidate through the real staging-to-curated implementation
    let dir = tempdir().unwrap();
    let catalog = Arc::new(FilesystemCatalogRepository::new(dir.path()));
    let blobs = Arc::new(FilesystemBlobStorage::new(dir.path()));

    let post = RedditPost {
        id: "harvest_1".into(),
        name: "t3_harvest_1".into(),
        title: "[32x32] Crystal Spire [No Grid]".into(),
        author: "spire_architect".into(),
        permalink: "https://www.reddit.com/r/battlemaps/comments/harvest_1/crystal_spire/".into(),
        url: image_url.clone(),
        score: 320,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone()).with_dimensions(512, 512)],
        tags: ParsedTags::parse_title("[32x32] Crystal Spire [No Grid]"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    // 3. Run ingest_posts
    let summary = service.ingest_posts(&[post.clone()]).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    // 4. Verify the layer blob landed at the uniform staging/{id}/{index}.png address
    let image_key = staging_layer("harvest_1", 0);
    assert!(
        blobs.exists_blob(&image_key).await.unwrap(),
        "layer blob must exist"
    );
    let saved_bytes = blobs.read_blob(&image_key).await.unwrap();
    assert_eq!(saved_bytes, png_bytes);

    // 5. Verify the staged candidate metadata carries the Issue #25 sidecar fields
    let candidate = catalog.get_candidate("harvest_1").await.unwrap();
    assert_eq!(candidate.id, "harvest_1");
    assert_eq!(candidate.source, Source::Reddit);
    assert!(candidate.source_url.as_deref().unwrap().contains(&image_url));
    assert_eq!(
        candidate.title.as_deref(),
        Some("[32x32] Crystal Spire [No Grid]")
    );
    assert_eq!(candidate.author.as_deref(), Some("spire_architect"));
    assert_eq!(candidate.parsed_gridless, Some(true));
    assert_eq!(candidate.parsed_dimensions, Some((32, 32)));
    assert_eq!(candidate.dimensions, (512, 512));
    // The first persistence derives the grid from the parsed lattice: the default 64 px grid would
    // leave only 8x8 cells and fail curation's 16x16 gate.
    assert_eq!(candidate.grid, GridConfig::new(16, 0, 0));
    assert!(candidate.created_at <= Utc::now());

    // 6. Verify the staging listing discovers the staged candidate
    let staged_candidates = catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap();
    assert_eq!(staged_candidates.len(), 1);
    let cand = &staged_candidates[0];
    assert_eq!(cand.id, "harvest_1");
    assert_eq!(cand.source, Source::Reddit);
    assert_eq!(cand.dimensions, (512, 512));
    assert_eq!(cand.parsed_gridless, Some(true));
    assert_eq!(cand.author.as_deref(), Some("spire_architect"));

    // 7. Verify repeated harvest skips previously downloaded candidate
    let second_summary = service.ingest_posts(&[post.clone()]).await.unwrap();
    assert_eq!(second_summary.processed, 1);
    assert_eq!(second_summary.downloaded, 0);
    assert_eq!(second_summary.skipped, 1);

    // 8. Curate through the production workflow: the curated map replaces its staging artifacts
    let curated = curation_service(&dir)
        .curate(
            "harvest_1",
            vec!["sci-fi".into(), "crystal".into()],
            None,
            Some("Crystal spire chamber".into()),
        )
        .await
        .unwrap();

    assert_eq!(curated.id, "harvest_1");
    assert_eq!(curated.grid, GridConfig::new(16, 0, 0));
    assert_eq!(curated.dimensions, (512, 512));
    assert!(
        catalog.get_candidate("harvest_1").await.is_err(),
        "candidate moved out of staging"
    );
    assert!(
        !blobs.exists_blob(&image_key).await.unwrap(),
        "layer moved out of staging"
    );
    assert!(catalog.get_curated("harvest_1").await.is_ok());

    // The curated Ground blob holds the bytes this ingestion downloaded, moved (not re-written)
    // by the curation store.
    let curated_key = BlobKey::CuratedLayer {
        id: "harvest_1".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    };
    let curated_bytes = blobs.read_blob(&curated_key).await.unwrap();
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
async fn test_ingestion_auto_seeds_grid_from_parsed_lattice() {
    // A 512x512 map titled `[32x32]` must stage with the 16 px grid its lattice implies: the
    // default 64 px grid leaves only 8x8 cells and fails curation's 16x16 eligibility gate.
    let png_bytes = create_test_png_sized(512, 512);
    let image_url = spawn_image_server("/seeded.png", png_bytes).await;
    let (catalog, blobs) = in_memory_ports();

    let post = RedditPost {
        id: "seeded_map".into(),
        name: "t3_seeded_map".into(),
        title: "[32x32] Sunken Vault".into(),
        author: "vault_keeper".into(),
        permalink: "/r/battlemaps/comments/seeded_map/".into(),
        url: image_url.clone(),
        score: 210,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone()).with_dimensions(512, 512)],
        tags: ParsedTags::parse_title("[32x32] Sunken Vault"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    let summary = service.ingest_post(&post).await.unwrap();
    assert_eq!(summary.downloaded, 1);

    let candidate = catalog.get_candidate("seeded_map").await.unwrap();
    assert_eq!(candidate.dimensions, (512, 512));
    assert_eq!(candidate.grid, GridConfig::new(16, 0, 0));
    assert!(candidate.grid.is_curation_eligible(512, 512));

    // Only the first persistence seeds the grid: an explicitly set grid survives a later save
    let mut regridded = candidate.clone();
    regridded.grid = GridConfig::new(48, 3, 5);
    catalog.save_candidate(&regridded).await.unwrap();
    assert_eq!(
        catalog.get_candidate("seeded_map").await.unwrap().grid,
        GridConfig::new(48, 3, 5)
    );
}

#[tokio::test]
async fn test_ingestion_backfills_dimensions_from_downloaded_bytes() {
    // A post whose payload carries neither media width/height nor parsed grid dimensions still
    // persists the measured image size, otherwise curation's 16x16 gate sees a (0, 0) map.
    let png_bytes = create_test_png_sized(320, 200);
    let image_url = spawn_image_server("/unmeasured.png", png_bytes).await;
    let (catalog, blobs) = in_memory_ports();

    let post = RedditPost {
        id: "unmeasured_map".into(),
        name: "t3_unmeasured_map".into(),
        title: "Ancient Ruins".into(),
        author: "ruin_walker".into(),
        permalink: "/r/battlemaps/comments/unmeasured_map/".into(),
        url: image_url.clone(),
        score: 90,
        created_utc: Utc::now(),
        media: vec![HarvestedMedia::new(image_url.clone())],
        tags: ParsedTags::parse_title("Ancient Ruins"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    let summary = service.ingest_post(&post).await.unwrap();
    assert_eq!(summary.downloaded, 1);

    let candidate = catalog.get_candidate("unmeasured_map").await.unwrap();
    assert_eq!(candidate.layers[0].dimensions, (320, 200));
    assert_eq!(candidate.dimensions, (320, 200));
    // Without a parsed lattice there is nothing to seed from: the default grid stands.
    assert_eq!(candidate.grid, GridConfig::default());
}

#[tokio::test]
async fn test_ingestion_blob_write_failure_leaves_no_candidate() {
    // Layer blobs are written before the manifest: a candidate visible in the catalog without the
    // images it describes would be skipped as already in staging on every later harvest.
    let png_bytes = create_test_png();
    let image_url = spawn_image_server("/flaky.png", png_bytes.clone()).await;

    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(FlakyBlobStorage::failing_from(1));

    let post = RedditPost {
        id: "flaky_map".into(),
        name: "t3_flaky_map".into(),
        title: "[20x20] Flaky Keep [Gridless]".into(),
        author: "keep_builder".into(),
        permalink: "/r/battlemaps/comments/flaky_map/".into(),
        url: image_url.clone(),
        score: 140,
        created_utc: Utc::now(),
        media: vec![
            HarvestedMedia::new(image_url.clone()),
            HarvestedMedia::new(image_url.clone()),
        ],
        tags: ParsedTags::parse_title("[20x20] Flaky Keep [Gridless]"),
    };

    let harvester = MockHarvester::new(vec![post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    let summary = service
        .ingest_posts(std::slice::from_ref(&post))
        .await
        .unwrap();
    assert_eq!(summary.downloaded, 0);
    assert_eq!(summary.skipped, 1);

    // Layer 0 was already written when layer 1 failed: it is rolled back, and no manifest is left
    assert!(
        catalog.get_candidate("flaky_map").await.is_err(),
        "no candidate without the layers it describes"
    );
    assert!(
        !blobs
            .exists_blob(&staging_layer("flaky_map", 0))
            .await
            .unwrap(),
        "partially written blobs are rolled back"
    );
    assert!(!blobs
        .exists_blob(&staging_layer("flaky_map", 1))
        .await
        .unwrap());

    // The post stays re-ingestable instead of being skipped as already in staging
    blobs.stop_failing();
    let retry = service.ingest_post(&post).await.unwrap();
    assert_eq!(retry.downloaded, 1);
    assert!(catalog.get_candidate("flaky_map").await.is_ok());
    assert_eq!(
        blobs
            .read_blob(&staging_layer("flaky_map", 1))
            .await
            .unwrap(),
        png_bytes
    );
}

#[tokio::test]
async fn test_multi_image_gallery_ingestion() {
    let png_bytes = create_test_png();
    let img_url = spawn_image_server("/gallery.png", png_bytes.clone()).await;

    // Filesystem-backed ports sharing one root with the production curation store, so the
    // rejection below purges the staging bundle through the real implementation
    let dir = tempdir().unwrap();
    let catalog = Arc::new(FilesystemCatalogRepository::new(dir.path()));
    let blobs = Arc::new(FilesystemBlobStorage::new(dir.path()));

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    let summary = service.ingest_post(&gallery_post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    // Every gallery layer landed at the uniform staging/{id}/{index}.png address
    for index in 0..2 {
        let key = staging_layer("gallery_post", index);
        assert!(blobs.exists_blob(&key).await.unwrap(), "layer {index} exists");
        assert_eq!(blobs.read_blob(&key).await.unwrap(), png_bytes);
    }
    assert!(!blobs.exists_blob(&staging_layer("gallery_post", 2)).await.unwrap());

    // Verify staged candidate metadata and layers
    let staged = catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap();
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

    // Reject gallery_post through the production curation workflow: the staging bundle and its
    // layer blobs are purged, not merely recorded in the rejection log
    curation_service(&dir)
        .reject(
            "gallery_post",
            RejectionReason::BlurryLowRes,
            Some("Blurry variant"),
        )
        .await
        .unwrap();

    // Every staging artifact is purged, not merely recorded in the rejection log: the manifest
    // leaves the catalog and both gallery layer blobs are deleted.
    assert!(catalog.get_candidate("gallery_post").await.is_err());
    for index in 0..2 {
        assert!(
            !blobs
                .exists_blob(&staging_layer("gallery_post", index))
                .await
                .unwrap(),
            "rejected candidate leaves no staging blob behind"
        );
    }
    assert!(catalog.is_rejected("gallery_post").await.unwrap());

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
    let (catalog, blobs) = in_memory_ports();

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
    let child_candidate = child_sidecar.to_candidate();
    catalog.save_candidate(&child_candidate).await.unwrap();
    blobs
        .write_blob(&staging_layer("gallery_split_0", 0), &create_test_png())
        .await
        .unwrap();
    assert!(catalog.get_candidate("gallery_split_0").await.is_ok());

    let harvester = MockHarvester::new(vec![split_child_post.clone()]);
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

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

    // Case 2: Split child rejected in the rejection log
    catalog
        .log_rejection(
            "gallery_rejected_0",
            RejectionReason::GridBurnedIn,
            Some("Burned grid on layer 0"),
        )
        .await
        .unwrap();
    assert!(catalog.is_rejected("gallery_rejected_0").await.unwrap());

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
    catalog.save_curated(&curated).await.unwrap();
    assert!(catalog.get_curated("gallery_curated_0").await.is_ok());

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

    let (catalog, blobs) = in_memory_ports();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    let query = RedditHarvestQuery::default();
    let summary = service.ingest_query_filtered(&query, true).await.unwrap();

    assert_eq!(summary.processed, 3);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 2);

    // Only gridless candidate should be in staging
    assert!(catalog.get_candidate("gridless_1").await.is_ok());
    assert!(catalog.get_candidate("gridded_1").await.is_err());
    assert!(catalog.get_candidate("unknown_1").await.is_err());
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

    let (catalog, blobs) = in_memory_ports();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

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
    let (catalog, blobs) = in_memory_ports();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone()).with_backoff(backoff);

    assert_eq!(service.backoff().max_retries, 3);

    let summary = service.ingest_post(&post).await.unwrap();
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.skipped, 0);

    // Verify the recovered layer was persisted
    assert!(blobs
        .exists_blob(&staging_layer("recover_post", 0))
        .await
        .unwrap());
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
    let (catalog, blobs) = in_memory_ports();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone()).with_backoff(backoff);

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
    assert!(catalog.get_candidate("post_1_fail").await.is_err());
    assert!(!blobs
        .exists_blob(&staging_layer("post_1_fail", 0))
        .await
        .unwrap());
    assert!(catalog.get_candidate("post_2_ok").await.is_ok());
    assert!(blobs
        .exists_blob(&staging_layer("post_2_ok", 0))
        .await
        .unwrap());
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
    let (catalog, blobs) = in_memory_ports();

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
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone()).with_backoff(backoff);

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
    let (catalog, blobs) = in_memory_ports();
    let harvester = MockHarvester::default();
    let service = RedditIngestionService::new(harvester, catalog.clone(), blobs.clone());

    assert_eq!(service.backoff().initial_delay, Duration::from_secs(3));
    assert_eq!(service.backoff().max_retries, 5);
    assert_eq!(service.backoff().backoff_factor, 2.0);

    let custom = BackoffConfig::new(Duration::from_secs(1), 2, 1.5);
    let service = service.with_backoff(custom.clone());
    assert_eq!(service.backoff(), &custom);
}
