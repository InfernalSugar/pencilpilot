//! Keypoint matching with a Procrustes (similarity) fit.
//!
//! The student's points are fitted onto the reference points with the best
//! scale + rotation + translation (no reflection, least squares). What is
//! left over after the fit is a *proportion/shape* error, independent of
//! where and how big the drawing is. Errors are expressed as a fraction of
//! the reference points' RMS radius so they don't depend on image size.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A reference point and where the student put the same feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeypointPair {
    pub reference: Point,
    pub drawing: Point,
}

/// Per-pair result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeypointError {
    /// Distance after the fit ÷ reference RMS radius.
    pub error: f32,
    /// Where the fitted drawing point landed (reference pixels).
    pub fitted: Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeypointResult {
    /// RMS of per-pair errors (0 = perfect proportions).
    pub keypoint_error: f32,
    pub max_error: f32,
    pub pairs: Vec<KeypointError>,
    /// For each consecutive pair of keypoints (i, i+1): signed angle (degrees)
    /// between the reference segment and the fitted drawing segment.
    pub angle_errors: Vec<f32>,
    pub mean_angle_error: f32,
    /// Fit parameters: drawing → reference.
    pub scale: f32,
    pub rotation: f32,
}

/// `keypoint_error` at which the similarity drops to 1/e.
pub(crate) const KEYPOINT_SIGMA: f32 = 0.12;

/// Procrustes fit. Needs at least two pairs with some spread.
pub fn fit_keypoints(pairs: &[KeypointPair]) -> Option<KeypointResult> {
    if pairs.len() < 2 {
        return None;
    }
    let n = pairs.len() as f32;
    let mean = |f: &dyn Fn(&KeypointPair) -> Point| {
        let (sx, sy) = pairs
            .iter()
            .map(f)
            .fold((0.0, 0.0), |(a, b), p| (a + p.x, b + p.y));
        Point {
            x: sx / n,
            y: sy / n,
        }
    };
    let mr = mean(&|p| p.reference);
    let md = mean(&|p| p.drawing);
    let (mut a, mut b, mut dd, mut rr) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for p in pairs {
        let (dx, dy) = (p.drawing.x - md.x, p.drawing.y - md.y);
        let (rx, ry) = (p.reference.x - mr.x, p.reference.y - mr.y);
        a += dx * rx + dy * ry;
        b += dx * ry - dy * rx;
        dd += dx * dx + dy * dy;
        rr += rx * rx + ry * ry;
    }
    if dd <= 1e-6 || rr <= 1e-6 {
        return None;
    }
    let rotation = b.atan2(a);
    let scale = (a * a + b * b).sqrt() / dd;
    let (s, c) = rotation.sin_cos();
    let fit = |p: Point| Point {
        x: mr.x + scale * (c * (p.x - md.x) - s * (p.y - md.y)),
        y: mr.y + scale * (s * (p.x - md.x) + c * (p.y - md.y)),
    };
    let radius = (rr / n).sqrt();
    let out: Vec<KeypointError> = pairs
        .iter()
        .map(|p| {
            let f = fit(p.drawing);
            let e = ((f.x - p.reference.x).powi(2) + (f.y - p.reference.y).powi(2)).sqrt();
            KeypointError {
                error: e / radius,
                fitted: f,
            }
        })
        .collect();
    let keypoint_error = (out.iter().map(|e| e.error * e.error).sum::<f32>() / n).sqrt();
    let max_error = out.iter().map(|e| e.error).fold(0.0, f32::max);
    let angle_errors: Vec<f32> = pairs
        .windows(2)
        .zip(out.windows(2))
        .map(|(p, f)| {
            let ar =
                (p[1].reference.y - p[0].reference.y).atan2(p[1].reference.x - p[0].reference.x);
            let ad = (f[1].fitted.y - f[0].fitted.y).atan2(f[1].fitted.x - f[0].fitted.x);
            let d = (ad - ar + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            d.to_degrees()
        })
        .collect();
    let mean_angle_error = if angle_errors.is_empty() {
        0.0
    } else {
        angle_errors.iter().map(|a| a.abs()).sum::<f32>() / angle_errors.len() as f32
    };
    Some(KeypointResult {
        keypoint_error,
        max_error,
        pairs: out,
        angle_errors,
        mean_angle_error,
        scale,
        rotation: rotation.to_degrees(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(rx: f32, ry: f32, dx: f32, dy: f32) -> KeypointPair {
        KeypointPair {
            reference: Point { x: rx, y: ry },
            drawing: Point { x: dx, y: dy },
        }
    }

    #[test]
    fn similarity_transform_has_zero_error() {
        let refs = [
            (100.0, 100.0),
            (200.0, 120.0),
            (150.0, 250.0),
            (90.0, 300.0),
        ];
        let (s, r, tx, ty) = (0.7f32, 0.3f32, 40.0, -20.0);
        let pairs: Vec<_> = refs
            .iter()
            .map(|&(x, y)| {
                let (sn, cs) = r.sin_cos();
                pair(x, y, s * (cs * x - sn * y) + tx, s * (sn * x + cs * y) + ty)
            })
            .collect();
        let k = fit_keypoints(&pairs).unwrap();
        assert!(k.keypoint_error < 1e-3, "{k:?}");
        assert!((k.scale - 1.0 / 0.7).abs() < 1e-3);
        assert!((k.rotation + 0.3f32.to_degrees()).abs() < 0.05);
        assert!(k.mean_angle_error < 0.05);
    }

    #[test]
    fn moved_point_is_the_worst() {
        // Eight points on a circle; the student moved point 2 outwards.
        let pairs: Vec<_> = (0..8)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (x, y) = (300.0 + 100.0 * a.cos(), 300.0 + 100.0 * a.sin());
                let k = if i == 2 { 1.5 } else { 1.0 };
                pair(
                    x,
                    y,
                    300.0 + k * 100.0 * a.cos(),
                    300.0 + k * 100.0 * a.sin(),
                )
            })
            .collect();
        let k = fit_keypoints(&pairs).unwrap();
        let worst = k
            .pairs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.error.total_cmp(&b.1.error))
            .unwrap()
            .0;
        assert_eq!(worst, 2);
        assert!(k.keypoint_error > 0.1);
    }

    #[test]
    fn degenerate_inputs() {
        assert!(fit_keypoints(&[pair(1.0, 1.0, 1.0, 1.0)]).is_none());
        assert!(fit_keypoints(&[pair(1.0, 1.0, 5.0, 5.0), pair(1.0, 1.0, 5.0, 5.0)]).is_none());
    }
}
