//! Export an Engram collection to an Anki `.apkg` (Legacy1 / V11).
//!
//! Symmetric with `import_apkg`: writes `collection.anki2` (uncompressed
//! SQLite, `ver=11`), a JSON media map, and media payloads when present.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use engram_core::Collection;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FLD_SEP: char = '\x1f';
const MODEL_ID: i64 = 1_600_000_000_000;
const DEFAULT_DECK_ID: i64 = 1;
const DECK_ID_BASE: i64 = 1_500_000_000_000;
const NOTE_ID_BASE: i64 = 1_700_000_000_000;
const CARD_ID_BASE: i64 = 1_700_000_100_000;

#[derive(Debug, Default, Clone)]
pub struct ExportReport {
    pub decks: u32,
    pub notes: u32,
    pub cards: u32,
    pub media_exported: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ExportOptions {
    /// Only export notes/cards belonging to this deck name.
    pub deck: Option<String>,
}

#[derive(Debug, Clone)]
struct ExportNote {
    id: String,
    deck_id: String,
    fields: Vec<String>,
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
struct ExportCard {
    id: String,
    note_id: String,
    deck_id: String,
    template_idx: u32,
}

struct ExportData {
    decks: Vec<(String, String)>,
    notes: Vec<ExportNote>,
    cards: Vec<ExportCard>,
}

/// Export `col` to an Anki Legacy1 (V11) `.apkg` at `out_path`.
pub fn export_apkg(
    col: &Collection,
    out_path: impl AsRef<Path>,
    options: ExportOptions,
) -> anyhow::Result<ExportReport> {
    let out_path = out_path.as_ref();
    let mut report = ExportReport::default();

    let data = load_export_data(col, options.deck.as_deref())?;
    let anki_ids = load_anki_id_map(col)?;

    // --- assign Anki deck ids (always include Default) ---
    let mut deck_anki: HashMap<String, i64> = HashMap::new();
    let mut deck_json = Map::new();
    deck_anki.insert("Default".into(), DEFAULT_DECK_ID);
    deck_json.insert(
        DEFAULT_DECK_ID.to_string(),
        json!({
            "id": DEFAULT_DECK_ID,
            "name": "Default",
            "conf": 1,
            "mod": Utc::now().timestamp(),
            "usn": 0,
            "dyn": 0,
            "desc": "",
        }),
    );
    report.decks += 1;

    let mut next_deck_id = DECK_ID_BASE;
    for (engram_id, name) in &data.decks {
        if name == "Default" {
            deck_anki.insert(engram_id.clone(), DEFAULT_DECK_ID);
            continue;
        }
        let anki_id = anki_ids
            .get(&("deck".to_string(), engram_id.clone()))
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| {
                let id = next_deck_id;
                next_deck_id += 10;
                id
            });
        deck_anki.insert(engram_id.clone(), anki_id);
        deck_json.insert(
            anki_id.to_string(),
            json!({
                "id": anki_id,
                "name": name,
                "conf": 1,
                "mod": Utc::now().timestamp(),
                "usn": 0,
                "dyn": 0,
                "desc": "",
            }),
        );
        report.decks += 1;
    }

    // --- field count / templates ---
    let max_fields = data
        .notes
        .iter()
        .map(|n| n.fields.len())
        .max()
        .unwrap_or(2)
        .max(1);
    let max_ord = data.cards.iter().map(|c| c.template_idx).max().unwrap_or(0);
    let template_count = (max_ord as usize + 1).max(1);

    let flds: Vec<Value> = (0..max_fields)
        .map(|i| {
            json!({
                "name": format!("Field{}", i + 1),
                "ord": i as i64,
                "sticky": false,
                "rtl": false,
                "font": "Arial",
                "size": 20,
            })
        })
        .collect();
    let tmpls: Vec<Value> = (0..template_count)
        .map(|i| {
            let name = if template_count == 1 {
                "Card 1".to_string()
            } else {
                format!("Card {}", i + 1)
            };
            let front_field = ((i % max_fields) + 1).max(1);
            json!({
                "name": name,
                "ord": i as i64,
                "qfmt": format!("{{{{Field{front_field}}}}}"),
                "afmt": "{{FrontSide}}<hr id=answer>{{Field2}}",
                "bqfmt": "",
                "bafmt": "",
                "did": Value::Null,
            })
        })
        .collect();

