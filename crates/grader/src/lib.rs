//! PencilPilot grading engine (grader v1, milestone M3).
//!
//! Pure image maths, no I/O and no AI/ML (ADR-0002): takes the student's
//! drawing and the reference as grayscale bitmaps of the same size and
//! returns a 1–10 score plus named measurements for the feedback engine.
//!
//! Pipeline (see [`grade_with`]):
//! 1. **Prep** — downsample to ≈200 px on the long side. Reference:
//!    Canny-style edge maps (the reference is a photo or illustration, so we
//!    compare the student's lines to its *edges*): a *strict* map of the
//!    strongest edges and a denser *detail* map that adds clearly visible
//!    detail (fur, folds, features). A letterbox frame around the picture is
//!    ignored in both images. Drawing: ink mask (darker than
//!    [`edges::INK_THRESHOLD`]), its outline, and `shading` (share of ink in
//!    filled areas).
//! 2. **Auto-alignment** — similarity transform from moments, refined by a
//!    bounded search (scale ±15 %, rotation ±10°, shift).
//! 3. **Placement & proportion** — the drawing's box as drawn vs the box of
//!    the part of the reference it depicts (edge-energy weighted, trimmed),
//!    so background clutter can't stretch it.
//! 4. **outline** — chamfer distance → `exp(-d/σ)`: precision against the
//!    detail map (correct detail is not "extra"), recall against the strict
//!    map weighted by edge strength (missing a strong edge costs more).
//! 5. **silhouette** — IoU of filled, dilated shapes.
//! 6. **grid** — outline similarity per cell (4×3).
//! 7. **tone** — correlation of heavily blurred tone maps + contrast ratio.
//! 8. **ssim** — structural similarity on the aligned low-res images.
//!    Tone and SSIM count in proportion to `shading`: pure line art has no
//!    tone to compare, so it is not marked down for it.
//! 9. **line_confidence** — stroke smoothness, retraces, speed (optional).
//! 10. **keypoints** — Procrustes fit of tapped points (optional).
//! 11. **relevance** — does it depict the reference at all? Correlation of
//!     local line directions (`relevance.rs`); low relevance caps the
//!     score near 1 and flags the grade `unrelated`, because the coverage
//!     measures above reward any ink in the right area.
//!
//! The weighted mean of the similarities (weights per [`Category`]) goes
//! through a [`ScoreCurve`] to give the 1–10 score.

mod align;
pub mod difficulty;
mod edges;
mod img;
pub mod keypoints;
mod metrics;
mod relevance;
pub mod score;
pub mod sketch;
pub mod strokes;

use serde::{Deserialize, Serialize};

pub use keypoints::{KeypointPair, KeypointResult, Point, fit_keypoints};
pub use score::{Category, DEFAULT_WEIGHTS, SKETCH_WEIGHTS, ScoreCurve, Weights};
pub use strokes::{Stroke, StrokeMetrics, StrokePoint, stroke_metrics};

use img::{distance_transform, downsample_mean, downsample_min, factor_for};

/// Long side of the working resolution.
pub const WORK_SIZE: u32 = 200;
/// Grid used for the per-region comparison.
pub const GRID_COLS: usize = 4;
pub const GRID_ROWS: usize = 3;
/// Share of ink in filled areas (`shading`) at or below which a drawing
/// counts as pure line art, and from which tone counts in full.
const SHADING_NONE: f32 = 0.10;
const SHADING_FULL: f32 = 0.35;
/// Reference boxes ignore this share of edge energy on each side.
const REF_BOX_TRIM: f32 = 0.02;
/// Reference edges within this many outline σ of the aligned drawing are
/// the part of the picture the drawing depicts (for placement).
const SUBJECT_RADIUS: f32 = 3.0;
/// Fewer low-res ink pixels than this = blank drawing.
const BLANK_INK: usize = 4;

/// A grayscale image, row-major, one byte per pixel (0 = black, 255 = white).
#[derive(Debug, Clone)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Gray {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, GradeError> {
        if pixels.len() != (width as usize) * (height as usize) {
            return Err(GradeError::SizeMismatch);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradeError {
    SizeMismatch,
    DifferentDimensions,
    Empty,
}

/// Everything besides the two images. All fields optional (JSON camelCase).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GradeOptions {
    pub category: Category,
    /// Finished drawing (default) or a rough sketch (ADR-0007).
    pub mode: Mode,
    /// Overrides the category's default weights.
    pub weights: Option<Weights>,
    pub curve: Option<ScoreCurve>,
    pub strokes: Vec<Stroke>,
    pub keypoints: Vec<KeypointPair>,
    /// Cap the score of drawings that don't depict the reference (see the
    /// `relevance` measurement and the `unrelated` flag). On unless `false`;
    /// exercises where shading or texture is the point (value drills) turn
    /// it off.
    pub relevance_gate: Option<bool>,
}

