//! The bottom strip, one per workspace, all dark with 22 px corners:
//! - Animate: the timeline (the design's Option 2): the "where you are and drawing" row of
//!   icon trays, the grid (layers, cels sized by their hold, the sound row, the playhead)
//!   and the "playback and view" bar.
//! - Draw: the frame strip (layer, onion skin, the drawings in this cut, New).
//! - Colour: the drawings to colour, with their progress.
//! - Review: the review player (the scrub track with the notes, previous note, Play, next).
//!
//! The grid is read-only here; editing it is the timeline's next step. Nothing animates:
//! trays open and fold back at once, so an idle strip draws nothing.

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::{Laid, Style};
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Rect, Response, id};

use crate::state::{Action, ColourStatus, State, Workspace};

// ---- the design's colours ----

const ON_DARK: [f32; 4] = hex(0xF6F4EF);
const MUTED: [f32; 4] = hex(0xA9A8AE);
const INK: [f32; 4] = hex(0x16161A);
const RAISED: [f32; 4] = hex(0x2A2A30);
const RAISED_OPEN: [f32; 4] = hex(0x3A3A42);
const TRAY_OPEN: [f32; 4] = hex(0x24242A);
const GRID: [f32; 4] = hex(0x1E1E24);
const GRID_LINE: [f32; 4] = hex(0x2B2B32);
const CEL: [f32; 4] = hex(0x34343C);
const CEL_PICKED: [f32; 4] = hex(0x4A4A55);
const CEL_LINE: [f32; 4] = hex(0x45454E);
const TRACK: [f32; 4] = hex(0x34343C);
const RULER: [f32; 4] = hex(0x8C8B92);
const CORAL: [f32; 4] = hex(0xE8492F);
const SUN: [f32; 4] = hex(0xFFD84D);
const LEAF: [f32; 4] = hex(0x5FD08A);
const SOUND: [f32; 4] = hex(0x4A3F7A);
const ACTIVE_ROW: [f32; 4] = hex(0x2E2A26);
const SKIN: [f32; 4] = hex(0xF2C9A8);
const SKIN_SHADE: [f32; 4] = hex(0xD99C7A);
const JACKET_SHADE: [f32; 4] = hex(0xA8321F);
const WHITE: [f32; 4] = hex(0xFFFFFF);

// ---- icons (24 × 24 paths, drawn as 2 px lines) ----

const CLAP: &str = "M4 10h16v10H4Z M4 10 5.5 5l14-1 0.5 4 M9 4.6 8 9.5 M14 4.2 13 9.5";
const FILM: &str = "M4 5h16v14H4Z M8 5v14 M16 5v14 M4 9h4 M4 15h4 M16 9h4 M16 15h4";
const PENCIL_PLUS: &str = "M4 20l1-4L15 6l3 3L8 19Z M13 8l3 3 M19 14v6 M16 17h6";
const DUP: &str = "M9 9h11v11H9Z M5 15V4h11";
const BLANK: &str = "M6 4h12v16H6Z";
const CLOCK: &str = "M12 3a9 9 0 1 0 0.01 0Z M12 7v5l3 2";
const KEY: &str = "M14.5 4a5 5 0 1 1-4.2 7.7L4 18v2h3v-2h2v-2h2l1.2-1.2 M16 8h0.01";
const DIAMOND: &str = "M12 4l8 8-8 8-8-8Z";
const DOT: &str = "M12 8a4 4 0 1 0 0.01 0Z";
const CLEAR: &str = "M6 6l12 12 M18 6 6 18";
const FRAMES: &str = "M3 6h5v12H3Z M9.5 6h5v12h-5Z M16 6h5v12h-5Z";
const EXTEND: &str = "M4 12h16 M16 8l4 4-4 4 M4 7v10";
const REVERSE: &str = "M4 9h13l-3-3 M20 15H7l3 3";
const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
const CHEVRON_LEFT: &str = "M15 6l-6 6 6 6";
const CHEVRON_DOWN: &str = "M6 9l6 6 6-6";
const PREVIOUS: &str = "M15 5l-7 7 7 7";
const NEXT: &str = "M9 5l7 7-7 7";
const PLUS: &str = "M12 5v14M5 12h14";
const LAYERS: &str = "M12 3 3 8l9 5 9-5Z M3 13l9 5 9-5";
/// the onion skin's two circles (the second drawn fainter)
const ONION_A: &str = "M2.6 12a6 6 0 1 0 12 0a6 6 0 1 0-12 0Z";
const ONION_B: &str = "M9.4 12a6 6 0 1 0 12 0a6 6 0 1 0-12 0Z";
const CAMERA: &str =
    "M5 6h9a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z M16 10l5-3v10l-5-3Z";
const PIN: &str = "M12 17v5M8 3h8l-1 6 3 3H6l3-3Z";
const FLIP: &str = "M5 4h10l4 4v12H5Z M9 4v6l2-1.5L13 10V4";
const LOOP: &str = "M17 2l4 4-4 4 M3 12V9a3 3 0 0 1 3-3h15 M7 22l-4-4 4-4 M21 12v3a3 3 0 0 1-3 3H3";
const WAVE: &str = "M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2";
/// filled shapes, as lines close enough together to fill them
const PLAY: &str = "M7 4v16l13-8Z M8.5 7v10l8.5-5Z M10 10v4l3.5-2Z";
const PAUSE: &str = "M7 5v14M9 5v14 M15 5v14M17 5v14";
const MOUNTAIN: &str = "M1.7 18.9 9.4 8.6l4.3 5.1 2.6-2.6 6 7.7Z M2.7 17.5H21.2 M3.9 16H20.1 \
     M5 14.5H18.9 M6.1 13H13.1 M14.4 13H17.7 M7.2 11.5H11.9 M15.9 11.5H16.6 M8.4 10H10.6";

// ---- the trays ----

struct TrayOption {
    label: &'static str,
    icon: &'static str,
    tag: &'static str,
}

struct Tray {
    name: &'static str,
    right: bool,
    /// coral (the New tray)
    accent: bool,
    options: &'static [TrayOption],
}

const fn opt(label: &'static str, icon: &'static str, tag: &'static str) -> TrayOption {
    TrayOption { label, icon, tag }
}

/// The timeline's icon trays, as the design has them: two on the left (where you are),
/// four on the right (drawing).
const TRAYS: [Tray; 6] = [
    Tray {
        name: "Scene",
        right: false,
        accent: false,
        options: &[
            opt("Scene 1", CLAP, "1"),
            opt("Scene 2", CLAP, "2"),
            opt("Scene 3", CLAP, "3"),
            opt("New scene", CLAP, "+"),
        ],
    },
    Tray {
        name: "Cut",
        right: false,
        accent: false,
        options: &[
            opt("Cut 1 · 1.5 s", FILM, "1"),
            opt("Cut 2 · 2.0 s", FILM, "2"),
            opt("Cut 3 · 3.0 s", FILM, "3"),
            opt("Cut 4 · 1.0 s", FILM, "4"),
            opt("New cut", FILM, "+"),
        ],
    },
    Tray {
        name: "New",
        right: true,
        accent: true,
        options: &[
            opt("New drawing", PENCIL_PLUS, ""),
            opt("Duplicate drawing", DUP, ""),
            opt("Blank frame", BLANK, ""),
            opt("New cut", FILM, "+"),
            opt("New scene", CLAP, "+"),
        ],
    },
    Tray {
        name: "Hold",
        right: true,
        accent: false,
        options: &[
            opt("Hold on 1s", CLOCK, "1s"),
            opt("Hold on 2s", CLOCK, "2s"),
            opt("Hold on 3s", CLOCK, "3s"),
        ],
    },
    Tray {
        name: "Mark frame",
        right: true,
        accent: false,
        options: &[
            opt("Key", KEY, "K"),
            opt("Breakdown", DIAMOND, "B"),
            opt("In-between", DOT, ""),
            opt("Clear mark", CLEAR, ""),
        ],
    },
    Tray {
        name: "Frames",
        right: true,
        accent: false,
        options: &[
            opt("Insert frame", FRAMES, "+"),
            opt("Remove frame", FRAMES, "−"),
            opt("Extend hold", EXTEND, ""),
            opt("Reverse frames", REVERSE, ""),
        ],
    },
];

/// How a drawing is marked in the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    None,
    Key,
    Breakdown,
}

/// A drawing held for some frames on a layer (numbered by its place in the row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exposure {
    pub hold: u32,
    pub mark: Mark,
}

const fn ex(hold: u32, mark: Mark) -> Exposure {
    Exposure { hold, mark }
}

/// What the grid shows ("Show ▾").
pub const SHOW: [&str; 3] = ["All", "Keys and breakdowns", "Keys only"];
/// Onion skin's range ("onion ▾").
pub const ONION_RANGES: [&str; 3] = [
    "1 before · 1 after",
    "2 before · 1 after",
    "2 before · 2 after",
];
pub const PLAY_RANGES: [&str; 2] = ["This cut", "The whole scene"];
pub const PLAY_MODES: [&str; 3] = ["Once", "Loop", "Ping-pong"];
pub const FRAME_RATES: [u32; 5] = [12, 24, 25, 30, 60];
/// The timeline's zoom: a frame's width, as the design's slider.
pub const ZOOM_MIN: f32 = 8.0;
pub const ZOOM_MAX: f32 = 44.0;
pub const ZOOM_STEP: f32 = 6.0;

/// This piece's own state (anything it needs that the rest of the app doesn't).
#[derive(Debug, Clone)]
pub struct StripsState {
    /// the open tray (an index into the trays), if any: one at a time
    pub open_tray: Option<usize>,
    /// each tray's current option
    pub picked: [usize; 6],
    /// each layer's drawings and their holds, by layer index (until the engine's exposure
    /// sheet comes in; a static layer is one drawing all cut long)
    pub exposures: Vec<Vec<Exposure>>,
    /// the cut's sound: first frame and length
    pub sound: (u32, u32),
    /// the grid's horizontal scroll, logical pixels
    pub scroll_x: f32,
    pub show: usize,
    pub onion_range: usize,
    pub play_range: usize,
    pub play_mode: usize,
    pub camera: bool,
    /// the last frame the review player's scrub asked for (so a drag asks once per frame)
    scrub_last: Option<u32>,
    /// whether a dropdown's menu was open when its button was pressed (that press closes it)
    pressed_open: bool,
    /// the layer a right-click menu is about
    menu_layer: usize,
    /// the drawing a right-click menu is about
    menu_drawing: usize,
    /// where the widgets went in the last pass (for the tests)
    #[cfg(test)]
    hits: std::collections::HashMap<String, Rect>,
}

