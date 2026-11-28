//! The export dialog: a pop-up card under the header's Export button (the design has none:
//! it's made in the design's language, a white card, pill tabs, the coral main action).
//! Its settings live in [`State::export`]; its buttons push the export's actions
//! ([`crate::shell::engine::export::ops`]), which the engine carries out.
//!
//! Ctrl+E opens it too (as File › Export…).

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Kind, Rect};

use crate::shell::engine::export::{
    FORMATS, Format, SIZES, Status, format_name, ops, out_size, range,
};
use crate::state::{Action, State};

const NAME: &str = "export/dialog";
const WIDTH: f32 = 440.0;
/// the card while exporting and once done
const CARD_WIDTH: f32 = 320.0;
const PAD: f32 = 18.0;
const GAP: f32 = 12.0;
const SUBTLE: [f32; 4] = hex(0x5E5D63);

/// The dialog, when it's up (and Ctrl+E). Drawn last, above everything.
pub fn build(f: &mut Frame, s: &mut State) {
    if !f.typing() && f.take_key("e", true) {
        s.actions.push(Action::File("Export…".into()));
    }
    let x = &mut s.export;
    if x.open && matches!(x.status, Status::Running { .. } | Status::Done(_)) {
        // while it runs, and once it's done: a card that leaves the canvas free (a pop-up
        // would take every press, and the window must go on being drawn in)
        if f.is_open(NAME) {
            f.close();
        }
        x.was_open = false;
        card(f, s);
        return;
    }
    let shown = f.is_open(NAME);
    if x.open && !shown {
        if x.was_open && !x.busy() {
            // a click elsewhere or Escape put it away
            x.open = false;
            x.was_open = false;
            s.actions.push(Action::Other(ops::CLOSE.into()));
            return;
        }
        f.open(NAME, 0.0, 0.0);
    } else if !x.open && shown {
        f.close();
    }
    x.was_open = x.open;
    if !x.open {
        return;
    }
    let anchor = s.header.layout.export;
    let h = height(f, s);
    let rect = Rect::new(
        (anchor.right() - WIDTH).max(8.0),
        anchor.bottom() + 10.0,
        WIDTH,
        h,
    );
    let mut actions = Vec::new();
    // (the card lays its content out in a column, GAP apart)
    f.popup_at(NAME, rect, 22.0, PAD, 1.0, |f| {
        match s.export.status.clone() {
            Status::Running { done, total } => running(f, done, total, &mut actions),
            Status::Done(p) => done(f, &p, &mut actions),
            Status::Failed(m) => failed(f, &m, &mut actions),
            Status::Idle | Status::Asking => settings(f, s, &mut actions),
        }
    });
    s.actions.extend(actions);
}

/// The card while exporting and once done: not a pop-up (it takes only presses on itself),
/// narrower, over the side panel.
fn card(f: &mut Frame, s: &mut State) {
    let anchor = s.header.layout.export;
    let w = CARD_WIDTH;
    let h = height(f, s);
    let rect = Rect::new((anchor.right() - w).max(8.0), anchor.bottom() + 10.0, w, h);
    let mut actions = Vec::new();
    let status = s.export.status.clone();
    f.overlay(|f| {
        let t = f.theme();
        f.block(rect);
        f.put(
            Quad::rect(
                rect.x + 2.0,
                rect.y + 8.0,
                rect.w,
                rect.h,
                alpha(t.ink, 0.14),
            )
            .radius(22.0)
            .shadow(18.0),
        );
        f.put(
            Quad::rect(rect.x, rect.y, rect.w, rect.h, t.card)
                .radius(22.0)
                .border(1.0, t.line),
        );
        f.column_in(rect.inset(PAD), GAP, |f| match status {
            Status::Running { done: d, total } => running(f, d, total, &mut actions),
            Status::Done(p) => done(f, &p, &mut actions),
            _ => {}
        });
    });
    s.actions.extend(actions);
}

