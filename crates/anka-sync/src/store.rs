//! SQLite-backed append-only change log for sync.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::protocol::{Change, Cursor};

pub struct SyncStore {
    conn: Connection,
}

impl SyncStore {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).context("open sync db")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS changes (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                device_id TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_changes_kind_id ON changes(kind, entity_id);
            CREATE INDEX IF NOT EXISTS idx_changes_seq ON changes(seq);
            "#,
        )?;
        Ok(Self { conn })
    }

    pub fn latest_seq(&self) -> Result<Cursor> {
        let seq: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM changes", [], |r| r.get(0))?;
        Ok(seq)
    }

    pub fn append(&mut self, changes: &[Change]) -> Result<Cursor> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO changes (kind, entity_id, updated_at, device_id, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for c in changes {
                let kind = match c.kind {
                    crate::protocol::ChangeKind::Note => "note",
                    crate::protocol::ChangeKind::Card => "card",
                    crate::protocol::ChangeKind::Revlog => "revlog",
                };
                let payload = serde_json::to_string(&c.payload)?;
                stmt.execute(params![
                    kind,
                    c.id,
                    c.updated_at,
                    c.device_id,
                    payload
                ])?;
            }
        }
        tx.commit()?;
        self.latest_seq()
    }

    pub fn pull(&self, since: Cursor, limit: u32) -> Result<(Vec<Change>, Cursor, bool)> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, kind, entity_id, updated_at, device_id, payload
             FROM changes
             WHERE seq > ?1
             ORDER BY seq ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit as i64], |r| {
            let seq: i64 = r.get(0)?;
            let kind: String = r.get(1)?;
            let id: String = r.get(2)?;
            let updated_at: String = r.get(3)?;
            let device_id: String = r.get(4)?;
            let payload: String = r.get(5)?;
            Ok((seq, kind, id, updated_at, device_id, payload))
        })?;

        let mut out = Vec::new();
        let mut last = since;
        for row in rows {
            let (seq, kind, id, updated_at, device_id, payload) = row?;
            let kind = match kind.as_str() {
                "note" => crate::protocol::ChangeKind::Note,
                "card" => crate::protocol::ChangeKind::Card,
                _ => crate::protocol::ChangeKind::Revlog,
            };
            let payload: serde_json::Value =
                serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
            out.push(Change {
                seq: Some(seq),
                kind,
                id,
                updated_at,
                device_id,
                payload,
            });
            last = seq;
        }
        let has_more = out.len() as u32 >= limit;
        Ok((out, last, has_more))
    }

    pub fn count(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM changes", [], |r| r.get(0))?;
        Ok(n)
    }
}

/// Dedup key helper used by clients when applying revlog.
#[allow(dead_code)]
pub fn revlog_unique_key(change: &Change) -> String {
    format!("revlog:{}", change.id)
}

#[allow(dead_code)]
fn _unused_optional(_conn: &Connection) -> Result<Option<i64>> {
    Ok(_conn
        .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
        .optional()?)
}
