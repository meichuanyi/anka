use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use anka_core::{Collection, Id};
use rusqlite::types::ValueRef;
use serde_json::Value;

use chrono::{TimeZone, Utc};

#[derive(Debug, Default, Clone)]
pub struct ImportReport {
    pub decks: u32,
    pub notes: u32,
    pub cards: u32,
    pub revlogs: u32,
    pub media_copied: u32,
    pub skipped_cards: u32,
    pub warnings: Vec<String>,
}

/// Import an Anki `.apkg` into `col`.
///
/// Supports:
/// - `collection.anki21` / `collection.anki2` (uncompressed SQLite, V11)
/// - `collection.anki21b` (zstd-compressed SQLite, V11 or V18 schema)
///
/// V18 notes/cards still use `\x1f`-separated `notes.flds`. Deck names come
/// from the split `decks` table (`name` uses `\x1f` as hierarchy separator)
/// or from residual `col.decks` JSON. Notetype field names come from
/// `notetypes`/`fields` or residual `col.models` JSON.
///
/// Media: V11 JSON `{"0":"file.png"}` maps are applied. V18 protobuf/zstd
/// media maps are skipped with a warning (no panic).
pub fn import_apkg(path: impl AsRef<Path>, col: &mut Collection) -> anyhow::Result<ImportReport> {
    let path = path.as_ref();
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let collection_bytes = read_collection_bytes(&mut zip)?;
    let mut report = ImportReport::default();
    if collection_bytes.is_empty() {
        anyhow::bail!("apkg has no collection database: {}", path.display());
    }

    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::write(tmp.path(), &collection_bytes)?;
    let src = rusqlite::Connection::open(tmp.path())?;

    let schema_ver = schema_version(&src).unwrap_or(11);
    if schema_ver > 18 {
        report.warnings.push(format!(
            "collection schema ver={schema_ver} is newer than 18; import is best-effort"
        ));
    }

    // Deck id (anki) -> name
    let deck_names = load_deck_names(&src, &mut report.warnings)?;
    // model id -> list of field names (optional, for future templates)
    let _models = load_models(&src, &mut report.warnings)?;

    // Map anki deck id -> anka deck id
    let mut deck_map: HashMap<String, Id> = HashMap::new();
    for (anki_id, name) in &deck_names {
        let deck = col.ensure_deck(name)?;
        deck_map.insert(anki_id.clone(), deck.id);
        col.put_anki_id("deck", anki_id, deck.id)?;
        report.decks += 1;
    }

    let default_deck = col.ensure_deck("Default")?;

    // notes
    let mut note_rows: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    {
        let mut stmt = src.prepare("SELECT id, flds, tags FROM notes")?;
        let rows = stmt.query_map([], |r| {
            let id: i64 = r.get(0)?;
            let flds: String = r.get(1)?;
            let tags: String = r.get(2)?;
            Ok((id, flds, tags))
        })?;
        for row in rows {
            let (id, flds, tags) = row?;
            let fields: Vec<String> = flds.split('\x1f').map(|s| s.to_string()).collect();
            let tag_list: Vec<String> = tags
                .split_whitespace()
                .filter(|t| !t.is_empty())
                .map(|t| t.to_string())
                .collect();
            note_rows.push((id.to_string(), fields, tag_list));
        }
    }

    // cards: (card_id, note_id, deck_id, template_ord)
    let mut cards: Vec<(String, String, String, u32)> = Vec::new();
    {
        let mut stmt = src.prepare("SELECT id, nid, did, ord FROM cards")?;
        let rows = stmt.query_map([], |r| {
            let id: i64 = r.get(0)?;
            let nid: i64 = r.get(1)?;
            let did: i64 = r.get(2)?;
            let ord: i64 = r.get(3)?;
            Ok((id, nid, did, ord))
        })?;
        for row in rows {
            let (id, nid, did, ord) = row?;
            cards.push((id.to_string(), nid.to_string(), did.to_string(), ord as u32));
        }
    }
    tracing::debug!(
        notes = note_rows.len(),
        cards = cards.len(),
        "loaded anki notes/cards"
    );
    if !cards.is_empty() && !note_rows.is_empty() {
        let per_note = cards.len() as f64 / note_rows.len() as f64;
        if per_note > 1.5 {
            report.warnings.push(format!(
                "multi-template deck detected (~{per_note:.1} cards/note); importing all cards"
            ));
        }
    }

    // Group card indices by note.
    let mut cards_by_note: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (_cid, nid, _did, _ord)) in cards.iter().enumerate() {
        cards_by_note.entry(nid.clone()).or_default().push(i);
    }

    col.begin().map_err(|e| anyhow::anyhow!("{e}"))?;
    let import_result: anyhow::Result<()> = (|| {
        for (anki_note_id, fields, tags) in note_rows {
            let card_idxs = cards_by_note.get(&anki_note_id).cloned().unwrap_or_default();
            // Place the note in the first related deck (or Default).
            let primary_anki_deck = card_idxs
                .first()
                .map(|i| cards[*i].2.clone())
                .unwrap_or_else(|| "1".into());
            let note_deck = deck_map
                .get(&primary_anki_deck)
                .copied()
                .unwrap_or(default_deck.id);

            let note = col.add_note_only(note_deck, "basic".into(), fields, tags)?;
            col.put_anki_id("note", &anki_note_id, note.id)?;
            report.notes += 1;

            if card_idxs.is_empty() {
                // Orphan note: keep a card so it remains studyable.
                let _card = col.add_card_for_note(note.id, note_deck, 0)?;
                report.cards += 1;
                continue;
            }

            for i in card_idxs {
                let (cid, _nid, anki_did, ord) = &cards[i];
                let deck_id = deck_map.get(anki_did).copied().unwrap_or(default_deck.id);
                let card = col.add_card_for_note(note.id, deck_id, *ord)?;
                col.put_anki_id("card", cid, card.id)?;
                report.cards += 1;
            }
        }
        Ok(())
    })();
    match import_result {
        Ok(()) => {
            col.commit().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Err(e) => {
            let _ = col.rollback();
            return Err(e);
        }
    }

    // Review history (optional; many share packages ship empty revlog).
    report.revlogs = import_revlog(&src, col, &mut report.warnings)?;

    // Media map + payloads (best-effort)
    report.media_copied = import_media(&mut zip, col, &mut report.warnings)?;

    Ok(report)
}