/// The card's height for what it shows.
fn height(f: &mut Frame, s: &State) -> f32 {
    let x = &s.export;
    let rows: Vec<f32> = match &x.status {
        Status::Running { .. } => vec![28.0, 18.0, 8.0, 44.0],
        Status::Done(p) => {
            let inner = CARD_WIDTH - 2.0 * PAD;
            let lines = |f: &mut Frame, t: &str| {
                f.lay_out(t, &Style::body(13.0, SUBTLE).weight(600), Some(inner))
                    .height
            };
            let name = p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let dir = p
                .parent()
                .map_or_else(String::new, |d| d.display().to_string());
            let a = lines(f, &format!("Exported to {name}")).max(18.0);
            let b = lines(f, &dir).max(16.0);
            vec![28.0, a, b, 44.0]
        }
        Status::Failed(m) => {
            let l = f.lay_out(m, &Style::body(13.0, SUBTLE), Some(WIDTH - 2.0 * PAD));
            vec![28.0, l.height, 44.0]
        }
        _ if x.format == Format::Psd => {
            let mut v = vec![28.0, 16.0, 44.0, 16.0, 16.0];
            if note(x).is_some() {
                v.push(16.0);
            }
            v.push(44.0);
            v
        }
        _ => {
            let mut v = vec![28.0, 16.0, 44.0, 16.0, 44.0, 16.0, 44.0, 16.0];
            if x.can_be_transparent() {
                v.push(26.0);
            }
            if note(x).is_some() {
                v.push(16.0);
            }
            v.push(44.0);
            v
        }
    };
    rows.iter().sum::<f32>() + GAP * (rows.len() as f32 - 1.0) + 2.0 * PAD + 2.0
}

/// What the dialog says about FFmpeg, if anything.
fn note(x: &crate::shell::engine::export::ExportUi) -> Option<&'static str> {
    match &x.videos {
        None => Some("Looking for FFmpeg…"),
        Some(v) if v.is_empty() => Some("MP4 and WebM need FFmpeg, which isn't installed."),
        Some(v) if !v.contains(&Format::Webm) => Some("This FFmpeg can't write WebM."),
        _ => None,
    }
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

fn settings(f: &mut Frame, s: &mut State, actions: &mut Vec<Action>) {
    let t = f.theme();
    title(f, "Export");
    // the format: the ones that can be written here
    small(f, "Format", SUBTLE, 16.0);
    let offered: Vec<Format> = FORMATS
        .into_iter()
        .filter(|fm| s.export.available(*fm) || s.export.videos.is_none())
        .collect();
    let names: Vec<&str> = offered.iter().map(|fm| format_name(*fm)).collect();
    let mut picked = offered
        .iter()
        .position(|fm| *fm == s.export.format)
        .unwrap_or(0);
    if f.tabs("export/format", &names, &mut picked).changed {
        s.export.format = offered[picked];
    }
    if s.export.format == Format::Psd {
        // the frame showing, at the document's size, with its layers
        small(f, "This frame", SUBTLE, 16.0);
        small(
            f,
            &format!(
                "Frame {} · {} × {} · layers, groups and masks",
                s.frame + 1,
                s.view.doc_w,
                s.view.doc_h
            ),
            t.ink,
            16.0,
        );
        if let Some(n) = note(&s.export) {
            small(f, n, SUBTLE, 16.0);
        }
        buttons(f, s, actions);
        return;
    }
    // the frames: the cut, or the loop range
    let (first, last) = range(s, true);
    small(f, "Frames", SUBTLE, 16.0);
    let cut = format!("Whole cut · {}", s.frames);
    let mut items = vec![cut.as_str()];
    let looped = format!("Loop range · {}–{}", first + 1, last + 1);
    if s.sheet.loop_range.is_some() {
        items.push(&looped);
    } else {
        s.export.loop_only = false;
    }
    let mut which = s.export.loop_only as usize;
    if f.tabs("export/range", &items, &mut which).changed {
        s.export.loop_only = which == 1;
    }
    // the size
    small(f, "Size", SUBTLE, 16.0);
    let names: Vec<&str> = SIZES.iter().map(|z| z.0).collect();
    let mut size = s.export.size.min(SIZES.len() - 1);
    if f.tabs("export/size", &names, &mut size).changed {
        s.export.size = size;
    }
    let video = matches!(s.export.format, Format::Mp4 | Format::Webm);
    let (w, h) = out_size(s.export.size, s.view.doc_w, s.view.doc_h, video);
    let (a, b) = range(s, s.export.loop_only);
    small(
        f,
        &format!("{w} × {h} · {} fps · {} frames", s.fps, b - a + 1),
        t.ink,
        16.0,
    );
    if s.export.can_be_transparent() {
        let mut on = s.export.transparent;
        if f.toggle("export/transparent", "Transparent background", &mut on)
            .changed
        {
            s.export.transparent = on;
        }
    }
    if let Some(n) = note(&s.export) {
        small(f, n, SUBTLE, 16.0);
    }
    buttons(f, s, actions);
}

