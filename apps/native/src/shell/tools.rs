//! The tool column (dark), with the colour swatch at its foot and the colour pop-up it
//! opens: a saturation/value square, a hue slider, hex, the eyedropper, recent colours and
//! the palette. A right-click on a tool opens its menu.

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Rect, id};

use crate::state::{Action, State, Tool, Workspace};

// the design's line icons (24 × 24)
const BRUSH: &str = "M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z";
const ERASER: &str = "m7 21-4-4L14 6l5 5-8 10Z M22 21H7";
const FILL: &str = "m5 11 7-7 7 7-7 7Z M20 15s2 2.5 2 4a2 2 0 0 1-4 0c0-1.5 2-4 2-4Z";
const LASSO: &str = "M7 18c-3-1-4-4-3-7 2-5 10-7 14-4 3 2 2 6-2 8-3 1-6 1-8 0 M7 18v3";
/// The design draws Select's rounded square dashed (3 on, 3 off); the icon raster has no
/// dashes, so here are the dashes themselves.
const SELECT: &str = "M4 7V6a2 2 0 0 1 2-2h1 M10 4h4 M17 4h1a2 2 0 0 1 2 2v1 M20 10v4 \
     M20 17v1a2 2 0 0 1-2 2h-1 M14 20h-4 M7 20H6a2 2 0 0 1-2-2v-1 M4 14v-4";
const TRANSFORM: &str = "M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3";
const EYEDROPPER: &str = "m14 7 3 3M5 19l9.5-9.5M17 4a2.1 2.1 0 0 1 3 3l-2 2-3-3Z M5 19v-2";
const EYEDROPPER_SMALL: &str = "m14 7 3 3M5 19l9.5-9.5M17 4a2.1 2.1 0 0 1 3 3l-2 2-3-3Z";
const TEXT: &str = "M5 6h14M12 6v13";
const HAND: &str = "M8 13V5.5a1.5 1.5 0 0 1 3 0V12 M11 11V4.5a1.5 1.5 0 0 1 3 0V12 M14 11.5V6a1.5 1.5 0 0 1 3 0v8c0 4-2.5 6-6 6s-5-2-6.5-5L3 11.5a1.5 1.5 0 0 1 2.5-1.5L8 13";
const SHAPE: &str = "M4 4h9v9H4Z M17 11a4 4 0 1 0 0.01 0Z";

const POPUP: &str = "tools/colours";
const TOOL_MENU: &str = "tools/menu";
const RAIL_MENU: &str = "tools/rail";

// the design's colours that the theme doesn't name
const RAISED_HOVER: [f32; 4] = hex(0x3A3A42);
const SWATCH_RING: [f32; 4] = hex(0xFFD84D);
const POPUP_LINE: [f32; 4] = hex(0xE4E0D6);
const SUBTLE: [f32; 4] = hex(0x5E5D63);
const LABEL: [f32; 4] = hex(0x44434A);
const QUIET: [f32; 4] = hex(0xEFECE4);

/// A tool's name (with its shortcut, as the design's tooltips), shortcut and icon.
pub fn tool_info(t: Tool) -> (&'static str, &'static str, &'static str) {
    match t {
        Tool::Brush => ("Brush", "B", BRUSH),
        Tool::Eraser => ("Eraser", "E", ERASER),
        Tool::Fill => ("Fill", "G", FILL),
        Tool::Lasso => ("Lasso fill", "F", LASSO),
        Tool::Select => ("Select", "M", SELECT),
        Tool::Transform => ("Transform", "V", TRANSFORM),
        Tool::Eyedropper => ("Eyedropper", "I", EYEDROPPER),
        Tool::Text => ("Text", "T", TEXT),
        Tool::Shape => ("Shape", "U", SHAPE),
        Tool::Hand => ("Hand", "Space", HAND),
    }
}

/// The tools each workspace's column shows, top to bottom (the design's tools data).
pub fn rail_tools(w: Workspace) -> &'static [Tool] {
    use Tool::*;
    match w {
        Workspace::Draw => &[
            Brush, Eraser, Fill, Lasso, Select, Transform, Eyedropper, Text,
        ],
        Workspace::Colour => &[Fill, Lasso, Eyedropper, Brush, Eraser],
        Workspace::Animate | Workspace::Review => {
            &[Brush, Eraser, Fill, Lasso, Select, Transform, Eyedropper]
        }
    }
}

