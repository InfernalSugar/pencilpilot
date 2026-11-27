//! Widgets. The interface is described in code each pass; what must last between passes
//! (values, which widget is held, focus, open menus) lives here, keyed by widget ids.
//!
//! **Nothing is drawn unless something changed.**
//! - Input runs a pass that builds the quads. If they're the same as what's on screen, no
//!   frame is sent, so an idle app (or a pointer moving over nothing that reacts) draws
//!   nothing.
//! - Timed things (a tooltip after a hover) say when the next pass is needed
//!   ([`Ui::next_pass_at`]), so the app never polls.
//!
//! **Overlays** (menus, right-click menus, pop-ups, tooltips) draw above everything. Where
//! one was open in the last pass, the pointer belongs to it, not to what's under it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use pencilpilot_gpu::{Quad, Renderer2d};

use crate::text::{Laid, Style, Text};
use crate::theme::{Theme, alpha, hex};

/// A widget's id, from its name.
pub type Id = u64;

pub fn id(name: &str) -> Id {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

/// How long the pointer rests on a widget before its tooltip shows.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
/// How long input must have stopped before the system's fonts join (see [`Ui::pass`]).
pub const FONTS_JOIN_WHEN_CALM: Duration = Duration::from_millis(400);

/// The pointer, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    /// went down / came up since the last pass
    pub pressed: bool,
    pub released: bool,
    /// the secondary (right) button went down since the last pass
    pub context: bool,
    pub inside: bool,
    /// scrolled since the last pass (logical pixels; positive: down)
    pub scroll: f32,
}

/// A key, as the interface needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// the keysym's name: "a", "Escape", "BackSpace", "Left"…
    pub name: String,
    /// what it types
    pub text: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
}

/// What happened to a widget this pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Response {
    pub hovered: bool,
    pub held: bool,
    pub clicked: bool,
    /// right-clicked
    pub context: bool,
    /// its value changed
    pub changed: bool,
}

/// A rectangle, logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// coral: the one main action
    Primary,
    /// quiet fill
    Secondary,
    /// text only, until hovered
    Ghost,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dir {
    Row,
    Column,
}

/// Where the next widget goes: an area filled from its start (and from its end, for
/// widgets placed on the right).
#[derive(Debug, Clone, Copy)]
struct Area {
    rect: Rect,
    dir: Dir,
    gap: f32,
    start: f32,
    end: f32,
    from_end: bool,
}

/// An open menu or pop-up: whose, and where.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    id: Id,
    x: f32,
    y: f32,
}

/// A text field's editing state.
#[derive(Debug, Clone, Default)]
struct Editing {
    caret: usize,
}

/// The interface between passes.
pub struct Ui {
    pub text: Text,
    pub theme: Theme,
    /// logical → device pixels
    pub scale: f32,
    pointer: Pointer,
    keys: Vec<Key>,
    held: Option<Id>,
    focus: Option<Id>,
    editing: Editing,
    values: HashMap<Id, f32>,
    open: Option<Open>,
    /// where overlays were in the last pass: the pointer is theirs there
    blocking: Vec<Rect>,
    hover: Option<(Id, Instant)>,
    icons: HashMap<(String, u32), Option<[f32; 4]>>,
    /// pictures in the atlas by key: their pixels' hash, size and place
    pictures: HashMap<String, Picture>,
    shown: Vec<Quad>,
    next_at: Option<Instant>,
    /// the last input (the system's fonts join only once it's been calm for a while)
    last_input: Instant,
    now: Instant,
}

impl Default for Ui {
    fn default() -> Self {
        Ui::new(1.0)
    }
}

impl Ui {
    pub fn new(scale: f32) -> Ui {
        Ui {
            text: Text::new(),
            theme: Theme::default(),
            scale,
            pointer: Pointer::default(),
            keys: Vec::new(),
            held: None,
            focus: None,
            editing: Editing::default(),
            values: HashMap::new(),
            open: None,
            blocking: Vec::new(),
            hover: None,
            icons: HashMap::new(),
            pictures: HashMap::new(),
            shown: Vec::new(),
            next_at: None,
            last_input: Instant::now(),
            now: Instant::now(),
        }
    }

    /// Input that went elsewhere (a stroke on the canvas): the person is busy, so slow work
    /// (the system's fonts joining) waits.
    pub fn active(&mut self) {
        self.last_input = Instant::now();
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.last_input = Instant::now();
        self.pointer.x = x;
        self.pointer.y = y;
        self.pointer.inside = true;
    }
    pub fn pointer_left(&mut self) {
        self.pointer.inside = false;
    }
    /// The primary button (left; a pen's tip).
    pub fn pointer_button(&mut self, down: bool) {
        self.last_input = Instant::now();
        if down && !self.pointer.down {
            self.pointer.pressed = true;
        }
        if !down && self.pointer.down {
            self.pointer.released = true;
        }
        self.pointer.down = down;
    }
    /// The secondary button (right; a pen's side button) went down.
    pub fn pointer_context(&mut self) {
        self.last_input = Instant::now();
        self.pointer.context = true;
    }
    pub fn scrolled(&mut self, dy: f32) {
        self.last_input = Instant::now();
        self.pointer.scroll += dy;
    }
    pub fn key(&mut self, k: Key) {
        self.last_input = Instant::now();
        self.keys.push(k);
    }

    /// A text field has the keyboard (shortcuts should leave letters alone).
    pub fn typing(&self) -> bool {
        self.focus.is_some()
    }

    /// Is a menu or pop-up open?
    pub fn overlay_open(&self) -> bool {
        self.open.is_some()
    }

