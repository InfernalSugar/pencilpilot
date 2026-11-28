//! The mixer is real-time safe: rendering (playing, a snippet, taking new commands) makes no
//! allocation and frees nothing. Counted by a global allocator, on the rendering thread only.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use pencilpilot_sound::Pcm;
use pencilpilot_sound::player::{Clock, Cmd, Mode, Voice, channel};

struct Counting;

static CALLS: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static WATCH: Cell<bool> = const { Cell::new(false) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if WATCH.with(Cell::get) {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if WATCH.with(Cell::get) {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if WATCH.with(Cell::get) {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn rendering_neither_allocates_nor_frees() {
    let (mut remote, mut mixer, mut recs) = channel(true);
    let pcm = Arc::new(Pcm {
        left: (0..96_000).map(|k| (k as f32 * 0.01).sin()).collect(),
        right: Some((0..96_000).map(|k| (k as f32 * 0.02).sin()).collect()),
    });
    let voices = vec![
        Voice {
            pcm: pcm.clone(),
            start: -500,
            gain: 0.7,
        },
        Voice {
            pcm,
            start: 3000,
            gain: 1.0,
        },
    ];
    let t0 = Instant::now();
    let clock = Clock {
        t0,
        from: 0,
        fps: 24,
        range: (0, 47),
        mode: Mode::Loop,
        cut_start: 10,
        shift: 0.0,
    };
    let mut buf = vec![0f32; 480 * 2];
    let mut n = 0u32;
    let mut render = |mixer: &mut pencilpilot_sound::player::Mixer| {
        n += 1;
        WATCH.with(|w| w.set(true));
        mixer.render(
            &mut buf,
            2,
            48_000,
            t0 + Duration::from_millis(10 * n as u64),
        );
        WATCH.with(|w| w.set(false));
    };
    // commands arrive between buffers: play, a change of clips mid-play, a snippet, stop
    remote.send(Cmd::Play {
        voices: voices.clone(),
        clock,
    });
    for _ in 0..300 {
        render(&mut mixer);
    }
    remote.send(Cmd::Play {
        voices: voices[..1].to_vec(),
        clock,
    });
    for _ in 0..50 {
        render(&mut mixer);
    }
    remote.send(Cmd::Snippet {
        voices: voices.clone(),
        at: 5000,
        len: 4000,
    });
    for _ in 0..20 {
        render(&mut mixer);
    }
    remote.send(Cmd::Stop);
    for _ in 0..5 {
        render(&mut mixer);
    }
    assert_eq!(CALLS.load(Ordering::Relaxed), 0, "allocations while mixing");
    // (what it let go of is freed here)
    remote.collect();
    let r = recs.as_mut().unwrap();
    // (every buffer reported)
    assert_eq!(r.slots(), 375);
}
