//! Auto-vetting of reference pictures fetched live (Pixabay) before students
//! see them.
//!
//! Everything here is plain image maths (ADR-0002), with two exceptions, and
//! neither is ever used for scoring or feedback on drawings:
//!
//! * an NSFW classifier, used **only as a safety guard** (ADR-0005);
//! * CLIP ([`clip`], [`sort`]), used **only to sort reference pictures** into
//!   category and topic (ADR-0009).
//!
//! ```no_run
//! use ref_filters::Filters;
//! let filters = Filters::new("crates/ref-filters/models/open_nsfw.onnx".as_ref())?;
//! let img = ref_filters::decode(&std::fs::read("pic.jpg")?)?;
//! let v = filters.check(&img);
//! if !v.ok {
//!     println!("rejected: {:?}", v.reasons);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! | reason            | hard? | how                                                         |
//! |-------------------|-------|-------------------------------------------------------------|
//! | `tiny`            | yes   | short side < [`Thresholds::min_short_side`]                 |
//! | `extreme_aspect`  | yes   | long ÷ short > [`Thresholds::max_aspect`]                   |
//! | `blank`           | yes   | flat tones or almost no edges ([`Verdict::blank`])          |
//! | `too_faint`       | yes   | grader difficulty score below [`Thresholds::min_difficulty`] |
//! | `too_busy`        | yes   | difficulty above [`Thresholds::max_difficulty`] or edge density above [`Thresholds::max_edge_density`] |
//! | `text_heavy`      | yes   | rows of glyph-like blobs ([`Verdict::text`])                |
//! | `nsfw`            | yes   | model probability ≥ [`Thresholds::max_nsfw`]                |
//! | `nsfw_error`      | yes   | the model failed on this image (fail closed)                |
//! | `nsfw_unchecked`  | no    | built with [`Filters::without_nsfw`]; see `nsfw_checked`    |

pub mod clip;
mod nsfw;
mod phash;
pub mod sort;
mod text;
pub mod words;
mod work;

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use grader::difficulty::{Difficulty, difficulty};
use image::{DynamicImage, GrayImage, RgbImage};
use serde::{Deserialize, Serialize};

pub use nsfw::{
    BGR_MEAN, INPUT as NSFW_INPUT, RESIZE as NSFW_RESIZE, preprocess as nsfw_preprocess,
};
pub use phash::{hamming, phash};

/// The drawing canvas references are fitted into (same as `ref-importer`).
pub const CANVAS: (u32, u32) = (800, 600);

/// Every cut-off in one place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thresholds {
    /// Reject if the short side is below this (px).
    pub min_short_side: u32,
    /// Reject if long side ÷ short side is above this.
    pub max_aspect: f32,
    /// Reject if [`Verdict::blank`] is at or above this.
    pub max_blank: f32,
    /// Reject if the difficulty score (0–100) is below this.
    pub min_difficulty: f32,
    /// Reject if the difficulty score (0–100) is above this.
    pub max_difficulty: f32,
    /// Also `too_busy` if the grader's edge density (edge pixels ÷ canvas)
    /// is above this. The 0–100 score saturates near 80 for pure texture,
    /// so edge density separates noise from busy-but-drawable pictures.
    pub max_edge_density: f32,
    /// Reject if [`Verdict::text`] is at or above this.
    pub max_text: f32,
    /// Reject if [`Verdict::nsfw`] is at or above this.
    pub max_nsfw: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            min_short_side: 400,
            max_aspect: 3.0,
            max_blank: 0.8,
            min_difficulty: 10.0,
            max_difficulty: 82.0,
            max_edge_density: 0.28,
            max_text: 0.5,
            max_nsfw: 0.3,
        }
    }
}

/// Result of [`Filters::check`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    /// False if any hard check fails.
    pub ok: bool,
    /// Stable snake_case reason codes (see the crate docs). May contain
    /// the non-blocking `nsfw_unchecked` even when `ok` is true.
    pub reasons: Vec<String>,
    /// 0–1 probability of unsafe content (0 when not checked).
    pub nsfw: f32,
    /// Whether the NSFW model actually ran on this image.
    pub nsfw_checked: bool,
    /// 0–1 how blank/flat it is.
    pub blank: f32,
    /// 0–1 how text-like it is.
    pub text: f32,
    /// 0–1 "airbrushed/AI-like smoothness" heuristic. Report only.
    pub smoothness: f32,
    /// 64-bit DCT perceptual hash; compare with [`hamming`].
    pub phash: u64,
    /// The grader's difficulty rating of the picture fitted into [`CANVAS`].
    pub difficulty: Difficulty,
}

