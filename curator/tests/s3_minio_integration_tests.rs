//! Integration tests for [`S3BlobStorage`] against an S3-compatible MinIO endpoint.
//!
//! Tests put -> stat -> get byte-identical round-trips for both single-part and multipart
//! uploads (> 5 MiB), verify that stat returns size and SHA-256 without downloading the payload,
//! and tests prefix listing, deletion, and public URL resolution.

use std::sync::Arc;

use curator::adapters::s3_blob_storage::{S3BlobStorage, S3StorageConfig, MULTIPART_THRESHOLD};
use curator::ports::blob_storage::{sha256_digest, BlobKey, BlobStorage};
use curator::ports::storage_error::StorageError;

struct EphemeralMinio {
    container_id: Option<String>,
    endpoint: String,
}

impl EphemeralMinio {
    async fn start() -> Self {
        // 1. Check if MINIO_ENDPOINT or R2_ENDPOINT is configured
        if let Ok(endpoint) =
            std::env::var("MINIO_ENDPOINT").or_else(|_| std::env::var("R2_ENDPOINT"))
        {
            if Self::is_healthy(&endpoint).await {
                return Self {
                    container_id: None,
                    endpoint,
                };
            }
        }

        // 2. Check default localhost:9000
        let default_ep = "http://127.0.0.1:9000".to_string();
        if Self::is_healthy(&default_ep).await {
            return Self {
                container_id: None,
                endpoint: default_ep,
            };
        }

        // 3. Start ephemeral container via docker
        for image in ["minio/minio:latest", "alpine/minio:latest-release"] {
            let output = std::process::Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "-p",
                    "127.0.0.1::9000",
                    "-e",
                    "MINIO_ROOT_USER=minioadmin",
                    "-e",
                    "MINIO_ROOT_PASSWORD=minioadmin",
                    image,
                    "server",
                    "/tmp/miniodata",
                ])
                .output();

            if let Ok(output) = output {
                if output.status.success() {
                    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let port_output = std::process::Command::new("docker")
                        .args(["port", &container_id, "9000/tcp"])
                        .output();
                    if let Ok(port_out) = port_output {
                        let host_port_str =
                            String::from_utf8_lossy(&port_out.stdout).trim().to_string();
                        let host_port = host_port_str
                            .lines()
                            .next()
                            .unwrap_or(&host_port_str)
                            .trim();
                        let endpoint = format!("http://{host_port}");

                        for _ in 0..50 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                            if Self::is_healthy(&endpoint).await {
                                return Self {
                                    container_id: Some(container_id),
                                    endpoint,
                                };
                            }
                        }
                    }
                    let _ = std::process::Command::new("docker")
                        .args(["rm", "-f", &container_id])
                        .output();
                }
            }
        }

        panic!("Could not connect to or start a MinIO instance for integration testing");
    }

    async fn is_healthy(endpoint: &str) -> bool {
        let health_url = format!("{endpoint}/minio/health/live");
        if let Ok(client) = reqwest::Client::builder()
            .timeout(tokio::time::Duration::from_millis(400))
            .build()
        {
            if let Ok(res) = client.get(&health_url).send().await {
                return res.status().is_success();
            }
        }
        false
    }
}

impl Drop for EphemeralMinio {
    fn drop(&mut self) {
        if let Some(id) = &self.container_id {
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", id])
                .output();
        }
    }
}

async fn setup_minio_storage(
    endpoint: &str,
    bucket: &str,
    public_url: Option<String>,
) -> S3BlobStorage {
    let config = S3StorageConfig {
        bucket_name: bucket.to_string(),
        endpoint: endpoint.to_string(),
        access_key_id: "minioadmin".to_string(),
        secret_access_key: "minioadmin".to_string(),
        region: "us-east-1".to_string(),
        public_url,
    };
    let storage = S3BlobStorage::new(config);

    // Ensure bucket exists
    let creds = aws_sdk_s3::config::Credentials::new(
        "minioadmin",
        "minioadmin",
        None,
        None,
        "test_minio",
    );
    let s3_conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .credentials_provider(creds)
        .region(aws_sdk_s3::config::Region::new("us-east-1"))
        .endpoint_url(endpoint)
        .force_path_style(true)
        .response_checksum_validation(aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired)
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_conf);

    let _ = client.create_bucket().bucket(bucket).send().await;

    storage
}

