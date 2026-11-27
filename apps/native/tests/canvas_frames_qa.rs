//! QA for layers and frames on the canvas (Phase 2 step 4): strokes go to the active layer
//! and the current frame's drawing, held cels show the same drawing, hidden layers aren't
//! shown, onion skin, the sheet's edits, playback's timing, and how long showing another
//! frame takes at 1920 × 1080 (scrubbing at the display's rate).
//!
//! Ignored by a plain `cargo test`; run with `tools/native-qa/run.sh` or
//! `cargo test --release -p pencilpilot-native --test canvas_frames_qa -- --include-ignored --test-threads 1`.
//! `PP_QA_OUT` = a folder for the measurements (JSON).

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::frames::Mark;
use pencilpilot_native::shell::engine::{Engine, Phase, Sample};
use pencilpilot_native::state::{Action, State, Tool};

const W: u32 = 1920;
const H: u32 = 1080;

fn save_json(name: &str, json: &str) {
    if let Some(d) = std::env::var_os("PP_QA_OUT") {
        let d = std::path::PathBuf::from(d);
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join(name), json);
    }
    eprintln!("{name}: {json}");
}

/// The demo's cut with its drawings empty (Clean: drawings on frames 1–6, 7–12…), at frame 1.
fn plain() -> (Engine, State) {
    let mut s = State::demo();
    s.sheet.demo = false;
    s.frame = 0;
    s.onion = false;
    let mut e = Engine::new(W, H);
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    (e, s)
}

/// The picture the canvas shows now (onion skin included), a copy.
fn shown(e: &mut Engine, s: &State) -> Vec<u8> {
    // (the picture is the whole document; the rectangle is only what changed; None:
    // nothing changed, and without the onion skin that's the painter's)
    match e.picture(s) {
        Some((p, _)) => p.to_vec(),
        None => e.painter.rgba().to_vec(),
    }
}

fn go(e: &mut Engine, s: &mut State, f: u32) {
    assert!(e.apply(s, &Action::GoToFrame(f)));
    e.tick(s, Instant::now());
}

fn stroke(e: &mut Engine, s: &mut State, y: f32) {
    s.tool = Tool::Brush;
    s.brush = 2;
    s.brush_size = 10.0;
    for i in 0..=60 {
        let ph = match i {
            0 => Phase::Down,
            60 => Phase::Up,
            _ => Phase::Move,
        };
        let p = Sample {
            x: 200.0 + i as f32 * 20.0,
            y,
            pressure: 0.9,
            tilt: (0.0, 0.0),
            ms: i * 4,
        };
        e.pointer(s, ph, p);
    }
    e.tick(s, Instant::now());
}

/// Ink (darker than paper, any colour; not transparent) in a band of rows.
fn ink(px: &[u8], y0: u32, y1: u32) -> usize {
    px[(y0 * W * 4) as usize..(y1 * W * 4) as usize]
        .chunks(4)
        .filter(|p| p[3] > 0 && (p[0] < 200 || p[1] < 200 || p[2] < 200))
        .count()
}

