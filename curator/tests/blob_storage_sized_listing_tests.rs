//! Red-phase tests for #292, port contract: one listing call must report each blob's size.
//!
//! `BlobStorage::list_prefix` receives every object's byte size in the `ListObjectsV2` response
//! contents (and `DirEntry::metadata()` reports it on the filesystem), then throws it away and
//! returns only `Vec<BlobKey>`. Accountancy therefore asks for each size again — a signed
//! `head_object` over the WAN per blob on Cloudflare R2, which is what made
//! `GET /api/storage/stats` take ~94s and starve the liveness probe.
//!
//! These tests pin the replacement: a sized listing that carries the size the enumeration
//! already learned, agreeing key for key with the key-only listing it replaces.
//!
//! This file deliberately does not compile against `main` (`BlobEntry` and
//! `list_prefix_with_sizes` do not exist yet): the port change is the acceptance criterion.

use curator::adapters::{FilesystemBlobStorage, InMemoryBlobStorage};
use curator::domain::LayerRole;
use curator::ports::blob_storage::{BlobEntry, BlobKey, BlobStorage};

fn staging_layer(id: &str, index: usize) -> BlobKey {
    BlobKey::StagingLayer {
        id: id.to_string(),
        index,
    }
}

fn curated_layer(id: &str, index: usize, role: LayerRole, elevation: Option<i8>) -> BlobKey {
    BlobKey::CuratedLayer {
        id: id.to_string(),
        index,
        role,
        elevation,
    }
}

/// `(canonical path, size)` pairs, so an assertion reads as the footprint the operator sees.
fn sized_paths(entries: &[BlobEntry]) -> Vec<(String, u64)> {
    entries
        .iter()
        .map(|entry| (entry.key.to_path(), entry.size))
        .collect()
}

/// The double every test in this repo uses: it must report the sizes it already stored.
#[tokio::test]
async fn test_in_memory_sized_listing_reports_every_key_with_its_size() {
    let blobs = InMemoryBlobStorage::new();
    let layer_0 = staging_layer("cand_a", 0);
    let layer_1 = staging_layer("cand_a", 1);
    blobs
        .write_blob(&layer_0, &[7u8; 512])
        .await
        .expect("write layer 0");
    blobs
        .write_blob(&layer_1, &[7u8; 128])
        .await
        .expect("write layer 1");
    // A sibling tree, so the listing proves it stays scoped to the requested prefix.
    blobs
        .write_blob(
            &BlobKey::TelemetryLog {
                name: "cost_ledger.json".to_string(),
            },
            &[1u8; 300],
        )
        .await
        .expect("write cost ledger");

    let entries = blobs
        .list_prefix_with_sizes(&BlobKey::StagingDir { id: None })
        .await
        .expect("sized staging listing");

    assert_eq!(
        sized_paths(&entries),
        vec![
            ("staging/cand_a/0.png".to_string(), 512),
            ("staging/cand_a/1.png".to_string(), 128),
        ],
        "the sized listing reports each blob with the size it holds"
    );

    // The sized listing replaces the key-only one, so the two must agree key for key.
    let key_only: Vec<String> = blobs
        .list_prefix(&BlobKey::StagingDir { id: None })
        .await
        .expect("key-only staging listing")
        .iter()
        .map(|key| key.to_path())
        .collect();
    let sized: Vec<String> = entries.iter().map(|entry| entry.key.to_path()).collect();
    assert_eq!(sized, key_only, "the sized listing agrees with list_prefix");

    // An empty prefix is an empty listing, not an error.
    assert!(
        blobs
            .list_prefix_with_sizes(&BlobKey::StagingDir {
                id: Some("never_staged".to_string()),
            })
            .await
            .expect("empty sized listing")
            .is_empty(),
        "a prefix with no blobs lists nothing"
    );
}

/// The filesystem adapter already walks the directory, so it must keep the length that walk
/// reports instead of stat'ing every file a second time.
#[tokio::test]
async fn test_filesystem_sized_listing_reports_the_lengths_of_its_directory_walk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let blobs = FilesystemBlobStorage::new(dir.path());

    let ground = curated_layer("map_1", 0, LayerRole::Ground, Some(0));
    let roof = curated_layer("map_1", 1, LayerRole::Roof, None);
    blobs
        .write_blob(&ground, &[1u8; 4096])
        .await
        .expect("write ground layer");
    blobs
        .write_blob(&roof, &[2u8; 64])
        .await
        .expect("write roof layer");

    let entries = blobs
        .list_prefix_with_sizes(&BlobKey::CuratedDir {
            id: Some("map_1".to_string()),
        })
        .await
        .expect("sized curated listing");

    assert_eq!(
        sized_paths(&entries),
        vec![
            ("curated/map_1/0_ground.png".to_string(), 4096),
            ("curated/map_1/1_roof.png".to_string(), 64),
        ],
        "the sized listing reports the on-disk length of every file it walked"
    );

    for entry in &entries {
        assert_eq!(
            entry.size,
            blobs
                .blob_size(&entry.key)
                .await
                .expect("size of a listed blob"),
            "the listing size is the blob's real length for '{}'",
            entry.key.to_path()
        );
    }
}
