//! QA for Phase 4's exports: the sound in MP4 and WebM (each cut of a project with a sound
//! before the scene, a muted one, gains, an undecodable file, two scenes), a project with no
//! sound, a scaled video, Cancel while the sound is going in; and the frame as a Photoshop
//! file (every blend mode, opacity, clipping, a hidden layer, a mask) with the reference
//! pictures `tools/native-qa/psd_check.py` compares it with.
//!
//! Ignored by a plain `cargo test`; run inside `env -u DISPLAY -u WAYLAND_DISPLAY
//! dbus-run-session -- …` with
//! `PP_QA_FIXTURES=<phase4.py fixtures dir> PP_QA_OUT=<dir> cargo test --release -p pencilpilot-native --test phase4_export_qa -- --include-ignored --test-threads 1`,
//! then `python3 -I tools/native-qa/phase4.py audio-check <video> <fixtures> sound --cut S,C`
//! on the videos and `psd_check.py` on the PSD (as `PP_QA_OUT/README` lists).

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::export::{Format, Status, ops};
use pencilpilot_native::shell::engine::{Engine, files};
use pencilpilot_native::state::{Action, State};

fn out_dir() -> PathBuf {
    let d = std::env::var_os("PP_QA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("pp-p4-export-{}", std::process::id()))
        });
    let _ = std::fs::create_dir_all(&d);
    d
}

fn fixtures() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("PP_QA_FIXTURES")?);
    d.join("sound.ppstudio").exists().then_some(d)
}

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let d = out_dir();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
            std::env::set_var("XDG_STATE_HOME", d.join("state"));
            std::env::set_var("PENCILPILOT_NO_SYSTEM_WINDOWS", "1");
            std::env::set_var("PENCILPILOT_NO_SOUND", "1");
        }
    });
}

fn answer(p: Option<&Path>) {
    unsafe {
        std::env::set_var(
            "PENCILPILOT_DIALOG_ANSWER",
            p.map(|p| p.as_os_str().to_owned()).unwrap_or_default(),
        )
    };
}

fn tick(e: &mut Engine, s: &mut State) {
    let now = Instant::now();
    let due = e
        .next_tick_at(s)
        .unwrap_or(now + Duration::from_millis(16))
        .min(now + Duration::from_millis(16));
    if due > now {
        std::thread::sleep(due - now);
    }
    e.tick(s, Instant::now());
    let _ = e.picture(s);
}

/// A copy of `src` that opens on scene `sc`, cut `cut` (the file's `extra`, as the web app
/// keeps it).
fn on_cut(src: &Path, sc: u64, cut: u64, to: &Path) {
    let mut p = ppstudio::Project::open(src).unwrap();
    if !p.manifest.extra.is_object() {
        p.manifest.extra = serde_json::json!({});
    }
    p.manifest.extra["scene"] = sc.into();
    p.manifest.extra["cut"] = cut.into();
    p.save(to).unwrap();
}

/// The project opened through File › Open, its sounds decoded (or failed).
fn opened(path: &Path) -> (Engine, State) {
    init();
    let mut s = State::demo();
    s.onion = false;
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    for _ in 0..3 {
        tick(&mut e, &mut s);
    }
    answer(Some(path));
    assert!(e.apply(&mut s, &Action::File(files::OPEN.into())));
    let t = Instant::now();
    loop {
        tick(&mut e, &mut s);
        let opened = s.project.path.as_deref() == Some(path);
        let decoded = s
            .sound
            .clips
            .iter()
            .all(|c| c.frames > 0 || c.failed.is_some());
        if opened && decoded {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "{} didn't open: {:?}",
            path.display(),
            s.project.note
        );
    }
    answer(None);
    (e, s)
}

