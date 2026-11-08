//! Red-phase tests for #292, service behaviour: storage accounting must not scale with the
//! number of curated maps.
//!
//! `StorageMaintenanceService::storage_stats` derived the per-theme curated breakdown by issuing
//! one `list_prefix(curated/{id}/)` per curated map (plus a `head_object` per layer), on top of
//! the whole-tree `curated/` listing it had already read. Against Cloudflare R2 that was ~100
//! extra listings and 226 redundant HEADs for one drawer open.
//!
//! The replacement joins the single `curated/` listing against `catalog.list_curated()` in
//! memory. These tests record every listing prefix the service asks the blob port for, so a
//! regression to per-map queries fails here regardless of how the adapter is implemented.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use curator::adapters::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{
    CuratedMap, DirectoryStats, GridConfig, Layer, LayerRole, RejectionReason, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::storage_error::StorageError;
use curator::services::StorageMaintenanceService;

/// A blob port double that forwards to the in-memory store while recording what was asked.
///
/// It implements only the port as it exists today, so it stays a faithful stand-in for a
/// key-only adapter: the sized listing keeps its trait default (list, then size each key).
/// That is exactly what makes this test port-shape independent — the assertion is about which
/// *prefixes* the service enumerates, not about which method it calls.
#[derive(Default)]
struct RecordingBlobs {
    inner: InMemoryBlobStorage,
    listed: Mutex<Vec<String>>,
    sized: Mutex<Vec<String>>,
}

impl RecordingBlobs {
    fn new() -> Self {
        Self::default()
    }

    /// Every `list_prefix` prefix the service asked for, in call order.
    fn listed_prefixes(&self) -> Vec<String> {
        self.listed.lock().expect("listing log lock").clone()
    }

    /// Every single-blob size query the service asked for, in call order.
    fn sized_keys(&self) -> Vec<String> {
        self.sized.lock().expect("size log lock").clone()
    }
}

#[async_trait]
impl BlobStorage for RecordingBlobs {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        self.inner.read_blob(key).await
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        self.inner.write_blob(key, bytes).await
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        self.inner.delete_blob(key).await
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        self.inner.exists_blob(key).await
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.sized
            .lock()
            .expect("size log lock")
            .push(key.to_path());
        self.inner.blob_size(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        self.listed
            .lock()
            .expect("listing log lock")
            .push(prefix.to_path());
        self.inner.list_prefix(prefix).await
    }
}

fn storage_service() -> (
    StorageMaintenanceService,
    Arc<InMemoryCatalogRepository>,
    Arc<RecordingBlobs>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(RecordingBlobs::new());
    (
        StorageMaintenanceService::new(catalog.clone(), blobs.clone()),
        catalog,
        blobs,
    )
}

fn curated_layer(id: &str, index: usize, role: LayerRole, elevation: Option<i8>) -> BlobKey {
    BlobKey::CuratedLayer {
        id: id.to_string(),
        index,
        role,
        elevation,
    }
}

