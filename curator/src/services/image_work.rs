//! Process-wide gate for pipelines that hold a full-resolution Ground-layer decode.
//!
//! A `9000×9400` Ground layer is roughly 250 MiB once [`image::DynamicImage`] has decoded it, and
//! the largest curated map (`13312×10240`) peaks near 770 MiB while decoding. The curator pod runs
//! with a 1536 MiB memory limit. Bulk AI refinement keeps a decode alive while the vision model
//! describes every Map Section, the readiness audit decodes every stored Ground layer in turn, and
//! a Thumbnail render decodes its source before resampling. Two of those overlapping exhaust the
//! cgroup: the kernel OOM-kills the process, and Kubernetes records the probe failures that follow
//! as the restart reason.
//!
//! Every pipeline that holds a decoded Ground layer therefore takes this single process-wide
//! permit for as long as the decode is alive, and decodes on the blocking pool so the HTTP
//! runtime keeps serving probes and requests while the image work runs.

use std::sync::{Arc, LazyLock};

use image::DynamicImage;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Single permit shared by every full-resolution image pipeline on this pod.
static IMAGE_WORK: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

/// The shared semaphore, for services that own their own decode gate.
pub fn shared() -> Arc<Semaphore> {
    IMAGE_WORK.clone()
}

/// Waits for the single image-work permit.
///
/// The caller must hold the returned permit for as long as its decoded image is alive: the permit
/// is the promise that only one full-resolution decode occupies memory at a time.
pub async fn acquire() -> OwnedSemaphorePermit {
    IMAGE_WORK
        .clone()
        .acquire_owned()
        .await
        .expect("image work semaphore is never closed")
}

/// Runs `work` on the blocking pool while holding the permit.
///
/// Use this for a decode-and-process unit that owns all of its inputs; `work`'s image is dropped
/// when it returns, which releases the permit. The join error is returned rather than swallowed:
/// a panicked image pipeline is a bug the caller has to report, not an empty result.
pub async fn run<T, F>(work: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let permit = acquire().await;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
}

/// A decoded Ground layer that holds the image-work permit for its whole lifetime.
///
/// Keeping the permit inside the decode is what makes the bound airtight: a caller cannot keep the
/// pixels without keeping the promise that no other pipeline decodes at the same time. Drop the
/// value (or call [`GroundImage::crop_sections`], which consumes it) before any long await such as
/// a vision round-trip.
pub struct GroundImage {
    image: Option<DynamicImage>,
    _permit: OwnedSemaphorePermit,
}

impl GroundImage {
    /// Decodes `bytes` on the blocking pool under the shared permit.
    ///
    /// Unreadable bytes and a panicked decoder both yield an empty value, which keeps the existing
    /// caller policy: an undecodable Ground layer has no pixels to crop or to plan samples from.
    pub async fn decode(bytes: Vec<u8>) -> Self {
        Self::decode_with(shared(), bytes).await
    }

    /// [`GroundImage::decode`] against an explicit permit pool; tests use their own.
    ///
    /// The permit travels into the blocking task and comes back only with the decoded pixels. If
    /// the caller's future is dropped mid-decode (request timeout, client disconnect), the
    /// detached blocking task keeps the permit until it finishes, so no other pipeline can start
    /// a second full-resolution decode while the first is still allocating.
    async fn decode_with(limit: Arc<Semaphore>, bytes: Vec<u8>) -> Self {
        let permit = limit
            .clone()
            .acquire_owned()
            .await
            .expect("image work semaphore is never closed");
        match tokio::task::spawn_blocking(move || {
            let image = image::load_from_memory(&bytes).ok();
            (permit, image)
        })
        .await
        {
            Ok((permit, image)) => Self {
                image,
                _permit: permit,
            },
            Err(err) => {
                tracing::error!(error = %err, "Ground layer decode task failed");
                // The panicked task unwound and dropped its permit with its stack. Re-acquire one
                // so a live `GroundImage` still honours "no pixels without the permit"; there are
                // no pixels here, so this is only about keeping the type's contract uniform.
                let permit = limit
                    .acquire_owned()
                    .await
                    .expect("image work semaphore is never closed");
                Self {
                    image: None,
                    _permit: permit,
                }
            }
        }
    }

    /// The decoded pixels, or `None` when the bytes were unreadable.
    pub fn image(&self) -> Option<&DynamicImage> {
        self.image.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([30, 60, 90, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode PNG");
        bytes
    }

    #[tokio::test]
    async fn run_serializes_image_work() {
        static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
        static PEAK: AtomicUsize = AtomicUsize::new(0);

        let mut handles = Vec::new();
        for _ in 0..4 {
            handles.push(tokio::spawn(async {
                image_work_probe().await;
            }));
        }
        for handle in handles {
            handle.await.expect("probe task joins");
        }
        assert_eq!(PEAK.load(Ordering::SeqCst), 1);

        async fn image_work_probe() {
            super::run(|| {
                let in_flight = IN_FLIGHT.fetch_add(1, Ordering::SeqCst) + 1;
                PEAK.fetch_max(in_flight, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
                IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
            })
            .await
            .expect("probe joins");
        }
    }

    #[tokio::test]
    async fn ground_image_releases_the_permit_only_when_dropped() {
        let limit = Arc::new(Semaphore::new(1));
        let decoded = GroundImage::decode_with(limit.clone(), png(8, 8)).await;
        assert_eq!(decoded.image().map(|img| (img.width(), img.height())), Some((8, 8)));
        assert_eq!(limit.available_permits(), 0, "a live decode holds the permit");
        drop(decoded);
        assert_eq!(limit.available_permits(), 1, "dropping the decode releases the permit");
    }

    #[tokio::test]
    async fn unreadable_bytes_yield_no_pixels_and_release_the_permit() {
        let limit = Arc::new(Semaphore::new(1));
        let decoded = GroundImage::decode_with(limit.clone(), vec![0, 1, 2, 3]).await;
        assert!(decoded.image().is_none());
        drop(decoded);
        assert_eq!(limit.available_permits(), 1);
    }
}
