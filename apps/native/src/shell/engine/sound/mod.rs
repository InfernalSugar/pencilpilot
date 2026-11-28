//! The scene's sound (`crates/sound`): its clips on the timeline, each file decoded once off
//! the window's thread (and its waveform), played in step with playback, a snippet while
//! scrubbing, and the mix for a video export.
//!
//! - **The clips** are the open scene's (`Scene::audio` in the file): placed by scene frame,
//!   so the open cut hears them from where it is in the scene (the cuts before it, one after
//!   another). The timeline shows them, mutes them and moves them ([`State::sound`]); saving
//!   writes them back.
//! - **Playback** follows the picture's clock (`frames::Frames::play_clock`): when Play shows
//!   its first frame, at what rate, over which frames, in which mode. The mixer works out
//!   which sample is heard when, so sound and picture stay together (see
//!   `pencilpilot_sound::player`).
//! - **No sound out of the speakers in tests and checks:** the system's output is only opened
//!   for a normal start of the app ([`allow_sound`], from `main.rs`), and never with
//!   `PENCILPILOT_NO_SOUND` or `PENCILPILOT_NO_SYSTEM_WINDOWS` set. Otherwise a silent output
//!   (no device at all) plays, recording what it played for `--stats`.
//!   `PENCILPILOT_SOUND_DEVICE` picks a device (cpal's id, e.g. `alsa:null`); in a check only
//!   `alsa:null` (ALSA's own silent device) is taken.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use pencilpilot_sound::player::{Clock, Cmd, Mode, Output, Player, Rec, Voice};
use pencilpilot_sound::{Pcm, RATE, frame_sample, mix};

use super::Engine;
use crate::state::{Action, State};

/// Whether the system's sound output may be opened (see the module's notes).
static SOUND: AtomicBool = AtomicBool::new(false);

/// For a normal start of the app (`main.rs`): the system's output may be used.
pub fn allow_sound() {
    SOUND.store(true, Ordering::Relaxed);
}

fn env_set(k: &str) -> bool {
    std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Where the sound goes.
pub fn output() -> Output {
    let device = std::env::var("PENCILPILOT_SOUND_DEVICE").ok();
    let allowed = SOUND.load(Ordering::Relaxed)
        && !env_set("PENCILPILOT_NO_SOUND")
        && !env_set("PENCILPILOT_NO_SYSTEM_WINDOWS");
    if allowed {
        return Output::Device(device.filter(|d| !d.is_empty()));
    }
    if device.as_deref() == Some("alsa:null") {
        return Output::Device(device);
    }
    // the silent output: PENCILPILOT_NULL_SOUND = "period,latency ms,drift ppm"
    let mut o = Output::null();
    if let (
        Ok(v),
        Output::Null {
            period,
            latency,
            drift,
            ..
        },
    ) = (std::env::var("PENCILPILOT_NULL_SOUND"), &mut o)
    {
        let f: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if let Some(p) = f.first() {
            *period = (*p as u32).clamp(32, 16384);
        }
        if let Some(l) = f.get(1) {
            *latency = Duration::from_secs_f64(l.clamp(0.0, 1000.0) / 1e3);
        }
        if let Some(d) = f.get(2) {
            *drift = d.clamp(-5000.0, 5000.0) * 1e-6;
        }
    }
    o
}

/// A clip as the timeline shows it (in [`State::sound`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundClip {
    pub name: String,
    /// the sound file in the project (`audio/<file>`)
    pub file: String,
    /// the scene frame it starts on (negative: before the scene)
    pub start: i64,
    pub gain: f32,
    pub muted: bool,
    /// its length in frames at the cut's rate (0 until it's decoded)
    pub frames: u32,
    /// the loudest point of each of its frames (0–1; empty until it's decoded)
    pub peaks: Arc<Vec<f32>>,
    /// couldn't be decoded: why
    pub failed: Option<String>,
}

impl SoundClip {
    /// What the file keeps of it (and what a change to is an unsaved change).
    pub fn key(&self) -> (String, String, i64, u32, bool) {
        (
            self.name.clone(),
            self.file.clone(),
            self.start,
            self.gain.to_bits(),
            self.muted,
        )
    }
}

/// The sound as the interface sees it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoundUi {
    /// the open scene's clips
    pub clips: Vec<SoundClip>,
    /// the scene frame the open cut's frame 0 is
    pub cut_start: i64,
    /// the clips are the open project's (saved with it); false: the demo's
    pub from_file: bool,
}

