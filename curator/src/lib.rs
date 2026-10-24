pub mod adapters;
pub mod api;
pub mod domain;
pub mod ports;
pub mod ui;
pub mod services;

use std::path::{Path, PathBuf};

/// Resolves the data directory from environment variables or defaults to "data".
pub fn resolve_data_dir() -> PathBuf {
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
