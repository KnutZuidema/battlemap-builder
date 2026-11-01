use std::collections::HashMap;

use image::DynamicImage;

use crate::domain::{
    crop_section_box, synthesize_map_sections, synthesize_sections, CuratedMap, GridConfig,
    LayerRole, MapSection,
};
use crate::services::description::DescriptionService;
use crate::services::image_work::GroundImage;

/// Shared inputs for deriving per-MapSection descriptions from a Ground layer image.
pub struct SectionDescriptionContext<'a> {
    /// Grid configuration used to derive the overlapping 16×16 section windows.
    pub grid: &'a GridConfig,
    /// Decoded Ground layer, holding the process-wide image-work permit. `None` when the bytes
    /// could not be read or decoded. Owned rather than borrowed so the decode can be dropped once
    /// the section crops exist, before the first vision round-trip.
    pub ground: Option<GroundImage>,
    /// Ground layer dimensions used to derive section origins.
    pub ground_dimensions: (u32, u32),
    /// Tags forwarded to vision generation for section crops.
    pub tags: &'a [String],
    /// Candidate title (never the map description), forwarded to vision generation.
    pub title: Option<&'a str>,
    /// Map-level physical description, reused verbatim for single-section maps.
    pub map_description: &'a str,
}

/// Ground layer dimensions used to derive Map Section origins.
///
/// The decoded image wins: the readiness audit crops sections out of exactly those pixels, so
/// planning origins from stale layer metadata mints sections whose boxes never fit the image and
/// a remediation can never clear `section_unresolvable`. Recorded metadata is the fallback when
/// the image cannot be read or decoded.
pub fn ground_dimensions(map: &CuratedMap, ground_image: Option<&DynamicImage>) -> (u32, u32) {
    ground_image
        .map(|img| (img.width(), img.height()))
        .or_else(|| {
            map.layers
                .iter()
                .find(|layer| layer.role == LayerRole::Ground)
                .or_else(|| map.layers.first())
                .map(|layer| layer.dimensions)
                .filter(|&(w, h)| w > 0 && h > 0)
        })
        .unwrap_or(map.dimensions)
}

/// Crops one 16×16 grid-cell Map Section from the Ground layer and encodes it as PNG.
///
/// Cropping policy (bounds checks, exact box) lives in [`crop_section_box`]; a section whose
/// box does not fit is unresolvable and yields `None` rather than a rescaled crop.
fn crop_section_png(
    image: &DynamicImage,
    grid: &GridConfig,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    let cropped = crop_section_box(image, grid, col, row)?;
    let mut bytes = Vec::new();
    cropped
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()
        .map(|_| bytes)
}

/// Derives descriptions for every Map Section of a map, delegating section policy to
/// [`synthesize_sections`].
///
/// Single-section maps (and grids that yield no full section window) reuse the map-level
/// `Physical Description` verbatim. Multi-section maps ask the vision `DescriptionService` for a
/// crop-specific description and fall back per section to the map-level description, so a single
/// failed section never blocks curation or regridding and no caption ever enters a
/// `Physical Description`.
pub async fn describe_map_sections(
    description_service: &DescriptionService,
    context: SectionDescriptionContext<'_>,
) -> Vec<MapSection> {
    let SectionDescriptionContext {
        grid,
        ground,
        ground_dimensions,
        tags,
        title,
        map_description,
    } = context;

    let (ground_w, ground_h) = ground_dimensions;
    let section_origins = grid.section_origins(ground_w, ground_h);
    if section_origins.len() <= 1 {
        return synthesize_map_sections(&section_origins, Some(map_description));
    }

    // Crop every Map Section up front and release the decoded Ground layer before the first vision
    // call: each crop is a PNG of one section, while the decode is hundreds of MiB and holds the
    // process-wide image-work permit that must not stay taken across the vision round-trips.
    let section_pngs = match ground {
        Some(ground) => crop_sections(ground, *grid, &section_origins).await,
        None => HashMap::new(),
    };

    // Section descriptions are best-effort: a failed vision call or an unavailable crop reuses the
    // map-level `Physical Description` (the section is a window onto the same encounter space).
    // Synthesizing a caption here instead would bake the `Trigger Phrase` and camera directives
    // into a `Physical Description` — the caption belongs to the LoRA dataset export, once
    // (ADR-0002, ADR-0007).
    let fallback = || map_description.trim().to_string();

    let mut section_descriptions = HashMap::with_capacity(section_origins.len());
    for &(col, row) in &section_origins {
        let description = match section_pngs.get(&(col, row)) {
            Some(bytes) => match description_service
                .describe(bytes, Some("image/png"), tags, title)
                .await
            {
                Ok(description) => description,
                Err(err) => {
                    tracing::warn!(
                        col,
                        row,
                        error = %err,
                        "Section description generation failed; reusing the map-level Physical Description"
                    );
                    fallback()
                }
            },
            None => {
                tracing::warn!(
                    col,
                    row,
                    "Ground layer section crop unavailable; reusing the map-level Physical Description"
                );
                fallback()
            }
        };

        section_descriptions.insert((col, row), description);
    }

    synthesize_sections(
        &section_origins,
        || map_description.to_string(),
        |col, row| {
            section_descriptions
                .remove(&(col, row))
                .unwrap_or_else(&fallback)
        },
    )
}

/// Crops every planned section on the blocking pool and drops the decoded Ground layer.
///
/// The closure owns the decode, so the image and the image-work permit are released when it
/// returns; a panicked crop task is logged and yields no crops, which the caller already handles
/// as an unavailable crop per section.
async fn crop_sections(
    mut ground: GroundImage,
    grid: GridConfig,
    origins: &[(u32, u32)],
) -> HashMap<(u32, u32), Vec<u8>> {
    let Some(image) = ground.take_image() else {
        return HashMap::new();
    };
    let origins = origins.to_vec();
    tokio::task::spawn_blocking(move || {
        origins
            .into_iter()
            .filter_map(|(col, row)| {
                crop_section_png(&image, &grid, col, row).map(|bytes| ((col, row), bytes))
            })
            .collect()
    })
    .await
    .unwrap_or_else(|err| {
        tracing::error!(error = %err, "Section crop task failed");
        HashMap::new()
    })
}
