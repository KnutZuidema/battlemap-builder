//! LoRA dataset export and readiness audit over the catalog and blob ports (ADR-0007).
//!
//! Every `Map Section` of a curated map becomes one `1024×1024` sample whose companion `.txt`
//! carries the `Trigger Phrase` plus that section's perspective-neutral `Physical Description`.
//! The archive is built entirely in memory (DEFLATE, no temporary files) from the Ground layer
//! bytes read through [`BlobStorage`].
//!
//! The sample planning, cropping and readiness classification are pure functions shared with the
//! legacy monolithic storage repository, which resolved Ground layer bytes
//! from the filesystem instead of the blob port. Both callers therefore ship identical archives.

use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};

use crate::domain::{
    crop_section_box, is_perspective_neutral, sample_description, synthesize_map_sections,
    BlockingIssue, CaptionGenerator, CuratedMap, DatasetIssue, DatasetIssueCode, DatasetReadiness,
    DatasetSnapshot, FreezeProgressUpdate, GridConfig, LayerRole, MapDatasetReadiness, MapSection,
    Theme, TRIGGER_PHRASE,
};
use crate::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use crate::ports::catalog_repository::{CatalogRepository, CuratedFilter};
use crate::ports::storage_error::StorageError;

/// One planned LoRA dataset sample: a `Map Section` box, the `Physical Description` it is
/// captioned with (resolved once through [`sample_description`]), and any defect that makes the
/// sample unfit for a `Training Run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatasetSamplePlan {
    pub(crate) col: u32,
    pub(crate) row: u32,
    pub(crate) description: Option<String>,
    pub(crate) issues: Vec<DatasetIssue>,
}

impl DatasetSamplePlan {
    pub(crate) fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Builds the readiness report of a single curated map.
pub(crate) fn map_readiness(
    map: &CuratedMap,
    sections: usize,
    ready_sections: usize,
    issues: Vec<DatasetIssue>,
) -> MapDatasetReadiness {
    MapDatasetReadiness {
        id: map.id.clone(),
        theme: map.primary_theme().to_string(),
        sections,
        ready_sections,
        issues,
    }
}

/// Renders a description for an issue detail without dumping an entire paragraph into the report.
pub(crate) fn describe_for_issue(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(MAX_CHARS).collect();
    format!("{head}…")
}

/// Enumerates the LoRA dataset samples a curated map contributes, resolving every section's
/// `Physical Description` through the one shared policy used by both the ZIP export and the
/// readiness audit.
///
/// Maps persisted before Map Sections existed fall back to the map's full-grid lattice with
/// the map-level `Physical Description` reused for every section. Defects are reported instead
/// of raised: the export logs them and skips only the affected sections.
pub(crate) fn plan_dataset_samples(
    map: &CuratedMap,
    image: &image::DynamicImage,
) -> (Vec<DatasetSamplePlan>, Vec<DatasetIssue>) {
    let mut issues = Vec::new();

    let synthesized_sections;
    let sections: &[MapSection] = if !map.sections.is_empty() {
        &map.sections
    } else {
        let origins = map.grid.section_origins(image.width(), image.height());
        if origins.is_empty() {
            issues.push(DatasetIssue::new(
                DatasetIssueCode::GridBelowMinimum,
                format!(
                    "{}px grid yields no full 16×16 Grid Cell window in a {}×{} Ground layer",
                    map.grid.cell_size_px,
                    image.width(),
                    image.height()
                ),
            ));
            return (Vec::new(), issues);
        }
        synthesized_sections = synthesize_map_sections(&origins, map.description.as_deref());
        &synthesized_sections
    };

    let mut samples = Vec::with_capacity(sections.len());
    for section in sections {
        let mut section_issues = Vec::new();

        if crop_section_box(image, &map.grid, section.col, section.row).is_none() {
            section_issues.push(DatasetIssue::new(
                DatasetIssueCode::SectionUnresolvable,
                format!(
                    "Map Section ({}, {}) does not fit inside the {}×{} Ground layer",
                    section.col,
                    section.row,
                    image.width(),
                    image.height()
                ),
            ));
        }

        let description = sample_description(section, map.description.as_deref());
        match description {
            Some(text) if !is_perspective_neutral(text) => {
                section_issues.push(DatasetIssue::new(
                    DatasetIssueCode::PhysicalDescriptionNotNeutral,
                    format!(
                        "Map Section ({}, {}) description holds camera, perspective, art-style or grid directives: \"{}\"",
                        section.col,
                        section.row,
                        describe_for_issue(text)
                    ),
                ));
            }
            None => {
                section_issues.push(DatasetIssue::new(
                    DatasetIssueCode::PhysicalDescriptionMissing,
                    format!(
                        "Map Section ({}, {}) resolves to no Physical Description",
                        section.col, section.row
                    ),
                ));
            }
            _ => {}
        }

        samples.push(DatasetSamplePlan {
            col: section.col,
            row: section.row,
            description: description.map(str::to_string),
            issues: section_issues,
        });
    }

    (samples, issues)
}

/// Crops the exact `16×16` Grid Cell box of Map Section `(col, row)` and resamples it to
/// `1024×1024` PNG bytes without letterbox padding bars.
///
/// Returns [`StorageError::InvalidData`] when the exact box does not fit inside the image, so
/// the LoRA ZIP export skips the section (with a warning) instead of a scale-changing crop.
pub(crate) fn crop_and_resample_section(
    dynamic_img: &image::DynamicImage,
    grid: &GridConfig,
    col: u32,
    row: u32,
) -> Result<Vec<u8>, StorageError> {
    if dynamic_img.width() == 0 || dynamic_img.height() == 0 {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Image dimensions are 0",
        )));
    }

    let cropped = crop_section_box(dynamic_img, grid, col, row).ok_or_else(|| {
        StorageError::InvalidData(format!(
            "16x16 Map Section ({col}, {row}) does not fit inside the {}x{} image",
            dynamic_img.width(),
            dynamic_img.height()
        ))
    })?;
    let cropped_rgba = cropped.to_rgba8();

    let mut resizer = Resizer::new();
    let mut scaled_rgba = image::RgbaImage::new(1024, 1024);
    let resize_options =
        ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));

    resizer
        .resize(&cropped_rgba, &mut scaled_rgba, &resize_options)
        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    scaled_rgba
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

    Ok(png_bytes)
}


