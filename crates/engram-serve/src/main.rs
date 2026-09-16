//! Local HTTP API for Engram review UI (browser / non-Tauri).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use engram_core::Collection;
use tower_http::cors::{Any, CorsLayer};

use engram_serve::AppState;

#[derive(Parser, Debug)]
#[command(name = "engram-serve", about = "Serve Engram review UI + JSON API")]
struct Args {
    /// Collection database
    #[arg(long, env = "ENGRAM_COLLECTION")]
    collection: Option<PathBuf>,

    /// Listen address
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,

    /// Optional static dist directory (apps/desktop/dist)
    #[arg(long)]
    static_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    let path = args
        .collection
        .unwrap_or_else(|| PathBuf::from("collection.egdb"));
    let collection = Collection::open_or_create(&path)?;
    let state = Arc::new(AppState {
        collection: std::sync::Mutex::new(collection),
        path: path.clone(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = engram_serve::router(state).layer(cors);

    let media_dir = engram_core::media_dir_for_collection(&path);
    if media_dir.is_dir() {
        tracing::info!("serving media from {}", media_dir.display());
        app = app.nest_service("/media", tower_http::services::ServeDir::new(media_dir));
    }

    if let Some(dir) = args.static_dir.clone().or_else(default_static) {
        if dir.is_dir() {
            tracing::info!("serving UI from {}", dir.display());
            app = app.fallback_service(tower_http::services::ServeDir::new(dir));
        }
    }

    tracing::info!(
        "engram serve on http://{}  collection={}",
        args.listen,
        path.display()
    );
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn default_static() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("apps/desktop/dist"),
        PathBuf::from("../apps/desktop/dist"),
        PathBuf::from("../../apps/desktop/dist"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}
