//! Exporting the cut: video (MP4, WebM, through FFmpeg), GIF, a PNG sequence; the export
//! dialog's settings and its progress.
//!
//! - this file: what the dialog asks for, the frames to write (the plan), and feeding the
//!   export from the window's painter;
//! - [`worker`]: the export itself, off the window thread: a painter of its own renders
//!   each frame, another thread writes them (`crates/export`: FFmpeg, GIF, PNG).
//!
//! The window's painter stays on the window thread and is never locked: each drawing an
//! export needs is copied out of it (a memory copy of its tiles,
//! [`brush::Painter::drawing_copy`]) just before the first frame that shows it, a few
//! frames ahead of the renderer, and the renderer lets it go after the last frame that
//! shows it. So the export holds only the drawings in use around the frame it's on, and the
//! window pays a copy per drawing, spread over its ticks.

mod resize;
#[cfg(test)]
mod tests;
pub mod worker;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub use pencilpilot_export::Format;

use super::Engine;
use crate::shell::dialog;
use crate::state::{Action, State};

/// The dialog's buttons, as [`Action::Other`].
pub mod ops {
    pub const START: &str = "Export: start";
    pub const CANCEL: &str = "Export: cancel";
    pub const CLOSE: &str = "Export: close";
    pub const SHOW: &str = "Export: show in folder";
    /// back to the settings (after a failure)
    pub const BACK: &str = "Export: back";
}

/// The formats offered, in the dialog's order.
pub const FORMATS: [Format; 4] = [Format::Mp4, Format::Webm, Format::Gif, Format::Png];

/// The sizes offered: the document's, half of it, or a height (the width follows the
/// document's shape).
pub const SIZES: [(&str, u32); 5] = [
    ("Document", 0),
    ("Half", 1),
    ("720p", 720),
    ("1080p", 1080),
    ("4K", 2160),
];

/// Plan frames the window copies drawings for ahead of the renderer.
const AHEAD: usize = 3;
/// Tiles copied at once (256 kB): a big drawing is copied over many ticks.
const COPY_TILES: usize = 8;
/// The most a tick spends copying drawings for the export (the window draws on).
const TICK_BUDGET: Duration = Duration::from_micros(1500);
/// How often the window looks at an export's progress.
const POLL: Duration = Duration::from_millis(8);
/// How often the dialog's progress moves: each change redraws the whole interface (with a
/// long timeline, more than a display frame), so a few times a second, not every frame.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Where the export stands, for the dialog.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Status {
    /// choosing the settings
    #[default]
    Idle,
    /// the file dialog is up
    Asking,
    /// writing: output frames done and in all
    Running {
        done: u32,
        total: u32,
    },
    /// written there (a PNG sequence: its first file)
    Done(PathBuf),
    Failed(String),
}

/// What the interface shows (in [`State::export`]): the dialog's settings, progress.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportUi {
    /// the dialog is up
    pub open: bool,
    pub format: Format,
    /// the video formats FFmpeg can write: None while it's being looked for, empty when
    /// there's no FFmpeg
    pub videos: Option<Vec<Format>>,
    /// the loop range rather than the whole cut
    pub loop_only: bool,
    /// in [`SIZES`]
    pub size: usize,
    /// PNG and GIF: keep the transparency (no white behind)
    pub transparent: bool,
    pub status: Status,
    /// (the dialog's own: it was up in the last pass)
    pub was_open: bool,
}

impl Default for ExportUi {
    fn default() -> Self {
        ExportUi {
            open: false,
            format: Format::Mp4,
            videos: None,
            loop_only: false,
            size: 0,
            transparent: false,
            status: Status::Idle,
            was_open: false,
        }
    }
}

impl ExportUi {
    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Asking | Status::Running { .. })
    }

    /// Whether this format can be written here (videos need FFmpeg with its encoder).
    pub fn available(&self, f: Format) -> bool {
        match f {
            Format::Mp4 | Format::Webm | Format::Prores => {
                self.videos.as_ref().is_some_and(|v| v.contains(&f))
            }
            _ => true,
        }
    }

    pub fn can_be_transparent(&self) -> bool {
        matches!(self.format, Format::Png | Format::Gif)
    }
}

/// A format's name in the dialog.
pub fn format_name(f: Format) -> &'static str {
    match f {
        Format::Mp4 => "MP4",
        Format::Webm => "WebM",
        Format::Gif => "GIF",
        Format::Png => "PNG",
        Format::Apng => "APNG",
        Format::Prores => "ProRes",
    }
}

fn is_video(f: Format) -> bool {
    matches!(f, Format::Mp4 | Format::Webm | Format::Prores)
}