/// Export… as `format` at size `size` to `path`, until done (or `cancel_after` frames, then
/// Cancel). The status at the end and the time it took.
fn export(
    e: &mut Engine,
    s: &mut State,
    format: Format,
    size: usize,
    path: &Path,
    cancel_after: Option<u32>,
) -> (Status, f64) {
    e.apply(s, &Action::File("Export…".into()));
    let t = Instant::now();
    while s.export.videos.is_none() {
        tick(e, s);
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "FFmpeg never looked for"
        );
    }
    s.export.format = format;
    s.export.size = size;
    s.export.transparent = false;
    s.export.loop_only = false;
    answer(Some(path));
    let t = Instant::now();
    e.apply(s, &Action::Other(ops::START.into()));
    let mut cancelled = false;
    loop {
        tick(e, s);
        match &s.export.status {
            Status::Running { done, .. } => {
                if !cancelled && cancel_after.is_some_and(|k| *done >= k) {
                    e.apply(s, &Action::Other(ops::CANCEL.into()));
                    cancelled = true;
                }
            }
            Status::Done(_) | Status::Failed(_) => break,
            Status::Idle if cancelled || t.elapsed() > Duration::from_secs(5) => break,
            _ => {}
        }
        assert!(
            t.elapsed() < Duration::from_secs(300),
            "the export never ended"
        );
    }
    answer(None);
    // (Cancel: let FFmpeg go)
    for _ in 0..30 {
        tick(e, s);
    }
    (s.export.status.clone(), t.elapsed().as_secs_f64() * 1e3)
}

fn ffmpeg_children() -> usize {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|d| d.ok())
        .filter(|d| {
            let p = d.path();
            let stat = std::fs::read_to_string(p.join("stat")).unwrap_or_default();
            let comm = std::fs::read_to_string(p.join("comm")).unwrap_or_default();
            comm.trim() == "ffmpeg"
                && stat
                    .rsplit_once(')')
                    .and_then(|(_, r)| r.split_whitespace().nth(1).map(|x| x == me))
                    .unwrap_or(false)
        })
        .count()
}

/// Each cut of the sound project as MP4 (and WebM for the second), the second cut scaled
/// to half, the project with no sound, and Cancel part way. What came out is listed in
/// `exports.json` (the sound is checked by `phase4.py audio-check`).
#[test]
#[ignore]
fn sound_into_videos() {
    let Some(fix) = fixtures() else {
        eprintln!("skipped: PP_QA_FIXTURES has no sound.ppstudio (phase4.py fixtures)");
        return;
    };
    let d = out_dir();
    let mut rows = Vec::new();
    for (sc, cut, formats) in [
        (0u64, 0u64, vec![(Format::Mp4, 0usize, "mp4")]),
        (
            0,
            1,
            vec![
                (Format::Mp4, 0, "mp4"),
                (Format::Webm, 0, "webm"),
                (Format::Mp4, 1, "half.mp4"),
            ],
        ),
        (1, 0, vec![(Format::Mp4, 0, "mp4")]),
    ] {
        let p = d.join(format!("sound-s{sc}c{cut}.ppstudio"));
        on_cut(&fix.join("sound.ppstudio"), sc, cut, &p);
        let (mut e, mut s) = opened(&p);
        let clips: Vec<String> = s
            .sound
            .clips
            .iter()
            .map(|c| {
                format!(
                    "{{\"file\": {:?}, \"start\": {}, \"frames\": {}, \"failed\": {}}}",
                    c.file,
                    c.start,
                    c.frames,
                    c.failed
                        .as_ref()
                        .map_or("null".to_string(), |f| format!("{f:?}"))
                )
            })
            .collect();
        for (f, size, ext) in formats {
            let out = d.join(format!("sound-s{sc}c{cut}.{ext}"));
            let _ = std::fs::remove_file(&out);
            let (st, ms) = export(&mut e, &mut s, f, size, &out, None);
            rows.push(format!(
                "{{\"scene\": {sc}, \"cut\": {cut}, \"file\": {:?}, \"status\": {:?}, \"ms\": {ms:.0}, \"exists\": {}, \"cutStart\": {}, \"frames\": {}, \"clips\": [{}]}}",
                out.display().to_string(),
                format!("{st:?}"),
                out.exists(),
                s.sound.cut_start,
                s.frames,
                clips.join(", ")
            ));
            assert!(
                matches!(st, Status::Done(_)),
                "{ext} of scene {sc} cut {cut}: {st:?}"
            );
        }
    }
    // no sound at all: a video with no sound
    let (mut e, mut s) = opened(&fix.join("sound-none.ppstudio"));
    let out = d.join("sound-none.mp4");
    let _ = std::fs::remove_file(&out);
    let (st, ms) = export(&mut e, &mut s, Format::Mp4, 0, &out, None);
    rows.push(format!(
        "{{\"file\": {:?}, \"status\": {:?}, \"ms\": {ms:.0}, \"exists\": {}}}",
        out.display().to_string(),
        format!("{st:?}"),
        out.exists()
    ));
    // Cancel after 10 frames of the minute with sound: nothing left, FFmpeg gone
    let (mut e, mut s) = opened(&fix.join("sound-60s.ppstudio"));
    let out = d.join("sound-60s-cancelled.mp4");
    let _ = std::fs::remove_file(&out);
    let (st, ms) = export(&mut e, &mut s, Format::Mp4, 0, &out, Some(10));
    let left: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|x| x.ok())
        .map(|x| x.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("sound-60s-cancelled"))
        .collect();
    let kids = ffmpeg_children();
    rows.push(format!(
        "{{\"cancel\": true, \"status\": {:?}, \"ms\": {ms:.0}, \"left\": {left:?}, \"ffmpegLeft\": {kids}}}",
        format!("{st:?}")
    ));
    let json = format!("[\n {}\n]", rows.join(",\n "));
    std::fs::write(d.join("exports.json"), &json).unwrap();
    eprintln!("{json}");
    assert!(left.is_empty(), "Cancel left {left:?}");
    assert_eq!(kids, 0, "FFmpeg still running after Cancel");
}

