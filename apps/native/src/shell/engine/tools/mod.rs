//! The canvas tools other than the Brush and the Eraser (Fill, Lasso, Select, Transform,
//! Eyedropper, Shape, Text), the selection's and the canvas's menus, and the clipboard.
//!
//! - this file: the tools on the canvas ([`pointer`]), what the menus and keys ask for
//!   ([`apply`]), and how the brush follows the brush settings ([`tune_brush`]);
//! - [`overlay`]: what's drawn over the drawing (the selection's edge, the transform's box
//!   and handles, the lasso, a shape being drawn, the text's caret, the grid and the
//!   symmetry lines), the selection's menu, the tools' keys and the brush settings pop-up;
//! - [`geom`]: maps, polygons and outlines; [`text`]: text drawn into pixels.
//!
//! Positions are the document's pixels. The interface ([`overlay`]) never touches the
//! engine: it changes [`ToolsUi`] and pushes [`Action::Other`]s, carried out here.

pub mod geom;
pub mod overlay;
pub mod text;

use brush::{Symmetry, SymmetryMode};
use hokusai::{BrushInput, BrushSetting};

use super::{Engine, Phase, Sample};
use crate::state::{Action, State, Tool, Workspace};
use geom::Affine;

/// The grid's spacing (document pixels), and how near (screen pixels) a point snaps to it.
pub const GRID: f32 = 64.0;
const SNAP_PX: f32 = 8.0;
/// Handles: how near (screen pixels) a press must be to take one; the turning handle's
/// distance above the box (screen pixels).
pub const HANDLE_REACH: f32 = 12.0;
pub const ROTATE_ARM: f32 = 28.0;

/// A shape the Shape tool draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeKind {
    #[default]
    Line,
    Rectangle,
    Ellipse,
}

impl ShapeKind {
    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
        }
    }
    pub fn next(self) -> ShapeKind {
        match self {
            ShapeKind::Line => ShapeKind::Rectangle,
            ShapeKind::Rectangle => ShapeKind::Ellipse,
            ShapeKind::Ellipse => ShapeKind::Line,
        }
    }
}

/// Symmetry, as the canvas menu offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sym {
    #[default]
    Off,
    Vertical,
    Horizontal,
    Both,
}

impl Sym {
    pub const ALL: [Sym; 4] = [Sym::Off, Sym::Vertical, Sym::Horizontal, Sym::Both];
    pub fn name(self) -> &'static str {
        match self {
            Sym::Off => "Off",
            Sym::Vertical => "Vertical",
            Sym::Horizontal => "Horizontal",
            Sym::Both => "Both",
        }
    }
    pub fn next(self) -> Sym {
        Sym::ALL[(Sym::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1) % 4]
    }
}

/// A transform in progress: the box lifted (x0, y0, x1, y1) and where it's taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XfBox {
    pub rect: [f32; 4],
    pub m: Affine,
}

impl XfBox {
    /// The box's point at (u, v) ∈ {−1, 0, 1}² (corners, edges' middles, centre), placed.
    pub fn point(&self, u: i32, v: i32) -> (f32, f32) {
        let (x, y) = self.local(u, v);
        geom::apply(self.m, x, y)
    }
    fn local(&self, u: i32, v: i32) -> (f32, f32) {
        let [x0, y0, x1, y1] = self.rect;
        let pick = |a: f32, b: f32, t: i32| match t {
            -1 => a,
            1 => b,
            _ => (a + b) / 2.0,
        };
        (pick(x0, x1, u), pick(y0, y1, v))
    }
    /// The corners, placed: top left, top right, bottom right, bottom left.
    pub fn corners(&self) -> [(f32, f32); 4] {
        [
            self.point(-1, -1),
            self.point(1, -1),
            self.point(1, 1),
            self.point(-1, 1),
        ]
    }
    /// The turning handle, `arm` document pixels out from the top edge's middle.
    pub fn rotate_handle(&self, arm: f32) -> (f32, f32) {
        let top = self.point(0, -1);
        let mid = self.point(0, 0);
        let (dx, dy) = (top.0 - mid.0, top.1 - mid.1);
        let len = dx.hypot(dy);
        // a box squashed flat: straight up
        let (ux, uy) = if len < 1e-3 {
            (0.0, -1.0)
        } else {
            (dx / len, dy / len)
        };
        (top.0 + ux * arm, top.1 + uy * arm)
    }
}

/// Text being typed: where (its top left), its size and what's typed (the interface
/// types into `text`); the caret's place from the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct TextUi {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub text: String,
    /// after the last character, from (x, y): x, top, height
    pub caret: (f32, f32, f32),
    /// the drawn text's box from (x, y): x, y, w, h
    pub bounds: [f32; 4],
}

