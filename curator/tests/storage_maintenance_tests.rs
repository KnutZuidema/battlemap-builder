//! Integration tests for `StorageMaintenanceService` and `BlobStorage`: byte accounting across the
//! staging and curated areas, staging reclamation, the size contract of both blob adapters, and the
//! request-count contract that keeps `GET /api/storage/stats` independent of the blob count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use curator::adapters::{FilesystemBlobStorage, InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{
    Candidate, CuratedMap, DirectoryStats, GridConfig, Layer, LayerRole, RejectionReason, Source,
    Theme,
};
use curator::ports::blob_storage::{BlobEntry, BlobKey, BlobStat, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use curator::ports::storage_error::StorageError;
use curator::services::StorageMaintenanceService;

/// Fresh in-memory catalog and blob ports, plus the service wired to them.
fn in_memory_service() -> (
    StorageMaintenanceService,
    Arc<InMemoryCatalogRepository>,
    Arc<InMemoryBlobStorage>,
) {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());
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

fn staging_layer(id: &str, index: usize) -> BlobKey {
    BlobKey::StagingLayer {
        id: id.to_string(),
        index,
    }
}

/// A curated map tagged `dungeon` + `crypt`, whose bundle holds a Ground and an Upper Floor layer.
fn curated_map(id: &str) -> CuratedMap {
    CuratedMap::with_now(id, Theme::Dungeon, vec!["crypt".into()], (2048, 2048))
        .with_grid(GridConfig::new(64, 0, 0))
        .with_description("Granite necropolis with carved pillars and iron braziers.")
        .with_layers(vec![
            Layer::new(0, "0_ground.png", LayerRole::Ground, Some(0), (2048, 2048)),
            Layer::new(1, "1_upper_1.png", LayerRole::UpperFloor, Some(1), (2048, 2048)),
        ])
}

/// What a `curated/{id}/` listing reports for one bundle: the reference the in-memory theme
/// breakdown is compared against.
async fn per_map_stats(blobs: &InMemoryBlobStorage, id: &str) -> (u64, usize) {
    let entries = blobs
        .list_prefix_with_sizes(&BlobKey::CuratedDir {
            id: Some(id.to_string()),
        })
        .await
        .expect("list one map bundle");
    (
        entries.iter().map(|entry| entry.size).sum::<u64>(),
        entries.len(),
    )
}

#[tokio::test]
async fn test_storage_stats_sums_blob_sizes_across_staging_and_curated() {
    let (service, catalog, blobs) = in_memory_service();

    blobs
        .write_blob(&staging_layer("cand_a", 0), &[7u8; 512])
        .await
        .expect("write staging blob");
    blobs
        .write_blob(&staging_layer("cand_b", 0), &[9u8; 256])
        .await
        .expect("write staging blob");

    let map = curated_map("curated_crypt");
    catalog.save_curated(&map).await.expect("save curated map");
    blobs
        .write_blob(&curated_layer(&map.id, 0, LayerRole::Ground, Some(0)), &[1u8; 1024])
        .await
        .expect("write ground layer");
    blobs
        .write_blob(
            &curated_layer(&map.id, 1, LayerRole::UpperFloor, Some(1)),
            &[2u8; 2048],
        )
        .await
        .expect("write upper floor layer");
    // Orphan bundle bytes are still curated storage even when no catalog row describes them.
    blobs
        .write_blob(
            &curated_layer("orphan_bundle", 0, LayerRole::Roof, None),
            &[3u8; 128],
        )
        .await
        .expect("write orphan layer");

    let stats = service.storage_stats().await.expect("stats succeed");

    assert_eq!(stats.staging.bytes, 768);
    assert_eq!(stats.staging.file_count, 2);

    // Staging is one uniform `staging/{id}/` layout, so the whole footprint sits in `root` while
    // `reddit` and `generated` stay present (the SPA renders all three) at zero.
    assert_eq!(
        stats.staging_breakdown.get("root").expect("root present"),
        &stats.staging
    );
    for key in ["reddit", "generated"] {
        assert_eq!(
            stats
                .staging_breakdown
                .get(key)
                .expect("legacy breakdown key present"),
            &DirectoryStats::default(),
            "'{key}' stays zero under the uniform staging layout"
        );
    }
    assert_eq!(
        stats.staging.bytes,
        stats.staging_breakdown["root"].bytes
            + stats.staging_breakdown["reddit"].bytes
            + stats.staging_breakdown["generated"].bytes
    );

    assert_eq!(stats.curated.bytes, 1024 + 2048 + 128);
    assert_eq!(stats.curated.file_count, 3);

    let dungeon = stats
        .curated_by_theme
        .get(&Theme::Dungeon)
        .expect("dungeon theme present");
    assert_eq!(dungeon.map_count, 1);
    assert_eq!(dungeon.bytes, 1024 + 2048);
    assert_eq!(dungeon.file_count, 2);
    assert_eq!(
        stats.curated_by_theme.get(&Theme::Cave).unwrap().map_count,
        0
    );

    // No rejection log was written and no telemetry blob exists, so both areas report an empty
    // footprint (`NotFound` is an absent log, not an error).
    assert_eq!(stats.rejected, DirectoryStats::default());
    assert_eq!(stats.telemetry, DirectoryStats::default());
    assert_eq!(stats.total_bytes, 768 + 1024 + 2048 + 128);
    assert_eq!(stats.total_files, 5);
}

#[tokio::test]
async fn test_storage_stats_reports_rejection_log_and_telemetry_footprints() {
    let (service, catalog, blobs) = in_memory_service();

    blobs
        .write_blob(&staging_layer("cand_a", 0), &[7u8; 512])
        .await
        .expect("write staging blob");
    blobs
        .write_blob(&staging_layer("cand_b", 0), &[8u8; 128])
        .await
        .expect("write staging blob");
    let rejections = br#"{"id":"rejected_one","reason":"irrelevant"}"#;
    blobs
        .write_blob(&BlobKey::RejectionLog, rejections)
        .await
        .expect("write rejection log");
    catalog
        .log_rejection(
            "rejected_one",
            RejectionReason::Irrelevant,
            Some("already curated"),
        )
        .await
        .expect("log rejection record");
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            },
            &[1u8; 300],
        )
        .await
        .expect("write cost ledger");
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "generation_audit.json".to_string(),
            },
            &[2u8; 40],
        )
        .await
        .expect("write generation audit ledger");

    let stats = service.storage_stats().await.expect("stats succeed");

    assert_eq!(stats.staging, DirectoryStats::new(640, 2));
    assert_eq!(
        stats.staging_breakdown["root"],
        DirectoryStats::new(640, 2)
    );
    assert_eq!(
        stats.staging.bytes,
        stats.staging_breakdown["root"].bytes
            + stats.staging_breakdown["reddit"].bytes
            + stats.staging_breakdown["generated"].bytes
    );
    // `bytes` is the rejection log blob's file size and `file_count` is the number of rejection
    // records the catalog holds (one here), not the single file the log occupies.
    assert_eq!(
        stats.rejected,
        DirectoryStats::new(rejections.len() as u64, 1)
    );
    assert_eq!(stats.telemetry, DirectoryStats::new(340, 2));
    assert_eq!(
        stats.total_bytes,
        640 + rejections.len() as u64 + 340
    );
    assert_eq!(stats.total_files, 2 + 1 + 2);
}