/// The output size for a [`SIZES`] choice of a `w` × `h` document (videos: even sides,
/// as their colour format needs).
pub fn out_size(choice: usize, w: u32, h: u32, video: bool) -> (u32, u32) {
    let (ow, oh) = match SIZES.get(choice).map_or(0, |s| s.1) {
        0 => (w, h),
        1 => (w.div_ceil(2), h.div_ceil(2)),
        tall => {
            let ow = (w as f64 * tall as f64 / h.max(1) as f64).round() as u32;
            (ow, tall)
        }
    };
    let even = |v: u32| if video { (v + 1) & !1 } else { v };
    (even(ow.max(1)), even(oh.max(1)))
}

/// The frames an export goes over (first, last; 0-based): the loop range, or the cut.
pub fn range(s: &State, loop_only: bool) -> (u32, u32) {
    let last = s.frames.saturating_sub(1);
    match s.sheet.loop_range {
        Some((a, b)) if loop_only && a.min(last) <= b.min(last) => (a.min(last), b.min(last)),
        _ => (0, last),
    }
}

/// One frame to write: each layer's drawing (painter layer id, drawing id; flattened, as
/// [`brush::Painter::render_frame`] takes them), held for `repeat` output frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanFrame {
    pub pairs: Vec<u32>,
    pub repeat: u32,
}

/// The frames an export writes (the same frames following each other made one held
/// frame), and each drawing's first and last plan frame.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub frames: Vec<PlanFrame>,
    pub spans: HashMap<u32, (usize, usize)>,
}

impl Plan {
    pub fn new(s: &State, first: u32, last: u32) -> Plan {
        // each track's drawing on each frame, from its cels in one pass
        let n = (last + 1).saturating_sub(first) as usize;
        let tracks: Vec<(u32, Vec<u32>)> = s
            .sheet
            .tracks
            .iter()
            .filter(|t| t.layer != 0)
            .map(|t| {
                let mut v = vec![0u32; n];
                for (at, c) in t.spans() {
                    let a = at.max(first);
                    let b = (at + c.hold).min(last + 1);
                    for f in a..b {
                        v[(f - first) as usize] = c.drawing;
                    }
                }
                (t.layer, v)
            })
            .collect();
        let mut frames: Vec<PlanFrame> = Vec::new();
        for f in 0..n {
            let pairs: Vec<u32> = tracks.iter().flat_map(|(l, v)| [*l, v[f]]).collect();
            match frames.last_mut() {
                Some(p) if p.pairs == pairs => p.repeat += 1,
                _ => frames.push(PlanFrame { pairs, repeat: 1 }),
            }
        }
        let mut spans: HashMap<u32, (usize, usize)> = HashMap::new();
        for (i, p) in frames.iter().enumerate() {
            for d in p.pairs.chunks(2).map(|c| c[1]).filter(|d| *d != 0) {
                spans.entry(d).and_modify(|s| s.1 = i).or_insert((i, i));
            }
        }
        Plan { frames, spans }
    }

    /// Output frames in all.
    pub fn total(&self) -> u32 {
        self.frames.iter().map(|f| f.repeat).sum()
    }
}

/// What the window still has to send the renderer, one at a time.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    /// a drawing's pixels (to merge into its layer's static run: the layer)
    Copy(u32, Option<u32>),
    /// a plan frame
    Frame(usize),
}

/// A drawing being copied a part at a time: its tiles, how far it's got, the copy so far,
/// and the painter's history when it started (changed: it starts again).
struct Partial {
    keys: Vec<(i32, i32)>,
    at: usize,
    copy: brush::DrawingCopy,
    edits: u64,
}

/// An export being written.
struct Run {
    tx: mpsc::Sender<worker::Msg>,
    plan: Plan,
    /// plan frames queued, and what's queued but not sent yet (the first: perhaps a part
    /// copied)
    queued: usize,
    pending: VecDeque<Item>,
    partial: Option<Partial>,
    /// tiles' memory to copy into (the renderer's, see [`worker::Shared::spare`])
    spare: brush::TileBuffers,
    /// the layers merged into static runs, bottom first
    runs: Vec<Vec<u32>>,
    shared: Arc<worker::Shared>,
    done: mpsc::Receiver<worker::Outcome>,
}

