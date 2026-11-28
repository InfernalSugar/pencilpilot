//! The drawing engine behind the canvas card: the brush engine (`crates/brush`) painting
//! with the brush, colour, size and tool the interface has picked.
//!
//! - this file: the engine, painting with the Brush and the Eraser, and handing out the
//!   rest: pointer input on the canvas for the other tools goes to [`tools`], what the
//!   interface asks for (actions) goes to [`tools`] then [`frames`];
//! - [`tools`]: Fill, Lasso, Select, Transform, Eyedropper, Shape, Text, the selection and
//!   canvas menus, the clipboard;
//! - [`frames`]: layers, drawings and their exposures, onion skin, playback, and the picture
//!   the canvas shows.
//!
//! Positions here are the document's pixels (the shell maps the screen through the view).

pub mod export;
pub mod files;
pub mod frames;
pub mod sound;
pub mod tools;

use brush::{Painter, set_brush_rgb};
use hokusai::BrushSetting;

use crate::state::{Action, State, Tool};

/// A transform's lifted pixels while they move: shown by the GPU over the document, so a
/// drag costs no redrawing at all; drawn into the layer once, on [`Engine::float_end`].
#[derive(Debug, Clone, PartialEq)]
pub struct Float {
    /// straight RGBA8, `w`×`h`
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
    /// where its top-left was lifted from (document pixels)
    pub at: (f32, f32),
    /// where it goes: document → document, `dest = (a·x + c·y + e, b·x + d·y + f)` as
    /// `[a, b, c, d, e, f]` (the painter's `transform_preview`'s matrix)
    pub m: [f32; 6],
    /// which lift this is (a new one each [`Engine::float_begin`]: the shell uploads `rgba`
    /// once per lift)
    pub lift: u64,
}

/// A brush file, parsed once (parsing takes milliseconds: not at every stroke's start).
fn parsed(src: &'static str) -> Option<hokusai::Brush> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<usize, hokusai::Brush>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    if let Some(b) = cache.get(&(src.as_ptr() as usize)) {
        return Some(b.clone());
    }
    let b = hokusai::myb::from_str(src).ok()?;
    cache.insert(src.as_ptr() as usize, b.clone());
    Some(b)
}

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Where a stroke started, and whether it has moved since.
#[derive(Debug, Clone, Copy)]
struct Tap {
    x: f32,
    y: f32,
    pressure: f32,
    tilt: (f32, f32),
    size: f32,
    moved: bool,
}

/// A pointer on the canvas, in document pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    /// 0–1 (a mouse: 0.5)
    pub pressure: f32,
    /// −1–1 each way
    pub tilt: (f32, f32),
    /// the input's time, ms
    pub ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

/// The brushes the interface lists, by name, and their files.
const BRUSHES: &[(&str, &str)] = &[
    (
        "2B pencil",
        include_str!("../../../../web/static/brushes/deevad-2b-pencil.myb"),
    ),
    (
        "Rough pencil",
        include_str!("../../../../web/static/brushes/pencilpilot-rough-pencil.myb"),
    ),
    (
        "Liner",
        include_str!("../../../../web/static/brushes/deevad-liner.myb"),
    ),
    (
        "Marker",
        include_str!("../../../../web/static/brushes/aotz-040-marker.myb"),
    ),
    (
        "Airbrush",
        include_str!("../../../../web/static/brushes/deevad-airbrush.myb"),
    ),
];

/// The Eraser tool's brush: solid, so it erases cleanly (a grainy brush only thins paint).
const ERASER: &str = include_str!("../../../../web/static/brushes/deevad-large-hard-eraser.myb");

