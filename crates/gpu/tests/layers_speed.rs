//! How long the GPU takes to composite a 4K picture of 20 layers (for tuning the shader).
//! Ignored by default (needs a GPU):
//! `cargo test --release -p pencilpilot-gpu --test layers_speed -- --ignored --nocapture`

use pencilpilot_gpu::{Compositor, Gpu, GpuLayer, TilePx};

#[test]
#[ignore]
fn composite_4k_20_layers() {
    let Ok(gpu) = Gpu::new() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let (w, h) = (3840u32, 2160u32);
    let lut: Vec<f32> = (0..=32768)
        .map(|i| (i as f32 / 32768.0).powf(1.0 / 2.2))
        .collect();
    let mut c = Compositor::new(&gpu, w, h, &lut, 2 << 30).unwrap();
    let (tx, ty) = (w.div_ceil(64) as i32, h.div_ceil(64) as i32);
    let mut layers = Vec::new();
    let mut px: Box<TilePx> = Box::new([[[0u16; 4]; 64]; 64]);
    for l in 0..20u64 {
        for (y, row) in px.iter_mut().enumerate() {
            for (x, p) in row.iter_mut().enumerate() {
                let a = if l == 0 {
                    32768
                } else {
                    ((x * 512 + y * 7 + l as usize * 999) % 32768) as u16
                };
                *p = [a / 3, a / 2, a / 4, a];
            }
        }
        c.start(l);
        for ky in 0..ty {
            for kx in 0..tx {
                // the paper everywhere; the rest in about half the tiles
                if l == 0 || (kx + ky + l as i32) % 2 == 0 {
                    c.put_tile(&gpu, l, (kx, ky), Some(&px)).unwrap();
                }
            }
        }
        layers.push(GpuLayer {
            source: l,
            mask: None,
            opacity: 0.9,
            blend: (l % 7) as u32,
            visible: true,
            clip: false,
        });
    }
    c.set_frame(&gpu, &layers, &[]);
    c.composite_all(&gpu).unwrap();
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = std::time::Instant::now();
        c.composite_all(&gpu).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    eprintln!(
        "4K, 20 layers, every tile: {:.2} ms median, {:.2} fastest",
        times[10], times[0]
    );
}
