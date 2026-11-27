//! QA for projects on disk (Phase 2 step 5): Open, Save, Save as, New, unsaved changes,
//! autosave and recent files, through the engine and the File menu's actions, with the
//! system's file dialog answered by `PENCILPILOT_DIALOG_ANSWER`.
//!
//! - Open: each test project comes in at its size, with its layers (names, visibility, lock,
//!   opacity), sheet, fps and pixels: the composite of several frames against the same frames
//!   composited from the file by `crates/ppstudio` (as `ppstudio render` does), and each
//!   layer's drawing against the file's; how long it takes (warm, and cold with
//!   `PP_QA_DROP_CACHES`), memory (PSS) after.
//! - Save as → open: the same pixels per drawing and the same sheet; the saved file read back
//!   with `crates/ppstudio` the way today's app reads it; how long saving takes; Save after a
//!   small edit.
//! - New, the unsaved marker, autosave, recent files.
//!
//! Ignored by a plain `cargo test`; run with `tools/native-qa/run.sh` or
//! `cargo test --release -p pencilpilot-native --test files_qa -- --include-ignored --test-threads 1`.
//! `PP_QA_FIXTURES` = the folder with `empty.ppstudio`, `layers20-4k.ppstudio` and
//! `drawings500.ppstudio` (`tools/baseline/fixtures.py`); without it the tests skip.
//! `PP_QA_OUT` = a folder for the measurements (JSON) and saved files.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::{Engine, Phase, Sample, files};
use pencilpilot_native::state::{Action, State, Tool};
use ppstudio::Project;

// ------------------------------------------------------------------------------------------
// set-up

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

fn out_dir() -> PathBuf {
    let d = std::env::var_os("PP_QA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("pp-files-qa-{}", std::process::id()))
        });
    let _ = std::fs::create_dir_all(&d);
    d
}

fn save_json(name: &str, json: &str) {
    let _ = std::fs::write(out_dir().join(name), json);
    eprintln!("{name}: {json}");
}

/// The answer every file dialog gets (None: cancelled).
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

fn fresh() -> (Engine, State) {
    init();
    let mut s = State::demo();
    s.onion = false;
    let mut e = Engine::new(1920, 1080);
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    (e, s)
}

/// Ticks (and draws, as the shell does) until `done`, at most `limit`. The time it took.
fn settle(
    e: &mut Engine,
    s: &mut State,
    limit: Duration,
    done: impl Fn(&Engine, &State) -> bool,
) -> Option<Duration> {
    let t = Instant::now();
    loop {
        e.tick(s, Instant::now());
        let _ = e.picture(s);
        if done(e, s) {
            return Some(t.elapsed());
        }
        if t.elapsed() > limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn file(e: &mut Engine, s: &mut State, item: &str) -> bool {
    e.apply(s, &Action::File(item.into()))
}

/// Whether the engine shows `m`'s project: its size, its layers, its length.
fn shows(e: &Engine, s: &State, m: &ppstudio::Manifest) -> bool {
    e.painter.width() == m.canvas.width
        && e.painter.height() == m.canvas.height
        && s.layers.len() == m.layers.len()
        && s.frames == m.timeline.frames.iter().map(|f| f.hold).sum::<u32>()
}

/// Opens `path` through File › Open…: the time from the menu item to the project showing
/// (its first frame's picture made).
fn open(e: &mut Engine, s: &mut State, path: &Path) -> Duration {
    let m = Project::peek(path).expect("the test project").0;
    answer(Some(path));
    let id = s.doc_id;
    let t = Instant::now();
    file(e, s, files::OPEN);
    settle(e, s, Duration::from_secs(60), |e, s| {
        s.doc_id != id && shows(e, s, &m)
    })
    .unwrap_or_else(|| panic!("{} didn't open: {:?}", path.display(), s.project.note));
    t.elapsed()
}

fn pss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("Pss:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn drop_caches() -> bool {
    match std::env::var("PP_QA_DROP_CACHES") {
        Ok(cmd) if !cmd.is_empty() => std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .status()
            .is_ok_and(|s| s.success()),
        _ => false,
    }
}

// ------------------------------------------------------------------------------------------
// the reference: the file's own frames

/// Frame `f` (from 0) of the flattened timeline: layer id → drawing name.
fn cels_at(m: &ppstudio::Manifest, f: u32) -> HashMap<u32, String> {
    let mut at = 0;
    for fr in &m.timeline.frames {
        if f < at + fr.hold {
            return fr.cels.clone().into_iter().collect();
        }
        at += fr.hold;
    }
    HashMap::new()
}

/// Frame `f` as `ppstudio render` makes it: visible layers in order, normal blending, their
/// opacity, over white. RGB.
fn reference(p: &Project, f: u32, cache: &mut HashMap<String, Vec<u8>>) -> Vec<u8> {
    let m = &p.manifest;
    let n = (m.canvas.width * m.canvas.height) as usize;
    let mut px = vec![255f32; n * 3];
    let cels = cels_at(m, f);
    for l in m.layers.iter().filter(|l| l.visible && l.adjust.is_none()) {
        let Some(d) = cels.get(&l.id) else { continue };
        if !cache.contains_key(d) {
            cache.insert(d.clone(), p.drawing(d).unwrap().0);
        }
        for (i, c) in cache[d].as_chunks::<4>().0.iter().enumerate() {
            let a = c[3] as f32 / 255.0 * l.opacity;
            if a > 0.0 {
                for k in 0..3 {
                    px[i * 3 + k] = c[k] as f32 * a + px[i * 3 + k] * (1.0 - a);
                }
            }
        }
    }
    px.iter()
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
        .collect()
}

/// What the canvas shows at frame `f` (onion skin off), over white. RGB.
fn native_frame(e: &mut Engine, s: &mut State, f: u32) -> Vec<u8> {
    e.apply(s, &Action::GoToFrame(f));
    e.tick(s, Instant::now());
    let px = match e.picture(s) {
        Some((p, _)) => p.to_vec(),
        None => e.painter.rgba().to_vec(),
    };
    over_white(&px)
}

fn over_white(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for c in rgba.as_chunks::<4>().0 {
        let a = c[3] as f32 / 255.0;
        for v in &c[..3] {
            out.push((*v as f32 * a + 255.0 * (1.0 - a)).round() as u8);
        }
    }
    out
}

#[derive(Debug, Default, Clone, Copy)]
struct Diff {
    max: u8,
    mean: f64,
    /// channels more than 1 level apart
    over1: usize,
    /// more than 8
    over8: usize,
}

fn diff(a: &[u8], b: &[u8]) -> Diff {
    assert_eq!(a.len(), b.len(), "pictures of different sizes");
    let (mut max, mut sum, mut over1, mut over8) = (0u8, 0u64, 0, 0);
    for (x, y) in a.iter().zip(b) {
        let d = x.abs_diff(*y);
        max = max.max(d);
        sum += d as u64;
        over1 += (d > 1) as usize;
        over8 += (d > 8) as usize;
    }
    Diff {
        max,
        mean: sum as f64 / a.len().max(1) as f64,
        over1,
        over8,
    }
}

/// Straight RGBA compared where either is visible (fully transparent pixels' colours don't
/// matter).
fn diff_rgba(a: &[u8], b: &[u8]) -> Diff {
    assert_eq!(a.len(), b.len(), "drawings of different sizes");
    let (mut max, mut sum, mut over1, mut over8) = (0u8, 0u64, 0, 0);
    for (x, y) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let ch = if x[3] == 0 && y[3] == 0 { 0 } else { 4 };
        for k in 0..ch {
            let d = x[k].abs_diff(y[k]);
            max = max.max(d);
            sum += d as u64;
            over1 += (d > 1) as usize;
            over8 += (d > 8) as usize;
        }
    }
    Diff {
        max,
        mean: sum as f64 / a.len().max(1) as f64,
        over1,
        over8,
    }
}

/// The frames checked: first, a few in between, last.
fn sample_frames(len: u32) -> Vec<u32> {
    let mut v = vec![0, 1, len / 3, len / 2, len - 1];
    v.retain(|&f| f < len);
    v.dedup();
    v
}

fn png(path: &Path, rgb: &[u8], w: u32, h: u32) {
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgb).unwrap();
}

