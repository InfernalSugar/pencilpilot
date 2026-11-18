//! Vetting: decide whether a downloaded picture may be shown, and how hard it is.
//!
//! Hard rules applied here (plain maths + Pixabay's own flags). The richer
//! checks (text-heavy, near-duplicates, NSFW model) come from the
//! `ref-filters` crate and plug in through [`Vetter`].

use grader::Gray;
use grader::difficulty::{Difficulty, difficulty};
use image::DynamicImage;
use serde_json::Value;

/// Canvas size references are fitted into (keep in sync with the web app).
pub const CANVAS: (u32, u32) = (800, 600);

#[derive(Debug, Clone)]
pub struct Vetted {
    pub difficulty: Difficulty,
    /// perceptual hash for near-duplicate detection, if the vetter computes one
    pub phash: Option<u64>,
}

pub trait Vetter: Send + Sync + 'static {
    /// `Ok` = safe and drawable; `Err(reason)` = reject.
    fn vet(&self, img: &DynamicImage) -> Result<Vetted, String>;
}

/// Pixabay's own flags + banned tags. Unknown AI flag counts as AI (reject).
pub fn pixabay_flags_ok(hit: &Value) -> Result<(), &'static str> {
    if hit["isAiGenerated"].as_bool() != Some(false) {
        return Err("ai_generated");
    }
    if hit["isGRated"].as_bool() != Some(true) {
        return Err("not_g_rated");
    }
    if hit["isLowQuality"].as_bool() == Some(true) {
        return Err("low_quality");
    }
    let tags = hit["tags"].as_str().unwrap_or("").to_lowercase();
    const BANNED: &[&str] = &[
        "ai generated",
        "ai-generated",
        "generative",
        "midjourney",
        "stable diffusion",
        "dall-e",
        "nude",
        "naked",
        "sexy",
        "lingerie",
        "bikini",
        "erotic",
        "underwear",
        "blood",
        "gore",
        "gun",
        "rifle",
        "pistol",
        "pokemon",
        "naruto",
        "disney",
        "marvel",
        "mickey",
        "pikachu",
        "text",
        "quote",
        "logo",
        "infographic",
    ];
    if BANNED
        .iter()
        .any(|b| tags.split(", ").any(|t| t.contains(b)))
    {
        return Err("banned_tag");
    }
    Ok(())
}

/// Composite any transparency onto white paper. Vector art often has a
/// transparent background, which would otherwise turn black.
pub fn on_white(img: DynamicImage) -> DynamicImage {
    if !img.color().has_alpha() {
        return img;
    }
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba)
}

/// Fit an image into the canvas (contain, centred, white) as grayscale.
pub fn fitted_gray(img: &DynamicImage) -> Gray {
    let g = img.to_luma8();
    let (w, h) = CANVAS;
    let s = (w as f32 / g.width() as f32).min(h as f32 / g.height() as f32);
    let (dw, dh) = (
        ((g.width() as f32 * s).round() as u32).clamp(1, w),
        ((g.height() as f32 * s).round() as u32).clamp(1, h),
    );
    let small = image::imageops::resize(&g, dw, dh, image::imageops::FilterType::Triangle);
    let mut canvas = image::GrayImage::from_pixel(w, h, image::Luma([255]));
    image::imageops::overlay(
        &mut canvas,
        &small,
        ((w - dw) / 2) as i64,
        ((h - dh) / 2) as i64,
    );
    Gray::new(w, h, canvas.into_raw()).expect("sizes match")
}

/// Baseline vetter: size/aspect checks and difficulty bounds (too faint / too busy).
pub struct BasicVetter;

impl Vetter for BasicVetter {
    fn vet(&self, img: &DynamicImage) -> Result<Vetted, String> {
        let (w, h) = (img.width(), img.height());
        if w.min(h) < 400 {
            return Err("tiny".into());
        }
        let aspect = w.max(h) as f32 / w.min(h) as f32;
        if aspect > 3.0 {
            return Err("extreme_aspect".into());
        }
        let d = difficulty(&fitted_gray(img));
        if d.score < 12.0 {
            return Err("too_faint".into());
        }
        if d.score > 82.0 {
            return Err("too_busy".into());
        }
        Ok(Vetted {
            difficulty: d,
            phash: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flags() {
        let ok = json!({"isAiGenerated": false, "isGRated": true, "isLowQuality": false, "tags": "cat, pet"});
        assert!(pixabay_flags_ok(&ok).is_ok());
        let mut ai = ok.clone();
        ai["isAiGenerated"] = json!(true);
        assert_eq!(pixabay_flags_ok(&ai), Err("ai_generated"));
        let mut unknown = ok.clone();
        unknown.as_object_mut().unwrap().remove("isAiGenerated");
        assert_eq!(pixabay_flags_ok(&unknown), Err("ai_generated"));
        let mut quote = ok.clone();
        quote["tags"] = json!("quote, motivation");
        assert_eq!(pixabay_flags_ok(&quote), Err("banned_tag"));
    }

    #[test]
    fn transparent_becomes_white() {
        let clear = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([0, 0, 0, 0]),
        ));
        let w = on_white(clear).to_rgb8();
        assert_eq!(w.get_pixel(0, 0).0, [255, 255, 255]);
        let half = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([0, 0, 0, 128]),
        ));
        assert_eq!(on_white(half).to_rgb8().get_pixel(0, 0).0, [127, 127, 127]);
    }

    #[test]
    fn basic_vetter_rejects_blank_and_tiny() {
        let blank = DynamicImage::new_luma8(800, 600);
        // all-black image has no edges at all
        assert_eq!(BasicVetter.vet(&blank).unwrap_err(), "too_faint");
        let tiny = DynamicImage::new_luma8(200, 200);
        assert_eq!(BasicVetter.vet(&tiny).unwrap_err(), "tiny");
        let wide = DynamicImage::new_luma8(2000, 500);
        assert_eq!(BasicVetter.vet(&wide).unwrap_err(), "extreme_aspect");
    }
}
