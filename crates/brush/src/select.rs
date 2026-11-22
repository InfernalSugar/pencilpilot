//! More ways to make and change the selection: grow / shrink it (Photoshop's
//! Select > Modify > Expand / Contract), select a colour wherever it is
//! (Color Range with a chosen colour) and select a layer's contents (Ctrl+click
//! a layer's thumbnail). Like the other selection changes they aren't undo steps.

use crate::Painter;

impl Painter {
    /// Combine a soft mask (0–255 per canvas pixel) into the selection: 0 replace,
    /// 1 add, 2 subtract, 3 intersect. Soft edges stay soft. An empty result
    /// means no selection.
    pub(crate) fn combine_selection_soft(&mut self, new: Vec<u8>, op: u8) -> bool {
        let n = (self.width * self.height) as usize;
        if new.len() != n {
            return false;
        }
        let out: Vec<u8> = match (op, self.selection.take()) {
            (1, Some(cur)) => cur.iter().zip(&new).map(|(&c, &m)| c.max(m)).collect(),
            (2, Some(cur)) => cur
                .iter()
                .zip(&new)
                .map(|(&c, &m)| c.min(255 - m))
                .collect(),
            (3, Some(cur)) => cur.iter().zip(&new).map(|(&c, &m)| c.min(m)).collect(),
            // nothing selected yet: subtracting or crossing leaves nothing
            (2 | 3, None) => Vec::new(),
            _ => new,
        };
        if out.iter().any(|&v| v > 0) {
            self.selection = Some(out);
        }
        true
    }

