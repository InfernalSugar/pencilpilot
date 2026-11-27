//! The canvas card: the white card the drawing sits in, and what's drawn over the drawing
//! (the design's pill and corner chip, review note pins) and its right-click menu.

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::hex;
use pencilpilot_ui::ui::{Frame, Rect, id};

use crate::state::{Action, State, Workspace};

const MENU: &str = "canvas/menu";
const SETTINGS: &str =
    "M4 7h10M18 7h2M4 17h4M12 17h8M16 5a2 2 0 1 0 0.01 0ZM10 15a2 2 0 1 0 0.01 0Z";
const CHIP: [f32; 4] = hex(0xF6F4EF);

/// Around the document when it doesn't fill the card.
const DESK: [f32; 4] = hex(0xECE9E2);

/// The card and the document in it, through the view. Returns the document's area (the
/// card inside its edge), where the view is measured from.
pub fn card(f: &mut Frame, r: Rect, s: &mut State) -> Rect {
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.card).radius(24.0));
    let inner = r.inset(12.0);
    s.view.follow(inner);
    let v = s.view;
    // the whole card shows the view (the desk round the document), its corners rounded
    f.put(
        Quad::canvas(
            [r.x, r.y, r.w, r.h],
            v.inverse_affine(inner, 1.0),
            [v.doc_w as f32, v.doc_h as f32],
            DESK,
            v.zoom >= 2.0,
        )
        .radius(24.0),
    );
    // pixels being moved, over the document (the GPU moves them; nothing is redrawn)
    if let Some(mv) = s.moving
        && let Some(map) = v.moving_affine(inner, 1.0, &mv)
    {
        f.put(
            Quad::canvas(
                [r.x, r.y, r.w, r.h],
                map,
                [mv.w as f32, mv.h as f32],
                [0.0; 4],
                v.zoom >= 2.0,
            )
            .radius(24.0)
            .second(),
        );
    }
    inner
}

/// What the pill says in each workspace: its title, its line, its button's name.
fn pill(s: &State) -> (String, String, &'static str) {
    match s.workspace {
        Workspace::Colour => (
            format!("Fill: {}", s.colour_name),
            "close gaps 4 px".into(),
            "Fill settings",
        ),
        Workspace::Review => (
            format!("{} · take 3", s.cut_name),
            "sent for review today".into(),
            "Review settings",
        ),
        _ => {
            let brush = s.brushes.get(s.brush).cloned().unwrap_or_default();
            (
                brush,
                format!("{} px · 100%", s.brush_size.round()),
                "Brush settings",
            )
        }
    }
}

/// The corner chip's text.
fn corner(s: &State) -> String {
    match s.workspace {
        Workspace::Colour => format!("Drawing {} of {}", s.drawing + 1, s.drawings),
        Workspace::Review => {
            let n = s.notes.iter().filter(|n| n.frame == s.frame).count();
            format!("{n} note{} on this frame", if n == 1 { "" } else { "s" })
        }
        Workspace::Draw if s.onion => "Onion skin: 1 before · 1 after".into(),
        Workspace::Animate if s.onion => "Onion skin: 2 before · 1 after".into(),
        _ => "Onion skin off".into(),
    }
}

