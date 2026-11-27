//! Resizing a frame to a size the painter can't shrink to by a whole number (a PNG or GIF
//! at 720p of a 1080p document, say; videos are scaled by FFmpeg): a triangle filter as
//! wide as the scale, so shrinking averages and growing blends neighbours.

use rayon::prelude::*;

/// For each output pixel along one side: the first source pixel and the weights from it.
fn weights(src: u32, dst: u32) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    let support = scale.max(1.0);
    (0..dst)
        .map(|o| {
            let c = (o as f32 + 0.5) * scale;
            let a = ((c - support).floor().max(0.0)) as usize;
            let b = ((c + support).ceil() as usize).min(src as usize);
            let mut w: Vec<f32> = (a..b)
                .map(|i| (1.0 - ((i as f32 + 0.5 - c) / support).abs()).max(0.0))
                .collect();
            let sum: f32 = w.iter().sum();
            if sum > 0.0 {
                w.iter_mut().for_each(|v| *v /= sum);
            } else {
                // (no source pixel near: the nearest)
                let i = (c as usize).min(src as usize - 1);
                return (i, vec![1.0]);
            }
            (a, w)
        })
        .collect()
}

/// Straight RGBA8 `sw` × `sh` to `dw` × `dh` (colour weighted by alpha, so see-through
/// pixels don't darken edges).
pub fn resize(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    if (sw, sh) == (dw, dh) || sw == 0 || sh == 0 {
        return src.to_vec();
    }
    let (sw_, dw_) = (sw as usize, dw as usize);
    let wx = weights(sw, dw);
    let wy = weights(sh, dh);
    // across: premultiplied floats, sh rows of dw
    let mut mid = vec![[0f32; 4]; sh as usize * dw_];
    mid.par_chunks_mut(dw_).enumerate().for_each(|(y, row)| {
        let s = &src[y * sw_ * 4..(y + 1) * sw_ * 4];
        for (o, (a, w)) in row.iter_mut().zip(&wx) {
            let mut acc = [0f32; 4];
            for (k, wk) in w.iter().enumerate() {
                let p = &s[(a + k) * 4..(a + k) * 4 + 4];
                let al = p[3] as f32 * wk;
                acc[0] += p[0] as f32 * al;
                acc[1] += p[1] as f32 * al;
                acc[2] += p[2] as f32 * al;
                acc[3] += al;
            }
            *o = acc;
        }
    });
    // down
    let mut out = vec![0u8; dw_ * dh as usize * 4];
    out.par_chunks_mut(dw_ * 4)
        .zip(&wy)
        .for_each(|(row, (a, w))| {
            for x in 0..dw_ {
                let mut acc = [0f32; 4];
                for (k, wk) in w.iter().enumerate() {
                    let p = mid[(a + k) * dw_ + x];
                    for c in 0..4 {
                        acc[c] += p[c] * wk;
                    }
                }
                let o = &mut row[x * 4..x * 4 + 4];
                if acc[3] > 0.0 {
                    for c in 0..3 {
                        o[c] = (acc[c] / acc[3]).round().clamp(0.0, 255.0) as u8;
                    }
                    o[3] = acc[3].round().clamp(0.0, 255.0) as u8;
                }
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizes_keep_colour_and_size() {
        // flat colour stays the colour, shrinking or growing, by any amount
        let src = [200, 40, 10, 255].repeat(30 * 20);
        for (w, h) in [(20, 13), (45, 31), (7, 5)] {
            let out = resize(&src, 30, 20, w, h);
            assert_eq!(out.len(), (w * h * 4) as usize);
            assert!(out.chunks(4).all(|p| p == [200, 40, 10, 255]), "{w}×{h}");
        }
        // half black, half see-through: the edge is half covered, and black (not grey from
        // the see-through pixels' colour)
        let mut src = Vec::new();
        for _ in 0..4 {
            for x in 0..8 {
                src.extend(if x < 4 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 0]
                });
            }
        }
        let out = resize(&src, 8, 4, 3, 2);
        let mid = &out[4..8];
        assert_eq!(mid[0], 0);
        assert!((mid[3] as i32 - 128).abs() < 20, "{mid:?}");
    }
}
