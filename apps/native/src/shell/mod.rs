//! The app's shell: header, tool column, the canvas in its card, the side panel and the
//! bottom strip, laid out as the design's FullApp artboards, in a window or off-screen.
//!
//! Each piece lives in its own file and has one job: draw itself in its region from the
//! [`State`], change the state, and push [`crate::state::Action`]s.

pub mod header;
pub mod layout;
pub mod panel;
pub mod strips;
pub mod tools;

use pencilpilot_gpu::{Canvas, DmabufImage, Gpu, Quad, Renderer2d};
use pencilpilot_ui::ui::{Frame, Rect, Ui};

use crate::state::State;

/// One pass of the whole shell. Returns the quad index the canvas goes in front of, and the
/// canvas's rectangle (logical pixels) inside its card.
pub fn build(f: &mut Frame, s: &mut State, width: f32, height: f32) -> (usize, Rect) {
    let r = layout::regions(width, height, s.workspace);
    let t = f.theme();
    header::build(f, r.header, s);
    if let Some(tr) = r.tools {
        tools::build(f, tr, s);
    }
    // the canvas's card; the drawing goes in front of it, the rest in front of the drawing
    f.put(Quad::rect(r.canvas.x, r.canvas.y, r.canvas.w, r.canvas.h, t.card).radius(24.0));
    let canvas = r.canvas.inset(12.0);
    let mark = f.mark();
    panel::build(f, r.panel, s);
    strips::build(f, r.strip, s);
    (mark, canvas)
}