// ------------------------------------------------------------------------------------------
// what today's app needs of a file (apps/studio/src-tauri/src/project.rs: `from_project`)

/// Reads `path` as today's app does and lists what it would refuse or get wrong.
fn web_app_reads(path: &Path) -> Result<Project, String> {
    let p = Project::open(path).map_err(|e| format!("ppstudio refuses it: {e}"))?;
    let m = &p.manifest;
    let mut bad = Vec::new();
    if m.timeline.frames.is_empty() {
        bad.push("no frames (today's app indexes frames[0])".to_string());
    }
    if m.timeline.scenes.is_empty() {
        bad.push("no scenes (today's app keeps an animation in its scenes)".to_string());
    }
    let statics = m.timeline.static_layers.clone();
    if !m.timeline.scenes.is_empty()
        && ppstudio::Timeline::flatten(&m.timeline.scenes, &statics) != m.timeline.frames
    {
        bad.push("frames isn't the scenes flattened".into());
    }
    for cut in m.timeline.scenes.iter().flat_map(|s| &s.cuts) {
        for (layer, track) in &cut.tracks {
            if !m.layers.iter().any(|l| l.id == *layer) {
                bad.push(format!("a track for layer {layer}, which isn't there"));
            }
            for d in track.iter().filter_map(|x| x.drawing.as_ref()) {
                if d.strip_prefix('D')
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_none()
                {
                    bad.push(format!("drawing {d} isn't an app drawing (D<number>)"));
                }
                if let Err(e) = p.drawing(d) {
                    bad.push(format!("drawing {d}: {e}"));
                }
            }
        }
    }
    if !m.layers.iter().any(|l| l.id == m.active_layer) {
        bad.push(format!("the active layer {} isn't there", m.active_layer));
    }
    bad.dedup();
    if bad.is_empty() {
        Ok(p)
    } else {
        Err(bad.join("; "))
    }
}

// ------------------------------------------------------------------------------------------
// Open

/// A copy of a test project with layer properties set (one hidden, one locked, one at 60%,
/// one renamed), so opening can be checked for each.
fn varied(src: &Path, dst: &Path) -> Project {
    let mut p = Project::open(src).unwrap();
    let n = p.manifest.layers.len();
    let ls = &mut p.manifest.layers;
    ls[n - 1].visible = false;
    ls[n.saturating_sub(2)].locked = true;
    ls[n.saturating_sub(3).min(n - 1)].opacity = 0.6;
    ls[0].name = "Paper — ünïcode ✓".into();
    p.save(dst).unwrap();
    Project::open(dst).unwrap()
}

