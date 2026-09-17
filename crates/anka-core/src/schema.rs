//! SQLite schema for an Anka collection.

pub const SCHEMA_VERSION: i64 = 1;

pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS decks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT REFERENCES decks(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_decks_name ON decks(name);

CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    notetype TEXT NOT NULL DEFAULT 'basic',
    fields_json TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notes_deck ON notes(deck_id);

CREATE TABLE IF NOT EXISTS cards (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    template_idx INTEGER NOT NULL DEFAULT 0,
    due_at TEXT NOT NULL,
    stability REAL NOT NULL DEFAULT 0,
    difficulty REAL NOT NULL DEFAULT 0,
    reps INTEGER NOT NULL DEFAULT 0,
    lapses INTEGER NOT NULL DEFAULT 0,
    last_review_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cards_due ON cards(due_at);
CREATE INDEX IF NOT EXISTS idx_cards_deck_due ON cards(deck_id, due_at);
CREATE INDEX IF NOT EXISTS idx_cards_note ON cards(note_id);

CREATE TABLE IF NOT EXISTS revlog (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    rating INTEGER NOT NULL,
    reviewed_at TEXT NOT NULL,
    elapsed_ms INTEGER NOT NULL DEFAULT 0,
    stability_after REAL NOT NULL,
    difficulty_after REAL NOT NULL,
    interval_days REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_revlog_card ON revlog(card_id, reviewed_at);

CREATE TABLE IF NOT EXISTS media (
    filename TEXT PRIMARY KEY,
    sha256 TEXT,
    size_bytes INTEGER,
    imported_at TEXT NOT NULL
);

-- Maps Anki ids → Anka ids for export fidelity.
CREATE TABLE IF NOT EXISTS anki_id_map (
    kind TEXT NOT NULL,
    anki_id TEXT NOT NULL,
    anka_id TEXT NOT NULL,
    PRIMARY KEY (kind, anki_id)
);

CREATE INDEX IF NOT EXISTS idx_anki_id_map_anka ON anki_id_map(kind, anka_id);
"#;

use crate::error::Result;

pub fn init(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}
