use image::DynamicImage;

use crate::domain::{CaptionGenerator, GridConfig, MapSection};
use crate::services::description::DescriptionService;

/// Shared inputs for deriving per-MapSection descriptions from a Ground layer image.
pub struct SectionDescriptionContext<'a> {
    /// Grid configuration used to derive the overlapping 16×16 section windows.
    pub grid: &'a GridConfig,
    /// Decoded Ground layer image; `None` when the bytes could not be read or decoded.
    pub ground_image: Option<&'a DynamicImage>,
    /// Ground layer dimensions used to derive section origins.
    pub ground_dimensions: (u32, u32),
    /// Tags forwarded to vision generation and the deterministic caption fallback.
    pub tags: &'a [String],
    /// Candidate title (never the map description), forwarded to vision generation.
    pub title: Option<&'a str>,
    /// Map-level physical description, reused verbatim for single-section maps.
    pub map_description: &'a str,
}

/// Crops one 16×16 grid-cell Map Section from the Ground layer and encodes it as PNG.
pub fn crop_section_png(
    image: &DynamicImage,
    grid: &GridConfig,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    let cell = grid.cell_size_px;
    if cell == 0 {
        return None;
    }
    let x = grid.offset_x.checked_add(col.checked_mul(cell)?)?;
    let y = grid.offset_y.checked_add(row.checked_mul(cell)?)?;
    let width = cell.checked_mul(16)?;
    let height = cell.checked_mul(16)?;

    if x.checked_add(width)? > image.width() || y.checked_add(height)? > image.height() {
        return None;
    }

    let cropped = image.crop_imm(x, y, width, height);
    let mut bytes = Vec::new();
    cropped
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()
        .map(|_| bytes)
}

/// Derives descriptions for every Map Section of a multi-section map.
///
/// Single-section maps (and grids that yield no full section window) reuse the map-level
/// physical description verbatim. Multi-section maps ask the vision `DescriptionService`
/// for a crop-specific description and fall back per section to the deterministic
/// caption generator, so a single failed section never blocks curation or regridding.
pub async fn describe_map_sections(
    description_service: &DescriptionService,
    context: SectionDescriptionContext<'_>,
) -> Vec<MapSection> {
    let SectionDescriptionContext {
        grid,
        ground_image,
        ground_dimensions,
        tags,
        title,
        map_description,
    } = context;

    let (ground_w, ground_h) = ground_dimensions;
    let section_origins = grid.section_origins(ground_w, ground_h);
    if section_origins.len() <= 1 {
        return vec![MapSection::new(0, 0, map_description)];
    }

    let primary_theme = tags.first().map(String::as_str).unwrap_or("dungeon");
    let fallback = || CaptionGenerator::generate_with_theme_str(primary_theme, tags, title);

    let mut sections = Vec::with_capacity(section_origins.len());
    for (col, row) in section_origins {
        let section_png =
            ground_image.and_then(|image| crop_section_png(image, grid, col, row));

        let description = match section_png {
            Some(bytes) => match description_service
                .describe(&bytes, Some("image/png"), tags, title)
                .await
            {
                Ok(description) => description,
                Err(err) => {
                    tracing::warn!(
                        col,
                        row,
                        error = %err,
                        "Section description generation failed; using deterministic caption fallback"
                    );
                    fallback()
                }
            },
            None => {
                tracing::warn!(
                    col,
                    row,
                    "Ground layer section crop unavailable; using deterministic caption fallback"
                );
                fallback()
            }
        };

        sections.push(MapSection::new(col, row, description));
    }

    sections
}
