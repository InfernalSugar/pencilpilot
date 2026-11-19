//! Does the drawing depict the reference at all?
//!
//! Coverage-style measures (silhouette, blurred tone, chamfer recall) reward
//! any mass of ink in the right place: dense scribbles over the subject's
//! area cover every reference edge and fill the silhouette. What they don't
//! do is *run the same way* as the reference's lines.
//!
//! So we compare line directions where they are: each image becomes a field
//! of local line directions (doubled-angle vectors `(cos 2θ, sin 2θ)` from
//! the smoothed structure tensor, scaled by coherence), kept only on the
//! drawing's lines and on the reference's edges (weighted by edge strength),
//! blurred by the outline tolerance σ so small wobbles and offsets still
//! line up, and correlated (normalised dot product) over the area near the
//! drawing. A copy, even a rough or partial one, scores ≈ 0.6–1. Random
//! scribbles and hatching score ≈ 0 because their directions don't follow
//! the reference: in doubled-angle form unrelated directions cancel out.
//! Plain maths (ADR-0002).
//!
//! Sketch mode uses the same tolerance (not the sketch's 3σ): wider blurs let
//! random lines line up by chance. Known gap: parallel hatching that happens
//! to run along a subject's dominant direction (vertical lines over a tall,
//! upright cat) can still look related; telling that apart from a genuine
//! drawing of a one-direction subject needs more than line directions.
//! Fixtures: `tests/relevance.rs`; real references: `calibrate relevance`.

use crate::ScoreCurve;
use crate::img::{Mask, Plane, blur, dilate};

/// Correlation at or below this: the drawing is unrelated (relevance 0).
pub(crate) const UNRELATED: f32 = 0.33;
/// Correlation at or above this: the drawing clearly depicts the reference
/// (relevance 1). Perfect copies of the calibration references score
/// 0.64–0.98; rough ones (wobbly, partial) mostly stay above this.
pub(crate) const RELATED: f32 = 0.55;

/// Fewer drawn line pixels (working resolution) than this: nothing to judge.
const MIN_LINE_PIXELS: usize = 20;

/// Local line direction as a doubled-angle vector scaled by coherence (0 where
/// directions are mixed or flat), from the smoothed structure tensor.
fn orientation_field(tone: &Plane) -> (Plane, Plane) {
    let (w, h) = (tone.w, tone.h);
    let b = blur(tone, 1.0);
    let mut a = Plane::filled(w, h, 0.0);
    let mut c = Plane::filled(w, h, 0.0);
    let mut t = Plane::filled(w, h, 0.0);
    if w >= 3 && h >= 3 {
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let p = |dx: isize, dy: isize| {
                    b.d[((y as isize + dy) as usize) * w + (x as isize + dx) as usize]
                };
                let gx =
                    (p(1, -1) + 2.0 * p(1, 0) + p(1, 1)) - (p(-1, -1) + 2.0 * p(-1, 0) + p(-1, 1));
                let gy =
                    (p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)) - (p(-1, -1) + 2.0 * p(0, -1) + p(1, -1));
                let i = y * w + x;
                a.d[i] = gx * gx - gy * gy;
                c.d[i] = 2.0 * gx * gy;
                t.d[i] = gx * gx + gy * gy;
            }
        }
    }
    let (a, c, t) = (blur(&a, 1.0), blur(&c, 1.0), blur(&t, 1.0));
    let mut u = Plane::filled(w, h, 0.0);
    let mut v = Plane::filled(w, h, 0.0);
    for i in 0..w * h {
        if t.d[i] > 1e-3 {
            u.d[i] = a.d[i] / t.d[i];
            v.d[i] = c.d[i] / t.d[i];
        }
    }
    (u, v)
}

