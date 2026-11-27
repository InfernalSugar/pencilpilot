//! The pen check (Phase 1): `pencilpilot --pen-check`. Draw with the pen (or the mouse); the app measures
//! how long each pen reading takes to reach the screen.
//!
//!   pencilpilot                     draw; a line a second says how it's doing
//!   pencilpilot --replay 10         a recorded 240 Hz pen stroke, again and again, for 10 s
//!   pencilpilot --report out.json   on closing, the measurements as JSON
//!   pencilpilot --save out.ppm      on closing, the picture (to check what was drawn)
//!   pencilpilot --draw-at-once      draw as soon as the compositor asks (for comparison)
//!
//! **Drawing late:** a frame is drawn just before the compositor needs it for the next
//! refresh (predicted from when the last frames were shown), so it carries the newest pen
//! readings. The margin before that refresh adjusts itself:
//! - a frame that misses its refresh adds 0.5 ms;
//! - 200 on time take 0.1 ms off, down to 1 ms.
//! - Where it needs more than three quarters of a frame, drawing late doesn't pay on this
//!   compositor: from then on, frames are drawn as soon as it asks.
//!
//! **Pen to screen:** from a pen reading's time to the moment the compositor reports the
//! frame with it on screen, both on the system's monotonic clock.
//! - **A real pen:** its readings carry the compositor's timestamp, in whole milliseconds.
//! - **`--replay`:** the readings are made here, so the time from the tablet to the
//!   compositor (about a millisecond) isn't in it.

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use brush::{Painter, set_brush_rgb};
use hokusai::BrushSetting;
use pencilpilot_gpu::{Canvas, Gpu};
use pencilpilot_window::{InputEvent, PenPhase, PointerEvent, Window, clock};

/// The screen around the canvas (the site's ink).
const AROUND: [f32; 4] = [22.0 / 255.0, 22.0 / 255.0, 26.0 / 255.0, 1.0];
const PENCIL: &str = include_str!("../../web/static/brushes/deevad-2b-pencil.myb");

struct Args {
    at_once: bool,
    replay: Option<f64>,
    report: Option<String>,
    save: Option<String>,
}