#[tokio::test]
async fn test_purge_staging_removes_staging_and_preserves_curated_and_rejections() {
    let (service, catalog, blobs) = in_memory_service();

    blobs
        .write_blob(&staging_layer("cand_a", 0), &[5u8; 400])
        .await
        .expect("write staging blob");
    blobs
        .write_blob(&staging_layer("cand_a", 1), &[6u8; 100])
        .await
        .expect("write staging blob");
    // Staging bytes left behind by a bundle no catalog row describes are reclaimed too.
    blobs
        .write_blob(&staging_layer("orphan", 0), &[6u8; 24])
        .await
        .expect("write orphan staging blob");

    catalog
        .save_candidate(&Candidate::with_now(
            "cand_a",
            Source::Reddit,
            "a.png",
            (1024, 1024),
        ))
        .await
        .expect("save candidate");
    catalog
        .save_candidate(&Candidate::with_now(
            "cand_b",
            Source::Manual,
            "b.png",
            (1024, 1024),
        ))
        .await
        .expect("save candidate");

    let map = curated_map("curated_crypt");
    catalog.save_curated(&map).await.expect("save curated map");
    let ground = [3u8; 900];
    blobs
        .write_blob(&curated_layer(&map.id, 0, LayerRole::Ground, Some(0)), &ground)
        .await
        .expect("write ground layer");
    catalog
        .log_rejection("rejected_one", RejectionReason::Irrelevant, Some("already curated"))
        .await
        .expect("log rejection");
    let rejections = br#"{"id":"rejected_one"}"#;
    blobs
        .write_blob(&BlobKey::RejectionLog, rejections)
        .await
        .expect("write rejection log");
    let ledger = [4u8; 77];
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            },
            &ledger,
        )
        .await
        .expect("write cost ledger");

    let result = service.purge_staging().await.expect("purge succeeds");

    assert_eq!(result.purged_files, 3);
    assert_eq!(result.reclaimed_bytes, 400 + 100 + 24);
    assert_eq!(result.remaining_staging_files, 0);

    assert!(blobs
        .list_prefix(&BlobKey::StagingDir { id: None })
        .await
        .expect("list staging")
        .is_empty());
    assert!(catalog
        .list_candidates(&CandidateFilter::default())
        .await
        .expect("list candidates")
        .is_empty());

    // Curated bundles, the rejection log and the telemetry ledger survive untouched: only the
    // `staging/` prefix is enumerated.
    assert_eq!(
        blobs
            .list_prefix(&BlobKey::CuratedDir { id: None })
            .await
            .expect("list curated")
            .len(),
        1
    );
    assert_eq!(
        blobs
            .read_blob(&BlobKey::RejectionLog)
            .await
            .expect("read rejection log"),
        rejections
    );
    assert_eq!(
        blobs
            .read_blob(&BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            })
            .await
            .expect("read cost ledger"),
        ledger
    );
    let stats = service.storage_stats().await.expect("stats after purge");
    assert_eq!(stats.staging, DirectoryStats::default());
    assert_eq!(
        stats.rejected,
        DirectoryStats::new(rejections.len() as u64, 1)
    );
    assert_eq!(stats.telemetry, DirectoryStats::new(ledger.len() as u64, 1));
    assert_eq!(
        blobs
            .read_blob(&curated_layer(&map.id, 0, LayerRole::Ground, Some(0)))
            .await
            .expect("read curated ground"),
        ground
    );
    assert!(catalog.get_curated(&map.id).await.is_ok());
    assert!(catalog
        .is_rejected("rejected_one")
        .await
        .expect("rejection check"));
}

