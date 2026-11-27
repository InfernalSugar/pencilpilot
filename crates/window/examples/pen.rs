//! The pen check: a window that darkens with pen pressure and prints what the pen reports
//! (tool, pressure, tilt, buttons) and how many readings arrive a second.
//!
//!   cargo run --release -p pencilpilot-window --example pen
//!
//! It opens on your screen. Draw on it light, then hard; tilt the pen; try the eraser end and
//! the side buttons. Close it when done.

use pencilpilot_window::{InputEvent, PenPhase};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gpu = pencilpilot_gpu::Gpu::new()?;
    let mut win = pencilpilot_window::Window::new(&gpu, "PencilPilot pen check")?;
    println!(
        "Draw on the window: light, then hard. Tilt the pen, flip to the eraser, press the side buttons."
    );
    let mut pressure = 0.0f32;
    let (mut samples, mut max_p, mut t0) = (0u32, 0.0f32, std::time::Instant::now());
    while win.is_open() {
        if !win.wants_frame() {
            win.wait()?;
        }
        for ev in win.take_input() {
            match ev {
                InputEvent::Pen(p) => {
                    samples += 1;
                    pressure = p.sample.pressure;
                    max_p = max_p.max(pressure);
                    if matches!(
                        p.phase,
                        PenPhase::Down | PenPhase::Up | PenPhase::Enter | PenPhase::Leave
                    ) {
                        println!(
                            "{:?} {:?} at ({:.0}, {:.0}) pressure {:.2} tilt ({:.0}°, {:.0}°) buttons {:#b}",
                            p.phase,
                            p.tool,
                            p.sample.x,
                            p.sample.y,
                            p.sample.pressure,
                            p.sample.tilt_x,
                            p.sample.tilt_y,
                            p.buttons
                        );
                    }
                }
                InputEvent::Key(k) if k.pressed => println!("key {} {:?}", k.name, k.text),
                _ => {}
            }
        }
        if win.wants_frame() {
            let img = win.acquire(&gpu)?;
            let v = 1.0 - 0.85 * pressure;
            gpu.clear(img, [v, v, v * 0.97, 1.0])?;
            win.present()?;
        }
        win.take_presented();
        if t0.elapsed().as_secs_f64() >= 1.0 {
            if samples > 0 {
                println!("  {samples} pen readings a second, highest pressure {max_p:.2}");
            }
            (samples, max_p, t0) = (0, 0.0, std::time::Instant::now());
        }
    }
    Ok(())
}