/// What the interface shows of these tools (in [`State::tooling`]): the selection's outline,
/// a transform's box, a shape being drawn… in document pixels; and the canvas menu's
/// settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolsUi {
    /// the selection's edge, as closed loops, and its bounding box (x0, y0, x1, y1)
    pub outline: Vec<Vec<(f32, f32)>>,
    pub selection: Option<[f32; 4]>,
    /// a rectangle being dragged out (x0, y0, x1, y1), a lasso being drawn
    pub marquee: Option<[f32; 4]>,
    pub lasso: Vec<(f32, f32)>,
    pub transform: Option<XfBox>,
    /// a shape being dragged out (from, to), and the kind the Shape tool draws
    pub shape: Option<[(f32, f32); 2]>,
    pub shape_kind: ShapeKind,
    pub text: Option<TextUi>,
    /// the canvas menu's settings
    pub grid: bool,
    pub snap: bool,
    pub symmetry: Sym,
    /// (only remembered for now: nothing records yet)
    pub timelapse: bool,
    /// something is on the clipboard
    pub has_clip: bool,
    /// Shift and Alt held (set by the shell before each press): Shift adds to the
    /// selection, keeps a shape square and a scale even, turns in 15° steps, moves along
    /// one axis; Alt takes away from the selection
    pub shift: bool,
    pub alt: bool,
    /// the interface's own memory (see `overlay`)
    pub memo: overlay::Memo,
}

impl Default for ToolsUi {
    fn default() -> Self {
        ToolsUi {
            outline: Vec::new(),
            selection: None,
            marquee: None,
            lasso: Vec::new(),
            transform: None,
            shape: None,
            shape_kind: ShapeKind::default(),
            text: None,
            grid: false,
            snap: true,
            symmetry: Sym::Off,
            timelapse: false,
            has_clip: false,
            shift: false,
            alt: false,
            memo: overlay::Memo::default(),
        }
    }
}

impl ToolsUi {
    /// The hint the canvas menu shows beside `item` (its state), if it has one.
    pub fn menu_hint(&self, item: &str) -> Option<String> {
        Some(match item {
            "Guides" => format!("Grid {} ›", if self.grid { "on" } else { "off" }),
            "Symmetry" => format!("{} ›", self.symmetry.name()),
            "Snap to guides" => if self.snap { "✓" } else { "" }.into(),
            "Record timelapse" => if self.timelapse { "● on" } else { "●" }.into(),
            _ => return None,
        })
    }
}

/// What the pixels copied are.
#[derive(Debug, Clone)]
struct Clip {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    /// straight RGBA8
    rgba: Vec<u8>,
}

/// What a press on the canvas is doing.
#[derive(Debug, Clone, Copy, Default)]
enum Drag {
    #[default]
    None,
    Marquee {
        from: (f32, f32),
        op: u8,
    },
    Lasso {
        op: u8,
    },
    Xf {
        grab: Grab,
        from: (f32, f32),
        m0: Affine,
    },
    Shape {
        from: (f32, f32),
        pressure: f32,
    },
    Pick,
}

/// Which part of the transform's box a press took.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    Move,
    /// a corner or an edge's middle: (u, v) ∈ {−1, 0, 1}²
    Scale(i32, i32),
    Rotate,
}

/// What these tools keep between events (a lasso's points, a transform in progress…).
#[derive(Default)]
pub struct Tools {
    drag: Drag,
    clip: Option<Clip>,
    typesetter: Option<text::Typesetter>,
    /// the layer's pixels when typing started
    text_base: Option<Vec<u8>>,
}

impl std::fmt::Debug for Tools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tools")
            .field("drag", &self.drag)
            .field("clip", &self.clip.as_ref().map(|c| (c.x, c.y, c.w, c.h)))
            .finish_non_exhaustive()
    }
}

/// The selection op the held keys make: 0 replace, 1 add (Shift), 2 take away (Alt).
fn op(s: &State) -> u8 {
    if s.tooling.alt {
        2
    } else if s.tooling.shift {
        1
    } else {
        0
    }
}

/// The colour as the painter takes it.
fn rgb(s: &State) -> [u8; 3] {
    let [r, g, b, _] = s.colour.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    [r, g, b]
}

fn opacity(s: &State) -> f32 {
    (s.panel.opacity / 100.0).clamp(0.0, 1.0)
}

/// A point snapped to the grid's lines (when the grid shows and snapping is on).
fn snap(s: &State, p: (f32, f32)) -> (f32, f32) {
    if !(s.tooling.grid && s.tooling.snap) {
        return p;
    }
    let reach = SNAP_PX / s.view.zoom.max(1e-3);
    let one = |v: f32| {
        let g = (v / GRID).round() * GRID;
        if (g - v).abs() <= reach { g } else { v }
    };
    (one(p.0), one(p.1))
}

