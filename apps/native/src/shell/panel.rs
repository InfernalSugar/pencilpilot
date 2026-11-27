//! The side panel: a white card whose tabs and content change with the workspace.
//!
//! - Animate: Colour (the saturation/value field and the palette as round swatches),
//!   Brushes, Layer.
//! - Draw: Brushes (the brush list, size, opacity and steady lines), Colour, Layer.
//! - Colour: Colour (the palette as named swatches and the fill options), Brushes, Layer.
//! - Review: no tabs; the review notes and a field to add one.
//!
//! Brushes, palette colours and notes have right-click menus.

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::{Laid, Style};
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Rect, id};

use crate::state::{Action, Note, State, Workspace};

/// The card's padding and the gap between its parts.
const PAD: f32 = 16.0;
const GAP: f32 = 14.0;
const TABS_BG: [f32; 4] = hex(0xEFECE4);
const TAB_FG: [f32; 4] = hex(0x44434A);
const MUTED: [f32; 4] = hex(0x5E5D63);
/// a picked row's fill (a brush, a named colour, a layer, a note)
const PICKED: [f32; 4] = hex(0xF6F4EF);
const NOTE_PICKED: [f32; 4] = hex(0xFFF4D1);
const RING: [f32; 4] = hex(0xFFD84D);
const CHECK_OFF: [f32; 4] = hex(0x8C8B92);
const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const LOCK: &str = "M6 11h12v9H6Z M8.5 11V8a3.5 3.5 0 0 1 7 0v3";

/// The right-click menus' names.
const BRUSH_MENU: &str = "panel/brush-menu";
const COLOUR_MENU: &str = "panel/colour-menu";
const NOTE_MENU: &str = "panel/note-menu";

/// Where something was drawn this pass, for tests to aim at (nothing outside tests).
macro_rules! hit {
    ($s:expr, $r:expr, $($name:tt)*) => {{
        #[cfg(test)]
        $s.panel.hits.push((format!($($name)*), $r));
        #[cfg(not(test))]
        let _ = (&$s, $r);
    }};
}

/// This piece's own state (anything it needs that the rest of the app doesn't).
#[derive(Debug, Clone)]
pub struct PanelState {
    /// each brush's size (the picked one's is `State::brush_size`)
    pub sizes: Vec<f32>,
    /// the brush's opacity and line steadying, percent
    pub opacity: f32,
    pub steady: f32,
    /// the colour field's hue, degrees (kept while the colour is a grey, which has none)
    pub hue: f32,
    /// the Colour workspace's fill options
    pub close_gaps: bool,
    pub fill_under: bool,
    /// the picked review note (None: the first on the current frame)
    pub note: Option<usize>,
    /// the note being typed
    pub draft: String,
    /// what the open right-click menu is for (a brush, colour or note index)
    menu_for: usize,
    /// the window (logical), to keep menus inside it
    screen: Rect,
    /// where the last right-click menu opened (tests aim at its items)
    #[cfg(test)]
    menu_at: Rect,
    #[cfg(test)]
    hits: Vec<(String, Rect)>,
}

impl Default for PanelState {
    fn default() -> Self {
        PanelState {
            sizes: Vec::new(),
            opacity: 100.0,
            steady: 30.0,
            // the coral of the design's field
            hue: rgb_to_hsv(hex(0xE8492F))[0],
            close_gaps: true,
            fill_under: true,
            note: None,
            draft: String::new(),
            menu_for: 0,
            screen: Rect::default(),
            #[cfg(test)]
            menu_at: Rect::default(),
            #[cfg(test)]
            hits: Vec::new(),
        }
    }
}

/// What a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    Colour,
    Brushes,
    Layers,
    Notes,
}

/// The panel's tabs in a workspace (none in Review).
pub fn tabs_for(w: Workspace) -> &'static [(&'static str, Content)] {
    match w {
        Workspace::Draw => &[
            ("Brushes", Content::Brushes),
            ("Colour", Content::Colour),
            ("Layer", Content::Layers),
        ],
        Workspace::Animate | Workspace::Colour => &[
            ("Colour", Content::Colour),
            ("Brushes", Content::Brushes),
            ("Layer", Content::Layers),
        ],
        Workspace::Review => &[],
    }
}

/// What the panel shows now.
pub fn content(s: &State) -> Content {
    let tabs = tabs_for(s.workspace);
    if tabs.is_empty() {
        return Content::Notes;
    }
    tabs[s.panel_tab[s.workspace.index()].min(tabs.len() - 1)].1
}

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    #[cfg(test)]
    s.panel.hits.clear();
    // (the panel is built at the top level: what's left of it is the window)
    s.panel.screen = f.remaining();
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.card).radius(24.0));
    let inner = r.inset(PAD);
    let mut y = inner.y;
    let tabs = tabs_for(s.workspace);
    if !tabs.is_empty() {
        let ws = s.workspace.index();
        let mut tab = s.panel_tab[ws].min(tabs.len() - 1);
        tab_bar(f, Rect::new(inner.x, y, inner.w, 44.0), tabs, &mut tab, s);
        s.panel_tab[ws] = tab;
        y += 44.0 + GAP;
    }
    let body = Rect::new(inner.x, y, inner.w, (inner.bottom() - y).max(0.0));
    match content(s) {
        Content::Colour if s.workspace == Workspace::Colour => named_palette(f, body, s),
        Content::Colour => colour_field(f, body, s),
        Content::Brushes => brushes(f, body, s),
        Content::Layers => layers(f, body, s),
        Content::Notes => notes(f, body, s),
    }
    menus(f, s);
}

// ---- the tabs ----

fn tab_bar(f: &mut Frame, r: Rect, tabs: &[(&str, Content)], picked: &mut usize, s: &mut State) {
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, TABS_BG).radius(14.0));
    let n = tabs.len() as f32;
    let w = (r.w - 8.0 - 4.0 * (n - 1.0)) / n;
    for (i, (label, _)) in tabs.iter().enumerate() {
        let tr = Rect::new(r.x + 4.0 + i as f32 * (w + 4.0), r.y + 4.0, w, 36.0);
        hit!(s, tr, "tab/{i}");
        let res = f.interact(id(&format!("panel/tab/{i}")), tr);
        if res.clicked {
            *picked = i;
        }
        let on = *picked == i;
        if on {
            f.put(Quad::rect(tr.x, tr.y, tr.w, tr.h, t.card).radius(10.0));
        } else if res.hovered {
            f.put(Quad::rect(tr.x, tr.y, tr.w, tr.h, alpha(t.card, 0.5)).radius(10.0));
        }
        let fg = if on { t.ink } else { TAB_FG };
        let laid = f.lay_out(label, &Style::body(13.0, fg).weight(600), None);
        f.put_text(
            &laid,
            tr.x + (tr.w - laid.width) / 2.0,
            tr.y + (tr.h - laid.height) / 2.0,
        );
    }
}

