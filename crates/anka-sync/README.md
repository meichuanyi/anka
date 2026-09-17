# anka-sync

Self-hosted sync server for Anka. Append-only change log; clients merge offline.

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
export ANKA_SYNC_TOKEN=dev-secret
export ANKA_SYNC_DB=./data/sync.db
cargo run -p anka-sync --release
# http://127.0.0.1:8788/health
```

## Run (Docker)

```bash
cd crates/anka-sync
export ANKA_SYNC_TOKEN=please-change-me
docker compose up -d --build
curl -s http://127.0.0.1:8788/health
```

Persist volume: `anka-sync-data` → `/data/sync.db`.

### Standalone image

```bash
docker build -f crates/anka-sync/Dockerfile -t anka-sync:0.1 .
docker run -d --name anka-sync -p 8788:8788 \
  -e ANKA_SYNC_TOKEN=please-change-me \
  -v anka-sync-data:/data \
  anka-sync:0.1
```

## Client

```bash
anka sync push --server http://127.0.0.1:8788 --token please-change-me
anka sync pull --server http://127.0.0.1:8788 --token please-change-me
```

## Merge rules (client)

- **revlog**: insert if local id missing (append-only)
- **note**: apply if remote `updatedAt` newer, or equal time + lexicographically larger `deviceId`
- **card**: same as note (schedule state)

See also `docs/SYNC.md`.
