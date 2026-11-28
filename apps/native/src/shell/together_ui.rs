//! Drawing together's interface: the dialog under the header's Draw together button (host,
//! join with a code, who's here, the shared layers and asking to draw on them), the chat
//! card, others' cursors on the canvas, and a short note when something needs saying.
//! The content is the current app's ShareDialog and TogetherChat; the look is the native
//! app's own widgets (a white card, the coral main action), as the export dialog.
//!
//! What it changes lives in [`State::together`]; its buttons push the together part's
//! actions ([`ops`]), which the engine carries out.

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Kind, Rect, id};

use crate::shell::engine::together::{Row, member_colour, ops};
use crate::state::{Action, State};

const NAME: &str = "together/dialog";
const WIDTH: f32 = 460.0;
const PAD: f32 = 18.0;
const GAP: f32 = 12.0;
const SUBTLE: [f32; 4] = hex(0x5E5D63);
const CHAT_W: f32 = 280.0;
/// the chat's lines shown at most
const CHAT_LINES: usize = 8;

/// The dialog, the chat and the note. Drawn above everything.
pub fn build(f: &mut Frame, s: &mut State) {
    dialog(f, s);
    note(f, s);
}

fn dialog(f: &mut Frame, s: &mut State) {
    let shown = f.is_open(NAME);
    let x = &mut s.together;
    if x.open && !shown {
        if x.was_open() {
            // a click elsewhere or Escape put it away
            x.open = false;
            x.set_was_open(false);
            return;
        }
        f.open(NAME, 0.0, 0.0);
    } else if !x.open && shown {
        f.close();
    }
    x.set_was_open(x.open);
    if !x.open {
        return;
    }
    let anchor = s.header.layout.together;
    let h = height(f, s);
    let rect = Rect::new(
        (anchor.right() - WIDTH).max(8.0),
        anchor.bottom() + 10.0,
        WIDTH,
        h,
    );
    let mut actions = Vec::new();
    f.popup_at(NAME, rect, 22.0, PAD, 1.0, |f| {
        if s.together.live() {
            live(f, s, &mut actions);
        } else {
            start(f, s, &mut actions);
        }
    });
    s.actions.extend(actions);
}

fn title(f: &mut Frame, text: &str) {
    let t = f.theme();
    let l = f.lay_out(text, &Style::display(20.0, t.ink).weight(800), None);
    let r = f.alloc(0.0, 28.0);
    f.put_text(&l, r.x, r.y + (r.h - l.height) / 2.0);
}

fn small(f: &mut Frame, text: &str, color: [f32; 4], h: f32) {
    let max = f.remaining().w;
    let l = f.lay_out(text, &Style::body(13.0, color).weight(600), Some(max));
    let r = f.alloc(0.0, h.max(l.height));
    f.put_text(&l, r.x, r.y);
}

fn lines_h(f: &mut Frame, text: &str, w: f32) -> f32 {
    f.lay_out(text, &Style::body(13.0, SUBTLE).weight(600), Some(w))
        .height
}

const START_NOTE: &str =
    "Solo projects never connect to anything. Drawing together starts only when you host or join.";
const LIVE_NOTE: &str = "The code is like a house key: anyone who has it can join. Drawings travel straight between your computers, encrypted. Everyone keeps a full copy: save it any time.";

/// The dialog's height for what it shows.
fn height(f: &mut Frame, s: &State) -> f32 {
    let inner = WIDTH - 2.0 * PAD;
    let x = &s.together;
    let mut rows: Vec<f32> = vec![28.0];
    if let Some(info) = &x.info {
        rows.push(16.0); // about
        rows.push(44.0); // code + copy
        rows.push(18.0); // here now
        rows.push(members_h(info.members.len()));
        if !x.synced {
            rows.push(16.0);
        } else if !x.rows.is_empty() {
            rows.push(18.0);
            rows.push(layers_h(&x.rows));
        }
        rows.push(lines_h(f, LIVE_NOTE, inner));
        rows.push(44.0);
    } else {
        rows.extend([16.0, 44.0, 18.0, 16.0, 44.0, 18.0, 44.0]);
        rows.push(lines_h(f, START_NOTE, inner));
        rows.push(44.0);
    }
    if x.busy {
        rows.push(16.0);
    }
    if !x.error.is_empty() {
        rows.push(lines_h(f, &x.error, inner));
    }
    rows.iter().sum::<f32>() + GAP * 1.5 * (rows.len() as f32 - 1.0) + 2.0 * PAD + 2.0
}

fn members_h(n: usize) -> f32 {
    (n.min(8) as f32) * 22.0
}

