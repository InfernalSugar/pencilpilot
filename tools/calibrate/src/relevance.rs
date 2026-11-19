//! `calibrate relevance`: does the grader tell genuine attempts from drawings
//! that have nothing to do with the reference?
//!
//! For each approved reference: genuine attempts (the perfect line art and the
//! synthetic mistakes at levels 0–3) and unrelated drawings that cover the
//! same area (dense scribbles, hatching, another reference's line art,
//! scribbles inside the subject's silhouette). Reports scores and the
//! `relevance` measurement per group.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use grader::{GradeOptions, Mode, grade_with};
use image::{GrayImage, Luma};
use serde_json::Value;

use crate::{FAMILIES, H, W, grader_category, line_art, load_fitted, perturb, root, to_gray};

struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn stroke(img: &mut GrayImage, a: (f32, f32), b: (f32, f32)) {
    let n = ((b.0 - a.0).hypot(b.1 - a.1) / 0.5).ceil().max(1.0) as usize;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (px, py) = (x as i64 + dx, y as i64 + dy);
            if px >= 0 && py >= 0 && px < W as i64 && py < H as i64 {
                img.put_pixel(px as u32, py as u32, Luma([30]));
            }
        }
    }
}

type Rect = (f32, f32, f32, f32);

/// Box of the line art's ink (2nd–98th percentile).
fn ink_box(art: &GrayImage) -> Rect {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for (x, y, p) in art.enumerate_pixels() {
        if p[0] < 128 {
            xs.push(x as f32);
            ys.push(y as f32);
        }
    }
    if xs.is_empty() {
        return (200.0, 150.0, 600.0, 450.0);
    }
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);
    let q = |v: &Vec<f32>, q: f32| v[((v.len() - 1) as f32 * q) as usize];
    (q(&xs, 0.02), q(&ys, 0.02), q(&xs, 0.98), q(&ys, 0.98))
}

/// Coarse silhouette of the line art: 16 px cells with ink, grown by 1 cell
/// and with enclosed holes left as they are (rough is fine here).
fn silhouette(art: &GrayImage) -> impl Fn(f32, f32) -> bool {
    let (cw, ch) = (W as i32 / 16, H as i32 / 16);
    let mut cells = vec![false; (cw * ch) as usize];
    for (x, y, p) in art.enumerate_pixels() {
        if p[0] < 128 {
            cells[((y as i32 / 16).min(ch - 1) * cw + (x as i32 / 16).min(cw - 1)) as usize] = true;
        }
    }
    let grown: Vec<bool> = (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dx| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0 && ny >= 0 && nx < cw && ny < ch && cells[(ny * cw + nx) as usize]
                })
            })
        })
        .collect();
    move |x: f32, y: f32| {
        let (cx, cy) = (x as i32 / 16, y as i32 / 16);
        x >= 0.0 && y >= 0.0 && cx < cw && cy < ch && grown[(cy * cw + cx) as usize]
    }
}

/// Dense scratchy random-walk scribbles inside `area` (and `inside`).
fn scribbles(area: Rect, n: usize, seed: u64, inside: &dyn Fn(f32, f32) -> bool) -> GrayImage {
    let mut img = GrayImage::from_pixel(W, H, Luma([255]));
    let mut r = Rng(seed.wrapping_mul(2654435761) | 1);
    let mut made = 0;
    for _ in 0..n * 50 {
        if made == n {
            break;
        }
        let mut p = (
            area.0 + r.f() * (area.2 - area.0),
            area.1 + r.f() * (area.3 - area.1),
        );
        if !inside(p.0, p.1) {
            continue;
        }
        made += 1;
        let mut a = r.f() * std::f32::consts::TAU;
        for _ in 0..(8 + (r.f() * 30.0) as usize) {
            a += (r.f() - 0.5) * 1.6;
            let q = (p.0 + 7.0 * a.cos(), p.1 + 7.0 * a.sin());
            if !inside(q.0, q.1) || q.0 < area.0 || q.0 > area.2 || q.1 < area.1 || q.1 > area.3 {
                a += std::f32::consts::PI * (0.6 + 0.4 * r.f());
                continue;
            }
            stroke(&mut img, p, q);
            p = q;
        }
    }
    img
}

