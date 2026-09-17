# Anka Roadmap

## M0 — Self-use core (current)

Goal: replace a personal Anki workflow for import + daily review on one machine.

- [x] Repo + architecture
- [ ] `anka-core`: collection open/create, decks, notes, cards
- [ ] SQLite schema + migrations
- [ ] FSRS answer/next-state
- [ ] `anka-apkg`: import `.apkg`
- [ ] `anka-cli`: `import`, `deck list`, `review`, `stats`

Success check: import a real `.apkg`, review 20 cards from CLI, collection survives restart.

## M1 — AI / MCP differentiation

- [ ] `anka-mcp` stdio server
- [ ] Tools: search, due cards, create notes, answer card
- [ ] Material → cards generation flow (app-side, provider-agnostic)
- [ ] Opt-in tools: batch ops, FSRS optimize

Success check: an MCP client can build a deck from a pasted outline and review it.

## M2 — Polished desktop UI

- [x] Tauri shell scaffold (`apps/desktop`)
- [x] Review screen: deck list → flip → grade (keyboard 1-4 / Space)
- [x] Dual transport: Tauri IPC + local HTTP (`anka-serve`)
- [x] FSRS feedback on grade (stability, next due)
- [x] Design system pass (dark void + paper card)
- [x] Media playback/render (HTTP `/media` + sound button + autoplay on card)
- [x] Card templates: Recite / Spelling / Dictation front-back
- [x] Mobile single-file app (`E:\projects\nas\index.html`)

## Remaining product gaps

### M2 finish
- [ ] Note editor / browse table
- [ ] Packaged installer (`tauri build`)
- [ ] Image occlusion / cloze templates

### M1 polish
- [ ] Material → cards generation flow (app-side, provider-agnostic)

### M3 — Own cloud (optional commercial)
- [ ] Auth + sync protocol (offline-first, conflict policy documented)
- [ ] Media sync
- [ ] Shareable decks
- [ ] Only then evaluate hosted plans / dual license

## Attention strategy

- Lead with: **Anki import + FSRS + MCP-native SRS**
- README demo: terminal review + MCP creating cards
- Keep core AGPL/GPL-friendly until commercial signal is real
