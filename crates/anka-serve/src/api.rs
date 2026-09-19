use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use anyhow::Context as _;
use anka_core::{
    extract_sounds, front_back_for_template, strip_html, CardTemplate, Collection, Id, Rating,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct AppState {
    pub collection: Mutex<Collection>,
    #[allow(dead_code)]
    pub path: PathBuf,
}

pub type Shared = Arc<AppState>;

fn lock_col(state: &Shared) -> Result<std::sync::MutexGuard<'_, Collection>, String> {
    state
        .collection
        .lock()
        .map_err(|_| "collection lock poisoned".to_string())
}

fn lock_col_mut(
    state: &Shared,
) -> Result<std::sync::MutexGuard<'_, Collection>, String> {
    lock_col(state)
}


#[derive(Deserialize)]
struct AnkiwebImportReq {
    /// Preferred: session hkey from /api/ankiweb/login
    hkey: Option<String>,
    user: Option<String>,
    password: Option<String>,
}

/// One-time full import from AnkiWeb into this collection.
/// The password is used in-memory for a single login call and never stored.
async fn ankiweb_import(
    State(state): State<Shared>,
    Json(req): Json<AnkiwebImportReq>,
) -> Result<Json<serde_json::Value>, String> {
    let state = state.clone();
    let report = tokio::task::spawn_blocking(move || {
        let data = match &req.hkey {
            Some(hkey) => anka_ankiweb::full_download_with_hkey(hkey)?,
            None => anka_ankiweb::pull(
                req.user.as_deref().context("缺少 AnkiWeb 邮箱")?,
                req.password.as_deref().context("缺少 AnkiWeb 密码")?,
            )?,
        };
        let mut col = state
            .collection
            .lock()
            .map_err(|_| anyhow::anyhow!("collection lock poisoned"))?;
        anka_ankiweb::import_into_collection(&data, &mut col)
    })
    .await
    .map_err(|e| format!("task join: {e}"))?
    .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::json!({
        "decks": report.decks,
        "notes": report.notes,
        "cards": report.cards,
        "revlogs": report.revlogs,
        "mediaCopied": report.media_copied,
    })))
}

