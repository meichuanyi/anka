# engram-sync

Self-hosted sync server for Engram. Append-only change log; clients merge offline.

## Protocol

| Method | Path | Auth | Body / Query |
|--------|------|------|----------------|
| GET | `/health` | no | — |
| GET | `/v1/status` | Bearer | — |
| POST | `/v1/push` | Bearer | `{deviceId, changes[]}` |
| GET | `/v1/pull?since=&limit=` | Bearer | — |

Change:

```json
{
  "kind": "note|card|revlog",
  "id": "uuid",
  "updatedAt": "2026-09-16T12:00:00Z",
  "deviceId": "laptop-a",
  "payload": {}
}
```

Pull returns `seq` per change; use `cursor` as next `since`.

## Run (local)

```bash
export ENGRAM_SYNC_TOKEN=dev-secret
export ENGRAM_SYNC_DB=./data/sync.db
cargo run -p engram-sync --release
# http://127.0.0.1:8788/health
```

## Run (Docker)

```bash
cd crates/engram-sync
export ENGRAM_SYNC_TOKEN=please-change-me
docker compose up -d --build
curl -s http://127.0.0.1:8788/health
```

Persist volume: `engram-sync-data` → `/data/sync.db`.

### Standalone image

```bash
docker build -f crates/engram-sync/Dockerfile -t engram-sync:0.1 .
docker run -d --name engram-sync -p 8788:8788 \
  -e ENGRAM_SYNC_TOKEN=please-change-me \
  -v engram-sync-data:/data \
  engram-sync:0.1
```

## Client

```bash
engram sync push --server http://127.0.0.1:8788 --token please-change-me
engram sync pull --server http://127.0.0.1:8788 --token please-change-me
```

## Merge rules (client)

- **revlog**: insert if local id missing (append-only)
- **note**: apply if remote `updatedAt` newer, or equal time + lexicographically larger `deviceId`
- **card**: same as note (schedule state)

See also `docs/SYNC.md`.
