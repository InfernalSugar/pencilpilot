use super::*;
use crate::state::{ColourStatus, Tool};

const W: u32 = 256;
const H: u32 = 128;

/// The demo's sheet (Clean: drawings 1–6 on frames 1–6, 7–12, 13–18…), its drawings
/// empty, on a small document.
fn plain() -> (Engine, State) {
    let mut s = State::demo();
    s.sheet.demo = false;
    s.frame = 0;
    s.onion = false;
    let mut e = Engine::new(W, H);
    tick(&mut e, &mut s, Instant::now());
    (e, s)
}

fn stroke(e: &mut Engine, s: &mut State, y: f32) {
    s.tool = Tool::Brush;
    s.brush_size = 8.0;
    e.down(s, 20.0, y, 0.9, (0.0, 0.0), 0);
    for i in 1..50 {
        e.moved(20.0 + i as f32 * 4.0, y, 0.9, (0.0, 0.0), i * 4);
    }
    e.up();
    tick(e, s, Instant::now());
}

fn go(e: &mut Engine, s: &mut State, f: u32) {
    assert!(apply(e, s, &Action::GoToFrame(f)));
    picture(e, s);
}

/// The picture as the canvas shows it.
fn shown(e: &mut Engine, s: &State) -> Vec<u8> {
    picture(e, s);
    if e.frames.onion_shown {
        e.frames.out.clone()
    } else {
        e.painter.rgba().to_vec()
    }
}

/// Pixels darker than the paper in rows y0..y1.
fn ink(px: &[u8], y0: u32, y1: u32) -> usize {
    px[(y0 * W * 4) as usize..(y1 * W * 4) as usize]
        .chunks(4)
        .filter(|p| p[0] < 200 && p[1] < 200)
        .count()
}

fn op(e: &mut Engine, s: &mut State, name: &str) {
    assert!(apply(e, s, &Action::Other(name.into())), "{name} is ours");
}

#[test]
fn the_painter_has_the_sheets_layers() {
    let (e, s) = plain();
    let names: Vec<&str> = e
        .painter
        .layers()
        .iter()
        .rev()
        .map(|l| l.props.name.as_str())
        .collect();
    assert_eq!(names, ["Clean", "Tie-down", "Rough", "Background"]);
    assert_eq!(s.layers.len(), 4);
    assert!(e.painter.layers()[0].props.locked, "Background is locked");
    assert_eq!(e.painter.active_layer(), s.sheet.tracks[0].layer);
    assert!(s.sheet.tracks.iter().all(|t| t.layer != 0));
    assert!(!e.painter.can_undo(), "setting up isn't an undo step");
}

#[test]
fn a_stroke_shows_on_its_drawings_frames_only() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 0);
    stroke(&mut e, &mut s, 30.0);
    let on1 = ink(&shown(&mut e, &s), 20, 40);
    assert!(on1 > 100, "painted on frame 1 ({on1} px)");
    // drawing 1 is held 6 frames: frame 4 shows it
    go(&mut e, &mut s, 3);
    assert_eq!(
        ink(&shown(&mut e, &s), 20, 40),
        on1,
        "held: frame 4 shows it"
    );
    // frame 7 is drawing 2: empty
    go(&mut e, &mut s, 6);
    assert_eq!(ink(&shown(&mut e, &s), 20, 40), 0, "frame 7 doesn't");
    // a stroke there is drawing 2's
    stroke(&mut e, &mut s, 90.0);
    assert!(ink(&shown(&mut e, &s), 80, 100) > 100);
    go(&mut e, &mut s, 1);
    let px = shown(&mut e, &s);
    assert_eq!(ink(&px, 20, 40), on1, "back on frame 2: drawing 1");
    assert_eq!(ink(&px, 80, 100), 0, "without drawing 2's stroke");
    // undo from another frame changes that drawing, not the one showing
    assert!(apply(&mut e, &mut s, &Action::NextFrame));
    assert_eq!(s.frame, 2);
    e.painter.undo();
    tick(&mut e, &mut s, Instant::now());
    assert_eq!(ink(&shown(&mut e, &s), 20, 40), on1);
    go(&mut e, &mut s, 6);
    assert_eq!(
        ink(&shown(&mut e, &s), 80, 100),
        0,
        "drawing 2's stroke undone"
    );
}