pub struct Engine {
    pub painter: Painter,
    stroking: Option<hokusai::Brush>,
    last_ms: Option<u32>,
    /// how the stroke started (a tap paints a dot)
    tap: Option<Tap>,
    /// the other tools' state ([`tools`])
    pub tools: tools::Tools,
    /// layers, drawings, playback ([`frames`])
    pub frames: frames::Frames,
    /// pixels being moved, shown by the GPU ([`Engine::float_begin`])
    pub float: Option<Float>,
    /// the open project on disk ([`files`])
    pub files: files::Files,
    /// an export in progress ([`export`])
    pub exporter: export::Exporter,
    /// the scene's sound ([`sound`])
    pub sound: sound::Sounds,
    lifts: u64,
}

impl Engine {
    pub fn new(width: u32, height: u32) -> Engine {
        Engine {
            painter: Painter::new(width, height),
            stroking: None,
            last_ms: None,
            tap: None,
            tools: tools::Tools::default(),
            frames: frames::Frames::default(),
            float: None,
            lifts: 0,
            files: files::Files::default(),
            exporter: export::Exporter::default(),
            sound: sound::Sounds::default(),
        }
    }

    /// Lifts the selection (or the active layer's paint) to move it: the pixels move on the
    /// GPU from now on ([`Engine::float_move`]); the layer shows without them. Returns the
    /// lifted box (x0, y0, x1, y1, document pixels), empty when there's nothing to lift.
    pub fn lift(&mut self) -> Vec<f32> {
        let b = self.painter.transform_begin_lifted();
        if b.is_empty() || !self.float_from_painter(false) {
            return Vec::new();
        }
        b
    }

    /// After `painter.transform_begin()`: the lifted pixels move on the GPU from now on
    /// ([`Engine::float_move`]); the layer shows without them. False when nothing's lifted.
    pub fn float_begin(&mut self) -> bool {
        self.float_from_painter(true)
    }

    /// The lifted pixels become the GPU's picture; `show_base`: the layer shows without
    /// them now (not needed after `transform_begin_lifted`, which already does).
    fn float_from_painter(&mut self, show_base: bool) -> bool {
        let Some((rgba, x, y, w, h)) = self.painter.transform_float() else {
            return false;
        };
        if show_base {
            self.painter.transform_show_base();
        }
        self.lifts += 1;
        self.float = Some(Float {
            rgba,
            w: w as u32,
            h: h as u32,
            at: (x as f32, y as f32),
            m: IDENTITY,
            lift: self.lifts,
        });
        true
    }

    /// Pixels that aren't on any layer yet (a paste) float at `at` (document pixels): the
    /// GPU shows them moving as lifted ones; whoever floated them draws them in (the
    /// painter has no transform for them, so [`Engine::float_end`] would only drop them).
    pub fn float_pixels(&mut self, rgba: Vec<u8>, w: u32, h: u32, at: (f32, f32)) {
        self.lifts += 1;
        self.float = Some(Float {
            rgba,
            w,
            h,
            at,
            m: IDENTITY,
            lift: self.lifts,
        });
    }

    /// Moves the lifted pixels to `m` (see [`Float::m`]): nothing is redrawn but the screen.
    pub fn float_move(&mut self, m: [f32; 6]) {
        if let Some(f) = &mut self.float {
            f.m = m;
        }
    }

    /// Ends the move: `commit` draws the pixels into the layer where they are (one undo
    /// step), otherwise they go back. Returns whether the picture changed.
    pub fn float_end(&mut self, commit: bool) -> bool {
        let Some(f) = self.float.take() else {
            return false;
        };
        if commit {
            self.painter.transform_preview(f.m);
            self.painter.transform_commit()
        } else {
            self.painter.transform_cancel();
            true
        }
    }