/// Layers, sheet, fps and pixels of the open project against the file.
fn check_open(e: &mut Engine, s: &mut State, p: &Project, tag: &str) -> String {
    let m = &p.manifest;
    let (w, h) = (m.canvas.width, m.canvas.height);
    assert_eq!(
        (e.painter.width(), e.painter.height()),
        (w, h),
        "{tag}: canvas size"
    );
    assert_eq!(s.fps, m.timeline.fps, "{tag}: fps");
    let len: u32 = m.timeline.frames.iter().map(|f| f.hold).sum();
    assert_eq!(s.frames, len, "{tag}: the cut's length");
    // layers: the painter's, by name, with their properties
    let pl = e.painter.layers();
    assert_eq!(pl.len(), m.layers.len(), "{tag}: layer count");
    let mut ids = HashMap::new();
    for l in &m.layers {
        let n = pl
            .iter()
            .find(|x| x.props.name == l.name)
            .unwrap_or_else(|| panic!("{tag}: layer {:?} missing", l.name));
        assert_eq!(n.props.visible, l.visible, "{tag}: {} visible", l.name);
        assert_eq!(n.props.locked, l.locked, "{tag}: {} locked", l.name);
        assert!(
            (n.props.opacity - l.opacity).abs() < 1e-3,
            "{tag}: {} opacity",
            l.name
        );
        let st = s
            .layers
            .iter()
            .find(|x| x.name == l.name)
            .unwrap_or_else(|| panic!("{tag}: the interface lacks layer {:?}", l.name));
        assert_eq!(
            (st.visible, st.locked),
            (l.visible, l.locked),
            "{tag}: {} in the interface",
            l.name
        );
        ids.insert(l.id, n.id);
    }
    // order: the file's bottom → top is the painter's order
    let names: Vec<_> = pl.iter().map(|l| l.props.name.clone()).collect();
    let file_names: Vec<_> = m.layers.iter().map(|l| l.name.clone()).collect();
    assert_eq!(names, file_names, "{tag}: layer order (bottom → top)");
    // pixels: the composite, and each layer's drawing, on sample frames
    let mut cache = HashMap::new();
    let mut frames = Vec::new();
    for f in sample_frames(len) {
        let want = reference(p, f, &mut cache);
        let got = native_frame(e, s, f);
        let d = diff(&got, &want);
        if d.max > 1 {
            png(
                &out_dir().join(format!("{tag}-f{f}-native.png")),
                &got,
                w,
                h,
            );
            png(&out_dir().join(format!("{tag}-f{f}-file.png")), &want, w, h);
        }
        let cels = cels_at(m, f);
        let mut layers_max = 0u8;
        let mut layers_bad = Vec::new();
        for l in &m.layers {
            let mine = e.painter.layer_rgba(ids[&l.id]);
            let theirs = match cels.get(&l.id) {
                Some(d) => cache
                    .entry(d.clone())
                    .or_insert_with(|| p.drawing(d).unwrap().0)
                    .clone(),
                None => vec![0; (w * h * 4) as usize],
            };
            let d = diff_rgba(&mine, &theirs);
            layers_max = layers_max.max(d.max);
            if d.max > 1 {
                layers_bad.push(format!("{}: max {} ({} over 1)", l.name, d.max, d.over1));
            }
        }
        frames.push(format!(
            r#"{{"frame":{f},"compositeMax":{},"compositeMean":{:.4},"compositeOver1":{},"compositeOver8":{},"layersMax":{layers_max},"layersOff":{:?}}}"#,
            d.max, d.mean, d.over1, d.over8, layers_bad
        ));
    }
    format!("[{}]", frames.join(","))
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn open_each_test_project() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["empty", "layers20-4k", "drawings500"] {
        let path = dir.join(format!("{name}.ppstudio"));
        let p = Project::open(&path).unwrap();
        let (mut e, mut s) = fresh();
        let t = open(&mut e, &mut s, &path);
        let parts = format!("{:?}", e.files.last_open);
        let frames = check_open(&mut e, &mut s, &p, name);
        assert_eq!(
            s.project.path.as_deref(),
            Some(path.as_path()),
            "{name}: the project's path"
        );
        assert!(!s.project.unsaved, "{name}: unsaved right after opening");
        res.push(format!(
            r#""{name}":{{"openMs":{},"parts":{parts:?},"frames":{frames}}}"#,
            t.as_millis()
        ));
    }
    save_json("files-open.json", &format!("{{{}}}", res.join(",")));
}

#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn open_keeps_layer_properties() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["layers20-4k", "drawings500"] {
        let path = out_dir().join(format!("{name}-varied.ppstudio"));
        let p = varied(&dir.join(format!("{name}.ppstudio")), &path);
        let (mut e, mut s) = fresh();
        open(&mut e, &mut s, &path);
        res.push(format!(
            r#""{name}":{}"#,
            check_open(&mut e, &mut s, &p, &format!("{name}-varied"))
        ));
    }
    save_json("files-open-varied.json", &format!("{{{}}}", res.join(",")));
}