/// Audits one curated map against the LoRA dataset contract, classifying the already-resolved
/// Ground layer bytes exactly like [`create_lora_zip`] does.
pub(crate) fn audit_map_readiness(
    map: &CuratedMap,
    ground_bytes: Result<Vec<u8>, StorageError>,
) -> MapDatasetReadiness {
    let ground_bytes = match ground_bytes {
        Ok(bytes) => bytes,
        Err(err) => {
            return map_readiness(
                map,
                0,
                0,
                vec![DatasetIssue::new(
                    DatasetIssueCode::GroundLayerMissing,
                    err.to_string(),
                )],
            )
        }
    };

    let image = match image::load_from_memory(&ground_bytes) {
        Ok(image) => image,
        Err(err) => {
            return map_readiness(
                map,
                0,
                0,
                vec![DatasetIssue::new(
                    DatasetIssueCode::GroundImageUnreadable,
                    err.to_string(),
                )],
            )
        }
    };

    let (samples, mut issues) = plan_dataset_samples(map, &image);
    let sections = samples.len();
    let ready_sections = samples.iter().filter(|sample| sample.is_ready()).count();
    for sample in samples {
        issues.extend(sample.issues);
    }

    map_readiness(map, sections, ready_sections, issues)
}

/// Blob key of a curated map's `Ground` layer image, falling back to its first layer.
///
/// Mirrors the layer resolution of the storage repository, so the export reads exactly the image
/// the catalog's `layers` metadata describes.
fn curated_ground_blob_key(map: &CuratedMap) -> BlobKey {
    let layer = map
        .layers
        .iter()
        .find(|l| l.role == LayerRole::Ground)
        .or_else(|| map.layers.first());

    match layer {
        Some(layer) => BlobKey::CuratedLayer {
            id: map.id.clone(),
            index: layer.index,
            role: layer.role,
            elevation: layer.elevation,
        },
        None => BlobKey::CuratedLayer {
            id: map.id.clone(),
            index: 0,
            role: LayerRole::Ground,
            elevation: Some(0),
        },
    }
}

