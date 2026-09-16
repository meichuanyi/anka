//! Engram MCP: local stdio against a collection, or stdio→HTTP bridge to engram-server.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use clap::Parser;
use engram_core::Collection;
use engram_mcp::{handle_message, ClientMessage};

#[derive(Parser, Debug)]
#[command(name = "engram-mcp", version, about = "MCP server for Engram")]
struct Args {
    /// Local collection (stdio mode)
    #[arg(long, env = "ENGRAM_COLLECTION")]
    collection: Option<PathBuf>,

    /// Remote MCP endpoint, e.g. http://nas:8787/mcp
    #[arg(long, env = "ENGRAM_MCP_URL")]
    remote_url: Option<String>,

    /// Bearer token for remote MCP
    #[arg(long, env = "ENGRAM_SERVER_TOKEN", default_value = "")]
    token: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let args = Args::parse();
    if let Some(url) = args.remote_url {
        return run_bridge(&url, &args.token);
    }

    let path = args
        .collection
        .unwrap_or_else(|| PathBuf::from("collection.egdb"));
    let col = Collection::open_or_create(&path)?;
    run_stdio_local(&Mutex::new(col))
}

fn run_stdio_local(state: &Mutex<Collection>) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<ClientMessage>(line) else {
            continue;
        };
        if msg.id.is_none() {
            continue;
        }
        let response = handle_message(state, msg);
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

/// Forward stdio JSON-RPC lines to remote `POST /mcp` via curl (no extra HTTP dep).
fn run_bridge(url: &str, token: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cmd = std::process::Command::new("curl");
        cmd.arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg(url)
            .arg("-H")
            .arg("content-type: application/json");
        if !token.is_empty() {
            cmd.arg("-H").arg(format!("Authorization: Bearer {token}"));
        }
        cmd.arg("--data-binary").arg(line);
        let out = cmd.output()?;
        if !out.status.success() {
            tracing::warn!("bridge error: {}", String::from_utf8_lossy(&out.stderr));
            continue;
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            continue;
        }
        writeln!(stdout, "{trimmed}")?;
        stdout.flush()?;
    }
    Ok(())
}
