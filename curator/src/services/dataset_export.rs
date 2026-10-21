//! LoRA dataset export and readiness audit over the catalog and blob ports (ADR-0007).
//!
//! Every `Map Section` of a curated map becomes one `1024×1024` sample whose companion `.txt`
//! carries the `Trigger Phrase` plus that section's perspective-neutral `Physical Description`.
//! The archive is built entirely in memory (DEFLATE, no temporary files) from the Ground layer
//! bytes read through [`BlobStorage`].
//!
//! The sample planning, cropping and readiness classification are pure functions shared with the
//! legacy [`crate::adapters::FilesystemStorage`] repository, which resolves Ground layer bytes
//! from the filesystem instead of the blob port. Both callers therefore ship identical archives.

use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};

use crate::domain::{
    crop_section_box, is_perspective_neutral, sample_description, synthesize_map_sections,
    CaptionGenerator, CuratedMap, DatasetIssue, DatasetIssueCode, DatasetReadiness, GridConfig,
    LayerRole, MapDatasetReadiness, MapSection, Theme,
};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
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

/// Packages curated maps and their already-resolved Ground layer bytes into a LoRA ZIP archive.
///
/// A map whose Ground layer bytes could not be resolved is skipped with a warning, as is a single
/// section whose exact box does not fit. Member names are `{theme}/{id}_c{col}_r{row}_lora1024`
/// with the `.png` sample and its `.txt` caption, whose `Trigger Phrase` is applied exactly once.
pub(crate) fn create_lora_zip(
    curated_maps: impl IntoIterator<Item = (CuratedMap, Result<Vec<u8>, StorageError>)>,
) -> Result<Vec<u8>, StorageError> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (map, ground_bytes) in curated_maps {
            let theme_str = map.primary_theme();

            let ground_bytes = match ground_bytes {
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

            let (samples, plan_issues) = plan_dataset_samples(&map, &dynamic_img);
            for issue in &plan_issues {
                tracing::warn!(
                    "LoRA dataset issue for '{}': {} ({})",
                    map.id,
                    issue.detail,
                    issue.code.as_str()
                );
            }

            for sample in samples {
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

                // The Trigger Phrase heads every caption and is applied exactly once, here.
                let caption_text = CaptionGenerator::generate_with_theme_str(
                    theme_str,
                    &map.tags,
                    sample.description.as_deref(),
                );

                let img_filename = format!(
                    "{theme_str}/{}_c{}_r{}_lora1024.png",
                    map.id, sample.col, sample.row
                );
                zip.start_file(&img_filename, options)?;
                zip.write_all(&image_bytes)?;

                let txt_filename = format!(
                    "{theme_str}/{}_c{}_r{}_lora1024.txt",
                    map.id, sample.col, sample.row
                );
                zip.start_file(&txt_filename, options)?;
                zip.write_all(caption_text.as_bytes())?;
            }
        }

        zip.finish()?;
    }

    Ok(buffer.into_inner())
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

    /// Resolves every map's Ground layer bytes first, then packages them into one in-memory
    /// archive through the shared [`create_lora_zip`] builder.
    async fn create_lora_zip(
        &self,
        curated_maps: &[CuratedMap],
    ) -> Result<Vec<u8>, StorageError> {
        let mut sources = Vec::with_capacity(curated_maps.len());
        for map in curated_maps {
            let ground_bytes = self.blobs.read_blob(&curated_ground_blob_key(map)).await;
            sources.push((map.clone(), ground_bytes));
        }
        create_lora_zip(sources)
    }
}