// ---- shared parts ----

/// A heading (the display face, 17 px) with a quiet note on its right; returns its height.
fn heading(f: &mut Frame, x: f32, y: f32, w: f32, text: &str, note: &str) -> f32 {
    let t = f.theme();
    let h = f.lay_out(text, &Style::display(17.0, t.ink), None);
    let n = f.lay_out(note, &Style::body(12.0, MUTED).weight(600), None);
    let height = h.height.max(n.height);
    f.put_text(&h, x, y + height - h.height);
    // the note's baseline near the heading's
    f.put_text(&n, x + w - n.width, y + height - n.height - 1.0);
    height
}

fn count(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// A quiet line at the foot of the panel; returns where it starts.
fn footer(f: &mut Frame, body: Rect, text: &str) -> f32 {
    let mut st = Style::body(12.0, MUTED);
    st.line_height = 1.5;
    let laid = f.lay_out(text, &st, Some(body.w));
    let y = body.bottom() - laid.height;
    f.put_text(&laid, body.x, y);
    y
}

/// A list in `area`, `content_h(width)` tall: drawn as is when it fits, in a scroll area
/// when not. `draw` gets the list's rectangle (moved by the scroll).
fn list(
    f: &mut Frame,
    name: &str,
    area: Rect,
    content_h: impl Fn(&mut Frame, f32) -> f32,
    draw: impl FnOnce(&mut Frame, Rect),
) {
    let h = content_h(f, area.w);
    if h <= area.h + 0.5 {
        draw(f, Rect::new(area.x, area.y, area.w, h));
        return;
    }
    // the scroll area keeps 10 px on the right for its bar
    let h = content_h(f, area.w - 10.0);
    f.column_in(area, 0.0, |f| {
        f.scroll_area(name, area.h, h, |f| {
            let r = f.alloc(0.0, h);
            draw(f, r);
        })
    });
}

/// A slider as the design's: a label and its value above a coral track. Returns whether
/// the value changed.
#[allow(clippy::too_many_arguments)]
fn slider(
    f: &mut Frame,
    name: &str,
    r: Rect,
    label: &str,
    shown: &str,
    value: &mut f32,
    min: f32,
    max: f32,
) -> bool {
    let t = f.theme();
    let l = f.lay_out(label, &Style::body(13.0, t.ink).weight(600), None);
    let v = f.lay_out(shown, &Style::body(13.0, MUTED).weight(600), None);
    f.put_text(&l, r.x, r.y);
    f.put_text(&v, r.right() - v.width, r.y);
    let track = Rect::new(r.x, r.y + l.height + 6.0, r.w, 20.0);
    let res = f.interact(id(name), track);
    let mut changed = false;
    let k = 16.0;
    if res.held {
        let u = ((f.pointer().x - track.x - k / 2.0) / (track.w - k)).clamp(0.0, 1.0);
        let nv = (min + u * (max - min)).round();
        if nv != *value {
            *value = nv;
            changed = true;
        }
    }
    let u = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let cy = track.y + track.h / 2.0;
    let kx = track.x + k / 2.0 + (track.w - k) * u;
    f.put(Quad::rect(track.x, cy - 2.5, track.w, 5.0, t.line).radius(2.5));
    f.put(Quad::rect(track.x, cy - 2.5, kx - track.x, 5.0, t.coral).radius(2.5));
    let k = if res.held || res.hovered { 18.0 } else { k };
    f.put(Quad::rect(kx - k / 2.0, cy - k / 2.0, k, k, t.coral).radius(k / 2.0));
    changed
}

/// The height of a slider with its label.
fn slider_height(f: &mut Frame) -> f32 {
    let t = f.theme();
    f.lay_out("Size", &Style::body(13.0, t.ink).weight(600), None)
        .height
        + 6.0
        + 20.0
}

/// A label with a checkbox on its right; clicking the row flips it.
fn checkbox(f: &mut Frame, name: &str, r: Rect, label: &str, on: &mut bool) -> bool {
    let t = f.theme();
    let res = f.interact(id(name), r);
    if res.clicked {
        *on = !*on;
    }
    let l = f.lay_out(label, &Style::body(13.0, t.ink).weight(600), None);
    f.put_text(&l, r.x, r.y + (r.h - l.height) / 2.0);
    let b = Rect::new(r.right() - 20.0, r.y + (r.h - 20.0) / 2.0, 20.0, 20.0);
    if *on {
        f.put(Quad::rect(b.x, b.y, b.w, b.h, t.coral).radius(4.0));
        f.icon(CHECK, b.x + 2.0, b.y + 2.0, 16.0, t.card);
    } else {
        let border = if res.hovered { t.ink } else { CHECK_OFF };
        f.put(
            Quad::rect(b.x, b.y, b.w, b.h, t.card)
                .radius(4.0)
                .border(1.5, border),
        );
    }
    res.clicked
}

/// A dark button (Post, New layer); returns whether it was clicked.
fn dark_button(f: &mut Frame, name: &str, r: Rect, text: &str) -> bool {
    let t = f.theme();
    let res = f.interact(id(name), r);
    let fill = if res.held {
        mix(t.ink, t.card, 0.25)
    } else if res.hovered {
        mix(t.ink, t.card, 0.12)
    } else {
        t.ink
    };
    f.put(Quad::rect(r.x, r.y, r.w, r.h, fill).radius(10.0));
    let l = f.lay_out(text, &Style::body(13.0, t.on_dark).weight(600), None);
    f.put_text(
        &l,
        r.x + (r.w - l.width) / 2.0,
        r.y + (r.h - l.height) / 2.0,
    );
    res.clicked
}

// ---- Colour ----

/// The colour field (saturation across, value up) and the palette as round swatches.
fn colour_field(f: &mut Frame, body: Rect, s: &mut State) {
    let t = f.theme();
    let field = Rect::new(body.x, body.y, body.w, 120.0);
    hit!(s, field, "field");
    let res = f.interact(id("panel/field"), field);
    let [h0, sat0, val0] = rgb_to_hsv(s.colour);
    if sat0 > 0.25 && val0 > 0.2 {
        s.panel.hue = h0;
    }
    if res.held {
        let p = f.pointer();
        let sat = ((p.x - field.x) / field.w).clamp(0.0, 1.0);
        let val = 1.0 - ((p.y - field.y) / field.h).clamp(0.0, 1.0);
        let c = hsv_to_rgb(s.panel.hue, sat, val);
        if c != s.colour {
            s.colour = c;
            s.colour_name = hex_name(c);
        }
    }
    draw_field(f, field, s.panel.hue);
    // where the colour is in the field
    let [_, sat, val] = rgb_to_hsv(s.colour);
    let (kx, ky) = (field.x + sat * field.w, field.y + (1.0 - val) * field.h);
    let (kx, ky) = (
        kx.clamp(field.x + 9.0, field.right() - 9.0),
        ky.clamp(field.y + 9.0, field.bottom() - 9.0),
    );
    let k = 16.0;
    f.put(
        Quad::rect(kx - k / 2.0, ky - k / 2.0, k, k, s.colour)
            .radius(k / 2.0)
            .border(3.0, t.card),
    );
    f.put(
        Quad::rect(
            kx - k / 2.0 - 1.0,
            ky - k / 2.0 - 1.0,
            k + 2.0,
            k + 2.0,
            [0.0; 4],
        )
        .radius(k / 2.0 + 1.0)
        .border(1.0, alpha(t.ink, 0.3)),
    );

    let mut y = field.bottom() + GAP;
    let n = s.palette.len();
    y += heading(
        f,
        body.x,
        y,
        body.w,
        &s.palette_name.clone(),
        &count(n, "colour"),
    ) + GAP;
    // six to a row, 8 apart, round
    let gap = 8.0;
    let rows = n.div_ceil(6);
    let area = Rect::new(body.x, y, body.w, (body.bottom() - y).max(0.0));
    let side = move |w: f32| (w - 5.0 * gap) / 6.0;
    list(
        f,
        "panel/swatches",
        area,
        |_, w| rows as f32 * (side(w) + gap) - gap,
        |f, r| {
            let d = side(r.w);
            for i in 0..n {
                let (col, row) = ((i % 6) as f32, (i / 6) as f32);
                let sr = Rect::new(r.x + col * (d + gap), r.y + row * (d + gap), d, d);
                swatch_round(f, sr, i, s);
            }
        },
    );
}

/// The saturation/value field at `hue`: the hue, white fading out to the right, black
/// fading in to the bottom (fine strips: quads have no gradients), with rounded corners.
fn draw_field(f: &mut Frame, r: Rect, hue: f32) {
    let t = f.theme();
    let radius = 18.0;
    f.put(Quad::rect(r.x, r.y, r.w, r.h, hsv_to_rgb(hue, 1.0, 1.0)).radius(radius));
    let step = 2.0;
    let n = (r.w / step).ceil() as usize;
    for i in 0..n {
        let x = r.x + i as f32 * step;
        let a = 1.0 - (i as f32 + 0.5) / n as f32;
        f.put(Quad::rect(
            x,
            r.y,
            step.min(r.right() - x),
            r.h,
            alpha(hex(0xFFFFFF), a),
        ));
    }
    let n = (r.h / step).ceil() as usize;
    for i in 0..n {
        let y = r.y + i as f32 * step;
        let a = (i as f32 + 0.5) / n as f32;
        f.put(Quad::rect(
            r.x,
            y,
            r.w,
            step.min(r.bottom() - y),
            alpha(hex(0x000000), a),
        ));
    }
    // the corners: a card-coloured frame whose inside edge is the field's rounded edge
    // (8 px reach the square corners: 18 × (√2 − 1) ≈ 7.5, and stay inside the card)
    let b = 8.0;
    f.put(
        Quad::rect(r.x - b, r.y - b, r.w + 2.0 * b, r.h + 2.0 * b, [0.0; 4])
            .radius(radius + b)
            .border(b, t.card),
    );
}

/// A round palette swatch; the picked one ringed.
fn swatch_round(f: &mut Frame, r: Rect, i: usize, s: &mut State) {
    let t = f.theme();
    hit!(s, r, "swatch/{i}");
    let res = f.interact(id(&format!("panel/swatch/{i}")), r);
    context_menu(f, s, COLOUR_MENU, r, i, res.context);
    if res.clicked {
        pick_colour(s, i);
    }
    let (cname, c) = &s.palette[i];
    let on = *cname == s.colour_name && *c == s.colour;
    let q = Quad::rect(r.x, r.y, r.w, r.h, *c).radius(r.w / 2.0);
    if on {
        f.put(q.border(3.0, RING));
    } else {
        f.put(q.border(1.0, alpha(t.ink, 0.15)));
        if res.hovered {
            f.put(
                Quad::rect(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0, [0.0; 4])
                    .radius(r.w / 2.0 + 2.0)
                    .border(2.0, alpha(t.ink, 0.2)),
            );
        }
    }
}

fn pick_colour(s: &mut State, i: usize) {
    let (name, c) = s.palette[i].clone();
    s.colour = c;
    s.colour_name = name;
}

/// The Colour workspace's palette: named swatches two to a row, the fill options, a hint.
fn named_palette(f: &mut Frame, body: Rect, s: &mut State) {
    let t = f.theme();
    let n = s.palette.len();
    let mut y = body.y;
    y += heading(
        f,
        body.x,
        y,
        body.w,
        &s.palette_name.clone(),
        &count(n, "colour"),
    ) + GAP;
    // from the foot: the hint, then the two options above it
    let foot = footer(
        f,
        body,
        "Right-click a colour to rename it, swap it everywhere or lock it.",
    );
    let opt_h = 24.0;
    let (gap, row_h) = (6.0, 44.0);
    let rows = n.div_ceil(2);
    let grid_h = rows as f32 * (row_h + gap) - gap;
    // the options right under the grid (the grid scrolls when they'd meet the hint)
    let room = foot - GAP - 2.0 * opt_h - GAP - GAP - y;
    let list_h = grid_h.min(room).max(0.0);
    let opts_y = y + list_h + GAP;
    let o1 = Rect::new(body.x, opts_y, body.w, opt_h);
    hit!(s, o1, "close-gaps");
    let mut close = s.panel.close_gaps;
    if checkbox(f, "panel/close-gaps", o1, "Close small gaps", &mut close) {
        s.actions
            .push(Action::Other(format!("Close small gaps: {close}")));
    }
    s.panel.close_gaps = close;
    let o2 = Rect::new(body.x, opts_y + opt_h + GAP, body.w, opt_h);
    let mut under = s.panel.fill_under;
    if checkbox(
        f,
        "panel/fill-under",
        o2,
        "Fill under the lines",
        &mut under,
    ) {
        s.actions
            .push(Action::Other(format!("Fill under the lines: {under}")));
    }
    s.panel.fill_under = under;

    let area = Rect::new(body.x, y, body.w, list_h);
    list(
        f,
        "panel/named",
        area,
        |_, _| rows as f32 * (row_h + gap) - gap,
        |f, r| {
            let w = (r.w - gap) / 2.0;
            for i in 0..n {
                let (col, row) = ((i % 2) as f32, (i / 2) as f32);
                let sr = Rect::new(r.x + col * (w + gap), r.y + row * (row_h + gap), w, row_h);
                hit!(s, sr, "named/{i}");
                let res = f.interact(id(&format!("panel/named/{i}")), sr);
                context_menu(f, s, COLOUR_MENU, sr, i, res.context);
                if res.clicked {
                    pick_colour(s, i);
                }
                let (cname, c) = &s.palette[i];
                let on = *cname == s.colour_name && *c == s.colour;
                if on {
                    f.put(Quad::rect(sr.x, sr.y, sr.w, sr.h, PICKED).radius(12.0));
                } else if res.hovered {
                    f.put(Quad::rect(sr.x, sr.y, sr.w, sr.h, alpha(PICKED, 0.6)).radius(12.0));
                }
                let d = 26.0;
                f.put(
                    Quad::rect(sr.x + 8.0, sr.y + (row_h - d) / 2.0, d, d, *c)
                        .radius(d / 2.0)
                        .border(1.0, alpha(t.ink, 0.15)),
                );
                let l = f.lay_out(
                    cname,
                    &Style::body(12.0, t.ink).weight(600),
                    Some(sr.w - 50.0),
                );
                f.put_text(&l, sr.x + 8.0 + d + 8.0, sr.y + (row_h - l.height) / 2.0);
            }
        },
    );
}

// ---- Brushes ----

/// How a brush's preview stroke looks (its width and opacity), from its name.
fn brush_look(name: &str) -> (f32, f32) {
    let n = name.to_lowercase();
    if n.contains("air") {
        (9.0, 0.35)
    } else if n.contains("marker") {
        (6.0, 0.9)
    } else if n.contains("liner") || n.contains("ink") {
        (3.0, 1.0)
    } else if n.contains("rough") {
        (2.5, 0.6)
    } else {
        (2.0, 0.85)
    }
}

/// A brush's starting size, from its name.
fn brush_size(name: &str) -> f32 {
    let n = name.to_lowercase();
    if n.contains("air") {
        40.0
    } else if n.contains("marker") {
        18.0
    } else if n.contains("liner") || n.contains("ink") {
        6.0
    } else if n.contains("rough") {
        10.0
    } else {
        12.0
    }
}

/// The design's preview stroke (a wave in a 64 × 20 box), stamped as round dots.
fn brush_stroke(f: &mut Frame, x: f32, y: f32, width: f32, color: [f32; 4]) {
    // "M2 14 C16 2 28 18 40 8 S58 6 62 10"
    let curves: [[(f32, f32); 4]; 2] = [
        [(2.0, 14.0), (16.0, 2.0), (28.0, 18.0), (40.0, 8.0)],
        [(40.0, 8.0), (52.0, -2.0), (58.0, 6.0), (62.0, 10.0)],
    ];
    let step = (width * 0.3).max(0.75);
    let d = width;
    for c in curves {
        let len: f32 = (0..3)
            .map(|i| (c[i + 1].0 - c[i].0).hypot(c[i + 1].1 - c[i].1))
            .sum();
        let n = (len / step).ceil() as usize;
        for k in 0..=n {
            let u = k as f32 / n as f32;
            let v = 1.0 - u;
            let b = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
            let px: f32 = (0..4).map(|i| b[i] * c[i].0).sum();
            let py: f32 = (0..4).map(|i| b[i] * c[i].1).sum();
            f.put(Quad::rect(x + px - d / 2.0, y + py - d / 2.0, d, d, color).radius(d / 2.0));
        }
    }
}

fn brushes(f: &mut Frame, body: Rect, s: &mut State) {
    let t = f.theme();
    let n = s.brushes.len();
    if s.panel.sizes.len() != n {
        s.panel.sizes = s.brushes.iter().map(|b| brush_size(b)).collect();
    }
    if s.brush < n {
        s.panel.sizes[s.brush] = s.brush_size;
    }
    // from the foot: the hint, then the three sliders
    let foot = footer(
        f,
        body,
        "Guides, symmetry and snapping are a right-click on the canvas away.",
    );
    let sh = slider_height(f);
    let (row_h, gap) = (48.0, 4.0);
    let list_h = n as f32 * (row_h + gap) - gap;
    // the sliders right under the list (the list scrolls when they'd meet the hint)
    let room = foot - GAP - 3.0 * sh - 2.0 * GAP - GAP - body.y;
    let list_h = list_h.min(room).max(0.0);
    let sliders_y = body.y + list_h + GAP;
    let at = |k: f32| Rect::new(body.x, sliders_y + k * (sh + GAP), body.w, sh);
    hit!(s, at(0.0), "size");
    let mut size = s.brush_size;
    let shown = format!("{} px", size.round());
    if slider(
        f,
        "panel/size",
        at(0.0),
        "Size",
        &shown,
        &mut size,
        1.0,
        60.0,
    ) {
        s.brush_size = size;
        if s.brush < n {
            s.panel.sizes[s.brush] = size;
        }
    }
    let mut op = s.panel.opacity;
    let shown = format!("{}%", op.round());
    slider(
        f,
        "panel/opacity",
        at(1.0),
        "Opacity",
        &shown,
        &mut op,
        0.0,
        100.0,
    );
    s.panel.opacity = op;
    let mut st = s.panel.steady;
    let shown = format!("{}%", st.round());
    slider(
        f,
        "panel/steady",
        at(2.0),
        "Steady lines",
        &shown,
        &mut st,
        0.0,
        100.0,
    );
    s.panel.steady = st;

    let area = Rect::new(body.x, body.y, body.w, list_h);
    list(
        f,
        "panel/brushes",
        area,
        |_, _| n as f32 * (row_h + gap) - gap,
        |f, r| {
            for i in 0..n {
                let br = Rect::new(r.x, r.y + i as f32 * (row_h + gap), r.w, row_h);
                hit!(s, br, "brush/{i}");
                let res = f.interact(id(&format!("panel/brush/{i}")), br);
                context_menu(f, s, BRUSH_MENU, br, i, res.context);
                if res.clicked && s.brush != i {
                    s.brush = i;
                    s.brush_size = s.panel.sizes[i];
                }
                let on = s.brush == i;
                let bg = if on {
                    PICKED
                } else if res.hovered {
                    mix(t.card, PICKED, 0.6)
                } else {
                    t.card
                };
                if bg != t.card {
                    f.put(Quad::rect(br.x, br.y, br.w, br.h, bg).radius(12.0));
                }
                let (w, op) = brush_look(&s.brushes[i]);
                brush_stroke(f, br.x + 12.0, br.y + 14.0, w, mix(bg, t.ink, op));
                let size = f.lay_out(
                    &format!("{} px", s.panel.sizes[i].round()),
                    &Style::body(12.0, MUTED),
                    None,
                );
                let name_x = br.x + 12.0 + 64.0 + 12.0;
                let l = f.lay_out(
                    &s.brushes[i],
                    &Style::body(13.0, t.ink).weight(600),
                    Some(br.right() - 12.0 - size.width - 12.0 - name_x),
                );
                f.put_text(&l, name_x, br.y + (row_h - l.height) / 2.0);
                f.put_text(
                    &size,
                    br.right() - 12.0 - size.width,
                    br.y + (row_h - size.height) / 2.0,
                );
            }
        },
    );
}

// ---- Layers ----

fn layers(f: &mut Frame, body: Rect, s: &mut State) {
    let t = f.theme();
    let n = s.layers.len();
    let mut y = body.y;
    y += heading(f, body.x, y, body.w, "Layers", &count(n, "layer")) + GAP;
    let b = Rect::new(body.x, body.bottom() - 40.0, body.w, 40.0);
    hit!(s, b, "new-layer");
    if dark_button(f, "panel/new-layer", b, "New layer") {
        s.actions.push(Action::NewLayer);
    }

    let area = Rect::new(body.x, y, body.w, (b.y - GAP - y).max(0.0));
    let (row_h, gap) = (48.0, 4.0);
    list(
        f,
        "panel/layers",
        area,
        |_, _| n as f32 * (row_h + gap) - gap,
        |f, r| {
            for i in 0..n {
                let lr = Rect::new(r.x, r.y + i as f32 * (row_h + gap), r.w, row_h);
                hit!(s, lr, "layer/{i}");
                let res = f.interact(id(&format!("panel/layer/{i}")), lr);
                if res.clicked {
                    s.active_layer = i;
                }
                let on = s.active_layer == i;
                if on {
                    f.put(Quad::rect(lr.x, lr.y, lr.w, lr.h, PICKED).radius(12.0));
                } else if res.hovered {
                    f.put(Quad::rect(lr.x, lr.y, lr.w, lr.h, alpha(PICKED, 0.6)).radius(12.0));
                }
                let layer = &s.layers[i];
                // its kind: C (a drawing per exposure) or S (static)
                let k = 26.0;
                let kr = Rect::new(lr.x + 12.0, lr.y + (row_h - k) / 2.0, k, k);
                let (kbg, kfg) = if on { (t.ink, t.card) } else { (t.line, t.ink) };
                f.put(Quad::rect(kr.x, kr.y, k, k, kbg).radius(8.0));
                let kl = f.lay_out(
                    &layer.kind.to_string(),
                    &Style::body(12.0, kfg).weight(700),
                    None,
                );
                f.put_text(
                    &kl,
                    kr.x + (k - kl.width) / 2.0,
                    kr.y + (k - kl.height) / 2.0,
                );
                let what = if layer.kind == 'S' {
                    "Static layer"
                } else {
                    "Cel layer"
                };
                let name = f.lay_out(&layer.name, &Style::body(13.0, t.ink).weight(600), None);
                let sub = f.lay_out(what, &Style::body(12.0, MUTED), None);
                let tx = kr.right() + 12.0;
                let ty = lr.y + (row_h - name.height - sub.height) / 2.0;
                f.put_text(&name, tx, ty);
                f.put_text(&sub, tx, ty + name.height);
                if layer.locked {
                    f.icon(LOCK, lr.right() - 12.0 - 16.0, lr.y + 16.0, 16.0, MUTED);
                }
                if !layer.visible {
                    let h = f.lay_out("Hidden", &Style::body(12.0, MUTED), None);
                    f.put_text(
                        &h,
                        lr.right() - 40.0 - h.width,
                        lr.y + (row_h - h.height) / 2.0,
                    );
                }
            }
        },
    );
}

// ---- Review notes ----

/// A note card's text for a card `w` wide: its author, its frame, the note.
fn note_text(f: &mut Frame, n: &Note, w: f32) -> (Laid, Laid, Laid) {
    let t = f.theme();
    let tw = w - 10.0 - 30.0 - 10.0 - 10.0;
    let who = f.lay_out(&n.author, &Style::body(12.0, t.ink).weight(700), None);
    let at = f.lay_out(
        &format!(" · frame {}", n.frame + 1),
        &Style::body(12.0, MUTED).weight(600),
        None,
    );
    let mut st = Style::body(13.0, t.ink);
    st.line_height = 1.4;
    let text = f.lay_out(&n.text, &st, Some(tw));
    (who, at, text)
}

fn note_height(who: &Laid, text: &Laid) -> f32 {
    (10.0 + who.height + 3.0 + text.height + 10.0).max(50.0)
}

/// The picked note: the one picked, else the first on the current frame.
fn picked_note(s: &State) -> Option<usize> {
    s.panel
        .note
        .filter(|&i| i < s.notes.len())
        .or_else(|| s.notes.iter().position(|n| n.frame == s.frame))
}

fn notes(f: &mut Frame, body: Rect, s: &mut State) {
    let t = f.theme();
    let mut y = body.y;
    let open = s.notes.iter().filter(|n| !n.resolved).count();
    let title = format!("Notes on {}", s.cut_name);
    y += heading(f, body.x, y, body.w, &title, &format!("{open} open")) + GAP;

    // the foot: a field and Post, under their label
    let row_h = t.control_height;
    let row_y = body.bottom() - row_h;
    let lab = f.lay_out(
        &format!("Add a note at frame {}", s.frame + 1),
        &Style::body(12.0, MUTED).weight(600),
        None,
    );
    let lab_y = row_y - 6.0 - lab.height;
    f.put_text(&lab, body.x, lab_y);
    let pw = f
        .lay_out("Post", &Style::body(13.0, t.on_dark).weight(600), None)
        .width
        + 24.0;
    let mut draft = std::mem::take(&mut s.panel.draft);
    f.row_in(Rect::new(body.x, row_y, body.w, row_h), 6.0, |f| {
        f.text_field(
            "panel/note-draft",
            &mut draft,
            "Type a note",
            body.w - pw - 6.0,
        );
    });
    let pr = Rect::new(body.right() - pw, row_y, pw, row_h);
    hit!(s, pr, "post");
    if dark_button(f, "panel/post", pr, "Post") && !draft.trim().is_empty() {
        s.notes.push(Note {
            author: "You".into(),
            text: draft.trim().to_string(),
            frame: s.frame,
            colour: t.coral,
            resolved: false,
        });
        s.panel.note = Some(s.notes.len() - 1);
        s.actions.push(Action::Other("Post note".into()));
        draft.clear();
    }
    s.panel.draft = draft;

    let area = Rect::new(body.x, y, body.w, (lab_y - GAP - y).max(0.0));
    let gap = 8.0;
    let picked = picked_note(s);
    let notes = s.notes.clone();
    list(
        f,
        "panel/notes",
        area,
        |f, w| {
            let hs: f32 = notes
                .iter()
                .map(|n| {
                    let (who, _, text) = note_text(f, n, w);
                    note_height(&who, &text)
                })
                .sum();
            hs + gap * notes.len().saturating_sub(1) as f32
        },
        |f, r| {
            let mut y = r.y;
            for (i, n) in notes.iter().enumerate() {
                let (who, at, text) = note_text(f, n, r.w);
                let h = note_height(&who, &text);
                let nr = Rect::new(r.x, y, r.w, h);
                y += h + gap;
                hit!(s, nr, "note/{i}");
                let res = f.interact(id(&format!("panel/note/{i}")), nr);
                context_menu(f, s, NOTE_MENU, nr, i, res.context);
                if res.clicked {
                    s.panel.note = Some(i);
                    s.actions.push(Action::GoToFrame(n.frame));
                }
                let on = picked == Some(i) || res.clicked;
                let bg = if on {
                    NOTE_PICKED
                } else if res.hovered {
                    mix(PICKED, t.line, 0.4)
                } else {
                    PICKED
                };
                f.put(Quad::rect(nr.x, nr.y, nr.w, nr.h, bg).radius(14.0));
                // the author's colour, with the note's number
                let d = 30.0;
                f.put(Quad::rect(nr.x + 10.0, nr.y + 10.0, d, d, n.colour).radius(d / 2.0));
                let num = f.lay_out(
                    &(i + 1).to_string(),
                    &Style::body(12.0, hex(0xFFFFFF)).weight(700),
                    None,
                );
                f.put_text(
                    &num,
                    nr.x + 10.0 + (d - num.width) / 2.0,
                    nr.y + 10.0 + (d - num.height) / 2.0,
                );
                let tx = nr.x + 10.0 + d + 10.0;
                f.put_text(&who, tx, nr.y + 10.0);
                f.put_text(&at, tx + who.width, nr.y + 10.0);
                f.put_text(&text, tx, nr.y + 10.0 + who.height + 3.0);
            }
        },
    );
}

// ---- right-click menus ----

/// A menu's heading and items (text, hint; "-" for a separator), for item `i` of the list
/// the menu `name` is for.
fn menu_items(s: &State, name: &str, i: usize) -> Option<(String, Vec<(String, String)>)> {
    let it = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(t, h)| (t.to_string(), h.to_string()))
            .collect()
    };
    match name {
        BRUSH_MENU => Some((
            s.brushes.get(i)?.clone(),
            it(&[
                ("Edit brush…", ""),
                ("Duplicate", ""),
                ("Rename…", ""),
                ("-", ""),
                ("Add to favourites", "☆"),
                ("Export brush…", ""),
                ("-", ""),
                ("Delete", ""),
            ]),
        )),
        COLOUR_MENU => {
            let (name, c) = s.palette.get(i)?;
            Some((
                format!("{name} · {}", hex_name(*c)),
                it(&[
                    ("Edit colour…", ""),
                    ("Rename…", ""),
                    ("Swap it everywhere…", ""),
                    ("-", ""),
                    ("Select every area with it", ""),
                    ("Lock this colour", ""),
                    ("-", ""),
                    ("Copy hex code", ""),
                    ("Delete", ""),
                ]),
            ))
        }
        NOTE_MENU => {
            let n = s.notes.get(i)?;
            let mut items = it(&[("Reply", ""), ("Resolve", "✓")]);
            items.push((format!("Jump to frame {}", n.frame + 1), String::new()));
            items.extend(it(&[("-", ""), ("Copy link", ""), ("Delete", "")]));
            Some((format!("Note {} · {}", i + 1, n.author), items))
        }
        _ => None,
    }
}