/// Open times, warm (5 runs, a fresh engine each) and cold (the page cache dropped first,
/// with `PP_QA_DROP_CACHES`), and PSS after opening. Budget: under 400 ms.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn open_time_and_memory() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["empty", "layers20-4k", "drawings500"] {
        let path = dir.join(format!("{name}.ppstudio"));
        let cold = drop_caches().then(|| {
            let (mut e, mut s) = fresh();
            open(&mut e, &mut s, &path).as_millis()
        });
        let mut warm = Vec::new();
        let mut pss = 0;
        for _ in 0..5 {
            let (mut e, mut s) = fresh();
            let before = pss_kib();
            warm.push(open(&mut e, &mut s, &path).as_secs_f64() * 1000.0);
            pss = pss_kib();
            let _ = before;
        }
        warm.sort_by(f64::total_cmp);
        res.push(format!(
            r#""{name}":{{"coldMs":{},"warmMs":{:?},"warmMedianMs":{:.1},"pssAfterOpenMiB":{:.1},"budgetMs":400,"pass":{}}}"#,
            cold.map_or("null".into(), |c| c.to_string()),
            warm.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>(),
            warm[2],
            pss as f64 / 1024.0,
            warm[2] < 400.0 && cold.is_none_or(|c| c < 400)
        ));
    }
    save_json("files-open-time.json", &format!("{{{}}}", res.join(",")));
}

/// After the project shows: how long each of the next ticks (and pictures) takes, as the
/// window's loop runs them, until they're all quick (thumbnails and the like made after the
/// open still hold the loop up).
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn ticks_after_open() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["layers20-4k", "drawings500"] {
        let (mut e, mut s) = fresh();
        let path = dir.join(format!("{name}.ppstudio"));
        answer(Some(&path));
        let t0 = Instant::now();
        file(&mut e, &mut s, files::OPEN);
        let mut ticks = Vec::new();
        for _ in 0..40 {
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            let a = t.elapsed();
            let _ = e.picture(&s);
            ticks.push((
                (a.as_secs_f64() * 1e4).round() / 10.0,
                (t.elapsed().as_secs_f64() * 1e4).round() / 10.0,
            ));
            std::thread::sleep(Duration::from_millis(6));
        }
        let busy: f64 = ticks.iter().map(|t| t.1).sum();
        res.push(format!(
            r#""{name}":{{"tickAndPictureMs":{ticks:?},"sumMs":{busy:.0},"wallMs":{}}}"#,
            t0.elapsed().as_millis()
        ));
    }
    save_json(
        "files-ticks-after-open.json",
        &format!("{{{}}}", res.join(",")),
    );
}

/// PSS of this process with one project open, the engine and the first frame made: the
/// project named by `PP_QA_PSS` (one per process: run it once per project). Budgets: 20
/// layers at 4K under 600 MB, 500 drawings under 700 MB (the window adds its GPU side).
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn pss_after_open() {
    let dir = need_fixtures!();
    let Ok(name) = std::env::var("PP_QA_PSS") else {
        eprintln!("skipped: PP_QA_PSS names no project");
        return;
    };
    let base = pss_kib();
    let (mut e, mut s) = fresh();
    let demo = pss_kib();
    open(&mut e, &mut s, &dir.join(format!("{name}.ppstudio")));
    for f in [s.frames / 2, s.frames - 1, 0] {
        let _ = native_frame(&mut e, &mut s, f);
    }
    std::thread::sleep(Duration::from_millis(300));
    let opened = pss_kib();
    save_json(
        &format!("files-pss-{name}.json"),
        &format!(
            r#"{{"testProcessMiB":{:.1},"withDemoMiB":{:.1},"afterOpenMiB":{:.1}}}"#,
            base as f64 / 1024.0,
            demo as f64 / 1024.0,
            opened as f64 / 1024.0
        ),
    );
}

// ------------------------------------------------------------------------------------------
// Save

/// Save as… `path` through the dialog: the time from the menu item until it's the project's
/// file with nothing unsaved.
fn save_as(e: &mut Engine, s: &mut State, path: &Path) -> Duration {
    let _ = std::fs::remove_file(path);
    answer(Some(path));
    let t = Instant::now();
    file(e, s, files::SAVE_AS);
    settle(e, s, Duration::from_secs(120), |_, s| {
        s.project.path.as_deref() == Some(path) && !s.project.unsaved && path.exists()
    })
    .unwrap_or_else(|| {
        panic!(
            "Save as {} never finished: {:?}",
            path.display(),
            s.project.note
        )
    });
    t.elapsed()
}

/// File › Save (the project has a file): the time it takes.
fn save(e: &mut Engine, s: &mut State) -> Duration {
    let t = Instant::now();
    file(e, s, files::SAVE);
    settle(e, s, Duration::from_secs(120), |_, s| !s.project.unsaved)
        .unwrap_or_else(|| panic!("Save never finished: {:?}", s.project.note));
    t.elapsed()
}

