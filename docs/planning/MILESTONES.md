# Milestones

Each heading is a GitHub milestone, and each bullet is an issue titled `Mx-Tyy — …`.

## M0 — Foundation (≈2–3 days)
- **M0-T01** Create the public repo, `dev` default branch, squash-only merges, branch protection on `main` `infra`
- **M0-T02** Monorepo setup (pnpm workspace + cargo workspace), MIT licence, README `infra`
- **M0-T03** CI: Rust tests and clippy, wasm-pack build, web lint/check/unit tests, Playwright smoke test `infra`
- **M0-T04** Planning docs: this folder, ADR-0001..0003, LICENSING, CONTENT_GUIDE `infra`
- **M0-T05** Create GitHub milestones, labels and issues `infra`

## M1 — "Fun" design system and mockups (≈1 week, in parallel with M2)
- **M1-T01** Fun-theme mockup as a claude.ai Artifact: bright palette, chunky rounded shapes, playful type, a mascot (a little pencil pilot with goggles), bouncy micro-animations, confetti, light and dark modes `design`
- **M1-T02** Mock up key screens: home/category picker, draw screen, results (score dial, overlay, tips), lesson page, progress page `design`
- **M1-T03** Turn the approved mockup into design tokens + Svelte components in `apps/web/src/lib/ui` `design` `web`

## M2 — Canvas and drawing core (≈2–3 weeks)
- **M2-T01** Fixed-size canvas: pointer events, pen pressure/tilt, smoothing, eraser, undo/redo, clear `web`
- **M2-T02** Record stroke data (points, pressure, timestamps) for stroke-quality grading `web`
- **M2-T03** Draw screen layout: reference beside the canvas, optional timer, submit `web`
- **M2-T04** Export drawing bitmap + stroke data to the grader `web` `grader`
- **M2-T05** Brush engine: [hokusai](https://github.com/reearth/hokusai) (MIT/Apache, pure-Rust port of libmypaint, the engine Krita also ships) in WASM + the mypaint-brushes pack (CC0, 100+ brushes: pencils, inks, markers, charcoal, airbrush, watercolour, smudge…); brush picker grouped by category, favourites `web` `legal`

## M3 — Grader v1, Rust → WASM (≈2–3 weeks)
- **M3-T01** Image preparation: grayscale, normalise, auto-alignment (scale/translate/rotate) `grader`
- **M3-T02** Silhouette overlap (IoU), grid comparison, light/dark comparison, SSIM `grader`
- **M3-T03** Outline matching (edge map + distance transform + chamfer distance) `grader`
- **M3-T04** Keypoint matching (Procrustes) + the "tap your points" step in the UI `grader` `web`
- **M3-T05** Stroke-quality metrics: smoothness, retrace ratio, speed `grader`
- **M3-T06** Weighted score → 1–10 curve with a passing threshold; per-category weight config `grader`
- **M3-T07** WASM bindings + web integration; grading in under 500 ms on a mid-range laptop `grader` `web`

## M4 — Feedback engine and results screen (≈2 weeks)
- **M4-T01** Rule format (TOML), rules engine, ranking (top 2–3 problems + one praise) `feedback`
- **M4-T02** Tip templating with real numbers + variant rotation `feedback`
- **M4-T03** Overlay renderer: fade slider, difference heatmap, worst grid cells, arrows/boxes/ghost outline `web`
- **M4-T04** First 20 rules + tips (proportions, angles, placement, line confidence, contrast) `content`
- **M4-T05** Results screen in the Fun style (score reveal animation, confetti at ≥8) `web` `design`

## M5 — References pipeline + MVP launch (≈2 weeks) → first public prototype
- **M5-T01** `tools/ref-importer`: Pixabay, Unsplash, Pexels, OpenGameArt (CC0/CC-BY), storing licence and attribution `content` `legal`
- **M5-T02** Safety: the sources' SafeSearch + a manual approval queue; flag and skip AI-made images `content` `legal`
- **M5-T03** `tools/ref-annotator` + keypoint marking for the first set `content`
- **M5-T04** Seed 3 categories (Real life, Animals, Anime), about 50 references each `content`
- **M5-T05** Attribution display on the site + licensing check script in CI `web` `legal`
- **M5-T06** Deploy a static preview (guest mode, browser storage); ADR-0004 hosting `infra`

## M6 — Lessons, drills, progress (≈4–6 weeks)
- **M6-T01** Lesson pages + 10 mini-lessons; each rule links to a lesson + drill `content` `web`
- **M6-T02** Drill mode / "Fundamentals": timed figures, lines, ellipses, perspective boxes `web` `grader`
- **M6-T03** Mistake history + recurring-pattern detection ("7 of your last 10…") `feedback`
- **M6-T04** Skill radar chart, streaks and badges `web` `design`
- **M6-T05** Adaptive reference picking (more of what the student struggles with) `feedback`

## M7 — Accounts and persistence (≈2–3 weeks)
- **M7-T01** Server routes + SQLite (Drizzle), email/OAuth sign-in `web` `infra`
- **M7-T02** Move guest progress into an account `web`
- **M7-T03** Privacy policy, data deletion, rate limits `legal` `web`

## M8 — Calibration and content scale-up (ongoing, ≈4–8 weeks)
- **M8-T01** Collect 20–30 labelled drawings per category; `tools/calibrate` tunes the weights `grader`
- **M8-T02** Grow to 75+ rules, 30 lessons, 30 drills; add the Cartoon + Environment categories `content`
- **M8-T03** Rate-the-feedback button (👍/👎 per tip) to find weak rules `feedback` `web`

## M9 — Polish and release v1.0
- **M9-T01** Accessibility, tablet/phone layouts, performance pass `web`
- **M9-T02** Optional community critique (rubric + "which is closer?" voting) behind a flag `web`
- **M9-T03** v1.0 release to `main` `infra`