/// Over the drawing: the pill (top left), the corner chip (top right), the review pins,
/// and the right-click menu.
pub fn overlays(f: &mut Frame, card: Rect, s: &mut State) {
    let t = f.theme();
    // the pill
    let (title, line, button) = pill(s);
    let tl = f.lay_out(&title, &Style::display(15.0, t.ink).weight(600), None);
    let ll = f.lay_out(&line, &Style::body(13.0, hex(0x5E5D63)).weight(600), None);
    let h = 44.0;
    let w = 14.0 + tl.width + 8.0 + ll.width + 8.0 + 32.0 + 8.0;
    let (x, y) = (card.x + 16.0, card.y + 14.0);
    f.put(Quad::rect(x, y, w, h, CHIP).radius(h / 2.0));
    f.put_text(&tl, x + 14.0, y + (h - tl.height) / 2.0);
    f.put_text(&ll, x + 14.0 + tl.width + 8.0, y + (h - ll.height) / 2.0);
    let b = Rect::new(x + w - 8.0 - 32.0, y + 6.0, 32.0, 32.0);
    let res = f.interact(id("canvas/pill"), b);
    f.put(
        Quad::rect(
            b.x,
            b.y,
            b.w,
            b.h,
            if res.hovered { hex(0xEFECE4) } else { t.card },
        )
        .radius(16.0),
    );
    f.icon_stroked(SETTINGS, b.x + 7.0, b.y + 7.0, 18.0, t.ink, 2.2);
    if res.clicked {
        s.actions.push(Action::Other(button.into()));
    }
    if res.hovered {
        f.tooltip(id("canvas/pill"), button, b);
    }
    // the corner chip
    let cl = f.lay_out(
        &corner(s),
        &Style::body(13.0, hex(0x44434A)).weight(600),
        None,
    );
    let (cw, ch) = (cl.width + 28.0, cl.height + 16.0);
    let cx = card.right() - 16.0 - cw;
    f.put(Quad::rect(cx, card.y + 14.0, cw, ch, CHIP).radius(ch / 2.0));
    f.put_text(&cl, cx + 14.0, card.y + 22.0);
    // review: pins for the notes on this frame, where they were left (spread over the card)
    if s.workspace == Workspace::Review {
        let here: Vec<usize> = (0..s.notes.len())
            .filter(|&i| s.notes[i].frame == s.frame)
            .collect();
        for (k, &i) in here.iter().enumerate() {
            let (px, py) = (
                card.x + card.w * (0.53 + 0.1 * k as f32),
                card.y + card.h * (0.38 + 0.32 * k as f32),
            );
            let r = Rect::new(px, py, 32.0, 32.0);
            let res = f.interact(id(&format!("canvas/pin/{i}")), r);
            f.put(
                Quad::rect(r.x, r.y + 4.0, 32.0, 32.0, [0.09, 0.09, 0.1, 0.25])
                    .radius(16.0)
                    .shadow(6.0),
            );
            // a speech-bubble pin: round, with its bottom-left corner square
            f.put(Quad::rect(r.x, r.y, 32.0, 32.0, t.card).radius(16.0));
            f.put(Quad::rect(r.x, r.y + 16.0, 16.0, 16.0, t.card).radius(4.0));
            f.put(Quad::rect(r.x + 3.0, r.y + 3.0, 26.0, 26.0, s.notes[i].colour).radius(13.0));
            f.put(Quad::rect(r.x + 3.0, r.y + 16.0, 13.0, 13.0, s.notes[i].colour).radius(2.0));
            let n = f.lay_out(
                &(i + 1).to_string(),
                &Style::body(13.0, t.card).weight(700),
                None,
            );
            f.put_text(
                &n,
                r.x + (32.0 - n.width) / 2.0,
                r.y + (32.0 - n.height) / 2.0,
            );
            if res.clicked {
                s.actions.push(Action::GoToFrame(s.notes[i].frame));
            }
            if res.hovered {
                f.tooltip(
                    id(&format!("canvas/pin/{i}")),
                    &format!("Note {} by {}", i + 1, s.notes[i].author),
                    r,
                );
            }
        }
    }
    // right-click: the canvas menu; in Review, what to do with the frame being reviewed
    f.context_area(MENU, card);
    // the tools' overlays, the selection's menu (instead of this one inside it), their keys
    crate::shell::engine::tools::overlay::draw(f, card, s);
    let review = s.workspace == Workspace::Review;
    let frame = s.frame + 1;
    let picked = f.menu(MENU, |m| {
        if review {
            m.head(&format!("Frame {frame}"));
            m.item("Add a note here", "N");
            m.item("Copy a link to this frame", "");
            m.separator();
            m.item("Hide note pins", "");
            m.item("Reset view", "0");
            return;
        }
        m.head("Canvas");
        m.item("Paste", "Ctrl+V");
        m.item("Select all", "Ctrl+A");
        m.separator();
        m.item("Guides", "Grid · Perspective · Rulers ›");
        m.item("Symmetry", "Off ›");
        m.item("Snap to guides", "✓");
        m.separator();
        m.item("Rotate view", "15°");
        m.item("Flip view", "H");
        m.item("Reset view", "0");
        m.separator();
        m.item("Record timelapse", "●");
    });
    if let Some(item) = picked
        && !view_item(s, card, &item)
    {
        s.actions.push(Action::Other(item));
    }
    // the view's keys (not while typing): 0 fits, 1 is 100 %, H flips, + and − zoom
    if !f.typing() {
        for (key, item) in [
            ("0", "Reset view"),
            ("1", "Actual size"),
            ("h", "Flip view"),
            ("plus", "Zoom in"),
            ("equal", "Zoom in"),
            ("KP_Add", "Zoom in"),
            ("minus", "Zoom out"),
            ("KP_Subtract", "Zoom out"),
        ] {
            if f.take_key(key, false) {
                view_item(s, card, item);
            }
        }
    }
}

/// The view's menu items and keys. Returns whether `item` was one.
pub fn view_item(s: &mut State, card: Rect, item: &str) -> bool {
    let c = (card.x + card.w / 2.0, card.y + card.h / 2.0);
    let card = card.inset(12.0);
    match item {
        "Reset view" => s.view.reset(card),
        "Actual size" => s.view.set_zoom(card, 1.0),
        "Flip view" => s.view.flip_view(card),
        "Zoom in" => s.view.zoom_at(card, c, 1.25),
        "Zoom out" => s.view.zoom_at(card, c, 0.8),
        "Rotate view" | "Rotate right" => s.view.rotate_about(card, c, 15f32.to_radians()),
        "Rotate left" => s.view.rotate_about(card, c, -15f32.to_radians()),
        _ => return false,
    }
    true
}
