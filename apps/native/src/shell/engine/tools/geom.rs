//! Geometry for the tools: affine maps, polygons filled into masks, and a mask's outline
//! traced into loops (for the selection's dashed edge).

use std::collections::HashMap;

/// An affine map as the painter takes it: `(a·x + c·y + e, b·x + d·y + f)`.
pub type Affine = [f32; 6];

pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn apply(m: Affine, x: f32, y: f32) -> (f32, f32) {
    let [a, b, c, d, e, f] = m;
    (a * x + c * y + e, b * x + d * y + f)
}

/// `p ∘ q`: q first, then p.
pub fn compose(p: Affine, q: Affine) -> Affine {
    let [a1, b1, c1, d1, e1, f1] = p;
    let [a2, b2, c2, d2, e2, f2] = q;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

pub fn invert(m: Affine) -> Option<Affine> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det.abs() < 1e-9 {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)])
}

pub fn translate(dx: f32, dy: f32) -> Affine {
    [1.0, 0.0, 0.0, 1.0, dx, dy]
}

/// Scale by (sx, sy) about (cx, cy).
pub fn scale_about(cx: f32, cy: f32, sx: f32, sy: f32) -> Affine {
    [sx, 0.0, 0.0, sy, cx - sx * cx, cy - sy * cy]
}

/// Turn by `angle` radians (clockwise on screen, y down) about (cx, cy).
pub fn rotate_about(cx: f32, cy: f32, angle: f32) -> Affine {
    let (s, c) = angle.sin_cos();
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

/// The pixels (one byte each, 255 inside) whose centres are inside the polygon (even-odd),
/// `w` × `h`; and whether any is.
pub fn fill_polygon(pts: &[(f32, f32)], w: usize, h: usize) -> (Vec<u8>, bool) {
    let mut out = vec![0u8; w * h];
    let mut any = false;
    if pts.len() < 3 {
        return (out, false);
    }
    let y0 = pts
        .iter()
        .map(|p| p.1)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let y1 = (pts
        .iter()
        .map(|p| p.1)
        .fold(f32::MIN, f32::max)
        .ceil()
        .max(0.0) as usize)
        .min(h);
    let mut xs: Vec<f32> = Vec::new();
    for y in y0..y1 {
        let cy = y as f32 + 0.5;
        xs.clear();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.1 <= cy) != (b.1 <= cy) {
                xs.push(a.0 + (cy - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            // pixels whose centre x + 0.5 lies in [x0, x1)
            let from = (pair[0] - 0.5).ceil().max(0.0) as usize;
            let to = ((pair[1] - 0.5).ceil().max(0.0) as usize).min(w);
            if from < to {
                out[y * w + from..y * w + to].fill(255);
                any = true;
            }
        }
    }
    (out, any)
}

/// The rectangle `[x0, y0, x1, y1]` (in pixels, any corner order) as a mask.
pub fn fill_rect(r: [f32; 4], w: usize, h: usize) -> (Vec<u8>, bool) {
    let (x0, x1) = (r[0].min(r[2]), r[0].max(r[2]));
    let (y0, y1) = (r[1].min(r[3]), r[1].max(r[3]));
    fill_polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], w, h)
}

/// The selected area's (`mask` > 127) edge as closed loops of points (pixel corners),
/// simplified to within half a pixel. A ragged mask with very many edges gets its
/// bounding box instead (the outline is only a guide).
pub fn outline(mask: &[u8], w: usize, h: usize) -> Vec<Vec<(f32, f32)>> {
    const MAX_EDGES: usize = 400_000;
    let on = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && mask[y as usize * w + x as usize] > 127
    };
    // directed unit edges, the selection on their right (y down): start → end
    let mut next: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
    let mut edges = 0usize;
    let (mut bx0, mut by0, mut bx1, mut by1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        let row = &mask[y * w..(y + 1) * w];
        if !row.iter().any(|&v| v > 127)
            && (y == 0 || !mask[(y - 1) * w..y * w].iter().any(|&v| v > 127))
        {
            continue;
        }
        for (x, &v) in row.iter().enumerate() {
            let here = v > 127;
            if here {
                bx0 = bx0.min(x);
                by0 = by0.min(y);
                bx1 = bx1.max(x + 1);
                by1 = by1.max(y + 1);
            }
            let (xi, yi) = (x as i64, y as i64);
            let (xu, yu) = (x as u32, y as u32);
            if here != on(xi, yi - 1) {
                // a horizontal edge along the top of (x, y)
                edges += 1;
                if here {
                    next.entry((xu, yu)).or_default().push((xu + 1, yu));
                } else {
                    next.entry((xu + 1, yu)).or_default().push((xu, yu));
                }
            }
            if here != on(xi - 1, yi) {
                edges += 1;
                if here {
                    next.entry((xu, yu + 1)).or_default().push((xu, yu));
                } else {
                    next.entry((xu, yu)).or_default().push((xu, yu + 1));
                }
            }
            if here && x + 1 == w {
                edges += 1;
                next.entry((xu + 1, yu)).or_default().push((xu + 1, yu + 1));
            }
            if here && y + 1 == h {
                edges += 1;
                next.entry((xu + 1, yu + 1)).or_default().push((xu, yu + 1));
            }
        }
        if edges > MAX_EDGES {
            break;
        }
    }
    if bx0 == usize::MAX {
        return Vec::new();
    }
    if edges > MAX_EDGES {
        let (x0, y0, x1, y1) = (bx0 as f32, by0 as f32, bx1 as f32, by1 as f32);
        return vec![vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]];
    }
    let mut loops = Vec::new();
    let mut starts: Vec<(u32, u32)> = next.keys().copied().collect();
    starts.sort_unstable();
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(|v| v.pop()) {
            let mut pts = vec![s];
            let mut at = first;
            let mut prev = s;
            while at != s {
                pts.push(at);
                let Some(v) = next.get_mut(&at) else { break };
                if v.is_empty() {
                    break;
                }
                // at a corner where two loops touch, turn the same way each time
                let i = if v.len() > 1 {
                    let (dx, dy) = (at.0 as i64 - prev.0 as i64, at.1 as i64 - prev.1 as i64);
                    // turn right first: (dx, dy) → (−dy, dx)
                    v.iter()
                        .position(|n| {
                            (n.0 as i64 - at.0 as i64, n.1 as i64 - at.1 as i64) == (-dy, dx)
                        })
                        .unwrap_or(v.len() - 1)
                } else {
                    0
                };
                let n = v.swap_remove(i);
                prev = at;
                at = n;
            }
            let pts: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 as f32, p.1 as f32)).collect();
            loops.push(simplify_loop(&pts, 0.6));
        }
    }
    loops
}