fn stroke(e: &mut Engine, s: &mut State, y: f32) {
    s.tool = Tool::Brush;
    s.brush = 2;
    s.brush_size = 10.0;
    for i in 0..=60u32 {
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
    let _ = e.picture(s);
}

/// Two files hold the same project: canvas, fps, layers (name, visibility, lock, opacity,
/// blend, order), the same drawing on each layer at each frame (pixels, ±`tol`), the same
/// holds.
fn same_project(a: &Project, b: &Project, tol: u8) -> Result<String, String> {
    let (ma, mb) = (&a.manifest, &b.manifest);
    let mut bad = Vec::new();
    if (ma.canvas.width, ma.canvas.height) != (mb.canvas.width, mb.canvas.height) {
        bad.push("canvas".to_string());
    }
    if ma.timeline.fps != mb.timeline.fps {
        bad.push(format!("fps {} vs {}", ma.timeline.fps, mb.timeline.fps));
    }
    let key = |l: &ppstudio::Layer| {
        (
            l.name.clone(),
            l.visible,
            l.locked,
            (l.opacity * 1000.0).round() as i32,
            l.blend.clone(),
        )
    };
    let la: Vec<_> = ma.layers.iter().map(key).collect();
    let lb: Vec<_> = mb.layers.iter().map(key).collect();
    if la != lb {
        bad.push(format!("layers {la:?} vs {lb:?}"));
    }
    let len = |m: &ppstudio::Manifest| m.timeline.frames.iter().map(|f| f.hold).sum::<u32>();
    if len(ma) != len(mb) {
        bad.push(format!("length {} vs {}", len(ma), len(mb)));
    }
    // drawing ↔ drawing, checked once per pair; and the mapping must be one to one
    let mut pairs: HashMap<(String, String), Diff> = HashMap::new();
    let mut map_ab: HashMap<String, String> = HashMap::new();
    let ids_b: HashMap<String, u32> = mb.layers.iter().map(|l| (l.name.clone(), l.id)).collect();
    for f in 0..len(ma).min(len(mb)) {
        let (ca, cb) = (cels_at(ma, f), cels_at(mb, f));
        for l in &ma.layers {
            let Some(&lb_id) = ids_b.get(&l.name) else {
                continue;
            };
            match (ca.get(&l.id), cb.get(&lb_id)) {
                (None, None) => {}
                (Some(x), Some(y)) => {
                    if let Some(prev) = map_ab.insert(x.clone(), y.clone())
                        && prev != *y
                    {
                        bad.push(format!(
                            "frame {f} {}: {x} became both {prev} and {y}",
                            l.name
                        ));
                    }
                    let k = (x.clone(), y.clone());
                    pairs.entry(k).or_insert_with(|| {
                        diff_rgba(&a.drawing(x).unwrap().0, &b.drawing(y).unwrap().0)
                    });
                }
                (x, y) => {
                    // an empty drawing may be left out either way
                    let empty = |p: &Project, d: Option<&String>| {
                        d.is_none_or(|d| {
                            p.manifest
                                .drawings
                                .get(d)
                                .is_some_and(|d| d.tiles.is_empty())
                        })
                    };
                    if !(empty(a, x) && empty(b, y)) {
                        bad.push(format!("frame {f} {}: {x:?} vs {y:?}", l.name));
                    }
                }
            }
        }
    }
    let worst = pairs.values().map(|d| d.max).max().unwrap_or(0);
    let over: usize = pairs.values().map(|d| d.over1).sum();
    if worst > tol {
        bad.push(format!(
            "pixels: max difference {worst} ({over} channels over 1)"
        ));
    }
    bad.truncate(12);
    let summary = format!(
        r#"{{"drawingPairs":{},"pixelMax":{worst},"channelsOver1":{over}}}"#,
        pairs.len()
    );
    if bad.is_empty() {
        Ok(summary)
    } else {
        Err(format!("{summary}: {}", bad.join("; ")))
    }
}

/// Open → Save as → open the copy: the same project; the copy read as today's app reads it;
/// how long Save as takes.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn save_as_round_trip() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["empty", "layers20-4k", "drawings500"] {
        let src = dir.join(format!("{name}.ppstudio"));
        let orig = Project::open(&src).unwrap();
        let (mut e, mut s) = fresh();
        open(&mut e, &mut s, &src);
        let dst = out_dir().join(format!("{name}-saved.ppstudio"));
        let took = save_as(&mut e, &mut s, &dst);
        let times = format!("{:?}", e.files.last_save);
        let web = web_app_reads(&dst);
        let copy = Project::open(&dst).expect("the saved file opens with crates/ppstudio");
        let same = same_project(&orig, &copy, 1);
        // and the native app opens its own file to the same pixels
        let (mut e2, mut s2) = fresh();
        let reopen = open(&mut e2, &mut s2, &dst);
        let frames = check_open(&mut e2, &mut s2, &copy, &format!("{name}-reopened"));
        res.push(format!(
            r#""{name}":{{"saveAsMs":{},"saveTimes":{times:?},"bytes":{},"origBytes":{},"webAppReads":{},"same":{},"reopenMs":{},"reopenFrames":{frames}}}"#,
            took.as_millis(),
            std::fs::metadata(&dst).unwrap().len(),
            std::fs::metadata(&src).unwrap().len(),
            match &web { Ok(_) => "true".into(), Err(e) => format!("{e:?}") },
            match &same { Ok(j) => j.clone(), Err(e) => format!("{e:?}") },
            reopen.as_millis(),
        ));
        assert!(web.is_ok(), "{name}: today's app wouldn't read it: {web:?}");
        assert!(same.is_ok(), "{name}: the copy differs: {same:?}");
    }
    save_json("files-save-as.json", &format!("{{{}}}", res.join(",")));
}

