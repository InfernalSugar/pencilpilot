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

## M5b — Endless references (user request, 2026-11-18)
- **M5b-T01** `crates/ref-service` (Rust/axum): ready-made pool of vetted Pixabay pictures per category × topic × tier, refilled in the background by demand; 24 h search cache, rate budget, parallel downloads, slot back-off; `/api/ref`, `/refimg`, `/api/report` `infra` `content`
- **M5b-T02** `crates/ref-filters`: blank / text-heavy / too busy-faint / pHash duplicates + NSFW guard (open-source model, **safety only**) `content` `legal`
- **M5b-T03** Web: canvas opens instantly, reference fades in (< 1 s), curated fallback, Beginner prefers hand-approved, Report button `web`
- **M5b-T04** Hosting for the Node site + ref-service (see ADR-0004) `infra`

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

## M10 — Studio features (requested 2026-11-19)
Added by the owner after M9. v1.0 (M9-T03) waits until these are in. Order matters: layers come first because tracing, sketch mode and lobbies build on them.

- **M10-T01** Layers, Krita/Photoshop style `web` `brush`
  Layer stack in the paint engine (one surface per layer, composited in WASM): add, delete, duplicate, rename, reorder (drag), show/hide, opacity, lock, alpha lock ("preserve transparency"), merge down, flatten. Blend modes: normal, multiply, screen, overlay, darken, lighten, add. Every layer action is undoable. A layers panel with thumbnails (folds away on phones like the brush panel). Grading, thumbnails and community sharing use the flattened visible layers. Stroke data records which layer each stroke went to. Later: groups and clipping masks.
- **M10-T02** Adjustable canvas and reference size `web` `grader`
  Canvas presets (4:3, 3:4, 1:1, 16:9, A4 portrait/landscape) plus custom size within limits. Zoom, pan, rotate and flip of the view (pinch on tablets, space-drag, keyboard). Reference panel: drag to resize, zoom and pan inside it, flip, or float it as a movable window. The grader fits the reference to the chosen canvas shape and grades at a fixed analysis resolution, so scores stay comparable across sizes.
- **M10-T03** Tracing mode `web`
  The reference (or your own picture, which never leaves the browser) sits on a locked "Tracing" layer inside the canvas at low opacity, with an opacity slider and move/scale/rotate handles. Traced drawings are marked "traced": they get the overlay and tips, but never count toward scores, streaks, progress or the community. Replaces the current Peek toggle.
- **M10-T04** Timed modes `web`
  30 s, 1 min, 2 min, 5 min and 10 min rounds plus a gesture session (a queue of references with a fixed time each, like a figure-drawing class). The canvas locks when time is up and grading starts automatically. Results and progress are tracked per time limit (a 2-minute drawing is judged against your other 2-minute drawings).
- **M10-T05** Sketch mode `grader` `feedback` `web`
  A mode (any category), not a separate category. Rough, hairy, multi-stroke lines are first merged into the lines you meant, then judged on proportions, placement, big shapes and angles only. Design: [ADR-0007](adr/0007-sketch-mode.md).
- **M10-T06** Replay `web`
  Watch any drawing being redrawn stroke by stroke from the recorded stroke data (play, pause, scrub, 1–8× speed), with layer changes, and the grader's tips appearing at the moments they relate to (for example, where proportions first went off). Export as a short video/GIF (later).
- **M10-T07** Daily challenge `web` `server`
  One reference a day, the same for everyone, picked from a calm, beginner-to-medium pool and announced at midnight UTC. Signed-in players get one scored try a day, plus a daily streak; they see how their score compares (percentile) and, with community on, others' drawings after they've submitted their own. Guests can play but don't appear in the results.
