# ADR-0002 — No AI grading or critique

**Status:** Accepted (2026-11-18)

## Decision
Scores and feedback are produced **only** by maths measurements and hand-written rules. No generative AI or vision-language models, and no learned scoring models. Optional ML pose detection is also out of scope unless this ADR is revisited.

## How grading works
Each method gives a 0–1 similarity, and they are blended with per-category weights into a 1–10 score:

| Method | Measures |
|---|---|
| Keypoint matching (Procrustes) | proportions, angles, placement |
| Outline matching (chamfer distance) | shape accuracy, tolerant of small wobbles |
| Silhouette overlap (IoU) | big shape |
| Grid comparison | *where* the errors are |
| Light/dark (blurred tone map) | shading and values |
| SSIM | general structural likeness |
| Line-direction correlation (structure tensor) | relevance: does the drawing depict the reference at all? Low relevance caps the score near 1 |
| Stroke quality | line confidence (smoothness, retraces, speed) |

Drawings are auto-aligned before comparison, a passing threshold keeps tiny errors near 10, and weights are tuned against human-labelled samples (M8).

## How feedback works
Measurement → diagnosis rule (TOML) → tip (templated with real numbers) → lesson + drill. Show the top 2–3 problems plus one strength, draw them on the drawing, and track recurring mistakes over time.

## Why
Free to run, consistent and explainable scores, and every piece of advice is written and checked by people.