/// Ramer–Douglas–Peucker on a closed loop.
fn simplify_loop(pts: &[(f32, f32)], tol: f32) -> Vec<(f32, f32)> {
    if pts.len() <= 4 {
        return pts.to_vec();
    }
    // split at the point farthest from the first, simplify both halves
    let far = (1..pts.len())
        .max_by(|&a, &b| {
            let da = (pts[a].0 - pts[0].0).hypot(pts[a].1 - pts[0].1);
            let db = (pts[b].0 - pts[0].0).hypot(pts[b].1 - pts[0].1);
            da.total_cmp(&db)
        })
        .unwrap_or(0);
    let mut a = rdp(&pts[..=far], tol);
    let mut closing: Vec<(f32, f32)> = pts[far..].to_vec();
    closing.push(pts[0]);
    let b = rdp(&closing, tol);
    a.pop();
    a.extend(&b[..b.len() - 1]);
    a
}

fn rdp(pts: &[(f32, f32)], tol: f32) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        let (a, b) = (pts[i], pts[j]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy).max(1e-6);
        let (mut best, mut at) = (0.0f32, 0usize);
        for (k, p) in pts.iter().enumerate().take(j).skip(i + 1) {
            let d = ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len;
            if d > best {
                best = d;
                at = k;
            }
        }
        if best > tol {
            keep[at] = true;
            stack.push((i, at));
            stack.push((at, j));
        }
    }
    pts.iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn maps_compose_and_invert() {
        let m = compose(
            translate(5.0, -3.0),
            compose(
                rotate_about(10.0, 10.0, 0.4),
                scale_about(2.0, 3.0, 1.5, 0.5),
            ),
        );
        let inv = invert(m).unwrap();
        for p in [(0.0, 0.0), (17.0, -4.0), (100.0, 50.0)] {
            let q = apply(m, p.0, p.1);
            assert!(near(apply(inv, q.0, q.1), p));
        }
        // the pivots stay
        assert!(near(
            apply(rotate_about(10.0, 10.0, 1.0), 10.0, 10.0),
            (10.0, 10.0)
        ));
        assert!(near(
            apply(scale_about(2.0, 3.0, 4.0, 4.0), 2.0, 3.0),
            (2.0, 3.0)
        ));
        // a quarter turn clockwise on screen: right goes down
        assert!(near(
            apply(
                rotate_about(0.0, 0.0, std::f32::consts::FRAC_PI_2),
                1.0,
                0.0
            ),
            (0.0, 1.0)
        ));
    }

    #[test]
    fn polygons_fill_their_pixels() {
        let (m, any) = fill_rect([2.0, 3.0, 12.0, 8.0], 20, 10);
        assert!(any);
        assert_eq!(m.iter().filter(|&&v| v > 0).count(), 10 * 5);
        assert_eq!(m[3 * 20 + 2], 255);
        assert_eq!(m[3 * 20 + 12], 0);
        // a triangle: about half its box
        let (t, _) = fill_polygon(&[(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)], 100, 100);
        let n = t.iter().filter(|&&v| v > 0).count();
        assert!((4800..=5200).contains(&n), "{n}");
    }

    #[test]
    fn outlines_trace_loops() {
        let (w, h) = (40, 30);
        let (mut m, _) = fill_rect([5.0, 5.0, 15.0, 12.0], w, h);
        let loops = outline(&m, w, h);
        assert_eq!(loops.len(), 1);
        assert_eq!(
            loops[0].len(),
            4,
            "a rectangle simplifies to its corners: {loops:?}"
        );
        for c in [(5.0, 5.0), (15.0, 5.0), (15.0, 12.0), (5.0, 12.0)] {
            assert!(loops[0].iter().any(|p| near(*p, c)), "{c:?} in {loops:?}");
        }
        // a second area, and a hole
        let (b, _) = fill_rect([20.0, 2.0, 38.0, 28.0], w, h);
        let (hole, _) = fill_rect([25.0, 10.0, 30.0, 15.0], w, h);
        for i in 0..w * h {
            m[i] = if (b[i] > 0 && hole[i] == 0) || m[i] > 0 {
                255
            } else {
                0
            };
        }
        assert_eq!(outline(&m, w, h).len(), 3);
        // touching the canvas's edges
        let all = vec![255u8; w * h];
        let l = outline(&all, w, h);
        assert_eq!(l.len(), 1);
        assert!(l[0].iter().any(|p| near(*p, (40.0, 30.0))));
        assert!(outline(&vec![0u8; w * h], w, h).is_empty());
    }
}
