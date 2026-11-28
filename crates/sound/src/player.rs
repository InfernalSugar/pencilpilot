//! Playing a scene's sounds in step with the picture.
//!
//! The window thread sends [`Cmd`]s (play from the picture's clock, a scrubbing snippet,
//! stop) through a lock-free ring; the [`Mixer`] runs in the sound output's callback and
//! never allocates, locks or frees there: what it's done with goes back through a second ring
//! to be dropped on the window's side.
//!
//! **Staying in step.** Playback's clock is the picture's ([`Clock`]: when a frame shows, and
//! the rate). For each buffer the mixer knows when its first sample will be heard (the
//! output's own timestamps, from cpal), so it knows which scene sample that is. It keeps its
//! own estimate of that time advancing sample by sample, and pulls it gently towards the
//! clock (at most 0.2 % faster or slower: no audible pitch change), so a sound card whose
//! crystal runs a little fast or slow never drifts from the picture. A jump (a seek, a loop's
//! restart, an underrun) larger than 3 ms resynchronises at once.
//!
//! **Outputs.** [`Output::Device`]: the system's (cpal: ALSA, which reaches PipeWire or
//! PulseAudio through their ALSA plugins). [`Output::Null`]: no device at all, a thread that
//! pulls buffers as a sound card would (a period, a latency, optionally a clock that runs
//! fast or slow): for tests and checks, which must never make a sound.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::{Pcm, RATE};

/// A clip as the mixer plays it (muted clips aren't sent).
#[derive(Debug, Clone)]
pub struct Voice {
    pub pcm: Arc<Pcm>,
    /// the scene sample (at [`RATE`]) its first sample is on
    pub start: i64,
    pub gain: f32,
}

/// How playback goes over its frames (the timeline's play modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Once,
    Loop,
    /// forwards and back: heard on the way forwards only (as the current app)
    PingPong,
}

/// The picture's playback clock: cut frame `from` shows at `t0`, `fps` frames a second, over
/// the cut's frames `range` (inclusive) in `mode`; the cut's frame 0 is scene frame
/// `cut_start`. `shift`: the frames as they reach the screen are that much later than the
/// clock says (seconds; negative: earlier), measured from their presentation times; the
/// sound follows it, gently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub t0: Instant,
    pub from: u32,
    pub fps: u32,
    pub range: (u32, u32),
    pub mode: Mode,
    pub cut_start: i64,
    pub shift: f64,
}

impl Clock {
    /// The same playback (a change of `shift` alone is followed without a jump).
    pub fn same_play(&self, o: &Clock) -> bool {
        Clock {
            shift: 0.0,
            ..*self
        } == Clock { shift: 0.0, ..*o }
    }

    /// How early (negative) or late a frame showing at `t` is against this clock: the
    /// difference from the nearest time `frame` starts (seconds). None in ping-pong, or
    /// for a frame outside the range.
    pub fn lateness(&self, frame: u32, t: Instant) -> Option<f64> {
        let (a, b) = (self.range.0, self.range.1.max(self.range.0));
        if !(a..=b).contains(&frame) || self.mode == Mode::PingPong {
            return None;
        }
        let fps = self.fps.max(1) as f64;
        let len = (b - a + 1) as f64;
        let off = self.from.clamp(a, b) as f64 - a as f64 + secs(self.t0, t) * fps;
        let at = (frame - a) as f64;
        let best = match self.mode {
            Mode::Loop => {
                let cycle = ((off - at) / len).round();
                at + cycle * len
            }
            _ => at,
        };
        Some((off - best) / fps)
    }

    /// The cut frame (fractional) showing `t` seconds after `t0`, or None when nothing is
    /// heard then (before the start, after a once-through's end, a ping-pong's way back).
    pub fn cut_frame(&self, t: f64) -> Option<f64> {
        if t < 0.0 {
            return None;
        }
        let (a, b) = (self.range.0 as f64, self.range.1.max(self.range.0) as f64);
        let len = b - a + 1.0;
        let off = (self.from as f64).clamp(a, b) - a + t * self.fps.max(1) as f64;
        let f = match self.mode {
            Mode::Once if off >= len => return None,
            Mode::Once => off,
            Mode::Loop => off.rem_euclid(len),
            Mode::PingPong if len > 2.0 => {
                let m = off.rem_euclid(2.0 * (len - 1.0));
                if m >= len {
                    return None;
                }
                m
            }
            Mode::PingPong => off.rem_euclid(len),
        };
        Some(a + f)
    }

