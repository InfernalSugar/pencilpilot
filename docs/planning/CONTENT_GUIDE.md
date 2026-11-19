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
number = "ratio_pct"          # how {n} is computed: ratio_pct | abs_pct | pct | deg | rad_deg | count
dir_pos = "wide"              # optional words for {dir} by the value's sign
dir_neg = "narrow"
overlay = "bbox"              # what "Show me" highlights: bbox | offset | grid | missing | extra
lesson = "measuring-with-units"
drill = "block-in-boxes"
categories = ["real-life"]    # optional: only for these categories
modes = ["sketch"]          # optional: "finished" / "sketch" (line and shading rules skip sketches by default)
```
The engine (`crates/feedback`) shows the top 3 fixes (one per skill) plus praise for the strongest skill.
Content is embedded at build time, so run `pnpm wasm` after editing.

- `above`/`below` are strict (`>`/`<`); `min`/`max` are inclusive. `rad_deg` turns radians into
  degrees (`curvature_jitter`), `count` prints the value as-is (`stroke_count`).
- **Graded bands.** Split one mistake into a mild and a strong rule that never overlap: the mild one
  uses `above = 1.15` + `max = 1.3`, the strong one `above = 1.3` (for `below`: `below = 0.85` +
  `min = 0.7`, then `below = 0.7`). Give the mild band the lower severity and gentler wording.
- **Category overrides.** To tune a rule for a category (anime, cartoon, real-life, animals,
  environment, fundamentals), add a rule with `categories = ["<that one>"]` and list every *other*
  category on the generic rule, so both can't fire. Line-art categories (anime, cartoon) are stricter
  on precision and line quality; cartoon is looser on size, placement and proportions; environment
  uses composition, perspective and atmospheric-perspective wording; real life and environment allow
  more retracing (shading).
- Rules on linked measurements must not overlap either: `smoothness = exp(−curvature_jitter ÷ 0.35)`
  and `speed_consistency = exp(−speed_cv)`.
- `lesson` and `drill` must be ids from the M8 plan (`docs/planning/M8_CONTENT_PLAN.md`).
- `cargo test -p pencilpilot-feedback` checks all of this: one rule per (measurement, category,
  value), bands without gaps, links, tip length and placeholders, and per-category sample drawings.

## Tips (`content/feedback/tips.toml`)
```toml
[tip.too-big]
title = "Your drawing is {n}% too big"
variants = ["…{n}%…", "…"]     # 2–4 wordings; picked per attempt so repeats differ
```
- Use `{n}` (the rule's number) in every variant and `{dir}` (only if the rule has dir words).
  Exception: `ink_coverage` rules, whose number is too small to quote.
- Structure: **what happened** (with the number) → **why it happens** → **one concrete fix**. At most 60 words.
- Titles may use `{n}`, but the lesson and progress pages show titles with the number removed, so
  keep them readable without it (no `({n}%)` endings).

## Praise
`[tip.praise-<skill>]` for placement, outline, silhouette, tone, line_confidence, proportions: shown for the strongest skill at ≥ 70%.
An optional `[tip.praise-<skill>-<category>]` (e.g. `praise-outline-anime`) replaces it for that
category; the item id stays `praise-<skill>`.

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
30 drills (13 from M6, 17 from M8; ids fixed by `docs/planning/M8_CONTENT_PLAN.md` and linked from
`rules.toml`). Each has a fixed id, a skill (the `/drills` index groups by it), a few how-to steps, a
round count, an optional countdown, a default brush, optional grader weights / score curve, and a
**seeded procedural target generator** (`target(rng, w, h)` → prompt SVG, answer SVG, optional canvas
guide). No pictures to license, endless rounds, deterministic tests. The M8 generators live in
`lib/drill/targets.ts`, the shared PRNG/SVG helpers in `lib/drill/svg.ts`. Only `contour-in-one-minute`,
`blind-contour` and `gesture-30-second` use real references (Endless first, then curated, then a
generated subject if no picture loads).

Adding a drill: write a generator that builds its picture in model units and fits it with `fitter`
(so every seed stays on the page), namespace any `id=` with `uid()` (many previews share one page), add
the def with weights that add up to 1, and document its scoring below. The unit tests check every drill
over 25 seeds for well-formed SVG, determinism and variety.

Scoring is maths only (ADR-0002). Each round renders the answer SVG (or the reference) to an 800×600
white canvas and runs `gradeImages(…, { category: 'fundamentals', strokes, weights, curve })`. The
canvas guide (dots, horizon, frames, face guides) is an overlay and is never graded. Weights below are
placement / outline / silhouette / grid / tone / SSIM / line confidence (keypoints are always 0 in
drills; "default" means the fundamentals weights 0.08 / 0.40 / 0.20 / 0.12 / 0.04 / 0.04 / 0.12 + 0.20
keypoints).

| Drill | Skill | Graded against | Round score (1–10) |
|---|---|---|---|
| `ghosted-lines` | line confidence | the line between the dots | 40% grader (default) + 60% line maths: ½ straightness + ½ endpoint error |
| `angle-matching` | proportions | the line copied on the right | 40% grader (default) + 60% line maths: 65% angle error + 35% straightness |
| `ellipse-pages` | line confidence | the ellipse | grader, default weights |
| `block-in-boxes`, `four-corner-marks`, `envelope-shapes` | placement | boxes / corner ticks / envelope | grader, default weights |
| `silhouette-fill` | big shapes | the filled blob | grader, silhouette-heavy (.10/.15/.50/.15/.10/0/0) |
| `grid-copy` | big shapes | the polygon | grader, default weights |
| `five-step-value-scale`, `three-value-thumbnail` | tone | the grey fills | grader, tone-heavy (.05/.05/.20/.10/.45/.15/0) |
| `head-units` | proportions | the mannequin | grader, default weights |
| `contour-in-one-minute` | outlines | the reference picture | grader, default weights |
| `blind-contour` | outlines | — | `1 + 9 × line_confidence` only (a "looking" exercise; the drawing isn't compared) |
| `gesture-30-second` | line confidence | the reference picture (fallback: a stick figure on a C/S line of action) | grader, line-confidence-heavy (.15/.10/.25/.10/0/0/.40) on a **loose curve** (floor 0.30, pass 0.75): a 30-second gesture can't match a photo's outline |
| `line-weight-pressure` | line confidence | one tapered stroke, thick end at the big dot | 40% grader (.15/.35/.20/.10/.10/0/.10) + 60% line maths: 50% taper + 25% straightness + 25% endpoint error. Without pressure data (a mouse reports a flat 0.5) the taper can't be read, so it is 50% straightness + 50% endpoints and the how-to says so |
| `ellipse-in-boxes` | line confidence | a true perspective ellipse (a circle on a ground or wall plane through a pinhole camera) | grader, outline-heavy with line confidence (.15/.45/.10/.10/0/0/.20) |
| `face-proportion-grid` | proportions | front-view face (eyes halfway, hairline/brow/nose/chin thirds); canvas shows crown and chin ticks only | grader, outline + placement (.20/.35/.15/.20/0/.05/.05) |
| `eye-placement` | proportions | the two eyes only; canvas shows the anime head, centre line and eye line | grader, placement-heavy (.35/.20/.10/.25/.05/.05/0) |
| `hand-mitten-shapes` | proportions | palm box + finger mitten with tubes + thumb wedge | grader, outline-heavy (.10/.35/.25/.15/0/.05/.10) |
| `negative-space-shapes` | big shapes | the gaps (frame minus object) filled dark; canvas shows the frame | grader, silhouette-heavy (.10/.10/.40/.20/.15/.05/0) |
| `animal-bean-construction` | big shapes | body bean, chest/hip/head circles, jointed legs (dog, cat, horse or bear) | grader, silhouette + outline (.10/.30/.30/.15/0/.05/.10) |
| `exaggerate-shapes` | big shapes | the object with its proportions pushed as instructed | **creative exercise** (labelled in the UI): silhouette + line confidence (.05/.05/.50/.05/0/0/.35) on a **loose curve** (floor 0.20, pass 0.70), since there's no single right answer |
| `cube-in-perspective` | placement | the cube's visible edges; canvas shows horizon + VP | grader, outline + placement (.25/.40/.10/.15/0/0/.10) |
| `two-point-boxes` | placement | the box's visible edges; canvas shows horizon + both VPs | same as the cube |
| `thumbnail-composition` | placement | a 3-value scene on the rule of thirds; canvas shows frame + thirds | grader, placement + tone (.25/.05/.15/.25/.25/.05/0) |
| `form-shading-spheres` | tone | light, halftone, core shadow, reflected light, cast shadow (5 flat greys) | grader, tone-heavy (.05/.05/.15/.10/.50/.15/0) |
| `cast-shadow-blocks` | tone | block faces in 3 values + darker cast shadow (projected from the light) | grader, tone-heavy (.05/.10/.15/.15/.40/.15/0) |
| `hatching-patches` | tone | 4–5 patches of hatching, each denser; canvas shows the boxes | grader, tone-heavy with line confidence (.05/0/.10/.15/.50/.10/.10) |
| `soft-edge-blend` | tone | a smooth dark → white gradient; canvas shows the box | grader, tone-heaviest (.05/0/.15/.10/.55/.15/0) |
| `atmospheric-layers` | tone | 4 hill layers, near dark → far light; canvas shows the frame | grader, tone-heavy (.05/.05/.10/.20/.50/.10/0) |

Line maths (`drill-metrics.ts`, on the longest non-eraser stroke): straightness = max distance from
the end-to-end chord ÷ chord length (1% → full marks, 8% → none); endpoint error = mean distance of the
stroke's ends to the dots ÷ dot distance (2% → full, 22% → none); angle error = least-squares stroke
direction vs the target, 0–90° (1° → full, 20° → none). Taper (`taperMetrics`): the stroke is turned
so it starts at the end nearest the thick dot and split by length; drop = (mean pressure of the first
quarter − mean of the last quarter) ÷ first-quarter mean (60% → full, 5% or less → none, so drawing
it the wrong way round scores 0); steadiness = share of the 5-point-smoothed pressure changes that go
down. Taper score = 75% drop + 25% steadiness. A pressure range under 0.03 counts as "no pressure data".

Finished sessions are saved in `localStorage['pp.drills']` as `{ id, at, avg, rounds: number[] }[]`
(last 200; `at` is ms since epoch, scores are 1–10 with one decimal).

## References (how to add pictures)
1. **Fetch candidates** (needs `PIXABAY_API_KEY` in the git-ignored `.env`):
   `pnpm refs fetch animals "horse" --count 8` (`--type illustration` or `--type vector` for anime/cartoon).
   Real life, animals and environment are **real photos only** (any tier): the importer refuses other
   types there, and a test checks the committed manifests. Beginner-friendliness comes from the tier.
   AI-generated, non-G-rated, low-quality and franchise/NSFW-tagged images are skipped automatically.
2. **Review** at http://localhost:5173/dev/references (dev server only). Approve only pictures that are
   safe for all ages, clearly hand-made (check hands, eyes, text), not a franchise character, and good to
   draw from. Give each one a short title. Rejected IDs are remembered so they are never fetched again.
3. Approved metadata lands in `content/references/<category>.json` (committed). **Images are never
   committed**: `apps/web/static/references/` is git-ignored. On a fresh machine or deploy run
   `pnpm refs sync` to download them again.
4. `pnpm refs status` shows counts.
5. `pnpm refs sort --dry-run` checks every manifest with the local picture sorter (ADR-0009: a CLIP
   model that only sorts reference pictures, never grades drawings; get it with
   `crates/ref-filters/fetch-model.sh clip`). Without `--dry-run` it re-files pictures in the wrong
   category/topic and moves bad ones to `rejected.json`. Topic `prompts` and the `_sorting` block in
   `content/topics.json` drive it. It also rejects pictures that are not one drawable subject (sheets,
   icon sets, patterns, text): topics are `"subject": "single"` by default or `"scene"` for wide views,
   and a topic's `avoid` prompts list what it must not show.