/// What kind of drawing is being graded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Finished,
    /// Rough multi-pass lines are merged first, then graded on proportions,
    /// big shapes and angles at a coarse scale.
    Sketch,
}

/// Outline tolerance in sketch mode, as a multiple of the usual σ.
const SKETCH_TOLERANCE: f32 = 3.0;

/// One named number. Similarities are 0–1 (1 = matches); raw values are
/// documented in [`MEASUREMENT_DOCS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub name: String,
    pub value: f32,
}

/// Stable measurement names and what they mean.
pub const MEASUREMENT_DOCS: &[(&str, &str)] = &[
    (
        "placement",
        "0–1 similarity of size and position before alignment",
    ),
    (
        "scale_ratio",
        "raw: drawing bbox size ÷ bbox of the part of the reference it depicts (1 = same)",
    ),
    (
        "aspect_error",
        "raw, signed: drawing aspect ÷ reference aspect − 1 (>0 too wide, <0 too narrow)",
    ),
    (
        "offset_x",
        "raw, signed: drawing centre − reference centre, fraction of canvas width (>0 = too far right)",
    ),
    ("offset_y", "raw, signed: same for height (>0 = too low)"),
    (
        "outline",
        "0–1 symmetric chamfer similarity after alignment",
    ),
    (
        "outline_precision",
        "0–1: drawn lines lie on reference edges, detail included (low = extra/misplaced lines)",
    ),
    (
        "outline_recall",
        "0–1: the strongest reference edges are covered by the drawing, weighted by edge strength (low = missing parts)",
    ),
    (
        "outline_distance",
        "raw: mean symmetric line distance, fraction of the canvas diagonal",
    ),
    ("silhouette", "0–1 IoU of filled, dilated shapes"),
    (
        "grid",
        "0–1 mean outline similarity over grid cells with content",
    ),
    (
        "worst_cell",
        "0–1 outline similarity of the worst grid cell (see grid.worst)",
    ),
    ("tone", "0–1 correlation of heavily blurred light/dark maps"),
    (
        "contrast_ratio",
        "raw: drawing tone range ÷ reference tone range (<1 = shading too flat)",
    ),
    (
        "ssim",
        "0–1 structural similarity on aligned, blurred low-res images",
    ),
    (
        "ink_coverage",
        "raw: fraction of the canvas covered by ink (working resolution)",
    ),
    (
        "shading",
        "raw 0–1: share of the ink in filled/shaded areas (0 = pure line art); tone and ssim count in proportion",
    ),
    (
        "angles",
        "0–1: main line directions of the drawing vs the reference (orientation histograms); sketch mode leans on it",
    ),
    (
        "structure",
        "raw −1…1: correlation of local line directions, drawing vs reference edges, near the drawing (≈0 = unrelated scribbles or hatching)",
    ),
    (
        "relevance",
        "0–1: does the drawing depict the reference at all (from structure); below 0.5 the grade is flagged unrelated and its score capped near 1",
    ),
    (
        "passes",
        "raw ≥1, sketch mode only: average number of separate strokes per line (1 = single clean passes)",
    ),
    (
        "line_confidence",
        "0–1 stroke quality (smoothness, retraces, speed); only with strokes",
    ),
    ("smoothness", "0–1 from curvature jitter; only with strokes"),
    (
        "curvature_jitter",
        "raw: RMS change of turning angle (rad) per 4 px; only with strokes",
    ),
    (
        "retrace_ratio",
        "raw 0–1: share of path laid over existing ink; only with strokes",
    ),
    (
        "speed_consistency",
        "0–1 from the speed coefficient of variation; only with strokes",
    ),
    (
        "speed_cv",
        "raw: length-weighted coefficient of variation of pen speed; only with strokes",
    ),
    (
        "stroke_count",
        "raw: number of non-eraser strokes; only with strokes",
    ),
    (
        "keypoints",
        "0–1 similarity from keypoint_error; only with keypoints",
    ),
    (
        "keypoint_error",
        "raw: RMS keypoint error after Procrustes ÷ reference spread",
    ),
    (
        "keypoint_max_error",
        "raw: worst single keypoint error, same unit",
    ),
    (
        "angle_error",
        "raw: mean absolute segment angle error, degrees",
    ),
];

/// Per-region outline similarity, row-major (`cells[row * cols + col]`).
/// Cells with nothing in either image score 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridResult {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<f32>,
    /// The lowest-scoring cell that has content, if any.
    pub worst: Option<CellRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellRef {
    pub col: usize,
    pub row: usize,
    pub index: usize,
}

/// Drawing → reference transform, in canvas pixels:
/// `p_ref = c + scale·R(rotation)·(p_drawing − c) + (dx, dy)`, `c` = canvas centre.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alignment {
    pub scale: f32,
    pub dx: f32,
    pub dy: f32,
    /// degrees, clockwise on screen (y points down)
    pub rotation: f32,
}

/// A box as fractions of the canvas (0–1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBoxInfo {
    pub drawing: Option<BBox>,
    pub reference: Option<BBox>,
}