impl SoundUi {
    /// The design's example: a line of dialogue on frames 7–28.
    pub fn demo() -> SoundUi {
        SoundUi {
            clips: vec![SoundClip {
                name: "dialogue".into(),
                file: String::new(),
                start: 6,
                gain: 1.0,
                muted: false,
                frames: 22,
                peaks: Arc::default(),
                failed: None,
            }],
            cut_start: 0,
            from_file: false,
        }
    }

    /// The clips as the file keeps them.
    pub fn file_clips(&self) -> Vec<ppstudio::AudioClip> {
        self.clips
            .iter()
            .map(|c| ppstudio::AudioClip {
                name: c.name.clone(),
                file: c.file.clone(),
                start: c.start,
                gain: c.gain,
                muted: c.muted,
            })
            .collect()
    }
}

/// A sound file being decoded or decoded: filled once, by the decoding thread; waited on by
/// an export, looked at (never waited on) by the window.
#[derive(Default)]
pub struct Slot {
    done: Mutex<Option<Result<Arc<Pcm>, String>>>,
    ready: Condvar,
}

impl Slot {
    fn fill(&self, r: Result<Arc<Pcm>, String>) {
        if let Ok(mut g) = self.done.lock() {
            *g = Some(r);
        }
        self.ready.notify_all();
    }

    /// The sound if it's decoded (None: not yet, or it couldn't be), without waiting.
    pub fn try_get(&self) -> Option<Result<Arc<Pcm>, String>> {
        self.done.try_lock().ok().and_then(|g| g.clone())
    }

    /// The sound, waiting for it (an export's thread).
    pub fn wait(&self) -> Result<Arc<Pcm>, String> {
        let mut g = self.done.lock().map_err(|e| e.to_string())?;
        loop {
            if let Some(r) = g.as_ref() {
                return r.clone();
            }
            g = self.ready.wait(g).map_err(|e| e.to_string())?;
        }
    }
}

/// What the export takes to mix the sound: the clips (their files being decoded or decoded),
/// from which scene frame, for how many frames, at what rate.
#[derive(Clone)]
pub struct ExportSound {
    pub clips: Vec<(Arc<Slot>, i64, f32)>,
    pub from: i64,
    pub frames: u32,
    pub fps: u32,
}

impl ExportSound {
    /// Writes the mix to a WAV file in the temporary folder (on the export's thread: it waits
    /// for files still being decoded). None when nothing is heard.
    pub fn write_wav(&self) -> Result<Option<PathBuf>, String> {
        let clips: Vec<mix::Clip> = self
            .clips
            .iter()
            .filter_map(|(slot, start, gain)| {
                slot.wait().ok().map(|pcm| mix::Clip {
                    pcm,
                    start: *start,
                    gain: *gain,
                    muted: false,
                })
            })
            .collect();
        let parts = [mix::Part {
            clips: &clips,
            from: self.from,
            frames: self.frames,
        }];
        if !mix::audible(&parts) {
            return Ok(None);
        }
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pencilpilot-export-{}-{}.wav",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        mix::write_wav(std::io::BufWriter::new(f), &parts, self.fps).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            e.to_string()
        })?;
        Ok(Some(path))
    }
}

/// What was last sent to the mixer.
#[derive(Debug, Clone, PartialEq)]
enum Sent {
    Nothing,
    Play(Clock, Vec<(String, i64, u32)>),
    Snippet,
}

/// The sound's part of the engine.
pub struct Sounds {
    player: Option<Player>,
    /// decoded files, by name
    cache: HashMap<String, Arc<Slot>>,
    /// waveforms made off the window's thread: (file, fps, peaks)
    peaks_rx: mpsc::Receiver<(String, u32, Vec<f32>)>,
    peaks_tx: mpsc::Sender<(String, u32, Vec<f32>)>,
    /// waveforms being made: (file, fps)
    peaks_due: HashSet<(String, u32)>,
    /// the document and frame last seen
    doc: Option<u32>,
    frame: u32,
    sent: Sent,
    /// when to pause the output (nothing to play for a while)
    pause_at: Option<Instant>,
    /// frames that reached the screen while playing (the frame, when), not looked at yet;
    /// the last one seen; how early or late each frame showed against playback's clock
    /// (seconds, the latest few), and the shift the sound follows
    shown: Vec<(u32, Instant)>,
    last_shown: Option<u32>,
    lateness: std::collections::VecDeque<f64>,
    pub shift: f64,
    /// for checks: when Play was asked for, the shift each play started with, and what the
    /// (silent) output played
    pub plays: Vec<Instant>,
    pub play_shifts: Vec<f64>,
    pub recs: Vec<Rec>,
}

