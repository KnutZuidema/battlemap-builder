use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use curator::adapters::{
    FileCostLedger, FilesystemBlobStorage, FilesystemCatalogRepository, RedditCrawler,
};
use curator::api::{create_router, AppState};
use curator::ports::blob_storage::BlobStorage;
use curator::ports::catalog_repository::CatalogRepository;
use curator::ports::harvest_run_ledger::HarvestRunLedger;
use tracing_subscriber::EnvFilter;

fn resolve_port() -> u16 {
    std::env::var("CURATOR_PORT")
        .or_else(|_| std::env::var("PORT"))
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(3002)
}

fn resolve_data_dir() -> PathBuf {
    std::env::var("CURATOR_DATA_DIR")
        .or_else(|_| std::env::var("DATA_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"))
}

/// Multi-location .env loader checking:
/// 1. `CURATOR_ENV_FILE` override
/// 2. `dotenvy::dotenv()` (current directory & parent directories)
/// 3. `curator/.env` (relative to current directory if executed from repo root)
/// 4. `../.env`
pub fn load_dotenv() -> Option<PathBuf> {
    // 1. Explicit CURATOR_ENV_FILE override
    if let Ok(env_path) = std::env::var("CURATOR_ENV_FILE") {
        let path = PathBuf::from(&env_path);
        if path.is_file() && dotenvy::from_path(&path).is_ok() {
            tracing::info!(
                path = %path.display(),
                "Loaded .env configuration from CURATOR_ENV_FILE"
            );
            return Some(path);
        }
    }

    // 2. Default dotenvy resolution (searches current directory & parents)
    if let Ok(path) = dotenvy::dotenv() {
        tracing::info!(
            path = %path.display(),
            "Loaded .env configuration from current/parent directory"
        );
        return Some(path);
    }

    // 3. curator/.env (relative to current directory if executed from repo root)
    let curator_env = Path::new("curator/.env");
    if curator_env.is_file() && dotenvy::from_path(curator_env).is_ok() {
        let path = curator_env.to_path_buf();
        tracing::info!(
            path = %path.display(),
            "Loaded .env configuration from curator/.env"
        );
        return Some(path);
    }

    // 4. ../.env
    let parent_env = Path::new("../.env");
    if parent_env.is_file() && dotenvy::from_path(parent_env).is_ok() {
        let path = parent_env.to_path_buf();
        tracing::info!(
            path = %path.display(),
            "Loaded .env configuration from ../.env"
        );
        return Some(path);
    }

    tracing::warn!("No .env configuration file discovered in candidate locations");
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load environment file from candidate paths
    let _ = load_dotenv();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "migrate-disk-to-s3" || arg == "--migrate-disk-to-s3") {
        let force = args.iter().any(|arg| arg == "--force" || arg == "-f");
        let dry_run = args.iter().any(|arg| arg == "--dry-run" || arg == "-n");
        return run_migration_cli(force, dry_run).await;
    }
    if args
        .iter()
        .any(|arg| arg == "migrate-disk-to-postgres" || arg == "--migrate-disk-to-postgres")
    {
        let force = args.iter().any(|arg| arg == "--force" || arg == "-f");
        let dry_run = args.iter().any(|arg| arg == "--dry-run" || arg == "-n");
        return run_catalog_migration_cli(force, dry_run).await;
    }

    let port = resolve_port();
    let data_dir = resolve_data_dir();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(
        data_dir = %data_dir.display(),
        "Initializing filesystem catalog and blob ports..."
    );
    let catalog_backend = curator::adapters::CatalogBackend::from_env();
    let (catalog, harvest_ledger): (
        Arc<dyn CatalogRepository + Send + Sync>,
        Arc<dyn HarvestRunLedger>,
    ) = match catalog_backend {
        curator::adapters::CatalogBackend::Postgres(db_url) => {
            tracing::info!("Initializing PostgreSQL catalog repository...");
            let repo = curator::adapters::PostgresCatalogRepository::connect(&db_url)
                .await
                .map_err(|e| format!("Failed to connect to PostgreSQL catalog: {e}"))?;
            repo.run_migrations()
                .await
                .map_err(|e| format!("Failed to run PostgreSQL catalog migrations: {e}"))?;
            let repo = Arc::new(repo);
            (repo.clone(), repo)
        }
        curator::adapters::CatalogBackend::Filesystem => {
            tracing::info!(
                data_dir = %data_dir.display(),
                "Initializing filesystem catalog repository..."
            );
            let catalog_repository = FilesystemCatalogRepository::new(&data_dir);
            let migrated = catalog_repository.migrate_legacy_generated_manifests();
            if migrated > 0 {
                tracing::info!(
                    count = migrated,
                    "Migrated legacy generated staging manifests to Candidate manifests"
                );
            }
            (
                Arc::new(catalog_repository),
                Arc::new(curator::adapters::NoopHarvestRunLedger),
            )
        }
    };
    let blob_backend = curator::adapters::BlobBackend::from_env()?;
    let blobs: Arc<dyn BlobStorage + Send + Sync> = match blob_backend {
        curator::adapters::BlobBackend::Filesystem => {
            tracing::info!(
                data_dir = %data_dir.display(),
                "Initializing filesystem blob storage"
            );
            Arc::new(FilesystemBlobStorage::new(&data_dir))
        }
        curator::adapters::BlobBackend::S3(config) => {
            tracing::info!(
                bucket = %config.bucket_name,
                endpoint = %config.endpoint,
                region = %config.region,
                public_url = ?config.public_url,
                "Initializing S3/R2 blob storage"
            );
            Arc::new(curator::adapters::S3BlobStorage::new(config))
        }
    };
    let cost_ledger_path = data_dir.join("telemetry/cost_ledger.json");
    tracing::info!(
        path = %cost_ledger_path.display(),
        "Initializing persistent cost ledger..."
    );
    let cost_tracker = Arc::new(FileCostLedger::new(&cost_ledger_path));

    let crawler = RedditCrawler::from_env();
    if let Some(creds) = crawler.credentials() {
        tracing::info!(
            client_id = %creds.client_id,
            user_agent = %crawler.user_agent(),
            base_url = %crawler.effective_base_url(),
            "Reddit harvester initialized with OAuth2 authentication"
        );
    } else {
        tracing::warn!(
            user_agent = %crawler.user_agent(),
            base_url = %crawler.effective_base_url(),
            "Reddit harvester initialized in UNAUTHENTICATED mode. Reddit public JSON endpoints may return 403 Forbidden. Configure REDDIT_CLIENT_ID and REDDIT_CLIENT_SECRET in .env."
        );
    }

    let state = AppState::with_all(Arc::new(crawler), cost_tracker, catalog, blobs)
        .with_harvest_run_ledger(harvest_ledger);

    // Resume any active/running training runs across server restart
    if let Err(e) = state.training_service.resume_non_terminal_runs().await {
        tracing::warn!("Failed to resume non-terminal training runs on startup: {e}");
    }
    let app = create_router(state);

    tracing::info!(
        port = port,
        bind_addr = %addr,
        "Curator server listening on http://{addr}"
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_migration_cli(force: bool, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = resolve_data_dir();
    tracing::info!(data_dir = %data_dir.display(), force, dry_run, "Connecting to S3 storage for migration...");

    let s3_config = curator::adapters::S3StorageConfig::from_env()?;
    tracing::info!(
        bucket = %s3_config.bucket_name,
        endpoint = %s3_config.endpoint,
        region = %s3_config.region,
        "Connected to S3 storage"
    );

    let target = Arc::new(curator::adapters::S3BlobStorage::new(s3_config));
    let options = curator::services::storage_migration::MigrationOptions {
        force,
        dry_run,
        ..Default::default()
    };

    let report = curator::services::storage_migration::run_storage_migration(&data_dir, target, options).await?;

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

/// Imports the filesystem catalog under the data directory into the PostgreSQL catalog.
async fn run_catalog_migration_cli(
    force: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = resolve_data_dir();
    let curator::adapters::CatalogBackend::Postgres(database_url) =
        curator::adapters::CatalogBackend::from_env()
    else {
        return Err(
            "DATABASE_URL is not set: 'migrate-disk-to-postgres' requires a PostgreSQL catalog \
             (for example postgres://curator:curator@postgres:5432/curator)"
                .into(),
        );
    };

    tracing::info!(
        data_dir = %data_dir.display(),
        force,
        dry_run,
        "Importing filesystem catalog into PostgreSQL..."
    );

    let repository = curator::adapters::PostgresCatalogRepository::connect(&database_url)
        .await
        .map_err(|e| format!("Failed to connect to PostgreSQL catalog: {e}"))?;
    if dry_run {
        tracing::info!("Dry run: skipping schema migrations and catalog writes");
    } else {
        repository
            .run_migrations()
            .await
            .map_err(|e| format!("Failed to run PostgreSQL catalog migrations: {e}"))?;
    }

    let options = curator::services::catalog_migration::DiskToPostgresOptions { force, dry_run };
    let report =
        curator::services::catalog_migration::run_disk_to_postgres_migration(&data_dir, &repository, options)
            .await?;

    print!("{}", report.render_summary());

    let totals = report.totals();
    if totals.failed > 0 {
        return Err(format!(
            "{} catalog entities failed to import; see the failure list above",
            totals.failed
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Serializes tests that mutate `CURATOR_ENV_FILE`, which is process-global.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_load_dotenv_from_custom_env_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut temp_file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(temp_file, "CURATOR_TEST_VAR_123=hello_world").unwrap();
        let path = temp_file.path().to_path_buf();

        std::env::set_var("CURATOR_ENV_FILE", path.to_str().unwrap());
        let loaded = load_dotenv();
        std::env::remove_var("CURATOR_ENV_FILE");

        assert_eq!(loaded, Some(path));
        assert_eq!(
            std::env::var("CURATOR_TEST_VAR_123").unwrap(),
            "hello_world"
        );
        std::env::remove_var("CURATOR_TEST_VAR_123");
    }

    #[test]
    fn test_load_dotenv_nonexistent_custom_env_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CURATOR_ENV_FILE", "/nonexistent/path/to/custom.env");
        let _ = load_dotenv();
        std::env::remove_var("CURATOR_ENV_FILE");
    }
}
