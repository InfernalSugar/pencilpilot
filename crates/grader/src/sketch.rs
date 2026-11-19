//! Sketch mode (ADR-0007): rough, multi-pass lines are merged into the lines
//! the student meant before grading.
//!
//! A sketcher draws one intended line as several scratchy, overlapping passes.
//! Each resampled stroke point is pulled towards nearby points of the same
//! direction (a mean shift across the line, never along it), so a bundle of
//! passes collapses onto one centre line. The result is redrawn as a clean
//! line drawing, which the normal pipeline then grades at a coarse scale.
//! Plain geometry, no learning (ADR-0002).

use std::collections::{HashMap, HashSet};

use crate::Gray;
use crate::strokes::Stroke;

/// Resampling step, as a share of the canvas diagonal (~4 px on 800×600).
const STEP: f32 = 0.004;
/// Points closer than this (share of the diagonal, ~12 px) can be one line.
const RADIUS: f32 = 0.012;
/// …if their directions differ by less than ~20° (|cos| above this).
const SAME_DIRECTION: f32 = 0.94;
const ITERATIONS: usize = 4;
/// Width of the redrawn clean line, share of the diagonal (~3 px).
const LINE_WIDTH: f32 = 0.003;

#[derive(Debug, Clone, Copy)]
struct Pt {
    x: f32,
    y: f32,
    /// unit tangent (direction doesn't matter: lines are undirected)
    tx: f32,
    ty: f32,
    stroke: usize,
}

/// The cleaned-up sketch.
#[derive(Debug, Clone)]
pub struct Consolidated {
    /// the merged lines, black on white, canvas size
    pub image: Gray,
    /// average number of separate strokes making up each line (1 = clean single passes)
    pub passes: f32,
}

fn resample(points: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let mut out = vec![points[0]];
    let mut carry = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        if len <= f32::EPSILON {
            continue;
        }
        let mut d = step - carry;
        while d <= len {
            let k = d / len;
            out.push((a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k));
            d += step;
        }
        carry = len - (d - step);
    }
    if out.len() == 1 {
        out.push(*points.last().unwrap());
    }
    out
}

fn tangents(stroke: usize, pts: &[(f32, f32)]) -> Vec<Pt> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let a = pts[i.saturating_sub(1)];
            let b = pts[(i + 1).min(n - 1)];
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy).max(1e-6);
            Pt {
                x: pts[i].0,
                y: pts[i].1,
                tx: dx / l,
                ty: dy / l,
                stroke,
            }
        })
        .collect()
}

struct Grid {
    cell: f32,
    map: HashMap<(i32, i32), Vec<usize>>,
}

impl Grid {
    fn new(pts: &[Pt], cell: f32) -> Self {
        let mut map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, p) in pts.iter().enumerate() {
            map.entry(Self::key(p.x, p.y, cell)).or_default().push(i);
        }
        Self { cell, map }
    }
    fn key(x: f32, y: f32, cell: f32) -> (i32, i32) {
        ((x / cell).floor() as i32, (y / cell).floor() as i32)
    }
    fn near(&self, x: f32, y: f32) -> impl Iterator<Item = usize> + '_ {
        let (cx, cy) = Self::key(x, y, self.cell);
        (-1..=1)
            .flat_map(move |dy| (-1..=1).map(move |dx| (cx + dx, cy + dy)))
            .filter_map(|k| self.map.get(&k))
            .flatten()
            .copied()
    }
}