/// Result of grading one drawing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grade {
    /// Final score, 1.0–10.0.
    pub score: f32,
    /// Weighted mean similarity (0–1) before the curve (pulled down when the
    /// drawing is unrelated, see the `relevance` measurement).
    pub similarity: f32,
    /// `similarity` reached the curve's passing threshold.
    pub passed: bool,
    pub category: Category,
    pub measurements: Vec<Measurement>,
    pub grid: GridResult,
    pub alignment: Alignment,
    pub bbox: BBoxInfo,
    pub keypoints: Option<KeypointResult>,
    /// `blank`, `reference_blank`, `alignment_at_limit`, `unrelated` (the
    /// drawing doesn't depict the reference; its score is capped near 1).
    pub flags: Vec<String>,
}

impl Grade {
    pub fn measurement(&self, name: &str) -> Option<f32> {
        self.measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.value)
    }

    /// `(name, value)` pairs, the shape the feedback engine consumes.
    pub fn measurement_pairs(&self) -> Vec<(String, f32)> {
        self.measurements
            .iter()
            .map(|m| (m.name.clone(), m.value))
            .collect()
    }
}

/// Grade with default options (fundamentals, no strokes or keypoints).
pub fn grade(drawing: &Gray, reference: &Gray) -> Result<Grade, GradeError> {
    grade_with(drawing, reference, &GradeOptions::default())
}

struct Out {
    m: Vec<Measurement>,
}

impl Out {
    fn push(&mut self, name: &str, value: f32) {
        let value = if value.is_finite() { value } else { 0.0 };
        self.m.push(Measurement {
            name: name.into(),
            value,
        });
    }
}

