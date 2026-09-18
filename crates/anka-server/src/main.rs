//! Self-hosted Anka server.
//!
//! - REST API for UI (`/api/*`)
//! - Remote MCP (JSON-RPC over HTTP) at `POST /mcp`
//! - Static UI + media
//!
//! Auth: `Authorization: Bearer <ANKA_SERVER_TOKEN>` on `/api/*` and `/mcp`
//! when token is non-empty.

mod auth;
mod mcp_http;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use anka_core::Collection;
use anka_serve::AppState;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::RequireToken;

#[derive(Parser, Debug)]
#[command(name = "anka-server", about = "Self-hosted Anka (API + MCP + UI)")]
struct Args {
    #[arg(long, env = "ANKA_COLLECTION", default_value = "/data/collection.akdb")]
    collection: PathBuf,

    #[arg(long, env = "ANKA_SERVER_LISTEN", default_value = "0.0.0.0:8787")]
    listen: SocketAddr,

    /// Bearer token; empty = open (dev only)
    #[arg(long, env = "ANKA_SERVER_TOKEN", default_value = "")]
    token: String,

    #[arg(long, env = "ANKA_SERVER_STATIC")]
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
    if let Some(parent) = args.collection.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let collection = Collection::open_or_create(&args.collection)?;
    let token = args.token.trim().to_string();
    if token.is_empty() {
        tracing::warn!("ANKA_SERVER_TOKEN empty — auth disabled (dev only)");
    }

    let state = Arc::new(AppState {
        collection: std::sync::Mutex::new(collection),
        path: args.collection.clone(),
    });

    let auth = RequireToken::new(token.clone());

    // Public health (no auth) for probes.
    let public = axum::Router::new().route("/health", axum::routing::get(health));

    // Protected API + MCP
    let protected = anka_serve::router(state.clone())
        .merge(mcp_http::router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(auth, auth::middleware));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = public.merge(protected).layer(cors).layer(TraceLayer::new_for_http());

    let media_dir = anka_core::media_dir_for_collection(&args.collection);
    if media_dir.is_dir() {
        tracing::info!("media: {}", media_dir.display());
        app = app.nest_service("/media", tower_http::services::ServeDir::new(media_dir));
    }

    if let Some(dir) = args.static_dir.clone().or_else(default_static) {
        if dir.is_dir() {
            tracing::info!("UI: {}", dir.display());
            app = app.fallback_service(tower_http::services::ServeDir::new(dir));
        }
    }

    tracing::info!(
        "anka-server {} collection={}",
        args.listen,
        args.collection.display()
    );
    // One-click client setup: opening this URL saves the connection token
    // automatically (server + token pair), no manual entry needed.
    if !token.is_empty() {
        tracing::info!(
            "setup link: http://{}:#t={}  (open in browser to auto-configure)",
            args.listen,
            token
        );
    }
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "ok": true, "app": "anka-server" }))
}

fn default_static() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("apps/desktop/dist"),
        PathBuf::from("dist"),
        PathBuf::from("/app/dist"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}
