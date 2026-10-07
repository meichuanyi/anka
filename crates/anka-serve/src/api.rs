use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
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

/// Handler error type. axum 0.8 renders a bare `String` error as 200 OK +
/// text/plain, which defeats every client's `res.ok` check; `ApiError`
/// produces a real 500 so clients can branch on status and show the message.
pub struct ApiError(pub String);

impl From<String> for ApiError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ApiError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        use axum::response::IntoResponse as _;
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            self.0,
        )
            .into_response()
    }
}

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
) -> Result<Json<serde_json::Value>, ApiError> {
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
#[derive(serde::Serialize)]
struct DailyStat {
    date: String,
    reviews: u32,
    due: u32,
}

async fn stats_daily(
    State(state): State<Shared>,
    Query(params): Query<std::collections::HashMap<String, u32>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let days = params.get("days").copied().unwrap_or(120).clamp(7, 365);
    let mut col = state.collection.lock().map_err(|_| "lock".to_string())?;
    let reviews = col.review_daily(days).map_err(|e| e.to_string())?;
    let forecast = col.due_forecast(days).map_err(|e| e.to_string())?;
    let mut map: std::collections::BTreeMap<String, DailyStat> = std::collections::BTreeMap::new();
    for (day, count) in reviews {
        let e = map.entry(day.clone()).or_insert_with(|| DailyStat { date: day.clone(), reviews: 0, due: 0 });
        e.reviews += count;
    }
    for (day, count) in forecast {
        let e = map.entry(day.clone()).or_insert_with(|| DailyStat { date: day.clone(), reviews: 0, due: 0 });
        e.due += count;
    }
    Ok(Json(serde_json::json!({
        "days": days,
        "stats": map.into_values().collect::<Vec<_>>(),
    })))
}

async fn ankiweb_login(
    Json(req): Json<AnkiwebLoginReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // blocking reqwest must not run on the async runtime
    let hkey = tokio::task::spawn_blocking(move || {
        anka_ankiweb::login(&req.user, &req.password).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Json(serde_json::json!({ "hkey": hkey })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiChatBody {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
    messages: Vec<anka_ai::AiMessage>,
}

/// Proxy a chat completion to an OpenAI-compatible endpoint. The client's
/// AI config rides in the body and is never persisted; empty fields fall
/// back to server-side env (ANKA_AI_BASE_URL / ANKA_AI_KEY / ANKA_AI_MODEL).
async fn ai_chat(Json(req): Json<AiChatBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let cfg = anka_ai::AiConfig {
        base_url: req
            .base_url
            .filter(|v| !v.is_empty())
            .or_else(|| env("ANKA_AI_BASE_URL"))
            .unwrap_or_default(),
        api_key: req
            .api_key
            .filter(|v| !v.is_empty())
            .or_else(|| env("ANKA_AI_KEY"))
            .unwrap_or_default(),
        model: req
            .model
            .filter(|v| !v.is_empty())
            .or_else(|| env("ANKA_AI_MODEL"))
            .unwrap_or_default(),
    };
    let text = tokio::task::spawn_blocking(move || anka_ai::chat_blocking(&cfg, &req.messages))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::json!({ "text": text })))
}

/// Two-way sync: run the anka-sync-agent (official engine) against AnkiWeb,
/// then merge the agent collection into the serving collection.
/// The password is used in-memory for a single login call and never stored.
async fn ankiweb_sync(
    State(state): State<Shared>,
    Json(req): Json<AnkiwebSyncReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
        tokio::task::spawn_blocking(move || {
            let hkey = match &req.hkey {
                Some(k) => k.clone(),
                None => anka_ankiweb::login(
                    req.user.as_deref().ok_or("缺少 AnkiWeb 邮箱")?,
                    req.password.as_deref().ok_or("缺少 AnkiWeb 密码")?,
                )
                .map_err(|e| e.to_string())?,
            };
            let mut col = state
                .collection
                .lock()
                .map_err(|_| "collection lock poisoned".to_string())?;
            let report = anka_ankiweb::pipeline::run_pipeline(
                &agent_bin, &agent_path, &media_dir, &mut col, &hkey,
            )
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "pushed": report.pushed,
                "schedUpdated": report.sched_updated,
                "schedSkipped": report.sched_skipped,
                "notesCreated": report.notes_created,
                "notesUpdated": report.notes_updated,
                "cards": report.cards,
                "mediaCopied": report.media_copied,
            }))
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(Json(result?))
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/decks", get(list_decks))
        .route("/api/decks/{id}", delete(delete_deck))
        .route("/api/due", get(due_cards))
        .route("/api/grade", post(grade_card))
        .route("/api/notes", get(search_notes).post(create_note))
        .route("/api/notes/{id}", get(get_note).put(update_note))
        .route("/api/ankiweb/import", post(ankiweb_import))
        .route("/api/stats/daily", get(stats_daily))
        .route("/api/ai/chat", post(ai_chat))
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

async fn list_decks(State(state): State<Shared>) -> Result<Json<Vec<DeckCountsDto>>, ApiError> {
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

/// Delete a deck; notes, cards and review history go with it (irreversible).
async fn delete_deck(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut col = lock_col_mut(&state)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let report = col
        .delete_deck(Id::from(uuid))
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "id": id,
        "name": report.name,
        "notes": report.notes,
        "cards": report.cards,
        "revlogs": report.revlogs,
    })))
}

