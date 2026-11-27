//! The tools through the engine and the painter's pixels.

use super::*;
use crate::shell::engine::{Engine, Phase, Sample};
use crate::state::{State, Tool};

fn at(x: f32, y: f32) -> Sample {
    Sample {
        x,
        y,
        pressure: 0.5,
        tilt: (0.0, 0.0),
        ms: 0,
    }
}

fn drag(e: &mut Engine, s: &mut State, from: (f32, f32), to: (f32, f32)) {
    e.pointer(s, Phase::Down, at(from.0, from.1));
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        e.pointer(
            s,
            Phase::Move,
            at(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
        );
    }
    e.pointer(s, Phase::Up, at(to.0, to.1));
}

fn click(e: &mut Engine, s: &mut State, p: (f32, f32)) {
    e.pointer(s, Phase::Down, at(p.0, p.1));
    e.pointer(s, Phase::Up, at(p.0, p.1));
}

fn other(e: &mut Engine, s: &mut State, name: &str) -> bool {
    e.apply(s, &Action::Other(name.into()))
}

/// The composite's pixel (flushed).
fn px(e: &mut Engine, x: u32, y: u32) -> [u8; 4] {
    e.painter.flush();
    let w = e.painter.width();
    let i = ((y * w + x) * 4) as usize;
    e.painter.rgba()[i..i + 4].try_into().unwrap()
}

/// The active layer's painted pixels (alpha > 128) inside (x0, y0)–(x1, y1).
fn painted(e: &Engine, r: [u32; 4]) -> usize {
    let w = e.painter.width();
    let l = e.painter.layer_rgba(e.painter.active_layer());
    let mut n = 0;
    for y in r[1]..r[3] {
        for x in r[0]..r[2] {
            if l[((y * w + x) * 4 + 3) as usize] > 128 {
                n += 1;
            }
        }
    }
    n
}

/// A solid block painted with the Liner (an opaque ink) on a fresh engine.
fn block(e: &mut Engine, s: &mut State, r: [f32; 4]) {
    let tool = s.tool;
    s.tool = Tool::Brush;
    s.brush = 2; // Liner
    s.brush_size = 10.0;
    let mut y = r[1] + 4.0;
    while y < r[3] - 3.0 {
        e.down(s, r[0] + 4.0, y, 1.0, (0.0, 0.0), 0);
        let mut x = r[0] + 4.0;
        let mut ms = 0;
        while x < r[2] - 4.0 {
            x += 4.0;
            ms += 8;
            e.moved(x.min(r[2] - 4.0), y, 1.0, (0.0, 0.0), ms);
        }
        e.up();
        y += 4.0;
    }
    e.painter.flush();
    s.tool = tool;
}

/// A closed ring (a square outline) painted with the Liner.
fn ring(e: &mut Engine, s: &mut State, r: [f32; 4]) {
    let tool = s.tool;
    s.tool = Tool::Brush;
    s.brush = 2;
    s.brush_size = 6.0;
    let c = [
        (r[0], r[1]),
        (r[2], r[1]),
        (r[2], r[3]),
        (r[0], r[3]),
        (r[0], r[1]),
    ];
    e.down(s, c[0].0, c[0].1, 1.0, (0.0, 0.0), 0);
    let mut ms = 0;
    for w in c.windows(2) {
        for i in 1..=40 {
            let t = i as f32 / 40.0;
            ms += 8;
            e.moved(
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
                1.0,
                (0.0, 0.0),
                ms,
            );
        }
    }
    e.up();
    e.painter.flush();
    s.tool = tool;
}

fn engine() -> (Engine, State) {
    let mut s = State::demo();
    s.workspace = crate::state::Workspace::Draw;
    (Engine::new(400, 300), s)
}

#[test]
fn fill_fills_a_closed_shape_and_undo_takes_it_away() {
    let (mut e, mut s) = engine();
    ring(&mut e, &mut s, [100.0, 80.0, 220.0, 200.0]);
    s.tool = Tool::Fill;
    s.colour = pencilpilot_ui::theme::hex(0x3D6BFF);
    let before = painted(&e, [90, 70, 230, 210]);
    click(&mut e, &mut s, (160.0, 140.0));
    let inside = painted(&e, [110, 90, 210, 190]);
    assert!(inside > 9500, "the inside is filled: {inside} of 10000 px");
    // not outside the ring
    assert_eq!(painted(&e, [250, 0, 400, 300]), 0, "nothing outside");
    let p = px(&mut e, 160, 140);
    assert!(p[2] > 200 && p[0] < 100, "blue inside: {p:?}");
    assert!(e.painter.undo());
    assert_eq!(
        painted(&e, [90, 70, 230, 210]),
        before,
        "undo took the fill away"
    );
}

