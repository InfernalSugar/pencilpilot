//! Run every check over a folder of pictures and print one row per file
//! plus a summary (for calibrating thresholds).
//!
//! ```sh
//! cargo run --release -p pencilpilot-ref-filters --example scan -- \
//!     apps/web/static/references [crates/ref-filters/models/open_nsfw.onnx]
//! ```

use std::path::Path;
use std::time::Instant;

use ref_filters::{Filters, decode, hamming};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .first()
        .map(String::as_str)
        .unwrap_or("apps/web/static/references");
    let filters = match args.get(1) {
        Some(m) => Filters::new(Path::new(m)).expect("load model"),
        None => Filters::without_nsfw(),
    };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    println!("file\tw\th\tok\treasons\tnsfw\tblank\ttext\tsmooth\tdiff\ttier\tms\tedge\tpieces");
    let mut rows = Vec::new();
    let mut times = Vec::new();
    for p in &files {
        let Ok(img) = std::fs::read(p)
            .map_err(|e| e.to_string())
            .and_then(|b| decode(&b).map_err(|e| e.to_string()))
        else {
            eprintln!("{}: can't decode", p.display());
            continue;
        };
        let t0 = Instant::now();
        let v = filters.check(&img);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        times.push(ms);
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        println!(
            "{name}\t{}\t{}\t{}\t{}\t{:.4}\t{:.3}\t{:.3}\t{:.3}\t{:.1}\t{}\t{ms:.1}\t{:.4}\t{}",
            img.width(),
            img.height(),
            v.ok,
            v.reasons.join(","),
            v.nsfw,
            v.blank,
            v.text,
            v.smoothness,
            v.difficulty.score,
            v.difficulty.tier,
            v.difficulty.edge_density,
            v.difficulty.pieces,
        );
        rows.push((name, v));
    }
    if rows.is_empty() {
        return;
    }

    let pct = |mut xs: Vec<f64>, q: f64| {
        xs.sort_by(f64::total_cmp);
        xs[((xs.len() - 1) as f64 * q).round() as usize]
    };
    eprintln!("\n{} images", rows.len());
    eprintln!(
        "time ms: median {:.1}, p95 {:.1}, max {:.1}",
        pct(times.clone(), 0.5),
        pct(times.clone(), 0.95),
        pct(times, 1.0)
    );
    for (label, f) in [
        (
            "nsfw",
            (|v: &ref_filters::Verdict| v.nsfw) as fn(&ref_filters::Verdict) -> f32,
        ),
        ("blank", |v| v.blank),
        ("text", |v| v.text),
        ("smoothness", |v| v.smoothness),
        ("difficulty", |v| v.difficulty.score),
    ] {
        let xs: Vec<f64> = rows.iter().map(|(_, v)| f(v) as f64).collect();
        eprintln!(
            "{label:>11}: min {:.4}  p5 {:.4}  median {:.4}  p95 {:.4}  max {:.4}",
            pct(xs.clone(), 0.0),
            pct(xs.clone(), 0.05),
            pct(xs.clone(), 0.5),
            pct(xs.clone(), 0.95),
            pct(xs, 1.0)
        );
    }
    let mut reasons = std::collections::BTreeMap::<&str, Vec<&str>>::new();
    for (name, v) in &rows {
        for r in &v.reasons {
            reasons.entry(r.as_str()).or_default().push(name);
        }
    }
    for (r, names) in &reasons {
        if names.len() > 20 {
            eprintln!("{r}: {}", names.len());
        } else {
            eprintln!("{r}: {} {:?}", names.len(), names);
        }
    }
    // Closest pairs by pHash.
    let mut pairs = Vec::new();
    for i in 0..rows.len() {
        for j in i + 1..rows.len() {
            pairs.push((hamming(rows[i].1.phash, rows[j].1.phash), i, j));
        }
    }
    pairs.sort();
    for &(d, i, j) in pairs.iter().take(5) {
        eprintln!("phash {d:>2}: {} ~ {}", rows[i].0, rows[j].0);
    }
}