#[derive(Deserialize)]
struct DueQuery {
    deck: Option<String>,
    limit: Option<u32>,
}

async fn due_cards(
    State(state): State<Shared>,
    Query(q): Query<DueQuery>,
) -> Result<Json<Vec<DueCardDto>>, ApiError> {
    let col = lock_col(&state)?;
    let deck_id = match q.deck.as_deref() {
        Some(name) if !name.is_empty() => {
            let decks = col.list_decks().map_err(|e| e.to_string())?;
            let found = decks.into_iter().find(|d| d.name == name);
            match found {
                Some(d) => Some(d.id),
                None => return Err(format!("牌组不存在: {name}").into()),
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
) -> Result<Json<GradeResult>, ApiError> {
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
) -> Result<Json<serde_json::Value>, ApiError> {
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
) -> Result<Json<NoteDto>, ApiError> {
    let mut col = lock_col_mut(&state)?;
    let deck = col.ensure_deck(&body.deck).map_err(|e| e.to_string())?;
    let (note, _card) = col
        .add_note(
            deck.id,
            vec![body.front, body.back],
            body.tags,
        )
        .map_err(|e| e.to_string())?;
    note_to_dto(&col, note).map_err(ApiError)
        .map(Json)
}

async fn get_note(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<NoteDto>, ApiError> {
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
) -> Result<Json<NoteDto>, ApiError> {
    if body.fields.iter().all(|f| f.trim().is_empty()) {
        return Err("字段不能全为空".into());
    }
    let mut col = lock_col_mut(&state)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let note = col
        .update_note_fields(Id::from(uuid), body.fields, body.tags)
        .map_err(|e| e.to_string())?;
    note_to_dto(&col, note).map_err(ApiError).map(Json)
}

/* ---------- App 更新中转（学 HarnessGate）：手机直连 GitHub 慢/不稳，
   服务器用 gh 下载一次并缓存，手机走局域网秒下。
   两个端点公开（无鉴权）——APK 在 GitHub 上本就是公开产物。 ---------- */

use std::process::Command;
use std::sync::OnceLock;

fn gh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        // systemd 服务没有 HOME，gh 找不到 ~/.config/gh 认证 → 显式兜底
        .env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
        .output()
        .map_err(|e| format!("服务器没有 gh 命令: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "gh {} 失败: {}",
            args.join(" "),
            err.chars().take(200).collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `gh release view --json` output → (tag, apk asset name, apk size).
fn gh_release_apk(tag: &str) -> Result<(String, String, u64), String> {
    let mut args: Vec<&str> = vec!["release", "view"];
    if !tag.is_empty() {
        args.push(tag);
    }
    args.extend_from_slice(&["--json", "tagName,assets"]);
    let out = gh(&args)?;
    let j: serde_json::Value =
        serde_json::from_str(&out).map_err(|e| format!("解析 release JSON: {e}"))?;
    let tag = j["tagName"]
        .as_str()
        .ok_or("release 无 tagName")?
        .to_string();
    let assets = j["assets"].as_array().ok_or("release 无 assets")?;
    // 精简签名包优先（anka-mobile-*.apk），CI debug 包是临时签名装不上
    let apk = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.starts_with("anka-mobile-") && n.ends_with(".apk"))
                .unwrap_or(false)
        })
        .or_else(|| {
            assets
                .iter()
                .find(|a| a["name"].as_str().map(|n| n.ends_with(".apk")).unwrap_or(false))
        })
        .ok_or("release 没有 Android APK 产物")?;
    let name = apk["name"].as_str().unwrap_or_default().to_string();
    let size = apk["size"].as_u64().unwrap_or(0);
    Ok((tag, name, size))
}

fn apk_cache_dir(state: &Shared) -> PathBuf {
    state
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("apk-cache")
}

/// Download-once cache (single-flight: concurrent callers wait on one download).
fn ensure_apk_cached(state: &Shared, tag: &str) -> Result<PathBuf, String> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().map_err(|_| "lock")?;
    let (tag, name, size) = gh_release_apk(tag)?;
    let dir = apk_cache_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| format!("建缓存目录: {e}"))?;
    let file = dir.join(&name);
    if file.metadata().map(|m| m.len()).ok() == Some(size) {
        return Ok(file); // 已缓存
    }
    let tmp = dir.join(format!("{name}.tmp"));
    let mut args: Vec<&str> = vec!["release", "download"];
    if !tag.is_empty() {
        args.push(&tag);
    }
    args.extend_from_slice(&["-p", &name, "-O"]);
    args.push(tmp.to_str().unwrap_or("apk.tmp"));
    gh(&args)?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("缓存落盘: {e}"))?;
    Ok(file)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppLatestDto {
    tag: String,
    version: String,
    apk_name: String,
    apk_size: u64,
    apk_path: String,
}

async fn app_latest() -> Result<Json<AppLatestDto>, ApiError> {
    let (tag, name, size) = tokio::task::spawn_blocking(|| gh_release_apk(""))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(ApiError)?;
    Ok(Json(AppLatestDto {
        version: tag.trim_start_matches('v').to_string(),
        apk_path: format!("/api/app/apk?tag={tag}"),
        tag,
        apk_name: name,
        apk_size: size,
    }))
}

async fn app_apk(
    State(state): State<Shared>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, ApiError> {
    let tag = p.get("tag").cloned().unwrap_or_default();
    let state2 = state.clone();
    let file = tokio::task::spawn_blocking(move || ensure_apk_cached(&state2, &tag))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(ApiError)?;
    let bytes = std::fs::read(&file).map_err(|e| format!("读取缓存 APK: {e}"))?;
    let name = file
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    use axum::http::{header, HeaderValue, StatusCode};
    use axum::response::IntoResponse as _;
    let mut resp = (StatusCode::OK, axum::body::Body::from(bytes)).into_response();
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.android.package-archive"),
    );
    if let Ok(cd) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, cd);
    }
    Ok(resp)
}

/// Public (no-auth) router: mounted outside the token middleware by anka-server.
pub fn app_update_router(state: Shared) -> Router {
    Router::new()
        .route("/api/app/latest", get(app_latest))
        .route("/api/app/apk", get(app_apk))
        .with_state(state)
}