    /// The scene sample (at [`RATE`], fractional) heard `t` seconds after `t0`.
    pub fn scene_sample(&self, t: f64) -> Option<f64> {
        self.cut_frame(t)
            .map(|f| (self.cut_start as f64 + f) * RATE as f64 / self.fps.max(1) as f64)
    }
}

/// When a play's sound actually starts (see [`Cmd::Play`]'s `start`): filled once, by the
/// mixer, with the time its first sample is heard.
#[derive(Debug)]
pub struct Started {
    base: Instant,
    /// ns after `base`; `u64::MAX` until filled
    at: AtomicU64,
}

impl Default for Started {
    fn default() -> Self {
        Started {
            base: Instant::now(),
            at: AtomicU64::new(u64::MAX),
        }
    }
}

impl Started {
    /// When the play's first sample is heard, once the mixer knows.
    pub fn at(&self) -> Option<Instant> {
        match self.at.load(Ordering::Acquire) {
            u64::MAX => None,
            ns => Some(self.base + Duration::from_nanos(ns)),
        }
    }
}

/// What the mixer is asked to do.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// play along with the picture. With `start`, a play just begun: its clock's `t0` moves to
    /// when the output's first buffer is heard, if that's later, and says so through `start`
    /// (the picture waits for it): the first frame's sound is heard from its start.
    Play {
        voices: Vec<Voice>,
        clock: Clock,
        start: Option<Arc<Started>>,
    },
    /// a snippet while scrubbing: `len` samples from scene sample `at`, faded in and out
    Snippet {
        voices: Vec<Voice>,
        at: i64,
        len: u32,
    },
    Stop,
}

/// A buffer the mixer made (for checks): when its first sample is heard, the scene sample
/// it is (NaN: silence), its loudest sample, and its length.
#[derive(Debug, Clone, Copy)]
pub struct Rec {
    pub heard_at: Instant,
    pub pos: f64,
    pub peak: f32,
    pub frames: u32,
    /// the output's rate
    pub rate: u32,
}

/// A snippet's fades: in over 5 ms, out over the last 10 ms (as the current app's).
const FADE_IN: f64 = 0.005;
const FADE_OUT: f64 = 0.010;
/// A difference from the clock bigger than this is a jump (an underrun, a seek, a loop's
/// restart): resynchronised at once. Output timestamps are good to well under a millisecond,
/// so anything this big is real.
const RESYNC: f64 = 0.003;
/// How far the mixer's time may run faster or slower than real time to follow the clock.
const MAX_WARP: f64 = 0.002;
/// A change of the picture's shift bigger than this is jumped to, not followed.
const SHIFT_JUMP: f64 = 0.003;

/// The window's side of the rings.
pub struct Remote {
    tx: rtrb::Producer<Box<Cmd>>,
    trash: rtrb::Consumer<Box<Cmd>>,
}

impl Remote {
    /// Sends a command (false: the ring was full and it was dropped). Takes back what the
    /// mixer is done with first.
    pub fn send(&mut self, c: Cmd) -> bool {
        self.collect();
        self.tx.push(Box::new(c)).is_ok()
    }

    /// Drops (here, not in the sound's thread) what the mixer is done with.
    pub fn collect(&mut self) {
        while self.trash.pop().is_ok() {}
    }
}

/// The real-time mixer (it runs in the output's callback).
pub struct Mixer {
    cmds: rtrb::Consumer<Box<Cmd>>,
    trash: rtrb::Producer<Box<Cmd>>,
    now: Option<Box<Cmd>>,
    /// playing: the time (s after the clock's `t0`) of the next sample, and the warp
    t: f64,
    err: f64,
    warp: f64,
    locked: bool,
    /// a snippet: samples played of it (at [`RATE`])
    snip: f64,
    rec: Option<rtrb::Producer<Rec>>,
}

/// The rings between the window and a mixer; with `record`, what the mixer plays is
/// reported too (for checks).
pub fn channel(record: bool) -> (Remote, Mixer, Option<rtrb::Consumer<Rec>>) {
    let (tx, cmds) = rtrb::RingBuffer::new(64);
    let (trash_tx, trash) = rtrb::RingBuffer::new(128);
    let (rec, recs) = if record {
        let (p, c) = rtrb::RingBuffer::new(1 << 16);
        (Some(p), Some(c))
    } else {
        (None, None)
    };
    (
        Remote { tx, trash },
        Mixer {
            cmds,
            trash: trash_tx,
            now: None,
            t: 0.0,
            err: 0.0,
            warp: 0.0,
            locked: false,
            snip: 0.0,
            rec,
        },
        recs,
    )
}

/// Seconds from `a` to `b` (negative when `b` is earlier).
fn secs(a: Instant, b: Instant) -> f64 {
    if b >= a {
        (b - a).as_secs_f64()
    } else {
        -(a - b).as_secs_f64()
    }
}