/// Import Anki `revlog` rows into Anka revlog.
///
/// Anki columns: id(ms), cid, usn, ease(1-4), ivl, lastIvl, factor, time(ms), type.
/// ease 0 = manual reschedule — skipped.
fn import_revlog(
    src: &rusqlite::Connection,
    col: &mut Collection,
    warnings: &mut Vec<String>,
) -> anyhow::Result<u32> {
    if !table_exists(src, "revlog") {
        return Ok(0);
    }
    let mut stmt =
        src.prepare("SELECT id, cid, ease, ivl, lastIvl, time FROM revlog ORDER BY id ASC")?;
    let rows = stmt.query_map([], |r| {
        let id: i64 = r.get(0)?;
        let cid: i64 = r.get(1)?;
        let ease: i64 = r.get(2)?;
        let ivl: i64 = r.get(3)?;
        let last_ivl: i64 = r.get(4)?;
        let time_ms: i64 = r.get(5)?;
        Ok((id, cid, ease, ivl, last_ivl, time_ms))
    })?;

    let mut imported = 0u32;
    let mut skipped_unmapped = 0u32;
    let mut skipped_ease = 0u32;

    col.begin().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut result: anyhow::Result<u32> = Ok(0);
    for row in rows {
        let (id_ms, cid, ease, ivl, _last_ivl, time_ms) = match row {
            Ok(v) => v,
            Err(e) => {
                result = Err(e.into());
                break;
            }
        };
        let Some(rating) = anka_core::Rating::from_u8(ease.clamp(1, 4) as u8) else {
            skipped_ease += 1;
            continue;
        };
        if ease == 0 {
            skipped_ease += 1;
            continue;
        }
        let anki_card = cid.to_string();
        let Some(card_id) = col.anka_id_for_anki("card", &anki_card)? else {
            skipped_unmapped += 1;
            continue;
        };
        let reviewed_at = anki_ms_to_utc(id_ms);
        let interval_days = if ivl > 0 {
            ivl as f32
        } else if ivl < 0 {
            (-(ivl as f32)) / 86_400.0
        } else {
            0.0
        };
        let entry = anka_core::RevlogEntry {
            id: anka_core::Id::new(),
            card_id,
            rating,
            reviewed_at,
            elapsed_ms: time_ms.clamp(0, i64::from(u32::MAX)) as u32,
            // Anki does not store FSRS S/D in classic revlog; leave zeros —
            // optimizer only needs rating + delta_t.
            stability_after: 0.0,
            difficulty_after: 0.0,
            interval_days,
        };
        if let Err(e) = col.add_revlog(entry) {
            result = Err(e.into());
            break;
        }
        imported += 1;
    }
    match result {
        Ok(_) => {
            col.commit().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Err(e) => {
            let _ = col.rollback();
            return Err(e);
        }
    }

    if skipped_ease > 0 {
        warnings.push(format!("revlog: skipped {skipped_ease} manual/ease-0 entries"));
    }
    if skipped_unmapped > 0 {
        warnings.push(format!(
            "revlog: skipped {skipped_unmapped} entries with unmapped card ids"
        ));
    }
    Ok(imported)
}

fn anki_ms_to_utc(ms: i64) -> chrono::DateTime<chrono::Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
}

