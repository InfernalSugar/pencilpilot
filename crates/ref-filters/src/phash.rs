//! 64-bit DCT perceptual hash (the classic "pHash", as in `imagehash.phash`).
//!
//! 1. Grayscale, box-filtered down to 32×32 (ignores aspect ratio).
//! 2. 2-D DCT-II; keep the 8×8 lowest frequencies (DC included).
//! 3. Bit = coefficient above the median of those 64 values.
//!
//! Resizing, re-encoding and mild colour shifts barely move the low
//! frequencies, so near-duplicates land within a few bits of each other.

use image::{DynamicImage, GrayImage};

const N: usize = 32;
const K: usize = 8;

/// Perceptual hash of an image (see module docs). Transparent pictures are
/// composited onto white first, as in [`crate::Filters::check`].
pub fn phash(img: &DynamicImage) -> u64 {
    phash_luma(&crate::flatten_alpha(img).to_luma8())
}

pub(crate) fn phash_luma(luma: &GrayImage) -> u64 {
    if luma.width() == 0 || luma.height() == 0 {
        return 0;
    }
    let small = image::imageops::thumbnail(luma, N as u32, N as u32);
    let px: Vec<f32> = small.pixels().map(|p| p.0[0] as f32).collect();

    // cos[u][x] = cos(π (2x + 1) u / 2N), only the K lowest frequencies.
    let mut cos = [[0f32; N]; K];
    for (u, row) in cos.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            *c = (std::f32::consts::PI * (2 * x + 1) as f32 * u as f32 / (2 * N) as f32).cos();
        }
    }
    // Rows first (N×K), then columns (K×K).
    let mut rows = [[0f32; K]; N];
    for y in 0..N {
        for (v, out) in rows[y].iter_mut().enumerate() {
            *out = (0..N).map(|x| px[y * N + x] * cos[v][x]).sum();
        }
    }
    let mut coef = [0f32; K * K];
    for u in 0..K {
        for v in 0..K {
            coef[u * K + v] = (0..N).map(|y| rows[y][v] * cos[u][y]).sum();
        }
    }
    let mut sorted = coef;
    sorted.sort_by(f32::total_cmp);
    let median = (sorted[K * K / 2 - 1] + sorted[K * K / 2]) / 2.0;
    coef.iter()
        .enumerate()
        .fold(0u64, |h, (i, &c)| if c > median { h | (1 << i) } else { h })
}

/// Number of differing bits. Near-duplicate if ≤ ~10.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};
    use std::io::Cursor;

    fn scene(seed: u32, w: u32, h: u32) -> DynamicImage {
        // Smooth blobs at seed-dependent places: a stand-in for a photo.
        let blobs: Vec<(f32, f32, f32, f32)> = (0..6)
            .map(|i| {
                let r = |k: u32| {
                    ((seed.wrapping_mul(2654435761) ^ (i * 40503 + k * 9973))
                        .wrapping_mul(2246822519)
                        >> 8) as f32
                        / (1u32 << 24) as f32
                };
                (r(1), r(2), 0.08 + 0.2 * r(3), r(4) * 2.0 - 1.0)
            })
            .collect();
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let v: f32 = blobs
                .iter()
                .map(|&(cx, cy, r, s)| {
                    let d2 = (fx - cx).powi(2) + (fy - cy).powi(2);
                    s * (-d2 / (r * r)).exp()
                })
                .sum();
            let g = (128.0 + 100.0 * v).clamp(0.0, 255.0) as u8;
            image::Rgb([g, g.saturating_add(20), g / 2 + 60])
        }))
    }

    #[test]
    fn same_image_resized_or_reencoded_is_close() {
        let a = scene(1, 1280, 853);
        let ha = phash(&a);
        let small = a.resize_exact(640, 426, image::imageops::FilterType::Triangle);
        assert!(hamming(ha, phash(&small)) <= 6);
        let mut jpg = Vec::new();
        a.to_rgb8()
            .write_to(&mut Cursor::new(&mut jpg), ImageFormat::Jpeg)
            .unwrap();
        let re = image::load_from_memory(&jpg).unwrap();
        assert!(hamming(ha, phash(&re)) <= 6);
        let tiny = a.thumbnail(300, 300);
        assert!(hamming(ha, phash(&tiny)) <= 6);
    }

    #[test]
    fn different_images_are_far_apart() {
        let hashes: Vec<u64> = (1..=6).map(|s| phash(&scene(s * 7, 800, 600))).collect();
        for i in 0..hashes.len() {
            for j in i + 1..hashes.len() {
                let d = hamming(hashes[i], hashes[j]);
                assert!(d >= 20, "scenes {i} and {j}: distance {d}");
            }
        }
    }

    #[test]
    fn hamming_counts_bits() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0b1011, 0b0001), 2);
        assert_eq!(hamming(u64::MAX, 0), 64);
    }
}
