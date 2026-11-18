//! Auto-alignment of the drawing onto the reference.
//!
//! The transform is a similarity about the canvas centre `c`:
//! `p_ref = c + s·R(θ)·(p_drawing − c) + (dx, dy)`.
//! It is first estimated from moments (centroid and RMS spread of the
//! drawing's outline vs the reference edges), then refined by a bounded
//! pattern search that minimises the symmetric mean chamfer distance.

use crate::Gray;
use crate::img::{FAR, Plane};

/// Bounds of the refinement search.
pub(crate) const SCALE_RANGE: f32 = 0.15;
pub(crate) const MAX_ROTATION_DEG: f32 = 10.0;
pub(crate) const MIN_SCALE: f32 = 0.5;
pub(crate) const MAX_SCALE: f32 = 2.0;
/// Max translation as a fraction of the canvas size.
pub(crate) const MAX_SHIFT: f32 = 0.35;
/// Chamfer distances are capped here (low-res px) so stray marks don't dominate.
pub(crate) const CAP: f32 = 25.0;
const MAX_POINTS: usize = 1500;
/// Cap for the robust refinement stage (low-res px).
const ROBUST_CAP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Similarity {
    pub scale: f32,
    /// radians
    pub rotation: f32,
    pub dx: f32,
    pub dy: f32,
    pub cx: f32,
    pub cy: f32,
}

impl Similarity {
    pub fn identity(cx: f32, cy: f32) -> Self {
        Self {
            scale: 1.0,
            rotation: 0.0,
            dx: 0.0,
            dy: 0.0,
            cx,
            cy,
        }
    }

    #[inline]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let (s, c) = self.rotation.sin_cos();
        let (ux, uy) = (x - self.cx, y - self.cy);
        (
            self.cx + self.scale * (c * ux - s * uy) + self.dx,
            self.cy + self.scale * (s * ux + c * uy) + self.dy,
        )
    }

    #[inline]
    pub fn invert(&self, x: f32, y: f32) -> (f32, f32) {
        let (s, c) = self.rotation.sin_cos();
        let (ux, uy) = (
            (x - self.cx - self.dx) / self.scale,
            (y - self.cy - self.dy) / self.scale,
        );
        (self.cx + c * ux + s * uy, self.cy - s * ux + c * uy)
    }

    /// Same transform expressed in a coordinate system `k` times larger.
    pub fn scaled(&self, k: f32) -> Self {
        Self {
            dx: self.dx * k,
            dy: self.dy * k,
            cx: self.cx * k,
            cy: self.cy * k,
            ..*self
        }
    }
}

fn subsample(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let step = p.len().div_ceil(MAX_POINTS).max(1);
    p.iter().step_by(step).copied().collect()
}

/// Centroid and RMS radius.
pub(crate) fn moments(p: &[(f32, f32)]) -> ((f32, f32), f32) {
    let n = p.len().max(1) as f32;
    let (sx, sy) = p.iter().fold((0.0, 0.0), |(a, b), &(x, y)| (a + x, b + y));
    let (mx, my) = (sx / n, sy / n);
    let var = p
        .iter()
        .map(|&(x, y)| (x - mx).powi(2) + (y - my).powi(2))
        .sum::<f32>()
        / n;
    ((mx, my), var.sqrt())
}

struct Problem<'a> {
    drawing_pts: Vec<(f32, f32)>,
    reference_pts: Vec<(f32, f32)>,
    dt_ref: &'a Plane,
    dt_drawing: &'a Plane,
    cap: f32,
}

impl Problem<'_> {
    fn cost(&self, t: &Similarity) -> f32 {
        let fwd: f32 = self
            .drawing_pts
            .iter()
            .map(|&(x, y)| {
                let (u, v) = t.apply(x, y);
                self.dt_ref.sample(u, v, self.cap).min(self.cap)
            })
            .sum::<f32>()
            / self.drawing_pts.len().max(1) as f32;
        let bwd: f32 = self
            .reference_pts
            .iter()
            .map(|&(x, y)| {
                let (u, v) = t.invert(x, y);
                (t.scale * self.dt_drawing.sample(u, v, FAR)).min(self.cap)
            })
            .sum::<f32>()
            / self.reference_pts.len().max(1) as f32;
        0.5 * (fwd + bwd)
    }
}

/// Result of [`align`]: the transform (low-res coordinates) and whether a
/// search bound was hit.
pub(crate) struct AlignResult {
    pub t: Similarity,
    pub clamped: bool,
}