#[test]
fn a_new_drawing_goes_after_the_current_one_and_holds() {
    let (mut e, mut s) = plain();
    // frame 15: Clean's drawing 3 (frames 13–18)
    go(&mut e, &mut s, 14);
    assert!(apply(&mut e, &mut s, &Action::NewDrawing));
    assert_eq!(s.frame, 18, "on the new drawing, after drawing 3");
    let t = &s.sheet.tracks[0];
    assert_eq!(t.cels[3].hold, 6, "held as long as drawing 3");
    assert_eq!(t.first_frame_of(4), Some(24), "drawing 4 moved along");
    assert_eq!(s.frames, 54, "Clean (and so the cut) is 6 frames longer");
    assert_eq!(s.drawings, 7);
    stroke(&mut e, &mut s, 30.0);
    let n = ink(&shown(&mut e, &s), 20, 40);
    assert!(n > 100);
    for f in 18..24 {
        go(&mut e, &mut s, f);
        assert_eq!(
            ink(&shown(&mut e, &s), 20, 40),
            n,
            "held on frame {}",
            f + 1
        );
    }
    go(&mut e, &mut s, 24);
    assert_eq!(
        ink(&shown(&mut e, &s), 20, 40),
        0,
        "not on drawing 4's frames"
    );
    // duplicate: a copy, after it
    go(&mut e, &mut s, 18);
    op(&mut e, &mut s, ops::DUPLICATE_DRAWING);
    assert_eq!(s.frame, 24);
    assert_eq!(ink(&shown(&mut e, &s), 20, 40), n, "the copy has its lines");
    let (d1, d2) = (
        s.sheet.tracks[0].drawing_at(18),
        s.sheet.tracks[0].drawing_at(24),
    );
    assert_ne!(d1, d2, "its own drawing");
}

/// As the web app's New drawing (apps/studio TimelinePanel `addDrawing`): inside a hold,
/// the new drawing goes after the whole exposure, held as long; past the layer's end it
/// starts on the current frame, held one frame.
#[test]
fn a_new_drawing_inside_a_hold_follows_the_web_app() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 2); // frame 3, in drawing 1's six frames
    assert!(apply(&mut e, &mut s, &Action::NewDrawing));
    let t = &s.sheet.tracks[0];
    let holds: Vec<(u32, u32)> = t.cels.iter().map(|c| (c.drawing, c.hold)).take(3).collect();
    assert_eq!(holds[0], (1, 6), "drawing 1 keeps frames 1–6");
    assert_eq!(holds[1].1, 6, "the new one: frames 7–12");
    assert_eq!(holds[2], (2, 6), "drawing 2 moved to 13–18");
    assert_eq!(s.frame, 6);
    // past the end of a layer: on the current frame, one frame long
    s.active_layer = 0;
    let end = s.sheet.tracks[0].end();
    go(&mut e, &mut s, end - 1);
    assert!(apply(&mut e, &mut s, &Action::NewLayer));
    go(&mut e, &mut s, 10);
    assert!(apply(&mut e, &mut s, &Action::NewDrawing));
    let t = &s.sheet.tracks[0];
    assert_eq!(t.drawing_at(9), 0);
    assert_ne!(t.drawing_at(10), 0);
    assert_eq!(t.end(), 11, "one frame");
}

#[test]
fn painting_a_blank_frame_starts_a_drawing_there() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 14);
    op(&mut e, &mut s, ops::BLANK_FRAME);
    assert_eq!(s.frame, 18);
    assert_eq!(s.sheet.tracks[0].drawing_at(18), 0, "a blank");
    assert_eq!(ink(&shown(&mut e, &s), 0, H), 0);
    stroke(&mut e, &mut s, 30.0);
    let d = s.sheet.tracks[0].drawing_at(18);
    assert_ne!(d, 0, "the stroke started a drawing on the blank");
    assert_eq!(s.sheet.tracks[0].drawing_at(19), d, "held as the blank was");
    assert!(ink(&shown(&mut e, &s), 20, 40) > 100);
    go(&mut e, &mut s, 14);
    assert_eq!(ink(&shown(&mut e, &s), 20, 40), 0);
}

