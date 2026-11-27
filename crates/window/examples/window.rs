//! A window that changes colour every frame and prints its frame rate each second:
//! `cargo run --release -p pencilpilot-window --example window` (opens on your screen).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gpu = pencilpilot_gpu::Gpu::new()?;
    let mut win = pencilpilot_window::Window::new(&gpu, "PencilPilot")?;
    println!("{}", gpu.info.name);
    let (mut n, mut shown, mut t0) = (0u32, 0usize, std::time::Instant::now());
    while win.is_open() {
        if !win.wants_frame() {
            win.wait()?;
        }
        if win.wants_frame() {
            let t = n as f32 / 60.0;
            let img = win.acquire(&gpu)?;
            gpu.clear(
                img,
                [0.5 + 0.5 * t.sin(), 0.5 + 0.5 * (t * 1.3).cos(), 0.6, 1.0],
            )?;
            win.present()?;
            n += 1;
        }
        shown += win.take_presented().len();
        if t0.elapsed().as_secs_f64() >= 1.0 {
            let hz = win.refresh_ns().map_or(0.0, |r| 1e9 / r as f64);
            println!(
                "{shown} fps on a {hz:.1} Hz monitor, {:?} px at ×{}",
                win.size(),
                win.scale()
            );
            (shown, t0) = (0, std::time::Instant::now());
        }
    }
    Ok(())
}
