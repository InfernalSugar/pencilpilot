//! Stroke-quality metrics from the recorded pointer input.
//!
//! * **smoothness** — curvature jitter: the path is resampled every
//!   [`RESAMPLE`] px and we take the RMS change of turning angle between
//!   neighbouring samples. Smooth curves turn steadily (small change),
//!   shaky or hairy lines flip direction constantly.
//! * **retrace ratio** — share of the path laid over cells (4 px) that were
//!   already inked earlier, by another stroke or by the same stroke going
//!   back and forth.
//! * **speed consistency** — coefficient of variation of pen speed within
//!   each stroke, length-weighted.
//!
//! `line_confidence` blends the three into one 0–1 value.

use serde::{Deserialize, Serialize};

/// One recorded input sample (canvas pixels, pressure 0–1, ms).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub pressure: f32,
    #[serde(default)]
    pub t: f64,
}

/// A stroke as recorded by the web app (`apps/web/src/lib/paint/strokes.ts`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stroke {
    pub brush_id: String,
    pub eraser: bool,
    pub points: Vec<StrokePoint>,
}

/// Raw stroke metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeMetrics {
    pub stroke_count: usize,
    /// RMS change of turning angle (radians) between resampled points.
    pub curvature_jitter: f32,
    pub smoothness: f32,
    /// 0–1, share of path length laid over already-inked cells.
    pub retrace_ratio: f32,
    pub retrace_score: f32,
    /// Length-weighted coefficient of variation of speed.
    pub speed_cv: f32,
    pub speed_consistency: f32,
    pub line_confidence: f32,
}

const RESAMPLE: f32 = 4.0;
const CELL: f32 = 4.0;
/// Same-stroke revisits closer than this many samples are not retraces.
const SELF_GAP: usize = 6;
const JITTER_SCALE: f32 = 0.35;
const RETRACE_FREE: f32 = 0.05;
const RETRACE_SCALE: f32 = 0.25;
const CV_SCALE: f32 = 1.0;

fn resample(pts: &[StrokePoint], step: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let Some(first) = pts.first() else {
        return out;
    };
    out.push((first.x, first.y));
    let mut prev = (first.x, first.y);
    let mut carry = 0.0f32;
    for p in &pts[1..] {
        let (mut ax, mut ay) = prev;
        let (bx, by) = (p.x, p.y);
        let mut seg = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        while carry + seg >= step && seg > 0.0 {
            let need = step - carry;
            let k = need / seg;
            ax += (bx - ax) * k;
            ay += (by - ay) * k;
            out.push((ax, ay));
            seg -= need;
            carry = 0.0;
        }
        carry += seg;
        prev = (bx, by);
    }
    out
}