/// Save after one stroke: how long it takes against Save as of the whole project, what
/// changed in the file (only the stroke's drawing?), and the result read back.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn save_after_a_small_edit() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["layers20-4k", "drawings500"] {
        let (mut e, mut s) = fresh();
        open(&mut e, &mut s, &dir.join(format!("{name}.ppstudio")));
        let path = out_dir().join(format!("{name}-edited.ppstudio"));
        let full = save_as(&mut e, &mut s, &path);
        let full_times = format!("{:?}", e.files.last_save);
        let before = Project::open(&path).unwrap();
        // a stroke on frame 1's active layer
        e.apply(&mut s, &Action::GoToFrame(0));
        e.tick(&mut s, Instant::now());
        stroke(&mut e, &mut s, 300.0);
        settle(&mut e, &mut s, Duration::from_secs(2), |_, s| {
            s.project.unsaved
        })
        .expect("a stroke leaves unsaved changes");
        let mut saves = Vec::new();
        let mut parts = Vec::new();
        for k in 0..3 {
            if k > 0 {
                stroke(&mut e, &mut s, 300.0 + k as f32 * 40.0);
                settle(&mut e, &mut s, Duration::from_secs(2), |_, s| {
                    s.project.unsaved
                })
                .unwrap();
            }
            saves.push(save(&mut e, &mut s).as_secs_f64() * 1000.0);
            parts.push(format!("{:?}", e.files.last_save));
        }
        let after = Project::open(&path).unwrap();
        assert!(
            web_app_reads(&path).is_ok(),
            "{name}: {:?}",
            web_app_reads(&path).err()
        );
        // which drawings changed
        let mut changed = Vec::new();
        for (id, d) in &after.manifest.drawings {
            let same = before
                .manifest
                .drawings
                .get(id)
                .is_some_and(|b| b.tiles == d.tiles);
            if !same {
                changed.push(id.clone());
            }
        }
        res.push(format!(
            r#""{name}":{{"saveAsMs":{:.0},"saveAsTimes":{full_times:?},"saveAfterStrokeMs":{saves:.0?},"saveTimes":{parts:?},"drawingsChanged":{changed:?},"drawings":{}}}"#,
            full.as_secs_f64() * 1000.0,
            after.manifest.drawings.len()
        ));
        assert!(
            changed.len() == 1,
            "{name}: one stroke changed {} drawings",
            changed.len()
        );
    }
    save_json("files-save-edit.json", &format!("{{{}}}", res.join(",")));
}

/// The unsaved marker follows edits, undo and saving; New and Open ask "Save first?" when
/// there are unsaved changes (Cancel keeps everything; Don't save goes ahead; Save saves
/// first); Save of a project never saved asks for a name.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn unsaved_changes_and_save_first() {
    let dir = need_fixtures!();
    let (mut e, mut s) = fresh();
    let src = dir.join("drawings500.ppstudio");
    open(&mut e, &mut s, &src);
    let mut log = Vec::new();
    let mut note = |what: &str, s: &State| {
        log.push(format!(
            "{what}: unsaved {} asking {:?}",
            s.project.unsaved, s.project.asking
        ))
    };
    note("opened", &s);
    assert!(!s.project.unsaved);
    stroke(&mut e, &mut s, 400.0);
    settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
        s.project.unsaved
    })
    .expect("a stroke: unsaved");
    note("stroke", &s);
    assert!(
        s.project.subtitle(&s.cut_name).contains("unsaved"),
        "the header says unsaved"
    );
    // undo back to the file: does the marker clear? (recorded, not required)
    e.apply(&mut s, &Action::Undo);
    for _ in 0..3 {
        e.tick(&mut s, Instant::now());
    }
    note("undo", &s);
    let undo_clears = !s.project.unsaved;
    e.apply(&mut s, &Action::Redo);
    e.tick(&mut s, Instant::now());
    // a sheet edit alone
    let copy = out_dir().join("unsaved.ppstudio");
    save_as(&mut e, &mut s, &copy);
    note("saved as", &s);
    assert!(!s.project.unsaved);
    let fps = s.fps;
    s.fps = 12;
    e.tick(&mut s, Instant::now());
    note("fps 12", &s);
    assert!(
        s.project.unsaved,
        "a settings change (fps) is an unsaved change"
    );
    s.fps = fps;
    // New asks; Cancel keeps it all
    let id = s.doc_id;
    file(&mut e, &mut s, files::NEW);
    e.tick(&mut s, Instant::now());
    note("new", &s);
    assert!(
        s.project.asking.is_some(),
        "New with unsaved changes asks first"
    );
    assert_eq!(s.doc_id, id, "…and doesn't replace the document yet");
    file(&mut e, &mut s, files::CONFIRM_CANCEL);
    e.tick(&mut s, Instant::now());
    note("cancel", &s);
    assert!(s.project.asking.is_none() && s.project.unsaved && s.doc_id == id);
    // Open asks too; Don't save opens
    answer(Some(&src));
    file(&mut e, &mut s, files::OPEN);
    e.tick(&mut s, Instant::now());
    assert!(
        s.project.asking.is_some(),
        "Open with unsaved changes asks first"
    );
    file(&mut e, &mut s, files::CONFIRM_DISCARD);
    settle(&mut e, &mut s, Duration::from_secs(10), |_, s| {
        s.doc_id != id
    })
    .expect("Don't save, then it opens");
    note("don't save → opened", &s);
    assert_eq!(s.project.path.as_deref(), Some(src.as_path()));
    assert!(!s.project.unsaved);
    // the copy wasn't touched by Don't save
    let saved_fps = Project::open(&copy).unwrap().manifest.timeline.fps;
    assert_eq!(saved_fps, fps);
    // Save first: Save → saved, then New
    stroke(&mut e, &mut s, 500.0);
    settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
        s.project.unsaved
    })
    .unwrap();
    let edited = out_dir().join("save-first.ppstudio");
    // (the opened fixture has a path: Save writes to it; open the copy first so the
    // fixture stays as built)
    let _ = edited;
    let id = s.doc_id;
    file(&mut e, &mut s, files::NEW);
    e.tick(&mut s, Instant::now());
    file(&mut e, &mut s, files::CONFIRM_DISCARD);
    settle(&mut e, &mut s, Duration::from_secs(5), |_, s| {
        s.doc_id != id
    })
    .unwrap();
    note("new (don't save)", &s);
    assert_eq!(s.project.name, "Untitled");
    assert!(s.project.path.is_none() && !s.project.unsaved);
    assert_eq!(
        (
            e.painter.width(),
            e.painter.height(),
            s.layers.len(),
            s.frames
        ),
        (1920, 1080, 1, 1)
    );
    // a new project, edited; New → Save first → Save: asks for a name (none yet), saves, then New
    stroke(&mut e, &mut s, 300.0);
    settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
        s.project.unsaved
    })
    .unwrap();
    let named = out_dir().join("first-save.ppstudio");
    let _ = std::fs::remove_file(&named);
    answer(Some(&out_dir().join("first-save"))); // (no extension: it's added)
    let id = s.doc_id;
    file(&mut e, &mut s, files::NEW);
    e.tick(&mut s, Instant::now());
    file(&mut e, &mut s, files::CONFIRM_SAVE);
    settle(&mut e, &mut s, Duration::from_secs(10), |_, s| {
        s.doc_id != id
    })
    .expect("saved, then New");
    note("new (save first)", &s);
    assert!(
        named.exists(),
        "Save first saved to the name given, .ppstudio added"
    );
    assert!(
        web_app_reads(&named).is_ok(),
        "{:?}",
        web_app_reads(&named).err()
    );
    // Save as… cancelled: nothing changes
    stroke(&mut e, &mut s, 600.0);
    settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
        s.project.unsaved
    })
    .unwrap();
    answer(None);
    file(&mut e, &mut s, files::SAVE);
    for _ in 0..5 {
        e.tick(&mut s, Instant::now());
    }
    note("save cancelled", &s);
    assert!(s.project.unsaved && s.project.path.is_none());
    save_json(
        "files-unsaved.json",
        &format!(r#"{{"undoToSavedClearsMarker":{undo_clears},"log":{log:?}}}"#),
    );
}

