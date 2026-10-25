//! S3-compatible [`BlobStorage`] implementation supporting Cloudflare R2 and MinIO.

use async_trait::async_trait;

use crate::ports::blob_storage::{sha256_digest, BlobKey, BlobStat, BlobStorage};
use crate::ports::storage_error::StorageError;

/// Chunk size and threshold for S3 multipart uploads (5 MiB, the S3 minimum part size).
pub const MULTIPART_THRESHOLD: usize = 5 * 1024 * 1024;
pub const PART_SIZE: usize = 5 * 1024 * 1024;

/// Errors arising from missing or invalid S3 / Cloudflare R2 configuration.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum S3ConfigError {
    #[error("Missing required S3/R2 configuration key: {0}")]
    MissingKey(&'static str),

    #[error("Invalid CURATOR_BLOB_BACKEND '{0}': expected 's3' or 'filesystem'")]
    InvalidBackend(String),
}

/// Strongly-typed configuration for connecting to an S3-compatible object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3StorageConfig {
    pub bucket_name: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub public_url: Option<String>,
}

impl S3StorageConfig {
    /// Loads and validates S3/R2 configuration from environment variables.
    ///
    /// Requires `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET_NAME`, and either
    /// `R2_ENDPOINT` or `R2_ACCOUNT_ID`. Fails loudly naming the missing key.
    pub fn from_env() -> Result<Self, S3ConfigError> {
        let access_key_id = get_non_empty_env("R2_ACCESS_KEY_ID")
            .ok_or(S3ConfigError::MissingKey("R2_ACCESS_KEY_ID"))?;

        let secret_access_key = get_non_empty_env("R2_SECRET_ACCESS_KEY")
            .ok_or(S3ConfigError::MissingKey("R2_SECRET_ACCESS_KEY"))?;

        let bucket_name = get_non_empty_env("R2_BUCKET_NAME")
            .ok_or(S3ConfigError::MissingKey("R2_BUCKET_NAME"))?;

        let endpoint = if let Some(ep) = get_non_empty_env("R2_ENDPOINT") {
            ep
        } else if let Some(account_id) = get_non_empty_env("R2_ACCOUNT_ID") {
            format!("https://{account_id}.r2.cloudflarestorage.com")
        } else {
            return Err(S3ConfigError::MissingKey("R2_ENDPOINT"));
        };

        let region = get_non_empty_env("R2_REGION")
            .or_else(|| get_non_empty_env("AWS_REGION"))
            .unwrap_or_else(|| "auto".to_string());

        let public_url = get_non_empty_env("R2_PUBLIC_URL");

        Ok(Self {
            bucket_name,
            endpoint,
            access_key_id,
            secret_access_key,
            region,
            public_url,
        })
    }
}

/// Helper to read an environment variable and treat empty/whitespace strings as None.
fn get_non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Returns true when any S3/R2 configuration variable is present in the environment.
pub fn is_s3_configured_in_env() -> bool {
    const CANDIDATES: &[&str] = &[
        "CURATOR_BLOB_BACKEND",
        "R2_ACCESS_KEY_ID",
        "R2_SECRET_ACCESS_KEY",
        "R2_BUCKET_NAME",
        "R2_ENDPOINT",
        "R2_ACCOUNT_ID",
        "R2_PUBLIC_URL",
    ];
    CANDIDATES.iter().any(|key| get_non_empty_env(key).is_some())
}

/// Resolved choice of blob storage backend based on configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobBackend {
    Filesystem,
    S3(S3StorageConfig),
}

impl BlobBackend {
    /// Selects the backend from environment variables.
    ///
    /// - If `CURATOR_BLOB_BACKEND` is set:
    ///   - `"filesystem"` => [`BlobBackend::Filesystem`]
    ///   - `"s3"` or `"r2"` => validates S3 config and returns [`BlobBackend::S3`]
    ///   - any other value => fails with [`S3ConfigError::InvalidBackend`]
    /// - If `CURATOR_BLOB_BACKEND` is not set:
    ///   - If any R2/S3 config variable is present, validates full S3 config and returns [`BlobBackend::S3`].
    ///     Missing credentials fail loudly naming the missing key; they do NOT fall back to filesystem.
    ///   - If no R2/S3 config is set, defaults to [`BlobBackend::Filesystem`].
    pub fn from_env() -> Result<Self, S3ConfigError> {
        if let Some(backend) = get_non_empty_env("CURATOR_BLOB_BACKEND") {
            match backend.to_lowercase().as_str() {
                "filesystem" => Ok(BlobBackend::Filesystem),
                "s3" | "r2" => S3StorageConfig::from_env().map(BlobBackend::S3),
                other => Err(S3ConfigError::InvalidBackend(other.to_string())),
            }
        } else if is_s3_configured_in_env() {
            S3StorageConfig::from_env().map(BlobBackend::S3)
        } else {
            Ok(BlobBackend::Filesystem)
        }
    }
}

