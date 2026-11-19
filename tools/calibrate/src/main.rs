//! Calibration for PencilPilot's grader (M8-T01).
//!
//! ```text
//! cargo run --release -p calibrate -- synthetic [--per 6] [--out docs/calibration/synthetic.md] [--no-gate]
//! cargo run --release -p calibrate -- relevance [--per 6] [--out docs/calibration/relevance.md]
//! cargo run --release -p calibrate -- fit [--labels data/calibration/labels.jsonl]
//! ```
//!
//! * **synthetic**: turns each approved reference into "perfect" line art, then
//!   makes drawings with known, controlled mistakes (too small, shifted, parts
//!   missing, wobbly, stray scribbles) at increasing strength. A fair grader
//!   scores the perfect version high and scores drop as mistakes grow. We report
//!   mean scores per level and the Spearman correlation (level vs score).
//! * **fit**: fits the score curve (floor / pass) to human 1–10 labels collected
//!   on the dev rating page (`/dev/rate`), per category and overall.

mod relevance;
mod weights;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use grader::{Category, GradeOptions, Gray, ScoreCurve, grade_with};
use image::{GrayImage, Luma};
use serde::Deserialize;
use serde_json::Value;

const W: u32 = 800;
const H: u32 = 600;

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Grader category for a site category (cartoon is graded like anime).
pub fn grader_category(c: &str) -> Category {
    let c = if c == "cartoon" { "anime" } else { c };
    serde_json::from_value(Value::String(c.into())).unwrap_or_default()
}

// ── Images ──────────────────────────────────────────────────────────────

/// Decode (sniffing the format), put transparency on white, fit into the canvas.
pub(crate) fn load_fitted(path: &Path) -> Option<GrayImage> {
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    let g = image::DynamicImage::ImageRgba8(rgba).to_luma8();
    let s = (W as f32 / g.width() as f32).min(H as f32 / g.height() as f32);
    let (dw, dh) = (
        ((g.width() as f32 * s) as u32).clamp(1, W),
        ((g.height() as f32 * s) as u32).clamp(1, H),
    );
    let small = image::imageops::resize(&g, dw, dh, image::imageops::FilterType::Triangle);
    let mut canvas = GrayImage::from_pixel(W, H, Luma([255]));
    image::imageops::overlay(
        &mut canvas,
        &small,
        ((W - dw) / 2) as i64,
        ((H - dh) / 2) as i64,
    );
    Some(canvas)
}

/// "Perfect" student line art: the strongest ~8% of edges, as dark lines.
pub fn line_art(reference: &GrayImage) -> GrayImage {
    let b = image::imageops::blur(reference, 1.6);
    let (w, h) = b.dimensions();
    let mut mag = vec![0f32; (w * h) as usize];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let p = |dx: i32, dy: i32| {
                b.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[0] as f32
            };
            let gx = p(1, -1) + 2.0 * p(1, 0) + p(1, 1) - p(-1, -1) - 2.0 * p(-1, 0) - p(-1, 1);
            let gy = p(-1, 1) + 2.0 * p(0, 1) + p(1, 1) - p(-1, -1) - 2.0 * p(0, -1) - p(1, -1);
            mag[(y * w + x) as usize] = (gx * gx + gy * gy).sqrt();
        }
    }
    let mut sorted: Vec<f32> = mag.iter().copied().filter(|m| *m > 1.0).collect();
    sorted.sort_by(f32::total_cmp);
    let cut = sorted
        .get((sorted.len() as f32 * 0.92) as usize)
        .copied()
        .unwrap_or(f32::MAX);
    GrayImage::from_fn(w, h, |x, y| {
        Luma([if mag[(y * w + x) as usize] >= cut {
            30
        } else {
            255
        }])
    })
}

/// Tiny deterministic RNG (xorshift) so reports are reproducible.
struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Resample `src` through a mapping from destination to source coordinates.
fn remap(src: &GrayImage, f: impl Fn(f32, f32) -> (f32, f32)) -> GrayImage {
    GrayImage::from_fn(W, H, |x, y| {
        let (sx, sy) = f(x as f32, y as f32);
        if sx < 0.0 || sy < 0.0 || sx >= W as f32 || sy >= H as f32 {
            Luma([255])
        } else {
            *src.get_pixel(sx as u32, sy as u32)
        }
    })
}

/// The kinds of mistakes we simulate, each at levels 0 (none) … 3 (strong).
pub const FAMILIES: &[&str] = &["scale", "shift", "missing", "wobble", "scribble"];

