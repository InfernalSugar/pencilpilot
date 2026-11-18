//! Turning images into comparable line maps.
//!
//! * Reference (photo or illustration): Canny-style edges — light blur,
//!   Sobel gradient, non-maximum suppression, hysteresis with thresholds
//!   relative to the strongest edges, and suppression of the artificial
//!   frame left by letterboxing.
//! * Drawing (line art + shading): dark pixels are ink; for the outline we
//!   keep only the ink near a non-ink pixel, so filled/shaded areas count
//!   by their boundary while thin lines are kept whole.

use crate::img::{Mask, Plane, blur, distance_transform};

/// Pixels darker than this (0–255) count as drawing ink. The canvas paper is
/// pure white (255), so any visible mark counts, including light pencil
/// strokes made with a mouse, which MyPaint treats as half pressure.
pub(crate) const INK_THRESHOLD: f32 = 232.0;

/// Ink pixels deeper than this (low-res px) inside a filled area are not
/// part of the outline.
const OUTLINE_BAND: f32 = 2.0;

/// Ink mask from a darkest-pixel downsample.
pub(crate) fn ink_mask(min_plane: &Plane) -> Mask {
    Mask {
        w: min_plane.w,
        h: min_plane.h,
        d: min_plane.d.iter().map(|&v| v < INK_THRESHOLD).collect(),
    }
}

/// The part of the ink that forms lines/boundaries.
pub(crate) fn ink_outline(ink: &Mask) -> Mask {
    let mut not_ink = ink.clone();
    not_ink.d.iter_mut().for_each(|b| *b = !*b);
    let dt = distance_transform(&not_ink);
    Mask {
        w: ink.w,
        h: ink.h,
        d: ink
            .d
            .iter()
            .zip(&dt.d)
            .map(|(&i, &d)| i && d <= OUTLINE_BAND)
            .collect(),
    }
}

/// Edge maps of the reference at two strictness levels.
pub(crate) struct ReferenceEdges {
    /// The strongest edges (hysteresis relative to the strongest edge): the
    /// lines every copy is expected to contain. Used for recall.
    pub strict: Mask,
    /// Strict edges plus clearly visible detail (fur, folds, features):
    /// lines a careful copy *may* contain. Used for precision, so correct
    /// detail is not counted as "extra".
    pub detail: Mask,
    /// Non-maximum-suppressed gradient magnitude (0 off the edge ridges).
    pub strength: Plane,
    /// Band along a letterbox border (the picture's frame), empty if the
    /// reference was not letterboxed.
    pub frame: Mask,
}

/// Edge map of the reference (low-res tone plane, 0–255): the strict map.
pub(crate) fn reference_edges(tone: &Plane) -> Mask {
    reference_edge_maps(tone).strict
}

/// Thresholds of the strict map, relative to the strongest edge (with an
/// absolute floor so flat or noisy-but-faint images give no edges).
const STRICT_HIGH: f32 = 0.22;
const STRICT_LOW: f32 = 0.4;
/// Thresholds of the detail map.
const DETAIL_HIGH: f32 = 0.10;
const DETAIL_LOW: f32 = 0.5;

/// Strict and detail edge maps of the reference.
pub(crate) fn reference_edge_maps(tone: &Plane) -> ReferenceEdges {
    let (w, h) = (tone.w, tone.h);
    if w < 3 || h < 3 {
        return ReferenceEdges {
            strict: Mask::new(w, h),
            detail: Mask::new(w, h),
            strength: Plane::filled(w, h, 0.0),
            frame: Mask::new(w, h),
        };
    }
    let thin = gradient_ridges(tone);
    let (_, max) = thin.min_max();
    let high = (STRICT_HIGH * max).max(6.0);
    let mut strict = hysteresis(&thin, high, (STRICT_LOW * high).max(3.0));
    let dhigh = (DETAIL_HIGH * max).max(6.0).min(high);
    let mut detail = hysteresis(&thin, dhigh, (DETAIL_LOW * dhigh).max(3.0));
    let frame = letterbox_frame(tone);
    for m in [&mut strict, &mut detail] {
        for (e, f) in m.d.iter_mut().zip(&frame.d) {
            *e &= !*f;
        }
    }
    for (e, s) in detail.d.iter_mut().zip(&strict.d) {
        *e |= *s;
    }
    ReferenceEdges {
        strict,
        detail,
        strength: thin,
        frame,
    }
}