impl Default for Sounds {
    fn default() -> Self {
        let (peaks_tx, peaks_rx) = mpsc::channel();
        Sounds {
            player: None,
            cache: HashMap::new(),
            peaks_rx,
            peaks_tx,
            peaks_due: HashSet::new(),
            doc: None,
            frame: u32::MAX,
            sent: Sent::Nothing,
            pause_at: None,
            shown: Vec::new(),
            last_shown: None,
            lateness: Default::default(),
            shift: 0.0,
            plays: Vec::new(),
            play_shifts: Vec::new(),
            recs: Vec::new(),
        }
    }
}

impl std::fmt::Debug for Sounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sounds")
            .field("files", &self.cache.len())
            .field("sent", &self.sent)
            .finish_non_exhaustive()
    }
}

/// How long the output stays open after the sound stops (a play or a scrub right after
/// doesn't wait for it to open again).
const KEEP_OPEN: Duration = Duration::from_millis(1500);

/// A scrub's snippet: about two frames' worth, at least 1/12 s (as the current app's).
pub fn snippet_len(fps: u32) -> u32 {
    let secs = (2.0 / fps.max(1) as f64).max(1.0 / 12.0);
    (secs * RATE as f64).round() as u32
}

impl Sounds {
    /// The decoded file, if it is.
    pub fn slot(&self, file: &str) -> Option<&Arc<Slot>> {
        self.cache.get(file)
    }

    fn player(&mut self) -> &mut Player {
        self.player.get_or_insert_with(|| {
            let out = output();
            let record = matches!(out, Output::Null { .. });
            Player::new(out, record)
        })
    }

    /// What the output played, for `--stats` (with the open cut's place in its scene and
    /// its rate, to say which frame each sample belongs to).
    pub fn stats_json(&mut self, cut_start: i64, fps: u32) -> String {
        self.drain();
        let base_i = Instant::now();
        let base_ns = pencilpilot_window::clock::now_ns();
        let ns = |t: Instant| -> i128 {
            base_ns as i128 - (base_i.saturating_duration_since(t).as_nanos() as i128)
                + (t.saturating_duration_since(base_i).as_nanos() as i128)
        };
        let (kind, rate, lat) = match &self.player {
            Some(p) => (
                match p.output() {
                    Output::Null { .. } => "null",
                    Output::Device(_) => "device",
                },
                p.info.rate.load(Ordering::Relaxed),
                p.info.latency_ns.load(Ordering::Relaxed),
            ),
            None => ("none", 0, 0),
        };
        let plays: Vec<String> = self.plays.iter().map(|t| ns(*t).to_string()).collect();
        let bufs: Vec<String> = self
            .recs
            .iter()
            .map(|r| {
                format!(
                    "[{}, {}, {:.4}, {}]",
                    ns(r.heard_at),
                    if r.pos.is_nan() {
                        "null".to_string()
                    } else {
                        format!("{:.2}", r.pos)
                    },
                    r.peak,
                    r.frames
                )
            })
            .collect();
        format!(
            "{{\"output\": \"{kind}\", \"rate\": {rate}, \"latencyNs\": {lat}, \"cutStart\": {cut_start}, \"fps\": {fps}, \"shiftMs\": {:.3}, \"playShiftsMs\": [{}], \"playLateMs\": [{}], \"playsNs\": [{}], \"buffers\": [{}]}}",
            self.shift * 1e3,
            self.play_shifts
                .iter()
                .map(|v| format!("{:.3}", v * 1e3))
                .collect::<Vec<_>>()
                .join(", "),
            self.lateness
                .iter()
                .map(|v| format!("{:.3}", v * 1e3))
                .collect::<Vec<_>>()
                .join(", "),
            plays.join(", "),
            bufs.join(", ")
        )
    }

    /// A frame reached the screen (the timeline frame it showed, the presentation's time,
    /// CLOCK_MONOTONIC ns): while playing, how early or late it was against playback's clock
    /// is how the sound is shifted to match.
    pub fn shown(&mut self, frame: u32, at_ns: u64) {
        if !matches!(self.sent, Sent::Play(..)) || self.shown.len() >= 256 {
            return;
        }
        let now_ns = pencilpilot_window::clock::now_ns();
        let now = Instant::now();
        let at = if at_ns <= now_ns {
            now.checked_sub(Duration::from_nanos(now_ns - at_ns))
        } else {
            now.checked_add(Duration::from_nanos(at_ns - now_ns))
        };
        if let Some(at) = at {
            self.shown.push((frame, at));
        }
    }