#[test]
fn fill_closes_small_gaps() {
    let (mut e, mut s) = engine();
    ring(&mut e, &mut s, [100.0, 80.0, 220.0, 200.0]);
    // a 3 px gap in the ring's top edge
    let w = 400;
    let mut cut = vec![0u8; 400 * 300];
    for y in 70..95 {
        for x in 158..161 {
            cut[y * w + x] = 255;
        }
    }
    e.painter.erase_mask(&cut);
    s.tool = Tool::Fill;
    s.panel.close_gaps = false;
    click(&mut e, &mut s, (160.0, 140.0));
    let leaked = painted(&e, [0, 0, 400, 60]);
    assert!(
        leaked > 1000,
        "without closing gaps the fill leaks out ({leaked})"
    );
    e.painter.undo();
    s.panel.close_gaps = true;
    click(&mut e, &mut s, (160.0, 140.0));
    assert_eq!(painted(&e, [0, 0, 400, 60]), 0, "closing gaps keeps it in");
    assert!(painted(&e, [110, 100, 210, 190]) > 8500);
}

#[test]
fn the_eyedropper_picks_the_painted_colour() {
    let (mut e, mut s) = engine();
    s.colour = pencilpilot_ui::theme::hex(0xE8492F);
    block(&mut e, &mut s, [50.0, 50.0, 150.0, 150.0]);
    s.colour = [0.0, 0.0, 0.0, 1.0];
    s.tool = Tool::Eyedropper;
    click(&mut e, &mut s, (100.0, 100.0));
    let got = s.colour.map(|c| (c * 255.0).round() as i32);
    assert!(
        (got[0] - 0xE8).abs() <= 3 && (got[1] - 0x49).abs() <= 3 && (got[2] - 0x2F).abs() <= 3,
        "picked {got:?}"
    );
    assert_eq!(s.colour_name, "Picked");
    // the paper
    click(&mut e, &mut s, (300.0, 250.0));
    assert!(s.colour[0] > 0.95 && s.colour[2] > 0.9, "{:?}", s.colour);
}

#[test]
fn select_and_lasso_make_selections_shift_adds_alt_takes_away() {
    let (mut e, mut s) = engine();
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (20.0, 30.0), (120.0, 90.0));
    let n = |e: &Engine| {
        e.painter
            .selection_mask()
            .iter()
            .filter(|&&v| v > 0)
            .count()
    };
    assert_eq!(n(&e), 100 * 60);
    assert_eq!(s.tooling.selection, Some([20.0, 30.0, 120.0, 90.0]));
    assert_eq!(s.tooling.outline.len(), 1);
    // Shift adds
    s.tooling.shift = true;
    drag(&mut e, &mut s, (200.0, 200.0), (210.0, 210.0));
    s.tooling.shift = false;
    assert_eq!(n(&e), 6000 + 100);
    assert_eq!(s.tooling.outline.len(), 2);
    // Alt takes away
    s.tooling.alt = true;
    drag(&mut e, &mut s, (20.0, 30.0), (70.0, 90.0));
    s.tooling.alt = false;
    assert_eq!(n(&e), 3000 + 100);
    // the lasso: a triangle
    s.tool = Tool::Lasso;
    e.pointer(&mut s, Phase::Down, at(100.0, 100.0));
    for p in [(200.0, 100.0), (100.0, 200.0)] {
        e.pointer(&mut s, Phase::Move, at(p.0, p.1));
    }
    e.pointer(&mut s, Phase::Up, at(100.0, 200.0));
    let t = n(&e);
    assert!((4800..=5200).contains(&t), "a triangle's pixels: {t}");
    assert!(
        s.tooling.lasso.is_empty(),
        "the lasso's path goes when it's done"
    );
    // a click deselects; Escape too
    s.tool = Tool::Select;
    click(&mut e, &mut s, (5.0, 5.0));
    assert!(!e.painter.has_selection() && s.tooling.outline.is_empty());
    other(&mut e, &mut s, "Select all");
    assert!(e.painter.has_selection());
    other(&mut e, &mut s, "Escape");
    assert!(!e.painter.has_selection() && s.tooling.selection.is_none());
}