    /// A pointer on the canvas (pen or mouse, pressed). The Brush and the Eraser paint;
    /// the other tools are [`tools`]'.
    pub fn pointer(&mut self, s: &mut State, phase: Phase, p: Sample) {
        if phase == Phase::Down {
            // (the painter shows the frame's drawings: the GPU may have shown them alone)
            frames::catch_up(self, s);
        }
        if Engine::paints(s) || self.is_stroking() {
            // pixels still being moved are put down first (the painter takes no stroke while
            // they float), as picking the brush would have
            if phase == Phase::Down && (self.float.is_some() || self.painter.transforming()) {
                tools::apply(self, s, &Action::Other("Transform: commit".into()));
                // (lifted other than by the Transform tool)
                if self.float.is_some() {
                    self.float_end(true);
                } else if self.painter.transforming() {
                    self.painter.transform_commit();
                }
            }
            match phase {
                Phase::Down => self.down(s, p.x, p.y, p.pressure, p.tilt, p.ms),
                Phase::Move => self.moved(p.x, p.y, p.pressure, p.tilt, p.ms),
                Phase::Up => self.up(),
            }
        } else {
            tools::pointer(self, s, phase, p);
        }
    }

    /// Whether a press on the canvas at this document point goes to the engine with the
    /// current tool (the Hand, and Review, move the view instead).
    pub fn takes_canvas_input(s: &State) -> bool {
        s.workspace != crate::state::Workspace::Review && s.tool != Tool::Hand
    }

    /// Carries out what the interface asked for. Returns whether the picture may have
    /// changed (the shell then shows it).
    pub fn apply(&mut self, s: &mut State, a: &Action) -> bool {
        // going from frame to frame needs nothing of the painter; anything else is done on
        // the drawings it shows (the GPU may have shown the frame alone)
        if !matches!(
            a,
            Action::GoToFrame(_)
                | Action::NextFrame
                | Action::PreviousFrame
                | Action::NextDrawing
                | Action::PreviousDrawing
                | Action::Play
                | Action::Stop
        ) {
            frames::catch_up(self, s);
        }
        match a {
            Action::Undo => self.painter.undo(),
            Action::Redo => self.painter.redo(),
            _ => {
                sound::apply(self, s, a);
                files::apply(self, s, a)
                    || export::apply(self, s, a)
                    || tools::apply(self, s, a)
                    || frames::apply(self, s, a)
            }
        }
    }

    /// Once a loop, before drawing: playback and anything else that moves with time.
    /// Returns whether the picture changed.
    pub fn tick(&mut self, s: &mut State, now: std::time::Instant) -> bool {
        let f = files::tick(self, s, now);
        let x = export::tick(self, s, now);
        let p = frames::tick(self, s, now);
        // (after playback's: it follows the clock playback has just set)
        let n = sound::tick(self, s, now);
        p | f | x | n
    }

    /// When [`Engine::tick`] next has something to do (the shell sleeps until then).
    pub fn next_tick_at(&self, s: &State) -> Option<std::time::Instant> {
        [
            frames::next_tick_at(self, s),
            files::next_tick_at(self, s),
            export::next_tick_at(self, s),
            sound::next_tick_at(self, s),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The canvas's picture (straight RGBA8, document size) and what changed in it since
    /// the last call (x, y, w, h), if anything.
    pub fn picture(&mut self, s: &State) -> Option<(&[u8], [u32; 4])> {
        frames::picture(self, s)
    }

    /// The canvas's picture made on the GPU into `comp` where it can be, otherwise the
    /// painter's as [`Engine::picture`] gives it ([`frames::gpu`]).
    pub fn picture_gpu(
        &mut self,
        s: &State,
        gpu: &pencilpilot_gpu::Gpu,
        comp: &mut pencilpilot_gpu::Compositor,
    ) -> frames::gpu::Pic<'_> {
        frames::gpu::picture(self, s, gpu, comp)
    }

    /// The drawings of the frames around this one go up to the GPU ahead of being shown,
    /// for `budget` at most ([`frames::gpu::ahead`]). Whether there's more to do.
    pub fn gpu_ahead(
        &mut self,
        s: &State,
        gpu: &pencilpilot_gpu::Gpu,
        comp: &mut pencilpilot_gpu::Compositor,
        budget: std::time::Duration,
    ) -> bool {
        frames::gpu::ahead(self, s, gpu, comp, budget)
    }

    /// The brush to paint with now: the picked one, in the picked colour and size, erasing
    /// with the Eraser tool.
    fn brush(s: &State) -> Option<hokusai::Brush> {
        let name = s.brushes.get(s.brush)?;
        let src = if s.tool == Tool::Eraser {
            ERASER
        } else {
            BRUSHES
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| *b)
                .unwrap_or(BRUSHES[0].1)
        };
        let mut b = parsed(src)?;
        let [r, g, bl, _] = s.colour.map(|c| (c * 255.0).round() as u8);
        set_brush_rgb(&mut b, r, g, bl);
        // the size: brush_size px across, as a log radius
        b.get_mut(BrushSetting::Radius).base_value = (s.brush_size.max(0.5) / 2.0).ln();
        if s.tool == Tool::Eraser {
            b.get_mut(BrushSetting::Eraser).base_value = 1.0;
        }
        tools::tune_brush(&mut b, s);
        Some(b)
    }

