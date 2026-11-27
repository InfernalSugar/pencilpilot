use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::shell::engine::frames::{Cel, Mark, Sheet, Track};
use crate::state::Layer;

/// A folder of its own for a check's files.
fn tmp(name: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "pp-native-export-{name}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The demo's engine, set up (its drawings painted).
fn demo() -> (Engine, State) {
    let mut s = State::demo();
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    e.tick(&mut s, Instant::now());
    (e, s)
}

/// Ticks until the export is over (or `limit` passes).
fn run_out(e: &mut Engine, s: &mut State, limit: Duration) {
    let t = Instant::now();
    while e.exporter.run.is_some() && t.elapsed() < limit {
        e.tick(s, Instant::now());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(e.exporter.run.is_none(), "the export finished in {limit:?}");
}

/// A frame as the window's painter renders it (each layer's drawing on `frame`).
fn engine_frame(e: &Engine, s: &State, frame: u32) -> Vec<u8> {
    let pairs: Vec<u32> = s
        .sheet
        .tracks
        .iter()
        .flat_map(|t| [t.layer, t.drawing_at(frame)])
        .collect();
    e.painter.render_frame(&pairs, 1, &[])
}

fn read_png(p: &Path) -> (u32, u32, Vec<u8>) {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

#[test]
fn sizes_ranges_and_the_plan() {
    assert_eq!(out_size(0, 1920, 1080, true), (1920, 1080));
    assert_eq!(out_size(1, 1921, 1081, false), (961, 541));
    assert_eq!(out_size(1, 1921, 1081, true), (962, 542));
    assert_eq!(out_size(3, 3840, 2160, true), (1920, 1080));
    assert_eq!(out_size(4, 1920, 1080, true), (3840, 2160));
    assert_eq!(out_size(2, 1000, 1000, true), (720, 720));

    let mut s = State::demo();
    s.sheet = Sheet::default();
    s.layers.truncate(2);
    s.sheet.tracks = vec![
        Track {
            layer: 7,
            kind: 'C',
            opacity: 1.0,
            cels: vec![
                Cel::new(1, 2, Mark::Key),
                Cel::new(0, 1, Mark::None),
                Cel::new(2, 3, Mark::None),
            ],
        },
        Track {
            layer: 9,
            kind: 'S',
            opacity: 1.0,
            cels: vec![Cel::new(3, 6, Mark::None)],
        },
    ];
    s.frames = 6;
    assert_eq!(range(&s, true), (0, 5), "no loop range: the cut");
    s.sheet.loop_range = Some((2, 9));
    assert_eq!(range(&s, true), (2, 5));
    assert_eq!(range(&s, false), (0, 5));
    let p = Plan::new(&s, 0, 5);
    // held frames go once, held
    assert_eq!(
        p.frames,
        vec![
            PlanFrame {
                pairs: vec![7, 1, 9, 3],
                repeat: 2
            },
            PlanFrame {
                pairs: vec![7, 0, 9, 3],
                repeat: 1
            },
            PlanFrame {
                pairs: vec![7, 2, 9, 3],
                repeat: 3
            },
        ]
    );
    assert_eq!(p.total(), 6);
    assert_eq!(p.spans[&1], (0, 0));
    assert_eq!(p.spans[&3], (0, 2), "the static drawing stays to the end");
    let p = Plan::new(&s, 2, 4);
    assert_eq!(p.total(), 3);
    assert_eq!(p.frames[0].pairs, vec![7, 0, 9, 3]);
}

/// The dialog's Export… asks for a file; cancelling the file dialog goes back to the
/// settings; a format FFmpeg can't write isn't started.
#[test]
fn export_asks_for_a_file_first() {
    let (mut e, mut s) = demo();
    assert!(e.apply(&mut s, &Action::File("Export…".into())));
    assert!(s.export.open);
    // FFmpeg is looked for meanwhile
    let t = Instant::now();
    while s.export.videos.is_none() && t.elapsed() < Duration::from_secs(10) {
        e.tick(&mut s, Instant::now());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(s.export.videos.is_some());
    s.export.format = Format::Gif;
    e.apply(&mut s, &Action::Other(ops::START.into()));
    assert_eq!(s.export.status, Status::Asking);
    assert!(s.export.busy());
    // closing is refused while busy; cancel goes back
    e.apply(&mut s, &Action::Other(ops::CLOSE.into()));
    assert!(s.export.open);
    e.apply(&mut s, &Action::Other(ops::CANCEL.into()));
    assert_eq!(s.export.status, Status::Idle);
    e.apply(&mut s, &Action::Other(ops::CLOSE.into()));
    assert!(!s.export.open);
    // a video without FFmpeg's encoder for it
    s.export.videos = Some(Vec::new());
    s.export.format = Format::Mp4;
    e.apply(&mut s, &Action::Other(ops::START.into()));
    assert_eq!(s.export.status, Status::Idle, "nothing asked");
}

/// A PNG sequence of the demo: a file per frame, each the window's own render of that frame
/// (held frames written each time), over white or transparent; the window's ticks stay
/// short meanwhile.
#[test]
fn png_sequence_matches_the_window() {
    let (mut e, mut s) = demo();
    let d = tmp("png");
    s.sheet.loop_range = Some((4, 15));
    s.export.loop_only = true;
    s.export.format = Format::Png;
    s.export.size = 1;
    start(&mut e, &mut s, d.join("walk.png"));
    assert_eq!(s.export_progress, Some((0, 12)));
    run_out(&mut e, &mut s, Duration::from_secs(60));
    assert_eq!(s.export_progress, None);
    let Status::Done(first) = &s.export.status else {
        panic!("{:?}", s.export.status)
    };
    assert!(first.ends_with("walk_0001.png"), "{first:?}");
    let mut names: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 12);
    assert_eq!(names[11], "walk_0012.png");
    // half size: the painter's own half-size render of frames 4 and 15
    let (w, h) = e.painter.frame_size(2);
    for (file, frame) in [("walk_0001.png", 4), ("walk_0012.png", 15)] {
        let (fw, fh, px) = read_png(&d.join(file));
        assert_eq!((fw, fh), (w, h));
        let pairs: Vec<u32> = s
            .sheet
            .tracks
            .iter()
            .flat_map(|t| [t.layer, t.drawing_at(frame)])
            .collect();
        assert_eq!(px, e.painter.render_frame(&pairs, 2, &[]), "frame {frame}");
    }
    // (the window's copies: measured in release by `export_fixtures`)
    eprintln!(
        "the window's longest tick of copying: {:?}",
        e.exporter.feed_max
    );

    // transparent: see-through where nothing's drawn (the demo's background is drawn
    // everywhere, so hide it)
    let bg = s.sheet.tracks.iter().position(|t| t.kind == 'S').unwrap();
    s.layers[bg].visible = false;
    e.tick(&mut s, Instant::now());
    let d2 = tmp("alpha");
    s.export.transparent = true;
    s.export.size = 0;
    s.sheet.loop_range = Some((0, 0));
    start(&mut e, &mut s, d2.join("a.png"));
    run_out(&mut e, &mut s, Duration::from_secs(60));
    let (_, _, px) = read_png(&d2.join("a_0001.png"));
    assert!(px.chunks(4).any(|p| p[3] == 0), "see-through somewhere");
    assert!(px.chunks(4).any(|p| p[3] == 255), "and drawn somewhere");
    let _ = std::fs::remove_dir_all(d);
    let _ = std::fs::remove_dir_all(d2);
}

/// A size the painter can't shrink to by a whole number is resized; a GIF holds its
/// frames.
#[test]
fn gif_at_another_size() {
    let (mut e, mut s) = demo();
    let d = tmp("gif");
    s.export.format = Format::Gif;
    s.export.size = 2; // 720p
    s.sheet.loop_range = Some((0, 11));
    s.export.loop_only = true;
    start(&mut e, &mut s, d.join("cut.gif"));
    run_out(&mut e, &mut s, Duration::from_secs(120));
    let Status::Done(p) = &s.export.status else {
        panic!("{:?}", s.export.status)
    };
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts.read_info(std::fs::File::open(p).unwrap()).unwrap();
    let (ow, oh) = out_size(2, s.view.doc_w, s.view.doc_h, false);
    assert_eq!((dec.width() as u32, dec.height() as u32), (ow, oh));
    let mut delays = Vec::new();
    while let Some(f) = dec.read_next_frame().unwrap() {
        delays.push(f.delay);
    }
    // 12 frames at 24 fps = half a second, however they're held
    assert_eq!(
        delays.iter().map(|d| *d as u32).sum::<u32>(),
        50,
        "{delays:?}"
    );
    let _ = std::fs::remove_dir_all(d);
}

/// Cancelling stops FFmpeg and takes the partial file away; the dialog goes back to its
/// settings.
#[test]
fn cancel_removes_the_partial_file() {
    if pencilpilot_export::find_ffmpeg()
        .is_none_or(|f| !pencilpilot_export::video_formats(&f).contains(&Format::Mp4))
    {
        return;
    }
    let (mut e, mut s) = demo();
    let d = tmp("cancel");
    let out = d.join("cut.mp4");
    s.export.format = Format::Mp4;
    start(&mut e, &mut s, out.clone());
    let t = Instant::now();
    while s.export_progress.is_none_or(|p| p.0 < 2) && t.elapsed() < Duration::from_secs(30) {
        e.tick(&mut s, Instant::now());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(e.exporter.run.is_some(), "still going");
    e.apply(&mut s, &Action::Other(ops::CANCEL.into()));
    run_out(&mut e, &mut s, Duration::from_secs(30));
    assert_eq!(s.export.status, Status::Idle);
    assert_eq!(s.export_progress, None);
    assert!(!out.exists(), "the partial file is gone");
    let _ = std::fs::remove_dir_all(d);
}

/// An MP4 of the demo decodes back to the window's frames: the frame count, the duration,
/// and the pictures (within H.264's loss).
#[test]
fn mp4_decodes_back_to_the_window_frames() {
    let Some(ff) = pencilpilot_export::find_ffmpeg() else {
        return;
    };
    if !pencilpilot_export::video_formats(&ff).contains(&Format::Mp4) {
        return;
    }
    let (mut e, mut s) = demo();
    let d = tmp("mp4");
    let out = d.join("cut.mp4");
    s.export.format = Format::Mp4;
    start(&mut e, &mut s, out.clone());
    run_out(&mut e, &mut s, Duration::from_secs(120));
    assert_eq!(s.export.status, Status::Done(out.clone()));
    let check = decode_check(&ff, &out, &e, &s, &[0, 20, 47], &d);
    assert_eq!(check.frames, s.frames);
    assert!((check.seconds - s.frames as f64 / s.fps as f64).abs() < 0.05);
    for (f, psnr) in &check.psnr {
        assert!(*psnr > 35.0, "frame {f}: {psnr:.1} dB");
    }
    let _ = std::fs::remove_dir_all(d);
}

/// What a video decodes back to: its frame count, its length, and each frame's PSNR
/// against the window's render.
struct Decoded {
    frames: u32,
    seconds: f64,
    psnr: Vec<(u32, f64)>,
}

fn decode_check(ff: &Path, video: &Path, e: &Engine, s: &State, at: &[u32], dir: &Path) -> Decoded {
    use std::process::Command;
    let probe = ff.with_file_name("ffprobe");
    let out = Command::new(&probe)
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=nb_read_frames:format=duration"])
        .args(["-of", "default=nw=1"])
        .arg(video)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let field = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(-1.0)
    };
    let frames = field("nb_read_frames") as u32;
    let seconds = field("duration");
    let mut psnr = Vec::new();
    for &f in at {
        let png = dir.join(format!("decoded-{f}.png"));
        let ok = Command::new(ff)
            .args(["-v", "error", "-y", "-i"])
            .arg(video)
            .args(["-vf", &format!("select=eq(n\\,{f})"), "-frames:v", "1"])
            .args(["-pix_fmt", "rgba"])
            .arg(&png)
            .status()
            .unwrap()
            .success();
        assert!(ok, "decoding frame {f}");
        let (_, _, px) = read_png(&png);
        let want = engine_frame(e, s, f);
        assert_eq!(px.len(), want.len());
        let mse = px
            .chunks(4)
            .zip(want.chunks(4))
            .map(|(a, b)| {
                (0..3)
                    .map(|c| (a[c] as f64 - b[c] as f64).powi(2))
                    .sum::<f64>()
            })
            .sum::<f64>()
            / (px.len() / 4 * 3) as f64;
        psnr.push((f, 10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10()));
    }
    Decoded {
        frames,
        seconds,
        psnr,
    }
}

// ---- the measuring fixtures ----

/// An engine with a project's first cut, as opening it would make it (until Open lands):
/// its layers, drawings and exposures.
pub fn engine_from(path: &Path) -> (Engine, State) {
    let p = ppstudio::Project::open(path).unwrap();
    let m = &p.manifest;
    let (w, h) = (m.canvas.width, m.canvas.height);
    let names: Vec<&String> = m.drawings.keys().collect();
    let id_of = |d: &str| names.iter().position(|n| *n == d).unwrap() as u32 + 1;
    let statics = &m.timeline.static_layers;
    let cut = &m.timeline.scenes[0].cuts[0];
    let mut s = State::demo();
    s.sheet = Sheet::default();
    s.layers.clear();
    s.frames = cut.length(statics);
    s.fps = m.timeline.fps;
    s.frame = 0;
    s.active_layer = 0;
    (s.view.doc_w, s.view.doc_h) = (w, h);
    // top first
    for l in m.layers.iter().rev() {
        let kind = if statics.contains(&l.id) { 'S' } else { 'C' };
        s.layers.push(Layer {
            name: l.name.clone(),
            visible: l.visible,
            locked: false,
            kind,
        });
        let cels = cut
            .tracks
            .get(&l.id)
            .map(|t| {
                t.iter()
                    .map(|x| {
                        let d = x.drawing.as_deref().map_or(0, id_of);
                        Cel::new(d, x.length.max(1), Mark::None)
                    })
                    .collect()
            })
            .unwrap_or_default();
        s.sheet.tracks.push(Track {
            layer: 0,
            kind,
            opacity: l.opacity,
            cels,
        });
    }
    let mut e = Engine::new(w, h);
    e.tick(&mut s, Instant::now());
    for (i, n) in names.iter().enumerate() {
        let (rgba, _) = p.drawing(n).unwrap();
        assert!(e.painter.put_drawing(i as u32 + 1, &rgba));
    }
    e.painter.flush();
    e.tick(&mut s, Instant::now());
    (e, s)
}

/// This process's memory now (resident, proportional), kB.
fn memory() -> (u64, u64) {
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0);
    let pss = std::fs::read_to_string("/proc/self/smaps_rollup")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("Pss:"))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0);
    (rss, pss)
}