fn read_collection_bytes(zip: &mut zip::ZipArchive<std::fs::File>) -> anyhow::Result<Vec<u8>> {
    // Prefer latest compressed form.
    if zip.file_names().any(|n| n == "collection.anki21b") {
        let mut f = zip.by_name("collection.anki21b")?;
        let mut compressed = Vec::new();
        f.read_to_end(&mut compressed)?;
        let bytes = zstd::decode_all(Cursor::new(compressed))?;
        return Ok(bytes);
    }
    for name in ["collection.anki21", "collection.anki2"] {
        if zip.file_names().any(|n| n == name) {
            let mut f = zip.by_name(name)?;
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
    }
    Ok(Vec::new())
}

fn schema_version(conn: &rusqlite::Connection) -> Option<i64> {
    conn.query_row("SELECT ver FROM col LIMIT 1", [], |r| r.get(0))
        .ok()
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// Anki V18 stores deck names with `\x1f` as the hierarchy separator.
/// Human-readable form uses `::`.
fn machine_name_to_human(name: &str) -> String {
    name.replace('\x1f', "::")
}

fn value_to_string(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Text(s) => String::from_utf8_lossy(s).into_owned(),
        ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
    }
}

fn load_deck_names(
    conn: &rusqlite::Connection,
    warnings: &mut Vec<String>,
) -> anyhow::Result<Vec<(String, String)>> {
    // V11 (and residual after incomplete migration): col.decks is a JSON object
    // `{id: {name, ...}}` with human-readable `::` names.
    if let Ok(json) = conn.query_row("SELECT decks FROM col LIMIT 1", [], |r| {
        r.get::<_, String>(0)
    }) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&json) {
            if !map.is_empty() {
                return Ok(deck_names_from_col_json(&json));
            }
        }
    }

    // V15+/V18: decks table. `name` is TEXT using `\x1f` separators.
    // `common`/`kind` are protobuf blobs and are ignored here.
    let mut out = Vec::new();
    if table_exists(conn, "decks") {
        let mut stmt = conn.prepare("SELECT id, name FROM decks")?;
        let rows = stmt.query_map([], |r| {
            let id: i64 = r.get(0)?;
            let name = value_to_string(r.get_ref(1)?);
            Ok((id, name))
        })?;
        for row in rows.flatten() {
            let human = machine_name_to_human(&row.1);
            if human.is_empty() {
                warnings.push(format!("deck {} has empty name; using Default", row.0));
                out.push((row.0.to_string(), "Default".into()));
            } else {
                out.push((row.0.to_string(), human));
            }
        }
    }

    if out.is_empty() {
        out.push(("1".into(), "Default".into()));
    }
    Ok(out)
}

