//! Per-category weights and the 0–1 → 1–10 score curve.
//!
//! Weights are plain data: one row per category in [`DEFAULT_WEIGHTS`].
//! They don't need to sum to 1 — the weighted mean is normalised over the
//! components that were actually measured (stroke quality and keypoints
//! are optional), so a missing input never drags the score down.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Anime,
    RealLife,
    Animals,
    Environment,
    #[default]
    Fundamentals,
}

/// How much each measurement counts towards the score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Weights {
    pub placement: f32,
    pub outline: f32,
    pub silhouette: f32,
    pub grid: f32,
    pub tone: f32,
    pub ssim: f32,
    pub line_confidence: f32,
    pub keypoints: f32,
    /// Main line directions vs the reference's (sketch mode leans on it).
    #[serde(default)]
    pub angles: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self::for_category(Category::Fundamentals)
    }
}

/// Sketch mode (ADR-0007): what a sketch is for. Proportions and placement,
/// big shapes and main angles; a loose outline; line neatness, shading and
/// SSIM don't count.
pub const SKETCH_WEIGHTS: Weights = Weights {
    placement: 0.45,
    outline: 0.10,
    silhouette: 0.15,
    grid: 0.10,
    tone: 0.0,
    ssim: 0.0,
    line_confidence: 0.0,
    keypoints: 0.20,
    angles: 0.20,
};

/// Default weights. Outline and proportion (keypoints) lead for anime and
/// fundamentals; tone and overall structure (SSIM) matter more for
/// real-life and environment studies.
pub const DEFAULT_WEIGHTS: [(Category, Weights); 5] = [
    (
        Category::Fundamentals,
        Weights {
            placement: 0.08,
            outline: 0.40,
            silhouette: 0.20,
            grid: 0.12,
            tone: 0.04,
            ssim: 0.04,
            line_confidence: 0.12,
            keypoints: 0.20,
            angles: 0.0,
        },
    ),
    (
        Category::Anime,
        Weights {
            placement: 0.08,
            outline: 0.40,
            silhouette: 0.15,
            grid: 0.12,
            tone: 0.04,
            ssim: 0.04,
            line_confidence: 0.12,
            keypoints: 0.25,
            angles: 0.0,
        },
    ),
    (
        Category::Animals,
        Weights {
            placement: 0.08,
            outline: 0.32,
            silhouette: 0.22,
            grid: 0.12,
            tone: 0.10,
            ssim: 0.06,
            line_confidence: 0.08,
            keypoints: 0.15,
            angles: 0.0,
        },
    ),
    (
        Category::RealLife,
        Weights {
            placement: 0.06,
            outline: 0.25,
            silhouette: 0.15,
            grid: 0.10,
            tone: 0.22,
            ssim: 0.12,
            line_confidence: 0.06,
            keypoints: 0.15,
            angles: 0.0,
        },
    ),
    (
        Category::Environment,
        Weights {
            placement: 0.05,
            outline: 0.22,
            silhouette: 0.10,
            grid: 0.15,
            tone: 0.28,
            ssim: 0.15,
            line_confidence: 0.05,
            keypoints: 0.05,
            angles: 0.0,
        },
    ),
];

impl Weights {
    pub fn for_category(c: Category) -> Self {
        DEFAULT_WEIGHTS
            .iter()
            .find(|(k, _)| *k == c)
            .map(|(_, w)| *w)
            .expect("every category has default weights")
    }
}

/// Maps the weighted similarity `m` (0–1) to a 1–10 score.
///
/// ```text
/// t     = clamp((m − floor) / (pass − floor), 0, 1)
/// score = 1 + 9 · t^gamma
/// ```
///
/// * `m ≥ pass` → 10: the passing threshold, so small wobbles and the
///   unavoidable gap between line art and a photo still earn full marks.
/// * `m ≤ floor` → 1: blank, random or unrelated drawings.
/// * `gamma < 1` bends the curve upwards so near-misses land at 8–9.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreCurve {
    pub floor: f32,
    pub pass: f32,
    pub gamma: f32,
}

impl Default for ScoreCurve {
    fn default() -> Self {
        Self {
            floor: 0.40,
            pass: 0.86,
            gamma: 1.0,
        }
    }
}

impl ScoreCurve {
    pub fn score(&self, m: f32) -> f32 {
        let span = (self.pass - self.floor).max(1e-6);
        let t = ((m - self.floor) / span).clamp(0.0, 1.0);
        1.0 + 9.0 * t.powf(self.gamma.max(0.05))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_endpoints_and_monotonic() {
        let c = ScoreCurve::default();
        assert_eq!(c.score(0.0), 1.0);
        assert_eq!(c.score(1.0), 10.0);
        assert_eq!(c.score(c.pass), 10.0);
        let mut last = 0.0;
        for i in 0..=100 {
            let s = c.score(i as f32 / 100.0);
            assert!(s >= last);
            last = s;
        }
    }

    #[test]
    fn every_category_has_weights() {
        for c in [
            Category::Anime,
            Category::RealLife,
            Category::Animals,
            Category::Environment,
            Category::Fundamentals,
        ] {
            let w = Weights::for_category(c);
            assert!(w.outline > 0.0);
        }
    }

    #[test]
    fn category_names_match_web_ids() {
        let c: Category = serde_json::from_str("\"real-life\"").unwrap();
        assert_eq!(c, Category::RealLife);
    }
}