#[tokio::test]
async fn test_in_memory_blob_size_reports_length_and_not_found() {
    let blobs = InMemoryBlobStorage::new();
    let key = staging_layer("cand", 0);

    assert!(matches!(
        blobs.blob_size(&key).await,
        Err(StorageError::NotFound(_))
    ));

    blobs
        .write_blob(&key, &[0u8; 42])
        .await
        .expect("write blob");
    assert_eq!(blobs.blob_size(&key).await.expect("size of present blob"), 42);

    let absent = staging_layer("cand", 1);
    assert!(matches!(
        blobs.blob_size(&absent).await,
        Err(StorageError::NotFound(_))
    ));
}

#[tokio::test]
async fn test_filesystem_blob_size_reports_length_and_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let blobs = FilesystemBlobStorage::new(dir.path());
    let key = BlobKey::ExportArchive {
        name: "lora-dataset".to_string(),
    };

    assert!(matches!(
        blobs.blob_size(&key).await,
        Err(StorageError::NotFound(_))
    ));

    blobs
        .write_blob(&key, &[0u8; 4096])
        .await
        .expect("write blob");
    assert_eq!(
        blobs.blob_size(&key).await.expect("size of present blob"),
        4096
    );

    let absent = BlobKey::ExportArchive {
        name: "absent".to_string(),
    };
    assert!(matches!(
        blobs.blob_size(&absent).await,
        Err(StorageError::NotFound(_))
    ));
}

