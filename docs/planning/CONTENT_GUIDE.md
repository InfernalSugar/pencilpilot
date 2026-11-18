# Content guide

The voice is **Fun, kind, specific**: a cheerful coach, not a judge.

## Rules (`content/rules/*.toml`)
```toml
[[rule]]
id = "head-too-large"
measurement = "head_body_ratio_error"   # produced by the grader
above = 0.20                            # fire when the head is 20%+ too big
severity = 4                            # 1 (minor) – 5 (major); used for ranking
tip = "head-too-large"                  # tips/head-too-large.md
lesson = "measuring-proportions"
drill = "head-units-gesture"
```

## Tips (`content/tips/<id>.md`)
- 2–4 variants separated by `---`, so repeat users don't read the same sentence.
- Use placeholders with real numbers: `{percent}`, `{degrees}`, `{direction}`.
- Structure: **what happened** (with the number) → **why it happens** → **one concrete fix**.
- Keep each variant under ~60 words. Beginners get the simple variants; `level = "advanced"` variants are optional.

## Praise
Each measurement also has a "nice job" line, used for the student's best area.

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
