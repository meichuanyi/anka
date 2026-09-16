//! Integration tests: import → export → re-import (V11 Legacy1).

use std::io::Write;
use std::path::{Path, PathBuf};

use engram_apkg::{export_apkg, import_apkg, ExportOptions, ExportReport, ImportReport};
use engram_core::Collection;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FLD_SEP: char = '\x1f';

fn write_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

/// Build a kaoyan-style V11 package: multi-field vocab notes, 2 decks,
/// one multi-template note (ord 0 + 1).
fn make_kaoyan_style_apkg(dir: &Path) -> PathBuf {
    let db_path = dir.join("src.anki2");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
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
          sfld integer NOT NULL,
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
        "#,
    )
    .unwrap();

    let decks_json = serde_json::json!({
        "1": {"id": 1i64, "name": "Default", "conf": 1i64},
        "1500000000010": {"id": 1500000000010i64, "name": "考研词汇::核心", "conf": 1i64},
        "1500000000020": {"id": 1500000000020i64, "name": "考研词汇::真题", "conf": 1i64},
    })
    .to_string();
    // 5-field model like typical vocab decks.
    let models_json = serde_json::json!({
        "1600000000000": {
            "id": 1600000000000i64,
            "name": "Basic",
            "flds": [
                {"name": "Word"},
                {"name": "Phonetic"},
                {"name": "Meaning"},
                {"name": "Example"},
                {"name": "Notes"},
            ],
            "tmpls": [
                {"name": "Card 1", "ord": 0},
                {"name": "Card 2", "ord": 1},
            ],
        }
    })
    .to_string();

    conn.execute(
        "INSERT INTO col VALUES (1,0,0,0,11,0,0,0,'{}',?1,?2,'{}','{}')",
        rusqlite::params![models_json, decks_json],
    )
    .unwrap();

    // 5 vocab notes. Fields: word / phonetic / meaning / example / notes
    let words = [
        ("abandon", "/əˈbændən/", "放弃；遗弃", "He abandoned the project.", "考研高频"),
        ("benefit", "/ˈbenɪfɪt/", "利益；好处", "Exercise benefits health.", ""),
        ("crucial", "/ˈkruːʃl/", "至关重要的", "Timing is crucial.", "真题常考"),
        ("dilemma", "/dɪˈlemə/", "困境；两难", "face a moral dilemma", ""),
        ("elaborate", "/ɪˈlæbərət/", "精心制作的；详尽的", "an elaborate plan", "多义词"),
    ];
    for (i, (w, ph, mean, ex, notes)) in words.iter().enumerate() {
        let nid = 1500000000001i64 + i as i64;
        let flds = [*w, *ph, *mean, *ex, *notes].join(&FLD_SEP.to_string());
        let tags = if i % 2 == 0 {
            " vocab kaoyan "
        } else {
            " vocab "
        };
        conn.execute(
            "INSERT INTO notes VALUES (?1,?2,1600000000000,0,0,?3,?4,0,0,0,'')",
            rusqlite::params![nid, format!("guid-{i}"), tags, flds],
        )
        .unwrap();
        // Card 0 always; card 1 only for first two notes (multi-template).
        let did = if i < 3 { 1500000000010i64 } else { 1500000000020i64 };
        let cid0 = 1500000000020i64 + (i as i64) * 2;
        conn.execute(
            "INSERT INTO cards VALUES (?1,?2,?3,0,0,0,0,0,?4,0,0,0,0,0,0,0,0,'')",
            rusqlite::params![cid0, nid, did, i as i64 + 1],
        )
        .unwrap();
        if i < 2 {
            let cid1 = cid0 + 1;
            conn.execute(
                "INSERT INTO cards VALUES (?1,?2,?3,1,0,0,0,0,?4,0,0,0,0,0,0,0,0,'')",
                rusqlite::params![cid1, nid, did, 100 + i as i64],
            )
            .unwrap();
        }
    }

    let bytes = std::fs::read(&db_path).unwrap();
    let apkg = dir.join("kaoyan-style.apkg");
    let media_map = serde_json::json!({"0": "audio/abandon.mp3", "1": "img.png"}).to_string();
    write_zip(
        &apkg,
        &[
            ("collection.anki2", bytes),
            ("media", media_map.into_bytes()),
            ("0", b"FAKE_MP3".to_vec()),
            ("1", b"FAKE_PNG".to_vec()),
        ],
    );
    apkg
}

fn import_into(dir: &Path, name: &str, apkg: &Path) -> (PathBuf, ImportReport) {
    let col_path = dir.join(name);
    let mut col = Collection::create(&col_path).unwrap();
    let report = import_apkg(apkg, &mut col).unwrap();
    (col_path, report)
}

fn open_col(path: &Path) -> Collection {
    Collection::open(path).unwrap()
}

fn deck_names(col: &Collection) -> Vec<String> {
    col.list_decks()
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect()
}