/// The tool a workspace starts with (Colour: Fill; the rest: Brush).
pub fn default_tool(w: Workspace) -> Tool {
    if w == Workspace::Colour {
        Tool::Fill
    } else {
        Tool::Brush
    }
}

/// Where the pop-up's parts were drawn in the last pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PopupLayout {
    pub rect: Rect,
    pub sv: Rect,
    pub hue: Rect,
    pub hex: Rect,
    pub eyedropper: Rect,
    pub recent: Vec<Rect>,
    pub palette: Vec<Rect>,
}

/// Where the column's parts were drawn in the last pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolsLayout {
    pub tools: Vec<(Tool, Rect)>,
    pub swatch: Rect,
    pub popup: Option<PopupLayout>,
}

/// This piece's own state.
#[derive(Debug, Clone)]
pub struct ToolsState {
    /// the tool each workspace had when it was left (in [`Workspace::ALL`] order)
    pub remembered: [Option<Tool>; 4],
    /// the workspace the column was last drawn for
    seen: Option<Workspace>,
    /// tools hidden from the column (by its right-click menu)
    pub hidden: Vec<Tool>,
    /// tools whose size and opacity follow the pen's pressure
    pub pressure_size: Vec<Tool>,
    pub pressure_opacity: Vec<Tool>,
    /// the tool whose right-click menu is open
    menu_tool: Option<Tool>,
    /// whether the pop-up was open when the swatch was pressed (a click then closes it)
    open_at_press: bool,
    /// whether the pop-up was open in the last pass, and the colour it opened with
    was_open: bool,
    opened_with: [f32; 4],
    /// the pop-up's hue (0–360), saturation and value, kept so that a grey keeps its hue;
    /// `hsv_for` is the colour they were taken from
    pub hsv: [f32; 3],
    hsv_for: Option<[f32; 4]>,
    /// the hex field's text, and the colour it was written for
    pub hex: String,
    hex_for: Option<[f32; 4]>,
    /// colours picked lately, newest first
    pub recent: Vec<(String, [f32; 4])>,
    pub layout: ToolsLayout,
}

impl Default for ToolsState {
    fn default() -> Self {
        ToolsState {
            remembered: [None; 4],
            seen: None,
            hidden: Vec::new(),
            pressure_size: vec![Tool::Brush, Tool::Eraser],
            pressure_opacity: Vec::new(),
            menu_tool: None,
            open_at_press: false,
            was_open: false,
            opened_with: [0.0; 4],
            hsv: [0.0; 3],
            hsv_for: None,
            hex: String::new(),
            hex_for: None,
            recent: vec![
                ("Rust".into(), hex(0xC5432C)),
                ("Skin".into(), hex(0xF2C9A8)),
                ("Plum".into(), hex(0x3B2A4F)),
                ("Sky".into(), hex(0x8FB8E8)),
            ],
            layout: ToolsLayout::default(),
        }
    }
}