/// Parallel lines over `area`, `spacing` px apart, at `angle` radians.
fn hatching(img: &mut GrayImage, area: Rect, spacing: f32, angle: f32) {
    let (cx, cy) = ((area.0 + area.2) / 2.0, (area.1 + area.3) / 2.0);
    let r = (area.2 - area.0).hypot(area.3 - area.1) / 2.0;
    let (dx, dy) = (angle.cos(), angle.sin());
    let mut o = -r;
    while o <= r {
        let (px, py) = (cx - dy * o, cy + dx * o);
        let pts: Vec<(f32, f32)> = (-200..=200)
            .map(|i| {
                let t = i as f32 / 200.0 * r;
                (px + dx * t, py + dy * t)
            })
            .filter(|&(x, y)| x >= area.0 && x <= area.2 && y >= area.1 && y <= area.3)
            .collect();
        if pts.len() >= 2 {
            stroke(img, pts[0], pts[pts.len() - 1]);
        }
        o += spacing;
    }
}

fn hatched(area: Rect, spacing: f32, angles: &[f32]) -> GrayImage {
    let mut img = GrayImage::from_pixel(W, H, Luma([255]));
    for &a in angles {
        hatching(&mut img, area, spacing, a);
    }
    img
}

/// The unrelated drawings for one reference (`other`: another reference's
/// line art, i.e. a different subject).
fn unrelated(art: &GrayImage, other: &GrayImage, seed: u64) -> Vec<(&'static str, GrayImage)> {
    let area = ink_box(art);
    let sil = silhouette(art);
    let any = |_: f32, _: f32| true;
    let dense = (((area.2 - area.0) * (area.3 - area.1)) / 900.0) as usize;
    vec![
        ("scribbles", scribbles(area, dense, seed, &any)),
        (
            "sparse scribbles",
            scribbles(area, dense / 4, seed + 1, &any),
        ),
        (
            "scribbles in silhouette",
            scribbles(area, dense, seed + 2, &sil),
        ),
        ("hatching 45°", hatched(area, 12.0, &[0.785])),
        (
            "hatching vertical",
            hatched(area, 14.0, &[std::f32::consts::FRAC_PI_2]),
        ),
        ("cross-hatching", hatched(area, 16.0, &[0.785, 2.356])),
        ("other subject", other.clone()),
    ]
}

/// The line art without the letterbox border (the edge between the white
/// bars and the picture): students don't draw the picture's frame. The grader
/// ignores that band anyway, but once a copy is drawn smaller or shifted the
/// frame lines would move off it and count as stray lines.
fn without_bars(art: &GrayImage, reference: &GrayImage) -> GrayImage {
    let white = |x: u32, y: u32| reference.get_pixel(x, y)[0] >= 248;
    let rows: Vec<bool> = (0..H).map(|y| (0..W).all(|x| white(x, y))).collect();
    let cols: Vec<bool> = (0..W).map(|x| (0..H).all(|y| white(x, y))).collect();
    let near = |v: &[bool], i: u32| {
        (i.saturating_sub(4)..(i + 5).min(v.len() as u32)).any(|j| v[j as usize])
    };
    GrayImage::from_fn(W, H, |x, y| {
        if near(&rows, y) || near(&cols, x) {
            Luma([255])
        } else {
            *art.get_pixel(x, y)
        }
    })
}

#[derive(Default)]
struct Stats {
    before: Vec<f32>,
    score: Vec<f32>,
    relevance: Vec<f32>,
}