/// Grade a drawing against a reference.
pub fn grade_with(
    drawing: &Gray,
    reference: &Gray,
    opts: &GradeOptions,
) -> Result<Grade, GradeError> {
    if drawing.width != reference.width || drawing.height != reference.height {
        return Err(GradeError::DifferentDimensions);
    }
    if drawing.width == 0 || drawing.height == 0 {
        return Err(GradeError::Empty);
    }
    let (cw, ch) = (drawing.width as f32, drawing.height as f32);
    let sketch_mode = opts.mode == Mode::Sketch;
    let weights = opts.weights.unwrap_or_else(|| {
        if sketch_mode {
            score::SKETCH_WEIGHTS
        } else {
            Weights::for_category(opts.category)
        }
    });
    let curve = opts.curve.unwrap_or_default();
    let mut flags = Vec::new();
    let mut out = Out { m: Vec::new() };

    // 1. Prep.
    let f = factor_for(drawing.width, drawing.height, WORK_SIZE);
    let ref_tone = downsample_mean(reference, f);
    let edges::ReferenceEdges {
        strict: ref_edges,
        detail: ref_detail,
        strength: ref_strength,
        frame,
    } = edges::reference_edge_maps(&ref_tone);
    let unframed = without_frame(drawing, &frame, f);
    let mut drawing: &Gray = &unframed;
    // Sketch mode: merge multi-pass lines into the lines meant, then grade those.
    let mut passes = None;
    let consolidated;
    if sketch_mode && !opts.strokes.is_empty() {
        let near_ink = img::dilate(&edges::ink_mask(&downsample_min(drawing, f)), 2.0);
        let (iw, ih) = (near_ink.w, near_ink.h);
        let keep = |x: f32, y: f32| {
            if x < 0.0 || y < 0.0 {
                return false;
            }
            let (cx, cy) = (x as usize / f, y as usize / f);
            cx < iw && cy < ih && near_ink.d[cy * iw + cx]
        };
        if let Some(c) = sketch::consolidate(&opts.strokes, drawing.width, drawing.height, keep) {
            passes = Some(c.passes);
            consolidated = c.image;
            drawing = &consolidated;
        }
    }
    let ink = edges::ink_mask(&downsample_min(drawing, f));
    let (w, h) = (ink.w, ink.h);
    let line = edges::ink_outline(&ink);
    let dt_detail = distance_transform(&ref_detail);
    let ref_pts = ref_edges.points();
    if ref_pts.is_empty() {
        flags.push("reference_blank".to_string());
    }
    // Low-res → canvas fractions.
    let to_bbox = |b: (f32, f32, f32, f32)| BBox {
        x: b.0 * f as f32 / cw,
        y: b.1 * f as f32 / ch,
        w: (b.2 - b.0) * f as f32 / cw,
        h: (b.3 - b.1) * f as f32 / ch,
    };
    // Edge energy (strength²) per reference edge pixel: boxes are taken
    // over edge mass, so faint clutter weighs little.
    let energy = img::Plane {
        w,
        h,
        d: ref_strength.d.iter().map(|v| v * v).collect(),
    };
    let whole_ref_bbox = metrics::mass_bbox(&weighted(&ref_detail, Some(&energy)), REF_BOX_TRIM);

    let ink_count = ink.count();
    if ink_count < BLANK_INK {
        flags.push("blank".to_string());
        let mut g = blank_grade(opts.category, &mut out, &mut flags);
        g.bbox.reference = whole_ref_bbox.map(to_bbox);
        return Ok(g);
    }
    let line_pts = line.points();
    let dt_line = distance_transform(&line);
    let shading = ink
        .d
        .iter()
        .zip(&line.d)
        .filter(|(a, b)| **a && !**b)
        .count() as f32
        / ink_count as f32;
    let shading_weight = ((shading - SHADING_NONE) / (SHADING_FULL - SHADING_NONE)).clamp(0.0, 1.0);

    // 2. Alignment.
    let al = align::align(&line_pts, &ref_pts, &dt_line, &dt_detail);
    if al.clamped {
        flags.push("alignment_at_limit".to_string());
    }
    let full_t = al.t.scaled(f as f32);
    let aligned = align::warp(drawing, &full_t);
    let a_ink = edges::ink_mask(&downsample_min(&aligned, f));
    let a_line = edges::ink_outline(&a_ink);
    let a_tone = downsample_mean(&aligned, f);
    let dt_a_line = distance_transform(&a_line);
    let sigma = metrics::sigma_px(w, h) * if sketch_mode { SKETCH_TOLERANCE } else { 1.0 };

    // 3. Placement & proportion: the drawing's box as drawn (before
    // alignment) vs the box of the part of the reference it depicts, i.e.
    // the reference edges near the aligned drawing. Background clutter the
    // student rightly left out can't stretch the reference box.
    let draw_bbox = metrics::robust_bbox(&line_pts);
    let near = SUBJECT_RADIUS * sigma;
    let depicted = img::Mask {
        w,
        h,
        d: ref_detail
            .d
            .iter()
            .zip(&dt_a_line.d)
            .map(|(&e, &d)| e && d <= near)
            .collect(),
    };
    let ref_bbox =
        metrics::mass_bbox(&weighted(&depicted, Some(&energy)), REF_BOX_TRIM).or(whole_ref_bbox);
    let placement = match (draw_bbox.map(to_bbox), ref_bbox.map(to_bbox)) {
        (Some(d), Some(r)) => {
            let size = |b: &BBox| (b.w * cw * b.h * ch).max(1.0).sqrt();
            let aspect = |b: &BBox| (b.w * cw).max(1.0) / (b.h * ch).max(1.0);
            let scale_ratio = size(&d) / size(&r);
            let aspect_error = aspect(&d) / aspect(&r) - 1.0;
            let offset_x = (d.x + d.w / 2.0) - (r.x + r.w / 2.0);
            let offset_y = (d.y + d.h / 2.0) - (r.y + r.h / 2.0);
            let z = (scale_ratio.ln() / 0.25).powi(2)
                + ((1.0 + aspect_error).ln() / 0.25).powi(2)
                + (offset_x.hypot(offset_y) / 0.10).powi(2);
            let p = (-0.5 * z).exp();
            out.push("placement", p);
            out.push("scale_ratio", scale_ratio);
            out.push("aspect_error", aspect_error);
            out.push("offset_x", offset_x);
            out.push("offset_y", offset_y);
            p
        }
        _ => {
            out.push("placement", 0.0);
            0.0
        }
    };

    // 4. Outline.
    let diag = ((w * w + h * h) as f32).sqrt();
    let fwd = metrics::mean_distance(&a_line, &dt_detail).unwrap_or(metrics::CAP);
    let bwd = metrics::weighted_mean_distance(&ref_edges, &ref_strength, &dt_a_line)
        .unwrap_or(metrics::CAP);
    let d = 0.5 * (fwd + bwd);
    let outline = (-d / sigma).exp();
    out.push("outline", outline);
    out.push("outline_precision", (-fwd / sigma).exp());
    out.push("outline_recall", (-bwd / sigma).exp());
    out.push("outline_distance", d / diag);

    // 5. Silhouette.
    let silhouette = metrics::silhouette(&a_ink, &ref_edges);
    out.push("silhouette", silhouette);

    // 6. Grid.
    let gs = metrics::grid_scores(
        &a_line, &ref_edges, &dt_a_line, &dt_detail, GRID_COLS, GRID_ROWS,
    );
    let content: Vec<(usize, f32)> = gs
        .cells
        .iter()
        .zip(&gs.has_content)
        .enumerate()
        .filter(|(_, (_, c))| **c)
        .map(|(i, (s, _))| (i, *s))
        .collect();
    let grid_mean = if content.is_empty() {
        0.0
    } else {
        content.iter().map(|c| c.1).sum::<f32>() / content.len() as f32
    };
    let worst = content.iter().min_by(|a, b| a.1.total_cmp(&b.1)).copied();
    out.push("grid", grid_mean);
    out.push("worst_cell", worst.map_or(0.0, |w| w.1));
    let grid = GridResult {
        cols: GRID_COLS,
        rows: GRID_ROWS,
        cells: gs.cells,
        worst: worst.map(|(i, _)| CellRef {
            col: i % GRID_COLS,
            row: i / GRID_COLS,
            index: i,
        }),
    };

    // 7. Tone.
    let tone = metrics::tone(&a_tone, &ref_tone);
    out.push("tone", tone.similarity);
    out.push("contrast_ratio", tone.contrast_ratio);

    // 8. SSIM.
    let content = img::Mask {
        w,
        h,
        d: a_ink
            .d
            .iter()
            .zip(&ref_edges.d)
            .map(|(a, b)| *a || *b)
            .collect(),
    };
    let ssim = metrics::ssim(&a_tone, &ref_tone, &img::dilate(&content, 0.04 * diag));
    out.push("ssim", ssim);
    out.push("ink_coverage", ink_count as f32 / (w * h) as f32);
    out.push("shading", shading);
    // Main line directions: drawing outline vs the depicted reference edges.
    let angles = metrics::orientation_similarity(&a_tone, &ref_tone, &a_line, &depicted);
    out.push("angles", angles);
    // Does it depict the reference at all? Line directions of the drawing
    // vs the reference edges. A traced letterbox frame is neither evidence
    // for nor against (it has no reference edges): lines near it are left
    // out, with room for a frame drawn a little off.
    let near_frame = img::dilate(&frame, 2.0 * metrics::sigma_px(w, h));
    let mut judged = a_line.clone();
    for (l, f) in judged.d.iter_mut().zip(&near_frame.d) {
        *l &= !*f;
    }
    let structure = relevance::structure_correlation(
        &a_tone,
        &judged,
        &ref_tone,
        &ref_detail,
        &ref_strength,
        // the usual tolerance in sketch mode too: blurring wider makes
        // random lines line up by chance
        metrics::sigma_px(w, h),
    );
    // No lines to judge (a fully painted drawing, all ink): no evidence
    // either way, so no gate.
    let relevance = structure.map_or(1.0, relevance::relevance);
    if let Some(s) = structure {
        out.push("structure", s);
    }
    out.push("relevance", relevance);
    if let Some(p) = passes {
        out.push("passes", p);
    }

    // 9. Strokes.
    let mut parts: Vec<(f32, f32)> = vec![
        (weights.placement, placement),
        (weights.outline, outline),
        (weights.silhouette, silhouette),
        (weights.grid, grid_mean),
        (weights.tone * shading_weight, tone.similarity),
        (weights.ssim * shading_weight, ssim),
        (weights.angles, angles),
    ];
    if let Some(s) = stroke_metrics(&opts.strokes, drawing.width, drawing.height) {
        out.push("line_confidence", s.line_confidence);
        out.push("smoothness", s.smoothness);
        out.push("curvature_jitter", s.curvature_jitter);
        out.push("retrace_ratio", s.retrace_ratio);
        out.push("speed_consistency", s.speed_consistency);
        out.push("speed_cv", s.speed_cv);
        out.push("stroke_count", s.stroke_count as f32);
        parts.push((weights.line_confidence, s.line_confidence));
    }

    // 10. Keypoints.
    let kp = fit_keypoints(&opts.keypoints);
    if let Some(k) = &kp {
        let sim = (-k.keypoint_error / keypoints::KEYPOINT_SIGMA).exp();
        out.push("keypoints", sim);
        out.push("keypoint_error", k.keypoint_error);
        out.push("keypoint_max_error", k.max_error);
        out.push("angle_error", k.mean_angle_error);
        parts.push((weights.keypoints, sim));
    }

    let wsum: f32 = parts.iter().map(|p| p.0.max(0.0)).sum();
    let similarity = if wsum > 0.0 {
        parts.iter().map(|p| p.0.max(0.0) * p.1).sum::<f32>() / wsum
    } else {
        0.0
    };
    let gate = opts.relevance_gate.unwrap_or(true) && !ref_pts.is_empty();
    if gate && relevance < relevance::FLAG_BELOW {
        flags.push("unrelated".to_string());
    }
    let similarity = if gate {
        relevance::cap(similarity, relevance, &curve)
    } else {
        similarity
    };
    let fs = f as f32;
    Ok(Grade {
        score: curve.score(similarity),
        similarity,
        passed: similarity >= curve.pass,
        category: opts.category,
        measurements: out.m,
        grid,
        alignment: Alignment {
            scale: full_t.scale,
            dx: al.t.dx * fs,
            dy: al.t.dy * fs,
            rotation: al.t.rotation.to_degrees(),
        },
        bbox: BBoxInfo {
            drawing: draw_bbox.map(to_bbox),
            reference: ref_bbox.map(to_bbox),
        },
        keypoints: kp,
        flags,
    })
}

