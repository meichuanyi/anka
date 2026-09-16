//! Integration tests: build synthetic Anki .apkg packages and import them.
//!
//! V11 uses JSON `col.decks`/`col.models` + uncompressed `collection.anki21`.
//! V18 uses split `decks`/`notetypes`/`fields` tables + zstd `collection.anki21b`.

use std::io::Write;
use std::path::{Path, PathBuf};

use engram_apkg::{import_apkg, ImportReport};
use engram_core::Collection;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FLD_SEP: char = '\x1f';
const DECK_SEP: char = '\x1f';

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

fn create_v11_sqlite(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
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
        "1500000000010": {"id": 1500000000010i64, "name": "Japanese::N3", "conf": 1i64},
    })
    .to_string();
    let models_json = serde_json::json!({
        "1600000000000": {
            "id": 1600000000000i64,
            "name": "Basic",
            "flds": [{"name": "Front"}, {"name": "Back"}],
        }
    })
    .to_string();

    conn.execute(
        "INSERT INTO col VALUES (1,0,0,0,11,0,0,0,'{}',?1,?2,'{}','{}')",
        rusqlite::params![models_json, decks_json],
    )
    .unwrap();

    // Two notes, two cards, different decks.
    conn.execute(
        "INSERT INTO notes VALUES (?1,'guid-front',1600000000000,0,0,' alpha beta ',?2,0,0,0,'')",
        rusqlite::params![1500000000001i64, format!("front one{FLD_SEP}back one")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO notes VALUES (?1,'guid-two',1600000000000,0,0,'',?2,0,0,0,'')",
        rusqlite::params![1500000000002i64, format!("front two{FLD_SEP}back two{FLD_SEP}extra")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cards VALUES (?1,?2,1500000000010,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
        rusqlite::params![1500000000020i64, 1500000000001i64],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cards VALUES (?1,?2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
        rusqlite::params![1500000000021i64, 1500000000002i64],
    )
    .unwrap();
}

fn create_v18_sqlite(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
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
        CREATE TABLE decks (
          id integer PRIMARY KEY NOT NULL,
          name text NOT NULL,
          mtime_secs integer NOT NULL,
          usn integer NOT NULL,
          common blob NOT NULL,
          kind blob NOT NULL
        );
        CREATE TABLE notetypes (
          id integer NOT NULL PRIMARY KEY,
          name text NOT NULL,
          mtime_secs integer NOT NULL,
          usn integer NOT NULL,
          config blob NOT NULL
        );
        CREATE TABLE fields (
          ntid integer NOT NULL,
          ord integer NOT NULL,
          name text NOT NULL,
          config blob NOT NULL,
          PRIMARY KEY (ntid, ord)
        ) without rowid;
        CREATE TABLE templates (
          ntid integer NOT NULL,
          ord integer NOT NULL,
          name text NOT NULL,
          mtime_secs integer NOT NULL,
          usn integer NOT NULL,
          config blob NOT NULL,
          PRIMARY KEY (ntid, ord)
        ) without rowid;
        "#,
    )
    .unwrap();

    // V15+ clears col.decks / col.models to empty strings.
    conn.execute(
        "INSERT INTO col VALUES (1,0,0,0,18,0,0,0,'{}','','','{}','{}')",
        [],
    )
    .unwrap();

    // Deck names use \x1f hierarchy separator (protobuf common/kind unused here).
    conn.execute(
        "INSERT INTO decks VALUES (1, 'Default', 0, 0, X'00', X'00')",
        [],
    )
    .unwrap();
    let japanese = format!("Japanese{DECK_SEP}N3");
    conn.execute(
        "INSERT INTO decks VALUES (1500000000010, ?1, 0, 0, X'00', X'00')",
        [&japanese],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO notetypes VALUES (1600000000000, 'Basic', 0, 0, X'00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fields VALUES (1600000000000, 0, 'Front', X'00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fields VALUES (1600000000000, 1, 'Back', X'00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO templates VALUES (1600000000000, 0, 'Card 1', 0, 0, X'00')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO notes VALUES (?1,'guid-v18',1600000000000,0,0,' vocab ',?2,0,0,0,'')",
        rusqlite::params![1500000000001i64, format!("猫{FLD_SEP}cat")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cards VALUES (?1,1500000000001,1500000000010,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
        [1500000000020i64],
    )
    .unwrap();
}

fn sqlite_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn make_v11_apkg(dir: &Path) -> PathBuf {
    let db_path = dir.join("col.anki21");
    create_v11_sqlite(&db_path);
    let bytes = sqlite_bytes(&db_path);
    let apkg = dir.join("v11.apkg");
    let media_map = serde_json::json!({"0": "hello.txt"}).to_string();
    write_zip(
        &apkg,
        &[
            ("collection.anki21", bytes),
            ("media", media_map.into_bytes()),
            ("0", b"hello media".to_vec()),
        ],
    );
    apkg
}

fn make_v18_apkg(dir: &Path, media_header: Vec<u8>) -> PathBuf {
    let db_path = dir.join("col.anki21b.raw");
    create_v18_sqlite(&db_path);
    let raw = sqlite_bytes(&db_path);
    let compressed = zstd::encode_all(std::io::Cursor::new(&raw), 0).unwrap();
    let apkg = dir.join("v18.apkg");
    write_zip(
        &apkg,
        &[
            ("collection.anki21b", compressed),
            ("media", media_header),
        ],
    );
    apkg
}

fn import_into(dir: &Path, apkg: &Path) -> (PathBuf, ImportReport) {
    let col_path = dir.join("target.egdb");
    let mut col = Collection::create(&col_path).unwrap();
    let report = import_apkg(apkg, &mut col).unwrap();
    (col_path, report)
}

fn open_col(path: &Path) -> Collection {
    Collection::open(path).unwrap()
}

#[test]
fn import_v11_json_decks_notes_cards() {
    let dir = TempDir::new().unwrap();
    let apkg = make_v11_apkg(dir.path());
    let (col_path, report) = import_into(dir.path(), &apkg);

    assert_eq!(report.notes, 2, "warnings: {:?}", report.warnings);
    assert_eq!(report.cards, 2);
    assert_eq!(report.decks, 2, "Default + Japanese::N3");
    assert_eq!(report.media_copied, 1, "warnings: {:?}", report.warnings);

    let col = open_col(&col_path);
    let decks = col.list_decks().unwrap();
    let names: Vec<&str> = decks.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"Default"), "names={names:?}");
    assert!(names.contains(&"Japanese::N3"), "names={names:?}");

    let (_total, notes) = col.search_notes("front one", 10, 0).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].fields[0], "front one");
    assert_eq!(notes[0].fields[1], "back one");
    assert_eq!(notes[0].tags, vec!["alpha".to_string(), "beta".to_string()]);

    let (_total, notes2) = col.search_notes("front two", 10, 0).unwrap();
    assert_eq!(notes2.len(), 1);
    assert_eq!(notes2[0].fields.len(), 3);

    // Media copied next to collection.
    let media = dir.path().join("media").join("hello.txt");
    assert!(media.exists(), "media file should be copied");
    assert_eq!(std::fs::read(&media).unwrap(), b"hello media");
}

#[test]
fn import_v18_split_tables_and_zstd() {
    let dir = TempDir::new().unwrap();
    // V18 media is protobuf/zstd — not JSON. Must warn, not panic.
    let apkg = make_v18_apkg(dir.path(), vec![0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x00, 0x01, 0x00]);
    let (col_path, report) = import_into(dir.path(), &apkg);

    assert_eq!(report.notes, 1, "warnings: {:?}", report.warnings);
    assert_eq!(report.cards, 1);
    assert!(
        report.decks >= 2,
        "expected Default + Japanese::N3, got {}",
        report.decks
    );
    assert_eq!(report.media_copied, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("media map is not JSON")),
        "expected media warning, got {:?}",
        report.warnings
    );

    let col = open_col(&col_path);
    let decks = col.list_decks().unwrap();
    let names: Vec<&str> = decks.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"Default"), "names={names:?}");
    assert!(
        names.contains(&"Japanese::N3"),
        "V18 \\x1f deck separator should become ::; names={names:?}"
    );

    let (_total, notes) = col.search_notes("猫", 10, 0).unwrap();
    assert_eq!(notes.len(), 1, "flds still \\x1f-separated in V18");
    assert_eq!(notes[0].fields[0], "猫");
    assert_eq!(notes[0].fields[1], "cat");
    assert_eq!(notes[0].tags, vec!["vocab".to_string()]);
}