/// A right-click on `r` (item `i`) opens the menu `name` at the pointer, moved to stay in
/// the window (the panel is at its right edge).
fn context_menu(f: &mut Frame, s: &mut State, name: &str, r: Rect, i: usize, context: bool) {
    f.context_area(name, r);
    if !context {
        return;
    }
    s.panel.menu_for = i;
    let Some((head, items)) = menu_items(s, name, i) else {
        return;
    };
    // the menu's size, measured as the toolkit's menu measures it
    let t = f.theme();
    let mut w = f
        .lay_out(
            &head.to_uppercase(),
            &Style::body(11.0, t.on_dark_muted).weight(600),
            None,
        )
        .width;
    let mut h = 12.0 + 26.0;
    for (text, hint) in &items {
        if text == "-" {
            h += 9.0;
            continue;
        }
        h += 32.0;
        let mut iw = f
            .lay_out(text, &Style::body(13.0, t.on_dark).weight(500), None)
            .width;
        if !hint.is_empty() {
            iw += f
                .lay_out(hint, &Style::body(12.0, t.on_dark_muted), None)
                .width
                + 24.0;
        }
        w = w.max(iw);
    }
    let w = w.max(160.0) + 20.0 + 12.0;
    let p = f.pointer();
    let sc = s.panel.screen;
    let x = p.x.min(sc.right() - 8.0 - w).max(sc.x);
    let y = p.y.min(sc.bottom() - 8.0 - h).max(sc.y);
    f.open(name, x, y);
    #[cfg(test)]
    {
        s.panel.menu_at = Rect::new(x, y, w, h);
    }
}

