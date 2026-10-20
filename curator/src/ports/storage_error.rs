//! Error type shared by every storage-facing port.

use thiserror::Error;

/// Errors arising from storage repository operations.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Item not found: {0}")]
    NotFound(String),

    #[error("Item already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("ZIP archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
}
