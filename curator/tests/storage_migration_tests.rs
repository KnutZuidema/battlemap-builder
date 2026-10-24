//! Integration tests for the disk-to-S3 storage migration service.

use std::fs;
use std::sync::Arc;

use curator::adapters::InMemoryBlobStorage;
use curator::domain::LayerRole;
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::services::storage_migration::{
    discover_disk_blobs, is_migration_completed, run_storage_migration, MigrationOptions,
    MIGRATION_MARKER_FILE,
};
use tempfile::TempDir;

/// Sets up a mock `/data` filesystem structure with a realistic mix of valid blobs,
/// non-blob metadata manifests, legacy pre-rendered LoRA files, and hidden directories.
fn setup_mock_data_dir() -> TempDir {
    let temp_dir = TempDir::new().expect("create temp dir");
    let root = temp_dir.path();

    // 1. Staging candidate with 2 layer images and a manifest
    let staging_cand = root.join("staging/cand_1");
    fs::create_dir_all(&staging_cand).expect("create staging dir");
    fs::write(staging_cand.join("0.png"), b"staging-layer-0-png-bytes").unwrap();
    fs::write(staging_cand.join("1.png"), b"staging-layer-1-png-bytes").unwrap();
    fs::write(staging_cand.join("manifest.json"), b"{\"id\":\"cand_1\"}").unwrap();

    // 2. Curated battlemap with Ground layer, Upper Floor layer, manifest, and legacy LoRA crop
    let curated_map = root.join("curated/map_alpha");
    fs::create_dir_all(&curated_map).expect("create curated dir");
    fs::write(curated_map.join("0_ground.png"), b"ground-layer-bytes").unwrap();
    fs::write(curated_map.join("1_upper_1.png"), b"upper-floor-bytes").unwrap();
    fs::write(curated_map.join("manifest.json"), b"{\"id\":\"map_alpha\"}").unwrap();
    fs::write(curated_map.join("map_alpha_lora1024.png"), b"legacy-crop").unwrap();
    fs::write(curated_map.join("map_alpha_lora1024.txt"), b"legacy-caption").unwrap();

    // 3. Rejections log
    let rejected_dir = root.join("rejected");
    fs::create_dir_all(&rejected_dir).expect("create rejected dir");
    fs::write(rejected_dir.join("rejections.json"), b"[{\"id\":\"r1\"}]").unwrap();

    // 4. Telemetry logs
    let telemetry_dir = root.join("telemetry");
    fs::create_dir_all(&telemetry_dir).expect("create telemetry dir");
    fs::write(telemetry_dir.join("cost_ledger.json"), b"{\"total\":0.0}").unwrap();

    // 5. Training artifacts (dataset snapshot)
    let datasets_dir = root.join("artifacts/datasets");
    fs::create_dir_all(&datasets_dir).expect("create datasets dir");
    fs::write(datasets_dir.join("snap-01.zip"), b"PK-zip-bytes").unwrap();

    // 6. Hidden files and directories that MUST be ignored
    let hidden_dir = root.join(".hidden");
    fs::create_dir_all(&hidden_dir).expect("create hidden dir");
    fs::write(hidden_dir.join("secret.png"), b"hidden-bytes").unwrap();
    fs::write(root.join(".DS_Store"), b"junk").unwrap();

    temp_dir
}

#[tokio::test]
async fn test_discover_disk_blobs_identifies_all_valid_blob_keys() {
    let temp = setup_mock_data_dir();
    let discovered = discover_disk_blobs(temp.path()).expect("discover disk blobs");

    let keys: Vec<String> = discovered.iter().map(|(k, _)| k.to_path()).collect();

    // Exact expected canonical blob paths
    let expected = vec![
        "artifacts/datasets/snap-01.zip",
        "curated/map_alpha/0_ground.png",
        "curated/map_alpha/1_upper_1.png",
        "rejected/rejections.json",
        "staging/cand_1/0.png",
        "staging/cand_1/1.png",
        "telemetry/cost_ledger.json",
    ];

    assert_eq!(keys, expected);
    assert_eq!(discovered.len(), 7);

    // Verify parsed variants
    let parsed_keys: Vec<&BlobKey> = discovered.iter().map(|(k, _)| k).collect();
    assert!(parsed_keys.iter().any(|k| matches!(
        k,
        BlobKey::CuratedLayer {
            id,
            index: 0,
            role: LayerRole::Ground,
            elevation: Some(0)
        } if id == "map_alpha"
    )));
    assert!(parsed_keys.iter().any(|k| matches!(
        k,
        BlobKey::CuratedLayer {
            id,
            index: 1,
            role: LayerRole::UpperFloor,
            elevation: Some(1)
        } if id == "map_alpha"
    )));
}

