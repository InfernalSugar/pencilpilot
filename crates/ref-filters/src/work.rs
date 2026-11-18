//! Shared low-level image maths: the working grayscale image, tone spread
//! and the smoothness (high-frequency energy) heuristic.

use image::GrayImage;

/// Long side of the working image used by the text and smoothness checks.
pub(crate) const WORK_LONG: u32 = 800;

/// |Laplacian| below this many gray levels is treated as quantisation noise.
const LAP_FLOOR: f32 = 3.0;

/// Grayscale working image as floats (0–255), long side ≤ [`WORK_LONG`].
pub(crate) struct Work {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Work {
    pub fn from_luma(luma: &GrayImage) -> Self {
        let (w, h) = luma.dimensions();
        let small;
        let src = if w.max(h) > WORK_LONG {
            // Box-filtered downscale: cheap and properly anti-aliased.
            small = image::imageops::thumbnail(luma, WORK_LONG, WORK_LONG);
            &small
        } else {
            luma
        };
        Self {
            w: src.width() as usize,
            h: src.height() as usize,
            d: src.as_raw().iter().map(|&v| v as f32).collect(),
        }
    }

    /// Standard deviation of the gray values.
    pub fn std_dev(&self) -> f32 {
        let n = self.d.len().max(1) as f64;
        let mean = self.d.iter().map(|&v| v as f64).sum::<f64>() / n;
        let var = self
            .d
            .iter()
            .map(|&v| (v as f64 - mean).powi(2))
            .sum::<f64>()
            / n;
        var.sqrt() as f32
    }

    /// Mean |Laplacian| ÷ mean |gradient| over the interior.
    ///
    /// Photos and hand-drawn art carry grain, texture and pencil/ink noise,
    /// so the Laplacian (second derivative) is large next to the gradient.
    /// Airbrushed or AI-rendered pictures are made of soft ramps and clean
    /// edges: strong gradients, very little second-derivative energy.
    /// Returns `None` for (nearly) flat images.
    pub fn hf_ratio(&self) -> Option<f32> {
        let (w, h) = (self.w, self.h);
        if w < 3 || h < 3 {
            return None;
        }
        let (mut lap, mut grad) = (0f64, 0f64);
        for y in 1..h - 1 {
            let row = y * w;
            for x in 1..w - 1 {
                let c = self.d[row + x];
                let l = self.d[row + x - 1];
                let r = self.d[row + x + 1];
                let u = self.d[row - w + x];
                let dn = self.d[row + w + x];
                // Soft floor: ignore 8-bit quantisation / JPEG jitter.
                lap += ((l + r + u + dn - 4.0 * c).abs() - LAP_FLOOR).max(0.0) as f64;
                let (gx, gy) = ((r - l) * 0.5, (dn - u) * 0.5);
                grad += ((gx * gx + gy * gy) as f64).sqrt();
            }
        }
        let n = ((w - 2) * (h - 2)) as f64;
        if grad / n < 0.05 {
            return None;
        }
        Some((lap / grad) as f32)
    }
}
