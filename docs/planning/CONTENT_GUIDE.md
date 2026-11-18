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
