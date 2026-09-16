# Sync design (Engram)

## Current status (M0–M2)

**There is no multi-device sync.** Engram is local-first:

- One SQLite collection file (`collection.egdb`)
- Media folder next to the collection
- All clients (CLI / MCP / serve / Tauri) open the **same local file**

“Sync” today means: point every tool at the same `ENGRAM_COLLECTION` path, or copy the file yourself.

```text
  CLI ──┐
  MCP ──┼──► collection.egdb + media/
  Web ──┤
Tauri ──┘
```

This is intentional: correct FSRS state and undo before inventing a cloud protocol.

## What we will NOT do

| Approach | Why not |
|----------|---------|
| Live AnkiWeb protocol | Proprietary, ToS risk, breaks on server changes |
| Treat `.apkg` as sync | Package is export/import, not a live replica |
| Silent last-write-wins on the whole DB | Destroys revlog and FSRS params |

## M3 target: own sync (offline-first)

### Goals

1. Multi-device without an account for local-only users
2. Preserve review history and FSRS parameters
3. Optional hosted relay later; self-host first
4. Honest conflict story

### Data units

| Unit | Conflict policy |
|------|-----------------|
| Note fields / tags | Field-level or LWW per note with vector clock |
| Card schedule (due, S, D, lapses) | **Merge by latest review event**, never blindly LWW |
| Revlog entries | **Append-only union** (ids are ULIDs) |
| Media files | Content-addressed; first-writer wins by hash |
| Deck hierarchy | LWW on name; soft-delete optional |

### Sketch

```text
Device A                    Relay / self-host              Device B
  │  push revlog + notes           │                          │
  │ ─────────────────────────────► │  append + merge          │
  │                                │ ◄────────────────────────│
  │  pull since cursor             │                          │
  │ ◄───────────────────────────── │                          │
```

- Each device keeps a `sync_cursor` (server sequence or hybrid logical clock)
- Pull: new revlog rows, note patches, media blobs
- Push: local unsynced rows (dirty flags)
- Revisions: CRDT-lite — revlog is naturally commutative; notes use updated_at + device_id tie-break

### Auth / hosting

1. **Self-host**: single binary `engram-sync` + token
2. **Hosted (optional commercial)**: same protocol, account + quota for media
3. Local-only users never need the network

### Security

- E2E encryption optional phase 2 (client-side media + field encryption)
- At minimum: TLS + bearer token; no plaintext collection dump on server if avoidable

## Practical advice until M3

| Need | Workaround |
|------|------------|
| Phone + PC same library | Use `engram-serve` on LAN; mobile browser hits `http://pc-ip:8787` |
| Backup | Copy `collection.egdb` + `media/` |
| Move machine | Zip the pair; or `engram export` / `import` `.apkg` |
| Share a deck | `engram export` → send `.apkg` |

## Sequencing

- [x] Single-machine multi-client (file + HTTP)
- [x] `engram-sync` self-host prototype (append-only log + token)
- [x] CLI `engram sync push|pull` with LWW / revlog union merge
- [ ] Media blob sync
- [ ] Hosted relay (only if demand)

## Quick start (MVP shipped)

Server:

```bash
# local
export ENGRAM_SYNC_TOKEN=please-change-me
cargo run -p engram-sync

# docker
cd crates/engram-sync && ENGRAM_SYNC_TOKEN=please-change-me docker compose up -d --build
```

Clients:

```bash
export ENGRAM_SYNC_SERVER=http://127.0.0.1:8788
export ENGRAM_SYNC_TOKEN=please-change-me
engram sync push
engram sync pull
```

Verified locally: device A `add` → `push` (2 changes) → device B `pull` → note visible.

## Related

- [COMPAT.md](COMPAT.md) — no AnkiWeb live sync
- [ARCHITECTURE.md](ARCHITECTURE.md) — offline-first principles