pub fn perturb(art: &GrayImage, family: &str, level: usize, seed: u64) -> GrayImage {
    let l = level as f32;
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let mut rng = Rng(seed.wrapping_mul(2654435761) | 1);
    match family {
        // drawn smaller: 100%, 85%, 70%, 55%
        "scale" => {
            let s = 1.0 - 0.15 * l;
            remap(art, |x, y| (cx + (x - cx) / s, cy + (y - cy) / s))
        }
        // drawn off-centre: 0, 6%, 12%, 20% of the width
        "shift" => {
            let d = [0.0, 0.06, 0.12, 0.2][level] * W as f32;
            remap(art, |x, y| (x - d, y - d * 0.5))
        }
        // parts left out: 0, 25%, 50%, 75% of a 4×3 grid's cells wiped
        "missing" => {
            let mut out = art.clone();
            let cells: Vec<(u32, u32)> = (0..3).flat_map(|r| (0..4).map(move |c| (c, r))).collect();
            let k = (cells.len() as f32 * 0.25 * l).round() as usize;
            let mut order: Vec<(f32, (u32, u32))> =
                cells.into_iter().map(|c| (rng.f(), c)).collect();
            order.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, (c, r)) in order.into_iter().take(k) {
                for y in r * 200..(r + 1) * 200 {
                    for x in c * 200..(c + 1) * 200 {
                        out.put_pixel(x, y, Luma([255]));
                    }
                }
            }
            out
        }
        // shaky / inaccurate lines: warp amplitude 0, 5, 12, 25 px
        "wobble" => {
            let a = [0.0, 5.0, 12.0, 25.0][level];
            let ph = rng.f() * std::f32::consts::TAU;
            remap(art, |x, y| {
                (x + a * (y / 31.0 + ph).sin(), y + a * (x / 37.0 + ph).cos())
            })
        }
        // stray lines: 0, 6, 18, 40 random strokes
        "scribble" => {
            let mut out = art.clone();
            for _ in 0..[0, 6, 18, 40][level] {
                let (x0, y0) = (rng.f() * W as f32, rng.f() * H as f32);
                let (x1, y1) = (x0 + (rng.f() - 0.5) * 300.0, y0 + (rng.f() - 0.5) * 300.0);
                let n = 200;
                for i in 0..=n {
                    let t = i as f32 / n as f32;
                    let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                    if x >= 0.0 && y >= 0.0 && x < W as f32 - 1.0 && y < H as f32 - 1.0 {
                        out.put_pixel(x as u32, y as u32, Luma([30]));
                        out.put_pixel(x as u32 + 1, y as u32, Luma([30]));
                    }
                }
            }
            out
        }
        _ => art.clone(),
    }
}

pub(crate) fn to_gray(g: &GrayImage) -> Gray {
    Gray::new(g.width(), g.height(), g.as_raw().clone()).expect("sizes match")
}

/// Spearman rank correlation.
pub fn spearman(xs: &[f32], ys: &[f32]) -> f32 {
    fn ranks(v: &[f32]) -> Vec<f32> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
        let mut r = vec![0f32; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            let avg = (i + j) as f32 / 2.0 + 1.0;
            for k in i..=j {
                r[idx[k]] = avg;
            }
            i = j + 1;
        }
        r
    }
    let (rx, ry) = (ranks(xs), ranks(ys));
    let n = xs.len() as f32;
    let (mx, my) = (rx.iter().sum::<f32>() / n, ry.iter().sum::<f32>() / n);
    let cov: f32 = rx.iter().zip(&ry).map(|(a, b)| (a - mx) * (b - my)).sum();
    let vx: f32 = rx.iter().map(|a| (a - mx).powi(2)).sum();
    let vy: f32 = ry.iter().map(|b| (b - my).powi(2)).sum();
    if vx == 0.0 || vy == 0.0 {
        0.0
    } else {
        cov / (vx * vy).sqrt()
    }
}

// ── synthetic ───────────────────────────────────────────────────────────

