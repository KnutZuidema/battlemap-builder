use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use curator::adapters::{FileCostLedger, FilesystemStorage, RedditCrawler};
use curator::api::{create_router, AppState};
use curator::ports::storage::StorageRepository;
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

    let port = resolve_port();
    let data_dir = resolve_data_dir();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(
        data_dir = %data_dir.display(),
        "Initializing filesystem storage repository..."
    );
    let fs_storage = FilesystemStorage::new(&data_dir);
    if let Err(err) = fs_storage.migrate_legacy_storage() {
        tracing::warn!(error = %err, "Failed to migrate legacy storage on startup");
    }
    let storage: Arc<dyn StorageRepository + Send + Sync> = Arc::new(fs_storage);

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

    let state = AppState::with_all(storage, Arc::new(crawler), cost_tracker);
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
