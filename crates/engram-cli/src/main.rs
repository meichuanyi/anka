use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use engram_core::{Collection, Rating};
use serde_json::json;

mod sync;

#[derive(Parser, Debug)]
#[command(name = "engram", version, about = "Modern AI-native spaced repetition")]
struct Cli {
    /// Path to the collection database (default: ./collection.egdb)
    #[arg(long, global = true, env = "ENGRAM_COLLECTION")]
    collection: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create or open the collection file
    Init,
    /// Import an Anki .apkg package
    Import {
        /// Path to .apkg
        path: PathBuf,
        /// Override default deck name for notes without a mapped deck
        #[arg(long)]
        deck: Option<String>,
    },
    /// Export the collection to an Anki .apkg package
    Export {
        /// Output .apkg path
        out: PathBuf,
        /// Only export this deck
        #[arg(long)]
        deck: Option<String>,
    },
    /// List decks and due counts
    Decks,
    /// List or search notes
    Notes {
        /// Substring match on fields/tags; omit to list recent
        query: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Add a basic front/back note
    Add {
        #[arg(long)]
        deck: String,
        #[arg(long)]
        front: String,
        #[arg(long)]
        back: String,
        #[arg(long)]
        tags: Vec<String>,
    },
    /// Interactive review session (or --show/--json for non-interactive)
    Review {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Filter by exact deck name (Chinese / hierarchical names ok)
        #[arg(long)]
        deck: Option<String>,
        /// Print due cards without grading
        #[arg(long)]
        show: bool,
        /// Machine-readable due cards (implies non-interactive)
        #[arg(long)]
        json: bool,
    },
    /// Collection stats
    Stats,
    /// Optimize FSRS parameters from review history
    Optimize {
        /// Persist optimized weights into the collection
        #[arg(long)]
        apply: bool,
    },
    /// Push/pull with engram-sync server
    Sync {
        #[command(subcommand)]
        cmd: SyncCmd,
    },
}

#[derive(Subcommand, Debug)]
enum SyncCmd {
    /// Upload local notes/cards/revlog to the server
    Push {
        #[arg(long, env = "ENGRAM_SYNC_SERVER", default_value = "http://127.0.0.1:8788")]
        server: String,
        #[arg(long, env = "ENGRAM_SYNC_TOKEN", default_value = "")]
        token: String,
    },
    /// Download remote changes and merge into local collection
    Pull {
        #[arg(long, env = "ENGRAM_SYNC_SERVER", default_value = "http://127.0.0.1:8788")]
        server: String,
        #[arg(long, env = "ENGRAM_SYNC_TOKEN", default_value = "")]
        token: String,
    },
}

fn resolve_collection(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("collection.egdb"))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let path = resolve_collection(cli.collection);