#[test]
fn the_colour_workspaces_lasso_fills() {
    let (mut e, mut s) = engine();
    s.workspace = crate::state::Workspace::Colour;
    s.tool = Tool::Lasso;
    e.pointer(&mut s, Phase::Down, at(50.0, 50.0));
    for p in [(150.0, 50.0), (150.0, 150.0), (50.0, 150.0)] {
        e.pointer(&mut s, Phase::Move, at(p.0, p.1));
    }
    e.pointer(&mut s, Phase::Up, at(50.0, 150.0));
    let n = painted(&e, [40, 40, 160, 160]);
    assert!((9500..=10100).contains(&n), "{n}");
    assert!(!e.painter.has_selection());
}

/// The painted pixels' box on the active layer.
fn ink_box(e: &Engine) -> Option<[u32; 4]> {
    let (w, h) = (e.painter.width(), e.painter.height());
    let l = e.painter.layer_rgba(e.painter.active_layer());
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    for y in 0..h {
        for x in 0..w {
            if l[((y * w + x) * 4 + 3) as usize] > 128 {
                b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            }
        }
    }
    (b[0] != u32::MAX).then_some(b)
}

#[test]
fn transform_moves_scales_and_turns_the_selection() {
    let (mut e, mut s) = engine();
    block(&mut e, &mut s, [40.0, 40.0, 120.0, 100.0]);
    let b0 = ink_box(&e).unwrap();
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (30.0, 30.0), (130.0, 110.0));
    // picking Transform shows its box around the paint at once (nothing lifted yet);
    // pressing in it lifts, and dragging moves it (the GPU shows it moving)
    s.tool = Tool::Transform;
    other(&mut e, &mut s, "Transform");
    let shown = s.tooling.transform.expect("the box shows on picking");
    assert!(e.float.is_none() && !e.painter.transforming());
    drag(&mut e, &mut s, (80.0, 70.0), (180.0, 120.0));
    assert!(e.float.is_some());
    assert_eq!(
        s.tooling.transform.unwrap().rect,
        shown.rect,
        "the box shown on picking is the box lifted (around the paint, {b0:?})"
    );
    let m = s.tooling.transform.unwrap().m;
    assert_eq!(e.float.as_ref().unwrap().m, m);
    assert!(
        (m[4] - 100.0).abs() < 1e-3 && (m[5] - 50.0).abs() < 1e-3,
        "{m:?}"
    );
    // Enter keeps it (one undo step)
    other(&mut e, &mut s, "Transform: commit");
    assert!(e.float.is_none() && s.tooling.transform.is_none());
    let b1 = ink_box(&e).unwrap();
    assert_eq!(
        [b1[0] - b0[0], b1[1] - b0[1], b1[2] - b0[2], b1[3] - b0[3]],
        [100, 50, 100, 50],
        "moved by (100, 50): {b0:?} → {b1:?}"
    );
    // the selection went with it
    assert_eq!(s.tooling.selection, Some([130.0, 80.0, 230.0, 160.0]));
    assert!(e.painter.undo());
    assert_eq!(ink_box(&e).unwrap(), b0, "undo puts it back");

    // a corner handle scales it, from the opposite corner
    other(&mut e, &mut s, "Deselect");
    assert!(other(&mut e, &mut s, "Transform"));
    let bx = s.tooling.transform.unwrap();
    let br = bx.point(1, 1);
    let tl = bx.point(-1, -1);
    s.view.zoom = 1.0;
    drag(
        &mut e,
        &mut s,
        br,
        (tl.0 + (br.0 - tl.0) * 2.0, tl.1 + (br.1 - tl.1) * 2.0),
    );
    other(&mut e, &mut s, "Transform: commit");
    let b2 = ink_box(&e).unwrap();
    let (w0, h0) = ((b0[2] - b0[0]) as i32, (b0[3] - b0[1]) as i32);
    let (w2, h2) = ((b2[2] - b2[0]) as i32, (b2[3] - b2[1]) as i32);
    assert!(
        (w2 - 2 * w0).abs() <= 2 && (h2 - 2 * h0).abs() <= 2,
        "{b0:?} → {b2:?}"
    );
    assert!(
        (b2[0] as i32 - b0[0] as i32).abs() <= 1,
        "the far corner stayed"
    );
    e.painter.undo();

    // the turning handle: a quarter turn (Shift: 15° steps)
    assert!(other(&mut e, &mut s, "Transform"));
    let bx = s.tooling.transform.unwrap();
    let knob = bx.rotate_handle(ROTATE_ARM);
    let c = bx.point(0, 0);
    let r = (knob.1 - c.1).abs();
    s.tooling.shift = true;
    drag(&mut e, &mut s, knob, (c.0 + r, c.1 + 3.0));
    s.tooling.shift = false;
    other(&mut e, &mut s, "Transform: commit");
    let b3 = ink_box(&e).unwrap();
    let (w3, h3) = ((b3[2] - b3[0]) as i32, (b3[3] - b3[1]) as i32);
    assert!(
        (w3 - h0).abs() <= 2 && (h3 - w0).abs() <= 2,
        "turned a quarter: {b3:?}"
    );

    // Escape puts it back as it was
    let before = ink_box(&e);
    assert!(other(&mut e, &mut s, "Transform"));
    drag(&mut e, &mut s, c, (c.0 + 30.0, c.1));
    other(&mut e, &mut s, "Escape");
    assert!(e.float.is_none());
    assert_eq!(ink_box(&e), before);
}

