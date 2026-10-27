//! Catalog and staging library operations over the decoupled catalog and blob ports.
//!
//! These are the operations the deleted monolithic storage repository used to own: staged
//! candidate edits (layers, grid, split, regroup), curated map edits and deletion, and the image
//! bytes for every `/api/image/...` route. Every method is expressed with the existing
//! [`CatalogRepository`] and [`BlobStorage`] port methods under the canonical [`BlobKey`] layout
//! (`staging/{id}/{index}.png`, `curated/{id}/{index}_{role}.png`), so the HTTP behaviour is
//! preserved without a single filesystem path.

use std::sync::Arc;

use crate::domain::{
    infer_bundle_layer_roles, normalize_tags, synthesize_map_sections, validate_layers_for_curation,
    Candidate, CaptionGenerator, CuratedMap, CuratedMapUpdate, Layer, LayerRole, RejectionReason,
};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::{CandidateFilter, CatalogRepository};
use crate::ports::storage_error::StorageError;
use crate::services::sections::ground_dimensions;

/// Application service for candidate/curated library mutations and image reads.
#[derive(Clone)]
pub struct LibraryService {
    catalog: Arc<dyn CatalogRepository + Send + Sync>,
    blobs: Arc<dyn BlobStorage + Send + Sync>,
}

