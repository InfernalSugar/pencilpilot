//! Vetting: decide whether a downloaded picture may be shown, and how hard it is.
//!
//! Pixabay's own flags are checked here; the image checks (blank, text-heavy,
//! too faint/busy, size, near-duplicates, NSFW guard) come from `ref-filters`.

use grader::difficulty::Difficulty;
use image::DynamicImage;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Vetted {
    pub difficulty: Difficulty,
    /// perceptual hash for near-duplicate detection, if the vetter computes one
    pub phash: Option<u64>,
}

pub trait Vetter: Send + Sync + 'static {
    /// `Ok` = safe and drawable; `Err(reason)` = reject.
    fn vet(&self, img: &DynamicImage) -> Result<Vetted, String>;
    /// Safety only (no drawability checks), for drawings people share.
    /// `Err(reason)` = don't show it. Fails closed.
    fn safe_to_share(&self, _img: &DynamicImage) -> Result<(), String> {
        Err("unsupported".into())
    }
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

/// The full check from `ref-filters`: blank, text-heavy, too faint/busy,
/// size/aspect, near-duplicate hash and the NSFW safety guard (ADR-0005).
pub struct FiltersVetter(pub ref_filters::Filters);

impl Vetter for FiltersVetter {
    fn vet(&self, img: &DynamicImage) -> Result<Vetted, String> {
        let v = self.0.check(img);
        if v.ok {
            Ok(Vetted {
                difficulty: v.difficulty,
                phash: Some(v.phash),
            })
        } else {
            Err(v.reasons.join(","))
        }
    }

    fn safe_to_share(&self, img: &DynamicImage) -> Result<(), String> {
        self.0.safe_to_share(img).map_err(String::from)
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
    fn filters_vetter_rejects_blank_and_hashes_good_images() {
        let v = FiltersVetter(ref_filters::Filters::without_nsfw());
        let blank = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            800,
            600,
            image::Rgb([255, 255, 255]),
        ));
        assert!(v.vet(&blank).unwrap_err().contains("blank"));
        // a drawable picture: a dark disc with a lighter ring
        let disc = DynamicImage::ImageRgb8(image::RgbImage::from_fn(800, 600, |x, y| {
            let d = ((x as f32 - 400.0).powi(2) + (y as f32 - 300.0).powi(2)).sqrt();
            let g = if d < 150.0 {
                40
            } else if d < 200.0 {
                160
            } else {
                255
            };
            image::Rgb([g, g, g])
        }));
        let ok = v.vet(&disc).expect("a simple disc is drawable");
        assert!(ok.phash.is_some());
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
    fn filters_vetter_rejects_tiny_and_extreme_aspect() {
        let v = FiltersVetter(ref_filters::Filters::without_nsfw());
        assert!(
            v.vet(&DynamicImage::new_luma8(200, 200))
                .unwrap_err()
                .contains("tiny")
        );
        assert!(
            v.vet(&DynamicImage::new_luma8(2000, 500))
                .unwrap_err()
                .contains("extreme_aspect")
        );
    }
}
