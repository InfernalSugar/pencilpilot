# Content guide

The voice is **Fun, kind, specific**: a cheerful coach, not a judge.

## Rules (`content/feedback/rules.toml`)
```toml
[[rule]]
id = "too-big"
measurement = "scale_ratio"   # a grader measurement (crates/grader MEASUREMENT_DOCS)
above = 1.15                  # or below = …; abs = true compares |value|
span = 0.3                    # how far past the threshold counts as "as bad as it gets"
min = 0.2                     # optional window: only fire between min and max
severity = 4                  # 1 (minor) – 5 (major); scaled up by how far past the threshold
skill = "placement"           # one fix per skill is shown
number = "ratio_pct"          # how {n} is computed: ratio_pct | abs_pct | pct | deg
dir_pos = "wide"              # optional words for {dir} by the value's sign
dir_neg = "narrow"
overlay = "bbox"              # what "Show me" highlights: bbox | offset | grid | missing | extra
lesson = "measuring-with-units"
drill = "block-in-boxes"
categories = ["real-life"]    # optional: only for these categories
```
The engine (`crates/feedback`) shows the top 3 fixes (one per skill) plus praise for the strongest skill.
Content is embedded at build time, so run `pnpm wasm` after editing.

## Tips (`content/feedback/tips.toml`)
```toml
[tip.too-big]
title = "Your drawing is {n}% too big"
variants = ["…{n}%…", "…"]     # 2–4 wordings; picked per attempt so repeats differ
```
- Use `{n}` (the rule's number) and `{dir}` (only if the rule has dir words).
- Structure: **what happened** (with the number) → **why it happens** → **one concrete fix**. Under ~60 words.

## Praise
`[tip.praise-<skill>]` for placement, outline, silhouette, tone, line_confidence, proportions: shown for the strongest skill at ≥ 70%.

## Lessons and drills (`content/lessons/`)
Short (2–3 minute read), picture-first, ending with a drill the site can grade.

One file per lesson, `content/lessons/<id>.md` (the id is what `lesson = "…"` in rules.toml points to):
```markdown
---
title: Measuring with units
skill: proportions          # placement | outline | silhouette | tone | line_confidence | proportions
minutes: 3
summary: One line for the lesson cards.
learn:                      # exactly 3 bullets for the "You'll learn" tile
  - …
drill: head-units           # a drill id used in rules.toml
next: finding-angles        # another lesson id ("Up next")
---

## Step title
Short paragraphs. 3–5 steps in all.
```
- Every lesson needs at least one **hand-drawn inline SVG** diagram: `role="img"`, a descriptive
  `aria-label`, `stroke="currentColor"`, and **no blank lines inside the `<svg>`** (Markdown would
  end the HTML block). Theme classes: `accent` (coral stroke), `faint` (muted stroke),
  `fill-yellow`, `fill-lilac`, `fill-paper`, `label` (text). Each step shows its own diagram, or
  the latest one before it.
- No `<script>`, event handlers or `javascript:` links (they are stripped, and a test fails on them).
- `src/lib/lessons.test.ts` checks ids, drill/next links, step counts and diagrams.

### Drills (`apps/web/src/lib/drills.ts`, pages `/drills` and `/drills/<id>`)
Each drill has a fixed id (the `drill = …` ids in `rules.toml`), a few how-to steps, a round count, an
optional countdown, and a **seeded procedural target generator** (`target(rng, w, h)` → prompt SVG,
answer SVG, optional canvas guide). No pictures to license, endless rounds, deterministic tests.
Only the two contour drills use real references (Endless first, then curated, then a generated
subject if no picture loads).

Scoring is maths only (ADR-0002). Each round renders the answer SVG (or the reference) to an 800×600
white canvas and runs `gradeImages(…, { category: 'fundamentals', strokes })`, then:

| Drills | Round score (1–10) |
|---|---|
| `ghosted-lines` | 40% grader + 60% line maths: ½ straightness + ½ endpoint error |
| `angle-matching` | 40% grader + 60% line maths: 65% angle error + 35% straightness |
| `blind-contour` | `1 + 9 × line_confidence` only (a "looking" exercise; the drawing isn't compared) |
| `five-step-value-scale`, `three-value-thumbnail` | grader with tone-heavy weights (tone 0.45) |
| `silhouette-fill` | grader with silhouette-heavy weights (silhouette 0.5) |
| everything else | grader, fundamentals weights |

Line maths (`drill-metrics.ts`, on the longest non-eraser stroke): straightness = max distance from
the end-to-end chord ÷ chord length (1% → full marks, 8% → none); endpoint error = mean distance of the
stroke's ends to the dots ÷ dot distance (2% → full, 22% → none); angle error = least-squares stroke
direction vs the target, 0–90° (1° → full, 20° → none).

Finished sessions are saved in `localStorage['pp.drills']` as `{ id, at, avg, rounds: number[] }[]`
(last 200; `at` is ms since epoch, scores are 1–10 with one decimal).

## References (how to add pictures)
1. **Fetch candidates** (needs `PIXABAY_API_KEY` in the git-ignored `.env`):
   `pnpm refs fetch animals "horse" --count 8` (`--type illustration` or `--type vector` for anime/cartoon).
   AI-generated, non-G-rated, low-quality and franchise/NSFW-tagged images are skipped automatically.
2. **Review** at http://localhost:5173/dev/references (dev server only). Approve only pictures that are
   safe for all ages, clearly hand-made (check hands, eyes, text), not a franchise character, and good to
   draw from. Give each one a short title. Rejected IDs are remembered so they are never fetched again.
3. Approved metadata lands in `content/references/<category>.json` (committed). **Images are never
   committed**: `apps/web/static/references/` is git-ignored. On a fresh machine or deploy run
   `pnpm refs sync` to download them again.
4. `pnpm refs status` shows counts.