#[test]
fn export_roundtrip_preserves_counts() {
    let dir = TempDir::new().unwrap();
    let src_apkg = make_kaoyan_style_apkg(dir.path());

    // 1) import source package
    let (col_a_path, imp1) = import_into(dir.path(), "col_a.egdb", &src_apkg);
    assert_eq!(imp1.notes, 5, "warnings: {:?}", imp1.warnings);
    // 5 notes: 2 with two cards, 3 with one → 7 cards
    assert_eq!(imp1.cards, 7, "warnings: {:?}", imp1.warnings);
    assert!(imp1.decks >= 3, "Default + two hierarchical, got {}", imp1.decks);
    assert_eq!(imp1.media_copied, 2);

    let col_a = open_col(&col_a_path);
    let names_a = deck_names(&col_a);
    assert!(names_a.iter().any(|n| n == "考研词汇::核心"), "{names_a:?}");
    assert!(names_a.iter().any(|n| n == "考研词汇::真题"), "{names_a:?}");

    // 2) export
    let out_apkg = dir.path().join("roundtrip.apkg");
    let exp: ExportReport = export_apkg(&col_a, &out_apkg, ExportOptions::default()).unwrap();
    assert_eq!(exp.notes, 5, "warnings: {:?}", exp.warnings);
    assert_eq!(exp.cards, 7, "warnings: {:?}", exp.warnings);
    assert!(exp.decks >= 3, "export decks={}", exp.decks);
    assert_eq!(exp.media_exported, 2, "warnings: {:?}", exp.warnings);

    // 3) re-import into a fresh collection
    let (col_b_path, imp2) = import_into(dir.path(), "col_b.egdb", &out_apkg);
    assert_eq!(imp2.notes, exp.notes, "warnings: {:?}", imp2.warnings);
    assert_eq!(imp2.cards, exp.cards, "warnings: {:?}", imp2.warnings);
    assert_eq!(imp2.media_copied, exp.media_exported);
    // Hierarchical names survive as ::
    let col_b = open_col(&col_b_path);
    let names_b = deck_names(&col_b);
    assert!(names_b.iter().any(|n| n == "考研词汇::核心"), "{names_b:?}");
    assert!(names_b.iter().any(|n| n == "考研词汇::真题"), "{names_b:?}");

    // Spot-check a note's fields after round-trip.
    let (_total, notes) = col_b.search_notes("abandon", 10, 0).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].fields[0], "abandon");
    assert_eq!(notes[0].fields[2], "放弃；遗弃");

    // Multi-template: first two notes should still have 2 cards each after reimport.
    // Total cards already asserted; check one note's card count via SQLite.
    let src = rusqlite::Connection::open(&col_b_path).unwrap();
    let abandon_cards: i64 = src
        .query_row(
            "SELECT COUNT(*) FROM cards c JOIN notes n ON c.note_id = n.id
             WHERE n.fields_json LIKE '%abandon%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(abandon_cards, 2, "multi-template cards should survive");

    // Media files land next to collection B.
    assert!(dir.path().join("media").join("abandon.mp3").exists()
        || dir.path().join("media").join("audio_abandon.mp3").exists()
        || dir.path().join("media").join("img.png").exists());
}

#[test]
fn export_deck_filter_limits_scope() {
    let dir = TempDir::new().unwrap();
    let src_apkg = make_kaoyan_style_apkg(dir.path());
    let (col_a_path, _) = import_into(dir.path(), "col_a.egdb", &src_apkg);
    let col_a = open_col(&col_a_path);

    let out = dir.path().join("filter.apkg");
    let exp = export_apkg(
        &col_a,
        &out,
        ExportOptions {
            deck: Some("考研词汇::真题".into()),
        },
    )
    .unwrap();
    // 真题 has notes 4,5 (0-indexed 3,4) → 2 notes, 2 cards
    assert_eq!(exp.notes, 2, "warnings: {:?}", exp.warnings);
    assert_eq!(exp.cards, 2, "warnings: {:?}", exp.warnings);

    let (col_b_path, imp) = import_into(dir.path(), "col_b.egdb", &out);
    assert_eq!(imp.notes, 2);
    assert_eq!(imp.cards, 2);
    let col_b = open_col(&col_b_path);
    let names = deck_names(&col_b);
    assert!(names.iter().any(|n| n == "考研词汇::真题"), "{names:?}");
}

#[test]
fn export_missing_deck_is_error() {
    let dir = TempDir::new().unwrap();
    let src_apkg = make_kaoyan_style_apkg(dir.path());
    let (col_a_path, _) = import_into(dir.path(), "col_a.egdb", &src_apkg);
    let col_a = open_col(&col_a_path);
    let err = export_apkg(
        &col_a,
        dir.path().join("x.apkg"),
        ExportOptions {
            deck: Some("nope".into()),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("deck not found"), "{err}");
}

#[test]
fn export_empty_collection_writes_valid_v11() {
    let dir = TempDir::new().unwrap();
    let col_path = dir.path().join("empty.egdb");
    let col = Collection::create(&col_path).unwrap();
    let out = dir.path().join("empty.apkg");
    let exp = export_apkg(&col, &out, ExportOptions::default()).unwrap();
    assert_eq!(exp.notes, 0);
    assert_eq!(exp.cards, 0);
    assert!(exp.decks >= 1);

    // Must be importable as V11.
    let (col_b_path, imp) = import_into(dir.path(), "col_b.egdb", &out);
    assert_eq!(imp.notes, 0);
    let names = deck_names(&open_col(&col_b_path));
    assert!(names.contains(&"Default".to_string()) || !names.is_empty());
}