/// [`BlobStorage`] implementation backed by an S3-compatible service (Cloudflare R2, MinIO, AWS S3).
#[derive(Clone)]
pub struct S3BlobStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    public_url_base: Option<String>,
}

impl S3BlobStorage {
    /// Creates a new `S3BlobStorage` from configuration.
    pub fn new(config: S3StorageConfig) -> Self {
        let creds = aws_sdk_s3::config::Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            "s3_blob_storage",
        );
        let s3_conf = aws_sdk_s3::config::Builder::new()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .credentials_provider(creds)
            .region(aws_sdk_s3::config::Region::new(config.region))
            .endpoint_url(config.endpoint)
            .force_path_style(true)
            .response_checksum_validation(aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired)
            .build();
        let client = aws_sdk_s3::Client::from_conf(s3_conf);

        Self {
            client,
            bucket: config.bucket_name,
            public_url_base: config.public_url,
        }
    }

    /// Creates an `S3BlobStorage` wrapping an existing `aws_sdk_s3::Client`.
    pub fn from_client(
        client: aws_sdk_s3::Client,
        bucket: impl Into<String>,
        public_url_base: Option<String>,
    ) -> Self {
        Self {
            client,
            bucket: bucket.into(),
            public_url_base,
        }
    }

    /// Name of the S3 bucket.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Optional public URL base (e.g. `https://pub-...r2.dev`).
    pub fn public_url_base(&self) -> Option<&str> {
        self.public_url_base.as_deref()
    }


    /// Resolves the public HTTPS URL for a public artifact key using the configured `public_url_base`.
    pub fn resolve_public_url(&self, key: &BlobKey) -> Option<String> {
        let base = self.public_url_base.as_deref()?;
        key.public_url(base)
    }

    /// Helper to convert S3 SDK errors to [`StorageError`].
    fn map_get_error(path: &str, err: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>) -> StorageError {
        if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &err {
            if service_err.err().is_no_such_key() {
                return StorageError::NotFound(format!("Blob '{path}' not found"));
            }
        }
        if err.raw_response().map(|r| r.status().as_u16()) == Some(404) {
            return StorageError::NotFound(format!("Blob '{path}' not found"));
        }
        StorageError::Io(std::io::Error::other(err))
    }

    fn map_head_error(path: &str, err: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::head_object::HeadObjectError>) -> StorageError {
        if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &err {
            if service_err.err().is_not_found() {
                return StorageError::NotFound(format!("Blob '{path}' not found"));
            }
        }
        if err.raw_response().map(|r| r.status().as_u16()) == Some(404) {
            return StorageError::NotFound(format!("Blob '{path}' not found"));
        }
        StorageError::Io(std::io::Error::other(err))
    }
}

#[async_trait]
impl BlobStorage for S3BlobStorage {
    async fn read_blob(&self, key: &BlobKey) -> Result<Vec<u8>, StorageError> {
        let path = key.to_path();
        let res = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&path)
            .send()
            .await
            .map_err(|err| Self::map_get_error(&path, err))?;

        let bytes = res
            .body
            .collect()
            .await
            .map_err(|err| StorageError::Io(std::io::Error::other(err)))?
            .into_bytes();