    /// A pass: build the interface (logical size `w` × `h`) with `build`. Returns the quads
    /// (device pixels) when they differ from what's on screen, None when nothing would change.
    ///
    /// When the pass acted on input (a press, a release, a key, a scroll), it builds once
    /// more with no new input before returning: a click that changed what an earlier part had
    /// already drawn (the tab picked before, a tool's highlight) then shows complete, in this
    /// frame, whoever drives the passes.
    pub fn pass(
        &mut self,
        r: &mut Renderer2d,
        w: f32,
        h: f32,
        mut build: impl FnMut(&mut Frame),
    ) -> Option<Vec<Quad>> {
        let p = self.pointer;
        let input =
            p.pressed || p.released || p.context || p.scroll != 0.0 || !self.keys.is_empty();
        let first = self.pass_once(r, w, h, &mut build);
        if !input {
            return first;
        }
        self.pass_once(r, w, h, &mut build).or(first)
    }

    fn pass_once(
        &mut self,
        r: &mut Renderer2d,
        w: f32,
        h: f32,
        build: &mut impl FnMut(&mut Frame),
    ) -> Option<Vec<Quad>> {
        // the system's fonts join when it's calm: joining lays every glyph out again, which
        // mustn't land in the middle of a stroke
        let calm = self.held.is_none()
            && !self.pointer.down
            && self.last_input.elapsed() >= FONTS_JOIN_WHEN_CALM;
        if calm && self.text.poll_fonts() {
            // the atlas started over: icons go in again, and the screen is redrawn
            self.icons.clear();
            self.pictures.clear();
            self.shown.clear();
        }
        self.text.new_pass();
        self.now = Instant::now();
        self.next_at = None;
        // Escape closes what's open
        if self.keys.iter().any(|k| k.name == "Escape") {
            if self.open.take().is_some() {
                self.keys.retain(|k| k.name != "Escape");
            } else if self.focus.take().is_some() {
                self.keys.retain(|k| k.name != "Escape");
            }
        }
        let open_before = self.open;
        let mut f = Frame {
            ui: self,
            r,
            quads: Vec::new(),
            overlay: Vec::new(),
            blocking: Vec::new(),
            areas: vec![Area {
                rect: Rect::new(0.0, 0.0, w, h),
                dir: Dir::Column,
                gap: 0.0,
                start: 0.0,
                end: 0.0,
                from_end: false,
            }],
            in_overlay: false,
            hovered_any: None,
        };
        build(&mut f);
        let Frame {
            mut quads,
            overlay,
            blocking,
            hovered_any,
            ..
        } = f;
        quads.extend(overlay);
        // a press outside every overlay closes what was open (and that press reaches nothing)
        let p = self.pointer;
        if (p.pressed || p.context)
            && self.open == open_before
            && open_before.is_some()
            && !self.blocking.iter().any(|b| b.contains(p.x, p.y))
        {
            self.open = None;
            self.shown.clear();
        }
        self.blocking = blocking;
        // hovering: when it started (for tooltips)
        match (hovered_any, self.hover) {
            (Some(id), Some((h, _))) if h == id => {}
            (Some(id), _) => self.hover = Some((id, self.now)),
            (None, _) => self.hover = None,
        }
        let p = &mut self.pointer;
        if p.released {
            self.held = None;
        }
        (p.pressed, p.released, p.context, p.scroll) = (false, false, false, 0.0);
        self.keys.clear();
        if quads == self.shown {
            return None;
        }
        self.shown = quads.clone();
        Some(quads)
    }

    /// What's on screen now (device pixels): to draw again when the window needs a frame
    /// though nothing changed (a resize, a new monitor).
    pub fn shown(&self) -> &[Quad] {
        &self.shown
    }

    /// When the next pass is due even without input (a tooltip about to show), if ever.
    pub fn next_pass_at(&self) -> Option<Instant> {
        // while the system's fonts load: a look when it's calm, then every quarter second
        let fonts = self.text.fonts_loading().then(|| {
            (self.last_input + FONTS_JOIN_WHEN_CALM).max(self.now + Duration::from_millis(250))
        });
        match (self.next_at, fonts) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// A picture in the atlas: its pixels' hash, its size, and its place (texture coordinates).
type Picture = (u64, u32, u32, Option<[f32; 4]>);

/// One pass: widgets are placed and drawn into it.
pub struct Frame<'a> {
    ui: &'a mut Ui,
    r: &'a mut Renderer2d,
    quads: Vec<Quad>,
    overlay: Vec<Quad>,
    blocking: Vec<Rect>,
    areas: Vec<Area>,
    in_overlay: bool,
    hovered_any: Option<Id>,
}

impl Frame<'_> {
    pub fn theme(&self) -> Theme {
        self.ui.theme
    }

    /// How many quads are drawn so far (overlays aside): to draw something else (the
    /// canvas) between those and what comes after.
    pub fn mark(&self) -> usize {
        self.quads.len()
    }

    /// The pointer, as this pass sees it.
    pub fn pointer(&self) -> Pointer {
        self.ui.pointer
    }

    /// The keys pressed since the last pass that nothing has taken yet (a focused text
    /// field takes the ones it types with).
    pub fn keys(&self) -> &[Key] {
        &self.ui.keys
    }

    /// Logical → device pixels (for clipping in device pixels, say).
    pub fn scale(&self) -> f32 {
        self.ui.scale
    }

    /// A text field has the keyboard (shortcuts should leave letters alone).
    pub fn typing(&self) -> bool {
        self.ui.focus.is_some()
    }

