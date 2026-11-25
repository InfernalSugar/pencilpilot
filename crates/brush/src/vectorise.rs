//! Line art to strokes: the ink of a drawing (a rough, a guide) as centrelines with a
//! width at each point, ready to be drawn again through a brush ("Ink this stroke").
//!
//! The ink is thinned to a one-pixel skeleton (Zhang–Suen), the skeleton is traced into
//! paths between its ends and junctions (and round closed loops), short spurs left by
//! thinning are pruned, and each path is smoothed and simplified. A point's width comes
//! from the distance transform: twice the distance from the centreline to the paper.

use std::collections::VecDeque;

use crate::select::distance_to;

/// A point of a stroke: where (canvas px) and how wide the line is there (px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VPoint {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

/// A stroke: its points in drawing order (a closed loop ends where it began).
pub type VStroke = Vec<VPoint>;

/// Ink specks smaller than this (px) are dust, not lines.
const MIN_AREA: usize = 12;
/// How far a simplified stroke may stray from the traced one (px).
const SIMPLIFY: f32 = 0.6;

/// The strokes of the ink in `alpha` (one byte a pixel, `w` × `h`): pixels above
/// `threshold` are ink.
pub fn vectorise(alpha: &[u8], w: usize, h: usize, threshold: u8) -> Vec<VStroke> {
    if w < 3 || h < 3 || alpha.len() < w * h {
        return Vec::new();
    }
    let mut ink: Vec<bool> = alpha[..w * h].iter().map(|&a| a > threshold).collect();
    remove_specks(&mut ink, w, h, MIN_AREA);
    let paper: Vec<bool> = ink.iter().map(|&b| !b).collect();
    let dist = distance_to(&paper, w, h);
    let skel = thin(&ink, w, h);
    let paths = trace(&skel, w, h);
    let mut strokes: Vec<VStroke> = paths
        .into_iter()
        .map(|p| {
            p.into_iter()
                .map(|i| VPoint {
                    x: (i % w) as f32 + 0.5,
                    y: (i / w) as f32 + 0.5,
                    // (the distance to the paper is measured from pixel centres)
                    width: (2.0 * dist[i] - 1.0).max(1.0),
                })
                .collect::<VStroke>()
        })
        .collect();
    prune_spurs(&mut strokes);
    strokes
        .into_iter()
        .filter(|s| s.len() >= 2)
        .map(|s| simplify(&smooth(&s), SIMPLIFY))
        .collect()
}

const NB: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

fn at(m: &[bool], w: usize, h: usize, x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && m[y as usize * w + x as usize]
}

/// Drop 8-connected ink pieces smaller than `min` pixels.
fn remove_specks(ink: &mut [bool], w: usize, h: usize, min: usize) {
    let mut seen = vec![false; w * h];
    let mut queue = VecDeque::new();
    let mut piece = Vec::new();
    for start in 0..w * h {
        if !ink[start] || seen[start] {
            continue;
        }
        piece.clear();
        seen[start] = true;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            piece.push(i);
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            for (dx, dy) in NB {
                let (nx, ny) = (x + dx, y + dy);
                if at(ink, w, h, nx, ny) {
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] {
                        seen[j] = true;
                        queue.push_back(j);
                    }
                }
            }
        }
        if piece.len() < min {
            for &i in &piece {
                ink[i] = false;
            }
        }
    }
}

