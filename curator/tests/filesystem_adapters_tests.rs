//! Tests for the filesystem-backed `BlobStorage`/`CatalogRepository` adapters.

use std::path::Path;
use std::sync::Arc;

use curator::adapters::filesystem_blob_storage::FilesystemBlobStorage;
use curator::adapters::filesystem_catalog_repository::FilesystemCatalogRepository;
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

    // Non-directory prefixes match by path prefix.
    let exports = storage
        .list_prefix(&BlobKey::ExportArchive { name: "bat".into() })
        .await
        .unwrap();
    assert_eq!(exports, vec![archive.clone()]);

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
