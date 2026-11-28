//! The app's shell: header, tool column, the canvas in its card, the side panel and the
//! bottom strip, laid out as the design's FullApp artboards, in a window or off-screen.
//!
//! Each piece lives in its own file and has one job: draw itself in its region from the
//! [`State`], change the state, and push [`crate::state::Action`]s.

pub mod canvas;
pub mod dialog;
pub mod engine;
pub mod export_ui;
pub mod header;
pub mod layout;
pub mod panel;
pub mod strips;
pub mod tools;
pub mod view;

use pencilpilot_gpu::{Canvas, Compositor, DmabufImage, Gpu, Renderer2d};
use pencilpilot_ui::ui::{Frame, Rect, Ui};

use crate::state::State;

/// One pass of the whole shell. Returns the quad index the canvas goes in front of, and the
/// canvas's rectangle (logical pixels) inside its card.
pub fn build(f: &mut Frame, s: &mut State, width: f32, height: f32) -> (usize, Rect) {
    let r = layout::regions(width, height, s.workspace);
    header::build(f, r.header, s);
    if let Some(tr) = r.tools {
        tools::build(f, tr, s);
    }
    // the canvas's card; the drawing goes in front of it, the rest in front of the drawing
    let canvas = canvas::card(f, r.canvas, s);
    let mark = f.mark();
    canvas::overlays(f, r.canvas, s);
    panel::build(f, r.panel, s);
    strips::build(f, r.strip, s);
    export_ui::build(f, s);
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
    let mut e = engine::Engine::new(s.view.doc_w, s.view.doc_h);
    // the engine sets up the layers, drawings and thumbnails on its first tick
    e.tick(s, std::time::Instant::now());
    ui.pass(&mut r, w, h, |f| {
        (mark, canvas_rect) = build(f, s, w, h);
    });
    // settled: what every piece shows after the first pass's changes
    ui.pass(&mut r, w, h, |f| {
        (mark, canvas_rect) = build(f, s, w, h);
    });
    let quads = ui.shown().to_vec();
    let (fw, fh) = ((w * scale).round() as u32, (h * scale).round() as u32);
    let frame = DmabufImage::new(&gpu, fw, fh, &[])?;
    // the document, as the engine shows it
    let mut canvas = Canvas::new(&gpu, s.view.doc_w, s.view.doc_h)?;
    match e.picture(s) {
        Some((rgba, _)) => canvas.upload_all(rgba),
        None => canvas.upload_all(e.painter.rgba()),
    }
    let _ = (mark, canvas_rect);
    r.draw_layered(&gpu, &frame, ui.theme.paper, &quads, Some(&mut canvas))?;
    let px = gpu.read_back(&frame)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    save_png(out, &rgba, fw, fh)
}

/// Straight RGBA8 as a PNG file.
fn save_png(path: &str, rgba: &[u8], w: u32, h: u32) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// The shell in a window. With `quit_after`, it closes after that many seconds; with
/// `stats`, it saves how it ran there as JSON (startup to the first frame shown, frames
/// presented): for checks on a headless display.
///
/// Pen and mouse strokes that start on the canvas card paint (with a painting tool, and no
/// menu open); everything else works the interface. A frame is drawn only when the
/// interface or the drawing changed.
/// The pixels being moved, from the engine: where they are (`s.moving`), and their picture on
/// the GPU (made once a lift). Returns whether where they are changed.
fn sync_moving(
    gpu: &Gpu,
    e: &engine::Engine,
    s: &mut State,
    tex: &mut Option<(Canvas, u64)>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let now = e.float.as_ref().map(|f| crate::shell::view::Moving {
        at: f.at,
        w: f.w,
        h: f.h,
        m: f.m,
    });
    let moved = now != s.moving;
    s.moving = now;
    match (&e.float, &*tex) {
        (Some(f), Some((_, lift))) if *lift == f.lift => {}
        (Some(f), _) => {
            let mut c = Canvas::new(gpu, f.w.max(1), f.h.max(1))?;
            c.upload_all(&f.rgba);
            *tex = Some((c, f.lift));
        }
        (None, Some(_)) => *tex = None,
        (None, None) => {}
    }
    Ok(moved)
}