fn wrap_angle(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

fn path_len(p: &[StrokePoint]) -> f32 {
    p.windows(2)
        .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
        .sum()
}

/// Compute stroke metrics. Eraser strokes are ignored. Returns `None` when
/// there is not enough input to say anything.
pub fn stroke_metrics(strokes: &[Stroke], width: u32, height: u32) -> Option<StrokeMetrics> {
    let strokes: Vec<&Stroke> = strokes
        .iter()
        .filter(|s| !s.eraser && s.points.len() >= 2)
        .collect();
    if strokes.is_empty() {
        return None;
    }

    // Smoothness.
    let (mut jsum, mut jn) = (0.0f32, 0usize);
    for s in &strokes {
        let r = resample(&s.points, RESAMPLE);
        if r.len() < 4 {
            continue;
        }
        let headings: Vec<f32> = r
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).atan2(w[1].0 - w[0].0))
            .collect();
        let turns: Vec<f32> = headings
            .windows(2)
            .map(|w| wrap_angle(w[1] - w[0]))
            .collect();
        for w in turns.windows(2) {
            jsum += (w[1] - w[0]).powi(2);
            jn += 1;
        }
    }
    let curvature_jitter = if jn > 0 {
        (jsum / jn as f32).sqrt()
    } else {
        0.0
    };
    let smoothness = (-curvature_jitter / JITTER_SCALE).exp();

    // Retrace ratio on a coarse occupancy grid.
    let gw = (width as f32 / CELL).ceil().max(1.0) as usize;
    let gh = (height as f32 / CELL).ceil().max(1.0) as usize;
    let mut owner: Vec<(u32, u32)> = vec![(u32::MAX, 0); gw * gh];
    let (mut total, mut retraced) = (0usize, 0usize);
    for (si, s) in strokes.iter().enumerate() {
        let r = resample(&s.points, CELL * 0.5);
        let mut last_cell = usize::MAX;
        for (pi, &(x, y)) in r.iter().enumerate() {
            let cx = ((x / CELL).floor().max(0.0) as usize).min(gw - 1);
            let cy = ((y / CELL).floor().max(0.0) as usize).min(gh - 1);
            let c = cy * gw + cx;
            if c == last_cell {
                continue;
            }
            last_cell = c;
            total += 1;
            let (os, op) = owner[c];
            if os != u32::MAX && (os != si as u32 || pi.saturating_sub(op as usize) > SELF_GAP) {
                retraced += 1;
            }
            owner[c] = (si as u32, pi as u32);
        }
    }
    let retrace_ratio = if total > 0 {
        retraced as f32 / total as f32
    } else {
        0.0
    };
    let retrace_score = (-(retrace_ratio - RETRACE_FREE).max(0.0) / RETRACE_SCALE).exp();

    // Speed consistency.
    let (mut cv_sum, mut w_sum) = (0.0f32, 0.0f32);
    for s in &strokes {
        let mut speeds = Vec::new();
        let (mut acc_d, mut acc_t) = (0.0f32, 0.0f64);
        for w in s.points.windows(2) {
            acc_d += ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            acc_t += w[1].t - w[0].t;
            if acc_t >= 8.0 {
                speeds.push(acc_d / acc_t as f32);
                acc_d = 0.0;
                acc_t = 0.0;
            }
        }
        if speeds.len() < 3 {
            continue;
        }
        let n = speeds.len() as f32;
        let mean = speeds.iter().sum::<f32>() / n;
        if mean <= 0.0 {
            continue;
        }
        let sd = (speeds.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt();
        let len = path_len(&s.points);
        cv_sum += sd / mean * len;
        w_sum += len;
    }
    let speed_cv = if w_sum > 0.0 { cv_sum / w_sum } else { 0.0 };
    let speed_consistency = (-speed_cv / CV_SCALE).exp();

    let line_confidence = 0.45 * smoothness + 0.35 * retrace_score + 0.2 * speed_consistency;
    Some(StrokeMetrics {
        stroke_count: strokes.len(),
        curvature_jitter,
        smoothness,
        retrace_ratio,
        retrace_score,
        speed_cv,
        speed_consistency,
        line_confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(pts: &[(f32, f32)], dt: f64) -> Stroke {
        Stroke {
            brush_id: "pen".into(),
            eraser: false,
            points: pts
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| StrokePoint {
                    x,
                    y,
                    pressure: 0.5,
                    t: i as f64 * dt,
                })
                .collect(),
        }
    }

    #[test]
    fn straight_line_is_smooth_and_not_retraced() {
        let pts: Vec<_> = (0..100).map(|i| (100.0 + i as f32 * 3.0, 200.0)).collect();
        let m = stroke_metrics(&[stroke(&pts, 10.0)], 800, 600).unwrap();
        assert!(m.smoothness > 0.99, "{m:?}");
        assert!(m.retrace_ratio < 0.01, "{m:?}");
        assert!(m.speed_consistency > 0.99, "{m:?}");
    }

    #[test]
    fn back_and_forth_is_retrace() {
        let mut pts: Vec<_> = (0..60).map(|i| (100.0 + i as f32 * 3.0, 200.0)).collect();
        pts.extend((0..60).rev().map(|i| (100.0 + i as f32 * 3.0, 201.0)));
        let m = stroke_metrics(&[stroke(&pts, 10.0)], 800, 600).unwrap();
        assert!(m.retrace_ratio > 0.35, "{m:?}");
    }

    #[test]
    fn eraser_and_empty_ignored() {
        let mut s = stroke(&[(0.0, 0.0), (5.0, 5.0)], 10.0);
        s.eraser = true;
        assert!(stroke_metrics(&[s], 800, 600).is_none());
        assert!(stroke_metrics(&[], 800, 600).is_none());
    }
}