const BUTTON: f32 = 44.0;
const STEP: f32 = 50.0; // 44 and the design's 6 px gap
const SWATCH: f32 = 40.0;
pub const POPUP_W: f32 = 300.0;

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    let t = f.theme();
    // each workspace keeps its own tool
    if s.tools.seen != Some(s.workspace) {
        if let Some(old) = s.tools.seen {
            s.tools.remembered[old.index()] = Some(s.tool);
        }
        s.tool = s.tools.remembered[s.workspace.index()].unwrap_or(default_tool(s.workspace));
        s.tools.seen = Some(s.workspace);
    }
    let mut layout = ToolsLayout::default();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.dark).radius(24.0));

    // ---- the tools ----
    let x = r.x + (r.w - BUTTON) / 2.0;
    let mut y = r.y + 10.0;
    let shown: Vec<Tool> = rail_tools(s.workspace)
        .iter()
        .copied()
        .filter(|t| !s.tools.hidden.contains(t))
        .collect();
    // the tools' keys (not while a text field has the keyboard: typing never switches tools)
    let typing = f.typing();
    for &tool in shown.iter().filter(|_| !typing) {
        let key = tool_info(tool).1;
        if key.len() == 1 && f.take_key(key, false) {
            s.tool = tool;
        }
    }
    for tool in shown {
        let b = Rect::new(x, y, BUTTON, BUTTON);
        y += STEP;
        layout.tools.push((tool, b));
        let (name, key, path) = tool_info(tool);
        let wid = id(&format!("tools/{name}"));
        let res = f.interact(wid, b);
        if res.clicked {
            s.tool = tool;
        }
        if res.context {
            s.tools.menu_tool = Some(tool);
            let p = f.pointer();
            f.open(TOOL_MENU, p.x, p.y);
        }
        let on = s.tool == tool;
        let (bg, fg) = if on {
            (t.coral, t.on_coral)
        } else if res.hovered || res.held {
            (RAISED_HOVER, t.on_dark)
        } else {
            (t.dark_raised, t.on_dark)
        };
        f.put(Quad::rect(b.x, b.y, b.w, b.h, bg).radius(12.0));
        f.icon(path, b.x + 12.0, b.y + 12.0, 20.0, fg);
        if res.hovered {
            f.tooltip(wid, &format!("{name} ({key})"), b);
        }
    }

    // ---- the swatch, at the foot ----
    let sw = Rect::new(
        r.x + (r.w - SWATCH) / 2.0,
        r.bottom() - 10.0 - SWATCH,
        SWATCH,
        SWATCH,
    );
    layout.swatch = sw;
    // a right-click on the column's empty part brings hidden tools back
    if !s.tools.hidden.is_empty() {
        f.context_area(RAIL_MENU, Rect::new(r.x, y, r.w, (sw.y - y).max(0.0)));
    }
    let res = f.interact(id("tools/swatch"), sw);
    if res.hovered && f.pointer().pressed {
        s.tools.open_at_press = f.is_open(POPUP);
    }
    if res.clicked && !s.tools.open_at_press {
        f.open(POPUP, 0.0, 0.0);
    }
    let open = f.is_open(POPUP);
    if open || res.hovered {
        f.put(
            Quad::rect(sw.x - 3.0, sw.y - 3.0, sw.w + 6.0, sw.h + 6.0, [0.0; 4])
                .radius(SWATCH)
                .border(2.0, t.on_dark),
        );
    }
    f.put(
        Quad::rect(sw.x, sw.y, sw.w, sw.h, s.colour)
            .radius(SWATCH / 2.0)
            .border(3.0, SWATCH_RING),
    );
    if res.hovered {
        f.tooltip(
            id("tools/swatch"),
            &format!("Colour: {}", s.colour_name),
            sw,
        );
    }

    // ---- the colour pop-up ----
    if open && !s.tools.was_open {
        s.tools.opened_with = s.colour;
    }
    if !open && s.tools.was_open && s.tools.opened_with != s.colour {
        remember(s);
    }
    s.tools.was_open = open;
    if open {
        layout.popup = Some(popup(f, r, sw, s));
    }

    // ---- the menus ----
    tool_menu(f, s);
    if f.menu(RAIL_MENU, |m| {
        m.item("Show every tool", "");
    })
    .is_some()
    {
        s.tools.hidden.clear();
    }
    s.tools.layout = layout;
}

/// The current colour goes first in the recent colours.
fn remember(s: &mut State) {
    let c = s.colour;
    s.tools.recent.retain(|(_, r)| *r != c);
    s.tools.recent.insert(0, (s.colour_name.clone(), c));
    s.tools.recent.truncate(8);
}

