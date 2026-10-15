//! SIMD-accelerated image processor adapter using `fast_image_resize`.

use std::io::Cursor;
use fast_image_resize::{
    CpuExtensions, FilterType, ResizeAlg, ResizeOptions, Resizer,
};
use image::{ImageFormat, RgbaImage};

use crate::ports::{
    ImageProcessor, ImageProcessorError, ResizeStrategy, LORA_TARGET_SIZE, PADDING_COLOR_RGBA,
};

/// Hardware-accelerated image processor using SIMD Lanczos3 resampling.
#[derive(Debug, Clone, Default)]
pub struct SimdImageProcessor;

impl SimdImageProcessor {
    pub fn new() -> Self {
        Self
    }

    /// Inspect the CPU extensions (AVX2, SSE4.1, Neon, etc.) detected and utilized by the resizer.
    pub fn cpu_extensions(&self) -> CpuExtensions {
        Resizer::new().cpu_extensions()
    }
}

impl ImageProcessor for SimdImageProcessor {
    fn normalize_lora(
        &self,
        image_bytes: &[u8],
        strategy: ResizeStrategy,
    ) -> Result<Vec<u8>, ImageProcessorError> {
        let dynamic_img = image::load_from_memory(image_bytes)
            .map_err(|e| ImageProcessorError::Decode(e.to_string()))?;

        let (src_w, src_h) = (dynamic_img.width(), dynamic_img.height());
        if src_w == 0 || src_h == 0 {
            return Err(ImageProcessorError::InvalidDimensions(src_w, src_h));
        }

        let src_rgba = dynamic_img.to_rgba8();
        let mut resizer = Resizer::new();
        let target_dim = LORA_TARGET_SIZE;

        let final_canvas = match strategy {
            ResizeStrategy::FitWithPadding => {
                let (scaled_w, scaled_h, offset_x, offset_y) = if src_w == src_h {
                    (target_dim, target_dim, 0, 0)
                } else if src_w > src_h {
                    let scaled_w = target_dim;
                    let scaled_h = ((src_h as f64 * target_dim as f64) / src_w as f64)
                        .round()
                        .clamp(1.0, target_dim as f64) as u32;
                    let offset_y = (target_dim - scaled_h) / 2;
                    (scaled_w, scaled_h, 0, offset_y)
                } else {
                    let scaled_h = target_dim;
                    let scaled_w = ((src_w as f64 * target_dim as f64) / src_h as f64)
                        .round()
                        .clamp(1.0, target_dim as f64) as u32;
                    let offset_x = (target_dim - scaled_w) / 2;
                    (scaled_w, scaled_h, offset_x, 0)
                };

                let mut scaled_rgba = RgbaImage::new(scaled_w, scaled_h);
                let resize_options = ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));

                resizer
                    .resize(&src_rgba, &mut scaled_rgba, &resize_options)
                    .map_err(|e| ImageProcessorError::Resize(e.to_string()))?;

                let mut canvas = RgbaImage::from_pixel(
                    target_dim,
                    target_dim,
                    image::Rgba(PADDING_COLOR_RGBA),
                );

                image::imageops::overlay(&mut canvas, &scaled_rgba, offset_x as i64, offset_y as i64);
                canvas
            }
            ResizeStrategy::CenterCrop => {
                let mut cropped_and_resized = RgbaImage::new(target_dim, target_dim);
                let resize_options = ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .fit_into_destination(Some((0.5, 0.5)));

                resizer
                    .resize(&src_rgba, &mut cropped_and_resized, &resize_options)
                    .map_err(|e| ImageProcessorError::Resize(e.to_string()))?;

                // Composite over dark neutral background in case input had transparency
                let mut canvas = RgbaImage::from_pixel(
                    target_dim,
                    target_dim,
                    image::Rgba(PADDING_COLOR_RGBA),
                );
                image::imageops::overlay(&mut canvas, &cropped_and_resized, 0, 0);
                canvas
            }
        };

        let mut output_bytes = Vec::new();
        let mut cursor = Cursor::new(&mut output_bytes);
        final_canvas
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| ImageProcessorError::Encode(e.to_string()))?;

        Ok(output_bytes)
    }
}