/// One layer row's height: its line, and a line per ask and for who else draws.
fn row_h(r: &Row) -> f32 {
    36.0 + r.asks.len() as f32 * 36.0
        + if r.mine && !r.writers.is_empty() {
            36.0
        } else {
            0.0
        }
}

fn layers_h(rows: &[Row]) -> f32 {
    rows.iter().map(row_h).sum::<f32>().min(220.0)
}

/// Not in a session: the name, Host, Join with a code.
fn start(f: &mut Frame, s: &mut State, actions: &mut Vec<Action>) {
    let t = f.theme();
    let w = f.remaining().w;
    title(f, "Draw together");
    small(f, "Your name", SUBTLE, 16.0);
    let mut name = s.together.name.clone();
    if f.text_field("together/name", &mut name, "What others see", w)
        .changed
    {
        s.together.name = name.chars().take(24).collect();
    }
    small(f, "Start a session", t.ink, 18.0);
    small(f, "You host; friends join with your code.", SUBTLE, 16.0);
    let busy = s.together.busy;
    f.row(44.0, |f| {
        let kind = if busy { Kind::Secondary } else { Kind::Primary };
        if f.button("together/host", "Host", kind).clicked && !busy {
            actions.push(Action::Other(ops::HOST.into()));
        }
    });
    small(f, "Join a session", t.ink, 18.0);
    let mut code = s.together.code.clone();
    f.row(44.0, |f| {
        f.then_right();
        if f.button("together/join", "Join", Kind::Primary).clicked && !busy {
            actions.push(Action::Other(ops::JOIN.into()));
        }
        let fw = f.remaining().w;
        if f.text_field("together/code", &mut code, "Paste a pp1-… code", fw)
            .changed
        {
            s.together.code = code.clone();
        }
    });
    small(f, START_NOTE, SUBTLE, 16.0);
    if busy {
        small(f, "Connecting…", t.ink, 16.0);
    }
    if !s.together.error.is_empty() {
        let e = s.together.error.clone();
        small(f, &e, t.coral, 16.0);
    }
    f.row(44.0, |f| {
        f.then_right();
        if f.button("together/close", "Close", Kind::Ghost).clicked {
            actions.push(Action::Other(ops::CLOSE.into()));
        }
    });
}

/// In a session: the code, who's here, the shared layers, Leave.
fn live(f: &mut Frame, s: &mut State, actions: &mut Vec<Action>) {
    let t = f.theme();
    let Some(info) = s.together.info.clone() else {
        return;
    };
    title(f, "Draw together");
    small(
        f,
        if info.host {
            "You’re hosting. Send this code to the people you want to draw with:"
        } else {
            "You’re in. Anyone with this code can join too:"
        },
        SUBTLE,
        16.0,
    );
    let copied = s.together.copied;
    f.row(44.0, |f| {
        f.then_right();
        let label = if copied { "Copied" } else { "Copy" };
        if f.button("together/copy", label, Kind::Secondary).clicked {
            actions.push(Action::Other(ops::COPY.into()));
        }
        // the code itself, in a box (long: it runs off the end, clipped)
        let r = f.alloc(f.remaining().w, 0.0);
        f.put(
            Quad::rect(r.x, r.y, r.w, r.h, t.card)
                .radius(t.control_radius)
                .border(1.5, t.line),
        );
        let l = f.lay_out(&info.invite, &Style::body(13.0, t.ink), None);
        let start = f.here();
        f.put_text(&l, r.x + 12.0, r.y + (r.h - l.height) / 2.0);
        f.clip_from(start, Rect::new(r.x + 6.0, r.y, r.w - 12.0, r.h));
    });
    small(
        f,
        &format!("Here now ({})", info.members.len()),
        t.ink,
        18.0,
    );
    let r = f.alloc(0.0, members_h(info.members.len()));
    for (i, m) in info.members.iter().take(8).enumerate() {
        let y = r.y + i as f32 * 22.0;
        f.put(Quad::rect(r.x, y + 5.0, 12.0, 12.0, member_colour(&m.id)).radius(6.0));
        let name = if m.name.trim().is_empty() {
            "Someone"
        } else {
            m.name.as_str()
        };
        let you = if m.id == info.me.id { " (you)" } else { "" };
        let l = f.lay_out(
            &format!("{name}{you}"),
            &Style::body(13.0, t.ink).weight(600),
            None,
        );
        f.put_text(&l, r.x + 20.0, y + (22.0 - l.height) / 2.0);
    }
    if !s.together.synced {
        small(f, "Getting the drawing from the others…", t.ink, 16.0);
    } else if !s.together.rows.is_empty() {
        small(f, "Layers", t.ink, 18.0);
        let rows = s.together.rows.clone();
        let content: f32 = rows.iter().map(row_h).sum();
        let h = layers_h(&rows);
        f.scroll_area("together/layers", h, content, |f| {
            for r in &rows {
                layer_row(f, r, actions);
            }
        });
    }
    small(f, LIVE_NOTE, SUBTLE, 16.0);
    if !s.together.error.is_empty() {
        let e = s.together.error.clone();
        small(f, &e, t.coral, 16.0);
    }
    f.row(44.0, |f| {
        f.then_right();
        if f.button("together/back", "Back to drawing", Kind::Primary)
            .clicked
        {
            actions.push(Action::Other(ops::CLOSE.into()));
        }
        if f.button("together/leave", "Leave", Kind::Secondary).clicked {
            actions.push(Action::Other(ops::LEAVE.into()));
        }
    });
}