fn deck_names_from_col_json(json: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else {
        out.push(("1".into(), "Default".into()));
        return out;
    };
    for (id, deck) in map {
        let name = deck
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Default")
            .to_string();
        out.push((id, name));
    }
    if out.is_empty() {
        out.push(("1".into(), "Default".into()));
    }
    out
}

fn load_models(
    conn: &rusqlite::Connection,
    warnings: &mut Vec<String>,
) -> anyhow::Result<HashMap<String, Vec<String>>> {
    // V11 / residual: col.models JSON
    if let Ok(json) = conn.query_row("SELECT models FROM col LIMIT 1", [], |r| {
        r.get::<_, String>(0)
    }) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&json) {
            if !map.is_empty() {
                let mut out = HashMap::new();
                for (mid, model) in map {
                    let fields = model
                        .get("flds")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|f| {
                                    f.get("name").and_then(|n| n.as_str()).map(String::from)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    out.insert(mid, fields);
                }
                return Ok(out);
            }
        }
    }

    // V15+/V18: notetypes + fields tables.
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if table_exists(conn, "notetypes") {
        let mut stmt = conn.prepare("SELECT id, name FROM notetypes")?;
        let rows = stmt.query_map([], |r| {
            let id: i64 = r.get(0)?;
            let name = value_to_string(r.get_ref(1)?);
            Ok((id, name))
        })?;
        for row in rows.flatten() {
            out.insert(row.0.to_string(), Vec::new());
        }
        if table_exists(conn, "fields") {
            let mut fstmt = conn.prepare("SELECT ntid, ord, name FROM fields ORDER BY ntid, ord")?;
            let frows = fstmt.query_map([], |r| {
                let ntid: i64 = r.get(0)?;
                let name = value_to_string(r.get_ref(2)?);
                Ok((ntid, name))
            })?;
            for row in frows.flatten() {
                out.entry(row.0.to_string()).or_default().push(row.1);
            }
        }
    }

    if out.is_empty() {
        warnings.push("no notetype/field metadata found; notes imported as basic".into());
    }
    Ok(out)
}

fn import_media(
    zip: &mut zip::ZipArchive<std::fs::File>,
    col: &mut Collection,
    warnings: &mut Vec<String>,
) -> anyhow::Result<u32> {
    let mut raw = Vec::new();
    {
        let Ok(mut f) = zip.by_name("media") else {
            return Ok(0);
        };
        f.read_to_end(&mut raw)?;
    }
    if raw.is_empty() {
        return Ok(0);
    }

    // V11: JSON map `{"0":"filename.png", ...}`.
    // V18: zstd-compressed protobuf MediaEntries — not applied in M0.
    let text = String::from_utf8_lossy(&raw);
    let mut map_value = serde_json::from_str::<Value>(&text).ok();

    // Some writers zstd-compress even a JSON map; try that before giving up.
    if map_value.is_none() {
        if let Ok(decoded) = zstd::decode_all(Cursor::new(&raw)) {
            let decoded_text = String::from_utf8_lossy(&decoded);
            map_value = serde_json::from_str::<Value>(&decoded_text).ok();
        }
    }

    let Some(Value::Object(map)) = map_value else {
        warnings.push(
            "media map is not JSON (likely V18 zstd/protobuf); skipped media files".into(),
        );
        return Ok(0);
    };

    let media_dir = col.path().parent().unwrap_or(Path::new(".")).join("media");
    std::fs::create_dir_all(&media_dir)?;
    let names: Vec<(String, String)> = map
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .collect();

    let mut copied = 0u32;
    for (index, name) in names {
        let mut bytes = Vec::new();
        {
            let mut mf = match zip.by_name(&index) {
                Ok(f) => f,
                Err(_) => {
                    warnings.push(format!("missing media payload {index} for {name}"));
                    continue;
                }
            };
            mf.read_to_end(&mut bytes)?;
        }
        // V18 media payloads may themselves be zstd-compressed; if the result
        // does not look like the original raw bytes we still write what we have.
        let safe = name.replace(['/', '\\'], "_");
        std::fs::write(media_dir.join(&safe), bytes)?;
        copied += 1;
    }
    Ok(copied)
}