    /// Take a key pressed since the last pass, by name (case aside), with or without Ctrl:
    /// a shortcut handled here, so nothing else acts on it. True if it was pressed.
    pub fn take_key(&mut self, name: &str, ctrl: bool) -> bool {
        let i = self
            .ui
            .keys
            .iter()
            .position(|k| k.name.eq_ignore_ascii_case(name) && k.ctrl == ctrl);
        i.map(|i| self.ui.keys.remove(i)).is_some()
    }

    /// Draw with `build` above everything (as menus and pop-ups are). Mark the area it
    /// covers with [`Frame::block`] so the pointer there is its.
    pub fn overlay(&mut self, build: impl FnOnce(&mut Self)) {
        let was = self.in_overlay;
        self.in_overlay = true;
        build(self);
        self.in_overlay = was;
    }

    /// The pointer over `rect` belongs to what's drawn there in the overlay.
    pub fn block(&mut self, rect: Rect) {
        self.blocking.push(rect);
    }

    fn s(&self) -> f32 {
        self.ui.scale
    }

    /// Draw a quad given in logical pixels (above what's drawn so far; in an overlay, above
    /// everything).
    pub fn put(&mut self, q: Quad) {
        let q = q.scaled(self.s());
        if self.in_overlay {
            self.overlay.push(q)
        } else {
            self.quads.push(q)
        }
    }

    /// Lay text out (logical sizes) for measuring and drawing.
    pub fn lay_out(&mut self, text: &str, style: &Style, max_width: Option<f32>) -> Laid {
        let s = self.s();
        let mut st = *style;
        st.size *= s;
        let laid = self
            .ui
            .text
            .lay_out(self.r, text, &st, max_width.map(|w| w * s));
        Laid {
            width: laid.width / s,
            height: laid.height / s,
            quads: laid.quads,
        }
    }

    /// Draw laid-out text with its top-left at (x, y), logical pixels.
    pub fn put_text(&mut self, laid: &Laid, x: f32, y: f32) {
        let s = self.s();
        let (dx, dy) = ((x * s).round(), (y * s).round());
        let qs = laid.quads.iter().map(|q| q.moved(dx, dy));
        if self.in_overlay {
            self.overlay.extend(qs)
        } else {
            self.quads.extend(qs)
        }
    }

    // ---- layout ----

    /// Space for a widget `w` × `h` in the current area (a column: full width when `w` is
    /// 0; a row: full height when `h` is 0).
    pub fn alloc(&mut self, w: f32, h: f32) -> Rect {
        let a = self.areas.last_mut().expect("an area");
        let r = a.rect;
        match a.dir {
            Dir::Column => {
                let w = if w <= 0.0 { r.w } else { w };
                let y = if a.from_end {
                    r.y + r.h - a.end - h
                } else {
                    r.y + a.start
                };
                if a.from_end {
                    a.end += h + a.gap
                } else {
                    a.start += h + a.gap
                }
                Rect::new(r.x, y, w, h)
            }
            Dir::Row => {
                let h = if h <= 0.0 { r.h } else { h };
                let x = if a.from_end {
                    r.x + r.w - a.end - w
                } else {
                    r.x + a.start
                };
                if a.from_end {
                    a.end += w + a.gap
                } else {
                    a.start += w + a.gap
                }
                Rect::new(x, r.y + (r.h - h) / 2.0, w, h)
            }
        }
    }

    /// What's left of the current area.
    pub fn remaining(&self) -> Rect {
        let a = self.areas.last().expect("an area");
        match a.dir {
            Dir::Column => Rect::new(
                a.rect.x,
                a.rect.y + a.start,
                a.rect.w,
                (a.rect.h - a.start - a.end).max(0.0),
            ),
            Dir::Row => Rect::new(
                a.rect.x + a.start,
                a.rect.y,
                (a.rect.w - a.start - a.end).max(0.0),
                a.rect.h,
            ),
        }
    }

    fn within(&mut self, rect: Rect, dir: Dir, gap: f32, build: impl FnOnce(&mut Self)) {
        self.areas.push(Area {
            rect,
            dir,
            gap,
            start: 0.0,
            end: 0.0,
            from_end: false,
        });
        build(self);
        self.areas.pop();
    }

    /// Lay widgets out in `rect` in a column (`gap` apart).
    pub fn column_in(&mut self, rect: Rect, gap: f32, build: impl FnOnce(&mut Self)) {
        self.within(rect, Dir::Column, gap, build);
    }

    /// Lay widgets out in `rect` in a row (`gap` apart).
    pub fn row_in(&mut self, rect: Rect, gap: f32, build: impl FnOnce(&mut Self)) {
        self.within(rect, Dir::Row, gap, build);
    }

    /// A row `h` tall; its widgets go left to right (after [`Frame::then_right`], right to
    /// left from the end).
    pub fn row(&mut self, h: f32, build: impl FnOnce(&mut Self)) {
        let rect = self.alloc(0.0, h);
        let gap = self.ui.theme.gap;
        self.within(rect, Dir::Row, gap, build);
    }

    /// From here on in this area, widgets are placed from its end.
    pub fn then_right(&mut self) {
        if let Some(a) = self.areas.last_mut() {
            a.from_end = true;
        }
    }