#[test]
fn onion_skin_tints_the_neighbours_under_the_drawing() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 0);
    stroke(&mut e, &mut s, 30.0); // drawing 1
    go(&mut e, &mut s, 12);
    stroke(&mut e, &mut s, 100.0); // drawing 3
    go(&mut e, &mut s, 6); // drawing 2: before is 1, after is 3
    s.onion = true;
    s.strips.onion_range = 0;
    let px = shown(&mut e, &s);
    let at = |px: &[u8], y: u32| {
        let i = ((y * W + 100) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let before = at(&px, 30);
    let after = at(&px, 100);
    assert!(
        before[0] > before[1] + 30,
        "the drawing before, in red: {before:?}"
    );
    assert!(
        after[1] > after[0] + 20,
        "the drawing after, in green: {after:?}"
    );
    // a stroke on drawing 2 over the onion stays dark (the onion is under it)
    s.tool = Tool::Brush;
    e.down(&s, 20.0, 30.0, 0.9, (0.0, 0.0), 0);
    for i in 1..50 {
        e.moved(20.0 + i as f32 * 4.0, 30.0, 0.9, (0.0, 0.0), i * 4);
    }
    let (_, r) = picture(&mut e, &s).expect("the stroke shows");
    assert!(r[3] <= H / 2, "the stroke's rectangle only: {r:?}");
    e.up();
    tick(&mut e, &mut s, Instant::now());
    let px = shown(&mut e, &s);
    let line = at(&px, 30);
    assert!(
        line[0] + 60 < before[0] && line[1] < before[1],
        "the line is on top, darker than the onion alone: {line:?} over {before:?}"
    );
    // off: plain again
    s.onion = false;
    let px = shown(&mut e, &s);
    assert_eq!(ink(&px, 95, 105), 0, "no onion without onion skin");
    // the range: 2 before
    s.onion = true;
    s.strips.onion_range = 1;
    go(&mut e, &mut s, 14);
    let entries = onion_entries(&s);
    assert_eq!(entries.len() / 3, 3, "two before, one after: {entries:?}");
}

#[test]
fn hidden_layers_leave_the_picture_and_undo_shows_them() {
    let (mut e, mut s) = plain();
    stroke(&mut e, &mut s, 30.0);
    assert!(ink(&shown(&mut e, &s), 20, 40) > 100);
    s.layers[0].visible = false;
    tick(&mut e, &mut s, Instant::now());
    assert_eq!(
        ink(&shown(&mut e, &s), 20, 40),
        0,
        "hidden: not in the picture"
    );
    // undo (the painter's) shows it again, and the interface follows
    assert!(e.painter.undo());
    tick(&mut e, &mut s, Instant::now());
    assert!(s.layers[0].visible);
    assert!(ink(&shown(&mut e, &s), 20, 40) > 100);
    // a locked layer takes no paint
    s.layers[0].locked = true;
    tick(&mut e, &mut s, Instant::now());
    go(&mut e, &mut s, 6);
    stroke(&mut e, &mut s, 90.0);
    assert_eq!(ink(&shown(&mut e, &s), 80, 100), 0, "locked");
}

#[test]
fn layers_are_added_duplicated_moved_and_deleted() {
    let (mut e, mut s) = plain();
    stroke(&mut e, &mut s, 30.0);
    assert!(apply(&mut e, &mut s, &Action::NewLayer));
    assert_eq!(e.painter.layers().len(), 5, "a painter layer");
    assert_eq!(s.layers.len(), 5);
    assert_eq!(s.active_layer, 0, "the new layer, above Clean, is active");
    assert_eq!(s.layers[1].name, "Clean");
    assert_eq!(e.painter.active_layer(), s.sheet.tracks[0].layer);
    // strokes go to it: a drawing on its blank frame
    stroke(&mut e, &mut s, 90.0);
    assert_ne!(s.sheet.tracks[0].drawing_at(0), 0);
    // undo twice: the stroke, then the layer
    e.painter.undo();
    e.painter.undo();
    tick(&mut e, &mut s, Instant::now());
    assert_eq!(s.layers.len(), 4, "undo took the layer away");
    e.painter.redo();
    tick(&mut e, &mut s, Instant::now());
    assert_eq!(s.layers.len(), 5, "redo brought it back");
    assert_ne!(s.sheet.tracks[0].drawing_at(0), 0, "with its cels");
    // move Clean (index 1) up, above the new layer
    s.active_layer = 1;
    op(&mut e, &mut s, ops::LAYER_UP);
    assert_eq!(s.layers[0].name, "Clean");
    assert_eq!(s.active_layer, 0, "still Clean");
    // duplicate Clean: its own copies of the drawings
    op(&mut e, &mut s, ops::DUPLICATE_LAYER);
    assert_eq!(s.layers.len(), 6);
    assert_eq!(s.active_layer, 0, "the copy is active");
    assert_eq!(s.layers[0].name, "Clean copy");
    let (a, b) = (&s.sheet.tracks[0], &s.sheet.tracks[1]);
    assert_eq!(a.cels.len(), b.cels.len());
    assert!(
        a.cels
            .iter()
            .zip(&b.cels)
            .all(|(x, y)| x.drawing != y.drawing && x.hold == y.hold)
    );
    // its frame-1 drawing has the stroke
    let d = a.drawing_at(0);
    assert!(!e.painter.drawing_empty(d));
    // delete it
    op(&mut e, &mut s, ops::DELETE_LAYER);
    assert_eq!(s.layers.len(), 5);
    assert_eq!(s.layers[0].name, "Clean");
}

#[test]
fn hold_mark_and_frame_edits_change_the_sheet() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 14); // Clean's drawing 3 (13–18)
    op(&mut e, &mut s, ops::HOLD[0]);
    assert_eq!(s.sheet.tracks[0].cels[2].hold, 1);
    assert_eq!(s.frames, 48, "Tie-down and Rough are 48 long");
    assert_eq!(s.sheet.tracks[0].end(), 43);
    assert_eq!(
        s.sheet.tracks[0].drawing_at(14),
        4,
        "frame 15 is drawing 4 now"
    );
    op(&mut e, &mut s, ops::KEY);
    assert_eq!(s.sheet.tracks[0].cels[3].mark, Mark::Key);
    op(&mut e, &mut s, ops::IN_BETWEEN);
    assert_eq!(s.sheet.tracks[0].cels[3].mark, Mark::InBetween);
    op(&mut e, &mut s, ops::CLEAR_MARK);
    assert_eq!(s.sheet.tracks[0].cels[3].mark, Mark::None);
    op(&mut e, &mut s, ops::INSERT_FRAME);
    assert_eq!(s.sheet.tracks[0].cels[3].hold, 7);
    op(&mut e, &mut s, ops::REMOVE_FRAME);
    op(&mut e, &mut s, ops::REMOVE_FRAME);
    assert_eq!(s.sheet.tracks[0].cels[3].hold, 5);
    op(&mut e, &mut s, ops::EXTEND_HOLD);
    assert_eq!(
        s.sheet.tracks[0].cels[3].hold, 6,
        "no blank after: one more"
    );
    op(&mut e, &mut s, ops::REVERSE);
    assert_eq!(s.sheet.tracks[0].cels[0].drawing, 6);
    op(&mut e, &mut s, ops::ALL_ON[1]);
    assert!(s.sheet.tracks[0].cels.iter().all(|c| c.hold == 2));
    assert_eq!(s.frames, 48);
    s.frame = 0;
    op(&mut e, &mut s, ops::REMOVE_DRAWING);
    assert_eq!(s.sheet.tracks[0].cels.len(), 5);
    // copy and paste: on another layer, a copy of the drawing
    go(&mut e, &mut s, 0);
    op(&mut e, &mut s, ops::COPY_FRAMES);
    s.active_layer = 1;
    op(&mut e, &mut s, ops::PASTE_FRAMES);
    assert_eq!(s.sheet.tracks[1].cels.len(), 4);
    let pasted = s.sheet.tracks[1].cels[1].drawing;
    assert!(!s.sheet.tracks[0].drawings().contains(&pasted));
    // the loop range
    go(&mut e, &mut s, 4);
    op(&mut e, &mut s, ops::LOOP_FROM);
    go(&mut e, &mut s, 9);
    op(&mut e, &mut s, ops::LOOP_TO);
    assert_eq!(s.sheet.loop_range, Some((4, 9)));
    // previous and next drawing on the active layer (Tie-down: 1–12, the paste, …)
    go(&mut e, &mut s, 0);
    assert!(apply(&mut e, &mut s, &Action::NextDrawing));
    assert_eq!(s.frame, 12);
    assert!(apply(&mut e, &mut s, &Action::PreviousDrawing));
    assert_eq!(s.frame, 0);
}

