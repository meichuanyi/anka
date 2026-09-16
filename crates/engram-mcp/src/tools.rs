//! MCP tool implementations for Engram.

use std::sync::Mutex;

use engram_core::{Collection, Id, Rating};
use serde_json::{json, Value};
use uuid::Uuid;

pub struct ToolError {
    pub message: String,
}

impl ToolError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

type ToolResult<T = Value> = Result<T, ToolError>;

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "deck.list",
            "List decks with new/learning/due counts",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Optional name substring filter" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "note.search",
            "Search notes by substring in fields or tags",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 20 },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "note.create",
            "Create a basic front/back note in a deck",
            json!({
                "type": "object",
                "properties": {
                    "deck": { "type": "string", "description": "Deck name (created if missing)" },
                    "front": { "type": "string" },
                    "back": { "type": "string" },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["deck", "front", "back"],
                "additionalProperties": false
            }),
        ),
        tool(
            "review.due",
            "List due cards (front/back preview)",
            json!({
                "type": "object",
                "properties": {
                    "deck": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "review.grade",
            "Grade one card (1=Again 2=Hard 3=Good 4=Easy)",
            json!({
                "type": "object",
                "properties": {
                    "cardId": { "type": "string" },
                    "rating": { "type": "integer", "minimum": 1, "maximum": 4 }
                },
                "required": ["cardId", "rating"],
                "additionalProperties": false
            }),
        ),
        tool(
            "stats.weak",
            "Summarize lapses and hard cards over a recent window",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "fsrs.optimize",
            "Optimize FSRS parameters from review history (apply=false previews)",
            json!({
                "type": "object",
                "properties": {
                    "apply": {
                        "type": "boolean",
                        "default": false,
                        "description": "Persist weights to the collection (L3)"
                    }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

pub fn call_tool(state: &Mutex<Collection>, params: &Value) -> ToolResult {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("missing tool name"))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    match name {
        "deck.list" => deck_list(state, &args),
        "note.search" => note_search(state, &args),
        "note.create" => note_create(state, &args),
        "review.due" => review_due(state, &args),
        "review.grade" => review_grade(state, &args),
        "stats.weak" => stats_weak(state, &args),
        "fsrs.optimize" => fsrs_optimize(state, &args),
        other => Err(ToolError::msg(format!("unknown tool: {other}"))),
    }
}

fn text_content(value: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }],
        "structuredContent": value,
        "isError": false
    })
}

fn error_content(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

/// Wrap handler errors into MCP tool-result errors (protocol ok, tool failed).
pub fn call_tool_wrapped(state: &Mutex<Collection>, params: &Value) -> Value {
    match call_tool(state, params) {
        Ok(v) => v,
        Err(e) => error_content(e.message),
    }
}

fn deck_list(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let counts = col
        .deck_counts()
        .map_err(|e| ToolError::msg(e.to_string()))?;
    let decks: Vec<Value> = counts
        .into_iter()
        .filter(|d| query.is_empty() || d.name.contains(query))
        .map(|d| {
            json!({
                "id": d.deck_id.to_string(),
                "name": d.name,
                "new": d.new_count,
                "learning": d.learning_count,
                "due": d.review_count
            })
        })
        .collect();
    Ok(text_content(json!({ "decks": decks })))
}

fn note_search(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("query is required"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let (total, notes) = col
        .search_notes(query, limit, offset)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    let items: Vec<Value> = notes
        .into_iter()
        .map(|n| {
            let (front, back) = engram_core::front_back(&n.fields);
            json!({
                "id": n.id.to_string(),
                "deckId": n.deck_id.to_string(),
                "front": front,
                "back": back,
                "fields": truncate_fields(&n.fields),
                "tags": n.tags
            })
        })
        .collect();
    Ok(text_content(json!({ "total": total, "items": items })))
}

fn note_create(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let mut col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let deck = args
        .get("deck")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("deck is required"))?;
    let front = args
        .get("front")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("front is required"))?;
    let back = args
        .get("back")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("back is required"))?;
    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let d = col
        .ensure_deck(deck)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    let (note, card) = col
        .add_note(d.id, vec![front.to_string(), back.to_string()], tags)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    Ok(text_content(json!({
        "noteId": note.id.to_string(),
        "cardId": card.id.to_string(),
        "deckId": d.id.to_string()
    })))
}

fn review_due(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
    let deck_id = match args.get("deck").and_then(|v| v.as_str()) {
        Some(name) => {
            let decks = col.list_decks().map_err(|e| ToolError::msg(e.to_string()))?;
            let found = decks.iter().find(|d| d.name == name).map(|d| d.id);
            Some(found.ok_or_else(|| ToolError::msg(missing_deck_message(name, &decks)))?)
        }
        None => None,
    };
    let due = col
        .due(deck_id, limit)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    let items: Vec<Value> = due
        .into_iter()
        .map(|item| {
            let (front, back) = engram_core::front_back(&item.note.fields);
            json!({
                "cardId": item.card.id.to_string(),
                "deckId": item.card.deck_id.to_string(),
                "front": front,
                "back": back,
                "fields": truncate_fields(&item.note.fields),
                "kind": format!("{:?}", item.card.state.kind()).to_lowercase(),
                "dueAt": item.card.state.due_at.to_rfc3339(),
                "stability": item.card.state.stability,
                "lapses": item.card.state.lapses
            })
        })
        .collect();
    Ok(text_content(json!({ "cards": items })))
}

/// Keep agent payloads small: real Anki notes can carry multi-KB HTML dictionary blobs.
fn truncate_fields(fields: &[String]) -> Vec<String> {
    const MAX: usize = 240;
    fields
        .iter()
        .map(|f| {
            if f.chars().count() <= MAX {
                f.clone()
            } else {
                let head: String = f.chars().take(MAX.saturating_sub(1)).collect();
                format!("{head}…")
            }
        })
        .collect()
}

fn missing_deck_message(name: &str, decks: &[engram_core::Deck]) -> String {
    if decks.is_empty() {
        return format!("deck not found: {name} (collection has no decks)");
    }
    let mut msg = format!("deck not found: {name}\navailable decks:");
    for d in decks.iter().take(20) {
        msg.push_str(&format!("\n  {}", d.name));
    }
    if decks.len() > 20 {
        msg.push_str(&format!("\n  … and {} more", decks.len() - 20));
    }
    msg
}

fn review_grade(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let mut col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let card_id_s = args
        .get("cardId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::msg("cardId is required"))?;
    let rating_n = args
        .get("rating")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ToolError::msg("rating is required"))?;
    let rating = Rating::from_u8(rating_n as u8)
        .ok_or_else(|| ToolError::msg("rating must be 1-4"))?;
    let uuid = Uuid::parse_str(card_id_s).map_err(|_| ToolError::msg("invalid cardId uuid"))?;
    let card_id = Id::from(uuid);
    let card = col
        .answer_card(card_id, rating, 0)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    Ok(text_content(json!({
        "cardId": card.id.to_string(),
        "nextDue": card.state.due_at.to_rfc3339(),
        "stability": card.state.stability,
        "difficulty": card.state.difficulty,
        "reps": card.state.reps,
        "lapses": card.state.lapses
    })))
}

fn stats_weak(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as u32;

    // M0 heuristic: scan due+known cards via deck counts + search empty for sample.
    // Prefer cards with lapses > 0 among due cards, else lowest stability.
    let mut due = col.due(None, 200).map_err(|e| ToolError::msg(e.to_string()))?;
    due.sort_by(|a, b| {
        b.card
            .state
            .lapses
            .cmp(&a.card.state.lapses)
            .then(
                a.card
                    .state
                    .stability
                    .partial_cmp(&b.card.state.stability)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let hard: Vec<Value> = due
        .into_iter()
        .take(limit as usize)
        .map(|item| {
            let (front, _back) = engram_core::front_back(&item.note.fields);
            json!({
                "cardId": item.card.id.to_string(),
                "front": front,
                "lapses": item.card.state.lapses,
                "stability": item.card.state.stability,
                "suggestion": if item.card.state.lapses > 0 {
                    "lapsed — consider rewriting or splitting this card"
                } else {
                    "low stability — schedule is aggressive or material is dense"
                }
            })
        })
        .collect();

    let decks = col.deck_counts().map_err(|e| ToolError::msg(e.to_string()))?;
    Ok(text_content(json!({
        "hardCards": hard,
        "decks": decks.iter().map(|d| json!({
            "name": d.name,
            "new": d.new_count,
            "learning": d.learning_count,
            "due": d.review_count
        })).collect::<Vec<_>>(),
        "note": "MVP weak-spot heuristic; days window is reserved for revlog analysis"
    })))
}

fn fsrs_optimize(state: &Mutex<Collection>, args: &Value) -> ToolResult {
    let mut col = state.lock().map_err(|_| ToolError::msg("collection lock poisoned"))?;
    let apply = args.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
    // L3: require explicit apply=true; always default preview.
    let report = col
        .optimize_fsrs(apply)
        .map_err(|e| ToolError::msg(e.to_string()))?;
    Ok(text_content(json!({
        "reviewCount": report.review_count,
        "itemCount": report.item_count,
        "cardCount": report.card_count,
        "params": report.params,
        "logLoss": report.log_loss,
        "applied": apply,
        "minRecommendedReviews": engram_core::MIN_REVIEWS_FOR_OPTIMIZE
    })))
}