fn tool_menu(f: &mut Frame, s: &mut State) {
    let Some(tool) = s.tools.menu_tool else {
        return;
    };
    let (name, key, _) = tool_info(tool);
    let pressure = matches!(tool, Tool::Brush | Tool::Eraser);
    let size = s.tools.pressure_size.contains(&tool);
    let opacity = s.tools.pressure_opacity.contains(&tool);
    let picked = f.menu(TOOL_MENU, |m| {
        m.head(name);
        m.item("Tool settings…", "");
        m.item("Reset this tool", "");
        m.separator();
        if pressure {
            m.item("Pressure changes size", if size { "✓" } else { "" });
            m.item("Pressure changes opacity", if opacity { "✓" } else { "" });
            m.separator();
        }
        m.item("Set shortcut…", key);
        m.item("Hide from the rail", "");
    });
    let flip = |v: &mut Vec<Tool>| {
        if let Some(i) = v.iter().position(|t| *t == tool) {
            v.remove(i);
        } else {
            v.push(tool);
        }
    };
    match picked.as_deref() {
        Some("Pressure changes size") => flip(&mut s.tools.pressure_size),
        Some("Pressure changes opacity") => flip(&mut s.tools.pressure_opacity),
        Some("Hide from the rail") => s.tools.hidden.push(tool),
        Some(item) => s.actions.push(Action::Other(format!("{name}: {item}"))),
        None => {}
    }
}

/// Palette chips per row in the pop-up.
const PER_ROW: usize = 8;
const CHIP: f32 = 28.0;

fn popup_height(palette: usize) -> f32 {
    let rows = palette.div_ceil(PER_ROW).max(1) as f32;
    // padding, the square, Hue and its slider, the hex row, Recent and its row, the
    // palette's label and rows, padding (12 apart)
    16.0 + 170.0
        + 12.0
        + 16.0
        + 4.0
        + 20.0
        + 12.0
        + 44.0
        + 12.0
        + 16.0
        + 8.0
        + CHIP
        + 12.0
        + 16.0
        + 8.0
        + rows * CHIP
        + (rows - 1.0) * 8.0
        + 16.0
}