/// A sound's sample at a fractional position (Catmull-Rom; silence outside it).
#[inline]
fn at(d: &[f32], x: f64) -> f32 {
    let i = x.floor();
    let f = (x - i) as f32;
    let i = i as i64;
    let n = d.len() as i64;
    if i < -2 || i > n {
        return 0.0;
    }
    let g = |k: i64| {
        if (0..n).contains(&k) {
            d[k as usize]
        } else {
            0.0
        }
    };
    if f == 0.0 {
        return g(i);
    }
    let (y0, y1, y2, y3) = (g(i - 1), g(i), g(i + 1), g(i + 2));
    let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
    let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c = -0.5 * y0 + 0.5 * y2;
    ((a * f + b) * f + c) * f + y1
}

/// The voices at scene sample `s`, with `env` on top.
#[inline]
fn voices_at(vs: &[Voice], s: f64, env: f32) -> (f32, f32) {
    let (mut l, mut r) = (0.0, 0.0);
    for v in vs {
        let x = s - v.start as f64;
        if x < -2.0 || x >= v.pcm.len() as f64 + 1.0 {
            continue;
        }
        let g = v.gain * env;
        l += at(&v.pcm.left, x) * g;
        r += at(v.pcm.right(), x) * g;
    }
    (l, r)
}

impl Mixer {
    /// Fills `out` (`channels` interleaved, at `rate` Hz) whose first sample is heard at
    /// `heard_at`. Real-time safe: no allocation, no lock, nothing freed.
    pub fn render(&mut self, out: &mut [f32], channels: usize, rate: u32, heard_at: Instant) {
        let ch = channels.max(1);
        let frames = out.len() / ch;
        while let Ok(c) = self.cmds.pop() {
            // the same play goes on smoothly; a new one, or the picture found to be more than
            // 3 ms from where the sound follows it (the first frames shown say so), starts
            // where the clock says at once
            let same_clock = matches!(
                (&self.now.as_deref(), &*c),
                (Some(Cmd::Play { clock: a, .. }), Cmd::Play { clock: b, .. })
                    if a.same_play(b) && (a.shift - b.shift).abs() <= SHIFT_JUMP
            );
            if !same_clock {
                self.locked = false;
            }
            self.snip = 0.0;
            if let Some(old) = self.now.replace(c)
                && let Err(rtrb::PushError::Full(old)) = self.trash.push(old)
            {
                // (never freed here: kept for good, which a full ring makes rare)
                std::mem::forget(old);
            }
        }
        let rate_f = rate.max(1) as f64;
        let mut peak = 0f32;
        let mut pos = f64::NAN;
        match self.now.as_deref_mut() {
            Some(Cmd::Play {
                voices,
                clock,
                start,
            }) => {
                // a play begun: it starts when its first sample can be heard (the window
                // keeps its own `Started`: nothing is freed here)
                if let Some(st) = start
                    && st.at.load(Ordering::Relaxed) == u64::MAX
                {
                    if heard_at > clock.t0 {
                        clock.t0 = heard_at;
                    }
                    let ns = clock.t0.saturating_duration_since(st.base).as_nanos() as u64;
                    st.at.store(ns.min(u64::MAX - 1), Ordering::Release);
                    self.locked = false;
                }
                let (voices, clock) = (&*voices, &*clock);
                let measured = secs(clock.t0, heard_at) - clock.shift;
                if !self.locked || (measured - self.t).abs() > RESYNC {
                    self.t = measured;
                    self.err = 0.0;
                    self.warp = 0.0;
                    self.locked = true;
                } else {
                    // the difference, smoothed, pulls the mixer's time towards the clock
                    let e = measured - self.t;
                    self.err += (e - self.err) * 0.1;
                    self.warp = (self.err / 0.5).clamp(-MAX_WARP, MAX_WARP);
                }
                let dt = (1.0 + self.warp) / rate_f;
                for i in 0..frames {
                    let t = self.t + i as f64 * dt;
                    let (l, r) = match clock.scene_sample(t) {
                        Some(s) => {
                            if i == 0 {
                                pos = s;
                            }
                            voices_at(voices, s, 1.0)
                        }
                        None => (0.0, 0.0),
                    };
                    peak = peak.max(l.abs()).max(r.abs());
                    put(out, i, ch, l, r);
                }
                self.t += frames as f64 * dt;
            }
            Some(Cmd::Snippet { voices, at, len }) => {
                let (voices, at, len) = (&*voices, &*at, &*len);
                let step = RATE as f64 / rate_f;
                let len = *len as f64;
                let (fin, fout) = (FADE_IN * RATE as f64, FADE_OUT * RATE as f64);
                for i in 0..frames {
                    let k = self.snip;
                    let (l, r) = if k < len {
                        if i == 0 {
                            pos = *at as f64 + k;
                        }
                        let env = (k / fin).min(1.0).min((len - k) / fout).max(0.0) as f32;
                        voices_at(voices, *at as f64 + k, env)
                    } else {
                        (0.0, 0.0)
                    };
                    self.snip += step;
                    peak = peak.max(l.abs()).max(r.abs());
                    put(out, i, ch, l, r);
                }
            }
            Some(Cmd::Stop) | None => out.fill(0.0),
        }
        if let Some(rec) = &mut self.rec {
            let _ = rec.push(Rec {
                heard_at,
                pos,
                peak,
                frames: frames as u32,
                rate,
            });
        }
    }
}

