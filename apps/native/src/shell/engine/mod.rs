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

pub mod frames;
pub mod tools;

use brush::{Painter, set_brush_rgb};
use hokusai::BrushSetting;

use crate::state::{Action, State, Tool};

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
    /// the other tools' state ([`tools`])
    pub tools: tools::Tools,
    /// layers, drawings, playback ([`frames`])
    pub frames: frames::Frames,
}

impl Engine {
    pub fn new(width: u32, height: u32) -> Engine {
        Engine {
            painter: Painter::new(width, height),
            stroking: None,
            last_ms: None,
            tools: tools::Tools::default(),
            frames: frames::Frames::default(),
        }
    }

    /// A pointer on the canvas (pen or mouse, pressed). The Brush and the Eraser paint;
    /// the other tools are [`tools`]'.
    pub fn pointer(&mut self, s: &mut State, phase: Phase, p: Sample) {
        if Engine::paints(s) || self.is_stroking() {
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
        match a {
            Action::Undo => self.painter.undo(),
            Action::Redo => self.painter.redo(),
            _ => tools::apply(self, s, a) || frames::apply(self, s, a),
        }
    }

    /// Once a loop, before drawing: playback and anything else that moves with time.
    /// Returns whether the picture changed.
    pub fn tick(&mut self, s: &mut State, now: std::time::Instant) -> bool {
        frames::tick(self, s, now)
    }

    /// When [`Engine::tick`] next has something to do (the shell sleeps until then).
    pub fn next_tick_at(&self, s: &State) -> Option<std::time::Instant> {
        frames::next_tick_at(self, s)
    }

    /// The canvas's picture (straight RGBA8, document size) and what changed in it since
    /// the last call (x, y, w, h), if anything.
    pub fn picture(&mut self, s: &State) -> Option<(&[u8], [u32; 4])> {
        frames::picture(self, s)
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
        let mut b = hokusai::myb::from_str(src).ok()?;
        let [r, g, bl, _] = s.colour.map(|c| (c * 255.0).round() as u8);
        set_brush_rgb(&mut b, r, g, bl);
        // the size: brush_size px across, as a log radius
        b.get_mut(BrushSetting::Radius).base_value = (s.brush_size.max(0.5) / 2.0).ln();
        if s.tool == Tool::Eraser {
            b.get_mut(BrushSetting::Eraser).base_value = 1.0;
        }
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
        let Some(b) = Engine::brush(s) else { return };
        self.painter.begin_stroke();
        self.painter
            .stroke_to(&b, x, y, pressure, tilt.0, tilt.1, 0.004);
        self.stroking = Some(b);
        self.last_ms = Some(ms);
    }

    pub fn moved(&mut self, x: f32, y: f32, pressure: f32, tilt: (f32, f32), ms: u32) {
        let Some(b) = &self.stroking else { return };
        let dt = self.last_ms.map_or(0.004, |l| {
            (ms.wrapping_sub(l) as f64 / 1000.0).clamp(0.0005, 0.1)
        });
        self.last_ms = Some(ms);
        self.painter
            .stroke_to(b, x, y, pressure, tilt.0, tilt.1, dt);
    }

    pub fn up(&mut self) {
        if let Some(b) = self.stroking.take() {
            self.painter.end_stroke(&b);
        }
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
}