/// Export… and Cancel.
fn buttons(f: &mut Frame, s: &State, actions: &mut Vec<Action>) {
    let asking = s.export.status == Status::Asking;
    let can = s.export.available(s.export.format) && !asking;
    f.row(44.0, |f| {
        f.then_right();
        let label = if asking {
            "Choosing a file…"
        } else {
            "Export…"
        };
        let kind = if can { Kind::Primary } else { Kind::Secondary };
        if f.button("export/go", label, kind).clicked && can {
            actions.push(Action::Other(ops::START.into()));
        }
        let back = if asking { ops::CANCEL } else { ops::CLOSE };
        if f.button("export/close", "Cancel", Kind::Ghost).clicked {
            actions.push(Action::Other(back.into()));
        }
    });
}

fn running(f: &mut Frame, done: u32, total: u32, actions: &mut Vec<Action>) {
    let t = f.theme();
    title(f, "Exporting…");
    small(
        f,
        &format!("Frame {} of {total}", done.min(total)),
        t.ink,
        18.0,
    );
    let r = f.alloc(0.0, 8.0);
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.line).radius(4.0));
    let u = if total > 0 {
        done as f32 / total as f32
    } else {
        0.0
    };
    if u > 0.0 {
        f.put(Quad::rect(r.x, r.y, (r.w * u).max(8.0), r.h, t.coral).radius(4.0));
    }
    f.row(44.0, |f| {
        f.then_right();
        if f.button("export/cancel", "Cancel", Kind::Secondary).clicked {
            actions.push(Action::Other(ops::CANCEL.into()));
        }
    });
}

fn done(f: &mut Frame, p: &std::path::Path, actions: &mut Vec<Action>) {
    let t = f.theme();
    title(f, "Exported");
    let name = p
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let dir = p
        .parent()
        .map_or_else(String::new, |d| d.display().to_string());
    small(f, &format!("Exported to {name}"), t.ink, 18.0);
    small(f, &dir, alpha(SUBTLE, 0.9), 16.0);
    f.row(44.0, |f| {
        f.then_right();
        if f.button("export/done", "Done", Kind::Primary).clicked {
            actions.push(Action::Other(ops::CLOSE.into()));
        }
        if f.button("export/show", "Show in folder", Kind::Secondary)
            .clicked
        {
            actions.push(Action::Other(ops::SHOW.into()));
        }
    });
}

fn failed(f: &mut Frame, m: &str, actions: &mut Vec<Action>) {
    title(f, "Couldn't export");
    small(f, m, SUBTLE, 16.0);
    f.row(44.0, |f| {
        f.then_right();
        if f.button("export/fail-back", "Back", Kind::Primary).clicked {
            actions.push(Action::Other(ops::BACK.into()));
        }
        if f.button("export/fail-close", "Close", Kind::Ghost).clicked {
            actions.push(Action::Other(ops::CLOSE.into()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Workspace;

    /// The dialog in each of its states, drawn off-screen (no window): `PP_SHOTS` = where.
    #[test]
    #[ignore = "pictures: PP_SHOTS=dir cargo test -p pencilpilot-native -- --ignored export_dialog_pictures"]
    fn export_dialog_pictures() {
        let dir = std::env::var("PP_SHOTS").expect("PP_SHOTS");
        let base = || {
            let mut s = State::demo();
            s.workspace = Workspace::Animate;
            s.export.open = true;
            s.export.videos = Some(vec![Format::Mp4, Format::Webm]);
            s
        };
        let mut shots: Vec<(&str, State)> = Vec::new();
        shots.push(("settings-mp4", base()));
        let mut s = base();
        s.export.format = Format::Png;
        s.export.transparent = true;
        s.export.size = 3;
        s.sheet.loop_range = Some((4, 15));
        s.export.loop_only = true;
        shots.push(("settings-png-loop", s));
        let mut s = base();
        s.export.videos = Some(Vec::new());
        s.export.format = Format::Gif;
        shots.push(("no-ffmpeg", s));
        let mut s = base();
        s.export.status = Status::Running {
            done: 17,
            total: 48,
        };
        shots.push(("running", s));
        let mut s = base();
        s.export.status = Status::Done("/home/ana/Films/Ember/Cut 2.mp4".into());
        shots.push(("done", s));
        let mut s = base();
        s.export.status = Status::Failed("ffmpeg stopped: No space left on device".into());
        shots.push(("failed", s));
        let mut s = base();
        s.export.format = Format::Psd;
        shots.push(("settings-psd", s));
        for (name, mut s) in shots {
            crate::shell::render_png(&mut s, 1440, 900, 1.0, &format!("{dir}/export-{name}.png"))
                .unwrap();
        }
    }
}