/// Sobel gradient magnitude after a light blur, thinned to one-pixel ridges
/// by non-maximum suppression.
fn gradient_ridges(tone: &Plane) -> Plane {
    let (w, h) = (tone.w, tone.h);
    let b = blur(tone, 1.0);
    let mut mag = Plane::filled(w, h, 0.0);
    let mut dir = vec![0u8; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let p = |dx: isize, dy: isize| {
                b.d[((y as isize + dy) as usize) * w + (x as isize + dx) as usize]
            };
            let gx = (p(1, -1) + 2.0 * p(1, 0) + p(1, 1)) - (p(-1, -1) + 2.0 * p(-1, 0) + p(-1, 1));
            let gy = (p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)) - (p(-1, -1) + 2.0 * p(0, -1) + p(1, -1));
            let m = (gx * gx + gy * gy).sqrt() / 8.0;
            mag.d[y * w + x] = m;
            // Quantise gradient direction to 0°, 45°, 90°, 135°.
            let a = gy.atan2(gx).to_degrees().rem_euclid(180.0);
            dir[y * w + x] = if !(22.5..157.5).contains(&a) {
                0
            } else if a < 67.5 {
                1
            } else if a < 112.5 {
                2
            } else {
                3
            };
        }
    }
    // Non-maximum suppression.
    let mut thin = Plane::filled(w, h, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let m = mag.d[i];
            if m <= 0.0 {
                continue;
            }
            let (a, c) = match dir[i] {
                0 => (mag.d[i - 1], mag.d[i + 1]),
                1 => (mag.d[i - w - 1], mag.d[i + w + 1]),
                2 => (mag.d[i - w], mag.d[i + w]),
                _ => (mag.d[i - w + 1], mag.d[i + w - 1]),
            };
            if m >= a && m > c {
                thin.d[i] = m;
            }
        }
    }
    thin
}

/// Ridges ≥ `high`, grown along connected ridges ≥ `low`.
fn hysteresis(thin: &Plane, high: f32, low: f32) -> Mask {
    let (w, h) = (thin.w, thin.h);
    let mut out = Mask::new(w, h);
    let mut stack: Vec<usize> = Vec::new();
    for (i, &m) in thin.d.iter().enumerate() {
        if m >= high {
            out.d[i] = true;
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        let (x, y) = ((i % w) as isize, (i / w) as isize);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if !out.d[j] && thin.d[j] >= low {
                    out.d[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    out
}

/// If the picture was letterboxed onto white, the border between the white
/// bars and the picture is a strong straight edge that is not part of the
/// subject. Detect bars (all-white margin rows/columns ending in a
/// row/column that is mostly non-white) and return a band along that
/// border: its edges are ignored in the reference, and a student who traces
/// the picture's frame is neither rewarded nor penalised for it.
pub(crate) fn letterbox_frame(tone: &Plane) -> Mask {
    let (w, h) = (tone.w, tone.h);
    let mut frame = Mask::new(w, h);
    let white = |v: f32| v >= 248.0;
    let row_white = |y: usize| (0..w).all(|x| white(tone.at(x, y)));
    let col_white = |x: usize| (0..h).all(|y| white(tone.at(x, y)));
    let row_solid = |y: usize| (0..w).filter(|&x| !white(tone.at(x, y))).count() * 10 >= w * 8;
    let col_solid = |x: usize| (0..h).filter(|&y| !white(tone.at(x, y))).count() * 10 >= h * 8;
    const BAND: usize = 2;
    let rows = |y0: usize, frame: &mut Mask| {
        for y in y0.saturating_sub(BAND)..(y0 + BAND + 1).min(h) {
            for x in 0..w {
                frame.d[y * w + x] = true;
            }
        }
    };
    if let Some(top) = (0..h).find(|&y| !row_white(y))
        && top >= 2
        && row_solid(top)
    {
        rows(top, &mut frame);
    }
    if let Some(bot) = (0..h).rev().find(|&y| !row_white(y))
        && bot + 3 <= h
        && row_solid(bot)
    {
        rows(bot, &mut frame);
    }
    let cols = |x0: usize, frame: &mut Mask| {
        for x in x0.saturating_sub(BAND)..(x0 + BAND + 1).min(w) {
            for y in 0..h {
                frame.d[y * w + x] = true;
            }
        }
    };
    if let Some(left) = (0..w).find(|&x| !col_white(x))
        && left >= 2
        && col_solid(left)
    {
        cols(left, &mut frame);
    }
    if let Some(right) = (0..w).rev().find(|&x| !col_white(x))
        && right + 3 <= w
        && col_solid(right)
    {
        cols(right, &mut frame);
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterboxed_photo_frame_is_not_an_edge() {
        // 60×40, white bars top/bottom, grey photo band with a dark square.
        let (w, h) = (60, 40);
        let mut p = Plane::filled(w, h, 255.0);
        for y in 8..32 {
            for x in 0..w {
                p.d[y * w + x] = 180.0;
            }
        }
        for y in 14..26 {
            for x in 24..36 {
                p.d[y * w + x] = 30.0;
            }
        }
        let e = reference_edges(&p);
        let rows_with_edges = |y: usize| (0..w).filter(|&x| e.d[y * w + x]).count();
        assert!(rows_with_edges(8) + rows_with_edges(7) < 5);
        assert!(e.count() > 20, "square edges kept");
    }
}
