//! Integration tests for `StorageMaintenanceService` and `BlobStorage::blob_size`: byte
//! accounting across the staging and curated areas, staging reclamation, and the size contract of
//! both blob adapters.

use std::sync::Arc;

use async_trait::async_trait;

use curator::adapters::{FilesystemBlobStorage, InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{
    Candidate, CuratedMap, DirectoryStats, GridConfig, Layer, LayerRole, RejectionReason, Source,
    Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use curator::ports::storage::StorageError;
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

#[tokio::test]
async fn test_storage_stats_sums_blob_sizes_across_staging_and_curated() {
    let (service, catalog, blobs) = in_memory_service();

    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_a".to_string(),
                index: 0,
            },
            &[7u8; 512],
        )
        .await
        .expect("write staging blob");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_b".to_string(),
                index: 0,
            },
            &[9u8; 256],
        )
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
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_a".to_string(),
                index: 0,
            },
            &[7u8; 512],
        )
        .await
        .expect("write staging blob");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_b".to_string(),
                index: 0,
            },
            &[8u8; 128],
        )
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
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_a".to_string(),
                index: 0,
            },
            &[5u8; 400],
        )
        .await
        .expect("write staging blob");
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_a".to_string(),
                index: 1,
            },
            &[6u8; 100],
        )
        .await
        .expect("write staging blob");
    // Staging bytes left behind by a bundle no catalog row describes are reclaimed too.
    blobs
        .write_blob(
            &BlobKey::StagingLayer {
                id: "orphan".to_string(),
                index: 0,
            },
            &[6u8; 24],
        )
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
    let key = BlobKey::StagingLayer {
        id: "cand".to_string(),
        index: 0,
    };

    assert!(matches!(
        blobs.blob_size(&key).await,
        Err(StorageError::NotFound(_))
    ));

    blobs
        .write_blob(&key, &[0u8; 42])
        .await
        .expect("write blob");
    assert_eq!(blobs.blob_size(&key).await.expect("size of present blob"), 42);

    let absent = BlobKey::StagingLayer {
        id: "cand".to_string(),
        index: 1,
    };
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
/// `directory_stats` skips it, so `GET /api/storage/stats` never turns an ordinary rename race
/// into a 404.
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
