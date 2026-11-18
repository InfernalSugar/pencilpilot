//! How hard a reference is to draw, so beginners get simple pictures first.
//!
//! Plain image statistics (no AI), computed on the same low-res edge map the
//! grader uses:
//!
//! * **edges**: how much line work there is (edge pixels relative to the
//!   working canvas). Busy photos with fur, leaves and texture have many.
//! * **pieces**: how many separate edge fragments there are (detail count).
//! * **tones**: spread of light/dark values (Shannon entropy of the tone
//!   histogram). Flat graphics are low, photos with soft shading are high.
//!
//! They are squashed to 0–1 and blended into a 0–100 `score`, which maps to
//! four tiers: 1 Beginner, 2 Easy, 3 Medium, 4 Hard.

use serde::{Deserialize, Serialize};

use crate::Gray;
use crate::edges::reference_edges;
use crate::img::{Mask, downsample_mean, factor_for};

const WORK_SIZE: u32 = 200;

/// Tier boundaries on the 0–100 score (calibrated on the first Pixabay batch;
/// see `tools/ref-importer` `classify --report`).
pub const TIER_BOUNDS: [f32; 3] = [32.0, 48.0, 64.0];

pub const TIER_NAMES: [&str; 4] = ["Beginner", "Easy", "Medium", "Hard"];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Difficulty {
    /// 0 (trivial) – 100 (very hard)
    pub score: f32,
    /// 1 Beginner … 4 Hard
    pub tier: u8,
    /// edge pixels ÷ working-canvas pixels
    pub edge_density: f32,
    /// separate edge fragments (≥ 3 px)
    pub pieces: u32,
    /// tone-histogram entropy, 0–1
    pub tone_entropy: f32,
}

pub fn tier_for(score: f32) -> u8 {
    1 + TIER_BOUNDS.iter().filter(|&&b| score >= b).count() as u8
}

/// Rate a reference that has been fitted into the drawing canvas.
pub fn difficulty(reference: &Gray) -> Difficulty {
    let f = factor_for(reference.width, reference.height, WORK_SIZE);
    let tone = downsample_mean(reference, f);
    let edges = reference_edges(&tone);

    let n = (edges.w * edges.h).max(1) as f32;
    let edge_density = edges.d.iter().filter(|&&b| b).count() as f32 / n;
    let pieces = count_pieces(&edges, 3);
    let tone_entropy = entropy(&tone.d, 16);

    // Squash each to 0–1 (x / (x + k): half-way at k).
    let e = edge_density / (edge_density + 0.06);
    let p = pieces as f32 / (pieces as f32 + 40.0);
    let score = 100.0 * (0.45 * e + 0.35 * p + 0.20 * tone_entropy);
    Difficulty {
        score,
        tier: tier_for(score),
        edge_density,
        pieces,
        tone_entropy,
    }
}

/// 8-connected components with at least `min` pixels.
fn count_pieces(m: &Mask, min: usize) -> u32 {
    let mut seen = vec![false; m.d.len()];
    let mut stack = Vec::new();
    let mut count = 0;
    for start in 0..m.d.len() {
        if !m.d[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = ((i % m.w) as i64, (i / m.w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= m.w as i64 || ny >= m.h as i64 {
                        continue;
                    }
                    let j = ny as usize * m.w + nx as usize;
                    if m.d[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if size >= min {
            count += 1;
        }
    }
    count
}

/// Normalised Shannon entropy (0–1) of values in 0–255.
fn entropy(values: &[f32], bins: usize) -> f32 {
    let mut hist = vec![0usize; bins];
    for &v in values {
        let b = ((v.clamp(0.0, 255.0) / 256.0) * bins as f32) as usize;
        hist[b.min(bins - 1)] += 1;
    }
    let n = values.len().max(1) as f32;
    let h: f32 = hist
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f32 / n;
            -p * p.log2()
        })
        .sum();
    h / (bins as f32).log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(f: impl Fn(u32, u32) -> u8) -> Gray {
        let (w, h) = (800, 600);
        let px = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        Gray::new(w, h, px).unwrap()
    }

    #[test]
    fn simple_shape_is_easier_than_busy_texture() {
        let disc = canvas(|x, y| {
            let (dx, dy) = (x as f32 - 400.0, y as f32 - 300.0);
            if dx * dx + dy * dy < 150.0 * 150.0 {
                40
            } else {
                255
            }
        });
        let busy = canvas(|x, y| {
            let v = ((x * 7 + y * 13) ^ (x * y / 9)) % 97;
            (v * 255 / 96) as u8
        });
        let (a, b) = (difficulty(&disc), difficulty(&busy));
        assert!(a.score < b.score, "{a:?} vs {b:?}");
        assert_eq!(a.tier, 1, "{a:?}");
        assert!(b.tier >= 3, "{b:?}");
    }

    #[test]
    fn tiers_follow_bounds() {
        assert_eq!(tier_for(0.0), 1);
        assert_eq!(tier_for(TIER_BOUNDS[0]), 2);
        assert_eq!(tier_for(TIER_BOUNDS[1] + 1.0), 3);
        assert_eq!(tier_for(99.0), 4);
    }

    #[test]
    fn blank_is_beginner() {
        let d = difficulty(&canvas(|_, _| 255));
        assert_eq!(d.tier, 1);
        assert_eq!(d.pieces, 0);
    }
}
