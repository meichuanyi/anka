<div align="center">

<img src="docs/assets/anka-totem.svg" alt="Anka — the firebird of memory" width="380"/>

# Anka

**Anki, reborn.** Modern, AI-native spaced repetition.

[![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)](https://www.rust-lang.org)

</div>

---

- **Import Anki `.apkg`** — bring decks with you
- **FSRS scheduling** — `fsrs` crate, not SM-2 folklore
- **MCP-native** (M1) — agents can search, generate, and review cards
- **No PyQt** — Rust core; UI is a client

> *Anka*（安卡）是神话中的不死鸟：从 Anki 的余烬中重生，翅膀上带着记忆的火花。

## Status (M0+M1)

Working:

- Collection create/open (SQLite), decks, notes/cards (multi-template)
- FSRS next-state on grade + `optimize` from imported revlog (`--apply` persists)
- CLI: `init` `import` `export` `decks` `notes` `add` `review` `stats` `optimize`
- `.apkg` import: V11 `collection.anki2/21`, modern `collection.anki21b` (zstd) + V18 split tables, revlog, media map
- `.apkg` export: V11 package + media (round-trip tested)
- MCP server: `anka-mcp` stdio, 7 tools (truncated agent payloads)
- Review UI: Tauri window **or** browser via `anka-serve` (audio playback included)
- Real-package validated: 考研词汇5500 (5494 notes / 16481 cards / ~5500 media)

Not yet:

- V18 media protobuf payloads
- Desktop installer package (`tauri build`)
- Note editor / browse table
- Full template engine (qfmt/afmt) / cloze semantics
- AnkiWeb sync

## Self-host (recommended)

One server owns the collection; UI + Agents are clients.

```bash
cd crates/anka-server
export ANKA_SERVER_TOKEN=please-change-me
docker compose up -d --build
# UI     http://host:8787/
# API    Authorization: Bearer please-change-me
# MCP    POST http://host:8787/mcp
```

Agent bridge (stdio host → remote MCP):

```bash
anka-mcp --remote-url http://host:8787/mcp --token please-change-me
```

Details: [crates/anka-server/README.md](crates/anka-server/README.md)

```bash
# browser UI + JSON API (no Tauri needed)
cargo build -p anka-serve
cd apps/desktop && npm install && npm run build

$env:ANKA_COLLECTION="E:\path\to\collection.akdb"
./target/debug/anka-serve --listen 127.0.0.1:8787 --static-dir apps/desktop/dist
# open http://127.0.0.1:8787
```

Tauri shell (native window):

```bash
cd apps/desktop
npm install
npm run tauri dev
```

API: `GET /api/decks` · `GET /api/due?deck=&limit=` · `POST /api/grade {cardId,rating}`  
`GET/POST /api/notes` · `GET/PUT /api/notes/:id` — browse / create / edit

UI: home → **+ 新建** / **浏览**（编辑字段与标签）

Sync: multi-device via self-hosted **`anka-sync`** (Docker-ready).  
`anka sync push|pull` · compose in `crates/anka-sync/`. Details: [docs/SYNC.md](docs/SYNC.md)

## Quick start

```bash
cargo build
cargo test

# CLI
$env:ANKA_COLLECTION="$PWD\.smoke\collection.akdb"
./target/debug/anka init
./target/debug/anka add --deck Demo --front "Q" --back "A"
./target/debug/anka review
./target/debug/anka optimize          # preview
./target/debug/anka optimize --apply  # persist
./target/debug/anka export backup.apkg

# MCP (stdio)
./target/debug/anka-mcp
# non-interactive review
./target/debug/anka review --deck "考研词汇5500::1 Recite" --json --limit 5
```

Connect Claude / Cursor via the stdio snippets in [docs/MCP.md](docs/MCP.md#connect-claude-desktop--cursor-stdio).

Import a package:

```bash
./target/debug/anka import path\to\deck.apkg
./target/debug/anka decks
```

## Workspace

```text
crates/anka-core   domain, SQLite, FSRS
crates/anka-apkg   Anki package import
crates/anka-cli    terminal client
docs/                architecture, roadmap, compat, apkg, mcp
```

## Docs

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Anki compatibility](docs/COMPAT.md)
- [.apkg notes](docs/APKG.md)
- [MCP design](docs/MCP.md)

## License

AGPL-3.0-or-later
