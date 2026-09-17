# anka-server

**Remote self-hosted Anka**: one process, one collection, one token.

| Endpoint | Purpose |
|----------|---------|
| `GET /health` | liveness (no auth) |
| `GET /api/decks` · `due` · `notes` … | review UI API (Bearer) |
| `POST /mcp` | remote MCP JSON-RPC (Bearer) |
| `/media/*` | audio |
| `/` | static web UI (if dist baked in) |

## Design (matches product intent)

- **Single-user self-host** (NAS / VPS)
- **Data lives on the server** — UI and Agents are thin clients
- **MCP is remote** (HTTP), not local stdio against a laptop DB
- Local `anka-sync` multi-master is optional / backup, not the main path

## Run (local binary)

```bash
export ANKA_SERVER_TOKEN=please-change-me
export ANKA_COLLECTION=./data/collection.akdb
export ANKA_SERVER_STATIC=apps/desktop/dist
cargo run -p anka-server
# http://127.0.0.1:8787/health
```

## Run (Docker)

```bash
cd crates/anka-server
export ANKA_SERVER_TOKEN=please-change-me
docker compose up -d --build
```

Volume: `anka-data` → `/data/collection.akdb` + `/data/media/`.

## Remote MCP

```bash
curl -s http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer please-change-me" \
  -H "content-type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Claude/Cursor: many hosts still prefer stdio. Use a thin bridge that POSTs to `/mcp`, or a host with HTTP MCP support.

Example bridge config (local helper → remote):

```json
{
  "mcpServers": {
    "anka": {
      "command": "anka-mcp-http-bridge",
      "args": ["--url", "http://nas.local:8787/mcp", "--token", "please-change-me"]
    }
  }
}
```

## Clients

- Browser: open `http://host:8787/`
- Mobile: same URL on LAN
- Tauri: point API base at the server (or keep local file for offline later)
