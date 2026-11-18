//! Small image-processing toolkit: float planes, boolean masks, resampling,
//! blur, distance transform and morphology. Everything is plain loops so it
//! compiles to wasm32 without threads or SIMD.

use crate::Gray;

/// Distance reported for "no feature anywhere" (keeps maths finite).
pub(crate) const FAR: f32 = 1.0e4;

/// A single-channel float image. Pixel `(x, y)` covers the continuous square
/// `[x, x+1) × [y, y+1)`, so its centre is at `(x + 0.5, y + 0.5)`.
#[derive(Debug, Clone)]
pub(crate) struct Plane {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Plane {
    pub fn filled(w: usize, h: usize, v: f32) -> Self {
        Self {
            w,
            h,
            d: vec![v; w * h],
        }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.d[y * self.w + x]
    }

    /// Bilinear sample at continuous coordinates; `outside` beyond the edges.
    #[inline]
    pub fn sample(&self, x: f32, y: f32, outside: f32) -> f32 {
        let fx = x - 0.5;
        let fy = y - 0.5;
        if !(fx > -1.0 && fy > -1.0 && fx < self.w as f32 && fy < self.h as f32) {
            return outside;
        }
        let x0 = fx.floor();
        let y0 = fy.floor();
        let ax = fx - x0;
        let ay = fy - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);
        let get = |x: isize, y: isize| -> f32 {
            if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
                outside
            } else {
                self.d[y as usize * self.w + x as usize]
            }
        };
        let a = get(x0, y0) * (1.0 - ax) + get(x0 + 1, y0) * ax;
        let b = get(x0, y0 + 1) * (1.0 - ax) + get(x0 + 1, y0 + 1) * ax;
        a * (1.0 - ay) + b * ay
    }

    pub fn min_max(&self) -> (f32, f32) {
        self.d
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    }

    /// Value at quantile `q` (0–1) of all pixels.
    pub fn quantile(&self, q: f32) -> f32 {
        if self.d.is_empty() {
            return 0.0;
        }
        let mut v = self.d.clone();
        let i = ((v.len() - 1) as f32 * q.clamp(0.0, 1.0)).round() as usize;
        let (_, m, _) = v.select_nth_unstable_by(i, |a, b| a.total_cmp(b));
        *m
    }
}

/// A boolean image.
#[derive(Debug, Clone)]
pub(crate) struct Mask {
    pub w: usize,
    pub h: usize,
    pub d: Vec<bool>,
}

impl Mask {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            d: vec![false; w * h],
        }
    }

    pub fn count(&self) -> usize {
        self.d.iter().filter(|&&b| b).count()
    }

    /// Coordinates (pixel centres) of all set pixels.
    pub fn points(&self) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for y in 0..self.h {
            for x in 0..self.w {
                if self.d[y * self.w + x] {
                    out.push((x as f32 + 0.5, y as f32 + 0.5));
                }
            }
        }
        out
    }

    pub fn and(&self, o: &Mask) -> Mask {
        Mask {
            w: self.w,
            h: self.h,
            d: self.d.iter().zip(&o.d).map(|(&a, &b)| a && b).collect(),
        }
    }
}

/// Integer downsample factor that brings the long side to about `target`.
pub(crate) fn factor_for(w: u32, h: u32, target: u32) -> usize {
    (w.max(h).div_ceil(target.max(1))).max(1) as usize
}

fn reduce(g: &Gray, f: usize, init: f32, fold: impl Fn(f32, f32) -> f32, mean: bool) -> Plane {
    let (gw, gh) = (g.width as usize, g.height as usize);
    let (w, h) = (gw.div_ceil(f), gh.div_ceil(f));
    let mut acc = vec![init; w * h];
    let mut cnt = vec![0u32; w * h];
    for y in 0..gh {
        let row = &g.pixels[y * gw..(y + 1) * gw];
        let base = (y / f) * w;
        for (x, &p) in row.iter().enumerate() {
            let i = base + x / f;
            acc[i] = fold(acc[i], p as f32);
            cnt[i] += 1;
        }
    }
    if mean {
        for (a, &c) in acc.iter_mut().zip(&cnt) {
            *a /= c.max(1) as f32;
        }
    }
    Plane { w, h, d: acc }
}

/// Box-average downsample by an integer factor (tone).
pub(crate) fn downsample_mean(g: &Gray, f: usize) -> Plane {
    reduce(g, f, 0.0, |a, b| a + b, true)
}

/// Darkest-pixel downsample by an integer factor (keeps thin lines).
pub(crate) fn downsample_min(g: &Gray, f: usize) -> Plane {
    reduce(g, f, 255.0, f32::min, false)
}

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as isize;
    let mut k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    k
}

/// Separable Gaussian blur with clamped edges.
pub(crate) fn blur(p: &Plane, sigma: f32) -> Plane {
    if sigma <= 0.0 || p.d.is_empty() {
        return p.clone();
    }
    let k = gaussian_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let (w, h) = (p.w as isize, p.h as isize);
    let mut tmp = vec![0.0f32; p.d.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let xx = (x + j as isize - r).clamp(0, w - 1);
                s += kv * p.d[(y * w + xx) as usize];
            }
            tmp[(y * w + x) as usize] = s;
        }
    }
    let mut out = vec![0.0f32; p.d.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let yy = (y + j as isize - r).clamp(0, h - 1);
                s += kv * tmp[(yy * w + x) as usize];
            }
            out[(y * w + x) as usize] = s;
        }
    }
    Plane {
        w: p.w,
        h: p.h,
        d: out,
    }
}

