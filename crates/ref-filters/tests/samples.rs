//! Checks against the real, approved sample references and the real model.
//!
//! Both are git-ignored, so every test here skips cleanly when they are
//! missing (CI). Locally:
//!
//! ```sh
//! crates/ref-filters/fetch-model.sh
//! REF_SAMPLES_DIR=$PWD/apps/web/static/references \
//!     cargo test --release -p pencilpilot-ref-filters --test samples -- --nocapture
//! ```

use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat};
use ref_filters::{FilterError, Filters, Thresholds, Verdict, decode, hamming, phash};

fn model_path() -> PathBuf {
    std::env::var_os("REF_FILTERS_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("models/open_nsfw.onnx"))
}

fn samples() -> Option<Vec<(String, DynamicImage)>> {
    let dir = std::env::var_os("REF_SAMPLES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/static/references")
        });
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort();
    Some(
        files
            .iter()
            .map(|p| {
                let img = decode(&std::fs::read(p).unwrap())
                    .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                (p.file_stem().unwrap().to_string_lossy().into_owned(), img)
            })
            .collect(),
    )
}

fn flagged<'a>(rows: &'a [(String, Verdict)], reason: &str) -> Vec<&'a str> {
    rows.iter()
        .filter(|(_, v)| v.has(reason))
        .map(|(n, _)| n.as_str())
        .collect()
}

fn pct(mut xs: Vec<f32>, q: f32) -> f32 {
    xs.sort_by(f32::total_cmp);
    xs[((xs.len() - 1) as f32 * q).round() as usize]
}

#[test]
fn missing_model_is_a_clear_error() {
    let err = Filters::new(Path::new("/nonexistent/open_nsfw.onnx")).unwrap_err();
    assert!(matches!(err, FilterError::ModelMissing(_)));
    assert!(err.to_string().contains("fetch-model.sh"));
}

#[test]
fn samples_pass_the_maths_checks() {
    let Some(samples) = samples() else {
        eprintln!("SKIP: no sample references (set REF_SAMPLES_DIR)");
        return;
    };
    let f = Filters::without_nsfw();
    let rows: Vec<(String, Verdict)> = samples
        .iter()
        .map(|(n, img)| (n.clone(), f.check(img)))
        .collect();
    for reason in [
        "tiny",
        "extreme_aspect",
        "blank",
        "too_faint",
        "too_busy",
        "text_heavy",
    ] {
        let hits = flagged(&rows, reason);
        eprintln!("{reason}: {} {hits:?}", hits.len());
    }
    // All samples are approved, drawable references: allow ≤ 2 % false
    // positives overall, none for the size checks.
    let rejected = rows.iter().filter(|(_, v)| !v.ok).count();
    assert!(
        rejected * 50 <= rows.len(),
        "{rejected} of {} rejected",
        rows.len()
    );
    assert!(flagged(&rows, "tiny").is_empty());
    assert!(
        rows.iter()
            .all(|(_, v)| v.has("nsfw_unchecked") && !v.nsfw_checked)
    );
}

#[test]
fn samples_phash_survives_resize_and_reencode() {
    let Some(samples) = samples() else {
        eprintln!("SKIP: no sample references (set REF_SAMPLES_DIR)");
        return;
    };
    let hashes: Vec<u64> = samples.iter().map(|(_, img)| phash(img)).collect();
    let mut worst = 0;
    for ((name, img), &h) in samples.iter().zip(&hashes) {
        let small = img.resize(640, 640, image::imageops::FilterType::Triangle);
        let mut jpg = Vec::new();
        ref_filters::flatten_alpha(&small)
            .to_rgb8()
            .write_to(&mut std::io::Cursor::new(&mut jpg), ImageFormat::Jpeg)
            .unwrap();
        let d = hamming(h, phash(&decode(&jpg).unwrap()));
        worst = worst.max(d);
        assert!(d <= 6, "{name}: resized + re-encoded distance {d}");
    }
    let mut pairs = Vec::new();
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            pairs.push(hamming(hashes[i], hashes[j]) as f32);
        }
    }
    let min = pct(pairs.clone(), 0.0);
    let under20 = pairs.iter().filter(|&&d| d < 20.0).count();
    eprintln!(
        "pHash: worst self-distance {worst}; distinct pairs min {min}, p5 {}, median {}, {under20} of {} pairs < 20",
        pct(pairs.clone(), 0.05),
        pct(pairs.clone(), 0.5),
        pairs.len()
    );
    // Distinct pictures must never look like near-duplicates (≤ 10).
    assert!(min > 10.0, "two different samples are within {min} bits");
}

#[test]
fn model_loads_and_runs() {
    let path = model_path();
    if !path.is_file() {
        eprintln!(
            "SKIP: no NSFW model at {} (run fetch-model.sh)",
            path.display()
        );
        return;
    }
    let f = Filters::new(&path).expect("model loads");
    assert!(f.has_nsfw_model());
    // A plain drawing-like picture is safe.
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(1280, 853, |x, y| {
        let (dx, dy) = (x as f32 - 640.0, y as f32 - 420.0);
        let r = (dx * dx + dy * dy).sqrt();
        if (r - 250.0).abs() < 4.0 {
            image::Rgb([20, 20, 20])
        } else {
            image::Rgb([235, 240, 250])
        }
    }));
    let v = f.check(&img);
    assert!(v.nsfw_checked && !v.has("nsfw_unchecked"), "{v:?}");
    assert!((0.0..0.1).contains(&v.nsfw), "{v:?}");
}

#[test]
fn samples_score_low_on_nsfw() {
    let path = model_path();
    let (true, Some(samples)) = (path.is_file(), samples()) else {
        eprintln!("SKIP: needs both the model and the samples");
        return;
    };
    let f = Filters::new(&path).unwrap();
    let rows: Vec<(String, Verdict)> = samples
        .iter()
        .map(|(n, img)| (n.clone(), f.check(img)))
        .collect();
    let scores: Vec<f32> = rows.iter().map(|(_, v)| v.nsfw).collect();
    let (p95, max) = (pct(scores.clone(), 0.95), pct(scores.clone(), 1.0));
    let hits = flagged(&rows, "nsfw");
    eprintln!(
        "NSFW on {} safe samples: median {:.4}, p95 {p95:.4}, max {max:.4}; flagged at {}: {hits:?}",
        rows.len(),
        pct(scores, 0.5),
        Thresholds::default().max_nsfw
    );
    assert!(p95 < 0.15, "p95 {p95}");
    // The conservative threshold costs a few false rejects (skin-heavy
    // close-ups); keep it under 4 %.
    assert!(hits.len() * 25 <= rows.len(), "{hits:?}");
}