#[test]
fn a_transform_drag_of_a_large_area_does_no_painting_per_step() {
    // the owner's freeze: moving a big painted area. Each pointer move now only moves the
    // GPU's picture; the painter draws once, when it's kept.
    let mut s = State::demo();
    let mut e = Engine::new(1920, 1080);
    block(&mut e, &mut s, [360.0, 140.0, 1560.0, 940.0]);
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (360.0, 140.0), (1560.0, 940.0));
    s.tool = Tool::Transform;
    let t = std::time::Instant::now();
    other(&mut e, &mut s, "Transform");
    let lift = t.elapsed();
    e.pointer(&mut s, Phase::Down, at(900.0, 500.0));
    let mut worst = std::time::Duration::ZERO;
    let t = std::time::Instant::now();
    for k in 1..=200 {
        let s0 = std::time::Instant::now();
        e.pointer(
            &mut s,
            Phase::Move,
            at(900.0 + k as f32 * 0.7, 500.0 + k as f32 * 0.3),
        );
        worst = worst.max(s0.elapsed());
    }
    let per = t.elapsed() / 200;
    e.pointer(&mut s, Phase::Up, at(1040.0, 560.0));
    let t = std::time::Instant::now();
    other(&mut e, &mut s, "Transform: commit");
    let keep = t.elapsed();
    eprintln!(
        "transform of 1200×800: lift {lift:?}, per pointer move {per:?} (worst {worst:?}), keep {keep:?}"
    );
    assert!(
        worst < std::time::Duration::from_millis(2),
        "{worst:?} per move"
    );
}

#[test]
fn flips_cut_copy_paste_clear_and_fill_selection() {
    let (mut e, mut s) = engine();
    // a block with a mark at its left
    block(&mut e, &mut s, [40.0, 40.0, 120.0, 100.0]);
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (30.0, 30.0), (130.0, 110.0));
    let b0 = ink_box(&e).unwrap();
    // flips stay in place for a symmetric block (its box) and take one undo step
    other(&mut e, &mut s, "Flip horizontally");
    assert_eq!(ink_box(&e).unwrap(), b0);
    assert!(e.painter.can_undo());
    // copy, clear, paste in place
    other(&mut e, &mut s, "Copy");
    assert!(s.tooling.has_clip);
    other(&mut e, &mut s, "Clear");
    assert!(ink_box(&e).is_none(), "cleared");
    other(&mut e, &mut s, "Paste in place");
    assert_eq!(ink_box(&e).unwrap(), b0, "pasted where it was");
    assert!(
        s.tooling.selection.is_some(),
        "the pasted pixels are selected"
    );
    // cut takes it away; Paste brings it back, ready to move
    other(&mut e, &mut s, "Cut");
    assert!(ink_box(&e).is_none());
    other(&mut e, &mut s, "Paste");
    assert_eq!(s.tool, Tool::Transform);
    assert!(s.tooling.transform.is_some() && e.float.is_some());
    assert!(ink_box(&e).is_none(), "floating: not on the layer yet");
    other(&mut e, &mut s, "Transform: commit");
    assert_eq!(ink_box(&e).unwrap(), b0);
    // fill selection
    other(&mut e, &mut s, "Deselect");
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (200.0, 200.0), (240.0, 230.0));
    other(&mut e, &mut s, "Fill selection");
    assert_eq!(painted(&e, [190, 190, 250, 240]), 40 * 30);
    // undo goes back step by step
    assert!(e.painter.undo());
    assert_eq!(painted(&e, [190, 190, 250, 240]), 0);
}

