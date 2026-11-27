//! Against a real compositor: open a window, show a frame from shared memory, and read when
//! the compositor says it reached the screen. It needs a Wayland display. The baseline's
//! headless one (no window on any screen) works:
//!
//!   mutter --headless --no-x11 --wayland-display pp-bench-wl --virtual-monitor 2560x1440@165 &
//!   PP_WL_LIVE=pp-bench-wl cargo test -p pencilpilot-wl --test live -- --nocapture
//!
//! Without PP_WL_LIVE it does nothing, so a desktop session never gets a window from tests.

#![cfg(target_os = "linux")]

use std::fs::File;
use std::os::fd::{AsFd, FromRawFd};

use pencilpilot_wl::*;

unsafe extern "C" {
    fn memfd_create(name: *const std::ffi::c_char, flags: u32) -> i32;
}

fn shared_memory(size: u64) -> File {
    // SAFETY: a plain syscall; the descriptor is ours
    let fd = unsafe {
        memfd_create(c"pp-wl-test".as_ptr(), 1 /* MFD_CLOEXEC */)
    };
    assert!(fd >= 0, "memfd_create");
    let f = unsafe { File::from_raw_fd(fd) };
    f.set_len(size).unwrap();
    f
}

#[test]
fn a_window_shows_a_frame_on_a_real_compositor() {
    let Ok(display_name) = std::env::var("PP_WL_LIVE") else {
        eprintln!("skipped: set PP_WL_LIVE to a Wayland display (a headless one)");
        return;
    };
    // SAFETY: tests here run on one thread before anything reads the environment
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display_name) };
    let (mut c, display) = Connection::connect().expect("connect");
    let registry = display.get_registry(&mut c).unwrap();
    c.roundtrip(display).unwrap();

    // the globals
    let mut globals = Vec::new();
    while let Some(ev) = c.next_event() {
        if let Ok(wl_registry::Event::Global {
            name,
            interface,
            version,
        }) = ev.parse()
        {
            globals.push((name, interface, version));
        }
    }
    let find = |want: &str| {
        globals
            .iter()
            .find(|(_, i, _)| i == want)
            .map(|(n, _, v)| (*n, *v))
    };
    eprintln!(
        "{} globals: {:?}",
        globals.len(),
        globals.iter().map(|g| &g.1).collect::<Vec<_>>()
    );
    let (n, v) = find("wl_compositor").expect("wl_compositor");
    let compositor: wl_compositor::WlCompositor = registry.bind(&mut c, n, v.min(6)).unwrap();
    let (n, v) = find("wl_shm").expect("wl_shm");
    let shm: wl_shm::WlShm = registry.bind(&mut c, n, v.min(1)).unwrap();
    let (n, v) = find("xdg_wm_base").expect("xdg_wm_base");
    let wm: xdg_wm_base::XdgWmBase = registry.bind(&mut c, n, v.min(6)).unwrap();
    let presentation = find("wp_presentation").map(|(n, v)| {
        registry
            .bind::<wp_presentation::WpPresentation>(&mut c, n, v.min(2))
            .unwrap()
    });

    // a window
    let surface = compositor.create_surface(&mut c).unwrap();
    let xs = wm.get_xdg_surface(&mut c, surface).unwrap();
    let top = xs.get_toplevel(&mut c).unwrap();
    top.set_title(&mut c, "PencilPilot wl test").unwrap();
    surface.commit(&mut c).unwrap();

    // wait for its first configure (answering pings)
    let (mut w, mut h) = (0, 0);
    loop {
        let ev = c.wait_event().unwrap();
        if ev.is(wm) {
            if let Ok(xdg_wm_base::Event::Ping { serial }) = ev.parse() {
                wm.pong(&mut c, serial).unwrap();
            }
        } else if ev.is(top) {
            if let Ok(xdg_toplevel::Event::Configure { width, height, .. }) = ev.parse() {
                (w, h) = (width, height);
            }
        } else if ev.is(xs)
            && let Ok(xdg_surface::Event::Configure { serial }) = ev.parse()
        {
            xs.ack_configure(&mut c, serial).unwrap();
            break;
        }
    }
    let (w, h) = (if w > 0 { w } else { 640 }, if h > 0 { h } else { 400 });
    eprintln!("configured at {w}×{h}");

    // a frame: coral, from shared memory
    let stride = w * 4;
    let size = (stride * h) as u64;
    let file = shared_memory(size);
    {
        use std::io::Write;
        let px: Vec<u8> = (0..w * h).flat_map(|_| [0x2f, 0x49, 0xe8, 0xff]).collect(); // BGRA
        (&file).write_all(&px).unwrap();
    }
    let pool = shm.create_pool(&mut c, file.as_fd(), size as i32).unwrap();
    let buffer = pool
        .create_buffer(&mut c, 0, w, h, stride, wl_shm::FORMAT_XRGB8888)
        .unwrap();
    surface.attach(&mut c, Some(buffer), 0, 0).unwrap();
    surface.damage_buffer(&mut c, 0, 0, w, h).unwrap();
    let frame = surface.frame(&mut c).unwrap();
    let feedback = presentation.map(|p| p.feedback(&mut c, surface).unwrap());
    surface.commit(&mut c).unwrap();
    c.flush().unwrap();

    // the compositor says when it showed it
    let mut framed = false;
    let mut presented = feedback.is_none();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !(framed && presented) {
        assert!(
            std::time::Instant::now() < deadline,
            "no frame callback / presentation in 5 s"
        );
        let ev = c.wait_event().unwrap();
        if ev.is(frame) {
            framed = true;
        } else if feedback.is_some_and(|f| ev.is(f)) {
            match ev.parse::<wp_presentation_feedback::Event>().unwrap() {
                wp_presentation_feedback::Event::Presented { refresh, flags, .. } => {
                    let hz = if refresh > 0 {
                        1e9 / refresh as f64
                    } else {
                        0.0
                    };
                    eprintln!(
                        "presented: the display refreshes every {refresh} ns ({hz:.1} Hz), flags {flags:#x}"
                    );
                    presented = true;
                }
                wp_presentation_feedback::Event::Discarded {} => panic!("the frame was discarded"),
                _ => {}
            }
        } else if ev.is(wm)
            && let Ok(xdg_wm_base::Event::Ping { serial }) = ev.parse()
        {
            wm.pong(&mut c, serial).unwrap();
        }
    }
    top.destroy(&mut c).unwrap();
    xs.destroy(&mut c).unwrap();
    surface.destroy(&mut c).unwrap();
    c.roundtrip(display).unwrap();
}
