//! QA for the canvas and its tools (Phase 2 step 4): the view's maths, strokes through the
//! view, the view's keys and menu items, the tools, layers and frames, and the budgets that
//! can be measured through the library (a transform's preview step on a large selection).
//!
//! The view's maths needs nothing and runs everywhere. The rest paints a full document or needs
//! a GPU (Vulkan), so it is ignored by a plain `cargo test`; run it with
//! `tools/native-qa/run.sh` or
//! `cargo test --release -p pencilpilot-native --test canvas_qa -- --include-ignored --test-threads 1`.
//!
//! `PP_QA_OUT` = a folder for the measurements (JSON) and pictures (PNG) the tests save.

#![cfg(target_os = "linux")]

use std::f32::consts::TAU;
use std::time::Instant;

use pencilpilot_gpu::{Error, Gpu, Renderer2d};
use pencilpilot_native::shell::engine::{Engine, Phase, Sample};
use pencilpilot_native::shell::view::View;
use pencilpilot_native::shell::{self, layout};
use pencilpilot_native::state::{State, Tool, Workspace};
use pencilpilot_ui::ui::{Key, Rect, Ui};

const W: f32 = 1440.0;
const H: f32 = 900.0;
const DOC: (u32, u32) = (1920, 1080);

/// Where the drawing goes in the window at 1440 × 900 (the canvas card less its padding),
/// as the shell lays it out.
fn canvas_rect(ws: Workspace) -> Rect {
    layout::regions(W, H, ws).canvas.inset(12.0)
}

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