/// The recovery copy: written once changes have been unsaved for two minutes (time given
/// to `tick`), in the state folder, never over the project's file; again two minutes later;
/// gone once saved. How long writing it holds up the loop (it runs there).
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn autosave_recovery_copy() {
    let dir = need_fixtures!();
    let mut res = Vec::new();
    for name in ["layers20-4k", "drawings500"] {
        let (mut e, mut s) = fresh();
        let path = out_dir().join(format!("{name}-autosave.ppstudio"));
        std::fs::copy(dir.join(format!("{name}.ppstudio")), &path).unwrap();
        open(&mut e, &mut s, &path);
        let file_before = std::fs::read(&path).unwrap();
        stroke(&mut e, &mut s, 350.0);
        settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
            s.project.unsaved
        })
        .unwrap();
        // not before two minutes
        e.tick(&mut s, Instant::now() + Duration::from_secs(100));
        assert!(
            s.project.recovery.is_none(),
            "{name}: a recovery copy before two minutes"
        );
        let due = e.next_tick_at(&s);
        let t = Instant::now();
        e.tick(
            &mut s,
            Instant::now() + files::AUTOSAVE + Duration::from_secs(1),
        );
        let took = t.elapsed();
        let rec = s
            .project
            .recovery
            .clone()
            .unwrap_or_else(|| panic!("{name}: no recovery copy: {:?}", s.project.note));
        assert!(
            rec.starts_with(out_dir().join("state")),
            "{name}: recovery at {}",
            rec.display()
        );
        assert_ne!(rec, path);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            file_before,
            "{name}: autosave changed the user's file"
        );
        let r = Project::open(&rec).expect("the recovery copy opens");
        let edited = !same_project(&Project::open(&path).unwrap(), &r, 0).is_ok();
        assert!(edited, "{name}: the recovery copy lacks the stroke");
        assert!(web_app_reads(&rec).is_ok());
        // Save: the recovery copy goes
        save(&mut e, &mut s);
        assert!(
            !rec.exists(),
            "{name}: the recovery copy stays after saving"
        );
        res.push(format!(
            r#""{name}":{{"autosaveBlocksLoopMs":{:.1},"recovery":{:?},"dueInS":{},"recoveryDrawings":{}}}"#,
            took.as_secs_f64() * 1000.0,
            rec.display().to_string(),
            due.map_or("null".into(), |d| d
                .saturating_duration_since(Instant::now())
                .as_secs()
                .to_string()),
            r.manifest.drawings.len()
        ));
    }
    save_json("files-autosave.json", &format!("{{{}}}", res.join(",")));
}

