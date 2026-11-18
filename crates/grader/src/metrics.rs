//! Similarity measurements on low-resolution maps.

use crate::img::{Mask, Plane, blur, dilate, fill_holes, resize_area};

/// Chamfer distances are capped here (low-res px) so a few stray marks
/// don't dominate a mean.
pub(crate) const CAP: f32 = crate::align::CAP;

/// Outline similarity: `exp(-d / σ)` with σ this fraction of the low-res
/// diagonal (≈ 5 px, i.e. ≈ 20 px on an 800×600 canvas).
pub(crate) const OUTLINE_SIGMA: f32 = 0.02;

/// Mean capped distance from each point of `pts` (a mask) to the features
/// of the distance transform `dt`. `None` if the mask is empty.
pub(crate) fn mean_distance(pts: &Mask, dt: &Plane) -> Option<f32> {
    let (mut s, mut n) = (0.0f32, 0usize);
    for (i, &b) in pts.d.iter().enumerate() {
        if b {
            s += dt.d[i].min(CAP);
            n += 1;
        }
    }
    (n > 0).then(|| s / n as f32)
}

/// Like [`mean_distance`], each point weighted by `weight` (e.g. edge
/// strength, so missing a strong edge costs more than missing a faint one).
pub(crate) fn weighted_mean_distance(pts: &Mask, weight: &Plane, dt: &Plane) -> Option<f32> {
    let (mut s, mut n) = (0.0f32, 0.0f32);
    for (i, &b) in pts.d.iter().enumerate() {
        if b {
            let k = weight.d[i].max(1e-3);
            s += k * dt.d[i].min(CAP);
            n += k;
        }
    }
    (n > 0.0).then(|| s / n)
}

pub(crate) fn sigma_px(w: usize, h: usize) -> f32 {
    OUTLINE_SIGMA * ((w * w + h * h) as f32).sqrt()
}

pub(crate) struct GridScores {
    pub cells: Vec<f32>,
    pub has_content: Vec<bool>,
}