        Ok(bytes.to_vec())
    }

    async fn write_blob(&self, key: &BlobKey, bytes: &[u8]) -> Result<(), StorageError> {
        self.write_blob_with_sha256(key, bytes, None).await
    }

    async fn write_blob_with_sha256(
        &self,
        key: &BlobKey,
        bytes: &[u8],
        sha256: Option<&str>,
    ) -> Result<(), StorageError> {
        let path = key.to_path();
        let digest = sha256
            .map(|s| s.to_string())
            .unwrap_or_else(|| sha256_digest(bytes));

        if bytes.len() <= MULTIPART_THRESHOLD {
            let body = aws_sdk_s3::primitives::ByteStream::from(bytes.to_vec());
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(&path)
                .metadata("sha256", &digest)
                .body(body)
                .send()
                .await
                .map_err(|err| StorageError::Io(std::io::Error::other(err)))?;
            Ok(())
        } else {
            // Multipart upload for large payloads
            let create_res = self
                .client
                .create_multipart_upload()
                .bucket(&self.bucket)
                .key(&path)
                .metadata("sha256", &digest)
                .send()
                .await
                .map_err(|err| StorageError::Io(std::io::Error::other(err)))?;

            let upload_id = create_res
                .upload_id()
                .ok_or_else(|| StorageError::Io(std::io::Error::other("Missing upload_id")))?;

            let mut completed_parts = Vec::new();

            for (part_number, chunk) in (1..).zip(bytes.chunks(PART_SIZE)) {
                let body = aws_sdk_s3::primitives::ByteStream::from(chunk.to_vec());
                let part_res = match self
                    .client
                    .upload_part()
                    .bucket(&self.bucket)
                    .key(&path)
                    .upload_id(upload_id)
                    .part_number(part_number)
                    .body(body)
                    .send()
                    .await
                {
                    Ok(res) => res,
                    Err(err) => {
                        let _ = self
                            .client
                            .abort_multipart_upload()
                            .bucket(&self.bucket)
                            .key(&path)
                            .upload_id(upload_id)
                            .send()
                            .await;
                        return Err(StorageError::Io(std::io::Error::other(err)));
                    }
                };

                let etag = part_res.e_tag().unwrap_or_default().to_string();
                completed_parts.push(
                    aws_sdk_s3::types::CompletedPart::builder()
                        .part_number(part_number)
                        .e_tag(etag)
                        .build(),
                );
            }

            let completed_upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build();

            if let Err(err) = self
                .client
                .complete_multipart_upload()
                .bucket(&self.bucket)
                .key(&path)
                .upload_id(upload_id)
                .multipart_upload(completed_upload)
                .send()
                .await
            {
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(&path)
                    .upload_id(upload_id)
                    .send()
                    .await;
                return Err(StorageError::Io(std::io::Error::other(err)));
            }

            Ok(())
        }
    }

    async fn delete_blob(&self, key: &BlobKey) -> Result<(), StorageError> {
        let path = key.to_path();
        match self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(&path)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) if err.raw_response().map(|r| r.status().as_u16()) == Some(404) => Ok(()),
            Err(err) => Err(StorageError::Io(std::io::Error::other(err))),
        }
    }

    async fn exists_blob(&self, key: &BlobKey) -> Result<bool, StorageError> {
        match self.stat_blob(key).await {
            Ok(_) => Ok(true),
            Err(StorageError::NotFound(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }

    async fn blob_size(&self, key: &BlobKey) -> Result<u64, StorageError> {
        self.stat_blob(key).await.map(|stat| stat.size)
    }

    async fn stat_blob(&self, key: &BlobKey) -> Result<BlobStat, StorageError> {
        let path = key.to_path();
        let res = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&path)
            .send()
            .await
            .map_err(|err| Self::map_head_error(&path, err))?;

        let size = res.content_length().unwrap_or(0) as u64;
        let sha256 = res
            .metadata()
            .and_then(|meta| {
                meta.get("sha256")
                    .or_else(|| meta.get("x-amz-meta-sha256"))
                    .cloned()
            })
            .or_else(|| res.checksum_sha256().map(|s| s.to_string()));

        Ok(BlobStat { size, sha256 })
    }

    async fn list_prefix(&self, prefix: &BlobKey) -> Result<Vec<BlobKey>, StorageError> {
        let prefix_str = prefix.to_path();
        let mut continuation_token: Option<String> = None;
        let mut matched_keys = Vec::new();

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix_str);

            if let Some(token) = continuation_token {
                req = req.continuation_token(token);
            }

            let res = req
                .send()
                .await
                .map_err(|err| StorageError::Io(std::io::Error::other(err)))?;

            for obj in res.contents() {
                if let Some(key_str) = obj.key() {
                    if let Some(key) = BlobKey::from_path(key_str) {
                        if prefix.is_prefix_of(&key) {
                            matched_keys.push(key);
                        }
                    }
                }
            }

            if res.is_truncated().unwrap_or(false) {
                continuation_token = res.next_continuation_token().map(|s| s.to_string());
                if continuation_token.is_none() {
                    break;
                }
            } else {
                break;
            }
        }

        matched_keys.sort_by_key(|k| k.to_path());
        matched_keys.dedup();
        Ok(matched_keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::config::ResponseChecksumValidation;

    #[test]
    fn test_s3_blob_storage_client_has_response_checksum_validation_when_required() {
        let config = S3StorageConfig {
            bucket_name: "test-bucket".to_string(),
            endpoint: "http://localhost:9000".to_string(),
            access_key_id: "minioadmin".to_string(),
            secret_access_key: "minioadmin".to_string(),
            region: "us-east-1".to_string(),
            public_url: None,
        };
        let storage = S3BlobStorage::new(config);
        let client_conf = storage.client.config();

        assert_eq!(
            client_conf.response_checksum_validation(),
            Some(&ResponseChecksumValidation::WhenRequired)
        );
    }
}