fn menus(f: &mut Frame, s: &mut State) {
    let i = s.panel.menu_for;
    for name in [BRUSH_MENU, COLOUR_MENU, NOTE_MENU] {
        if !f.is_open(name) {
            continue;
        }
        let Some((head, items)) = menu_items(s, name, i) else {
            f.close();
            continue;
        };
        let Some(item) = f.menu(name, |m| {
            m.head(&head);
            for (text, hint) in &items {
                if text == "-" {
                    m.separator();
                } else if text.starts_with("Delete") {
                    m.danger(text, hint);
                } else {
                    m.item(text, hint);
                }
            }
        }) else {
            continue;
        };
        if name == NOTE_MENU && item.starts_with("Jump to frame") {
            s.panel.note = Some(i);
            s.actions.push(Action::GoToFrame(s.notes[i].frame));
        } else {
            let what = match name {
                BRUSH_MENU => s.brushes[i].clone(),
                COLOUR_MENU => s.palette[i].0.clone(),
                _ => format!("note {}", i + 1),
            };
            s.actions.push(Action::Other(format!("{item} ({what})")));
        }
    }
}

// ---- colour maths ----

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Straight RGBA → hue (degrees), saturation, value.
pub fn rgb_to_hsv(c: [f32; 4]) -> [f32; 3] {
    let [r, g, b, _] = c;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max <= 0.0 { 0.0 } else { d / max };
    [h, s, max]
}