    /// Grow the selection by `px` pixels, or shrink it with a negative `px`.
    /// The new edge is a smooth round one, half a pixel soft; a feathered edge
    /// keeps its softness on the side that doesn't move. False if there's no
    /// selection or it shrank away to nothing (then there's none).
    pub fn grow_selection(&mut self, px: i32) -> bool {
        let Some(sel) = self.selection.as_ref() else {
            return false;
        };
        if px == 0 {
            return true;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let r = px.unsigned_abs() as f32;
        // a pixel's distance to the nearest one on the other side of the edge;
        // its own edge is half a pixel nearer than its centre
        let ramp = |t: f32| ((t + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        let out: Vec<u8> = if px > 0 {
            let inside: Vec<bool> = sel.iter().map(|&v| v >= 128).collect();
            let d = distance_to(&inside, w, h);
            sel.iter()
                .zip(&d)
                .map(|(&v, &d)| v.max(ramp(r - (d - 0.5))))
                .collect()
        } else {
            let outside: Vec<bool> = sel.iter().map(|&v| v < 128).collect();
            let d = distance_to(&outside, w, h);
            sel.iter()
                .zip(&d)
                .map(|(&v, &d)| v.min(ramp((d - 0.5) - r)))
                .collect()
        };
        if out.iter().all(|&v| v == 0) {
            self.selection = None;
            return false;
        }
        self.selection = Some(out);
        true
    }

    /// Select every pixel of colour `rgb` (sRGB), give or take `tolerance` (0–1,
    /// per channel), wherever it is: on the active layer (soft by its
    /// see-through edges) or in what you see (`all_layers`). Combined by `op`.
    pub fn select_colour(
        &mut self,
        rgb: [u8; 3],
        tolerance: f32,
        all_layers: bool,
        op: u8,
    ) -> bool {
        let Some(li) = self.index_of(self.active) else {
            return false;
        };
        let src = self.sample_source(li, all_layers);
        let tol = (tolerance.clamp(0.0, 1.0) * 255.0) as i32;
        let m = src
            .iter()
            .map(|p| {
                let near = (0..3).all(|i| (p[i] as i32 - rgb[i] as i32).abs() <= tol);
                if near { p[3] } else { 0 }
            })
            .collect();
        self.combine_selection_soft(m, op)
    }

    /// Select what's on layer `id` (soft where it's see-through), combined by
    /// `op`. False if there's no such layer.
    pub fn select_layer(&mut self, id: u32, op: u8) -> bool {
        let Some(li) = self.index_of(id) else {
            return false;
        };
        let m = self.sample_source(li, false).iter().map(|p| p[3]).collect();
        self.combine_selection_soft(m, op)
    }
}

/// Each pixel's distance to the nearest `true` one (0 on them; infinite with
/// none at all): the exact Euclidean distance transform of Felzenszwalb and
/// Huttenlocher, a column pass then a row pass.
pub(crate) fn distance_to(m: &[bool], w: usize, h: usize) -> Vec<f32> {
    const FAR: f32 = 1e20;
    let mut d: Vec<f32> = m.iter().map(|&b| if b { 0.0 } else { FAR }).collect();
    let mut line = vec![0f32; w.max(h)];
    let mut out = vec![0f32; w.max(h)];
    for x in 0..w {
        for y in 0..h {
            line[y] = d[y * w + x];
        }
        squared_1d(&line[..h], &mut out[..h]);
        for y in 0..h {
            d[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        line[..w].copy_from_slice(&d[y * w..(y + 1) * w]);
        squared_1d(&line[..w], &mut out[..w]);
        d[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    d.iter()
        .map(|&v| if v >= FAR { f32::INFINITY } else { v.sqrt() })
        .collect()
}

/// One line of the distance transform: `out[q]` = min over p of `f[p]` + (q − p)².
fn squared_1d(f: &[f32], out: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    // the lower envelope of the parabolas rooted at each p
    let mut v = vec![0usize; n];
    let mut z = vec![0f32; n + 1];
    let mut k = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    let cross = |q: usize, p: usize| {
        let (q2, p2) = ((q * q) as f32, (p * p) as f32);
        ((f[q] + q2) - (f[p] + p2)) / (2.0 * (q as f32 - p as f32))
    };
    for q in 1..n {
        let mut s = cross(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = cross(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let dq = q as f32 - v[k] as f32;
        *o = dq * dq + f[v[k]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(p: &mut Painter, x0: usize, y0: usize, x1: usize, y1: usize) {
        let (w, h) = (p.width as usize, p.height as usize);
        let mut m = vec![0u8; w * h];
        for y in y0..y1 {
            for x in x0..x1 {
                m[y * w + x] = 255;
            }
        }
        p.set_selection(&m, 0);
    }

    #[test]
    fn distance_transform_is_euclidean() {
        let (w, h) = (9, 7);
        let mut m = vec![false; w * h];
        m[3 * w + 4] = true;
        let d = distance_to(&m, w, h);
        assert_eq!(d[3 * w + 4], 0.0);
        assert_eq!(d[3 * w + 7], 3.0);
        assert!((d[(3 + 2) * w + 4 + 2] - 8f32.sqrt()).abs() < 1e-4);
        assert!(distance_to(&[false; 4], 2, 2)[0].is_infinite());
    }

    #[test]
    fn selection_grows_and_shrinks() {
        let mut p = Painter::new(100, 80);
        assert!(!p.grow_selection(4), "nothing selected");
        square(&mut p, 40, 30, 60, 50);
        assert!(p.grow_selection(5));
        let m = p.selection_mask();
        assert_eq!(m[40 * 100 + 36], 255, "5 px out to the left");
        assert_eq!(m[40 * 100 + 64], 255, "and to the right");
        assert_eq!(m[40 * 100 + 30], 0, "not 10");
        // the corners round off
        assert_eq!(m[25 * 100 + 35], 0);
        assert!(m[26 * 100 + 36] < 128, "softly");
        assert!(p.grow_selection(-5));
        let m = p.selection_mask();
        assert_eq!(m[40 * 100 + 36], 0, "back in");
        assert_eq!(m[40 * 100 + 41], 255);
        assert_eq!(m[40 * 100 + 50], 255);
        // shrinking past its size leaves no selection
        assert!(!p.grow_selection(-30));
        assert!(!p.has_selection());
    }

    #[test]
    fn colour_and_layer_contents_select_softly_and_combine() {
        let mut p = Painter::new(60, 40);
        let mut rgba = vec![0u8; 60 * 40 * 4];
        let mut put = |x: usize, y: usize, c: [u8; 4]| {
            let i = (y * 60 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&c);
        };
        put(5, 5, [200, 30, 30, 255]);
        put(50, 30, [205, 28, 33, 128]);
        put(20, 20, [30, 30, 200, 255]);
        let id = p.add_image_layer("x", &rgba);
        assert!(p.select_colour([200, 30, 30], 0.05, false, 0));
        let m = p.selection_mask();
        assert_eq!(m[5 * 60 + 5], 255);
        assert!(
            (120..=135).contains(&m[30 * 60 + 50]),
            "half see-through: half selected"
        );
        assert_eq!(m[20 * 60 + 20], 0, "another colour");
        // add the layer's contents: the blue dot joins
        assert!(p.select_layer(id, 1));
        assert_eq!(p.selection_mask()[20 * 60 + 20], 255);
        // take the red away again
        p.select_colour([200, 30, 30], 0.05, false, 2);
        let m = p.selection_mask();
        assert_eq!(m[5 * 60 + 5], 0);
        assert_eq!(m[20 * 60 + 20], 255);
        // a new selection of just the layer
        assert!(p.select_layer(id, 0));
        assert!((120..=135).contains(&p.selection_mask()[30 * 60 + 50]));
        // all layers: the paper is white, so white is everywhere else
        p.deselect();
        p.select_colour([255, 255, 255], 0.02, true, 0);
        let m = p.selection_mask();
        assert_eq!(m[0], 255);
        assert_eq!(m[5 * 60 + 5], 0);
        assert!(!p.select_layer(999, 0));
    }
}
