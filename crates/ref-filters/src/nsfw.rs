//! NSFW safety guard: Yahoo open_nsfw (ResNet-50 "thin", BSD-2-Clause) as
//! ONNX, run with `tract` (pure Rust). This is the only learned model in the
//! crate and it is used **only** to keep unsafe pictures away from students,
//! never for scoring (ADR-0002). See README.md for provenance.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, RgbImage, imageops::FilterType};
use tract_onnx::prelude::*;

use crate::FilterError;

/// Side the picture is resized to before the centre crop.
pub const RESIZE: u32 = 256;
/// Network input side (centre crop of the resized picture).
pub const INPUT: usize = 224;
/// Per-channel means subtracted from the **BGR** pixels (Caffe/VGG means).
pub const BGR_MEAN: [f32; 3] = [104.0, 117.0, 123.0];
/// JPEG quality of the round trip (PIL's default, as in the reference).
const JPEG_QUALITY: u8 = 75;

type Plan = std::sync::Arc<TypedSimplePlan>;

pub(crate) struct NsfwModel {
    plan: Plan,
}

impl NsfwModel {
    pub fn load(path: &Path) -> Result<Self, FilterError> {
        if !path.is_file() {
            return Err(FilterError::ModelMissing(path.to_path_buf()));
        }
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|m| {
                m.with_input_fact(0, f32::fact([1, INPUT, INPUT, 3]).into())?
                    .into_optimized()?
                    .into_runnable()
            })
            .map_err(|e| FilterError::Model(format!("{e:#}")))?;
        Ok(Self { plan })
    }

    /// Probability (0–1) that the picture is NSFW.
    pub fn score(&self, img: &DynamicImage) -> Result<f32, FilterError> {
        let input = preprocess(img);
        let out = self
            .plan
            .run(tvec!(input.into_tvalue()))
            .map_err(|e| FilterError::Model(format!("{e:#}")))?;
        let probs = out[0]
            .to_plain_array_view::<f32>()
            .map_err(|e| FilterError::Model(format!("{e:#}")))?;
        // Output is softmax [sfw, nsfw].
        probs
            .iter()
            .nth(1)
            .copied()
            .ok_or_else(|| FilterError::Model("model output has fewer than 2 values".into()))
    }
}

/// open_nsfw "YAHOO" preprocessing, as in `opennsfw2.preprocess_image`:
/// (transparent → composited on white) RGB → bilinear resize to 256×256 → JPEG round trip (q 75) → centre crop
/// 224×224 → RGB→BGR → subtract [104, 117, 123]. Output is NHWC float32
/// `[1, 224, 224, 3]`, values roughly −123…+151 (no scaling to 0–1).
pub fn preprocess(img: &DynamicImage) -> Tensor {
    let rgb = crate::flatten_alpha(img).to_rgb8();
    let resized = if rgb.width() == 0 || rgb.height() == 0 {
        RgbImage::new(RESIZE, RESIZE)
    } else {
        image::imageops::resize(&rgb, RESIZE, RESIZE, FilterType::Triangle)
    };
    // The original Caffe pipeline saw JPEG artefacts; the reference keeps a
    // JPEG round trip to match. Fall back to the plain pixels if it fails.
    let mut jpg = Vec::new();
    let resized = JpegEncoder::new_with_quality(Cursor::new(&mut jpg), JPEG_QUALITY)
        .encode_image(&resized)
        .ok()
        .and_then(|_| image::load_from_memory_with_format(&jpg, image::ImageFormat::Jpeg).ok())
        .map(|d| d.to_rgb8())
        .unwrap_or(resized);
    let off = ((RESIZE as usize - INPUT) / 2) as u32;
    let mut data = Vec::with_capacity(INPUT * INPUT * 3);
    for y in 0..INPUT as u32 {
        for x in 0..INPUT as u32 {
            let p = resized.get_pixel(x + off, y + off).0;
            data.push(p[2] as f32 - BGR_MEAN[0]);
            data.push(p[1] as f32 - BGR_MEAN[1]);
            data.push(p[0] as f32 - BGR_MEAN[2]);
        }
    }
    tract_ndarray::Array4::from_shape_vec((1, INPUT, INPUT, 3), data)
        .expect("shape matches data length")
        .into_tensor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessing_matches_reference_layout() {
        // A flat colour survives resize + JPEG almost exactly, so every
        // pixel must be (B, G, R) − mean, in NHWC order.
        let img =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(640, 480, image::Rgb([200, 100, 50])));
        let t = preprocess(&img);
        assert_eq!(t.shape(), &[1, INPUT, INPUT, 3]);
        let v = t.to_plain_array_view::<f32>().unwrap();
        let expect = [50.0 - 104.0, 100.0 - 117.0, 200.0 - 123.0];
        for y in [0, 100, 223] {
            for x in [0, 57, 223] {
                for c in 0..3 {
                    let got = v[[0, y, x, c]];
                    assert!((got - expect[c]).abs() <= 3.0, "({x},{y},{c}) = {got}");
                }
            }
        }
    }

    #[test]
    fn preprocessing_takes_the_centre_crop() {
        // Left half black, right half white, 256 px wide: the crop starts at
        // x = 16, so column 111 (x = 127) is black and 112 (x = 128) white.
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(256, 256, |x, _| {
            if x < 128 {
                image::Rgb([0, 0, 0])
            } else {
                image::Rgb([255, 255, 255])
            }
        }));
        let v = preprocess(&img);
        let v = v.to_plain_array_view::<f32>().unwrap();
        assert!(v[[0, 100, 100, 0]] < -90.0);
        assert!(v[[0, 100, 125, 0]] > 130.0);
    }
}