/// Keeps the tools' state in step with the painter (an undo while transforming or
/// typing cancels those).
fn sync(e: &mut Engine, s: &mut State) {
    if s.tooling.transform.is_some() && !e.painter.transforming() {
        e.float = None;
        e.tools.drag = Drag::None;
        s.tooling.transform = None;
        refresh_selection(e, s);
    }
    if s.tooling.text.is_some() && e.painter.text_layer() == 0 {
        s.tooling.text = None;
        e.tools.text_base = None;
    }
}

/// A pointer on the canvas with one of these tools.
pub fn pointer(e: &mut Engine, s: &mut State, phase: Phase, p: Sample) {
    sync(e, s);
    let at = (p.x, p.y);
    if phase == Phase::Down {
        // a press with another tool ends the typing and the transform
        if s.tool != Tool::Text {
            text_commit(e, s);
        }
        if s.tool != Tool::Transform {
            xf_commit(e, s);
        }
    }
    match (s.tool, phase) {
        (Tool::Fill, Phase::Down) => {
            fill_at(e, s, at);
        }
        (Tool::Eyedropper, Phase::Down) => {
            e.tools.drag = Drag::Pick;
            pick(e, s, at);
        }
        (Tool::Eyedropper, Phase::Move) if matches!(e.tools.drag, Drag::Pick) => pick(e, s, at),
        (Tool::Select, Phase::Down) => {
            let from = snap(s, at);
            e.tools.drag = Drag::Marquee { from, op: op(s) };
            s.tooling.marquee = Some([from.0, from.1, from.0, from.1]);
        }
        (Tool::Select, Phase::Move) => {
            if let Drag::Marquee { from, .. } = e.tools.drag {
                let to = snap(s, at);
                s.tooling.marquee = Some([from.0, from.1, to.0, to.1]);
            }
        }
        (Tool::Select, Phase::Up) => {
            if let Drag::Marquee { from, op } = std::mem::take(&mut e.tools.drag) {
                let to = snap(s, at);
                s.tooling.marquee = None;
                select_rect(e, s, [from.0, from.1, to.0, to.1], op);
            }
        }
        (Tool::Lasso, Phase::Down) => {
            e.tools.drag = Drag::Lasso { op: op(s) };
            s.tooling.lasso = vec![at];
        }
        (Tool::Lasso, Phase::Move) => {
            if matches!(e.tools.drag, Drag::Lasso { .. }) {
                let last = s.tooling.lasso.last().copied().unwrap_or(at);
                // a point a document pixel or a screen pixel on, whichever is more
                let step = (1.0 / s.view.zoom.max(1e-3)).max(1.0);
                if (at.0 - last.0).hypot(at.1 - last.1) >= step {
                    s.tooling.lasso.push(at);
                }
            }
        }
        (Tool::Lasso, Phase::Up) => {
            if let Drag::Lasso { op } = std::mem::take(&mut e.tools.drag) {
                s.tooling.lasso.push(at);
                let pts = std::mem::take(&mut s.tooling.lasso);
                lasso_done(e, s, &pts, op);
            }
        }
        (Tool::Transform, Phase::Down) => {
            if !xf_begin(e, s) {
                return;
            }
            let Some(b) = s.tooling.transform else { return };
            e.tools.drag = Drag::Xf {
                grab: grab_at(&b, at, s.view.zoom),
                from: at,
                m0: b.m,
            };
        }
        (Tool::Transform, Phase::Move | Phase::Up) => {
            if let Drag::Xf { grab, from, m0 } = e.tools.drag {
                let m = drag_map(s, grab, from, m0, at);
                xf_set(e, s, m);
            }
            if phase == Phase::Up {
                e.tools.drag = Drag::None;
            }
        }
        (Tool::Shape, Phase::Down) => {
            let from = snap(s, at);
            e.tools.drag = Drag::Shape {
                from,
                pressure: p.pressure,
            };
            s.tooling.shape = Some([from, from]);
        }
        (Tool::Shape, Phase::Move) => {
            if let Drag::Shape { from, .. } = e.tools.drag {
                s.tooling.shape = Some([from, shape_end(s, from, snap(s, at))]);
            }
        }
        (Tool::Shape, Phase::Up) => {
            if let Drag::Shape { from, pressure } = std::mem::take(&mut e.tools.drag) {
                let to = shape_end(s, from, snap(s, at));
                s.tooling.shape = None;
                draw_shape(e, s, s.tooling.shape_kind, from, to, pressure);
            }
        }
        (Tool::Text, Phase::Down) => {
            // a press on the text being typed keeps typing; anywhere else, the text is
            // kept and new text starts there
            if let Some(t) = &s.tooling.text {
                let [bx, by, bw, bh] = t.bounds;
                let pad = 8.0 / s.view.zoom.max(1e-3);
                let (x, y) = (at.0 - t.x, at.1 - t.y);
                if x >= bx - pad && y >= by - pad && x <= bx + bw + pad && y <= by + bh + pad {
                    return;
                }
                text_commit(e, s);
            }
            text_start(e, s, at);
        }
        (_, Phase::Up) => e.tools.drag = Drag::None,
        _ => {}
    }
}