#[inline]
fn put(out: &mut [f32], i: usize, ch: usize, l: f32, r: f32) {
    let o = &mut out[i * ch..(i + 1) * ch];
    if ch == 1 {
        o[0] = (l + r) * 0.5;
    } else {
        o[0] = l;
        o[1] = r;
        o[2..].fill(0.0);
    }
}

/// Where the sound goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// the system's output (cpal); a device id (`"alsa:null"`…), or None for the default
    Device(Option<String>),
    /// no device: a thread pulling `period` samples at a time at `rate`, heard `latency`
    /// after it's asked, its clock running `drift` faster (0.0001: 100 ppm)
    Null {
        rate: u32,
        period: u32,
        latency: Duration,
        drift: f64,
    },
}

impl Output {
    /// The silent output tests and checks use: 10 ms periods heard 20 ms later.
    pub fn null() -> Output {
        Output::Null {
            rate: RATE,
            period: 480,
            latency: Duration::from_millis(20),
            drift: 0.0,
        }
    }
}

/// What the output's thread is told.
enum Ctl {
    /// open the device, not playing yet (so a play starts at once)
    Prepare,
    Run,
    Pause,
}

/// The output's thread's report.
#[derive(Debug, Default)]
pub struct OutputInfo {
    /// samples a second (0 until it's open)
    pub rate: AtomicU64,
    /// the output's latency last seen, ns
    pub latency_ns: AtomicU64,
    /// it couldn't be opened, or failed
    pub failed: AtomicBool,
}

/// The sound being played: the rings to a mixer, and its output (opened, on a thread of
/// its own, the first time it's needed; paused while there's nothing to play).
pub struct Player {
    remote: Remote,
    output: Output,
    mixer: Option<Mixer>,
    ctl: Option<mpsc::Sender<Ctl>>,
    running: bool,
    pub info: Arc<OutputInfo>,
    recs: Option<rtrb::Consumer<Rec>>,
}

impl Player {
    /// A player for `output`; `record`: what it plays is kept for [`Player::take_recs`].
    pub fn new(output: Output, record: bool) -> Player {
        let (remote, mixer, recs) = channel(record);
        Player {
            remote,
            output,
            mixer: Some(mixer),
            ctl: None,
            running: false,
            info: Arc::default(),
            recs,
        }
    }

    pub fn output(&self) -> &Output {
        &self.output
    }

    /// Sends a command, starting the output if it isn't running.
    pub fn send(&mut self, c: Cmd) {
        self.remote.send(c);
        self.run(true);
    }

