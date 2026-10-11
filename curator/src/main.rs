use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use curator::adapters::FilesystemStorage;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load optional .env file
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let port = resolve_port();
    let data_dir = resolve_data_dir();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(
        data_dir = %data_dir.display(),
        "Initializing filesystem storage repository..."
    );
    let storage: Arc<dyn StorageRepository + Send + Sync> =
        Arc::new(FilesystemStorage::new(&data_dir));

    let state = AppState::new(storage);
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
