//! QA for the canvas tools (Phase 2 step 4): each tool's effect on the document's pixels,
//! undo and redo of each, the selection's and the canvas's menu items, and moving a large
//! selection with the Transform tool. Through the engine with document-pixel samples (the
//! window maps the screen through the view; `canvas_qa.rs` checks that map).
//!
//! These paint full documents, so they're ignored by a plain `cargo test`; run them with
//! `tools/native-qa/run.sh` or
//! `cargo test --release -p pencilpilot-native --test canvas_tools_qa -- --include-ignored --test-threads 1`.
//! `PP_QA_OUT` = a folder for the measurements (JSON) and pictures (PNG).

#![cfg(target_os = "linux")]

use std::time::Instant;

use pencilpilot_native::shell::engine::tools::ShapeKind;
use pencilpilot_native::shell::engine::{Engine, Phase, Sample};
use pencilpilot_native::state::{Action, State, Tool};

const W: u32 = 1920;
const H: u32 = 1080;

fn out_dir() -> Option<std::path::PathBuf> {
    let d = std::path::PathBuf::from(std::env::var_os("PP_QA_OUT")?);
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn save_json(name: &str, json: &str) {
    if let Some(d) = out_dir() {
        let _ = std::fs::write(d.join(name), json);
    }
    eprintln!("{name}: {json}");
}

fn save_png(name: &str, e: &mut Engine) {
    let Some(d) = out_dir() else { return };
    e.painter.flush();
    let file = std::io::BufWriter::new(std::fs::File::create(d.join(name)).unwrap());
    let mut enc = png::Encoder::new(file, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(e.painter.rgba())
        .unwrap();
}

struct Doc {
    e: Engine,
    s: State,
    ms: u32,
}

fn smp(x: f32, y: f32, ms: u32) -> Sample {
    Sample {
        x,
        y,
        pressure: 0.9,
        tilt: (0.0, 0.0),
        ms,
    }
}

impl Doc {
    fn new() -> Doc {
        let mut s = State::demo();
        s.brush = 2; // Liner: solid, so the pixels are easy to read
        s.brush_size = 8.0;
        Doc {
            e: Engine::new(W, H),
            s,
            ms: 0,
        }
    }

    fn tool(&mut self, t: Tool) -> &mut Self {
        self.s.tool = t;
        self
    }

    /// A press, moves along the points, a release, with the current tool.
    fn drag(&mut self, pts: &[(f32, f32)]) {
        let n = pts.len();
        for (i, p) in pts.iter().enumerate() {
            self.ms += 4;
            let ph = if i == 0 {
                Phase::Down
            } else if i == n - 1 {
                Phase::Up
            } else {
                Phase::Move
            };
            let sample = smp(p.0, p.1, self.ms);
            self.e.pointer(&mut self.s, ph, sample);
        }
        self.e.painter.flush();
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32)) {
        let n = 60;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
            })
            .collect();
        self.drag(&pts);
    }

    fn click(&mut self, p: (f32, f32)) {
        self.drag(&[p, p]);
    }

    /// A closed box outline with the brush.
    fn outline(&mut self, r: [f32; 4]) {
        let t = self.s.tool;
        self.s.tool = Tool::Brush;
        let [x0, y0, x1, y1] = r;
        self.line((x0, y0), (x1, y0));
        self.line((x1, y0), (x1, y1));
        self.line((x1, y1), (x0, y1));
        self.line((x0, y1), (x0, y0));
        self.s.tool = t;
    }

    fn act(&mut self, name: &str) -> bool {
        let a = match name {
            "Undo" => Action::Undo,
            "Redo" => Action::Redo,
            n => Action::Other(n.into()),
        };
        let r = self.e.apply(&mut self.s, &a);
        self.e.painter.flush();
        r
    }

    fn px(&mut self, x: u32, y: u32) -> [u8; 4] {
        self.e.painter.flush();
        let i = ((y * W + x) * 4) as usize;
        self.e.painter.rgba()[i..i + 4].try_into().unwrap()
    }

    /// Pixels in a rectangle that differ from white paper.
    fn inked(&mut self, r: [u32; 4]) -> usize {
        self.e.painter.flush();
        let rgba = self.e.painter.rgba();
        let mut n = 0;
        for y in r[1]..r[3] {
            for x in r[0]..r[2] {
                let i = ((y * W + x) * 4) as usize;
                if rgba[i] < 200 || rgba[i + 1] < 200 || rgba[i + 2] < 200 {
                    n += 1;
                }
            }
        }
        n
    }

    fn snapshot(&mut self) -> Vec<u8> {
        self.e.painter.flush();
        self.e.painter.rgba().to_vec()
    }
}

