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
    /// Add notes to the agent collection locally (no network)
    AddBatch {
        #[arg(long)]
        agent: PathBuf,
        /// JSON file: array of {deck, fields[2], tags}
        #[arg(long)]
        file: PathBuf,
        /// Where to write the added notes' new Anki ids (JSON array)
        #[arg(long)]
        map_out: PathBuf,
    },
    /// Login to AnkiWeb and save the session hkey (password used once)
    Login {
        #[arg(long)]
        user: String,
        #[arg(long, env = "ANKIWEB_PASSWORD", hide_env_values = true)]
        password: String,
        /// Where to write {"hkey": ...}
        #[arg(long)]
        hkey_out: PathBuf,
    },
    /// Two-way incremental sync between the agent collection and AnkiWeb
    Sync {
        /// Path to the agent collection (.anki2), created if missing
        #[arg(long)]
        agent: PathBuf,
        /// AnkiWeb email (omit if --hkey given)
        #[arg(long)]
        user: Option<String>,
        /// AnkiWeb password (omit if --hkey given)
        #[arg(long, env = "ANKIWEB_PASSWORD", hide_env_values = true)]
        password: Option<String>,
        /// Session hkey from a previous login (preferred: no password needed)
        #[arg(long)]
        hkey: Option<String>,
        /// Optional JSON file of notes to add before syncing
        /// (array of {deck, fields[2], tags})
        #[arg(long)]
        add_batch: Option<PathBuf>,
        /// Where to write the added notes' new Anki ids (JSON array)
        #[arg(long)]
        map_out: Option<PathBuf>,
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
        Command::AddBatch {
            agent,
            file,
            map_out,
        } => {
            let mut col = open_col(&agent)?;
            let added = add_batch_notes(&mut col, &file)?;
            std::fs::write(&map_out, serde_json::to_vec_pretty(&added)?)?;
            println!("added {} notes", added.len());
        }
        Command::Login {
            user,
            password,
            hkey_out,
        } => {
            let client = reqwest::Client::new();
            let auth = anki::sync::login::sync_login(&user, &password, None, client)
                .await
                .context("AnkiWeb 登录失败")?;
            std::fs::write(
                &hkey_out,
                serde_json::to_vec_pretty(&serde_json::json!({ "hkey": auth.hkey }))?,
            )?;
            println!("login ok, hkey saved");
        }
        Command::Sync {
            agent,
            user,
            password,
            hkey,
            ..
        } => {
            let client = reqwest::Client::new();
            let auth = match (hkey, user, password) {
                (Some(k), _, _) => anki::sync::login::SyncAuth {
                    hkey: k,
                    endpoint: None,
                    io_timeout_secs: None,
                },
                (None, Some(u), Some(p)) => {
                    println!("login {u} @ AnkiWeb ...");
                    anki::sync::login::sync_login(&u, &p, None, client.clone())
                        .await
                        .context("AnkiWeb 登录失败")?
                }
                _ => anyhow::bail!("需要 --hkey 或 --user/--password"),
            };
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

#[derive(serde::Deserialize)]
struct BatchNote {
    deck: String,
    fields: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(serde::Serialize)]
struct AddedNote {
    index: usize,
    note_id: i64,
    card_ids: Vec<i64>,
}

fn add_batch_notes(
    col: &mut anki::collection::Collection,
    batch_file: &PathBuf,
) -> Result<Vec<AddedNote>> {
    let raw = std::fs::read_to_string(batch_file)?;
    let batch: Vec<BatchNote> = serde_json::from_str(&raw).context("解析 add_batch JSON 失败")?;
    let notetype = col
        .get_notetype_by_name("Basic")?
        .ok_or_else(|| anyhow::anyhow!("Basic notetype not found"))?;
    let mut out = Vec::new();
    for (index, b) in batch.into_iter().enumerate() {
        let deck = col.get_or_create_normal_deck(&b.deck)?;
        let mut note = anki::notes::Note::new(&notetype);
        for (i, f) in b.fields.iter().enumerate() {
            note.set_field(i, f.clone())?;
        }
        note.tags = b.tags;
        col.add_note(&mut note, deck.id)?;
        let card_ids = col
            .search_cards(
                anki::search::SearchNode::CardIds(note.id.0.to_string()),
                anki::search::SortMode::NoOrder,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .into_iter()
            .map(|cid| cid.0)
            .collect();
        out.push(AddedNote {
            index,
            note_id: note.id.0,
            card_ids,
        });
    }
    Ok(out)
}