#[tokio::test]
async fn test_run_storage_migration_end_to_end_and_idempotent() {
    let temp = setup_mock_data_dir();
    let target = Arc::new(InMemoryBlobStorage::new());

    assert!(!is_migration_completed(temp.path()));

    // 1. Initial migration run
    let report = run_storage_migration(temp.path(), target.clone(), MigrationOptions::default())
        .await
        .expect("run migration");

    assert_eq!(report.total_discovered, 7);
    assert_eq!(report.migrated_count, 7);
    assert_eq!(report.skipped_count, 0);
    assert!(!report.dry_run);
    assert!(report.total_bytes_migrated > 0);

    // Marker file was written
    assert!(is_migration_completed(temp.path()));
    let marker_path = temp.path().join(MIGRATION_MARKER_FILE);
    assert!(marker_path.is_file());

    // Verify all 7 blobs exist in target with expected bytes
    assert_eq!(
        target
            .read_blob(&BlobKey::StagingLayer {
                id: "cand_1".into(),
                index: 0
            })
            .await
            .unwrap(),
        b"staging-layer-0-png-bytes"
    );
    assert_eq!(
        target
            .read_blob(&BlobKey::CuratedLayer {
                id: "map_alpha".into(),
                index: 0,
                role: LayerRole::Ground,
                elevation: Some(0)
            })
            .await
            .unwrap(),
        b"ground-layer-bytes"
    );
    assert_eq!(
        target.read_blob(&BlobKey::RejectionLog).await.unwrap(),
        b"[{\"id\":\"r1\"}]"
    );

    // 2. Second migration run without force (should be skipped via marker)
    let second_report =
        run_storage_migration(temp.path(), target.clone(), MigrationOptions::default())
            .await
            .expect("second migration");

    // Returns previously written report
    assert_eq!(second_report.migrated_count, 7);

    // 3. Migration run with force = true
    let force_options = MigrationOptions {
        force: true,
        ..Default::default()
    };
    let force_report = run_storage_migration(temp.path(), target.clone(), force_options)
        .await
        .expect("force migration");

    // All 7 were skipped because they already exist in target
    assert_eq!(force_report.total_discovered, 7);
    assert_eq!(force_report.migrated_count, 0);
    assert_eq!(force_report.skipped_count, 7);
    assert_eq!(force_report.total_bytes_migrated, 0);
}

#[tokio::test]
async fn test_run_storage_migration_dry_run_does_not_modify_target_or_write_marker() {
    let temp = setup_mock_data_dir();
    let target = Arc::new(InMemoryBlobStorage::new());

    let dry_run_options = MigrationOptions {
        dry_run: true,
        ..Default::default()
    };

    let report = run_storage_migration(temp.path(), target.clone(), dry_run_options)
        .await
        .expect("dry run");

    assert_eq!(report.total_discovered, 7);
    assert_eq!(report.migrated_count, 7);
    assert_eq!(report.skipped_count, 0);
    assert!(report.dry_run);
    assert!(report.total_bytes_migrated > 0);

    // Marker was NOT written
    assert!(!is_migration_completed(temp.path()));

    // Target storage is still empty
    assert!(!target
        .exists_blob(&BlobKey::StagingLayer {
            id: "cand_1".into(),
            index: 0
        })
        .await
        .unwrap());
}

#[tokio::test]
async fn test_run_storage_migration_empty_directory() {
    let temp = TempDir::new().expect("create empty temp dir");
    let target = Arc::new(InMemoryBlobStorage::new());

    let report = run_storage_migration(temp.path(), target, MigrationOptions::default())
        .await
        .expect("migration on empty dir");

    assert_eq!(report.total_discovered, 0);
    assert_eq!(report.migrated_count, 0);
    assert_eq!(report.skipped_count, 0);
    assert_eq!(report.total_bytes_migrated, 0);
    assert!(is_migration_completed(temp.path()));
}

#[tokio::test]
async fn test_run_storage_migration_with_partially_existing_blobs() {
    let temp = setup_mock_data_dir();
    let target = Arc::new(InMemoryBlobStorage::new());

    // Pre-populate one of the 7 blobs into target
    target
        .write_blob(
            &BlobKey::StagingLayer {
                id: "cand_1".into(),
                index: 0,
            },
            b"staging-layer-0-png-bytes",
        )
        .await
        .unwrap();

    let report = run_storage_migration(temp.path(), target.clone(), MigrationOptions::default())
        .await
        .expect("migration with partial existing");

    // 1 skipped, 6 migrated
    assert_eq!(report.total_discovered, 7);
    assert_eq!(report.skipped_count, 1);
    assert_eq!(report.migrated_count, 6);
    assert!(is_migration_completed(temp.path()));
}
