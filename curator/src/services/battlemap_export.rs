//! Full Battlemap ZIP export over the catalog, blob, and archive-spool ports (ADR-0003, ADR-0015).
//!
//! Unlike [`crate::services::DatasetExportService`], which crops the `Ground` Layer into 16×16
//! `Map Section` boxes resampled to `1024×1024` for LoRA training, this service packages complete,
//! full-resolution `Curated Map` bundles: every included `Layer` verbatim from `BlobStorage`
//! (zero image decode or re-encode, never acquiring the `image_work` decode permit) alongside
//! pretty-printed `<id>/manifest.json`.
//!
//! Archives are spooled to disk via [`ArchiveSpool`] with `CompressionMethod::Stored` members,
//! keeping peak resident memory bounded to a single Layer blob at a time.

use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;

use crate::domain::{CuratedMap, Layer, LayerRole, Theme};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::{CatalogRepository, CuratedFilter};
use crate::ports::storage_error::StorageError;
use crate::services::archive_spool::{ArchiveSpool, ArchiveWriter, SpooledArchive};

/// Resolves the ordered list of included `(BlobKey, canonical_filename)` pairs for a curated map.
///
/// Legacy `CuratedMap` records with an empty `layers` list fall back to resolving `0_ground.png`
/// at `BlobKey::CuratedLayer { id, index: 0, role: LayerRole::Ground, elevation: Some(0) }`.
/// Any `LayerRole::Exclude` layer is skipped.
fn resolve_included_layers(map: &CuratedMap) -> Vec<(BlobKey, String)> {
    if map.layers.is_empty() {
        return vec![(
            BlobKey::CuratedLayer {
                id: map.id.clone(),
                index: 0,
                role: LayerRole::Ground,
                elevation: Some(0),
            },
            Layer::format_filename(0, LayerRole::Ground, Some(0), "png"),
        )];
    }

    map.layers
        .iter()
        .filter(|layer| layer.role.is_included())
        .map(|layer| {
            (
                BlobKey::CuratedLayer {
                    id: map.id.clone(),
                    index: layer.index,
                    role: layer.role,
                    elevation: layer.elevation,
                },
                Layer::format_filename(layer.index, layer.role, layer.elevation, "png"),
            )
        })
        .collect()
}

/// Writes one `Stored` member into the spooled ZIP archive (ADR-0015).
fn write_stored_entry(
    zip: &mut zip::ZipWriter<ArchiveWriter>,
    path: &str,
    bytes: &[u8],
) -> Result<(), StorageError> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    zip.start_file(path, options)?;
    zip.write_all(bytes)?;
    Ok(())
}

/// Service exporting full-resolution `Curated Map` bundles as spooled ZIP archives.
///
/// Stateless: borrows the catalog, blob, and archive-spool handles so handlers construct it per
/// request from cheap `Arc` clones. All operations are pure reads.
#[derive(Clone)]
pub struct BattlemapExportService {
    catalog: Arc<dyn CatalogRepository>,
    blobs: Arc<dyn BlobStorage>,
    spool: Arc<ArchiveSpool>,
}

impl BattlemapExportService {
    /// Creates a service reading curated maps from `catalog`, layer blobs verbatim from `blobs`,
    /// and spooling archives through `spool`.
    pub fn new(
        catalog: Arc<dyn CatalogRepository>,
        blobs: Arc<dyn BlobStorage>,
        spool: Arc<ArchiveSpool>,
    ) -> Self {
        Self {
            catalog,
            blobs,
            spool,
        }
    }

