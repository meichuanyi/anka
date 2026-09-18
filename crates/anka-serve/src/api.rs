use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
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
    user: String,
    password: String,
}

/// One-time full import from AnkiWeb into this collection.
/// The password is used in-memory for a single login call and never stored.
async fn ankiweb_import(
    State(state): State<Shared>,
    Json(req): Json<AnkiwebImportReq>,
) -> Result<Json<serde_json::Value>, String> {
    let state = state.clone();
    let report = tokio::task::spawn_blocking(move || {
        let hkey = anka_ankiweb::login(&req.user, &req.password)?;
        let data = anka_ankiweb::full_download(&hkey)?;
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

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/decks", get(list_decks))
        .route("/api/due", get(due_cards))
        .route("/api/grade", post(grade_card))
        .route("/api/notes", get(search_notes).post(create_note))
        .route("/api/notes/{id}", get(get_note).put(update_note))
        .route("/api/ankiweb/import", post(ankiweb_import))
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