/// Line-direction correlation (−1…1, ≈ 0 for unrelated drawings) between
/// the drawing's lines `d_lines` and the reference's edges `r_edges`
/// (weighted by `r_strength`), blurred by `sigma` low-res px and compared
/// within `2·sigma` of the drawing. `None` when the drawing has (almost) no
/// lines (e.g. painted edge to edge) or the reference no edges.
pub(crate) fn structure_correlation(
    d_tone: &Plane,
    d_lines: &Mask,
    r_tone: &Plane,
    r_edges: &Mask,
    r_strength: &Plane,
    sigma: f32,
) -> Option<f32> {
    if d_lines.count() < MIN_LINE_PIXELS {
        return None;
    }
    let (w, h) = (d_tone.w, d_tone.h);
    let (du, dv) = orientation_field(d_tone);
    let (ru, rv) = orientation_field(r_tone);
    let (_, smax) = r_strength.min_max();
    let smax = smax.max(1e-3);
    let mut fdu = Plane::filled(w, h, 0.0);
    let mut fdv = Plane::filled(w, h, 0.0);
    let mut fru = Plane::filled(w, h, 0.0);
    let mut frv = Plane::filled(w, h, 0.0);
    for i in 0..w * h {
        if d_lines.d[i] {
            fdu.d[i] = du.d[i];
            fdv.d[i] = dv.d[i];
        }
        if r_edges.d[i] {
            // strong edges count more, faint detail a little
            let k = (r_strength.d[i] / smax).max(0.05);
            fru.d[i] = k * ru.d[i];
            frv.d[i] = k * rv.d[i];
        }
    }
    let (fdu, fdv, fru, frv) = (
        blur(&fdu, sigma),
        blur(&fdv, sigma),
        blur(&fru, sigma),
        blur(&frv, sigma),
    );
    // Only near the drawing: parts the student left out (or background the
    // reference has and the student rightly skipped) are not counted here.
    let near = dilate(d_lines, 2.0 * sigma);
    let (mut sab, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..w * h {
        if !near.d[i] {
            continue;
        }
        let (a0, a1) = (fdu.d[i] as f64, fdv.d[i] as f64);
        let (b0, b1) = (fru.d[i] as f64, frv.d[i] as f64);
        sab += a0 * b0 + a1 * b1;
        saa += a0 * a0 + a1 * a1;
        sbb += b0 * b0 + b1 * b1;
    }
    (saa > 1e-9 && sbb > 1e-9).then(|| (sab / (saa.sqrt() * sbb.sqrt())) as f32)
}

/// Relevance 0–1 from the correlation: smoothstep from [`UNRELATED`] to
/// [`RELATED`].
pub(crate) fn relevance(correlation: f32) -> f32 {
    let t = ((correlation - UNRELATED) / (RELATED - UNRELATED)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Highest score (1–10) a drawing with relevance 0 can get.
/// (Above it, similarity is squeezed by [`ABOVE_CEILING`], so a better
/// drawing still scores a little higher: ≈ 1.7–1.9.)
pub(crate) const UNRELATED_CEILING: f32 = 1.7;
/// Share of the similarity above the ceiling an unrelated drawing keeps.
const ABOVE_CEILING: f32 = 0.02;
/// Below this relevance the grade is flagged `unrelated` (and the feedback
/// engine shows only the "doesn't look like the reference" tip).
pub(crate) const FLAG_BELOW: f32 = 0.5;

/// Blend the similarity towards the one that scores [`UNRELATED_CEILING`]
/// as relevance falls, so score, `passed` and similarity stay consistent.
/// Order is kept: a higher similarity never ends up lower.
pub(crate) fn cap(similarity: f32, relevance: f32, curve: &ScoreCurve) -> f32 {
    let t = ((UNRELATED_CEILING - 1.0) / 9.0).powf(1.0 / curve.gamma.max(0.05));
    let ceiling = curve.floor + (curve.pass - curve.floor) * t;
    let squeezed = if similarity > ceiling {
        ceiling + ABOVE_CEILING * (similarity - ceiling)
    } else {
        similarity
    };
    relevance * similarity + (1.0 - relevance) * squeezed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_ramp_and_cap() {
        assert_eq!(relevance(0.0), 0.0);
        assert_eq!(relevance(UNRELATED), 0.0);
        assert_eq!(relevance(RELATED), 1.0);
        assert_eq!(relevance(0.9), 1.0);
        let mid = relevance(0.44);
        assert!(mid > 0.4 && mid < 0.6, "{mid}");
        let c = ScoreCurve::default();
        assert_eq!(cap(0.8, 1.0, &c), 0.8);
        let top = c.score(cap(1.0, 0.0, &c));
        assert!(top > UNRELATED_CEILING && top < 2.0, "{top}");
        assert!(cap(0.9, 0.0, &c) > cap(0.8, 0.0, &c));
        assert_eq!(cap(0.3, 0.0, &c), 0.3);
        assert!(cap(0.8, 0.5, &c) > cap(0.8, 0.2, &c));
    }
}
