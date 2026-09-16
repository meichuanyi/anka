# Compatibility with Anki

Engram is **compatible with Anki data files**, not with the Anki application platform.

## Supported

| Capability | Status | Notes |
|------------|--------|-------|
| Import `.apkg` | Planned (M0) | Notes, cards, decks, media; revlog best-effort |
| Import `.colpkg` | Later | Full collection; higher fidelity requirements |
| Export `.apkg` | Later (M0.5+) | Best-effort for sharing back to Anki users |
| FSRS scheduling | Yes (going forward) | Uses official `fsrs` crate semantics |
| Historical FSRS params | Import when present | Re-optimize if data is sparse |

## Not supported

| Capability | Why |
|------------|-----|
| AnkiWeb live sync | Proprietary protocol; ToS + fragility |
| Anki add-ons | Bound to PyQt/Anki internals |
| Full Anki template engine | Subset first; extend if import fidelity demands |
| SM-2 as primary scheduler | Legacy; may approximate once on import |
| Qt UI themes / deck options parity | Different product |

## Fidelity principles

1. Never silently drop fields we claim to import — log skipped items.
2. Keep an `anki_id_map` side table so future export can restore identity.
3. Prefer user-visible import report over “it worked” when media/HTML is partial.
4. HTML in fields is stored as-is; sanitization is a display concern.

## Messaging

**Do say:** “Import your Anki decks; schedule with FSRS; expose everything to AI via MCP.”

**Don’t say:** “Drop-in Anki replacement with AnkiWeb sync and add-on support.”