static SNAPSHOT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Error returned when attempting to freeze a dataset snapshot.
#[derive(Debug, thiserror::Error)]
pub enum FreezeDatasetError {
    #[error("Dataset scope is not ready: {summary}")]
    NotReady {
        summary: String,
        blocking_issues: Vec<BlockingIssue>,
        readiness: DatasetReadiness,
    },
    #[error("Dataset snapshot freeze was cancelled")]
    Cancelled,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Error returned when attempting to delete a dataset snapshot.
#[derive(Debug, thiserror::Error)]
pub enum DeleteDatasetError {
    #[error("Dataset snapshot '{0}' is referenced by one or more training runs")]
    Referenced(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Service exporting curated maps as LoRA training archives and auditing dataset readiness.
///
/// Stateless: it borrows the catalog and blob ports, so handlers construct it per request from
/// cheap `Arc` clones.
#[derive(Clone)]
pub struct DatasetExportService {
    catalog: Arc<dyn CatalogRepository>,
    blobs: Arc<dyn BlobStorage>,
}

impl DatasetExportService {
    /// Creates a service reading curated maps from `catalog` and layer images from `blobs`.
    pub fn new(catalog: Arc<dyn CatalogRepository>, blobs: Arc<dyn BlobStorage>) -> Self {
        Self { catalog, blobs }
    }

    /// Exports every curated map (optionally restricted to `theme`) as a LoRA ZIP archive.
    pub async fn export_lora_dataset(
        &self,
        theme: Option<&Theme>,
    ) -> Result<Vec<u8>, StorageError> {
        let curated_maps = self.list_curated(theme).await?;
        self.create_lora_zip(&curated_maps).await
    }

    /// Exports the curated maps named by `ids`, in that order, ignoring repeated or unknown ids.
    pub async fn export_selected_lora_dataset(
        &self,
        ids: &[String],
    ) -> Result<Vec<u8>, StorageError> {
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

        self.create_lora_zip(&selected_maps).await
    }

    /// Audits the curated store against the LoRA dataset export contract, mirroring exactly what
    /// the dataset ZIP ships per Map Section.
    pub async fn audit_dataset_readiness(
        &self,
        theme: Option<&Theme>,
    ) -> Result<DatasetReadiness, StorageError> {
        let curated_maps = self.list_curated(theme).await?;
        let mut reports = Vec::with_capacity(curated_maps.len());
        for map in &curated_maps {
            let ground_bytes = self.blobs.read_blob(&curated_ground_blob_key(map)).await;
            reports.push(audit_map_readiness(map, ground_bytes));
        }
        Ok(DatasetReadiness::from_maps(reports))
    }

    async fn list_curated(&self, theme: Option<&Theme>) -> Result<Vec<CuratedMap>, StorageError> {
        self.catalog
            .list_curated(&CuratedFilter {
                theme: theme.copied(),
                ..Default::default()
            })
            .await
    }

    /// Resolves every map's Ground layer bytes and packages them into a LoRA ZIP archive.
    /// Streams maps one-by-one to bound input image memory to O(1) maps.
    async fn create_lora_zip(
        &self,
        curated_maps: &[CuratedMap],
    ) -> Result<Vec<u8>, StorageError> {
        let (bytes, _) = self
            .package_lora_zip_streaming(curated_maps, |_, _, _, _, _| true)
            .await?;
        Ok(bytes)
    }

    /// Streams maps one by one, crops/resamples sections, and writes into a LoRA ZIP archive.
    /// Drops decoded images after processing each map to keep peak memory usage to O(1) maps.
    async fn package_lora_zip_streaming<F>(
        &self,
        curated_maps: &[CuratedMap],
        mut on_section: F,
    ) -> Result<(Vec<u8>, usize), StorageError>
    where
        F: FnMut(usize, usize, &CuratedMap, u32, u32) -> bool,
    {
        let mut buffer = std::io::Cursor::new(Vec::new());
        let mut completed_samples = 0;

        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            for (map_idx, map) in curated_maps.iter().enumerate() {
                let theme_str = map.primary_theme();

                let ground_bytes = match self.blobs.read_blob(&curated_ground_blob_key(map)).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!("Skipping '{}' in LoRA ZIP export: {e}", map.id);
                        continue;
                    }
                };

                let dynamic_img = match image::load_from_memory(&ground_bytes) {
                    Ok(img) => img,
                    Err(e) => {
                        tracing::warn!("Failed to decode ground image for '{}': {e}", map.id);
                        continue;
                    }
                };
                drop(ground_bytes);

                let (samples, plan_issues) = plan_dataset_samples(map, &dynamic_img);
                for issue in &plan_issues {
                    tracing::warn!(
                        "LoRA dataset issue for '{}': {} ({})",
                        map.id,
                        issue.detail,
                        issue.code.as_str()
                    );
                }

                for sample in samples {
                    if !on_section(completed_samples, map_idx, map, sample.col, sample.row) {
                        return Err(StorageError::Io(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "LoRA dataset packaging cancelled",
                        )));
                    }

                    let image_bytes = match crop_and_resample_section(
                        &dynamic_img,
                        &map.grid,
                        sample.col,
                        sample.row,
                    ) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to crop/resample section ({}, {}) for '{}': {e}",
                                sample.col,
                                sample.row,
                                map.id
                            );
                            continue;
                        }
                    };