#[test]
fn boiling_a_held_drawing_alternates_copies_on_twos() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 0);
    stroke(&mut e, &mut s, 30.0);
    op(&mut e, &mut s, ops::BOIL);
    let t = &s.sheet.tracks[0];
    let first: Vec<(u32, u32)> = t.cels[..3].iter().map(|c| (c.drawing, c.hold)).collect();
    assert_eq!(first[0].0, 1);
    assert!(first.iter().all(|c| c.1 == 2), "{first:?}");
    assert_eq!(s.frames, 48, "the same length");
    assert!(!e.painter.drawing_empty(first[1].0));
}

#[test]
fn playback_advances_at_the_frame_rate_and_loops() {
    let (mut e, mut s) = plain();
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    s.frame = 44;
    assert!(apply(&mut e, &mut s, &Action::Play));
    assert!(s.playing);
    tick(&mut e, &mut s, at(0));
    assert_eq!(s.frame, 44);
    // the next frame is due 1/24 s on
    let due = next_tick_at(&e, &s).unwrap();
    assert_eq!(due.duration_since(t0).as_micros(), 41_666);
    tick(&mut e, &mut s, at(40));
    assert_eq!(s.frame, 44, "not yet");
    assert!(tick(&mut e, &mut s, at(42)));
    assert_eq!(s.frame, 45);
    // a late loop skips ahead (no drift): 100 ms is frame +2
    tick(&mut e, &mut s, at(100));
    assert_eq!(s.frame, 46);
    // looping the cut: 48 frames, so 4 on from 44 is frame 0
    tick(&mut e, &mut s, at(167));
    assert_eq!(s.frame, 0, "looped");
    // a second of frames: 24 on
    let mut shown = 0;
    let mut last = s.frame;
    for ms in (170..=1170).step_by(5) {
        tick(&mut e, &mut s, at(ms));
        if s.frame != last {
            shown += 1;
            last = s.frame;
        }
    }
    assert_eq!(shown, 24, "24 frames in a second");
    assert!(apply(&mut e, &mut s, &Action::Stop));
    assert!(!s.playing);
    tick(&mut e, &mut s, at(1200));
    assert_eq!(next_tick_at(&e, &s), None, "idle when stopped");
    // once: stops on the last frame
    s.strips.play_mode = 0;
    s.frame = 46;
    apply(&mut e, &mut s, &Action::Play);
    tick(&mut e, &mut s, at(2000));
    tick(&mut e, &mut s, at(2200));
    assert_eq!(s.frame, 47);
    assert!(!s.playing, "once: stopped at the end");
    // ping-pong in the loop range 5–7
    s.strips.play_mode = 2;
    s.sheet.loop_range = Some((4, 6));
    apply(&mut e, &mut s, &Action::Play);
    tick(&mut e, &mut s, at(3000));
    let mut seen = vec![s.frame];
    for k in 1..7 {
        tick(&mut e, &mut s, at(3000 + k * 1000 / 24 + 1));
        seen.push(s.frame);
    }
    assert_eq!(seen, [4, 5, 6, 5, 4, 5, 6]);
    // 12 fps: twice as long a frame
    s.fps = 12;
    tick(&mut e, &mut s, at(5000));
    let f0 = s.frame;
    tick(&mut e, &mut s, at(5060));
    assert_eq!(s.frame, f0, "not after 60 ms at 12 fps");
    tick(&mut e, &mut s, at(5084));
    assert_ne!(s.frame, f0);
}

