//! Collection = one user library (SQLite file + media dir).

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::ids::Id;
use crate::model::{Card, Deck, DeckCounts, DueCard, Note, Rating, RevlogEntry};
use crate::scheduler::Scheduler;
use crate::schema;
use crate::store::Store;

pub struct Collection {
    conn: Connection,
    path: PathBuf,
    scheduler: Scheduler,
}

impl Collection {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        schema::init(&conn)?;
        Ok(Self {
            conn,
            path,
            scheduler: Scheduler::new(None)?,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(Error::NotFound(format!(
                "collection not found: {}",
                path.display()
            )));
        }
        let conn = Connection::open(&path)?;
        let mut col = Self {
            conn,
            path,
            scheduler: Scheduler::new(None)?,
        };
        col.reload_scheduler()?;
        Ok(col)
    }

    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            Self::open(path)
        } else {
            Self::create(path)
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn store(&self) -> Store<'_> {
        Store::new(&self.conn)
    }

    pub fn ensure_deck(&mut self, name: &str) -> Result<Deck> {
        if let Some(d) = self.store().deck_by_name(name)? {
            return Ok(d);
        }
        let now = Utc::now();
        let deck = Deck {
            id: Id::new(),
            name: name.to_string(),
            parent_id: None,
            created_at: now,
            updated_at: now,
        };
        self.store().insert_deck(&deck)?;
        Ok(deck)
    }

    pub fn list_decks(&self) -> Result<Vec<Deck>> {
        self.store().list_decks()
    }

    /// Add a basic note: `fields[0]` = front, `fields[1]` = back when present.
    /// Creates one card.
    pub fn add_note(
        &mut self,
        deck_id: Id,
        fields: Vec<String>,
        tags: Vec<String>,
    ) -> Result<(Note, Card)> {
        let note = self.add_note_only(deck_id, "basic".into(), fields, tags)?;
        let card = self.add_card_for_note(note.id, deck_id, 0)?;
        Ok((note, card))
    }

    /// Insert a note without creating cards.
    pub fn add_note_only(
        &mut self,
        deck_id: Id,
        notetype: String,
        fields: Vec<String>,
        tags: Vec<String>,
    ) -> Result<Note> {
        let now = Utc::now();
        let note = Note {
            id: Id::new(),
            deck_id,
            notetype,
            fields,
            tags,
            created_at: now,
            updated_at: now,
        };
        self.store().insert_note(&note)?;
        Ok(note)
    }

    /// Attach a new card to an existing note (multi-template / multi-deck).
    pub fn add_card_for_note(
        &mut self,
        note_id: Id,
        deck_id: Id,
        template_idx: u32,
    ) -> Result<Card> {
        let now = Utc::now();
        let card = Card {
            id: Id::new(),
            note_id,
            deck_id,
            template_idx,
            state: crate::model::CardState::new_card(now),
            created_at: now,
            updated_at: now,
        };
        self.store().insert_card(&card)?;
        Ok(card)
    }

    pub fn begin(&mut self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    pub fn search_notes(&self, query: &str, limit: u32, offset: u32) -> Result<(u64, Vec<Note>)> {
        self.store().search_notes(query, limit, offset)
    }

    pub fn get_note(&self, id: Id) -> Result<Option<Note>> {
        self.store().get_note(id)
    }

    pub fn get_card(&self, id: Id) -> Result<Option<Card>> {
        self.store().get_card(id)
    }

    pub fn update_note_fields(
        &mut self,
        id: Id,
        fields: Vec<String>,
        tags: Vec<String>,
    ) -> Result<Note> {
        let mut note = self
            .store()
            .get_note(id)?
            .ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        let now = Utc::now();
        self.store().update_note_fields(id, &fields, &tags, now)?;
        note.fields = fields;
        note.tags = tags;
        note.updated_at = now;
        Ok(note)
    }

    pub fn due(&self, deck_id: Option<Id>, limit: u32) -> Result<Vec<DueCard>> {
        let now = Utc::now();
        let cards = self.store().due_cards(deck_id, now, limit)?;
        let mut out = Vec::with_capacity(cards.len());
        for card in cards {
            let note = self
                .store()
                .get_note(card.note_id)?
                .ok_or_else(|| Error::NotFound(format!("note {}", card.note_id)))?;
            out.push(DueCard { card, note });
        }
        Ok(out)
    }

    pub fn answer_card(&mut self, card_id: Id, rating: Rating, elapsed_ms: u32) -> Result<Card> {
        let mut card = self
            .store()
            .get_card(card_id)?
            .ok_or_else(|| Error::NotFound(format!("card {card_id}")))?;
        let now = Utc::now();
        let next = self.scheduler.next_state(&card.state, rating, now)?;
        let interval_days = (next.due_at - now).num_seconds().max(0) as f32 / 86_400.0;
        let revlog = RevlogEntry {
            id: Id::new(),
            card_id,
            rating,
            reviewed_at: now,
            elapsed_ms,
            stability_after: next.stability,
            difficulty_after: next.difficulty,
            interval_days,
        };
        self.store().update_card_state(card_id, &next, now)?;
        self.store().insert_revlog(&revlog)?;
        card.state = next;
        card.updated_at = now;
        Ok(card)
    }

    pub fn deck_counts(&self) -> Result<Vec<DeckCounts>> {
        let now = Utc::now();
        let rows = self.store().count_cards(None, now)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, new_count, learning_count, review_count)| DeckCounts {
                deck_id: id,
                name,
                new_count,
                learning_count,
                review_count,
            })
            .collect())
    }

    pub fn all_revlog(&self) -> Result<Vec<RevlogEntry>> {
        self.store().all_revlog()
    }

    pub fn revlog_for_card(&self, card_id: Id) -> Result<Vec<RevlogEntry>> {
        self.store().revlog_for_card(card_id)
    }

    pub fn all_notes(&self) -> Result<Vec<Note>> {
        self.store().all_notes()
    }

    pub fn all_cards(&self) -> Result<Vec<Card>> {
        self.store().all_cards()
    }

    pub fn has_revlog(&self, id: Id) -> Result<bool> {
        self.store().has_revlog(id)
    }

    pub fn upsert_note_sync(&mut self, note: Note) -> Result<()> {
        if let Some(d) = self.store().get_deck(note.deck_id)? {
            let _ = d;
        } else {
            // Ensure deck exists under same id — insert minimal deck row via ensure + update id is hard;
            // create deck by name if missing using note.deck_id as identity is not supported.
            // Callers should resolve deck by name first.
        }
        self.store().upsert_note_from_sync(&note)
    }

    pub fn upsert_card_sync(&mut self, card: Card) -> Result<()> {
        self.store().upsert_card_from_sync(&card)
    }

    pub fn deck_by_name_or_create(&mut self, name: &str) -> Result<Deck> {
        self.ensure_deck(name)
    }

    pub fn put_anki_id(&mut self, kind: &str, anki_id: &str, engram_id: Id) -> Result<()> {
        self.store().put_anki_id(kind, anki_id, engram_id)
    }

    pub fn engram_id_for_anki(&self, kind: &str, anki_id: &str) -> Result<Option<Id>> {
        self.store().get_anki_id(kind, anki_id)
    }

    pub fn add_revlog(&mut self, entry: RevlogEntry) -> Result<()> {
        self.store().insert_revlog(&entry)
    }

    /// Replace the live scheduler with personalized weights.
    pub fn set_scheduler_params(&mut self, weights: Vec<f32>) -> Result<()> {
        self.scheduler = Scheduler::new(Some(weights))?;
        Ok(())
    }

    /// Reload scheduler weights from meta (if present).
    pub fn reload_scheduler(&mut self) -> Result<()> {
        if let Some(w) = self.fsrs_params()? {
            self.scheduler = Scheduler::new(Some(w))?;
        }
        Ok(())
    }
}
