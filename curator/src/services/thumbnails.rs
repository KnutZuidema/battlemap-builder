//! Derived Thumbnails for curated library cards (ADR-0013).
//!
//! A Thumbnail is a rebuildable, non-authoritative rendering of one curated Layer: long edge at
//! most [`THUMBNAIL_LONG_EDGE`] pixels, aspect preserved, Lanczos3-resampled, alpha composited over
//! [`PADDING_COLOR_RGB`], JPEG-encoded at quality [`THUMBNAIL_JPEG_QUALITY`]. It is stored under
//! [`BlobKey::CuratedThumbnail`] (`thumbs/curated/{id}/{index}_{slug}.jpg`) and served by
//! `GET /api/thumb/curated/{id}` instead of transferring the multi-megabyte Ground layer.
//!
//! Generation happens on first request and is bounded twice: a semaphore of one serializes the
//! decode, and a per-key single flight makes concurrent requests for the same Thumbnail await one
//! generation and share its bytes. Once stored, a Thumbnail is served from storage and never
//! regenerated. Failures surface as [`ThumbnailError`] (502 at the HTTP edge); the original Layer
//! bytes are never substituted for a Thumbnail.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use tokio::sync::{Mutex, Semaphore};

use crate::domain::{Layer, LayerRole};
use crate::ports::blob_storage::{BlobKey, BlobStorage};
use crate::ports::catalog_repository::CatalogRepository;
use crate::ports::image_processor::PADDING_COLOR_RGB;
use crate::ports::storage_error::StorageError;

/// Longest edge of a derived Thumbnail in pixels; shorter images are never upscaled.
pub const THUMBNAIL_LONG_EDGE: u32 = 768;

/// JPEG quality of a derived Thumbnail.
pub const THUMBNAIL_JPEG_QUALITY: u8 = 82;

/// Failure to serve a derived Thumbnail.
#[derive(Debug)]
pub enum ThumbnailError {
    /// The Curated Map, its Layer, or the Layer blob does not exist.
    NotFound(String),
    /// The blob port failed while reading the source or writing the Thumbnail.
    Storage(StorageError),
    /// The source Layer could not be decoded, resampled, or JPEG-encoded. Served as `502`, never
    /// as a fallback to the original bytes.
    Generation(String),
}

impl From<StorageError> for ThumbnailError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(msg) => ThumbnailError::NotFound(msg),
            other => ThumbnailError::Storage(other),
        }
    }
}