/// An action these tools carry out. Returns whether it was theirs and the picture may have
/// changed.
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    sync(e, s);
    let Action::Other(name) = a else {
        return false;
    };
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    match name.as_str() {
        "Select all" => {
            xf_commit(e, s);
            e.painter.select_all();
            refresh_selection(e, s);
        }
        "Deselect" => {
            xf_commit(e, s);
            e.painter.deselect();
            refresh_selection(e, s);
        }
        "Invert selection" => {
            xf_commit(e, s);
            e.painter.invert_selection();
            refresh_selection(e, s);
        }
        "Copy" => {
            xf_commit(e, s);
            copy(e, s);
        }
        "Cut" => {
            xf_commit(e, s);
            if copy(e, s) {
                e.painter.erase_mask(&vec![255; w * h]);
            }
        }
        "Clear" => {
            xf_commit(e, s);
            e.painter.erase_mask(&vec![255; w * h]);
        }
        "Paste" | "Paste in place" => {
            if paste(e, s) && name == "Paste" {
                // the pasted pixels, ready to move
                s.tool = Tool::Transform;
                xf_begin(e, s);
            }
        }
        "Fill selection" => {
            xf_commit(e, s);
            e.painter.fill_selection(rgb(s), opacity(s));
        }
        "Flip horizontally" | "Flip vertically" => flip(e, s, name == "Flip horizontally"),
        "Transform" => {
            s.tool = Tool::Transform;
            xf_begin(e, s);
        }
        "Transform: commit" => xf_commit(e, s),
        "Transform: cancel" => xf_cancel(e, s),
        "Guides" | "Grid" => s.tooling.grid = !s.tooling.grid,
        "Snap to guides" => s.tooling.snap = !s.tooling.snap,
        "Record timelapse" => s.tooling.timelapse = !s.tooling.timelapse,
        "Symmetry" => set_symmetry(e, s, s.tooling.symmetry.next()),
        "Shape: next" => s.tooling.shape_kind = s.tooling.shape_kind.next(),
        "Text: changed" => text_update(e, s),
        "Text: commit" => text_commit(e, s),
        "Text: cancel" => text_cancel(e, s),
        // Escape: what's in progress stops; else the selection goes
        "Escape" => {
            if s.tooling.transform.is_some() {
                xf_cancel(e, s);
            } else if s.tooling.text.is_some() {
                text_cancel(e, s);
            } else {
                e.tools.drag = Drag::None;
                s.tooling.marquee = None;
                s.tooling.lasso.clear();
                s.tooling.shape = None;
                e.painter.deselect();
                refresh_selection(e, s);
            }
        }
        other => {
            let Some(sym) = other
                .strip_prefix("Symmetry: ")
                .and_then(|n| Sym::ALL.into_iter().find(|s| s.name() == n))
            else {
                return false;
            };
            set_symmetry(e, s, sym);
        }
    }
    true
}

// ---- brush settings ----

/// A mapping's value at `x` (its points, piecewise straight, held flat past the ends).
fn mapping_at(points: &[(f32, f32)], x: f32) -> f32 {
    match points {
        [] => 0.0,
        [p] => p.1,
        _ => {
            if x <= points[0].0 {
                return points[0].1;
            }
            for w in points.windows(2) {
                let (a, b) = (w[0], w[1]);
                if x <= b.0 {
                    let t = if b.0 > a.0 {
                        (x - a.0) / (b.0 - a.0)
                    } else {
                        1.0
                    };
                    return a.1 + (b.1 - a.1) * t;
                }
            }
            points[points.len() - 1].1
        }
    }
}

/// Makes the pen's pressure stop changing `setting`: its pressure mappings become their
/// value at full pressure.
fn drop_pressure(b: &mut hokusai::Brush, setting: BrushSetting) {
    let v = b.get_mut(setting);
    let mut add = 0.0;
    v.inputs.retain(|m| {
        if m.input == BrushInput::Pressure {
            add += mapping_at(&m.points, 1.0);
            false
        } else {
            true
        }
    });
    v.base_value += add;
}