/// An export in progress, between calls.
#[derive(Default)]
pub struct Exporter {
    /// FFmpeg being looked for (and what it can write)
    looking: Option<mpsc::Receiver<Vec<Format>>>,
    /// the file dialog asked
    asked: Option<dialog::Asked>,
    run: Option<Run>,
    /// the last export's outcome and measurements (for checks)
    pub last: Option<worker::Outcome>,
    /// the window's time spent copying drawings for the export: the longest tick, in all
    pub feed_max: Duration,
    pub feed_total: Duration,
    /// each tick's copying of the last export (for checks)
    pub feed_ticks: Vec<Duration>,
    /// the longest single step of it, and what it was; the longest whole tick, and start
    pub step_max: (Duration, String),
    pub tick_max: Duration,
    pub start_took: Duration,
    /// when the dialog's progress last moved
    shown_at: Option<Instant>,
}

impl std::fmt::Debug for Exporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exporter")
            .field("running", &self.run.is_some())
            .finish()
    }
}

/// The video formats FFmpeg can write here (none without FFmpeg), looked for on another
/// thread (running it takes a while).
fn look_for_ffmpeg() -> mpsc::Receiver<Vec<Format>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let v = pencilpilot_export::find_ffmpeg()
            .map(|p| pencilpilot_export::video_formats(&p))
            .unwrap_or_default();
        let _ = tx.send(v);
    });
    rx
}

/// An export action (Export…, the dialog's buttons). Returns whether it was one and the
/// picture may have changed.
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    match a {
        Action::File(n) if n == "Export…" => {
            s.export.open = true;
            if !s.export.busy() {
                s.export.status = Status::Idle;
            }
            if s.export.videos.is_none() && e.exporter.looking.is_none() {
                e.exporter.looking = Some(look_for_ffmpeg());
            }
            true
        }
        Action::Other(n) if n == ops::START => {
            if !s.export.busy() && !s.export.available(s.export.format) {
                // (said in the dialog, rather than nothing happening)
                let f = format_name(s.export.format);
                s.export.status = Status::Failed(match &s.export.videos {
                    None => format!("Still looking for FFmpeg, which {f} needs: a moment."),
                    Some(v) if v.is_empty() => format!(
                        "{f} needs FFmpeg, which isn't installed. Install it (or set \
                         PENCILPILOT_FFMPEG to it), or export a GIF or PNGs."
                    ),
                    Some(_) => format!("This FFmpeg can't write {f}."),
                });
            } else if !s.export.busy() {
                let f = s.export.format;
                let name = format!("{}.{}", file_stem(&s.cut_name), f.extension());
                let label = match f {
                    Format::Png => "PNG images".to_string(),
                    _ => format!("{} file", format_name(f)),
                };
                e.exporter.asked = Some(dialog::ask(dialog::Ask {
                    title: "Export".into(),
                    save: true,
                    name: Some(name),
                    filters: vec![(label, vec![format!("*.{}", f.extension())])],
                }));
                s.export.status = Status::Asking;
            }
            true
        }
        Action::Other(n) if n == ops::CANCEL => {
            e.exporter.asked = None;
            if let Some(r) = &e.exporter.run {
                r.shared.cancel.store(true, Ordering::Relaxed);
                let _ = r.tx.send(worker::Msg::End);
            }
            if s.export.status == Status::Asking {
                s.export.status = Status::Idle;
            }
            true
        }
        Action::Other(n) if n == ops::CLOSE => {
            if !s.export.busy() {
                s.export.open = false;
                s.export.status = Status::Idle;
            }
            true
        }
        Action::Other(n) if n == ops::BACK => {
            if !s.export.busy() {
                s.export.status = Status::Idle;
            }
            true
        }
        Action::Other(n) if n == ops::SHOW => {
            if let Status::Done(p) = &s.export.status {
                show_in_folder(p);
            }
            true
        }
        _ => false,
    }
}

/// A file name from the cut's.
fn file_stem(cut: &str) -> String {
    let s: String = cut
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    if s.trim().is_empty() {
        "Export".into()
    } else {
        s.trim().to_string()
    }
}

/// Opens the folder a file is in, in the file manager (on the person's click).
fn show_in_folder(p: &std::path::Path) {
    let Some(dir) = p.parent() else { return };
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Ok(mut c) = std::process::Command::new(opener)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // (reaped once it's done)
        std::thread::spawn(move || c.wait());
    }
}