    /// A white card with the design's corners and soft shadow; its content in a column.
    pub fn card(&mut self, rect: Rect, build: impl FnOnce(&mut Self)) {
        let t = self.ui.theme;
        self.put(
            Quad::rect(
                rect.x + 2.0,
                rect.y + 6.0,
                rect.w,
                rect.h,
                alpha(t.ink, 0.08),
            )
            .radius(t.card_radius)
            .shadow(14.0),
        );
        self.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, t.card).radius(t.card_radius));
        let inner = rect.inset(t.pad);
        self.within(inner, Dir::Column, t.gap * 1.5, build);
    }

    // ---- interaction ----

    fn pointer_free(&self) -> bool {
        let p = self.ui.pointer;
        self.in_overlay || !self.ui.blocking.iter().any(|b| b.contains(p.x, p.y))
    }

    /// Hover, press, click and right-click on `rect` for the widget `id` (custom widgets).
    pub fn interact(&mut self, id: Id, rect: Rect) -> Response {
        let p = self.ui.pointer;
        let over = p.inside && rect.contains(p.x, p.y) && self.pointer_free();
        if over && p.pressed {
            self.ui.held = Some(id);
        }
        if over {
            self.hovered_any = Some(id);
        }
        let held = self.ui.held == Some(id);
        Response {
            hovered: over,
            held: held && p.down,
            clicked: held && p.released && over,
            context: over && p.context,
            changed: false,
        }
    }

    // ---- widgets ----

    pub fn label(&mut self, text: &str) {
        let t = self.ui.theme;
        let laid = self.lay_out(text, &Style::body(t.text, t.ink), None);
        let r = self.alloc(laid.width, laid.height);
        self.put_text(&laid, r.x, r.y);
    }

    pub fn heading(&mut self, text: &str) {
        let t = self.ui.theme;
        let laid = self.lay_out(text, &Style::display(t.heading, t.ink), None);
        let r = self.alloc(laid.width, laid.height);
        self.put_text(&laid, r.x, r.y);
    }

    pub fn button(&mut self, name: &str, text: &str, kind: Kind) -> Response {
        let t = self.ui.theme;
        let fg = if kind == Kind::Primary {
            t.on_coral
        } else {
            t.ink
        };
        let laid = self.lay_out(text, &Style::body(t.text, fg).weight(600), None);
        let rect = self.alloc(laid.width + 2.0 * 18.0, t.control_height);
        let res = self.interact(id(name), rect);
        let base = match kind {
            Kind::Primary => t.coral,
            Kind::Secondary => t.line,
            Kind::Ghost => alpha(t.line, 0.0),
        };
        let fill = if res.held {
            mix(base, t.ink, 0.18)
        } else if res.hovered {
            if kind == Kind::Ghost {
                t.line
            } else {
                mix(base, t.card, 0.12)
            }
        } else {
            base
        };
        self.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, fill).radius(t.control_height / 2.0));
        self.put_text(
            &laid,
            rect.x + (rect.w - laid.width) / 2.0,
            rect.y + (rect.h - laid.height) / 2.0,
        );
        res
    }

    /// An icon (an SVG path in a 24 × 24 box, drawn as a 2 px line, as the design's icons)
    /// at `size`, in `color`.
    pub fn icon(&mut self, path: &str, x: f32, y: f32, size: f32, color: [f32; 4]) {
        self.icon_stroked(path, x, y, size, color, 2.0);
    }

    /// An icon with its line `stroke` wide (in the 24 × 24 box; the design's are 2 to 2.2).
    pub fn icon_stroked(
        &mut self,
        path: &str,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
        stroke: f32,
    ) {
        let px = (size * self.s()).round().max(1.0) as u32;
        let key = (format!("{path}|{stroke}"), px);
        let uv = match self.ui.icons.get(&key) {
            Some(uv) => *uv,
            None => {
                let uv = raster_icon(path, px, stroke)
                    .and_then(|rgba| self.ui.text.place(self.r, px, px, &rgba));
                self.ui.icons.insert(key, uv);
                uv
            }
        };
        if let Some(uv) = uv {
            let s = self.s();
            let q = Quad::glyph(
                (x * s).round(),
                (y * s).round(),
                px as f32,
                px as f32,
                uv,
                color,
            );
            if self.in_overlay {
                self.overlay.push(q)
            } else {
                self.quads.push(q)
            }
        }
    }

    /// A small picture (straight RGBA8, `w` × `h`) shown in `r`, one quad: it's kept in the
    /// atlas under `key` and put there again only when its pixels change (in the same place
    /// while its size doesn't).
    pub fn picture(&mut self, key: &str, w: u32, h: u32, rgba: &[u8], r: Rect) {
        if w == 0 || h == 0 || rgba.len() != (w * h * 4) as usize {
            return;
        }
        // FNV-1a over the pixels: cheap enough for thumbnails every pass
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for c in rgba.chunks(8) {
            let mut v = 0u64;
            for (i, b) in c.iter().enumerate() {
                v |= (*b as u64) << (i * 8);
            }
            hash = (hash ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let premul = || -> Vec<u8> {
            rgba.chunks(4)
                .flat_map(|p| {
                    let a = p[3] as u32;
                    [
                        (p[0] as u32 * a / 255) as u8,
                        (p[1] as u32 * a / 255) as u8,
                        (p[2] as u32 * a / 255) as u8,
                        p[3],
                    ]
                })
                .collect()
        };
        let uv = match self.ui.pictures.get(key) {
            Some(&(h0, _, _, uv)) if h0 == hash => uv,
            Some(&(_, w0, h1, Some(uv))) if (w0, h1) == (w, h) => {
                // the same place, new pixels
                let n = pencilpilot_gpu::ATLAS_SIZE as f32;
                let (x, y) = ((uv[0] * n).round() as u32, (uv[1] * n).round() as u32);
                self.r.atlas_put(x, y, w, h, &premul());
                self.ui
                    .pictures
                    .insert(key.to_string(), (hash, w, h, Some(uv)));
                Some(uv)
            }
            _ => {
                let uv = self.ui.text.place(self.r, w, h, &premul());
                self.ui.pictures.insert(key.to_string(), (hash, w, h, uv));
                uv
            }
        };
        if let Some(uv) = uv {
            self.put(Quad::image(r.x, r.y, r.w, r.h, uv, 1.0));
        }
    }

    /// A square icon button (`size` logical px); `on` shows it picked. Its `label` shows as
    /// a tooltip after a moment's hover.
    pub fn icon_button(
        &mut self,
        name: &str,
        path: &str,
        label: &str,
        on: bool,
        dark: bool,
    ) -> Response {
        let t = self.ui.theme;
        let size = 34.0;
        let rect = self.alloc(size, size);
        let res = self.interact(id(name), rect);
        let (bg, fg) = match (dark, on) {
            (true, true) => (t.on_dark, t.ink),
            (true, false) => (
                if res.hovered {
                    mix(t.dark_raised, t.on_dark, 0.12)
                } else {
                    t.dark_raised
                },
                t.on_dark,
            ),
            (false, true) => (t.ink, t.card),
            (false, false) => (
                if res.hovered {
                    t.line
                } else {
                    alpha(t.line, 0.0)
                },
                t.ink,
            ),
        };
        self.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, bg).radius(9.0));
        self.icon(path, rect.x + 8.0, rect.y + 8.0, 18.0, fg);
        if res.hovered {
            self.tooltip(id(name), label, rect);
        }
        res
    }

    /// A tooltip under `anchor`, once the pointer has rested on `owner` a moment.
    pub fn tooltip(&mut self, owner: Id, text: &str, anchor: Rect) {
        let Some((h, since)) = self.ui.hover else {
            // the hover starts this pass: the tooltip is due after the delay
            self.ui.next_at = Some(self.ui.now + TOOLTIP_DELAY);
            return;
        };
        if h != owner {
            self.ui.next_at = Some(self.ui.now + TOOLTIP_DELAY);
            return;
        }
        let due = since + TOOLTIP_DELAY;
        if self.ui.now < due {
            self.ui.next_at = Some(due);
            return;
        }
        let t = self.ui.theme;
        let was = self.in_overlay;
        self.in_overlay = true;
        let laid = self.lay_out(text, &Style::body(t.small, t.on_dark).weight(500), None);
        let (w, hh) = (laid.width + 16.0, laid.height + 10.0);
        let x = anchor.x + (anchor.w - w) / 2.0;
        // under its anchor, or above it where there's no room below
        let root = self.areas[0].rect;
        let below = anchor.bottom() + 6.0;
        let y = if below + hh > root.bottom() - 4.0 {
            anchor.y - 6.0 - hh
        } else {
            below
        };
        let x = x.clamp(root.x + 4.0, (root.right() - w - 4.0).max(root.x + 4.0));
        self.put(Quad::rect(x, y, w, hh, t.ink).radius(8.0));
        self.put_text(&laid, x + 8.0, y + 5.0);
        self.in_overlay = was;
    }

    /// A switch with its label; clicking anywhere on it flips `on`.
    pub fn toggle(&mut self, name: &str, text: &str, on: &mut bool) -> Response {
        let t = self.ui.theme;
        let laid = self.lay_out(text, &Style::body(t.text, t.ink), None);
        let (tw, th) = (44.0f32, 26.0f32);
        let rect = self.alloc(tw + 10.0 + laid.width, th.max(laid.height));
        let mut res = self.interact(id(name), rect);
        if res.clicked {
            *on = !*on;
            res.changed = true;
        }
        let track = if *on { t.coral } else { t.line };
        let ty = rect.y + (rect.h - th) / 2.0;
        self.put(Quad::rect(rect.x, ty, tw, th, track).radius(th / 2.0));
        let knob = th - 6.0;
        let kx = if *on {
            rect.x + tw - knob - 3.0
        } else {
            rect.x + 3.0
        };
        self.put(Quad::rect(kx, ty + 3.0, knob, knob, t.card).radius(knob / 2.0));
        self.put_text(
            &laid,
            rect.x + tw + 10.0,
            rect.y + (rect.h - laid.height) / 2.0,
        );
        res
    }

    /// A horizontal slider, `w` wide; dragging sets `value` between `min` and `max`.
    pub fn slider(&mut self, name: &str, value: &mut f32, min: f32, max: f32, w: f32) -> Response {
        let t = self.ui.theme;
        let rect = self.alloc(w, 28.0);
        let mut res = self.interact(id(name), rect);
        if res.held {
            let u = ((self.ui.pointer.x - rect.x) / rect.w).clamp(0.0, 1.0);
            let v = min + u * (max - min);
            if v != *value {
                *value = v;
                res.changed = true;
            }
        }
        let u = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let cy = rect.y + rect.h / 2.0;
        self.put(Quad::rect(rect.x, cy - 2.0, rect.w, 4.0, t.line).radius(2.0));
        self.put(Quad::rect(rect.x, cy - 2.0, rect.w * u, 4.0, t.ink).radius(2.0));
        let k = if res.held { 20.0 } else { 18.0 };
        self.put(
            Quad::rect(rect.x + rect.w * u - k / 2.0, cy - k / 2.0, k, k, t.card)
                .radius(k / 2.0)
                .border(2.0, t.ink),
        );
        res
    }

    /// Segmented tabs; clicking one picks it.
    pub fn tabs(&mut self, name: &str, items: &[&str], picked: &mut usize) -> Response {
        let t = self.ui.theme;
        let laid: Vec<Laid> = items
            .iter()
            .map(|s| self.lay_out(s, &Style::body(t.text, t.ink).weight(600), None))
            .collect();
        let widths: Vec<f32> = laid.iter().map(|l| l.width + 32.0).collect();
        let total: f32 = widths.iter().sum::<f32>() + 8.0;
        let rect = self.alloc(total, t.control_height);
        self.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, t.line).radius(rect.h / 2.0));
        let mut res = Response::default();
        let mut x = rect.x + 4.0;
        for (i, (l, w)) in laid.iter().zip(&widths).enumerate() {
            let tab = Rect::new(x, rect.y + 4.0, *w, rect.h - 8.0);
            let r = self.interact(id(&format!("{name}/{i}")), tab);
            res.hovered |= r.hovered;
            if r.clicked && *picked != i {
                *picked = i;
                res.changed = true;
                res.clicked = true;
            }
            if *picked == i {
                self.put(Quad::rect(tab.x, tab.y, tab.w, tab.h, t.card).radius(tab.h / 2.0));
            } else if r.hovered {
                self.put(
                    Quad::rect(tab.x, tab.y, tab.w, tab.h, alpha(t.card, 0.5)).radius(tab.h / 2.0),
                );
            }
            self.put_text(
                l,
                tab.x + (tab.w - l.width) / 2.0,
                tab.y + (tab.h - l.height) / 2.0,
            );
            x += w;
        }
        res
    }

    /// A one-line text field, `w` wide. Clicking it focuses it; typing edits `value`
    /// (Backspace, Delete, Left, Right, Home, End; Escape or a click elsewhere leaves it).
    /// The caret doesn't blink, so a focused field draws nothing while idle.
    pub fn text_field(
        &mut self,
        name: &str,
        value: &mut String,
        placeholder: &str,
        w: f32,
    ) -> Response {
        let t = self.ui.theme;
        let me = id(name);
        let rect = self.alloc(w, t.control_height);
        let mut res = self.interact(me, rect);
        let p = self.ui.pointer;
        if res.clicked || (res.hovered && p.pressed) {
            if self.ui.focus != Some(me) {
                self.ui.editing.caret = value.len();
            }
            self.ui.focus = Some(me);
        } else if p.pressed
            && self.pointer_free()
            && !rect.contains(p.x, p.y)
            && self.ui.focus == Some(me)
        {
            self.ui.focus = None;
        }
        let focused = self.ui.focus == Some(me);
        if focused {
            let keys = std::mem::take(&mut self.ui.keys);
            let mut caret = self.ui.editing.caret.min(value.len());
            for k in &keys {
                match k.name.as_str() {
                    "BackSpace" => {
                        if let Some((i, _)) = value[..caret].char_indices().next_back() {
                            value.replace_range(i..caret, "");
                            caret = i;
                            res.changed = true;
                        }
                    }
                    "Delete" => {
                        if let Some(c) = value[caret..].chars().next() {
                            value.replace_range(caret..caret + c.len_utf8(), "");
                            res.changed = true;
                        }
                    }
                    "Left" => {
                        caret = value[..caret]
                            .char_indices()
                            .next_back()
                            .map_or(0, |(i, _)| i)
                    }
                    "Right" => {
                        caret = value[caret..]
                            .chars()
                            .next()
                            .map_or(caret, |c| caret + c.len_utf8())
                    }
                    "Home" => caret = 0,
                    "End" => caret = value.len(),
                    _ => {
                        if let Some(text) = &k.text
                            && !k.ctrl
                        {
                            value.insert_str(caret, text);
                            caret += text.len();
                            res.changed = true;
                        }
                    }
                }
            }
            self.ui.editing.caret = caret;
            // keys the field didn't use stay for others (Escape was handled before the pass)
            self.ui.keys = keys
                .into_iter()
                .filter(|k| {
                    k.text.is_none()
                        && !matches!(
                            k.name.as_str(),
                            "BackSpace" | "Delete" | "Left" | "Right" | "Home" | "End"
                        )
                })
                .collect();
        }
        let border = if focused {
            t.ink
        } else if res.hovered {
            t.muted
        } else {
            t.line
        };
        self.put(
            Quad::rect(rect.x, rect.y, rect.w, rect.h, t.card)
                .radius(t.control_radius)
                .border(if focused { 2.0 } else { 1.5 }, border),
        );
        let pad = 14.0;
        let (shown, color) = if value.is_empty() {
            (placeholder, t.muted)
        } else {
            (value.as_str(), t.ink)
        };
        let laid = self.lay_out(shown, &Style::body(t.text, color), None);
        let ty = rect.y + (rect.h - laid.height) / 2.0;
        // long text stays inside the field
        let start = if self.in_overlay {
            self.overlay.len()
        } else {
            self.quads.len()
        };
        self.put_text(&laid, rect.x + pad, ty);
        let sc = self.s();
        let clip = [
            (rect.x + pad / 2.0) * sc,
            rect.y * sc,
            (rect.right() - pad / 2.0) * sc,
            rect.bottom() * sc,
        ];
        let list = if self.in_overlay {
            &mut self.overlay
        } else {
            &mut self.quads
        };
        for q in &mut list[start..] {
            *q = q.clip(clip);
        }
        if focused {
            let before = if value.is_empty() {
                0.0
            } else {
                self.lay_out(
                    &value[..self.ui.editing.caret.min(value.len())],
                    &Style::body(t.text, t.ink),
                    None,
                )
                .width
            };
            self.put(
                Quad::rect(
                    rect.x + pad + before,
                    ty + 2.0,
                    2.0,
                    laid.height.max(t.text) - 4.0,
                    t.coral,
                )
                .radius(1.0),
            );
        }
        res
    }

    /// A scrolling area `h` tall: `build` lays out content `content_h` tall inside it,
    /// moved by the scroll and clipped to the area. The wheel scrolls it.
    pub fn scroll_area(
        &mut self,
        name: &str,
        h: f32,
        content_h: f32,
        build: impl FnOnce(&mut Self),
    ) {
        let t = self.ui.theme;
        let rect = self.alloc(0.0, h);
        let key = format!("{name}/scroll");
        let max = (content_h - h).max(0.0);
        let mut off = self.value(&key, 0.0).clamp(0.0, max);
        let p = self.ui.pointer;
        if p.scroll != 0.0 && rect.contains(p.x, p.y) && self.pointer_free() {
            off = (off + p.scroll).clamp(0.0, max);
        }
        self.set_value(&key, off);
        let (q0, o0) = (self.quads.len(), self.overlay.len());
        let inner = Rect::new(rect.x, rect.y - off, rect.w - 10.0, content_h.max(h));
        // the pointer reaches only what's inside the visible part
        let saved = self.ui.pointer;
        if !rect.contains(p.x, p.y) {
            self.ui.pointer.inside = false;
        }
        self.within(inner, Dir::Column, t.gap, build);
        self.ui.pointer.inside = saved.inside;
        let s = self.s();
        let clip = [rect.x * s, rect.y * s, rect.right() * s, rect.bottom() * s];
        for q in &mut self.quads[q0..] {
            *q = q.clip(clip);
        }
        let _ = o0;
        if max > 0.0 {
            let bar_h = (h * h / content_h).max(24.0);
            let y = rect.y + (h - bar_h) * off / max;
            self.put(Quad::rect(rect.right() - 6.0, y, 4.0, bar_h, alpha(t.ink, 0.25)).radius(2.0));
        }
    }

    // ---- menus and pop-ups ----

    /// Is the menu or pop-up called `name` open?
    pub fn is_open(&self, name: &str) -> bool {
        self.ui.open.is_some_and(|o| o.id == id(name))
    }

    /// Open the menu or pop-up `name` at (x, y) (logical), closing any other.
    pub fn open(&mut self, name: &str, x: f32, y: f32) {
        self.ui.open = Some(Open { id: id(name), x, y });
    }

    pub fn close(&mut self) {
        self.ui.open = None;
    }

    /// A right-click (or a pen's side button) over `area` opens the menu `name` there.
    pub fn context_area(&mut self, name: &str, area: Rect) {
        let p = self.ui.pointer;
        if p.context && area.contains(p.x, p.y) && self.pointer_free() {
            self.open(name, p.x, p.y);
        }
    }

    /// The menu `name`, if open: `build` adds its items. Dark, as the design's menus; an
    /// item clicked closes it, and its text is returned.
    pub fn menu(&mut self, name: &str, build: impl FnOnce(&mut Menu)) -> Option<String> {
        let o = self.ui.open.filter(|o| o.id == id(name))?;
        let mut m = Menu { items: Vec::new() };
        build(&mut m);
        let t = self.ui.theme;
        let was = self.in_overlay;
        self.in_overlay = true;
        let pad = 6.0;
        // measure
        let laid: Vec<(Laid, Option<Laid>)> = m
            .items
            .iter()
            .map(|it| match it {
                Item::Head(s) => (
                    self.lay_out(
                        &s.to_uppercase(),
                        &Style::body(11.0, t.on_dark_muted).weight(600),
                        None,
                    ),
                    None,
                ),
                Item::Entry {
                    text, hint, danger, ..
                } => (
                    self.lay_out(
                        text,
                        &Style::body(13.0, if *danger { hex(0xFF7A66) } else { t.on_dark })
                            .weight(500),
                        None,
                    ),
                    (!hint.is_empty())
                        .then(|| self.lay_out(hint, &Style::body(12.0, t.on_dark_muted), None)),
                ),
                Item::Sep => (Laid::default(), None),
            })
            .collect();
        let w = laid
            .iter()
            .map(|(a, b)| a.width + b.as_ref().map_or(0.0, |b| b.width + 24.0))
            .fold(160.0f32, f32::max)
            + 20.0
            + 2.0 * pad;
        let heights: Vec<f32> = m
            .items
            .iter()
            .map(|it| match it {
                Item::Head(_) => 26.0,
                Item::Entry { .. } => 32.0,
                Item::Sep => 9.0,
            })
            .collect();
        let h = heights.iter().sum::<f32>() + 2.0 * pad;
        // inside the window: moved left or up where it would run off
        let root = self.areas[0].rect;
        let x = o.x.min(root.right() - w - 8.0).max(root.x + 8.0);
        let y = o.y.min(root.bottom() - h - 8.0).max(root.y + 8.0);
        let rect = Rect::new(x, y, w, h);
        self.blocking.push(rect);
        self.put(
            Quad::rect(rect.x, rect.y + 8.0, w, h, alpha(t.ink, 0.18))
                .radius(16.0)
                .shadow(18.0),
        );
        self.put(
            Quad::rect(rect.x, rect.y, w, h, hex(0x1E1E24))
                .radius(16.0)
                .border(1.0, hex(0x34343C)),
        );
        let mut y = rect.y + pad;
        let mut chosen = None;
        for (i, ((it, (l, hint)), ih)) in m.items.iter().zip(&laid).zip(&heights).enumerate() {
            let row = Rect::new(rect.x + pad, y, w - 2.0 * pad, *ih);
            match it {
                Item::Head(_) => self.put_text(l, row.x + 10.0, row.y + 8.0),
                Item::Sep => self.put(Quad::rect(
                    row.x + 8.0,
                    row.y + 4.0,
                    row.w - 16.0,
                    1.0,
                    hex(0x34343C),
                )),
                Item::Entry { .. } => {
                    let r = self.interact(id(&format!("{name}/item/{i}")), row);
                    if r.hovered {
                        self.put(Quad::rect(row.x, row.y, row.w, row.h, hex(0x2E2E36)).radius(9.0));
                    }
                    self.put_text(l, row.x + 10.0, row.y + (row.h - l.height) / 2.0);
                    if let Some(hint) = hint {
                        self.put_text(
                            hint,
                            row.right() - 10.0 - hint.width,
                            row.y + (row.h - hint.height) / 2.0,
                        );
                    }
                    if r.clicked {
                        chosen = Some(i);
                    }
                }
            }
            y += ih;
        }
        self.in_overlay = was;
        let i = chosen?;
        self.close();
        match &m.items[i] {
            Item::Entry { text, picked, .. } => {
                picked.set(true);
                Some(text.clone())
            }
            _ => None,
        }
    }

    /// A pop-up card at `rect` (logical) if `name` is open, with the corners, padding and
    /// border given (a border of 0: none). Its content goes in a column.
    pub fn popup_at(
        &mut self,
        name: &str,
        rect: Rect,
        radius: f32,
        pad: f32,
        border: f32,
        build: impl FnOnce(&mut Self),
    ) {
        if !self.is_open(name) {
            return;
        }
        let t = self.ui.theme;
        let was = self.in_overlay;
        self.in_overlay = true;
        self.blocking.push(rect);
        self.put(
            Quad::rect(
                rect.x + 2.0,
                rect.y + 8.0,
                rect.w,
                rect.h,
                alpha(t.ink, 0.14),
            )
            .radius(radius)
            .shadow(18.0),
        );
        let card = Quad::rect(rect.x, rect.y, rect.w, rect.h, t.card).radius(radius);
        self.put(if border > 0.0 {
            card.border(border, t.line)
        } else {
            card
        });
        let inner = rect.inset(pad);
        self.within(inner, Dir::Column, t.gap * 1.5, build);
        self.in_overlay = was;
    }

    /// A pop-up card under `anchor` (logical), `w` × `h`, if `name` is open.
    pub fn popup(
        &mut self,
        name: &str,
        anchor: Rect,
        w: f32,
        h: f32,
        build: impl FnOnce(&mut Self),
    ) {
        if !self.is_open(name) {
            return;
        }
        let t = self.ui.theme;
        let was = self.in_overlay;
        self.in_overlay = true;
        let rect = Rect::new(anchor.x, anchor.bottom() + 8.0, w, h);
        self.blocking.push(rect);
        self.put(
            Quad::rect(rect.x + 2.0, rect.y + 8.0, w, h, alpha(t.ink, 0.14))
                .radius(t.card_radius)
                .shadow(18.0),
        );
        self.put(Quad::rect(rect.x, rect.y, w, h, t.card).radius(t.card_radius));
        let inner = rect.inset(t.pad);
        self.within(inner, Dir::Column, t.gap * 1.5, build);
        self.in_overlay = was;
    }

    /// A value kept between passes (for widgets the app doesn't hold state for).
    pub fn value(&mut self, name: &str, default: f32) -> f32 {
        *self.ui.values.entry(id(name)).or_insert(default)
    }
    pub fn set_value(&mut self, name: &str, v: f32) {
        self.ui.values.insert(id(name), v);
    }
}