fn popup(f: &mut Frame, rail: Rect, sw: Rect, s: &mut State) -> PopupLayout {
    let t = f.theme();
    // the colour's hue, saturation and value (kept while the colour is what they make)
    if s.tools.hsv_for != Some(s.colour) {
        let [h, sa, v] = rgb_to_hsv(s.colour);
        // a grey keeps the hue it had
        let h = if sa < 1e-4 || v < 1e-4 {
            s.tools.hsv[0]
        } else {
            h
        };
        s.tools.hsv = [h, sa, v];
        s.tools.hsv_for = Some(s.colour);
    }
    if s.tools.hex_for != Some(s.colour) {
        s.tools.hex = to_hex(s.colour);
        s.tools.hex_for = Some(s.colour);
    }
    let h = popup_height(s.palette.len());
    // beside the column, its foot level with the swatch's
    let x = rail.right() + 12.0;
    let y = (sw.bottom() - h).max(16.0);
    let mut l = PopupLayout {
        rect: Rect::new(x, y, POPUP_W, h),
        ..Default::default()
    };
    let anchor = Rect::new(x, y - 8.0, 0.0, 0.0);
    f.popup(POPUP, anchor, POPUP_W, h, |f| {
        // the design's pop-up: 22 px corners and a hairline (over the toolkit's card)
        f.put(
            Quad::rect(x, y, POPUP_W, h, t.card)
                .radius(22.0)
                .border(1.0, POPUP_LINE),
        );
        let ix = x + 16.0;
        let iw = POPUP_W - 32.0;
        let mut cy = y + 16.0;

        // saturation (across) and value (down)
        l.sv = Rect::new(ix, cy, iw, 170.0);
        let res = f.interact(id("tools/colours/sv"), l.sv);
        if res.held {
            let p = f.pointer();
            let sa = ((p.x - l.sv.x) / l.sv.w).clamp(0.0, 1.0);
            let v = 1.0 - ((p.y - l.sv.y) / l.sv.h).clamp(0.0, 1.0);
            set_hsv(s, [s.tools.hsv[0], sa, v]);
        }
        sv_square(f, l.sv, s.tools.hsv[0]);
        let [_, sa, v] = s.tools.hsv;
        let (mx, my) = (l.sv.x + sa * l.sv.w, l.sv.y + (1.0 - v) * l.sv.h);
        f.put(
            Quad::rect(mx - 9.0, my - 9.0, 18.0, 18.0, [0.0; 4])
                .radius(9.0)
                .border(1.0, t.ink),
        );
        f.put(
            Quad::rect(mx - 8.0, my - 8.0, 16.0, 16.0, [0.0; 4])
                .radius(8.0)
                .border(3.0, t.card),
        );
        cy += 170.0 + 12.0;

        // hue
        small_label(f, "Hue", ix, cy, LABEL);
        cy += 16.0 + 4.0;
        l.hue = Rect::new(ix, cy, iw, 20.0);
        let res = f.interact(id("tools/colours/hue"), l.hue);
        if res.held {
            let u = ((f.pointer().x - l.hue.x) / l.hue.w).clamp(0.0, 1.0);
            set_hsv(s, [u * 360.0, s.tools.hsv[1], s.tools.hsv[2]]);
        }
        hue_bar(f, Rect::new(ix, cy + 4.0, iw, 12.0));
        let kx = ix + s.tools.hsv[0] / 360.0 * iw;
        let k = if res.held { 20.0 } else { 18.0 };
        f.put(
            Quad::rect(
                kx - k / 2.0,
                cy + 10.0 - k / 2.0,
                k,
                k,
                hsv_to_rgb(s.tools.hsv[0], 1.0, 1.0),
            )
            .radius(k / 2.0)
            .border(3.0, t.card),
        );
        f.put(
            Quad::rect(
                kx - k / 2.0 - 1.0,
                cy + 9.0 - k / 2.0,
                k + 2.0,
                k + 2.0,
                [0.0; 4],
            )
            .radius(k / 2.0 + 1.0)
            .border(1.0, alpha(t.ink, 0.35)),
        );
        cy += 20.0 + 12.0;

        // the colour, its hex code, the eyedropper
        f.put(
            Quad::rect(ix, cy, 44.0, 44.0, s.colour)
                .radius(12.0)
                .border(1.0, alpha(t.ink, 0.12)),
        );
        l.hex = Rect::new(ix + 52.0, cy, iw - 104.0, 44.0);
        let mut text = std::mem::take(&mut s.tools.hex);
        let mut changed = false;
        f.row_in(l.hex, 0.0, |f| {
            changed = f
                .text_field("tools/colours/hex", &mut text, "Hex", l.hex.w)
                .changed;
        });
        if changed
            && let Some(c) = parse_hex(&text)
            && c != s.colour
        {
            s.colour = c;
            s.colour_name = to_hex(c);
            s.tools.hex_for = Some(c);
        }
        s.tools.hex = text;
        l.eyedropper = Rect::new(ix + iw - 44.0, cy, 44.0, 44.0);
        let e = l.eyedropper;
        let res = f.interact(id("tools/colours/eyedropper"), e);
        let bg = if res.held {
            POPUP_LINE
        } else if res.hovered {
            hex(0xE7E3D9)
        } else {
            QUIET
        };
        f.put(Quad::rect(e.x, e.y, e.w, e.h, bg).radius(12.0));
        f.icon(EYEDROPPER_SMALL, e.x + 13.0, e.y + 13.0, 18.0, t.ink);
        if res.hovered {
            f.tooltip(
                id("tools/colours/eyedropper"),
                "Pick from the canvas (I)",
                e,
            );
        }
        if res.clicked {
            s.tool = Tool::Eyedropper;
            f.close();
        }
        cy += 44.0 + 12.0;

        // recent colours
        small_label(f, "Recent", ix, cy, SUBTLE);
        cy += 16.0 + 8.0;
        let recent = s.tools.recent.clone();
        for (i, (name, c)) in recent.iter().enumerate().take(PER_ROW) {
            let r = Rect::new(ix + i as f32 * (CHIP + 6.0), cy, CHIP, CHIP);
            l.recent.push(r);
            if chip(
                f,
                &format!("tools/colours/recent/{i}"),
                r,
                *c,
                name,
                s.colour == *c,
            ) {
                s.colour = *c;
                s.colour_name = name.clone();
            }
        }
        cy += CHIP + 12.0;

        // the palette
        small_label(f, "Palette", ix, cy, SUBTLE);
        cy += 16.0 + 8.0;
        let palette = s.palette.clone();
        for (i, (name, c)) in palette.iter().enumerate() {
            let (row, col) = (i / PER_ROW, i % PER_ROW);
            let r = Rect::new(
                ix + col as f32 * (CHIP + 6.0),
                cy + row as f32 * (CHIP + 8.0),
                CHIP,
                CHIP,
            );
            l.palette.push(r);
            if chip(
                f,
                &format!("tools/colours/palette/{i}"),
                r,
                *c,
                name,
                s.colour == *c,
            ) {
                s.colour = *c;
                s.colour_name = name.clone();
            }
        }
    });
    l
}

