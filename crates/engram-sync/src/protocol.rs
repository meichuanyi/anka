//! Engram sync protocol types (shared by server; client uses serde JSON).

use serde::{Deserialize, Serialize};

/// Opaque cursor returned by pull.
pub type Cursor = i64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Note,
    Card,
    Revlog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// Global sequence assigned by server on pull; ignored/optional on push.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<Cursor>,
    pub kind: ChangeKind,
    /// Stable entity id (engram uuid string).
    pub id: String,
    /// ISO-8601 updated/reviewed time for merge.
    pub updated_at: String,
    /// Device that produced the change (tie-break).
    pub device_id: String,
    /// Kind-specific JSON payload.
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub device_id: String,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResponse {
    pub accepted: u32,
    pub latest_seq: Cursor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResponse {
    pub changes: Vec<Change>,
    pub cursor: Cursor,
    pub has_more: bool,
}

// --- payloads (documented shapes; server stores opaque JSON) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NotePayload {
    pub deck_name: String,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct CardPayload {
    pub note_id: String,
    pub deck_name: String,
    pub template_idx: u32,
    pub due_at: String,
    pub stability: f32,
    pub difficulty: f32,
    pub reps: u32,
    pub lapses: u32,
    pub last_review_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RevlogPayload {
    pub card_id: String,
    pub rating: u8,
    pub reviewed_at: String,
    pub elapsed_ms: u32,
    pub interval_days: f32,
}
