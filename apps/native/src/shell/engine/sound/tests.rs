//! The scene's sound through the engine: a project's clips shown and decoded, muted, moved
//! and saved; playback heard in step with the frames (on the silent output, which records
//! what it played); scrubbing's snippets; video exports with the scene's sound.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pencilpilot_sound::decode::wav_i16;
use ppstudio::{AudioClip, Cut, Exposure, Layer, Project, Scene};

use super::*;
use crate::shell::engine::export::{Format, Status};
use crate::shell::engine::files;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join(format!("pp-sound-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn layer(id: u32, name: &str) -> Layer {
    Layer {
        id,
        name: name.into(),
        visible: true,
        opacity: 1.0,
        blend: "normal".into(),
        locked: false,
        alpha_lock: false,
        clip: false,
        imported: false,
        group: 0,
        adjust: None,
        style: None,
        text: None,
        lines: false,
    }
}

fn ex(d: &str, length: u32) -> Exposure {
    Exposure {
        drawing: Some(d.into()),
        length,
    }
}

/// Music: three seconds of a stereo signal no two stretches of which look alike (for the
/// correlation check), at 48 kHz; `k`: its sample.
fn music_at(k: usize) -> (i16, i16) {
    let t = k as f64 / 48_000.0;
    let l = (t * 220.0 * std::f64::consts::TAU * (1.0 + t)).sin() * 0.4
        + (t * 3.0 * std::f64::consts::TAU).sin() * 0.2;
    let r = (t * 330.0 * std::f64::consts::TAU).sin() * 0.3;
    ((l * 32767.0) as i16, (r * 32767.0) as i16)
}

/// A project: one scene of two cuts (10 frames, then 24), the second open; its sound: music
/// from 5 frames before the scene (gain 0.8), a line at 44.1 kHz on scene frame 14, a muted
/// clip. 24 fps, 192 × 108.
fn sound_project(path: &Path) {
    sound_project_of(path, 12, 3, (192, 108));
}

/// The same with the open cut's two drawings held `hold` frames each, `music` seconds of
/// music, at a size.
fn sound_project_of(path: &Path, hold: u32, music_secs: usize, (w, h): (u32, u32)) {
    let mut p = Project::new("PencilPilot 0.1.0", "2026-11-28T10:00:00Z", w, h, 24);
    p.manifest.layers = vec![layer(1, "Ink")];
    for (i, d) in ["D1", "D2", "D3"].iter().enumerate() {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 10..40 {
            for x in (10 + i as u32 * 40)..(40 + i as u32 * 40) {
                let o = ((y * w + x) * 4) as usize;
                px[o..o + 4].copy_from_slice(&[200, 40, 40, 255]);
            }
        }
        p.put_drawing(d, &px, None).unwrap();
    }
    let music: Vec<i16> = (0..48_000 * music_secs)
        .flat_map(|k| {
            let (l, r) = music_at(k);
            [l, r]
        })
        .collect();
    let music = p.put_audio(&wav_i16(&music, 2, 48_000), "wav").unwrap();
    let line: Vec<i16> = (0..44_100)
        .map(|k| ((k as f64 * 0.05).sin() * 8000.0) as i16)
        .collect();
    let line = p.put_audio(&wav_i16(&line, 1, 44_100), "wav").unwrap();
    let clip = |name: &str, file: &str, start: i64, gain: f32, muted: bool| AudioClip {
        name: name.into(),
        file: file.into(),
        start,
        gain,
        muted,
    };
    p.manifest.timeline.scenes = vec![Scene {
        name: "Chase".into(),
        cuts: vec![
            Cut {
                name: "Cut 1".into(),
                tracks: [(1, vec![ex("D1", 10)])].into(),
                ..Default::default()
            },
            Cut {
                name: "Cut 2".into(),
                tracks: [(1, vec![ex("D2", hold), ex("D3", hold)])].into(),
                ..Default::default()
            },
        ],
        audio: vec![
            clip("music", &music, -5, 0.8, false),
            clip("line", &line, 14, 1.0, false),
            clip("muted", &line, 0, 1.0, true),
        ],
        ..Default::default()
    }];
    p.manifest.timeline.frames = ppstudio::Timeline::flatten(&p.manifest.timeline.scenes, &[]);
    p.manifest.extra = serde_json::json!({ "scene": 0, "cut": 1, "frame": 0 });
    p.save(path).unwrap();
}

/// An engine with the project open and its sounds decoded.
fn opened(path: &Path) -> (Engine, State) {
    let mut s = State::demo();
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    e.tick(&mut s, Instant::now());
    assert!(files::open_path(&mut e, &mut s, path), "{}", s.project.note);
    let t = Instant::now();
    while s
        .sound
        .clips
        .iter()
        .any(|c| c.frames == 0 && c.failed.is_none())
        && t.elapsed() < Duration::from_secs(20)
    {
        e.tick(&mut s, Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
    (e, s)
}

#[test]
fn a_projects_sound_shows_on_the_timeline() {
    let d = dir("shows");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (_e, s) = opened(&path);
    let c = &s.sound.clips;
    assert_eq!(c.len(), 3);
    assert!(s.sound.from_file);
    // the open cut starts on scene frame 10 (the cut before it is 10 frames)
    assert_eq!(s.sound.cut_start, 10);
    assert_eq!(
        (c[0].name.as_str(), c[0].start, c[0].gain),
        ("music", -5, 0.8)
    );
    assert!(c[2].muted);
    // decoded, with its waveform: three seconds at 24 fps; the line resampled to 48 kHz
    assert_eq!(c[0].frames, 72);
    assert_eq!(c[0].peaks.len(), 72);
    assert!(
        c[0].peaks.iter().all(|p| (0.3..=0.62).contains(p)),
        "{:?}",
        &c[0].peaks[..4]
    );
    assert_eq!(c[1].frames, 24);
    assert!(c.iter().all(|c| c.failed.is_none()));
    // nothing changed yet
    assert!(!s.project.unsaved);
}

#[test]
fn muting_and_moving_are_saved() {
    let d = dir("saved");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (mut e, mut s) = opened(&path);
    s.sound.clips[1].start += 3;
    s.sound.clips[0].muted = true;
    for _ in 0..3 {
        e.tick(&mut s, Instant::now());
    }
    assert!(s.project.unsaved, "a clip moved is an unsaved change");
    let out = d.join("b.ppstudio");
    assert!(files::save_to(&mut e, &mut s, &out), "{}", s.project.note);
    let back = Project::open(&out).unwrap();
    let a = &back.manifest.timeline.scenes[0].audio;
    assert_eq!(a[1].start, 17);
    assert!(a[0].muted);
    // the sound files themselves, copied over as they were
    let orig = Project::open(&path).unwrap();
    assert_eq!(back.audio, orig.audio);
    // and it opens with them
    let (_e2, s2) = opened(&out);
    assert_eq!(s2.sound.clips[1].start, 17);
    assert!(s2.sound.clips[0].muted);
    assert_eq!(s2.sound.clips[1].frames, 24);
}

/// The scene sample (fractional) the silent output played at `t`, from its buffers.
fn heard_at(recs: &[Rec], t: Instant) -> Option<f64> {
    let i = recs.iter().rposition(|r| r.heard_at <= t)?;
    let r = &recs[i];
    if r.pos.is_nan() {
        return None;
    }
    let dt = t.saturating_duration_since(r.heard_at).as_secs_f64();
    (dt * (r.rate as f64) < r.frames as f64).then_some(r.pos + dt * RATE as f64)
}

/// Playback through the engine's own loop (ticks at `now + lead`, as the window's): when each
/// frame shows against the sound the silent output plays then. Returns the offsets (ms;
/// positive: the sound is ahead), and how long after Play the sound was first heard.
fn play_and_measure(e: &mut Engine, s: &mut State, secs: f64, lead: Duration) -> (Vec<f64>, f64) {
    s.strips.play_mode = 1;
    let fps = s.fps as f64;
    e.apply(s, &Action::Play);
    let start = Instant::now();
    let mut shown: Vec<(Instant, u32)> = Vec::new();
    let mut last = u32::MAX;
    let mut clock = None;
    while start.elapsed().as_secs_f64() < secs {
        let now = Instant::now();
        e.tick(s, now + lead);
        clock = clock.or(e.frames.play_clock());
        if s.frame != last {
            // (it shows `lead` from now)
            shown.push((now + lead, s.frame));
            last = s.frame;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    e.apply(s, &Action::Stop);
    e.tick(s, Instant::now());
    std::thread::sleep(Duration::from_millis(60));
    e.sound.drain();
    let recs = e.sound.recs.clone();
    let mut offs = Vec::new();
    // (looping: positions are within the loop, so a buffer that runs over its end is
    // measured modulo the loop's length)
    let (a, b) = crate::shell::engine::frames::play_range(s);
    let span = (b - a + 1) as f64 * RATE as f64 / fps;
    // each frame's own time on playback's clock (the k-th after the first: t0 + k / fps;
    // when the loop saw it change is later, by however busy the machine is)
    let (t0, _, _) = clock.expect("it played");
    let len = b - a + 1;
    let mut k = 0u32;
    for w in shown.windows(2) {
        let (prev, f) = (w[0].1, &w[1].1);
        // (frames on since the first, a skipped one counted too)
        k += (f + len - prev) % len;
        let t = t0 + Duration::from_secs_f64(k as f64 / fps);
        if let Some(pos) = heard_at(&recs, t) {
            let want = (s.sound.cut_start as f64 + *f as f64) * RATE as f64 / fps;
            let d = (pos - want + span / 2.0).rem_euclid(span) - span / 2.0;
            offs.push(d / RATE as f64 * 1e3);
        }
    }
    let play = *e.sound.plays.last().unwrap();
    let first = recs
        .iter()
        .find(|r| r.heard_at >= play && r.peak > 0.0)
        .map_or(f64::INFINITY, |r| {
            r.heard_at.saturating_duration_since(play).as_secs_f64() * 1e3
        });
    (offs, first)
}

#[test]
fn playback_is_heard_in_step_with_the_frames() {
    let d = dir("sync");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (mut e, mut s) = opened(&path);
    // (a frame shows 8 ms after the tick that makes it: the window's lead at 165 Hz)
    let (offs, first) = play_and_measure(&mut e, &mut s, 3.0, Duration::from_millis(8));
    assert!(offs.len() > 50, "{} frames measured", offs.len());
    let worst = offs.iter().fold(0f64, |m, v| m.max(v.abs()));
    // the sound at each frame's time is that frame's, to well under a millisecond (the
    // windowed check measures against the frames as they reach the screen)
    assert!(worst < 0.5, "worst {worst:.2} ms; {offs:?}");
    assert!(first < 50.0, "the sound started {first:.1} ms after Play");
}

#[test]
fn scrubbing_plays_a_snippet() {
    let d = dir("scrub");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (mut e, mut s) = opened(&path);
    e.sound.drain();
    e.sound.recs.clear();
    e.apply(&mut s, &Action::GoToFrame(6));
    e.tick(&mut s, Instant::now());
    assert_eq!(e.sound.sent, Sent::Snippet);
    std::thread::sleep(Duration::from_millis(250));
    e.sound.drain();
    let heard: Vec<&Rec> = e.sound.recs.iter().filter(|r| r.peak > 0.0).collect();
    // 1/12 s (two frames at 24 fps are less), from scene frame 16
    assert!(!heard.is_empty());
    assert!(
        (heard[0].pos - 16.0 * 2000.0).abs() < 1.0,
        "{}",
        heard[0].pos
    );
    let samples: u32 = heard.iter().map(|r| r.frames).sum();
    assert!((4000..=4480).contains(&samples), "{samples}");
    // the output is paused once it's been quiet a while
    assert!(next_tick_at(&e, &s).is_some());
}

/// A video export of the cut: the sound of the same frames (the music from where the cut is
/// in it, the line on its frame, the muted clip not), checked with FFmpeg: the stream, its
/// length, and its samples against the mix.
#[test]
fn videos_get_the_scenes_sound() {
    let Some(ff) = pencilpilot_export::find_ffmpeg() else {
        return;
    };
    let formats = pencilpilot_export::video_formats(&ff);
    let d = dir("export");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (mut e, mut s) = opened(&path);
    for (f, codec) in [(Format::Mp4, "aac"), (Format::Webm, "opus")] {
        if !formats.contains(&f) {
            continue;
        }
        let out = d.join(format!("cut.{}", f.extension()));
        s.export.format = f;
        crate::shell::engine::export::start(&mut e, &mut s, out.clone());
        let t = Instant::now();
        while !matches!(s.export.status, Status::Done(_) | Status::Failed(_))
            && t.elapsed() < Duration::from_secs(60)
        {
            e.tick(&mut s, Instant::now());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(s.export.status, Status::Done(out.clone()));
        let r = check_sound(&ff, &out, &e, &s, codec);
        eprintln!(
            "{f:?}: {codec}, {:.3} s, best lag {:.2} ms, correlation {:.4}",
            r.0, r.1, r.2
        );
        assert!((r.0 - 1.0).abs() < 0.03, "{f:?}: {} s of sound", r.0);
        assert!(r.1.abs() <= 1.0, "{f:?}: off by {} ms", r.1);
        // (Opus, at 160 kb/s, keeps less of the waveform's detail than AAC at 192)
        assert!(r.2 > 0.95, "{f:?}: correlation {}", r.2);
        s.export.status = Status::Idle;
    }
}

/// The exported file's sound: its length (s), and against the mix the engine makes for the
/// cut: the lag that matches best (ms) and the correlation there.
fn check_sound(ff: &Path, out: &Path, e: &Engine, s: &State, codec: &str) -> (f64, f64, f64) {
    use std::process::Command;
    let probe = Command::new(ff.with_file_name("ffprobe"))
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .args(["stream=codec_name,sample_rate,channels:format=duration"])
        .args(["-of", "default=nw=1"])
        .arg(out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout).to_string();
    assert!(text.contains(&format!("codec_name={codec}")), "{text}");
    assert!(text.contains("channels=2"), "{text}");
    // the samples, decoded by FFmpeg
    let raw = Command::new(ff)
        .args(["-v", "error", "-i"])
        .arg(out)
        .args(["-vn", "-f", "f32le", "-ac", "2", "-ar", "48000", "-"])
        .output()
        .unwrap()
        .stdout;
    let got: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let secs = got.len() as f64 / 2.0 / 48_000.0;
    // the mix, as the export made it
    let x = for_export(e, s, 0, s.frames - 1).unwrap();
    let clips: Vec<mix::Clip> = x
        .clips
        .iter()
        .map(|(sl, st, g)| mix::Clip {
            pcm: sl.wait().unwrap(),
            start: *st,
            gain: *g,
            muted: false,
        })
        .collect();
    let want = mix::mixdown(
        &[mix::Part {
            clips: &clips,
            from: x.from,
            frames: x.frames,
        }],
        x.fps,
    )
    .unwrap();
    // the lag (±48 samples) with the best normalised correlation, both channels together
    // (lossy codecs code stereo jointly)
    let n = (want.len() / 2).min(got.len() / 2);
    let (mut best, mut at) = (-2.0, 0i64);
    for lag in -48i64..=48 {
        let (mut sxy, mut sxx, mut syy) = (0f64, 0f64, 0f64);
        for i in 2000..n.saturating_sub(2000) {
            let j = i as i64 + lag;
            if j < 0 || j as usize >= got.len() / 2 {
                continue;
            }
            let j = j as usize;
            let a = (want[i * 2] + want[i * 2 + 1]) as f64;
            let b = (got[j * 2] + got[j * 2 + 1]) as f64;
            sxy += a * b;
            sxx += a * a;
            syy += b * b;
        }
        let c = sxy / (sxx * syy).sqrt().max(1e-12);
        if c > best {
            (best, at) = (c, lag);
        }
    }
    (secs, at as f64 / 48.0, best)
}

/// The windowed check's project (`tools/native-qa/sound_window.py`): a cut of 1500 frames at
/// 24 fps (62.5 s) under 65 s of music, at 1920 × 1080. `PP_SOUND_FIXTURE` = where.
#[test]
#[ignore = "a fixture: PP_SOUND_FIXTURE=x.ppstudio cargo test -p pencilpilot-native -- --ignored sound_fixture"]
fn sound_fixture() {
    let p = PathBuf::from(std::env::var("PP_SOUND_FIXTURE").expect("PP_SOUND_FIXTURE"));
    // (`PP_SOUND_SIZE` = 3840x2160: another size)
    let size = std::env::var("PP_SOUND_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1920, 1080));
    sound_project_of(&p, 750, 65, size);
}

/// Peak memory of this process and its children (FFmpeg) while exporting the windowed
/// check's fixture to MP4 and WebM with its 62.5 s of sound, and the files' sound (stream,
/// length). `PP_SOUND_FIXTURE` = the fixture (see `sound_fixture`), `PP_SOUND_OUT` = where
/// the videos go.
#[test]
#[ignore = "memory: PP_SOUND_FIXTURE=… cargo test --release -p pencilpilot-native -- --ignored export_with_sound_memory --nocapture"]
fn export_with_sound_memory() {
    use std::process::Command;
    let fixture = PathBuf::from(std::env::var("PP_SOUND_FIXTURE").expect("PP_SOUND_FIXTURE"));
    let out = PathBuf::from(std::env::var("PP_SOUND_OUT").unwrap_or_else(|_| "/tmp".into()));
    let ff = pencilpilot_export::find_ffmpeg().expect("ffmpeg");
    let (mut e, mut s) = opened(&fixture);
    // (`PP_SOUND_MUTE`: the same without sound, to compare)
    let muted = std::env::var_os("PP_SOUND_MUTE").is_some();
    for c in &mut s.sound.clips {
        c.muted |= muted;
    }
    let rss = |pid: u32| -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("VmRSS:"))
                    .and_then(|v| v.split_whitespace().next()?.parse().ok())
            })
            .unwrap_or(0)
    };
    let children = |pid: u32| -> Vec<u32> {
        let mut v = Vec::new();
        if let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) {
            for t in tasks.flatten() {
                if let Ok(c) = std::fs::read_to_string(t.path().join("children")) {
                    v.extend(c.split_whitespace().filter_map(|x| x.parse::<u32>().ok()));
                }
            }
        }
        v
    };
    let me = std::process::id();
    for f in [Format::Mp4, Format::Webm] {
        let path = out.join(format!("sound-export.{}", f.extension()));
        s.export.format = f;
        s.export.size = 0;
        let t0 = Instant::now();
        crate::shell::engine::export::start(&mut e, &mut s, path.clone());
        let (mut peak, mut peak_ff) = (0u64, 0u64);
        while !matches!(s.export.status, Status::Done(_) | Status::Failed(_)) {
            e.tick(&mut s, Instant::now());
            let ff_kb: u64 = children(me).into_iter().map(rss).sum();
            peak = peak.max(rss(me) + ff_kb);
            peak_ff = peak_ff.max(ff_kb);
            std::thread::sleep(Duration::from_millis(10));
        }
        let took = t0.elapsed().as_secs_f64();
        assert_eq!(s.export.status, Status::Done(path.clone()));
        let probe = Command::new(ff.with_file_name("ffprobe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name,duration",
            ])
            .args(["-of", "csv=p=0"])
            .arg(&path)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&probe.stdout).replace('\n', "; ");
        eprintln!(
            "{f:?} {}x{}: {took:.1} s ({:.1} ms a frame), peak {:.0} MiB (FFmpeg {:.0} MiB), streams: {text}",
            s.view.doc_w,
            s.view.doc_h,
            took * 1e3 / s.frames as f64,
            peak as f64 / 1024.0,
            peak_ff as f64 / 1024.0
        );
        assert!(muted || text.contains("audio"), "{text}");
        // (4K WebM is over it without sound too: libvpx's own, an open item of step 5)
        if f == Format::Mp4 || s.view.doc_h <= 1080 {
            assert!(peak < 1024 * 1024, "{} MiB", peak / 1024);
        }
        s.export.status = Status::Idle;
    }
}

/// The timeline with the test project's sound (its rows, waveforms, the muted clip), drawn
/// off-screen: `PP_SHOTS` = where.
#[test]
#[ignore = "a picture: PP_SHOTS=dir cargo test -p pencilpilot-native -- --ignored sound_rows_picture"]
fn sound_rows_picture() {
    let shots = std::env::var("PP_SHOTS").expect("PP_SHOTS");
    let d = dir("picture");
    let path = d.join("a.ppstudio");
    sound_project(&path);
    let (_e, mut s) = opened(&path);
    s.workspace = crate::state::Workspace::Animate;
    // (`render_png` sets up an engine of its own, with no file open: the clips are kept as
    // the demo's are)
    (s.doc_id, s.sound.from_file) = (0, false);
    crate::shell::render_png(&mut s, 1440, 900, 1.0, &format!("{shots}/sound-rows.png")).unwrap();
}
