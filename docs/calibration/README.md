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