/// The brush settings (opacity, steady lines, what the pen's pressure changes) put into
/// brush `b` for the current tool. [`Engine::brush`] should call it on the brush it makes.
pub fn tune_brush(b: &mut hokusai::Brush, s: &State) {
    // steady lines: the stroke trails the pen, smoothing its wobble (0–4)
    let steady = (s.panel.steady / 100.0).clamp(0.0, 1.0) * 4.0;
    let st = b.get_mut(BrushSetting::SlowTracking);
    st.base_value = st.base_value.max(steady);
    if !s.tools.pressure_size.contains(&s.tool) {
        drop_pressure(b, BrushSetting::Radius);
    }
    if !s.tools.pressure_opacity.contains(&s.tool) {
        drop_pressure(b, BrushSetting::Opaque);
        drop_pressure(b, BrushSetting::OpaqueMultiply);
    }
    // the opacity multiplies the dabs' (as the web app's slider does): `opaque` itself is
    // often more than 1 and clipped, so it goes on `opaque_multiply`
    let op = opacity(s);
    if op < 1.0 {
        // dabs overlap many times over: without this a see-through dab still builds up
        // to solid ink within one stroke
        let lin = b.get_mut(BrushSetting::OpaqueLinearize);
        lin.base_value = lin.base_value.max(0.9);
        let v = b.get_mut(BrushSetting::OpaqueMultiply);
        v.base_value *= op;
        for m in &mut v.inputs {
            for p in &mut m.points {
                p.1 *= op;
            }
        }
    }
}

// ---- fill and eyedropper ----

fn fill_at(e: &mut Engine, s: &State, at: (f32, f32)) -> bool {
    let (w, h) = (e.painter.width() as f32, e.painter.height() as f32);
    if at.0 < 0.0 || at.1 < 0.0 || at.0 >= w || at.1 >= h {
        return false;
    }
    // the design's "close gaps 4 px"; "fill under the lines" tucks the paint 2 px under them
    let gap = if s.panel.close_gaps { 4 } else { 0 };
    let grow = if s.panel.fill_under { 2 } else { 0 };
    e.painter.fill(
        at.0 as u32,
        at.1 as u32,
        rgb(s),
        opacity(s),
        0.12,
        true,
        grow,
        gap,
    )
}

fn pick(e: &mut Engine, s: &mut State, at: (f32, f32)) {
    let (w, h) = (e.painter.width() as i64, e.painter.height() as i64);
    let (x, y) = (at.0.floor() as i64, at.1.floor() as i64);
    if x < 0 || y < 0 || x >= w || y >= h {
        return;
    }
    e.painter.flush();
    let i = ((y * w + x) * 4) as usize;
    let px = &e.painter.rgba()[i..i + 4];
    s.colour = [
        px[0] as f32 / 255.0,
        px[1] as f32 / 255.0,
        px[2] as f32 / 255.0,
        1.0,
    ];
    s.colour_name = "Picked".into();
}

// ---- the selection ----

/// The outline and box the interface shows follow the painter's selection.
fn refresh_selection(e: &Engine, s: &mut State) {
    let mask = e.painter.selection_mask();
    if mask.is_empty() {
        s.tooling.outline.clear();
        s.tooling.selection = None;
        return;
    }
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    let loops = geom::outline(&mask, w, h);
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in loops.iter().flatten() {
        b = [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)];
    }
    s.tooling.selection = (!loops.is_empty()).then_some(b);
    s.tooling.outline = loops;
}

fn select_rect(e: &mut Engine, s: &mut State, r: [f32; 4], op: u8) {
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    // a click (no rectangle) clears the selection (adding or taking away nothing: keeps it)
    let (mask, any) = geom::fill_rect(r, w, h);
    if !any && op != 0 {
        return;
    }
    e.painter.set_selection(&mask, op);
    refresh_selection(e, s);
}

fn lasso_done(e: &mut Engine, s: &mut State, pts: &[(f32, f32)], op: u8) {
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    let (mask, any) = geom::fill_polygon(pts, w, h);
    // the Colour workspace's lasso fills; elsewhere it selects
    if s.workspace == Workspace::Colour {
        if any {
            e.painter.fill_mask(&mask, rgb(s), opacity(s));
        }
        return;
    }
    if !any && op != 0 {
        return;
    }
    e.painter.set_selection(&mask, op);
    refresh_selection(e, s);
}

// ---- the clipboard ----

