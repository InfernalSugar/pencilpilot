//! What the tools draw over the drawing, in screen space through the view: the
//! selection's dashed edge, a rectangle or lasso being drawn, the transform's box and
//! handles, a shape being dragged out, the text's caret, the grid and the symmetry lines;
//! the selection's right-click menu; the tools' keys; the brush (and fill) settings
//! pop-up the pill's button opens.
//!
//! It only reads and changes [`State`] and pushes [`Action::Other`]s for the engine
//! ([`super::apply`]).

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::hex;
use pencilpilot_ui::ui::{Frame, Rect};

use super::{GRID, ROTATE_ARM, Sym, shape_points};
use crate::state::{Action, State, Tool};

pub const SELECTION_MENU: &str = "canvas/selection";
pub const SETTINGS: &str = "canvas/brush-settings";

const DARK: [f32; 4] = [0.09, 0.09, 0.1, 0.9];
const LIGHT: [f32; 4] = [1.0, 1.0, 1.0, 0.95];
const ACCENT: [f32; 4] = hex(0x3D6BFF);
const GUIDE: [f32; 4] = [0.24, 0.42, 1.0, 0.35];
const SYM_LINE: [f32; 4] = [0.91, 0.29, 0.18, 0.7];
/// A dash's length (screen pixels).
const DASH: f32 = 5.0;

/// The interface's own memory between passes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Memo {
    /// the settings pop-up was open in the last pass (a click on the pill then closes it)
    settings_was_open: bool,
    /// the settings pop-up is the Fill's
    fill_settings: bool,
    /// a transform was asked for since the Transform tool was picked
    pub(super) transform_asked: bool,
    /// the rest of a press of Ctrl+Shift+V (the toolkit's Ctrl keys don't tell Shift)
    paste_in_place: bool,
}

/// Lines and dots in screen space, kept inside the card.
struct Pen<'a, 'f> {
    f: &'a mut Frame<'f>,
    clip: [f32; 4],
}

impl Pen<'_, '_> {
    fn inside(&self, x: f32, y: f32, pad: f32) -> bool {
        x >= self.clip[0] - pad
            && y >= self.clip[1] - pad
            && x <= self.clip[2] + pad
            && y <= self.clip[3] + pad
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        if self.inside(x, y, w.max(h)) {
            self.f.put(Quad::rect(x, y, w, h, c).clip(self.clip));
        }
    }

    /// A line `width` wide; dashed dark and light when `dashed` (the dash pattern carries on
    /// from `phase`, returned).
    fn line(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        width: f32,
        c: [f32; 4],
        dashed: bool,
        mut phase: f32,
    ) -> f32 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        if len < 1e-3 {
            return phase;
        }
        let colour = |phase: f32| {
            if !dashed || (phase / DASH) as i64 % 2 == 0 {
                c
            } else {
                LIGHT
            }
        };
        let hw = width / 2.0;
        // straight across or down: a rectangle per dash
        if dx.abs() < 0.01 || dy.abs() < 0.01 {
            let mut t = 0.0;
            while t < len {
                let run = if dashed {
                    (DASH - phase % DASH).min(len - t)
                } else {
                    len - t
                };
                let (x0, y0) = (a.0 + dx * t / len, a.1 + dy * t / len);
                let (x1, y1) = (a.0 + dx * (t + run) / len, a.1 + dy * (t + run) / len);
                self.rect(
                    x0.min(x1) - hw,
                    y0.min(y1) - hw,
                    (x1 - x0).abs() + width,
                    (y1 - y0).abs() + width,
                    colour(phase),
                );
                t += run;
                phase += run;
            }
            return phase;
        }
        // any other way: dots, a pixel and a half apart
        let step = 1.5f32.min(width.max(1.0));
        let n = (len / step).ceil() as usize;
        for i in 0..=n {
            let t = (i as f32 * step).min(len);
            let (x, y) = (a.0 + dx * t / len, a.1 + dy * t / len);
            if self.inside(x, y, width) {
                let col = colour(phase + t);
                self.f
                    .put(Quad::rect(x - hw, y - hw, width, width, col).clip(self.clip));
            }
        }
        phase + len
    }

    /// A closed or open path, dashed.
    fn path(&mut self, pts: &[(f32, f32)], closed: bool, width: f32, c: [f32; 4], dashed: bool) {
        let mut phase = 0.0;
        for w in pts.windows(2) {
            phase = self.line(w[0], w[1], width, c, dashed, phase);
        }
        if closed && pts.len() > 2 {
            self.line(pts[pts.len() - 1], pts[0], width, c, dashed, phase);
        }
    }

    fn handle(&mut self, p: (f32, f32), round: bool) {
        let s = 10.0;
        let q = Quad::rect(p.0 - s / 2.0, p.1 - s / 2.0, s, s, LIGHT)
            .radius(if round { s / 2.0 } else { 2.0 })
            .border(1.5, ACCENT);
        self.f.put(q.clip(self.clip));
    }
}