    /// Strict single-map export.
    ///
    /// Returns [`StorageError::NotFound`] if the `CuratedMap` does not exist, has no included
    /// Layers, or if any included Layer blob is missing from storage (never returning a silent
    /// partial or empty archive).
    pub async fn export_battlemap(&self, id: &str) -> Result<SpooledArchive, StorageError> {
        let map = self.catalog.get_curated(id).await?;
        let layers = resolve_included_layers(&map);
        if layers.is_empty() {
            return Err(StorageError::NotFound(format!(
                "Curated map '{id}' has no included layers"
            )));
        }

        let mut zip = zip::ZipWriter::new(self.spool.create().await?);

        let manifest_bytes = serde_json::to_vec_pretty(&map)?;
        let manifest_path = format!("{}/manifest.json", map.id);
        write_stored_entry(&mut zip, &manifest_path, &manifest_bytes)?;
        drop(manifest_bytes);

        for (key, filename) in layers {
            let bytes = self.blobs.read_blob(&key).await?;
            let entry_path = format!("{}/{filename}", map.id);
            write_stored_entry(&mut zip, &entry_path, &bytes)?;
        }

        let writer = zip.finish()?;
        let archive = writer.finish()?;
        Ok(archive)
    }

    /// Streams all `Curated Map` bundles matching the optional `Theme` filter one by one into a
    /// spooled ZIP archive.
    ///
    /// Missing Layer blobs log a `tracing::warn!` and are skipped; if 0 Layers could be read for a
    /// map, its `manifest.json` is omitted too. Other storage errors propagate.
    pub async fn export_battlemaps(
        &self,
        theme: Option<&Theme>,
    ) -> Result<SpooledArchive, StorageError> {
        let curated_maps = self
            .catalog
            .list_curated(&CuratedFilter {
                theme: theme.copied(),
                ..Default::default()
            })
            .await?;
        self.package_battlemaps_streaming(&curated_maps).await
    }

    /// Deduplicates `ids` preserving order, skips unknown ids (`StorageError::NotFound`),
    /// propagates other storage errors, and packages resolved maps via the multi-map streaming
    /// path.
    pub async fn export_selected_battlemaps(
        &self,
        ids: &[String],
    ) -> Result<SpooledArchive, StorageError> {
        let mut selected_maps = Vec::new();
        let mut seen = HashSet::new();

        for id in ids {
            if !seen.insert(id.as_str()) {
                continue;
            }
            match self.catalog.get_curated(id).await {
                Ok(map) => selected_maps.push(map),
                Err(StorageError::NotFound(_)) => {}
                Err(err) => return Err(err),
            }
        }

        self.package_battlemaps_streaming(&selected_maps).await
    }

    /// Packages `curated_maps` one Layer blob at a time into a spooled ZIP archive.
    async fn package_battlemaps_streaming(
        &self,
        curated_maps: &[CuratedMap],
    ) -> Result<SpooledArchive, StorageError> {
        let mut zip = zip::ZipWriter::new(self.spool.create().await?);

        for map in curated_maps {
            let layers = resolve_included_layers(map);
            let mut manifest_written = false;

            for (key, filename) in layers {
                let bytes = match self.blobs.read_blob(&key).await {
                    Ok(bytes) => bytes,
                    Err(StorageError::NotFound(msg)) => {
                        tracing::warn!(
                            map_id = %map.id,
                            layer = %filename,
                            error = %msg,
                            "Skipping missing Layer blob in Battlemap ZIP export"
                        );
                        continue;
                    }
                    Err(err) => return Err(err),
                };

                if !manifest_written {
                    let manifest_bytes = serde_json::to_vec_pretty(map)?;
                    let manifest_path = format!("{}/manifest.json", map.id);
                    write_stored_entry(&mut zip, &manifest_path, &manifest_bytes)?;
                    manifest_written = true;
                }

                let entry_path = format!("{}/{filename}", map.id);
                write_stored_entry(&mut zip, &entry_path, &bytes)?;
            }

            if !manifest_written {
                tracing::warn!(
                    map_id = %map.id,
                    "Omitting map from Battlemap ZIP export because no Layer blobs could be read"
                );
            }
        }

        let writer = zip.finish()?;
        let archive = writer.finish()?;
        Ok(archive)
    }
}