    /// The frames shown since the last tick against `clock`: the shift the sound follows
    /// (the median of the latest 48 frames' lateness, from the third; moved when it's 0.5 ms
    /// away).
    fn follow(&mut self, clock: &Clock) {
        for (f, t) in std::mem::take(&mut self.shown) {
            // (a frame's first showing only)
            if self.last_shown == Some(f) {
                continue;
            }
            self.last_shown = Some(f);
            if let Some(l) = clock.lateness(f, t)
                && l.abs() < 0.1
            {
                self.lateness.push_back(l);
                if self.lateness.len() > 48 {
                    self.lateness.pop_front();
                }
            }
        }
        // (the shift found by the last play holds until this one's first frames say)
        if self.lateness.len() >= 3 {
            let mut v: Vec<f64> = self.lateness.iter().copied().collect();
            v.sort_by(f64::total_cmp);
            let m = v[v.len() / 2];
            if (m - self.shift).abs() > 0.0005 {
                self.shift = m;
            }
        }
    }

    fn drain(&mut self) {
        if let Some(p) = &mut self.player {
            p.collect();
            let r = p.take_recs();
            self.recs.extend(r);
        }
    }
}

/// The open scene's clips and where the open cut starts in it, from the file as opened.
fn scene_clips(e: &Engine) -> Option<(Vec<ppstudio::AudioClip>, i64)> {
    let doc = &e.files.doc;
    let base = doc.base.as_ref()?;
    let scene = base.timeline.scenes.get(doc.scene)?;
    let statics = &base.timeline.static_layers;
    let start: i64 = scene
        .cuts
        .iter()
        .take(doc.cut)
        .map(|c| c.length(statics) as i64)
        .sum();
    Some((scene.audio.clone(), start))
}

/// Decodes the files not decoded yet, on a thread of their own, from the project's file (or
/// its kept-open copy, read here, when it has no path).
fn decode_files(e: &mut Engine, s: &State, files: Vec<String>) {
    if files.is_empty() {
        return;
    }
    let mut jobs: Vec<(String, Arc<Slot>, Option<Vec<u8>>)> = Vec::new();
    for f in files {
        let slot = Arc::new(Slot::default());
        e.sound.cache.insert(f.clone(), slot.clone());
        // (no path: read from the kept-open file now)
        let bytes = match &s.project.path {
            Some(_) => None,
            None => e
                .files
                .doc
                .source
                .as_mut()
                .and_then(|o| o.entry(&format!("audio/{f}"), ppstudio::AUDIO_MAX).ok()),
        };
        jobs.push((f, slot, bytes));
    }
    let path = s.project.path.clone();
    let fps = s.fps.max(1);
    let tx = e.sound.peaks_tx.clone();
    for (f, _, _) in &jobs {
        e.sound.peaks_due.insert((f.clone(), fps));
    }
    let spawned = std::thread::Builder::new()
        .name("sound-decode".into())
        .spawn(move || {
            lower_priority();
            let ffmpeg = pencilpilot_export::find_ffmpeg();
            let mut opened = path.as_ref().and_then(|p| {
                let f = std::fs::File::open(p).ok()?;
                ppstudio::Opened::read(std::io::BufReader::new(f)).ok()
            });
            for (name, slot, bytes) in jobs {
                let bytes = bytes.or_else(|| {
                    opened
                        .as_mut()?
                        .entry(&format!("audio/{name}"), ppstudio::AUDIO_MAX)
                        .ok()
                });
                let ext = name.rsplit_once('.').map_or("", |(_, x)| x).to_string();
                let r = match bytes {
                    Some(b) => {
                        pencilpilot_sound::decode::decode(&b, &ext, ffmpeg.as_deref()).map(Arc::new)
                    }
                    None => Err("the sound file isn't in the project".into()),
                };
                if let Ok(p) = &r {
                    let _ = tx.send((name.clone(), fps, pencilpilot_sound::peaks::peaks(p, fps)));
                } else {
                    let _ = tx.send((name.clone(), fps, Vec::new()));
                }
                slot.fill(r);
            }
        });
    if spawned.is_err() {
        e.sound.peaks_due.clear();
    }
}

