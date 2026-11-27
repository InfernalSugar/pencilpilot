//! The project's questions, as a small pop-up over the window: "Save first?" (New, Open,
//! Quit with unsaved changes), "Recover your unsaved changes?" (a recovery copy found at
//! start).

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::alpha;
use pencilpilot_ui::ui::{Frame, Kind, Rect, id};

use crate::state::{Action, State};

/// A question and its answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub title: String,
    pub detail: String,
    /// each answer: its button's text, the action it pushes ([`Action::File`]), its kind;
    /// the first is the main one, on the right, the others to its left in order
    pub buttons: Vec<(String, String, Kind)>,
    /// pushed when it's closed otherwise (Escape, a click outside)
    pub dismiss: String,
}

/// The pop-up's name.
pub const POPUP: &str = "files/question";

/// A pill button at `rect` (drawn by hand, so its place is known): true when clicked.
fn pill(f: &mut Frame, name: &str, rect: Rect, text: &str, kind: Kind) -> bool {
    let t = f.theme();
    let fg = if kind == Kind::Primary {
        t.on_coral
    } else {
        t.ink
    };
    let laid = f.lay_out(text, &Style::body(t.text, fg).weight(600), None);
    let res = f.interact(id(name), rect);
    let base = match kind {
        Kind::Primary => t.coral,
        Kind::Secondary => t.line,
        Kind::Ghost => alpha(t.line, 0.0),
    };
    let fill = if res.held || res.hovered {
        crate::shell::header::mix(base, t.ink, if res.held { 0.18 } else { 0.08 })
    } else {
        base
    };
    f.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, fill).radius(rect.h / 2.0));
    f.put_text(
        &laid,
        rect.x + (rect.w - laid.width) / 2.0,
        rect.y + (rect.h - laid.height) / 2.0,
    );
    res.clicked
}

/// The question the project asks (in [`crate::shell::engine::files::ProjectUi::asking`]),
/// centred over `r`, while it asks it. Its buttons' places go in `ask_buttons`.
pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    // (its state between passes: 0 not showing, 1 showing, 2 answered: the engine has yet
    // to take the answer)
    let Some(q) = s.project.asking.clone() else {
        if f.value(POPUP, 0.0) != 0.0 {
            f.set_value(POPUP, 0.0);
        }
        return;
    };
    let state = f.value(POPUP, 0.0);
    if state == 2.0 {
        return;
    }
    if !f.is_open(POPUP) {
        if state == 1.0 {
            // closed by a click outside
            f.set_value(POPUP, 2.0);
            s.actions.push(Action::File(q.dismiss.clone()));
            return;
        }
        f.open(POPUP, 0.0, 0.0);
        f.set_value(POPUP, 1.0);
    }
    let t = f.theme();
    let widths: Vec<f32> = q
        .buttons
        .iter()
        .map(|(text, _, _)| {
            f.lay_out(text, &Style::body(t.text, t.ink).weight(600), None)
                .width
                + 36.0
        })
        .collect();
    let row = widths.iter().sum::<f32>() + 8.0 * widths.len().saturating_sub(1) as f32;
    let title_w = f
        .lay_out(&q.title, &Style::display(20.0, t.ink).weight(800), None)
        .width;
    let w = (row + 48.0).max(title_w + 56.0).clamp(440.0, r.w - 32.0);
    let h = 168.0;
    let card = Rect::new(r.x + (r.w - w) / 2.0, r.y + r.h * 0.3, w, h);
    let mut rects = Vec::new();
    let mut x = card.right() - 24.0;
    for bw in &widths {
        x -= bw;
        rects.push(Rect::new(x, card.bottom() - 24.0 - 44.0, *bw, 44.0));
        x -= 8.0;
    }
    s.project.ask_buttons = rects.clone();
    let mut picked = None;
    f.popup_at(POPUP, card, 20.0, 24.0, 1.0, |f| {
        f.heading(&q.title);
        f.label(&q.detail);
        for (i, (text, action, kind)) in q.buttons.iter().enumerate() {
            if pill(f, &format!("{POPUP}/{i}"), rects[i], text, *kind) {
                picked = Some(action.clone());
            }
        }
    });
    if f.take_key("Escape", false) {
        picked = Some(q.dismiss.clone());
    }
    if let Some(p) = picked {
        f.set_value(POPUP, 2.0);
        f.close();
        s.actions.push(Action::File(p));
    }
}