/// The frame as a Photoshop file: the blend-mode project `phase4_composite_qa` writes
/// (`PP_QA_OUT/composite.ppstudio`, frame 1, a mask given to its "Masked" layer), 20 layers at
/// 4K (frame 3) and the 40-layer project. Next to each PSD, the CPU's picture of the same
/// frame (`.cpu.raw`, RGBA) for `psd_check.py --composite`.
#[test]
#[ignore]
fn frame_as_psd() {
    let Some(fix) = fixtures() else {
        eprintln!("skipped: PP_QA_FIXTURES not set");
        return;
    };
    let d = out_dir();
    let mut rows = Vec::new();
    let composite = d.join("composite.ppstudio");
    let mut jobs: Vec<(PathBuf, u32, &str)> = vec![
        (fix.join("layers20-4k.ppstudio"), 3, "layers20-4k"),
        (fix.join("layers40.ppstudio"), 0, "layers40"),
    ];
    if composite.exists() {
        jobs.insert(0, (composite, 1, "composite"));
    } else {
        eprintln!("no composite.ppstudio in PP_QA_OUT: run phase4_composite_qa first");
    }
    for (p, frame, name) in jobs {
        let (mut e, mut s) = opened(&p);
        if name == "composite" {
            let i = s.layers.iter().position(|l| l.name == "Masked").unwrap();
            let id = s.sheet.tracks[i].layer;
            let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
            // a grey ramp left to right with a black hole (as phase4_composite_qa's)
            let mut m = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let hole =
                        (x as i32 - 250).abs() < 20 && (y as i32 - (h as i32 - 24)).abs() < 12;
                    m[y * w + x] = if hole { 0 } else { (x * 255 / (w - 1)) as u8 };
                }
            }
            assert!(e.painter.restore_mask(id, &m));
        }
        assert!(e.apply(&mut s, &Action::GoToFrame(frame)));
        for _ in 0..20 {
            tick(&mut e, &mut s);
        }
        let cpu = match e.picture(&s) {
            Some((px, _)) => px.to_vec(),
            None => e.painter.rgba().to_vec(),
        };
        std::fs::write(d.join(format!("{name}.cpu.raw")), &cpu).unwrap();
        let out = d.join(format!("{name}.psd"));
        let _ = std::fs::remove_file(&out);
        let (st, ms) = export(&mut e, &mut s, Format::Psd, 0, &out, None);
        rows.push(format!(
            "{{\"project\": {name:?}, \"frame\": {frame}, \"status\": {:?}, \"ms\": {ms:.0}, \"bytes\": {}, \"size\": [{}, {}], \"layers\": {}}}",
            format!("{st:?}"),
            std::fs::metadata(&out).map_or(0, |m| m.len()),
            e.painter.width(),
            e.painter.height(),
            s.layers.len()
        ));
        assert!(matches!(st, Status::Done(_)), "PSD of {name}: {st:?}");
    }
    let json = format!("[\n {}\n]", rows.join(",\n "));
    std::fs::write(d.join("psd.json"), &json).unwrap();
    eprintln!("{json}");
}
