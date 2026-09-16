# Engram MCP Design

MCP is a first-class client of `engram-core`, not a side API. Goal: **让 Agent 陪你复习**.

## Tagline

> engram：让 Agent 陪你复习的记忆系统。

## Permission tiers

| Tier | Tools | Policy |
|------|-------|--------|
| L0 read | list / search / due / stats / get_params | On by default |
| L1 single write | deck.create, note.create, review.grade, apkg.import | On by default, can disable |
| L2 batch / confirm | review.batch, card.delete | Default off or `confirm: true` |
| L3 system params | fsrs.optimize (`apply=true`) | Explicit confirm + audit log |

Principles: local collection; reversible drafts for bulk creates; no silent network sync.

## MVP tools (7) — M0/M1 shipped in `engram-mcp`

| Tool | In | Out | RO |
|------|----|-----|----|
| `deck.list` | `{query?}` | `{decks:[{id,name,new,learning,due}]}` | ✓ |
| `note.search` | `{query,limit,offset}` | `{total,items:[{id,deckId,front,back,fields,tags}]}` | ✓ |
| `note.create` | `{deck,front,back,tags?}` | `{noteId,cardId,deckId}` | ✗ |
| `review.due` | `{deck?,limit=20}` | `{cards:[{cardId,front,back,dueAt,kind,…}]}` | ✓ |
| `review.grade` | `{cardId,rating:1-4}` | `{cardId,nextDue,stability,difficulty}` | ✗ |
| `stats.weak` | `{limit?}` | `{hardCards[],decks[]}` | ✓ |
| `fsrs.optimize` | `{apply?=false}` | `{params,logLoss,applied}` | ✗ L3 |

Notes:

- `front`/`back` come from `engram_core::front_back` (same helper as CLI `notes` / `review --show`), so multi-field Anki notes preview consistently.
- `fields` in search/due payloads are truncated (~240 chars/field) so Collins-style HTML dictionary blobs do not flood agent context.
- `deck` is an **exact** deck name, including hierarchical Chinese names such as `考研词汇5500::1 Recite`.

Transport: JSON-RPC 2.0 over stdio (`initialize`, `tools/list`, `tools/call`).

```bash
export ENGRAM_COLLECTION=./collection.egdb
./target/debug/engram-mcp
# or
./target/debug/engram-mcp --collection /path/to/collection.egdb
```

## Connect Claude Desktop / Cursor (stdio)

Point the client at the `engram-mcp` binary and pass your collection path.

**Claude Desktop** (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "engram": {
      "command": "E:/projects/nas/engram/target/debug/engram-mcp.exe",
      "args": ["--collection", "E:/projects/nas/engram/.smoke/kaoyan3.egdb"],
      "env": {
        "RUST_LOG": "warn"
      }
    }
  }
}
```

**Cursor** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "engram": {
      "command": "E:/projects/nas/engram/target/debug/engram-mcp.exe",
      "args": ["--collection", "E:/projects/nas/engram/.smoke/kaoyan3.egdb"]
    }
  }
}
```

On macOS/Linux use the absolute path to `target/debug/engram-mcp` (no `.exe`). Relative paths in `args` are resolved from the client's working directory — prefer absolute paths.

Smoke-check over stdin without a client:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"deck.list","arguments":{}}}' \
  | ENGRAM_COLLECTION=./collection.egdb ./target/debug/engram-mcp
```

## Post-MVP tools

| Tool | Notes |
|------|-------|
| `deck.create` | idempotent by name |
| `review.batch` | L2 |
| `card.delete` | L2 |
| `apkg.import` | L1 |
| `fsrs.get_params` | L0 |
| `fsrs.optimize` | L3 when applying |

Unified response envelope: `{ok, message, ...payload}`.

## Resources

- `engram://decks`
- `engram://deck/{id}/stats`
- `engram://fsrs/params`

Read-only; no write resources.

## Prompts (post-MVP)

- `note_to_cards` — long text → structured card drafts
- `review_session` — tutor-style review script
- `gap_fill` — weak spots → supplemental cards

## User stories

1. **Material → deck**: agent reads notes → drafts `note.create` batch → user confirms → store.
2. **Study buddy**: `review.due` → explain → user rates → `review.grade`.
3. **Gap fill**: `stats.weak` → propose cards → `note.create`; optional `fsrs.optimize(apply:false)`.

## Differentiation vs Anki

Anki has add-ons and community HTTP bridges, but no documented first-class MCP. Engram ships MCP tools as part of the product surface with stable schemas and explicit permission tiers.

## Implementation notes (M1)

- Binary: `engram-mcp` (stdio transport first).
- Opens the same collection path as CLI (`ENGRAM_COLLECTION` or config).
- Never call out to network from tool handlers in M1.