fn synthetic(per: usize, out: &Path, no_gate: bool) {
    let refs_dir = root().join("content/references");
    let images = root().join("apps/web/static/references");
    // (category, family) → level → scores
    let mut by: BTreeMap<(String, &str), [Vec<f32>; 4]> = BTreeMap::new();
    let mut used = 0;
    for entry in fs::read_dir(&refs_dir).expect("content/references") {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if path.extension().is_none_or(|e| e != "json")
            || name == "candidates"
            || name == "rejected"
        {
            continue;
        }
        let list: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let refs = list["references"].as_array().cloned().unwrap_or_default();
        for (i, r) in refs.iter().take(per).enumerate() {
            let id = r["id"].as_str().unwrap_or("");
            let Some(reference) = load_fitted(&images.join(format!("{id}.jpg"))) else {
                eprintln!("  skip {id}: image missing (run `pnpm refs sync`)");
                continue;
            };
            used += 1;
            let opts = GradeOptions {
                category: grader_category(&name),
                relevance_gate: Some(!no_gate),
                ..Default::default()
            };
            let art = line_art(&reference);
            let refg = to_gray(&reference);
            for fam in FAMILIES {
                for level in 0..4 {
                    let d = perturb(&art, fam, level, i as u64 * 31 + level as u64);
                    let g = grade_with(&to_gray(&d), &refg, &opts).expect("grade");
                    if level == 0 && *fam == "scale" && std::env::var("CAL_DEBUG").is_ok() {
                        let m: Vec<String> = g
                            .measurements
                            .iter()
                            .map(|m| format!("{}={:.2}", m.name, m.value))
                            .collect();
                        eprintln!(
                            "DEBUG {name}/{id} score={:.1} sim={:.2} {}",
                            g.score,
                            g.similarity,
                            m.join(" ")
                        );
                    }
                    by.entry((name.clone(), fam)).or_default()[level].push(g.score);
                }
            }
            eprintln!("  graded {name}/{id}");
        }
    }

    let mut md = String::from(
        "# Synthetic calibration report\n\nGenerated by `cargo run --release -p calibrate -- synthetic`.\n\n\
Each approved reference is turned into perfect line art (strongest ~8% of edges), then drawn with a known mistake at \
levels 0 (none) → 3 (strong). Scores should start high and fall as the mistake grows: **ρ** is the Spearman \
correlation between level and score (−1 = perfectly falling).\n\n\
| Category | Mistake | L0 | L1 | L2 | L3 | ρ | OK |\n|---|---|---|---|---|---|---|---|\n",
    );
    let mut fails = 0;
    for ((cat, fam), levels) in &by {
        let means: Vec<f32> = levels
            .iter()
            .map(|v| v.iter().sum::<f32>() / v.len().max(1) as f32)
            .collect();
        let (xs, ys): (Vec<f32>, Vec<f32>) = levels
            .iter()
            .enumerate()
            .flat_map(|(l, v)| v.iter().map(move |s| (l as f32, *s)))
            .unzip();
        let rho = spearman(&xs, &ys);
        let ok = means[0] >= 7.0 && rho <= -0.4 && means[3] < means[0];
        if !ok {
            fails += 1;
        }
        md += &format!(
            "| {cat} | {fam} | {:.1} | {:.1} | {:.1} | {:.1} | {rho:.2} | {} |\n",
            means[0],
            means[1],
            means[2],
            means[3],
            if ok { "✅" } else { "⚠️" }
        );
    }
    md += &format!(
        "\n{used} references; {fails} row(s) flagged ⚠️ (perfect < 7, or scores not falling clearly).\n\n\
Notes: \"scale\" and \"shift\" are partly forgiven on purpose: the grader aligns the drawing before comparing \
(placement is only part of the score), so a correct drawing that is simply smaller or off-centre still scores \
fairly well.\n"
    );
    fs::create_dir_all(out.parent().unwrap()).ok();
    fs::write(out, &md).unwrap();
    println!("{md}");
}

// ── fit ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Label {
    category: String,
    similarity: f32,
    human: f32,
}

pub fn mae(labels: &[(f32, f32)], c: &ScoreCurve) -> f32 {
    labels
        .iter()
        .map(|(s, h)| (c.score(*s) - h).abs())
        .sum::<f32>()
        / labels.len().max(1) as f32
}

/// Grid-search floor/pass minimising mean absolute error vs human scores.
pub fn fit_curve(labels: &[(f32, f32)]) -> (ScoreCurve, f32) {
    let mut best = (ScoreCurve::default(), f32::MAX);
    for f in 15..=65 {
        for p in (f + 10)..=98 {
            let c = ScoreCurve {
                floor: f as f32 / 100.0,
                pass: p as f32 / 100.0,
                gamma: 1.0,
            };
            let e = mae(labels, &c);
            if e < best.1 {
                best = (c, e);
            }
        }
    }
    best
}

