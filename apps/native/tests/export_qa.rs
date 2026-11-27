//! QA for exporting (Phase 2 step 5): each format through the export dialog's actions, from
//! the test projects opened with File › Open, the file dialog answered by
//! `PENCILPILOT_DIALOG_ANSWER`; the engine ticked as the window ticks it (asleep until
//! `next_tick_at`).
//!
//! - each format writes a file: MP4, WebM (FFmpeg), GIF, a PNG sequence; with `PP_QA_REFS`
//!   (a folder of `ppstudio render <project> <dir>/<name> --every` renders) each is
//!   compared with the file's own frames by `tools/native-qa/export_check.py` (decoded back:
//!   PSNR, mean difference, frame count, duration, frame rate; PNGs exactly);
//! - how long a frame takes (MP4: under 30 ms at 1080p, under 100 ms at 4K) and the whole
//!   process tree's peak memory, FFmpeg included (under 1 GB);
//! - Cancel stops FFmpeg and leaves no partial file; an FFmpeg that fails part way; no
//!   FFmpeg at all (`ffmpeg_missing`, run with FFmpeg hidden: see run.sh).
//!
//! Ignored by a plain `cargo test`; run with `tools/native-qa/run.sh` or
//! `cargo test --release -p pencilpilot-native --test export_qa -- --include-ignored --test-threads 1`.
//! `PP_QA_FIXTURES` = the test projects (`tools/baseline/fixtures.py`); without them, or
//! without FFmpeg, the tests that need them skip. `PP_QA_OUT` = a folder for the results.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::export::{Format, Status, ops};
use pencilpilot_native::shell::engine::{Engine, files};
use pencilpilot_native::state::{Action, State};

fn fixtures() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("PP_QA_FIXTURES")?);
    d.join("drawings500.ppstudio").exists().then_some(d)
}

macro_rules! need_fixtures {
    () => {
        match fixtures() {
            Some(d) => d,
            None => {
                eprintln!("skipped: PP_QA_FIXTURES isn't a folder of test projects");
                return;
            }
        }
    };
}

macro_rules! need_ffmpeg {
    () => {
        if pencilpilot_export::find_ffmpeg().is_none() {
            eprintln!("skipped: no FFmpeg");
            return;
        }
    };
}

fn out_dir() -> PathBuf {
    let d = std::env::var_os("PP_QA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("pp-export-qa-{}", std::process::id()))
        });
    let _ = std::fs::create_dir_all(&d);
    d
}

fn save_json(name: &str, json: &str) {
    let _ = std::fs::write(out_dir().join(name), json);
    eprintln!("{name}: {json}");
}

fn answer(p: Option<&Path>) {
    // (the tests run on one thread: --test-threads 1)
    unsafe {
        std::env::set_var(
            "PENCILPILOT_DIALOG_ANSWER",
            p.map(|p| p.as_os_str().to_owned()).unwrap_or_default(),
        )
    };
}

/// Recent files and recovery copies go to the QA folder, never the user's own.
fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let d = out_dir();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
            std::env::set_var("XDG_STATE_HOME", d.join("state"));
        }
    });
}

/// One tick as the window runs it: asleep until the engine's next tick is due (at most
/// `cap`), then tick and draw.
fn window_tick(e: &mut Engine, s: &mut State, cap: Duration) {
    let now = Instant::now();
    let due = e.next_tick_at(s).unwrap_or(now + cap).min(now + cap);
    if due > now {
        std::thread::sleep(due - now);
    }
    e.tick(s, Instant::now());
    let _ = e.picture(s);
}

/// A test project opened through File › Open.
fn opened(path: &Path) -> (Engine, State) {
    init();
    let mut s = State::demo();
    s.onion = false;
    let mut e = Engine::new(1920, 1080);
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    let m = ppstudio::Project::peek(path).unwrap().0;
    answer(Some(path));
    let id = s.doc_id;
    e.apply(&mut s, &Action::File(files::OPEN.into()));
    let t = Instant::now();
    while !(s.doc_id != id && e.painter.width() == m.canvas.width) {
        e.tick(&mut s, Instant::now());
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "{} didn't open",
            path.display()
        );
    }
    let _ = e.picture(&s);
    (e, s)
}