/// A round colour chip; true when clicked. The current colour has the design's yellow ring.
fn chip(f: &mut Frame, name: &str, r: Rect, c: [f32; 4], label: &str, on: bool) -> bool {
    let t = f.theme();
    let res = f.interact(id(name), r);
    if on || res.hovered {
        let ring = if on { SWATCH_RING } else { POPUP_LINE };
        f.put(
            Quad::rect(r.x - 4.0, r.y - 4.0, r.w + 8.0, r.h + 8.0, [0.0; 4])
                .radius(r.w)
                .border(3.0, ring),
        );
    }
    f.put(
        Quad::rect(r.x, r.y, r.w, r.h, c)
            .radius(r.w / 2.0)
            .border(1.0, alpha(t.ink, 0.15)),
    );
    if res.hovered {
        f.tooltip(id(name), label, r);
    }
    res.clicked
}

fn small_label(f: &mut Frame, text: &str, x: f32, y: f32, c: [f32; 4]) {
    let laid = f.lay_out(text, &Style::body(12.0, c).weight(600), None);
    f.put_text(&laid, x, y);
}

fn set_hsv(s: &mut State, hsv: [f32; 3]) {
    let c = hsv_to_rgb(hsv[0], hsv[1], hsv[2]);
    s.tools.hsv = hsv;
    s.tools.hsv_for = Some(c);
    if c != s.colour {
        s.colour = c;
        s.colour_name = to_hex(c);
    }
}

/// The saturation/value square for `hue`, as the design's two gradients over the hue: white
/// from the left, black from the bottom; 14 px corners.
fn sv_square(f: &mut Frame, r: Rect, hue: f32) {
    let pure = hsv_to_rgb(hue, 1.0, 1.0);
    f.put(Quad::gradient(r.x, r.y, r.w, r.h, [1.0; 4], pure, (1.0, 0.0)).radius(14.0));
    f.put(
        Quad::gradient(
            r.x,
            r.y,
            r.w,
            r.h,
            [0.0; 4],
            [0.0, 0.0, 0.0, 1.0],
            (0.0, 1.0),
        )
        .radius(14.0),
    );
}

/// The hue slider's rainbow (six gradients, red round to red), with round ends.
fn hue_bar(f: &mut Frame, r: Rect) {
    let t = f.theme();
    let seg = r.w / 6.0;
    for k in 0..6 {
        let (a, b) = (
            hsv_to_rgb(k as f32 * 60.0, 1.0, 1.0),
            hsv_to_rgb((k + 1) as f32 * 60.0, 1.0, 1.0),
        );
        // (a hair of overlap, so no seam shows between them)
        f.put(Quad::gradient(
            r.x + k as f32 * seg,
            r.y,
            seg + 0.5,
            r.h,
            a,
            b,
            (1.0, 0.0),
        ));
    }
    round_corners(f, r, r.h / 2.0, t.card);
}

/// Rounds a square-cornered picture's corners: a ring in the background colour (`bg`)
/// around `r`, its inner edge `r` with corners of `radius`.
fn round_corners(f: &mut Frame, r: Rect, radius: f32, bg: [f32; 4]) {
    f.put(
        Quad::rect(
            r.x - radius,
            r.y - radius,
            r.w + 2.0 * radius,
            r.h + 2.0 * radius,
            [0.0; 4],
        )
        .radius(2.0 * radius)
        .border(radius + 0.5, bg),
    );
}

// ---- colours ----

/// Hue (0–360), saturation and value (0–1) of a straight RGBA colour.
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