fn fit(path: &Path) {
    let text = fs::read_to_string(path).unwrap_or_else(|_| {
        eprintln!(
            "No labels at {} yet. Rate drawings at /dev/rate (dev server) first.",
            path.display()
        );
        std::process::exit(1)
    });
    let labels: Vec<Label> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut groups: BTreeMap<String, Vec<(f32, f32)>> = BTreeMap::new();
    for l in &labels {
        groups
            .entry(l.category.clone())
            .or_default()
            .push((l.similarity, l.human));
        groups
            .entry("(all)".into())
            .or_default()
            .push((l.similarity, l.human));
    }
    let now = ScoreCurve::default();
    println!(
        "| Category | Labels | Current MAE | Fitted floor | Fitted pass | Fitted MAE |\n|---|---|---|---|---|---|"
    );
    for (cat, v) in &groups {
        if v.len() < 5 {
            println!(
                "| {cat} | {} | {:.2} | – | – | need ≥ 5 |",
                v.len(),
                mae(v, &now)
            );
            continue;
        }
        let (c, e) = fit_curve(v);
        println!(
            "| {cat} | {} | {:.2} | {:.2} | {:.2} | {e:.2} |",
            v.len(),
            mae(v, &now),
            c.floor,
            c.pass
        );
    }
    println!(
        "\nCurrent curve: floor {:.2}, pass {:.2}. Apply a fitted curve in crates/grader/src/score.rs (ScoreCurve::default) once there are 20–30 labels per category.",
        now.floor, now.pass
    );

    // Weights too, for ratings that carry the grader's measurements.
    let ratings: Vec<weights::Rating> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let fitted = weights::fit_all(&ratings, 12);
    if fitted.is_empty() {
        println!(
            "\nNo ratings with measurements yet (rate drawings saved since the measurements were added)."
        );
        return;
    }
    println!(
        "\n## Weights per category\n\n| Group | Ratings | Current error | Fitted error | Fitted error on unseen ratings |\n|---|---|---|---|---|"
    );
    let mut out = BTreeMap::new();
    for (k, v) in &fitted {
        match v {
            Ok(f) => {
                println!(
                    "| {k} | {} | {:.2} | {:.2} | {:.2} |",
                    f.labels, f.current_mae, f.fitted_mae, f.left_out_mae
                );
                out.insert(k.clone(), f);
            }
            Err(n) => println!("| {k} | {n} | – | – | need ≥ 12 |"),
        }
    }
    println!(
        "\nErrors are average points out of 10 between the grader and you. Only adopt a fit when its error on unseen ratings is clearly lower than the current error."
    );
    if !out.is_empty() {
        let dest = path.with_file_name("fitted-weights.json");
        fs::write(&dest, serde_json::to_string_pretty(&out).unwrap()).unwrap();
        println!("Fitted weights and curves written to {}", dest.display());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("synthetic") => synthetic(
            flag(&args, "--per")
                .and_then(|v| v.parse().ok())
                .unwrap_or(6),
            &flag(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| root().join("docs/calibration/synthetic.md")),
            args.iter().any(|a| a == "--no-gate"),
        ),
        Some("relevance") => relevance::run(
            flag(&args, "--per")
                .and_then(|v| v.parse().ok())
                .unwrap_or(6),
            &flag(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| root().join("docs/calibration/relevance.md")),
        ),
        Some("fit") => fit(&flag(&args, "--labels")
            .map(PathBuf::from)
            .unwrap_or_else(|| root().join("data/calibration/labels.jsonl"))),
        _ => {
            eprintln!(
                "usage: calibrate synthetic [--per N] [--out FILE] [--no-gate] | calibrate relevance [--per N] [--out FILE] | calibrate fit [--labels FILE]"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spearman_basics() {
        assert!((spearman(&[1., 2., 3.], &[3., 2., 1.]) + 1.0).abs() < 1e-6);
        assert!((spearman(&[1., 2., 3.], &[1., 5., 9.]) - 1.0).abs() < 1e-6);
        assert_eq!(spearman(&[1., 1., 1.], &[1., 2., 3.]), 0.0);
    }

    #[test]
    fn fit_recovers_a_known_curve() {
        let truth = ScoreCurve {
            floor: 0.3,
            pass: 0.8,
            gamma: 1.0,
        };
        let labels: Vec<(f32, f32)> = (0..50)
            .map(|i| {
                let s = i as f32 / 50.0;
                (s, truth.score(s))
            })
            .collect();
        let (c, e) = fit_curve(&labels);
        assert!(e < 0.05, "{e}");
        assert!(
            (c.floor - 0.3).abs() <= 0.02 && (c.pass - 0.8).abs() <= 0.02,
            "{c:?}"
        );
    }

    #[test]
    fn perturbations_get_worse_with_level() {
        // a ring as "reference" and as line art
        let reference = GrayImage::from_fn(W, H, |x, y| {
            let d = ((x as f32 - 400.0).powi(2) + (y as f32 - 300.0).powi(2)).sqrt();
            Luma([if d < 160.0 { 60 } else { 255 }])
        });
        let art = line_art(&reference);
        let refg = to_gray(&reference);
        let opts = GradeOptions {
            category: grader_category("animals"),
            ..Default::default()
        };
        for fam in ["missing", "wobble", "scribble"] {
            let s: Vec<f32> = (0..4)
                .map(|l| {
                    grade_with(&to_gray(&perturb(&art, fam, l, 7)), &refg, &opts)
                        .unwrap()
                        .score
                })
                .collect();
            assert!(s[0] >= 8.0, "{fam} perfect scored {}", s[0]);
            assert!(s[3] < s[0], "{fam}: {s:?}");
        }
    }

    #[test]
    fn categories_map() {
        assert_eq!(grader_category("cartoon"), grader_category("anime"));
        assert_eq!(grader_category("nonsense"), Category::default());
    }
}