    let models_json = json!({
        MODEL_ID.to_string(): {
            "id": MODEL_ID,
            "name": "Basic",
            "type": 0,
            "mod": Utc::now().timestamp(),
            "usn": 0,
            "sortf": 0,
            "did": DEFAULT_DECK_ID,
            "tmpls": tmpls,
            "flds": flds,
            "css": ".card { font-family: arial; font-size: 20px; text-align: center; color: black; background-color: white; }",
            "latexPre": "",
            "latexPost": "",
            "req": [[0, "any", [0]]],
        }
    });

    // --- notes ---
    let mut note_anki: HashMap<String, i64> = HashMap::new();
    let mut note_rows: Vec<(i64, String, i64, String, String, String)> = Vec::new();
    let mut next_note_id = NOTE_ID_BASE;
    for note in &data.notes {
        let anki_id = anki_ids
            .get(&("note".to_string(), note.id.clone()))
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| {
                let id = next_note_id;
                next_note_id += 1;
                id
            });
        note_anki.insert(note.id.clone(), anki_id);

        let mut fields = note.fields.clone();
        fields.resize(max_fields, String::new());
        let flds_str = fields.join(&FLD_SEP.to_string());
        let sfld = fields.first().cloned().unwrap_or_default();
        let tags = if note.tags.is_empty() {
            String::new()
        } else {
            format!(" {} ", note.tags.join(" "))
        };
        let guid = Uuid::new_v4().simple().to_string();
        note_rows.push((
            anki_id,
            guid,
            MODEL_ID,
            tags,
            flds_str,
            sfld,
        ));
        report.notes += 1;
    }