    /// Whether the tool paints on the canvas.
    pub fn paints(s: &State) -> bool {
        matches!(s.tool, Tool::Brush | Tool::Eraser)
    }

    pub fn is_stroking(&self) -> bool {
        self.stroking.is_some()
    }

    /// A stroke's sample (canvas pixels; pressure 0–1; tilt −1–1; the input's time in ms).
    pub fn down(&mut self, s: &State, x: f32, y: f32, pressure: f32, tilt: (f32, f32), ms: u32) {
        frames::catch_up(self, s);
        let Some(b) = Engine::brush(s) else { return };
        self.painter.begin_stroke();
        self.painter
            .stroke_to(&b, x, y, pressure, tilt.0, tilt.1, 0.004);
        self.stroking = Some(b);
        self.last_ms = Some(ms);
        self.tap = Some(Tap {
            x,
            y,
            pressure,
            tilt,
            size: s.brush_size,
            moved: false,
        });
    }

    pub fn moved(&mut self, x: f32, y: f32, pressure: f32, tilt: (f32, f32), ms: u32) {
        let Some(b) = &self.stroking else { return };
        let dt = self.last_ms.map_or(0.004, |l| {
            (ms.wrapping_sub(l) as f64 / 1000.0).clamp(0.0005, 0.1)
        });
        self.last_ms = Some(ms);
        if let Some(t) = &mut self.tap
            && ((x - t.x).abs() > 0.5 || (y - t.y).abs() > 0.5)
        {
            t.moved = true;
        }
        self.painter
            .stroke_to(b, x, y, pressure, tilt.0, tilt.1, dt);
    }

