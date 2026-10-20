//! Tests for the filesystem-backed `BlobStorage`/`CatalogRepository` adapters.

use std::path::Path;
use std::sync::Arc;

use curator::adapters::filesystem_blob_storage::FilesystemBlobStorage;
use curator::adapters::filesystem_catalog_repository::FilesystemCatalogRepository;
use curator::adapters::filesystem_storage::FilesystemStorage;
use curator::domain::{
    Candidate, CuratedMap, LayerRole, RejectionReason, RejectionRecord, Source, Theme,
};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use curator::ports::storage_error::StorageError;
use tempfile::tempdir;

fn candidate(id: &str, source: Source, parent_group_id: Option<&str>) -> Candidate {
    let candidate = Candidate::with_now(id, source, format!("{id}.png"), (512, 512));
    match parent_group_id {
        Some(parent) => candidate.with_parent_group_id(parent),
        None => candidate,
    }
}

fn curated(id: &str, theme: Theme, tags: &[&str]) -> CuratedMap {
    CuratedMap::with_now(
        id,
        theme,
        tags.iter().map(|tag| (*tag).to_string()).collect(),
        (1024, 1024),
    )
}

fn candidate_ids(candidates: Vec<Candidate>) -> Vec<String> {
    candidates.into_iter().map(|candidate| candidate.id).collect()
}