                    let caption_text = CaptionGenerator::generate_with_theme_str(
                        theme_str,
                        &map.tags,
                        sample.description.as_deref(),
                    );

                    let img_filename = format!(
                        "{theme_str}/{}_c{}_r{}_lora1024.png",
                        map.id, sample.col, sample.row
                    );
                    zip.start_file(&img_filename, options)
                        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;
                    zip.write_all(&image_bytes)
                        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

                    let txt_filename = format!(
                        "{theme_str}/{}_c{}_r{}_lora1024.txt",
                        map.id, sample.col, sample.row
                    );
                    zip.start_file(&txt_filename, options)
                        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;
                    zip.write_all(caption_text.as_bytes())
                        .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

                    completed_samples += 1;
                }
            }

            zip.finish()
                .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;
        }

        Ok((buffer.into_inner(), completed_samples))
    }

    /// Freezes an immutable dataset snapshot for the given scope, gated on dataset readiness.
    pub async fn freeze_dataset_snapshot(
        &self,
        theme: Option<&Theme>,
        custom_id: Option<String>,
    ) -> Result<DatasetSnapshot, FreezeDatasetError> {
        self.freeze_dataset_snapshot_with_progress(theme, custom_id, |_| true).await
    }

    /// Freezes an immutable dataset snapshot for the given scope with granular real-time progress callbacks.
    /// Streams maps one-by-one to reduce peak memory usage and updates progress per section.
    pub async fn freeze_dataset_snapshot_with_progress<F>(
        &self,
        theme: Option<&Theme>,
        custom_id: Option<String>,
        mut on_progress: F,
    ) -> Result<DatasetSnapshot, FreezeDatasetError>
    where
        F: FnMut(FreezeProgressUpdate) -> bool + Send,
    {
        if !on_progress(FreezeProgressUpdate {
            phase: "starting".to_string(),
            completed_maps: 0,
            total_maps: 0,
            completed_samples: 0,
            total_samples: 0,
            current_map: None,
            current_section: None,
            message: "Starting dataset snapshot freeze...".to_string(),
            snapshot: None,
        }) {
            return Err(FreezeDatasetError::Cancelled);
        }

        // 1. Audit readiness with progress reported per map
        let curated_maps = self.list_curated(theme).await?;
        let total_maps = curated_maps.len();
        let mut reports = Vec::with_capacity(total_maps);

        for (idx, map) in curated_maps.iter().enumerate() {
            if !on_progress(FreezeProgressUpdate {
                phase: "auditing".to_string(),
                completed_maps: idx,
                total_maps,
                completed_samples: 0,
                total_samples: 0,
                current_map: Some(map.id.clone()),
                current_section: None,
                message: format!("Auditing readiness: map {} of {} ('{}')", idx + 1, total_maps, map.id),
                snapshot: None,
            }) {
                return Err(FreezeDatasetError::Cancelled);
            }

            let ground_bytes = self.blobs.read_blob(&curated_ground_blob_key(map)).await;
            reports.push(audit_map_readiness(map, ground_bytes));
        }

        let readiness = DatasetReadiness::from_maps(reports);
        if !readiness.ready {
            let blocking_issues = readiness.blocking_issues();
            let summary = if readiness.maps_total == 0 {
                "Dataset scope is not ready: no curated maps exist in the requested scope".to_string()
            } else {
                format!(
                    "Dataset scope is not ready: {} blocking issue(s) across {} map(s)",
                    blocking_issues.iter().map(|b| b.count).sum::<usize>(),
                    readiness.maps_total - readiness.maps_ready
                )
            };
            let _ = on_progress(FreezeProgressUpdate {
                phase: "error".to_string(),
                completed_maps: readiness.maps_ready,
                total_maps,
                completed_samples: 0,
                total_samples: 0,
                current_map: None,
                current_section: None,
                message: summary.clone(),
                snapshot: None,
            });
            return Err(FreezeDatasetError::NotReady {
                summary,
                blocking_issues,
                readiness,
            });
        }

        let total_samples = readiness.sections_ready;
        if !on_progress(FreezeProgressUpdate {
            phase: "packaging".to_string(),
            completed_maps: 0,
            total_maps,
            completed_samples: 0,
            total_samples,
            current_map: None,
            current_section: None,
            message: format!("Starting packaging for {total_maps} maps ({total_samples} samples)..."),
            snapshot: None,
        }) {
            return Err(FreezeDatasetError::Cancelled);
        }

        // 2. Stream zip packaging map-by-map via shared streaming packager
        let (zip_bytes, completed_samples) = match self
            .package_lora_zip_streaming(
                &curated_maps,
                |done, map_idx, map, col, row| {
                    on_progress(FreezeProgressUpdate {
                        phase: "packaging".to_string(),
                        completed_maps: map_idx,
                        total_maps,
                        completed_samples: done,
                        total_samples,
                        current_map: Some(map.id.clone()),
                        current_section: Some((col, row)),
                        message: format!(
                            "Resampling section ({col}, {row}) for '{}' ({}/{} samples)",
                            map.id,
                            done + 1,
                            total_samples
                        ),
                        snapshot: None,
                    })
                },
            )
            .await
        {
            Ok(res) => res,
            Err(StorageError::Io(e)) if e.kind() == std::io::ErrorKind::Interrupted => {
                return Err(FreezeDatasetError::Cancelled);
            }
            Err(e) => return Err(FreezeDatasetError::Storage(e)),
        };

        let sha256 = sha256_digest(&zip_bytes);
        let size_bytes = zip_bytes.len() as u64;

        if !on_progress(FreezeProgressUpdate {
            phase: "finalizing".to_string(),
            completed_maps: total_maps,
            total_maps,
            completed_samples,
            total_samples,
            current_map: None,
            current_section: None,
            message: "Writing frozen dataset snapshot artifact to storage...".to_string(),
            snapshot: None,
        }) {
            return Err(FreezeDatasetError::Cancelled);
        }

        let id = custom_id.unwrap_or_else(|| {
            let theme_slug = theme.map(|t| t.as_str()).unwrap_or("all");
            let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let counter = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
            format!("ds_{theme_slug}_{ts}_{counter:02}")
        });

        let blob_key = BlobKey::DatasetSnapshot { id: id.clone() };
        self.blobs
            .write_blob_with_sha256(&blob_key, &zip_bytes, Some(&sha256))
            .await?;

        let artifact_key = blob_key.to_path();
        let artifact_url = std::env::var("R2_PUBLIC_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .and_then(|u| blob_key.public_url(&u));

        let snapshot = DatasetSnapshot {
            id: id.clone(),
            theme: theme.copied(),
            created_at: chrono::Utc::now(),
            sha256,
            size_bytes,
            maps_count: readiness.maps_ready,
            samples_count: completed_samples,
            trigger_phrase: TRIGGER_PHRASE.to_string(),
            artifact_key,
            artifact_url,
        };

        if let Err(err) = self.catalog.save_dataset_snapshot(&snapshot).await {
            let _ = self.blobs.delete_blob(&blob_key).await;
            return Err(FreezeDatasetError::Storage(err));
        }

        let _ = on_progress(FreezeProgressUpdate {
            phase: "complete".to_string(),
            completed_maps: total_maps,
            total_maps,
            completed_samples,
            total_samples,
            current_map: None,
            current_section: None,
            message: format!("Frozen dataset snapshot '{}' created", snapshot.id),
            snapshot: Some(snapshot.clone()),
        });

        Ok(snapshot)
    }

    /// Loads the snapshot record by id.
    pub async fn get_dataset_snapshot(&self, id: &str) -> Result<DatasetSnapshot, StorageError> {
        self.catalog.get_dataset_snapshot(id).await
    }

    /// Lists snapshot records matching `theme`, ordered newest first.
    pub async fn list_dataset_snapshots(
        &self,
        theme: Option<&Theme>,
    ) -> Result<Vec<DatasetSnapshot>, StorageError> {
        self.catalog.list_dataset_snapshots(theme).await
    }

    /// Reads the raw ZIP archive bytes of the dataset snapshot.
    pub async fn read_dataset_snapshot_archive(&self, id: &str) -> Result<Vec<u8>, StorageError> {
        // Ensure snapshot exists in the catalog before reading blob
        self.catalog.get_dataset_snapshot(id).await?;
        self.blobs
            .read_blob(&BlobKey::DatasetSnapshot {
                id: id.to_string(),
            })
            .await
    }

    /// Deletes the dataset snapshot if unreferenced by any training run.
    pub async fn delete_dataset_snapshot(&self, id: &str) -> Result<(), DeleteDatasetError> {
        // Ensure snapshot exists
        self.catalog.get_dataset_snapshot(id).await?;

        if self.catalog.is_dataset_snapshot_referenced(id).await? {
            return Err(DeleteDatasetError::Referenced(id.to_string()));
        }

        // Delete blob from storage first
        self.blobs
            .delete_blob(&BlobKey::DatasetSnapshot {
                id: id.to_string(),
            })
            .await?;

        // Delete catalog record
        self.catalog.delete_dataset_snapshot(id).await?;
        Ok(())
    }
}