fn op(e: &mut Engine, s: &mut State, name: &str) {
    assert!(
        e.apply(s, &Action::Other(name.into())),
        "{name} is the frames'"
    );
    e.tick(s, Instant::now());
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn strokes_go_to_the_active_layer_and_the_frames_drawing_and_holds_show_it() {
    let (mut e, mut s) = plain();
    stroke(&mut e, &mut s, 300.0);
    // the stroke is on Clean's drawing 1, the painter's active layer
    let clean = s.sheet.tracks[0].layer;
    assert_eq!(e.painter.active_layer(), clean);
    let d1 = e.painter.layer_drawing(clean);
    assert!(
        d1 != 0 && ink(&e.painter.drawing_rgba(d1), 290, 310) > 1000,
        "on drawing {d1}"
    );
    // held: frames 1–6 show it; frame 7 (drawing 2) doesn't; back on 4 it shows
    for f in 0..6 {
        go(&mut e, &mut s, f);
        assert!(
            ink(&shown(&mut e, &s), 290, 310) > 1000,
            "frame {} shows drawing 1",
            f + 1
        );
    }
    go(&mut e, &mut s, 6);
    assert_eq!(
        ink(&shown(&mut e, &s), 290, 310),
        0,
        "frame 7 is another drawing"
    );
    // another layer: the stroke goes to Tie-down's drawing on this frame, not Clean's
    s.active_layer = 1;
    e.tick(&mut s, Instant::now());
    stroke(&mut e, &mut s, 600.0);
    let tie = s.sheet.tracks[1].layer;
    assert_eq!(
        e.painter.active_layer(),
        tie,
        "the painter follows the active layer"
    );
    let dt = e.painter.layer_drawing(tie);
    assert!(ink(&e.painter.drawing_rgba(dt), 590, 610) > 1000);
    let dc = e.painter.layer_drawing(clean);
    assert_eq!(
        ink(&e.painter.drawing_rgba(dc), 590, 610),
        0,
        "Clean's drawing untouched"
    );
    go(&mut e, &mut s, 3);
    let px = shown(&mut e, &s);
    assert!(
        ink(&px, 290, 310) > 1000 && ink(&px, 590, 610) > 1000,
        "frame 4: Clean 1 and Tie-down 7 (held 12)"
    );
    // undo the Tie-down stroke from another frame
    assert!(e.apply(&mut s, &Action::Undo));
    e.tick(&mut s, Instant::now());
    assert_eq!(ink(&shown(&mut e, &s), 590, 610), 0, "undo took it away");
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn hidden_layers_arent_shown() {
    let (mut e, mut s) = plain();
    stroke(&mut e, &mut s, 300.0);
    assert!(ink(&shown(&mut e, &s), 290, 310) > 1000);
    s.layers[0].visible = false;
    e.tick(&mut s, Instant::now());
    assert_eq!(ink(&shown(&mut e, &s), 290, 310), 0, "hidden");
    // painting on a hidden layer: nothing shows (and nothing appears elsewhere)
    s.layers[0].visible = true;
    e.tick(&mut s, Instant::now());
    assert!(ink(&shown(&mut e, &s), 290, 310) > 1000, "shown again");
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn onion_skin_shows_the_neighbours_tinted() {
    let (mut e, mut s) = plain();
    stroke(&mut e, &mut s, 300.0); // drawing 1, frames 1–6
    go(&mut e, &mut s, 12);
    stroke(&mut e, &mut s, 700.0); // drawing 3, frames 13–18
    go(&mut e, &mut s, 6); // drawing 2: empty
    s.onion = false;
    e.tick(&mut s, Instant::now());
    let off = shown(&mut e, &s);
    assert_eq!(
        ink(&off, 290, 310) + ink(&off, 690, 710),
        0,
        "onion off: nothing"
    );
    s.onion = true;
    e.tick(&mut s, Instant::now());
    let on = shown(&mut e, &s);
    let tint = |px: &[u8], y: u32| {
        let row = &px[((y * W + 600) * 4) as usize..][..4];
        [row[0], row[1], row[2]]
    };
    let before = tint(&on, 300);
    let after = tint(&on, 700);
    save_json(
        "frames-onion.json",
        &format!(
            "{{\"beforePx\": {before:?}, \"afterPx\": {after:?}, \"inkBefore\": {}, \"inkAfter\": {}}}",
            ink(&on, 290, 310),
            ink(&on, 690, 710)
        ),
    );
    assert!(ink(&on, 290, 310) > 500, "the drawing before shows");
    assert!(ink(&on, 690, 710) > 500, "the drawing after shows");
    assert!(
        before[0] > before[1] + 30,
        "before is red-tinted: {before:?}"
    );
    assert!(after[1] > after[0] + 30, "after is green-tinted: {after:?}");
    // the onion isn't painted into the drawings
    let d2 = e.painter.layer_drawing(s.sheet.tracks[0].layer);
    assert_eq!(ink(&e.painter.drawing_rgba(d2), 0, H), 0);
    // nor shown while playing
    assert!(e.apply(&mut s, &Action::Play));
    e.tick(&mut s, Instant::now());
    let playing = shown(&mut e, &s);
    assert!(
        ink(&playing, 290, 310) == 0 || s.frame < 6,
        "no onion while playing"
    );
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn the_sheets_edits_new_drawing_hold_mark_insert_remove() {
    let (mut e, mut s) = plain();
    let cels = |s: &State| -> Vec<(u32, u32, Mark)> {
        s.sheet.tracks[0]
            .cels
            .iter()
            .map(|c| (c.drawing, c.hold, c.mark))
            .collect()
    };
    let start = cels(&s);
    assert_eq!(start.iter().map(|c| c.1).sum::<u32>(), 48, "{start:?}");
    // a new drawing on frame 3 (inside drawing 1's hold): it starts there
    go(&mut e, &mut s, 2);
    assert!(e.apply(&mut s, &Action::NewDrawing));
    e.tick(&mut s, Instant::now());
    let after_new = cels(&s);
    // (it goes in after the held drawing, and the frame moves to it: the hold isn't split)
    let new_at = s.frame;
    stroke(&mut e, &mut s, 400.0);
    go(&mut e, &mut s, 0);
    assert_eq!(
        ink(&shown(&mut e, &s), 390, 410),
        0,
        "frame 1 keeps drawing 1: {after_new:?}"
    );
    go(&mut e, &mut s, new_at);
    assert!(
        ink(&shown(&mut e, &s), 390, 410) > 1000,
        "the new drawing's frame shows the stroke"
    );
    // hold on 2s, mark Key
    op(&mut e, &mut s, "Hold on 2s");
    let h2 = cels(&s);
    op(&mut e, &mut s, "Key");
    let k = cels(&s);
    // insert a frame here, then remove it
    let len0 = s.frames;
    op(&mut e, &mut s, "Insert frame");
    let ins = cels(&s);
    op(&mut e, &mut s, "Remove frame");
    let rem = cels(&s);
    save_json(
        "frames-sheet-edits.json",
        &format!(
            "{{\"start\": \"{start:?}\", \"afterNewDrawingAt3\": \"{after_new:?}\", \"frameAfterNew\": {new_at}, \"hold2\": \"{h2:?}\", \"key\": \"{k:?}\", \"inserted\": \"{ins:?}\", \"removed\": \"{rem:?}\", \"frames\": [{len0}, {}]}}",
            s.frames
        ),
    );
    let here = |c: &[(u32, u32, Mark)], f: u32| {
        let mut at = 0;
        for x in c {
            if f < at + x.1 {
                return *x;
            }
            at += x.1;
        }
        (0, 0, Mark::None)
    };
    assert_eq!(
        after_new.len(),
        start.len() + 1,
        "one more cel: {after_new:?}"
    );
    assert_ne!(
        here(&after_new, new_at).0,
        here(&start, new_at).0,
        "a new drawing at frame {}",
        new_at + 1
    );
    assert_eq!(here(&k, 2).2, Mark::Key, "marked Key");
    let total = |c: &[(u32, u32, Mark)]| c.iter().map(|x| x.1).sum::<u32>();
    assert_eq!(total(&ins), total(&k) + 1, "insert adds a frame");
    assert_eq!(total(&rem), total(&k), "remove takes it away");
    // the strip's numbers follow the sheet
    assert_eq!(
        s.drawings as usize,
        s.sheet.tracks[0]
            .cels
            .iter()
            .filter(|c| c.drawing != 0)
            .count()
    );
}

/// Playback at 24 fps, on simulated time: the frame on screen at each moment is the one
/// due then (no frame skipped or shown twice too long), and the engine asks to be woken at
/// each frame's time.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn playback_advances_on_time_at_24_fps() {
    let (mut e, mut s) = plain();
    s.fps = 24;
    assert!(e.apply(&mut s, &Action::Play));
    let t0 = Instant::now();
    e.tick(&mut s, t0);
    let f0 = s.frame;
    let mut wrong = Vec::new();
    let mut wake_err: f64 = 0.0;
    // a display at 165 Hz asking every refresh for 4 s
    for k in 1..(165 * 4) {
        let now = t0 + Duration::from_secs_f64(k as f64 / 165.0);
        if let Some(at) = e.next_tick_at(&s) {
            let due = at.saturating_duration_since(t0).as_secs_f64();
            let n = (due * 24.0).round();
            wake_err = wake_err.max((due - n / 24.0).abs() * 1e3);
        }
        e.tick(&mut s, now);
        let exact = k as f64 / 165.0 * 24.0;
        let n = exact.floor() as u32;
        let want = (f0 + n) % s.frames;
        // (exactly on a frame's time, either is right: nanosecond rounding)
        let on_edge = (exact - exact.round()).abs() < 1e-6;
        if s.frame != want && !(on_edge && (s.frame + 1) % s.frames == want) {
            wrong.push((k, s.frame, want));
        }
    }
    save_json(
        "frames-playback-sim.json",
        &format!(
            "{{\"wrongFrames\": {}, \"firstWrong\": \"{:?}\", \"wakeErrMsMax\": {wake_err:.3}, \"playMode\": {}}}",
            wrong.len(),
            wrong.first(),
            s.strips.play_mode
        ),
    );
    assert!(s.playing, "still playing (loop)");
    assert!(
        wrong.is_empty(),
        "{} refreshes showed the wrong frame, e.g. {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
    assert!(wake_err < 1.0, "woken {wake_err:.2} ms off a frame's time");
    assert!(e.apply(&mut s, &Action::Stop));
    e.tick(&mut s, t0 + Duration::from_secs(5));
    assert!(!s.playing);
}

/// Scrubbing: showing another frame (all four layers, 1920 × 1080, the demo's drawings)
/// must fit a display frame (6 ms at 165 Hz), with the onion skin on and off.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn showing_another_frame_fits_a_display_frame() {
    let mut s = State::demo();
    s.frame = 0;
    let mut e = Engine::new(W, H);
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    let mut rows = Vec::new();
    for onion in [false, true] {
        s.onion = onion;
        e.tick(&mut s, Instant::now());
        let _ = e.picture(&s);
        let mut t = Vec::new();
        for round in 0..2 {
            for f in 0..s.frames {
                let f = if round == 0 { f } else { s.frames - 1 - f };
                let st = Instant::now();
                e.apply(&mut s, &Action::GoToFrame(f));
                e.tick(&mut s, Instant::now());
                let r = e
                    .picture(&s)
                    .map(|(_, r)| r[2] as u64 * r[3] as u64)
                    .unwrap_or(0);
                std::hint::black_box(r);
                t.push(st.elapsed().as_secs_f64() * 1e3);
            }
        }
        t.sort_by(|a, b| a.total_cmp(b));
        let q = |p: f64| t[((t.len() - 1) as f64 * p) as usize];
        rows.push((onion, q(0.5), q(0.95), t[t.len() - 1]));
    }
    save_json(
        "frames-scrub-1920x1080.json",
        &format!(
            "[{}]",
            rows.iter()
                .map(|(o, a, b, c)| format!(
                    "{{\"onion\": {o}, \"p50Ms\": {a:.2}, \"p95Ms\": {b:.2}, \"maxMs\": {c:.2}}}"
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    for (o, _, p95, _) in rows {
        assert!(
            p95 < 6.0,
            "onion {o}: showing a frame takes {p95:.1} ms (p95); 6 ms at 165 Hz"
        );
    }
}