/// The shell drawn off-screen at `width` × `height` (logical) and `scale`, as PNG: for
/// screenshots and checks, with no window anywhere.
pub fn render_png(
    s: &mut State,
    width: u32,
    height: u32,
    scale: f32,
    out: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let gpu = Gpu::new()?;
    let mut r = Renderer2d::new(&gpu)?;
    let mut ui = Ui::new(scale);
    ui.text.wait_for_fonts();
    let (w, h) = (width as f32, height as f32);
    let mut mark = 0;
    let mut canvas_rect = Rect::default();
    let quads = ui
        .pass(&mut r, w, h, |f| {
            (mark, canvas_rect) = build(f, s, w, h);
        })
        .unwrap_or_default();
    let (fw, fh) = ((w * scale).round() as u32, (h * scale).round() as u32);
    let frame = DmabufImage::new(&gpu, fw, fh, &[])?;
    // the drawing: paper for now (the engine's canvas plugs in here)
    let (cw, ch) = (
        (canvas_rect.w * scale).round().max(1.0) as u32,
        (canvas_rect.h * scale).round().max(1.0) as u32,
    );
    let mut canvas = Canvas::new(&gpu, cw, ch)?;
    let paper: Vec<u8> = (0..cw * ch).flat_map(|_| [255u8, 255, 255, 255]).collect();
    canvas.upload_all(&paper);
    let at = [
        (canvas_rect.x * scale).round() as i32,
        (canvas_rect.y * scale).round() as i32,
        cw as i32,
        ch as i32,
    ];
    r.draw_layered(
        &gpu,
        &frame,
        ui.theme.paper,
        &quads,
        mark,
        Some((&mut canvas, at)),
    )?;
    let px = gpu.read_back(&frame)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut enc = png::Encoder::new(file, fw, fh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// The shell in a window. With `quit_after`, it closes after that many seconds; with
/// `stats`, it saves how it ran there as JSON (startup to the first frame shown, frames
/// presented): for checks on a headless display.
pub fn run_window(
    quit_after: Option<f64>,
    stats: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use pencilpilot_window::{InputEvent, PointerEvent, Window};
    let started = std::time::Instant::now();
    let mut first_shown: Option<f64> = None;
    let mut presented = 0u64;
    let mut drawn = 0u64;
    let gpu = Gpu::new()?;
    let mut win = Window::new(&gpu, "PencilPilot")?;
    let mut r = Renderer2d::new(&gpu)?;
    let mut ui = Ui::new(win.scale() as f32);
    let mut s = State::demo();
    let mut canvas: Option<Canvas> = None;
    let mut mark = 0;
    let mut canvas_rect = Rect::default();
    let mut dirty = true;
    let deadline = quit_after.map(|s| started + std::time::Duration::from_secs_f64(s));
    while win.is_open() && deadline.is_none_or(|d| std::time::Instant::now() < d) {
        if !win.wants_frame() || !dirty {
            let next = match (ui.next_pass_at(), deadline) {
                (Some(a), Some(d)) => Some(a.min(d)),
                (a, d) => a.or(d),
            };
            match next {
                Some(at) => {
                    let ns = at
                        .saturating_duration_since(std::time::Instant::now())
                        .as_nanos() as u64;
                    win.wait_until(pencilpilot_window::clock::now_ns() + ns)?;
                    dirty = true;
                }
                None => win.wait()?,
            }
        }
        for ev in win.take_input() {
            dirty = true;
            match ev {
                InputEvent::Pointer(PointerEvent::Move { x, y, .. })
                | InputEvent::Pointer(PointerEvent::Enter { x, y }) => {
                    ui.pointer_moved(x as f32, y as f32)
                }
                InputEvent::Pointer(PointerEvent::Leave) => ui.pointer_left(),
                InputEvent::Pointer(PointerEvent::Button {
                    button: 0x110,
                    pressed,
                    x,
                    y,
                    ..
                }) => {
                    ui.pointer_moved(x as f32, y as f32);
                    ui.pointer_button(pressed);
                }
                InputEvent::Pointer(PointerEvent::Button {
                    button: 0x111,
                    pressed: true,
                    x,
                    y,
                    ..
                }) => {
                    ui.pointer_moved(x as f32, y as f32);
                    ui.pointer_context();
                }
                InputEvent::Pointer(PointerEvent::Scroll { dy, x, y, .. }) => {
                    ui.pointer_moved(x as f32, y as f32);
                    ui.scrolled(dy as f32);
                }
                InputEvent::Pen(p) => {
                    // the pen works the interface too (the canvas's drawing comes with the engine)
                    use pencilpilot_window::PenPhase;
                    ui.pointer_moved(p.sample.x as f32, p.sample.y as f32);
                    match p.phase {
                        PenPhase::Down => ui.pointer_button(true),
                        PenPhase::Up => ui.pointer_button(false),
                        PenPhase::Leave => ui.pointer_left(),
                        _ => {}
                    }
                    if p.buttons & 1 != 0 && p.phase == PenPhase::Down {
                        ui.pointer_context();
                    }
                }
                InputEvent::Key(k) if k.pressed => ui.key(pencilpilot_ui::ui::Key {
                    name: k.name.clone(),
                    text: k.text.clone(),
                    ctrl: k.mods.ctrl,
                    shift: k.mods.shift,
                }),
                _ => {}
            }
        }
        for _ in win.take_presented() {
            presented += 1;
            first_shown.get_or_insert(started.elapsed().as_secs_f64() * 1e3);
        }
        if !win.wants_frame() || !dirty {
            continue;
        }
        let (lw, lh) = win.logical_size();
        ui.scale = win.scale() as f32;
        let (w, h) = (lw as f32, lh as f32);
        let changed = ui.pass(&mut r, w, h, |f| {
            (mark, canvas_rect) = build(f, &mut s, w, h);
        });
        // (the app carries out the actions here, as the engine comes in)
        s.actions.clear();
        dirty = false;
        let size_now = win.size();
        let scale = ui.scale;
        let (cw, ch) = (
            (canvas_rect.w * scale).round().max(1.0) as u32,
            (canvas_rect.h * scale).round().max(1.0) as u32,
        );
        let new_canvas = canvas
            .as_ref()
            .is_none_or(|c| (c.width, c.height) != (cw, ch));
        if new_canvas {
            let mut c = Canvas::new(&gpu, cw, ch)?;
            c.upload_all(&vec![255u8; (cw * ch * 4) as usize]);
            canvas = Some(c);
        }
        if changed.is_none() && !new_canvas {
            continue;
        }
        let img = win.acquire(&gpu)?;
        let at = [
            (canvas_rect.x * scale).round() as i32,
            (canvas_rect.y * scale).round() as i32,
            cw as i32,
            ch as i32,
        ];
        let _ = size_now;
        r.draw_layered(
            &gpu,
            img,
            ui.theme.paper,
            ui.shown(),
            mark,
            canvas.as_mut().map(|c| (c, at)),
        )?;
        win.present()?;
        drawn += 1;
    }
    if let Some(path) = stats {
        let json = format!(
            "{{\"seconds\": {:.3}, \"startupToFirstFrameMs\": {}, \"framesDrawn\": {drawn}, \"framesPresented\": {presented}}}\n",
            started.elapsed().as_secs_f64(),
            first_shown.map_or("null".into(), |v| format!("{v:.1}"))
        );
        std::fs::write(path, json)?;
    }
    Ok(())
}