    pub fn up(&mut self) {
        if let Some(b) = self.stroking.take() {
            // a tap: a dot where it landed (a brush lays down nothing without moving), a
            // little there and back
            if let Some(t) = self.tap
                && !t.moved
            {
                let d = (t.size / 4.0).max(0.6);
                for (dx, dy) in [(d, 0.0), (-d, d * 0.5), (0.0, 0.0)] {
                    self.painter.stroke_to(
                        &b,
                        t.x + dx,
                        t.y + dy,
                        t.pressure,
                        t.tilt.0,
                        t.tilt.1,
                        0.01,
                    );
                }
            }
            self.painter.end_stroke(&b);
        }
        self.tap = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{State, Tool};

    fn stroke(e: &mut Engine, s: &State, y: f32) {
        e.down(s, 20.0, y, 0.9, (0.0, 0.0), 0);
        for i in 1..60 {
            e.moved(20.0 + i as f32 * 3.0, y, 0.9, (0.0, 0.0), i * 4);
        }
        e.up();
        e.painter.flush();
    }

    /// pixels darker than the paper in a row band
    fn ink(e: &Engine, y0: u32, y1: u32) -> usize {
        let w = e.painter.width();
        e.painter.rgba()[(y0 * w * 4) as usize..(y1 * w * 4) as usize]
            .chunks(4)
            .filter(|p| p[0] < 200)
            .count()
    }

    #[test]
    fn strokes_paint_erase_and_undo() {
        let mut s = State::demo();
        let mut e = Engine::new(240, 80);
        stroke(&mut e, &s, 30.0);
        let painted = ink(&e, 20, 40);
        assert!(painted > 100, "the stroke painted ({painted} px)");
        // the colour: the demo's ink (22, 22, 26), not coral
        let w = e.painter.width();
        let px = &e.painter.rgba()[((30 * w + 100) * 4) as usize..][..3];
        // (the pencil is grainy and see-through: ink over paper is a neutral grey, no colour cast)
        assert!(
            px[0] < 200
                && (px[0] as i32 - px[2] as i32).abs() < 12
                && (px[0] as i32 - px[1] as i32).abs() < 12,
            "ink, not another colour: {px:?}"
        );
        // the eraser takes it away
        s.tool = Tool::Eraser;
        s.brush_size = 20.0;
        stroke(&mut e, &s, 30.0);
        let left = ink(&e, 20, 40);
        assert!(left < painted / 4, "the eraser removed it ({left} px left)");
        // undo brings it back
        assert!(e.painter.undo());
        e.painter.flush();
        assert!(
            ink(&e, 20, 40) > painted * 3 / 4,
            "undo brought the stroke back"
        );
        // a bigger brush paints a wider line
        s.tool = Tool::Brush;
        s.brush_size = 4.0;
        stroke(&mut e, &s, 60.0);
        let thin = ink(&e, 45, 75);
        s.brush_size = 16.0;
        let mut e2 = Engine::new(240, 80);
        stroke(&mut e2, &s, 60.0);
        assert!(ink(&e2, 45, 75) > thin * 2, "16 px paints wider than 4 px");
    }

    /// Moving a large painted area (the owner's freeze in today's app): lifting it once,
    /// then each step of the drag only changes a matrix; the pixels land where it ends.
    #[test]
    fn moving_a_large_area_costs_nothing_per_step() {
        use std::time::Instant;
        let mut e = Engine::new(1920, 1080);
        let (w, h) = (1920usize, 1080usize);
        // 1200 × 800 of solid paint from (300, 100)
        let mut cov = vec![0u8; w * h];
        for y in 100..900 {
            cov[y * w + 300..y * w + 1500].fill(255);
        }
        assert!(e.painter.fill_mask(&cov, [200, 30, 30], 1.0));
        e.painter.flush();
        let red = |e: &Engine, x: usize, y: usize| {
            let p = &e.painter.rgba()[(y * w + x) * 4..][..3];
            p[0] > 150 && p[1] < 80
        };
        assert!(red(&e, 305, 105));

        let t = Instant::now();
        assert_eq!(e.lift(), vec![300.0, 100.0, 1500.0, 900.0]);
        let lift = t.elapsed();
        let f = e.float.as_ref().unwrap();
        assert_eq!((f.w, f.h, f.at), (1200, 800, (300.0, 100.0)));
        e.painter.flush();
        assert!(
            !red(&e, 305, 105),
            "while moving, the layer shows without the lifted paint"
        );

        // a drag: 200 steps
        let t = Instant::now();
        for i in 0..200 {
            let k = i as f32 * 0.5;
            e.float_move([1.0, 0.0, 0.0, 1.0, k, k / 2.0]);
            e.painter.flush();
        }
        let per_step = t.elapsed() / 200;

        let t = Instant::now();
        e.float_move([1.0, 0.0, 0.0, 1.0, 100.0, 50.0]);
        assert!(e.float_end(true));
        e.painter.flush();
        let commit = t.elapsed();
        assert!(e.float.is_none());
        assert!(red(&e, 405, 155), "the paint is where the drag left it");
        assert!(!red(&e, 305, 105), "and not where it was");
        assert!(
            red(&e, 1595, 945) && !red(&e, 1605, 955),
            "its far corner moved too"
        );
        // one undo puts it back
        assert!(e.painter.undo());
        e.painter.flush();
        assert!(red(&e, 305, 105) && !red(&e, 1550, 920));
        // a move by a fraction of a pixel (any zoom but 100 %) resamples every pixel
        e.painter.redo();
        e.painter.flush();
        assert!(!e.lift().is_empty());
        let t = Instant::now();
        e.float_move([1.0, 0.0, 0.0, 1.0, 93.3, 46.65]);
        assert!(e.float_end(true));
        e.painter.flush();
        let commit_fraction = t.elapsed();
        eprintln!(
            "1200×800 move: lift {lift:?}, each drag step {per_step:?}, commit {commit:?}, \
             commit after a fractional move {commit_fraction:?}"
        );
        assert!(per_step.as_micros() < 1000, "a drag step: {per_step:?}");
    }

    /// A stroke started while pixels are still being moved (a key and a pen press in the same
    /// batch of input): the move is kept and the stroke paints.
    #[test]
    fn a_stroke_while_pixels_float_keeps_both() {
        let mut s = State::demo();
        let mut e = Engine::new(240, 120);
        let (w, h) = (240usize, 120usize);
        let mut cov = vec![0u8; w * h];
        for y in 10..30 {
            cov[y * w + 10..y * w + 30].fill(255);
        }
        assert!(e.painter.fill_mask(&cov, [200, 30, 30], 1.0));
        e.painter.flush();
        assert!(!e.lift().is_empty());
        e.float_move([1.0, 0.0, 0.0, 1.0, 100.0, 0.0]);
        // the brush, before the interface turned the tool change into a commit
        s.tool = Tool::Brush;
        let at = |x: f32, ms: u32| Sample {
            x,
            y: 80.0,
            pressure: 0.9,
            tilt: (0.0, 0.0),
            ms,
        };
        e.pointer(&mut s, Phase::Down, at(20.0, 0));
        for i in 1..60 {
            e.pointer(&mut s, Phase::Move, at(20.0 + i as f32 * 3.0, i * 4));
        }
        e.pointer(&mut s, Phase::Up, at(200.0, 240));
        e.painter.flush();
        assert!(e.float.is_none() && !e.painter.transforming());
        let red = |e: &Engine, x: usize, y: usize| {
            let p = &e.painter.rgba()[(y * w + x) * 4..][..3];
            p[0] > 150 && p[1] < 80
        };
        assert!(red(&e, 120, 20) && !red(&e, 20, 20), "the move was kept");
        assert!(ink(&e, 70, 90) > 100, "and the stroke painted");
    }

    /// A tap (press and release without moving) paints a dot there, about the brush's size.
    #[test]
    fn a_tap_paints_a_dot() {
        let mut s = State::demo();
        for size in [4.0, 12.0, 40.0] {
            s.brush_size = size;
            let mut e = Engine::new(200, 200);
            e.down(&s, 100.0, 100.0, 0.8, (0.0, 0.0), 0);
            e.up();
            e.painter.flush();
            let w = 200usize;
            let inked: Vec<(usize, usize)> = (0..200 * 200)
                .filter(|i| e.painter.rgba()[i * 4] < 200)
                .map(|i| (i % w, i / w))
                .collect();
            assert!(!inked.is_empty(), "{size} px: the tap painted nothing");
            let far = inked
                .iter()
                .map(|&(x, y)| ((x as f32 - 100.0).powi(2) + (y as f32 - 100.0).powi(2)).sqrt())
                .fold(0.0, f32::max);
            assert!(
                far <= size,
                "{size} px: a dot, not a line (reaches {far} px)"
            );
            // one undo takes it
            assert!(e.painter.undo());
        }
    }
}

#[cfg(test)]
mod stroke_end_timing {
    use super::*;
    use crate::state::{State, Workspace};
    use std::time::Instant;