/// Both real adapters report the same sizes through `list_prefix_with_sizes` that they report
/// through `blob_size`, so accounting can rely on the listing for either backend.
#[tokio::test]
async fn test_both_adapters_report_listing_sizes_matching_blob_size() {
    let dir = tempfile::tempdir().expect("tempdir");
    let filesystem = FilesystemBlobStorage::new(dir.path());
    let in_memory = InMemoryBlobStorage::new();

    let keys = [
        (staging_layer("cand", 0), 300usize),
        (staging_layer("cand", 1), 120),
        (curated_layer("m1", 0, LayerRole::Ground, Some(0)), 2048),
    ];

    for (key, size) in &keys {
        let payload = vec![0u8; *size];
        filesystem.write_blob(key, &payload).await.expect("fs write");
        in_memory.write_blob(key, &payload).await.expect("mem write");
    }

    for prefix in [
        BlobKey::StagingDir { id: None },
        BlobKey::CuratedDir { id: None },
    ] {
        let mut expected: Vec<u64> = Vec::new();
        for key in filesystem.list_prefix(&prefix).await.expect("fs list") {
            expected.push(filesystem.blob_size(&key).await.expect("fs size"));
        }

        let fs_entries = filesystem
            .list_prefix_with_sizes(&prefix)
            .await
            .expect("fs sized list");
        let mem_entries = in_memory
            .list_prefix_with_sizes(&prefix)
            .await
            .expect("mem sized list");

        let fs_sizes: Vec<u64> = fs_entries.iter().map(|entry| entry.size).collect();
        let mem_sizes: Vec<u64> = mem_entries.iter().map(|entry| entry.size).collect();
        assert_eq!(
            fs_sizes, expected,
            "the filesystem listing sizes must match its own blob_size reads"
        );
        assert_eq!(
            mem_sizes, expected,
            "the in-memory listing sizes must match the filesystem's"
        );
    }
}

/// [`BlobStorage`] double forwarding to an in-memory store, except that one key's size read fails
/// with [`StorageError::NotFound`] — as if the writer had renamed the blob away between the
/// listing and the size read.
struct VanishingBlobStorage {
    inner: Arc<InMemoryBlobStorage>,
    vanishing: BlobKey,
}

#[async_trait]
impl BlobStorage for VanishingBlobStorage {
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
        if *key == self.vanishing {
            return Err(StorageError::NotFound(format!(
                "Blob '{}' not found",
                key.to_path()
            )));
        }
        self.inner.blob_size(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        self.inner.list_prefix(prefix).await
    }
}

/// The hidden atomic-write temp files the cost ledger and the blob adapter leave inside
/// `telemetry/` are not ledgers: they must never inflate `telemetry.bytes`/`file_count`.
#[tokio::test]
async fn test_storage_stats_ignores_hidden_telemetry_temp_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs: Arc<dyn BlobStorage> = Arc::new(FilesystemBlobStorage::new(dir.path()));

    let cost_ledger = BlobKey::TelemetryLog {
        name: "cost_ledger.json".to_string(),
    };
    blobs
        .write_blob(&cost_ledger, &[1u8; 300])
        .await
        .expect("write cost ledger");
    // Exactly what `FileCostLedger::atomic_save` leaves in `telemetry/` while it replaces the
    // ledger: a hidden temp file next to it.
    std::fs::write(
        dir.path().join("telemetry/.cost_ledger_4321_1.tmp"),
        [2u8; 40],
    )
    .expect("write in-flight temp file");

    assert_eq!(
        std::fs::read_dir(dir.path().join("telemetry"))
            .expect("telemetry dir")
            .count(),
        2,
        "the temp file really is on disk next to the ledger"
    );
    assert_eq!(
        blobs
            .list_prefix(&BlobKey::TelemetryDir)
            .await
            .expect("list telemetry"),
        vec![cost_ledger]
    );

    let service = StorageMaintenanceService::new(catalog, blobs);
    let stats = service.storage_stats().await.expect("stats succeed");

    // Only the committed ledger is counted, and its temp file's bytes are nowhere in the totals.
    assert_eq!(stats.telemetry, DirectoryStats::new(300, 1));
    assert_eq!(stats.total_bytes, 300);
    assert_eq!(stats.total_files, 1);
}

