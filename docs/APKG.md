# Anki `.apkg` format (import notes for Anka)

Source of truth: Anki repo (`rslib/src/import_export/`, `storage/`).

## Zip members

| Member | Meaning |
|--------|---------|
| `meta` | protobuf PackageMetadata; `version=3` = latest |
| `collection.anki21b` | main DB; latest is **zstd-compressed SQLite** |
| `collection.anki21` / `collection.anki2` | legacy uncompressed SQLite |
| `media` | latest: zstd+protobuf MediaEntries; legacy: JSON map `{"0":"a.jpg"}` |
| `0`,`1`,… | media file payloads (index names) |

Version detect: read `meta`; else if `collection.anki21` → Legacy2; else Legacy1.  
Schema version: `col.ver` (V11 vs V18+).

## What Anka imports today

| Item | V11 | V18 |
|------|-----|-----|
| decks | `col.decks` JSON | split `decks.name` (TEXT, `\x1f` hierarchy) or residual JSON |
| notes | `notes.flds` (`\x1f`) | same |
| cards | `cards.nid/did/ord` | same |
| notetypes | `col.models` JSON | `notetypes` + `fields` tables |
| media map | JSON map + payloads | **warning only** (zstd+protobuf) |
| revlog / schedule | skipped | skipped |

Multi-template notes create **one Anka card per Anki card** (template `ord` preserved).

## Critical V15+ gotcha

V15+ often clears `col.decks` / `col.models` to `''`. Import **must** fall through to split tables or every deck becomes `Default`. Anka tests cover this (`import_v18_col_residual_decks_json`).

## V18 deck names

`decks.name` uses `\x1f` as hierarchy separator (human form `::`). Import converts to `::`.

## Anka mapping

| Anki | Anka |
|------|--------|
| deck name `A::B` | deck name kept |
| note fields | `Note.fields` |
| each card | one Anka card in that card's deck (`template_idx = ord`) |
| anki ids | `anki_id_map` side table |

## Real-package validation

Validated with `考研词汇5500.apkg` (Legacy1 `collection.anki2`, V11):

| Metric | Source | Anka |
|--------|--------|--------|
| notes | 5494 | 5494 |
| cards | 16481 (3 templates) | 16481 |
| decks | Recite / Spelling / Dictation | same |
| media | 5497 JSON map | 5495 copied (2 missing payloads warned) |
| import time | — | ~170s debug build |

Lessons:

- Multi-template notes must create **one card per Anki card** (not one per note).
- Multi-field dictionary notes need preview heuristics: phonetic + short defs, strip HTML.
- `cargo test` does **not** refresh `target/debug/anka.exe` — always `cargo build` before CLI runs.

## Export (later)

Build temp SQLite (V11 is enough for many importers) → zip with `meta` + zstd collection + media map.

## Remaining limits

- V18 media protobuf not decoded
- `decks.common` / `decks.kind` blobs ignored (filtered decks)
- Notetype name not stored on notes (`basic` hardcoded)
- Revlog / FSRS history not imported yet
- Schema > 18 is best-effort with warning