impl Verdict {
    /// True if `reason` is among the reasons.
    pub fn has(&self, reason: &str) -> bool {
        self.reasons.iter().any(|r| r == reason)
    }
}

#[derive(Debug)]
pub enum FilterError {
    /// The model file does not exist.
    ModelMissing(PathBuf),
    /// The model could not be loaded or run.
    Model(String),
    /// The image could not be decoded.
    Decode(String),
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelMissing(p) => write!(
                f,
                "NSFW model not found at {} (run crates/ref-filters/fetch-model.sh)",
                p.display()
            ),
            Self::Model(e) => write!(f, "NSFW model error: {e}"),
            Self::Decode(e) => write!(f, "can't decode image: {e}"),
        }
    }
}

impl std::error::Error for FilterError {}

/// Decode image bytes, sniffing the real format (Pixabay sometimes serves
/// PNG/BMP under a .jpg name).
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, FilterError> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| FilterError::Decode(e.to_string()))?
        .decode()
        .map_err(|e| FilterError::Decode(e.to_string()))
}

/// Holds the NSFW model (if any) and the thresholds. `Send + Sync`: share
/// one instance (e.g. in an `Arc`) across request handlers.
pub struct Filters {
    nsfw: Option<nsfw::NsfwModel>,
    thresholds: Thresholds,
}

impl fmt::Debug for Filters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filters")
            .field("nsfw_model", &self.nsfw.is_some())
            .field("thresholds", &self.thresholds)
            .finish()
    }
}

impl Filters {
    /// Load the NSFW model from `model_path` (ONNX).
    pub fn new(model_path: &Path) -> Result<Self, FilterError> {
        Ok(Self {
            nsfw: Some(nsfw::NsfwModel::load(model_path)?),
            thresholds: Thresholds::default(),
        })
    }

    /// No model: `nsfw` is always 0, `nsfw_checked` false and the reasons
    /// include `nsfw_unchecked`. For tests and CI only.
    pub fn without_nsfw() -> Self {
        Self {
            nsfw: None,
            thresholds: Thresholds::default(),
        }
    }

    /// Replace the thresholds.
    pub fn with_thresholds(mut self, thresholds: Thresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    pub fn thresholds(&self) -> &Thresholds {
        &self.thresholds
    }

    /// Whether an NSFW model is loaded.
    pub fn has_nsfw_model(&self) -> bool {
        self.nsfw.is_some()
    }

    /// The NSFW guard alone, for drawings people choose to share (drawability
    /// checks don't apply to them). Fails closed: with no model loaded, or if
    /// the model errors, the picture is not safe to show.
    pub fn safe_to_share(&self, img: &DynamicImage) -> Result<(), &'static str> {
        let Some(model) = &self.nsfw else {
            return Err("nsfw_unchecked");
        };
        match model.score(flatten_alpha(img).as_ref()) {
            Ok(p) if p < self.thresholds.max_nsfw => Ok(()),
            Ok(_) => Err("nsfw"),
            Err(_) => Err("nsfw_error"),
        }
    }

    /// Run every check on a decoded image.
    pub fn check(&self, img: &DynamicImage) -> Verdict {
        let t = &self.thresholds;
        let mut reasons = Vec::new();
        let (w, h) = (img.width(), img.height());
        let (short, long) = (w.min(h), w.max(h));

        if short < t.min_short_side {
            reasons.push("tiny");
        }
        if short == 0 || long as f32 / short as f32 > t.max_aspect {
            reasons.push("extreme_aspect");
        }

        let flat = flatten_alpha(img);
        let img = flat.as_ref();
        let luma = img.to_luma8();
        let (diff, coverage) = fitted_difficulty(&luma);
        let work = work::Work::from_luma(&luma);

        let blank = blankness(work.std_dev(), diff.edge_density, coverage);
        if blank >= t.max_blank {
            reasons.push("blank");
        }
        if diff.score < t.min_difficulty {
            reasons.push("too_faint");
        }
        if diff.score > t.max_difficulty || diff.edge_density > t.max_edge_density {
            reasons.push("too_busy");
        }

        let text = text::text_stats(&work).score;
        if text >= t.max_text {
            reasons.push("text_heavy");
        }

        let smoothness = work.hf_ratio().map_or(0.0, smoothness_from_ratio);
        let phash = phash::phash_luma(&luma);

        let (nsfw, nsfw_checked) = match &self.nsfw {
            Some(model) => match model.score(img) {
                Ok(p) => {
                    if p >= t.max_nsfw {
                        reasons.push("nsfw");
                    }
                    (p, true)
                }
                Err(_) => {
                    // Fail closed: if the guard can't run, don't show it.
                    reasons.push("nsfw_error");
                    (1.0, false)
                }
            },
            None => (0.0, false),
        };

        let ok = reasons.is_empty();
        let mut reasons: Vec<String> = reasons.into_iter().map(String::from).collect();
        if self.nsfw.is_none() {
            reasons.push("nsfw_unchecked".into());
        }
        Verdict {
            ok,
            reasons,
            nsfw,
            nsfw_checked,
            blank,
            text,
            smoothness,
            phash,
            difficulty: diff,
        }
    }
}

