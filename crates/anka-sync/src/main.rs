//! Self-hosted Anka sync server.
//!
//! Env:
//! - `ANKA_SYNC_TOKEN` — bearer token (empty = open, for dev)
//! - `ANKA_SYNC_DB` — sqlite path (default `./data/sync.db`)
//! - `ANKA_SYNC_LISTEN` — bind addr (default `0.0.0.0:8788`)

mod api;
mod protocol;
mod store;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::api::AppState;
use crate::store::SyncStore;

#[derive(Parser, Debug)]
#[command(name = "anka-sync", version, about = "Anka sync server")]
struct Args {
    /// Listen address
    #[arg(long, env = "ANKA_SYNC_LISTEN", default_value = "0.0.0.0:8788")]
    listen: SocketAddr,

    /// SQLite database path
    #[arg(long, env = "ANKA_SYNC_DB", default_value = "./data/sync.db")]
    db: PathBuf,

    /// Bearer token (empty disables auth — lab only)
    #[arg(long, env = "ANKA_SYNC_TOKEN", default_value = "")]
    token: String,
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
    let store = SyncStore::open(&args.db)?;
    let token = args.token.trim().to_string();
    if token.is_empty() {
        tracing::warn!("ANKA_SYNC_TOKEN is empty — auth disabled (dev only)");
    }

    let state = Arc::new(AppState {
        store: tokio::sync::Mutex::new(store),
        token,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = api::router(state).layer(cors).layer(TraceLayer::new_for_http());

    tracing::info!("anka-sync listening on {} db={}", args.listen, args.db.display());
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