fn save_png(name: &str, w: u32, h: u32, rgba: &[u8]) {
    let Some(d) = out_dir() else { return };
    let file = std::io::BufWriter::new(std::fs::File::create(d.join(name)).unwrap());
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Distance from p to the segment a–b.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

/// A spread of views: fitted, zoomed (down to the minimum and up to the maximum), turned
/// (30° included), flipped, panned, and all of them together.
fn views(card: Rect) -> Vec<(String, View)> {
    let mut out = Vec::new();
    let mut v = View::default();
    v.reset(card);
    out.push(("fitted".to_string(), v));
    for zoom in [0.02, 0.1, 0.5, 1.0, 3.0, 12.0, 64.0] {
        for deg in [0.0f32, 30.0, 90.0, 181.0, 330.0] {
            for flip in [false, true] {
                let mut v = View::default();
                v.reset(card);
                v.set_zoom(card, zoom);
                v.rotate_about(card, (card.x + 200.0, card.y + 150.0), deg.to_radians());
                if flip {
                    v.flip_view(card);
                }
                v.pan_by(-37.0, 21.5);
                out.push((format!("zoom {zoom} turned {deg}° flipped {flip}"), v));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// 1. the view: document ↔ screen (no GPU)

#[test]
fn view_round_trips_both_ways_at_every_zoom_angle_and_flip() {
    let card = canvas_rect(Workspace::Animate);
    let mut worst_doc = 0.0f32;
    let mut worst_screen = 0.0f32;
    for (name, v) in views(card) {
        for d in [(0.0, 0.0), (1920.0, 1080.0), (333.3, 777.7), (960.0, 540.0)] {
            let s = v.to_screen(card, d.0, d.1);
            let back = v.to_doc(card, s.0, s.1);
            // in document pixels, relative to how big a screen pixel is there
            let e = dist(back, d) * v.zoom.max(1.0);
            worst_doc = worst_doc.max(e);
            assert!(e < 0.05, "{name}: doc {d:?} → screen {s:?} → doc {back:?}");
        }
        for s in [
            (card.x, card.y),
            (card.right(), card.bottom()),
            (512.5, 333.25),
        ] {
            let d = v.to_doc(card, s.0, s.1);
            let back = v.to_screen(card, d.0, d.1);
            let e = dist(back, s);
            worst_screen = worst_screen.max(e);
            assert!(
                e < 0.05,
                "{name}: screen {s:?} → doc {d:?} → screen {back:?}"
            );
        }
    }
    save_json(
        "view-roundtrip.json",
        &format!(
            "{{\"worstDocErrScreenPx\": {worst_doc:.5}, \"worstScreenErrPx\": {worst_screen:.5}}}"
        ),
    );
}

#[test]
fn zooming_and_turning_keep_the_point_under_the_pointer() {
    let card = canvas_rect(Workspace::Animate);
    let mut worst = 0.0f32;
    for (name, v0) in views(card) {
        for at in [
            (card.x + 10.0, card.y + 10.0),
            (700.0, 333.0),
            (card.right() - 5.0, card.bottom() - 5.0),
        ] {
            for factor in [1.25, 0.8, 1.2f32.powi(5), 1.0 / 1.2f32.powi(3)] {
                let mut v = v0;
                let before = v.to_doc(card, at.0, at.1);
                v.zoom_at(card, at, factor);
                // (at the limits the zoom clamps, but the point still stays)
                let after = v.to_screen(card, before.0, before.1);
                worst = worst.max(dist(after, at));
                assert!(
                    dist(after, at) < 0.05,
                    "{name}: zoom ×{factor} at {at:?} moved it to {after:?}"
                );
                assert!(!v.fitted, "{name}: zoomed by hand, still 'fitted'");
            }
            let mut v = v0;
            let before = v.to_doc(card, at.0, at.1);
            v.rotate_about(card, at, 30f32.to_radians());
            let after = v.to_screen(card, before.0, before.1);
            worst = worst.max(dist(after, at));
            assert!(
                dist(after, at) < 0.05,
                "{name}: turning about {at:?} moved it to {after:?}"
            );
        }
    }
    save_json(
        "view-pinned.json",
        &format!("{{\"worstDriftPx\": {worst:.5}}}"),
    );
}

#[test]
fn turning_and_flipping_go_the_right_way() {
    let card = canvas_rect(Workspace::Animate);
    let c = (card.x + card.w / 2.0, card.y + card.h / 2.0);
    let mut v = View::default();
    v.reset(card);
    v.rotate_about(card, c, 30f32.to_radians());
    // the document's +x axis now points 30° clockwise on screen (y down)
    let a = v.to_screen(card, 960.0, 540.0);
    let b = v.to_screen(card, 1060.0, 540.0);
    let ang = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
    assert!((ang - 30.0).abs() < 0.01, "turned 30°: +x shows at {ang}°");
    // flipped: the left edge of the document shows on the right of its centre
    let mut f = View::default();
    f.reset(card);
    f.flip_view(card);
    assert!(f.to_screen(card, 0.0, 540.0).0 > c.0);
    assert!(f.to_screen(card, 1920.0, 540.0).0 < c.0);
    // flipping twice puts the view back
    let mut g = v;
    g.flip_view(card);
    g.flip_view(card);
    for d in [(0.0, 0.0), (1920.0, 1080.0), (500.0, 200.0)] {
        assert!(
            dist(g.to_screen(card, d.0, d.1), v.to_screen(card, d.0, d.1)) < 0.05,
            "flip twice ≠ the view before: {v:?} vs {g:?}"
        );
    }
    // a turned, flipped view stays a mirror: the screen's turn of +x is −(angle)
    let mut h = v;
    h.flip_view(card);
    let a = h.to_screen(card, 960.0, 540.0);
    let b = h.to_screen(card, 860.0, 540.0); // −x shows where +x did before the flip
    let ang2 = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
    assert!(
        (ang2 - 30.0).abs() < 0.01 || (ang2 + 30.0).abs() < 0.01,
        "{ang2}"
    );
    assert!(h.angle >= 0.0 && h.angle < TAU);
}

#[test]
fn fit_shows_the_whole_document_in_every_workspace() {
    for ws in Workspace::ALL {
        let card = canvas_rect(ws);
        let mut v = View::default();
        v.reset(card);
        let tl = v.to_screen(card, 0.0, 0.0);
        let br = v.to_screen(card, 1920.0, 1080.0);
        assert!(
            tl.0 >= card.x - 0.01
                && tl.1 >= card.y - 0.01
                && br.0 <= card.right() + 0.01
                && br.1 <= card.bottom() + 0.01,
            "{}: {tl:?} {br:?} outside {card:?}",
            ws.name()
        );
        // one side touches the card less the margin
        let gap_x = (card.w - (br.0 - tl.0)) / 2.0;
        let gap_y = (card.h - (br.1 - tl.1)) / 2.0;
        assert!(
            (gap_x.min(gap_y) - 24.0).abs() < 0.5,
            "{}: margins {gap_x} {gap_y}",
            ws.name()
        );
    }
}

// ---------------------------------------------------------------------------------------
// 1b. a stroke through the view lands under the pointer (the window's mapping, the engine's
//     pixels; no GPU)

/// Paints a straight screen-space stroke from `a` to `b` through `view`, the way the window
/// does (each pointer sample mapped with `View::to_doc` over the canvas rectangle).
fn screen_stroke(e: &mut Engine, s: &mut State, card: Rect, a: (f32, f32), b: (f32, f32)) {
    let n = 60;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let (dx, dy) = s.view.to_doc(card, x, y);
        let phase = match i {
            0 => Phase::Down,
            _ if i == n => Phase::Up,
            _ => Phase::Move,
        };
        let p = Sample {
            x: dx,
            y: dy,
            pressure: 0.8,
            tilt: (0.0, 0.0),
            ms: i * 8,
        };
        e.pointer(s, phase, p);
    }
    e.painter.flush();
}

/// The document's pixels with ink (darker than the paper), as document points.
fn ink_points(rgba: &[u8], w: u32) -> Vec<(f32, f32)> {
    rgba.chunks(4)
        .enumerate()
        .filter(|(_, p)| p[0] < 180 && p[3] > 0)
        .map(|(i, _)| ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5))
        .collect()
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn a_stroke_at_300_percent_turned_30_and_flipped_lands_under_the_pointer() {
    let card = canvas_rect(Workspace::Animate);
    let mut report = Vec::new();
    for (name, zoom, deg, flip) in [
        ("fitted", 0.0, 0.0, false),
        ("300%", 3.0, 0.0, false),
        ("300% turned 30°", 3.0, 30.0, false),
        ("300% turned 30° flipped", 3.0, 30.0, true),
        ("50% turned 200° flipped", 0.5, 200.0, true),
    ] {
        let mut s = State::demo();
        s.brush_size = 6.0;
        s.view.reset(card);
        let c = (card.x + card.w / 2.0, card.y + card.h / 2.0);
        if zoom > 0.0 {
            s.view.set_zoom(card, zoom);
        }
        s.view.rotate_about(card, c, (deg as f32).to_radians());
        if flip {
            s.view.flip_view(card);
        }
        let mut e = Engine::new(DOC.0, DOC.1);
        // a stroke across the screen, left to right and a little down
        let (a, b) = ((c.0 - 120.0, c.1 - 30.0), (c.0 + 120.0, c.1 + 40.0));
        screen_stroke(&mut e, &mut s, card, a, b);
        let ink = ink_points(e.painter.rgba(), DOC.0);
        assert!(
            ink.len() > 50,
            "{name}: the stroke painted ({} px)",
            ink.len()
        );
        // every inked document pixel shows within the brush's reach of the screen line
        let reach = (s.brush_size / 2.0 + 2.0) * s.view.zoom + 1.5;
        let mut worst = 0.0f32;
        for &d in &ink {
            let p = s.view.to_screen(card, d.0, d.1);
            worst = worst.max(seg_dist(p, a, b));
        }
        // and the line is covered from end to end: ink near both ends and the middle
        let near_screen = |q: (f32, f32)| {
            ink.iter()
                .map(|&d| dist(s.view.to_screen(card, d.0, d.1), q))
                .fold(f32::MAX, f32::min)
        };
        let ends = [
            near_screen(a),
            near_screen(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)),
            near_screen(b),
        ];
        report.push(format!(
            "{{\"view\": \"{name}\", \"inkPx\": {}, \"worstOffLineScreenPx\": {worst:.2}, \"reachScreenPx\": {reach:.2}, \"gapAtEndsMiddleEndPx\": [{:.2}, {:.2}, {:.2}]}}",
            ink.len(),
            ends[0],
            ends[1],
            ends[2]
        ));
        assert!(
            worst <= reach,
            "{name}: ink {worst:.1} px off the pointer's line (reach {reach:.1})"
        );
        // the middle is covered; the ends may fall short by the brush's slow tracking
        // (the 2B pencil's `slow_tracking` 1.03: a few document pixels at the stroke's end)
        let lag = reach + 8.0 * s.view.zoom;
        assert!(
            ends[1] <= reach,
            "{name}: the middle isn't covered ({ends:?}, reach {reach:.1})"
        );
        for g in [ends[0], ends[2]] {
            assert!(
                g <= lag,
                "{name}: an end isn't covered ({ends:?}, allowed {lag:.1})"
            );
        }
        if name.contains("flipped") && zoom == 3.0 {
            save_png(
                "stroke-300-30-flipped-doc.png",
                DOC.0,
                DOC.1,
                e.painter.rgba(),
            );
        }
    }
    save_json(
        "stroke-through-view.json",
        &format!("[{}]", report.join(", ")),
    );
}

// ---------------------------------------------------------------------------------------
// the shell through the library (needs a GPU for the interface's text)

struct Harness {
    r: Renderer2d,
    ui: Ui,
    s: State,
    card: Rect,
    _gpu: Gpu,
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
            card: Rect::default(),
            _gpu: gpu,
        };
        h.pass();
        h.s.view.reset(h.card);
        h.pass();
        Some(h)
    }

    fn pass(&mut self) -> bool {
        let s = &mut self.s;
        let mut card = Rect::default();
        let drew = self
            .ui
            .pass(&mut self.r, W, H, |f| {
                card = shell::build(f, s, W, H).1;
            })
            .is_some();
        self.card = card;
        drew
    }

    fn input_pass(&mut self) {
        self.pass();
        self.pass();
    }

    fn key(&mut self, name: &str, text: &str) {
        self.ui.key(Key {
            name: name.into(),
            text: Some(text.into()),
            ctrl: false,
            shift: false,
        });
        self.input_pass();
    }

    fn click(&mut self, x: f32, y: f32) {
        self.ui.pointer_moved(x, y);
        self.input_pass();
        self.ui.pointer_button(true);
        self.input_pass();
        self.ui.pointer_button(false);
        self.input_pass();
    }
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_canvas_rect_the_shell_returns_is_the_cards_inside() {
    let Some(mut h) = Harness::new() else { return };
    for ws in Workspace::ALL {
        h.s.workspace = ws;
        h.input_pass();
        assert_eq!(h.card, canvas_rect(ws), "{}", ws.name());
    }
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_views_keys_do_what_they_say() {
    let Some(mut h) = Harness::new() else { return };
    let card = h.card;
    let fit = h.s.view.fit_zoom(card);
    let mut log = Vec::new();
    h.key("1", "1");
    log.push(format!("1 → zoom {:.4}", h.s.view.zoom));
    assert!(
        (h.s.view.zoom - 1.0).abs() < 1e-4,
        "1 is 100 %: {}",
        h.s.view.zoom
    );
    h.key("plus", "+");
    assert!(
        (h.s.view.zoom - 1.25).abs() < 1e-4,
        "+ zooms in: {}",
        h.s.view.zoom
    );
    h.key("equal", "=");
    assert!(
        (h.s.view.zoom - 1.5625).abs() < 1e-3,
        "= zooms in too: {}",
        h.s.view.zoom
    );
    h.key("minus", "-");
    assert!(
        (h.s.view.zoom - 1.25).abs() < 1e-3,
        "− zooms out: {}",
        h.s.view.zoom
    );
    h.key("KP_Subtract", "-");
    assert!(
        (h.s.view.zoom - 1.0).abs() < 1e-3,
        "keypad − zooms out: {}",
        h.s.view.zoom
    );
    h.key("KP_Add", "+");
    assert!(
        (h.s.view.zoom - 1.25).abs() < 1e-3,
        "keypad + zooms in: {}",
        h.s.view.zoom
    );
    // + and − zoom about the card's centre: its document point stays put
    let c = (card.x + card.w / 2.0, card.y + card.h / 2.0);
    let before = h.s.view.to_doc(card, c.0, c.1);
    h.key("plus", "+");
    assert!(dist(h.s.view.to_doc(card, c.0, c.1), before) < 0.05);
    h.key("h", "h");
    assert!(h.s.view.flip, "H flips");
    h.key("h", "h");
    assert!(!h.s.view.flip, "H again unflips");
    h.s.view.rotate_about(card, c, 0.5);
    h.s.view.pan_by(30.0, 40.0);
    h.key("0", "0");
    log.push(format!("0 → {:?}", h.s.view));
    assert!(
        h.s.view.fitted
            && (h.s.view.zoom - fit).abs() < 1e-4
            && h.s.view.angle == 0.0
            && h.s.view.pan == (0.0, 0.0)
            && !h.s.view.flip,
        "0 fits: {:?}",
        h.s.view
    );
    // the limits
    for _ in 0..60 {
        h.key("plus", "+");
    }
    assert!(h.s.view.zoom <= 64.0 + 1e-3, "{}", h.s.view.zoom);
    for _ in 0..120 {
        h.key("minus", "-");
    }
    assert!(h.s.view.zoom >= 0.02 - 1e-5, "{}", h.s.view.zoom);
    log.push(format!("limits: {:.4} after 120 −", h.s.view.zoom));
    // none of them paint or change the tool
    assert_eq!(h.s.tool, Tool::Brush);
    assert!(
        h.s.actions
            .iter()
            .all(|a| !matches!(a, pencilpilot_native::state::Action::Undo))
    );
    save_json("view-keys.json", &format!("{log:?}"));
}

/// Focuses a text field in the shell (the Review panel's note), returns whether it did.
fn focus_a_text_field(h: &mut Harness) -> bool {
    h.s.workspace = Workspace::Review;
    h.input_pass();
    let p = layout::regions(W, H, Workspace::Review).panel;
    let mut y = p.bottom() - 4.0;
    while y > p.bottom() - 120.0 {
        let mut x = p.x + 20.0;
        while x < p.x + 160.0 {
            h.click(x, y);
            if h.ui.typing() {
                return true;
            }
            x += 20.0;
        }
        y -= 6.0;
    }
    false
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_views_keys_dont_fire_while_typing() {
    let Some(mut h) = Harness::new() else { return };
    assert!(focus_a_text_field(&mut h), "found no text field to type in");
    let before = h.s.view;
    for (k, t) in [
        ("0", "0"),
        ("1", "1"),
        ("h", "h"),
        ("plus", "+"),
        ("minus", "-"),
        ("equal", "="),
    ] {
        h.key(k, t);
    }
    assert_eq!(h.s.view, before, "the view changed while typing");
    assert!(
        h.s.panel.draft.contains("01h"),
        "the letters went into the field: {:?}",
        h.s.panel.draft
    );
}

// ---------------------------------------------------------------------------------------
// 4. budgets through the library: moving a large painted area

/// The document painted full of strokes across `area` (x, y, w, h), with the Marker.
fn painted(area: (f32, f32, f32, f32)) -> (Engine, State) {
    let mut s = State::demo();
    s.brush = 3; // Marker
    s.brush_size = 14.0;
    let mut e = Engine::new(DOC.0, DOC.1);
    let (x, y, w, h) = area;
    let mut row = 0;
    let mut yy = y + 6.0;
    while yy < y + h - 6.0 {
        let p = |x: f32, y: f32, ms: u32| Sample {
            x,
            y,
            pressure: 0.9,
            tilt: (0.0, 0.0),
            ms,
        };
        e.pointer(&mut s, Phase::Down, p(x + 6.0, yy, 0));
        let n = 120;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let wob = ((t * 40.0 + row as f32).sin()) * 4.0;
            e.pointer(
                &mut s,
                Phase::Move,
                p(x + 6.0 + (w - 12.0) * t, yy + wob, i * 4),
            );
        }
        e.pointer(&mut s, Phase::Up, p(x + w - 6.0, yy, n * 4 + 4));
        yy += 9.0;
        row += 1;
    }
    e.painter.flush();
    (e, s)
}

fn stats(v: &mut [f64]) -> (f64, f64, f64, f64) {
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    (q(0.5), q(0.95), q(0.99), v[v.len() - 1])
}

/// The owner's top complaint today: moving a large painted area froze the app. With
/// 1200 × 800 of strokes selected, each preview step of a drag (move the lifted pixels, then
/// what the window does with the picture: flush and copy out the changed rectangle) must stay
/// under 16 ms, and letting go must finish within 100 ms (BUDGETS.md).
///
/// This measures the brush crate's CPU preview (`transform_preview` every step), which the
/// app no longer uses for a drag (step4 e771f04: the engine's float); kept as the comparison.
/// The budget is held by `moving_a_1200x800_painted_selection_through_the_engines_float`.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn moving_a_1200x800_painted_selection_stays_interactive() {
    drag_a_large_selection(4.0, "transform-1200x800.json");
}

/// The same, by fractions of a pixel each step (a view not at 100 %: the pointer's moves
/// land between document pixels, and the lifted pixels are resampled).
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn moving_a_1200x800_painted_selection_by_fractions_stays_interactive() {
    drag_a_large_selection(4.37, "transform-1200x800-fractional.json");
}

fn drag_a_large_selection(step: f32, json: &str) {
    let area = (360.0, 140.0, 1200.0, 800.0);
    let t0 = Instant::now();
    let (mut e, _s) = painted(area);
    let paint_ms = t0.elapsed().as_secs_f64() * 1e3;
    let inked = ink_points(e.painter.rgba(), DOC.0).len();
    // the selection: the painted rectangle
    let (dw, dh) = (DOC.0 as usize, DOC.1 as usize);
    let mut mask = vec![0u8; dw * dh];
    // (with a 20 px margin: the strokes' wobble)
    for yy in (area.1 - 20.0) as usize..(area.1 + area.3 + 20.0) as usize {
        for xx in (area.0 - 20.0) as usize..(area.0 + area.2 + 20.0) as usize {
            mask[yy * dw + xx] = 255;
        }
    }
    e.painter.set_selection(&mask, 0);
    let t = Instant::now();
    let lifted = e.painter.transform_begin();
    let begin_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(lifted.len(), 4, "nothing lifted");
    e.painter.flush();
    // a drag: 90 steps, 4 px right and 2 px down each, the way a pointer moves at 165 Hz
    let mut steps = Vec::new();
    let mut previews = Vec::new();
    let mut areas = Vec::new();
    for i in 1..=90 {
        let (dx, dy) = (i as f32 * step, i as f32 * step / 2.0);
        let t = Instant::now();
        assert!(e.painter.transform_preview([1.0, 0.0, 0.0, 1.0, dx, dy]));
        let p = t.elapsed().as_secs_f64() * 1e3;
        // what the window does next: the picture's changed rectangle, copied for the GPU
        let d = e.painter.flush();
        if let Some(d) = d {
            let rgba = e.painter.rgba();
            let mut copy = Vec::with_capacity((d.w * d.h * 4) as usize);
            for row in d.y..d.y + d.h {
                let o = ((row * DOC.0 + d.x) * 4) as usize;
                copy.extend_from_slice(&rgba[o..o + (d.w * 4) as usize]);
            }
            std::hint::black_box(&copy);
            areas.push(d.w as u64 * d.h as u64);
        }
        steps.push(t.elapsed().as_secs_f64() * 1e3);
        previews.push(p);
    }
    let t = Instant::now();
    assert!(e.painter.transform_commit());
    e.painter.flush();
    let commit_ms = t.elapsed().as_secs_f64() * 1e3;
    // it moved: ink now starts 360 px right, 180 px down of where it did
    let ink = ink_points(e.painter.rgba(), DOC.0);
    let minx = ink.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let miny = ink.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let t = Instant::now();
    assert!(e.painter.undo());
    e.painter.flush();
    let undo_ms = t.elapsed().as_secs_f64() * 1e3;
    let back = ink_points(e.painter.rgba(), DOC.0);
    let bminx = back.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let (p50, p95, p99, max) = stats(&mut steps);
    let (pp50, _, _, pmax) = stats(&mut previews);
    let mean_area = areas.iter().sum::<u64>() as f64 / areas.len().max(1) as f64;
    save_json(
        json,
        &format!(
            "{{\"paintedMs\": {paint_ms:.0}, \"inkPx\": {inked}, \"lifted\": {lifted:?}, \"beginMs\": {begin_ms:.1}, \"stepMs\": {{\"p50\": {p50:.2}, \"p95\": {p95:.2}, \"p99\": {p99:.2}, \"max\": {max:.2}}}, \"previewOnlyMs\": {{\"p50\": {pp50:.2}, \"max\": {pmax:.2}}}, \"meanChangedAreaPx\": {mean_area:.0}, \"commitMs\": {commit_ms:.1}, \"undoMs\": {undo_ms:.1}, \"inkMinAfterMove\": [{minx}, {miny}], \"inkMinXAfterUndo\": {bminx}, \"budgetStepMs\": 16, \"budgetCommitMs\": 100}}"
        ),
    );
    assert!(
        minx >= area.0 + 355.0,
        "the pixels moved by the drag: min x {minx}"
    );
    assert!(bminx < area.0 + 10.0, "undo put them back: min x {bminx}");
    // (the app no longer previews a drag this way, see the float test below: these numbers
    // are the CPU path's, kept for comparison, not held to the budget)
    let _ = (p95, begin_ms, commit_ms);
}

/// The same move through the engine's float (step4's `Engine::float_begin` / `float_move` /
/// `float_end`): during the drag the lifted pixels are a GPU picture, so a step costs the CPU
/// nothing; lifting and letting go are measured (whole-pixel and fractional moves), and the
/// pixels must land where the matrix says.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn moving_a_1200x800_painted_selection_through_the_engines_float() {
    let area = (360.0, 140.0, 1200.0, 800.0);
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (name, dx, dy) in [("whole", 360.0f32, 180.0f32), ("fractional", 393.3, 196.65)] {
        let (mut e, _s) = painted(area);
        let (dw, dh) = (DOC.0 as usize, DOC.1 as usize);
        let mut mask = vec![0u8; dw * dh];
        for yy in (area.1 - 20.0) as usize..(area.1 + area.3 + 20.0) as usize {
            for xx in (area.0 - 20.0) as usize..(area.0 + area.2 + 20.0) as usize {
                mask[yy * dw + xx] = 255;
            }
        }
        e.painter.set_selection(&mask, 0);
        let t = Instant::now();
        let lifted = e.painter.transform_begin();
        assert_eq!(lifted.len(), 4);
        assert!(e.float_begin(), "float_begin");
        e.painter.flush();
        let lift_ms = t.elapsed().as_secs_f64() * 1e3;
        let mut steps = Vec::new();
        for i in 1..=90 {
            let k = i as f32 / 90.0;
            let t = Instant::now();
            e.float_move([1.0, 0.0, 0.0, 1.0, dx * k, dy * k]);
            let _ = e.painter.flush();
            steps.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let t = Instant::now();
        assert!(e.float_end(true));
        e.painter.flush();
        let commit_ms = t.elapsed().as_secs_f64() * 1e3;
        let ink = ink_points(e.painter.rgba(), DOC.0);
        let minx = ink.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let (p50, _, p99, max) = stats(&mut steps);
        rows.push(format!(
            "{{\"move\": \"{name}\", \"liftMs\": {lift_ms:.1}, \"stepMs\": {{\"p50\": {p50:.4}, \"p99\": {p99:.4}, \"max\": {max:.4}}}, \"commitMs\": {commit_ms:.1}, \"inkMinX\": {minx}}}"
        ));
        problems.extend(check_float(name, minx, lifted[0] + dx, commit_ms, p99));
    }
    save_json(
        "transform-float-1200x800.json",
        &format!("[{}]", rows.join(", ")),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn check_float(name: &str, minx: f32, want: f32, commit_ms: f64, p99: f64) -> Vec<String> {
    let mut out = Vec::new();
    if (minx - want).abs() > 1.5 {
        out.push(format!("{name}: the pixels landed at x {minx}, not {want}"));
    }
    if commit_ms >= 100.0 {
        out.push(format!(
            "{name}: letting go takes {commit_ms:.0} ms; budget 100 ms"
        ));
    }
    if p99 >= 1.0 {
        out.push(format!("{name}: a drag step costs the CPU {p99:.2} ms"));
    }
    out
}
