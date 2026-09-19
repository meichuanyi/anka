use std::path::PathBuf;
use std::sync::Mutex;

use anka_core::{
    extract_sounds, front_back_for_template, media_dir_for_collection, strip_html, CardTemplate,
    Collection, Rating,
};
use serde::Serialize;
use tauri::{Manager, State};

pub struct AppState {
    pub collection: Mutex<Collection>,
    pub path: PathBuf,
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
    /// Absolute filesystem paths (frontend uses convertFileSrc / file URL).
    sounds: Vec<String>,
    template: String,
    stability: f32,
    lapses: u32,
    kind: String,
}

#[tauri::command]
fn list_decks(state: State<'_, AppState>) -> Result<Vec<DeckCountsDto>, String> {
    let col = state.collection.lock().map_err(|e| e.to_string())?;
    let counts = col.deck_counts().map_err(|e| e.to_string())?;
    Ok(counts
        .into_iter()
        .map(|d| DeckCountsDto {
            deck_id: d.deck_id.to_string(),
            name: d.name,
            new_count: d.new_count,
            learning_count: d.learning_count,
            review_count: d.review_count,
        })
        .collect())
}

#[tauri::command]
fn due_cards(
    state: State<'_, AppState>,
    deck: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<DueCardDto>, String> {
    let col = state.collection.lock().map_err(|e| e.to_string())?;
    let deck_id = match deck.as_deref() {
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
        .due(deck_id, limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;
    let name_by_id: std::collections::HashMap<String, String> = col
        .list_decks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|d| (d.id.to_string(), d.name))
        .collect();

    let media_dir = media_dir_for_collection(&state.path);
    Ok(due
        .into_iter()
        .map(|item| {
            let deck_name = name_by_id
                .get(&item.card.deck_id.to_string())
                .cloned()
                .unwrap_or_default();
            let template = CardTemplate::from_idx_and_deck(item.card.template_idx, &deck_name);
            let (front, back, phonetic) = front_back_for_template(&item.note.fields, template);
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
            let sounds: Vec<String> = extract_sounds(&item.note.fields)
                .into_iter()
                .map(|name| {
                    let safe = name.replace(['/', '\\'], "_");
                    media_dir.join(safe).to_string_lossy().into_owned()
                })
                .collect();
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
        .collect())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GradeResult {
    next_due: String,
    stability: f32,
    difficulty: f32,
    reps: u32,
    lapses: u32,
}

#[tauri::command]
fn grade_card(
    state: State<'_, AppState>,
    card_id: String,
    rating: u8,
) -> Result<GradeResult, String> {
    let mut col = state.collection.lock().map_err(|e| e.to_string())?;
    let rating = Rating::from_u8(rating).ok_or_else(|| "评分必须是 1-4".to_string())?;
    let uuid = uuid::Uuid::parse_str(&card_id).map_err(|e| e.to_string())?;
    let id = anka_core::Id::from(uuid);
    let card = col.answer_card(id, rating, 0).map_err(|e| e.to_string())?;
    Ok(GradeResult {
        next_due: card.state.due_at.to_rfc3339(),
        stability: card.state.stability,
        difficulty: card.state.difficulty,
        reps: card.state.reps,
        lapses: card.state.lapses,
    })
}

#[tauri::command]
fn create_note(
    state: State<'_, AppState>,
    deck: String,
    front: String,
    back: String,
    tags: Option<Vec<String>>,
) -> Result<NoteDto, String> {
    let mut col = state.collection.lock().map_err(|e| e.to_string())?;
    let d = col.ensure_deck(&deck).map_err(|e| e.to_string())?;
    let (note, _card) = col
        .add_note(d.id, vec![front, back], tags.unwrap_or_default())
        .map_err(|e| e.to_string())?;
    note_dto(&col, note)
}

#[tauri::command]
fn update_note(
    state: State<'_, AppState>,
    id: String,
    fields: Vec<String>,
    tags: Option<Vec<String>>,
) -> Result<NoteDto, String> {
    let mut col = state.collection.lock().map_err(|e| e.to_string())?;
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let note = col
        .update_note_fields(
            anka_core::Id::from(uuid),
            fields,
            tags.unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
    note_dto(&col, note)
}

#[tauri::command]
fn get_note(state: State<'_, AppState>, id: String) -> Result<NoteDto, String> {
    let col = state.collection.lock().map_err(|e| e.to_string())?;
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let note = col
        .get_note(anka_core::Id::from(uuid))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "笔记不存在".to_string())?;
    note_dto(&col, note)
}

#[tauri::command]
fn search_notes(
    state: State<'_, AppState>,
    q: Option<String>,
    limit: Option<u32>,
) -> Result<SearchNotesResult, String> {
    let col = state.collection.lock().map_err(|e| e.to_string())?;
    let (total, notes) = col
        .search_notes(q.as_deref().unwrap_or(""), limit.unwrap_or(30), 0)
        .map_err(|e| e.to_string())?;
    let mut items = Vec::with_capacity(notes.len());
    for n in notes {
        items.push(note_dto(&col, n)?);
    }
    Ok(SearchNotesResult { total, items })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoteDto {
    id: String,
    deck_id: String,
    deck_name: String,
    front: String,
    back: String,
    fields: Vec<String>,
    tags: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchNotesResult {
    total: u64,
    items: Vec<NoteDto>,
}

fn note_dto(col: &Collection, note: anka_core::Note) -> Result<NoteDto, String> {
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

#[tauri::command]
fn ankiweb_sync(state: State<'_, AppState>, hkey: String) -> Result<serde_json::Value, String> {
    use std::path::PathBuf;
    let agent_bin = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("anka-sync-agent"))
        .ok_or("no exe dir".to_string())?;
    let agent_path = state
        .path
        .parent()
        .map(|p| p.join("sync-agent.anki2"))
        .ok_or("collection has no parent".to_string())?;
    let run_agent = |args: &[&str]| -> Result<(), String> {
        let status = std::process::Command::new(&agent_bin)
            .args(args)
            .status()
            .map_err(|e| format!("需要 anka-sync-agent（随 App 分发）: {e}"))?;
        if !status.success() {
            return Err("同步代理执行失败".into());
        }
        Ok(())
    };
    let to_str = |p: &PathBuf| p.to_string_lossy().as_ref().to_string();

    // 1. bootstrap
    if !agent_path.exists() {
        run_agent(&[
            "pull",
            "--agent",
            &to_str(&agent_path),
            "--hkey",
            &hkey,
        ])?;
    }
    // 2. merge down
    {
        let mut col = state
            .collection
            .lock()
            .map_err(|_| "collection lock poisoned".to_string())?;
        anka_ankiweb::merge_agent_into_collection(&agent_path, &mut col)
            .map_err(|e| e.to_string())?;
    }
    // 3. push unmapped notes
    let (batch, pairs) = {
        let mut col = state
            .collection
            .lock()
            .map_err(|_| "collection lock poisoned".to_string())?;
        let mapped: std::collections::HashSet<String> =
            col.mapped_note_ids().map_err(|e| e.to_string())?.into_iter().collect();
        let all_notes = col.all_notes().map_err(|e| e.to_string())?;
        let all_cards = col.all_cards().map_err(|e| e.to_string())?;
        let decks_list = col.list_decks().map_err(|e| e.to_string())?;
        let mut batch: Vec<serde_json::Value> = Vec::new();
        let mut pairs: Vec<(anka_core::Id, anka_core::Id)> = Vec::new();
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
                "deck": deck_name, "fields": [front, back], "tags": note.tags,
            }));
            if let Some(card_id) = first_card {
                pairs.push((note.id, card_id));
            }
        }
        (batch, pairs)
    };
    let batch_file = agent_path.with_extension("batch.json");
    let map_file = agent_path.with_extension("map.json");
    if !batch.is_empty() {
        std::fs::write(&batch_file, serde_json::to_vec(&batch).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        run_agent(&[
            "add-batch",
            "--agent",
            &to_str(&agent_path),
            "--file",
            &to_str(&batch_file),
            "--map-out",
            &to_str(&map_file),
        ])?;
    }
    // 4. two-way sync
    run_agent(&["sync", "--agent", &to_str(&agent_path), "--hkey", &hkey])?;
    // 5. record mappings
    if !batch.is_empty() {
        let mut col = state
            .collection
            .lock()
            .map_err(|_| "collection lock poisoned".to_string())?;
        if let Ok(map_raw) = std::fs::read_to_string(&map_file) {
            if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&map_raw) {
                for entry in &entries {
                    let idx = entry["index"].as_u64().unwrap_or(0) as usize;
                    if let Some((note_id, card_id)) = pairs.get(idx) {
                        if let Some(anki_note) = entry["noteId"].as_i64() {
                            col.put_anki_id("note", &anki_note.to_string(), *note_id)
                                .map_err(|e| e.to_string())?;
                        }
                        if let Some(anki_card) = entry["cardIds"].as_array().and_then(|a| a.first()) {
                            if let Some(c) = anki_card.as_i64() {
                                col.put_anki_id("card", &c.to_string(), *card_id)
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(&map_file);
    let _ = std::fs::remove_file(&batch_file);
    // 6. scheduling + final content merge
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
    Ok(serde_json::json!({
        "schedUpdated": sched_updated, "schedSkipped": sched_skipped,
        "notesCreated": notes_created, "notesUpdated": notes_updated, "cards": cards,
    }))
}

#[tauri::command]
fn ankiweb_login(user: String, password: String) -> Result<serde_json::Value, String> {
    let hkey = anka_ankiweb::login(&user, &password).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "hkey": hkey }))
}

#[tauri::command]
fn ankiweb_import(
    state: State<'_, AppState>,
    user: String,
    password: String,
) -> Result<serde_json::Value, String> {
    let mut client =
        anka_ankiweb::AnkiWebClient::new().map_err(|e| e.to_string())?;
    client.login(&user, &password).map_err(|e| e.to_string())?;
    let data = client.full_download().map_err(|e| e.to_string())?;
    let mut col = state
        .collection
        .lock()
        .map_err(|_| "collection lock poisoned".to_string())?;
    let report = anka_ankiweb::import_into_collection(&data, &mut col)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "decks": report.decks,
        "notes": report.notes,
        "cards": report.cards,
        "revlogs": report.revlogs,
        "mediaCopied": report.media_copied,
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Desktop: ANKA_COLLECTION env or CWD (as before).
            // Mobile: no env/CWD — the collection lives in the app sandbox.
            let path = std::env::var("ANKA_COLLECTION")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    #[cfg(mobile)]
                    {
                        app.path()
                            .app_data_dir()
                            .expect("app data dir unavailable")
                            .join("collection.akdb")
                    }
                    #[cfg(desktop)]
                    {
                        PathBuf::from("collection.akdb")
                    }
                });

            let collection = Collection::open_or_create(&path)
                .unwrap_or_else(|e| panic!("无法打开收藏 {}: {e}", path.display()));

            app.manage(AppState {
                collection: Mutex::new(collection),
                path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_decks,
            due_cards,
            grade_card,
            create_note,
            update_note,
            get_note,
            search_notes,
            ankiweb_import,
            ankiweb_login,
            ankiweb_sync
        ])
        .run(tauri::generate_context!())
        .expect("error while running Anka desktop");
}