/// The budgets' export measurement: each fixture to MP4 at its size, timed, with memory and
/// a decode check. `PP_FIXTURES` = the fixtures' folder; `PP_EXPORT_OUT` = where the files
/// and the numbers go. FFmpeg's own peak goes through a wrapper that samples it
/// (`PENCILPILOT_FFMPEG`, see the report).
#[test]
#[ignore = "timing: PP_FIXTURES=… cargo test --release -p pencilpilot-native -- --ignored export_fixtures --nocapture"]
fn export_fixtures() {
    let dir = PathBuf::from(std::env::var("PP_FIXTURES").expect("PP_FIXTURES"));
    let out = PathBuf::from(std::env::var("PP_EXPORT_OUT").unwrap_or_else(|_| "/tmp".into()));
    let ff = pencilpilot_export::find_ffmpeg().expect("ffmpeg");
    let real_ff = std::env::var("PP_REAL_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or(ff.clone());
    let only = std::env::var("PP_FIXTURE").ok();
    let mut report = Vec::new();
    for name in ["drawings500", "layers20-4k"] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let (mut e, mut s) = engine_from(&dir.join(format!("{name}.ppstudio")));
        // settled first (the first ticks make the drawings' thumbnails)
        let mut quiet = 0;
        for _ in 0..5000 {
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            quiet = if t.elapsed() < Duration::from_micros(300) {
                quiet + 1
            } else {
                0
            };
            if quiet == 50 {
                break;
            }
        }
        let (rss0, pss0) = memory();
        let video = out.join(format!("{name}.mp4"));
        let _ = std::fs::remove_file(&video);
        s.export.format = Format::Mp4;
        let t0 = Instant::now();
        start(&mut e, &mut s, video.clone());
        let (mut rss_max, mut pss_max) = (rss0, pss0);
        let mut ticks = Vec::new();
        // as the window does: a tick, then a sleep until the export's next one
        let mut n = 0;
        while e.exporter.run.is_some() {
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            ticks.push(t.elapsed());
            n += 1;
            if n % 4 == 0 {
                let (r, p) = memory();
                rss_max = rss_max.max(r);
                pss_max = pss_max.max(p);
            }
            if let Some(at) = e.next_tick_at(&s) {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
            }
        }
        let (r, p) = memory();
        rss_max = rss_max.max(r);
        pss_max = pss_max.max(p);
        let wall = t0.elapsed();
        let o = e.exporter.last.clone().unwrap();
        assert!(o.result.is_ok(), "{:?}", o.result);
        let st = &o.stats;
        let n = st.written.max(1);
        ticks.sort();
        let p99 = ticks[(ticks.len() * 99 / 100).min(ticks.len() - 1)];
        let mut feeds = e.exporter.feed_ticks.clone();
        feeds.sort();
        let q = |v: &[Duration], f: f64| v[((v.len() as f64 * f) as usize).min(v.len() - 1)];
        let feed_q = format!(
            "p50 {:.2} p90 {:.2} p99 {:.2} max {:.2} ({} ticks)",
            q(&feeds, 0.5).as_secs_f64() * 1e3,
            q(&feeds, 0.9).as_secs_f64() * 1e3,
            q(&feeds, 0.99).as_secs_f64() * 1e3,
            q(&feeds, 1.0).as_secs_f64() * 1e3,
            feeds.len()
        );
        let mid = s.frames / 2;
        let check = decode_check(&real_ff, &video, &e, &s, &[0, mid, s.frames - 1], &out);
        let line = serde_line(&[
            ("fixture", name.to_string()),
            (
                "size",
                format!("{}x{}", e.painter.width(), e.painter.height()),
            ),
            ("frames_out", st.written.to_string()),
            ("frames_rendered", st.rendered.to_string()),
            (
                "ms_per_frame_wall",
                format!("{:.1}", wall.as_secs_f64() * 1e3 / n as f64),
            ),
            (
                "ms_render_per_rendered_frame",
                format!(
                    "{:.1}",
                    st.render.as_secs_f64() * 1e3 / st.rendered.max(1) as f64
                ),
            ),
            (
                "ms_render_max",
                format!("{:.1}", st.render_max.as_secs_f64() * 1e3),
            ),
            (
                "ms_write_per_out_frame",
                format!("{:.1}", st.write.as_secs_f64() * 1e3 / n as f64),
            ),
            (
                "ms_write_max",
                format!("{:.1}", st.write_max.as_secs_f64() * 1e3),
            ),
            (
                "held_drawings_max_mb",
                format!("{:.0}", st.held_max as f64 / 1e6),
            ),
            ("ms_merge", format!("{:.1}", st.merge.as_secs_f64() * 1e3)),
            (
                "window_feed_max_ms",
                format!("{:.2}", e.exporter.feed_max.as_secs_f64() * 1e3),
            ),
            (
                "window_feed_total_ms",
                format!("{:.1}", e.exporter.feed_total.as_secs_f64() * 1e3),
            ),
            (
                "window_tick_p99_ms",
                format!("{:.2}", p99.as_secs_f64() * 1e3),
            ),
            (
                "window_tick_ms",
                format!(
                    "p50 {:.2} p90 {:.2} p99 {:.2} max {:.2}",
                    q(&ticks, 0.5).as_secs_f64() * 1e3,
                    q(&ticks, 0.9).as_secs_f64() * 1e3,
                    q(&ticks, 0.99).as_secs_f64() * 1e3,
                    q(&ticks, 1.0).as_secs_f64() * 1e3
                ),
            ),
            ("window_feed_ms", feed_q),
            ("rss_before_mb", format!("{:.0}", rss0 as f64 / 1024.0)),
            ("rss_peak_mb", format!("{:.0}", rss_max as f64 / 1024.0)),
            ("pss_before_mb", format!("{:.0}", pss0 as f64 / 1024.0)),
            ("pss_peak_mb", format!("{:.0}", pss_max as f64 / 1024.0)),
            ("decoded_frames", check.frames.to_string()),
            ("decoded_seconds", format!("{:.3}", check.seconds)),
            (
                "psnr_db",
                check
                    .psnr
                    .iter()
                    .map(|(f, p)| format!("{f}:{p:.1}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        ]);
        eprintln!("{line}");
        report.push(line);
        assert_eq!(check.frames, s.frames);
        assert!(check.psnr.iter().all(|(_, p)| *p > 30.0));
    }
    std::fs::write(out.join("export-fixtures.jsonl"), report.join("\n") + "\n").unwrap();
}

fn serde_line(kv: &[(&str, String)]) -> String {
    let body: Vec<String> = kv
        .iter()
        .map(|(k, v)| format!("\"{k}\": \"{}\"", v.replace('"', "'")))
        .collect();
    format!("{{{}}}", body.join(", "))
}

/// Layers that show the same drawings all through are merged into one for the export (the
/// renderer holds and composites less); the frames come out as the window shows them.
#[test]
fn still_layers_are_merged_and_look_the_same() {
    let (mut e, mut s) = demo();
    // Tie-down and Rough held still, over the static Background; half opacity on one
    for i in [1, 2] {
        s.layers[i].kind = 'S';
    }
    s.sheet.tracks[2].opacity = 0.5;
    e.tick(&mut s, Instant::now());
    let layers: Vec<worker::LayerSetup> = e
        .painter
        .layers()
        .iter()
        .map(|l| worker::LayerSetup {
            id: l.id,
            props: l.props.clone(),
            mask: Vec::new(),
        })
        .collect();
    let plan = Plan::new(&s, 0, s.frames - 1);
    let runs = static_runs(&layers, &plan);
    let bottom3: Vec<u32> = s.sheet.tracks[1..].iter().rev().map(|t| t.layer).collect();
    assert_eq!(runs, vec![bottom3], "the three still layers, bottom up");

    let d = tmp("merged");
    s.export.format = Format::Png;
    s.sheet.loop_range = Some((0, 23));
    s.export.loop_only = true;
    start(&mut e, &mut s, d.join("m.png"));
    run_out(&mut e, &mut s, Duration::from_secs(60));
    assert!(
        matches!(s.export.status, Status::Done(_)),
        "{:?}",
        s.export.status
    );
    for (file, frame) in [("m_0001.png", 0), ("m_0024.png", 23)] {
        let (_, _, px) = read_png(&d.join(file));
        let want = engine_frame(&e, &s, frame);
        let worst = px
            .iter()
            .zip(&want)
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        assert!(worst <= 2, "frame {frame}: off by up to {worst}");
    }
    let _ = std::fs::remove_dir_all(d);
}