#[test]
fn shapes_draw_with_the_brush() {
    let (mut e, mut s) = engine();
    s.brush = 2;
    s.brush_size = 6.0;
    s.tool = Tool::Shape;
    drag(&mut e, &mut s, (40.0, 50.0), (300.0, 50.0));
    let line = painted(&e, [0, 40, 400, 60]);
    assert!(line > 250 * 3, "a line: {line}");
    assert_eq!(painted(&e, [0, 70, 400, 300]), 0);
    assert!(s.tooling.shape.is_none());
    s.tooling.shape_kind = ShapeKind::Rectangle;
    drag(&mut e, &mut s, (100.0, 100.0), (200.0, 200.0));
    // its edges, not its inside
    assert!(painted(&e, [95, 95, 205, 105]) > 300);
    assert!(painted(&e, [195, 95, 205, 205]) > 300);
    assert_eq!(painted(&e, [110, 110, 190, 190]), 0);
    s.tooling.shape_kind = ShapeKind::Ellipse;
    drag(&mut e, &mut s, (220.0, 100.0), (380.0, 260.0));
    assert!(painted(&e, [295, 95, 305, 106]) > 10, "its top");
    assert_eq!(painted(&e, [280, 160, 320, 200]), 0, "its middle");
    // one undo step each
    assert!(e.painter.undo());
    assert_eq!(painted(&e, [295, 95, 305, 106]), 0);
}

#[test]
fn text_types_onto_the_layer_and_keeps_as_one_step() {
    let (mut e, mut s) = engine();
    s.tool = Tool::Text;
    s.brush_size = 6.0; // 48 px text
    click(&mut e, &mut s, (40.0, 60.0));
    assert!(s.tooling.text.is_some());
    s.tooling.text.as_mut().unwrap().text = "Hi there".into();
    other(&mut e, &mut s, "Text: changed");
    let typed = painted(&e, [0, 0, 400, 300]);
    assert!(typed > 400, "the text shows while typing: {typed}");
    let t = s.tooling.text.clone().unwrap();
    assert!(
        t.caret.0 > 100.0,
        "the caret is after the text: {:?}",
        t.caret
    );
    // click away: kept
    click(&mut e, &mut s, (300.0, 250.0));
    assert_eq!(painted(&e, [0, 0, 400, 300]), typed);
    // the new text there, empty, goes when another tool is used
    s.tool = Tool::Brush;
    other(&mut e, &mut s, "Text: commit");
    assert!(e.painter.undo());
    assert_eq!(painted(&e, [0, 0, 400, 300]), 0, "one undo step");
    // Escape drops what's typed
    s.tool = Tool::Text;
    click(&mut e, &mut s, (40.0, 60.0));
    s.tooling.text.as_mut().unwrap().text = "Gone".into();
    other(&mut e, &mut s, "Text: changed");
    assert!(painted(&e, [0, 0, 400, 300]) > 0);
    other(&mut e, &mut s, "Escape");
    assert_eq!(painted(&e, [0, 0, 400, 300]), 0);
}

#[test]
fn canvas_menu_settings_symmetry_and_hints() {
    let (mut e, mut s) = engine();
    assert_eq!(s.tooling.menu_hint("Symmetry").unwrap(), "Off ›");
    other(&mut e, &mut s, "Symmetry");
    assert_eq!(s.tooling.symmetry, Sym::Vertical);
    // a stroke on the left is mirrored on the right
    s.brush = 2;
    s.brush_size = 8.0;
    e.down(&s, 40.0, 150.0, 1.0, (0.0, 0.0), 0);
    for i in 1..20 {
        e.moved(40.0 + i as f32 * 3.0, 150.0, 1.0, (0.0, 0.0), i * 8);
    }
    e.up();
    assert!(painted(&e, [300, 140, 370, 160]) > 100, "mirrored");
    other(&mut e, &mut s, "Symmetry: Off");
    assert_eq!(s.tooling.symmetry, Sym::Off);
    for item in ["Guides", "Snap to guides", "Record timelapse"] {
        let before = s.tooling.menu_hint(item);
        assert!(other(&mut e, &mut s, item));
        assert_ne!(s.tooling.menu_hint(item), before, "{item}");
    }
    assert!(s.tooling.grid && !s.tooling.snap && s.tooling.timelapse);
    // snapping to the grid
    s.tooling.snap = true;
    s.view.zoom = 1.0;
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (62.0, 66.0), (125.0, 131.0));
    assert_eq!(s.tooling.selection, Some([64.0, 64.0, 128.0, 128.0]));
    assert!(!other(&mut e, &mut s, "Something else"));
}