/// A key that vanishes between the listing and its size read must not fail the whole aggregation:
/// the sized listing skips it, so `GET /api/storage/stats` never turns an ordinary rename race
/// into a 404.
///
/// The double below forwards the sizing listing to the port's default (it implements only
/// `list_prefix`), which is the path a key-only adapter takes.
#[tokio::test]
async fn test_storage_stats_skips_a_key_that_vanishes_mid_scan() {
    let inner = Arc::new(InMemoryBlobStorage::new());
    let cost_ledger = BlobKey::TelemetryLog {
        name: "cost_ledger.json".to_string(),
    };
    let vanished = BlobKey::TelemetryLog {
        name: "generation_audit.json".to_string(),
    };
    inner
        .write_blob(&cost_ledger, &[1u8; 300])
        .await
        .expect("write cost ledger");
    inner
        .write_blob(&vanished, &[2u8; 40])
        .await
        .expect("write audit ledger");

    let blobs: Arc<dyn BlobStorage> = Arc::new(VanishingBlobStorage {
        inner: inner.clone(),
        vanishing: vanished.clone(),
    });
    // Both ledgers are listed; only the vanished one's size read fails.
    assert_eq!(
        blobs
            .list_prefix(&BlobKey::TelemetryDir)
            .await
            .expect("list telemetry")
            .len(),
        2
    );
    let service =
        StorageMaintenanceService::new(Arc::new(InMemoryCatalogRepository::new()), blobs);

    let stats = service
        .storage_stats()
        .await
        .expect("a vanished key must not fail the stats");

    // The skipped key contributes neither bytes nor a file count.
    assert_eq!(stats.telemetry, DirectoryStats::new(300, 1));
    assert_eq!(stats.total_bytes, 300);
    assert_eq!(stats.total_files, 1);
}

/// `rejected.bytes` is the rejection log blob's file size while `rejected.file_count` is the
/// number of rejection records the catalog holds — the pre-cut-over operator semantics, which the
/// SPA renders as `N records`.
#[tokio::test]
async fn test_storage_stats_rejected_counts_records_not_files() {
    let (service, catalog, blobs) = in_memory_service();

    for id in ["rej_1", "rej_2", "rej_3"] {
        catalog
            .log_rejection(id, RejectionReason::BlurryLowRes, None)
            .await
            .expect("log rejection");
    }
    assert_eq!(
        catalog
            .rejection_records()
            .await
            .expect("read rejection log")
            .len(),
        3
    );
    let log = br#"[{"id":"rej_1"},{"id":"rej_2"},{"id":"rej_3"}]"#;
    blobs
        .write_blob(&BlobKey::RejectionLog, log)
        .await
        .expect("write rejection log");

    let stats = service.storage_stats().await.expect("stats succeed");

    // Three records in one file: three reads as `records`, the file size as `bytes`.
    assert_eq!(stats.rejected, DirectoryStats::new(log.len() as u64, 3));
    assert_eq!(stats.total_bytes, log.len() as u64);
    assert_eq!(stats.total_files, 3);

    // With no log written at all, both numbers are zero rather than an error.
    let (empty_service, _catalog, _blobs) = in_memory_service();
    let empty = empty_service.storage_stats().await.expect("stats succeed");
    assert_eq!(empty.rejected, DirectoryStats::default());
    assert_eq!(empty.total_bytes, 0);
    assert_eq!(empty.total_files, 0);
}

