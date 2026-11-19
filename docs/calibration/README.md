# Calibration (M8-T01)

Two tools in `tools/calibrate`:

1. **Synthetic benchmark**: `cargo run --release -p calibrate -- synthetic [--per 6]`
   writes `docs/calibration/synthetic.md`. A "perfect" student drawing is made from each approved
   reference (its strongest edges as line art); then known mistakes are added at levels 0–3
   (smaller, shifted, parts missing, wobbly, stray scribbles). A fair grader scores level 0 high
   and falls as mistakes grow. `CAL_DEBUG=1` prints every measurement for the perfect drawings.
2. **Human labels**: rate your own drawings on the dev page `/dev/rate` (each saved from the
   results page with *Save for calibration*). Then `cargo run --release -p calibrate -- fit` fits
   the score curve (floor/pass) per category to your 1–10 ratings.

## Findings
- **Baseline (2026-11-19)**, `synthetic-baseline.md`: perfect line copies of photos scored only
  2.8 (real-life) / 3.7 (animals) / 6.5 (anime). Causes, from `CAL_DEBUG`:
  - tone/SSIM carry a lot of weight but outline drawings have no shading;
  - outline precision compares against only the strongest reference edges, so real detail lines
    count as "extra";
  - placement uses bounding boxes that one stray background edge can stretch.

## After: grader fairness for line drawings (M8-T01)

Report: `synthetic.md` (`--per 6`, 18 references). **0 rows flagged** (was 15).

| Category | L0 before | L0 after | ρ after (missing / scribble / wobble / scale / shift) |
|---|---|---|---|
| animals | 3.7 | **9.3** | −0.87 / −0.69 / −0.81 / −0.64 / −0.72 |
| anime | 6.5 | **9.7** | −0.90 / −0.80 / −0.89 / −0.75 / −0.68 |
| real-life | 2.8 | **9.1** | −0.90 / −0.78 / −0.86 / −0.71 / −0.68 |

Every family ends well below L0 (L3: missing 1.0–2.2, scribble 2.9–3.8, wobble 3.3–5.8). Lowest
single perfect copy: 7.6 (`px-390555`). Golden scribbles and blanks still score 1.0. Grading an
800×600 drawing takes ≈ 45 ms in release.

What changed in `crates/grader` and why (from `CAL_DEBUG`; L0 means animals / anime / real-life when
that change alone is switched off, measured while developing):

1. **Letterbox frame ignored in the drawing too** (`edges::letterbox_frame`, `without_frame`). The
   grader already dropped the edge between the white bars and a letterboxed photo from the
   reference, but a drawing that traces the picture's frame was then penalised for it (extra lines,
   a box stretched to the whole picture: `scale_ratio` up to 1.49). Now ink along that band is
   ignored as well: tracing the frame neither helps nor hurts. Without it: 5.4 / 7.7 / 5.4. Caveat:
   the benchmark's line art always traces the frame, so this is a large part of the jump. Once a
   drawing is scaled or shifted, the frame it traced leaves the band and counts as extra lines,
   which is why "scale"/"shift" now fall faster on letterboxed references. On references that fill
   the canvas (most anime ones) 85 % scale still scores ≈ 9.
2. **Tone and SSIM count in proportion to shading.** New measurement `shading` = share of the ink in
   filled areas (ink deeper than the outline band). Perfect line art measures 0.00–0.08; a shaded
   copy 0.9+. Tone and SSIM weights are multiplied by `clamp((shading − 0.10) / 0.25, 0, 1)`: zero for
   line art, full from 0.35. Wrong shading is still punished (golden `tone_counts_in_proportion_to_shading`).
   Without it: 7.6 / 8.7 / 5.8.
3. **Precision against a detail edge map.** `reference_edge_maps` returns the strict map (unchanged
   thresholds, used for recall, silhouette and grid content) and a denser *detail* map (hysteresis
   from 10 % of the strongest edge, same absolute floor). Drawn lines are compared with the detail
   map (`outline_precision`, the forward half of `outline` and `grid`, and alignment), so correct
   fur/fold/feature lines are not "extra". Perfect copies' precision: 0.18–0.90 → 0.87–0.92.
   Without it: 8.7 / 9.3 / 8.0. Scribbles still land far from both maps.
4. **Recall weighted by edge strength.** Missing a strong edge costs more than missing a faint one.
   Without it: about −0.4 on L0 in each category.
5. **Placement compares the subject.** The reference box is now the box of the reference edges near
   the *aligned* drawing (within 3 σ), weighted by edge energy (strength²) and trimmed by 2 % of that
   energy per side. Background clutter the student rightly left out (and single stray edges) can't
   stretch it; a drawing that is too small, shifted or squashed is still caught because the drawing's
   own box is measured as drawn. Perfect copies with placement < 0.5: 3 → 1.

`DEFAULT_WEIGHTS` and the `ScoreCurve` were **not** changed.

### Rules whose thresholds may need a look (`content/feedback/rules.toml`)
- `no-shading` / `flat-shading` (`contrast_ratio`) and `lights-and-darks-misplaced` (`tone`): values
  are unchanged, but tone no longer lowers the score of line art. These tips will still fire on every
  pure line drawing in real-life/animals/environment; consider gating them on the new `shading`
  measurement (e.g. only when `shading` ≥ 0.1, or as a gentle "try adding shading" tip).