/// Zhang–Suen thinning: the ink worn down to a one-pixel skeleton, keeping its shape's
/// connections.
fn thin(ink: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut m = ink.to_vec();
    let mut gone = Vec::new();
    loop {
        let mut changed = false;
        for pass in 0..2 {
            gone.clear();
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    if !m[y as usize * w + x as usize] {
                        continue;
                    }
                    // P2..P9, clockwise from north
                    let p: [bool; 8] = NB.map(|(dx, dy)| at(&m, w, h, x + dx, y + dy));
                    let b = p.iter().filter(|&&v| v).count();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let a = (0..8).filter(|&k| !p[k] && p[(k + 1) % 8]).count();
                    if a != 1 {
                        continue;
                    }
                    let (n, e, s, wst) = (p[0], p[2], p[4], p[6]);
                    let ok = if pass == 0 {
                        !(n && e && s) && !(e && s && wst)
                    } else {
                        !(n && e && wst) && !(n && s && wst)
                    };
                    if ok {
                        gone.push(y as usize * w + x as usize);
                    }
                }
            }
            if !gone.is_empty() {
                changed = true;
                for &i in &gone {
                    m[i] = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
    m
}

/// A skeleton pixel's neighbours, the side ones first (a path takes those before
/// cutting a corner).
fn neighbours(m: &[bool], w: usize, h: usize, i: usize) -> Vec<usize> {
    let (x, y) = ((i % w) as i32, (i / w) as i32);
    let mut out = Vec::with_capacity(8);
    for k in [0, 2, 4, 6, 1, 3, 5, 7] {
        let (dx, dy) = NB[k];
        if at(m, w, h, x + dx, y + dy) {
            out.push((y + dy) as usize * w + (x + dx) as usize);
        }
    }
    out
}

/// How many separate branches meet at a skeleton pixel: 1 at an end, 2 along a path,
/// 3 or more at a junction (neighbours that touch each other count as one branch).
fn branches(m: &[bool], w: usize, h: usize, i: usize) -> usize {
    let (x, y) = ((i % w) as i32, (i / w) as i32);
    let p: [bool; 8] = NB.map(|(dx, dy)| at(m, w, h, x + dx, y + dy));
    let n = p.iter().filter(|&&v| v).count();
    if n <= 1 {
        return n;
    }
    // runs of set neighbours round the ring (a corner neighbour between two side ones
    // joins them)
    let runs = (0..8).filter(|&k| p[k] && !p[(k + 7) % 8]).count();
    if runs == 0 { 1 } else { runs }
}

/// The skeleton as paths: between ends and junctions, then round any loops left.
fn trace(skel: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let node: Vec<bool> = (0..w * h)
        .map(|i| skel[i] && branches(skel, w, h, i) != 2)
        .collect();
    let mut used = vec![false; w * h];
    let mut paths = Vec::new();
    // from each end or junction, along every branch not yet walked
    for start in 0..w * h {
        if !node[start] {
            continue;
        }
        for first in neighbours(skel, w, h, start) {
            if used[first] && !node[first] {
                continue;
            }
            if node[first] {
                // two nodes side by side: a path of two, walked once
                if start < first {
                    paths.push(vec![start, first]);
                }
                continue;
            }
            let mut path = vec![start, first];
            used[first] = true;
            let mut cur = first;
            loop {
                let next = neighbours(skel, w, h, cur)
                    .into_iter()
                    .find(|&n| n != path[path.len() - 2] && (node[n] || !used[n]));
                let Some(n) = next else { break };
                path.push(n);
                if node[n] {
                    break;
                }
                used[n] = true;
                cur = n;
            }
            paths.push(path);
        }
    }
    // loops: no ends or junctions on them
    for start in 0..w * h {
        if !skel[start] || node[start] || used[start] {
            continue;
        }
        let mut path = vec![start];
        used[start] = true;
        let mut cur = start;
        while let Some(n) = neighbours(skel, w, h, cur).into_iter().find(|&n| !used[n]) {
            path.push(n);
            used[n] = true;
            cur = n;
        }
        // (closed: back to the start)
        path.push(start);
        paths.push(path);
    }
    paths
}

fn length(s: &[VPoint]) -> f32 {
    s.windows(2)
        .map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y))
        .sum()
}

/// Spurs: short paths from a junction to a free end, left by thinning a thick line's
/// corners and ends. Gone when shorter than the line is wide (or 4 px).
fn prune_spurs(strokes: &mut Vec<VStroke>) {
    let ends: Vec<(f32, f32)> = strokes
        .iter()
        .flat_map(|s| [s[0], s[s.len() - 1]])
        .map(|p| (p.x, p.y))
        .collect();
    // how many paths meet at a point (a junction is where three or more do)
    let meets = |x: f32, y: f32| {
        ends.iter()
            .filter(|&&(ex, ey)| (ex - x).abs() < 0.01 && (ey - y).abs() < 0.01)
            .count()
    };
    let spurs: Vec<bool> = strokes
        .iter()
        .map(|s| {
            let (a, b) = (s[0], s[s.len() - 1]);
            let (ma, mb) = (meets(a.x, a.y), meets(b.x, b.y));
            let free_end = (ma == 1) != (mb == 1) && ma.max(mb) >= 3;
            let wide = s.iter().map(|p| p.width).fold(0.0f32, f32::max);
            free_end && length(s) < wide.max(4.0)
        })
        .collect();
    let mut k = 0;
    strokes.retain(|_| {
        k += 1;
        !spurs[k - 1]
    });
}

