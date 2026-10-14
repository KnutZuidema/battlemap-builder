//! Port defining image processing and normalization operations for LoRA fine-tuning.

use thiserror::Error;

/// Target dimension for LoRA normalization (1024x1024).
pub const LORA_TARGET_SIZE: u32 = 1024;

/// Dark neutral background color (#0b0f17 / RGB [11, 15, 23]) used for letterboxing/pillarboxing.
pub const PADDING_COLOR_RGB: [u8; 3] = [11, 15, 23];
pub const PADDING_COLOR_RGBA: [u8; 4] = [11, 15, 23, 255];

/// Strategy for standardizing battlemap images to exact 1024x1024 resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeStrategy {
    /// Aspect-preserving letterbox/pillarbox padding with dark neutral `#0b0f17`
    /// (RGB [11, 15, 23]) background canvas.
    #[default]
    FitWithPadding,
    /// Smart center crop to 1:1 aspect ratio before resizing to 1024x1024.
    CenterCrop,
}

/// Errors occurring during image processing, decoding, resizing, or encoding.
#[derive(Debug, Error)]
pub enum ImageProcessorError {
    #[error("Failed to decode image: {0}")]
    Decode(String),

    #[error("Failed to resize image: {0}")]
    Resize(String),

    #[error("Failed to encode image to PNG: {0}")]
    Encode(String),

    #[error("Invalid image dimensions: {0}x{1}")]
    InvalidDimensions(u32, u32),

    #[error("I/O error during image processing: {0}")]
    Io(#[from] std::io::Error),
}

/// Port trait defining operations for normalizing images to exact LoRA dimensions.
pub trait ImageProcessor: Send + Sync {
    /// Resizes and standardizes the provided raw image bytes into exact 1024x1024 PNG bytes
    /// using hardware-accelerated SIMD Lanczos3 resampling.
    fn normalize_lora(
        &self,
        image_bytes: &[u8],
        strategy: ResizeStrategy,
    ) -> Result<Vec<u8>, ImageProcessorError>;
}

impl<T: ?Sized + ImageProcessor> ImageProcessor for std::sync::Arc<T> {
    fn normalize_lora(
        &self,
        image_bytes: &[u8],
        strategy: ResizeStrategy,
    ) -> Result<Vec<u8>, ImageProcessorError> {
        (**self).normalize_lora(image_bytes, strategy)
    }
}