    /// What a stroke's start and end cost the frame they land in (tick and picture).
    #[test]
    #[ignore = "timing: cargo test --release -- --ignored stroke_end --nocapture"]
    fn stroke_end_costs() {
        let mut s = State::demo();
        s.workspace = Workspace::Draw;
        let mut e = Engine::new(1920, 1080);
        e.tick(&mut s, Instant::now());
        let _ = e.picture(&s);
        let at = |x: f32, ms: u32| Sample {
            x,
            y: 500.0,
            pressure: 0.8,
            tilt: (0.0, 0.0),
            ms,
        };
        for round in 0..5 {
            let t = Instant::now();
            e.pointer(&mut s, Phase::Down, at(300.0, 0));
            let down = t.elapsed();
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            let _ = e.picture(&s);
            let down_frame = t.elapsed();
            let mut mid = std::time::Duration::ZERO;
            for i in 1..60 {
                e.pointer(&mut s, Phase::Move, at(300.0 + i as f32 * 8.0, i * 4));
                let t = Instant::now();
                e.tick(&mut s, Instant::now());
                let _ = e.picture(&s);
                mid = mid.max(t.elapsed());
            }
            let t = Instant::now();
            e.pointer(&mut s, Phase::Up, at(780.0, 240));
            let up = t.elapsed();
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            let tick = t.elapsed();
            let t = Instant::now();
            let _ = e.picture(&s);
            let pic = t.elapsed();
            eprintln!(
                "round {round}: down {down:?}, its frame {down_frame:?}, slowest mid frame {mid:?}, up {up:?}, tick after {tick:?}, picture after {pic:?}"
            );
        }
    }