fn args() -> Args {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let val = |name: &str| {
        a.iter()
            .position(|x| x == name)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    Args {
        at_once: a.iter().any(|x| x == "--draw-at-once"),
        replay: val("--replay").and_then(|v| v.parse().ok()),
        report: val("--report"),
        save: val("--save"),
    }
}

/// A recorded pen stroke: a wave across the canvas, light to hard and back, at 240 Hz.
struct Replay {
    start: u64,
    until: u64,
    sent: u64,
    stroke: u64,
    w: f32,
    h: f32,
}

impl Replay {
    const HZ: u64 = 240;
    /// samples a stroke (1 s), then 0.3 s lifted
    const STROKE: u64 = 240;
    const GAP: u64 = 72;

    /// The samples due by `now`: (time, phase, x, y, pressure), in device pixels.
    fn due(&mut self, now: u64) -> Vec<(u64, PenPhase, f32, f32, f32)> {
        let mut out = Vec::new();
        while self.start + self.sent * 1_000_000_000 / Self::HZ <= now && now < self.until {
            let t = self.start + self.sent * 1_000_000_000 / Self::HZ;
            let i = self.sent % (Self::STROKE + Self::GAP);
            self.sent += 1;
            if i >= Self::STROKE {
                continue;
            }
            if i == 0 {
                self.stroke += 1;
            }
            let u = i as f32 / (Self::STROKE - 1) as f32;
            let lane = (self.stroke as f32 * 0.137).fract();
            let x = self.w * (0.1 + 0.8 * u);
            let y = self.h * (0.15 + 0.7 * lane)
                + self.h * 0.06 * (u * 12.0 + self.stroke as f32).sin();
            let p = 0.25 + 0.65 * (u * std::f32::consts::PI).sin().powi(2);
            let phase = match i {
                0 => PenPhase::Down,
                _ if i == Self::STROKE - 1 => PenPhase::Up,
                _ => PenPhase::Move,
            };
            out.push((t, phase, x, y, p));
        }
        out
    }
}

/// p-th percentile
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() as f64 * p / 100.0) as usize).min(s.len() - 1)]
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = args();
    let gpu = Gpu::new()?;
    let mut win = Window::new(&gpu, "PencilPilot")?;
    let (w, h) = win.size();
    let scale = win.scale() as f32;
    eprintln!(
        "PencilPilot (native, Phase 1): {} · canvas {w}×{h} at ×{scale}",
        gpu.info.name
    );

    let mut painter = Painter::new(w, h);
    let mut pencil = hokusai::myb::from_str(PENCIL).map_err(|e| format!("the pencil: {e:?}"))?;
    let r = pencil.get(BrushSetting::Radius).base_value;
    pencil.get_mut(BrushSetting::Radius).base_value = r + 1.0; // a bit bigger than its default
    set_brush_rgb(&mut pencil, 22, 22, 26);
    let mut canvas = Canvas::new(&gpu, w, h)?;
    canvas.upload_all(painter.rgba());

    let started = clock::now_ns();
    let mut replay = args.replay.map(|s| Replay {
        start: started + 300_000_000,
        until: started + 300_000_000 + (s * 1e9) as u64,
        sent: 0,
        stroke: 0,
        w: w as f32,
        h: h as f32,
    });

    // measurements
    let mut waiting: Vec<u64> = Vec::new(); // reading times painted, not yet in a frame
    let mut in_frame: BTreeMap<u64, Vec<u64>> = BTreeMap::new(); // frame → its readings
    let mut pen_to_screen: Vec<f64> = Vec::new(); // ms
    let mut presents: VecDeque<u64> = VecDeque::new();
    let mut intervals: Vec<f64> = Vec::new();
    let mut last_present: Option<u64> = None;
    let mut refresh_ns = 0u32;
    let mut paint_ms: Vec<f64> = Vec::new();
    let mut gpu_ms: Vec<f64> = Vec::new();
    let mut last_pen_ms: Option<u32> = None;
    let mut mouse_down = false;
    let mut second = Instant::now();
    let mut second_lat: Vec<f64> = Vec::new();
    // drawing late
    let mut margin_ns: u64 = 3_000_000;
    let mut on_time = 0u32;
    let mut late_frames = 0u32;
    let mut targets: BTreeMap<u64, u64> = BTreeMap::new(); // frame → the refresh it was drawn for
    let mut last_shown: Option<u64> = None;
    let mut margins: Vec<f64> = Vec::new();
    let mut at_once = args.at_once;

    let finished = |r: &Option<Replay>| {
        r.as_ref()
            .is_some_and(|r| clock::now_ns() > r.until + 500_000_000)
    };
    while win.is_open() && !finished(&replay) {
        // the refresh the next frame is for, and when to draw it
        let plan = match (at_once, last_shown, refresh_ns) {
            (false, Some(last), p) if p > 0 => {
                let p = p as u64;
                let now = clock::now_ns();
                let n = (now + margin_ns + 200_000).saturating_sub(last).div_ceil(p);
                let target = last + n.max(1) * p;
                Some((target, target - margin_ns))
            }
            _ => None,
        };
        if !win.wants_frame() {
            win.wait()?;
        } else if let Some((_, draw_at)) = plan
            && clock::now_ns() < draw_at
        {
            // the pen's readings keep coming in while waiting
            win.wait_until(draw_at)?;
        }
        let t_paint = Instant::now();
        // the person's input
        for ev in win.take_input() {
            match ev {
                InputEvent::Pen(p) => {
                    let (x, y) = (p.sample.x as f32 * scale, p.sample.y as f32 * scale);
                    let dt = last_pen_ms.map_or(0.004, |l| {
                        (p.time_ms.wrapping_sub(l) as f64 / 1000.0).clamp(0.0005, 0.1)
                    });
                    last_pen_ms = Some(p.time_ms);
                    let (tx, ty) = (p.sample.tilt_x / 90.0, p.sample.tilt_y / 90.0);
                    match p.phase {
                        PenPhase::Down => {
                            painter.begin_stroke();
                            painter.stroke_to(&pencil, x, y, p.sample.pressure, tx, ty, dt);
                        }
                        PenPhase::Move => {
                            painter.stroke_to(&pencil, x, y, p.sample.pressure, tx, ty, dt)
                        }
                        PenPhase::Up => painter.end_stroke(&pencil),
                        _ => continue,
                    }
                    // the compositor's time, ms, on the monotonic clock
                    waiting.push(p.time_ms as u64 * 1_000_000);
                }
                InputEvent::Pointer(PointerEvent::Button {
                    button: 0x110,
                    pressed,
                    x,
                    y,
                    time_ms,
                }) => {
                    let (x, y) = (x as f32 * scale, y as f32 * scale);
                    if pressed {
                        mouse_down = true;
                        painter.begin_stroke();
                        painter.stroke_to(&pencil, x, y, 0.5, 0.0, 0.0, 0.004);
                    } else if mouse_down {
                        mouse_down = false;
                        painter.end_stroke(&pencil);
                    }
                    waiting.push(time_ms as u64 * 1_000_000);
                }
                InputEvent::Pointer(PointerEvent::Move { x, y, time_ms }) if mouse_down => {
                    painter.stroke_to(
                        &pencil,
                        x as f32 * scale,
                        y as f32 * scale,
                        0.5,
                        0.0,
                        0.0,
                        0.004,
                    );
                    waiting.push(time_ms as u64 * 1_000_000);
                }
                _ => {}
            }
        }
        // the recorded stroke
        if let Some(r) = &mut replay {
            for (t, phase, x, y, p) in r.due(clock::now_ns()) {
                match phase {
                    PenPhase::Down => {
                        painter.begin_stroke();
                        painter.stroke_to(&pencil, x, y, p, 0.0, 0.0, 1.0 / 240.0);
                    }
                    PenPhase::Up => {
                        painter.stroke_to(&pencil, x, y, p, 0.0, 0.0, 1.0 / 240.0);
                        painter.end_stroke(&pencil);
                    }
                    _ => painter.stroke_to(&pencil, x, y, p, 0.0, 0.0, 1.0 / 240.0),
                }
                waiting.push(t);
            }
        }
        if let Some(r) = painter.flush() {
            canvas.upload(painter.rgba(), r.x, r.y, r.w, r.h);
        }
        paint_ms.push(t_paint.elapsed().as_secs_f64() * 1e3);

        let due = plan.is_none_or(|(_, draw_at)| clock::now_ns() >= draw_at);
        if win.wants_frame() && due {
            let t_gpu = Instant::now();
            let img = win.acquire(&gpu)?;
            gpu.show(&mut canvas, img, AROUND)?;
            gpu_ms.push(t_gpu.elapsed().as_secs_f64() * 1e3);
            let frame = win.present()?;
            if let Some((target, _)) = plan {
                targets.insert(frame, target);
                margins.push(margin_ns as f64 / 1e6);
            }
            if !waiting.is_empty() {
                in_frame.insert(frame, std::mem::take(&mut waiting));
            }
        }
        for p in win.take_presented() {
            if let Some(last) = last_present {
                intervals.push((p.at_ns - last) as f64 / 1e6);
            }
            last_present = Some(p.at_ns);
            last_shown = Some(p.at_ns);
            if let Some(target) = targets.remove(&p.frame)
                && refresh_ns > 0
            {
                if p.at_ns > target + refresh_ns as u64 / 2 {
                    // missed its refresh: more room
                    late_frames += 1;
                    margin_ns = (margin_ns + 500_000).min(refresh_ns as u64);
                    on_time = 0;
                    if !at_once && margin_ns * 4 > refresh_ns as u64 * 3 {
                        at_once = true;
                        eprintln!(
                            "drawing late doesn't pay on this compositor: drawing as soon as it asks"
                        );
                    }
                } else {
                    on_time += 1;
                    if on_time >= 200 {
                        margin_ns = margin_ns.saturating_sub(100_000).max(1_000_000);
                        on_time = 0;
                    }
                }
            }
            targets.retain(|&f, _| f > p.frame);
            presents.push_back(p.at_ns);
            if p.refresh_ns > 0 {
                refresh_ns = p.refresh_ns;
            }
            if let Some(times) = in_frame.remove(&p.frame) {
                for t in times {
                    let ms = p.at_ns.saturating_sub(t) as f64 / 1e6;
                    pen_to_screen.push(ms);
                    second_lat.push(ms);
                }
            }
            // frames never reported (skipped) don't pile up
            in_frame.retain(|&f, _| f > p.frame);
        }
        if second.elapsed().as_secs_f64() >= 1.0 {
            let cutoff = clock::now_ns().saturating_sub(1_000_000_000);
            while presents.front().is_some_and(|&t| t < cutoff) {
                presents.pop_front();
            }
            let hz = if refresh_ns > 0 {
                1e9 / refresh_ns as f64
            } else {
                0.0
            };
            if second_lat.is_empty() {
                eprintln!("{} fps on a {hz:.0} Hz monitor", presents.len());
            } else {
                eprintln!(
                    "{} fps on a {hz:.0} Hz monitor · pen to screen {:.1} ms median, {:.1} ms slowest ({} readings) · drawn {:.1} ms before the refresh",
                    presents.len(),
                    pct(&second_lat, 50.0),
                    pct(&second_lat, 100.0),
                    second_lat.len(),
                    margin_ns as f64 / 1e6
                );
            }
            second_lat.clear();
            second = Instant::now();
        }
    }

    let hz = if refresh_ns > 0 {
        1e9 / refresh_ns as f64
    } else {
        0.0
    };
    let frame_ms = if refresh_ns > 0 {
        refresh_ns as f64 / 1e6
    } else {
        1000.0 / 60.0
    };
    let missed = intervals.iter().filter(|&&i| i > frame_ms * 1.5).count();
    let dist = |v: &[f64]| {
        format!(
            "{{\"n\": {}, \"p50\": {:.2}, \"p95\": {:.2}, \"p99\": {:.2}, \"max\": {:.2}}}",
            v.len(),
            pct(v, 50.0),
            pct(v, 95.0),
            pct(v, 99.0),
            pct(v, 100.0)
        )
    };
    let json = format!(
        "{{\n  \"gpu\": \"{}\",\n  \"canvas\": [{w}, {h}],\n  \"refreshHz\": {hz:.2},\n  \"replay\": {},\n  \"penToScreenMs\": {},\n  \"frameIntervalMs\": {},\n  \"framesMissed\": {missed},\n  \"framesDiscarded\": {},\n  \"drawnAtOnce\": {},\n  \"framesLateForTheirRefresh\": {late_frames},\n  \"marginMs\": {},\n  \"paintMs\": {},\n  \"gpuMs\": {}\n}}",
        gpu.info.name,
        args.replay.is_some(),
        dist(&pen_to_screen),
        dist(&intervals),
        win.discarded(),
        at_once,
        dist(&margins),
        dist(&paint_ms),
        dist(&gpu_ms)
    );
    eprintln!("{json}");
    if let Some(path) = args.report {
        std::fs::write(path, json)?;
    }
    if let Some(path) = args.save {
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        ppm.extend(
            painter
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]]),
        );
        std::fs::write(path, ppm)?;
    }
    Ok(())
}