    // --- cards ---
    let mut card_rows: Vec<(i64, i64, i64, i64)> = Vec::new();
    let mut next_card_id = CARD_ID_BASE;
    for card in &data.cards {
        let Some(&nid) = note_anki.get(&card.note_id) else {
            report.warnings.push(format!(
                "skipped card {}: note {} missing from export set",
                card.id, card.note_id
            ));
            continue;
        };
        let did = deck_anki
            .get(&card.deck_id)
            .copied()
            .unwrap_or(DEFAULT_DECK_ID);
        let anki_id = anki_ids
            .get(&("card".to_string(), card.id.clone()))
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| {
                let id = next_card_id;
                next_card_id += 1;
                id
            });
        card_rows.push((anki_id, nid, did, card.template_idx as i64));
        report.cards += 1;
    }

    // --- build SQLite ---
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    {
        let conn = Connection::open(&db_path)?;
        create_v11_schema(&conn)?;
        let now_ms = Utc::now().timestamp_millis();
        let now_s = Utc::now().timestamp();
        let conf = json!({
            "nextPos": 1,
            "estTimes": true,
            "activeDecks": [1],
            "sortType": "noteFld",
            "timeLim": 0,
            "sortBackwards": false,
            "addToCur": true,
            "curDeck": 1,
            "newBury": true,
            "newSpread": 0,
            "dueCounts": true,
            "curModel": MODEL_ID.to_string(),
            "collapseTime": 1200,
        });
        let dconf = json!({
            "1": {
                "id": 1,
                "name": "Default",
                "mod": now_s,
                "usn": 0,
                "maxTaken": 60,
                "autoplay": true,
                "timer": 0,
                "replayq": true,
                "new": {
                    "bury": true,
                    "delays": [1, 10],
                    "initialFactor": 2500,
                    "ints": [1, 4, 0],
                    "order": 1,
                    "perDay": 20,
                },
                "rev": {
                    "bury": true,
                    "ease4": 1.3,
                    "fuzz": 0.05,
                    "ivlFct": 1,
                    "maxIvl": 36500,
                    "minSpace": 1,
                    "perDay": 200,
                },
                "lapse": {
                    "delays": [10],
                    "leechAction": 1,
                    "leechFails": 8,
                    "minInt": 1,
                    "mult": 0,
                },
                "dyn": false,
            }
        });
        conn.execute(
            "INSERT INTO col VALUES (1, ?1, ?2, ?2, 11, 0, 0, 0, ?3, ?4, ?5, ?6, '{}')",
            rusqlite::params![
                now_s,
                now_ms,
                conf.to_string(),
                models_json.to_string(),
                Value::Object(deck_json.clone()).to_string(),
                dconf.to_string(),
            ],
        )?;

        {
            let mut stmt = conn.prepare(
                "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, 0, 0, '')",
            )?;
            for (id, guid, mid, tags, flds_str, sfld) in &note_rows {
                stmt.execute(rusqlite::params![*id, guid, *mid, now_s, tags, flds_str, sfld])?;
            }
        }
        {
            let mut stmt = conn.prepare(
                "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, \"left\", odue, odid, flags, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, ?6, 0, 0, 0, 0, 0, 0, 0, 0, '')",
            )?;
            let mut due = 1i64;
            for (id, nid, did, ord) in &card_rows {
                stmt.execute(rusqlite::params![*id, *nid, *did, *ord, now_s, due])?;
                due += 1;
            }
        }
    }

    let db_bytes = std::fs::read(&db_path)?;

    // --- media ---
    let media_dir = col
        .path()
        .parent()
        .unwrap_or(Path::new("."))
        .join("media");
    let mut media_entries: Vec<(String, String, Vec<u8>)> = Vec::new();
    if media_dir.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        collect_media_files(&media_dir, &media_dir, &mut files, &mut report.warnings)?;
        files.sort();
        for (idx, file) in files.iter().enumerate() {
            let rel = file
                .strip_prefix(&media_dir)
                .unwrap_or(file.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            let safe = sanitize_media_name(&rel);
            let bytes = std::fs::read(file)?;
            media_entries.push((idx.to_string(), safe, bytes));
        }
    }
    report.media_exported = media_entries.len() as u32;

    // --- zip ---
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::File::create(out_path)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("collection.anki2", opts)?;
    zip.write_all(&db_bytes)?;

    let media_map: Map<String, Value> = media_entries
        .iter()
        .map(|(zip_name, original, _)| (zip_name.clone(), Value::String(original.clone())))
        .collect();
    zip.start_file("media", opts)?;
    zip.write_all(Value::Object(media_map).to_string().as_bytes())?;

    for (zip_name, _original, bytes) in &media_entries {
        zip.start_file(zip_name.as_str(), opts)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;

    tracing::debug!(
        out = %out_path.display(),
        decks = report.decks,
        notes = report.notes,
        cards = report.cards,
        media = report.media_exported,
        "exported apkg"
    );
    Ok(report)
}