#[test]
fn import_v18_col_residual_decks_json() {
    // Hybrid: V18-shaped tables absent for decks but col.decks still holds JSON.
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("hybrid.sqlite");
    {
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
            "1": {"id": 1, "name": "Default", "conf": 1},
            "99": {"id": 99, "name": "ResidualDeck", "conf": 1},
        })
        .to_string();
        conn.execute(
            "INSERT INTO col VALUES (1,0,0,0,18,0,0,0,'{}','',?1,'{}','{}')",
            [&decks_json],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes VALUES (10,'g',1,0,0,'',?1,0,0,0,'')",
            [format!("q{FLD_SEP}a")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cards VALUES (11,10,99,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
            [],
        )
        .unwrap();
    }
    let raw = sqlite_bytes(&db_path);
    let compressed = zstd::encode_all(std::io::Cursor::new(&raw), 0).unwrap();
    let apkg = dir.path().join("hybrid.apkg");
    write_zip(&apkg, &[("collection.anki21b", compressed)]);

    let (col_path, report) = import_into(dir.path(), &apkg);
    assert_eq!(report.notes, 1);
    let col = open_col(&col_path);
    let names: Vec<String> = col
        .list_decks()
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert!(
        names.contains(&"ResidualDeck".to_string()),
        "names={names:?}"
    );
}

