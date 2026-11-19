//! Checks against the real CLIP files. Skipped when they are missing (CI):
//! run `crates/ref-filters/fetch-model.sh clip` first.

use std::path::{Path, PathBuf};

use image::{DynamicImage, Rgb, RgbImage};
use ref_filters::clip::{self, ClipText, ClipVision};

fn model_dir() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("models/clip-vit-b32");
    if clip::present(&d) {
        Some(d)
    } else {
        eprintln!("CLIP model missing, skipping (run fetch-model.sh clip)");
        None
    }
}

#[test]
fn tokenizer_matches_the_reference_ids() {
    let Some(d) = model_dir() else { return };
    let text = ClipText::load(&d).unwrap();
    // ids from the Hugging Face tokenizer for the same files
    let cases: &[(&str, &[i64])] = &[
        (
            "a photo of a cat",
            &[49406, 320, 1125, 539, 320, 2368, 49407],
        ),
        (
            "an anime portrait of a character's face",
            &[49406, 550, 6469, 5352, 539, 320, 4009, 568, 1710, 49407],
        ),
        (
            "a cartoon of a man riding a scooter",
            &[
                49406, 320, 7651, 539, 320, 786, 6765, 320, 14199, 652, 49407,
            ],
        ),
    ];
    for (p, want) in cases {
        let ids = text.tokenizer().encode(p);
        assert_eq!(&ids[..want.len()], *want, "{p}");
        assert_eq!(ids.len(), clip::CONTEXT);
    }
}

#[test]
fn zero_shot_colours() {
    let Some(d) = model_dir() else { return };
    let (text, vision) = (ClipText::load(&d).unwrap(), ClipVision::load(&d).unwrap());
    let prompts = ["a red square", "a green square", "a blue square"];
    let t: Vec<Vec<f32>> = prompts.iter().map(|p| text.embed(p).unwrap()).collect();
    for (i, c) in [[220, 20, 20], [20, 200, 40], [20, 40, 220]]
        .into_iter()
        .enumerate()
    {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 300, Rgb(c)));
        let e = vision.embed(&img).unwrap();
        assert_eq!(e.len(), clip::DIM);
        let sims: Vec<f32> = t
            .iter()
            .map(|t| t.iter().zip(&e).map(|(a, b)| a * b).sum())
            .collect();
        let best = (0..3).max_by(|a, b| sims[*a].total_cmp(&sims[*b])).unwrap();
        assert_eq!(best, i, "{} → {sims:?}", prompts[i]);
    }
}