/// Whether document point `p` is inside the outline's loops (even-odd).
fn in_outline(loops: &[Vec<(f32, f32)>], p: (f32, f32)) -> bool {
    let mut inside = false;
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if (a.1 <= p.1) != (b.1 <= p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Everything the tools show over the drawing, and their menu, keys and pop-up. `card` is
/// the canvas card (the view's rectangle is inside it, 12 px in).
pub fn draw(f: &mut Frame, card: Rect, s: &mut State) {
    let inner = card.inset(12.0);
    let v = s.view;
    let to = |p: (f32, f32)| v.to_screen(inner, p.0, p.1);
    let clip = [card.x, card.y, card.right(), card.bottom()];
    let z = v.zoom;
    {
        let mut pen = Pen { f, clip };
        // the grid
        if s.tooling.grid {
            let (w, h) = (v.doc_w as f32, v.doc_h as f32);
            let mut x = GRID;
            while x < w {
                pen.line(to((x, 0.0)), to((x, h)), 1.0, GUIDE, false, 0.0);
                x += GRID;
            }
            let mut y = GRID;
            while y < h {
                pen.line(to((0.0, y)), to((w, y)), 1.0, GUIDE, false, 0.0);
                y += GRID;
            }
        }
        // symmetry's mirror lines
        let (w, h) = (v.doc_w as f32, v.doc_h as f32);
        if matches!(s.tooling.symmetry, Sym::Vertical | Sym::Both) {
            pen.line(
                to((w / 2.0, 0.0)),
                to((w / 2.0, h)),
                1.5,
                SYM_LINE,
                false,
                0.0,
            );
        }
        if matches!(s.tooling.symmetry, Sym::Horizontal | Sym::Both) {
            pen.line(
                to((0.0, h / 2.0)),
                to((w, h / 2.0)),
                1.5,
                SYM_LINE,
                false,
                0.0,
            );
        }
        // the selection's edge (not while it's being transformed: the box shows then)
        if s.tooling.transform.is_none() {
            for l in &s.tooling.outline {
                let pts: Vec<(f32, f32)> = l.iter().map(|p| to(*p)).collect();
                pen.path(&pts, true, 1.5, DARK, true);
            }
        }
        // a rectangle or a lasso being drawn
        if let Some([x0, y0, x1, y1]) = s.tooling.marquee {
            let pts = [to((x0, y0)), to((x1, y0)), to((x1, y1)), to((x0, y1))];
            pen.path(&pts, true, 1.5, DARK, true);
        }
        if s.tooling.lasso.len() > 1 {
            let pts: Vec<(f32, f32)> = s.tooling.lasso.iter().map(|p| to(*p)).collect();
            pen.path(&pts, false, 1.5, DARK, true);
        }
        // the transform's box, its handles and the turning handle
        if let Some(b) = s.tooling.transform {
            let c: Vec<(f32, f32)> = b.corners().iter().map(|p| to(*p)).collect();
            pen.path(&c, true, 1.5, ACCENT, false);
            let top = to(b.point(0, -1));
            let knob = to(b.rotate_handle(ROTATE_ARM / z.max(1e-3)));
            pen.line(top, knob, 1.5, ACCENT, false, 0.0);
            for (u, vv) in [
                (-1, -1),
                (1, -1),
                (1, 1),
                (-1, 1),
                (0, -1),
                (1, 0),
                (0, 1),
                (-1, 0),
            ] {
                pen.handle(to(b.point(u, vv)), false);
            }
            pen.handle(knob, true);
        }
        // a shape being dragged out: its line, as wide as the brush
        if let Some([a, b]) = s.tooling.shape {
            let pts: Vec<(f32, f32)> = shape_points(s.tooling.shape_kind, a, b)
                .iter()
                .step_by(4)
                .map(|p| to(*p))
                .collect();
            let width = (s.brush_size * z).clamp(1.5, 200.0);
            let mut col = s.colour;
            col[3] = 0.6;
            pen.path(&pts, false, width, col, false);
        }
        // the text: its caret and a light box round it
        if let Some(t) = &s.tooling.text {
            let (cx, ctop, ch) = t.caret;
            let a = to((t.x + cx, t.y + ctop));
            let b = to((t.x + cx, t.y + ctop + ch));
            pen.line(a, b, 2.0, hex(0xE8492F), false, 0.0);
            let [bx, by, bw, bh] = t.bounds;
            if bw > 0.0 {
                let pad = 6.0 / z.max(1e-3);
                let (x0, y0) = (t.x + bx - pad, t.y + by - pad);
                let (x1, y1) = (t.x + bx + bw + pad, t.y + by + bh + pad);
                let pts = [to((x0, y0)), to((x1, y0)), to((x1, y1)), to((x0, y1))];
                pen.path(&pts, true, 1.0, ACCENT, true);
            }
        }
    }

    // the Shape tool's kind, in a chip at the card's foot
    if s.tool == Tool::Shape {
        let text = format!("Shape: {} · U changes it", s.tooling.shape_kind.name());
        let l = f.lay_out(&text, &Style::body(13.0, hex(0x44434A)).weight(600), None);
        let (w, h) = (l.width + 28.0, l.height + 16.0);
        let (x, y) = (card.x + 16.0, card.bottom() - 14.0 - h);
        f.put(Quad::rect(x, y, w, h, hex(0xF6F4EF)).radius(h / 2.0));
        f.put_text(&l, x + 14.0, y + 8.0);
    }

    selection_menu(f, card, s);
    keys(f, s);
    transform_follows_the_tool(s);
    settings(f, card, s);
}

/// The Transform tool lifts what's selected as it's picked, and keeps it when another
/// tool is.
fn transform_follows_the_tool(s: &mut State) {
    if s.tool == Tool::Transform {
        if s.tooling.transform.is_none() && !s.tooling.memo.transform_asked {
            s.actions.push(Action::Other("Transform".into()));
            s.tooling.memo.transform_asked = true;
        }
    } else {
        s.tooling.memo.transform_asked = false;
        if s.tooling.transform.is_some()
            && !s
                .actions
                .contains(&Action::Other("Transform: commit".into()))
        {
            s.actions.push(Action::Other("Transform: commit".into()));
        }
    }
}

/// A right-click inside the selection opens its menu (instead of the canvas's).
fn selection_menu(f: &mut Frame, card: Rect, s: &mut State) {
    let inner = card.inset(12.0);
    let p = f.pointer();
    if p.context
        && card.contains(p.x, p.y)
        && !s.tooling.outline.is_empty()
        && in_outline(&s.tooling.outline, s.view.to_doc(inner, p.x, p.y))
    {
        f.open(SELECTION_MENU, p.x, p.y);
    }
    let clip = s.tooling.has_clip;
    let picked = f.menu(SELECTION_MENU, |m| {
        m.head("Selection");
        m.item("Cut", "Ctrl+X");
        m.item("Copy", "Ctrl+C");
        if clip {
            m.item("Paste in place", "Ctrl+Shift+V");
        }
        m.separator();
        m.item("Transform", "V");
        m.item("Flip horizontally", "");
        m.item("Flip vertically", "");
        m.separator();
        m.item("Fill selection", "G");
        m.danger("Clear", "Del");
    });
    if let Some(item) = picked {
        if item == "Transform" {
            s.tool = Tool::Transform;
            s.tooling.memo.transform_asked = true;
        }
        s.actions.push(Action::Other(item));
    }
}

/// The tools' keys (not while a text field has the keyboard); while text is typed on the
/// canvas, every key types.
fn keys(f: &mut Frame, s: &mut State) {
    if f.typing() {
        return;
    }
    let push = |s: &mut State, a: &str| s.actions.push(Action::Other(a.into()));
    if let Some(t) = &mut s.tooling.text {
        let keys: Vec<_> = f.keys().to_vec();
        let mut changed = false;
        let mut end = None;
        for k in &keys {
            match k.name.as_str() {
                "Return" | "KP_Enter" if k.shift => {
                    t.text.push('\n');
                    changed = true;
                }
                "Return" | "KP_Enter" => end = Some("Text: commit"),
                "Escape" => end = Some("Text: cancel"),
                "BackSpace" => changed |= t.text.pop().is_some(),
                _ => {
                    if let Some(txt) = &k.text
                        && !k.ctrl
                        && !txt.chars().any(char::is_control)
                    {
                        t.text.push_str(txt);
                        changed = true;
                    }
                }
            }
        }
        // the keys are the text's (Ctrl+Z still undoes: the shell takes it first)
        for k in &keys {
            f.take_key(&k.name, k.ctrl);
        }
        if changed {
            push(s, "Text: changed");
        }
        if let Some(a) = end {
            push(s, a);
        }
        return;
    }
    // Ctrl+Shift+V: paste in place (the toolkit's take_key doesn't tell Shift)
    let shift_v = f
        .keys()
        .iter()
        .any(|k| k.ctrl && k.shift && k.name.eq_ignore_ascii_case("v"));
    for (key, ctrl, action) in [
        ("c", true, "Copy"),
        ("x", true, "Cut"),
        ("v", true, if shift_v { "Paste in place" } else { "Paste" }),
        ("a", true, "Select all"),
        ("d", true, "Deselect"),
    ] {
        if f.take_key(key, ctrl) {
            push(s, action);
        }
    }
    let selected = s.tooling.selection.is_some();
    if (f.take_key("Delete", false) || (selected && f.take_key("BackSpace", false))) && selected {
        push(s, "Clear");
    }
    if f.take_key("Escape", false) {
        push(s, "Escape");
    }
    if s.tooling.transform.is_some()
        && (f.take_key("Return", false) || f.take_key("KP_Enter", false))
    {
        push(s, "Transform: commit");
    }
    if f.take_key("l", false) {
        s.tool = Tool::Lasso;
    }
    if f.take_key("u", false) {
        if s.tool == Tool::Shape {
            s.tooling.shape_kind = s.tooling.shape_kind.next();
        } else {
            s.tool = Tool::Shape;
        }
    }
}

/// The pill's "Brush settings" / "Fill settings" button (and a tool menu's "Tool
/// settings…") open the settings pop-up under the pill.
fn settings(f: &mut Frame, card: Rect, s: &mut State) {
    let was_open = s.tooling.memo.settings_was_open;
    let mut asked = None;
    s.actions.retain(|a| match a {
        Action::Other(n) if n == "Brush settings" || n == "Fill settings" => {
            asked = Some(n == "Fill settings");
            false
        }
        Action::Other(n) if n.ends_with(": Tool settings…") => {
            asked = Some(n.starts_with("Fill"));
            false
        }
        _ => true,
    });
    let (x, y) = (card.x + 16.0, card.y + 66.0);
    if let Some(fill) = asked {
        // a click on the pill's button while it's open closes it
        if !was_open || fill != s.tooling.memo.fill_settings {
            s.tooling.memo.fill_settings = fill;
            f.open(SETTINGS, x, y);
        }
    }
    let fill = s.tooling.memo.fill_settings;
    let pressure_tool = if matches!(s.tool, Tool::Eraser) {
        Tool::Eraser
    } else {
        Tool::Brush
    };
    let h = if fill { 228.0 } else { 330.0 };
    let w = 300.0;
    f.popup_at(SETTINGS, Rect::new(x, y, w, h), 22.0, 16.0, 1.0, |f| {
        let t = f.theme();
        let label = |f: &mut Frame, text: &str, value: &str| {
            let l = f.lay_out(text, &Style::body(13.0, t.ink).weight(600), None);
            let vl = f.lay_out(value, &Style::body(13.0, hex(0x5E5D63)).weight(600), None);
            let r = f.alloc(0.0, l.height);
            f.put_text(&l, r.x, r.y);
            f.put_text(&vl, r.right() - vl.width, r.y);
        };
        let iw = w - 32.0;
        f.heading(if fill {
            "Fill settings"
        } else {
            "Brush settings"
        });
        if !fill {
            label(f, "Size", &format!("{} px", s.brush_size.round()));
            let mut size = s.brush_size;
            if f.slider("canvas/settings/size", &mut size, 1.0, 60.0, iw)
                .changed
            {
                s.brush_size = size.round().max(1.0);
                if let Some(v) = s.panel.sizes.get_mut(s.brush) {
                    *v = s.brush_size;
                }
            }
        }
        label(f, "Opacity", &format!("{}%", s.panel.opacity.round()));
        let mut op = s.panel.opacity;
        if f.slider("canvas/settings/opacity", &mut op, 0.0, 100.0, iw)
            .changed
        {
            s.panel.opacity = op.round();
        }
        if fill {
            let mut gaps = s.panel.close_gaps;
            f.toggle("canvas/settings/gaps", "Close gaps 4 px", &mut gaps);
            s.panel.close_gaps = gaps;
            let mut under = s.panel.fill_under;
            f.toggle("canvas/settings/under", "Fill under the lines", &mut under);
            s.panel.fill_under = under;
            return;
        }
        label(f, "Steady lines", &format!("{}%", s.panel.steady.round()));
        let mut st = s.panel.steady;
        if f.slider("canvas/settings/steady", &mut st, 0.0, 100.0, iw)
            .changed
        {
            s.panel.steady = st.round();
        }
        let flip = |list: &mut Vec<Tool>, on: bool| {
            list.retain(|t| *t != pressure_tool);
            if on {
                list.push(pressure_tool);
            }
        };
        let mut ps = s.tools.pressure_size.contains(&pressure_tool);
        if f.toggle("canvas/settings/psize", "Pressure changes size", &mut ps)
            .changed
        {
            flip(&mut s.tools.pressure_size, ps);
        }
        let mut po = s.tools.pressure_opacity.contains(&pressure_tool);
        if f.toggle(
            "canvas/settings/popacity",
            "Pressure changes opacity",
            &mut po,
        )
        .changed
        {
            flip(&mut s.tools.pressure_opacity, po);
        }
    });
    s.tooling.memo.settings_was_open = f.is_open(SETTINGS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_in_the_outline() {
        let sq = vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]];
        assert!(in_outline(&sq, (5.0, 5.0)));
        assert!(!in_outline(&sq, (15.0, 5.0)));
        let mut holed = sq.clone();
        holed.push(vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)]);
        assert!(!in_outline(&holed, (5.0, 5.0)));
        assert!(in_outline(&holed, (1.0, 5.0)));
    }
}
