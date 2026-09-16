# Engram

Modern, AI-native spaced repetition.

- **Import Anki `.apkg`** — bring decks with you
- **FSRS scheduling** — `fsrs` crate, not SM-2 folklore
- **MCP-native** (M1) — agents can search, generate, and review cards
- **No PyQt** — Rust core; UI is a client

## Status (M0+M1)

Working:

- Collection create/open (SQLite), decks, notes/cards (multi-template)
- FSRS next-state on grade + `optimize` from imported revlog (`--apply` persists)
- CLI: `init` `import` `export` `decks` `notes` `add` `review` `stats` `optimize`
- `.apkg` import: V11 `collection.anki2/21`, modern `collection.anki21b` (zstd) + V18 split tables, revlog, media map
- `.apkg` export: V11 package + media (round-trip tested)
- MCP server: `engram-mcp` stdio, 7 tools (truncated agent payloads)
- Review UI: Tauri window **or** browser via `engram-serve` (audio playback included)
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
cd crates/engram-server
export ENGRAM_SERVER_TOKEN=please-change-me
docker compose up -d --build
# UI     http://host:8787/
# API    Authorization: Bearer please-change-me
# MCP    POST http://host:8787/mcp
```

Agent bridge (stdio host → remote MCP):

```bash
engram-mcp --remote-url http://host:8787/mcp --token please-change-me
```

Details: [crates/engram-server/README.md](crates/engram-server/README.md)

```bash
# browser UI + JSON API (no Tauri needed)
cargo build -p engram-serve
cd apps/desktop && npm install && npm run build

$env:ENGRAM_COLLECTION="E:\path\to\collection.egdb"
./target/debug/engram-serve --listen 127.0.0.1:8787 --static-dir apps/desktop/dist
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

Sync: multi-device via self-hosted **`engram-sync`** (Docker-ready).  
`engram sync push|pull` · compose in `crates/engram-sync/`. Details: [docs/SYNC.md](docs/SYNC.md)

## Quick start

```bash
cargo build
cargo test

# CLI
$env:ENGRAM_COLLECTION="$PWD\.smoke\collection.egdb"
./target/debug/engram init
./target/debug/engram add --deck Demo --front "Q" --back "A"
./target/debug/engram review
./target/debug/engram optimize          # preview
./target/debug/engram optimize --apply  # persist
./target/debug/engram export backup.apkg

# MCP (stdio)
./target/debug/engram-mcp
# non-interactive review
./target/debug/engram review --deck "考研词汇5500::1 Recite" --json --limit 5
```

Connect Claude / Cursor via the stdio snippets in [docs/MCP.md](docs/MCP.md#connect-claude-desktop--cursor-stdio).

Import a package:

```bash
./target/debug/engram import path\to\deck.apkg
./target/debug/engram decks
```

## Workspace

```text
crates/engram-core   domain, SQLite, FSRS
crates/engram-apkg   Anki package import
crates/engram-cli    terminal client
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
