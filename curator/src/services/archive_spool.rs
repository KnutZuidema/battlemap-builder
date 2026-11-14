//! Bounded, disk-backed spool for dataset archives (ADR-0015).
//!
//! A frozen snapshot or a dataset download is a single ZIP whose size grows with the curated
//! library, not with the server's memory. This module writes that archive to a temporary file
//! under `DATA_DIR/tmp/` while it is being built, then hands the file to the upload or the HTTP
//! body, so the peak resident cost is one resampled section instead of the whole archive.
//!
//! Two limits bound the spool:
//!
//! - [`ArchiveSpool::create`] waits on a process-wide single permit: at most one spooled archive
//!   exists at a time, because a second concurrent freeze or export would double the disk demand
//!   before the first is uploaded or streamed out.
//! - [`ArchiveWriter`] enforces `CURATOR_MAX_ARCHIVE_BYTES` (default 8 GiB) while it writes, so an
//!   unexpectedly large dataset fails with a clear error instead of filling the volume.
//!
//! Spool files are derived and never authoritative: [`ArchiveWriter::finish`] yields a
//! [`SpooledArchive`] that deletes its file on drop, and [`ArchiveSpool::sweep_stale`] removes
//! files a crash left behind.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::ports::storage_error::StorageError;

/// Default ceiling for one spooled archive: 8 GiB, half the cluster's 20 GiB volume.
pub const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Directory below the data directory holding in-flight archives.
pub const SPOOL_DIR_NAME: &str = "tmp";

/// Spool files older than this are presumed abandoned and swept at boot.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Process-wide slot gate: one spooled archive at a time.
static SPOOL_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

/// Disambiguates sequential spool files written by one process.
static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Directory and limits for spooled dataset archives.
#[derive(Debug, Clone)]
pub struct ArchiveSpool {
    root: PathBuf,
}

impl ArchiveSpool {
    /// Creates a spool rooted at `root`, creating the directory on first use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves the spool directory from the configured data directory (`DATA_DIR/tmp`).
    ///
    /// The size limit is read when a spool is created, so an invalid
    /// `CURATOR_MAX_ARCHIVE_BYTES` fails the operation that needs the spool instead of server
    /// startup.
    pub fn from_env() -> Self {
        Self::new(crate::resolve_data_dir().join(SPOOL_DIR_NAME))
    }

    /// Root directory of the spool.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Maximum archive size for this process, from `CURATOR_MAX_ARCHIVE_BYTES`.
    pub fn max_archive_bytes(&self) -> Result<u64, StorageError> {
        max_archive_bytes_from_env()
    }

    /// Waits for the single spool slot and opens a new temporary archive.
    ///
    /// The slot is released when the finished [`SpooledArchive`] is dropped, so a caller must not
    /// hold one archive across another spool operation: the second waits forever by design rather
    /// than doubling the disk demand.
    pub async fn create(&self) -> Result<ArchiveWriter, StorageError> {
        let max_bytes = self.max_archive_bytes()?;
        let permit = Arc::clone(&SPOOL_SLOTS)
            .acquire_owned()
            .await
            .map_err(|_| StorageError::Io(io::Error::other("archive spool closed")))?;

        let root = self.root.clone();
        let spool = tokio::task::spawn_blocking(move || SpoolFile::create_in(&root))
            .await
            .map_err(|err| StorageError::Io(io::Error::other(err)))??;

        Ok(ArchiveWriter {
            spool,
            max_bytes,
            position: 0,
            permit: Some(permit),
        })
    }

    /// Removes spool files older than [`STALE_AFTER`], returning how many were removed.
    ///
    /// Called once at boot: a crash during a freeze leaves its archive behind, and nothing else
    /// knows the file exists.
    pub fn sweep_stale(&self) -> io::Result<usize> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err),
        };

        let now = SystemTime::now();
        let mut removed = 0usize;
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(now);
            if now.duration_since(modified).unwrap_or_default() <= STALE_AFTER {
                continue;
            }
            if std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Reads `CURATOR_MAX_ARCHIVE_BYTES`, defaulting to [`DEFAULT_MAX_ARCHIVE_BYTES`].
fn max_archive_bytes_from_env() -> Result<u64, StorageError> {
    let Ok(raw) = std::env::var("CURATOR_MAX_ARCHIVE_BYTES") else {
        return Ok(DEFAULT_MAX_ARCHIVE_BYTES);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_MAX_ARCHIVE_BYTES);
    }
    let parsed: u64 = trimmed.parse().map_err(|_| {
        StorageError::InvalidData(format!(
            "CURATOR_MAX_ARCHIVE_BYTES must be a byte count, got '{raw}'"
        ))
    })?;
    if parsed == 0 {
        return Err(StorageError::InvalidData(
            "CURATOR_MAX_ARCHIVE_BYTES must be greater than zero".to_string(),
        ));
    }
    Ok(parsed)
}

