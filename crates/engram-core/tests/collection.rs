use engram_core::{Collection, Rating};
use tempfile::TempDir;

#[test]
fn create_add_review_roundtrip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.egdb");
    let mut col = Collection::create(&path).unwrap();

    let deck = col.ensure_deck("Default").unwrap();
    let (note, card) = col
        .add_note(deck.id, vec!["Q".into(), "A".into()], vec!["tag".into()])
        .unwrap();
    assert_eq!(note.fields.len(), 2);

    let due = col.due(None, 10).unwrap();
    assert_eq!(due.len(), 1);

    let updated = col.answer_card(card.id, Rating::Good, 0).unwrap();
    assert!(updated.state.stability > 0.0);
    assert!(updated.state.due_at > chrono::Utc::now());

    let due2 = col.due(None, 10).unwrap();
    assert!(due2.is_empty() || due2[0].card.id != card.id || due2[0].card.state.due_at > chrono::Utc::now());

    let counts = col.deck_counts().unwrap();
    assert_eq!(counts.len(), 1);
}

#[test]
fn search_notes_finds_field_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.egdb");
    let mut col = Collection::create(&path).unwrap();
    let deck = col.ensure_deck("Default").unwrap();
    col.add_note(deck.id, vec!["mitochondria".into(), "powerhouse".into()], vec![])
        .unwrap();
    let (total, items) = col.search_notes("mitochondria", 10, 0).unwrap();
    assert_eq!(total, 1);
    assert_eq!(items.len(), 1);
}

#[test]
fn search_notes_prefers_headword_over_body_hit() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.egdb");
    let mut col = Collection::create(&path).unwrap();
    let deck = col.ensure_deck("Vocab::1").unwrap();
    // Body/HTML hit first (older), headword hit second (newer).
    col.add_note(
        deck.id,
        vec![
            "yet".into(),
            "[jet]".into(),
            "We should not yet abandon this option.".into(),
        ],
        vec![],
    )
    .unwrap();
    col.add_note(
        deck.id,
        vec![
            "abandon".into(),
            "[ə'bænd(ə)n]".into(),
            "vt. 遗弃；放弃".into(),
        ],
        vec![],
    )
    .unwrap();
    let (total, items) = col.search_notes("abandon", 10, 0).unwrap();
    assert_eq!(total, 2);
    assert_eq!(items[0].fields[0], "abandon");
}

#[test]
fn front_back_skips_phonetic_and_sound() {
    let fields = vec![
        "abandon".into(),
        "[ə'bænd(ə)n]".into(),
        String::new(),
        "n. 狂热；放任 vt. 遗弃；放弃".into(),
        "[sound:abandon.mp3]".into(),
        "<div class=\"x\">lots of html</div>".into(),
    ];
    let (front, back) = engram_core::front_back(&fields);
    assert_eq!(front, "abandon");
    assert!(back.contains("遗弃"));
    assert!(back.contains("ə'bænd"));
    assert!(!back.contains("[sound:"));
}

#[test]
fn deck_counts_report_new_cards() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.egdb");
    let mut col = Collection::create(&path).unwrap();
    let deck = col.ensure_deck("考研词汇5500::1 Recite").unwrap();
    col.add_note(deck.id, vec!["a".into(), "b".into()], vec![]).unwrap();
    col.add_note(deck.id, vec!["c".into(), "d".into()], vec![]).unwrap();
    let counts = col.deck_counts().unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].new_count, 2);
    assert_eq!(counts[0].review_count, 0);
}