fn near_rgb(p: [u8; 4], c: [f32; 4], tol: i32) -> bool {
    (0..3).all(|k| (p[k] as i32 - (c[k] * 255.0).round() as i32).abs() <= tol)
}

const CORAL: [f32; 4] = [
    0xE8 as f32 / 255.0,
    0x49 as f32 / 255.0,
    0x2F as f32 / 255.0,
    1.0,
];

fn diff_count(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| {
            p.iter()
                .zip(q.iter())
                .any(|(x, y)| (*x as i32 - *y as i32).abs() > 2)
        })
        .count()
}

// ---------------------------------------------------------------------------------------
// Fill

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn fill_fills_the_closed_area_and_not_beyond_and_undoes() {
    let mut d = Doc::new();
    d.outline([400.0, 300.0, 800.0, 600.0]);
    let before = d.snapshot();
    d.s.colour = CORAL;
    d.tool(Tool::Fill).click((600.0, 450.0));
    let inside = d.px(600, 450);
    assert!(
        near_rgb(inside, CORAL, 6),
        "the inside is coral: {inside:?}"
    );
    // near the inner edge too (tucked under the line)
    assert!(near_rgb(d.px(412, 312), CORAL, 30), "{:?}", d.px(412, 312));
    // and not outside: the paper around stays white
    for (x, y) in [
        (380, 450),
        (820, 450),
        (600, 280),
        (600, 620),
        (100, 100),
        (1800, 1000),
    ] {
        let p = d.px(x, y);
        assert!(
            p[0] > 240 && p[1] > 240 && p[2] > 240,
            "outside ({x}, {y}) is {p:?}"
        );
    }
    let filled = d.snapshot();
    assert!(d.act("Undo"));
    assert_eq!(
        diff_count(&d.snapshot(), &before),
        0,
        "undo takes the fill away"
    );
    assert!(d.act("Redo"));
    assert_eq!(diff_count(&d.snapshot(), &filled), 0, "redo puts it back");
    save_png("tools-fill.png", &mut d.e);
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn fill_closes_small_gaps_but_not_big_ones() {
    for (gap, closes) in [(3.0f32, true), (40.0, false)] {
        let mut d = Doc::new();
        d.outline([400.0, 300.0, 800.0, 600.0]);
        // a hole in the left edge: erase a band
        d.s.tool = Tool::Eraser;
        d.s.brush_size = gap.max(2.0);
        d.line((370.0, 450.0), (430.0, 450.0));
        d.s.brush_size = 8.0;
        d.s.colour = CORAL;
        d.tool(Tool::Fill).click((600.0, 450.0));
        let out = d.px(200, 450);
        let leaked = near_rgb(out, CORAL, 20);
        assert_eq!(
            !leaked, closes,
            "gap {gap} px: outside is {out:?} (closes gaps of 4 px: {closes})"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Select, Lasso: the selection limits painting and filling

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn a_selection_limits_painting_and_fill() {
    let mut d = Doc::new();
    d.tool(Tool::Select).line((500.0, 400.0), (900.0, 700.0));
    assert_eq!(
        d.s.tooling.selection.map(|b| b.map(|v| v.round())),
        Some([500.0, 400.0, 900.0, 700.0])
    );
    // a stroke across the whole width: ink only inside the selection
    d.tool(Tool::Brush).line((100.0, 550.0), (1800.0, 550.0));
    let inside = d.inked([500, 530, 900, 570]);
    let outside = d.inked([0, 500, 495, 600]) + d.inked([905, 500, 1920, 600]);
    assert!(inside > 2000, "painted inside ({inside} px)");
    assert_eq!(outside, 0, "painted outside the selection ({outside} px)");
    // fill on empty paper inside the selection: the selection, not the whole page
    d.s.colour = CORAL;
    d.tool(Tool::Fill).click((600.0, 450.0));
    assert!(near_rgb(d.px(600, 450), CORAL, 6));
    let p = d.px(300, 300);
    assert!(
        p[0] > 240 && p[1] > 240,
        "fill leaked out of the selection: {p:?}"
    );
    // Escape (no tool busy) deselects; painting is free again
    d.act("Escape");
    assert!(d.s.tooling.selection.is_none());
    d.tool(Tool::Brush).line((100.0, 200.0), (300.0, 200.0));
    assert!(d.inked([100, 190, 300, 210]) > 500);
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn lasso_selects_adds_and_takes_away() {
    let mut d = Doc::new();
    let tri = [
        (400.0, 300.0),
        (800.0, 300.0),
        (600.0, 700.0),
        (400.0, 300.0),
    ];
    d.tool(Tool::Lasso).drag(&tri);
    let b = d.s.tooling.selection.expect("a selection");
    assert!(
        (b[0] - 400.0).abs() < 2.0 && (b[2] - 800.0).abs() < 2.0 && (b[3] - 700.0).abs() < 2.0,
        "{b:?}"
    );
    // take away a square with Alt+Select
    d.s.tooling.alt = true;
    d.tool(Tool::Select).line((550.0, 300.0), (650.0, 400.0));
    d.s.tooling.alt = false;
    // add a square with Shift+Select
    d.s.tooling.shift = true;
    d.tool(Tool::Select).line((1000.0, 300.0), (1100.0, 400.0));
    d.s.tooling.shift = false;
    d.s.colour = CORAL;
    d.act("Fill selection");
    assert!(near_rgb(d.px(600, 450), CORAL, 6), "inside the triangle");
    assert!(
        near_rgb(d.px(1050, 350), CORAL, 6),
        "inside the added square"
    );
    let hole = d.px(600, 350);
    assert!(
        hole[0] > 240 && hole[1] > 240,
        "the square taken away: {hole:?}"
    );
    let out = d.px(420, 650);
    assert!(
        out[0] > 240 && out[1] > 240,
        "outside the triangle: {out:?}"
    );
}

// ---------------------------------------------------------------------------------------
// Transform

/// A blob at (x, y): a filled 100 × 60 box.
fn blob(d: &mut Doc, x: f32, y: f32) {
    d.outline([x, y, x + 100.0, y + 60.0]);
    let c = d.s.colour;
    d.s.colour = CORAL;
    let t = d.s.tool;
    d.tool(Tool::Fill).click((x + 50.0, y + 30.0));
    d.s.colour = c;
    d.s.tool = t;
}

/// The bounding box of non-white pixels.
fn ink_box(d: &mut Doc) -> Option<[u32; 4]> {
    d.e.painter.flush();
    let rgba = d.e.painter.rgba();
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    let mut any = false;
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            if rgba[i] < 200 || rgba[i + 1] < 200 || rgba[i + 2] < 200 {
                b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
                any = true;
            }
        }
    }
    any.then_some(b)
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn transform_moves_the_pixels_by_the_drag_and_undoes() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    let b0 = ink_box(&mut d).unwrap();
    let before = d.snapshot();
    d.tool(Tool::Select).line((480.0, 380.0), (620.0, 480.0));
    // a drag inside the box moves it by the drag
    d.tool(Tool::Transform).line((550.0, 430.0), (850.0, 530.0));
    assert!(d.s.tooling.transform.is_some(), "the transform box shows");
    d.act("Transform: commit");
    let b1 = ink_box(&mut d).unwrap();
    assert_eq!(
        [b1[0] as i64 - b0[0] as i64, b1[1] as i64 - b0[1] as i64],
        [300, 100],
        "moved by the drag: {b0:?} → {b1:?}"
    );
    assert_eq!(b1[2] - b1[0], b0[2] - b0[0], "same size");
    assert!(
        d.inked([b0[0], b0[1], b0[0] + 80, b0[1] + 50]) == 0,
        "nothing left behind"
    );
    save_png("tools-transform-moved.png", &mut d.e);
    assert!(d.act("Undo"));
    assert_eq!(diff_count(&d.snapshot(), &before), 0, "undo puts it back");
    assert!(d.act("Redo"));
    assert_eq!(ink_box(&mut d), Some(b1), "redo moves it again");
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn transform_escape_puts_it_back_and_switching_tool_keeps_it() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    let before = d.snapshot();
    d.tool(Tool::Select).line((480.0, 380.0), (620.0, 480.0));
    d.tool(Tool::Transform).line((550.0, 430.0), (750.0, 430.0));
    d.act("Escape");
    assert!(d.s.tooling.transform.is_none());
    assert_eq!(diff_count(&d.snapshot(), &before), 0, "Escape puts it back");
    d.tool(Tool::Transform).line((550.0, 430.0), (750.0, 430.0));
    // picking another tool keeps the move (the interface asks for "Transform: commit" when
    // the tool changes)
    d.tool(Tool::Brush);
    d.act("Transform: commit");
    let b = ink_box(&mut d).unwrap();
    assert!(b[2] >= 795, "kept after switching tool: {b:?}");
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn transform_handles_scale_and_turn() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    d.tool(Tool::Select).line((490.0, 390.0), (610.0, 470.0));
    d.tool(Tool::Transform);
    // start the transform (a press inside, no move) to know the box
    d.click((550.0, 430.0));
    let bx = d.s.tooling.transform.expect("box");
    let br = bx.point(1, 1);
    // drag the bottom-right corner 100 px right and 60 down: ×(1 + 100/w)
    let w0 = bx.rect[2] - bx.rect[0];
    d.line(br, (br.0 + w0, br.1 + (bx.rect[3] - bx.rect[1])));
    d.act("Transform: commit");
    let b = ink_box(&mut d).unwrap();
    let w = (b[2] - b[0]) as f32;
    assert!((w / w0 - 2.0).abs() < 0.06, "doubled in width: {w0} → {w}");
    // turn 90° with the handle above the box
    d.tool(Tool::Select).line(
        (b[0] as f32 - 5.0, b[1] as f32 - 5.0),
        (b[2] as f32 + 5.0, b[3] as f32 + 5.0),
    );
    d.tool(Tool::Transform)
        .click(((b[0] + b[2]) as f32 / 2.0, (b[1] + b[3]) as f32 / 2.0));
    let bx = d.s.tooling.transform.expect("box");
    let c = bx.point(0, 0);
    let h = bx.rotate_handle(28.0 / d.s.view.zoom);
    let r = (h.0 - c.0).hypot(h.1 - c.1);
    d.line(h, (c.0 + r, c.1));
    d.act("Transform: commit");
    let t = ink_box(&mut d).unwrap();
    let (tw, th) = ((t[2] - t[0]) as f32, (t[3] - t[1]) as f32);
    assert!(
        (tw - (b[3] - b[1]) as f32).abs() < 6.0 && (th - w).abs() < 6.0,
        "turned 90°: {b:?} → {t:?}"
    );
}

/// The owner's top complaint: moving a large painted area froze the app. Through the
/// Transform tool: 1200 × 800 of strokes selected, the press (lift), each drag step (CPU
/// cost per pointer move: the GPU moves the picture) and the release (commit), with whole and
/// fractional moves.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn transform_tool_moving_1200x800_of_strokes() {
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (name, step) in [("whole", 4.0f32), ("fractional", 4.37)] {
        let mut d = Doc::new();
        d.s.brush = 3; // Marker
        d.s.brush_size = 14.0;
        let mut y = 150.0;
        while y < 930.0 {
            d.line((370.0, y), (1550.0, y + 10.0));
            y += 9.0;
        }
        d.tool(Tool::Select).line((360.0, 140.0), (1560.0, 940.0));
        d.tool(Tool::Transform);
        let from = (900.0, 500.0);
        let t = Instant::now();
        d.ms += 4;
        d.e.pointer(&mut d.s, Phase::Down, smp(from.0, from.1, d.ms));
        let _ = d.e.painter.flush();
        let lift_ms = t.elapsed().as_secs_f64() * 1e3;
        let mut steps = Vec::new();
        for i in 1..=90 {
            let p = (from.0 + step * i as f32, from.1 + step * i as f32 / 2.0);
            d.ms += 4;
            let t = Instant::now();
            d.e.pointer(&mut d.s, Phase::Move, smp(p.0, p.1, d.ms));
            let _ = d.e.painter.flush();
            steps.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let end = (from.0 + step * 90.0, from.1 + step * 45.0);
        d.ms += 4;
        d.e.pointer(&mut d.s, Phase::Up, smp(end.0, end.1, d.ms));
        let t = Instant::now();
        d.act("Transform: commit");
        let commit_ms = t.elapsed().as_secs_f64() * 1e3;
        steps.sort_by(|a, b| a.total_cmp(b));
        let p99 = steps[(steps.len() - 1) * 99 / 100];
        let b = ink_box(&mut d).unwrap();
        rows.push(format!(
            "{{\"move\": \"{name}\", \"liftAtPressMs\": {lift_ms:.1}, \"stepMsP99\": {p99:.4}, \"commitMs\": {commit_ms:.1}, \"inkBox\": {b:?}}}"
        ));
        if lift_ms > 100.0 {
            problems.push(format!("{name}: the press (lift) takes {lift_ms:.0} ms"));
        }
        if p99 > 1.0 {
            problems.push(format!("{name}: a drag step costs {p99:.2} ms"));
        }
        if commit_ms >= 100.0 {
            problems.push(format!(
                "{name}: letting go takes {commit_ms:.0} ms (budget 100)"
            ));
        }
        let want_x = 370.0 - 7.0 + step * 90.0;
        if (b[0] as f32 - want_x).abs() > 8.0 {
            problems.push(format!("{name}: moved to x {} (want about {want_x})", b[0]));
        }
    }
    save_json(
        "tools-transform-1200x800.json",
        &format!("[{}]", rows.join(", ")),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------
// Eyedropper, Shape, Text

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn eyedropper_picks_the_colour_under_it() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    d.s.colour = [0.0, 0.0, 0.0, 1.0];
    d.tool(Tool::Eyedropper).click((550.0, 430.0));
    assert!(
        (0..3).all(|k| (d.s.colour[k] - CORAL[k]).abs() < 0.03),
        "picked {:?}",
        d.s.colour
    );
    // dragging picks as it goes: ends on paper, white
    d.tool(Tool::Eyedropper)
        .line((550.0, 430.0), (300.0, 200.0));
    assert!(
        d.s.colour[0] > 0.95 && d.s.colour[2] > 0.95,
        "{:?}",
        d.s.colour
    );
    // the picture didn't change
    assert!(!d.e.painter.can_redo());
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn shapes_draw_a_line_a_rectangle_and_an_ellipse_and_undo() {
    for kind in [ShapeKind::Line, ShapeKind::Rectangle, ShapeKind::Ellipse] {
        let mut d = Doc::new();
        d.s.tooling.shape_kind = kind;
        d.tool(Tool::Shape).line((400.0, 300.0), (800.0, 600.0));
        let b = ink_box(&mut d).unwrap_or_else(|| panic!("{kind:?} drew nothing"));
        for (got, want) in b.iter().zip([400u32, 300, 800, 600]) {
            assert!(
                (*got as i64 - want as i64).abs() <= 8,
                "{kind:?}: box {b:?}"
            );
        }
        let mid = d.inked([590, 440, 610, 460]);
        let corner = d.inked([400, 300, 412, 312]);
        match kind {
            ShapeKind::Line => assert!(mid > 0 && d.inked([700, 300, 800, 380]) == 0, "{kind:?}"),
            ShapeKind::Rectangle => {
                assert!(mid == 0 && corner > 0, "{kind:?}: hollow with corners")
            }
            ShapeKind::Ellipse => assert!(
                mid == 0 && corner == 0 && d.inked([596, 296, 604, 306]) > 0,
                "{kind:?}"
            ),
        }
        assert!(d.act("Undo"));
        assert!(ink_box(&mut d).is_none(), "{kind:?}: undo takes it away");
        save_png(&format!("tools-shape-{}.png", kind.name()), &mut d.e);
    }
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn text_types_onto_the_layer_and_undoes() {
    let mut d = Doc::new();
    d.tool(Tool::Text).click((600.0, 400.0));
    let t = d.s.tooling.text.as_mut().expect("typing starts");
    t.text = "Hello".into();
    d.act("Text: changed");
    d.act("Text: commit");
    assert!(d.s.tooling.text.is_none());
    let b = ink_box(&mut d).expect("the text drew");
    assert!(
        b[0] >= 595 && b[1] >= 395 && b[0] < 640 && b[1] < 460,
        "where it was clicked: {b:?}"
    );
    save_png("tools-text.png", &mut d.e);
    assert!(d.act("Undo"));
    assert!(ink_box(&mut d).is_none(), "undo takes the text away");
    // cancelled text leaves nothing
    d.tool(Tool::Text).click((600.0, 400.0));
    d.s.tooling.text.as_mut().unwrap().text = "Gone".into();
    d.act("Text: changed");
    d.act("Text: cancel");
    assert!(ink_box(&mut d).is_none(), "cancelled text left pixels");
}

// ---------------------------------------------------------------------------------------
// the selection's menu (the design's OtherMenus: Cut, Copy, Paste in place, Transform, Flip
// horizontally, Flip vertically, Move to layer, Fill selection, Clear) and the canvas menu

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_selection_menus_items_do_what_they_say() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    // an asymmetric mark so flips show: a dot at the blob's left
    d.tool(Tool::Brush).click((480.0, 420.0));
    let start = d.snapshot();
    d.tool(Tool::Select).line((470.0, 390.0), (610.0, 470.0));
    // Cut: the pixels go
    assert!(d.act("Cut"));
    assert_eq!(d.inked([470, 390, 610, 470]), 0, "Cut leaves nothing");
    assert!(d.s.tooling.has_clip, "Cut fills the clipboard");
    // Paste in place: back where they were
    assert!(d.act("Paste in place"));
    d.act("Transform: commit");
    assert!(
        diff_count(&d.snapshot(), &start) < 50,
        "Paste in place puts them back where they were ({} px differ)",
        diff_count(&d.snapshot(), &start)
    );
    // Copy then Paste: a copy, ready to move (Transform), the original stays
    d.tool(Tool::Select).line((470.0, 390.0), (610.0, 470.0));
    assert!(d.act("Copy"));
    assert!(d.act("Paste"));
    assert_eq!(d.s.tool, Tool::Transform, "Paste picks Transform");
    d.line((550.0, 430.0), (550.0, 730.0));
    d.act("Transform: commit");
    assert!(d.inked([470, 390, 610, 470]) > 1000, "the original stays");
    assert!(
        d.inked([470, 690, 610, 770]) > 1000,
        "the copy moved down 300"
    );
    // Flip horizontally, then vertically: the selected pixels mirror about their box's middle
    d.act("Escape");
    d.tool(Tool::Select).line((470.0, 390.0), (610.0, 470.0));
    let before = d.snapshot();
    // the lifted box: the ink inside the selection
    let b = {
        let mut b = [u32::MAX, u32::MAX, 0, 0];
        for y in 390..470u32 {
            for x in 470..610u32 {
                let i = ((y * W + x) * 4) as usize;
                if before[i] < 200 || before[i + 1] < 200 || before[i + 2] < 200 {
                    b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
                }
            }
        }
        b
    };
    assert!(d.act("Flip horizontally"));
    let after = d.snapshot();
    // (the painter mirrors about the box of every lifted pixel, faint edges included, so
    // the axis may sit a pixel or two off this box's: the best of a few offsets)
    let mirrored = |x: u32, y: u32, h: bool, o: i64| -> [u8; 4] {
        let (mx, my) = if h {
            ((b[0] + b[2] - 1) as i64 - x as i64 + o, y as i64)
        } else {
            (x as i64, (b[1] + b[3] - 1) as i64 - y as i64 + o)
        };
        let i = ((my * W as i64 + mx) * 4) as usize;
        before[i..i + 4].try_into().unwrap()
    };
    let mismatch = |img: &[u8], h: bool| {
        (-3..=3)
            .map(|o| {
                let mut n = 0;
                for y in b[1]..b[3] {
                    for x in b[0]..b[2] {
                        let i = ((y * W + x) * 4) as usize;
                        let (p, q) = (&img[i..i + 4], mirrored(x, y, h, o));
                        if (0..3).any(|k| (p[k] as i32 - q[k] as i32).abs() > 24) {
                            n += 1;
                        }
                    }
                }
                n
            })
            .min()
            .unwrap()
    };
    let area = ((b[2] - b[0]) * (b[3] - b[1])) as usize;
    let mh = mismatch(&after, true);
    assert!(
        mh * 100 < area * 2,
        "flipped horizontally: {mh} of {area} px differ from the mirror"
    );
    assert!(d.act("Flip horizontally"));
    assert!(d.act("Flip vertically"));
    let mv = mismatch(&d.snapshot(), false);
    assert!(
        mv * 100 < area * 2,
        "flipped vertically: {mv} of {area} px differ from the mirror"
    );
    // Fill selection and Clear
    d.s.colour = [0.0, 0.0, 1.0, 1.0];
    assert!(d.act("Fill selection"));
    assert!(near_rgb(d.px(475, 395), [0.0, 0.0, 1.0, 1.0], 6));
    assert!(d.act("Clear"));
    assert_eq!(
        d.inked([470, 390, 610, 470]),
        0,
        "Clear empties the selection"
    );
    assert!(d.inked([470, 690, 610, 770]) > 1000, "…and nothing else");
    // every one undoes, back to the start
    let mut n = 0;
    while d.e.painter.can_undo() && n < 50 {
        d.act("Undo");
        n += 1;
    }
    assert!(ink_box(&mut d).is_none(), "{n} undos back to empty");
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_canvas_menus_items_do_what_they_say() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    assert!(d.act("Select all"));
    assert_eq!(
        d.s.tooling.selection.map(|b| b.map(|v| v.round())),
        Some([0.0, 0.0, 1920.0, 1080.0])
    );
    d.act("Invert selection");
    assert!(
        d.s.tooling.selection.is_none(),
        "inverting everything selects nothing: {:?}",
        d.s.tooling.selection
    );
    d.act("Deselect");
    // Guides, Snap, Symmetry change their state and the hint
    let g = d.s.tooling.grid;
    d.act("Guides");
    assert_ne!(d.s.tooling.grid, g);
    d.act("Snap to guides");
    d.act("Symmetry");
    assert_eq!(d.s.tooling.symmetry.name(), "Vertical");
    // symmetry: a stroke on the left paints its mirror on the right
    d.tool(Tool::Brush).line((300.0, 800.0), (400.0, 800.0));
    assert!(d.inked([1520, 790, 1620, 810]) > 200, "the mirror stroke");
    d.act("Symmetry: Off");
    // Select (marquee) snaps to the grid when the grid shows and snap is on
    d.s.tooling.grid = true;
    d.s.tooling.snap = true;
    d.tool(Tool::Select).line((131.0, 131.0), (253.0, 250.0));
    assert_eq!(
        d.s.tooling.selection.map(|b| b.map(|v| v.round())),
        Some([128.0, 128.0, 256.0, 256.0]),
        "snapped to the 64 px grid"
    );
}

/// A stroke that starts while a transform is still floating (the pen pressed before the
/// interface's pass has turned the tool change into a commit: a key and a press in the same
/// batch of input): the move and the stroke must both survive.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn a_stroke_while_a_transform_floats_keeps_both() {
    let mut d = Doc::new();
    blob(&mut d, 500.0, 400.0);
    d.tool(Tool::Select).line((480.0, 380.0), (620.0, 480.0));
    d.tool(Tool::Transform).line((550.0, 430.0), (750.0, 430.0));
    // (inside the selection, which follows the moved pixels to 680–820 × 380–480: a
    // selection limits painting)
    d.tool(Tool::Brush).line((690.0, 473.0), (810.0, 473.0));
    let stroke_now = d.inked([690, 469, 810, 478]);
    d.act("Transform: commit");
    let b = ink_box(&mut d).unwrap();
    let stroke = d.inked([690, 469, 810, 478]);
    let moved = d.inked([700, 400, 800, 460]);
    save_json(
        "tools-stroke-while-floating.json",
        &format!(
            "{{\"strokePxBeforeCommit\": {stroke_now}, \"strokePx\": {stroke}, \"movedBlobPx\": {moved}, \"inkBox\": {b:?}}}"
        ),
    );
    assert!(stroke > 500, "the stroke painted ({stroke} px)");
    assert!(
        moved > 3000,
        "the move was kept ({moved} px at the new place)"
    );
}