/// An opaque colour from hue (0–360), saturation and value (0–1).
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 4] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
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
pub fn to_hex(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

/// A colour from "#RRGGBB", "RRGGBB" or "#RGB".
pub fn parse_hex(s: &str) -> Option<[f32; 4]> {
    let s = s.trim().trim_start_matches('#');
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = match s.len() {
        6 => u32::from_str_radix(s, 16).ok()?,
        3 => {
            let v = u32::from_str_radix(s, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            (r * 17) << 16 | (g * 17) << 8 | (b * 17)
        }
        _ => return None,
    };
    Some(hex(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::header::harness::Harness;

    fn rect_of(h: &Harness, tool: Tool) -> Rect {
        h.s.tools
            .layout
            .tools
            .iter()
            .find(|(t, _)| *t == tool)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{tool:?} isn't in the column"))
    }

    #[test]
    fn each_workspace_shows_its_tools() {
        let Some(mut h) = Harness::new() else { return };
        for w in [Workspace::Animate, Workspace::Draw, Workspace::Colour] {
            h.s.workspace = w;
            h.pass();
            let shown: Vec<Tool> = h.s.tools.layout.tools.iter().map(|(t, _)| *t).collect();
            assert_eq!(shown, rail_tools(w), "{w:?}");
            for (_, r) in &h.s.tools.layout.tools {
                assert_eq!((r.w, r.h), (44.0, 44.0));
            }
            let sw = h.s.tools.layout.swatch;
            assert_eq!((sw.w, sw.h), (40.0, 40.0));
        }
        assert!(h.idle(), "idle: nothing drawn");
    }

    #[test]
    fn tool_keys_pick_tools() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        h.key("e", Some("e"));
        h.pass();
        assert_eq!(h.s.tool, Tool::Eraser, "E picks the eraser");
        h.key("i", Some("i"));
        h.pass();
        assert_eq!(h.s.tool, Tool::Eyedropper, "I picks the eyedropper");
        h.key("b", Some("b"));
        h.pass();
        assert_eq!(h.s.tool, Tool::Brush, "B picks the brush");
        // a tool the workspace doesn't show isn't picked by its key
        h.s.workspace = Workspace::Colour;
        h.pass();
        h.key("v", Some("v"));
        h.pass();
        assert_ne!(h.s.tool, Tool::Transform, "Colour has no Transform");
        assert!(h.idle());
    }

    #[test]
    fn tools_pick_and_each_workspace_keeps_its_own() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        assert_eq!(h.s.tool, Tool::Brush);
        let r = rect_of(&h, Tool::Eraser);
        h.click_in(r);
        assert_eq!(h.s.tool, Tool::Eraser, "a click picks the tool");
        h.s.workspace = Workspace::Colour;
        h.pass();
        assert_eq!(h.s.tool, Tool::Fill, "Colour starts with Fill");
        let r = rect_of(&h, Tool::Lasso);
        h.click_in(r);
        assert_eq!(h.s.tool, Tool::Lasso);
        h.s.workspace = Workspace::Animate;
        h.pass();
        assert_eq!(h.s.tool, Tool::Eraser, "Animate kept its tool");
        h.s.workspace = Workspace::Colour;
        h.pass();
        assert_eq!(h.s.tool, Tool::Lasso, "and Colour its own");
        assert!(h.idle());
    }

    #[test]
    fn a_right_click_on_a_tool_opens_its_menu() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        let b = rect_of(&h, Tool::Brush);
        let (px, py) = (b.x + 22.0, b.y + 22.0);
        h.right_click(px, py);
        assert!(h.ui.overlay_open(), "the right-click opened the menu");
        assert_eq!(h.s.tool, Tool::Brush);
        h.save("tools-menu");
        // padding 6, the head 26, two items, a separator, then the pressure items
        let opacity_y = py + 6.0 + 26.0 + 64.0 + 9.0 + 32.0 + 16.0;
        h.click(px + 60.0, opacity_y);
        assert!(h.s.tools.pressure_opacity.contains(&Tool::Brush));
        assert!(!h.ui.overlay_open());

        h.right_click(px, py);
        let settings_y = py + 6.0 + 26.0 + 16.0;
        h.click(px + 60.0, settings_y);
        assert_eq!(
            h.s.actions,
            vec![Action::Other("Brush: Tool settings…".into())]
        );

        // Hide from the rail, then the column's empty part brings it back
        h.right_click(px, py);
        let hide_y = py + 6.0 + 26.0 + 64.0 + 9.0 + 64.0 + 9.0 + 32.0 + 16.0;
        h.click(px + 60.0, hide_y);
        assert!(h.s.tools.hidden.contains(&Tool::Brush));
        h.pass();
        assert!(
            h.s.tools
                .layout
                .tools
                .iter()
                .all(|(t, _)| *t != Tool::Brush)
        );
        let sw = h.s.tools.layout.swatch;
        h.right_click(sw.x + 20.0, sw.y - 30.0);
        assert!(h.ui.overlay_open());
        h.click(sw.x + 80.0, sw.y - 30.0 + 6.0 + 16.0);
        assert!(h.s.tools.hidden.is_empty(), "Show every tool");
        assert!(h.idle());
    }

    #[test]
    fn the_swatch_opens_the_colour_pop_up_and_picking_sets_the_colour() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        let sw = h.s.tools.layout.swatch;
        h.click_in(sw);
        assert!(h.ui.overlay_open(), "the swatch opened the pop-up");
        h.pass();
        let p = h.s.tools.layout.popup.clone().expect("the pop-up's layout");
        assert_eq!(p.rect.w, POPUP_W);
        assert!((p.rect.bottom() - sw.bottom()).abs() < 0.01);
        assert_eq!(p.palette.len(), h.s.palette.len());
        assert_eq!(h.s.tools.hex, "#16161A");
        h.save("tools-colours");

        // a palette colour
        h.click_in(p.palette[1]);
        assert_eq!(h.s.colour, hex(0xE8492F));
        assert_eq!(h.s.colour_name, "Coral");
        assert!(h.ui.overlay_open(), "picking keeps the pop-up open");
        h.pass();
        assert_eq!(h.s.tools.hex, "#E8492F", "the hex field follows");

        // the square: saturation across, value down
        let (sx, sy) = (p.sv.x + p.sv.w * 0.5, p.sv.y + p.sv.h * 0.25);
        h.drag((p.sv.x + 5.0, p.sv.y + 5.0), (sx, sy));
        let [hue, sa, v] = rgb_to_hsv(h.s.colour);
        assert!(
            (sa - 0.5).abs() < 0.02 && (v - 0.75).abs() < 0.02,
            "{sa} {v}"
        );
        assert!((hue - h.s.tools.hsv[0]).abs() < 1.0);

        // the hue
        h.drag(
            (p.hue.x + 2.0, p.hue.y + 10.0),
            (p.hue.x + p.hue.w / 3.0, p.hue.y + 10.0),
        );
        let [hue, ..] = rgb_to_hsv(h.s.colour);
        assert!((hue - 120.0).abs() < 2.0, "green: {hue}");

        // hex
        h.click_in(p.hex);
        for _ in 0..7 {
            h.key("BackSpace", None);
        }
        for c in ["3", "D", "6", "B", "F", "F"] {
            h.key(c, Some(c));
        }
        h.pass();
        assert_eq!(h.s.colour, hex(0x3D6BFF), "typed: {}", h.s.tools.hex);

        // the swatch again closes it; the colour goes first in the recent ones
        h.click_in(sw);
        assert!(!h.ui.overlay_open(), "the swatch closed it");
        h.pass();
        assert_eq!(h.s.tools.recent[0].1, hex(0x3D6BFF));

        // a recent colour; then the eyedropper picks its tool and closes the pop-up
        h.click_in(sw);
        h.pass();
        let p = h.s.tools.layout.popup.clone().unwrap();
        h.click_in(p.recent[1]);
        assert_eq!(
            h.s.colour,
            hex(0xC5432C),
            "Rust, the first of the old recent ones"
        );
        h.click_in(p.eyedropper);
        assert_eq!(h.s.tool, Tool::Eyedropper);
        assert!(!h.ui.overlay_open());
        assert!(h.idle());
    }

    #[test]
    fn colours_round_trip() {
        for v in [0x16161A, 0xE8492F, 0x3D6BFF, 0xFFFFFF, 0x000000, 0x5FD08A] {
            let c = hex(v);
            let [h, s, val] = rgb_to_hsv(c);
            assert_eq!(to_hex(hsv_to_rgb(h, s, val)), to_hex(c));
            assert_eq!(parse_hex(&to_hex(c)), Some(c));
        }
        assert_eq!(parse_hex("#fff"), Some(hex(0xFFFFFF)));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(parse_hex("#GG0000"), None);
    }
}