/// Hue (degrees), saturation, value → straight RGBA, opaque.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 4] {
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m, 1.0]
}

/// "#RRGGBB".
pub fn hex_name(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::layout;
    use pencilpilot_gpu::{DmabufImage, Error, Gpu, Renderer2d};
    use pencilpilot_ui::ui::Ui;

    const W: f32 = 1440.0;
    const H: f32 = 900.0;

    /// The panel alone in a 1440 × 900 window, driven with simulated input.
    struct Harness {
        r: Renderer2d,
        ui: Ui,
        s: State,
        gpu: Gpu,
    }

    fn harness(w: Workspace) -> Option<Harness> {
        let gpu = match Gpu::new() {
            Ok(g) => g,
            Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => {
                eprintln!("skipped: {e}");
                return None;
            }
            Err(e) => panic!("{e}"),
        };
        let r = Renderer2d::new(&gpu).unwrap();
        let mut ui = Ui::new(1.0);
        ui.text.wait_for_fonts();
        let mut s = State::demo();
        s.workspace = w;
        let mut h = Harness { r, ui, s, gpu };
        h.pass();
        Some(h)
    }

    impl Harness {
        /// A pass; whether it drew anything new.
        fn pass(&mut self) -> bool {
            let panel = layout::regions(W, H, self.s.workspace).panel;
            let s = &mut self.s;
            self.ui
                .pass(&mut self.r, W, H, |f| build(f, panel, s))
                .is_some()
        }
        /// With `PP_SAVE` set (a folder), the window as it is now, as `<name>.png`.
        fn save(&mut self, name: &str) {
            let Ok(dir) = std::env::var("PP_SAVE") else {
                return;
            };
            let frame = DmabufImage::new(&self.gpu, W as u32, H as u32, &[]).unwrap();
            self.r
                .draw(&self.gpu, &frame, self.ui.theme.paper, self.ui.shown())
                .unwrap();
            let px = self.gpu.read_back(&frame).unwrap();
            let rgba: Vec<u8> = px.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
            let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
        fn at(&self, name: &str) -> Rect {
            self.s
                .panel
                .hits
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("{name} isn't drawn"))
                .1
        }
        fn shown(&self, name: &str) -> bool {
            self.s.panel.hits.iter().any(|(n, _)| n == name)
        }
        fn click_at(&mut self, x: f32, y: f32) {
            self.ui.pointer_moved(x, y);
            self.pass();
            self.ui.pointer_button(true);
            self.pass();
            self.ui.pointer_button(false);
            self.pass();
        }
        fn click(&mut self, name: &str) {
            let r = self.at(name);
            self.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
        }
        /// Right-click `name`; returns where its menu is (the pointer, kept in the window).
        fn right_click(&mut self, name: &str) {
            let r = self.at(name);
            self.ui.pointer_moved(r.x + r.w / 2.0, r.y + r.h / 2.0);
            self.pass();
            self.ui.pointer_context();
            self.pass();
            // the menu's first pass marks its area
            self.pass();
        }
    }

    /// Where a menu's entries are: below its padding and heading, separators 9 px.
    fn entry_y(menu_y: f32, items: &[&str], entry: &str) -> f32 {
        let mut y = menu_y + 6.0 + 26.0;
        for it in items {
            if *it == entry {
                return y + 16.0;
            }
            y += if *it == "-" { 9.0 } else { 32.0 };
        }
        panic!("no {entry}");
    }

    #[test]
    fn tabs_follow_the_workspace() {
        let names = |w| tabs_for(w).iter().map(|t| t.0).collect::<Vec<_>>();
        assert_eq!(names(Workspace::Draw), ["Brushes", "Colour", "Layer"]);
        assert_eq!(names(Workspace::Animate), ["Colour", "Brushes", "Layer"]);
        assert_eq!(names(Workspace::Colour), ["Colour", "Brushes", "Layer"]);
        assert!(names(Workspace::Review).is_empty());
        let mut s = State::demo();
        s.workspace = Workspace::Review;
        assert_eq!(content(&s), Content::Notes);
        s.workspace = Workspace::Draw;
        s.panel_tab[1] = 9; // out of range: the last tab
        assert_eq!(content(&s), Content::Layers);
    }

    #[test]
    fn colours_convert_both_ways() {
        for c in [hex(0xE8492F), hex(0x5FD08A), hex(0x3D6BFF), hex(0x16161A)] {
            let [h, s, v] = rgb_to_hsv(c);
            let back = hsv_to_rgb(h, s, v);
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 1e-4, "{c:?} → {back:?}");
            }
        }
        assert_eq!(hex_name(hex(0xF2C3A0)), "#F2C3A0");
    }

    #[test]
    fn the_tabs_switch_and_idle_draws_nothing() {
        let Some(mut h) = harness(Workspace::Animate) else {
            return;
        };
        assert!(!h.pass(), "nothing changed: nothing drawn");
        // the pointer over the card where nothing reacts
        let card = layout::regions(W, H, Workspace::Animate).panel;
        h.ui.pointer_moved(card.x + 8.0, card.bottom() - 8.0);
        assert!(
            !h.pass(),
            "the pointer over nothing that reacts: nothing drawn"
        );
        assert!(h.shown("field") && !h.shown("brush/0"));
        h.click("tab/1");
        assert_eq!(h.s.panel_tab[Workspace::Animate.index()], 1);
        assert!(h.shown("brush/0"), "Brushes shows its brushes");
        assert!(!h.shown("field"));
        h.click("tab/2");
        assert!(h.shown("layer/0"), "Layer shows the layers");
        h.click("layer/2");
        assert_eq!(h.s.active_layer, 2);
        h.click("new-layer");
        assert_eq!(h.s.actions, [Action::NewLayer]);
        // the other workspaces keep their own tab
        assert_eq!(h.s.panel_tab[Workspace::Draw.index()], 0);
        h.ui.pointer_moved(card.x + 8.0, card.bottom() - 8.0);
        h.pass();
        assert!(!h.pass(), "idle again: nothing drawn");
    }

    #[test]
    fn a_brush_picks_and_the_size_follows() {
        let Some(mut h) = harness(Workspace::Draw) else {
            return;
        };
        assert_eq!(h.s.brush, 0);
        h.click("brush/3");
        assert_eq!(h.s.brush, 3, "Marker picked");
        assert_eq!(h.s.brush_size, 18.0, "its size");
        // drag the size slider to its end
        let r = h.at("size");
        let y = r.bottom() - 10.0;
        h.ui.pointer_moved(r.x + 20.0, y);
        h.pass();
        h.ui.pointer_button(true);
        h.pass();
        h.ui.pointer_moved(r.right() + 40.0, y);
        h.pass();
        h.ui.pointer_button(false);
        h.pass();
        assert_eq!(h.s.brush_size, 60.0);
        // and the brush keeps it
        h.click("brush/0");
        assert_eq!(h.s.brush_size, 6.0, "the pencil's own size");
        h.click("brush/3");
        assert_eq!(h.s.brush_size, 60.0);
    }

    #[test]
    fn a_swatch_sets_the_colour() {
        let Some(mut h) = harness(Workspace::Animate) else {
            return;
        };
        h.click("swatch/4");
        assert_eq!(h.s.colour, hex(0x3D6BFF));
        assert_eq!(h.s.colour_name, "Sky");
        // the field: its top-right is the hue at full saturation and value
        let r = h.at("field");
        h.click_at(r.right() - 1.0, r.y + 1.0);
        let [hue, sat, val] = rgb_to_hsv(h.s.colour);
        let sky = rgb_to_hsv(hex(0x3D6BFF))[0];
        assert!((hue - sky).abs() < 1.0 && sat > 0.98 && val > 0.98);
        assert!(h.s.colour_name.starts_with('#'));

        // the Colour workspace's named swatches
        h.s.workspace = Workspace::Colour;
        h.pass();
        h.click("named/6");
        assert_eq!(h.s.colour_name, "Skin");
        assert_eq!(h.s.colour, hex(0xF2C3A0));
        assert!(h.s.panel.close_gaps);
        h.click("close-gaps");
        assert!(!h.s.panel.close_gaps, "the option flipped");
    }

    #[test]
    fn a_note_goes_to_its_frame_and_a_new_one_posts() {
        let Some(mut h) = harness(Workspace::Review) else {
            return;
        };
        assert!(!h.shown("tab/0"), "Review has no tabs");
        assert_eq!(picked_note(&h.s), Some(0), "the note on this frame");
        h.click("note/1");
        assert_eq!(h.s.actions, [Action::GoToFrame(22)]);
        assert_eq!(picked_note(&h.s), Some(1));
        h.s.actions.clear();
        // type a note and post it
        let post = h.at("post");
        h.click_at(post.x - 60.0, post.y + post.h / 2.0);
        for c in ["O", "k"] {
            h.ui.key(pencilpilot_ui::ui::Key {
                name: c.into(),
                text: Some(c.into()),
                ctrl: false,
                shift: false,
            });
        }
        h.pass();
        h.click("post");
        assert_eq!(h.s.notes.len(), 3);
        assert_eq!(h.s.notes[2].text, "Ok");
        assert_eq!(h.s.notes[2].frame, h.s.frame);
        assert!(h.s.panel.draft.is_empty());
    }

    #[test]
    fn right_click_menus_open_and_pick() {
        let Some(mut h) = harness(Workspace::Draw) else {
            return;
        };
        // a brush: Duplicate
        h.right_click("brush/1");
        assert!(h.ui.overlay_open(), "the brush's menu opened");
        h.save("panel-brush-menu");
        let m = h.s.panel.menu_at;
        assert!(m.right() <= W, "the menu stays in the window: {m:?}");
        let items = ["Edit brush…", "Duplicate"];
        h.click_at(m.x + 30.0, entry_y(m.y, &items, "Duplicate"));
        assert!(!h.ui.overlay_open(), "picking closed it");
        assert_eq!(
            h.s.actions,
            [Action::Other("Duplicate (Rough pencil)".into())]
        );
        assert_eq!(h.s.brush, 0, "nothing under the menu was picked");
        h.s.actions.clear();

        // a palette colour: Lock this colour
        h.click("tab/1");
        h.right_click("swatch/6");
        assert!(h.ui.overlay_open(), "the colour's menu opened");
        h.save("panel-colour-menu");
        let m = h.s.panel.menu_at;
        assert!(m.right() <= W, "the menu stays in the window: {m:?}");
        let items = [
            "Edit colour…",
            "Rename…",
            "Swap it everywhere…",
            "-",
            "Select every area with it",
            "Lock this colour",
        ];
        h.click_at(m.x + 30.0, entry_y(m.y, &items, "Lock this colour"));
        assert_eq!(
            h.s.actions,
            [Action::Other("Lock this colour (Skin)".into())]
        );
        h.s.actions.clear();

        // a note: Jump to frame 23
        h.s.workspace = Workspace::Review;
        h.pass();
        h.right_click("note/1");
        assert!(h.ui.overlay_open(), "the note's menu opened");
        h.save("panel-note-menu");
        let m = h.s.panel.menu_at;
        let items = ["Reply", "Resolve", "Jump to frame 23"];
        h.click_at(m.x + 30.0, entry_y(m.y, &items, "Jump to frame 23"));
        assert_eq!(h.s.actions, [Action::GoToFrame(22)]);
        assert_eq!(picked_note(&h.s), Some(1));
    }

    #[test]
    fn a_long_list_scrolls() {
        let Some(mut h) = harness(Workspace::Review) else {
            return;
        };
        h.s.notes = (0..20)
            .map(|i| Note {
                author: "Sam".into(),
                text: format!("Note number {i}"),
                frame: i,
                colour: hex(0xE8492F),
                resolved: false,
            })
            .collect();
        h.pass();
        let first = h.at("note/0");
        let fifth = h.at("note/5");
        h.ui.pointer_moved(first.x + 40.0, first.y + 10.0);
        h.pass();
        h.ui.scrolled(fifth.y - first.y);
        h.pass();
        let moved = h.at("note/0");
        assert!(
            (moved.y - (first.y - (fifth.y - first.y))).abs() < 0.5,
            "the list moved up by the scroll"
        );
        h.save("panel-notes-scrolled");
        // what's under the pointer now is the sixth note
        h.click_at(first.x + 40.0, first.y + 10.0);
        assert_eq!(h.s.actions, [Action::GoToFrame(5)]);
        // and it can't scroll past its end
        h.ui.scrolled(1e5);
        h.pass();
        let last = h.at("note/19");
        let area_bottom = h.at("post").y;
        assert!(
            last.bottom() < area_bottom,
            "the last note shows at the end"
        );
    }
}