/// Starts writing to `path` what the dialog asks for.
fn start(e: &mut Engine, s: &mut State, path: PathBuf) {
    let x = &s.export;
    let (first, last) = range(s, x.loop_only);
    let plan = Plan::new(s, first, last);
    let (dw, dh) = (e.painter.width(), e.painter.height());
    let out = out_size(x.size, dw, dh, is_video(x.format));
    let layers: Vec<worker::LayerSetup> = e
        .painter
        .layers()
        .iter()
        .map(|l| worker::LayerSetup {
            id: l.id,
            props: l.props.clone(),
            mask: e.painter.mask_copy(l.id),
        })
        .collect();
    let runs = static_runs(&layers, &plan);
    let job = worker::Job {
        format: x.format,
        path,
        doc: (dw, dh),
        out,
        fps: s.fps.max(1),
        transparent: x.transparent && x.can_be_transparent(),
        frames: plan.frames.len() as u32,
        total: plan.total(),
        layers,
        runs: runs.clone(),
        // (x264 with frame threads takes 1.4 GB at 1080p, 2 GB at 4K; sliced: 0.45, 1.2)
        lean: true,
    };
    let total = job.total;
    let (tx, shared, done) = worker::spawn(job);
    s.export.status = Status::Running { done: 0, total };
    s.export_progress = Some((0, total));
    e.exporter.feed_max = Duration::ZERO;
    e.exporter.feed_total = Duration::ZERO;
    e.exporter.feed_ticks.clear();
    e.exporter.step_max = (Duration::ZERO, String::new());
    e.exporter.tick_max = Duration::ZERO;
    e.exporter.run = Some(Run {
        tx,
        plan,
        queued: 0,
        pending: VecDeque::new(),
        partial: None,
        spare: brush::TileBuffers::default(),
        runs,
        shared,
        done,
    });
}