/// A spool file that removes itself from disk when dropped, however the work ends.
#[derive(Debug)]
struct SpoolFile {
    path: PathBuf,
    file: File,
}

impl SpoolFile {
    /// Creates a uniquely named archive in `root`; never reuses an existing path.
    fn create_in(root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        loop {
            let sequence = SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!(
                "dataset-archive-{}-{sequence}.zip",
                std::process::id()
            ));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => return Ok(Self { path, file }),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
    }
}

impl Drop for SpoolFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A [`Write`] + [`Seek`] handle over a temporary file, enforcing the archive ceiling.
///
/// `zip::ZipWriter` needs to seek back to patch member headers, so the ceiling is tracked against
/// the current write position, never against the sum of writes.
pub struct ArchiveWriter {
    spool: SpoolFile,
    max_bytes: u64,
    position: u64,
    permit: Option<OwnedSemaphorePermit>,
}

impl ArchiveWriter {
    /// Flushes the archive, records its length, and yields the finished spool.
    ///
    /// The spool slot stays held by the returned [`SpooledArchive`] until it is dropped, so the
    /// upload or the HTTP body finishes before another archive may be spooled.
    pub fn finish(mut self) -> io::Result<SpooledArchive> {
        self.spool.file.flush()?;
        let len = self.spool.file.metadata()?.len();
        let permit = self.permit.take();
        Ok(SpooledArchive {
            spool: self.spool,
            len,
            _permit: permit,
        })
    }
}

impl Write for ArchiveWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let end = self.position.saturating_add(buf.len() as u64);
        if end > self.max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!(
                    "dataset archive exceeds CURATOR_MAX_ARCHIVE_BYTES ({} bytes)",
                    self.max_bytes
                ),
            ));
        }
        let written = self.spool.file.write(buf)?;
        self.position += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.spool.file.flush()
    }
}

impl Seek for ArchiveWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let position = self.spool.file.seek(pos)?;
        self.position = position;
        Ok(position)
    }
}

/// A finished archive on disk that deletes itself (and releases the spool slot) on drop.
#[derive(Debug)]
pub struct SpooledArchive {
    spool: SpoolFile,
    len: u64,
    _permit: Option<OwnedSemaphorePermit>,
}

impl SpooledArchive {
    /// Size of the finished archive in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the archive is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Path of the finished archive; valid only while `self` is alive.
    pub fn path(&self) -> &Path {
        &self.spool.path
    }

    /// Opens an independent read handle over the archive.
    pub fn open(&self) -> io::Result<File> {
        File::open(&self.spool.path)
    }

    /// Computes the archive's SHA-256 digest, streaming the file instead of reading it whole.
    pub async fn sha256(&self) -> io::Result<String> {
        use tokio::io::AsyncReadExt;

        let mut file = tokio::fs::File::from_std(self.open()?);
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(max_bytes: u64) -> (ArchiveWriter, PathBuf) {
        let spool = SpoolFile::create_in(&std::env::temp_dir()).expect("create spool file");
        let path = spool.path.clone();
        (
            ArchiveWriter {
                spool,
                max_bytes,
                position: 0,
                permit: None,
            },
            path,
        )
    }

    #[test]
    fn writer_rejects_writes_past_the_ceiling() {
        let (mut archive, _path) = writer(8);
        archive.write_all(b"12345678").unwrap();

        let err = archive.write(b"9").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert!(
            err.to_string().contains("CURATOR_MAX_ARCHIVE_BYTES"),
            "{err}"
        );
    }

    #[test]
    fn ceiling_counts_the_current_position_not_the_sum_of_writes() {
        // ZIP writers seek back to patch headers; rewriting existing bytes must not consume headroom.
        let (mut archive, _path) = writer(8);
        archive.write_all(b"12345678").unwrap();
        archive.seek(SeekFrom::Start(0)).unwrap();
        archive.write_all(b"87654321").unwrap();

        archive.seek(SeekFrom::Start(8)).unwrap();
        let err = archive.write(b"x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn finished_archive_deletes_its_file_on_drop() {
        let (mut archive, path) = writer(u64::MAX);
        archive.write_all(b"abc").unwrap();
        let finished = archive.finish().unwrap();

        assert_eq!(finished.len(), 3);
        assert_eq!(finished.path(), path);
        assert_eq!(std::fs::read(finished.path()).unwrap(), b"abc");

        drop(finished);
        assert!(!path.exists(), "the spool file must not outlive its archive");
    }
}