/// Copies the active layer's selected pixels (all of them without a selection). Whether
/// anything was.
fn copy(e: &mut Engine, s: &mut State) -> bool {
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    let layer = e.painter.layer_rgba(e.painter.active_layer());
    if layer.len() != w * h * 4 {
        return false;
    }
    let sel = e.painter.selection_mask();
    let picked = |i: usize| if sel.is_empty() { 255 } else { sel[i] };
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if layer[i * 4 + 3] > 0 && picked(i) > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 == usize::MAX {
        return false;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut rgba = vec![0u8; cw * ch * 4];
    for y in 0..ch {
        for x in 0..cw {
            let i = (y0 + y) * w + x0 + x;
            let o = (y * cw + x) * 4;
            rgba[o..o + 3].copy_from_slice(&layer[i * 4..i * 4 + 3]);
            rgba[o + 3] = ((layer[i * 4 + 3] as u32 * picked(i) as u32 + 127) / 255) as u8;
        }
    }
    e.tools.clip = Some(Clip {
        x: x0 as i32,
        y: y0 as i32,
        w: cw,
        h: ch,
        rgba,
    });
    s.tooling.has_clip = true;
    true
}

/// `top` (straight RGBA8) over `under`, in place.
fn over(under: &mut [u8], top: &[u8]) {
    let ta = top[3] as f32 / 255.0;
    if ta <= 0.0 {
        return;
    }
    let ua = under[3] as f32 / 255.0;
    let oa = ta + ua * (1.0 - ta);
    for k in 0..3 {
        let c = (top[k] as f32 * ta + under[k] as f32 * ua * (1.0 - ta)) / oa;
        under[k] = c.round().clamp(0.0, 255.0) as u8;
    }
    under[3] = (oa * 255.0).round() as u8;
}

/// The active layer's pixels become `rgba` (straight RGBA8, canvas size): one undo step.
/// (Through the painter's text editing, which replaces a layer's pixels as one step.)
fn put_layer(e: &mut Engine, rgba: &[u8]) -> bool {
    let (w, h) = (e.painter.width(), e.painter.height());
    let id = e.painter.active_layer();
    if e.painter.text_begin(id, "") == 0 {
        return false;
    }
    e.painter.text_preview("", 0, 0, w, h, rgba);
    e.painter.text_commit()
}

/// Pastes the clipboard where it was copied from, on the active layer, and selects it.
fn paste(e: &mut Engine, s: &mut State) -> bool {
    xf_commit(e, s);
    text_commit(e, s);
    let Some(c) = e.tools.clip.clone() else {
        return false;
    };
    let (w, h) = (e.painter.width() as i32, e.painter.height() as i32);
    let mut layer = e.painter.layer_rgba(e.painter.active_layer());
    if layer.len() != (w * h * 4) as usize {
        return false;
    }
    let mut mask = vec![0u8; (w * h) as usize];
    for y in 0..c.h as i32 {
        for x in 0..c.w as i32 {
            let (cx, cy) = (c.x + x, c.y + y);
            if cx < 0 || cy < 0 || cx >= w || cy >= h {
                continue;
            }
            let o = (y as usize * c.w + x as usize) * 4;
            let i = (cy * w + cx) as usize;
            if c.rgba[o + 3] > 0 {
                over(&mut layer[i * 4..i * 4 + 4], &c.rgba[o..o + 4]);
                mask[i] = 255;
            }
        }
    }
    if !put_layer(e, &layer) {
        return false;
    }
    e.painter.set_selection(&mask, 0);
    refresh_selection(e, s);
    true
}

// ---- transform ----

/// Lifts the selection (or the whole active layer) to transform it: the GPU shows the
/// lifted pixels moving ([`Engine::float_begin`]), nothing is redrawn per pointer move.
/// Whether there's anything lifted.
fn xf_begin(e: &mut Engine, s: &mut State) -> bool {
    if s.tooling.transform.is_some() && e.painter.transforming() {
        return true;
    }
    text_commit(e, s);
    let b = e.lift();
    if b.len() != 4 {
        // (lifted, but no picture for the GPU: put it back)
        e.painter.transform_cancel();
        return false;
    }
    s.tooling.transform = Some(XfBox {
        rect: [b[0], b[1], b[2], b[3]],
        m: geom::IDENTITY,
    });
    true
}

/// The transform's map is now `m` (the GPU moves the pixels; the box follows).
fn xf_set(e: &mut Engine, s: &mut State, m: Affine) {
    e.float_move(m);
    if let Some(b) = &mut s.tooling.transform {
        b.m = m;
    }
}

/// Keeps the transform (one undo step); the selection follows the pixels.
fn xf_commit(e: &mut Engine, s: &mut State) {
    if s.tooling.transform.take().is_none() {
        return;
    }
    e.float_end(true);
    e.tools.drag = Drag::None;
    refresh_selection(e, s);
}

fn xf_cancel(e: &mut Engine, s: &mut State) {
    if s.tooling.transform.take().is_none() {
        return;
    }
    e.float_end(false);
    e.tools.drag = Drag::None;
}

/// What part of the box a press at `p` takes (`zoom`: screen pixels per document pixel).
fn grab_at(b: &XfBox, p: (f32, f32), zoom: f32) -> Grab {
    let reach = HANDLE_REACH / zoom.max(1e-3);
    let near = |q: (f32, f32)| (q.0 - p.0).hypot(q.1 - p.1) <= reach;
    if near(b.rotate_handle(ROTATE_ARM / zoom.max(1e-3))) {
        return Grab::Rotate;
    }
    for (u, v) in [
        (-1, -1),
        (1, -1),
        (1, 1),
        (-1, 1),
        (0, -1),
        (1, 0),
        (0, 1),
        (-1, 0),
    ] {
        if near(b.point(u, v)) {
            return Grab::Scale(u, v);
        }
    }
    let inside = geom::invert(b.m).is_some_and(|inv| {
        let (x, y) = geom::apply(inv, p.0, p.1);
        let [x0, y0, x1, y1] = b.rect;
        x >= x0 && x <= x1 && y >= y0 && y <= y1
    });
    if inside { Grab::Move } else { Grab::Rotate }
}

/// The map a drag from `from` to `to` makes of `m0`.
fn drag_map(s: &State, grab: Grab, from: (f32, f32), m0: Affine, to: (f32, f32)) -> Affine {
    let Some(b) = s.tooling.transform else {
        return m0;
    };
    let b = XfBox {
        rect: b.rect,
        m: m0,
    };
    match grab {
        Grab::Move => {
            let to = if s.tooling.shift {
                // along one axis
                if (to.0 - from.0).abs() > (to.1 - from.1).abs() {
                    (to.0, from.1)
                } else {
                    (from.0, to.1)
                }
            } else {
                to
            };
            geom::compose(geom::translate(to.0 - from.0, to.1 - from.1), m0)
        }
        Grab::Rotate => {
            let c = b.point(0, 0);
            let mut a = (to.1 - c.1).atan2(to.0 - c.0) - (from.1 - c.1).atan2(from.0 - c.0);
            if s.tooling.shift {
                let step = 15f32.to_radians();
                a = (a / step).round() * step;
            }
            geom::compose(geom::rotate_about(c.0, c.1, a), m0)
        }
        Grab::Scale(u, v) => {
            let Some(inv) = geom::invert(m0) else {
                return m0;
            };
            let p = geom::apply(inv, to.0, to.1);
            let (ax, ay) = b.local(-u, -v);
            let (hx, hy) = b.local(u, v);
            let factor = |p: f32, a: f32, h: f32| {
                let f = (p - a) / (h - a);
                if !f.is_finite() {
                    1.0
                } else if f.abs() < 0.01 {
                    0.01f32.copysign(f)
                } else {
                    f
                }
            };
            let mut sx = if u != 0 { factor(p.0, ax, hx) } else { 1.0 };
            let mut sy = if v != 0 { factor(p.1, ay, hy) } else { 1.0 };
            if s.tooling.shift && u != 0 && v != 0 {
                let k = sx.abs().max(sy.abs());
                sx = k.copysign(sx);
                sy = k.copysign(sy);
            }
            geom::compose(m0, geom::scale_about(ax, ay, sx, sy))
        }
    }
}

/// Flips the selection (or the layer) about its middle; a transform being made flips.
fn flip(e: &mut Engine, s: &mut State, horizontally: bool) {
    let begun = s.tooling.transform.is_some();
    if !xf_begin(e, s) {
        return;
    }
    let Some(b) = s.tooling.transform else { return };
    let c = b.point(0, 0);
    let f = if horizontally {
        geom::scale_about(c.0, c.1, -1.0, 1.0)
    } else {
        geom::scale_about(c.0, c.1, 1.0, -1.0)
    };
    xf_set(e, s, geom::compose(f, b.m));
    if !begun {
        xf_commit(e, s);
    }
}

// ---- symmetry ----

fn set_symmetry(e: &mut Engine, s: &mut State, sym: Sym) {
    s.tooling.symmetry = sym;
    let mode = match sym {
        Sym::Off => None,
        Sym::Vertical => Some(SymmetryMode::Vertical),
        Sym::Horizontal => Some(SymmetryMode::Horizontal),
        Sym::Both => Some(SymmetryMode::Quadrant),
    };
    let (w, h) = (e.painter.width() as f32, e.painter.height() as f32);
    e.painter.set_symmetry(mode.map(|mode| Symmetry {
        mode,
        cx: w / 2.0,
        cy: h / 2.0,
    }));
}

// ---- shapes ----

/// Where a shape's drag ends: with Shift, a line at a multiple of 45°, a square, a circle.
fn shape_end(s: &State, from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
    if !s.tooling.shift {
        return to;
    }
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    match s.tooling.shape_kind {
        ShapeKind::Line => {
            let q = std::f32::consts::FRAC_PI_4;
            let a = (dy.atan2(dx) / q).round() * q;
            let len = dx.hypot(dy);
            (from.0 + len * a.cos(), from.1 + len * a.sin())
        }
        _ => {
            let k = dx.abs().max(dy.abs());
            (from.0 + k.copysign(dx), from.1 + k.copysign(dy))
        }
    }
}

/// The points a shape's stroke goes through, about every 2 px.
pub fn shape_points(kind: ShapeKind, a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    let seg = |p: (f32, f32), q: (f32, f32), out: &mut Vec<(f32, f32)>| {
        let n = ((q.0 - p.0).hypot(q.1 - p.1) / 2.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let t = i as f32 / n as f32;
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    };
    let mut out = Vec::new();
    match kind {
        ShapeKind::Line => seg(a, b, &mut out),
        ShapeKind::Rectangle => {
            let c = [a, (b.0, a.1), b, (a.0, b.1)];
            for i in 0..4 {
                seg(c[i], c[(i + 1) % 4], &mut out);
            }
        }
        ShapeKind::Ellipse => {
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let (rx, ry) = (((b.0 - a.0) / 2.0).abs(), ((b.1 - a.1) / 2.0).abs());
            let around = std::f32::consts::TAU * ((rx * rx + ry * ry) / 2.0).sqrt();
            let n = (around / 2.0).ceil().max(8.0) as usize;
            for i in 0..n {
                let t = i as f32 / n as f32 * std::f32::consts::TAU;
                out.push((cx + rx * t.cos(), cy + ry * t.sin()));
            }
        }
    }
    if kind == ShapeKind::Line {
        out.push(b);
    } else {
        // closed: back to the start, and a little on so the brush meets itself
        let more: Vec<(f32, f32)> = out.iter().take(4).copied().collect();
        out.extend(more);
    }
    out
}

/// Draws a shape with the current brush as one stroke (one undo step).
fn draw_shape(
    e: &mut Engine,
    s: &State,
    kind: ShapeKind,
    a: (f32, f32),
    b: (f32, f32),
    pressure: f32,
) {
    if (b.0 - a.0).hypot(b.1 - a.1) < 1.0 {
        return;
    }
    let Some(mut br) = Engine::brush(s) else {
        return;
    };
    tune_brush(&mut br, s);
    // a shape's line goes exactly where it was dragged: no trailing
    br.get_mut(BrushSetting::SlowTracking).base_value = 0.0;
    let pressure = pressure.clamp(0.05, 1.0);
    e.painter.begin_stroke();
    for p in shape_points(kind, a, b) {
        e.painter
            .stroke_to(&br, p.0, p.1, pressure, 0.0, 0.0, 0.004);
    }
    e.painter.end_stroke(&br);
}

// ---- text ----

/// The text's size from the brush's (document pixels).
fn text_size(s: &State) -> f32 {
    (s.brush_size * 8.0).clamp(16.0, 512.0)
}

fn text_start(e: &mut Engine, s: &mut State, at: (f32, f32)) {
    xf_commit(e, s);
    let base = e.painter.layer_rgba(e.painter.active_layer());
    if base.is_empty() || e.painter.text_begin(e.painter.active_layer(), "") == 0 {
        return;
    }
    let size = text_size(s);
    let lh = (size * 1.25).ceil();
    e.tools.text_base = Some(base);
    s.tooling.text = Some(TextUi {
        x: at.0,
        y: at.1,
        size,
        text: String::new(),
        caret: (0.0, 0.0, lh),
        bounds: [0.0, 0.0, 0.0, lh],
    });
}

/// The typed text shows on the layer (typing isn't an undo step; keeping it is).
fn text_update(e: &mut Engine, s: &mut State) {
    let ([r, g, b], op) = (rgb(s), opacity(s));
    let Some(t) = &mut s.tooling.text else { return };
    let Some(base) = &e.tools.text_base else {
        return;
    };
    let ts = e.tools.typesetter.get_or_insert_with(text::Typesetter::new);
    let d = ts.draw(&t.text, t.size);
    t.caret = d.caret;
    t.bounds = [d.x as f32, d.y as f32, d.w as f32, d.h as f32];
    let (w, h) = (e.painter.width() as i32, e.painter.height() as i32);
    let mut px = base.clone();
    let (ox, oy) = (t.x.round() as i32 + d.x, t.y.round() as i32 + d.y);
    for y in 0..d.h as i32 {
        for x in 0..d.w as i32 {
            let c = d.coverage[(y * d.w as i32 + x) as usize];
            let (cx, cy) = (ox + x, oy + y);
            if c == 0 || cx < 0 || cy < 0 || cx >= w || cy >= h {
                continue;
            }
            let i = ((cy * w + cx) * 4) as usize;
            let a = (c as f32 * op).round() as u8;
            over(&mut px[i..i + 4], &[r, g, b, a]);
        }
    }
    e.painter.text_preview("", 0, 0, w as u32, h as u32, &px);
}

fn text_commit(e: &mut Engine, s: &mut State) {
    let Some(t) = s.tooling.text.take() else {
        return;
    };
    e.tools.text_base = None;
    // nothing typed: the layer stays exactly as it was
    if t.text.trim().is_empty() {
        e.painter.text_cancel();
    } else {
        e.painter.text_commit();
    }
}

fn text_cancel(e: &mut Engine, s: &mut State) {
    if s.tooling.text.take().is_some() {
        e.painter.text_cancel();
    }
    e.tools.text_base = None;
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