/// The decoding threads run below the window's priority.
fn lower_priority() {
    #[cfg(unix)]
    // SAFETY: this thread's own nice value
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 5);
    }
}

/// A waveform at another rate (the cut's rate changed), off the window's thread.
fn make_peaks(e: &mut Engine, file: &str, fps: u32, pcm: Arc<Pcm>) {
    if !e.sound.peaks_due.insert((file.to_string(), fps)) {
        return;
    }
    let tx = e.sound.peaks_tx.clone();
    let name = file.to_string();
    let _ = std::thread::Builder::new()
        .name("sound-peaks".into())
        .spawn(move || {
            let _ = tx.send((name, fps, pencilpilot_sound::peaks::peaks(&pcm, fps)));
        });
}

/// What the interface asks for that's the sound's (Play's moment, for checks).
pub fn apply(e: &mut Engine, _s: &mut State, a: &Action) {
    if *a == Action::Play {
        e.sound.plays.push(Instant::now());
    }
}

/// The clips the mixer plays now: not muted, decoded.
fn voices(e: &Engine, s: &State) -> (Vec<Voice>, Vec<(String, i64, u32)>) {
    let fps = s.fps.max(1);
    let mut v = Vec::new();
    let mut key = Vec::new();
    for c in s.sound.clips.iter().filter(|c| !c.muted && c.gain > 0.0) {
        if let Some(Ok(pcm)) = e.sound.cache.get(&c.file).and_then(|sl| sl.try_get()) {
            v.push(Voice {
                pcm,
                start: frame_sample(c.start, fps),
                gain: c.gain,
            });
            key.push((c.file.clone(), c.start, c.gain.to_bits()));
        }
    }
    (v, key)
}

/// Another document: its open scene's clips (and their files decoded). Called when a file
/// is opened or a new one made (before it counts as saved), and by [`tick`]. Returns whether
/// the interface changed.
pub fn load_doc(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    if e.sound.doc == Some(s.doc_id) {
        return false;
    }
    let mut changed = false;
    e.sound.doc = Some(s.doc_id);
    e.sound.frame = s.frame;
    match scene_clips(e) {
        Some((clips, cut_start)) => {
            s.sound = SoundUi {
                clips: clips
                    .into_iter()
                    .map(|c| SoundClip {
                        name: c.name,
                        file: c.file,
                        start: c.start,
                        gain: c.gain,
                        muted: c.muted,
                        frames: 0,
                        peaks: Arc::default(),
                        failed: None,
                    })
                    .collect(),
                cut_start,
                from_file: true,
            };
            changed = true;
        }
        None if s.sound.from_file || s.doc_id != 0 => {
            // a new document (not the demo's)
            s.sound = SoundUi::default();
            changed = true;
        }
        None => {}
    }
    let used: HashSet<String> = s.sound.clips.iter().map(|c| c.file.clone()).collect();
    e.sound.cache.retain(|f, _| used.contains(f));
    let missing: Vec<String> = used
        .into_iter()
        .filter(|f| !f.is_empty() && !e.sound.cache.contains_key(f))
        .collect();
    decode_files(e, s, missing);
    if e.sound.sent != Sent::Nothing {
        if let Some(p) = &mut e.sound.player {
            p.send(Cmd::Stop);
        }
        e.sound.sent = Sent::Nothing;
        e.sound.pause_at = Some(now + KEEP_OPEN);
    }
    changed
}

