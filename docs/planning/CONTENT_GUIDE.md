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