/// Composite transparent pictures onto white (how the site shows them);
/// opaque ones are borrowed unchanged. Pixabay serves many line-art PNGs
/// whose ink lives only in the alpha channel; dropping alpha would make
/// them look blank. [`Filters::check`] and [`phash`] do this themselves.
pub fn flatten_alpha(img: &DynamicImage) -> Cow<'_, DynamicImage> {
    if !img.color().has_alpha() {
        return Cow::Borrowed(img);
    }
    let rgba = img.to_rgba8();
    let rgb = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let a = a as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        image::Rgb([mix(r), mix(g), mix(b)])
    });
    Cow::Owned(DynamicImage::ImageRgb8(rgb))
}

/// Fit into the 800×600 canvas on white exactly like `ref-importer`'s
/// `fitted_gray`, then rate with the grader. Also returns the fraction of
/// the canvas the picture covers (the rest is white padding).
fn fitted_difficulty(luma: &GrayImage) -> (Difficulty, f32) {
    let (cw, ch) = CANVAS;
    let mut canvas = GrayImage::from_pixel(cw, ch, image::Luma([255]));
    let mut coverage = 0.0;
    if luma.width() > 0 && luma.height() > 0 {
        let s = (cw as f32 / luma.width() as f32).min(ch as f32 / luma.height() as f32);
        let (dw, dh) = (
            ((luma.width() as f32 * s).round() as u32).clamp(1, cw),
            ((luma.height() as f32 * s).round() as u32).clamp(1, ch),
        );
        let small = image::imageops::resize(luma, dw, dh, image::imageops::FilterType::Triangle);
        image::imageops::overlay(
            &mut canvas,
            &small,
            ((cw - dw) / 2) as i64,
            ((ch - dh) / 2) as i64,
        );
        coverage = (dw * dh) as f32 / (cw * ch) as f32;
    }
    let gray = grader::Gray::new(cw, ch, canvas.into_raw()).expect("canvas size matches");
    (difficulty(&gray), coverage)
}

/// Gray std below which a picture counts as flat (fully flat at 0).
const FLAT_STD: f32 = 24.0;
/// Edge density (relative to the picture's own area) below which it counts
/// as edgeless (fully edgeless at 0).
const EDGELESS_DENSITY: f32 = 0.02;

/// 0–1: max of "flat tones" and "almost no edges".
fn blankness(std: f32, edge_density: f32, coverage: f32) -> f32 {
    let flat = (1.0 - std / FLAT_STD).clamp(0.0, 1.0);
    let own_density = if coverage > 0.0 {
        edge_density / coverage
    } else {
        0.0
    };
    let edgeless = (1.0 - own_density / EDGELESS_DENSITY).clamp(0.0, 1.0);
    flat.max(edgeless)
}

/// |Laplacian| ÷ |gradient| at which smoothness is 0.5.
const SMOOTH_HALF: f32 = 1.0;