/// Once a loop, after playback's: the document's clips, decoding, the waveforms, and what
/// the mixer plays. Returns whether the interface changed.
pub fn tick(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    let mut changed = false;
    // another document: its scene's clips
    changed |= load_doc(e, s, now);
    let fps = s.fps.max(1);
    // waveforms made
    while let Ok((file, at, peaks)) = e.sound.peaks_rx.try_recv() {
        e.sound.peaks_due.remove(&(file.clone(), at));
        let decoded = e.sound.cache.get(&file).and_then(|sl| sl.try_get());
        for c in s.sound.clips.iter_mut().filter(|c| c.file == file) {
            match &decoded {
                Some(Ok(pcm)) if at == fps => {
                    c.frames = pcm.frames(fps);
                    c.peaks = Arc::new(peaks.clone());
                    c.failed = None;
                }
                Some(Err(m)) => c.failed = Some(m.clone()),
                _ => {}
            }
            changed = true;
        }
    }
    // the cut's rate changed: waveforms at the new one
    let stale: Vec<(String, Arc<Pcm>)> = s
        .sound
        .clips
        .iter()
        .filter_map(|c| match e.sound.cache.get(&c.file)?.try_get()? {
            Ok(p) if c.frames != p.frames(fps) => Some((c.file.clone(), p)),
            _ => None,
        })
        .collect();
    for (f, p) in stale {
        make_peaks(e, &f, fps, p);
    }
    // what's heard
    let want = if s.playing
        && let Some((t0, from, at)) = e.frames.play_clock()
    {
        let (a, b) = super::frames::play_range(s);
        let clock = Clock {
            t0,
            from,
            fps: at,
            range: (a, b),
            mode: match s.strips.play_mode {
                0 => Mode::Once,
                2 => Mode::PingPong,
                _ => Mode::Loop,
            },
            cut_start: s.sound.cut_start,
            shift: 0.0,
        };
        // a new play: the frames shown before it say nothing about it
        let new = !matches!(&e.sound.sent, Sent::Play(c, _) if c.same_play(&clock));
        if new {
            e.sound.shown.clear();
            e.sound.last_shown = None;
            e.sound.lateness.clear();
        }
        e.sound.follow(&clock);
        if new {
            e.sound.play_shifts.push(e.sound.shift);
        }
        Some(Clock {
            shift: e.sound.shift,
            ..clock
        })
    } else {
        None
    };
    let audible = s.sound.clips.iter().any(|c| !c.muted && !c.file.is_empty());
    match want {
        Some(clock) => {
            let (vs, key) = voices(e, s);
            let next = Sent::Play(clock, key);
            if next != e.sound.sent && (audible || e.sound.sent != Sent::Nothing) {
                e.sound.player().send(Cmd::Play { voices: vs, clock });
                e.sound.sent = next;
                e.sound.pause_at = None;
            }
        }
        None => {
            if matches!(e.sound.sent, Sent::Play(..)) {
                if let Some(p) = &mut e.sound.player {
                    p.send(Cmd::Stop);
                }
                e.sound.sent = Sent::Nothing;
                e.sound.pause_at = Some(now + KEEP_OPEN);
            }
            // scrubbing: a snippet of the frame moved to
            if s.frame != e.sound.frame && !s.playing && audible {
                let (vs, _) = voices(e, s);
                if !vs.is_empty() {
                    let at = frame_sample(s.sound.cut_start + s.frame as i64, fps);
                    let len = snippet_len(fps);
                    e.sound.player().send(Cmd::Snippet {
                        voices: vs,
                        at,
                        len,
                    });
                    e.sound.sent = Sent::Snippet;
                    e.sound.pause_at =
                        Some(now + Duration::from_secs_f64(len as f64 / RATE as f64) + KEEP_OPEN);
                }
            }
        }
    }
    e.sound.frame = s.frame;
    if let Some(t) = e.sound.pause_at
        && Instant::now() >= t
        && !s.playing
    {
        e.sound.pause_at = None;
        if let Some(p) = &mut e.sound.player {
            p.run(false);
        }
        if e.sound.sent == Sent::Snippet {
            e.sound.sent = Sent::Nothing;
        }
    }
    e.sound.drain();
    changed
}

/// When [`tick`] next has something to do.
pub fn next_tick_at(e: &Engine, s: &State) -> Option<Instant> {
    let now = Instant::now();
    let mut at: Vec<Instant> = Vec::new();
    if e.sound.doc != Some(s.doc_id) || e.sound.frame != s.frame {
        at.push(now);
    }
    if !e.sound.peaks_due.is_empty() {
        // (a decode's result is picked up within a few frames)
        at.push(now + Duration::from_millis(30));
    }
    if let Some(t) = e.sound.pause_at {
        at.push(t);
    }
    at.into_iter().min()
}

/// The sound an export of cut frames `first..=last` mixes (None: nothing to hear).
pub fn for_export(e: &Engine, s: &State, first: u32, last: u32) -> Option<ExportSound> {
    let clips: Vec<(Arc<Slot>, i64, f32)> = s
        .sound
        .clips
        .iter()
        .filter(|c| !c.muted && c.gain > 0.0)
        .filter_map(|c| Some((e.sound.cache.get(&c.file)?.clone(), c.start, c.gain)))
        .collect();
    (!clips.is_empty()).then(|| ExportSound {
        clips,
        from: s.sound.cut_start + first as i64,
        frames: last + 1 - first,
        fps: s.fps.max(1),
    })
}