    /// A stroke's start doesn't parse its brush again (it did, at every stroke).
    #[test]
    fn a_brush_is_parsed_once() {
        let s = State::demo();
        let t = std::time::Instant::now();
        let _ = hokusai::myb::from_str(BRUSHES[0].1).unwrap();
        let parse = t.elapsed();
        let _ = Engine::brush(&s).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..10 {
            let _ = Engine::brush(&s).unwrap();
        }
        let cached = t.elapsed() / 10;
        eprintln!("parsing the brush {parse:?}, from the cache {cached:?}");
        assert!(cached < parse, "{cached:?} vs {parse:?}");
    }
}

#[cfg(test)]
mod lift_with_selection {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "timing: cargo test --release -- --ignored lift_with_a_selection --nocapture"]
    fn lift_with_a_selection() {
        let mut e = Engine::new(1920, 1080);
        let (w, h) = (1920usize, 1080usize);
        let mut cov = vec![0u8; w * h];
        for y in 100..900 {
            cov[y * w + 300..y * w + 1500].fill(255);
        }
        assert!(e.painter.fill_mask(&cov, [200, 30, 30], 1.0));
        e.painter.flush();
        let mut sel = vec![0u8; w * h];
        for y in 80..920 {
            sel[y * w + 280..y * w + 1520].fill(255);
        }
        e.painter.set_selection(&sel, 0);
        let mut st = crate::state::State::demo();
        st.tool = crate::state::Tool::Transform;
        for _ in 0..3 {
            let t = Instant::now();
            assert!(e.apply(&mut st, &Action::Other("Transform".into())));
            let whole = t.elapsed();
            let t = Instant::now();
            e.painter.flush();
            eprintln!("the Transform action: {whole:?}, flush {:?}", t.elapsed());
            e.apply(&mut st, &Action::Other("Transform: cancel".into()));
            e.painter.flush();
        }
        for _ in 0..3 {
            let t = Instant::now();
            assert!(!e.lift().is_empty());
            let lift = t.elapsed();
            let t = Instant::now();
            e.painter.flush();
            let flush = t.elapsed();
            eprintln!("with a selection: lift {lift:?}, flush {flush:?}");
            e.float_end(false);
            e.painter.flush();
        }
    }
}