// ------------------------------------------------------------------------------------------
// memory: this process and its children (FFmpeg)

fn children_of(pid: u32) -> Vec<u32> {
    let mut out = vec![pid];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        if let Ok(rd) = std::fs::read_dir(format!("/proc/{p}/task")) {
            for t in rd.flatten() {
                if let Ok(c) = std::fs::read_to_string(t.path().join("children")) {
                    out.extend(c.split_whitespace().filter_map(|x| x.parse::<u32>().ok()));
                }
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

fn pss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("Pss:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

/// Samples the process tree's PSS every 20 ms while it runs: (peak total, peak of the
/// children, KiB).
struct Sampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    peak_kids: Arc<AtomicU64>,
    h: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    fn start() -> Sampler {
        let (stop, peak, peak_kids) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        );
        let (s2, p2, k2) = (stop.clone(), peak.clone(), peak_kids.clone());
        let me = std::process::id();
        let h = std::thread::spawn(move || {
            while !s2.load(Ordering::Relaxed) {
                let tree = children_of(me);
                let kids: u64 = tree.iter().filter(|&&p| p != me).map(|&p| pss_kib(p)).sum();
                let all = kids + pss_kib(me);
                p2.fetch_max(all, Ordering::Relaxed);
                k2.fetch_max(kids, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Sampler {
            stop,
            peak,
            peak_kids,
            h: Some(h),
        }
    }

    fn finish(mut self) -> (f64, f64) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.h.take() {
            let _ = h.join();
        }
        (
            self.peak.load(Ordering::Relaxed) as f64 / 1024.0,
            self.peak_kids.load(Ordering::Relaxed) as f64 / 1024.0,
        )
    }
}

fn ffmpeg_children() -> Vec<u32> {
    let me = std::process::id();
    children_of(me)
        .into_iter()
        .filter(|&p| p != me)
        .filter(|p| {
            std::fs::read_to_string(format!("/proc/{p}/comm")).is_ok_and(|c| c.contains("ffmpeg"))
        })
        .collect()
}

// ------------------------------------------------------------------------------------------
// exporting

struct Run {
    status: Status,
    ms: f64,
    total: u32,
    /// the most one tick took while exporting (the window's loop pays it)
    tick_max_ms: f64,
    ticks: usize,
    peak_mib: f64,
    peak_ffmpeg_mib: f64,
    before_mib: f64,
}

/// Export… with these settings to `path`: until it's done or failed (or `stop_after`
/// output frames, then Cancel).
fn export(
    e: &mut Engine,
    s: &mut State,
    format: Format,
    size: usize,
    transparent: bool,
    path: &Path,
    stop_after: Option<u32>,
) -> Run {
    e.apply(s, &Action::File("Export…".into()));
    let t = Instant::now();
    while s.export.videos.is_none() {
        window_tick(e, s, Duration::from_millis(5));
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "FFmpeg never looked for"
        );
    }
    assert!(s.export.open, "Export… opens the dialog");
    s.export.format = format;
    s.export.size = size;
    s.export.transparent = transparent;
    s.export.loop_only = false;
    answer(Some(path));
    let before = pss_kib(std::process::id()) as f64 / 1024.0;
    let sampler = Sampler::start();
    let t = Instant::now();
    e.apply(s, &Action::Other(ops::START.into()));
    let (mut tick_max, mut ticks) = (0f64, 0);
    let mut total = 0;
    loop {
        let t1 = Instant::now();
        window_tick(e, s, Duration::from_millis(16));
        tick_max = tick_max.max(t1.elapsed().as_secs_f64() * 1000.0);
        ticks += 1;
        match &s.export.status {
            Status::Running { done, total: n } => {
                total = *n;
                if stop_after.is_some_and(|k| *done >= k) {
                    e.apply(s, &Action::Other(ops::CANCEL.into()));
                }
            }
            Status::Asking | Status::Idle => {}
            Status::Done(_) | Status::Failed(_) => break,
        }
        if stop_after.is_some() && matches!(s.export.status, Status::Idle) && ticks > 3 {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(600),
            "the export never ended"
        );
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (peak, kids) = sampler.finish();
    Run {
        status: s.export.status.clone(),
        ms,
        total,
        tick_max_ms: (tick_max * 10.0).round() / 10.0,
        ticks,
        peak_mib: peak,
        peak_ffmpeg_mib: kids,
        before_mib: before,
    }
}

/// `export_check.py` on what was written, against the project's renders (`PP_QA_REFS`).
fn check(kind: &str, what: &Path, project: &str) -> String {
    let Some(refs) = std::env::var_os("PP_QA_REFS").map(PathBuf::from) else {
        return "null".into();
    };
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/native-qa/export_check.py");
    let out = std::process::Command::new("python3")
        .args([
            "-I".as_ref(),
            script.as_os_str(),
            kind.as_ref(),
            what.as_os_str(),
            refs.join(project).as_os_str(),
            "--brief".as_ref(),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("{:?}", String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => format!("{:?}", e.to_string()),
    }
}

fn run_json(r: &Run) -> String {
    format!(
        r#""status":{:?},"ms":{:.0},"frames":{},"msPerFrame":{:.1},"tickMaxMs":{},"ticks":{},"pssBeforeMiB":{:.0},"peakTreeMiB":{:.0},"peakFfmpegMiB":{:.0}"#,
        format!("{:?}", r.status),
        r.ms,
        r.total,
        r.ms / r.total.max(1) as f64,
        r.tick_max_ms,
        r.ticks,
        r.before_mib,
        r.peak_mib,
        r.peak_ffmpeg_mib
    )
}

/// Every format from both test projects at their own size: a file each, compared with
/// the file's frames; per-frame times and peak memory.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn each_format_from_each_project() {
    let dir = need_fixtures!();
    need_ffmpeg!();
    let mut res = Vec::new();
    for name in ["drawings500", "layers20-4k"] {
        for (format, ext, kind) in [
            (Format::Mp4, "mp4", "video"),
            (Format::Webm, "webm", "video"),
            (Format::Gif, "gif", "gif"),
            (Format::Png, "png", "pngseq"),
        ] {
            // (a fresh engine each: the memory measured is one export's)
            let (mut e, mut s) = opened(&dir.join(format!("{name}.ppstudio")));
            let sub = out_dir().join(format!("{name}-{ext}"));
            let _ = std::fs::remove_dir_all(&sub);
            std::fs::create_dir_all(&sub).unwrap();
            let path = sub.join(format!("{name}.{ext}"));
            let r = export(&mut e, &mut s, format, 0, false, &path, None);
            let written = match &r.status {
                Status::Done(p) => p.clone(),
                other => panic!("{name} {ext}: {other:?}"),
            };
            let target = if kind == "pngseq" {
                sub.clone()
            } else {
                written.clone()
            };
            let files: Vec<_> = std::fs::read_dir(&sub)
                .unwrap()
                .flatten()
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .collect();
            let c = check(kind, &target, name);
            res.push(format!(
                r#""{name}-{ext}":{{{},"written":{:?},"files":{},"firstFiles":{:?},"bytes":{},"check":{c}}}"#,
                run_json(&r),
                written.display().to_string(),
                files.len(),
                {
                    let mut f = files.clone();
                    f.sort();
                    f.truncate(3);
                    f
                },
                std::fs::read_dir(&sub).unwrap().flatten().map(|f| f.metadata().map_or(0, |m| m.len())).sum::<u64>(),
            ));
            save_json("export-formats.json", &format!("{{{}}}", res.join(",")));
        }
    }
}

/// Transparent PNG and GIF: no white behind (alpha kept), and the same picture over
/// white; and the sizes offered (half, 720p) for MP4.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn transparency_and_sizes() {
    let dir = need_fixtures!();
    need_ffmpeg!();
    let mut res = Vec::new();
    let path = dir.join("drawings500.ppstudio");
    for (format, ext, transparent, size) in [
        (Format::Png, "png", true, 0),
        (Format::Gif, "gif", true, 0),
        (Format::Mp4, "mp4", false, 1),
        (Format::Mp4, "mp4", false, 2),
    ] {
        let (mut e, mut s) = opened(&path);
        let sub = out_dir().join(format!("d500-{ext}-t{}-s{size}", transparent as u8));
        let _ = std::fs::remove_dir_all(&sub);
        std::fs::create_dir_all(&sub).unwrap();
        let out = sub.join(format!("d500.{ext}"));
        let r = export(&mut e, &mut s, format, size, transparent, &out, None);
        let Status::Done(written) = &r.status else {
            panic!("{ext}: {:?}", r.status)
        };
        let kind = match ext {
            "png" => "pngseq",
            "gif" => "gif",
            _ => "video",
        };
        let target = if kind == "pngseq" {
            sub.clone()
        } else {
            written.clone()
        };
        // is there transparency in the first PNG?
        let alpha = if kind == "pngseq" {
            let mut names: Vec<_> = std::fs::read_dir(&sub)
                .unwrap()
                .flatten()
                .map(|f| f.path())
                .collect();
            names.sort();
            let dec = png::Decoder::new(std::io::BufReader::new(
                std::fs::File::open(&names[0]).unwrap(),
            ));
            let info = dec.read_info().unwrap();
            format!("{:?}", info.info().color_type)
        } else {
            "null".into()
        };
        res.push(format!(
            r#""{ext}-transparent{transparent}-size{size}":{{{},"pngColour":{alpha:?},"check":{}}}"#,
            run_json(&r),
            check(kind, &target, "drawings500")
        ));
    }
    save_json("export-options.json", &format!("{{{}}}", res.join(",")));
}

/// Cancel part way (MP4 of 20 layers at 4K, after 5 frames): FFmpeg stops, no partial
/// file is left, the dialog can export again.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn cancel_stops_ffmpeg_and_removes_the_file() {
    let dir = need_fixtures!();
    need_ffmpeg!();
    let mut res = Vec::new();
    for (format, ext) in [
        (Format::Mp4, "mp4"),
        (Format::Webm, "webm"),
        (Format::Gif, "gif"),
        (Format::Png, "png"),
    ] {
        let (mut e, mut s) = opened(&dir.join("layers20-4k.ppstudio"));
        let sub = out_dir().join(format!("cancel-{ext}"));
        let _ = std::fs::remove_dir_all(&sub);
        std::fs::create_dir_all(&sub).unwrap();
        let path = sub.join(format!("cut.{ext}"));
        let mut saw_ffmpeg = false;
        e.apply(&mut s, &Action::File("Export…".into()));
        while s.export.videos.is_none() {
            window_tick(&mut e, &mut s, Duration::from_millis(5));
        }
        s.export.format = format;
        s.export.size = 0;
        answer(Some(&path));
        e.apply(&mut s, &Action::Other(ops::START.into()));
        let t = Instant::now();
        loop {
            window_tick(&mut e, &mut s, Duration::from_millis(16));
            saw_ffmpeg |= !ffmpeg_children().is_empty();
            if let Status::Running { done, .. } = s.export.status
                && done >= 5
            {
                break;
            }
            assert!(
                t.elapsed() < Duration::from_secs(120),
                "{ext}: 5 frames never came: {:?}",
                s.export.status
            );
        }
        let had_file = std::fs::read_dir(&sub).unwrap().count();
        let t = Instant::now();
        e.apply(&mut s, &Action::Other(ops::CANCEL.into()));
        let mut ended = None;
        while t.elapsed() < Duration::from_secs(10) {
            window_tick(&mut e, &mut s, Duration::from_millis(16));
            if !s.export.busy() && ended.is_none() {
                ended = Some(t.elapsed());
            }
            if ended.is_some() && ffmpeg_children().is_empty() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        let left: Vec<_> = std::fs::read_dir(&sub)
            .unwrap()
            .flatten()
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        let ffmpeg_left = ffmpeg_children();
        res.push(format!(
            r#""{ext}":{{"status":{:?},"cancelToIdleMs":{:?},"sawFfmpeg":{saw_ffmpeg},"filesDuring":{had_file},"filesLeft":{left:?},"ffmpegLeft":{ffmpeg_left:?}}}"#,
            format!("{:?}", s.export.status),
            ended.map(|d| d.as_millis())
        ));
        assert!(
            ended.is_some(),
            "{ext}: Cancel didn't stop it: {:?}",
            s.export.status
        );
        assert!(
            ffmpeg_left.is_empty(),
            "{ext}: FFmpeg still running after Cancel"
        );
        assert!(left.is_empty(), "{ext}: Cancel left {left:?}");
        // and it can export again
        let again = sub.join(format!("again.{ext}"));
        let r = export(&mut e, &mut s, format, 1, false, &again, None);
        assert!(
            matches!(r.status, Status::Done(_)),
            "{ext}: after Cancel: {:?}",
            r.status
        );
    }
    save_json("export-cancel.json", &format!("{{{}}}", res.join(",")));
}

/// An FFmpeg that fails part way (a stand-in that reads a little and exits 1): a message,
/// no partial file, no panic; one that can't encode anything: videos not offered.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn ffmpeg_failing() {
    let dir = need_fixtures!();
    need_ffmpeg!();
    let real = pencilpilot_export::find_ffmpeg().unwrap();
    let bin = out_dir().join("fake-ffmpeg");
    std::fs::create_dir_all(&bin).unwrap();
    // fails once frames come: answers what it's asked about encoders as the real one does
    let dying = bin.join("dying");
    std::fs::write(
        &dying,
        format!(
            "#!/bin/sh\ncase \"$*\" in *-encoders*|*-version*) exec {} \"$@\";; esac\nhead -c 30000000 >/dev/null\necho 'Conversion failed!' >&2\nexit 1\n",
            real.display()
        ),
    )
    .unwrap();
    let broken = bin.join("broken");
    std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
    for f in [&dying, &broken] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut res = Vec::new();
    unsafe { std::env::set_var("PENCILPILOT_FFMPEG", &dying) };
    let (mut e, mut s) = opened(&dir.join("drawings500.ppstudio"));
    let path = out_dir().join("dying.mp4");
    let _ = std::fs::remove_file(&path);
    let r = export(&mut e, &mut s, Format::Mp4, 0, false, &path, None);
    let left = path.exists() || path.with_extension("mp4.part").exists();
    res.push(format!(
        r#""diesPartWay":{{{},"fileLeft":{left}}}"#,
        run_json(&r)
    ));
    assert!(
        matches!(r.status, Status::Failed(_)),
        "a failing FFmpeg: {:?}",
        r.status
    );
    unsafe { std::env::set_var("PENCILPILOT_FFMPEG", &broken) };
    let (mut e, mut s) = opened(&dir.join("drawings500.ppstudio"));
    e.apply(&mut s, &Action::File("Export…".into()));
    let t = Instant::now();
    while s.export.videos.is_none() && t.elapsed() < Duration::from_secs(10) {
        window_tick(&mut e, &mut s, Duration::from_millis(5));
    }
    res.push(format!(
        r#""cantEncode":{{"videos":{:?},"format":{:?},"mp4Available":{}}}"#,
        format!("{:?}", s.export.videos),
        format!("{:?}", s.export.format),
        s.export.available(Format::Mp4)
    ));
    unsafe { std::env::remove_var("PENCILPILOT_FFMPEG") };
    save_json(
        "export-ffmpeg-failing.json",
        &format!("{{{}}}", res.join(",")),
    );
}

/// No FFmpeg anywhere (run with it hidden, e.g. `bwrap --dev-bind / / --bind /dev/null
/// /usr/bin/ffmpeg …`; with FFmpeg found it skips): video formats aren't offered, asking
/// for one anyway says why, GIF and PNG still export, nothing panics.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn ffmpeg_missing() {
    let dir = need_fixtures!();
    if pencilpilot_export::find_ffmpeg().is_some() {
        eprintln!("skipped: FFmpeg is here (hide it to run this)");
        return;
    }
    let (mut e, mut s) = opened(&dir.join("drawings500.ppstudio"));
    e.apply(&mut s, &Action::File("Export…".into()));
    let t = Instant::now();
    while s.export.videos.is_none() && t.elapsed() < Duration::from_secs(10) {
        window_tick(&mut e, &mut s, Duration::from_millis(5));
    }
    let offered = format!("{:?} format {:?}", s.export.videos, s.export.format);
    // MP4 asked for anyway
    s.export.format = Format::Mp4;
    let path = out_dir().join("noffmpeg.mp4");
    let _ = std::fs::remove_file(&path);
    answer(Some(&path));
    e.apply(&mut s, &Action::Other(ops::START.into()));
    for _ in 0..50 {
        window_tick(&mut e, &mut s, Duration::from_millis(10));
        if matches!(s.export.status, Status::Done(_) | Status::Failed(_)) {
            break;
        }
    }
    let mp4 = format!("{:?} (format then {:?})", s.export.status, s.export.format);
    let gif = out_dir().join("noffmpeg.gif");
    let r = export(&mut e, &mut s, Format::Gif, 1, false, &gif, None);
    save_json(
        "export-ffmpeg-missing.json",
        &format!(
            r#"{{"offered":{offered:?},"mp4Anyway":{mp4:?},"gif":{{{}}}}}"#,
            run_json(&r)
        ),
    );
    assert!(s.export.videos.as_ref().is_some_and(|v| v.is_empty()));
    assert!(
        matches!(r.status, Status::Done(_)),
        "GIF without FFmpeg: {:?}",
        r.status
    );
}

/// Where the export dialog's Export button is in the app's default 1280 × 800 window (for
/// the windowed checks, which click it): a sweep of clicks with the dialog open, through the
/// interface as the window builds it. Writes `export-button.json`.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn export_button_position() {
    use pencilpilot_gpu::{Error, Gpu, Renderer2d};
    use pencilpilot_native::shell;
    use pencilpilot_ui::ui::Ui;
    let (w, h) = (1280.0, 800.0);
    let gpu = match Gpu::new() {
        Ok(g) => g,
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => {
            eprintln!("skipped: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let mut r = Renderer2d::new(&gpu).unwrap();
    let mut ui = Ui::new(1.0);
    ui.text.wait_for_fonts();
    let mut found = None;
    let mut webm = None;
    'sweep: for gy in (0..(h as i32)).step_by(8) {
        for gx in ((w as i32 / 2)..(w as i32)).step_by(8) {
            let mut s = State::demo();
            s.export.open = true;
            s.export.videos = Some(vec![Format::Mp4, Format::Webm]);
            let mut pass = |ui: &mut Ui, s: &mut State| {
                let _ = ui.pass(&mut r, w, h, |f| {
                    shell::build(f, s, w, h);
                });
            };
            pass(&mut ui, &mut s);
            pass(&mut ui, &mut s);
            let (x, y) = (gx as f32, gy as f32);
            ui.pointer_moved(x, y);
            pass(&mut ui, &mut s);
            ui.pointer_button(true);
            pass(&mut ui, &mut s);
            ui.pointer_button(false);
            pass(&mut ui, &mut s);
            if webm.is_none() && s.export.format == Format::Webm {
                webm = Some((x, y));
            }
            if s.actions
                .iter()
                .any(|a| *a == Action::Other(ops::START.into()))
            {
                found = Some((x, y));
                if webm.is_some() {
                    break 'sweep;
                }
            }
            // (the pointer leaves, so the next start is clean)
            ui.pointer_moved(1.0, 1.0);
        }
    }
    let (x, y) = found.expect("no Export button in the dialog");
    let (wx, wy) = webm.expect("no WebM choice in the dialog");
    // (a few pixels inside the first hit)
    save_json(
        "export-button.json",
        &format!(
            r#"{{"export":[{},{}],"webm":[{},{}]}}"#,
            x + 6.0,
            y + 6.0,
            wx + 6.0,
            wy + 6.0
        ),
    );
}