fn summary(v: &[f32]) -> String {
    if v.is_empty() {
        return "–".into();
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    let mean = s.iter().sum::<f32>() / s.len() as f32;
    format!("{mean:.2} ({:.2}–{:.2})", s[0], s[s.len() - 1])
}

pub fn run(per: usize, out: &Path) {
    let refs_dir = root().join("content/references");
    let images = root().join("apps/web/static/references");
    let mut refs: Vec<(String, GrayImage)> = Vec::new();
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
        for r in list["references"]
            .as_array()
            .into_iter()
            .flatten()
            .take(per)
        {
            let id = r["id"].as_str().unwrap_or("");
            if let Some(img) = load_fitted(&images.join(format!("{id}.jpg"))) {
                refs.push((name.clone(), img));
            }
        }
    }
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    let arts: Vec<GrayImage> = refs
        .iter()
        .map(|(_, r)| without_bars(&line_art(r), r))
        .collect();
    // (mode, group, kind) → stats
    let mut by: BTreeMap<(&str, &str, String), Stats> = BTreeMap::new();
    // genuine attempts whose score the gate lowered: (drop, label)
    let mut genuine_drops: Vec<(f32, String)> = Vec::new();
    let mut highest_unrelated: Vec<(f32, String)> = Vec::new();
    for (i, (cat, reference)) in refs.iter().enumerate() {
        let refg = to_gray(reference);
        let art = &arts[i];
        let other = &arts[(i + refs.len() / 2 + 1) % refs.len()];
        let mut drawings: Vec<(&str, String, GrayImage)> = Vec::new();
        for fam in FAMILIES {
            for level in 0..3 {
                drawings.push((
                    "genuine",
                    format!("{fam} L{level}"),
                    perturb(art, fam, level, i as u64 * 31 + level as u64),
                ));
            }
        }
        for (kind, img) in unrelated(art, other, i as u64 + 7) {
            drawings.push(("unrelated", kind.to_string(), img));
        }
        for (mode, mname) in [(Mode::Finished, "finished"), (Mode::Sketch, "sketch")] {
            let opts = GradeOptions {
                category: grader_category(cat),
                mode,
                ..Default::default()
            };
            for (group, kind, img) in &drawings {
                let g = grade_with(&to_gray(img), &refg, &opts).expect("grade");
                let ungated = GradeOptions {
                    relevance_gate: Some(false),
                    ..opts.clone()
                };
                let before = grade_with(&to_gray(img), &refg, &ungated).expect("grade");
                let rel = g.measurement("relevance").unwrap_or(f32::NAN);
                let s = by.entry((mname, group, kind.clone())).or_default();
                s.before.push(before.score);
                s.score.push(g.score);
                s.relevance.push(rel);
                let label = format!(
                    "{mname} {cat}#{i} {kind}: score {:.1} → {:.1}, relevance {rel:.2}",
                    before.score, g.score
                );
                if *group == "genuine" {
                    genuine_drops.push((before.score - g.score, label));
                } else {
                    highest_unrelated.push((rel, label));
                }
            }
        }
        eprintln!("  {cat}#{i}");
    }
    let mut md = String::from(
        "# Relevance report\n\nGenerated by `cargo run --release -p calibrate -- relevance`.\n\n\
Genuine attempts are the synthetic mistakes of the calibration benchmark at levels 0–2; unrelated drawings \
cover the same area as the subject but don't depict it. Values: mean (min–max).\n\n\
| Mode | Group | Drawing | Score without gate | Score with gate | Relevance |\n|---|---|---|---|---|---|\n",
    );
    for ((mode, group, kind), s) in &by {
        md += &format!(
            "| {mode} | {group} | {kind} | {} | {} | {} |\n",
            summary(&s.before),
            summary(&s.score),
            summary(&s.relevance)
        );
    }
    genuine_drops.sort_by(|a, b| b.0.total_cmp(&a.0));
    highest_unrelated.sort_by(|a, b| b.0.total_cmp(&a.0));
    let n = genuine_drops.len();
    let dropped = |by: f32| genuine_drops.iter().filter(|d| d.0 >= by).count();
    md += &format!(
        "\nGenuine attempts whose score the gate lowered: by ≥ 0.1 in {} of {n}, by ≥ 1 in {}, by ≥ 3 in {}. Largest drops:\n\n",
        dropped(0.1),
        dropped(1.0),
        dropped(3.0)
    );
    for (_, l) in genuine_drops.iter().take(12) {
        md += &format!("- {l}\n");
    }
    md += "\nHighest relevance among unrelated drawings:\n\n";
    for (_, l) in highest_unrelated.iter().take(8) {
        md += &format!("- {l}\n");
    }
    md += &format!("\n{} references.\n", refs.len());
    fs::create_dir_all(out.parent().unwrap()).ok();
    fs::write(out, &md).unwrap();
    println!("{md}");
}
