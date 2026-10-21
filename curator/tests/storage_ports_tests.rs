//! Tests for the `BlobStorage`/`CatalogRepository` ports and their in-memory doubles.

use std::sync::Arc;

use curator::adapters::in_memory::{InMemoryBlobStorage, InMemoryCatalogRepository};
use curator::domain::{Candidate, CuratedMap, LayerRole, RejectionReason, Source, Theme};
use curator::ports::blob_storage::{BlobKey, BlobStorage};
use curator::ports::catalog_repository::{CandidateFilter, CatalogRepository, CuratedFilter};
use curator::ports::storage_error::StorageError;

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

#[tokio::test]
async fn blob_round_trip_lists_and_deletes_idempotently() {
    let storage = InMemoryBlobStorage::new();
    let layer_0 = BlobKey::StagingLayer { id: "cand-1".into(), index: 0 };
    let layer_1 = BlobKey::StagingLayer { id: "cand-1".into(), index: 1 };
    let curated_layer = BlobKey::CuratedLayer {
        id: "cand-1".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    };

    assert!(!storage.exists_blob(&layer_0).await.unwrap());
    assert!(matches!(
        storage.read_blob(&layer_0).await,
        Err(StorageError::NotFound(_))
    ));

    storage.write_blob(&layer_0, b"first").await.unwrap();
    storage.write_blob(&layer_1, b"second").await.unwrap();
    storage.write_blob(&curated_layer, b"curated").await.unwrap();

    assert!(storage.exists_blob(&layer_0).await.unwrap());
    assert_eq!(storage.read_blob(&layer_0).await.unwrap(), b"first".to_vec());

    // Writing an existing key overwrites the previous content.
    storage.write_blob(&layer_0, b"updated").await.unwrap();
    assert_eq!(storage.read_blob(&layer_0).await.unwrap(), b"updated".to_vec());

    // Prefix listings are path-ordered and scoped to the requested directory.
    assert_eq!(
        storage.list_prefix(&BlobKey::StagingDir { id: None }).await.unwrap(),
        vec![layer_0.clone(), layer_1.clone()]
    );
    assert_eq!(
        storage
            .list_prefix(&BlobKey::StagingDir { id: Some("cand-1".into()) })
            .await
            .unwrap(),
        vec![layer_0.clone(), layer_1.clone()]
    );
    assert!(storage
        .list_prefix(&BlobKey::StagingDir { id: Some("cand-2".into()) })
        .await
        .unwrap()
        .is_empty());
    // The curated tree is genuinely populated, and both the whole tree and the owning bundle
    // list its layer; a sibling id never does.
    assert_eq!(
        storage
            .list_prefix(&BlobKey::CuratedDir { id: Some("cand-1".into()) })
            .await
            .unwrap(),
        vec![curated_layer.clone()]
    );
    assert_eq!(
        storage.list_prefix(&BlobKey::CuratedDir { id: None }).await.unwrap(),
        vec![curated_layer.clone()]
    );
    assert!(storage
        .list_prefix(&BlobKey::CuratedDir { id: Some("cand-2".into()) })
        .await
        .unwrap()
        .is_empty());

    storage.delete_blob(&layer_0).await.unwrap();
    assert!(!storage.exists_blob(&layer_0).await.unwrap());
    // Deleting a missing blob is a no-op, not an error.
    storage.delete_blob(&layer_0).await.unwrap();
    assert!(matches!(
        storage.read_blob(&layer_0).await,
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn blob_key_to_path_resolves_canonical_paths() {
    assert_eq!(
        BlobKey::StagingLayer { id: "abc".into(), index: 2 }.to_path(),
        "staging/abc/2.png"
    );
    assert_eq!(
        BlobKey::CuratedLayer { id: "abc".into(), index: 0, role: LayerRole::Ground, elevation: Some(0) }
            .to_path(),
        "curated/abc/0_ground.png"
    );
    assert_eq!(
        BlobKey::CuratedLayer {
            id: "abc".into(),
            index: 1,
            role: LayerRole::UpperFloor,
            elevation: Some(1),
        }
        .to_path(),
        "curated/abc/1_upper_1.png"
    );
    // A non-Ground role with a negative elevation is encoded into the layer filename.
    assert_eq!(
        BlobKey::CuratedLayer {
            id: "abc".into(),
            index: 2,
            role: LayerRole::Basement,
            elevation: Some(-1),
        }
        .to_path(),
        "curated/abc/2_basement_1.png"
    );
    // The archive name is bare: `to_path` appends `.zip` unconditionally and never strips a
    // suffix, so structurally unequal keys can never alias onto one path.
    let bare_archive = BlobKey::ExportArchive { name: "dataset".into() };
    let suffixed_archive = BlobKey::ExportArchive { name: "dataset.zip".into() };
    assert_eq!(bare_archive.to_path(), "exports/dataset.zip");
    assert_eq!(suffixed_archive.to_path(), "exports/dataset.zip.zip");
    assert_ne!(bare_archive, suffixed_archive);
    assert_ne!(bare_archive.to_path(), suffixed_archive.to_path());
    assert_eq!(BlobKey::StagingDir { id: None }.to_path(), "staging/");
    assert_eq!(
        BlobKey::StagingDir { id: Some("abc".into()) }.to_path(),
        "staging/abc/"
    );
    assert_eq!(BlobKey::CuratedDir { id: None }.to_path(), "curated/");
    assert_eq!(
        BlobKey::CuratedDir { id: Some("abc".into()) }.to_path(),
        "curated/abc/"
    );
    assert_eq!(BlobKey::RejectionLog.to_path(), "rejected/rejections.json");
    assert_eq!(
        BlobKey::TelemetryLog { name: "cost_ledger.json".into() }.to_path(),
        "telemetry/cost_ledger.json"
    );
    // The telemetry directory prefix renders with its trailing slash, so it never swallows a
    // sibling whose path merely starts with the same text.
    assert_eq!(BlobKey::TelemetryDir.to_path(), "telemetry/");
    assert!(BlobKey::TelemetryDir.is_prefix_of(&BlobKey::TelemetryLog {
        name: "cost_ledger.json".into(),
    }));
    assert!(!BlobKey::TelemetryDir.is_prefix_of(&BlobKey::RejectionLog));
    assert!(!BlobKey::TelemetryDir.is_prefix_of(&BlobKey::ExportArchive {
        name: "telemetry".into(),
    }));
    assert!(!BlobKey::StagingDir { id: None }.is_prefix_of(&BlobKey::TelemetryLog {
        name: "cost_ledger.json".into(),
    }));
    assert!(!BlobKey::CuratedDir { id: None }.is_prefix_of(&BlobKey::RejectionLog));

    // A curated bundle prefix contains its own layers and is itself contained by the whole
    // curated tree, but never matches a sibling bundle whose id merely starts with the same text.
    let bundle = BlobKey::CuratedDir { id: Some("abc".into()) };
    assert!(bundle.is_prefix_of(&BlobKey::CuratedLayer {
        id: "abc".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    }));
    assert!(BlobKey::CuratedDir { id: None }.is_prefix_of(&BlobKey::CuratedLayer {
        id: "abc".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    }));
    assert!(!bundle.is_prefix_of(&BlobKey::CuratedLayer {
        id: "abcd".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    }));
    // Staging and curated trees never match each other, and exports hang off neither.
    assert!(!BlobKey::StagingDir { id: None }
        .is_prefix_of(&BlobKey::ExportArchive { name: "dataset".into() }));
    assert!(!bundle.is_prefix_of(&BlobKey::StagingLayer { id: "abc".into(), index: 0 }));
}

#[tokio::test]
async fn blob_list_prefix_orders_by_path_and_rejects_sibling_id_collisions() {
    let storage = InMemoryBlobStorage::new();
    let cand_1 = BlobKey::StagingLayer { id: "cand-1".into(), index: 0 };
    let cand_10 = BlobKey::StagingLayer { id: "cand-10".into(), index: 0 };
    let curated_layer = BlobKey::CuratedLayer {
        id: "cand-1".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    };

    // Written in reverse path order to prove the listing sorts by canonical path.
    storage.write_blob(&cand_10, b"ten").await.unwrap();
    storage.write_blob(&cand_1, b"one").await.unwrap();
    storage.write_blob(&curated_layer, b"curated").await.unwrap();

    assert_eq!(
        storage.list_prefix(&BlobKey::StagingDir { id: None }).await.unwrap(),
        vec![cand_1.clone(), cand_10.clone()]
    );
    // `staging/cand-1/` is a full directory prefix: it must not leak sibling id `cand-10`.
    assert_eq!(
        storage
            .list_prefix(&BlobKey::StagingDir { id: Some("cand-1".into()) })
            .await
            .unwrap(),
        vec![cand_1.clone()]
    );
    assert_eq!(
        storage
            .list_prefix(&BlobKey::StagingDir { id: Some("cand-10".into()) })
            .await
            .unwrap(),
        vec![cand_10.clone()]
    );
    // The curated prefix never returns staging blobs, even for a colliding id.
    assert_eq!(
        storage
            .list_prefix(&BlobKey::CuratedDir { id: Some("cand-1".into()) })
            .await
            .unwrap(),
        vec![curated_layer.clone()]
    );
    assert!(storage
        .list_prefix(&BlobKey::StagingDir { id: Some("cand-11".into()) })
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn telemetry_dir_prefix_lists_only_telemetry_logs() {
    let storage = InMemoryBlobStorage::new();
    let cost_ledger = BlobKey::TelemetryLog { name: "cost_ledger.json".into() };
    let audit = BlobKey::TelemetryLog { name: "generation_audit.json".into() };
    let rejection = BlobKey::RejectionLog;
    let staging = BlobKey::StagingLayer { id: "cand-1".into(), index: 0 };

    // Written out of path order to prove the listing sorts by canonical path.
    storage.write_blob(&audit, b"audit").await.unwrap();
    storage.write_blob(&cost_ledger, b"ledger").await.unwrap();
    storage.write_blob(&rejection, b"rejections").await.unwrap();
    storage.write_blob(&staging, b"layer").await.unwrap();

    assert_eq!(
        storage.list_prefix(&BlobKey::TelemetryDir).await.unwrap(),
        vec![cost_ledger.clone(), audit.clone()]
    );
    // The rejection log is not a telemetry log, and the whole key is its own prefix.
    assert_eq!(
        storage.list_prefix(&rejection).await.unwrap(),
        vec![rejection.clone()]
    );
    assert!(storage
        .list_prefix(&BlobKey::TelemetryDir)
        .await
        .unwrap()
        .iter()
        .all(|key| BlobKey::TelemetryDir.is_prefix_of(key)));
}

#[tokio::test]
async fn catalog_candidate_crud_and_filter_queries() {
    let repository = InMemoryCatalogRepository::new();

    repository
        .save_candidate(&candidate("b-reddit", Source::Reddit, Some("grp-1")))
        .await
        .unwrap();
    repository
        .save_candidate(&candidate("a-reddit", Source::Reddit, None))
        .await
        .unwrap();
    repository
        .save_candidate(&candidate("c-generated", Source::Generated, Some("grp-1")))
        .await
        .unwrap();

    assert_eq!(
        repository.get_candidate("a-reddit").await.unwrap().source,
        Source::Reddit
    );
    assert!(matches!(
        repository.get_candidate("missing").await,
        Err(StorageError::NotFound(_))
    ));

    // Saving an existing id upserts instead of duplicating.
    let mut updated = candidate("c-generated", Source::Manual, Some("grp-2"));
    updated.title = Some("updated title".into());
    repository.save_candidate(&updated).await.unwrap();
    assert_eq!(
        repository
            .get_candidate("c-generated")
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("updated title")
    );

    let all = repository
        .list_candidates(&CandidateFilter::default())
        .await
        .unwrap();
    assert_eq!(candidate_ids(all), vec!["a-reddit", "b-reddit", "c-generated"]);

    // `limit: Some(0)` yields an empty page even though three candidates match.
    assert!(candidate_ids(
        repository
            .list_candidates(&CandidateFilter {
                limit: Some(0),
                ..Default::default()
            })
            .await
            .unwrap()
    )
    .is_empty());

    // An `offset` past the end yields an empty page rather than an error.
    assert!(candidate_ids(
        repository
            .list_candidates(&CandidateFilter {
                offset: Some(99),
                ..Default::default()
            })
            .await
            .unwrap()
    )
    .is_empty());

    let reddit = repository
        .list_candidates(&CandidateFilter {
            source: Some(Source::Reddit),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(reddit), vec!["a-reddit", "b-reddit"]);

    let grouped = repository
        .list_candidates(&CandidateFilter {
            parent_group_id: Some("grp-1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(grouped), vec!["b-reddit"]);

    let paged = repository
        .list_candidates(&CandidateFilter {
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(paged), vec!["b-reddit"]);

    let manual_grouped = repository
        .list_candidates(&CandidateFilter {
            source: Some(Source::Manual),
            parent_group_id: Some("grp-2".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(candidate_ids(manual_grouped), vec!["c-generated"]);

    repository.delete_candidate("a-reddit").await.unwrap();
    repository.delete_candidate("a-reddit").await.unwrap();
    assert!(matches!(
        repository.get_candidate("a-reddit").await,
        Err(StorageError::NotFound(_))
    ));
    assert_eq!(
        candidate_ids(
            repository
                .list_candidates(&CandidateFilter::default())
                .await
                .unwrap()
        ),
        vec!["b-reddit", "c-generated"]
    );
}

#[tokio::test]
async fn catalog_curated_crud_and_filter_queries() {
    let repository = InMemoryCatalogRepository::new();

    repository.save_curated(&curated("m1", Theme::Dungeon, &["stone"])).await.unwrap();
    repository
        .save_curated(&curated("m2", Theme::Cave, &["stone", "water"]))
        .await
        .unwrap();
    repository.save_curated(&curated("m3", Theme::SciFi, &["metal"])).await.unwrap();

    let stored = repository.get_curated("m2").await.unwrap();
    assert_eq!(stored.theme(), Some(Theme::Cave));
    assert!(stored.tags.contains(&"water".to_string()));
    assert!(matches!(
        repository.get_curated("missing").await,
        Err(StorageError::NotFound(_))
    ));

    let all = repository.list_curated(&CuratedFilter::default()).await.unwrap();
    assert_eq!(curated_ids(all), vec!["m1", "m2", "m3"]);

    // `limit: Some(0)` yields an empty page, and an `offset` past the end yields an empty
    // page rather than an error.
    assert!(curated_ids(
        repository
            .list_curated(&CuratedFilter {
                limit: Some(0),
                ..Default::default()
            })
            .await
            .unwrap()
    )
    .is_empty());
    assert!(curated_ids(
        repository
            .list_curated(&CuratedFilter {
                offset: Some(99),
                ..Default::default()
            })
            .await
            .unwrap()
    )
    .is_empty());

    let dungeon = repository
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Dungeon),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(dungeon), vec!["m1"]);

    // Tag filters are conjunctive: every listed tag must be present.
    let stone = repository
        .list_curated(&CuratedFilter {
            tags: vec!["stone".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(stone), vec!["m1", "m2"]);

    let stone_water = repository
        .list_curated(&CuratedFilter {
            tags: vec!["stone".into(), "water".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(stone_water), vec!["m2"]);

    let by_ids = repository
        .list_curated(&CuratedFilter {
            ids: Some(vec!["m3".into(), "m1".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(by_ids), vec!["m1", "m3"]);

    let paged = repository
        .list_curated(&CuratedFilter {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(paged), vec!["m2", "m3"]);

    let combined = repository
        .list_curated(&CuratedFilter {
            theme: Some(Theme::Cave),
            tags: vec!["water".into()],
            ids: Some(vec!["m1".into(), "m2".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(curated_ids(combined), vec!["m2"]);

    // Saving an existing id upserts instead of duplicating.
    repository
        .save_curated(&curated("m3", Theme::SciFi, &["metal"]).with_description("hangar"))
        .await
        .unwrap();
    assert_eq!(
        repository.get_curated("m3").await.unwrap().description.as_deref(),
        Some("hangar")
    );
    assert_eq!(repository.list_curated(&CuratedFilter::default()).await.unwrap().len(), 3);

    repository.delete_curated("m3").await.unwrap();
    repository.delete_curated("m3").await.unwrap();
    assert!(matches!(
        repository.get_curated("m3").await,
        Err(StorageError::NotFound(_))
    ));
    assert_eq!(
        curated_ids(repository.list_curated(&CuratedFilter::default()).await.unwrap()),
        vec!["m1", "m2"]
    );
}

#[tokio::test]
async fn rejection_log_is_queryable() {
    let repository = InMemoryCatalogRepository::new();

    // An empty log reads as an empty list rather than an error.
    assert!(repository.rejection_records().await.unwrap().is_empty());

    assert!(!repository.is_rejected("cand-1").await.unwrap());
    repository
        .log_rejection("cand-1", RejectionReason::GridBurnedIn, Some("baked-in grid"))
        .await
        .unwrap();
    assert!(repository.is_rejected("cand-1").await.unwrap());
    assert!(!repository.is_rejected("cand-2").await.unwrap());

    // Repeated rejections append to the log instead of failing.
    repository
        .log_rejection("cand-1", RejectionReason::BlurryLowRes, None)
        .await
        .unwrap();
    assert!(repository.is_rejected("cand-1").await.unwrap());

    // The whole log is readable, in append order and with every field preserved.
    let records = repository.rejection_records().await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, "cand-1");
    assert_eq!(records[0].reason, RejectionReason::GridBurnedIn);
    assert_eq!(records[0].notes.as_deref(), Some("baked-in grid"));
    assert_eq!(records[1].id, "cand-1");
    assert_eq!(records[1].reason, RejectionReason::BlurryLowRes);
    assert_eq!(records[1].notes, None);
}

#[tokio::test]
async fn doubles_are_usable_through_arc_dyn_ports() {
    let blobs: Arc<dyn BlobStorage> = Arc::new(InMemoryBlobStorage::new());
    let catalog: Arc<dyn CatalogRepository> = Arc::new(InMemoryCatalogRepository::new());

    let layer = BlobKey::CuratedLayer {
        id: "map-1".into(),
        index: 0,
        role: LayerRole::Ground,
        elevation: Some(0),
    };
    blobs.write_blob(&layer, b"layer zero").await.unwrap();
    assert_eq!(blobs.read_blob(&layer).await.unwrap(), b"layer zero".to_vec());
    assert_eq!(
        blobs.list_prefix(&BlobKey::CuratedDir { id: None }).await.unwrap(),
        vec![layer.clone()]
    );

    catalog
        .save_candidate(&candidate("cand-1", Source::Reddit, Some("grp-1")))
        .await
        .unwrap();
    assert_eq!(catalog.get_candidate("cand-1").await.unwrap().id, "cand-1");

    catalog
        .save_curated(&curated("map-1", Theme::Tavern, &["wood"]))
        .await
        .unwrap();
    assert_eq!(
        curated_ids(
            catalog
                .list_curated(&CuratedFilter {
                    theme: Some(Theme::Tavern),
                    ..Default::default()
                })
                .await
                .unwrap()
        ),
        vec!["map-1"]
    );

    assert!(!catalog.is_rejected("cand-1").await.unwrap());
    catalog
        .log_rejection("cand-1", RejectionReason::Irrelevant, None)
        .await
        .unwrap();
    assert!(catalog.is_rejected("cand-1").await.unwrap());
    // The `Arc<dyn CatalogRepository>` blanket impl forwards the record listing too.
    assert_eq!(catalog.rejection_records().await.unwrap().len(), 1);

    catalog.delete_candidate("cand-1").await.unwrap();
    catalog.delete_curated("map-1").await.unwrap();
    blobs.delete_blob(&layer).await.unwrap();
    assert!(matches!(
        catalog.get_candidate("cand-1").await,
        Err(StorageError::NotFound(_))
    ));
    assert!(matches!(
        catalog.get_curated("map-1").await,
        Err(StorageError::NotFound(_))
    ));
    assert!(blobs
        .list_prefix(&BlobKey::CuratedDir { id: None })
        .await
        .unwrap()
        .is_empty());
}