/// A moving average over three points (the ends stay put): pixel steps made smooth.
fn smooth(s: &[VPoint]) -> VStroke {
    if s.len() < 3 {
        return s.to_vec();
    }
    let mut out = Vec::with_capacity(s.len());
    out.push(s[0]);
    for k in 1..s.len() - 1 {
        let (a, b, c) = (s[k - 1], s[k], s[k + 1]);
        out.push(VPoint {
            x: (a.x + 2.0 * b.x + c.x) / 4.0,
            y: (a.y + 2.0 * b.y + c.y) / 4.0,
            width: (a.width + 2.0 * b.width + c.width) / 4.0,
        });
    }
    out.push(s[s.len() - 1]);
    out
}

/// Ramer–Douglas–Peucker: only the points the shape needs, within `eps` px.
fn simplify(s: &[VPoint], eps: f32) -> VStroke {
    if s.len() < 3 {
        return s.to_vec();
    }
    let mut keep = vec![false; s.len()];
    keep[0] = true;
    keep[s.len() - 1] = true;
    let mut stack = vec![(0, s.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (p, q) = (s[a], s[b]);
        let (dx, dy) = (q.x - p.x, q.y - p.y);
        let len = dx.hypot(dy);
        let mut far = (0.0f32, 0);
        for (k, r) in s.iter().enumerate().take(b).skip(a + 1) {
            let d = if len < 1e-6 {
                (r.x - p.x).hypot(r.y - p.y)
            } else {
                ((r.x - p.x) * dy - (r.y - p.y) * dx).abs() / len
            };
            // (a change of width matters too: keep where the line swells or thins)
            let dw = (r.width
                - (p.width + (q.width - p.width) * ((k - a) as f32 / (b - a) as f32)))
                .abs()
                / 2.0;
            let d = d.max(dw);
            if d > far.0 {
                far = (d, k);
            }
        }
        if far.0 > eps {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    s.iter()
        .zip(&keep)
        .filter_map(|(p, &k)| k.then_some(*p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a picture of ink: `draw(x, y)` says if a pixel is ink
    fn picture(w: usize, h: usize, draw: impl Fn(f32, f32) -> bool) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                if draw(x, y) { 255 } else { 0 }
            })
            .collect()
    }

    fn ends(s: &VStroke) -> ((f32, f32), (f32, f32)) {
        let (a, b) = (s[0], s[s.len() - 1]);
        ((a.x, a.y), (b.x, b.y))
    }

    #[test]
    fn a_thick_straight_line_is_one_stroke_down_its_middle_as_wide_as_it() {
        // 6 px wide, from x 20 to 180 at y 50
        let a = picture(200, 100, |x, y| {
            (20.0..180.0).contains(&x) && (y - 50.0).abs() < 3.0
        });
        let s = vectorise(&a, 200, 100, 96);
        assert_eq!(s.len(), 1, "{s:?}");
        let s = &s[0];
        // straight: two points do it
        assert!(s.len() <= 4, "{} points", s.len());
        let ((x0, y0), (x1, y1)) = ends(s);
        let (l, r) = (x0.min(x1), x0.max(x1));
        assert!(l < 26.0 && r > 174.0, "from {l} to {r}");
        assert!((y0 - 50.0).abs() < 1.0 && (y1 - 50.0).abs() < 1.0);
        let mid = s.iter().map(|p| p.width).fold(0.0f32, f32::max);
        assert!((mid - 6.0).abs() <= 1.5, "width {mid}");
    }

    #[test]
    fn a_ring_is_one_closed_stroke_round_its_middle() {
        // radius 30, 4 px wide
        let a = picture(100, 100, |x, y| {
            ((x - 50.0).hypot(y - 50.0) - 30.0).abs() < 2.0
        });
        let s = vectorise(&a, 100, 100, 96);
        assert_eq!(s.len(), 1, "{s:?}");
        let s = &s[0];
        let ((x0, y0), (x1, y1)) = ends(s);
        assert!((x0 - x1).abs() < 0.01 && (y0 - y1).abs() < 0.01, "closed");
        for p in s {
            let r = (p.x - 50.0).hypot(p.y - 50.0);
            assert!((r - 30.0).abs() < 1.5, "off the ring: {p:?}");
            assert!((p.width - 4.0).abs() <= 1.5, "width {}", p.width);
        }
        // a circle needs more than a few points, not one a pixel
        assert!((12..120).contains(&s.len()), "{} points", s.len());
    }

    #[test]
    fn a_cross_is_four_arms_from_its_middle() {
        let a = picture(120, 120, |x, y| {
            ((x - 60.0).abs() < 2.0 && (10.0..110.0).contains(&y))
                || ((y - 60.0).abs() < 2.0 && (10.0..110.0).contains(&x))
        });
        let s = vectorise(&a, 120, 120, 96);
        assert_eq!(s.len(), 4, "{:?}", s.iter().map(ends).collect::<Vec<_>>());
        for arm in &s {
            let ((x0, y0), (x1, y1)) = ends(arm);
            // one end at the middle, the other out at an arm's end
            let near = |x: f32, y: f32| (x - 60.0).hypot(y - 60.0) < 4.0;
            assert!(near(x0, y0) != near(x1, y1), "{:?}", ends(arm));
            assert!(length(arm) > 40.0);
        }
    }

    #[test]
    fn specks_are_dust_and_thin_diagonals_are_lines() {
        let a = picture(100, 100, |x, y| {
            // a 3×3 speck, and a 1-2 px diagonal from (10, 10) to (90, 90)
            ((70.0..73.0).contains(&x) && (20.0..23.0).contains(&y))
                || ((x - y).abs() < 1.0 && (10.0..90.0).contains(&x))
        });
        let s = vectorise(&a, 100, 100, 96);
        assert_eq!(s.len(), 1, "{s:?}");
        assert!(length(&s[0]) > 100.0);
        assert!(
            s[0].len() <= 4,
            "a straight diagonal: {} points",
            s[0].len()
        );
    }

    #[test]
    fn a_painted_s_curve_is_one_stroke_on_it_wider_where_pressed() {
        use crate::{Painter, set_brush_rgb};
        let mut p = Painter::new(400, 200);
        // the Clean-up pen: thin at light pressure, swelling as it's pressed
        let myb = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/web/static/brushes/pencilpilot-clean-up-pen.myb"),
        )
        .unwrap();
        let mut b = hokusai::myb::from_str(&myb).unwrap();
        b.get_mut(hokusai::BrushSetting::Radius).base_value = 1.8;
        set_brush_rgb(&mut b, 0, 0, 0);
        let curve = |t: f32| {
            (
                40.0 + 320.0 * t,
                100.0 + 50.0 * (t * std::f32::consts::TAU).sin(),
            )
        };
        p.begin_stroke();
        for i in 0..=200 {
            let t = i as f32 / 200.0;
            let (x, y) = curve(t);
            p.stroke_to(
                &b,
                x,
                y,
                0.15 + 0.85 * (t * std::f32::consts::PI).sin(),
                0.0,
                0.0,
                0.01,
            );
        }
        p.end_stroke(&b);
        p.flush();
        let li = p.index_of(p.active_layer()).unwrap();
        let alpha: Vec<u8> = p.sample_source(li, false).iter().map(|px| px[3]).collect();
        let s = vectorise(&alpha, 400, 200, 96);
        assert_eq!(s.len(), 1, "{} strokes", s.len());
        let s = &s[0];
        // on the curve all along
        for q in s {
            let near = (0..=400)
                .map(|i| {
                    let (x, y) = curve(i as f32 / 400.0);
                    (q.x - x).hypot(q.y - y)
                })
                .fold(f32::MAX, f32::min);
            assert!(near < 2.0, "{q:?} is {near} px off the curve");
        }
        // wider in the middle (pressed) than at its ends
        let mid = s
            .iter()
            .filter(|q| (q.x - 200.0).abs() < 40.0)
            .map(|q| q.width)
            .fold(0.0f32, f32::max);
        let end = s.iter().take(2).map(|q| q.width).fold(0.0f32, f32::max);
        assert!(mid > end * 1.5, "middle {mid}, end {end}");
    }

    #[test]
    fn nothing_from_blank_or_tiny_pictures() {
        assert!(vectorise(&[0; 100 * 100], 100, 100, 96).is_empty());
        assert!(vectorise(&[255; 4], 2, 2, 96).is_empty());
        assert!(vectorise(&[255; 10], 100, 100, 96).is_empty());
    }
}

#[cfg(test)]
mod speed {
    use super::*;

    /// (run with --ignored --nocapture) a busy 1080p frame
    #[test]
    #[ignore]
    fn a_full_hd_frame() {
        let (w, h) = (1920usize, 1080usize);
        let a: Vec<u8> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                let wave = (y - 540.0 - 200.0 * (x / 150.0).sin()).abs() < 3.0;
                let rings = (0..12).any(|k| {
                    let (cx, cy) = (160.0 * k as f32 + 80.0, 300.0 + 40.0 * (k % 3) as f32);
                    ((x - cx).hypot(y - cy) - 60.0).abs() < 2.5
                });
                let hatch = y > 800.0 && ((x + y) as i32 % 24) < 3;
                if wave || rings || hatch { 255 } else { 0 }
            })
            .collect();
        let t = std::time::Instant::now();
        let s = vectorise(&a, w, h, 96);
        eprintln!("{} strokes in {:?}", s.len(), t.elapsed());
    }
}
