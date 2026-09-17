use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Again),
            2 => Some(Self::Hard),
            3 => Some(Self::Good),
            4 => Some(Self::Easy),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    New,
    Learning,
    Review,
    Relearning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    pub id: Id,
    pub name: String,
    pub parent_id: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Id,
    pub deck_id: Id,
    /// Notetype name; M0 uses a single default type.
    pub notetype: String,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardState {
    pub due_at: DateTime<Utc>,
    pub stability: f32,
    pub difficulty: f32,
    pub reps: u32,
    pub lapses: u32,
    pub last_review_at: Option<DateTime<Utc>>,
}

impl CardState {
    pub fn new_card(now: DateTime<Utc>) -> Self {
        Self {
            due_at: now,
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            last_review_at: None,
        }
    }

    pub fn kind(&self) -> CardKind {
        if self.reps == 0 {
            CardKind::New
        } else if self.lapses > 0 && self.stability < 1.0 {
            CardKind::Relearning
        } else if self.stability < 1.0 {
            CardKind::Learning
        } else {
            CardKind::Review
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: Id,
    pub note_id: Id,
    pub deck_id: Id,
    /// Card template index within the note (0 = first).
    pub template_idx: u32,
    pub state: CardState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevlogEntry {
    pub id: Id,
    pub card_id: Id,
    pub rating: Rating,
    pub reviewed_at: DateTime<Utc>,
    pub elapsed_ms: u32,
    pub stability_after: f32,
    pub difficulty_after: f32,
    pub interval_days: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DueCard {
    pub card: Card,
    pub note: Note,
}

/// Study-mode for a card. Maps from template_idx and/or deck name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardTemplate {
    /// Word → meaning (default / Recite)
    Recite,
    /// Meaning → word (Spelling)
    Spelling,
    /// Audio/listen → word (Dictation)
    Dictation,
}

impl CardTemplate {
    pub fn from_idx_and_deck(idx: u32, deck_name: &str) -> Self {
        let n = deck_name.to_ascii_lowercase();
        if n.contains("spell") {
            return Self::Spelling;
        }
        if n.contains("dictat") || n.contains("听写") {
            return Self::Dictation;
        }
        match idx {
            1 => Self::Spelling,
            2 => Self::Dictation,
            _ => Self::Recite,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recite => "recite",
            Self::Spelling => "spelling",
            Self::Dictation => "dictation",
        }
    }
}

/// Front/back for a specific study template.
///
/// Vocabulary-style multi-field notes:
/// - field0 headword, field1 phonetic, later fields = definitions/examples.
pub fn front_back_for_template(
    fields: &[String],
    template: CardTemplate,
) -> (String, String, String) {
    let (head, defs_with_meta) = front_back(fields);
    let phonetic = fields
        .iter()
        .map(|s| s.trim())
        .find(|s| is_phonetic(s) || looks_like_phonetic_brackets(s))
        .unwrap_or("")
        .to_string();

    // For inverted templates, prefer definition text without leading phonetic.
    let defs = if phonetic.is_empty() {
        defs_with_meta.clone()
    } else {
        defs_with_meta
            .split(" · ")
            .filter(|p| *p != phonetic)
            .collect::<Vec<_>>()
            .join(" · ")
    };

    match template {
        CardTemplate::Recite => (head, defs_with_meta, phonetic),
        CardTemplate::Spelling => {
            let front = if defs.is_empty() {
                "释义".to_string()
            } else {
                defs
            };
            (front, head, String::new())
        }
        CardTemplate::Dictation => {
            let has_sound = fields.iter().any(|f| is_sound_ref(f));
            let front = if has_sound {
                "听音写词".to_string()
            } else if !phonetic.is_empty() {
                format!("根据音标写词 {phonetic}")
            } else {
                "听写 / 回忆拼写".to_string()
            };
            (front, head, String::new())
        }
    }
}

/// Front/back preview for multi-field notes.
/// Front is the first non-empty field. Back picks short definition-like
/// fields first, skips phonetics/sound/HTML blobs, and strips tags.
pub fn front_back(fields: &[String]) -> (String, String) {
    let front = fields
        .iter()
        .find(|f| !f.trim().is_empty())
        .map(|s| strip_html(s))
        .map(|s| truncate_chars(&s, 120))
        .unwrap_or_default();

    let mut phonetic: Option<String> = None;
    let mut chosen: Vec<String> = Vec::new();
    let mut fallback: Vec<String> = Vec::new();

    for f in fields.iter().skip(1) {
        let raw = f.trim();
        if raw.is_empty() || is_sound_ref(raw) {
            continue;
        }
        if is_phonetic(raw) || looks_like_phonetic_brackets(raw) {
            if phonetic.is_none() {
                phonetic = Some(raw.to_string());
            }
            continue;
        }
        let text = strip_html(raw);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if text.chars().count() <= 160 && !looks_like_html_doc(raw) {
            chosen.push(text.to_string());
        } else {
            fallback.push(text.to_string());
        }
        if chosen.len() >= 4 {
            break;
        }
    }

    if chosen.is_empty() {
        if let Some(fb) = fallback.into_iter().next() {
            chosen.push(truncate_chars(&fb, 200));
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = phonetic {
        parts.push(p);
    }
    parts.extend(chosen);
    if parts.is_empty() {
        if let Some(second) = fields.get(1) {
            let t = strip_html(second);
            let t = t.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }

    (front, parts.join(" · "))
}

fn looks_like_phonetic_brackets(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('[')
        && t.ends_with(']')
        && t.len() <= 24
        && !t.contains(' ')
        && !t.contains("<")
}

fn is_phonetic(s: &str) -> bool {
    let t = s.trim();
    if !(t.starts_with('[') && t.ends_with(']')) {
        return false;
    }
    let inner = &t[1..t.len() - 1];
    inner.contains('ə')
        || inner.contains('ɪ')
        || inner.contains('æ')
        || inner.contains('ʃ')
        || inner.contains('θ')
        || inner.contains('ˈ')
        || inner.contains('ˌ')
        || inner.contains(':')
}

fn is_sound_ref(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("[sound:") || t.starts_with("[Sound:")
}

fn looks_like_html_doc(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("<html")
        || l.contains("<head")
        || l.contains("<div")
        || l.contains("<font")
        || l.chars().filter(|c| *c == '<').count() >= 3
}

/// Minimal HTML/text cleanup for terminal previews.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse whitespace.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    collapsed.trim().to_string()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

/// Extract `[sound:file.mp3]` / `[Sound:file.mp3]` references from note fields.
pub fn extract_sounds(fields: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for field in fields {
        let mut rest = field.as_str();
        while let Some(start) = rest.find("[sound:") {
            let after = &rest[start + 7..];
            if let Some(end) = after.find(']') {
                let name = after[..end].trim();
                if !name.is_empty() {
                    out.push(name.to_string());
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
        // Also handle capitalized [Sound:...]
        let mut rest = field.as_str();
        while let Some(start) = rest.find("[Sound:") {
            let after = &rest[start + 7..];
            if let Some(end) = after.find(']') {
                let name = after[..end].trim();
                if !name.is_empty() && !out.iter().any(|s| s == name) {
                    out.push(name.to_string());
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
    }
    out
}

/// Absolute media directory for a collection path (`<parent>/media`).
pub fn media_dir_for_collection(collection_path: &std::path::Path) -> std::path::PathBuf {
    collection_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("media")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckCounts {
    pub deck_id: Id,
    pub name: String,
    pub new_count: u64,
    pub learning_count: u64,
    pub review_count: u64,
}
