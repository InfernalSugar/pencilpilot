//! Zero-shot match pictures against prompts with the CLIP sorter model.
//!
//! ```sh
//! cargo run --release -p pencilpilot-ref-filters --example clip_probe -- \
//!     crates/ref-filters/models/clip-vit-b32 pic1.jpg pic2.jpg -- "a photo of a cat" "a cartoon dog"
//! ```

use std::path::Path;
use std::time::Instant;

use ref_filters::clip::{ClipText, ClipVision};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let t = Instant::now();
    let dir = Path::new(&args[0]);
    let (text, vision) = (
        ClipText::load(dir).expect("load CLIP text"),
        ClipVision::load(dir).expect("load CLIP vision"),
    );
    eprintln!("loaded in {:?}", t.elapsed());
    let prompts = &args[(split + 1).min(args.len())..];
    let texts: Vec<Vec<f32>> = prompts
        .iter()
        .map(|p| text.embed(p).expect("text"))
        .collect();
    for f in &args[1..split] {
        let img = ref_filters::decode(&std::fs::read(f).expect("read")).expect("decode");
        let t = Instant::now();
        let e = vision.embed(&img).expect("image");
        let ms = t.elapsed().as_millis();
        let logits: Vec<f32> = texts
            .iter()
            .map(|t| 100.0 * t.iter().zip(&e).map(|(a, b)| a * b).sum::<f32>())
            .collect();
        let m = logits.iter().cloned().fold(f32::MIN, f32::max);
        let z: f32 = logits.iter().map(|l| (l - m).exp()).sum();
        println!("{f} ({ms} ms)");
        for (p, l) in prompts.iter().zip(&logits) {
            println!("  {:5.3}  {l:6.2}  {p}", (l - m).exp() / z);
        }
    }
}