impl LibraryService {
    /// Creates a service reading and writing through `catalog` and `blobs`.
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        Self { catalog, blobs }
    }

    /// Canonical staging address of layer `index` of candidate `id`.
    fn staging_key(id: &str, index: usize) -> BlobKey {
        BlobKey::StagingLayer {
            id: id.to_string(),
            index,
        }
    }

    /// Canonical curated address of `layer` inside the bundle of `id`.
    fn curated_key(id: &str, index: usize, layer: &Layer) -> BlobKey {
        BlobKey::CuratedLayer {
            id: id.to_string(),
            index,
            role: layer.role,
            elevation: layer.elevation,
        }
    }

    /// Decodes the pixel dimensions of `bytes`, or `None` when the image is unreadable.
    fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    }

    /// Deletes every blob below the directory `prefix`.
    async fn purge_blobs_under(&self, prefix: BlobKey) -> Result<(), StorageError> {
        for key in self.blobs.list_prefix(&prefix).await? {
            self.blobs.delete_blob(&key).await?;
        }
        Ok(())
    }

    /// Deletes every blob below the staging directory of `id`.
    async fn purge_staging_blobs(&self, id: &str) -> Result<(), StorageError> {
        self.purge_blobs_under(BlobKey::StagingDir {
            id: Some(id.to_string()),
        })
        .await
    }

    /// Read the staging image bytes and display filename of candidate `id`.
    pub async fn staging_image(&self, id: &str) -> Result<(Vec<u8>, String), StorageError> {
        let candidate = self.catalog.get_candidate(id).await?;
        let index = candidate
            .resolved_layers()
            .first()
            .map(|layer| layer.index)
            .unwrap_or(0);
        let bytes = self.blobs.read_blob(&Self::staging_key(id, index)).await?;
        Ok((bytes, candidate.resolved_filename()))
    }

    /// Read the bytes and display filename of layer `index` of staged candidate `id`.
    pub async fn staging_layer_image(
        &self,
        id: &str,
        index: usize,
    ) -> Result<(Vec<u8>, Option<String>), StorageError> {
        let candidate = self.catalog.get_candidate(id).await?;
        let bytes = self.blobs.read_blob(&Self::staging_key(id, index)).await?;
        let filename = candidate
            .resolved_layers()
            .into_iter()
            .find(|layer| layer.index == index)
            .map(|layer| layer.filename)
            .or_else(|| (index == 0).then(|| candidate.resolved_filename()));
        Ok((bytes, filename))
    }

    /// Read the bytes and display filename of the Ground layer of curated map `id`.
    pub async fn curated_ground_image(
        &self,
        id: &str,
    ) -> Result<(Vec<u8>, Option<String>), StorageError> {
        let map = self.catalog.get_curated(id).await?;
        let layer = map
            .layers
            .iter()
            .find(|layer| layer.role == LayerRole::Ground)
            .or_else(|| map.layers.first())
            .ok_or_else(|| {
                StorageError::NotFound(format!("Curated map '{id}' has no layer to serve"))
            })?;
        let bytes = self
            .blobs
            .read_blob(&Self::curated_key(id, layer.index, layer))
            .await?;
        Ok((bytes, Some(layer.filename.clone())))
    }

    /// Read the bytes and display filename of layer `index` of curated map `id`.
    pub async fn curated_layer_image(
        &self,
        id: &str,
        index: usize,
    ) -> Result<(Vec<u8>, Option<String>), StorageError> {
        let map = self.catalog.get_curated(id).await?;
        let layer = map
            .layers
            .iter()
            .find(|layer| layer.index == index)
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Curated image layer {index} for '{id}' not found"
                ))
            })?;
        let bytes = self
            .blobs
            .read_blob(&Self::curated_key(id, layer.index, layer))
            .await?;
        Ok((bytes, Some(layer.filename.clone())))
    }

    /// Replace the layer assignments of staged candidate `id`, persisting the manifest.
    pub async fn update_staging_layers(
        &self,
        id: &str,
        layers: Vec<Layer>,
    ) -> Result<Candidate, StorageError> {
        let mut candidate = self.catalog.get_candidate(id).await?;
        if layers.is_empty() {
            return Err(StorageError::InvalidData(
                "Candidate layers cannot be empty".into(),
            ));
        }
        candidate.layers = layers;
        self.catalog.save_candidate(&candidate).await?;
        Ok(candidate)
    }

    /// Replace the grid configuration of staged candidate `id`, persisting the manifest.
    pub async fn update_staging_grid(
        &self,
        id: &str,
        grid: crate::domain::GridConfig,
    ) -> Result<Candidate, StorageError> {
        let mut candidate = self.catalog.get_candidate(id).await?;
        candidate.grid = grid;
        self.catalog.save_candidate(&candidate).await?;
        Ok(candidate)
    }

    /// Split multi-layer staged bundle `id` into standalone `{id}_{n}` candidates.
    ///
    /// Each child carries `parent_group_id: Some(id)`, its own layer re-indexed to `0`, and the
    /// source layer blob moved to `staging/{child}/0.png`. The parent manifest and blobs are
    /// removed only after every child was written.
    pub async fn split_staging_bundle(&self, id: &str) -> Result<Vec<Candidate>, StorageError> {
        let candidate = self.catalog.get_candidate(id).await?;
        let layers = candidate.resolved_layers();
        if layers.len() <= 1 {
            return Err(StorageError::InvalidData(format!(
                "Cannot split single-layer candidate '{id}'"
            )));
        }

        let mut layer_images = Vec::with_capacity(layers.len());
        for layer in &layers {
            let bytes = self
                .blobs
                .read_blob(&Self::staging_key(id, layer.index))
                .await
                .map_err(|err| match err {
                    StorageError::NotFound(_) => StorageError::NotFound(format!(
                        "Layer image '{}' not found in bundle '{id}'",
                        layer.filename
                    )),
                    other => other,
                })?;
            layer_images.push(bytes);
        }

        let mut split_candidates = Vec::with_capacity(layers.len());
        for (idx, (layer, bytes)) in layers.iter().zip(layer_images).enumerate() {
            let child_id = format!("{id}_{idx}");
            let child_img_filename = "0.png";
            let dims = if layer.dimensions != (0, 0) {
                layer.dimensions
            } else {
                candidate.dimensions
            };

            self.blobs
                .write_blob(&Self::staging_key(&child_id, 0), &bytes)
                .await?;

            let child_layer = Layer {
                index: 0,
                filename: child_img_filename.to_string(),
                role: LayerRole::Ground,
                elevation: Some(0),
                caption: layer.caption.clone(),
                source_url: layer
                    .source_url
                    .clone()
                    .or_else(|| candidate.source_url.clone()),
                dimensions: dims,
            };

            let child_candidate = Candidate {
                id: child_id.clone(),
                source: candidate.source,
                filename: format!("{child_id}/{child_img_filename}"),
                dimensions: dims,
                created_at: candidate.created_at,
                source_url: child_layer.source_url.clone(),
                title: candidate.title.clone(),
                author: candidate.author.clone(),
                parsed_gridless: candidate.parsed_gridless,
                parsed_dimensions: candidate.parsed_dimensions,
                grid: candidate.grid,
                layers: vec![child_layer],
                parent_group_id: Some(id.to_string()),
            };

            self.catalog.save_candidate(&child_candidate).await?;
            split_candidates.push(child_candidate);
        }

        self.purge_staging_blobs(id).await?;
        self.catalog.delete_candidate(id).await?;
        Ok(split_candidates)
    }

    /// Regroup every staged candidate grouped under `parent_id` back into one multi-layer bundle.
    ///
    /// Layer roles are re-inferred from the children's captions exactly as the first ingestion of
    /// the parent bundle did, so a split/regroup round trip restores the original assignments.
    pub async fn regroup_staging_bundle(&self, parent_id: &str) -> Result<Candidate, StorageError> {
        let prefix = format!("{parent_id}_");
        let mut matching: Vec<(usize, Candidate)> = Vec::new();
        for candidate in self
            .catalog
            .list_candidates(&CandidateFilter::default())
            .await?
        {
            let matches_parent = candidate.parent_group_id.as_deref() == Some(parent_id)
                || candidate
                    .id
                    .strip_prefix(&prefix)
                    .map(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false);
            if !matches_parent {
                continue;
            }
            let suffix_index = candidate
                .id
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .unwrap_or(matching.len());
            matching.push((suffix_index, candidate));
        }

        if matching.is_empty() {
            return Err(StorageError::NotFound(format!(
                "No split staging candidates found for parent bundle '{parent_id}'"
            )));
        }
        matching.sort_by_key(|(index, _)| *index);

        let captions: Vec<Option<String>> = matching
            .iter()
            .map(|(_, candidate)| {
                candidate
                    .layers
                    .first()
                    .and_then(|layer| layer.caption.clone())
            })
            .collect();
        let inferred_roles = infer_bundle_layer_roles(&captions, matching[0].1.title.as_deref());

        let mut regrouped_layers: Vec<Layer> = Vec::with_capacity(matching.len());
        for (new_index, ((_, child), (role, elevation))) in
            matching.iter().zip(inferred_roles).enumerate()
        {
            let child_layers = child.resolved_layers();
            let first_layer = child_layers.first().ok_or_else(|| {
                StorageError::InvalidData(format!(
                    "Child candidate '{}' has no layer to regroup",
                    child.id
                ))
            })?;
            let bytes = self
                .blobs
                .read_blob(&Self::staging_key(&child.id, first_layer.index))
                .await
                .map_err(|err| match err {
                    StorageError::NotFound(_) => StorageError::NotFound(format!(
                        "Child image '{}' not found in '{}'",
                        first_layer.filename, child.id
                    )),
                    other => other,
                })?;
            let layer_filename = format!("{new_index}.png");
            self.blobs
                .write_blob(&Self::staging_key(parent_id, new_index), &bytes)
                .await?;

            regrouped_layers.push(Layer {
                index: new_index,
                filename: layer_filename,
                role,
                elevation,
                caption: first_layer.caption.clone(),
                source_url: first_layer
                    .source_url
                    .clone()
                    .or_else(|| child.source_url.clone()),
                dimensions: first_layer.dimensions,
            });
        }

        for (_, child) in &matching {
            self.purge_staging_blobs(&child.id).await?;
            self.catalog.delete_candidate(&child.id).await?;
        }

        let first_candidate = &matching[0].1;
        let first_file = regrouped_layers
            .first()
            .map(|layer| layer.filename.clone())
            .unwrap_or_else(|| "0.png".to_string());
        let first_dimensions = regrouped_layers
            .first()
            .map(|layer| layer.dimensions)
            .unwrap_or(first_candidate.dimensions);

        let regrouped = Candidate {
            id: parent_id.to_string(),
            source: first_candidate.source,
            filename: format!("{parent_id}/{first_file}"),
            dimensions: first_dimensions,
            created_at: first_candidate.created_at,
            source_url: first_candidate.source_url.clone(),
            title: first_candidate.title.clone(),
            author: first_candidate.author.clone(),
            parsed_gridless: first_candidate.parsed_gridless,
            parsed_dimensions: first_candidate.parsed_dimensions,
            grid: first_candidate.grid,
            layers: regrouped_layers,
            parent_group_id: None,
        };
        self.catalog.save_candidate(&regrouped).await?;
        Ok(regrouped)
    }

    /// Apply a partial metadata/layer update to curated map `id`, preserving omitted fields.
    ///
    /// Layer payloads may omit provenance fields, which are carried over from the existing layer
    /// at the same index (or filename); the included layers are then re-indexed into the canonical
    /// `{index}_{role}.png` filenames and their blobs rewritten at the matching [`BlobKey`].
    pub async fn update_curated(
        &self,
        id: &str,
        update: CuratedMapUpdate,
    ) -> Result<CuratedMap, StorageError> {
        let mut map = self.catalog.get_curated(id).await?;
        let old_description = map.description.clone();
        let existing_layers = map.layers.clone();
        let existing_keys = self
            .blobs
            .list_prefix(&BlobKey::CuratedDir {
                id: Some(id.to_string()),
            })
            .await?;

        let CuratedMapUpdate {
            themes,
            tags,
            description,
            layers,
            grid,
            sections,
        } = update;

        let mut tags_updated = false;
        if let Some(tags) = tags {
            map.tags = normalize_tags(tags);
            tags_updated = true;
        }
        // Legacy `themes` payloads fold into the single `tags` list. An empty or whitespace-only
        // array is a no-op so tags-only clients can still PATCH.
        if let Some(themes) = themes {
            let legacy_tags = normalize_tags(themes);
            if !legacy_tags.is_empty() {
                map.tags = normalize_tags(map.tags.into_iter().chain(legacy_tags));
                tags_updated = true;
            }
        }
        if tags_updated {
            map.validate_tags().map_err(StorageError::InvalidData)?;
        }

        if let Some(description) = description {
            let trimmed = description.trim();
            map.description = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        if let Some(new_layers) = layers {
            validate_layers_for_curation(&new_layers).map_err(StorageError::InvalidData)?;

            let mut effective_layers: Vec<Layer> = if new_layers.iter().any(|l| l.role.is_excluded())
            {
                new_layers
                    .into_iter()
                    .filter(|l| l.role.is_included())
                    .collect()
            } else {
                new_layers
            };

            for layer in &mut effective_layers {
                let existing = existing_layers
                    .iter()
                    .find(|existing| existing.index == layer.index)
                    .or_else(|| {
                        existing_layers
                            .iter()
                            .find(|existing| existing.filename == layer.filename)
                    });
                let Some(existing) = existing else { continue };
                if layer.source_url.is_none() {
                    layer.source_url = existing.source_url.clone();
                }
                if layer.caption.is_none() {
                    layer.caption = existing.caption.clone();
                }
            }

            let mut validation_map = map.clone();
            validation_map.layers = effective_layers.clone();
            validation_map
                .validate_layers()
                .map_err(StorageError::InvalidData)?;

            let mut rewritten: Vec<(Layer, Vec<u8>)> = Vec::with_capacity(effective_layers.len());
            for layer in &effective_layers {
                let bytes = self
                    .resolve_curated_layer_bytes(
                        id,
                        layer,
                        &existing_layers,
                        &existing_keys,
                        effective_layers.len(),
                    )
                    .await?;
                let dimensions = if layer.dimensions != (0, 0) {
                    layer.dimensions
                } else {
                    Self::image_dimensions(&bytes).unwrap_or(layer.dimensions)
                };
                let mut layer = layer.clone();
                layer.dimensions = dimensions;
                rewritten.push((layer, bytes));
            }

            let mut updated_layers = Vec::with_capacity(rewritten.len());
            let mut written_keys: Vec<BlobKey> = Vec::with_capacity(rewritten.len());
            for (new_index, (mut layer, bytes)) in rewritten.into_iter().enumerate() {
                layer.index = new_index;
                layer.filename = Layer::format_filename(new_index, layer.role, layer.elevation, "png");
                let key = Self::curated_key(id, new_index, &layer);
                self.blobs.write_blob(&key, &bytes).await?;
                written_keys.push(key);
                updated_layers.push(layer);
            }

            // `BlobKey` identity is the canonical path, so a key re-derived from a file name by
            // `list_prefix` (e.g. `{1, Roof, None}` for `1_roof.png`) matches the constructed key
            // it was just written under (e.g. `{1, Roof, Some(1)}`) and the fresh blob survives.
            for old_key in &existing_keys {
                if !written_keys.contains(old_key) {
                    self.blobs.delete_blob(old_key).await?;
                }
            }

            if map.dimensions == (0, 0) {
                map.dimensions = updated_layers
                    .first()
                    .map(|layer| layer.dimensions)
                    .unwrap_or((0, 0));
            }
            map.layers = updated_layers;
        }

        let grid_requested = grid.is_some();
        let mut grid_dimensions: Option<(u32, u32)> = None;
        if let Some(new_grid) = grid {
            // Same resolution the readiness audit uses: sections are cropped out of the decoded
            // Ground pixels, so the grid is validated against them rather than stale metadata.
            let ground_image = match self.curated_ground_image(id).await {
                Ok((bytes, _)) => image::load_from_memory(&bytes).ok(),
                Err(_) => None,
            };
            let (ground_w, ground_h) = ground_dimensions(&map, ground_image.as_ref());
            grid_dimensions = Some((ground_w, ground_h));
            if !new_grid.is_curation_eligible(ground_w, ground_h) {
                return Err(StorageError::InvalidData(
                    "New grid configuration yields fewer than 16×16 full Grid Cells".into(),
                ));
            }
            map.grid = new_grid;
        }

        if let Some(sections) = sections {
            map.sections = sections;
        } else if grid_requested {
            let (ground_w, ground_h) =
                grid_dimensions.unwrap_or_else(|| ground_dimensions(&map, None));
            let origins = map.grid.section_origins(ground_w, ground_h);
            map.sections = synthesize_map_sections(&origins, map.description.as_deref());
        }

        if old_description != map.description {
            let primary_index = map
                .layers
                .iter()
                .position(|layer| layer.role == LayerRole::Ground)
                .unwrap_or(0);
            if let Some(first) = map.layers.get_mut(primary_index) {
                first.caption = map.description.clone();
            }
            if map.sections.len() == 1 {
                if let Some(description) = &map.description {
                    map.sections[0].description = description.clone();
                }
            }
        }
        self.ensure_curated_caption(&mut map);

        self.catalog.save_curated(&map).await?;
        Ok(map)
    }

    /// Resolve the bytes backing `layer` in an update payload by walking the same fallbacks the
    /// filesystem repository used: the payload's own `filename`, the layer's own address, the
    /// existing layer at that index or position, any blob whose filename starts with `{index}_`,
    /// the existing layer with the same role, and finally the sole existing layer of a
    /// single-layer map.
    ///
    /// The payload `filename` is consulted first because a PATCH may reassign existing files
    /// across indices (e.g. index 0 carrying `1_upper_1.png` with role `ground`); the payload
    /// names the source image, the index/role only name its destination.
    async fn resolve_curated_layer_bytes(
        &self,
        id: &str,
        layer: &Layer,
        existing_layers: &[Layer],
        existing_keys: &[BlobKey],
        effective_layer_count: usize,
    ) -> Result<Vec<u8>, StorageError> {
        if !layer.filename.is_empty() {
            let named = format!("curated/{id}/{}", layer.filename);
            if let Some(key) = existing_keys
                .iter()
                .find(|key| key.to_path() == named)
            {
                if let Ok(bytes) = self.blobs.read_blob(key).await {
                    return Ok(bytes);
                }
            }
        }

        let direct = Self::curated_key(id, layer.index, layer);
        if let Ok(bytes) = self.blobs.read_blob(&direct).await {
            return Ok(bytes);
        }

        let mut candidates: Vec<BlobKey> = Vec::new();
        if let Some(existing) = existing_layers.iter().find(|l| l.index == layer.index) {
            candidates.push(Self::curated_key(id, existing.index, existing));
        }
        if let Some(existing) = existing_layers.get(layer.index) {
            candidates.push(Self::curated_key(id, existing.index, existing));
        }
        let index_prefix = format!("curated/{id}/{}_", layer.index);
        candidates.extend(
            existing_keys
                .iter()
                .filter(|key| key.to_path().starts_with(&index_prefix))
                .cloned(),
        );
        if let Some(existing) = existing_layers.iter().find(|l| l.role == layer.role) {
            candidates.push(Self::curated_key(id, existing.index, existing));
        }
        if effective_layer_count == 1 && existing_layers.len() == 1 {
            let existing = &existing_layers[0];
            candidates.push(Self::curated_key(id, existing.index, existing));
        }

        for key in candidates {
            if let Ok(bytes) = self.blobs.read_blob(&key).await {
                return Ok(bytes);
            }
        }

        Err(StorageError::NotFound(format!(
            "Layer image source file not found for layer {layer:?}"
        )))
    }

    /// Ensures a curated map carries a primary layer caption and a map description.
    fn ensure_curated_caption(&self, map: &mut CuratedMap) {
        let has_caption = map
            .layers
            .first()
            .and_then(|layer| layer.caption.as_ref())
            .is_some();
        if !has_caption {
            let caption = CaptionGenerator::generate_for_curated(map, None);
            if let Some(first) = map.layers.first_mut() {
                first.caption = Some(caption);
            } else {
                map.layers.push(Layer {
                    index: 0,
                    filename: "0_ground.png".to_string(),
                    role: LayerRole::Ground,
                    elevation: Some(0),
                    caption: Some(caption),
                    source_url: None,
                    dimensions: map.dimensions,
                });
            }
        }
        if map.description.is_none() {
            let primary_caption = map
                .layers
                .iter()
                .find(|layer| layer.role == LayerRole::Ground)
                .or_else(|| map.layers.first())
                .and_then(|layer| layer.caption.as_ref())
                .filter(|caption| !caption.trim().is_empty());
            if let Some(caption) = primary_caption {
                map.description = Some(caption.clone());
            }
        }
    }

    /// Delete curated map `id`, its layer blobs, and append its rejection record.
    pub async fn delete_curated(
        &self,
        id: &str,
        reason: RejectionReason,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        self.catalog.get_curated(id).await?;

        self.purge_blobs_under(BlobKey::CuratedDir {
            id: Some(id.to_string()),
        })
        .await?;
        self.catalog.delete_curated(id).await?;
        self.catalog.log_rejection(id, reason, notes).await?;
        Ok(())
    }

    /// Persist a generated benchmark image as a regular staging candidate.
    ///
    /// Legacy generated manifests were raw benchmark metadata; the flat catalog only reads
    /// [`Candidate`] manifests, so the output is stored as one `staging/{id}/0.png` layer plus a
    /// real candidate manifest.
    pub async fn save_generated_candidate(
        &self,
        metadata: &crate::domain::BenchmarkMetadata,
        image_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let mut candidate = metadata.to_candidate();
        candidate.filename = format!("{}/0.png", candidate.id);
        candidate.layers = vec![Layer::new(
            0,
            "0.png",
            LayerRole::Ground,
            Some(0),
            candidate.dimensions,
        )];
        self.blobs
            .write_blob(&Self::staging_key(&candidate.id, 0), image_bytes)
            .await?;
        self.catalog.save_candidate(&candidate).await
    }
}
