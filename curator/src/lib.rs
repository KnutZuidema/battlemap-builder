pub mod adapters;
pub mod api;
pub mod domain;
pub mod ports;
pub mod ui;
pub mod services;

use std::path::{Path, PathBuf};

/// Resolves the data directory from `CURATOR_DATA_DIR`, then `DATA_DIR`, defaulting to the
/// platform user data directory:
/// - Linux: `$XDG_DATA_HOME/battlemap-builder` or `~/.local/share/battlemap-builder`
/// - macOS: `~/Library/Application Support/battlemap-builder`
/// - Windows: `%APPDATA%\battlemap-builder`
pub fn resolve_data_dir() -> PathBuf {
    if let Some(dir) =
        std::env::var_os("CURATOR_DATA_DIR").or_else(|| std::env::var_os("DATA_DIR"))
    {
        return PathBuf::from(dir);
    }

    default_user_data_dir()
}

fn default_user_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join("AppData").join("Roaming")));
        return base
            .unwrap_or_else(|| PathBuf::from("."))
            .join("battlemap-builder");
    }

    #[cfg(target_os = "macos")]
    {
        return home
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("battlemap-builder");
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".local").join("share")));
        base.unwrap_or_else(|| PathBuf::from("."))
            .join("battlemap-builder")
    }
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