#[derive(Deserialize)]
struct AnkiwebSyncReq {
    /// Preferred: session hkey from /api/ankiweb/login
    hkey: Option<String>,
    user: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct AnkiwebLoginReq {
    user: String,
    password: String,
}

/// Login to AnkiWeb once; returns the long-lived session hkey.
/// The password is used in-memory for this single call and never stored.
async fn ankiweb_login(
    Json(req): Json<AnkiwebLoginReq>,
) -> Result<Json<serde_json::Value>, String> {
    // blocking reqwest must not run on the async runtime
    let hkey = tokio::task::spawn_blocking(move || {
        anka_ankiweb::login(&req.user, &req.password).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Json(serde_json::json!({ "hkey": hkey })))
}

/// Two-way sync: run the anka-sync-agent (official engine) against AnkiWeb,
/// then merge the agent collection into the serving collection.
/// The password is used in-memory for a single login call and never stored.
async fn ankiweb_sync(
    State(state): State<Shared>,
    Json(req): Json<AnkiwebSyncReq>,
) -> Result<Json<serde_json::Value>, String> {
    let agent_bin = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("anka-sync-agent"))
        .ok_or_else(|| "no exe dir".to_string())?;
    let agent_path = state
        .path
        .parent()
        .map(|p| p.join("sync-agent.anki2"))
        .ok_or_else(|| "collection has no parent".to_string())?;
    let media_dir = anka_core::media_dir_for_collection(&state.path);

    let result: Result<serde_json::Value, String> =
        tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let run_agent = |args: &[&str]| -> Result<(), String> {
            let status = std::process::Command::new(&agent_bin)
                .args(args)
                .status()
                .map_err(|e| format!("anka-sync-agent 未找到: {e}"))?;
            if !status.success() {
                return Err("同步代理执行失败（详见服务端日志/上方提示）".to_string());
            }
            Ok(())
        };

        // 1. bootstrap: first run pulls AnkiWeb into the agent collection
        if !agent_path.exists() {
            let mut pull_args = vec![
                "pull".to_string(),
                "--agent".to_string(),
                agent_path.to_string_lossy().as_ref().to_string(),
            ];
            if let Some(hkey) = &req.hkey {
                pull_args.push("--hkey".into());
                pull_args.push(hkey.clone());
            } else {
                pull_args.push("--user".into());
                pull_args.push(req.user.clone().unwrap_or_default());
                pull_args.push("--password".into());
                pull_args.push(req.password.clone().unwrap_or_default());
            }
            run_agent(&pull_args.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;
        }

        // 2. merge AnkiWeb copy -> Anka (dedup by anki_id_map)
        {
            let mut col = state
                .collection
                .lock()
                .map_err(|_| "collection lock poisoned".to_string())?;
            anka_ankiweb::merge_agent_into_collection(&agent_path, &mut col)
                .map_err(|e| e.to_string())?;
        }

        // 3. push unmapped Anka notes into the agent collection
        let mut batch: Vec<serde_json::Value> = Vec::new();
        let mut pairs: Vec<(anka_core::Id, anka_core::Id)> = Vec::new();
        {
            let col = state.collection.lock().map_err(|_| "lock")?;
            let mapped: std::collections::HashSet<String> = col
                .mapped_note_ids()
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect();
            let all_notes = col.all_notes().map_err(|e| e.to_string())?;
            let all_cards = col.all_cards().map_err(|e| e.to_string())?;
            let decks_list = col.list_decks().map_err(|e| e.to_string())?;
            for note in &all_notes {
                if mapped.contains(&note.id.to_string()) {
                    continue;
                }
                let deck_name = decks_list
                    .iter()
                    .find(|d| d.id == note.deck_id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "Default".into());
                let first_card = all_cards
                    .iter()
                    .find(|c| c.note_id == note.id)
                    .map(|c| c.id);
                let (front, back) = anka_core::front_back(&note.fields);
                batch.push(serde_json::json!({
                    "deck": deck_name,
                    "fields": [front, back],
                    "tags": note.tags,
                }));
                if let Some(card_id) = first_card {
                    pairs.push((note.id, card_id));
                }
            }
        }
        let batch_file = agent_path.with_extension("batch.json");
        let map_file = agent_path.with_extension("map.json");
        if !batch.is_empty() {
            std::fs::write(
                &batch_file,
                serde_json::to_vec(&batch).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            run_agent(&[
                "add-batch",
                "--agent",
                agent_path.to_string_lossy().as_ref(),
                "--file",
                batch_file.to_string_lossy().as_ref(),
                "--map-out",
                map_file.to_string_lossy().as_ref(),
            ])?;
        }

        // 4. two-way sync with AnkiWeb (uploads pushed notes, pulls changes)
        let mut sync_args = vec![
            "sync".to_string(),
            "--agent".to_string(),
            agent_path.to_string_lossy().as_ref().to_string(),
        ];
        if let Some(hkey) = &req.hkey {
            sync_args.push("--hkey".into());
            sync_args.push(hkey.clone());
        } else {
            sync_args.push("--user".into());
            sync_args.push(req.user.clone().unwrap_or_default());
            sync_args.push("--password".into());
            sync_args.push(req.password.clone().unwrap_or_default());
        }
        run_agent(&sync_args.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;

        // 5. record mappings for pushed notes
        if !batch.is_empty() {
            let map_raw = std::fs::read_to_string(&map_file)
                .map_err(|e| format!("读取 map-out 失败: {e}"))?;
            let entries: Vec<serde_json::Value> =
                serde_json::from_str(&map_raw).map_err(|e| e.to_string())?;
            let mut col = state.collection.lock().map_err(|_| "lock")?;
            for entry in &entries {
                let idx = entry["index"].as_u64().unwrap_or(0) as usize;
                if let Some((note_id, card_id)) = pairs.get(idx) {
                    if let Some(anki_note) = entry["noteId"].as_i64() {
                        col.put_anki_id("note", &anki_note.to_string(), *note_id)
                            .map_err(|e| e.to_string())?;
                    }
                    if let Some(anki_card) =
                        entry["cardIds"].as_array().and_then(|a| a.first())
                    {
                        if let Some(c) = anki_card.as_i64() {
                            col.put_anki_id("card", &c.to_string(), *card_id)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&batch_file);

        // 6. scheduling state merge + final content merge + media copy
        let (sched_updated, sched_skipped, notes_created, notes_updated, cards) = {
            let mut col = state
                .collection
                .lock()
                .map_err(|_| "collection lock poisoned".to_string())?;
            let (su, sk) = anka_ankiweb::merge_scheduling_from_agent(&agent_path, &mut col)
                .map_err(|e| e.to_string())?;
            let report = anka_ankiweb::merge_agent_into_collection(&agent_path, &mut col)
                .map_err(|e| e.to_string())?;
            (su, sk, report.notes, report.notes_updated, report.cards)
        };

        // media: copy any new files from the agent media dir
        let mut media_copied = 0u32;
        if let Some(agent_media) = agent_path.parent().map(|p| p.join("sync-agent.media")) {
            if agent_media.is_dir() {
                std::fs::create_dir_all(&media_dir).map_err(|e| e.to_string())?;
                for entry in std::fs::read_dir(&agent_media)
                    .map_err(|e| e.to_string())?
                    .flatten()
                {
                    let target = media_dir.join(entry.file_name());
                    if !target.exists() {
                        std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
                        media_copied += 1;
                    }
                }
            }
        }

        Ok(serde_json::json!({
            "notesCreated": notes_created,
            "notesUpdated": notes_updated,
            "cards": cards,
            "mediaCopied": media_copied,
            "schedUpdated": sched_updated,
            "schedSkipped": sched_skipped,
        }))
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(value) => Ok(Json(value)),
        Err(e) => {
            // surface agent stderr details for AnkiWeb-side errors
            Err(e)
        }
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/decks", get(list_decks))
        .route("/api/due", get(due_cards))
        .route("/api/grade", post(grade_card))
        .route("/api/notes", get(search_notes).post(create_note))
        .route("/api/notes/{id}", get(get_note).put(update_note))
        .route("/api/ankiweb/import", post(ankiweb_import))
        .route("/api/ankiweb/sync", post(ankiweb_sync))
        .route("/api/ankiweb/login", post(ankiweb_login))
        .with_state(state)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    ok: bool,
    app: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        app: "anka",
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeckCountsDto {
    deck_id: String,
    name: String,
    new_count: u64,
    learning_count: u64,
    review_count: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DueCardDto {
    card_id: String,
    deck_id: String,
    deck_name: String,
    front: String,
    phonetic: String,
    back: String,
    example: String,
    sounds: Vec<String>,
    template: String,
    stability: f32,
    lapses: u32,
    kind: String,
}

async fn list_decks(State(state): State<Shared>) -> Result<Json<Vec<DeckCountsDto>>, String> {
    let col = lock_col(&state)?;
    let counts = col.deck_counts().map_err(|e| e.to_string())?;
    Ok(Json(
        counts
            .into_iter()
            .map(|d| DeckCountsDto {
                deck_id: d.deck_id.to_string(),
                name: d.name,
                new_count: d.new_count,
                learning_count: d.learning_count,
                review_count: d.review_count,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct DueQuery {
    deck: Option<String>,
    limit: Option<u32>,
}

async fn due_cards(
    State(state): State<Shared>,
    Query(q): Query<DueQuery>,
) -> Result<Json<Vec<DueCardDto>>, String> {
    let col = lock_col(&state)?;
    let deck_id = match q.deck.as_deref() {
        Some(name) if !name.is_empty() => {
            let decks = col.list_decks().map_err(|e| e.to_string())?;
            let found = decks.into_iter().find(|d| d.name == name);
            match found {
                Some(d) => Some(d.id),
                None => return Err(format!("牌组不存在: {name}")),
            }
        }
        _ => None,
    };
    let due = col
        .due(deck_id, q.limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;
    let name_by_id: std::collections::HashMap<String, String> = col
        .list_decks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|d| (d.id.to_string(), d.name))
        .collect();

    Ok(Json(
        due.into_iter()
            .map(|item| {
                let deck_name = name_by_id
                    .get(&item.card.deck_id.to_string())
                    .cloned()
                    .unwrap_or_default();
                let template =
                    CardTemplate::from_idx_and_deck(item.card.template_idx, &deck_name);
                let (front, back, phonetic) =
                    front_back_for_template(&item.note.fields, template);
                let example = item
                    .note
                    .fields
                    .iter()
                    .map(|s| s.trim())
                    .find(|s| {
                        s.chars().count() > 20
                            && s.contains(' ')
                            && !s.starts_with('<')
                            && !s.starts_with('[')
                            && (s.contains('.') || s.contains('。') || s.contains('!'))
                    })
                    .map(|s| {
                        let t = strip_html(s);
                        let t = t.trim();
                        if t.chars().count() > 180 {
                            t.chars().take(180).collect::<String>() + "…"
                        } else {
                            t.to_string()
                        }
                    })
                    .unwrap_or_default();
                let sounds = extract_sounds(&item.note.fields);
                DueCardDto {
                    card_id: item.card.id.to_string(),
                    deck_id: item.card.deck_id.to_string(),
                    deck_name,
                    front,
                    phonetic,
                    back,
                    example,
                    sounds,
                    template: template.as_str().to_string(),
                    stability: item.card.state.stability,
                    lapses: item.card.state.lapses,
                    kind: format!("{:?}", item.card.state.kind()).to_lowercase(),
                }
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GradeBody {
    card_id: String,
    rating: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GradeResult {
    next_due: String,
    stability: f32,
    difficulty: f32,
    reps: u32,
    lapses: u32,
}

async fn grade_card(
    State(state): State<Shared>,
    Json(body): Json<GradeBody>,
) -> Result<Json<GradeResult>, String> {
    let mut col = lock_col_mut(&state)?;
    let rating = Rating::from_u8(body.rating).ok_or_else(|| "评分必须是 1-4".to_string())?;
    let uuid = Uuid::parse_str(&body.card_id).map_err(|e| e.to_string())?;
    let id = Id::from(uuid);
    let card = col.answer_card(id, rating, 0).map_err(|e| e.to_string())?;
    Ok(Json(GradeResult {
        next_due: card.state.due_at.to_rfc3339(),
        stability: card.state.stability,
        difficulty: card.state.difficulty,
        reps: card.state.reps,
        lapses: card.state.lapses,
    }))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NoteDto {
    id: String,
    deck_id: String,
    deck_name: String,
    front: String,
    back: String,
    fields: Vec<String>,
    tags: Vec<String>,
}

fn note_to_dto(col: &Collection, note: anka_core::Note) -> Result<NoteDto, String> {
    let deck_name = col
        .list_decks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.id == note.deck_id)
        .map(|d| d.name)
        .unwrap_or_default();
    let (front, back) = anka_core::front_back(&note.fields);
    Ok(NoteDto {
        id: note.id.to_string(),
        deck_id: note.deck_id.to_string(),
        deck_name,
        front,
        back,
        fields: note.fields,
        tags: note.tags,
    })
}

#[derive(Deserialize)]
struct NotesQuery {
    q: Option<String>,
    limit: Option<u32>,
}

async fn search_notes(
    State(state): State<Shared>,
    Query(q): Query<NotesQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let col = lock_col(&state)?;
    let (total, notes) = col
        .search_notes(q.q.as_deref().unwrap_or(""), q.limit.unwrap_or(30), 0)
        .map_err(|e| e.to_string())?;
    let mut items = Vec::with_capacity(notes.len());
    for n in notes {
        items.push(note_to_dto(&col, n)?);
    }
    Ok(Json(serde_json::json!({ "total": total, "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNoteBody {
    deck: String,
    front: String,
    back: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_note(
    State(state): State<Shared>,
    Json(body): Json<CreateNoteBody>,
) -> Result<Json<NoteDto>, String> {
    let mut col = lock_col_mut(&state)?;
    let deck = col.ensure_deck(&body.deck).map_err(|e| e.to_string())?;
    let (note, _card) = col
        .add_note(
            deck.id,
            vec![body.front, body.back],
            body.tags,
        )
        .map_err(|e| e.to_string())?;
    note_to_dto(&col, note)
        .map(Json)
}

async fn get_note(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<NoteDto>, String> {
    let col = lock_col(&state)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let note = col
        .get_note(Id::from(uuid))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "笔记不存在".to_string())?;
    Ok(Json(note_to_dto(&col, note)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNoteBody {
    fields: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

async fn update_note(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<UpdateNoteBody>,
) -> Result<Json<NoteDto>, String> {
    if body.fields.iter().all(|f| f.trim().is_empty()) {
        return Err("字段不能全为空".into());
    }
    let mut col = lock_col_mut(&state)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let note = col
        .update_note_fields(Id::from(uuid), body.fields, body.tags)
        .map_err(|e| e.to_string())?;
    note_to_dto(&col, note).map(Json)
}