/// Estimate the drawing → reference transform on low-res outline maps.
pub(crate) fn align(
    drawing_pts: &[(f32, f32)],
    reference_pts: &[(f32, f32)],
    dt_drawing: &Plane,
    dt_ref: &Plane,
) -> AlignResult {
    let (w, h) = (dt_ref.w as f32, dt_ref.h as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let id = Similarity::identity(cx, cy);
    if drawing_pts.is_empty() || reference_pts.is_empty() {
        return AlignResult {
            t: id,
            clamped: false,
        };
    }
    let mut prob = Problem {
        drawing_pts: subsample(drawing_pts),
        reference_pts: subsample(reference_pts),
        dt_ref,
        dt_drawing,
        cap: CAP,
    };
    let ((mdx, mdy), sd) = moments(drawing_pts);
    let ((mrx, mry), sr) = moments(reference_pts);
    let s0 = if sd > 0.5 {
        (sr / sd).clamp(MIN_SCALE, MAX_SCALE)
    } else {
        1.0
    };
    let centred = |s: f32| Similarity {
        scale: s,
        dx: mrx - cx - s * (mdx - cx),
        dy: mry - cy - s * (mdy - cy),
        ..id
    };
    let max_shift = (MAX_SHIFT * w, MAX_SHIFT * h);
    let clamp_shift = |t: Similarity| Similarity {
        dx: t.dx.clamp(-max_shift.0, max_shift.0),
        dy: t.dy.clamp(-max_shift.1, max_shift.1),
        ..t
    };
    // Starting points: moments, centroid-only, identity. Keep the best.
    let mut best = [centred(s0), centred(1.0), id]
        .into_iter()
        .map(clamp_shift)
        .map(|t| (prob.cost(&t), t))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("non-empty");
    // Scale may move ±15 % around the moment estimate (and around 1).
    let (a, b) = (s0.ln().min(0.0), s0.ln().max(0.0));
    let ln_lo = (a - (1.0 + SCALE_RANGE).ln()).max(MIN_SCALE.ln());
    let ln_hi = (b + (1.0 + SCALE_RANGE).ln()).min(MAX_SCALE.ln());
    let rot_max = MAX_ROTATION_DEG.to_radians();

    // Pattern search over (ln s, θ, dx, dy), in two stages: first with the
    // generous cap to get into the basin, then with a tight (robust) cap so
    // the bulk of the drawing is fitted and a single wrong part (a head
    // drawn too big) stays an outlier instead of being spread everywhere.
    let stages = [
        (CAP, [0.05f32, 3.0f32.to_radians(), 3.0, 3.0]),
        (ROBUST_CAP, [0.02f32, 1.0f32.to_radians(), 1.0, 1.0]),
    ];
    for (cap, mut steps) in stages {
        prob.cap = cap;
        best.0 = prob.cost(&best.1);
        let min_steps = [0.004f32, 0.25f32.to_radians(), 0.25, 0.25];
        let mut evals = 0;
        while evals < 600 {
            let mut improved = false;
            for k in 0..4 {
                for sign in [1.0f32, -1.0] {
                    let mut t = best.1;
                    match k {
                        0 => {
                            let ln = (t.scale.ln() + sign * steps[0]).clamp(ln_lo, ln_hi);
                            // Keep the drawing's centroid fixed while scaling.
                            let (px, py) = t.apply(mdx, mdy);
                            t.scale = ln.exp();
                            let (qx, qy) = t.apply(mdx, mdy);
                            t.dx += px - qx;
                            t.dy += py - qy;
                        }
                        1 => {
                            let (px, py) = t.apply(mdx, mdy);
                            t.rotation = (t.rotation + sign * steps[1]).clamp(-rot_max, rot_max);
                            let (qx, qy) = t.apply(mdx, mdy);
                            t.dx += px - qx;
                            t.dy += py - qy;
                        }
                        2 => t.dx += sign * steps[2],
                        _ => t.dy += sign * steps[3],
                    }
                    let t = clamp_shift(t);
                    evals += 1;
                    let c = prob.cost(&t);
                    if c + 1e-6 < best.0 {
                        best = (c, t);
                        improved = true;
                        break;
                    }
                }
            }
            if !improved {
                if steps.iter().zip(&min_steps).all(|(s, m)| s <= m) {
                    break;
                }
                for (s, m) in steps.iter_mut().zip(&min_steps) {
                    *s = (*s * 0.5).max(*m);
                }
            }
        }
    }
    let t = best.1;
    let eps = 1e-3;
    let clamped = t.scale.ln() <= ln_lo + eps
        || t.scale.ln() >= ln_hi - eps
        || t.rotation.abs() >= rot_max - eps
        || t.dx.abs() >= max_shift.0 - eps
        || t.dy.abs() >= max_shift.1 - eps;
    AlignResult { t, clamped }
}

/// Resample the full-resolution drawing into the reference frame
/// (bilinear, white outside).
pub(crate) fn warp(g: &Gray, t: &Similarity) -> Gray {
    let (w, h) = (g.width as usize, g.height as usize);
    let src = Plane {
        w,
        h,
        d: g.pixels.iter().map(|&p| p as f32).collect(),
    };
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = t.invert(x as f32 + 0.5, y as f32 + 0.5);
            out.push(src.sample(u, v, 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    Gray {
        width: g.width,
        height: g.height,
        pixels: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_undoes_apply() {
        let t = Similarity {
            scale: 1.3,
            rotation: 0.1,
            dx: 4.0,
            dy: -7.0,
            cx: 50.0,
            cy: 40.0,
        };
        let (x, y) = t.apply(12.0, 33.0);
        let (u, v) = t.invert(x, y);
        assert!((u - 12.0).abs() < 1e-3 && (v - 33.0).abs() < 1e-3);
    }
}