#[test]
fn import_v11_empty_package_is_error() {
    let dir = TempDir::new().unwrap();
    let apkg = dir.path().join("empty.apkg");
    write_zip(&apkg, &[("README", b"no collection".to_vec())]);
    let col_path = dir.path().join("target.egdb");
    let mut col = Collection::create(&col_path).unwrap();
    let err = import_apkg(&apkg, &mut col).unwrap_err();
    assert!(
        err.to_string().contains("no collection database"),
        "err={err}"
    );
}

#[test]
fn import_v11_revlog_history() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("c.anki21");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE col (
              id integer PRIMARY KEY, crt integer, mod integer, scm integer,
              ver integer, dty integer, usn integer, ls integer,
              conf text, models text, decks text, dconf text, tags text
            );
            CREATE TABLE notes (
              id integer PRIMARY KEY, guid text, mid integer, mod integer, usn integer,
              tags text, flds text, sfld integer, csum integer, flags integer, data text
            );
            CREATE TABLE cards (
              id integer PRIMARY KEY, nid integer, did integer, ord integer, mod integer,
              usn integer, type integer, queue integer, due integer, ivl integer,
              factor integer, reps integer, lapses integer, "left" integer, odue integer,
              odid integer, flags integer, data text
            );
            CREATE TABLE revlog (
              id integer PRIMARY KEY, cid integer, usn integer, ease integer,
              ivl integer, lastIvl integer, factor integer, time integer, type integer
            );
            "#,
        )
        .unwrap();
        let decks_json = serde_json::json!({
            "1": {"id": 1, "name": "Default", "conf": 1}
        })
        .to_string();
        let models_json = serde_json::json!({
            "1600000000000": {
                "id": 1600000000000i64,
                "name": "Basic",
                "flds": [{"name": "Front"}, {"name": "Back"}],
            }
        })
        .to_string();
        conn.execute(
            "INSERT INTO col VALUES (1,0,0,0,11,0,0,0,'{}',?1,?2,'{}','{}')",
            rusqlite::params![models_json, decks_json],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes VALUES (20,'g',1600000000000,0,0,'',?1,0,0,0,'')",
            [format!("q{FLD_SEP}a")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cards VALUES (21,20,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
            [],
        )
        .unwrap();
        // Two reviews + one manual (ease=0, should skip).
        // ts base 2020-01-01T00:00:00Z = 1577836800000
        conn.execute(
            "INSERT INTO revlog VALUES (1577836800000,21,0,3,1,0,2500,1200,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO revlog VALUES (1578009600000,21,0,2,3,1,2100,1500,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO revlog VALUES (1578096000000,21,0,0,3,3,0,0,0)",
            [],
        )
        .unwrap();
    }
    let raw = sqlite_bytes(&db_path);
    let apkg = dir.path().join("rev.apkg");
    write_zip(&apkg, &[("collection.anki21", raw)]);

    let (col_path, report) = import_into(dir.path(), &apkg);
    assert_eq!(report.notes, 1);
    assert_eq!(report.cards, 1);
    assert_eq!(report.revlogs, 2, "ease=0 should be skipped");
    let col = open_col(&col_path);
    let due = col.due(None, 10).unwrap();
    assert_eq!(due.len(), 1);
    let rev = col.revlog_for_card(due[0].card.id).unwrap();
    assert_eq!(rev.len(), 2);
}