/// A menu's items, added in order.
pub struct Menu {
    items: Vec<Item>,
}

enum Item {
    Head(String),
    Entry {
        text: String,
        hint: String,
        danger: bool,
        picked: std::rc::Rc<std::cell::Cell<bool>>,
    },
    Sep,
}

/// Whether a menu item was picked (read after the menu is built).
#[derive(Clone)]
pub struct Picked(std::rc::Rc<std::cell::Cell<bool>>);

impl Picked {
    pub fn get(&self) -> bool {
        self.0.get()
    }
}

impl Menu {
    pub fn head(&mut self, text: &str) {
        self.items.push(Item::Head(text.into()));
    }
    pub fn separator(&mut self) {
        self.items.push(Item::Sep);
    }
    /// An item (with a hint on its right: a shortcut, a ✓, a current value). `Picked`
    /// says whether it was clicked, once `menu` has returned (not while building it).
    pub fn item(&mut self, text: &str, hint: &str) -> Picked {
        let p = std::rc::Rc::new(std::cell::Cell::new(false));
        self.items.push(Item::Entry {
            text: text.into(),
            hint: hint.into(),
            danger: false,
            picked: p.clone(),
        });
        Picked(p)
    }

    /// An item that destroys something (Delete…), in red.
    pub fn danger(&mut self, text: &str, hint: &str) -> Picked {
        let p = std::rc::Rc::new(std::cell::Cell::new(false));
        self.items.push(Item::Entry {
            text: text.into(),
            hint: hint.into(),
            danger: true,
            picked: p.clone(),
        });
        Picked(p)
    }
}

/// An icon path (24 × 24 box) as a white mask, `px` square, 2 px lines with round ends.
fn raster_icon(path: &str, px: u32, stroke: f32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{path}" fill="none" stroke="#fff" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round"/></svg>"##
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let k = px as f32 / 24.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    // white, premultiplied: every channel is the coverage
    Some(
        pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[3]; 4])
            .collect(),
    )
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}
