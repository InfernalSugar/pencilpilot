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
//!
//! The weighted mean of the similarities (weights per [`Category`]) goes
//! through a [`ScoreCurve`] to give the 1–10 score.

mod align;
pub mod difficulty;
mod edges;
mod img;
pub mod keypoints;
mod metrics;
pub mod score;
pub mod strokes;

use serde::{Deserialize, Serialize};

pub use keypoints::{KeypointPair, KeypointResult, Point, fit_keypoints};
pub use score::{Category, DEFAULT_WEIGHTS, ScoreCurve, Weights};
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
    /// Overrides the category's default weights.
    pub weights: Option<Weights>,
    pub curve: Option<ScoreCurve>,
    pub strokes: Vec<Stroke>,
    pub keypoints: Vec<KeypointPair>,
}

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
    /// Weighted mean similarity (0–1) before the curve.
    pub similarity: f32,
    /// `similarity` reached the curve's passing threshold.
    pub passed: bool,
    pub category: Category,
    pub measurements: Vec<Measurement>,
    pub grid: GridResult,
    pub alignment: Alignment,
    pub bbox: BBoxInfo,
    pub keypoints: Option<KeypointResult>,
    /// `blank`, `reference_blank`, `alignment_at_limit`.
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
    let weights = opts
        .weights
        .unwrap_or_else(|| Weights::for_category(opts.category));
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
    let drawing = &without_frame(drawing, &frame, f);
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
    let sigma = metrics::sigma_px(w, h);

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

    // 9. Strokes.
    let mut parts: Vec<(f32, f32)> = vec![
        (weights.placement, placement),
        (weights.outline, outline),
        (weights.silhouette, silhouette),
        (weights.grid, grid_mean),
        (weights.tone * shading_weight, tone.similarity),
        (weights.ssim * shading_weight, ssim),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn img(fill: u8) -> Gray {
        Gray::new(40, 30, vec![fill; 1200]).unwrap()
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
