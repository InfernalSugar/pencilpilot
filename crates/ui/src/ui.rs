//! Widgets. The interface is described in code each pass; what must last between passes
//! (values, which widget is held, focus) lives here, keyed by widget ids.
//!
//! **Nothing is drawn unless something changed.** Input runs a pass that builds the quads.
//! If they're the same as what's on screen, no frame is sent, so an idle app (or a pointer
//! moving over nothing that reacts) draws nothing.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use pencilpilot_gpu::{Quad, Renderer2d};

use crate::text::{Laid, Style, Text};
use crate::theme::{Theme, alpha};

/// A widget's id: from its name (and its parent's, for widgets made in loops).
pub type Id = u64;

pub fn id(name: &str) -> Id {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

/// The pointer, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    /// went down / came up since the last pass
    pub pressed: bool,
    pub released: bool,
    pub inside: bool,
}

/// What happened to a widget this pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Response {
    pub hovered: bool,
    pub held: bool,
    pub clicked: bool,
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

/// The interface between passes.
pub struct Ui {
    pub text: Text,
    pub theme: Theme,
    /// logical → device pixels
    pub scale: f32,
    pointer: Pointer,
    held: Option<Id>,
    values: HashMap<Id, f32>,
    shown: Vec<Quad>,
    /// a widget asked for another pass (an animation)
    again: bool,
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
            held: None,
            values: HashMap::new(),
            shown: Vec::new(),
            again: false,
        }
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.pointer.x = x;
        self.pointer.y = y;
        self.pointer.inside = true;
    }
    pub fn pointer_left(&mut self) {
        self.pointer.inside = false;
    }
    pub fn pointer_button(&mut self, down: bool) {
        if down && !self.pointer.down {
            self.pointer.pressed = true;
        }
        if !down && self.pointer.down {
            self.pointer.released = true;
        }
        self.pointer.down = down;
    }

    /// A pass: build the interface (logical size `w` × `h`) with `build`. Returns the quads
    /// (device pixels) when they differ from what's on screen, None when nothing would change.
    pub fn pass(
        &mut self,
        r: &mut Renderer2d,
        w: f32,
        h: f32,
        build: impl FnOnce(&mut Frame),
    ) -> Option<Vec<Quad>> {
        if self.text.poll_fonts() {
            // new glyphs may now exist: the next pass draws whatever it builds
            self.shown.clear();
        }
        self.again = false;
        let mut f = Frame {
            ui: self,
            r,
            quads: Vec::new(),
            areas: vec![Area {
                rect: Rect::new(0.0, 0.0, w, h),
                dir: Dir::Column,
                gap: 0.0,
                start: 0.0,
                end: 0.0,
                from_end: false,
            }],
        };
        build(&mut f);
        let quads = f.quads;
        let p = &mut self.pointer;
        if p.released {
            self.held = None;
        }
        (p.pressed, p.released) = (false, false);
        if quads == self.shown && !self.again {
            return None;
        }
        self.shown = quads.clone();
        Some(quads)
    }

    /// Something asked for another pass soon (an animation running).
    pub fn wants_another_pass(&self) -> bool {
        self.again
    }
}

/// One pass: widgets are placed and drawn into it.
pub struct Frame<'a> {
    ui: &'a mut Ui,
    r: &'a mut Renderer2d,
    quads: Vec<Quad>,
    areas: Vec<Area>,
}

impl Frame<'_> {
    pub fn theme(&self) -> Theme {
        self.ui.theme
    }

    fn s(&self) -> f32 {
        self.ui.scale
    }

    /// Draw a quad given in logical pixels.
    fn put(&mut self, q: Quad) {
        let s = self.s();
        self.quads.push(q.scaled(s));
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

    fn put_text(&mut self, laid: &Laid, x: f32, y: f32) {
        let s = self.s();
        let (dx, dy) = ((x * s).round(), (y * s).round());
        self.quads
            .extend(laid.quads.iter().map(|q| q.moved(dx, dy)));
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

    fn interact(&mut self, id: Id, rect: Rect) -> Response {
        let p = self.ui.pointer;
        let over = p.inside && rect.contains(p.x, p.y);
        if over && p.pressed {
            self.ui.held = Some(id);
        }
        let held = self.ui.held == Some(id);
        Response {
            hovered: over,
            held: held && p.down,
            clicked: held && p.released && over,
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
        let fg = match kind {
            Kind::Primary => t.on_coral,
            _ => t.ink,
        };
        let laid = self.lay_out(text, &Style::body(t.text, fg).weight(600), None);
        let rect = self.alloc(laid.width + 2.0 * 18.0, t.control_height);
        let res = self.interact(id(name), rect);
        let base = match kind {
            Kind::Primary => t.coral,
            Kind::Secondary => t.line,
            Kind::Ghost => alpha(t.line, 0.0),
        };
        // hover lifts it a little, holding darkens it
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

    /// A value kept between passes (for widgets the app doesn't hold state for).
    pub fn value(&mut self, name: &str, default: f32) -> f32 {
        *self.ui.values.entry(id(name)).or_insert(default)
    }
    pub fn set_value(&mut self, name: &str, v: f32) {
        self.ui.values.insert(id(name), v);
    }
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}