fn create_v11_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE col (
          id integer PRIMARY KEY,
          crt integer NOT NULL,
          mod integer NOT NULL,
          scm integer NOT NULL,
          ver integer NOT NULL,
          dty integer NOT NULL,
          usn integer NOT NULL,
          ls integer NOT NULL,
          conf text NOT NULL,
          models text NOT NULL,
          decks text NOT NULL,
          dconf text NOT NULL,
          tags text NOT NULL
        );
        CREATE TABLE notes (
          id integer PRIMARY KEY,
          guid text NOT NULL,
          mid integer NOT NULL,
          mod integer NOT NULL,
          usn integer NOT NULL,
          tags text NOT NULL,
          flds text NOT NULL,
          sfld text NOT NULL,
          csum integer NOT NULL,
          flags integer NOT NULL,
          data text NOT NULL
        );
        CREATE TABLE cards (
          id integer PRIMARY KEY,
          nid integer NOT NULL,
          did integer NOT NULL,
          ord integer NOT NULL,
          mod integer NOT NULL,
          usn integer NOT NULL,
          type integer NOT NULL,
          queue integer NOT NULL,
          due integer NOT NULL,
          ivl integer NOT NULL,
          factor integer NOT NULL,
          reps integer NOT NULL,
          lapses integer NOT NULL,
          "left" integer NOT NULL,
          odue integer NOT NULL,
          odid integer NOT NULL,
          flags integer NOT NULL,
          data text NOT NULL
        );
        CREATE TABLE revlog (
          id integer PRIMARY KEY,
          cid integer NOT NULL,
          usn integer NOT NULL,
          ease integer NOT NULL,
          ivl integer NOT NULL,
          lastIvl integer NOT NULL,
          factor integer NOT NULL,
          time integer NOT NULL,
          type integer NOT NULL
        );
        CREATE TABLE graves (
          usn integer NOT NULL,
          oid integer NOT NULL,
          type integer NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn load_export_data(col: &Collection, deck_filter: Option<&str>) -> anyhow::Result<ExportData> {
    let decks_all = col.list_decks()?;
    let filter_id = match deck_filter {
        Some(name) => {
            let d = decks_all
                .iter()
                .find(|d| d.name == name)
                .ok_or_else(|| anyhow::anyhow!("deck not found: {name}"))?;
            Some(d.id.to_string())
        }
        None => None,
    };

    let mut decks: Vec<(String, String)> = decks_all
        .iter()
        .map(|d| (d.id.to_string(), d.name.clone()))
        .collect();

    // Read notes/cards from the collection SQLite (no full-list API on Collection).
    let src = Connection::open_with_flags(
        col.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    let mut notes: Vec<ExportNote> = Vec::new();
    {
        let mut stmt = src.prepare("SELECT id, deck_id, fields_json, tags_json FROM notes")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, deck_id, fields_json, tags_json) = row?;
            let fields: Vec<String> = serde_json::from_str(&fields_json).unwrap_or_default();
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            notes.push(ExportNote {
                id,
                deck_id,
                fields,
                tags,
            });
        }
    }

    let mut cards: Vec<ExportCard> = Vec::new();
    {
        let mut stmt =
            src.prepare("SELECT id, note_id, deck_id, template_idx FROM cards")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (id, note_id, deck_id, template_idx) = row?;
            cards.push(ExportCard {
                id,
                note_id,
                deck_id,
                template_idx: template_idx.max(0) as u32,
            });
        }
    }

    if let Some(fid) = filter_id {
        let mut keep_notes: HashSet<String> = notes
            .iter()
            .filter(|n| n.deck_id == fid)
            .map(|n| n.id.clone())
            .collect();
        for c in &cards {
            if c.deck_id == fid {
                keep_notes.insert(c.note_id.clone());
            }
        }
        notes.retain(|n| keep_notes.contains(&n.id));
        cards.retain(|c| keep_notes.contains(&c.note_id));

        let used: HashSet<String> = notes
            .iter()
            .map(|n| n.deck_id.clone())
            .chain(cards.iter().map(|c| c.deck_id.clone()))
            .collect();
        decks.retain(|(id, _)| used.contains(id));
        if !decks.iter().any(|(_, n)| n == "Default") {
            if let Some(def) = decks_all.into_iter().find(|d| d.name == "Default") {
                decks.push((def.id.to_string(), def.name));
            }
        }
    }

    Ok(ExportData {
        decks,
        notes,
        cards,
    })
}

/// Reverse of import's `put_anki_id`: (kind, engram_id) -> anki_id.
fn load_anki_id_map(col: &Collection) -> anyhow::Result<HashMap<(String, String), String>> {
    let src = Connection::open_with_flags(
        col.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut out = HashMap::new();
    let table_exists = src
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='anki_id_map'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !table_exists {
        return Ok(out);
    }
    let mut stmt = src.prepare("SELECT kind, anki_id, engram_id FROM anki_id_map")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, anki_id, engram_id) = row?;
        out.insert((kind, engram_id), anki_id);
    }
    Ok(out)
}

fn collect_media_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    let _ = root;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(format!("cannot read media dir {}: {e}", dir.display()));
            return Ok(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_media_files(root, &path, out, warnings)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// Keep only a safe relative name (no path traversal, no separators).
fn sanitize_media_name(name: &str) -> String {
    let cleaned = name.replace(['/', '\\'], "_").replace("..", "_");
    if cleaned.is_empty() || cleaned == "." {
        "media.bin".to_string()
    } else {
        cleaned
    }
}