- **M10-T08** Private lobbies (draw together) `web` `server`
  Private room joined with an invite link/code (up to 8 people). Players show a display name (signed-in name, or a nickname for guests). The lobby stays open between games: after a game ends, the host starts the next one without anyone leaving. The host can remove people. Strokes travel as stroke data (not pixels) and are redrawn by the same brush engine on every screen. Nothing is stored after the room closes unless a player saves their own drawing. Two modes, chosen when the lobby is set up (and changeable between games):
  - **Competition.** Setup: number of rounds and the time limit per round (plus category and difficulty for the reference). Each round, every player gets the same reference and draws on their own canvas; others' canvases stay hidden during the round so nobody copies. When time runs out, all canvases lock and are graded. Then the drawings are revealed one at a time with who made them; players can 👍 or 👎 each one (for fun, it doesn't decide anything). The round winner is the highest grader score. Game winner: the highest total score over all rounds (tie-break: most round wins). A scoreboard shows between rounds, and "Play again" starts a new game in the same lobby.
  - **Free draw.** Everyone draws together on one shared canvas and sees each other's strokes live as they draw, with name-tagged cursors. A reference is optional: any player can show one (from the site's references) beside the canvas or remove it. Each player gets their own layer, so undo only affects your own strokes. No scoring; anyone can save the canvas as an image.

## M11 — Drawing tools (requested 2026-11-19)
Procreate / Photoshop-style tools. Groups 1 and 2 first (owner's choice).

**Group 1: Everyday tools**
- **M11-T01** Eyedropper `web`
  Pick a colour from the canvas: an eyedropper tool, Alt+click while drawing, or press-and-hold with a finger. Samples what you see (all visible layers).
- **M11-T02** Fill / paint bucket (+ ColorDrop) `brush` `web`
  Fill an area with the current colour, with tolerance and a small grow so fills tuck under anti-aliased lines; samples all visible layers or just the current one; drag a colour onto the canvas to fill (Procreate ColorDrop). Respects selections. Undoable.
- **M11-T03** Stroke smoothing (StreamLine) `web`
  A smoothing slider (0–100) that steadies wobbly strokes with a lazy-brush stabiliser; the smoothed line is what's recorded (grading, replay).
- **M11-T04** QuickShape `web`
  Draw a line, circle/ellipse, rectangle or polyline and hold at the end: it snaps to the clean shape, keeping brush and pressure.
- **M11-T05** Colour panel `web`
  Colour wheel / HSB square, recent colours, saved palettes (plus a few starter palettes), hex entry.
- **M11-T06** Touch gestures `web`
  Two-finger tap = undo, three-finger tap = redo (as in Procreate), alongside the existing pinch/rotate.

**Group 2: Fixing proportions**
- **M11-T07** Selections `brush` `web`
  Rectangle, lasso and magic-wand selections (add/subtract), shown with marching ants; painting, fill and clear stay inside the selection; select all / deselect / invert.
- **M11-T08** Transform `brush` `web`
  Move, scale, rotate and flip the selection or the whole layer with handles; live preview; commit or cancel; undoable.
- **M11-T09** Liquify `brush` `web`
  A push (forward warp) brush to nudge parts of a layer into place, with size and strength; undoable as one step.

**Group 3: Drawing guides**
- **M11-T10** Drawing guides `web`
  Grid, perspective guides (1/2/3 point) and symmetry (strokes mirrored live, vertical / horizontal / quadrant / radial).
- **M11-T11** Ruler `web`
  A movable ruler; strokes near it snap straight along its edge.

**Group 4: Layers, advanced**
- **M11-T12** Clipping masks `brush` `web`
  Clip a layer to the one below (paint only inside it).
- **M11-T13** Layer masks `brush` `web`
  Hide parts of a layer without erasing (paint black/white on the mask).
- **M11-T14** Layer groups `brush` `web`
  Group layers; group visibility, opacity and blend.

**Group 5: Adjustments**
- **M11-T15** Adjustments and gradient `brush` `web`
  Hue/saturation, brightness/contrast, Gaussian blur, recolour a layer; gradient fill (linear/radial).

**Group 6: Files and brushes**
- **M11-T16** Import a picture as a layer `web`
  Bring an image from the device in as a new layer (stays in the browser).
- **M11-T17** Layered export `web`
  Export with layers (PSD) as well as PNG.
- **M11-T18** Brush editor and pressure curve `brush` `web`
  A simple brush studio (spacing, jitter, pressure → size/opacity) and a personal pen pressure curve.
