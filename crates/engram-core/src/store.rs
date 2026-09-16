//! Thin SQLite access layer for collection entities.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::ids::Id;
use crate::model::{Card, CardState, Deck, Note, Rating, RevlogEntry};

fn parse_id(s: &str) -> Result<Id> {
    Uuid::parse_str(s)
        .map(Id::from)
        .map_err(|e| Error::Other(format!("invalid id '{s}': {e}")))
}

fn dt(s: &str) -> Result<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>()
        .map_err(|e| Error::Other(format!("invalid timestamp '{s}': {e}")))
}

fn dt_s(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

pub struct Store<'c> {
    conn: &'c Connection,
}

impl<'c> Store<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    // --- decks ---

    pub fn insert_deck(&self, deck: &Deck) -> Result<()> {
        self.conn.execute(
            "INSERT INTO decks (id, name, parent_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                deck.id.to_string(),
                deck.name,
                deck.parent_id.map(|p| p.to_string()),
                dt_s(deck.created_at),
                dt_s(deck.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn get_deck(&self, id: Id) -> Result<Option<Deck>> {
        self.conn
            .query_row(
                "SELECT id, name, parent_id, created_at, updated_at FROM decks WHERE id = ?1",
                [id.to_string()],
                row_to_deck,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn deck_by_name(&self, name: &str) -> Result<Option<Deck>> {
        self.conn
            .query_row(
                "SELECT id, name, parent_id, created_at, updated_at FROM decks WHERE name = ?1",
                [name],
                row_to_deck,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_decks(&self) -> Result<Vec<Deck>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, created_at, updated_at FROM decks ORDER BY name",
        )?;
        let rows = stmt.query_map([], row_to_deck)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    // --- notes ---

    pub fn insert_note(&self, note: &Note) -> Result<()> {
        let fields_json = serde_json::to_string(&note.fields)
            .map_err(|e| Error::Other(e.to_string()))?;
        let tags_json =
            serde_json::to_string(&note.tags).map_err(|e| Error::Other(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO notes (id, deck_id, notetype, fields_json, tags_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                note.id.to_string(),
                note.deck_id.to_string(),
                note.notetype,
                fields_json,
                tags_json,
                dt_s(note.created_at),
                dt_s(note.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn get_note(&self, id: Id) -> Result<Option<Note>> {
        self.conn
            .query_row(
                "SELECT id, deck_id, notetype, fields_json, tags_json, created_at, updated_at
                 FROM notes WHERE id = ?1",
                [id.to_string()],
                row_to_note,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn search_notes(&self, query: &str, limit: u32, offset: u32) -> Result<(u64, Vec<Note>)> {
        let like = format!("%{}%", query);
        let total: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE fields_json LIKE ?1 OR tags_json LIKE ?1",
            [&like],
            |r| r.get(0),
        )?;
        // Prefer exact/substring hits on the first field (headword) so vocabulary
        // decks surface the word itself before incidental body/HTML matches.
        let mut stmt = self.conn.prepare(
            "SELECT id, deck_id, notetype, fields_json, tags_json, created_at, updated_at
             FROM notes
             WHERE fields_json LIKE ?1 OR tags_json LIKE ?1
             ORDER BY
               CASE WHEN COALESCE(json_extract(fields_json, '$[0]'), '') LIKE ?1 THEN 0 ELSE 1 END,
               updated_at DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![like, limit, offset], row_to_note)?;
        let items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok((total, items))
    }

    pub fn update_note_fields(
        &self,
        id: Id,
        fields: &[String],
        tags: &[String],
        updated_at: DateTime<Utc>,
    ) -> Result<()> {
        let fields_json =
            serde_json::to_string(fields).map_err(|e| Error::Other(e.to_string()))?;
        let tags_json =
            serde_json::to_string(tags).map_err(|e| Error::Other(e.to_string()))?;
        self.conn.execute(
            "UPDATE notes SET fields_json = ?2, tags_json = ?3, updated_at = ?4 WHERE id = ?1",
            params![id.to_string(), fields_json, tags_json, dt_s(updated_at)],
        )?;
        Ok(())
    }

    // --- cards ---

    pub fn insert_card(&self, card: &Card) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cards (
                id, note_id, deck_id, template_idx,
                due_at, stability, difficulty, reps, lapses, last_review_at,
                created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                card.id.to_string(),
                card.note_id.to_string(),
                card.deck_id.to_string(),
                card.template_idx as i64,
                dt_s(card.state.due_at),
                card.state.stability,
                card.state.difficulty,
                card.state.reps as i64,
                card.state.lapses as i64,
                card.state.last_review_at.map(dt_s),
                dt_s(card.created_at),
                dt_s(card.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn get_card(&self, id: Id) -> Result<Option<Card>> {
        self.conn
            .query_row(
                "SELECT id, note_id, deck_id, template_idx, due_at, stability, difficulty,
                        reps, lapses, last_review_at, created_at, updated_at
                 FROM cards WHERE id = ?1",
                [id.to_string()],
                row_to_card,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn due_cards(&self, deck_id: Option<Id>, now: DateTime<Utc>, limit: u32) -> Result<Vec<Card>> {
        let now_s = dt_s(now);
        match deck_id {
            Some(d) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, note_id, deck_id, template_idx, due_at, stability, difficulty,
                            reps, lapses, last_review_at, created_at, updated_at
                     FROM cards
                     WHERE deck_id = ?1 AND due_at <= ?2
                     ORDER BY due_at ASC
                     LIMIT ?3",
                )?;
                let rows = stmt.query_map(params![d.to_string(), now_s, limit as i64], row_to_card)?;
                rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, note_id, deck_id, template_idx, due_at, stability, difficulty,
                            reps, lapses, last_review_at, created_at, updated_at
                     FROM cards
                     WHERE due_at <= ?1
                     ORDER BY due_at ASC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![now_s, limit as i64], row_to_card)?;
                rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
            }
        }
    }

    pub fn update_card_state(&self, id: Id, state: &CardState, updated_at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE cards SET due_at=?2, stability=?3, difficulty=?4, reps=?5, lapses=?6,
                              last_review_at=?7, updated_at=?8
             WHERE id=?1",
            params![
                id.to_string(),
                dt_s(state.due_at),
                state.stability,
                state.difficulty,
                state.reps as i64,
                state.lapses as i64,
                state.last_review_at.map(dt_s),
                dt_s(updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn count_cards(&self, deck_id: Option<Id>, now: DateTime<Utc>) -> Result<Vec<(Id, String, u64, u64, u64)>> {
        // (deck_id, name, new, learning, review)
        let now_s = dt_s(now);
        let sql = r#"
            SELECT d.id, d.name,
              SUM(CASE WHEN c.reps = 0 THEN 1 ELSE 0 END) as new_count,
              SUM(CASE WHEN c.reps > 0 AND c.stability < 1.0 THEN 1 ELSE 0 END) as learning_count,
              SUM(CASE WHEN c.reps > 0 AND c.stability >= 1.0 AND c.due_at <= ?1 THEN 1 ELSE 0 END) as review_due
            FROM decks d
            LEFT JOIN cards c ON c.deck_id = d.id
            WHERE (?2 IS NULL OR d.id = ?2)
            GROUP BY d.id
            ORDER BY d.name
        "#;
        let deck_filter = deck_id.map(|d| d.to_string());
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![now_s, deck_filter], |row| {
            let id_s: String = row.get(0)?;
            let name: String = row.get(1)?;
            let new_c: i64 = row.get(2)?;
            let learn_c: i64 = row.get(3)?;
            let rev_c: i64 = row.get(4)?;
            Ok((id_s, name, new_c as u64, learn_c as u64, rev_c as u64))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id_s, name, n, l, rev) = r?;
            out.push((parse_id(&id_s)?, name, n, l, rev));
        }
        Ok(out)
    }

    // --- revlog ---

    pub fn insert_revlog(&self, entry: &RevlogEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO revlog (id, card_id, rating, reviewed_at, elapsed_ms,
                                 stability_after, difficulty_after, interval_days)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                entry.id.to_string(),
                entry.card_id.to_string(),
                entry.rating.as_u8() as i64,
                dt_s(entry.reviewed_at),
                entry.elapsed_ms as i64,
                entry.stability_after,
                entry.difficulty_after,
                entry.interval_days,
            ],
        )?;
        Ok(())
    }

    pub fn revlog_for_card(&self, card_id: Id) -> Result<Vec<RevlogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, card_id, rating, reviewed_at, elapsed_ms, stability_after, difficulty_after, interval_days
             FROM revlog WHERE card_id = ?1 ORDER BY reviewed_at ASC",
        )?;
        let rows = stmt.query_map([card_id.to_string()], row_to_revlog)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn all_revlog(&self) -> Result<Vec<RevlogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, card_id, rating, reviewed_at, elapsed_ms, stability_after, difficulty_after, interval_days
             FROM revlog ORDER BY card_id ASC, reviewed_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_revlog)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn all_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, deck_id, notetype, fields_json, tags_json, created_at, updated_at
             FROM notes ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], row_to_note)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn all_cards(&self) -> Result<Vec<Card>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, deck_id, template_idx, due_at, stability, difficulty,
                    reps, lapses, last_review_at, created_at, updated_at
             FROM cards ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], row_to_card)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn has_revlog(&self, id: Id) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM revlog WHERE id = ?1",
            [id.to_string()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn upsert_note_from_sync(
        &self,
        note: &Note,
    ) -> Result<()> {
        let fields_json = serde_json::to_string(&note.fields)
            .map_err(|e| Error::Other(e.to_string()))?;
        let tags_json =
            serde_json::to_string(&note.tags).map_err(|e| Error::Other(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO notes (id, deck_id, notetype, fields_json, tags_json, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET
               fields_json=excluded.fields_json,
               tags_json=excluded.tags_json,
               updated_at=excluded.updated_at",
            params![
                note.id.to_string(),
                note.deck_id.to_string(),
                note.notetype,
                fields_json,
                tags_json,
                dt_s(note.created_at),
                dt_s(note.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_card_from_sync(&self, card: &Card) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cards (
                id, note_id, deck_id, template_idx,
                due_at, stability, difficulty, reps, lapses, last_review_at,
                created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET
               note_id=excluded.note_id,
               deck_id=excluded.deck_id,
               template_idx=excluded.template_idx,
               due_at=excluded.due_at,
               stability=excluded.stability,
               difficulty=excluded.difficulty,
               reps=excluded.reps,
               lapses=excluded.lapses,
               last_review_at=excluded.last_review_at,
               updated_at=excluded.updated_at",
            params![
                card.id.to_string(),
                card.note_id.to_string(),
                card.deck_id.to_string(),
                card.template_idx as i64,
                dt_s(card.state.due_at),
                card.state.stability,
                card.state.difficulty,
                card.state.reps as i64,
                card.state.lapses as i64,
                card.state.last_review_at.map(dt_s),
                dt_s(card.created_at),
                dt_s(card.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    // --- anki map ---

    pub fn put_anki_id(&self, kind: &str, anki_id: &str, engram_id: Id) -> Result<()> {
        self.conn.execute(
            "INSERT INTO anki_id_map (kind, anki_id, engram_id) VALUES (?1,?2,?3)
             ON CONFLICT(kind, anki_id) DO UPDATE SET engram_id = excluded.engram_id",
            params![kind, anki_id, engram_id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_anki_id(&self, kind: &str, anki_id: &str) -> Result<Option<Id>> {
        self.conn
            .query_row(
                "SELECT engram_id FROM anki_id_map WHERE kind = ?1 AND anki_id = ?2",
                params![kind, anki_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|s| parse_id(&s))
            .transpose()
    }
}

fn row_to_deck(row: &Row<'_>) -> rusqlite::Result<Deck> {
    let id_s: String = row.get(0)?;
    let name: String = row.get(1)?;
    let parent_s: Option<String> = row.get(2)?;
    let created_s: String = row.get(3)?;
    let updated_s: String = row.get(4)?;
    Ok(Deck {
        id: parse_id(&id_s).unwrap_or_else(|_| Id::new()),
        name,
        parent_id: parent_s.and_then(|p| parse_id(&p).ok()),
        created_at: dt(&created_s).unwrap_or_else(|_| Utc::now()),
        updated_at: dt(&updated_s).unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_note(row: &Row<'_>) -> rusqlite::Result<Note> {
    let id_s: String = row.get(0)?;
    let deck_s: String = row.get(1)?;
    let notetype: String = row.get(2)?;
    let fields_json: String = row.get(3)?;
    let tags_json: String = row.get(4)?;
    let created_s: String = row.get(5)?;
    let updated_s: String = row.get(6)?;
    let fields = serde_json::from_str(&fields_json).unwrap_or_default();
    let tags = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Note {
        id: parse_id(&id_s).unwrap_or_else(|_| Id::new()),
        deck_id: parse_id(&deck_s).unwrap_or_else(|_| Id::new()),
        notetype,
        fields,
        tags,
        created_at: dt(&created_s).unwrap_or_else(|_| Utc::now()),
        updated_at: dt(&updated_s).unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_card(row: &Row<'_>) -> rusqlite::Result<Card> {
    let id_s: String = row.get(0)?;
    let note_s: String = row.get(1)?;
    let deck_s: String = row.get(2)?;
    let template_idx: i64 = row.get(3)?;
    let due_s: String = row.get(4)?;
    let stability: f32 = row.get(5)?;
    let difficulty: f32 = row.get(6)?;
    let reps: i64 = row.get(7)?;
    let lapses: i64 = row.get(8)?;
    let last_s: Option<String> = row.get(9)?;
    let created_s: String = row.get(10)?;
    let updated_s: String = row.get(11)?;
    Ok(Card {
        id: parse_id(&id_s).unwrap_or_else(|_| Id::new()),
        note_id: parse_id(&note_s).unwrap_or_else(|_| Id::new()),
        deck_id: parse_id(&deck_s).unwrap_or_else(|_| Id::new()),
        template_idx: template_idx as u32,
        state: CardState {
            due_at: dt(&due_s).unwrap_or_else(|_| Utc::now()),
            stability,
            difficulty,
            reps: reps as u32,
            lapses: lapses as u32,
            last_review_at: last_s.and_then(|s| dt(&s).ok()),
        },
        created_at: dt(&created_s).unwrap_or_else(|_| Utc::now()),
        updated_at: dt(&updated_s).unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_revlog(row: &Row<'_>) -> rusqlite::Result<RevlogEntry> {
    let id_s: String = row.get(0)?;
    let card_s: String = row.get(1)?;
    let rating: i64 = row.get(2)?;
    let reviewed_s: String = row.get(3)?;
    let elapsed: i64 = row.get(4)?;
    let stab: f32 = row.get(5)?;
    let diff: f32 = row.get(6)?;
    let interval: f32 = row.get(7)?;
    Ok(RevlogEntry {
        id: parse_id(&id_s).unwrap_or_else(|_| Id::new()),
        card_id: parse_id(&card_s).unwrap_or_else(|_| Id::new()),
        rating: Rating::from_u8(rating as u8).unwrap_or(Rating::Good),
        reviewed_at: dt(&reviewed_s).unwrap_or_else(|_| Utc::now()),
        elapsed_ms: elapsed as u32,
        stability_after: stab,
        difficulty_after: diff,
        interval_days: interval,
    })
}
