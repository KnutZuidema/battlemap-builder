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

/// Maps a blocking-pool join failure to the `io::Error` every caller reports.
///
/// A decode, crop or audit blocking task reports a panic through this one shape, so a panicked
/// pipeline fails the operation instead of looking like an empty result. Callers wrap the result
/// in their own error type.
pub fn join_error(context: impl std::fmt::Display, err: tokio::task::JoinError) -> std::io::Error {
    std::io::Error::other(format!("{context} failed: {err}"))
}

/// A decoded Ground layer that holds the image-work permit for its whole lifetime.
///
/// Keeping the permit inside the decode is what makes the bound airtight: a caller cannot keep the
/// pixels without keeping the promise that no other pipeline decodes at the same time. Release it
/// as soon as the pixels are no longer needed: run the CPU work through
/// [`GroundImage::with_pixels`] and drop the value before awaiting anything unrelated to this
/// decode. A pipeline whose own unit of work reads the pixels between awaits — refinement crops one
/// Map Section at a time while it describes each section
/// (`services::sections::describe_map_sections`) — deliberately keeps the permit across those
/// vision round-trips: other image pipelines queue behind it instead of decoding on top of it.
pub struct GroundImage {
    /// The decoded pixels, or the reason the decoder produced none.
    decoded: Result<DynamicImage, String>,
    /// True when the blocking decode task panicked instead of returning unreadable bytes.
    panicked: bool,
    /// Absent only when the decode task panicked; there are no pixels to protect then.
    _permit: Option<OwnedSemaphorePermit>,
}

impl GroundImage {
    /// Decodes `bytes` on the blocking pool under the shared permit.
    ///
    /// Unreadable bytes and a panicked decoder both yield a value without pixels, and
    /// [`GroundImage::error`] carries the cause either way. [`GroundImage::panicked`] tells the two
    /// apart: undecodable bytes are input a caller may skip, while a panicked decoder is a bug that
    /// has to fail the operation instead of looking like an empty result.
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
            let decoded = image::load_from_memory(&bytes).map_err(|err| err.to_string());
            (permit, decoded)
        })
        .await
        {
            Ok((permit, decoded)) => Self {
                decoded,
                panicked: false,
                _permit: Some(permit),
            },
            Err(err) => {
                tracing::error!(error = %err, "Ground layer decode task failed");
                // The panicked task unwound and dropped its permit with its stack. There are no
                // pixels to protect, so do not queue for a second permit: the caller has already
                // failed and a bulk refinement may hold the only permit for minutes.
                Self {
                    decoded: Err(err.to_string()),
                    panicked: true,
                    _permit: None,
                }
            }
        }
    }

    /// The decoded pixels, or `None` when the bytes were unreadable.
    pub fn image(&self) -> Option<&DynamicImage> {
        self.decoded.as_ref().ok()
    }

    /// The decoder error when the bytes were unreadable, or the panic when the task failed.
    pub fn error(&self) -> Option<&str> {
        self.decoded.as_ref().err().map(String::as_str)
    }

    /// True when the decode task panicked rather than the bytes being unreadable.
    pub fn panicked(&self) -> bool {
        self.panicked
    }

    /// Runs `work` against the decoded pixels on the blocking pool.
    ///
    /// Takes `self: &Arc<Self>` so the blocking task owns a clone of the decode, and with it the
    /// permit: a dropped request future (request timeout, client disconnect) cannot release the
    /// permit while the pixels are still being read.
    ///
    /// Panics inside `work` come back as a `JoinError`; callers report them instead of treating
    /// them as an empty result. Callers that treat an unreadable Ground layer as recoverable check
    /// [`GroundImage::image`] first, because `with_pixels` panics when there are no pixels.
    pub async fn with_pixels<T, F>(self: &Arc<Self>, work: F) -> Result<T, tokio::task::JoinError>
    where
        F: FnOnce(&DynamicImage) -> T + Send + 'static,
        T: Send + 'static,
    {
        let ground = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let image = ground
                .image()
                .expect("ground image verified before the blocking task");
            work(image)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

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
        assert!(!decoded.panicked());
        assert_eq!(limit.available_permits(), 0, "a live decode holds the permit");
        drop(decoded);
        assert_eq!(limit.available_permits(), 1, "dropping the decode releases the permit");
    }

    #[tokio::test]
    async fn unreadable_bytes_yield_no_pixels_and_release_the_permit() {
        let limit = Arc::new(Semaphore::new(1));
        let decoded = GroundImage::decode_with(limit.clone(), vec![0, 1, 2, 3]).await;
        assert!(decoded.image().is_none());
        assert!(
            !decoded.panicked(),
            "unreadable input is not a panicked pipeline"
        );
        assert!(
            decoded.error().is_some(),
            "callers that skip an unreadable Ground layer need the decoder's cause"
        );
        drop(decoded);
        assert_eq!(limit.available_permits(), 1);
    }

    #[tokio::test]
    async fn with_pixels_runs_on_the_decoded_image_and_releases_the_permit_afterwards() {
        let limit = Arc::new(Semaphore::new(1));
        let decoded = Arc::new(GroundImage::decode_with(limit.clone(), png(8, 8)).await);
        let dimensions = decoded
            .with_pixels(|image| (image.width(), image.height()))
            .await
            .expect("blocking task joins");
        assert_eq!(dimensions, (8, 8));
        assert_eq!(
            limit.available_permits(),
            0,
            "the decode still holds the permit after the blocking crop"
        );
        drop(decoded);
        assert_eq!(limit.available_permits(), 1);
    }

    /// A dropped owner must not release the permit while the detached crop still reads the
    /// pixels: that is the whole point of moving an `Arc` clone into the blocking task.
    #[tokio::test]
    async fn with_pixels_keeps_the_permit_when_its_owner_is_dropped() {
        let limit = Arc::new(Semaphore::new(1));
        let decoded = Arc::new(GroundImage::decode_with(limit.clone(), png(8, 8)).await);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let crop = tokio::spawn({
            let decoded = Arc::clone(&decoded);
            async move {
                decoded
                    .with_pixels(move |_| {
                        let _ = started_tx.send(());
                        let _ = release_rx.recv();
                    })
                    .await
            }
        });

        started_rx.await.expect("blocking crop starts");
        drop(decoded);
        assert_eq!(
            limit.available_permits(),
            0,
            "a dropped owner must not release the permit while the crop still reads the pixels"
        );

        release_tx.send(()).expect("crop is still running");
        crop.await.expect("crop task joins").expect("blocking crop joins");
        assert_eq!(
            limit.available_permits(),
            1,
            "the permit is released once the last owner is gone"
        );
    }

    /// Dropping the decode future mid-decode must not release the permit: the detached blocking
    /// task still holds the pixels (and the permit) until it finishes.
    #[tokio::test]
    async fn dropping_a_decode_future_keeps_the_permit_until_the_decode_finishes() {
        let limit = Arc::new(Semaphore::new(1));
        let mut decode = Box::pin(GroundImage::decode_with(limit.clone(), png(2048, 2048)));

        let mut cx = Context::from_waker(Waker::noop());
        assert!(
            matches!(decode.as_mut().poll(&mut cx), Poll::Pending),
            "the decode must hand its permit to the blocking pool"
        );

        drop(decode);
        assert_eq!(
            limit.available_permits(),
            0,
            "a dropped request future must not release the permit while the decode still runs"
        );

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while limit.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the detached decode releases the permit when it finishes");
    }
}