impl Default for StripsState {
    fn default() -> Self {
        use Mark::{Breakdown as B, Key as K, None as N};
        StripsState {
            open_tray: None,
            // Scene 1, Cut 2, New drawing, Hold on 2s, Key, Insert frame
            picked: [0, 1, 0, 1, 0, 0],
            // the design's example cut
            exposures: vec![
                vec![ex(6, K), ex(6, N), ex(6, B), ex(6, K), ex(8, N), ex(16, K)],
                vec![ex(12, K), ex(12, N), ex(24, K)],
                vec![ex(12, N), ex(12, N), ex(12, N), ex(12, N)],
                vec![],
            ],
            sound: (6, 22),
            scroll_x: 0.0,
            show: 0,
            onion_range: 1,
            play_range: 0,
            play_mode: 1,
            camera: false,
            scrub_last: None,
            pressed_open: false,
            menu_layer: 0,
            menu_drawing: 0,
            #[cfg(test)]
            hits: Default::default(),
        }
    }
}

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    // twice: first for the input (what's clicked changes the state, menus open and pick),
    // then to draw the state as it is now, so a click shows in the pass it happens in
    DRAW.set(false);
    strip(f, r, s);
    #[cfg(test)]
    s.strips.hits.clear();
    DRAW.set(true);
    put(f, Quad::rect(r.x, r.y, r.w, r.h, INK).radius(22.0));
    strip(f, r, s);
}

fn strip(f: &mut Frame, r: Rect, s: &mut State) {
    match s.workspace {
        Workspace::Animate => timeline(f, r, s),
        Workspace::Draw => frame_strip(f, r, s),
        Workspace::Colour => to_colour(f, r, s),
        Workspace::Review => review_player(f, r, s),
    }
}

// ---- small helpers ----

thread_local! {
    /// Whether this run of the strip draws (the second) or only takes the input (the first).
    static DRAW: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn drawing() -> bool {
    DRAW.get()
}

fn put(f: &mut Frame, q: Quad) {
    if drawing() {
        f.put(q);
    }
}

fn icon(f: &mut Frame, path: &str, x: f32, y: f32, size: f32, color: [f32; 4]) {
    if drawing() {
        f.icon(path, x, y, size, color);
    }
}

fn put_text(f: &mut Frame, laid: &Laid, x: f32, y: f32) {
    if drawing() {
        f.put_text(laid, x, y);
    }
}

fn tooltip(f: &mut Frame, owner: u64, label: &str, r: Rect) {
    if drawing() {
        f.tooltip(owner, label, r);
    }
}

/// A widget's response: clicks count in the input run only (the drawing run sees the same
/// pointer, and mustn't act twice).
fn act(f: &mut Frame, owner: u64, r: Rect) -> Response {
    let mut res = f.interact(owner, r);
    if drawing() {
        res.clicked = false;
        res.context = false;
    }
    res
}

#[cfg(test)]
fn hit(s: &mut State, name: &str, r: Rect) {
    s.strips.hits.insert(name.to_string(), r);
}
#[cfg(not(test))]
fn hit(_: &mut State, _: &str, _: Rect) {}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn text(f: &mut Frame, s: &str, size: f32, weight: u16, color: [f32; 4]) -> Laid {
    f.lay_out(s, &Style::body(size, color).weight(weight), None)
}

/// Text with its left edge at `x`, centred on `cy`.
fn text_at(f: &mut Frame, laid: &Laid, x: f32, cy: f32) {
    put_text(f, laid, x, cy - laid.height / 2.0);
}

/// An icon `size` square, centred in `r`.
fn icon_in(f: &mut Frame, path: &str, r: Rect, size: f32, color: [f32; 4]) {
    icon(
        f,
        path,
        r.x + (r.w - size) / 2.0,
        r.y + (r.h - size) / 2.0,
        size,
        color,
    );
}

/// A flat dark button: `bg`, lighter when hovered, darker while held.
fn pad(f: &mut Frame, name: &str, r: Rect, bg: [f32; 4], radius: f32) -> Response {
    let res = act(f, id(name), r);
    let fill = if res.held {
        mix(bg, [0.0, 0.0, 0.0, bg[3].max(0.5)], 0.2)
    } else if res.hovered {
        if bg[3] == 0.0 {
            alpha(ON_DARK, 0.08)
        } else {
            mix(bg, ON_DARK, 0.12)
        }
    } else {
        bg
    };
    put(f, Quad::rect(r.x, r.y, r.w, r.h, fill).radius(radius));
    res
}

/// A dark icon button.
fn icon_pad(
    f: &mut Frame,
    name: &str,
    r: Rect,
    bg: [f32; 4],
    path: &str,
    size: f32,
    fg: [f32; 4],
) -> Response {
    let res = pad(f, name, r, bg, 9.0);
    icon_in(f, path, r, size, fg);
    res
}

/// The onion skin's two circles in `r`.
fn onion_icon(f: &mut Frame, x: f32, cy: f32, size: f32, fg: [f32; 4]) {
    let y = cy - size / 2.0;
    icon(f, ONION_A, x, y, size, fg);
    icon(f, ONION_B, x, y, size, alpha(fg, fg[3] * 0.45));
}

/// Where three groups go in a row from `x0` to `x1`: the first at the start, the last at the
/// end, the middle one centred in the space between (as the design's flexible spacers).
fn three(x0: f32, x1: f32, gap: f32, a: f32, b: f32, c: f32) -> (f32, f32, f32) {
    let free = (x1 - x0 - a - b - c).max(2.0 * gap);
    (x0, x0 + a + free / 2.0, x1 - c)
}

// ---- menus (dark, opened above what they're for: the strip is at the window's foot) ----

enum M {
    Head(String),
    Item(String, String),
    Sep,
}

fn item(t: &str, hint: &str) -> M {
    M::Item(t.into(), hint.into())
}

fn check(on: bool) -> &'static str {
    if on { "✓" } else { "" }
}

fn menu_height(items: &[M]) -> f32 {
    12.0 + items
        .iter()
        .map(|m| match m {
            M::Head(_) => 26.0,
            M::Item(..) => 32.0,
            M::Sep => 9.0,
        })
        .sum::<f32>()
}

/// Open the menu `name` with its foot just above `y`, its left edge at `x` (kept on the strip).
fn open_above(f: &mut Frame, name: &str, items: &[M], x: f32, y: f32, strip: Rect) {
    let x = x.min(strip.right() - 280.0).max(strip.x);
    f.open(name, x, y - 6.0 - menu_height(items));
}

fn show_menu(f: &mut Frame, name: &str, items: &[M]) -> Option<String> {
    // in the input run: the menu draws itself above everything, and a pick changes the state
    // before the strip is drawn
    if drawing() {
        return None;
    }
    f.menu(name, |m| {
        for it in items {
            match it {
                M::Head(t) => m.head(t),
                M::Item(t, h) => {
                    m.item(t, h);
                }
                M::Sep => m.separator(),
            }
        }
    })
}

/// A button that opens a menu above it (a second press while it's open closes it).
fn menu_button(
    f: &mut Frame,
    s: &mut State,
    menu: &str,
    res: Response,
    items: &[M],
    r: Rect,
    strip: Rect,
) {
    if res.held && f.pointer().pressed {
        s.strips.pressed_open = f.is_open(menu);
    }
    if res.clicked && !s.strips.pressed_open {
        open_above(f, menu, items, r.x, r.y, strip);
    }
}

fn layer_options(s: &State, layer: usize) -> Vec<M> {
    let Some(l) = s.layers.get(layer) else {
        return vec![];
    };
    vec![
        M::Head(l.name.clone()),
        item(
            if l.locked {
                "Unlock layer"
            } else {
                "Lock layer"
            },
            "",
        ),
        item("Show as outlines", ""),
        M::Sep,
        item("Cel layer: a drawing per exposure", check(l.kind == 'C')),
        item("Static: one drawing all cut long", check(l.kind == 'S')),
        M::Sep,
        item("All on ones", ""),
        item("All on twos", ""),
        item("All on threes", ""),
        M::Sep,
        item("Lip sync from a sound…", ""),
    ]
}

fn layer_option_picked(s: &mut State, layer: usize, picked: &str) {
    let Some(l) = s.layers.get_mut(layer) else {
        return;
    };
    match picked {
        "Lock layer" => l.locked = true,
        "Unlock layer" => l.locked = false,
        "Cel layer: a drawing per exposure" => l.kind = 'C',
        "Static: one drawing all cut long" => l.kind = 'S',
        other => s.actions.push(Action::Other(other.to_string())),
    }
}

fn onion_menu(s: &State) -> Vec<M> {
    let mut v = vec![
        M::Head("Onion skin".into()),
        item(if s.onion { "Turn off" } else { "Turn on" }, "O"),
        M::Sep,
    ];
    v.extend(
        ONION_RANGES
            .iter()
            .enumerate()
            .map(|(i, t)| item(t, check(s.strips.onion_range == i))),
    );
    v
}

fn onion_picked(s: &mut State, picked: &str) {
    if let Some(i) = ONION_RANGES.iter().position(|t| *t == picked) {
        s.strips.onion_range = i;
    } else {
        s.onion = !s.onion;
    }
}

