# ADR-0005 — Endless references, with an AI model for safety only

**Status:** Accepted (2026-11-18, user decision)

## Context
A few dozen hand-approved references isn't enough variety. The user wants near-endless references, auto-checked, and the canvas open within seconds.

## Decision
- A Rust service (`crates/ref-service`) keeps a **ready pool** of vetted Pixabay pictures per category × topic × tier, topped up in the background for the slots students actually ask for. Clicking a topic opens the canvas instantly; the reference appears in about 0.1 s locally, ~1 s worst case. If nothing is ready within 1.5 s, the web app falls back to hand-approved references.
- Pixabay rules: no hotlinking (we download to our server and serve `/refimg/…`), search results cached 24 h, API calls ≈ 50/min (limit 100), no mass downloads (we only fetch for requested slots, with a back-off for slots that yield nothing).
- Every picture must pass: Pixabay `isAiGenerated == false` (unknown = reject), `isGRated`, not `isLowQuality`, a tag blocklist (AI/NSFW/franchise/text), size/aspect, too faint / too busy, text-heavy, near-duplicate (pHash), and an **NSFW classifier**.
- **AI is allowed here only as a safety guard (NSFW)**, never for scoring or feedback (ADR-0002 still holds for grading). Reliable detectors for AI-made images don't exist, so we rely on Pixabay's label plus a Report button.
- **Image kind per category** (user decision, 2026-11-19): real life, animals and environment search Pixabay for `photo` at every tier; anime and cartoon stay drawn (`vector` / `illustration`). Drawings that were already in those three categories are dropped from the pool when the service starts (their ids stay in `seen`, so they are never fetched again). How easy a picture is comes from the difficulty tier, not from the image kind.
- Beginners get hand-approved pictures first; Endless kicks in once they've seen them all, and for every other tier.
- **Report** blocks a picture for everyone immediately (logged to `reports.jsonl` for review).

## Consequences
- The site now needs a server (Node for SvelteKit + the Rust service): see M5b-T04.
- Pool state and images live in `data/ref-service/` (git-ignored).