/// Two staging layers, two curated maps (one tagged with two themes) and one orphan bundle no
/// catalog row describes, plus one telemetry ledger and a rejection log.
///
/// `(staging_bytes, curated_bytes, telemetry_bytes, rejected_bytes)` = (768, 3712, 340, 2).
async fn seed(
    catalog: &Arc<InMemoryCatalogRepository>,
    blobs: &Arc<RecordingBlobs>,
) -> Vec<CuratedMap> {
    for (id, size) in [("cand_a", 512usize), ("cand_b", 256)] {
        blobs
            .write_blob(
                &BlobKey::StagingLayer {
                    id: id.to_string(),
                    index: 0,
                },
                &vec![7u8; size],
            )
            .await
            .expect("write staging layer");
    }

    // A single-theme bundle with two layers: 3072 bytes over 2 files.
    let dungeon_map = CuratedMap::with_now(
        "map_dungeon",
        Theme::Dungeon,
        vec!["crypt".to_string()],
        (2048, 2048),
    )
    .with_grid(GridConfig::new(64, 0, 0))
    .with_layers(vec![
        Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (2048, 2048)),
        Layer::new(1, "1_upper_1.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
    ]);
    catalog
        .save_curated(&dungeon_map)
        .await
        .expect("save dungeon map");
    blobs
        .write_blob(
            &curated_layer(&dungeon_map.id, 0, LayerRole::Ground, Some(0)),
            &[1u8; 1024],
        )
        .await
        .expect("write dungeon ground");
    blobs
        .write_blob(
            &curated_layer(&dungeon_map.id, 1, LayerRole::UpperFloor, Some(1)),
            &[2u8; 2048],
        )
        .await
        .expect("write dungeon upper floor");

    // A bundle tagged with two canonical themes: 512 bytes over 1 file, counted for both.
    let multitheme_map = CuratedMap::with_now(
        "map_multitheme",
        Theme::Cave,
        vec!["dungeon".to_string()],
        (2048, 2048),
    );
    catalog
        .save_curated(&multitheme_map)
        .await
        .expect("save multitheme map");
    blobs
        .write_blob(
            &curated_layer(&multitheme_map.id, 0, LayerRole::Ground, Some(0)),
            &[3u8; 512],
        )
        .await
        .expect("write multitheme ground");

    // Orphaned bundle bytes: curated storage with no catalog row, so no theme owns them.
    blobs
        .write_blob(
            &curated_layer("orphan_bundle", 0, LayerRole::Roof, None),
            &[4u8; 128],
        )
        .await
        .expect("write orphan layer");

    for (name, size) in [("cost_ledger.json", 300usize), ("generation_audit.json", 40)] {
        blobs
            .write_blob(
                &BlobKey::TelemetryLog {
                    name: name.to_string(),
                },
                &vec![1u8; size],
            )
            .await
            .expect("write telemetry ledger");
    }
    blobs
        .write_blob(&BlobKey::RejectionLog, b"[]")
        .await
        .expect("write rejection log");

    vec![dungeon_map, multitheme_map]
}

/// The whole per-theme breakdown must come from the one `curated/` listing the footprint already
/// reads — never from a listing per catalog map.
#[tokio::test]
async fn test_storage_stats_costs_one_curated_listing_and_no_per_map_listings() {
    let (service, catalog, blobs) = storage_service();
    let maps = seed(&catalog, &blobs).await;

    let stats = service.storage_stats().await.expect("stats succeed");

    let listed = blobs.listed_prefixes();

    // The acceptance criterion: `staging/`, `curated/` and `telemetry/` — three listings, of
    // which the curated one is shared by the per-theme breakdown.
    let curated_listings: Vec<&String> = listed
        .iter()
        .filter(|prefix| prefix.starts_with("curated/"))
        .collect();
    assert_eq!(
        curated_listings,
        vec!["curated/"],
        "the curated footprint must cost exactly one whole-tree listing, got {listed:?}"
    );
    for map in &maps {
        assert!(
            !listed.contains(&format!("curated/{}/", map.id)),
            "no per-map listing may be issued for '{}', got {listed:?}",
            map.id
        );
    }
    for prefix in ["staging/", "telemetry/"] {
        assert_eq!(
            listed.iter().filter(|listed| listed.as_str() == prefix).count(),
            1,
            "'{prefix}' is enumerated once, got {listed:?}"
        );
    }

    // The rejection log is one known key, so it costs one size query and nothing else about it
    // is enumerated.
    let sized = blobs.sized_keys();
    assert_eq!(
        sized
            .iter()
            .filter(|key| key.as_str() == "rejected/rejections.json")
            .count(),
        1,
        "the rejection log is sized once, got {sized:?}"
    );

    // The single listing still accounts for every curated byte, orphan bundles included.
    assert_eq!(stats.curated, DirectoryStats::new(1024 + 2048 + 512 + 128, 4));
    assert_eq!(stats.staging, DirectoryStats::new(768, 2));
    assert_eq!(stats.telemetry, DirectoryStats::new(340, 2));
    assert_eq!(stats.rejected, DirectoryStats::new(2, 0));
    assert_eq!(stats.total_bytes, 768 + 3712 + 2 + 340);
    assert_eq!(stats.total_files, 2 + 4 + 0 + 2);
}