/// [`BlobStorage`] double forwarding to an in-memory store while counting the calls that made
/// `GET /api/storage/stats` expensive: the sized listings and the per-blob size reads.
///
/// A per-blob size read is one signed `head_object` over the WAN on S3 (#292), so the number of
/// those calls is the number that must not grow with the blob count.
struct CountingBlobStorage {
    inner: Arc<InMemoryBlobStorage>,
    sized_listings: AtomicUsize,
    listings: AtomicUsize,
    size_reads: AtomicUsize,
}

impl CountingBlobStorage {
    fn new() -> Self {
        Self {
            inner: Arc::new(InMemoryBlobStorage::new()),
            sized_listings: AtomicUsize::new(0),
            listings: AtomicUsize::new(0),
            size_reads: AtomicUsize::new(0),
        }
    }

    fn sized_listings(&self) -> usize {
        self.sized_listings.load(Ordering::SeqCst)
    }

    fn listings(&self) -> usize {
        self.listings.load(Ordering::SeqCst)
    }

    fn size_reads(&self) -> usize {
        self.size_reads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl BlobStorage for CountingBlobStorage {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        self.inner.read_blob(key).await
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        self.inner.write_blob(key, bytes).await
    }

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        self.inner.write_blob_with_sha256(key, bytes, sha256).await
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        self.inner.delete_blob(key).await
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        self.inner.exists_blob(key).await
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.size_reads.fetch_add(1, Ordering::SeqCst);
        self.inner.blob_size(key).await
    }

    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        self.size_reads.fetch_add(1, Ordering::SeqCst);
        self.inner.stat_blob(key).await
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        self.listings.fetch_add(1, Ordering::SeqCst);
        self.inner.list_prefix(prefix).await
    }

    async fn list_prefix_with_sizes(
        &self,
        prefix: &BlobKey,
    ) -> Result<Vec<BlobEntry>, StorageError> {
        self.sized_listings.fetch_add(1, Ordering::SeqCst);
        self.inner.list_prefix_with_sizes(prefix).await
    }
}

/// The request-count contract from issue #292: `storage_stats` costs one sized listing per prefix
/// (staging, curated, telemetry) plus the rejection log's single size read — three listings and one
/// size read — no matter how many blobs they cover. On S3 that is 4 calls instead of the 931
/// sequential `head_object`s that took ~94s and starved the liveness probe.
#[tokio::test]
async fn test_storage_stats_costs_three_listings_and_one_size_read_for_any_blob_count() {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(CountingBlobStorage::new());

    // 40 staging layers, 6 curated bundles with 4 layers each (24 blobs), 2 telemetry ledgers.
    for index in 0..40usize {
        let candidate_id = format!("cand_{}", index % 8);
        blobs
            .write_blob(&staging_layer(&candidate_id, index), &[1u8; 64])
            .await
            .expect("write staging blob");
    }
    for map_index in 0..6usize {
        let map_id = format!("map_{map_index}");
        let map = CuratedMap::with_now(
            map_id.as_str(),
            Theme::Dungeon,
            vec!["dungeon".into()],
            (2048, 2048),
        );
        catalog.save_curated(&map).await.expect("save curated map");
        for layer_index in 0..4usize {
            blobs
                .write_blob(
                    &curated_layer(map_id.as_str(), layer_index, LayerRole::Ground, Some(0)),
                    &[2u8; 128],
                )
                .await
                .expect("write curated layer");
        }
    }
    for name in ["cost_ledger.json", "generation_audit.json"] {
        blobs
            .write_blob(&BlobKey::TelemetryLog { name: name.to_string() }, &[3u8; 16])
            .await
            .expect("write telemetry ledger");
    }
    blobs
        .write_blob(&BlobKey::RejectionLog, b"[]")
        .await
        .expect("write rejection log");

    let service = StorageMaintenanceService::new(catalog, blobs.clone());
    let stats = service.storage_stats().await.expect("stats succeed");

    // Every blob is accounted for...
    assert_eq!(stats.staging.file_count, 40);
    assert_eq!(stats.curated.file_count, 24);
    assert_eq!(stats.telemetry.file_count, 2);
    assert_eq!(stats.total_bytes, 40 * 64 + 24 * 128 + 2 * 16 + 2);

    // ...from three sized listings and exactly one single-object size read (the rejection log).
    assert_eq!(
        blobs.sized_listings(),
        3,
        "staging, curated and telemetry each cost one sized listing"
    );
    assert_eq!(
        blobs.size_reads(),
        1,
        "only the rejection log is a single-object size read; nothing scales with the blob count"
    );
    assert_eq!(
        blobs.listings(),
        0,
        "no key-only listing may be used for accounting"
    );
}

