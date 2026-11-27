//! The app's shell: header, tool column, the canvas in its card, the side panel and the
//! bottom strip, laid out as the design's FullApp artboards, in a window or off-screen.
//!
//! Each piece lives in its own file and has one job: draw itself in its region from the
//! [`State`], change the state, and push [`crate::state::Action`]s.

pub mod canvas;
pub mod engine;
pub mod header;
pub mod layout;
pub mod panel;
pub mod strips;
pub mod tools;
pub mod view;

use pencilpilot_gpu::{Canvas, DmabufImage, Gpu, Renderer2d};
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
    /// when it reached the screen, ns
    shown_ns: Option<u64>,
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
    let mut r = Renderer2d::new(&gpu)?;
    let mut ui = Ui::new(win.scale() as f32);
    let mut s = State::demo();
    if let Some(w) = opts.workspace {
        s.workspace = w;
    }
    let mut canvas: Option<Canvas> = None;
    let mut recs: Vec<FrameRec> = Vec::new();
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
        // `show_delay`; half a refresh more picks the refresh nearest each frame's time
        let lead = std::time::Duration::from_nanos(
            pacer.show_delay_ns(pencilpilot_window::clock::now_ns()) + pacer.refresh_ns() / 2,
        );
        if !win.wants_frame() || !(dirty || canvas_dirty) {
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
            let view_here =
                |x: f64, y: f64, ui: &Ui| !ui.overlay_open() && cr.contains(x as f32, y as f32);
            let sample = |s: &State, x: f64, y: f64, pressure: f32, tilt: (f32, f32), ms: u32| {
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
                        }
                    }
                    Drag::Pan(lx, ly) => {
                        s.view.pan_by(x as f32 - lx, y as f32 - ly);
                        drag = Drag::Pan(x as f32, y as f32);
                    }
                    Drag::Rotate(a) => {
                        let c = (cr.x + cr.w / 2.0, cr.y + cr.h / 2.0);
                        let now = (y as f32 - c.1).atan2(x as f32 - c.0);
                        s.view.rotate_about(cr, c, now - a);
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
                    if pressed && view_here(x, y, &ui) && (middle || moves_view(&s, space)) {
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
                            PenPhase::Down | PenPhase::Move | PenPhase::Up | PenPhase::Leave,
                        ) => {
                            if let Some(e) = &mut eng {
                                let phase = match p.phase {
                                    PenPhase::Down => engine::Phase::Down,
                                    PenPhase::Move => engine::Phase::Move,
                                    _ => engine::Phase::Up,
                                };
                                let smp = sample(&s, x, y, p.sample.pressure, tilt, p.time_ms);
                                if phase == engine::Phase::Down {
                                    (s.tooling.shift, s.tooling.alt) = (shift, alt);
                                }
                                e.pointer(&mut s, phase, smp);
                            }
                            if matches!(p.phase, PenPhase::Up | PenPhase::Leave) {
                                drag = Drag::None;
                            }
                        }
                        (Drag::Pan(lx, ly), PenPhase::Move) => {
                            s.view.pan_by(x as f32 - lx, y as f32 - ly);
                            drag = Drag::Pan(x as f32, y as f32);
                        }
                        (Drag::Pan(..), PenPhase::Up | PenPhase::Leave) => drag = Drag::None,
                        (Drag::Pan(..), PenPhase::Down) => {}
                        _ => {
                            // the interface: the pen's side button right-clicks
                            ui.pointer_moved(x as f32, y as f32);
                            match p.phase {
                                PenPhase::Down if p.buttons & 1 != 0 => ui.pointer_context(),
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
        // pixels being moved: the GPU shows them where they are now
        if let Some(e) = &eng {
            let now = e.float.as_ref().map(|f| crate::shell::view::Moving {
                at: f.at,
                w: f.w,
                h: f.h,
                m: f.m,
            });
            if now != s.moving {
                s.moving = now;
                dirty = true;
            }
            match (&e.float, &float_tex) {
                (Some(f), Some((_, lift))) if *lift == f.lift => {}
                (Some(f), _) => {
                    let mut c = Canvas::new(&gpu, f.w.max(1), f.h.max(1))?;
                    c.upload_all(&f.rgba);
                    float_tex = Some((c, f.lift));
                }
                (None, Some(_)) => float_tex = None,
                (None, None) => {}
            }
        }
        // time: playback
        if let Some(e) = &mut eng
            && e.tick(&mut s, std::time::Instant::now() + lead)
        {
            dirty = true;
        }
        // the picture's changes go up to the GPU
        if let Some(e) = &mut eng
            && let Some(c) = &mut canvas
            && let Some((rgba, [x, y, w, h])) = e.picture(&s)
        {
            c.upload(rgba, x, y, w, h);
            canvas_dirty = true;
        }
        for p in win.take_presented() {
            pacer.shown(&p);
            if let Some(r) = recs.iter_mut().rev().find(|r| r.frame == p.frame) {
                r.shown_ns = Some(p.at_ns);
            }
            presented += 1;
            first_shown.get_or_insert(started.elapsed().as_secs_f64() * 1e3);
        }
        if !win.wants_frame() || !(dirty || canvas_dirty) {
            continue;
        }
        // drawn late: just before the refresh it's for, with the newest input
        let now = pencilpilot_window::clock::now_ns();
        if plan.is_none_or(|p| now > p.target) {
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
        let work_from = plan.map_or(woke, |p| p.draw_at.min(woke));
        let (lw, lh) = win.logical_size();
        ui.scale = win.scale() as f32;
        let (w, h) = (lw as f32, lh as f32);
        let pass_from = pencilpilot_window::clock::now_ns();
        let first = ui.pass(&mut r, w, h, |f| {
            (mark, canvas_rect) = build(f, &mut s, w, h);
        });
        let changed = first;
        let pass_ns = pencilpilot_window::clock::now_ns().saturating_sub(pass_from);
        dirty = false;
        // what the interface asked for
        for a in std::mem::take(&mut s.actions) {
            if let Some(e) = &mut eng
                && e.apply(&mut s, &a)
            {
                canvas_dirty = true;
                dirty = true;
            }
        }
        if let Some(e) = &mut eng
            && let Some(c) = &mut canvas
            && let Some((rgba, [x, y, w, h])) = e.picture(&s)
        {
            c.upload(rgba, x, y, w, h);
            canvas_dirty = true;
        }
        if let Some(e) = &eng {
            let now = (e.painter.can_undo(), e.painter.can_redo());
            if now != (s.can_undo, s.can_redo) {
                (s.can_undo, s.can_redo) = now;
                dirty = true; // the Undo and Redo buttons follow: another pass next frame
            }
        }
        let (cw, ch) = (s.view.doc_w, s.view.doc_h);
        // the document, made once the card's size is known (the view fits it there)
        let new_canvas = canvas.is_none();
        if new_canvas {
            s.view.reset(canvas_rect);
            let mut e = engine::Engine::new(cw, ch);
            // set up (layers, drawings) before the first input reaches it
            e.tick(&mut s, std::time::Instant::now());
            let mut c = Canvas::new(&gpu, cw, ch)?;
            c.upload_all(e.painter.rgba());
            canvas = Some(c);
            eng = Some(e);
        }
        if changed.is_none() && !new_canvas && !canvas_dirty {
            continue;
        }
        canvas_dirty = false;
        let gpu_from = pencilpilot_window::clock::now_ns();
        let img = win.acquire(&gpu)?;
        r.draw_canvases(
            &gpu,
            img,
            ui.theme.paper,
            ui.shown(),
            canvas.as_mut(),
            float_tex.as_mut().map(|(c, _)| c),
        )?;
        let frame = win.present()?;
        let work_ns = pencilpilot_window::clock::now_ns().saturating_sub(work_from);
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
            gpu_ns: pencilpilot_window::clock::now_ns().saturating_sub(gpu_from),
            wake_ns: woke.saturating_sub(work_from),
            pass_ns,
            shown_ns: None,
        });
    }
    if let (Some(path), Some(e)) = (&opts.save_doc, &eng) {
        save_png(path, e.painter.rgba(), s.view.doc_w, s.view.doc_h)?;
    }
    if let Some(path) = stats {
        let v = s.view;
        let opt = |v: Option<u64>| v.map_or("null".into(), |v| v.to_string());
        let frames: Vec<String> = recs
            .iter()
            .map(|r| {
                format!(
                    "{{\"frame\": {}, \"inputMs\": {}, \"receivedNs\": {}, \"events\": {}, \"drawnNs\": {}, \"shownNs\": {}, \"sheetFrame\": {}, \"workNs\": {}, \"gpuNs\": {}, \"wakeNs\": {}, \"passNs\": {}}}",
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
                    r.pass_ns
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
        let json = format!(
            "{{\"seconds\": {:.3}, \"startupToFirstFrameMs\": {}, \"framesDrawn\": {drawn}, \"framesPresented\": {presented}, \"drawingLate\": {}, \"marginMs\": {:.2}, \"framesLate\": {}, \"coolDowns\": {},\n \"view\": {{\"zoom\": {}, \"angle\": {}, \"flip\": {}, \"pan\": [{}, {}], \"docSize\": [{}, {}]}},\n \"canvasRect\": [{}, {}, {}, {}],\n \"state\": {{\"workspace\": {:?}, \"tool\": \"{:?}\", \"activeLayer\": {}, \"frame\": {}, \"playing\": {}, \"colour\": {:?}, \"layers\": [{}]}},\n \"frames\": [\n  {}\n ]}}\n",
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
            frames.join(",\n  ")
        );
        std::fs::write(path, json)?;
    }
    Ok(())
}