#[test]
fn playback_shows_each_frames_drawings() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 0);
    stroke(&mut e, &mut s, 30.0);
    go(&mut e, &mut s, 5);
    let t0 = Instant::now();
    apply(&mut e, &mut s, &Action::Play);
    tick(&mut e, &mut s, t0);
    assert!(ink(&shown(&mut e, &s), 20, 40) > 100, "frame 6: drawing 1");
    tick(&mut e, &mut s, t0 + Duration::from_millis(42));
    assert_eq!(s.frame, 6);
    let r = picture(&mut e, &s);
    assert!(r.is_some(), "the frame change goes up");
    assert_eq!(ink(e.painter.rgba(), 20, 40), 0, "frame 7: drawing 2");
}

#[test]
fn a_stroke_uploads_only_its_rectangle() {
    let (mut e, s) = plain();
    let (_, r) = picture(&mut e, &s).expect("the first picture");
    assert_eq!(r, [0, 0, W, H], "the whole document first");
    assert!(picture(&mut e, &s).is_none(), "then nothing");
    e.down(&s, 20.0, 20.0, 0.9, (0.0, 0.0), 0);
    e.moved(40.0, 24.0, 0.9, (0.0, 0.0), 8);
    let (_, r) = picture(&mut e, &s).expect("the stroke");
    assert!(r[2] * r[3] < W * H / 4, "a small rectangle: {r:?}");
    e.up();
}