/// The per-theme breakdown is computed in memory from the one `curated/` listing, so it must still
/// report exactly what a `curated/{id}/` listing per map would have reported: each theme's bytes
/// and file count are the sum of its maps' bundle footprints, orphans stay out of every theme, and
/// a map tagged with the same theme twice is counted once.
#[tokio::test]
async fn test_curated_theme_breakdown_equals_a_per_map_listing_without_querying_each_map() {
    let catalog = Arc::new(InMemoryCatalogRepository::new());
    let blobs = Arc::new(InMemoryBlobStorage::new());

    // A two-theme map with a repeated tag, a single-theme map, and an orphan bundle.
    let multi = CuratedMap::with_now(
        "multi_theme",
        Theme::Dungeon,
        vec!["dungeon".into(), "cave".into(), "dungeon".into()],
        (2048, 2048),
    );
    let single = CuratedMap::with_now(
        "single_theme",
        Theme::Tavern,
        vec!["tavern".into()],
        (1024, 1024),
    );
    for map in [&multi, &single] {
        catalog.save_curated(map).await.expect("save curated map");
    }

    for (index, size) in [(0usize, 1000usize), (1, 500)] {
        let payload = vec![1u8; size];
        blobs
            .write_blob(
                &curated_layer(&multi.id, index, LayerRole::Ground, Some(0)),
                &payload,
            )
            .await
            .expect("write multi-theme layer");
    }
    blobs
        .write_blob(
            &curated_layer(&single.id, 0, LayerRole::Ground, Some(0)),
            &[2u8; 700],
        )
        .await
        .expect("write single-theme layer");
    blobs
        .write_blob(
            &curated_layer("orphan_bundle", 0, LayerRole::Roof, None),
            &[3u8; 333],
        )
        .await
        .expect("write orphan layer");

    let service = StorageMaintenanceService::new(catalog, blobs.clone());
    let stats = service.storage_stats().await.expect("stats succeed");

    // The control: what the service used to compute for each map, by listing its own bundle.
    assert_eq!(per_map_stats(&blobs, "multi_theme").await, (1500, 2));
    assert_eq!(per_map_stats(&blobs, "single_theme").await, (700, 1));

    let dungeon = stats.curated_by_theme.get(&Theme::Dungeon).expect("dungeon");
    assert_eq!(
        (dungeon.bytes, dungeon.file_count, dungeon.map_count),
        (1500, 2, 1),
        "a map tagged with the same theme twice is counted once"
    );

    // Tagged `cave` as well, so the same bundle contributes to both themes.
    let cave = stats.curated_by_theme.get(&Theme::Cave).expect("cave");
    assert_eq!((cave.bytes, cave.file_count, cave.map_count), (1500, 2, 1));

    let tavern = stats.curated_by_theme.get(&Theme::Tavern).expect("tavern");
    assert_eq!((tavern.bytes, tavern.file_count, tavern.map_count), (700, 1, 1));

    // The orphan bundle inflates no theme even though it is part of the overall curated footprint.
    assert_eq!(stats.curated.bytes, 1500 + 700 + 333);
    assert_eq!(stats.curated.file_count, 4);
    let themed_bytes: u64 = stats
        .curated_by_theme
        .values()
        .map(|entry| entry.bytes)
        .sum();
    assert_eq!(
        themed_bytes,
        1500 + 1500 + 700,
        "each multi-theme bundle is attributed to each of its themes"
    );

    // Every canonical theme is present, including the ones with nothing curated.
    assert_eq!(stats.curated_by_theme.len(), Theme::ALL.len());
    for theme in Theme::ALL {
        assert!(stats.curated_by_theme.contains_key(theme));
    }
}