/// The drawings' frame spans on a layer: (first frame, hold, mark) for each drawing.
fn spans(s: &State, layer: usize) -> Vec<(u32, u32, Mark)> {
    let mut at = 0;
    s.strips
        .exposures
        .get(layer)
        .map(|v| {
            v.iter()
                .map(|e| {
                    let span = (at, e.hold, e.mark);
                    at += e.hold;
                    span
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---- Animate: the timeline ----

const TRIGGER_H: f32 = 34.0;

fn tray_trigger_w(f: &mut Frame, t: &Tray, cur: &TrayOption) -> (f32, Option<Laid>) {
    let fg = if t.accent { WHITE } else { ON_DARK };
    let tag = (!cur.tag.is_empty()).then(|| text(f, cur.tag, 11.0, 700, fg));
    let w = 8.0 + 18.0 + tag.as_ref().map_or(0.0, |l| 3.0 + l.width) + 3.0 + 12.0 + 6.0;
    (w, tag)
}

fn tray_option_w(f: &mut Frame, o: &TrayOption, fg: [f32; 4]) -> (f32, Option<Laid>) {
    let tag = (!o.tag.is_empty()).then(|| text(f, o.tag, 11.0, 700, fg));
    let w = (6.0 + 18.0 + tag.as_ref().map_or(0.0, |l| 2.0 + l.width) + 6.0).max(34.0);
    (w, tag)
}

/// A tray's width: its trigger, and its options when open (2 px around).
fn tray_w(f: &mut Frame, s: &State, i: usize) -> f32 {
    let t = &TRAYS[i];
    let cur = &t.options[s.strips.picked[i].min(t.options.len() - 1)];
    let (mut w, _) = tray_trigger_w(f, t, cur);
    if s.strips.open_tray == Some(i) {
        w += 4.0;
        for (k, o) in t.options.iter().enumerate() {
            w += tray_option_w(f, o, ON_DARK).0 + if k > 0 { 2.0 } else { 0.0 };
        }
    }
    w + 4.0
}

/// One tray at `x`: its trigger (the current option's icon and ›), and, when open, every
/// option beside it. Picking one does it, makes it the tray's icon and folds the tray back.
fn tray(f: &mut Frame, s: &mut State, i: usize, x: f32, cy: f32) {
    let t = &TRAYS[i];
    let open = s.strips.open_tray == Some(i);
    let w = tray_w(f, s, i);
    let wrap = Rect::new(x, cy - TRIGGER_H / 2.0 - 2.0, w, TRIGGER_H + 4.0);
    if open {
        put(
            f,
            Quad::rect(wrap.x, wrap.y, wrap.w, wrap.h, TRAY_OPEN).radius(12.0),
        );
    }
    let cur = &t.options[s.strips.picked[i].min(t.options.len() - 1)];
    let (tw, tag) = tray_trigger_w(f, t, cur);
    let tr = Rect::new(x + 2.0, cy - TRIGGER_H / 2.0, tw, TRIGGER_H);
    let (bg, fg) = match (t.accent, open) {
        (true, _) => (CORAL, WHITE),
        (false, true) => (RAISED_OPEN, ON_DARK),
        (false, false) => (RAISED, ON_DARK),
    };
    let name = format!("strips/tray/{}", t.name);
    hit(s, &name, tr);
    let res = pad(f, &name, tr, bg, 10.0);
    let mut ix = tr.x + 8.0;
    icon(f, cur.icon, ix, cy - 9.0, 18.0, fg);
    ix += 18.0 + 3.0;
    if let Some(l) = &tag {
        text_at(f, l, ix, cy);
        ix += l.width + 3.0;
    }
    let chevron = if open { CHEVRON_LEFT } else { CHEVRON_RIGHT };
    icon(f, chevron, ix, cy - 6.0, 12.0, alpha(fg, 0.75));
    if res.hovered {
        let label = format!(
            "{}: {}{}",
            t.name,
            cur.label,
            if open { ". Collapse" } else { ". Show more" }
        );
        tooltip(f, id(&name), &label, tr);
    }
    if res.clicked {
        s.strips.open_tray = if open { None } else { Some(i) };
    }
    if !open {
        return;
    }
    let mut ox = tr.right() + 4.0;
    let current = s.strips.picked[i];
    for (k, o) in t.options.iter().enumerate() {
        let on = k == current;
        let fg = if on { INK } else { ON_DARK };
        let (ow, tag) = tray_option_w(f, o, fg);
        let r = Rect::new(ox, cy - TRIGGER_H / 2.0, ow, TRIGGER_H);
        let oname = format!("strips/tray/{}/{}", t.name, o.label);
        hit(s, &oname, r);
        let res = pad(
            f,
            &oname,
            r,
            if on { ON_DARK } else { alpha(ON_DARK, 0.0) },
            9.0,
        );
        let content = 18.0 + tag.as_ref().map_or(0.0, |l| 2.0 + l.width);
        let mut cx = r.x + (r.w - content) / 2.0;
        icon(f, o.icon, cx, cy - 9.0, 18.0, fg);
        cx += 20.0;
        if let Some(l) = &tag {
            text_at(f, l, cx, cy);
        }
        if res.hovered {
            tooltip(f, id(&oname), o.label, r);
        }
        if res.clicked {
            s.strips.picked[i] = k;
            s.strips.open_tray = None;
            s.actions.push(match o.label {
                "New drawing" => Action::NewDrawing,
                label => Action::Other(label.to_string()),
            });
        }
        ox += ow + 2.0;
    }
}

fn timeline(f: &mut Frame, r: Rect, s: &mut State) {
    let (x0, x1) = (r.x + 12.0, r.right() - 12.0);
    // the "where you are and drawing" row: two trays on the left, four on the right
    let cy = r.y + 8.0 + 19.0;
    let mut x = x0;
    for i in (0..TRAYS.len()).filter(|i| !TRAYS[*i].right) {
        let w = tray_w(f, s, i);
        tray(f, s, i, x, cy);
        x += w + 6.0;
    }
    let right: Vec<usize> = (0..TRAYS.len()).filter(|i| TRAYS[*i].right).collect();
    let widths: Vec<f32> = right.iter().map(|i| tray_w(f, s, *i)).collect();
    let mut x = x1 - widths.iter().sum::<f32>() - 6.0 * (right.len() as f32 - 1.0);
    for (i, w) in right.into_iter().zip(widths) {
        tray(f, s, i, x, cy);
        x += w + 6.0;
    }

    // the grid
    let bar_h = 30.0;
    let grid = Rect::new(x0, r.y + 54.0, x1 - x0, r.h - 54.0 - 8.0 - bar_h - 8.0);
    grid_area(f, grid, r, s);

    // the "playback and view" bar
    playback_bar(
        f,
        Rect::new(x0, r.bottom() - 8.0 - bar_h, x1 - x0, bar_h),
        r,
        s,
    );
}

const NAMES_W: f32 = 170.0;
const EYE: &str = "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z M12 9a3 3 0 1 0 0.01 0Z";
const EYE_OFF: &str = "M3 3l18 18 M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4.1 M6.6 6.6A17 17 0 0 0 2 12s3.5 7 10 7a9.7 9.7 0 0 0 4.4-1";
const RULER_H: f32 = 24.0;

fn grid_area(f: &mut Frame, g: Rect, strip: Rect, s: &mut State) {
    put(f, Quad::rect(g.x, g.y, g.w, g.h, GRID).radius(12.0));
    let rows = s.layers.len() + 1;
    let rh = ((g.h - RULER_H) / rows as f32).clamp(20.0, 31.0);
    let cw = s.frame_width.clamp(ZOOM_MIN, ZOOM_MAX);
    let fx = g.x + NAMES_W;
    let fw = g.w - NAMES_W;
    // as many frames as fill the width, and some after the cut's end
    let columns = (s.frames + 8).max((fw / cw).ceil() as u32 + 1);
    let max_scroll = (columns as f32 * cw - fw).max(0.0);
    let p = f.pointer();
    let frames_area = Rect::new(fx, g.y, fw, g.h);
    if !drawing()
        && p.scroll != 0.0
        && p.inside
        && frames_area.contains(p.x, p.y)
        && !f.is_open_any()
    {
        s.strips.scroll_x += p.scroll;
    }
    s.strips.scroll_x = s.strips.scroll_x.clamp(0.0, max_scroll);
    let left = fx - s.strips.scroll_x;
    let clip = [fx, g.y, g.right(), g.bottom()];
    let first = (s.strips.scroll_x / cw).floor() as u32;
    let last = (((s.strips.scroll_x + fw) / cw).ceil() as u32).min(columns);

    // the ruler: a click goes to that frame
    let ruler = Rect::new(fx, g.y, fw, RULER_H);
    hit(s, "strips/ruler", ruler);
    let res = act(f, id("strips/ruler"), ruler);
    if res.clicked {
        let k = ((p.x - left) / cw).floor().max(0.0) as u32;
        s.actions
            .push(Action::GoToFrame(k.min(s.frames.saturating_sub(1))));
    }
    for k in first..last {
        let fnum = k + 1;
        let cx = left + k as f32 * cw;
        let current = k == s.frame;
        if current {
            put(f, Quad::rect(cx, g.y, cw, RULER_H, CORAL).clip(clip));
        }
        if fnum == 1 || fnum % 5 == 0 || current {
            let l = text(
                f,
                &fnum.to_string(),
                10.0,
                600,
                if current { INK } else { RULER },
            );
            let tx = cx + 3.0;
            if tx >= fx && tx + l.width <= g.right() - 2.0 {
                put_text(f, &l, tx, g.y + 6.0);
            }
        }
    }

    // above the names: the heading and New layer
    let hl = text(f, "LAYERS", 10.0, 700, RULER);
    text_at(f, &hl, g.x + 10.0, g.y + RULER_H / 2.0);
    let nb = Rect::new(g.x + NAMES_W - 6.0 - 22.0, g.y + 1.0, 22.0, 22.0);
    hit(s, "strips/new-layer", nb);
    let res = act(f, id("strips/new-layer"), nb);
    put(f, Quad::rect(nb.x, nb.y, nb.w, nb.h, RAISED).radius(6.0));
    f.icon(PLUS, nb.x + 4.0, nb.y + 4.0, 14.0, ON_DARK);
    if res.clicked {
        s.actions.push(Action::NewLayer);
    }

    // the rows: a layer each, then the sound
    for (li, layer) in s.layers.clone().iter().enumerate() {
        let y = g.y + RULER_H + li as f32 * rh;
        put(f, Quad::rect(g.x, y, g.w, 1.0, GRID_LINE));
        let name_r = Rect::new(g.x, y + 1.0, NAMES_W, rh - 1.0);
        if li == s.active_layer {
            let q = Quad::rect(name_r.x, name_r.y, name_r.w, name_r.h, ACTIVE_ROW);
            // the grid's rounded corner, bottom-left, isn't the row's business: clip square
            put(f, q.clip([g.x, g.y, g.right(), g.bottom()]));
        }
        let ny = name_r.y + name_r.h / 2.0;
        let badge = Rect::new(g.x + NAMES_W - 8.0 - 20.0, ny - 10.0, 20.0, 20.0);
        // the eye: hide or show the layer
        let eye = Rect::new(g.x + 6.0, ny - 12.0, 24.0, 24.0);
        let ename = format!("strips/eye/{li}");
        hit(s, &ename, eye);
        let er = act(f, id(&ename), eye);
        if er.hovered {
            put(
                f,
                Quad::rect(eye.x, eye.y, eye.w, eye.h, RAISED).radius(6.0),
            );
        }
        f.icon(
            if layer.visible { EYE } else { EYE_OFF },
            eye.x + 4.0,
            eye.y + 4.0,
            16.0,
            if layer.visible { ON_DARK } else { RULER },
        );
        if er.clicked {
            s.layers[li].visible = !s.layers[li].visible;
        }
        let name_x = eye.right() + 6.0;
        let mut nl = text(
            f,
            &layer.name,
            12.0,
            600,
            if layer.visible { ON_DARK } else { RULER },
        );
        if nl.width > badge.x - 6.0 - name_x {
            nl = text(f, &ellipsize(&layer.name, 12), 12.0, 600, ON_DARK);
        }
        text_at(f, &nl, name_x, ny);
        put(
            f,
            Quad::rect(badge.x, badge.y, badge.w, badge.h, RAISED).radius(6.0),
        );
        let bl = text(f, &layer.kind.to_string(), 11.0, 700, ON_DARK);
        text_at(f, &bl, badge.x + (20.0 - bl.width) / 2.0, ny);
        let lname = format!("strips/layer/{li}");
        hit(s, &lname, name_r);
        let res = act(f, id(&lname), name_r);
        if res.clicked {
            s.active_layer = li;
        }
        if res.context {
            s.strips.menu_layer = li;
            let items = layer_options(s, li);
            open_above(f, "strips/layer-options", &items, p.x, p.y, strip);
        }

        // the cels
        let cel_spans: Vec<(u32, u32, Mark, String)> = if layer.kind == 'S' {
            vec![(0, s.frames, Mark::None, "Static".into())]
        } else {
            spans(s, li)
                .into_iter()
                .enumerate()
                .map(|(n, (a, h, m))| (a, h, m, (n + 1).to_string()))
                .collect()
        };
        for (n, (start, hold, mark, label)) in cel_spans.into_iter().enumerate() {
            let cx = left + start as f32 * cw + 1.0;
            let w = hold as f32 * cw - 2.0;
            if cx + w < fx || cx > g.right() {
                continue;
            }
            let cr = Rect::new(cx, ny - 11.0, w, 22.0);
            let cname = format!("strips/cel/{li}/{n}");
            hit(s, &cname, cr);
            let visible = Rect::new(
                cr.x.max(fx),
                cr.y,
                (cr.right().min(g.right()) - cr.x.max(fx)).max(0.0),
                cr.h,
            );
            let res = act(f, id(&cname), visible);
            let picked = li == s.active_layer && (start..start + hold).contains(&s.frame);
            let shown = match s.strips.show {
                1 => mark != Mark::None,
                2 => mark == Mark::Key,
                _ => true,
            } || layer.kind == 'S';
            let mut fill = if picked { CEL_PICKED } else { CEL };
            if res.hovered {
                fill = mix(fill, ON_DARK, 0.08);
            }
            let (bw, bc) = if mark == Mark::Key {
                (2.0, SUN)
            } else {
                (1.0, CEL_LINE)
            };
            let a = if shown { 1.0 } else { 0.35 };
            put(
                f,
                Quad::rect(cr.x, cr.y, cr.w, cr.h, alpha(fill, a))
                    .radius(5.0)
                    .border(bw, alpha(bc, a))
                    .clip(clip),
            );
            // the number stays in sight while the cel is scrolled half out
            let ll = text(f, &label, 11.0, 700, alpha(ON_DARK, a));
            let badge = match mark {
                Mark::Key => "K",
                Mark::Breakdown => "B",
                Mark::None => "",
            };
            let bl = (!badge.is_empty()).then(|| text(f, badge, 9.0, 700, alpha(SUN, a)));
            let need = ll.width + bl.as_ref().map_or(0.0, |b| 4.0 + b.width);
            let tx = visible.x + 4.0;
            if visible.w >= need + 8.0 {
                text_at(f, &ll, tx, ny);
                if let Some(b) = &bl {
                    text_at(f, b, tx + ll.width + 4.0, ny + 0.5);
                }
            } else if visible.w >= ll.width + 8.0 {
                text_at(f, &ll, tx, ny);
            }
            if res.clicked {
                s.active_layer = li;
                s.actions
                    .push(Action::GoToFrame(start.min(s.frames.saturating_sub(1))));
            }
            if res.context {
                s.active_layer = li;
                s.strips.menu_layer = li;
                s.strips.menu_drawing = n;
                let items = frame_menu(s, li, n);
                open_above(f, "strips/frame", &items, p.x, p.y, strip);
            }
        }
    }
    // the sound
    let y = g.y + RULER_H + s.layers.len() as f32 * rh;
    put(f, Quad::rect(g.x, y, g.w, 1.0, GRID_LINE));
    let sy = y + 1.0 + (rh - 1.0) / 2.0;
    icon(f, WAVE, g.x + 8.0, sy - 7.0, 14.0, MUTED);
    let dl = text(f, "dialogue", 12.0, 600, ON_DARK);
    text_at(f, &dl, g.x + 8.0 + 14.0 + 6.0, sy);
    let (ss, sl) = s.strips.sound;
    if sl > 0 {
        put(
            f,
            Quad::rect(left + ss as f32 * cw, sy - 9.0, sl as f32 * cw, 18.0, SOUND)
                .radius(5.0)
                .clip(clip),
        );
    }
    // the playhead, over the ruler and every row
    let ph = left + s.frame as f32 * cw;
    let bottom = (y + rh).min(g.bottom());
    put(
        f,
        Quad::rect(ph, g.y, cw, bottom - g.y, alpha(CORAL, 0.28)).clip(clip),
    );
    // the names' column and the frames, apart
    put(f, Quad::rect(fx, g.y, 1.0, bottom - g.y, GRID_LINE));

    // the menus
    if let Some(p) = show_menu(
        f,
        "strips/layer-options",
        &layer_options(s, s.strips.menu_layer),
    ) {
        layer_option_picked(s, s.strips.menu_layer, &p);
    }
    let items = frame_menu(s, s.strips.menu_layer, s.strips.menu_drawing);
    if let Some(p) = show_menu(f, "strips/frame", &items) {
        s.actions.push(Action::Other(p));
    }
}

fn frame_menu(s: &State, layer: usize, drawing: usize) -> Vec<M> {
    let name = s.layers.get(layer).map_or("", |l| l.name.as_str());
    let head = match spans(s, layer).get(drawing) {
        Some((a, h, _)) if s.layers.get(layer).is_some_and(|l| l.kind == 'C') => format!(
            "Drawing {} · {name} · frames {}–{}",
            drawing + 1,
            a + 1,
            a + h
        ),
        _ => format!("Static · {name}"),
    };
    vec![
        M::Head(head),
        item("Remove drawing", "⌫"),
        item("Hold longer", "+1"),
        item("Hold shorter", "−1"),
        M::Sep,
        item("Copy frames", "Alt+C"),
        item("Paste frames", "Alt+V"),
        item("Cycle…", ""),
        item("Boil…", ""),
    ]
}

fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

/// A dark dropdown button: an optional icon, optional text, then ▾. Returns its width
/// (measuring only, when `at` is None) and its response.
fn dropdown(
    f: &mut Frame,
    name: &str,
    at: Option<(f32, f32)>,
    glyph: Option<(&str, f32)>,
    label: Option<&str>,
    bg: [f32; 4],
    fg: [f32; 4],
) -> (f32, Response, Rect) {
    let laid = label.map(|t| text(f, t, 12.0, 600, fg));
    let w = 8.0
        + glyph.map_or(0.0, |(_, sz)| sz + 4.0)
        + laid.as_ref().map_or(0.0, |l| l.width + 4.0)
        + 10.0
        + 8.0;
    let Some((x, cy)) = at else {
        return (w, Response::default(), Rect::default());
    };
    let r = Rect::new(x, cy - 15.0, w, 30.0);
    let res = pad(f, name, r, bg, 9.0);
    let mut ix = x + 8.0;
    if let Some((path, sz)) = glyph {
        if path == ONION_A {
            onion_icon(f, ix, cy, sz, fg);
        } else {
            icon(f, path, ix, cy - sz / 2.0, sz, fg);
        }
        ix += sz + 4.0;
    }
    if let Some(l) = &laid {
        text_at(f, l, ix, cy);
        ix += l.width + 4.0;
    }
    icon(f, CHEVRON_DOWN, ix, cy - 5.0, 10.0, fg);
    (w, res, r)
}

fn show_items(s: &State) -> Vec<M> {
    let mut v = vec![M::Head("Show in the timeline".into())];
    v.extend(
        SHOW.iter()
            .enumerate()
            .map(|(i, t)| item(t, check(s.strips.show == i))),
    );
    v
}

fn play_items(s: &State) -> Vec<M> {
    let mut v = vec![M::Head("Play".into())];
    v.extend(
        PLAY_RANGES
            .iter()
            .enumerate()
            .map(|(i, t)| item(t, check(s.strips.play_range == i))),
    );
    v.push(M::Sep);
    v.extend(
        PLAY_MODES
            .iter()
            .enumerate()
            .map(|(i, t)| item(t, check(s.strips.play_mode == i))),
    );
    v
}

fn fps_items(s: &State) -> Vec<M> {
    let mut v = vec![M::Head("Frame rate".into())];
    v.extend(
        FRAME_RATES
            .iter()
            .map(|r| item(&format!("{r} fps"), check(s.fps == *r))),
    );
    v
}

fn show_label(i: usize) -> &'static str {
    match i {
        1 => "Show: K + B",
        2 => "Show: Keys",
        _ => "Show: All",
    }
}

fn playback_bar(f: &mut Frame, b: Rect, strip: Rect, s: &mut State) {
    let cy = b.y + b.h / 2.0;
    let gap = 6.0;
    // measure: the view group, playback in the middle, the zoom at the end
    let onion_w = dropdown(f, "", None, Some((ONION_A, 16.0)), None, SUN, INK).0;
    let show = show_label(s.strips.show);
    let show_w = dropdown(f, "", None, None, Some(show), RAISED, ON_DARK).0;
    let view_w = onion_w + show_w + 3.0 * 30.0 + 4.0 * gap;
    let loop_w = dropdown(f, "", None, Some((LOOP, 14.0)), None, RAISED, ON_DARK).0;
    let fps = format!("{} fps", s.fps);
    let fps_w = dropdown(f, "", None, None, Some(&fps), RAISED, ON_DARK).0;
    let secs = s.frame as f32 / s.fps.max(1) as f32;
    let now = text(f, &(s.frame + 1).to_string(), 13.0, 700, ON_DARK);
    let rest = text(
        f,
        &format!(" / {} · {:.2} s ·", s.frames, secs),
        13.0,
        500,
        MUTED,
    );
    let counter_w = 8.0 + now.width + rest.width + 8.0;
    let play_w = 30.0 + 40.0 + 30.0 + loop_w + counter_w + fps_w + 5.0 * gap;
    let zoom_w = 26.0 + 120.0 + 26.0 + 2.0 * gap;
    let (vx, px, zx) = three(b.x, b.right(), gap, view_w, play_w, zoom_w);

    // view: onion ▾, Show ▾, camera, pin, flip
    let mut x = vx;
    let (onion_bg, onion_fg) = if s.onion {
        (SUN, INK)
    } else {
        (RAISED, ON_DARK)
    };
    let (w, res, r) = dropdown(
        f,
        "strips/onion",
        Some((x, cy)),
        Some((ONION_A, 16.0)),
        None,
        onion_bg,
        onion_fg,
    );
    hit(s, "strips/onion", r);
    // the ▾ (its last 18 px) or a right-click: the settings; the rest: on or off
    let p = f.pointer();
    let on_chevron = p.x >= r.right() - 18.0;
    if res.held && p.pressed {
        s.strips.pressed_open = f.is_open("strips/onion-menu");
    }
    if res.clicked {
        if on_chevron {
            if !s.strips.pressed_open {
                open_above(f, "strips/onion-menu", &onion_menu(s), r.x, r.y, strip);
            }
        } else {
            s.onion = !s.onion;
        }
    }
    if res.context {
        open_above(f, "strips/onion-menu", &onion_menu(s), r.x, r.y, strip);
    }
    x += w + gap;
    let (w, res, r) = dropdown(
        f,
        "strips/show",
        Some((x, cy)),
        None,
        Some(show),
        RAISED,
        ON_DARK,
    );
    hit(s, "strips/show", r);
    menu_button(f, s, "strips/show-menu", res, &show_items(s), r, strip);
    x += w + gap;
    let cam = Rect::new(x, cy - 15.0, 30.0, 30.0);
    hit(s, "strips/camera", cam);
    let (cbg, cfg) = if s.strips.camera {
        (ON_DARK, INK)
    } else {
        (RAISED, ON_DARK)
    };
    if icon_pad(f, "strips/camera", cam, cbg, CAMERA, 15.0, cfg).clicked {
        s.strips.camera = !s.strips.camera;
    }
    x += 30.0 + gap;
    let pin = Rect::new(x, cy - 15.0, 30.0, 30.0);
    hit(s, "strips/pin", pin);
    if icon_pad(f, "strips/pin", pin, RAISED, PIN, 15.0, ON_DARK).clicked {
        s.actions.push(Action::Other("Pin this frame".into()));
    }
    x += 30.0 + gap;
    let flip = Rect::new(x, cy - 15.0, 30.0, 30.0);
    hit(s, "strips/flip", flip);
    if icon_pad(f, "strips/flip", flip, RAISED, FLIP, 15.0, ON_DARK).clicked {
        s.actions.push(Action::Other("Flip".into()));
    }

    // playback: previous, Play, next, play options ▾, the frame counter, fps ▾
    let mut x = px;
    let prev = Rect::new(x, cy - 15.0, 30.0, 30.0);
    hit(s, "strips/previous-frame", prev);
    if icon_pad(
        f,
        "strips/previous-frame",
        prev,
        RAISED,
        PREVIOUS,
        13.0,
        ON_DARK,
    )
    .clicked
    {
        s.actions.push(Action::PreviousFrame);
    }
    x += 30.0 + gap;
    let play = Rect::new(x, cy - 15.0, 40.0, 30.0);
    hit(s, "strips/play", play);
    let play_icon = if s.playing { PAUSE } else { PLAY };
    if icon_pad(f, "strips/play", play, CORAL, play_icon, 13.0, WHITE).clicked {
        s.actions.push(if s.playing {
            Action::Stop
        } else {
            Action::Play
        });
    }
    x += 40.0 + gap;
    let next = Rect::new(x, cy - 15.0, 30.0, 30.0);
    hit(s, "strips/next-frame", next);
    if icon_pad(f, "strips/next-frame", next, RAISED, NEXT, 13.0, ON_DARK).clicked {
        s.actions.push(Action::NextFrame);
    }
    x += 30.0 + gap;
    let (w, res, r) = dropdown(
        f,
        "strips/play-options",
        Some((x, cy)),
        Some((LOOP, 14.0)),
        None,
        RAISED,
        ON_DARK,
    );
    hit(s, "strips/play-options", r);
    menu_button(f, s, "strips/play-menu", res, &play_items(s), r, strip);
    x += w + gap;
    text_at(f, &now, x + 8.0, cy);
    text_at(f, &rest, x + 8.0 + now.width, cy);
    x += counter_w + gap;
    let (_, res, r) = dropdown(
        f,
        "strips/fps",
        Some((x, cy)),
        None,
        Some(&fps),
        RAISED,
        ON_DARK,
    );
    hit(s, "strips/fps", r);
    menu_button(f, s, "strips/fps-menu", res, &fps_items(s), r, strip);

    // the zoom: narrower, the slider, wider
    let mut x = zx;
    let small = Rect::new(x, cy - 13.0, 26.0, 26.0);
    hit(s, "strips/zoom-out", small);
    let res = pad(f, "strips/zoom-out", small, alpha(RAISED, 0.0), 8.0);
    icon_in(f, MOUNTAIN, small, 12.0, MUTED);
    if res.clicked {
        s.frame_width = (s.frame_width - ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }
    x += 26.0 + gap;
    let slider = Rect::new(x, cy - 13.0, 120.0, 26.0);
    hit(s, "strips/zoom", slider);
    zoom_slider(f, slider, &mut s.frame_width);
    x += 120.0 + gap;
    let big = Rect::new(x, cy - 13.0, 26.0, 26.0);
    hit(s, "strips/zoom-in", big);
    let res = pad(f, "strips/zoom-in", big, alpha(RAISED, 0.0), 8.0);
    icon_in(f, MOUNTAIN, big, 22.0, MUTED);
    if res.clicked {
        s.frame_width = (s.frame_width + ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    // the menus
    if let Some(p) = show_menu(f, "strips/onion-menu", &onion_menu(s)) {
        onion_picked(s, &p);
    }
    if let Some(p) = show_menu(f, "strips/show-menu", &show_items(s)) {
        s.strips.show = SHOW.iter().position(|t| *t == p).unwrap_or(0);
    }
    if let Some(p) = show_menu(f, "strips/play-menu", &play_items(s)) {
        if let Some(i) = PLAY_RANGES.iter().position(|t| *t == p) {
            s.strips.play_range = i;
        }
        if let Some(i) = PLAY_MODES.iter().position(|t| *t == p) {
            s.strips.play_mode = i;
        }
    }
    if let Some(p) = show_menu(f, "strips/fps-menu", &fps_items(s))
        && let Some(rate) = p.strip_suffix(" fps").and_then(|v| v.parse().ok())
    {
        s.fps = rate;
    }
}

/// The timeline's zoom: a light knob on a dark track (as the design's range input).
fn zoom_slider(f: &mut Frame, r: Rect, value: &mut f32) {
    let res = act(f, id("strips/zoom"), r);
    let inner = Rect::new(r.x + 7.0, r.y, r.w - 14.0, r.h);
    if res.held {
        let u = ((f.pointer().x - inner.x) / inner.w).clamp(0.0, 1.0);
        *value = (ZOOM_MIN + u * (ZOOM_MAX - ZOOM_MIN)).round();
    }
    let u = ((*value - ZOOM_MIN) / (ZOOM_MAX - ZOOM_MIN)).clamp(0.0, 1.0);
    let cy = r.y + r.h / 2.0;
    put(
        f,
        Quad::rect(inner.x, cy - 2.0, inner.w, 4.0, TRACK).radius(2.0),
    );
    put(
        f,
        Quad::rect(inner.x, cy - 2.0, inner.w * u, 4.0, ON_DARK).radius(2.0),
    );
    let k = if res.held || res.hovered { 16.0 } else { 14.0 };
    let kx = inner.x + inner.w * u;
    put(
        f,
        Quad::rect(kx - k / 2.0, cy - k / 2.0, k, k, ON_DARK).radius(k / 2.0),
    );
}

// ---- Draw: the frame strip ----

fn frame_strip(f: &mut Frame, r: Rect, s: &mut State) {
    let cy = r.y + r.h / 2.0;
    let gap = 8.0;
    let (x0, x1) = (r.x + 12.0, r.right() - 12.0);
    let layer = s
        .layers
        .get(s.active_layer)
        .map_or(String::new(), |l| l.name.clone());
    let ll = text(f, &layer, 13.0, 600, ON_DARK);
    let layer_w = 12.0 + 16.0 + 8.0 + ll.width + 12.0;
    let left_w = layer_w + gap + 40.0;
    let n = s.drawings as usize;
    let cells_w = n as f32 * 44.0 + n.saturating_sub(1) as f32 * 4.0;
    let mid_w = 36.0 + gap + cells_w + gap + 36.0;
    let span = spans(s, s.active_layer)
        .get(s.drawing as usize)
        .map(|(a, h, _)| (a + 1, a + h));
    let here = text(f, &format!("Drawing {}", s.drawing + 1), 13.0, 700, ON_DARK);
    let frames_l = span.map(|(a, b)| text(f, &format!(" · frames {a}–{b}"), 13.0, 500, MUTED));
    let new_l = text(f, "New", 13.0, 600, WHITE);
    let new_w = 12.0 + 16.0 + 6.0 + new_l.width + 12.0;
    let info_w = here.width + frames_l.as_ref().map_or(0.0, |l| l.width);
    let right_w = info_w + gap + new_w;
    let (lx, mx, rx) = three(x0, x1, gap, left_w, mid_w, right_w);

    // the layer: a click picks another, a right-click opens its options
    let lr = Rect::new(lx, cy - 18.0, layer_w, 36.0);
    hit(s, "strips/layer", lr);
    let res = pad(f, "strips/layer", lr, RAISED, 10.0);
    icon(f, LAYERS, lr.x + 12.0, cy - 8.0, 16.0, ON_DARK);
    text_at(f, &ll, lr.x + 12.0 + 16.0 + 8.0, cy);
    let pick_items = layer_pick(s);
    menu_button(f, s, "strips/layer-pick", res, &pick_items, lr, r);
    if res.context {
        s.strips.menu_layer = s.active_layer;
        let items = layer_options(s, s.active_layer);
        open_above(f, "strips/layer-options", &items, lr.x, lr.y, r);
    }
    // onion skin
    let or = Rect::new(lx + layer_w + gap, cy - 18.0, 40.0, 36.0);
    hit(s, "strips/onion", or);
    let (bg, fg) = if s.onion {
        (SUN, INK)
    } else {
        (RAISED, ON_DARK)
    };
    let res = pad(f, "strips/onion", or, bg, 10.0);
    onion_icon(f, or.x + 12.0, cy, 16.0, fg);
    if res.clicked {
        s.onion = !s.onion;
    }
    if res.context {
        open_above(f, "strips/onion-menu", &onion_menu(s), or.x, or.y, r);
    }

    // previous, the drawings in this cut, next
    let mut x = mx;
    let pr = Rect::new(x, cy - 18.0, 36.0, 36.0);
    hit(s, "strips/previous-drawing", pr);
    if icon_pad(
        f,
        "strips/previous-drawing",
        pr,
        RAISED,
        PREVIOUS,
        14.0,
        ON_DARK,
    )
    .clicked
    {
        s.actions.push(Action::PreviousDrawing);
    }
    x += 36.0 + gap;
    for i in 0..n {
        let c = Rect::new(x, cy - 18.0, 44.0, 36.0);
        let name = format!("strips/drawing/{i}");
        hit(s, &name, c);
        let res = act(f, id(&name), c);
        let current = i == s.drawing as usize;
        let fill = if res.hovered && !current {
            mix(ON_DARK, INK, 0.08)
        } else {
            ON_DARK
        };
        let mut q = Quad::rect(c.x, c.y, c.w, c.h, fill).radius(8.0);
        if current {
            q = q.border(3.0, CORAL);
        }
        put(f, q);
        let nl = text(f, &(i + 1).to_string(), 11.0, 700, INK);
        text_at(f, &nl, c.x + (c.w - nl.width) / 2.0, cy);
        if res.clicked && !current {
            s.drawing = i as u32;
            s.actions
                .push(Action::Other(format!("Go to drawing {}", i + 1)));
        }
        x += 44.0 + 4.0;
    }
    x += gap - 4.0;
    let nr = Rect::new(x, cy - 18.0, 36.0, 36.0);
    hit(s, "strips/next-drawing", nr);
    if icon_pad(f, "strips/next-drawing", nr, RAISED, NEXT, 14.0, ON_DARK).clicked {
        s.actions.push(Action::NextDrawing);
    }

    // where you are, and New
    text_at(f, &here, rx, cy);
    if let Some(l) = &frames_l {
        text_at(f, l, rx + here.width, cy);
    }
    let new = Rect::new(rx + info_w + gap, cy - 18.0, new_w, 36.0);
    hit(s, "strips/new-drawing", new);
    let res = pad(f, "strips/new-drawing", new, CORAL, 10.0);
    icon(f, PLUS, new.x + 12.0, cy - 8.0, 16.0, WHITE);
    text_at(f, &new_l, new.x + 12.0 + 16.0 + 6.0, cy);
    if res.clicked {
        s.actions.push(Action::NewDrawing);
    }

    // the menus
    if let Some(p) = show_menu(f, "strips/layer-pick", &layer_pick(s)) {
        if let Some(i) = s.layers.iter().position(|l| l.name == p) {
            s.active_layer = i;
        } else {
            s.actions.push(Action::NewLayer);
        }
    }
    if let Some(p) = show_menu(
        f,
        "strips/layer-options",
        &layer_options(s, s.strips.menu_layer),
    ) {
        layer_option_picked(s, s.strips.menu_layer, &p);
    }
    if let Some(p) = show_menu(f, "strips/onion-menu", &onion_menu(s)) {
        onion_picked(s, &p);
    }
}

fn layer_pick(s: &State) -> Vec<M> {
    let mut v = vec![M::Head("Layer".into())];
    v.extend(
        s.layers
            .iter()
            .enumerate()
            .map(|(i, l)| item(&l.name, check(i == s.active_layer))),
    );
    v.push(M::Sep);
    v.push(item("New layer", ""));
    v
}

// ---- Colour: the drawings to colour ----

fn status_of(c: ColourStatus) -> (&'static str, [f32; 4]) {
    match c {
        ColourStatus::ToDo => ("To do", MUTED),
        ColourStatus::Flatted => ("Flatted", SUN),
        ColourStatus::Done => ("Done", LEAF),
    }
}

fn to_colour(f: &mut Frame, r: Rect, s: &mut State) {
    let (x0, x1) = (r.x + 12.0, r.right() - 12.0);
    // the head: where, how far along, the progress bar, a hint
    let cy = r.y + 10.0 + 9.0;
    let layer = s
        .layers
        .get(s.active_layer)
        .map_or(String::new(), |l| l.name.clone());
    let cut = TRAYS[1].options[s.strips.picked[1]].label;
    let cut = cut.split(" · ").next().unwrap_or(cut);
    let title = text(f, &format!("{cut} · {layer} layer"), 13.0, 700, ON_DARK);
    text_at(f, &title, x0, cy);
    let total = s.to_colour.len();
    let done = s
        .to_colour
        .iter()
        .filter(|c| **c == ColourStatus::Done)
        .count();
    let count = text(
        f,
        &format!("{done} of {total} drawings coloured"),
        12.0,
        500,
        MUTED,
    );
    let mut x = x0 + title.width + 10.0;
    text_at(f, &count, x, cy);
    x += count.width + 10.0;
    put(f, Quad::rect(x, cy - 3.0, 140.0, 6.0, TRACK).radius(3.0));
    let u = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    if u > 0.0 {
        put(
            f,
            Quad::rect(x, cy - 3.0, 140.0 * u, 6.0, LEAF)
                .radius(3.0)
                .clip([x, cy - 3.0, x + 140.0, cy + 3.0]),
        );
    }
    hit(s, "strips/progress", Rect::new(x, cy - 3.0, 140.0 * u, 6.0));
    let hint = text(
        f,
        "Right-click a drawing to copy its colours to the next ones",
        11.0,
        500,
        MUTED,
    );
    if x + 140.0 + 20.0 + hint.width <= x1 {
        text_at(f, &hint, x1 - hint.width, cy);
    }

    // the drawings, as cards
    let top = r.y + 10.0 + 18.0 + 8.0;
    let card_h = (r.bottom() - 10.0 - top).max(60.0);
    let thumb_h = (card_h - 2.0 * 2.0 - 2.0 * 6.0 - 6.0 - 16.0).clamp(24.0, 56.0);
    // "Drawing 4" on every card, or just "4" on every card when a status needs the room
    let widest = (0..total)
        .map(|i| text(f, status_of(s.to_colour[i]).0, 11.0, 500, MUTED).width)
        .fold(0.0, f32::max);
    let name_w = text(f, &format!("Drawing {total}"), 12.0, 600, ON_DARK).width;
    let full_names = name_w + 4.0 + 12.0 + widest <= 104.0;
    let mut x = x0;
    for i in 0..total {
        if x + 120.0 > x1 {
            break;
        }
        let c = Rect::new(x, top, 120.0, card_h);
        let name = format!("strips/card/{i}");
        hit(s, &name, c);
        let res = act(f, id(&name), c);
        let current = i == s.drawing as usize;
        let fill = if res.hovered {
            mix(TRAY_OPEN, ON_DARK, 0.06)
        } else {
            TRAY_OPEN
        };
        put(
            f,
            Quad::rect(c.x, c.y, c.w, c.h, fill)
                .radius(12.0)
                .border(2.0, if current { CORAL } else { alpha(CORAL, 0.0) }),
        );
        let th = Rect::new(c.x + 8.0, c.y + 8.0, 104.0, thumb_h);
        thumbnail(f, th, s.to_colour[i]);
        let ly = th.bottom() + 6.0 + 8.0;
        let (st, sc) = status_of(s.to_colour[i]);
        let sl = text(f, st, 11.0, 500, sc);
        let sx = c.right() - 8.0 - sl.width;
        text_at(f, &sl, sx, ly);
        let label = if full_names {
            format!("Drawing {}", i + 1)
        } else {
            (i + 1).to_string()
        };
        let nl = text(f, &label, 12.0, 600, ON_DARK);
        text_at(f, &nl, c.x + 8.0, ly);
        put(
            f,
            Quad::rect(sx - 4.0 - 8.0, ly - 4.0, 8.0, 8.0, sc).radius(4.0),
        );
        if res.clicked && !current {
            s.drawing = i as u32;
            s.actions
                .push(Action::Other(format!("Go to drawing {}", i + 1)));
        }
        if res.context {
            s.strips.menu_drawing = i;
            let items = card_menu(s, i);
            let p = f.pointer();
            open_above(f, "strips/card-menu", &items, p.x, p.y, r);
        }
        x += 120.0 + 8.0;
    }
    let items = card_menu(s, s.strips.menu_drawing);
    if let Some(p) = show_menu(f, "strips/card-menu", &items) {
        let i = s.strips.menu_drawing;
        let set = match p.as_str() {
            "Mark as to do" => Some(ColourStatus::ToDo),
            "Mark as flatted" => Some(ColourStatus::Flatted),
            "Mark as done" => Some(ColourStatus::Done),
            _ => None,
        };
        match (set, s.to_colour.get_mut(i)) {
            (Some(st), Some(c)) => *c = st,
            _ => s.actions.push(Action::Other(p)),
        }
    }
}

fn card_menu(s: &State, i: usize) -> Vec<M> {
    let st = s.to_colour.get(i).copied().unwrap_or(ColourStatus::ToDo);
    vec![
        M::Head(format!("Drawing {}", i + 1)),
        item("Copy its colours to the next ones", ""),
        M::Sep,
        item("Mark as to do", check(st == ColourStatus::ToDo)),
        item("Mark as flatted", check(st == ColourStatus::Flatted)),
        item("Mark as done", check(st == ColourStatus::Done)),
    ]
}

/// The design's little figure (a head, a jacket), coloured as far as the drawing is.
fn thumbnail(f: &mut Frame, r: Rect, st: ColourStatus) {
    put(f, Quad::rect(r.x, r.y, r.w, r.h, WHITE).radius(8.0));
    let clip = [r.x, r.y, r.right(), r.bottom()];
    let k = r.h / 56.0;
    let cx = r.x + r.w / 2.0;
    let (skin, jacket) = match st {
        ColourStatus::ToDo => (WHITE, WHITE),
        _ => (SKIN, CORAL),
    };
    let hd = 30.0 * k;
    put(
        f,
        Quad::rect(cx - hd / 2.0, r.y + 4.0 * k, hd, hd, skin)
            .radius(hd / 2.0)
            .border(1.6, INK)
            .clip(clip),
    );
    if st == ColourStatus::Done {
        // the shading the flats don't have yet
        let sh = hd * 0.3;
        put(
            f,
            Quad::rect(
                cx + hd / 2.0 - sh - 2.0,
                r.y + 4.0 * k + hd * 0.3,
                sh,
                hd * 0.5,
                SKIN_SHADE,
            )
            .radius(sh / 2.0)
            .clip(clip),
        );
    }
    let jw = 40.0 * k;
    let jy = r.y + 36.0 * k;
    put(
        f,
        Quad::rect(cx - jw / 2.0, jy, jw, r.bottom() - jy + 8.0, jacket)
            .radius(6.0 * k)
            .border(1.6, INK)
            .clip(clip),
    );
    if st == ColourStatus::Done {
        put(
            f,
            Quad::rect(
                cx + jw / 2.0 - 10.0 * k,
                jy + 2.0,
                8.0 * k,
                r.bottom() - jy,
                JACKET_SHADE,
            )
            .clip(clip),
        );
    }
}

// ---- Review: the review player ----

fn time(secs: f32) -> String {
    let m = (secs / 60.0).floor();
    format!("{}:{:05.2}", m as u32, secs - m * 60.0)
}

fn review_player(f: &mut Frame, r: Rect, s: &mut State) {
    let (x0, x1) = (r.x + 14.0, r.right() - 14.0);
    let top = r.y + ((r.h - (28.0 + 10.0 + 36.0)) / 2.0).max(10.0);
    let frames = s.frames.max(1);
    let at = |fr: u32| x0 + (x1 - x0) * ((fr + 1) as f32 / frames as f32).clamp(0.0, 1.0);

    // the scrub track: dragging goes to the frame under the pointer
    let track = Rect::new(x0, top, x1 - x0, 28.0);
    hit(s, "strips/scrub", track);
    let res = act(f, id("strips/scrub"), track);
    if res.held {
        let u = ((f.pointer().x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        let fr = ((u * frames as f32).round() as u32).saturating_sub(1);
        if s.strips.scrub_last != Some(fr) && fr != s.frame {
            s.actions.push(Action::GoToFrame(fr));
        }
        s.strips.scrub_last = Some(fr);
    } else {
        s.strips.scrub_last = None;
    }
    put(
        f,
        Quad::rect(x0, top + 11.0, x1 - x0, 6.0, TRACK).radius(3.0),
    );
    let px = at(s.frame);
    put(
        f,
        Quad::rect(x0, top + 11.0, px - x0, 6.0, CORAL).radius(3.0),
    );
    // the notes, then the knob over them (as the design)
    for n in &s.notes {
        let mx = at(n.frame);
        // a pin: round but for its bottom-left corner
        put(f, Quad::rect(mx - 9.0, top, 18.0, 18.0, INK).radius(9.0));
        put(
            f,
            Quad::rect(mx - 9.0, top + 9.0, 9.0, 9.0, INK).radius(3.0),
        );
        put(
            f,
            Quad::rect(mx - 7.0, top + 2.0, 14.0, 14.0, n.colour).radius(7.0),
        );
        put(
            f,
            Quad::rect(mx - 7.0, top + 9.0, 7.0, 7.0, n.colour).radius(2.0),
        );
    }

    put(
        f,
        Quad::rect(px - 11.0, top + 3.0, 22.0, 22.0, CORAL).radius(11.0),
    );
    put(
        f,
        Quad::rect(px - 8.0, top + 6.0, 16.0, 16.0, WHITE).radius(8.0),
    );
    // previous note, Play, next note, the time; asking for changes, approving
    let cy = top + 28.0 + 10.0 + 18.0;
    let mut x = x0;
    let pr = Rect::new(x, cy - 18.0, 36.0, 36.0);
    hit(s, "strips/previous-note", pr);
    if icon_pad(
        f,
        "strips/previous-note",
        pr,
        RAISED,
        PREVIOUS,
        14.0,
        ON_DARK,
    )
    .clicked
        && let Some(fr) = s
            .notes
            .iter()
            .map(|n| n.frame)
            .filter(|fr| *fr < s.frame)
            .max()
    {
        s.actions.push(Action::GoToFrame(fr));
    }
    x += 36.0 + 8.0;
    let play = Rect::new(x, cy - 18.0, 52.0, 36.0);
    hit(s, "strips/play", play);
    let play_icon = if s.playing { PAUSE } else { PLAY };
    let res = pad(f, "strips/play", play, CORAL, 10.0);
    icon_in(f, play_icon, play, 15.0, WHITE);
    if res.clicked {
        s.actions.push(if s.playing {
            Action::Stop
        } else {
            Action::Play
        });
    }
    x += 52.0 + 8.0;
    let nr = Rect::new(x, cy - 18.0, 36.0, 36.0);
    hit(s, "strips/next-note", nr);
    if icon_pad(f, "strips/next-note", nr, RAISED, NEXT, 14.0, ON_DARK).clicked
        && let Some(fr) = s
            .notes
            .iter()
            .map(|n| n.frame)
            .filter(|fr| *fr > s.frame)
            .min()
    {
        s.actions.push(Action::GoToFrame(fr));
    }
    x += 36.0 + 8.0;
    let fps = s.fps.max(1) as f32;
    let now = text(f, &time(s.frame as f32 / fps), 13.0, 700, ON_DARK);
    let rest = text(
        f,
        &format!(" / {} · frame {}", time(s.frames as f32 / fps), s.frame + 1),
        13.0,
        500,
        MUTED,
    );
    text_at(f, &now, x + 8.0, cy);
    text_at(f, &rest, x + 8.0 + now.width, cy);
    let info_end = x + 8.0 + now.width + rest.width + 8.0;

    let approve_l = text(f, "Approve cut", 13.0, 700, INK);
    let ask_l = text(f, "Ask for changes", 13.0, 600, ON_DARK);
    let aw = approve_l.width + 28.0;
    let kw = ask_l.width + 28.0;
    let ar = Rect::new(x1 - aw, cy - 18.0, aw, 36.0);
    let kr = Rect::new(ar.x - 8.0 - kw, cy - 18.0, kw, 36.0);
    hit(s, "strips/approve", ar);
    hit(s, "strips/ask", kr);
    if pad(f, "strips/ask", kr, RAISED, 10.0).clicked {
        s.actions.push(Action::Other("Ask for changes".into()));
    }
    text_at(f, &ask_l, kr.x + 14.0, cy);
    if pad(f, "strips/approve", ar, LEAF, 10.0).clicked {
        s.actions.push(Action::Other("Approve cut".into()));
    }
    text_at(f, &approve_l, ar.x + 14.0, cy);
    let hint = text(
        f,
        "Right-click the player for speed, loop and side-by-side",
        11.0,
        500,
        MUTED,
    );
    if info_end + 16.0 + hint.width <= kr.x - 8.0 {
        text_at(f, &hint, kr.x - 8.0 - hint.width, cy);
    }
    // a right-click anywhere on the player: speed, loop, side by side
    let p = f.pointer();
    if !drawing() && p.context && p.inside && r.contains(p.x, p.y) && !f.is_open_any() {
        open_above(f, "strips/player-menu", &player_menu(), p.x, p.y, r);
    }
    if let Some(p) = show_menu(f, "strips/player-menu", &player_menu()) {
        s.actions.push(Action::Other(p));
    }
}

fn player_menu() -> Vec<M> {
    vec![
        M::Head("Player".into()),
        item("Speed", "1×"),
        item("Loop", ""),
        item("Side by side", ""),
    ]
}

/// Whether any of the strips' menus is open (the pointer is theirs then).
trait OpenAny {
    fn is_open_any(&self) -> bool;
}

impl OpenAny for Frame<'_> {
    fn is_open_any(&self) -> bool {
        [
            "strips/layer-options",
            "strips/frame",
            "strips/onion-menu",
            "strips/show-menu",
            "strips/play-menu",
            "strips/fps-menu",
            "strips/layer-pick",
            "strips/card-menu",
            "strips/player-menu",
        ]
        .iter()
        .any(|m| self.is_open(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::layout;
    use pencilpilot_gpu::{Error, Gpu, Renderer2d};
    use pencilpilot_ui::ui::Ui;

    /// The strip alone, at the design's 1440 × 900, with simulated input.
    struct H {
        ui: Ui,
        r: Renderer2d,
        _gpu: Gpu,
        s: State,
    }

    impl H {
        fn new(ws: Workspace) -> Option<H> {
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
            s.workspace = ws;
            let mut h = H {
                ui,
                r,
                _gpu: gpu,
                s,
            };
            h.pass();
            Some(h)
        }
        fn strip(&self) -> Rect {
            layout::regions(1440.0, 900.0, self.s.workspace).strip
        }
        /// A pass; whether it drew anything.
        fn pass(&mut self) -> bool {
            let strip = self.strip();
            let s = &mut self.s;
            self.ui
                .pass(&mut self.r, 1440.0, 900.0, |f| build(f, strip, s))
                .is_some()
        }
        fn at(&self, name: &str) -> Rect {
            *self
                .s
                .strips
                .hits
                .get(name)
                .unwrap_or_else(|| panic!("no {name} in this pass"))
        }
        fn has(&self, name: &str) -> bool {
            self.s.strips.hits.contains_key(name)
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
        fn right_click(&mut self, name: &str) {
            let r = self.at(name);
            self.ui.pointer_moved(r.x + r.w / 2.0, r.y + r.h / 2.0);
            self.pass();
            self.ui.pointer_context();
            self.pass();
            self.pass();
        }
        /// Click item `index` (counting heads and separators) of a menu opened above
        /// (`x`, `y`) with `items`.
        fn pick(&mut self, items: &[M], x: f32, y: f32, index: usize) {
            let strip = self.strip();
            let x = x.min(strip.right() - 280.0).max(strip.x);
            let mut iy = y - 6.0 - menu_height(items) + 6.0;
            for it in &items[..index] {
                iy += match it {
                    M::Head(_) => 26.0,
                    M::Item(..) => 32.0,
                    M::Sep => 9.0,
                };
            }
            assert!(matches!(items[index], M::Item(..)), "an item to pick");
            self.click_at(x + 30.0, iy + 16.0);
        }
        /// With `PP_SAVE` set (a folder), what's on screen as a PPM there.
        fn save(&mut self, name: &str) {
            let Ok(dir) = std::env::var("PP_SAVE") else {
                return;
            };
            let frame = pencilpilot_gpu::DmabufImage::new(&self._gpu, 1440, 900, &[]).unwrap();
            self.r
                .draw(&self._gpu, &frame, self.ui.theme.paper, self.ui.shown())
                .unwrap();
            let px = self._gpu.read_back(&frame).unwrap();
            let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
            ppm.extend(px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
            std::fs::write(format!("{dir}/{name}.ppm"), ppm).unwrap();
        }
        fn take(&mut self) -> Vec<Action> {
            std::mem::take(&mut self.s.actions)
        }
        /// Away from everything, then idle: nothing more to draw.
        fn settles(&mut self) -> bool {
            self.ui.pointer_moved(4.0, 4.0);
            self.pass();
            !self.pass() && !self.pass()
        }
    }

    #[test]
    fn trays_open_pick_fold_back_and_stay_one_at_a_time() {
        let Some(mut h) = H::new(Workspace::Animate) else {
            return;
        };
        assert_eq!(h.s.strips.open_tray, None);
        assert!(!h.has("strips/tray/Scene/Scene 2"), "closed: no options");
        h.click("strips/tray/Scene");
        assert_eq!(h.s.strips.open_tray, Some(0), "the icon opened its tray");
        assert!(h.has("strips/tray/Scene/Scene 2"), "its options show");
        // another tray: the first folds back
        h.click("strips/tray/Hold");
        assert_eq!(h.s.strips.open_tray, Some(3), "only one tray at a time");
        assert!(!h.has("strips/tray/Scene/Scene 2"));
        assert!(h.has("strips/tray/Hold/Hold on 1s"));
        h.save("strips-tray-open");
        // picking: it's done, it becomes the tray's icon, the tray folds back
        h.click("strips/tray/Hold/Hold on 1s");
        assert_eq!(h.s.strips.picked[3], 0);
        assert_eq!(h.s.strips.open_tray, None, "picking folded it back");
        assert!(!h.has("strips/tray/Hold/Hold on 1s"));
        assert_eq!(h.take(), vec![Action::Other("Hold on 1s".into())]);
        // New drawing from the New tray
        h.click("strips/tray/New");
        h.click("strips/tray/New/Duplicate drawing");
        assert_eq!(h.s.strips.picked[2], 1);
        h.click("strips/tray/New");
        h.click("strips/tray/New/New drawing");
        assert_eq!(
            h.take(),
            vec![
                Action::Other("Duplicate drawing".into()),
                Action::NewDrawing
            ]
        );
        // tapping the icon again closes it without changing anything
        h.click("strips/tray/Frames");
        assert_eq!(h.s.strips.open_tray, Some(5));
        h.click("strips/tray/Frames");
        assert_eq!(h.s.strips.open_tray, None);
        assert_eq!(h.s.strips.picked[5], 0);
        assert!(h.take().is_empty());
        assert!(h.settles(), "idle draws nothing");
    }

    #[test]
    fn play_and_the_frame_buttons_push_their_actions() {
        let Some(mut h) = H::new(Workspace::Animate) else {
            return;
        };
        h.click("strips/play");
        h.click("strips/previous-frame");
        h.click("strips/next-frame");
        assert_eq!(
            h.take(),
            vec![Action::Play, Action::PreviousFrame, Action::NextFrame]
        );
        h.s.playing = true;
        h.click("strips/play");
        assert_eq!(h.take(), vec![Action::Stop], "playing: Play stops");
        // the ruler: frame 20
        let r = h.at("strips/ruler");
        let cw = h.s.frame_width;
        h.click_at(r.x + 19.5 * cw, r.y + 10.0);
        assert_eq!(h.take(), vec![Action::GoToFrame(19)]);
        // a cel: its first frame (Clean's drawing 2 starts at frame 7)
        h.click("strips/cel/0/1");
        assert_eq!(h.take(), vec![Action::GoToFrame(6)]);
        // the camera frame, pin and flip
        h.click("strips/camera");
        assert!(h.s.strips.camera);
        h.click("strips/pin");
        h.click("strips/flip");
        assert_eq!(
            h.take(),
            vec![
                Action::Other("Pin this frame".into()),
                Action::Other("Flip".into())
            ]
        );
        assert!(h.settles());
    }

    #[test]
    fn the_zoom_changes_frame_width_and_the_grid_follows() {
        let Some(mut h) = H::new(Workspace::Animate) else {
            return;
        };
        let cel = |h: &H| h.at("strips/cel/0/1"); // Clean's drawing 2: frames 7 to 12
        let before = cel(&h);
        assert!((before.w - (6.0 * 22.0 - 2.0)).abs() < 0.01);
        // drag the slider to three quarters
        let r = h.at("strips/zoom");
        let inner_x = r.x + 7.0;
        let inner_w = r.w - 14.0;
        h.ui.pointer_moved(r.x + r.w / 2.0, r.y + r.h / 2.0);
        h.pass();
        h.ui.pointer_button(true);
        h.pass();
        h.ui.pointer_moved(inner_x + 0.75 * inner_w, r.y + 40.0); // it follows off the track
        h.pass();
        h.ui.pointer_button(false);
        h.pass();
        assert_eq!(h.s.frame_width, 35.0, "8 + 3/4 of 36");
        let after = cel(&h);
        assert!(
            (after.w - (6.0 * 35.0 - 2.0)).abs() < 0.01,
            "the cel widened: {after:?}"
        );
        assert!(after.x > before.x, "and moved along");
        // the mountains: wider, wider (to the end), narrower
        h.click("strips/zoom-in");
        assert_eq!(h.s.frame_width, 41.0);
        h.click("strips/zoom-in");
        assert_eq!(h.s.frame_width, ZOOM_MAX);
        h.click("strips/zoom-out");
        assert_eq!(h.s.frame_width, 38.0);
        assert!((cel(&h).w - (6.0 * 38.0 - 2.0)).abs() < 0.01);
        // the wheel over the grid scrolls it
        let r = h.at("strips/ruler");
        h.ui.pointer_moved(r.x + 100.0, r.y + 60.0);
        h.pass();
        h.ui.scrolled(120.0);
        h.pass();
        assert_eq!(h.s.strips.scroll_x, 120.0);
        assert!(h.settles());
    }

    #[test]
    fn onion_skin_toggles() {
        let Some(mut h) = H::new(Workspace::Animate) else {
            return;
        };
        assert!(h.s.onion);
        let r = h.at("strips/onion");
        h.click_at(r.x + 10.0, r.y + r.h / 2.0);
        assert!(!h.s.onion, "off");
        h.click_at(r.x + 10.0, r.y + r.h / 2.0);
        assert!(h.s.onion, "on again");
        // its ▾: the settings, and onion skin stays as it was
        h.click_at(r.right() - 6.0, r.y + r.h / 2.0);
        assert!(h.s.onion);
        assert!(h.ui.overlay_open(), "the onion skin menu opened");
        let items = onion_menu(&h.s);
        h.pick(&items, r.x, r.y, 3); // the head, Turn off, a separator, then the ranges
        assert_eq!(h.s.strips.onion_range, 0);
        assert!(!h.ui.overlay_open());

        let Some(mut h) = H::new(Workspace::Draw) else {
            return;
        };
        h.click("strips/onion");
        assert!(!h.s.onion);
        h.click("strips/onion");
        assert!(h.s.onion);
        assert!(h.settles());
    }

    #[test]
    fn the_timeline_menus_open_above_and_pick() {
        let Some(mut h) = H::new(Workspace::Animate) else {
            return;
        };
        // fps ▾: opens, a second press closes it, then 30 fps
        h.click("strips/fps");
        assert!(h.ui.overlay_open());
        h.click("strips/fps");
        assert!(!h.ui.overlay_open(), "pressed again: closed");
        h.click("strips/fps");
        h.save("strips-fps-menu");
        let r = h.at("strips/fps");
        let items = fps_items(&h.s);
        h.pick(&items, r.x, r.y, 4); // head, 12, 24, 25, 30
        assert_eq!(h.s.fps, 30);
        // Show ▾: keys only
        h.click("strips/show");
        let r = h.at("strips/show");
        let items = show_items(&h.s);
        h.pick(&items, r.x, r.y, 3);
        assert_eq!(h.s.strips.show, 2);
        // a layer's name: a click picks it, a right-click opens its options
        h.click("strips/layer/2");
        assert_eq!(h.s.active_layer, 2);
        h.right_click("strips/layer/1");
        assert!(h.ui.overlay_open());
        let r = h.at("strips/layer/1");
        let items = layer_options(&h.s, 1);
        h.pick(&items, r.x + r.w / 2.0, r.y + r.h / 2.0, 1);
        assert!(h.s.layers[1].locked, "Lock layer");
        assert!(h.settles());
    }

    #[test]
    fn the_frame_strip_buttons_do_what_they_say() {
        let Some(mut h) = H::new(Workspace::Draw) else {
            return;
        };
        h.click("strips/previous-drawing");
        h.click("strips/next-drawing");
        h.click("strips/new-drawing");
        assert_eq!(
            h.take(),
            vec![
                Action::PreviousDrawing,
                Action::NextDrawing,
                Action::NewDrawing
            ]
        );
        assert_eq!(h.at("strips/drawing/0").w, 44.0);
        assert_eq!(h.at("strips/drawing/0").h, 36.0);
        h.click("strips/drawing/5");
        assert_eq!(h.s.drawing, 5);
        assert_eq!(h.take(), vec![Action::Other("Go to drawing 6".into())]);
        // the layer: a click lists the layers to pick from
        h.click("strips/layer");
        assert!(h.ui.overlay_open());
        let r = h.at("strips/layer");
        let items = layer_pick(&h.s);
        h.pick(&items, r.x, r.y, 2); // head, Clean, Tie-down
        assert_eq!(h.s.active_layer, 1);
        // a right-click: the layer's options
        h.right_click("strips/layer");
        assert!(h.ui.overlay_open());
        let items = layer_options(&h.s, 1);
        h.pick(&items, r.x, r.y, 5); // head, lock, outlines, sep, cel, static
        assert_eq!(h.s.layers[1].kind, 'S');
        assert!(h.settles());
    }

    #[test]
    fn the_drawings_to_colour_pick_and_mark() {
        let Some(mut h) = H::new(Workspace::Colour) else {
            return;
        };
        let bar = h.at("strips/progress").w;
        assert!((bar - 140.0 * 3.0 / 8.0).abs() < 0.01, "3 of 8 done");
        assert_eq!(h.at("strips/card/0").w, 120.0);
        h.click("strips/card/6");
        assert_eq!(h.s.drawing, 6);
        assert_eq!(h.take(), vec![Action::Other("Go to drawing 7".into())]);
        h.right_click("strips/card/5");
        assert!(h.ui.overlay_open());
        let r = h.at("strips/card/5");
        let items = card_menu(&h.s, 5);
        h.pick(&items, r.x + r.w / 2.0, r.y + r.h / 2.0, 5); // head, copy, sep, to do, flatted, done
        assert_eq!(h.s.to_colour[5], ColourStatus::Done);
        let bar = h.at("strips/progress").w;
        assert!((bar - 140.0 * 4.0 / 8.0).abs() < 0.01, "4 of 8 done");
        assert!(h.settles());
    }

    #[test]
    fn the_review_player_plays_scrubs_and_jumps_between_notes() {
        let Some(mut h) = H::new(Workspace::Review) else {
            return;
        };
        h.click("strips/play");
        assert_eq!(h.take(), vec![Action::Play]);
        // frame 15 (index 14): the next note is Rin's at 22; none before
        h.click("strips/next-note");
        h.click("strips/previous-note");
        assert_eq!(h.take(), vec![Action::GoToFrame(22)]);
        h.s.frame = 30;
        h.click("strips/previous-note");
        assert_eq!(h.take(), vec![Action::GoToFrame(22)]);
        // the scrub: halfway is frame 24 (index 23), and a drag asks once per frame
        let t = h.at("strips/scrub");
        h.ui.pointer_moved(t.x + t.w / 2.0, t.y + 14.0);
        h.pass();
        h.ui.pointer_button(true);
        h.pass();
        h.pass();
        h.ui.pointer_moved(t.x + t.w * 0.75, t.y + 14.0);
        h.pass();
        h.ui.pointer_button(false);
        h.pass();
        assert_eq!(h.take(), vec![Action::GoToFrame(23), Action::GoToFrame(35)]);
        h.click("strips/ask");
        h.click("strips/approve");
        assert_eq!(
            h.take(),
            vec![
                Action::Other("Ask for changes".into()),
                Action::Other("Approve cut".into())
            ]
        );
        assert!(h.settles());
    }

    #[test]
    fn every_strip_is_idle_when_nothing_happens() {
        for ws in Workspace::ALL {
            let Some(mut h) = H::new(ws) else {
                return;
            };
            assert!(!h.pass(), "{ws:?}: the second pass draws nothing");
            h.ui.pointer_moved(700.0, 880.0); // over the strip, on nothing in particular
            h.pass();
            assert!(h.settles(), "{ws:?}");
        }
    }
}