fn curated_ids(maps: Vec<CuratedMap>) -> Vec<String> {
    maps.into_iter().map(|map| map.id).collect()
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Recursively collects `(relative path, bytes)` for every file under `root`, sorted by path, so
/// two snapshots of a tree compare structurally as well as byte-for-byte.
fn snapshot_dir(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut snapshot = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                snapshot.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    snapshot.sort();
    snapshot
}

fn candidate_manifest(id: &str, source: Source) -> Vec<u8> {
    serde_json::to_string_pretty(&candidate(id, source, None))
        .unwrap()
        .into_bytes()
}

fn blob_keys() -> Vec<BlobKey> {
    vec![
        BlobKey::StagingLayer {
            id: "cand-1".into(),
            index: 2,
        },
        BlobKey::CuratedLayer {
            id: "map-1".into(),
            index: 3,
            role: LayerRole::UpperFloor,
            elevation: Some(2),
        },
        BlobKey::ExportArchive {
            name: "dataset-42".into(),
        },
    ]
}

#[tokio::test]
async fn blob_round_trip_every_variant_lands_at_exact_path() {
    let dir = tempdir().unwrap();
    let storage = FilesystemBlobStorage::new(dir.path());

    for (position, key) in blob_keys().into_iter().enumerate() {
        let payload = format!("payload-{position}").into_bytes();
        assert!(!storage.exists_blob(&key).await.unwrap());
        assert!(matches!(
            storage.read_blob(&key).await,
            Err(StorageError::NotFound(_))
        ));

        storage.write_blob(&key, &payload).await.unwrap();

        let expected = dir.path().join(key.to_path());
        assert_eq!(std::fs::read(&expected).unwrap(), payload);
        assert!(storage.exists_blob(&key).await.unwrap());
        assert_eq!(storage.read_blob(&key).await.unwrap(), payload);

        // Writing again replaces the previous contents atomically.
        storage.write_blob(&key, b"replacement").await.unwrap();
        assert_eq!(storage.read_blob(&key).await.unwrap(), b"replacement");

        storage.delete_blob(&key).await.unwrap();
        assert!(!storage.exists_blob(&key).await.unwrap());
        assert!(!expected.exists());
        // Deletion is idempotent.
        storage.delete_blob(&key).await.unwrap();
    }

    assert_eq!(
        BlobKey::StagingLayer { id: "cand-1".into(), index: 2 }.to_path(),
        "staging/cand-1/2.png"
    );
    assert_eq!(
        BlobKey::CuratedLayer {
            id: "map-1".into(),
            index: 3,
            role: LayerRole::UpperFloor,
            elevation: Some(2),
        }
        .to_path(),
        "curated/map-1/3_upper_2.png"
    );
    assert_eq!(
        BlobKey::ExportArchive { name: "dataset-42".into() }.to_path(),
        "exports/dataset-42.zip"
    );
}

#[tokio::test]
async fn blob_dir_prefixes_do_not_match_siblings() {
    let dir = tempdir().unwrap();
    let storage = FilesystemBlobStorage::new(dir.path());

    let cand_1 = BlobKey::StagingLayer { id: "cand-1".into(), index: 0 };
    let cand_10 = BlobKey::StagingLayer { id: "cand-10".into(), index: 0 };
    let map_1 = BlobKey::CuratedLayer {
        id: "map-1".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    };
    let map_2 = BlobKey::CuratedLayer {
        id: "map-2".into(),
        index: 1,
        role: LayerRole::Roof,
        elevation: None,
    };
    let archive = BlobKey::ExportArchive { name: "batch".into() };

    for key in [&cand_1, &cand_10, &map_1, &map_2, &archive] {
        storage.write_blob(key, key.to_path().as_bytes()).await.unwrap();
    }
    // A flat legacy export file that must never be reported as a blob of `map-1`.
    write_file(&dir.path().join("curated/map-1_lora1024.png"), b"legacy");

    let staging_1 = storage
        .list_prefix(&BlobKey::StagingDir { id: Some("cand-1".into()) })
        .await
        .unwrap();
    assert_eq!(staging_1, vec![cand_1.clone()]);

    let all_staging = storage
        .list_prefix(&BlobKey::StagingDir { id: None })
        .await
        .unwrap();
    assert_eq!(all_staging, vec![cand_1.clone(), cand_10.clone()]);

    let curated_1 = storage
        .list_prefix(&BlobKey::CuratedDir { id: Some("map-1".into()) })
        .await
        .unwrap();
    assert_eq!(curated_1, vec![map_1.clone()]);

    let all_curated = storage
        .list_prefix(&BlobKey::CuratedDir { id: None })
        .await
        .unwrap();
    assert_eq!(all_curated, vec![map_1.clone(), map_2.clone()]);

    // Prefixes match on the canonical path: an exact key is its own prefix.
    let exports = storage.list_prefix(&archive).await.unwrap();
    assert_eq!(exports, vec![archive.clone()]);

    // Prefixes are not matched against the archive name: `bat.zip` is not a prefix of
    // `batch.zip`, so `ExportArchive { name: "bat" }` matches nothing.
    let name_prefix = storage
        .list_prefix(&BlobKey::ExportArchive { name: "bat".into() })
        .await
        .unwrap();
    assert!(name_prefix.is_empty());

    let missing = storage
        .list_prefix(&BlobKey::StagingDir { id: Some("absent".into()) })
        .await
        .unwrap();
    assert!(missing.is_empty());

    for key in all_staging.into_iter().chain(all_curated).chain(exports) {
        let path = dir.path().join(key.to_path());
        assert!(path.is_file());
        let relative = path.strip_prefix(dir.path()).unwrap();
        assert_eq!(relative.to_str().unwrap(), key.to_path());
    }
}

#[tokio::test]
async fn blob_storage_is_usable_as_trait_object() {
    let dir = tempdir().unwrap();
    let storage: Arc<dyn BlobStorage> = Arc::new(FilesystemBlobStorage::new(dir.path()));
    let key = BlobKey::StagingLayer { id: "cand-1".into(), index: 0 };

    storage.write_blob(&key, b"bytes").await.unwrap();
    assert_eq!(storage.read_blob(&key).await.unwrap(), b"bytes");
}

#[tokio::test]
async fn candidate_crud_uses_staging_manifest_path() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());
    let c = candidate("cand-1", Source::Reddit, Some("grp-1"));

    assert!(matches!(
        storage.get_candidate("cand-1").await,
        Err(StorageError::NotFound(_))
    ));

    storage.save_candidate(&c).await.unwrap();
    let manifest = dir.path().join("staging/cand-1/manifest.json");
    assert!(manifest.is_file());
    let parsed: Candidate =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(parsed.id, "cand-1");
    assert_eq!(storage.get_candidate("cand-1").await.unwrap().id, "cand-1");

    // Upsert overwrites in place.
    let mut updated = c.clone();
    updated.title = Some("updated".into());
    storage.save_candidate(&updated).await.unwrap();
    assert_eq!(
        storage.get_candidate("cand-1").await.unwrap().title.as_deref(),
        Some("updated")
    );

    storage.delete_candidate("cand-1").await.unwrap();
    assert!(!manifest.exists());
    assert!(matches!(
        storage.get_candidate("cand-1").await,
        Err(StorageError::NotFound(_))
    ));
    storage.delete_candidate("cand-1").await.unwrap();
}

