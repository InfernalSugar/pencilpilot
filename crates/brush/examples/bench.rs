//! The engine benchmark, natively: the same workload as the app's /dev/bench page
//! (which runs it as WebAssembly), so the two can be compared.
//!
//! ```sh
//! cargo run --release -p pencilpilot-brush --example bench
//! ```
//!
//! 1. **Painting:** three brushes (pencil, airbrush, liner) × 60 strokes × 120 samples
//!    on a 1920×1080 canvas, flushing every 4 samples (a 240 Hz pen, 60 Hz screen).
//! 2. **Compositing:** 10 painted layers; the bottom one's visibility is toggled and
//!    the whole canvas recomposited, 20 times.

use std::time::Instant;

use brush::{Painter, set_brush_rgb};
use hokusai::BrushSetting;

const W: u32 = 1920;
const H: u32 = 1080;
const BRUSHES: [(&str, f32); 3] = [
    ("deevad-2b-pencil", 1.0),
    ("deevad-airbrush", 3.0),
    ("deevad-liner", 2.0),
];

fn load(id: &str, scale: f32) -> hokusai::Brush {
    let path = format!(
        "{}/../../apps/web/static/brushes/{id}.myb",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut b = hokusai::myb::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let r = b.get(BrushSetting::Radius).base_value;
    b.get_mut(BrushSetting::Radius).base_value = r + scale.log2();
    set_brush_rgb(&mut b, 22, 22, 26);
    b
}

/// One stroke of the shared workload.
fn stroke(p: &mut Painter, b: &hokusai::Brush, s: usize, flush: bool) -> usize {
    let y0 = 100.0 + ((s * 13) % 880) as f32;
    p.begin_stroke();
    for i in 0..120 {
        let fi = i as f32;
        let x = 100.0 + fi * 14.0;
        let y = y0 + 40.0 * (fi / 9.0 + s as f32).sin();
        let pr = 0.3 + 0.6 * (fi / 20.0).sin().powi(2);
        p.stroke_to(b, x, y, pr, 0.0, 0.0, 0.008);
        if flush && i % 4 == 3 {
            p.flush();
        }
    }
    p.end_stroke(b);
    if flush {
        p.flush();
    }
    120
}

fn main() {
    // painting
    let mut p = Painter::new(W, H);
    p.flush();
    let mut samples = 0;
    let mut per_brush = Vec::new();
    let t = Instant::now();
    for (id, scale) in BRUSHES {
        let b = load(id, scale);
        let tb = Instant::now();
        let mut n = 0;
        for s in 0..60 {
            n += stroke(&mut p, &b, s, true);
        }
        per_brush.push(format!(
            "\"{id}\":{:.4}",
            tb.elapsed().as_secs_f64() * 1000.0 / n as f64
        ));
        samples += n;
    }
    let paint_ms = t.elapsed().as_secs_f64() * 1000.0;

    // compositing
    let mut p = Painter::new(W, H);
    let air = load("deevad-airbrush", 3.0);
    let mut bottom = 0;
    for l in 0..10 {
        if l > 0 {
            p.add_layer("");
        } else {
            bottom = p.active_layer();
        }
        for s in 0..20 {
            stroke(&mut p, &air, s * 7 + l, false);
        }
    }
    p.flush();
    let t = Instant::now();
    for i in 0..20 {
        let mut props = p
            .layers()
            .iter()
            .find(|l| l.id == bottom)
            .unwrap()
            .props
            .clone();
        props.visible = i % 2 == 1;
        p.set_layer_props(bottom, props, false);
        p.flush();
    }
    let comp_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;

    println!(
        "{{\"where\":\"native\",\"paintMs\":{paint_ms:.1},\"samples\":{samples},\"msPerSample\":{:.4},\"perBrush\":{{{}}},\"compositeMs\":{comp_ms:.1}}}",
        paint_ms / samples as f64,
        per_brush.join(",")
    );
}