/// A shared layer: its name and owner; asking to draw on it; its owner's answers.
fn layer_row(f: &mut Frame, r: &Row, actions: &mut Vec<Action>) {
    let t = f.theme();
    let line = |f: &mut Frame, text: &str, sub: &str| {
        let l = f.lay_out(text, &Style::body(13.0, t.ink).weight(600), None);
        let o = f.lay_out(sub, &Style::body(12.0, SUBTLE), None);
        let rr = f.remaining();
        f.put_text(&l, rr.x, rr.y + (rr.h - l.height) / 2.0);
        f.put_text(&o, rr.x + l.width + 8.0, rr.y + (rr.h - o.height) / 2.0);
    };
    f.row(36.0, |f| {
        f.then_right();
        if r.can_draw && !r.mine {
            small_tag(f, "you can draw");
        } else if !r.can_draw {
            let label = if r.asked { "Asked" } else { "Ask to draw here" };
            if f.button(&format!("together/ask/{}", r.lid), label, Kind::Secondary)
                .clicked
                && !r.asked
            {
                actions.push(Action::Other(format!("{}{}", ops::ASK, r.lid)));
            }
        }
        line(f, &r.name, if r.mine { "yours" } else { &r.owner });
    });
    for (who, name, text, own, draw) in &r.asks {
        f.row(36.0, |f| {
            f.then_right();
            let k = format!("{} {who}", r.lid);
            if f.button(&format!("together/deny/{k}"), "No", Kind::Ghost)
                .clicked
            {
                actions.push(Action::Other(format!("{}{k}", ops::DENY)));
            }
            if (*own || *draw)
                && f.button(
                    &format!("together/hand/{k}"),
                    "Hand it over",
                    Kind::Secondary,
                )
                .clicked
            {
                actions.push(Action::Other(format!("{}{k}", ops::HAND)));
            }
            if !*own {
                let label = if *draw { "Let them draw" } else { "Allow" };
                if f.button(&format!("together/let/{k}"), label, Kind::Primary)
                    .clicked
                {
                    actions.push(Action::Other(format!("{}{k}", ops::LET)));
                }
            }
            line(f, name, text);
        });
    }
    if r.mine && !r.writers.is_empty() {
        f.row(36.0, |f| {
            f.then_right();
            for (who, name) in &r.writers {
                let k = format!("{} {who}", r.lid);
                if f.button(
                    &format!("together/revoke/{k}"),
                    &format!("Take back from {name}"),
                    Kind::Ghost,
                )
                .clicked
                {
                    actions.push(Action::Other(format!("{}{k}", ops::REVOKE)));
                }
            }
            line(f, "Also drawing", "");
        });
    }
}

fn small_tag(f: &mut Frame, text: &str) {
    let t = f.theme();
    let l = f.lay_out(text, &Style::body(12.0, t.ink).weight(600), None);
    let r = f.alloc(l.width + 20.0, 26.0);
    f.put(Quad::rect(r.x, r.y, r.w, r.h, hex(0xD9D2FF)).radius(13.0));
    f.put_text(&l, r.x + 10.0, r.y + (r.h - l.height) / 2.0);
}

/// Something to tell this person (an ask came, they said no…): a chip under the header.
fn note(f: &mut Frame, s: &mut State) {
    let Some((text, _)) = s.together.note.clone() else {
        return;
    };
    let t = f.theme();
    let anchor = s.header.layout.together;
    let l = f.lay_out(
        &text,
        &Style::body(13.0, t.on_dark).weight(600),
        Some(420.0),
    );
    let (w, h) = (l.width + 28.0, l.height + 16.0);
    let r = Rect::new((anchor.right() - w).max(8.0), anchor.bottom() + 10.0, w, h);
    if f.is_open(NAME) {
        return;
    }
    f.overlay(|f| {
        f.put(Quad::rect(r.x, r.y, r.w, r.h, t.ink).radius(h.min(32.0) / 2.0));
        f.put_text(&l, r.x + 14.0, r.y + 8.0);
    });
}

