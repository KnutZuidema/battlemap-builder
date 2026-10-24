//! Tests for the filesystem-backed `BlobStorage`/`CatalogRepository` adapters.

use std::path::Path;
use std::sync::Arc;

use curator::adapters::filesystem_blob_storage::FilesystemBlobStorage;
use curator::adapters::filesystem_catalog_repository::FilesystemCatalogRepository;
use curator::domain::{
    BenchmarkMetadata, Candidate, CuratedMap, LayerRole, RejectionReason, RejectionRecord, Source,
    Theme,
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
        BlobKey::RejectionLog,
        BlobKey::TelemetryLog {
            name: "cost_ledger.json".into(),
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
    assert_eq!(BlobKey::RejectionLog.to_path(), "rejected/rejections.json");
    assert_eq!(
        BlobKey::TelemetryLog { name: "cost_ledger.json".into() }.to_path(),
        "telemetry/cost_ledger.json"
    );
    assert_eq!(BlobKey::TelemetryDir.to_path(), "telemetry/");
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

    let rejection = BlobKey::RejectionLog;
    let ledger = BlobKey::TelemetryLog { name: "cost_ledger.json".into() };
    for key in [&rejection, &ledger] {
        storage.write_blob(key, key.to_path().as_bytes()).await.unwrap();
    }
    // Siblings whose paths merely start with `telemetry` must never be swallowed by the
    // `telemetry/` prefix.
    write_file(&dir.path().join("telemetry.json"), b"legacy");
    write_file(&dir.path().join("telemetry_archive/cost_ledger.json"), b"old");

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

    let telemetry = storage
        .list_prefix(&BlobKey::TelemetryDir)
        .await
        .unwrap();
    assert_eq!(telemetry, vec![ledger.clone()]);

    // The rejection log is its own key, and it lives outside the telemetry tree.
    assert_eq!(
        storage.list_prefix(&rejection).await.unwrap(),
        vec![rejection.clone()]
    );
    assert!(!telemetry.contains(&rejection));

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

    for key in all_staging
        .into_iter()
        .chain(all_curated)
        .chain(exports)
        .chain(telemetry)
        .chain([rejection.clone()])
    {
        let path = dir.path().join(key.to_path());
        assert!(path.is_file());
        let relative = path.strip_prefix(dir.path()).unwrap();
        assert_eq!(relative.to_str().unwrap(), key.to_path());
    }
}

/// The atomic-write temp files both in-repo writers drop into the directory they are about to
/// replace (`FileCostLedger::atomic_save` writes `telemetry/.cost_ledger_{pid}_{n}.tmp`,
/// `FilesystemBlobStorage::atomic_write` writes `.{name}.{pid}.{n}.tmp`) are hidden, and hidden
/// files are not blobs: they must not appear as telemetry ledgers, exactly like the retired
/// directory walk that skipped dotfiles.
#[tokio::test]
async fn telemetry_dir_prefix_ignores_hidden_atomic_write_temp_files() {
    let dir = tempdir().unwrap();
    let storage = FilesystemBlobStorage::new(dir.path());
    let ledger = BlobKey::TelemetryLog {
        name: "cost_ledger.json".into(),
    };
    storage.write_blob(&ledger, b"ledger").await.unwrap();

    // An in-flight replacement of the ledger and of a second, not-yet-written ledger.
    write_file(&dir.path().join("telemetry/.cost_ledger_4321_1.tmp"), b"in flight");
    write_file(
        &dir.path().join("telemetry/.generation_audit.json.4321_2.tmp"),
        b"in flight",
    );
    let on_disk = std::fs::read_dir(dir.path().join("telemetry"))
        .unwrap()
        .count();
    assert_eq!(on_disk, 3, "the temp files really are on disk");

    assert_eq!(
        storage.list_prefix(&BlobKey::TelemetryDir).await.unwrap(),
        vec![ledger]
    );
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
async fn rejection_records_reads_the_log_and_reports_malformed_data() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    // No log yet: an empty list, not an error.
    assert!(storage.rejection_records().await.unwrap().is_empty());

    storage
        .log_rejection("bad-1", RejectionReason::BlurryLowRes, Some("too small"))
        .await
        .unwrap();
    storage
        .log_rejection("bad-2", RejectionReason::GridBurnedIn, None)
        .await
        .unwrap();

    let records = storage.rejection_records().await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, "bad-1");
    assert_eq!(records[0].reason, RejectionReason::BlurryLowRes);
    assert_eq!(records[0].notes.as_deref(), Some("too small"));
    assert_eq!(records[1].id, "bad-2");
    assert_eq!(records[1].notes, None);

    // A malformed log mirrors the other catalog reads instead of reading as empty.
    write_file(&dir.path().join("rejected/rejections.json"), b"{not json");
    assert!(matches!(
        storage.rejection_records().await,
        Err(StorageError::InvalidData(_))
    ));
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
async fn legacy_generated_manifest_migration_restores_visibility() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    let metadata = BenchmarkMetadata::new(
        "gen-1",
        "google/gemini-3.1-flash-image",
        "cavern",
        "A moody cavern battlemap",
        Some(120),
        Some(1024),
        1500,
        0.04,
    );
    // Pre-#176 generated bundle: the raw `BenchmarkMetadata` document as the manifest, plus the
    // layer blob the retired `save_staging_generated` wrote next to it.
    write_file(
        &dir.path().join("staging/gen-1/manifest.json"),
        serde_json::to_string_pretty(&metadata).unwrap().as_bytes(),
    );
    write_file(&dir.path().join("staging/gen-1/0.png"), b"legacy-image-bytes");
    // A manifest that is neither a `Candidate` nor `BenchmarkMetadata` must be left alone.
    write_file(&dir.path().join("staging/foreign/manifest.json"), b"{not json");

    // Before the migration the strict catalog cannot see the generated bundle.
    assert!(matches!(
        storage.get_candidate("gen-1").await,
        Err(StorageError::InvalidData(_))
    ));
    assert!(storage
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap()
        .is_empty());

    assert_eq!(storage.migrate_legacy_generated_manifests(), 1);

    let migrated = storage.get_candidate("gen-1").await.unwrap();
    assert_eq!(migrated.id, "gen-1");
    assert_eq!(migrated.source, Source::Generated);
    assert_eq!(migrated.filename, "gen-1.png");
    assert_eq!(migrated.dimensions, (1024, 1024));
    assert_eq!(
        migrated.source_url.as_deref(),
        Some("openrouter://google/gemini-3.1-flash-image")
    );
    assert_eq!(migrated.resolved_layers()[0].index, 0);
    assert_eq!(
        candidate_ids(
            storage
                .list_candidates(&CandidateFilter::default())
                .await
                .unwrap()
        ),
        vec!["gen-1"]
    );
    // The layer blob is untouched and still sits at the path the migrated candidate addresses.
    assert_eq!(
        std::fs::read(dir.path().join("staging/gen-1/0.png")).unwrap(),
        b"legacy-image-bytes"
    );
    assert_eq!(
        std::fs::read(dir.path().join("staging/foreign/manifest.json")).unwrap(),
        b"{not json"
    );

    // Running it again is a no-op: nothing is rewritten and nothing is reported.
    assert_eq!(storage.migrate_legacy_generated_manifests(), 0);
    assert_eq!(storage.get_candidate("gen-1").await.unwrap(), migrated);
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

#[tokio::test]
async fn filesystem_catalog_trash_and_unreject_lifecycle() {
    let dir = tempdir().unwrap();
    let storage = FilesystemCatalogRepository::new(dir.path());

    let cand = candidate("cand-trash-1", Source::Reddit, None);

    // 1. Save to trash
    storage.save_trashed_candidate(&cand).await.unwrap();
    let loaded = storage.get_trashed_candidate("cand-trash-1").await.unwrap();
    assert_eq!(loaded.id, "cand-trash-1");

    // 2. Log rejection and query latest
    assert!(!storage.is_rejected("cand-trash-1").await.unwrap());
    storage
        .log_rejection("cand-trash-1", RejectionReason::BlurryLowRes, Some("too blurry"))
        .await
        .unwrap();
    assert!(storage.is_rejected("cand-trash-1").await.unwrap());
    let latest = storage.latest_rejection().await.unwrap().expect("rejection record exists");
    assert_eq!(latest.id, "cand-trash-1");

    // 3. Remove rejection (unreject)
    storage.remove_rejection("cand-trash-1").await.unwrap();
    assert!(!storage.is_rejected("cand-trash-1").await.unwrap());
    assert!(storage.latest_rejection().await.unwrap().is_none());

    // 4. Delete trashed candidate
    storage.delete_trashed_candidate("cand-trash-1").await.unwrap();
    assert!(matches!(
        storage.get_trashed_candidate("cand-trash-1").await,
        Err(StorageError::NotFound(_))
    ));
}