/// Felzenszwalb–Huttenlocher 1-D squared distance transform.
fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let qf = q as f64;
        loop {
            let p = v[k];
            let pf = p as f64;
            let s = ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * qf - 2.0 * pf);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0 and z[0] = -inf: cannot happen, but stay safe.
                v[0] = q;
                z[1] = f64::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        let qf = q as f64;
        while z[k + 1] < qf {
            k += 1;
        }
        let p = v[k] as f64;
        *out = (qf - p) * (qf - p) + f[v[k]];
    }
}

/// Euclidean distance (in pixels) from every pixel to the nearest set pixel.
/// Returns [`FAR`] everywhere if the mask is empty.
pub(crate) fn distance_transform(m: &Mask) -> Plane {
    const BIG: f64 = 1.0e20;
    let (w, h) = (m.w, m.h);
    let n = w.max(h);
    let mut grid: Vec<f64> = m.d.iter().map(|&b| if b { 0.0 } else { BIG }).collect();
    let mut f = vec![0.0f64; n];
    let mut d = vec![0.0f64; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        grid[y * w..(y + 1) * w].copy_from_slice(&d[..w]);
    }
    Plane {
        w,
        h,
        d: grid.iter().map(|&v| (v.sqrt() as f32).min(FAR)).collect(),
    }
}

/// Disc dilation by radius `r` pixels.
pub(crate) fn dilate(m: &Mask, r: f32) -> Mask {
    let dt = distance_transform(m);
    Mask {
        w: m.w,
        h: m.h,
        d: dt.d.iter().map(|&v| v <= r).collect(),
    }
}

/// Fill enclosed holes: everything not reachable from the border through
/// unset pixels becomes set.
pub(crate) fn fill_holes(m: &Mask) -> Mask {
    let (w, h) = (m.w, m.h);
    let mut outside = vec![false; w * h];
    let mut stack = Vec::new();
    let push = |i: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if !m.d[i] && !outside[i] {
            outside[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        push(x, &mut outside, &mut stack);
        push((h - 1) * w + x, &mut outside, &mut stack);
    }
    for y in 0..h {
        push(y * w, &mut outside, &mut stack);
        push(y * w + w - 1, &mut outside, &mut stack);
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x > 0 {
            push(i - 1, &mut outside, &mut stack);
        }
        if x + 1 < w {
            push(i + 1, &mut outside, &mut stack);
        }
        if y > 0 {
            push(i - w, &mut outside, &mut stack);
        }
        if y + 1 < h {
            push(i + w, &mut outside, &mut stack);
        }
    }
    Mask {
        w,
        h,
        d: outside.iter().map(|&o| !o).collect(),
    }
}

/// Area-average a plane down to roughly `cols × rows` cells.
pub(crate) fn resize_area(p: &Plane, cols: usize, rows: usize) -> Plane {
    let mut acc = vec![0.0f32; cols * rows];
    let mut cnt = vec![0u32; cols * rows];
    for y in 0..p.h {
        let cy = (y * rows / p.h).min(rows - 1);
        for x in 0..p.w {
            let cx = (x * cols / p.w).min(cols - 1);
            acc[cy * cols + cx] += p.d[y * p.w + x];
            cnt[cy * cols + cx] += 1;
        }
    }
    for (a, &c) in acc.iter_mut().zip(&cnt) {
        *a /= c.max(1) as f32;
    }
    Plane {
        w: cols,
        h: rows,
        d: acc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_transform_matches_brute_force() {
        let mut m = Mask::new(13, 9);
        for &(x, y) in &[(2usize, 3usize), (10, 7), (6, 0)] {
            m.d[y * 13 + x] = true;
        }
        let dt = distance_transform(&m);
        for y in 0..9 {
            for x in 0..13 {
                let want = [(2.0f32, 3.0f32), (10.0, 7.0), (6.0, 0.0)]
                    .iter()
                    .map(|&(px, py)| ((x as f32 - px).powi(2) + (y as f32 - py).powi(2)).sqrt())
                    .fold(f32::MAX, f32::min);
                assert!((dt.at(x, y) - want).abs() < 1e-4, "({x},{y})");
            }
        }
    }

    #[test]
    fn empty_mask_is_far() {
        let dt = distance_transform(&Mask::new(5, 4));
        assert!(dt.d.iter().all(|&v| v == FAR));
    }

    #[test]
    fn fill_holes_fills_ring() {
        let mut m = Mask::new(7, 7);
        for i in 1..6 {
            for &(x, y) in &[(i, 1), (i, 5), (1, i), (5, i)] {
                m.d[y * 7 + x] = true;
            }
        }
        let f = fill_holes(&m);
        assert!(f.d[3 * 7 + 3]);
        assert!(!f.d[0]);
        assert_eq!(f.count(), 25);
    }

    #[test]
    fn blur_preserves_constant() {
        let p = Plane::filled(10, 6, 42.0);
        let b = blur(&p, 2.0);
        assert!(b.d.iter().all(|v| (v - 42.0).abs() < 1e-3));
    }
}