/// Merge multi-pass lines. `keep(x, y)` says whether the finished drawing
/// still has ink near a canvas point (so erased parts stay erased). `None`
/// if there are no usable strokes.
pub fn consolidate(
    strokes: &[Stroke],
    width: u32,
    height: u32,
    keep: impl Fn(f32, f32) -> bool,
) -> Option<Consolidated> {
    let diag = (width as f32).hypot(height as f32);
    let step = (diag * STEP).max(1.5);
    let radius = diag * RADIUS;

    // 1. Resampled points with tangents; strokes that were erased away don't count.
    let mut pts: Vec<Pt> = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (si, s) in strokes.iter().enumerate() {
        if s.eraser || s.points.len() < 2 {
            continue;
        }
        let raw: Vec<(f32, f32)> = s.points.iter().map(|p| (p.x, p.y)).collect();
        let r = resample(&raw, step);
        let t = tangents(si, &r);
        let start = pts.len();
        pts.extend(t.into_iter().filter(|p| keep(p.x, p.y)));
        if pts.len() - start >= 2 {
            ranges.push(start..pts.len());
        } else {
            pts.truncate(start);
        }
    }
    if pts.is_empty() {
        return None;
    }

    // 2. Mean shift across the line. Count the passes on the first round.
    let grid = Grid::new(&pts, radius);
    let sigma2 = (radius * 0.5).powi(2) * 2.0;
    let mut cur: Vec<(f32, f32)> = pts.iter().map(|p| (p.x, p.y)).collect();
    let mut passes_sum = 0.0;
    for it in 0..ITERATIONS {
        let prev = cur.clone();
        for (i, p) in pts.iter().enumerate() {
            let (px, py) = prev[i];
            // normal to the stroke's direction
            let (nx, ny) = (-p.ty, p.tx);
            let (mut wsum, mut shift) = (0.0f32, 0.0f32);
            let mut strokes_here: HashSet<usize> = HashSet::new();
            for j in grid.near(p.x, p.y) {
                let q = &pts[j];
                if (p.tx * q.tx + p.ty * q.ty).abs() < SAME_DIRECTION {
                    continue;
                }
                let (qx, qy) = prev[j];
                let (dx, dy) = (qx - px, qy - py);
                let d2 = dx * dx + dy * dy;
                if d2 > radius * radius {
                    continue;
                }
                let w = (-d2 / sigma2).exp();
                wsum += w;
                shift += w * (dx * nx + dy * ny);
                if it == 0 {
                    strokes_here.insert(q.stroke);
                }
            }
            if wsum > 0.0 {
                let s = shift / wsum;
                cur[i] = (px + nx * s, py + ny * s);
            }
            if it == 0 {
                passes_sum += strokes_here.len().max(1) as f32;
            }
        }
    }

    // 3. Redraw the merged polylines as a clean line drawing.
    let (w, h) = (width as usize, height as usize);
    let mut px = vec![255u8; w * h];
    let half = (diag * LINE_WIDTH * 0.5).max(1.0);
    let mut stamp = |x: f32, y: f32| {
        let (x0, x1) = ((x - half).floor() as i64, (x + half).ceil() as i64);
        let (y0, y1) = ((y - half).floor() as i64, (y + half).ceil() as i64);
        for yy in y0.max(0)..=y1.min(h as i64 - 1) {
            for xx in x0.max(0)..=x1.min(w as i64 - 1) {
                let (dx, dy) = (xx as f32 + 0.5 - x, yy as f32 + 0.5 - y);
                if dx * dx + dy * dy <= half * half {
                    px[yy as usize * w + xx as usize] = 0;
                }
            }
        }
    };
    for r in &ranges {
        let line = &cur[r.clone()];
        for seg in line.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            // a gap means the stroke was cut by erasing: don't bridge it
            if len > step * 4.0 {
                continue;
            }
            let n = (len / (half * 0.5)).ceil().max(1.0) as usize;
            for k in 0..=n {
                let t = k as f32 / n as f32;
                stamp(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
    }
    Some(Consolidated {
        image: Gray {
            width,
            height,
            pixels: px,
        },
        passes: passes_sum / pts.len() as f32,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::strokes::StrokePoint;

    pub(crate) fn stroke(points: &[(f32, f32)]) -> Stroke {
        Stroke {
            brush_id: "pencil".into(),
            eraser: false,
            points: points
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| StrokePoint {
                    x,
                    y,
                    pressure: 0.5,
                    t: i as f64 * 8.0,
                })
                .collect(),
        }
    }

    /// Deterministic wobble in [-1, 1].
    pub(crate) fn wobble(seed: u32, i: usize) -> f32 {
        let v = (seed.wrapping_mul(2_654_435_761) ^ (i as u32).wrapping_mul(40_503)) % 2001;
        v as f32 / 1000.0 - 1.0
    }

    /// One hand-drawn pass along a polyline, offset and jittered.
    pub(crate) fn pass(path: &[(f32, f32)], offset: f32, jitter: f32, seed: u32) -> Stroke {
        let pts = resample(path, 6.0);
        let n = pts.len();
        let out: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let a = pts[i.saturating_sub(1)];
                let b = pts[(i + 1).min(n - 1)];
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l = dx.hypot(dy).max(1e-6);
                let (nx, ny) = (-dy / l, dx / l);
                let o = offset + jitter * wobble(seed, i);
                (pts[i].0 + nx * o, pts[i].1 + ny * o)
            })
            .collect();
        stroke(&out)
    }

    fn ink_rows(g: &Gray, x: usize) -> Vec<usize> {
        (0..g.height as usize)
            .filter(|&y| g.pixels[y * g.width as usize + x] < 128)
            .collect()
    }

    #[test]
    fn hairy_passes_collapse_to_one_line() {
        let path = [(100.0, 300.0), (700.0, 300.0)];
        let hairy: Vec<Stroke> = (0..5)
            .map(|k| pass(&path, (k as f32 - 2.0) * 2.5, 1.5, k))
            .collect();
        let c = consolidate(&hairy, 800, 600, |_, _| true).unwrap();
        let rows = ink_rows(&c.image, 400);
        // five passes ~10 px apart become one ~3 px line
        assert!(!rows.is_empty() && rows.len() <= 6, "rows: {rows:?}");
        assert!(c.passes > 3.5, "passes {}", c.passes);

        let clean = consolidate(&[pass(&path, 0.0, 0.0, 0)], 800, 600, |_, _| true).unwrap();
        assert!(clean.passes < 1.2);
    }

    #[test]
    fn crossing_lines_stay_separate() {
        let h = pass(&[(100.0, 300.0), (700.0, 300.0)], 0.0, 0.0, 1);
        let v = pass(&[(400.0, 50.0), (400.0, 550.0)], 0.0, 0.0, 2);
        let c = consolidate(&[h, v], 800, 600, |_, _| true).unwrap();
        // the vertical line is still there, away from the crossing
        assert!(ink_rows(&c.image, 400).iter().any(|&y| y > 500));
        assert!(c.passes < 1.3, "{}", c.passes);
    }

    #[test]
    fn erased_parts_stay_erased() {
        let line = pass(&[(100.0, 300.0), (700.0, 300.0)], 0.0, 0.0, 0);
        // the drawing has no ink right of x = 400
        let c = consolidate(&[line], 800, 600, |x, _| x < 400.0).unwrap();
        assert!(!ink_rows(&c.image, 300).is_empty());
        assert!(ink_rows(&c.image, 600).is_empty());
        assert!(consolidate(&[], 800, 600, |_, _| true).is_none());
    }
}