impl std::fmt::Display for ThumbnailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThumbnailError::NotFound(msg) => write!(f, "{msg}"),
            ThumbnailError::Storage(err) => write!(f, "{err}"),
            ThumbnailError::Generation(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ThumbnailError {}

/// Application service generating and serving derived Thumbnails of curated Layers.
#[derive(Clone)]
pub struct ThumbnailService {
    catalog: Arc<dyn CatalogRepository + Send + Sync>,
    blobs: Arc<dyn BlobStorage + Send + Sync>,
    /// Single permit: exactly one Thumbnail is decoded at a time on this pod.
    decode_limit: Arc<Semaphore>,
    /// Per-Thumbnail single flight, keyed by canonical path. Entries hold only a [`Weak`]
    /// reference, so a finished flight leaves nothing behind.
    inflight: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
}

impl ThumbnailService {
    /// Creates a service resolving Curated Maps through `catalog` and bytes through `blobs`.
    pub fn new(
        catalog: Arc<dyn CatalogRepository + Send + Sync>,
        blobs: Arc<dyn BlobStorage + Send + Sync>,
    ) -> Self {
        Self {
            catalog,
            blobs,
            decode_limit: Arc::new(Semaphore::new(1)),
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns the Thumbnail bytes of the Ground layer of Curated Map `id`, generating and
    /// persisting it on first request.
    ///
    /// The Ground layer is resolved exactly like
    /// [`crate::services::LibraryService::curated_ground_image`]: the `Ground` role, otherwise the
    /// map's first Layer.
    pub async fn curated_thumbnail(&self, id: &str) -> Result<Vec<u8>, ThumbnailError> {
        let map = self.catalog.get_curated(id).await?;
        let layer = map
            .layers
            .iter()
            .find(|layer| layer.role == LayerRole::Ground)
            .or_else(|| map.layers.first())
            .ok_or_else(|| {
                ThumbnailError::NotFound(format!("Curated map '{id}' has no layer to thumbnail"))
            })?;
        self.ensure_curated_thumbnail(id, layer).await
    }

    /// Returns the stored Thumbnail of `layer` of Curated Map `id`, generating it when absent.
    async fn ensure_curated_thumbnail(
        &self,
        id: &str,
        layer: &Layer,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let source = BlobKey::CuratedLayer {
            id: id.to_string(),
            index: layer.index,
            role: layer.role,
            elevation: layer.elevation,
        };
        let thumbnail = BlobKey::CuratedThumbnail {
            id: id.to_string(),
            index: layer.index,
            role: layer.role,
            elevation: layer.elevation,
        };

        if let Some(bytes) = self.stored(&thumbnail).await? {
            return Ok(bytes);
        }

        let flight = self.key_lock(&thumbnail.to_path()).await;
        let _guard = flight.lock().await;

        // A concurrent request for this key may have finished while this one waited for the
        // flight, so the store is consulted again before paying the decode.
        if let Some(bytes) = self.stored(&thumbnail).await? {
            return Ok(bytes);
        }

        let source_bytes = self.blobs.read_blob(&source).await?;
        let source_len = source_bytes.len();
        let rendered = self.render(source_bytes).await?;
        self.blobs.write_blob(&thumbnail, &rendered).await?;
        tracing::debug!(
            path = %thumbnail.to_path(),
            source_bytes = source_len,
            thumbnail_bytes = rendered.len(),
            "Generated derived Thumbnail"
        );
        Ok(rendered)
    }

    /// Returns the stored bytes for `key`, or `None` when nothing is stored there.
    async fn stored(&self, key: &BlobKey) -> Result<Option<Vec<u8>>, ThumbnailError> {
        match self.blobs.read_blob(key).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(StorageError::NotFound(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Acquires the single flight guarding generation of the Thumbnail at `path`.
    async fn key_lock(&self, path: &str) -> Arc<Mutex<()>> {
        let mut inflight = self.inflight.lock().await;
        inflight.retain(|_, flight| flight.strong_count() > 0);
        if let Some(flight) = inflight.get(path).and_then(Weak::upgrade) {
            return flight;
        }
        let flight = Arc::new(Mutex::new(()));
        inflight.insert(path.to_string(), Arc::downgrade(&flight));
        flight
    }

    /// Decodes `source_bytes` into a Thumbnail on the blocking pool, holding the single decode
    /// permit for the whole decode-resample-encode sequence.
    async fn render(&self, source_bytes: Vec<u8>) -> Result<Vec<u8>, ThumbnailError> {
        let permit = self
            .decode_limit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ThumbnailError::Generation("Thumbnail decode limiter closed".into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            render_thumbnail(&source_bytes)
        })
        .await
        .map_err(|err| ThumbnailError::Generation(format!("Thumbnail render task failed: {err}")))?
    }
}

/// Renders `source` (any `image`-decodable bytes) into the derived Thumbnail format.
pub fn render_thumbnail(source: &[u8]) -> Result<Vec<u8>, ThumbnailError> {
    let decoded =
        image::load_from_memory(source).map_err(|err| ThumbnailError::Generation(err.to_string()))?;
    let (source_width, source_height) = (decoded.width(), decoded.height());
    if source_width == 0 || source_height == 0 {
        return Err(ThumbnailError::Generation(format!(
            "Source image is {source_width}x{source_height}"
        )));
    }

    let (width, height) = thumbnail_dimensions(source_width, source_height);
    let source_rgba = decoded.to_rgba8();
    let mut scaled = image::RgbaImage::new(width, height);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    let mut resizer = Resizer::new();
    resizer
        .resize(&source_rgba, &mut scaled, &options)
        .map_err(|err| ThumbnailError::Generation(format!("Resize failed: {err}")))?;

    // Transparent pixels become the dark padding colour instead of encoding as black, matching
    // every other derived image in the pipeline.
    let mut canvas = image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([
            PADDING_COLOR_RGB[0],
            PADDING_COLOR_RGB[1],
            PADDING_COLOR_RGB[2],
            u8::MAX,
        ]),
    );
    image::imageops::overlay(&mut canvas, &scaled, 0, 0);
    let composited = image::DynamicImage::ImageRgba8(canvas).to_rgb8();

    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, THUMBNAIL_JPEG_QUALITY)
        .encode_image(&image::DynamicImage::ImageRgb8(composited))
        .map_err(|err| ThumbnailError::Generation(err.to_string()))?;
    Ok(encoded)
}

/// Thumbnail dimensions for a `width`×`height` source: the long edge shrinks to
/// [`THUMBNAIL_LONG_EDGE`], the aspect ratio is preserved, and smaller sources are kept as they are.
fn thumbnail_dimensions(width: u32, height: u32) -> (u32, u32) {
    let long_edge = width.max(height);
    if long_edge <= THUMBNAIL_LONG_EDGE {
        return (width, height);
    }
    let scale = f64::from(THUMBNAIL_LONG_EDGE) / f64::from(long_edge);
    let scaled = |edge: u32| {
        (f64::from(edge) * scale).round().clamp(1.0, f64::from(THUMBNAIL_LONG_EDGE)) as u32
    };
    (scaled(width), scaled(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, pixel: image::Rgba<u8>) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, pixel);
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode PNG");
        bytes
    }

    #[test]
    fn thumbnail_shrinks_long_edge_and_keeps_aspect() {
        let bytes = render_thumbnail(&png(2000, 1000, image::Rgba([120, 40, 200, 255])))
            .expect("render Thumbnail");
        let decoded = image::load_from_memory(&bytes).expect("decode Thumbnail");
        assert_eq!(decoded.width(), THUMBNAIL_LONG_EDGE);
        assert_eq!(decoded.height(), THUMBNAIL_LONG_EDGE / 2);

        let tall = render_thumbnail(&png(1000, 2000, image::Rgba([120, 40, 200, 255])))
            .expect("render Thumbnail");
        let decoded = image::load_from_memory(&tall).expect("decode Thumbnail");
        assert_eq!(decoded.height(), THUMBNAIL_LONG_EDGE);
        assert_eq!(decoded.width(), THUMBNAIL_LONG_EDGE / 2);
    }

    #[test]
    fn thumbnail_is_jpeg_and_never_upscales() {
        let bytes = render_thumbnail(&png(300, 200, image::Rgba([10, 200, 30, 255])))
            .expect("render Thumbnail");
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");
        let decoded = image::load_from_memory(&bytes).expect("decode Thumbnail");
        assert_eq!((decoded.width(), decoded.height()), (300, 200));
    }

    #[test]
    fn transparency_is_composited_over_the_padding_colour() {
        let bytes =
            render_thumbnail(&png(64, 64, image::Rgba([255, 255, 255, 0]))).expect("render");
        let pixel = image::load_from_memory(&bytes)
            .expect("decode Thumbnail")
            .to_rgb8()
            .get_pixel(32, 32)
            .0;
        for (channel, expected) in pixel.iter().zip(PADDING_COLOR_RGB) {
            assert!(
                (i16::from(*channel) - i16::from(expected)).abs() <= 4,
                "channel {channel} should be within JPEG noise of {expected}"
            );
        }
    }

    #[test]
    fn undecodable_source_is_a_generation_failure() {
        assert!(matches!(
            render_thumbnail(b"not an image"),
            Err(ThumbnailError::Generation(_))
        ));
    }
}