#[test]
fn brush_settings_change_what_the_brush_paints() {
    let mut s = State::demo();
    s.brush = 2;
    s.brush_size = 10.0;
    let paint = |s: &State| {
        let mut e = Engine::new(240, 80);
        let mut b = Engine::brush(s).unwrap();
        tune_brush(&mut b, s);
        e.painter.begin_stroke();
        for i in 0..60 {
            // a wobbly line
            let y = 40.0 + if i % 2 == 0 { 6.0 } else { -6.0 };
            e.painter
                .stroke_to(&b, 20.0 + i as f32 * 3.0, y, 0.3, 0.0, 0.0, 0.008);
        }
        e.painter.end_stroke(&b);
        e.painter.flush();
        let px = e.painter.rgba().to_vec();
        let dark: u64 = px.chunks(4).map(|p| 255 - p[0] as u64).sum();
        // how far the line strays from the middle
        let mut spread = 0u64;
        for (i, p) in px.chunks(4).enumerate() {
            let y = (i / 240) as i64;
            spread += (255 - p[0] as u64) * (y - 40).unsigned_abs();
        }
        (dark, spread as f64 / dark.max(1) as f64)
    };
    s.panel.steady = 0.0;
    let (full, wobbly) = paint(&s);
    s.panel.opacity = 40.0;
    let (light, _) = paint(&s);
    assert!(light < full * 6 / 10, "opacity 40 %: {light} vs {full}");
    s.panel.opacity = 100.0;
    s.panel.steady = 100.0;
    let (_, steady) = paint(&s);
    assert!(
        steady < wobbly * 0.7,
        "steady lines smooth the wobble: {steady} vs {wobbly}"
    );
    s.panel.steady = 0.0;
    // pressure changes size (the 2B pencil's does): at a light pressure, off paints wider
    s.brush = 0;
    let (thin, _) = paint(&s);
    s.tools.pressure_size.retain(|t| *t != Tool::Brush);
    let (wide, _) = paint(&s);
    assert!(
        wide > thin,
        "pressure off: full size at light pressure ({wide} vs {thin})"
    );
}

#[test]
fn moving_the_whole_layer_keeps_its_pixels_intact() {
    let (mut e, mut s) = engine();
    ring(&mut e, &mut s, [40.0, 40.0, 140.0, 120.0]);
    block(&mut e, &mut s, [60.0, 60.0, 90.0, 80.0]);
    let before = e.painter.layer_rgba(e.painter.active_layer());
    // (after a copy, clear and paste in place, as the keys do)
    for a in ["Select all", "Copy", "Clear", "Paste in place", "Escape"] {
        other(&mut e, &mut s, a);
    }
    s.tool = Tool::Transform;
    other(&mut e, &mut s, "Transform");
    drag(&mut e, &mut s, (90.0, 80.0), (190.0, 130.0));
    other(&mut e, &mut s, "Transform: commit");
    let after = e.painter.layer_rgba(e.painter.active_layer());
    let w = 400usize;
    let mut wrong = 0;
    for y in 0..250usize {
        for x in 0..300usize {
            let a = before[(y * w + x) * 4 + 3];
            let b = after[((y + 50) * w + x + 100) * 4 + 3];
            if (a as i32 - b as i32).abs() > 8 {
                wrong += 1;
            }
        }
    }
    assert_eq!(wrong, 0, "every pixel moved by (100, 50)");
}

/// The picture shown (the composite) agrees with the layer: every pixel the layer has
/// solid is dark on screen, every one it hasn't is paper.
fn picture_matches_layer(e: &mut Engine) -> usize {
    e.painter.flush();
    let l = e.painter.layer_rgba(e.painter.active_layer());
    let c = e.painter.rgba();
    l.chunks(4)
        .zip(c.chunks(4))
        .filter(|(l, c)| (l[3] > 200 && c[0] > 100) || (l[3] == 0 && c[0] < 200))
        .count()
}

