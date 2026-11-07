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
    if let Some(dir) = non_empty_var("CURATOR_DATA_DIR").or_else(|| non_empty_var("DATA_DIR")) {
        return PathBuf::from(dir);
    }

    default_user_data_dir()
}

/// Reads an environment variable, treating an empty value as unset so a blank `DATA_DIR=`
/// in a `.env` file cannot redirect storage to the current directory.
fn non_empty_var(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    non_empty_var("HOME")
        .or_else(|| non_empty_var("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_user_data_dir() -> PathBuf {
    let home = home_dir();

    #[cfg(target_os = "windows")]
    {
        let base = non_empty_var("APPDATA")
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
        let base = non_empty_var("XDG_DATA_HOME")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Environment variables are process-global, so these tests serialize and restore them.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sets environment variables for the test body and restores them on drop.
    struct EnvGuard {
        previous: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn set(vars: &[(&'static str, Option<&str>)]) -> Self {
            let previous = vars
                .iter()
                .map(|(key, _)| (*key, std::env::var_os(key)))
                .collect();
            for (key, value) in vars {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.previous {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    fn test_resolve_data_dir_curator_override_wins_over_data_dir() {
        let _lock = env_lock();
        let _env = EnvGuard::set(&[
            ("CURATOR_DATA_DIR", Some("/tmp/curator-override")),
            ("DATA_DIR", Some("/tmp/data-override")),
        ]);

        assert_eq!(resolve_data_dir(), PathBuf::from("/tmp/curator-override"));
    }

    #[test]
    fn test_resolve_data_dir_skips_empty_override_and_uses_data_dir() {
        let _lock = env_lock();
        let _env = EnvGuard::set(&[
            ("CURATOR_DATA_DIR", Some("")),
            ("DATA_DIR", Some("/tmp/data-override")),
        ]);

        assert_eq!(resolve_data_dir(), PathBuf::from("/tmp/data-override"));
    }

    #[test]
    fn test_resolve_data_dir_falls_back_to_platform_user_data_dir() {
        let _lock = env_lock();
        let home = PathBuf::from("/tmp/curator-home");
        let _env = EnvGuard::set(&[
            ("CURATOR_DATA_DIR", Some("")),
            ("DATA_DIR", Some("")),
            ("XDG_DATA_HOME", Some("")),
            ("APPDATA", Some("")),
            ("HOME", Some("/tmp/curator-home")),
            ("USERPROFILE", Some("")),
        ]);

        #[cfg(target_os = "windows")]
        let expected = home.join("AppData").join("Roaming").join("battlemap-builder");
        #[cfg(target_os = "macos")]
        let expected = home
            .join("Library")
            .join("Application Support")
            .join("battlemap-builder");
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let expected = home.join(".local").join("share").join("battlemap-builder");

        assert_eq!(resolve_data_dir(), expected);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn test_resolve_data_dir_uses_non_empty_xdg_data_home() {
        let _lock = env_lock();
        let _env = EnvGuard::set(&[
            ("CURATOR_DATA_DIR", None),
            ("DATA_DIR", None),
            ("XDG_DATA_HOME", Some("/tmp/xdg-data")),
            ("HOME", Some("/tmp/curator-home")),
        ]);

        assert_eq!(
            resolve_data_dir(),
            PathBuf::from("/tmp/xdg-data").join("battlemap-builder")
        );
    }
}