/// The in-memory join must attribute bytes exactly as a per-map `curated/{id}/` listing did:
/// a map tagged with two themes counts for both, and bytes no catalog row describes stay in the
/// overall curated total without inflating a theme.
#[tokio::test]
async fn test_curated_by_theme_is_derived_from_the_single_listing_joined_with_the_catalog() {
    let (service, catalog, blobs) = storage_service();
    seed(&catalog, &blobs).await;

    let stats = service.storage_stats().await.expect("stats succeed");

    let dungeon = stats
        .curated_by_theme
        .get(&Theme::Dungeon)
        .expect("dungeon theme present");
    assert_eq!(
        *dungeon,
        curator::domain::ThemeStorageStats::new(Theme::Dungeon, 1024 + 2048 + 512, 3, 2),
        "the dungeon theme sums both of its maps' layers"
    );

    let cave = stats
        .curated_by_theme
        .get(&Theme::Cave)
        .expect("cave theme present");
    assert_eq!(
        *cave,
        curator::domain::ThemeStorageStats::new(Theme::Cave, 512, 1, 1),
        "a bundle tagged with two themes counts for each of them"
    );

    // Every theme stays present at zero, because the drawer renders the whole set.
    let tavern = stats
        .curated_by_theme
        .get(&Theme::Tavern)
        .expect("tavern theme present");
    assert_eq!(
        *tavern,
        curator::domain::ThemeStorageStats::new(Theme::Tavern, 0, 0, 0)
    );

    // Orphaned bundle bytes are curated storage but belong to no theme.
    let themed_bytes: u64 = stats
        .curated_by_theme
        .values()
        .map(|theme_stats| theme_stats.bytes)
        .sum();
    assert_eq!(
        themed_bytes + 128,
        stats.curated.bytes,
        "only the orphan bundle's 128 bytes fall outside the per-theme breakdown"
    );
}

/// Purging staging takes reclaimed bytes from the same sized enumeration instead of a size query
/// per blob, and still reports the same totals.
#[tokio::test]
async fn test_purge_staging_reclaims_the_listed_sizes_and_keeps_curated_data() {
    let (service, catalog, blobs) = storage_service();
    let maps = seed(&catalog, &blobs).await;

    let stats_before = service.storage_stats().await.expect("stats before purge");
    let purged = service.purge_staging().await.expect("purge succeeds");

    assert_eq!(purged.purged_files, 2);
    assert_eq!(purged.reclaimed_bytes, stats_before.staging.bytes);
    assert_eq!(purged.remaining_staging_files, 0);

    let stats_after = service.storage_stats().await.expect("stats after purge");
    assert_eq!(stats_after.staging, DirectoryStats::default());
    assert_eq!(stats_after.curated, stats_before.curated);
    assert_eq!(stats_after.telemetry, stats_before.telemetry);
    assert_eq!(stats_after.rejected, stats_before.rejected);

    // Curated bundles and the rejection/telemetry records survive untouched.
    for map in &maps {
        assert_eq!(
            blobs
                .read_blob(&curated_layer(&map.id, 0, LayerRole::Ground, Some(0)))
                .await
                .expect("curated ground layer survives the purge")
                .len(),
            if map.id == "map_multitheme" { 512 } else { 1024 }
        );
    }
    assert_eq!(
        catalog
            .rejection_records()
            .await
            .expect("read rejection log")
            .len(),
        0
    );
    assert!(matches!(
        catalog
            .list_curated(&Default::default())
            .await
            .expect("list curated"),
        ref maps if maps.len() == 2
    ));
    let _ = RejectionReason::Unspecified;
}