    /// Starts (true) or pauses the output.
    pub fn run(&mut self, on: bool) {
        if on == self.running {
            return;
        }
        if on && self.ctl.is_none() {
            let Some(mixer) = self.mixer.take() else {
                return;
            };
            self.ctl = Some(start(self.output.clone(), mixer, self.info.clone()));
        }
        if let Some(c) = &self.ctl {
            let _ = c.send(if on { Ctl::Run } else { Ctl::Pause });
        }
        self.running = on;
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Opens the output without playing (paused, it costs nothing), so the next play or
    /// scrub starts at once.
    pub fn prepare(&mut self) {
        if self.ctl.is_none() {
            let Some(mixer) = self.mixer.take() else {
                return;
            };
            self.ctl = Some(start(self.output.clone(), mixer, self.info.clone()));
        }
        if let Some(c) = &self.ctl {
            let _ = c.send(Ctl::Prepare);
        }
    }

    /// How long after it's asked the output plays (its latency once known; 50 ms until).
    pub fn start_latency(&self) -> Duration {
        match self.info.latency_ns.load(Ordering::Relaxed) {
            0 => Duration::from_millis(50),
            ns => Duration::from_nanos(ns),
        }
    }

    /// Drops what the mixer is done with (here, not in the sound's thread).
    pub fn collect(&mut self) {
        self.remote.collect();
    }

    /// What the mixer played since the last call (with `record`).
    pub fn take_recs(&mut self) -> Vec<Rec> {
        let mut v = Vec::new();
        if let Some(r) = &mut self.recs {
            while let Ok(x) = r.pop() {
                v.push(x);
            }
        }
        v
    }
}

/// The output's thread: it owns the device's stream (or is the null one), and is told to
/// run or pause.
fn start(output: Output, mixer: Mixer, info: Arc<OutputInfo>) -> mpsc::Sender<Ctl> {
    let (tx, rx) = mpsc::channel();
    let spawned =
        std::thread::Builder::new()
            .name("sound-out".into())
            .spawn(move || match output {
                Output::Null {
                    rate,
                    period,
                    latency,
                    drift,
                } => null_output(rx, mixer, info, rate, period, latency, drift),
                Output::Device(id) => {
                    if let Err(e) = device_output(rx, mixer, info.clone(), id.as_deref()) {
                        info.failed.store(true, Ordering::Relaxed);
                        eprintln!("sound: {e}");
                    }
                }
            });
    if spawned.is_err() {
        // (no thread: the sound stays silent)
    }
    tx
}

/// A sound card that isn't there: buffers pulled on time, by an absolute schedule.
fn null_output(
    rx: mpsc::Receiver<Ctl>,
    mut mixer: Mixer,
    info: Arc<OutputInfo>,
    rate: u32,
    period: u32,
    latency: Duration,
    drift: f64,
) {
    info.rate.store(rate as u64, Ordering::Relaxed);
    info.latency_ns
        .store(latency.as_nanos() as u64, Ordering::Relaxed);
    let mut buf = vec![0f32; period as usize * 2];
    let per = Duration::from_secs_f64(period as f64 / rate as f64 / (1.0 + drift));
    'outer: loop {
        // paused: wait to be told to run
        loop {
            match rx.recv() {
                Ok(Ctl::Run) => break,
                Ok(Ctl::Pause | Ctl::Prepare) => {}
                Err(_) => break 'outer,
            }
        }
        let t0 = Instant::now();
        let mut n: u32 = 0;
        loop {
            match rx.try_recv() {
                Ok(Ctl::Pause) => break,
                Ok(Ctl::Run | Ctl::Prepare) => {}
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => break 'outer,
            }
            let due = t0 + per * n;
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            mixer.render(&mut buf, 2, rate, due + latency);
            n += 1;
        }
    }
}

/// When a buffer is heard, on this process's clock, from cpal's stream timestamps.
///
/// cpal's `callback` time isn't "now": on ALSA it's when the device last reported its
/// position (the last period's interrupt; with PipeWire's ALSA plugin, the graph's last
/// cycle, up to a whole quantum ago: 21 ms at 1024), and `playback` is that plus the
/// samples queued then. So both are mapped to [`Instant`] by the stream clock's offset from
/// it: the smallest `now − callback` seen (a callback that ran right after the report), rising
/// at most 1 µs a callback (the two clocks may drift apart a little).
#[derive(Debug, Default)]
pub struct StreamClock {
    base: Option<Instant>,
    /// `Instant` − stream time, seconds
    off: f64,
}

impl StreamClock {
    /// When the buffer of a callback run at `now` will be heard (`callback`, `playback`: the
    /// stream's times, ns).
    pub fn heard(&mut self, now: Instant, callback: u128, playback: u128) -> Instant {
        let base = *self.base.get_or_insert(now);
        let sample = now.saturating_duration_since(base).as_secs_f64() - callback as f64 / 1e9;
        if self.base == Some(now) || sample < self.off {
            self.off = sample;
        } else {
            self.off += (sample - self.off).min(1e-6);
        }
        let t = playback as f64 / 1e9 + self.off;
        if t >= 0.0 {
            base + Duration::from_secs_f64(t)
        } else {
            base.checked_sub(Duration::from_secs_f64(-t))
                .unwrap_or(base)
        }
    }
}

