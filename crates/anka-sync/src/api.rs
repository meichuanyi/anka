use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::protocol::{PullResponse, PushRequest, PushResponse};
use crate::store::SyncStore;

pub struct AppState {
    pub store: Mutex<SyncStore>,
    pub token: String,
}

pub type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
        .route("/v1/status", get(status))
        .with_state(state)
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    if token.is_empty() {
        return true; // open mode (dev only)
    }
    let Some(h) = headers.get(axum::http::header::AUTHORIZATION) else {
        return false;
    };
    let Ok(s) = h.to_str() else { return false };
    let bearer = s.strip_prefix("Bearer ").unwrap_or(s);
    bearer == token
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "app": "anka-sync" }))
}

async fn status(State(state): State<Shared>, headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorized(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let store = state.store.lock().await;
    let latest = store.latest_seq().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let count = store.count().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "latestSeq": latest,
        "changes": count
    })))
}

async fn push(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, StatusCode> {
    if !authorized(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if req.device_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut changes = req.changes;
    for c in &mut changes {
        if c.device_id.trim().is_empty() {
            c.device_id = req.device_id.clone();
        }
    }
    let mut store = state.store.lock().await;
    let accepted = changes.len() as u32;
    let latest = store
        .append(&changes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(PushResponse {
        accepted,
        latest_seq: latest,
    }))
}

#[derive(Deserialize)]
struct PullQuery {
    #[serde(default)]
    since: i64,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    500
}

async fn pull(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<PullQuery>,
) -> Result<Json<PullResponse>, StatusCode> {
    if !authorized(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let limit = q.limit.clamp(1, 2000);
    let store = state.store.lock().await;
    let (changes, cursor, has_more) = store
        .pull(q.since, limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(PullResponse {
        changes,
        cursor,
        has_more,
    }))
}