#[tokio::test]
async fn test_minio_single_part_and_multipart_round_trip() {
    let minio = EphemeralMinio::start().await;
    let bucket = "test-artifacts-bucket";
    let public_url = Some("https://pub-r2.dev".to_string());
    let storage = setup_minio_storage(&minio.endpoint, bucket, public_url).await;

    // 1. Single-part upload (< 5 MiB)
    let small_data = b"small dataset snapshot payload for unit verification".to_vec();
    let small_key = BlobKey::DatasetSnapshot {
        id: "snapshot-01".into(),
    };
    let small_sha256 = sha256_digest(&small_data);

    assert_eq!(small_key.to_path(), "artifacts/datasets/snapshot-01.zip");
    assert!(small_key.is_public_artifact());
    assert_eq!(
        storage.resolve_public_url(&small_key),
        Some("https://pub-r2.dev/artifacts/datasets/snapshot-01.zip".into())
    );

    // Initial check: not found
    assert!(!storage.exists_blob(&small_key).await.unwrap());
    assert!(matches!(
        storage.read_blob(&small_key).await,
        Err(StorageError::NotFound(_))
    ));
    assert!(matches!(
        storage.stat_blob(&small_key).await,
        Err(StorageError::NotFound(_))
    ));

    // Put small blob
    storage.write_blob(&small_key, &small_data).await.unwrap();

    // Verify exists
    assert!(storage.exists_blob(&small_key).await.unwrap());

    // Stat without downloading: size and SHA-256 match
    let small_stat = storage.stat_blob(&small_key).await.unwrap();
    assert_eq!(small_stat.size, small_data.len() as u64);
    assert_eq!(small_stat.sha256, Some(small_sha256.clone()));

    // Get blob: byte-identical
    let small_read = storage.read_blob(&small_key).await.unwrap();
    assert_eq!(small_read, small_data);

    // 2. Multipart upload (> 5 MiB threshold, e.g. 6 MiB)
    assert_eq!(MULTIPART_THRESHOLD, 5 * 1024 * 1024);
    let large_size = 6 * 1024 * 1024; // 6 MiB
    let mut large_data = Vec::with_capacity(large_size);
    for i in 0..large_size {
        large_data.push((i % 251) as u8);
    }
    let large_sha256 = sha256_digest(&large_data);
    let large_key = BlobKey::CheckpointAdapter {
        run_id: "run-42".into(),
        step: 1000,
    };

    assert_eq!(
        large_key.to_path(),
        "artifacts/runs/run-42/checkpoints/step-1000/adapter.safetensors"
    );
    assert!(large_key.is_public_artifact());

    // Write large blob (exercises multipart upload)
    storage.write_blob(&large_key, &large_data).await.unwrap();

    // Stat without downloading: size and SHA-256 match
    let large_stat = storage.stat_blob(&large_key).await.unwrap();
    assert_eq!(large_stat.size, large_data.len() as u64);
    assert_eq!(large_stat.sha256, Some(large_sha256.clone()));

    // Verify blob_size helper
    assert_eq!(storage.blob_size(&large_key).await.unwrap(), large_data.len() as u64);

    // Get large blob: byte-identical
    let large_read = storage.read_blob(&large_key).await.unwrap();
    assert_eq!(large_read.len(), large_data.len());
    assert_eq!(large_read, large_data);

    // 3. Samples image key under same checkpoint
    let samples_key = BlobKey::CheckpointSamples {
        run_id: "run-42".into(),
        step: 1000,
    };
    let samples_data = b"png-preview-grid-bytes".to_vec();
    storage.write_blob(&samples_key, &samples_data).await.unwrap();

    // 4. Published adapter key
    let pub_adapter_key = BlobKey::PublishedAdapter {
        id: "run-42-s1000".into(),
    };
    let pub_adapter_data = b"published-adapter-safetensors".to_vec();
    storage.write_blob(&pub_adapter_key, &pub_adapter_data).await.unwrap();

    // 5. list_prefix tests
    let checkpoint_prefix = BlobKey::CheckpointDir {
        run_id: "run-42".into(),
        step: 1000,
    };
    let listed_checkpoint = storage.list_prefix(&checkpoint_prefix).await.unwrap();
    assert_eq!(listed_checkpoint.len(), 2);
    assert!(listed_checkpoint.contains(&large_key));
    assert!(listed_checkpoint.contains(&samples_key));

    let run_prefix = BlobKey::RunCheckpointsDir {
        run_id: "run-42".into(),
    };
    let listed_run = storage.list_prefix(&run_prefix).await.unwrap();
    assert_eq!(listed_run.len(), 2);

    let datasets_prefix = BlobKey::DatasetSnapshotsDir;
    let listed_datasets = storage.list_prefix(&datasets_prefix).await.unwrap();
    assert_eq!(listed_datasets, vec![small_key.clone()]);

    let adapters_prefix = BlobKey::PublishedAdaptersDir;
    let listed_adapters = storage.list_prefix(&adapters_prefix).await.unwrap();
    assert_eq!(listed_adapters, vec![pub_adapter_key.clone()]);

    let all_artifacts_prefix = BlobKey::ArtifactsDir;
    let listed_all = storage.list_prefix(&all_artifacts_prefix).await.unwrap();
    assert_eq!(listed_all.len(), 4);

    // 6. Delete blob and idempotent delete
    storage.delete_blob(&small_key).await.unwrap();
    assert!(!storage.exists_blob(&small_key).await.unwrap());
    assert!(matches!(
        storage.read_blob(&small_key).await,
        Err(StorageError::NotFound(_))
    ));
    // Idempotent delete succeeds
    storage.delete_blob(&small_key).await.unwrap();

    storage.delete_blob(&large_key).await.unwrap();
    assert!(!storage.exists_blob(&large_key).await.unwrap());
}

#[tokio::test]
async fn test_arc_dyn_s3_blob_storage_port() {
    let minio = EphemeralMinio::start().await;
    let bucket = "test-arc-bucket";
    let storage = Arc::new(setup_minio_storage(&minio.endpoint, bucket, None).await);
    let dyn_storage: Arc<dyn BlobStorage> = storage.clone();

    let key = BlobKey::DatasetSnapshot {
        id: "snapshot-dyn".into(),
    };
    let data = b"payload via arc dyn BlobStorage".to_vec();
    dyn_storage.write_blob(&key, &data).await.unwrap();

    assert!(dyn_storage.exists_blob(&key).await.unwrap());
    let stat = dyn_storage.stat_blob(&key).await.unwrap();
    assert_eq!(stat.size, data.len() as u64);
    assert_eq!(stat.sha256, Some(sha256_digest(&data)));

    let read = dyn_storage.read_blob(&key).await.unwrap();
    assert_eq!(read, data);

    dyn_storage.delete_blob(&key).await.unwrap();
    assert!(!dyn_storage.exists_blob(&key).await.unwrap());
}