/// The system's output through cpal: 48 kHz stereo floats where the device takes them,
/// 10 ms buffers where it lets us choose. Paused while there's nothing to play; a device that
/// can't pause (ALSA's `null`) is closed instead, and opened again to play.
fn device_output(
    rx: mpsc::Receiver<Ctl>,
    mixer: Mixer,
    info: Arc<OutputInfo>,
    id: Option<&str>,
) -> Result<(), String> {
    use cpal::traits::StreamTrait;
    // (only the stream's callback locks it, so it never waits: a stream closed and opened
    // again hands it to the new one)
    let mixer = Arc::new(std::sync::Mutex::new(mixer));
    let mut stream: Option<cpal::Stream> = None;
    while let Ok(c) = rx.recv() {
        match c {
            Ctl::Prepare => {
                if stream.is_none() {
                    stream = open_device(&mixer, &info, id).ok();
                }
            }
            Ctl::Run => {
                if stream.is_none() {
                    stream = Some(open_device(&mixer, &info, id)?);
                }
                if let Some(s) = &stream
                    && let Err(e) = s.play()
                {
                    info.failed.store(true, Ordering::Relaxed);
                    eprintln!("sound: {e}");
                    stream = None;
                }
            }
            Ctl::Pause => {
                if let Some(s) = &stream
                    && s.pause().is_err()
                {
                    // (it can't pause: closed, and opened again, ready for the next play)
                    stream = open_device(&mixer, &info, id).ok();
                }
            }
        }
    }
    Ok(())
}