/// Layers next to each other that show the same drawing all through the export, in normal
/// blending with no mask, style or clipping (a background, still line work): merged into
/// one before the first frame, so the renderer holds and composites one drawing for them.
/// Runs of two or more, bottom first, by layer id.
fn static_runs(layers: &[worker::LayerSetup], plan: &Plan) -> Vec<Vec<u32>> {
    let Some(first) = plan.frames.first() else {
        return Vec::new();
    };
    if layers.iter().any(|l| l.props.clip) {
        return Vec::new();
    }
    let shows = |f: &PlanFrame, id: u32| f.pairs.chunks(2).find(|c| c[0] == id).map(|c| c[1]);
    let mut runs = Vec::new();
    let mut run: Vec<u32> = Vec::new();
    for l in layers {
        let p = &l.props;
        let d = shows(first, l.id);
        let still = d.is_some() && plan.frames.iter().all(|f| shows(f, l.id) == d);
        if still
            && p.blend == brush::Blend::Normal
            && p.adjust.is_none()
            && !p.style.any()
            && l.mask.is_none()
        {
            run.push(l.id);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    runs.push(run);
    runs.retain(|r| r.len() >= 2);
    runs
}

/// Copies the drawings the next plan frames show and sends those frames, a few ahead of
/// the renderer, within the tick's budget (a drawing at a time: the copy of one 4K
/// drawing is the most a tick spends past it).
fn feed(e: &mut Engine) {
    // (not while a stroke or a move is half done: the drawing isn't what it'll be)
    if e.is_stroking() || e.float.is_some() || e.painter.transforming() {
        return;
    }
    let t0 = Instant::now();
    let Some(r) = &mut e.exporter.run else { return };
    let rendered = r.shared.rendered.load(Ordering::Relaxed) as usize;
    // (never waits: the renderer holds it only to put more in)
    if let Ok(mut g) = r.shared.spare.try_lock() {
        r.spare.append(std::mem::take(&mut *g));
    }
    while t0.elapsed() < TICK_BUDGET {
        if r.pending.is_empty() {
            if r.queued >= r.plan.frames.len() || r.queued >= rendered + AHEAD {
                break;
            }
            let i = r.queued;
            if i == 0 {
                // the static runs first, bottom up
                for l in r.runs.iter().flatten() {
                    let pairs = &r.plan.frames[0].pairs;
                    if let Some(c) = pairs.chunks(2).find(|c| c[0] == *l) {
                        r.pending.push_back(Item::Copy(c[1], Some(*l)));
                    }
                }
            }
            for c in r.plan.frames[i].pairs.chunks(2) {
                let merged = r.runs.iter().flatten().any(|l| *l == c[0]);
                if c[1] != 0 && !merged && r.plan.spans[&c[1]].0 == i {
                    r.pending.push_back(Item::Copy(c[1], None));
                }
            }
            r.pending.push_back(Item::Frame(i));
            r.queued += 1;
        }
        let Some(&item) = r.pending.front() else {
            break;
        };
        let ts = Instant::now();
        let msg = match item {
            Item::Copy(d, merge) => {
                let p = &e.painter;
                let part = r.partial.get_or_insert_with(|| Partial {
                    keys: p.drawing_tile_keys(d),
                    at: 0,
                    copy: brush::DrawingCopy::empty(),
                    edits: p.edits(),
                });
                if part.edits != p.edits() {
                    // (painted meanwhile: again from the start)
                    *part = Partial {
                        keys: p.drawing_tile_keys(d),
                        at: 0,
                        copy: brush::DrawingCopy::empty(),
                        edits: p.edits(),
                    };
                }
                // only into the renderer's memory: the window allocates none (with the
                // export allocating on every core, a fresh page can take milliseconds)
                let n = COPY_TILES.min(r.spare.len());
                if n == 0 && part.at < part.keys.len() {
                    break;
                }
                let to = (part.at + n).min(part.keys.len());
                let had = r.spare.len();
                part.copy
                    .absorb(p.drawing_copy_tiles(d, &part.keys[part.at..to], &mut r.spare));
                r.shared
                    .used
                    .fetch_add(had - r.spare.len(), Ordering::Relaxed);
                part.at = to;
                if part.at < part.keys.len() {
                    let took = ts.elapsed();
                    if took > e.exporter.step_max.0 {
                        e.exporter.step_max = (took, format!("part of {item:?}"));
                    }
                    continue;
                }
                let copy = r.partial.take().map(|p| p.copy).filter(|c| !c.is_empty());
                match merge {
                    Some(layer) => worker::Msg::Merge(layer, copy),
                    None => worker::Msg::Drawing(d, copy),
                }
            }
            Item::Frame(i) => {
                let f = &r.plan.frames[i];
                let forget = f
                    .pairs
                    .chunks(2)
                    .map(|c| c[1])
                    .filter(|d| *d != 0 && r.plan.spans[d].1 == i)
                    .collect();
                worker::Msg::Frame {
                    pairs: f.pairs.clone(),
                    repeat: f.repeat,
                    forget,
                }
            }
        };
        r.pending.pop_front();
        if r.tx.send(msg).is_err() {
            break;
        }
        let took = ts.elapsed();
        if took > e.exporter.step_max.0 {
            e.exporter.step_max = (took, format!("{item:?}"));
        }
        if item == Item::Frame(r.plan.frames.len() - 1) {
            let _ = r.tx.send(worker::Msg::End);
        }
    }
    let took = t0.elapsed();
    e.exporter.feed_max = e.exporter.feed_max.max(took);
    e.exporter.feed_total += took;
    e.exporter.feed_ticks.push(took);
}

/// Once a loop: an export's progress. Returns whether the interface should redraw.
pub fn tick(e: &mut Engine, s: &mut State, _now: Instant) -> bool {
    let t0 = Instant::now();
    let r = tick_(e, s);
    e.exporter.tick_max = e.exporter.tick_max.max(t0.elapsed());
    r
}

fn tick_(e: &mut Engine, s: &mut State) -> bool {
    let before = s.export.clone();
    let progress = s.export_progress;
    if let Some(rx) = &e.exporter.looking
        && let Ok(v) = rx.try_recv()
    {
        e.exporter.looking = None;
        // a video format FFmpeg can't write gives way to one it can
        if is_video(s.export.format) && !v.contains(&s.export.format) {
            s.export.format = v.first().copied().unwrap_or(Format::Png);
        }
        s.export.videos = Some(v);
    }
    if let Some(a) = &e.exporter.asked
        && let Some(answer) = a.answer()
    {
        e.exporter.asked = None;
        match answer {
            Some(p) => {
                let t = Instant::now();
                start(e, s, p);
                e.exporter.start_took = t.elapsed();
            }
            None => s.export.status = Status::Idle,
        }
    }
    feed(e);
    if let Some(r) = &e.exporter.run {
        let done = r.shared.written.load(Ordering::Relaxed);
        let due = e
            .exporter
            .shown_at
            .is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
        if let Status::Running { total, done: was } = s.export.status
            && done != was
            && due
        {
            s.export.status = Status::Running { done, total };
            s.export_progress = Some((done, total));
            e.exporter.shown_at = Some(Instant::now());
        }
        if let Ok(out) = r.done.try_recv() {
            s.export.status = match &out.result {
                Ok(p) => Status::Done(p.clone()),
                Err(m) if m.is_empty() => Status::Idle,
                Err(m) => Status::Failed(m.clone()),
            };
            s.export_progress = None;
            e.exporter.run = None;
            e.exporter.last = Some(out);
        }
    }
    s.export != before || s.export_progress != progress
}

/// When [`tick`] next has something to do.
pub fn next_tick_at(e: &Engine, _s: &State) -> Option<Instant> {
    let x = &e.exporter;
    (x.looking.is_some() || x.asked.is_some() || x.run.is_some()).then(|| Instant::now() + POLL)
}
