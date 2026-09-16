# Engram Architecture

Engram is a modern, AI-native spaced repetition system. It is **compatible with Anki data** (`.apkg` import/export) and **FSRS scheduling**, but is not a fork: no PyQt, no AnkiWeb hard dependency, no plugin ABI.

## Product thesis

1. Self-use first; open source to earn attention; commercialize only if traction appears.
2. Differentiate on **AI content pipeline + MCP programmability + polished UI**, not on reinventing the forgetting curve.
3. Own the sync story later; never block MVP on AnkiWeb protocol reverse-engineering.

## Target: remote self-host (product default)

```text
                    ┌──────────────────────────────────────┐
                    │  engram-server (Docker / VPS / NAS)   │
                    │  /api/*   review & edit               │
                    │  /mcp     remote MCP (JSON-RPC HTTP)  │
                    │  /media   audio                       │
                    │  /        web UI                      │
                    │  data: collection.egdb + media/       │
                    └──────────────────▲───────────────────┘
                                       │ HTTPS + Bearer token
          ┌────────────────────────────┼────────────────────────────┐
          │                            │                            │
     Browser / phone              Tauri thin client            Agent (MCP)
     复习 UI                      打同一 API                   eng ram-mcp --remote-url
```

- **Single-user self-host**: one collection + one token.
- **MCP is server-side**; clients use HTTP or a stdio→HTTP bridge.
- Local multi-master `engram-sync` is optional backup, not the primary story.

## Historical: multi-local + sync log (still available)

```text
┌──────────────────────────────────────────────────────────────────┐
│  Clients                                                         │
│  eng ram CLI · eng ram-mcp · Tauri desktop · mobile index.html   │
├──────────────────────────────────────────────────────────────────┤
│  Local edge                                                      │
│  eng ram-serve (HTTP API + static UI + /media)                   │
├──────────────────────────────────────────────────────────────────┤
│  Core (Rust)  eng ram-core                                       │
│  Collection · Notes/Cards · FSRS(+optimize) · Search             │
│  eng ram-apkg (.apkg import/export)                              │
├──────────────────────────────────────────────────────────────────┤
│  Storage                                                         │
│  SQLite collection.egdb  +  media/                               │
├──────────────────────────────────────────────────────────────────┤
│  Optional self-host sync                                         │
│  eng ram-sync (append-only log)  ← Docker / compose              │
└──────────────────────────────────────────────────────────────────┘
         ▲ push/pull (CLI)                    ▲ import/export
         │                                    │
    eng ram-sync :8788                   Anki .apkg (edge only)
```

### Layer rules

- Core never calls UI or MCP.
- UI / MCP / CLI only go through core APIs (library or local HTTP).
- Anki formats are an **edge adapter** (`engram-apkg`), not the internal schema.
- Sync is optional; one machine can share one `collection.egdb` without a server.

## Workspace layout

```text
engram/
  Cargo.toml              # workspace
  crates/
    engram-core/          # domain + SQLite + FSRS
    engram-apkg/          # .apkg / .colpkg import-export
    engram-mcp/           # MCP server binary
    engram-cli/           # developer/self-use CLI
  apps/                   # later: desktop (Tauri), web
  docs/
    ARCHITECTURE.md
    ROADMAP.md
    COMPAT.md             # what we do / do not promise vs Anki
```

## Domain model (core)

| Entity | Meaning |
|--------|---------|
| `Deck` | Hierarchy of study material |
| `Note` | Source fields + notetype |
| `Card` | One reviewable item with scheduling state |
| `Notetype` | Field/template definition (simplified vs Anki) |
| `Revlog` | One answer event |
| `Media` | File referenced by notes |
| `Preset` | FSRS + daily limits |

Internal IDs are our own UUIDs/ULIDs. Import maps Anki IDs → Engram IDs and keeps a side table for re-export fidelity.

## Scheduling

- Algorithm: **FSRS** via [`fsrs`](https://crates.io/crates/fsrs) (`fsrs-rs`).
- Do not reimplement memory models in v1.
- Optimizer consumes imported/historical revlog when available.
- SM-2 is **not** implemented (import may approximate state once).

## Storage

- Single SQLite database per collection (`collection.egdb` or similar).
- Media directory next to the DB.
- Schema is owned by Engram; migrations are explicit.
- Prefer boring relational tables over document blobs for notes/cards.

## Anki compatibility

See `COMPAT.md`. Summary:

| In scope | Out of scope |
|----------|--------------|
| Import `.apkg` (notes/cards/media/revlog when present) | Anki add-ons |
| Export `.apkg` best-effort | Live AnkiWeb account sync |
| FSRS-equivalent scheduling going forward | Full Anki template engine parity |
| Read FSRS-related fields when present | Qt themes / legacy SM-2 behavior |

## AI + MCP

MCP is a **first-class client**, not a bolt-on:

- Tools operate on the same core as UI.
- Default tools are read-oriented; destructive/optimization tools require explicit opt-in flags.
- Card generation from raw material is a product feature; the model is called by the app, not hard-wired to one vendor in core.

## Clients roadmap

| Phase | Client | Goal |
|-------|--------|------|
| M0 | CLI | Import, list, review, stats — self-use loop |
| M1 | MCP | Agent can create/search/answer |
| M2 | Desktop (Tauri) | Polished daily UI |
| M3 | Sync + optional Web | Multi-device |

## Non-goals (v1)

- AnkiWeb protocol compatibility
- Python add-on host
- Mobile apps
- Multi-user realtime collaboration
- Replacing FSRS with a custom algorithm

## Design principles

1. **Library first** — anything the UI can do, CLI/MCP can do.
2. **Offline first** — no account required for core study loop.
3. **Adapter at the edge** — Anki is import/export, not the source of truth.
4. **Small public API** — hide SQLite and FSRS details behind domain operations.
5. **Honest compatibility** — document gaps instead of silent loss.