/// Opens the device's stream (not playing yet).
fn open_device(
    mixer: &Arc<std::sync::Mutex<Mixer>>,
    info: &Arc<OutputInfo>,
    id: Option<&str>,
) -> Result<cpal::Stream, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let device = match id {
        Some(id) => {
            let id: cpal::DeviceId = id.parse().map_err(|e| format!("{e}"))?;
            host.device_by_id(&id)
                .ok_or_else(|| format!("no sound device {id}"))?
        }
        None => host
            .default_output_device()
            .ok_or("no sound output on this system")?,
    };
    // stereo floats at 48 kHz if it takes them, else what it prefers
    let wanted = device.supported_output_configs().ok().and_then(|mut c| {
        c.find(|r| {
            r.channels() == 2
                && r.sample_format() == cpal::SampleFormat::F32
                && (r.min_sample_rate()..=r.max_sample_rate()).contains(&RATE)
        })
        .map(|r| r.with_sample_rate(RATE))
    });
    let supported = match wanted {
        Some(c) => c,
        None => device.default_output_config().map_err(|e| e.to_string())?,
    };
    let format = supported.sample_format();
    // (the device's own buffer size: a fixed one smaller than PipeWire's quantum, 1024 by
    // default, underruns every cycle or two)
    let config: cpal::StreamConfig = supported.config();
    let rate = config.sample_rate;
    let ch = config.channels as usize;
    info.rate.store(rate as u64, Ordering::Relaxed);
    let latency = info.clone();
    let failed = info.clone();
    let on_err = move |e: cpal::Error| {
        eprintln!("sound: {e}");
        failed.failed.store(true, Ordering::Relaxed);
    };
    let mut clock = StreamClock::default();
    let mut heard = move |cb: &cpal::OutputCallbackInfo| {
        let ts = cb.timestamp();
        let now = Instant::now();
        let at = clock.heard(now, ts.callback.as_nanos(), ts.playback.as_nanos());
        latency.latency_ns.store(
            at.saturating_duration_since(now).as_nanos() as u64,
            Ordering::Relaxed,
        );
        at
    };
    let mx = mixer.clone();
    let stream = match format {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                config,
                move |data: &mut [f32], cb: &cpal::OutputCallbackInfo| match mx.try_lock() {
                    Ok(mut m) => m.render(data, ch, rate, heard(cb)),
                    Err(_) => data.fill(0.0),
                },
                on_err,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => {
            // (converted from a buffer made here, once)
            let mut scratch = vec![0f32; 8192 * ch];
            device
                .build_output_stream(
                    config,
                    move |data: &mut [i16], cb: &cpal::OutputCallbackInfo| {
                        let t = heard(cb);
                        let Ok(mut m) = mx.try_lock() else {
                            data.fill(0);
                            return;
                        };
                        let mut done = 0;
                        while done < data.len() {
                            let n = (data.len() - done).min(scratch.len());
                            let n = n - n % ch;
                            if n == 0 {
                                break;
                            }
                            let at = t + Duration::from_secs_f64(
                                (done / ch) as f64 / rate.max(1) as f64,
                            );
                            m.render(&mut scratch[..n], ch, rate, at);
                            for (d, s) in data[done..done + n].iter_mut().zip(&scratch[..n]) {
                                *d = crate::mix::to_i16(*s);
                            }
                            done += n;
                        }
                    },
                    on_err,
                    None,
                )
                .map_err(|e| e.to_string())?
        }
        f => return Err(format!("the sound device wants {f} samples")),
    };
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize) -> Arc<Pcm> {
        // each sample its own value: where the mixer is in it shows
        Arc::new(Pcm {
            left: (0..n).map(|k| (k % 1000) as f32 / 1000.0).collect(),
            right: None,
        })
    }

    #[test]
    fn the_clock_maps_play_time_to_frames_like_the_picture() {
        let t0 = Instant::now();
        let c = Clock {
            t0,
            from: 2,
            fps: 24,
            range: (0, 9),
            mode: Mode::Loop,
            cut_start: 100,
            shift: 0.0,
        };
        assert_eq!(c.cut_frame(-0.01), None);
        assert_eq!(c.cut_frame(0.0), Some(2.0));
        assert!((c.cut_frame(8.5 / 24.0).unwrap() - 0.5).abs() < 1e-9);
        // scene frame 102 at 48 kHz
        assert_eq!(c.scene_sample(0.0), Some(102.0 * 2000.0));
        let once = Clock {
            mode: Mode::Once,
            ..c
        };
        assert_eq!(once.cut_frame(8.0 / 24.0), None);
        let pp = Clock {
            mode: Mode::PingPong,
            from: 0,
            ..c
        };
        // forwards 0–9 (and frame 9's own time), back 8–1: silent
        assert!(pp.cut_frame(9.5 / 24.0).is_some());
        assert_eq!(pp.cut_frame(11.0 / 24.0), None);
        assert!((pp.cut_frame(18.5 / 24.0).unwrap() - 0.5).abs() < 1e-9);
    }

    /// Drives a mixer as a sound card would, on simulated time: `drift` (the card's clock
    /// against the picture's), callback jitter, a minute of play. The sample heard at each
    /// buffer's start is compared with the one the clock says: the most it's off, after the
    /// first buffer, in ms.
    fn simulate(drift: f64, jitter_ms: f64, period: usize) -> (f64, f64) {
        let (mut remote, mut mixer, _) = channel(false);
        let base = Instant::now();
        let clock = Clock {
            t0: base + Duration::from_millis(30),
            from: 0,
            fps: 24,
            range: (0, 24 * 120 - 1),
            mode: Mode::Once,
            cut_start: 0,
            shift: 0.0,
        };
        // a slow ramp: each sample's value says which it is
        let total = 48_000 * 70;
        let ramp = Arc::new(Pcm {
            left: (0..total).map(|k| k as f32 / total as f32).collect(),
            right: None,
        });
        remote.send(Cmd::Play {
            voices: vec![Voice {
                pcm: ramp,
                start: 0,
                gain: 1.0,
            }],
            clock,
            start: None,
        });
        let rate = 48_000.0;
        let mut buf = vec![0f32; period * 2];
        let (mut worst, mut last) = (0f64, 0f64);
        let mut seed = 12345u64;
        let n = (62.0 * rate / period as f64) as usize;
        for k in 0..n {
            // the card's position, in its own time, turned into the picture's
            let true_t = k as f64 * period as f64 / rate / (1.0 + drift);
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = ((seed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 2.0 * jitter_ms / 1e3;
            let heard = base + Duration::from_secs_f64(true_t + 0.05 + j.max(-0.04));
            mixer.render(&mut buf, 2, rate as u32, heard);
            // what it played at the buffer's start vs what the picture shows then
            let t = true_t + 0.05 - 0.03;
            if t > 0.5 {
                let want = t * 48_000.0;
                let got = buf[0] as f64 * total as f64;
                let ms = (got - want) / 48.0;
                worst = worst.max(ms.abs());
                if t > 59.0 {
                    last = last.max(ms.abs());
                }
            }
        }
        (worst, last)
    }

    #[test]
    fn it_stays_in_step_with_the_picture_for_a_minute() {
        // a perfect card: exact
        let (w, _) = simulate(0.0, 0.0, 480);
        // (to the ramp's float precision: a sample is 0.02 ms)
        assert!(w < 0.05, "{w} ms");
        // a card 200 ppm fast or slow, its callbacks 1 ms late or early: within a millisecond,
        // and no drift at the minute's end
        for drift in [2e-4, -2e-4] {
            let (w, last) = simulate(drift, 1.0, 480);
            assert!(w < 1.0, "drift {drift}: {w} ms");
            assert!(last < 0.5, "drift {drift}: {last} ms at the end");
        }
        // big buffers too
        let (w, _) = simulate(1e-4, 0.5, 2048);
        assert!(w < 1.0, "{w} ms");
    }

    /// A device that reports its position once a quantum (1024 samples), its callbacks (480
    /// samples) coming any time after: the time each buffer is heard comes out the same
    /// whenever the callback ran.
    #[test]
    fn stream_times_map_to_when_its_heard() {
        let mut c = StreamClock::default();
        let base = Instant::now();
        let q = 1024.0 / 48_000.0;
        let lat = 0.030;
        let mut seed = 7u64;
        let mut worst = 0f64;
        for k in 0..2000u64 {
            // buffer k's first sample is truly heard at this time (from the stream's start)
            let heard = k as f64 * 480.0 / 48_000.0 + lat;
            // the callback runs a little before it's needed; the device last reported at the
            // quantum boundary before that, with this much queued
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let run = heard - lat - (seed >> 40) as f64 / (1u64 << 24) as f64 * 0.004;
            let report = (run / q).floor() * q;
            let queued = heard - report;
            let now = base + Duration::from_secs_f64(run.max(0.0) + 1.0);
            let cb = ((report + 5.0) * 1e9) as u128;
            let pb = ((report + 5.0 + queued) * 1e9) as u128;
            let got = c.heard(now, cb, pb);
            let want = base + Duration::from_secs_f64(heard + 1.0);
            if k > 50 {
                worst = worst.max(secs(want, got).abs());
            }
        }
        assert!(worst < 0.0005, "{} ms", worst * 1e3);
    }

    #[test]
    fn snippets_fade_and_stop() {
        let (mut remote, mut mixer, _) = channel(false);
        let pcm = Arc::new(Pcm {
            left: vec![0.5; 48_000],
            right: None,
        });
        remote.send(Cmd::Snippet {
            voices: vec![Voice {
                pcm,
                start: 0,
                gain: 1.0,
            }],
            at: 1000,
            len: 4000,
        });
        let mut buf = vec![0f32; 6000 * 2];
        mixer.render(&mut buf, 2, 48_000, Instant::now());
        assert_eq!(buf[0], 0.0);
        assert!((buf[120 * 2] - 0.25).abs() < 1e-3, "{}", buf[240]);
        assert!((buf[1000 * 2] - 0.5).abs() < 1e-6);
        assert!(buf[3990 * 2] < 0.02);
        assert!(buf[4000 * 2..].iter().all(|v| *v == 0.0));
        // stop: silence
        remote.send(Cmd::Stop);
        mixer.render(&mut buf, 2, 48_000, Instant::now());
        assert!(buf.iter().all(|v| *v == 0.0));
        // what the mixer let go of comes back to be dropped here
        remote.collect();
    }

    #[test]
    fn the_null_output_plays_on_its_own_time() {
        let mut p = Player::new(Output::null(), true);
        let t0 = Instant::now() + Duration::from_millis(40);
        p.send(Cmd::Play {
            voices: vec![Voice {
                pcm: tone(48_000),
                start: 0,
                gain: 1.0,
            }],
            clock: Clock {
                t0,
                from: 0,
                fps: 24,
                range: (0, 23),
                mode: Mode::Once,
                cut_start: 0,
                shift: 0.0,
            },
            start: None,
        });
        std::thread::sleep(Duration::from_millis(300));
        p.run(false);
        let recs = p.take_recs();
        assert!(recs.len() > 20, "{}", recs.len());
        // each buffer's first sample: the clock's sample when it's heard
        for r in recs.iter().filter(|r| !r.pos.is_nan()) {
            let want = secs(t0, r.heard_at) * 48_000.0;
            assert!((r.pos - want).abs() < 1.0, "{} vs {want}", r.pos);
        }
        assert!(recs.iter().any(|r| r.peak > 0.0));
    }

    /// A real output device, the ALSA `null` one only (it makes no sound, anywhere):
    /// `cargo test -p pencilpilot-sound -- --ignored alsa_null`.
    #[test]
    #[ignore = "opens the ALSA null device"]
    fn alsa_null_device_plays() {
        let mut p = Player::new(Output::Device(Some("alsa:null".into())), true);
        let t0 = Instant::now() + Duration::from_millis(50);
        p.send(Cmd::Play {
            voices: vec![Voice {
                pcm: tone(48_000),
                start: 0,
                gain: 1.0,
            }],
            clock: Clock {
                t0,
                from: 0,
                fps: 24,
                range: (0, 23),
                mode: Mode::Once,
                cut_start: 0,
                shift: 0.0,
            },
            start: None,
        });
        std::thread::sleep(Duration::from_millis(500));
        p.run(false);
        let recs = p.take_recs();
        assert!(
            !p.info.failed.load(Ordering::Relaxed),
            "the null device failed"
        );
        assert!(!recs.is_empty(), "no callbacks");
        eprintln!(
            "alsa null: {} callbacks, rate {}, {} frames each, latency {:.1} ms",
            recs.len(),
            p.info.rate.load(Ordering::Relaxed),
            recs[0].frames,
            p.info.latency_ns.load(Ordering::Relaxed) as f64 / 1e6
        );
    }
}
