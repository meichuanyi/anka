//! Anka's AnkiWeb two-way sync agent.
//!
//! Wraps the official Anki engine (rslib) to keep a hidden Anki-format
//! "agent collection" in sync with AnkiWeb. Anka's own store mirrors
//! changes to/from the agent collection via its anki_id_map layer.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "anka-sync-agent", about = "Anka AnkiWeb two-way sync agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Two-way incremental sync between the agent collection and AnkiWeb
    Sync {
        /// Path to the agent collection (.anki2), created if missing
        #[arg(long)]
        agent: PathBuf,
        #[arg(long)]
        user: String,
        #[arg(long, env = "ANKIWEB_PASSWORD", hide_env_values = true)]
        password: String,
    },
    /// Replace the agent collection with the AnkiWeb copy (safe: local only)
    Pull {
        #[arg(long)]
        agent: PathBuf,
        #[arg(long)]
        user: String,
        #[arg(long, env = "ANKIWEB_PASSWORD", hide_env_values = true)]
        password: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Sync {
            agent,
            user,
            password,
        } => {
            println!("login {user} @ AnkiWeb ...");
            let client = reqwest::Client::new();
            let auth = anki::sync::login::sync_login(&user, &password, None, client.clone())
                .await
                .context("AnkiWeb 登录失败")?;
            println!("two-way sync ...");
            let mut col = open_col(&agent)?;
            let _output = col.normal_sync(auth, client).await?;
            println!("sync complete");
        }
        Command::Pull { agent, user, password } => {
            println!("login {user} @ AnkiWeb ...");
            let client = reqwest::Client::new();
            let auth = anki::sync::login::sync_login(&user, &password, None, client.clone())
                .await
                .context("AnkiWeb 登录失败")?;
            println!("full download into agent collection ...");
            let col = open_col(&agent)?;
            col.full_download(auth, client).await?;
            println!("agent collection replaced with AnkiWeb copy");
        }
    }
    Ok(())
}

fn open_col(path: &PathBuf) -> Result<anki::collection::Collection> {
    anki::collection::CollectionBuilder::new(path)
        .build()
        .context("打开 agent 收藏失败")
}