- `extra-lines` (`outline_precision` < 0.5): precision is now against the detail map, so typical
  values for good drawings rose (≈ 0.9); 0.5 now means clearly wrong lines. Probably fine; could rise.
- `missing-parts` (`outline_recall` < 0.55): strength-weighted, typical good values 0.73–0.95.
- `one-area-off` (`worst_cell` < 0.45): cells are now scored against the detail map; on perfect
  copies the median rose from 0.02 to ≈ 0.6 (range 0.01–0.74), so it fires on fewer good drawings.
- `too-big` / `too-small` / `wrong-width` / `off-*` (`scale_ratio`, `aspect_error`, offsets): now
  relative to the part of the reference the drawing depicts. Leaving parts out no longer reads as
  "too small" (that's `missing-parts`). Perfect copies: `scale_ratio` 1.01–1.25 (median ≈ 1.05; the
  2 % trim of the reference box reads a few percent large), so the 0.85 / 1.15 thresholds still fit.

## Follow-up: shading tips are gated
After the fairness fix, line art isn't marked down for missing shading, so tips that *judge*
shading (flat, too dark, values misplaced, edges, …) now require `shading ≥ 0.1` via the
engine's new `gate` field. The gentle "add some shading" nudges (`no-shading`,
`scene-no-shading`) still appear for line art.

## Relevance gate: drawings that don't depict the reference (2026-11-19)

**Problem.** Dense scribbles over an anime picture scored 4.2 with "big shapes 81 %, light & dark 69 %"
and the praise "The big shapes are right". Silhouette IoU, the blurred tone map and chamfer recall all
reward *any* mass of ink in the right area.

**Measure** (`crates/grader/src/relevance.rs`, measurements `structure` and `relevance`). Each image
becomes a field of local line directions (doubled-angle structure-tensor vectors, scaled by
coherence) on the drawing's lines and on the reference's edges (weighted by edge strength), blurred
by the outline tolerance σ and correlated over the area near the drawing. Copies, even rough or
partial ones, line up (≈ 0.6–1); scribbles and hatching don't (≈ 0), because unrelated directions
cancel out. `relevance` = smoothstep of that correlation from 0.33 to 0.55. Below 0.5 the grade gets
the flag `unrelated`; the similarity is blended towards the one that scores ≈ 1.7 (order kept, so
unrelated drawings land at 1.7–1.9). The feedback rule `not-the-reference` is *exclusive* like the
blank-canvas one: no other tips and no praise. Sketch mode uses the same tolerance (a wider blur lets
random lines line up by chance). Value drills turn the gate off (`relevanceGate: false`). A fully
painted drawing with no lines left is not judged. ≈ +5 ms per grade (34 ms total at 800×600, release).

Candidates tried and dropped: oriented chamfer precision/recall (cos 2Δθ) separated worse on real
references (genuine perfect copies down to 0.24); mean-centred correlation hurt busy line art (anime
copies 0.98 → 0.78); adding the drawing's tonal edges (for painted drawings) diluted hatched shading;
reference-coherence weighting and wider blurs changed little.

**Fixtures** (`crates/grader/tests/relevance.rs`, `--nocapture` prints the table): 4 references
(stick-figure photo, shaded disc, textured photo, anime bust), each with good copies, rough attempts
(wobble 10–18 px, shifted + small, big + wobbly, half missing, short lines, hairy sketch, shaded) and
unrelated drawings over the same area (dense / sparse scribbles, scribbles inside the silhouette,
hatching, cross-hatching, a house, a car), graded finished and as sketches (with strokes):

| Group | Finished: before → after | Sketch: before → after |
|---|---|---|
| Good (8) | 10.00 → 10.00 | 10.00 → 10.00 |
| Rough (36) | 8.78 (1.0–10) → 8.78, none changed | 8.63 (1.0–10) → 8.63, none changed |
| Unrelated (36) | 5.44 (1.7–8.4) → 2.41 (1.7–8.4) | 7.89 (1.0–10) → 2.45 (1.0–9.5) |

The user’s case (dense scribbles over the anime bust): 6.7 → 1.8 finished, 8.5 → 2.1 sketch.

**Real references** (`cargo run --release -p calibrate -- relevance`, report `relevance.md`):
18 references × (5 mistake families × levels 0–2 + 7 unrelated drawings), finished and sketch.
Genuine attempts: 5 of 540 lowered by ≥ 0.1 (all on one dark anime picture whose synthetic copy is
a few scattered fragments). Unrelated scribbles: 6.1–9.3 → 1.8–1.9 on average.
`calibrate synthetic` (with vs `--no-gate`): every L0 and L1 cell unchanged; L2 within 0.6; L3
(25 px warp, 40 stray strokes) drops a further 1–2 points.

**Known gap.** Parallel hatching that runs along a subject's dominant direction (vertical lines over
an upright cat, cross-hatching over an anime picture full of diagonal hair) can still look related
(2 of 18 references). Telling that apart from a genuine drawing of a one-direction subject needs
more than line directions.