/// The layers' compositor on the GPU for a `w` × `h` document (none if the GPU can't make
/// one: the CPU composites then). It may take up to `PENCILPILOT_GPU_LAYERS_MB` of GPU memory,
/// its tiles and buffers together (920 MiB if not set: the whole app stays under 1 GiB).
fn compositor(gpu: &Gpu, w: u32, h: u32) -> Option<Compositor> {
    let mb: u64 = std::env::var("PENCILPILOT_GPU_LAYERS_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(920);
    match Compositor::new(gpu, w, h, brush::srgb_table(), mb << 20) {
        Ok(c) => Some(c),
        Err(err) => {
            eprintln!("pencilpilot: layers composited on the CPU ({err})");
            None
        }
    }
}

/// The engine's picture to the GPU: composited there (`comp`) where it can be, otherwise what
/// changed in the painter's picture goes up; or all of it in a new GPU picture when the
/// document's size changed (a project opened, a new one; the view is fitted again). Returns
/// (something went up or is to be composited, the picture was remade).
fn upload_picture(
    gpu: &Gpu,
    e: &mut engine::Engine,
    s: &mut State,
    canvas: &mut Option<Canvas>,
    comp: &mut Option<Compositor>,
    card: Rect,
) -> Result<(bool, bool), Box<dyn std::error::Error>> {
    let Some(c) = canvas else {
        return Ok((false, false));
    };
    let (pw, ph) = (e.painter.width(), e.painter.height());
    if (c.width, c.height) != (pw, ph) {
        (s.view.doc_w, s.view.doc_h) = (pw, ph);
        s.view.reset(card);
        let mut c = Canvas::new(gpu, pw, ph)?;
        *comp = compositor(gpu, pw, ph);
        let on_gpu = matches!(
            comp.as_mut().map(|k| e.picture_gpu(s, gpu, k)),
            Some(engine::frames::gpu::Pic::Gpu(_))
        );
        if !on_gpu {
            e.painter.flush();
            match e.picture(s) {
                // (a whole picture of the new size, or the painter's when nothing's new)
                Some((rgba, _)) if rgba.len() == (pw * ph * 4) as usize => c.upload_all(rgba),
                _ => c.upload_all(e.painter.rgba()),
            }
        }
        *canvas = Some(c);
        return Ok((true, true));
    }
    let pic = match comp.as_mut() {
        Some(k) => e.picture_gpu(s, gpu, k),
        None => engine::frames::gpu::Pic::Cpu(e.picture(s)),
    };
    match pic {
        engine::frames::gpu::Pic::Gpu(changed) => Ok((changed, false)),
        engine::frames::gpu::Pic::Cpu(Some((rgba, [x, y, w, h]))) => {
            c.upload(rgba, x, y, w, h);
            Ok((true, false))
        }
        engine::frames::gpu::Pic::Cpu(None) => Ok((false, false)),
    }
}

/// What a held mouse button or pen is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    None,
    /// the engine's tool (painting, selecting…)
    Engine,
    /// moving the view: where the pointer was
    Pan(f32, f32),
    /// turning the view: the pointer's angle about the card's centre
    Rotate(f32),
}

/// Whether a press on the canvas moves the view rather than using the tool.
fn moves_view(s: &State, space: bool) -> bool {
    space || !engine::Engine::takes_canvas_input(s)
}

/// How to run the window (for checks: see `main.rs`).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// close after this many seconds
    pub quit_after: Option<f64>,
    /// save how it ran here, as JSON
    pub stats: Option<String>,
    /// save the document here, as PNG, on closing
    pub save_doc: Option<String>,
    /// start in this workspace
    pub workspace: Option<crate::state::Workspace>,
    /// open this project once started
    pub open: Option<String>,
}

/// A frame drawn, for `--stats`.
struct FrameRec {
    /// `present`'s number
    frame: u64,
    /// the oldest input it shows: the event's own time (compositor ms) and when it came (ns)
    input: Option<(u32, u64)>,
    /// how many input events since the frame before
    events: u32,
    /// when it was handed over, ns
    drawn_ns: u64,
    /// the timeline frame it showed
    sheet_frame: u32,
    /// how long it took to make, from when it was meant to start to handing it over, ns;
    /// of that, the GPU's part (a buffer to draw in, drawing), ns
    work_ns: u64,
    gpu_ns: u64,
    /// of the rest: waking after its time, and the interface's pass, ns
    wake_ns: u64,
    pass_ns: u64,
    /// when the compositor asked for it, from when it was meant to start, ns (negative:
    /// before)
    asked_ns: i64,
    /// the document it showed: its size, and which one (`State::doc_id`)
    doc: (u32, u32),
    doc_gen: u32,
    /// an export running then: frames done, of how many
    export: Option<(u32, u32)>,
    /// when it reached the screen, ns
    shown_ns: Option<u64>,
    /// drawn as soon as it could be (not late, for the pen): how long such frames take to
    /// reach the screen once handed over is what playback leads by
    at_once: bool,
    /// playback: when this frame was due on screen (its place on the cut's clock), ns
    due_ns: Option<u64>,
    /// playback: how long it was held back once made, until due (not in work or GPU), ns
    held_ns: u64,
}