/// Outline similarity per cell of a `cols × rows` grid.
pub(crate) fn grid_scores(
    drawing: &Mask,
    edges: &Mask,
    dt_drawing: &Plane,
    dt_edges: &Plane,
    cols: usize,
    rows: usize,
) -> GridScores {
    let (w, h) = (drawing.w, drawing.h);
    let sigma = sigma_px(w, h);
    let n = cols * rows;
    let mut fwd = vec![(0.0f32, 0usize); n];
    let mut bwd = vec![(0.0f32, 0usize); n];
    for y in 0..h {
        let r = (y * rows / h).min(rows - 1);
        for x in 0..w {
            let c = (x * cols / w).min(cols - 1);
            let i = y * w + x;
            let k = r * cols + c;
            if drawing.d[i] {
                fwd[k].0 += dt_edges.d[i].min(CAP);
                fwd[k].1 += 1;
            }
            if edges.d[i] {
                bwd[k].0 += dt_drawing.d[i].min(CAP);
                bwd[k].1 += 1;
            }
        }
    }
    let mut cells = Vec::with_capacity(n);
    let mut has_content = Vec::with_capacity(n);
    for k in 0..n {
        let f = (fwd[k].1 > 0).then(|| fwd[k].0 / fwd[k].1 as f32);
        let b = (bwd[k].1 > 0).then(|| bwd[k].0 / bwd[k].1 as f32);
        let d = match (f, b) {
            (Some(f), Some(b)) => Some(0.5 * (f + b)),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        has_content.push(d.is_some());
        cells.push(d.map_or(1.0, |d| (-d / sigma).exp()));
    }
    GridScores { cells, has_content }
}

/// IoU of the filled, dilated shapes.
pub(crate) fn silhouette(drawing: &Mask, edges: &Mask) -> f32 {
    let diag = ((drawing.w.pow(2) + drawing.h.pow(2)) as f32).sqrt();
    let close = 0.01 * diag; // join small gaps before filling
    let slack = 0.02 * diag; // forgiveness
    let shape = |m: &Mask| dilate(&fill_holes(&dilate(m, close)), slack);
    let (a, b) = (shape(drawing), shape(edges));
    let inter = a.and(&b).count();
    let union = a.d.iter().zip(&b.d).filter(|(x, y)| **x || **y).count();
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

/// Heavily blurred, coarse tone maps.
fn tone_map(p: &Plane) -> Plane {
    let sigma = 0.03 * p.w.max(p.h) as f32;
    let b = blur(p, sigma);
    let cols = p.w.clamp(1, 32);
    let rows = (cols * p.h).div_ceil(p.w).max(1);
    resize_area(&b, cols, rows)
}

pub(crate) struct Tone {
    pub similarity: f32,
    pub contrast_ratio: f32,
}

/// Light/dark pattern agreement (Pearson correlation of the tone maps,
/// negative clamped to 0) and value range ratio.
pub(crate) fn tone(drawing: &Plane, reference: &Plane) -> Tone {
    let (a, b) = (tone_map(drawing), tone_map(reference));
    let n = a.d.len() as f32;
    let ma = a.d.iter().sum::<f32>() / n;
    let mb = b.d.iter().sum::<f32>() / n;
    let (mut sab, mut saa, mut sbb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.d.iter().zip(&b.d) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    let r = if saa > 1e-3 && sbb > 1e-3 {
        sab / (saa.sqrt() * sbb.sqrt())
    } else {
        0.0
    };
    let range = |p: &Plane| p.quantile(0.98) - p.quantile(0.02);
    let rb = range(&b);
    let contrast_ratio = if rb > 1.0 { range(&a) / rb } else { 1.0 };
    Tone {
        similarity: r.clamp(0.0, 1.0),
        contrast_ratio,
    }
}

/// Mean SSIM (Gaussian window σ = 1.5) on lightly blurred images, averaged
/// over `region` (the area near either image's content) so a mostly-white
/// canvas doesn't make everything look alike.
pub(crate) fn ssim(a: &Plane, b: &Plane, region: &Mask) -> f32 {
    let a = blur(a, 1.0);
    let b = blur(b, 1.0);
    let prod = |x: &Plane, y: &Plane| Plane {
        w: x.w,
        h: x.h,
        d: x.d.iter().zip(&y.d).map(|(p, q)| p * q).collect(),
    };
    let win = 1.5;
    let mu_a = blur(&a, win);
    let mu_b = blur(&b, win);
    let aa = blur(&prod(&a, &a), win);
    let bb = blur(&prod(&b, &b), win);
    let ab = blur(&prod(&a, &b), win);
    let c1 = (0.01f32 * 255.0).powi(2);
    let c2 = (0.03f32 * 255.0).powi(2);
    let (mut s, mut n) = (0.0f32, 0usize);
    for i in 0..a.d.len() {
        if !region.d[i] {
            continue;
        }
        n += 1;
        let (ma, mb) = (mu_a.d[i], mu_b.d[i]);
        let va = (aa.d[i] - ma * ma).max(0.0);
        let vb = (bb.d[i] - mb * mb).max(0.0);
        let cov = ab.d[i] - ma * mb;
        s +=
            ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
    }
    if n == 0 {
        return 0.0;
    }
    (s / n as f32).clamp(0.0, 1.0)
}

/// Robust bounding box (0.5th–99.5th percentile of point coordinates):
/// `(x0, y0, x1, y1)`.
pub(crate) fn robust_bbox(pts: &[(f32, f32)]) -> Option<(f32, f32, f32, f32)> {
    if pts.is_empty() {
        return None;
    }
    let q = |mut v: Vec<f32>, lo: f32, hi: f32| {
        v.sort_by(|a, b| a.total_cmp(b));
        let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
        (at(lo), at(hi))
    };
    let (x0, x1) = q(pts.iter().map(|p| p.0).collect(), 0.005, 0.995);
    let (y0, y1) = q(pts.iter().map(|p| p.1).collect(), 0.005, 0.995);
    // A pixel has extent 1: grow by half a pixel each side.
    Some((x0 - 0.5, y0 - 0.5, x1 + 0.5, y1 + 0.5))
}

/// Bounding box of weighted points, from mass quantiles: each side is
/// where `trim` of the total weight lies beyond it. With edge strength as
/// weight, a few faint stray edges (background texture, a speck) can't
/// stretch the box. `(x0, y0, x1, y1)`.
pub(crate) fn mass_bbox(pts: &[(f32, f32, f32)], trim: f32) -> Option<(f32, f32, f32, f32)> {
    let total: f32 = pts.iter().map(|p| p.2.max(0.0)).sum();
    if pts.is_empty() || total <= 0.0 {
        return None;
    }
    let q = |mut v: Vec<(f32, f32)>| {
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let at = |q: f32| {
            let target = q * total;
            let mut acc = 0.0;
            for &(x, w) in &v {
                acc += w.max(0.0);
                if acc >= target {
                    return x;
                }
            }
            v[v.len() - 1].0
        };
        (at(trim), at(1.0 - trim))
    };
    let (x0, x1) = q(pts.iter().map(|p| (p.0, p.2)).collect());
    let (y0, y1) = q(pts.iter().map(|p| (p.1, p.2)).collect());
    Some((x0 - 0.5, y0 - 0.5, x1 + 0.5, y1 + 0.5))
}