#[test]
fn opacity_drags_are_one_undo_step() {
    let (mut e, mut s) = plain();
    let t0 = Instant::now();
    let id = s.sheet.tracks[0].layer;
    let op = |e: &Engine| {
        e.painter
            .layers()
            .iter()
            .find(|l| l.id == id)
            .unwrap()
            .props
            .opacity
    };
    for v in [0.8, 0.6, 0.5] {
        s.sheet.tracks[0].opacity = v;
        tick(&mut e, &mut s, t0);
    }
    assert_eq!(op(&e), 0.5);
    assert!(!e.painter.can_undo(), "not recorded while it moves");
    assert!(next_tick_at(&e, &s).is_some());
    tick(&mut e, &mut s, Instant::now() + Duration::from_millis(400));
    assert!(e.painter.can_undo(), "recorded once it rests");
    e.painter.undo();
    tick(&mut e, &mut s, Instant::now());
    assert_eq!(op(&e), 1.0);
    assert_eq!(s.sheet.tracks[0].opacity, 1.0, "the slider follows undo");
}

#[test]
fn thumbnails_follow_the_drawings() {
    let (mut e, mut s) = plain();
    go(&mut e, &mut s, 0);
    stroke(&mut e, &mut s, 30.0);
    picture(&mut e, &s);
    assert!(next_tick_at(&e, &s).is_some_and(|t| t <= Instant::now()));
    tick(&mut e, &mut s, Instant::now());
    let t = s.sheet.thumbs.get(&1).expect("drawing 1's thumbnail");
    let painted = t.px.chunks(4).filter(|p| p[3] > 40).count();
    assert!(painted > 10, "the line shows in it ({painted} px)");
    // its colour status shows for the active layer's drawings
    s.sheet.status.insert(2, ColourStatus::Done);
    derive(&mut s);
    assert_eq!(s.to_colour[1], ColourStatus::Done);
    // idle: nothing due
    tick(&mut e, &mut s, Instant::now());
    picture(&mut e, &s);
    assert_eq!(next_tick_at(&e, &s), None);
}

#[test]
fn the_demo_paints_its_drawings() {
    let mut s = State::demo();
    let mut e = Engine::new(1920, 1080);
    let started = Instant::now();
    tick(&mut e, &mut s, Instant::now());
    let took = started.elapsed();
    for t in &s.sheet.tracks {
        for d in t.drawings() {
            assert!(!e.painter.drawing_empty(d), "drawing {d} has lines");
        }
    }
    assert_eq!(s.frame, 14);
    let t = Instant::now();
    assert!(picture(&mut e, &s).is_some());
    eprintln!(
        "demo set up in {took:?}, first picture (with onion skin) in {:?}",
        t.elapsed()
    );
    // a frame change with the onion skin
    let t = Instant::now();
    apply(&mut e, &mut s, &Action::GoToFrame(18));
    assert!(picture(&mut e, &s).is_some());
    eprintln!(
        "a new drawing's picture, onion skin and all: {:?}",
        t.elapsed()
    );
}

#[test]
#[ignore = "timing, release: cargo test --release -p pencilpilot-native -- --ignored onion_timing --nocapture"]
fn onion_timing() {
    let mut s = State::demo();
    let mut e = Engine::new(1920, 1080);
    tick(&mut e, &mut s, Instant::now());
    picture(&mut e, &s);
    let mut times = Vec::new();
    let mut parts = Vec::new();
    for k in 0..96u32 {
        let f = (k * 7) % 48;
        let t = Instant::now();
        apply(&mut e, &mut s, &Action::GoToFrame(f));
        picture(&mut e, &s);
        times.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let ent = onion_entries(&s);
        let _ = e.painter.onion_rgba(&ent, 1);
        parts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    parts.sort_by(f64::total_cmp);
    let p = |v: &[f64], q: f64| v[((v.len() - 1) as f64 * q) as usize];
    eprintln!(
        "frame change + picture with onion: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; onion_rgba alone p50 {:.2} p95 {:.2}",
        p(&times, 0.5),
        p(&times, 0.95),
        p(&times, 1.0),
        p(&parts, 0.5),
        p(&parts, 0.95)
    );
}
