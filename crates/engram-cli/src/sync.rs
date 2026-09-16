//! Client-side push/pull against engram-sync.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use engram_core::{Card, Collection, Id, Note, Rating, RevlogEntry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Change {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seq: Option<i64>,
    kind: String,
    id: String,
    updated_at: String,
    device_id: String,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResponse {
    changes: Vec<Change>,
    cursor: i64,
    #[allow(dead_code)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushResponse {
    accepted: u32,
    latest_seq: i64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SyncState {
    cursor: i64,
    device_id: String,
}

fn sync_state_path(col: &Collection) -> std::path::PathBuf {
    let mut p = col.path().to_path_buf();
    p.set_extension("sync.json");
    p
}

fn load_state(col: &Collection) -> SyncState {
    let path = sync_state_path(col);
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(st) = serde_json::from_str(&s) {
            return st;
        }
    }
    SyncState {
        cursor: 0,
        device_id: format!("dev-{}", &Uuid::new_v4().to_string()[..8]),
    }
}

fn save_state(col: &Collection, st: &SyncState) -> Result<()> {
    let path = sync_state_path(col);
    std::fs::write(path, serde_json::to_string_pretty(st)?)?;
    Ok(())
}

fn client(_token: &str) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?)
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>()
        .with_context(|| format!("bad timestamp {s}"))
}

pub struct SyncClient {
    pub server: String,
    pub token: String,
}

impl SyncClient {
    pub fn push(&self, col: &mut Collection) -> Result<()> {
        let mut state = load_state(col);
        if state.device_id.is_empty() {
            state.device_id = format!("dev-{}", &Uuid::new_v4().to_string()[..8]);
        }
        let base = self.server.trim_end_matches('/');
        let url = format!("{base}/v1/push");
        let http = client(&self.token)?;

        let mut changes: Vec<Change> = Vec::new();

        for note in col.all_notes()? {
            let deck_name = col
                .list_decks()?
                .into_iter()
                .find(|d| d.id == note.deck_id)
                .map(|d| d.name)
                .unwrap_or_else(|| "Default".into());
            changes.push(Change {
                seq: None,
                kind: "note".into(),
                id: note.id.to_string(),
                updated_at: note.updated_at.to_rfc3339(),
                device_id: state.device_id.clone(),
                payload: serde_json::json!({
                    "deckName": deck_name,
                    "fields": note.fields,
                    "tags": note.tags,
                }),
            });
        }

        for card in col.all_cards()? {
            let deck_name = col
                .list_decks()?
                .into_iter()
                .find(|d| d.id == card.deck_id)
                .map(|d| d.name)
                .unwrap_or_else(|| "Default".into());
            changes.push(Change {
                seq: None,
                kind: "card".into(),
                id: card.id.to_string(),
                updated_at: card.updated_at.to_rfc3339(),
                device_id: state.device_id.clone(),
                payload: serde_json::json!({
                    "noteId": card.note_id.to_string(),
                    "deckName": deck_name,
                    "templateIdx": card.template_idx,
                    "dueAt": card.state.due_at.to_rfc3339(),
                    "stability": card.state.stability,
                    "difficulty": card.state.difficulty,
                    "reps": card.state.reps,
                    "lapses": card.state.lapses,
                    "lastReviewAt": card.state.last_review_at.map(|t| t.to_rfc3339()),
                }),
            });
        }

        for rev in col.all_revlog()? {
            changes.push(Change {
                seq: None,
                kind: "revlog".into(),
                id: rev.id.to_string(),
                updated_at: rev.reviewed_at.to_rfc3339(),
                device_id: state.device_id.clone(),
                payload: serde_json::json!({
                    "cardId": rev.card_id.to_string(),
                    "rating": rev.rating.as_u8(),
                    "reviewedAt": rev.reviewed_at.to_rfc3339(),
                    "elapsedMs": rev.elapsed_ms,
                    "intervalDays": rev.interval_days,
                }),
            });
        }

        // Batch to keep request sizes reasonable
        let chunk = 500;
        let mut accepted = 0u32;
        let mut latest = 0i64;
        for batch in changes.chunks(chunk) {
            let body = serde_json::json!({
                "deviceId": state.device_id,
                "changes": batch,
            });
            let resp = http
                .post(&url)
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .context("push request")?;
            if !resp.status().is_success() {
                bail!("push failed: HTTP {}", resp.status());
            }
            let parsed: PushResponse = resp.json()?;
            accepted += parsed.accepted;
            latest = parsed.latest_seq;
        }
        save_state(col, &state)?;
        println!(
            "pushed {accepted} changes from device={} (server seq={latest})",
            state.device_id
        );
        Ok(())
    }