pub fn run_window(opts: RunOptions) -> Result<(), Box<dyn std::error::Error>> {
    let (quit_after, stats) = (opts.quit_after, opts.stats.as_deref());
    use pencilpilot_window::{InputEvent, PenPhase, PointerEvent, Window};

    use crate::state::Action;
    let started = std::time::Instant::now();
    let mut first_shown: Option<f64> = None;
    let (mut presented, mut drawn) = (0u64, 0u64);
    let gpu = Gpu::new()?;
    let mut win = Window::new(&gpu, "PencilPilot")?;
    // the close button asks (unsaved changes): the app closes when it's ready
    win.ask_before_closing();
    let mut r = Renderer2d::new(&gpu)?;
    let mut ui = Ui::new(win.scale() as f32);
    let mut s = State::demo();
    s.project.startup = opts.open.as_ref().map(std::path::PathBuf::from);
    if let Some(w) = opts.workspace {
        s.workspace = w;
    }
    let mut canvas: Option<Canvas> = None;
    // how long the last frames playback moved on took to make, ns
    let mut play_work: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    // how long the last frames drawn at once took from being handed over to being shown, ns
    let mut shown_delay: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    // the timeline frame last handed over; when input last came
    let mut last_sheet_frame = u32::MAX;
    let mut last_input = std::time::Instant::now();
    // the layers composited on the GPU (made with the canvas)
    let mut comp: Option<Compositor> = None;
    let mut recs: Vec<FrameRec> = Vec::new();
    // the document's GPU picture was remade (another size) since the last frame
    let mut resized_now = false;
    // what the input since the last frame did: moved the view / reached the engine
    let (mut view_moved, mut drew) = (false, false);
    let mut pacer =
        pencilpilot_window::pace::Pacer::new(std::env::var_os("PP_DRAW_AT_ONCE").is_some());
    let mut plan: Option<pencilpilot_window::pace::Plan> = None;
    // the moving pixels' picture, and which lift it holds
    let mut float_tex: Option<(Canvas, u64)> = None;
    let (mut oldest_input, mut events): (Option<(u32, u64)>, u32) = (None, 0);
    let mut eng: Option<engine::Engine> = None;
    let mut mark = 0;
    let mut canvas_rect = Rect::default();
    // the interface may look different (input came) / the drawing changed
    let (mut dirty, mut canvas_dirty) = (true, false);
    // what a held button or pen is doing; Space and Shift held
    let mut drag = Drag::None;
    let (mut space, mut shift, mut alt) = (false, false, false);
    let deadline = quit_after.map(|s| started + std::time::Duration::from_secs_f64(s));
    while win.is_open() && deadline.is_none_or(|d| std::time::Instant::now() < d) {
        // playback goes by when frames are shown: a frame started now shows after about
        // `show_delay`; half a refresh more picks the refresh nearest each frame's time.
        // While playing, frames are made a little ahead (as long as the slowest recent one
        // took) and handed over when due (`hold`), so how long one takes doesn't move it.
        // (playing, frames are drawn at once and handed over when due: a frame handed over
        // at a moment shows at the first refresh after the compositor's own delay, on
        // average that delay and half a refresh later, which is what's measured; handed
        // over that long before its time it shows on the refresh nearest its time)
        let lead_ns = match (s.playing, shown_delay.len()) {
            (true, n) if n > 0 => shown_delay.iter().sum::<u64>() / n as u64,
            // (nothing measured yet: half a refresh, the delay with no compositor latency)
            (true, _) => pacer.refresh_ns() / 2,
            (false, _) => {
                pacer.show_delay_ns(pencilpilot_window::clock::now_ns()) + pacer.refresh_ns() / 2
            }
        };
        let lead_shown = std::time::Duration::from_nanos(lead_ns);
        let ahead = if s.playing {
            // (the slowest recent one and half again: a busy GPU makes some slower; at most
            // 20 ms, half a frame of a 24 fps cut)
            // (the 90th percentile of the recent ones: one hitch mustn't hold every frame
            // after it back by 20 ms)
            let mut w: Vec<u64> = play_work.iter().copied().collect();
            w.sort_unstable();
            let most = w.get(w.len() * 9 / 10).copied().unwrap_or(2_000_000);
            std::time::Duration::from_nanos((most * 3 / 2 + 500_000).min(20_000_000))
        } else {
            std::time::Duration::ZERO
        };
        let lead = lead_shown + ahead;
        if !win.wants_frame() || !(dirty || canvas_dirty) {
            // nothing to draw: the drawings of the frames around go up to the GPU meanwhile
            // (not while input comes in: scrubbing, drawing; playing, until the next frame)
            if let (Some(e), Some(k)) = (&mut eng, &mut comp)
                && last_input.elapsed() > std::time::Duration::from_millis(100)
            {
                let room = match e.next_tick_at(&s) {
                    Some(t) if s.playing => t
                        .checked_sub(lead)
                        .unwrap_or(t)
                        .saturating_duration_since(std::time::Instant::now())
                        .saturating_sub(std::time::Duration::from_millis(3)),
                    _ => std::time::Duration::from_millis(3),
                };
                if !room.is_zero() {
                    e.gpu_ahead(&s, &gpu, k, room.min(std::time::Duration::from_millis(8)));
                }
            }
            let tick = eng
                .as_ref()
                .and_then(|e| e.next_tick_at(&s))
                .map(|t| t.checked_sub(lead).unwrap_or(t));
            let next = [ui.next_pass_at(), deadline, tick]
                .into_iter()
                .flatten()
                .min();
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
        let cr = canvas_rect;
        // the input since the last read: to the engine (strokes), the view or the interface
        // (a macro: it's read again just before drawing, below)
        macro_rules! read_input {
            () => {
                for ev in win.take_input() {
                    dirty = true;
                    let t = match &ev {
                        InputEvent::Pointer(
                            PointerEvent::Move { time_ms, .. }
                            | PointerEvent::Button { time_ms, .. }
                            | PointerEvent::Scroll { time_ms, .. },
                        ) => Some(*time_ms),
                        InputEvent::Pen(p) => Some(p.time_ms),
                        InputEvent::Key(k) => Some(k.time_ms),
                        _ => None,
                    };
                    if let Some(t) = t {
                        last_input = std::time::Instant::now();
                        ui.active();
                        events += 1;
                        oldest_input.get_or_insert((t, pencilpilot_window::clock::now_ns()));
                    }
                    // a press here goes to the engine (its tool), not the interface
                    let engine_here = |x: f64, y: f64, s: &State, ui: &Ui| {
                        engine::Engine::takes_canvas_input(s)
                            && !ui.overlay_open()
                            && cr.contains(x as f32, y as f32)
                    };
                    // a press here moves the view (the Hand, Space held, the middle button, Review)
                    let view_here = |x: f64, y: f64, ui: &Ui| {
                        !ui.overlay_open() && cr.contains(x as f32, y as f32)
                    };
                    let sample =
                        |s: &State, x: f64, y: f64, pressure: f32, tilt: (f32, f32), ms: u32| {
                            let (dx, dy) = s.view.to_doc(cr, x as f32, y as f32);
                            engine::Sample {
                                x: dx,
                                y: dy,
                                pressure,
                                tilt,
                                ms,
                            }
                        };
                    match ev {
                        InputEvent::Pointer(PointerEvent::Move { x, y, time_ms }) => match drag {
                            Drag::Engine => {
                                if let Some(e) = &mut eng {
                                    let p = sample(&s, x, y, 0.5, (0.0, 0.0), time_ms);
                                    e.pointer(&mut s, engine::Phase::Move, p);
                                    drew = true;
                                }
                            }
                            Drag::Pan(lx, ly) => {
                                s.view.pan_by(x as f32 - lx, y as f32 - ly);
                                view_moved = true;
                                drag = Drag::Pan(x as f32, y as f32);
                            }
                            Drag::Rotate(a) => {
                                let c = (cr.x + cr.w / 2.0, cr.y + cr.h / 2.0);
                                let now = (y as f32 - c.1).atan2(x as f32 - c.0);
                                s.view.rotate_about(cr, c, now - a);
                                view_moved = true;
                                drag = Drag::Rotate(now);
                            }
                            Drag::None => ui.pointer_moved(x as f32, y as f32),
                        },
                        InputEvent::Pointer(PointerEvent::Enter { x, y }) => {
                            ui.pointer_moved(x as f32, y as f32)
                        }
                        InputEvent::Pointer(PointerEvent::Leave) => ui.pointer_left(),
                        InputEvent::Pointer(PointerEvent::Button {
                            button: button @ (0x110 | 0x112),
                            pressed,
                            x,
                            y,
                            time_ms,
                        }) => {
                            let middle = button == 0x112;
                            if pressed && view_here(x, y, &ui) && (middle || moves_view(&s, space))
                            {
                                drag = if shift && !middle {
                                    let c = (cr.x + cr.w / 2.0, cr.y + cr.h / 2.0);
                                    Drag::Rotate((y as f32 - c.1).atan2(x as f32 - c.0))
                                } else {
                                    Drag::Pan(x as f32, y as f32)
                                };
                            } else if pressed && !middle && engine_here(x, y, &s, &ui) {
                                if let Some(e) = &mut eng {
                                    let p = sample(&s, x, y, 0.5, (0.0, 0.0), time_ms);
                                    (s.tooling.shift, s.tooling.alt) = (shift, alt);
                                    e.pointer(&mut s, engine::Phase::Down, p);
                                    drew = true;
                                    drag = Drag::Engine;
                                }
                            } else if !pressed && drag != Drag::None {
                                if drag == Drag::Engine
                                    && let Some(e) = &mut eng
                                {
                                    let p = sample(&s, x, y, 0.5, (0.0, 0.0), time_ms);
                                    e.pointer(&mut s, engine::Phase::Up, p);
                                }
                                drag = Drag::None;
                            } else if !middle {
                                ui.pointer_moved(x as f32, y as f32);
                                ui.pointer_button(pressed);
                            }
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
                        InputEvent::Pointer(PointerEvent::Scroll {
                            dy, x, y, wheel, ..
                        }) => {
                            // over the drawing the wheel zooms (Ctrl too: a touchpad's pinch); the
                            // interface's scroll areas take it everywhere else
                            if view_here(x, y, &ui) {
                                let steps = if wheel { dy / 10.0 } else { dy / 40.0 };
                                let factor = 1.2f32.powf(-steps as f32);
                                s.view.zoom_at(cr, (x as f32, y as f32), factor);
                                view_moved = true;
                            } else {
                                ui.pointer_moved(x as f32, y as f32);
                                ui.scrolled(dy as f32);
                            }
                        }
                        InputEvent::Pen(p) => {
                            let (x, y) = (p.sample.x, p.sample.y);
                            let tilt = (p.sample.tilt_x / 90.0, p.sample.tilt_y / 90.0);
                            let down = p.phase == PenPhase::Down && p.buttons == 0;
                            if down && view_here(x, y, &ui) && moves_view(&s, space) {
                                drag = Drag::Pan(x as f32, y as f32);
                            } else if down && engine_here(x, y, &s, &ui) {
                                drag = Drag::Engine;
                            }
                            match (drag, p.phase) {
                                (
                                    Drag::Engine,
                                    PenPhase::Down
                                    | PenPhase::Move
                                    | PenPhase::Up
                                    | PenPhase::Leave,
                                ) => {
                                    if let Some(e) = &mut eng {
                                        let phase = match p.phase {
                                            PenPhase::Down => engine::Phase::Down,
                                            PenPhase::Move => engine::Phase::Move,
                                            _ => engine::Phase::Up,
                                        };
                                        let smp =
                                            sample(&s, x, y, p.sample.pressure, tilt, p.time_ms);
                                        if phase == engine::Phase::Down {
                                            (s.tooling.shift, s.tooling.alt) = (shift, alt);
                                        }
                                        e.pointer(&mut s, phase, smp);
                                        drew = true;
                                    }
                                    if matches!(p.phase, PenPhase::Up | PenPhase::Leave) {
                                        drag = Drag::None;
                                    }
                                }
                                (Drag::Pan(lx, ly), PenPhase::Move) => {
                                    s.view.pan_by(x as f32 - lx, y as f32 - ly);
                                    view_moved = true;
                                    drag = Drag::Pan(x as f32, y as f32);
                                }
                                (Drag::Pan(..), PenPhase::Up | PenPhase::Leave) => {
                                    drag = Drag::None
                                }
                                (Drag::Pan(..), PenPhase::Down) => {}
                                _ => {
                                    // the interface: the pen's side button right-clicks
                                    ui.pointer_moved(x as f32, y as f32);
                                    match p.phase {
                                        PenPhase::Down if p.buttons & 1 != 0 => {
                                            ui.pointer_context()
                                        }
                                        PenPhase::Down => ui.pointer_button(true),
                                        PenPhase::Up => ui.pointer_button(false),
                                        PenPhase::Leave => ui.pointer_left(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        InputEvent::Key(k) => {
                            if k.name == "space" && !ui.typing() {
                                space = k.pressed;
                            }
                            if k.name.starts_with("Shift") {
                                shift = k.pressed;
                            }
                            if k.name.starts_with("Alt") {
                                alt = k.pressed;
                            }
                            if !k.pressed {
                                continue;
                            }
                            if k.mods.ctrl && k.name.eq_ignore_ascii_case("z") {
                                s.actions.push(if k.mods.shift {
                                    Action::Redo
                                } else {
                                    Action::Undo
                                });
                            }
                            ui.key(pencilpilot_ui::ui::Key {
                                name: k.name.clone(),
                                text: k.text.clone(),
                                ctrl: k.mods.ctrl,
                                shift: k.mods.shift,
                            });
                        }
                        _ => {}
                    }
                }
            };
        }
        read_input!();
        // pixels being moved: the GPU shows them where they are now
        if let Some(e) = &eng {
            dirty |= sync_moving(&gpu, e, &mut s, &mut float_tex)?;
        }
        // time: playback
        if let Some(e) = &mut eng
            && e.tick(&mut s, std::time::Instant::now() + lead)
        {
            dirty = true;
        }
        // the picture's changes go up to the GPU
        if let Some(e) = &mut eng {
            let (up, re) = upload_picture(&gpu, e, &mut s, &mut canvas, &mut comp, canvas_rect)?;
            canvas_dirty |= up;
            resized_now |= re;
        }
        for p in win.take_presented() {
            pacer.shown(&p);
            if let Some(r) = recs.iter_mut().rev().find(|r| r.frame == p.frame) {
                r.shown_ns = Some(p.at_ns);
                // (the sound follows the frames as they reach the screen)
                if let Some(e) = &mut eng {
                    e.sound.shown(r.sheet_frame, p.at_ns);
                }
                let delay = p.at_ns.saturating_sub(r.drawn_ns);
                // (one that missed a refresh or two says nothing about the compositor's delay)
                if r.at_once && delay < 2 * pacer.refresh_ns().max(1) {
                    shown_delay.push_back(delay);
                    if shown_delay.len() > 64 {
                        shown_delay.pop_front();
                    }
                }
            }
            presented += 1;
            first_shown.get_or_insert(started.elapsed().as_secs_f64() * 1e3);
        }
        if !win.wants_frame() || !(dirty || canvas_dirty) {
            continue;
        }
        // drawn late: just before the refresh it's for, with the newest input
        let now = pencilpilot_window::clock::now_ns();
        if (view_moved && !drew) || s.playing {
            // only the view moved: drawn at once (drawing late is for the pen's newest
            // sample; for zooming it only risks a missed refresh); playing, the same (frames
            // are made ahead and handed over when due)
            plan = None;
        } else if plan.is_none_or(|p| now > p.target) {
            plan = pacer.plan(now);
        }
        if let Some(p) = plan
            && now < p.draw_at
        {
            win.wait_until(p.draw_at)?;
            continue;
        }
        // (from when it was meant to start: waking late under load counts too)
        let woke = pencilpilot_window::clock::now_ns();
        let asked = win.frame_asked_at();
        let work_from = plan.map_or(woke, |p| p.draw_at.min(woke));
        let (lw, lh) = win.logical_size();
        ui.scale = win.scale() as f32;
        let (w, h) = (lw as f32, lh as f32);
        // playing counts as busy: the system's fonts joining (every text laid out and drawn
        // again, ~70 ms at 4K × 20 layers) waits until it stops
        if s.playing {
            ui.active();
        }
        let pass_from = pencilpilot_window::clock::now_ns();
        let first = ui.pass(&mut r, w, h, |f| {
            (mark, canvas_rect) = build(f, &mut s, w, h);
        });
        let mut changed = first;
        let pass_ns = pencilpilot_window::clock::now_ns().saturating_sub(pass_from);
        dirty = false;
        // what the interface asked for
        if win.take_close_request() {
            s.actions.push(Action::File("Quit".into()));
        }
        let acted = !s.actions.is_empty();
        for a in std::mem::take(&mut s.actions) {
            let quit = a == Action::File("Quit".into());
            let handled = match &mut eng {
                Some(e) => e.apply(&mut s, &a),
                None => false,
            };
            if handled {
                canvas_dirty = true;
                dirty = true;
            } else if quit {
                // (nothing asks first: close)
                s.quit_now = true;
            }
        }
        if std::mem::take(&mut s.quit_now) {
            win.close();
            break;
        }
        if let Some(e) = &mut eng {
            let (up, re) = upload_picture(&gpu, e, &mut s, &mut canvas, &mut comp, canvas_rect)?;
            canvas_dirty |= up;
            resized_now |= re;
        }
        let mut undo_changed = false;
        if let Some(e) = &eng {
            let now = (e.painter.can_undo(), e.painter.can_redo());
            if now != (s.can_undo, s.can_redo) {
                (s.can_undo, s.can_redo) = now;
                undo_changed = true;
            }
        }
        // what the actions changed shows in this frame, not the next: the interface again
        // (a tool's box, Undo greyed…), and pixels just lifted
        if acted || undo_changed {
            if let Some(e) = &eng {
                sync_moving(&gpu, e, &mut s, &mut float_tex)?;
            }
            let again = ui.pass(&mut r, w, h, |f| {
                (mark, canvas_rect) = build(f, &mut s, w, h);
            });
            changed = again.or(changed);
            // (the actions this pass asked for wait for the next frame)
            dirty |= !s.actions.is_empty();
        }
        let (cw, ch) = (s.view.doc_w, s.view.doc_h);
        // the document, made once the card's size is known (the view fits it there)
        let new_canvas = canvas.is_none();
        if new_canvas {
            s.view.reset(canvas_rect);
            let mut e = engine::Engine::new(cw, ch);
            // set up (layers, drawings) before the first input reaches it; this may open a
            // project (the file named at start), so the picture takes the painter's size
            e.tick(&mut s, std::time::Instant::now());
            let (pw, ph) = (e.painter.width(), e.painter.height());
            if (pw, ph) != (cw, ch) {
                (s.view.doc_w, s.view.doc_h) = (pw, ph);
                s.view.reset(canvas_rect);
            }
            let mut c = Canvas::new(&gpu, pw, ph)?;
            c.upload_all(e.painter.rgba());
            canvas = Some(c);
            comp = compositor(&gpu, pw, ph);
            eng = Some(e);
        }
        // a document of another size (a project opened, a new one) whose picture hasn't
        // changed since (no upload went through the check above)
        if let Some(e) = &mut eng {
            let (up, re) = upload_picture(&gpu, e, &mut s, &mut canvas, &mut comp, canvas_rect)?;
            canvas_dirty |= up;
            resized_now |= re;
        }
        let resized = std::mem::take(&mut resized_now);
        if resized {
            // the interface (the view's quad) catches up next pass
            dirty = true;
        }
        if changed.is_none() && !new_canvas && !canvas_dirty && !resized {
            continue;
        }
        canvas_dirty = false;
        // a stroke's samples that came while this frame was being made go in it too (they'd
        // wait a whole refresh otherwise); anything else they did shows next frame
        if drag == Drag::Engine {
            win.poll()?;
            // (what it notes for pacing is for the next frame, and reset at present)
            #[allow(unused_assignments)]
            {
                read_input!();
            }
            if let Some(e) = &mut eng {
                let (_, re) = upload_picture(&gpu, e, &mut s, &mut canvas, &mut comp, canvas_rect)?;
                resized_now |= re;
            }
        }
        let gpu_from = pencilpilot_window::clock::now_ns();
        let img = win.acquire(&gpu)?;
        let on_gpu = eng.as_ref().is_some_and(|e| e.frames.gpu.on);
        r.draw_composited(
            &gpu,
            img,
            ui.theme.paper,
            ui.shown(),
            canvas.as_mut(),
            float_tex.as_mut().map(|(c, _)| c),
            comp.as_mut().filter(|_| on_gpu),
        )?;
        // playback: a new frame drawn at once is handed over when due (it was made ahead)
        let moved_on = s.playing && s.frame != last_sheet_frame;
        let at_once = plan.is_none();
        let mut due_ns = None;
        // how long the frame was held back once made (not part of making it)
        let mut held_ns = 0u64;
        if moved_on && plan.is_none() {
            let made = pencilpilot_window::clock::now_ns().saturating_sub(work_from);
            play_work.push_back(made);
            if play_work.len() > 48 {
                play_work.pop_front();
            }
            if let Some(due) = eng.as_ref().and_then(engine::frames::play_due) {
                let now = std::time::Instant::now();
                due_ns = Some(if due >= now {
                    pencilpilot_window::clock::now_ns() + (due - now).as_nanos() as u64
                } else {
                    pencilpilot_window::clock::now_ns()
                        .saturating_sub((now - due).as_nanos() as u64)
                });
                let hold = due.checked_sub(lead_shown).unwrap_or(due);
                let wait = hold.saturating_duration_since(std::time::Instant::now());
                if !wait.is_zero() && wait < std::time::Duration::from_millis(50) {
                    held_ns = wait.as_nanos() as u64;
                    std::thread::sleep(wait);
                }
            }
        }
        if !s.playing {
            play_work.clear();
        }
        last_sheet_frame = s.frame;
        let frame = win.present()?;
        (view_moved, drew) = (false, false);

        let work_ns = pencilpilot_window::clock::now_ns()
            .saturating_sub(work_from)
            .saturating_sub(held_ns);
        pacer.drawn(
            frame,
            plan.take(),
            // (only frames input asked for: the startup's say nothing about drawing)
            (events > 0).then_some(work_ns),
        );
        drawn += 1;
        recs.push(FrameRec {
            frame,
            input: oldest_input.take(),
            events: std::mem::take(&mut events),
            drawn_ns: pencilpilot_window::clock::now_ns(),
            sheet_frame: s.frame,
            work_ns,
            gpu_ns: pencilpilot_window::clock::now_ns()
                .saturating_sub(gpu_from)
                .saturating_sub(held_ns),
            held_ns,
            wake_ns: woke.saturating_sub(work_from),
            pass_ns,
            asked_ns: asked as i64 - work_from as i64,
            doc: (s.view.doc_w, s.view.doc_h),
            doc_gen: s.doc_id,
            export: s.export_progress,
            shown_ns: None,
            at_once,
            due_ns,
        });
    }
    if let (Some(path), Some(e)) = (&opts.save_doc, &mut eng) {
        // (the painter's own picture catches up with what the GPU composited)
        engine::frames::catch_up(e, &s);
        e.painter.flush();
        save_png(path, e.painter.rgba(), s.view.doc_w, s.view.doc_h)?;
    }
    if let Some(path) = stats {
        let v = s.view;
        let opt = |v: Option<u64>| v.map_or("null".into(), |v| v.to_string());
        let frames: Vec<String> = recs
            .iter()
            .map(|r| {
                format!(
                    "{{\"frame\": {}, \"inputMs\": {}, \"receivedNs\": {}, \"events\": {}, \"drawnNs\": {}, \"shownNs\": {}, \"sheetFrame\": {}, \"workNs\": {}, \"gpuNs\": {}, \"wakeNs\": {}, \"passNs\": {}, \"askedNs\": {}, \"doc\": [{}, {}], \"docGen\": {}, \"export\": {}, \"dueNs\": {}, \"heldNs\": {}}}",
                    r.frame,
                    opt(r.input.map(|i| i.0 as u64)),
                    opt(r.input.map(|i| i.1)),
                    r.events,
                    r.drawn_ns,
                    opt(r.shown_ns),
                    r.sheet_frame,
                    r.work_ns,
                    r.gpu_ns,
                    r.wake_ns,
                    r.pass_ns,
                    r.asked_ns,
                    r.doc.0,
                    r.doc.1,
                    r.doc_gen,
                    r.export
                        .map_or("null".to_string(), |(d, n)| format!("[{d}, {n}]")),
                    opt(r.due_ns),
                    r.held_ns
                )
            })
            .collect();
        let layers: Vec<String> = s
            .layers
            .iter()
            .map(|l| {
                format!(
                    "{{\"name\": {:?}, \"visible\": {}, \"locked\": {}}}",
                    l.name, l.visible, l.locked
                )
            })
            .collect();
        let c = canvas_rect;
        // what the sound played (the silent output's buffers, in checks)
        let sound = eng.as_mut().map_or_else(
            || "null".to_string(),
            |e| e.sound.stats_json(s.sound.cut_start, s.fps),
        );
        let gpu_layers = comp.as_ref().map_or("null".to_string(), |k| {
            format!(
                "{{\"on\": {}, \"gpuMiB\": {:.1}, \"tiles\": {}, \"sources\": {}, \"uploads\": {}}}",
                eng.as_ref().is_some_and(|e| e.frames.gpu.on),
                k.gpu_bytes() as f64 / (1 << 20) as f64,
                k.tiles_held(),
                k.sources_held(),
                k.uploads
            )
        });
        let fonts_joined = ui.fonts_joined.map_or("null".to_string(), |t| {
            format!(
                "{:.1}",
                t.saturating_duration_since(started).as_secs_f64() * 1e3
            )
        });
        let json = format!(
            "{{\"seconds\": {:.3}, \"fontsJoinedMs\": {fonts_joined}, \"gpuLayers\": {gpu_layers}, \"startupToFirstFrameMs\": {}, \"framesDrawn\": {drawn}, \"framesPresented\": {presented}, \"drawingLate\": {}, \"marginMs\": {:.2}, \"framesLate\": {}, \"coolDowns\": {},\n \"view\": {{\"zoom\": {}, \"angle\": {}, \"flip\": {}, \"pan\": [{}, {}], \"docSize\": [{}, {}]}},\n \"canvasRect\": [{}, {}, {}, {}],\n \"state\": {{\"workspace\": {:?}, \"tool\": \"{:?}\", \"activeLayer\": {}, \"frame\": {}, \"playing\": {}, \"colour\": {:?}, \"layers\": [{}]}},\n \"sound\": {},\n \"frames\": [\n  {}\n ]}}\n",
            started.elapsed().as_secs_f64(),
            first_shown.map_or("null".into(), |v| format!("{v:.1}")),
            pacer.drawing_late(),
            pacer.margin_ms(),
            pacer.late,
            pacer.cool_downs,
            v.zoom,
            v.angle,
            v.flip,
            v.pan.0,
            v.pan.1,
            v.doc_w,
            v.doc_h,
            c.x,
            c.y,
            c.w,
            c.h,
            s.workspace.name(),
            s.tool,
            s.active_layer,
            s.frame,
            s.playing,
            s.colour,
            layers.join(", "),
            sound,
            frames.join(",\n  ")
        );
        std::fs::write(path, json)?;
    }
    Ok(())
}