/// Low high-frequency ratio → high smoothness.
fn smoothness_from_ratio(r: f32) -> f32 {
    SMOOTH_HALF / (SMOOTH_HALF + r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, RgbImage};

    fn gray(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> DynamicImage {
        DynamicImage::ImageLuma8(GrayImage::from_fn(w, h, |x, y| Luma([f(x, y)])))
    }

    /// A drawable picture: a few overlapping shapes with outlines.
    fn shapes(w: u32, h: u32) -> DynamicImage {
        gray(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let d1 = ((fx - 0.4).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let d2 = ((fx - 0.65).powi(2) + (fy - 0.4).powi(2)).sqrt();
            if (d1 - 0.25).abs() < 0.006 || (d2 - 0.15).abs() < 0.006 {
                20
            } else if d1 < 0.25 {
                170
            } else if d2 < 0.15 || (fx > 0.1 && fx < 0.3 && fy > 0.7 && fy < 0.9) {
                110
            } else {
                235
            }
        })
    }

    #[test]
    fn plain_shapes_pass() {
        let v = Filters::without_nsfw().check(&shapes(1280, 853));
        assert!(v.ok, "{v:?}");
        assert_eq!(v.reasons, ["nsfw_unchecked"]);
        assert!(!v.nsfw_checked);
        assert_eq!(v.nsfw, 0.0);
    }

    #[test]
    fn tiny_and_extreme_aspect() {
        let f = Filters::without_nsfw();
        let v = f.check(&shapes(300, 380));
        assert!(!v.ok && v.has("tiny"), "{v:?}");
        let v = f.check(&shapes(1600, 450));
        assert!(!v.ok && v.has("extreme_aspect") && !v.has("tiny"), "{v:?}");
        let v = f.check(&DynamicImage::new_rgb8(0, 0));
        assert!(v.has("tiny") && v.has("extreme_aspect"));
    }

    #[test]
    fn uniform_and_gradient_are_blank() {
        let f = Filters::without_nsfw();
        let v = f.check(&gray(1000, 700, |_, _| 128));
        assert!(v.has("blank") && v.has("too_faint") && !v.ok, "{v:?}");
        assert!(v.blank > 0.99);
        // A soft sky gradient: plenty of tone range, no edges.
        let v = f.check(&gray(1000, 700, |_, y| (40 + y * 200 / 700) as u8));
        assert!(v.has("blank"), "{v:?}");
    }

    #[test]
    fn noise_is_too_busy() {
        // Random 8 px cells: edges and fragments everywhere.
        let v = Filters::without_nsfw().check(&gray(1000, 700, |x, y| {
            let (cx, cy) = (x / 8, y / 8);
            let v =
                (cx.wrapping_mul(7919) ^ cy.wrapping_mul(104729)).wrapping_mul(2654435761) >> 24;
            v as u8
        }));
        assert!(v.has("too_busy"), "{v:?}");
    }

    #[test]
    fn thresholds_are_configurable() {
        let f = Filters::without_nsfw().with_thresholds(Thresholds {
            min_short_side: 1000,
            ..Thresholds::default()
        });
        assert!(f.check(&shapes(1280, 853)).has("tiny"));
        assert_eq!(f.thresholds().min_short_side, 1000);
    }

    #[test]
    fn verdict_round_trips_as_json() {
        let v = Filters::without_nsfw().check(&shapes(800, 600));
        let s = serde_json::to_string(&v).unwrap();
        let back: Verdict = serde_json::from_str(&s).unwrap();
        assert_eq!(back.phash, v.phash);
        assert_eq!(back.reasons, v.reasons);
    }

    #[test]
    fn airbrushed_is_smoother_than_grainy() {
        let f = Filters::without_nsfw();
        let soft = |x: u32, y: u32| {
            let (fx, fy) = (x as f32 / 1000.0 - 0.5, y as f32 / 700.0 - 0.5);
            128.0 + 100.0 * (-(fx * fx + fy * fy) * 8.0).exp() - 30.0 * (fx * 9.0).sin()
        };
        let a = f.check(&gray(1000, 700, |x, y| soft(x, y) as u8));
        let b = f.check(&gray(1000, 700, |x, y| {
            let n = ((x.wrapping_mul(7919) ^ y.wrapping_mul(104729)).wrapping_mul(2654435761) >> 28)
                as f32;
            (soft(x, y) + n - 8.0).clamp(0.0, 255.0) as u8
        }));
        assert!(
            a.smoothness > b.smoothness + 0.2,
            "{} vs {}",
            a.smoothness,
            b.smoothness
        );
    }

    #[test]
    fn transparent_line_art_is_not_blank() {
        // Black outline drawn only in the alpha channel (RGB all zero), as
        // in Pixabay's transparent PNGs.
        let shapes = shapes(1000, 700).to_luma8();
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(1000, 700, |x, y| {
            let v = shapes.get_pixel(x, y).0[0];
            image::Rgba([0, 0, 0, 255 - v])
        }));
        let v = Filters::without_nsfw().check(&img);
        assert!(!v.has("blank") && !v.has("too_faint"), "{v:?}");
    }

    #[test]
    fn colour_input_works() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(900, 600, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        }));
        let v = Filters::without_nsfw().check(&img);
        assert_eq!(
            v.difficulty.tier,
            grader::difficulty::tier_for(v.difficulty.score)
        );
    }

    #[test]
    fn filters_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Filters>();
    }
}