/// Set pixels of `m` as `(x, y, weight)` (pixel centres; weight 1 without
/// a weight plane).
fn weighted(m: &img::Mask, weight: Option<&img::Plane>) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for y in 0..m.h {
        for x in 0..m.w {
            let i = y * m.w + x;
            if m.d[i] {
                out.push((
                    x as f32 + 0.5,
                    y as f32 + 0.5,
                    weight.map_or(1.0, |p| p.d[i]),
                ));
            }
        }
    }
    out
}

/// The drawing with the reference's letterbox frame band painted white
/// (see [`edges::letterbox_frame`]).
fn without_frame<'a>(drawing: &'a Gray, frame: &img::Mask, f: usize) -> std::borrow::Cow<'a, Gray> {
    if !frame.d.iter().any(|&b| b) {
        return std::borrow::Cow::Borrowed(drawing);
    }
    let mut g = drawing.clone();
    let w = g.width as usize;
    for (i, p) in g.pixels.iter_mut().enumerate() {
        let (x, y) = ((i % w) / f, (i / w) / f);
        if frame.d[y * frame.w + x] {
            *p = 255;
        }
    }
    std::borrow::Cow::Owned(g)
}

fn blank_grade(category: Category, out: &mut Out, flags: &mut Vec<String>) -> Grade {
    for name in ["placement", "outline", "silhouette", "grid", "tone", "ssim"] {
        out.push(name, 0.0);
    }
    out.push("ink_coverage", 0.0);
    out.push("shading", 0.0);
    Grade {
        score: 1.0,
        similarity: 0.0,
        passed: false,
        category,
        measurements: std::mem::take(&mut out.m),
        grid: GridResult {
            cols: GRID_COLS,
            rows: GRID_ROWS,
            cells: vec![0.0; GRID_COLS * GRID_ROWS],
            worst: None,
        },
        alignment: Alignment {
            scale: 1.0,
            dx: 0.0,
            dy: 0.0,
            rotation: 0.0,
        },
        bbox: BBoxInfo {
            drawing: None,
            reference: None,
        },
        keypoints: None,
        flags: std::mem::take(flags),
    }
}

