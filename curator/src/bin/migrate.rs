//! Standalone CLI executable to migrate local disk blobs to S3/Cloudflare R2 storage.
//!
//! Intended to be run as a one-time Kubernetes Job or one-off administrative task.

use std::path::PathBuf;
use std::sync::Arc;

use curator::adapters::{S3BlobStorage, S3StorageConfig};
use curator::services::storage_migration::{run_storage_migration, MigrationOptions};
use tracing_subscriber::EnvFilter;

fn resolve_data_dir() -> PathBuf {
    std::env::var("CURATOR_DATA_DIR")
        .or_else(|_| std::env::var("DATA_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Multi-location .env loader
    let _ = curator::load_dotenv();
    let args: Vec<String> = std::env::args().collect();
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let dry_run = args.iter().any(|a| a == "--dry-run" || a == "-n");

    let data_dir = resolve_data_dir();
    tracing::info!(data_dir = %data_dir.display(), force, dry_run, "Connecting to S3 storage...");

    let s3_config = S3StorageConfig::from_env()?;
    tracing::info!(
        bucket = %s3_config.bucket_name,
        endpoint = %s3_config.endpoint,
        region = %s3_config.region,
        "Connected to S3 storage"
    );

    let target = Arc::new(S3BlobStorage::new(s3_config));
    let options = MigrationOptions {
        force,
        dry_run,
        ..Default::default()
    };

    let report = run_storage_migration(&data_dir, target, options).await?;

    println!("==================================================");
    println!("Storage Migration Report");
    println!("==================================================");
    println!("Status:             SUCCESS");
    println!("Total Discovered:   {}", report.total_discovered);
    println!("Migrated:           {}", report.migrated_count);
    println!("Skipped (in S3):    {}", report.skipped_count);
    println!("Total Bytes:        {} bytes", report.total_bytes_migrated);
    println!("Dry Run:            {}", report.dry_run);
    println!("Completed At:       {}", report.completed_at);
    println!("==================================================");

    Ok(())
}