    match cli.command {
        Commands::Init => {
            let col = Collection::open_or_create(&path)?;
            println!("collection ready: {}", col.path().display());
        }
        Commands::Import { path: apkg, deck } => {
            let mut col = Collection::open_or_create(&path)?;
            if let Some(name) = deck {
                col.ensure_deck(&name)?;
            }
            let report = engram_apkg::import_apkg(&apkg, &mut col)?;
            println!("imported {}", apkg.display());
            println!(
                "  decks={} notes={} cards={} revlog={} media={}",
                report.decks, report.notes, report.cards, report.revlogs, report.media_copied
            );
            for w in &report.warnings {
                println!("  warn: {w}");
            }
            if report.skipped_cards > 0 {
                println!("  skipped_cards={}", report.skipped_cards);
            }
        }
        Commands::Export { out, deck } => {
            let col = Collection::open(&path)?;
            let options = engram_apkg::ExportOptions { deck };
            let report = engram_apkg::export_apkg(&col, &out, options)?;
            println!("exported {}", out.display());
            println!(
                "  decks={} notes={} cards={} media={}",
                report.decks, report.notes, report.cards, report.media_exported
            );
            for w in &report.warnings {
                println!("  warn: {w}");
            }
        }
        Commands::Decks => {
            let col = Collection::open(&path)?;
            let counts = col.deck_counts()?;
            if counts.is_empty() {
                println!("no decks yet — try: engram import some.apkg");
                return Ok(());
            }
            println!("{:<32} {:>6} {:>8} {:>8}", "deck", "new", "learning", "due");
            for d in counts {
                println!(
                    "{:<32} {:>6} {:>8} {:>8}",
                    truncate(&d.name, 32),
                    d.new_count,
                    d.learning_count,
                    d.review_count
                );
            }
        }
        Commands::Notes { query, limit } => {
            let col = Collection::open(&path)?;
            let (total, notes) = match query {
                Some(q) => col.search_notes(&q, limit, 0)?,
                None => col.search_notes("", limit, 0)?,
            };
            println!("{total} note(s)");
            for n in notes {
                let (front, back) = engram_core::front_back(&n.fields);
                println!(
                    "- [{}] {} | {}",
                    &n.id.to_string()[..8],
                    truncate(&front, 60),
                    truncate(&back, 40)
                );
            }
        }
        Commands::Add {
            deck,
            front,
            back,
            tags,
        } => {
            let mut col = Collection::open_or_create(&path)?;
            let d = col.ensure_deck(&deck)?;
            let (note, _card) = col.add_note(d.id, vec![front, back], tags)?;
            println!("added note {}", note.id);
        }
        Commands::Review { limit, deck, show, json } => {
            let mut col = Collection::open(&path)?;
            let deck_id = match deck {
                Some(name) => Some(resolve_deck_id(&col, &name)?),
                None => None,
            };
            let due = col.due(deck_id, limit)?;
            if json {
                let cards: Vec<_> = due
                    .iter()
                    .map(|item| {
                        let (front, back) = engram_core::front_back(&item.note.fields);
                        json!({
                            "cardId": item.card.id.to_string(),
                            "deckId": item.card.deck_id.to_string(),
                            "front": front,
                            "back": back,
                            "kind": format!("{:?}", item.card.state.kind()).to_lowercase(),
                            "dueAt": item.card.state.due_at.to_rfc3339(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "cards": cards }))?
                );
                return Ok(());
            }
            if due.is_empty() {
                println!("no cards due. nice.");
                return Ok(());
            }
            if !show && !std::io::stdin().is_terminal() {
                bail!(
                    "review needs an interactive terminal; use --show or --json for scripts\n\
                     example: engram review --deck <name> --json"
                );
            }
            println!("{} card(s) due\n", due.len());
            if show {
                for item in due {
                    let (front, back) = engram_core::front_back(&item.note.fields);
                    println!("--- {}\nQ: {front}\nA: {back}\n", item.card.id);
                }
                return Ok(());
            }
            run_review_session(&mut col, due)?;
        }
        Commands::Stats => {
            let col = Collection::open(&path)?;
            let counts = col.deck_counts()?;
            let mut new = 0u64;
            let mut learning = 0u64;
            let mut due = 0u64;
            for d in &counts {
                new += d.new_count;
                learning += d.learning_count;
                due += d.review_count;
            }
            println!("collection: {}", path.display());
            println!("decks: {}", counts.len());
            println!("new: {new}  learning: {learning}  due: {due}");
            println!("now: {}", Utc::now().to_rfc3339());
            if let Some(params) = col.fsrs_params()? {
                println!(
                    "fsrs: personalized ({} params)",
                    params.len()
                );
            } else {
                println!("fsrs: default weights");
            }
            if !counts.is_empty() {
                println!();
                println!("{:<36} {:>6} {:>8} {:>8}", "deck", "new", "learning", "due");
                for d in &counts {
                    println!(
                        "{:<36} {:>6} {:>8} {:>8}",
                        truncate(&d.name, 36),
                        d.new_count,
                        d.learning_count,
                        d.review_count
                    );
                }
            }
        }
        Commands::Optimize { apply } => {
            let mut col = Collection::open(&path)?;
            match col.optimize_fsrs(apply) {
                Ok(report) => {
                    println!(
                        "reviews={} items={} cards={}",
                        report.review_count, report.item_count, report.card_count
                    );
                    if let Some(ll) = report.log_loss {
                        println!("log_loss={ll:.4}");
                    }
                    println!("params={:?}", report.params);
                    if apply {
                        println!("applied personalized FSRS weights");
                    } else {
                        println!("preview only — re-run with --apply to save");
                    }
                }
                Err(e) => {
                    println!("optimize failed: {e}");
                    if report_min_hint(&e) {
                        println!("hint: need more review history (~{}+ answers)", engram_core::MIN_REVIEWS_FOR_OPTIMIZE);
                    }
                    std::process::exit(1);
                }
            }
        }
        Commands::Sync { cmd } => {
            let mut col = Collection::open_or_create(&path)?;
            match cmd {
                SyncCmd::Push { server, token } => {
                    let c = sync::SyncClient { server, token };
                    c.push(&mut col)?;
                }
                SyncCmd::Pull { server, token } => {
                    let c = sync::SyncClient { server, token };
                    c.pull(&mut col)?;
                }
            }
        }
    }

    Ok(())
}

fn report_min_hint(e: &engram_core::Error) -> bool {
    matches!(e, engram_core::Error::Invalid(_) | engram_core::Error::Scheduler(_))
}

/// Resolve an exact deck name, listing candidates on miss (helps Chinese / hierarchical names).
fn resolve_deck_id(col: &Collection, name: &str) -> Result<engram_core::Id> {
    let decks = col.list_decks()?;
    if let Some(d) = decks.iter().find(|d| d.name == name) {
        return Ok(d.id);
    }
    let mut msg = format!("deck not found: {name}");
    if decks.is_empty() {
        msg.push_str("\nno decks in collection yet — try: engram import some.apkg");
    } else {
        msg.push_str("\navailable decks:");
        for d in decks.iter().take(30) {
            msg.push_str(&format!("\n  {}", d.name));
        }
        if decks.len() > 30 {
            msg.push_str(&format!("\n  … and {} more", decks.len() - 30));
        }
    }
    Err(anyhow::anyhow!(msg))
}

fn run_review_session(col: &mut Collection, due: Vec<engram_core::DueCard>) -> Result<()> {
    use std::io::{BufRead, Write};

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let total = due.len();
    for (i, item) in due.into_iter().enumerate() {
        let (front, back) = engram_core::front_back(&item.note.fields);
        let front = if front.is_empty() {
            "(empty)".into()
        } else {
            front
        };
        let back = if back.is_empty() {
            "(no answer)".into()
        } else {
            back
        };

        println!("\n[{}/{}] {}", i + 1, total, truncate(&front, 120));
        print!("  press Enter to show answer… ");
        std::io::stdout().flush()?;
        let _ = lines.next();

        println!("  {}", truncate(&back, 200));
        print!("  grade [1=Again 2=Hard 3=Good 4=Easy] (q quit): ");
        std::io::stdout().flush()?;
        let Some(Ok(raw)) = lines.next() else { break };
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("q") {
            break;
        }
        let n: u8 = raw.parse().context("enter 1-4 or q")?;
        let Some(rating) = Rating::from_u8(n) else {
            bail!("rating must be 1-4");
        };
        let updated = col.answer_card(item.card.id, rating, 0)?;
        println!(
            "  → due {}  S={:.2} D={:.1}",
            updated.state.due_at.format("%Y-%m-%d %H:%M"),
            updated.state.stability,
            updated.state.difficulty
        );
    }
    println!("\nsession done.");
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