/// Recompute (similarity, score) from a grade's measurements with other
/// weights or another curve, exactly as [`grade_with`] combines them. Used by
/// `tools/calibrate` to fit weights to human ratings without re-grading.
pub fn rescore(
    m: &std::collections::HashMap<String, f32>,
    weights: &Weights,
    curve: &ScoreCurve,
) -> (f32, f32) {
    let get = |k: &str| m.get(k).copied();
    let shading = get("shading").unwrap_or(0.0);
    let shading_weight = ((shading - SHADING_NONE) / (SHADING_FULL - SHADING_NONE)).clamp(0.0, 1.0);
    let mut parts: Vec<(f32, f32)> = vec![
        (weights.placement, get("placement").unwrap_or(0.0)),
        (weights.outline, get("outline").unwrap_or(0.0)),
        (weights.silhouette, get("silhouette").unwrap_or(0.0)),
        (weights.grid, get("grid").unwrap_or(0.0)),
        (weights.tone * shading_weight, get("tone").unwrap_or(0.0)),
        (weights.ssim * shading_weight, get("ssim").unwrap_or(0.0)),
        (weights.angles, get("angles").unwrap_or(0.0)),
    ];
    if let Some(v) = get("line_confidence") {
        parts.push((weights.line_confidence, v));
    }
    if let Some(v) = get("keypoints") {
        parts.push((weights.keypoints, v));
    }
    let wsum: f32 = parts.iter().map(|p| p.0.max(0.0)).sum();
    let similarity = if wsum > 0.0 {
        parts.iter().map(|p| p.0.max(0.0) * p.1).sum::<f32>() / wsum
    } else {
        0.0
    };
    let similarity = match get("relevance") {
        Some(r) => relevance::cap(similarity, r, curve),
        None => similarity,
    };
    (similarity, curve.score(similarity))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod sketch_mode {
        use super::*;
        use crate::sketch::tests::pass;

        const W: u32 = 800;
        const H: u32 = 600;

        /// A simple subject: a box with a circle on top (body and head).
        fn subject(scale: f32, dx: f32) -> Vec<Vec<(f32, f32)>> {
            subject_head(scale, dx, 1.0)
        }

        /// …with the head `head` times its real size (a proportion mistake).
        fn subject_head(scale: f32, dx: f32, head: f32) -> Vec<Vec<(f32, f32)>> {
            let t = |x: f32, y: f32| {
                (
                    400.0 + (x - 400.0) * scale + dx,
                    330.0 + (y - 330.0) * scale,
                )
            };
            let rect = vec![
                t(300.0, 300.0),
                t(500.0, 300.0),
                t(500.0, 480.0),
                t(300.0, 480.0),
                t(300.0, 300.0),
            ];
            let circle = (0..=48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    let r = 70.0 * head;
                    t(400.0 + r * a.cos(), 300.0 - r + r * a.sin())
                })
                .collect();
            vec![rect, circle]
        }

        /// Raster of strokes, as the canvas would show them.
        fn render(strokes: &[Stroke], width: f32) -> Gray {
            let mut px = vec![255u8; (W * H) as usize];
            for s in strokes {
                for seg in s.points.windows(2) {
                    let (a, b) = ((seg[0].x, seg[0].y), (seg[1].x, seg[1].y));
                    let n = ((b.0 - a.0).hypot(b.1 - a.1) / 0.7).ceil().max(1.0) as usize;
                    for k in 0..=n {
                        let t = k as f32 / n as f32;
                        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                        let r = width / 2.0;
                        for yy in (y - r).floor() as i32..=(y + r).ceil() as i32 {
                            for xx in (x - r).floor() as i32..=(x + r).ceil() as i32 {
                                if xx >= 0
                                    && yy >= 0
                                    && (xx as u32) < W
                                    && (yy as u32) < H
                                    && ((xx as f32 + 0.5 - x).powi(2)
                                        + (yy as f32 + 0.5 - y).powi(2))
                                        <= r * r
                                {
                                    px[(yy as u32 * W + xx as u32) as usize] = 40;
                                }
                            }
                        }
                    }
                }
            }
            Gray::new(W, H, px).unwrap()
        }

        fn clean(scale: f32, dx: f32) -> Vec<Stroke> {
            subject(scale, dx)
                .iter()
                .enumerate()
                .map(|(i, p)| pass(p, 0.0, 0.6, i as u32))
                .collect()
        }

        /// Every line drawn as four or five scratchy passes ~4 px apart that
        /// overshoot the ends, like a real rough sketch.
        fn hairy(scale: f32, dx: f32) -> Vec<Stroke> {
            hairy_of(subject(scale, dx))
        }

        fn hairy_of(shapes: Vec<Vec<(f32, f32)>>) -> Vec<Stroke> {
            let mut out = Vec::new();
            for (i, p) in shapes.iter().enumerate() {
                for k in 0..(4 + i % 2) {
                    let mut path = p.clone();
                    // overshoot: extend both ends along their direction
                    let ext = |a: (f32, f32), b: (f32, f32), by: f32| {
                        let (dx, dy) = (a.0 - b.0, a.1 - b.1);
                        let l = dx.hypot(dy).max(1e-6);
                        (a.0 + dx / l * by, a.1 + dy / l * by)
                    };
                    let n = path.len();
                    path.insert(0, ext(p[0], p[1], 6.0 + 4.0 * k as f32));
                    path.push(ext(p[n - 1], p[n - 2], 6.0 + 3.0 * k as f32));
                    out.push(pass(
                        &path,
                        (k as f32 - 2.0) * 4.0,
                        3.0,
                        (i * 10 + k) as u32,
                    ));
                }
            }
            out
        }

        /// Guide lines a sketcher puts on the Construction layer (a centre line and a box).
        fn construction() -> Vec<Stroke> {
            vec![
                pass(&[(400.0, 100.0), (400.0, 520.0)], 0.0, 1.0, 900),
                pass(
                    &[
                        (250.0, 140.0),
                        (550.0, 140.0),
                        (550.0, 520.0),
                        (250.0, 520.0),
                        (250.0, 140.0),
                    ],
                    0.0,
                    1.0,
                    901,
                ),
            ]
        }

        fn score(strokes: Vec<Stroke>, mode: Mode) -> (f32, Grade) {
            let reference = render(&clean(1.0, 0.0), 3.0);
            let drawing = render(&strokes, 2.0);
            let g = grade_with(
                &drawing,
                &reference,
                &GradeOptions {
                    mode,
                    strokes,
                    ..Default::default()
                },
            )
            .unwrap();
            (g.score, g)
        }

        #[test]
        fn hairy_sketches_score_like_clean_ones_in_sketch_mode() {
            let (clean_s, _) = score(clean(1.0, 0.0), Mode::Sketch);
            let (hairy_s, g) = score(hairy(1.0, 0.0), Mode::Sketch);
            // without sketch mode the guide lines are part of the drawing too
            let (hairy_f, _) = score([hairy(1.0, 0.0), construction()].concat(), Mode::Finished);
            let (wrong_s, _) = score(hairy(1.35, 60.0), Mode::Sketch);
            eprintln!(
                "sketch: clean {clean_s:.2} hairy {hairy_s:.2} wrong {wrong_s:.2}; finished hairy {hairy_f:.2}"
            );
            let m = |n: &str| g.measurements.iter().find(|x| x.name == n).map(|x| x.value);
            assert!(m("passes").unwrap() > 3.0, "passes {:?}", m("passes"));
            assert!(hairy_s >= 8.0, "a good rough sketch scores well: {hairy_s}");
            assert!(
                (clean_s - hairy_s).abs() <= 1.0,
                "clean {clean_s} vs hairy {hairy_s}"
            );
            assert!(hairy_s > hairy_f, "sketch mode is kinder to rough lines");
            assert!(
                wrong_s <= hairy_s - 2.0,
                "wrong size/place still costs: {wrong_s}"
            );
        }

        #[test]
        fn mistakes_cost_more_the_bigger_they_are() {
            // size and place on the page
            let s: Vec<f32> = [1.0f32, 1.15, 1.3, 1.5]
                .iter()
                .map(|&k| score(hairy(k, (k - 1.0) * 120.0), Mode::Sketch).0)
                .collect();
            eprintln!("by size/place mistake: {s:?}");
            assert!(s.windows(2).all(|w| w[1] < w[0] + 0.05), "{s:?}");
            assert!(s[0] - s[3] >= 3.0, "{s:?}");
            // proportions inside the drawing: the head too big for the body
            let p: Vec<f32> = [1.0f32, 1.25, 1.5, 1.8]
                .iter()
                .map(|&k| score(hairy_of(subject_head(1.0, 0.0, k)), Mode::Sketch).0)
                .collect();
            eprintln!("by head-size mistake: {p:?}");
            assert!(p.windows(2).all(|w| w[1] < w[0] + 0.05), "{p:?}");
            assert!(p[0] - p[3] >= 2.0, "{p:?}");
        }
    }

    fn img(fill: u8) -> Gray {
        Gray::new(40, 30, vec![fill; 1200]).unwrap()
    }

    #[test]
    fn rescore_matches_grading() {
        // a box drawn a little off against a shaded box: every part of the score in play
        let (w, h) = (160usize, 120usize);
        let mut r = Gray::new(w as u32, h as u32, vec![255; w * h]).unwrap();
        let mut d = r.clone();
        for y in 30..90 {
            for x in 40..120 {
                r.pixels[y * w + x] = 90;
                let edge = y == 32 || y == 88 || x == 42 || x == 117;
                if edge {
                    d.pixels[y * w + x] = 20;
                }
            }
        }
        for cat in [Category::Anime, Category::RealLife, Category::Environment] {
            let opts = GradeOptions {
                category: cat,
                ..Default::default()
            };
            let g = grade_with(&d, &r, &opts).unwrap();
            let m: std::collections::HashMap<String, f32> = g
                .measurements
                .iter()
                .map(|x| (x.name.clone(), x.value))
                .collect();
            let (sim, score) = rescore(&m, &Weights::for_category(cat), &ScoreCurve::default());
            assert!(
                (sim - g.similarity).abs() < 1e-5,
                "{cat:?}: {sim} vs {}",
                g.similarity
            );
            assert!(
                (score - g.score).abs() < 1e-4,
                "{cat:?}: {score} vs {}",
                g.score
            );
        }
    }

    #[test]
    fn blank_drawing_scores_one() {
        let mut r = img(255);
        for y in 10..20 {
            for x in 10..30 {
                r.pixels[y * 40 + x] = 40;
            }
        }
        let g = grade(&img(255), &r).unwrap();
        assert_eq!(g.score, 1.0);
        assert!(g.flags.iter().any(|f| f == "blank"));
    }

    #[test]
    fn rejects_bad_sizes() {
        assert_eq!(
            Gray::new(2, 2, vec![0; 3]).unwrap_err(),
            GradeError::SizeMismatch
        );
        let small = Gray::new(2, 2, vec![0; 4]).unwrap();
        assert_eq!(
            grade(&small, &img(0)).unwrap_err(),
            GradeError::DifferentDimensions
        );
    }

    #[test]
    fn measurement_names_are_documented() {
        let mut r = img(255);
        let mut d = img(255);
        for y in 5..25 {
            for x in 8..32 {
                r.pixels[y * 40 + x] = 60;
                if y == 5 || y == 24 || x == 8 || x == 31 {
                    d.pixels[y * 40 + x] = 0;
                }
            }
        }
        let opts = GradeOptions {
            strokes: vec![Stroke {
                brush_id: "pen".into(),
                eraser: false,
                points: (0..20)
                    .map(|i| StrokePoint {
                        x: 8.0 + i as f32,
                        y: 5.0,
                        pressure: 0.5,
                        t: i as f64 * 16.0,
                    })
                    .collect(),
            }],
            keypoints: vec![
                KeypointPair {
                    reference: Point { x: 8.0, y: 5.0 },
                    drawing: Point { x: 8.0, y: 5.0 },
                },
                KeypointPair {
                    reference: Point { x: 31.0, y: 24.0 },
                    drawing: Point { x: 31.0, y: 24.0 },
                },
            ],
            ..Default::default()
        };
        let g = grade_with(&d, &r, &opts).unwrap();
        for m in &g.measurements {
            assert!(
                MEASUREMENT_DOCS.iter().any(|(n, _)| *n == m.name),
                "undocumented measurement {}",
                m.name
            );
            assert!(m.value.is_finite());
        }
        assert!(g.measurement("keypoints").is_some());
        assert!(g.measurement("line_confidence").is_some());
    }
}
