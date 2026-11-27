//! The window against a real compositor. Needs a display; use a headless one with two
//! monitors at different rates, so no window opens on any screen:
//!
//!   mutter --headless --no-x11 --wayland-display pp-bench-win \
//!     --virtual-monitor 1920x1080@165 --virtual-monitor 1920x1080@60 &
//!   PP_WL_LIVE=pp-bench-win cargo test -p pencilpilot-window -- --nocapture --test-threads 1

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use pencilpilot_gpu::Gpu;
use pencilpilot_window::{Presented, Window};

fn live() -> bool {
    match std::env::var("PP_WL_LIVE") {
        Ok(d) => {
            // SAFETY: tests run one at a time (--test-threads 1), before anything reads it
            unsafe { std::env::set_var("WAYLAND_DISPLAY", d) };
            true
        }
        Err(_) => {
            eprintln!("skipped: set PP_WL_LIVE to a Wayland display (a headless one)");
            false
        }
    }
}

/// Draw for `secs` (a colour that changes every frame); the frames shown.
fn run(win: &mut Window, gpu: &Gpu, secs: f64) -> Vec<Presented> {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut n = 0u32;
    let mut shown = Vec::new();
    while Instant::now() < end {
        if !win.wants_frame() {
            win.wait().unwrap();
        }
        if win.wants_frame() {
            let t = n as f32 / 60.0;
            let img = win.acquire(gpu).unwrap();
            gpu.clear(
                img,
                [0.5 + 0.5 * t.sin(), 0.5 + 0.5 * (t * 1.3).cos(), 0.6, 1.0],
            )
            .unwrap();
            win.present().unwrap();
            n += 1;
        }
        shown.extend(win.take_presented());
    }
    shown
}

/// (frames a second, the refresh the compositor reported, Hz)
fn rate(shown: &[Presented]) -> (f64, f64) {
    let s = &shown[shown.len() / 5..]; // past the first fifth: settled
    let span = (s.last().unwrap().at_ns - s[0].at_ns) as f64 / 1e9;
    let fps = (s.len() - 1) as f64 / span;
    let hz = 1e9 / s.last().unwrap().refresh_ns as f64;
    (fps, hz)
}

#[test]
fn it_draws_at_the_monitors_rate() {
    if !live() {
        return;
    }
    let gpu = Gpu::new().unwrap();
    let mut win = Window::new(&gpu, "PencilPilot window test").unwrap();
    let shown = run(&mut win, &gpu, 2.0);
    let (fps, hz) = rate(&shown);
    eprintln!(
        "{} frames shown, {fps:.1} fps on a {hz:.1} Hz monitor, {} skipped",
        shown.len(),
        win.discarded()
    );
    assert!((fps - hz).abs() / hz < 0.03, "{fps:.1} fps on {hz:.1} Hz");
    assert_eq!(win.discarded(), 0);
}

#[test]
fn it_follows_the_monitor_it_is_on() {
    if !live() {
        return;
    }
    let gpu = Gpu::new().unwrap();
    let mut win = Window::new(&gpu, "PencilPilot window test").unwrap();
    run(&mut win, &gpu, 0.3);
    let monitors = win.all_outputs();
    eprintln!(
        "monitors: {:?}",
        monitors
            .iter()
            .map(|(_, o)| (o.name.clone(), o.refresh_mhz))
            .collect::<Vec<_>>()
    );
    assert!(
        monitors.len() >= 2,
        "this test needs two monitors at different rates"
    );
    for (output, info) in monitors {
        // (out of fullscreen first: mutter keeps a fullscreen window where it is)
        win.set_fullscreen(None);
        run(&mut win, &gpu, 0.3);
        win.set_fullscreen(Some(Some(&output)));
        run(&mut win, &gpu, 0.5); // moving over
        let shown = run(&mut win, &gpu, 1.5);
        let (fps, hz) = rate(&shown);
        let want = info.refresh_mhz as f64 / 1000.0;
        eprintln!(
            "on {} ({want:.1} Hz): {fps:.1} fps, the compositor reports {hz:.1} Hz, drawn at {:?}, the window is on {:?}",
            info.name,
            win.size(),
            win.outputs().iter().map(|o| &o.name).collect::<Vec<_>>()
        );
        assert!(
            (hz - want).abs() / want < 0.01,
            "reported {hz:.1} Hz on a {want:.1} Hz monitor"
        );
        assert!(
            (fps - want).abs() / want < 0.03,
            "{fps:.1} fps on a {want:.1} Hz monitor"
        );
        assert_eq!(
            win.size(),
            (info.width as u32, info.height as u32),
            "fullscreen fills the monitor"
        );
    }
}

#[test]
fn maximising_resizes_the_frames() {
    if !live() {
        return;
    }
    let gpu = Gpu::new().unwrap();
    let mut win = Window::new(&gpu, "PencilPilot window test").unwrap();
    run(&mut win, &gpu, 0.3);
    let before = win.size();
    win.set_maximized(true);
    run(&mut win, &gpu, 0.5);
    let after = win.size();
    eprintln!("{before:?} → {after:?}");
    assert_ne!(before, after);
    assert!(after.0 >= before.0 && after.1 >= before.1);
}