    pub fn pull(&self, col: &mut Collection) -> Result<()> {
        let mut state = load_state(col);
        if state.device_id.is_empty() {
            state.device_id = format!("dev-{}", &Uuid::new_v4().to_string()[..8]);
        }
        let base = self.server.trim_end_matches('/');
        let http = client(&self.token)?;

        let mut applied = 0u32;
        let mut skipped = 0u32;
        let mut cursor = state.cursor;

        loop {
            let url = format!("{base}/v1/pull?since={cursor}&limit=500");
            let resp = http
                .get(&url)
                .bearer_auth(&self.token)
                .send()
                .context("pull request")?;
            if !resp.status().is_success() {
                bail!("pull failed: HTTP {}", resp.status());
            }
            let page: PullResponse = resp.json()?;
            if page.changes.is_empty() {
                break;
            }
            for ch in page.changes {
                match apply_change(col, &ch, &state.device_id) {
                    Ok(true) => applied += 1,
                    Ok(false) => skipped += 1,
                    Err(e) => {
                        tracing::warn!("skip change {}: {e}", ch.id);
                        skipped += 1;
                    }
                }
            }
            cursor = page.cursor;
            if !page.has_more {
                break;
            }
        }

        state.cursor = cursor;
        save_state(col, &state)?;
        println!("pull applied={applied} skipped={skipped} cursor={cursor}");
        Ok(())
    }
}

/// Returns true if applied, false if ignored (older/duplicate).
fn apply_change(col: &mut Collection, ch: &Change, local_device: &str) -> Result<bool> {
    let remote_ts = parse_ts(&ch.updated_at)?;
    let id = Id::from(Uuid::parse_str(&ch.id)?);

    match ch.kind.as_str() {
        "note" => {
            let deck_name = ch
                .payload
                .get("deckName")
                .and_then(|v| v.as_str())
                .unwrap_or("Default");
            let fields: Vec<String> = ch
                .payload
                .get("fields")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let tags: Vec<String> = ch
                .payload
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            if let Some(local) = col.get_note(id)? {
                if !should_apply(remote_ts, &ch.device_id, local.updated_at, local_device) {
                    return Ok(false);
                }
            }
            let deck = col.deck_by_name_or_create(deck_name)?;
            // Keep local deck_id if note exists to avoid orphans; else use resolved deck.
            let deck_id = col
                .get_note(id)?
                .map(|n| n.deck_id)
                .unwrap_or(deck.id);
            let note = Note {
                id,
                deck_id,
                notetype: "basic".into(),
                fields,
                tags,
                created_at: remote_ts,
                updated_at: remote_ts,
            };
            col.upsert_note_sync(note)?;
            Ok(true)
        }
        "card" => {
            let due_at = parse_ts(
                ch.payload
                    .get("dueAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&ch.updated_at),
            )?;
            let last = ch
                .payload
                .get("lastReviewAt")
                .and_then(|v| v.as_str())
                .map(parse_ts)
                .transpose()?;
            let note_id = Id::from(Uuid::parse_str(
                ch.payload
                    .get("noteId")
                    .and_then(|v| v.as_str())
                    .context("card.noteId")?,
            )?);
            let deck_name = ch
                .payload
                .get("deckName")
                .and_then(|v| v.as_str())
                .unwrap_or("Default");
            let deck = col.deck_by_name_or_create(deck_name)?;

            if let Some(local) = col.get_card(id)? {
                if !should_apply(remote_ts, &ch.device_id, local.updated_at, local_device) {
                    return Ok(false);
                }
            }
            let card = Card {
                id,
                note_id,
                deck_id: deck.id,
                template_idx: ch
                    .payload
                    .get("templateIdx")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                state: engram_core::CardState {
                    due_at,
                    stability: ch
                        .payload
                        .get("stability")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32,
                    difficulty: ch
                        .payload
                        .get("difficulty")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32,
                    reps: ch
                        .payload
                        .get("reps")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    lapses: ch
                        .payload
                        .get("lapses")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    last_review_at: last,
                },
                created_at: remote_ts,
                updated_at: remote_ts,
            };
            col.upsert_card_sync(card)?;
            Ok(true)
        }
        "revlog" => {
            if col.has_revlog(id)? {
                return Ok(false);
            }
            let card_id = Id::from(Uuid::parse_str(
                ch.payload
                    .get("cardId")
                    .and_then(|v| v.as_str())
                    .context("revlog.cardId")?,
            )?);
            let rating = ch
                .payload
                .get("rating")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u8;
            let Some(rating) = Rating::from_u8(rating) else {
                bail!("bad rating");
            };
            let entry = RevlogEntry {
                id,
                card_id,
                rating,
                reviewed_at: parse_ts(
                    ch.payload
                        .get("reviewedAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&ch.updated_at),
                )?,
                elapsed_ms: ch
                    .payload
                    .get("elapsedMs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                stability_after: 0.0,
                difficulty_after: 0.0,
                interval_days: ch
                    .payload
                    .get("intervalDays")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
            };
            col.add_revlog(entry)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn should_apply(
    remote_ts: DateTime<Utc>,
    remote_device: &str,
    local_ts: DateTime<Utc>,
    local_device: &str,
) -> bool {
    if remote_ts > local_ts {
        return true;
    }
    if remote_ts < local_ts {
        return false;
    }
    // Tie-break: higher device id wins (deterministic).
    remote_device > local_device && remote_device != local_device
}