#[test]
fn the_picture_follows_transforms_clears_and_undos() {
    let mut s = State::demo();
    let mut e = Engine::new(1920, 1080);
    block(&mut e, &mut s, [300.0, 200.0, 700.0, 500.0]);
    e.painter.flush();
    for a in ["Select all", "Copy", "Clear"] {
        other(&mut e, &mut s, a);
    }
    assert_eq!(picture_matches_layer(&mut e), 0, "after clear");
    e.painter.undo();
    assert_eq!(picture_matches_layer(&mut e), 0, "after undoing the clear");
    other(&mut e, &mut s, "Escape");
    s.tool = Tool::Transform;
    other(&mut e, &mut s, "Transform");
    e.painter.flush();
    drag(&mut e, &mut s, (500.0, 350.0), (800.0, 450.0));
    other(&mut e, &mut s, "Transform: commit");
    assert_eq!(picture_matches_layer(&mut e), 0, "after a transform");
}

#[test]
fn clear_then_undo_and_paste_restore_the_exact_layer() {
    let mut s = State::demo();
    let mut e = Engine::new(1920, 1080);
    block(&mut e, &mut s, [300.0, 200.0, 700.0, 500.0]);
    let l0 = e.painter.layer_rgba(e.painter.active_layer());
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (200.0, 100.0), (800.0, 600.0));
    other(&mut e, &mut s, "Clear");
    e.painter.undo();
    let l1 = e.painter.layer_rgba(e.painter.active_layer());
    let diff = |a: &[u8], b: &[u8]| {
        a.chunks(4)
            .zip(b.chunks(4))
            .filter(|(a, b)| (a[3] as i32 - b[3] as i32).abs() > 2)
            .count()
    };
    assert_eq!(diff(&l0, &l1), 0, "undoing a clear");
    for a in ["Copy", "Clear", "Paste in place"] {
        other(&mut e, &mut s, a);
    }
    let l2 = e.painter.layer_rgba(e.painter.active_layer());
    assert_eq!(diff(&l0, &l2), 0, "pasting in place");
}

#[test]
fn a_pasted_copy_moves_on_its_own_and_undo_takes_only_it() {
    let (mut e, mut s) = engine();
    block(&mut e, &mut s, [40.0, 40.0, 120.0, 100.0]);
    let original = e.painter.layer_rgba(e.painter.active_layer());
    let source = painted(&e, [40, 40, 120, 100]);
    let all = painted(&e, [0, 0, 400, 300]);
    s.tool = Tool::Select;
    drag(&mut e, &mut s, (30.0, 30.0), (130.0, 110.0));
    other(&mut e, &mut s, "Copy");
    other(&mut e, &mut s, "Paste");
    // the copy moves; the original stays
    drag(&mut e, &mut s, (80.0, 70.0), (280.0, 170.0));
    assert_eq!(
        painted(&e, [40, 40, 120, 100]),
        source,
        "the original stays while moving"
    );
    other(&mut e, &mut s, "Transform: commit");
    assert_eq!(
        painted(&e, [40, 40, 120, 100]),
        source,
        "the original stays"
    );
    let copy = painted(&e, [240, 140, 320, 200]);
    assert!(
        (copy as i64 - source as i64).abs() <= source as i64 / 50,
        "{copy} vs {source}"
    );
    // undo takes only the pasted copy
    assert!(e.painter.undo());
    assert_eq!(painted(&e, [240, 140, 320, 200]), 0);
    let now = e.painter.layer_rgba(e.painter.active_layer());
    let diff = original
        .chunks(4)
        .zip(now.chunks(4))
        .filter(|(a, b)| (a[3] as i32 - b[3] as i32).abs() > 2)
        .count();
    assert_eq!(diff, 0, "the layer as it was before the paste");
    // Escape lets go of a paste without putting it down
    other(&mut e, &mut s, "Paste");
    other(&mut e, &mut s, "Escape");
    assert!(e.float.is_none() && s.tooling.transform.is_none());
    assert_eq!(painted(&e, [0, 0, 400, 300]), all);
    // and a brush stroke while it floats puts it down first (the engine's rule)
    other(&mut e, &mut s, "Paste");
    s.tool = Tool::Brush;
    e.pointer(&mut s, Phase::Down, at(300.0, 250.0));
    e.pointer(&mut s, Phase::Up, at(310.0, 250.0));
    assert!(e.float.is_none());
    assert!(painted(&e, [40, 40, 120, 100]) >= source);
}