/// Over the canvas: others' cursors (a dot and their name in their colour) and the chat
/// card in its corner.
pub fn canvas(f: &mut Frame, outer: Rect, s: &mut State) {
    if !s.together.live() {
        return;
    }
    // (the document's area: the card inside its edge, as the view measures it)
    let card = outer.inset(12.0);
    let t = f.theme();
    for c in s.together.cursors.clone() {
        let (x, y) = s.view.to_screen(card, c.x, c.y);
        if !card.contains(x, y) {
            continue;
        }
        f.put(Quad::rect(x - 6.0, y - 6.0, 12.0, 12.0, t.card).radius(6.0));
        f.put(Quad::rect(x - 4.0, y - 4.0, 8.0, 8.0, c.colour).radius(4.0));
        let l = f.lay_out(&c.name, &Style::body(12.0, t.on_coral).weight(700), None);
        let (w, h) = (l.width + 14.0, l.height + 6.0);
        f.put(Quad::rect(x + 8.0, y + 6.0, w, h, c.colour).radius(h / 2.0));
        f.put_text(&l, x + 15.0, y + 9.0);
    }
    chat(f, card, s);
}

/// The chat: a small card in the canvas's corner; its button shows unread messages.
fn chat(f: &mut Frame, card: Rect, s: &mut State) {
    let t = f.theme();
    let open = s.together.chat_open;
    let n = s.together.chat.len().min(CHAT_LINES);
    let line_h = 20.0;
    let h = if open {
        40.0 + n as f32 * line_h + 8.0 + 44.0 + 12.0
    } else {
        40.0
    };
    let r = Rect::new(
        card.right() - 16.0 - CHAT_W,
        card.bottom() - 16.0 - h,
        CHAT_W,
        h,
    );
    let mut actions = Vec::new();
    f.overlay(|f| {
        f.block(r);
        f.put(
            Quad::rect(r.x + 2.0, r.y + 6.0, r.w, r.h, alpha(t.ink, 0.12))
                .radius(18.0)
                .shadow(12.0),
        );
        f.put(
            Quad::rect(r.x, r.y, r.w, r.h, t.card)
                .radius(18.0)
                .border(1.0, t.line),
        );
        let head = Rect::new(r.x, r.y, r.w, 40.0);
        let res = f.interact(id("together/chat/head"), head);
        if res.clicked {
            actions.push(Action::Other(ops::CHAT.into()));
        }
        let l = f.lay_out("Chat", &Style::body(14.0, t.ink).weight(700), None);
        f.put_text(&l, head.x + 16.0, head.y + (40.0 - l.height) / 2.0);
        if !open && s.together.unread > 0 {
            let b = f.lay_out(
                &s.together.unread.to_string(),
                &Style::body(12.0, t.on_coral).weight(700),
                None,
            );
            let bw = b.width + 14.0;
            let bx = head.x + 16.0 + l.width + 8.0;
            f.put(Quad::rect(bx, head.y + 10.0, bw, 20.0, t.coral).radius(10.0));
            f.put_text(&b, bx + 7.0, head.y + 10.0 + (20.0 - b.height) / 2.0);
        }
        if !open {
            return;
        }
        let from = s.together.chat.len() - n;
        for (i, c) in s.together.chat[from..].iter().enumerate() {
            let y = r.y + 40.0 + i as f32 * line_h;
            let (who, colour) = if c.system {
                (String::new(), SUBTLE)
            } else if c.mine {
                ("You ".to_string(), member_colour(&c.from))
            } else {
                (format!("{} ", c.name), member_colour(&c.from))
            };
            let a = f.lay_out(&who, &Style::body(13.0, colour).weight(700), None);
            let b = f.lay_out(
                &c.text,
                &Style::body(13.0, if c.system { SUBTLE } else { t.ink }),
                None,
            );
            let start = f.here();
            f.put_text(&a, r.x + 16.0, y);
            f.put_text(&b, r.x + 16.0 + a.width, y);
            f.clip_from(start, Rect::new(r.x + 8.0, y, r.w - 16.0, line_h));
        }
        let fr = Rect::new(r.x + 12.0, r.bottom() - 12.0 - 44.0, r.w - 24.0, 44.0);
        let mut text = s.together.chat_text.clone();
        f.column_in(fr, 0.0, |f| {
            if f.text_field("together/chat/text", &mut text, "Say something", fr.w)
                .changed
            {
                s.together.chat_text = text.chars().take(500).collect();
            }
        });
        if f.typing() && f.take_key("Return", false) {
            actions.push(Action::Other(ops::SAY.into()));
        }
    });
    s.actions.extend(actions);
}
