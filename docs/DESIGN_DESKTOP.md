# Anka Desktop — Design Spec (M2)

Mode: **expressive product UI** (review session is the hero). Not an admin console.

## Subject

Anka is a modern SRS client. The daily job is **one focused review session**: see a prompt, recall, grade, next. Everything else (decks, stats, import) is secondary chrome.

Audience: self-learners (e.g. 考研词汇) who want calm, fast, beautiful study — not Anki's dense browser.

## Tokens

```
--void      #101318   app chrome / outer field
--surface   #181C22   panels
--paper     #F3EFE6   card face (memory membrane)
--ink       #1A1D21   text on paper
--mist      #9AA3AF   secondary text on dark
--signal    #6FE3A6   Good / success / progress
--hard      #E6C36A   Hard
--easy      #7EB6FF   Easy
--lapse     #F07178   Again / errors
--line      #2A3038   hairlines
```

## Type

| Role | Stack | Use |
|------|-------|-----|
| display | `Georgia, 'Songti SC', 'SimSun', serif` | Card headword — lexicographic, memorable |
| body | `'Segoe UI', 'PingFang SC', 'Microsoft YaHei', system-ui, sans-serif` | Definitions, UI |
| data | `'Cascadia Code', Consolas, ui-monospace, monospace` | due, S/D, counts |

Scale: headword 48–64px / 1.15; definition 17px / 1.65; UI 13–14px; data 12px.

## Layout

```
┌──────────────────────────────────────────────┐
│  deck · 12/40                    ● 90% target│  ← thin instrument strip
├──────────────────────────────────────────────┤
│                                              │
│              ┌────────────────┐              │
│              │   abandon      │              │  ← paper card
│              │   [ə'bænd(ə)n] │              │
│              │                │              │
│              │  (answer zone) │              │
│              └────────────────┘              │
│                                              │
│         [ Again ] [ Hard ] [ Good ] [ Easy ] │  ← grade pads
└──────────────────────────────────────────────┘
```

- Outer: full viewport `--void`
- Card: max-width 560px, paper fill, large radius, soft shadow
- No left nav in review mode; Esc / click deck name returns to deck list

## Signature

**Memory membrane**: a single paper-colored card floating in a dark field. The headword is set like a dictionary entry (serif). Revealing the answer expands the membrane downward — one orchestrated height/opacity transition, not a modal.

## Interaction

| Key | Action |
|-----|--------|
| Space / Enter | Show answer |
| 1–4 | Again / Hard / Good / Easy |
| Esc | Back to decks |

- Before reveal: only front + phonetic hint line
- After reveal: definition (stripped HTML), example if short
- Grade buttons disabled until reveal
- Empty deck: quiet invitation, not error red
- Loading: card shell with shimmer, no layout jump

## Non-goals (M2)

- Editor, stats graphs, media audio play, add-on chrome, AnkiWeb

## Implementation notes

- Tauri 2 commands call `anka-core` Collection directly (no HTTP server required)
- Frontend: Vite + vanilla TS + CSS (no Tailwind) for full token control
- Collection path: `ANKA_COLLECTION` or picker
