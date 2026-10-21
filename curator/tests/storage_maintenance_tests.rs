//! Integration tests for `StorageMaintenanceService` and `BlobStorage::blob_size`: byte
//! accounting across the staging and curated areas, staging reclamation, and the size contract of
//! both blob adapters.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    for key in ["root", "reddit", "generated"] {
        assert_eq!(
            stats
                .staging_breakdown
                .get(key)
                .expect("legacy breakdown key present"),
            &DirectoryStats::default(),
            "'{key}' stays zero under the uniform staging layout"
        );
    }

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

    assert_eq!(stats.rejected, DirectoryStats::default());
    assert_eq!(stats.telemetry, DirectoryStats::default());
    assert_eq!(stats.total_bytes, 768 + 1024 + 2048 + 128);
    assert_eq!(stats.total_files, 5);
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

    // Curated bundles and the rejection log survive untouched.
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
    let root = TempRoot::new("blob-size");
    let blobs = FilesystemBlobStorage::new(root.path());
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

/// Self-cleaning scratch directory below the system temp root, so the `FilesystemBlobStorage`
/// adapter can be exercised without the `tempfile` crate.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "curator-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create scratch root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