#[tokio::test]
async fn curated_crud_uses_curated_manifest_path_without_theme_dirs() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());
    let map = curated("map-1", Theme::Dungeon, &["tavern"]);

    assert!(matches!(
        storage.get_curated("map-1").await,
        Err(StorageError::NotFound(_))
    ));

    storage.save_curated(&map).await.unwrap();
    let manifest = dir.path().join("curated/map-1/manifest.json");
    assert!(manifest.is_file());
    let parsed: CuratedMap =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(parsed.id, "map-1");

    // Theme membership is derived from tags, never from a `curated/<theme>/` directory.
    assert!(parsed.tags.iter().any(|tag| tag == "dungeon"));
    assert!(!dir.path().join("curated/dungeon").exists());
    assert_eq!(
        std::fs::read_dir(dir.path().join("curated"))
            .unwrap()
            .count(),
        1
    );

    assert_eq!(storage.get_curated("map-1").await.unwrap().id, "map-1");
    storage.delete_curated("map-1").await.unwrap();
    assert!(!manifest.exists());
    assert!(matches!(
        storage.get_curated("map-1").await,
        Err(StorageError::NotFound(_))
    ));
    storage.delete_curated("map-1").await.unwrap();
}

#[tokio::test]
async fn list_candidates_filters_sorts_and_paginates() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    storage
        .save_candidate(&candidate("cand-3", Source::Manual, Some("grp-b")))
        .await
        .unwrap();
    storage
        .save_candidate(&candidate("cand-1", Source::Reddit, Some("grp-a")))
        .await
        .unwrap();
    storage
        .save_candidate(&candidate("cand-2", Source::Reddit, Some("grp-a")))
        .await
        .unwrap();

    let all = storage
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap();
    assert_eq!(candidate_ids(all), vec!["cand-1", "cand-2", "cand-3"]);

    let reddit = storage
        .list_candidates(&CandidateFilter {
            source: Some(Source::Reddit),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(reddit), vec!["cand-1", "cand-2"]);

    let grouped = storage
        .list_candidates(&CandidateFilter {
            parent_group_id: Some("grp-a".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(grouped), vec!["cand-1", "cand-2"]);

    let limited = storage
        .list_candidates(&CandidateFilter {
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(limited), vec!["cand-2"]);

    let zero_limit = storage
        .list_candidates(&CandidateFilter {
            limit: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(zero_limit.is_empty());

    let past_end = storage
        .list_candidates(&CandidateFilter {
            offset: Some(99),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(past_end.is_empty());
}

#[tokio::test]
async fn list_curated_applies_theme_tags_ids_and_pagination() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    storage
        .save_curated(&curated("map-1", Theme::Dungeon, &["tavern", "water"]))
        .await
        .unwrap();
    storage
        .save_curated(&curated("map-2", Theme::Dungeon, &["cave"]))
        .await
        .unwrap();
    storage
        .save_curated(&curated("map-3", Theme::Urban, &["tavern"]))
        .await
        .unwrap();

    let dungeon = storage
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Dungeon),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(dungeon), vec!["map-1", "map-2"]);

    let tags = storage
        .list_curated(&CuratedFilter {
            tags: vec!["dungeon".into(), "cave".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(tags), vec!["map-2"]);

    let ids = storage
        .list_curated(&CuratedFilter {
            ids: Some(vec!["map-3".into(), "map-1".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(ids), vec!["map-1", "map-3"]);

    let paged = storage
        .list_curated(&CuratedFilter {
            limit: Some(1),
            offset: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(paged), vec!["map-3"]);

    let past_end = storage
        .list_curated(&CuratedFilter {
            offset: Some(10),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(past_end.is_empty());

    let zero_limit = storage
        .list_curated(&CuratedFilter {
            limit: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(zero_limit.is_empty());
}

#[tokio::test]
async fn rejection_log_appends_and_accumulates() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    assert!(!storage.is_rejected("bad-1").await.unwrap());

    storage
        .log_rejection("bad-1", RejectionReason::BlurryLowRes, Some("too small"))
        .await
        .unwrap();
    storage
        .log_rejection("bad-2", RejectionReason::GridBurnedIn, None)
        .await
        .unwrap();
    storage
        .log_rejection("bad-1", RejectionReason::Irrelevant, None)
        .await
        .unwrap();

    assert!(storage.is_rejected("bad-1").await.unwrap());
    assert!(storage.is_rejected("bad-2").await.unwrap());
    assert!(!storage.is_rejected("good-1").await.unwrap());

    let log = dir.path().join("rejected/rejections.json");
    assert!(log.is_file());
    let records: Vec<RejectionRecord> =
        serde_json::from_str(&std::fs::read_to_string(&log).unwrap()).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].id, "bad-1");
    assert_eq!(records[0].reason, RejectionReason::BlurryLowRes);
    assert_eq!(records[0].notes.as_deref(), Some("too small"));
    assert_eq!(records[1].id, "bad-2");
    assert_eq!(records[2].id, "bad-1");
    assert_eq!(records[2].reason, RejectionReason::Irrelevant);
}

#[tokio::test]
async fn malformed_manifest_is_invalid_data_and_listing_skips_it() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    write_file(&dir.path().join("staging/broken/manifest.json"), b"{not json");
    write_file(&dir.path().join("curated/broken/manifest.json"), b"[1, 2, 3]");

    assert!(matches!(
        storage.get_candidate("broken").await,
        Err(StorageError::InvalidData(_))
    ));
    assert!(matches!(
        storage.get_curated("broken").await,
        Err(StorageError::InvalidData(_))
    ));

    storage
        .save_candidate(&candidate("cand-1", Source::Manual, None))
        .await
        .unwrap();
    storage
        .save_curated(&curated("map-1", Theme::Dungeon, &["cave"]))
        .await
        .unwrap();

    let candidates = storage
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap();
    assert_eq!(candidate_ids(candidates), vec!["cand-1"]);
    let maps = storage
        .list_curated(&CuratedFilter::default())
        .await
        .unwrap();
    assert_eq!(curated_ids(maps), vec!["map-1"]);
}

#[tokio::test]
async fn catalog_repository_is_usable_as_trait_object() {
    let dir = tempdir().unwrap();
    let storage: Arc<dyn CatalogRepository> =
        Arc::new(FilesystemCatalogRepository::new(dir.path()));

    storage
        .save_candidate(&candidate("cand-1", Source::Generated, None))
        .await
        .unwrap();
    assert_eq!(storage.get_candidate("cand-1").await.unwrap().id, "cand-1");
}

/// Legacy `staging/{reddit,generated}/{id}/` bundles written before the port cutover must be
/// flattened into `staging/{id}/` at startup, otherwise the flat-only catalog never sees them and
/// ingestion deduplication re-downloads posts that are already staged.
#[tokio::test]
async fn startup_migration_flattens_legacy_staging_bundles_for_dedup() {
    let dir = tempdir().unwrap();
    let legacy_reddit = dir.path().join("staging/reddit/legacy_crypt");
    let legacy_generated = dir.path().join("staging/generated/gen_crypt");
    write_file(&legacy_reddit.join("0.png"), b"legacy-reddit-layer");
    write_file(&legacy_reddit.join("manifest.json"), &candidate_manifest("legacy_crypt", Source::Reddit));
    write_file(&legacy_generated.join("0.png"), b"legacy-generated-layer");
    write_file(
        &legacy_generated.join("manifest.json"),
        &candidate_manifest("gen_crypt", Source::Generated),
    );

    // The constructor runs the startup migration; run it again to prove it is safe on every boot.
    let storage = FilesystemStorage::new(dir.path());
    storage.migrate_legacy_storage().unwrap();

    // The legacy bundle directories and their now-empty subfolders are gone.
    assert!(!legacy_reddit.exists());
    assert!(!legacy_generated.exists());
    assert!(!dir.path().join("staging/reddit").exists());
    assert!(!dir.path().join("staging/generated").exists());

    // Their bytes survive at the uniform `staging/{id}/` location.
    let flat_reddit = dir.path().join("staging/legacy_crypt");
    let flat_generated = dir.path().join("staging/gen_crypt");
    assert!(flat_reddit.join("manifest.json").is_file());
    assert!(flat_generated.join("manifest.json").is_file());
    assert_eq!(
        std::fs::read(flat_reddit.join("0.png")).unwrap(),
        b"legacy-reddit-layer"
    );
    assert_eq!(
        std::fs::read(flat_generated.join("0.png")).unwrap(),
        b"legacy-generated-layer"
    );

    // Deduplication reads the flat catalog, so the migrated candidates are visible to it.
    let catalog = FilesystemCatalogRepository::new(dir.path());
    assert_eq!(
        catalog.get_candidate("legacy_crypt").await.unwrap().id,
        "legacy_crypt"
    );
    assert_eq!(
        catalog.get_candidate("gen_crypt").await.unwrap().id,
        "gen_crypt"
    );
    assert_eq!(
        candidate_ids(
            catalog
                .list_candidates(&CandidateFilter::default())
                .await
                .unwrap()
        ),
        vec!["gen_crypt".to_string(), "legacy_crypt".to_string()]
    );
}

#[tokio::test]
async fn startup_migration_never_clobbers_an_existing_flat_bundle() {
    let dir = tempdir().unwrap();
    let flat = dir.path().join("staging/shared_id");
    let legacy = dir.path().join("staging/reddit/shared_id");
    let manifest = candidate_manifest("shared_id", Source::Reddit);
    write_file(&flat.join("0.png"), b"flat-bundle-layer");
    write_file(&flat.join("manifest.json"), &manifest);
    write_file(&legacy.join("0.png"), b"legacy-bundle-layer");
    write_file(&legacy.join("manifest.json"), &manifest);

    let storage = FilesystemStorage::new(dir.path());
    storage.migrate_legacy_storage().unwrap();

    // The pre-existing flat bundle wins and is untouched; the legacy copy is not merged into it
    // and is left fully intact rather than half-moved.
    assert_eq!(
        std::fs::read(flat.join("0.png")).unwrap(),
        b"flat-bundle-layer"
    );
    assert!(flat.join("manifest.json").is_file());
    assert_eq!(
        std::fs::read(legacy.join("0.png")).unwrap(),
        b"legacy-bundle-layer"
    );
    assert!(legacy.join("manifest.json").is_file());

    // Only the flat bundle is catalogued, so the id is listed exactly once.
    let catalog = FilesystemCatalogRepository::new(dir.path());
    assert_eq!(
        catalog.get_candidate("shared_id").await.unwrap().id,
        "shared_id"
    );
    assert_eq!(
        catalog
            .list_candidates(&CandidateFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn startup_migration_flattening_is_idempotent() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join("staging/reddit/legacy_crypt");
    write_file(&legacy.join("0.png"), b"legacy-reddit-layer");
    write_file(
        &legacy.join("manifest.json"),
        &candidate_manifest("legacy_crypt", Source::Reddit),
    );

    let storage = FilesystemStorage::new(dir.path());
    let after_first_run = snapshot_dir(dir.path());

    storage.migrate_legacy_storage().unwrap();
    storage.migrate_legacy_storage().unwrap();

    assert_eq!(snapshot_dir(dir.path()), after_first_run);
    assert!(dir.path().join("staging/legacy_crypt/manifest.json").is_file());
}
