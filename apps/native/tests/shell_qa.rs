//! QA for the shell (Phase 2 step 3): the shell built through the library with simulated
//! input, checked against the design (the FullApp artboards at 1440 × 900) and the budgets.
//!
//! These need a GPU (Vulkan), so they are ignored by a plain `cargo test`; run them with
//! `tools/native-qa/run.sh` or
//! `cargo test -p pencilpilot-native --test shell_qa -- --include-ignored --test-threads 1`.
//!
//! The tests don't know where each piece puts its widgets: they find them by clicking across
//! a region (a sweep) and watching what each click changes in the [`State`], which actions it
//! asks for, and whether a menu or pop-up opened. `PP_QA_LOG` = a file to write every
//! sweep's findings to.

#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, BTreeSet};

use pencilpilot_gpu::{DmabufImage, Error, Gpu, Quad, Renderer2d};
use pencilpilot_native::shell::{self, layout};
use pencilpilot_native::state::{State, Tool, Workspace};
use pencilpilot_ui::ui::{Key, Rect, Ui};

const W: f32 = 1440.0;
const H: f32 = 900.0;

/// The design's regions at 1440 × 900 (FullApp*.dc.html, as headless Chromium lays them out):
/// tools, canvas card, side panel, bottom strip, as (x, y, w, h).
type Boxes = (Option<[f32; 4]>, [f32; 4], [f32; 4], [f32; 4]);
fn design_regions(w: Workspace) -> Boxes {
    match w {
        Workspace::Animate => (
            Some([16.0, 84.0, 64.0, 509.0]),
            [92.0, 84.0, 1007.5, 509.0],
            [1111.5, 84.0, 312.5, 509.0],
            [16.0, 605.0, 1408.0, 279.0],
        ),
        Workspace::Draw => (
            Some([16.0, 84.0, 64.0, 736.0]),
            [92.0, 84.0, 1007.5, 736.0],
            [1111.5, 84.0, 312.5, 736.0],
            [16.0, 832.0, 1408.0, 52.0],
        ),
        Workspace::Colour => (
            Some([16.0, 84.0, 64.0, 651.0]),
            [92.0, 84.0, 1007.5, 651.0],
            [1111.5, 84.0, 312.5, 651.0],
            [16.0, 747.0, 1408.0, 137.0],
        ),
        Workspace::Review => (
            None,
            [16.0, 84.0, 1083.5, 694.0],
            [1111.5, 84.0, 312.5, 694.0],
            [16.0, 790.0, 1408.0, 94.0],
        ),
    }
}
/// The design's header: 56 tall (its 44 px buttons centred), from y 16.
const DESIGN_HEADER: [f32; 4] = [16.0, 16.0, 1408.0, 56.0];
/// The design's workspace tabs, left to right, and each one's middle (x) at y 44.
const DESIGN_TABS: [(Workspace, f32); 4] = [
    (Workspace::Draw, 591.0),
    (Workspace::Animate, 673.5),
    (Workspace::Colour, 761.5),
    (Workspace::Review, 845.5),
];
/// Position and size tolerance against the design, logical pixels.
const TOL: f32 = 2.0;

/// The tool column's tools in the design, top to bottom (the colour swatch at the foot aside).
fn design_tools(w: Workspace) -> Vec<Tool> {
    use Tool::*;
    match w {
        Workspace::Animate => vec![Brush, Eraser, Fill, Lasso, Select, Transform, Eyedropper],
        Workspace::Draw => vec![
            Brush, Eraser, Fill, Lasso, Select, Transform, Eyedropper, Text,
        ],
        Workspace::Colour => vec![Fill, Lasso, Eyedropper, Brush, Eraser],
        Workspace::Review => vec![],
    }
}

struct Harness {
    r: Renderer2d,
    ui: Ui,
    s: State,
    log: Vec<String>,
    gpu: Gpu,
}