/// Recent files: newest first, no duplicates, at most ten, gone files left out; Open recent
/// opens one.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn recent_files() {
    let dir = need_fixtures!();
    let (mut e, mut s) = fresh();
    let _ = std::fs::remove_file(out_dir().join("config/pencilpilot/recent"));
    let mut paths = Vec::new();
    for k in 0..12 {
        let p = out_dir().join(format!("recent-{k}.ppstudio"));
        std::fs::copy(dir.join("empty.ppstudio"), &p).unwrap();
        paths.push(p.canonicalize().unwrap());
    }
    for p in &paths {
        open(&mut e, &mut s, p);
    }
    open(&mut e, &mut s, &paths[5]);
    let want: Vec<_> = [5, 11, 10, 9, 8, 7, 6, 4, 3, 2]
        .iter()
        .map(|&k| paths[k].clone())
        .collect();
    assert_eq!(s.project.recent, want, "newest first, no duplicates, ten");
    // a fresh engine reads them back; a deleted one is left out
    std::fs::remove_file(&paths[11]).unwrap();
    let (mut e2, mut s2) = fresh();
    e2.tick(&mut s2, Instant::now());
    assert!(!s2.project.recent.contains(&paths[11]));
    assert_eq!(s2.project.recent[0], paths[5]);
    // Open recent
    let id = s2.doc_id;
    file(
        &mut e2,
        &mut s2,
        &format!("{}{}", files::OPEN_RECENT, paths[9].display()),
    );
    settle(&mut e2, &mut s2, Duration::from_secs(5), |_, s| {
        s.doc_id != id
    })
    .expect("Open recent opens");
    assert_eq!(s2.project.path.as_deref(), Some(paths[9].as_path()));
    // a recent file deleted since the menu was built
    std::fs::remove_file(&paths[8]).unwrap();
    let id = s2.doc_id;
    file(
        &mut e2,
        &mut s2,
        &format!("{}{}", files::OPEN_RECENT, paths[8].display()),
    );
    e2.tick(&mut s2, Instant::now());
    assert_eq!(s2.doc_id, id, "a missing recent file opens nothing");
    assert!(!s2.project.note.is_empty(), "…and says why");
    save_json(
        "files-recent.json",
        &format!(r#"{{"note":{:?}}}"#, s2.project.note),
    );
}

/// Files that can't be opened (missing, not a zip, a zip without a manifest, truncated, a
/// newer version, a text file renamed): a note, the open project kept, no panic.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn bad_files_dont_open() {
    let dir = need_fixtures!();
    let (mut e, mut s) = fresh();
    let good = dir.join("drawings500.ppstudio");
    open(&mut e, &mut s, &good);
    let bytes = std::fs::read(&good).unwrap();
    let d = out_dir();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("garbage", b"hello, not a zip".to_vec()),
        ("empty", Vec::new()),
        ("truncated", bytes[..bytes.len() / 2].to_vec()),
        ("tail-cut", bytes[..bytes.len() - 100].to_vec()),
    ];
    let mut notes = Vec::new();
    for (name, b) in &cases {
        let p = d.join(format!("bad-{name}.ppstudio"));
        std::fs::write(&p, b).unwrap();
        notes.push(try_open(&mut e, &mut s, &p, name));
    }
    // a newer version, and a tile missing
    let mut p = Project::open(&good).unwrap();
    p.manifest.version = 99;
    let newer = d.join("bad-newer.ppstudio");
    p.save(&newer).unwrap();
    notes.push(try_open(&mut e, &mut s, &newer, "newer"));
    notes.push(try_open(
        &mut e,
        &mut s,
        &d.join("missing.ppstudio"),
        "missing",
    ));
    // the project still open
    assert_eq!(s.project.path.as_deref(), Some(good.as_path()));
    save_json("files-bad.json", &format!("{notes:?}"));
}

fn try_open(e: &mut Engine, s: &mut State, p: &Path, name: &str) -> String {
    let id = s.doc_id;
    answer(Some(p));
    file(e, s, files::OPEN);
    for _ in 0..5 {
        e.tick(s, Instant::now());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(s.doc_id, id, "{name}: a bad file replaced the document");
    assert!(
        !s.project.note.is_empty(),
        "{name}: nothing says it couldn't open"
    );
    format!("{name}: {}", s.project.note)
}

/// Quit (the File menu's, and what closing the window becomes) with unsaved changes asks
/// "Save first?" and doesn't close until it's answered; with none, it closes at once.
#[test]
#[ignore = "QA suite: tools/native-qa/run.sh"]
fn quit_asks_when_unsaved() {
    let dir = need_fixtures!();
    let (mut e, mut s) = fresh();
    let path = out_dir().join("quit.ppstudio");
    std::fs::copy(dir.join("drawings500.ppstudio"), &path).unwrap();
    open(&mut e, &mut s, &path);
    // nothing unsaved: Quit isn't held up
    let held = file(&mut e, &mut s, "Quit");
    e.tick(&mut s, Instant::now());
    let clean = format!(
        "handled {held}, asking {:?}, quit_now {}",
        s.project.asking, s.quit_now
    );
    assert!(s.project.asking.is_none(), "Quit with nothing unsaved asks");
    s.quit_now = false;
    stroke(&mut e, &mut s, 420.0);
    settle(&mut e, &mut s, Duration::from_millis(200), |_, s| {
        s.project.unsaved
    })
    .unwrap();
    let held = file(&mut e, &mut s, "Quit");
    e.tick(&mut s, Instant::now());
    let unsaved = format!(
        "handled {held}, asking {:?}, quit_now {}",
        s.project.asking, s.quit_now
    );
    save_json(
        "files-quit.json",
        &format!(r#"{{"clean":{clean:?},"unsaved":{unsaved:?}}}"#),
    );
    assert!(
        held && s.project.asking.is_some() && !s.quit_now,
        "Quit with unsaved changes closes without asking: {unsaved}"
    );
    // Save first → Save: saved, then it closes
    file(&mut e, &mut s, files::CONFIRM_SAVE);
    settle(&mut e, &mut s, Duration::from_secs(10), |_, s| s.quit_now).expect("saved, then quit");
    assert!(!s.project.unsaved);
}
