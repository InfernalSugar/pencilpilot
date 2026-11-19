# ADR-0007: Sketch mode

- Status: accepted and built (2026-11-19), M10-T05
- Builds on: ADR-0002 (no AI grading), M10-T01 (layers), M10-T04 (timed modes)

## Problem

A rough sketch looks nothing like a clean drawing to today's grader, even when
it is a good sketch:

- **One intended line is drawn as 3–6 hairy, overlapping strokes.** The outline
  matching (`outline_precision`, `outline_distance`) counts the extra strokes as
  wrong lines.
- **Construction lines** (boxes, centre lines, ovals) are deliberate, but they
  look like "extra lines" too.
- **Stroke metrics punish the sketching habit**: `retrace_ratio`,
  `line_confidence`, `smoothness` and `stroke_count` reward one clean pass.
- **Detail and shading aren't there yet.** `ssim`, `tone` and `shading` expect a
  finished drawing.

So a sketch needs a different reading of the same maths, not a separate grader.

## Decision

### 1. A mode, not a category

Sketching is how you draw, not what you draw: you can sketch an anime face or a
landscape. The draw setup gets a **Finished / Sketch** switch for every category,
and Sketch pairs naturally with the timed modes (1, 2 and 5 minutes). A short
"Sketching" drill set goes into Fundamentals for practising the skill on its
own.

### 2. Merge hairy strokes into the line you meant (stroke consolidation)

We already record every stroke as points. Before grading, strokes that run
**close and roughly parallel** are grouped and replaced by **one centre line**:

1. Resample every stroke at fixed spacing (about 4 px).
2. Two stroke pieces belong together when, over most of their overlap, they are
   within a few pixels of each other (scaled to the canvas, about 1.2% of its
   diagonal) and their directions differ by under ~20°.
3. Grouping is transitive (union–find), so a bundle of six scratchy passes
   becomes one group.
4. Each group becomes a single centre line: walk along the group's main
   direction and average the points across it, then smooth lightly.

This is plain geometry. It is in the same family as published stroke
consolidation methods, and uses no machine learning (ADR-0002 still holds). It
runs in Rust in the grader and is fast enough in the browser: hundreds of
strokes take a few milliseconds with a spatial grid.

A side benefit: the number of passes per line ("you went over this line 5
times") becomes a measurement we can give tips on.

### 3. Construction lines go on their own layer

With layers (M10-T01), Sketch mode opens with two layers: **Construction**
(light blue, low opacity) and **Sketch**. The Construction layer is never
graded as outline, so boxes and centre lines cost nothing. Its only use is a
small bonus check: do the construction shapes sit roughly where the subject's
big shapes are? Students who sketch on one layer can still tag strokes as
construction afterwards (select, then "mark as construction").

Without a construction layer, faint strokes (low pressure and opacity) would be
treated as construction automatically. That's a fallback only, because pressure
is unreliable with a mouse.

### 4. Grade the big picture, at a coarse scale

The consolidated sketch is graded on what a sketch is for:

| Measurement | Sketch weight | Notes |
|---|---|---|
| Proportions (`scale_ratio`, `aspect_error`) | 30% | unchanged maths |
| Placement (`placement`, offsets) | 15% | unchanged |
| Big shapes (`silhouette`, coarse `grid`) | 25% | grid at 1/4 resolution, outlines closed with morphology first |
| Angles / gesture (`angle_error`, dominant directions) | 20% | main line directions from the structure tensor at a coarse scale |
| Outline at a coarse scale (`outline_distance`) | 10% | wider tolerance band (about 3× the finished one) |
| Line confidence, smoothness, retraces | 0% | shown as info, never scored |
| Tone, shading, SSIM | 0% | not expected in a sketch |

These weights live in the grader config like the category ones, and calibration
(#37) tunes them.

### 5. Sketch-specific tips

New rules in `content/feedback/rules.toml` with `mode = "sketch"`. For example:
"Block in the big shapes first", "Your head-to-body ratio is off: check it before
adding detail", "Lots of passes on the same line: try one confident ghosted
stroke" (gentle, never scored). Finished-drawing tips about shading and line
quality are skipped in sketch mode.

## How we'll know it works

Extend the synthetic benchmark (`calibrate synthetic`) with sketch versions of
each reference:

- every line drawn as 3–6 jittery offset passes, plus construction boxes
- the good sketch must score close to the clean copy (target: within 1 point,
  and at least 8/10)
- real mistakes (wrong size, shifted, missing parts) must still score clearly
  lower, with Spearman ρ ≥ 0.9 across mistake levels, as for finished drawings
- then hand-rated sketches on `/dev/rate`, as in #37

If consolidation alone isn't enough on the benchmark, a fallback is to grade the
sketch's **density map**: blur the ink heavily and treat it as a soft
silhouette. This is less precise, but it can't be thrown off by hairy lines.

## Consequences

- Sketch mode depends on layers (for construction) but can ship a first
  version without them, using the faint-stroke fallback.
- Scores in sketch mode are kept apart from finished-drawing scores on the
  progress page, as with timed modes.

## Results (synthetic benchmark, `cargo test -p pencilpilot-grader sketch_mode`)

A box-and-circle subject drawn as a rough sketch: every line is made of 4–5
scratchy passes about 4 px apart that overshoot the ends, plus guide lines.

| Drawing | Sketch mode | Finished mode |
|---|---|---|
| clean single-pass copy | 10.0 | |
| rough multi-pass copy (guides on the Construction layer) | **10.0** | 4.4 (guides included) |
| rough, too big and shifted | 6.8 | |
| size/place mistake ×1.0 / 1.15 / 1.3 / 1.5 | 10 / 10 / 7.9 / 5.5 | |
| head too big ×1.0 / 1.25 / 1.5 / 1.8 | 10 / 10 / 7.8 / 5.2 | |

Small mistakes (15–25%) are still forgiven, which suits a sketch. Human
ratings (#37) can tighten the curve.

## As built

- `GradeOptions.mode = "sketch"`: strokes are merged by a mean shift
  across the line (`crates/grader/src/sketch.rs`). Points within ~1.2% of the
  diagonal and within ~20° of each other's direction are pulled together.
  The merged lines are redrawn clean, masked by the real ink so erased parts
  stay erased, then graded with σ × 3 and `SKETCH_WEIGHTS`.
- New measurements: `angles` (orientation histograms, cosine) and `passes`.
- The web app opens sketch mode with a Construction layer (blue, 50%) under a
  Sketch layer. Construction is left out of the graded image and strokes.
- Feedback rules have `modes`. Rules on line and shading measurements skip
  sketches by default; `sketch-many-passes` (gentle) and `sketch-angles-off`
  are sketch-only.
- Attempts store `mode = "sketch"`. Progress can show sketches on their own,
  and sketches don't feed the skill radar.