impl Harness {
    fn new() -> Option<Harness> {
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
        let mut h = Harness {
            r,
            ui,
            s: State::demo(),
            log: Vec::new(),
            gpu,
        };
        h.pass();
        Some(h)
    }

    fn at(&mut self, w: Workspace) {
        self.s = State::demo();
        self.s.workspace = w;
        self.escape();
        self.ui.pointer_moved(2.0, 2.0);
        self.pass();
        self.pass();
    }

    /// One pass; whether it drew anything.
    fn pass(&mut self) -> bool {
        let s = &mut self.s;
        self.ui
            .pass(&mut self.r, W, H, |f| {
                shell::build(f, s, W, H);
            })
            .is_some()
    }

    fn escape(&mut self) {
        self.ui.key(Key {
            name: "Escape".into(),
            text: None,
            ctrl: false,
            shift: false,
        });
        self.pass();
    }

    fn key(&mut self, name: &str, text: &str) -> bool {
        self.ui.key(Key {
            name: name.into(),
            text: Some(text.into()),
            ctrl: false,
            shift: false,
        });
        self.pass()
    }

    /// A left click at (x, y): returns whether the pass the button came up in drew.
    fn click(&mut self, x: f32, y: f32) -> bool {
        self.ui.pointer_moved(x, y);
        self.pass();
        self.ui.pointer_button(true);
        self.pass();
        self.ui.pointer_button(false);
        self.pass()
    }

    fn right_click(&mut self, x: f32, y: f32) -> bool {
        self.ui.pointer_moved(x, y);
        self.pass();
        self.ui.pointer_context();
        self.pass()
    }

    /// What a click (or right-click) at (x, y) does, from a clean start; then everything is
    /// put back. `stale`: the pass after the click, with no input, still drew something (so
    /// the frame the click produced wasn't its final look).
    fn probe(&mut self, x: f32, y: f32, right: bool) -> Probe {
        let before = self.s.clone();
        if right {
            self.right_click(x, y);
        } else {
            self.click(x, y);
        }
        let mut effects = diff(&before, &self.s);
        if self.ui.overlay_open() {
            effects.insert("overlay".into());
        }
        let stale = self.pass();
        // put back
        self.s = before;
        self.escape();
        self.ui.pointer_moved(2.0, 2.0);
        self.pass();
        self.pass();
        Probe {
            x,
            y,
            effects,
            stale,
        }
    }

    /// Click across `rect` every `step` px; the clicks that did something.
    fn sweep(&mut self, rect: Rect, step: f32, right: bool) -> Vec<Probe> {
        let mut out = Vec::new();
        let mut y = rect.y + step / 2.0;
        while y < rect.bottom() {
            let mut x = rect.x + step / 2.0;
            while x < rect.right() {
                let p = self.probe(x, y, right);
                if !p.effects.is_empty() {
                    out.push(p);
                }
                x += step;
            }
            y += step;
        }
        for p in &out {
            self.log.push(format!(
                "{} {:?} ({:.0}, {:.0}){}: {:?}",
                self.s.workspace.name(),
                if right { "right" } else { "left" },
                p.x,
                p.y,
                if p.stale { " STALE" } else { "" },
                p.effects
            ));
        }
        out
    }

    /// The pass's quads drawn (no canvas, the interface only) and saved as PNG.
    fn save(&mut self, quads: &[Quad], path: &str) {
        let (w, h) = (W as u32, H as u32);
        let frame = DmabufImage::new(&self.gpu, w, h, &[]).unwrap();
        self.r
            .draw(&self.gpu, &frame, self.ui.theme.paper, quads)
            .unwrap();
        let px = self.gpu.read_back(&frame).unwrap();
        let rgba: Vec<u8> = px.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }

    fn regions(&self) -> layout::Regions {
        layout::regions(W, H, self.s.workspace)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Ok(path) = std::env::var("PP_QA_LOG") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                for l in &self.log {
                    let _ = writeln!(f, "{l}");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Probe {
    x: f32,
    y: f32,
    effects: BTreeSet<String>,
    stale: bool,
}

/// What changed between two states, as "field=new value" (and "action:…" for each action).
fn diff(a: &State, b: &State) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    macro_rules! field {
        ($($f:ident),*) => {$(
            let (x, y) = (format!("{:?}", a.$f), format!("{:?}", b.$f));
            if x != y {
                let mut v = y;
                v.truncate(60);
                out.insert(format!("{}={}", stringify!($f), v));
            }
        )*};
    }
    field!(
        workspace,
        tool,
        colour,
        colour_name,
        palette,
        brush,
        brush_size,
        panel_tab,
        layers,
        active_layer,
        frame,
        frames,
        fps,
        playing,
        onion,
        frame_width,
        drawings,
        drawing,
        to_colour,
        notes,
        can_undo,
        can_redo,
        header,
        tools,
        panel,
        strips
    );
    for act in &b.actions {
        out.insert(format!("action:{act:?}"));
    }
    out
}

fn has(probes: &[Probe], pred: impl Fn(&str) -> bool) -> bool {
    probes.iter().any(|p| p.effects.iter().any(|e| pred(e)))
}

fn summary(probes: &[Probe]) -> String {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for p in probes {
        for e in &p.effects {
            *seen.entry(e.clone()).or_default() += 1;
        }
    }
    format!("{seen:#?}")
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOL
}

fn rect_problems(name: &str, got: Rect, want: [f32; 4]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, g, w) in [
        ("x", got.x, want[0]),
        ("y", got.y, want[1]),
        ("w", got.w, want[2]),
        ("h", got.h, want[3]),
    ] {
        if !near(g, w) {
            out.push(format!("{name}.{k} = {g} (design {w}, {:+})", g - w));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// layout against the design (no GPU needed, but kept with the rest of the QA suite)

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn layout_matches_the_design_regions() {
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        let r = layout::regions(W, H, w);
        let (tools, canvas, panel, strip) = design_regions(w);
        let n = w.name();
        let header = Rect::new(r.header.x, r.header.y, r.header.w, r.header.h);
        problems.extend(rect_problems(&format!("{n} header"), header, DESIGN_HEADER));
        match (r.tools, tools) {
            (Some(t), Some(d)) => problems.extend(rect_problems(&format!("{n} tools"), t, d)),
            (None, None) => {}
            (got, want) => problems.push(format!("{n} tools: {got:?} (design {want:?})")),
        }
        problems.extend(rect_problems(&format!("{n} canvas"), r.canvas, canvas));
        problems.extend(rect_problems(&format!("{n} panel"), r.panel, panel));
        problems.extend(rect_problems(&format!("{n} strip"), r.strip, strip));
    }
    assert!(
        problems.is_empty(),
        "{} differences:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------------------
// idle

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn idle_draws_nothing_in_every_workspace() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        h.s = State::demo();
        h.s.workspace = w;
        h.ui.pointer_moved(2.0, 2.0);
        h.pass();
        for i in 0..3 {
            if h.pass() {
                problems.push(format!("{}: idle pass {} drew", w.name(), i + 1));
            }
        }
        // resting over each region's middle (a hover may draw once, then nothing)
        let r = h.regions();
        let mut spots = vec![
            ("canvas", r.canvas),
            ("panel", r.panel),
            ("strip", r.strip),
            ("header", r.header),
        ];
        if let Some(t) = r.tools {
            spots.push(("tools", t));
        }
        for (name, rect) in spots {
            h.ui.pointer_moved(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            h.pass();
            h.pass();
            if h.pass() {
                problems.push(format!(
                    "{}: resting over the {name} keeps drawing",
                    w.name()
                ));
            }
        }
        h.ui.pointer_left();
        h.pass();
        if h.pass() {
            problems.push(format!(
                "{}: after the pointer left, still drawing",
                w.name()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A due pass (a tooltip about to show) is the only reason to wake without input; with the
/// pointer away, nothing is due.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn nothing_is_due_while_idle() {
    let Some(mut h) = Harness::new() else { return };
    for w in Workspace::ALL {
        h.at(w);
        h.ui.pointer_left();
        h.pass();
        assert!(
            h.ui.next_pass_at().is_none(),
            "{}: a pass is due with nothing happening",
            w.name()
        );
    }
}

// ---------------------------------------------------------------------------------------
// the header

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn workspace_tabs_switch_every_workspace_in_the_designs_order() {
    let Some(mut h) = Harness::new() else { return };
    h.at(Workspace::Animate);
    let hd = h.regions().header;
    let row = Rect::new(hd.x, hd.y + hd.h / 2.0 - 2.0, hd.w, 4.0);
    let probes = h.sweep(row, 4.0, false);
    // where each workspace's tab is (the middle of the clicks that picked it)
    let mut xs: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
    for p in &probes {
        for w in Workspace::ALL {
            if p.effects.contains(&format!("workspace={w:?}")) {
                xs.entry(w.name()).or_default().push(p.x);
            }
        }
    }
    // Animate is already picked: start from Draw to find Animate's tab
    h.at(Workspace::Draw);
    for p in h.sweep(row, 4.0, false) {
        if p.effects.contains("workspace=Animate") {
            xs.entry("Animate").or_default().push(p.x);
        }
    }
    let mut problems = Vec::new();
    let mut order: Vec<(f32, &str)> = Vec::new();
    for (w, design_x) in DESIGN_TABS {
        match xs.get(w.name()) {
            None => problems.push(format!("no click in the header picks {}", w.name())),
            Some(v) => {
                let mid = (v[0] + v[v.len() - 1]) / 2.0;
                order.push((mid, w.name()));
                if (mid - design_x).abs() > 8.0 {
                    problems.push(format!(
                        "{} tab's middle at x {mid:.0} (design {design_x}, {:+.0})",
                        w.name(),
                        mid - design_x
                    ));
                }
            }
        }
    }
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let got: Vec<&str> = order.iter().map(|o| o.1).collect();
    let want: Vec<&str> = DESIGN_TABS.iter().map(|t| t.0.name()).collect();
    if got != want {
        problems.push(format!("tab order {got:?} (design {want:?})"));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The budget: a click shows on the next display frame. The pass the click lands in must
/// draw its result, and the pass after (no input) must have nothing left to draw: otherwise
/// the window, which only draws again on input, keeps showing a half-updated frame.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn a_click_shows_complete_on_the_next_frame() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        h.at(w);
        let r = h.regions();
        let mut areas = vec![
            (
                "header",
                Rect::new(r.header.x, r.header.y, r.header.w, r.header.h),
            ),
            ("panel", r.panel),
            ("strip", r.strip),
        ];
        if let Some(t) = r.tools {
            areas.push(("tools", t));
        }
        for (name, rect) in areas {
            for p in h.sweep(rect, 12.0, false) {
                if p.stale {
                    problems.push(format!(
                        "{} {name}: click at ({:.0}, {:.0}) → {:?}: the next idle pass still drew",
                        w.name(),
                        p.x,
                        p.y,
                        p.effects
                    ));
                }
            }
        }
    }
    problems.dedup_by(|a, b| a.split(" → ").nth(1) == b.split(" → ").nth(1));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Pictures for the report (`PP_SAVE` = a folder): the frame a workspace tab click produces,
/// and the frame the next pass (no input) produces.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn save_the_frames_around_a_workspace_click() {
    let Ok(dir) = std::env::var("PP_SAVE") else {
        return;
    };
    let Some(mut h) = Harness::new() else { return };
    // find the Draw tab from Animate
    h.at(Workspace::Animate);
    let hd = h.regions().header;
    let row = Rect::new(hd.x, hd.y + hd.h / 2.0 - 2.0, hd.w, 4.0);
    let Some(p) = h
        .sweep(row, 4.0, false)
        .into_iter()
        .find(|p| p.effects.contains("workspace=Draw"))
    else {
        return;
    };
    h.at(Workspace::Animate);
    h.ui.pointer_moved(p.x, p.y);
    h.pass();
    h.ui.pointer_button(true);
    h.pass();
    h.ui.pointer_button(false);
    let s = &mut h.s;
    let click =
        h.ui.pass(&mut h.r, W, H, |f| {
            shell::build(f, s, W, H);
        })
        .unwrap_or_else(|| h.ui.shown().to_vec());
    let s = &mut h.s;
    let next =
        h.ui.pass(&mut h.r, W, H, |f| {
            shell::build(f, s, W, H);
        })
        .unwrap_or_else(|| h.ui.shown().to_vec());
    h.save(&click, &format!("{dir}/click-frame-animate-to-draw.png"));
    h.save(&next, &format!("{dir}/next-frame-animate-to-draw.png"));
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn header_menus_open_and_their_items_ask_for_actions() {
    let Some(mut h) = Harness::new() else { return };
    h.at(Workspace::Animate);
    let hd = h.regions().header;
    let row = Rect::new(hd.x, hd.y + hd.h / 2.0 - 2.0, 600.0, 4.0);
    let openers: Vec<Probe> = h
        .sweep(row, 4.0, false)
        .into_iter()
        .filter(|p| p.effects.contains("overlay"))
        .collect();
    assert!(
        !openers.is_empty(),
        "no click on the header's left half (where the design has File and View) opens a menu"
    );
    // File: the first opener; its items are below it
    let first = openers[0].clone();
    h.click(first.x, first.y);
    assert!(h.ui.overlay_open());
    let mut actions = BTreeSet::new();
    let mut y = first.y + 20.0;
    while y < first.y + 420.0 {
        let before = h.s.clone();
        h.click(first.x + 40.0, y);
        for a in &h.s.actions {
            actions.insert(format!("{a:?}"));
        }
        h.s = before;
        if !h.ui.overlay_open() {
            h.click(first.x, first.y);
        }
        y += 8.0;
    }
    h.escape();
    assert!(
        actions.iter().any(|a| a.starts_with("File(")),
        "the first header menu (File) asks for no File action; it asked for {actions:?}"
    );
    // Escape and a click outside close it
    h.click(first.x, first.y);
    assert!(h.ui.overlay_open());
    h.escape();
    assert!(!h.ui.overlay_open(), "Escape doesn't close the menu");
    h.click(first.x, first.y);
    h.click(700.0, 400.0);
    assert!(
        !h.ui.overlay_open(),
        "a click outside doesn't close the menu"
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn header_has_undo_redo_draw_together_and_export() {
    let Some(mut h) = Harness::new() else { return };
    h.at(Workspace::Animate);
    h.s.can_redo = true;
    let hd = h.regions().header;
    let right = Rect::new(hd.right() - 420.0, hd.y + hd.h / 2.0 - 2.0, 420.0, 4.0);
    let probes = h.sweep(right, 4.0, false);
    let mut problems = Vec::new();
    if !has(&probes, |e| e == "action:Undo") {
        problems.push("no Undo".to_string());
    }
    // Redo is greyed in the demo (nothing to redo): accept it either way, but it must exist
    let probes_redo = {
        h.s.can_redo = true;
        h.sweep(right, 4.0, false)
    };
    if !has(&probes_redo, |e| e == "action:Redo") {
        problems.push("no Redo (with can_redo set)".to_string());
    }
    let others: BTreeSet<String> = probes
        .iter()
        .flat_map(|p| p.effects.iter().cloned())
        .filter(|e| e != "action:Undo" && e != "action:Redo")
        .collect();
    // the design's two coloured buttons on the right: Draw together and Export
    if others.len() < 2 {
        problems.push(format!(
            "the design has Draw together and Export right of Redo; clicks there do only {others:?}"
        ));
    }
    assert!(
        problems.is_empty(),
        "{}\nall: {}",
        problems.join("\n"),
        summary(&probes)
    );
}

/// Greyed Undo / Redo: when there's nothing to undo, Undo asks for nothing.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn undo_and_redo_follow_can_undo_and_can_redo() {
    let Some(mut h) = Harness::new() else { return };
    h.at(Workspace::Animate);
    h.s.can_undo = false;
    h.s.can_redo = false;
    let hd = h.regions().header;
    let right = Rect::new(hd.right() - 420.0, hd.y + hd.h / 2.0 - 2.0, 420.0, 4.0);
    let probes = h.sweep(right, 4.0, false);
    assert!(
        !has(&probes, |e| e == "action:Undo" || e == "action:Redo"),
        "Undo or Redo asked for with can_undo and can_redo both false: {}",
        summary(&probes)
    );
}

// ---------------------------------------------------------------------------------------
// the tool column

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn tool_column_has_the_designs_tools_in_order_and_picks_them() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        h.at(w);
        let want = design_tools(w);
        let Some(t) = h.regions().tools else {
            if !want.is_empty() {
                problems.push(format!("{}: no tool column", w.name()));
            }
            continue;
        };
        // start from a tool the column doesn't have, so every button's click changes it
        h.s.tool = Tool::Hand;
        let start = h.s.clone();
        let col = Rect::new(t.x + t.w / 2.0 - 2.0, t.y, 4.0, t.h);
        let mut order: Vec<Tool> = Vec::new();
        let mut opens_colours = false;
        let mut y = t.y + 3.0;
        while y < t.bottom() {
            let p = h.probe(col.x + 2.0, y, false);
            h.s = start.clone();
            for e in &p.effects {
                if let Some(v) = e.strip_prefix("tool=") {
                    let tool = [
                        Tool::Brush,
                        Tool::Eraser,
                        Tool::Fill,
                        Tool::Lasso,
                        Tool::Select,
                        Tool::Transform,
                        Tool::Eyedropper,
                        Tool::Text,
                        Tool::Shape,
                        Tool::Hand,
                    ]
                    .into_iter()
                    .find(|t| format!("{t:?}") == v)
                    .unwrap();
                    if order.last() != Some(&tool) {
                        order.push(tool);
                    }
                }
            }
            if p.effects.contains("overlay") && y > t.bottom() - 70.0 {
                opens_colours = true;
            }
            y += 4.0;
        }
        if order != want {
            problems.push(format!(
                "{}: tools top to bottom {order:?} (design {want:?})",
                w.name()
            ));
        }
        if !opens_colours {
            problems.push(format!(
                "{}: the colour swatch at the column's foot opens no colour pop-up",
                w.name()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The design names each tool with its key: Brush (B), Eraser (E), Fill (G), Lasso fill (F),
/// Select (M), Transform (V), Eyedropper (I), Text (T).
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn tool_keys_pick_tools() {
    let Some(mut h) = Harness::new() else { return };
    h.at(Workspace::Draw);
    let mut problems = Vec::new();
    for (k, tool) in [
        ("e", Tool::Eraser),
        ("g", Tool::Fill),
        ("f", Tool::Lasso),
        ("m", Tool::Select),
        ("v", Tool::Transform),
        ("i", Tool::Eyedropper),
        ("t", Tool::Text),
        ("b", Tool::Brush),
    ] {
        h.key(k, k);
        h.pass();
        if h.s.tool != tool {
            problems.push(format!("key {k}: tool {:?} (want {tool:?})", h.s.tool));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------
// the side panel

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn panel_tabs_three_where_the_design_has_them() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        h.at(w);
        let p = h.regions().panel;
        let top = Rect::new(p.x, p.y, p.w, 72.0);
        let mut picked = BTreeSet::new();
        for start in 0..3 {
            h.s.panel_tab[w.index()] = start;
            let probes = h.sweep(top, 6.0, false);
            for pr in &probes {
                for e in &pr.effects {
                    if e.starts_with("panel_tab=") {
                        picked.insert(e.clone());
                    }
                }
            }
        }
        let want = if w == Workspace::Review { 0 } else { 3 };
        if picked.len() != want {
            problems.push(format!(
                "{}: {} panel tab picks found at the panel's top (design: {want}): {picked:?}",
                w.name(),
                picked.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn panel_content_reacts() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    // what a click in the panel's body should be able to change, per workspace (the design)
    let wants: [(Workspace, &[&str]); 4] = [
        (Workspace::Animate, &["colour"]),
        (Workspace::Draw, &["brush", "brush_size"]),
        (Workspace::Colour, &["colour"]),
        (Workspace::Review, &[]),
    ];
    for (w, want) in wants {
        h.at(w);
        let p = h.regions().panel;
        let body = Rect::new(p.x, p.y + 72.0, p.w, p.h - 72.0);
        let probes = h.sweep(body, 12.0, false);
        for f in want {
            if !has(&probes, |e| e.starts_with(&format!("{f}="))) {
                problems.push(format!(
                    "{}: no click in the panel changes `{f}`; clicks did: {}",
                    w.name(),
                    summary(&probes)
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------
// the bottom strips

/// A control's name, and how to tell its click from the effects.
type Check<'a> = (&'a str, &'a dyn Fn(&str) -> bool);

fn strip_probes(h: &mut Harness, w: Workspace, step: f32) -> Vec<Probe> {
    h.at(w);
    let r = h.regions().strip;
    h.sweep(r, step, false)
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn animate_strip_main_controls() {
    let Some(mut h) = Harness::new() else { return };
    let probes = strip_probes(&mut h, Workspace::Animate, 8.0);
    let mut problems = Vec::new();
    let checks: [Check; 7] = [
        ("Play", &|e| e == "action:Play" || e == "playing=true"),
        ("Previous frame", &|e| {
            e == "action:PreviousFrame" || e == "frame=13"
        }),
        ("Next frame", &|e| {
            e == "action:NextFrame" || e == "frame=15"
        }),
        ("Onion skin toggle", &|e| e == "onion=false"),
        ("New layer", &|e| e == "action:NewLayer"),
        ("a frame cell picks its frame", &|e| {
            e.starts_with("action:GoToFrame")
                || (e.starts_with("frame=") && e != "frame=13" && e != "frame=15")
        }),
        ("frame width (zoom)", &|e| e.starts_with("frame_width=")),
    ];
    for (name, pred) in checks {
        if !has(&probes, pred) {
            problems.push(name.to_string());
        }
    }
    assert!(
        problems.is_empty(),
        "Animate timeline: missing {}\nclicks did: {}",
        problems.join(", "),
        summary(&probes)
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn draw_strip_main_controls() {
    let Some(mut h) = Harness::new() else { return };
    let probes = strip_probes(&mut h, Workspace::Draw, 6.0);
    let mut problems = Vec::new();
    let checks: [Check; 5] = [
        ("Previous drawing", &|e| {
            e == "action:PreviousDrawing" || e == "drawing=2"
        }),
        ("Next drawing", &|e| {
            e == "action:NextDrawing" || e == "drawing=4"
        }),
        ("a drawing picks it", &|e| {
            e.starts_with("drawing=") && e != "drawing=2" && e != "drawing=4"
        }),
        ("New drawing", &|e| e == "action:NewDrawing"),
        ("Onion skin toggle", &|e| e == "onion=false"),
    ];
    for (name, pred) in checks {
        if !has(&probes, pred) {
            problems.push(name.to_string());
        }
    }
    assert!(
        problems.is_empty(),
        "Draw frame strip: missing {}\nclicks did: {}",
        problems.join(", "),
        summary(&probes)
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn colour_strip_picks_drawings() {
    let Some(mut h) = Harness::new() else { return };
    let probes = strip_probes(&mut h, Workspace::Colour, 8.0);
    let picks: BTreeSet<String> = probes
        .iter()
        .flat_map(|p| p.effects.iter().cloned())
        .filter(|e| e.starts_with("drawing=") || e.starts_with("action:GoToFrame"))
        .collect();
    // the demo has 8 drawings to colour, drawing 4 (index 3) current: the other 7 pickable
    assert!(
        picks.len() >= 5,
        "Colour strip: clicking drawings picks {} distinct ones: {picks:?}\nclicks did: {}",
        picks.len(),
        summary(&probes)
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn review_strip_main_controls() {
    let Some(mut h) = Harness::new() else { return };
    let probes = strip_probes(&mut h, Workspace::Review, 6.0);
    let mut problems = Vec::new();
    if !has(&probes, |e| e == "action:Play" || e == "playing=true") {
        problems.push("Play");
    }
    // Previous / next note: to a note's frame (the demo's notes are at frames 14 and 22)
    if !has(&probes, |e| {
        e == "frame=22" || e == "action:GoToFrame(22)" || e.contains("Note")
    }) {
        problems.push("Next note");
    }
    // scrubbing: a click on the bar goes to a frame
    if !has(&probes, |e| {
        (e.starts_with("frame=") && e != "frame=22") || e.starts_with("action:GoToFrame")
    }) {
        problems.push("scrub bar");
    }
    let others = probes
        .iter()
        .flat_map(|p| p.effects.iter())
        .filter(|e| e.starts_with("action:Other"))
        .count();
    if others == 0 {
        problems.push("Ask for changes / Approve cut");
    }
    assert!(
        problems.is_empty(),
        "Review player: missing {}\nclicks did: {}",
        problems.join(", "),
        summary(&probes)
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn right_click_menus_where_the_design_has_them() {
    let Some(mut h) = Harness::new() else { return };
    let mut problems = Vec::new();
    for w in Workspace::ALL {
        h.at(w);
        let r = h.regions();
        // the design: right-click a frame, layer or the ruler (Animate); the layer button
        // (Draw); a drawing (Colour); the player (Review); the canvas (everywhere)
        let probes = h.sweep(r.strip, 16.0, true);
        if !has(&probes, |e| e == "overlay") {
            problems.push(format!(
                "{}: no right-click menu anywhere on the strip",
                w.name()
            ));
        }
        let c = r.canvas;
        let p = h.probe(c.x + c.w / 2.0, c.y + c.h / 2.0, true);
        if !p.effects.contains("overlay") {
            problems.push(format!(
                "{}: right-click on the canvas opens nothing",
                w.name()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every workspace builds at small and large window sizes without panicking, and its regions
/// stay inside the window.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn shell_builds_at_other_sizes() {
    let Some(mut h) = Harness::new() else { return };
    for (w, hh) in [
        (800.0, 600.0),
        (1024.0, 640.0),
        (2560.0, 1440.0),
        (480.0, 360.0),
    ] {
        for ws in Workspace::ALL {
            h.s = State::demo();
            h.s.workspace = ws;
            let s = &mut h.s;
            let _ = h.ui.pass(&mut h.r, w, hh, |f| {
                shell::build(f, s, w, hh);
            });
            let r = layout::regions(w, hh, ws);
            for (n, rc) in [("canvas", r.canvas), ("panel", r.panel), ("strip", r.strip)] {
                assert!(
                    rc.x >= 0.0
                        && rc.y >= 0.0
                        && rc.right() <= w + 0.01
                        && rc.bottom() <= hh + 0.01,
                    "{} at {w}x{hh}: {n} {rc:?} outside the window",
                    ws.name()
                );
            }
        }
    }
}
