//! The export off the window thread: a renderer with its own painter (the layers as they
//! were when the export started, the drawings as the window copies them in) makes each
//! frame; a writer puts them in the file (`crates/export`: FFmpeg, GIF, PNG). The two
//! overlap: a frame renders while the one before it is being written.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brush::{DrawingCopy, LayerProps, Painter, TileBuffers};

use super::{Format, resize};

/// A layer as the export renders it.
pub struct LayerSetup {
    /// the window painter's id (the plan's pairs name it)
    pub id: u32,
    pub props: LayerProps,
    /// its mask, packed (empty: none)
    pub mask: Vec<u8>,
}

/// What to write.
pub struct Job {
    pub format: Format,
    pub path: PathBuf,
    /// the document's size, and the file's
    pub doc: (u32, u32),
    pub out: (u32, u32),
    pub fps: u32,
    pub transparent: bool,
    /// plan frames, and output frames in all
    pub frames: u32,
    pub total: u32,
    /// bottom first
    pub layers: Vec<LayerSetup>,
    /// layers merged into one before the first frame (each run bottom first: see
    /// `static_runs`); their drawings come as [`Msg::Merge`]
    pub runs: Vec<Vec<u32>>,
    /// MP4: x264 with less memory (see `pencilpilot_export::Spec::lean`)
    pub lean: bool,
}

/// From the window to the renderer, in order.
pub enum Msg {
    /// a drawing's pixels, before the first frame showing it (None: it's empty)
    Drawing(u32, Option<DrawingCopy>),
    /// a static run's layer's drawing (the layer's id), merged at once, bottom up
    Merge(u32, Option<DrawingCopy>),
    /// a frame: each layer's drawing (the window's layer ids), how long it's held, and the
    /// drawings no later frame shows
    Frame {
        pairs: Vec<u32>,
        repeat: u32,
        forget: Vec<u32>,
    },
    /// no more frames (or cancelled)
    End,
}

/// Counters the window reads.
pub struct Shared {
    /// plan frames rendered
    pub rendered: AtomicU32,
    /// output frames written
    pub written: AtomicU32,
    pub cancel: AtomicBool,
    /// tiles' memory for the window to copy drawings into (made here, so the window's
    /// copies cost no page faults), and how many tiles were offered and used in all
    pub spare: Mutex<TileBuffers>,
    pub offered: AtomicUsize,
    pub used: AtomicUsize,
}

/// The window's spare tiles: topped up below LOW, kept to at most HIGH (32 MB).
const SPARE_LOW: usize = 512;
const SPARE_HIGH: usize = 1024;

impl Shared {
    /// Tiles the window has to copy into.
    fn spare(&self) -> usize {
        self.offered
            .load(Ordering::Relaxed)
            .saturating_sub(self.used.load(Ordering::Relaxed))
    }

    /// Tiles' memory to the window, up to what it keeps.
    fn offer(&self, mut b: TileBuffers) {
        b.truncate(SPARE_HIGH.saturating_sub(self.spare()));
        if b.is_empty() {
            return;
        }
        self.offered.fetch_add(b.len(), Ordering::Relaxed);
        if let Ok(mut g) = self.spare.lock() {
            g.append(b);
        }
    }

    /// More, when the window is running low.
    fn top_up(&self) {
        let have = self.spare();
        if have < SPARE_LOW {
            self.offer(TileBuffers::allocate(SPARE_LOW - have));
        }
    }
}

/// How an export went.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// plan frames rendered, output frames written
    pub rendered: u32,
    pub written: u32,
    /// rendering a frame (the painter's, and resizing): in all and the longest
    pub render: Duration,
    pub render_max: Duration,
    /// handing a frame to the writer (FFmpeg's pipe, GIF or PNG encoding): in all, longest
    pub write: Duration,
    pub write_max: Duration,
    /// from start to the file finished
    pub wall: Duration,
    /// most drawing bytes the renderer held at once
    pub held_max: usize,
    /// merging the static runs, in all
    pub merge: Duration,
}

/// The static runs' merged drawings' ids (past any the window's painter gives out).
const MERGED: u32 = 1 << 30;

/// The end of an export: where it went (Err with no text: cancelled), and its numbers.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub result: Result<PathBuf, String>,
    pub stats: Stats,
}

/// From the renderer to the writer.
enum Out {
    Frame(Vec<u8>, u32),
    /// the renderer is done (its numbers; an error if it failed)
    Done(Stats, Option<String>),
}

/// Starts the export's threads. Frames go in through the sender; the outcome comes back
/// once.
pub fn spawn(job: Job) -> (mpsc::Sender<Msg>, Arc<Shared>, mpsc::Receiver<Outcome>) {
    let (tx, rx) = mpsc::channel::<Msg>();
    let (out_tx, out_rx) = mpsc::sync_channel::<Out>(1);
    let (done_tx, done_rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        rendered: AtomicU32::new(0),
        written: AtomicU32::new(0),
        cancel: AtomicBool::new(false),
        spare: Mutex::new(TileBuffers::default()),
        offered: AtomicUsize::new(0),
        used: AtomicUsize::new(0),
    });
    let started = Instant::now();
    // the shrink the painter can do itself (a whole fraction of the document), and the size
    // it renders at
    let (dw, dh) = job.doc;
    let (ow, oh) = job.out;
    let k = if ow > 0 && dw % ow == 0 && dh % oh.max(1) == 0 && dw / ow == dh / oh.max(1) {
        (dw / ow).max(1)
    } else {
        1
    };
    let rendered_size = (dw.div_ceil(k), dh.div_ceil(k));
    let video = matches!(job.format, Format::Mp4 | Format::Webm | Format::Prores);
    // videos are scaled by FFmpeg; the others here
    let resize_to = (!video && rendered_size != job.out).then_some(job.out);
    let spec = pencilpilot_export::Spec {
        format: job.format,
        path: job.path.to_string_lossy().into_owned(),
        width: ow,
        height: oh,
        input: (video && rendered_size != job.out).then_some(rendered_size),
        fps: job.fps,
        transparent: job.transparent,
        frames: job.frames,
        total: job.total,
        once: false,
        audio: None,
        quality: Default::default(),
        meta: vec![("Software".into(), "PencilPilot".into())],
        lean: job.lean,
        yuv: video,
        background: true,
    };

    let sh = shared.clone();
    let transparent = job.transparent;
    let (layers, runs) = (job.layers, job.runs);
    std::thread::Builder::new()
        .name("export-render".into())
        .spawn(move || {
            lower_priority();
            // its own threads to render on, half the cores: rayon's global ones stay free for
            // the window, and FFmpeg has room
            let n = std::thread::available_parallelism().map_or(4, |n| n.get());
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads((n / 2).max(2))
                .thread_name(|i| format!("export-{i}"))
                .start_handler(|_| lower_priority())
                .build()
                .ok();
            let (stats, err) = render(
                &rx,
                &out_tx,
                &sh,
                pool.as_ref(),
                (dw, dh),
                k,
                transparent,
                resize_to,
                video,
                layers,
                runs,
            );
            let _ = out_tx.send(Out::Done(stats, err));
        })
        .expect("a thread");

    let sh = shared.clone();
    std::thread::Builder::new()
        .name("export-write".into())
        .spawn(move || {
            lower_priority();
            let (result, mut stats) = write(spec, &out_rx, &sh);
            stats.wall = started.elapsed();
            let _ = done_tx.send(Outcome { result, stats });
        })
        .expect("a thread");
    (tx, shared, done_rx)
}

/// The renderer: a painter like the window's, its drawings as they come.
#[allow(clippy::too_many_arguments)]
fn render(
    rx: &mpsc::Receiver<Msg>,
    out: &mpsc::SyncSender<Out>,
    sh: &Shared,
    pool: Option<&rayon::ThreadPool>,
    (dw, dh): (u32, u32),
    k: u32,
    transparent: bool,
    resize_to: Option<(u32, u32)>,
    yuv: bool,
    layers: Vec<LayerSetup>,
    runs: Vec<Vec<u32>>,
) -> (Stats, Option<String>) {
    let mut st = Stats::default();
    let mut p = Painter::new(dw, dh);
    p.set_undo_limit(brush::UNDO_LIMIT_RANGE.start().to_owned());
    let mut ids: HashMap<u32, u32> = HashMap::new();
    let mut props: HashMap<u32, LayerProps> = HashMap::new();
    for (i, l) in layers.into_iter().enumerate() {
        let id = if i == 0 {
            p.layers()[0].id
        } else {
            p.add_layer(&l.props.name)
        };
        p.set_layer_props(id, l.props.clone(), false);
        if !l.mask.is_empty() {
            p.put_mask_tiles(id, &l.mask);
        }
        ids.insert(l.id, id);
        props.insert(l.id, l.props);
    }
    p.clear_history();
    // the static runs: each merged into its bottom layer's drawing as they come
    let mut merging: Vec<(DrawingCopy, usize)> =
        runs.iter().map(|_| (DrawingCopy::empty(), 0)).collect();
    let run_of = |layer: u32| runs.iter().position(|r| r.contains(&layer));
    let merged_id = |r: usize| MERGED + r as u32;
    let mut held: HashMap<u32, usize> = HashMap::new();
    let (rw, rh) = p.frame_size(k);
    loop {
        if sh.cancel.load(Ordering::Relaxed) {
            return (st, None);
        }
        sh.top_up();
        let msg = match rx.recv_timeout(Duration::from_millis(4)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            // the window let go of the export without ending it: it stops
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                sh.cancel.store(true, Ordering::Relaxed);
                return (st, None);
            }
        };
        match msg {
            Msg::Drawing(d, copy) => {
                if let Some(c) = copy {
                    held.insert(d, c.bytes());
                    p.put_drawing_copy(d, c);
                }
                st.held_max = st.held_max.max(held.values().sum());
            }
            Msg::Merge(layer, copy) => {
                let Some(r) = run_of(layer) else { continue };
                let t = Instant::now();
                if let (Some(c), Some(lp)) = (copy, props.get(&layer))
                    && lp.visible
                {
                    let acc = &mut merging[r].0;
                    sh.offer(on(pool, || acc.merge(c, lp.opacity, lp.blend)));
                }
                merging[r].1 += 1;
                if merging[r].1 == runs[r].len() {
                    // the run's done: its bottom layer shows it, the others nothing
                    let acc = std::mem::replace(&mut merging[r].0, DrawingCopy::empty());
                    held.insert(merged_id(r), acc.bytes());
                    p.put_drawing_copy(merged_id(r), acc);
                    for (k, l) in runs[r].iter().enumerate() {
                        let (Some(&id), Some(lp)) = (ids.get(l), props.get(l)) else {
                            continue;
                        };
                        let mut lp = lp.clone();
                        lp.visible = k == 0;
                        lp.opacity = 1.0;
                        p.set_layer_props(id, lp, false);
                    }
                }
                st.held_max = st
                    .held_max
                    .max(held.values().sum::<usize>() + merging[r].0.bytes());
                st.merge += t.elapsed();
            }
            Msg::Frame {
                pairs,
                repeat,
                forget,
            } => {
                let t = Instant::now();
                let pairs: Vec<u32> = pairs
                    .chunks(2)
                    .filter_map(|c| {
                        let l = *ids.get(&c[0])?;
                        match run_of(c[0]) {
                            None => Some([l, c[1]]),
                            Some(r) if runs[r][0] == c[0] => Some([l, merged_id(r)]),
                            // (hidden: its paint is in the run's bottom layer)
                            Some(_) => None,
                        }
                    })
                    .flatten()
                    .collect();
                let painter = &p;
                let mut px = on(pool, || {
                    if transparent {
                        painter.render_frame_alpha(&pairs, k, &[])
                    } else {
                        painter.render_frame(&pairs, k, &[])
                    }
                });
                if let Some((ow, oh)) = resize_to {
                    px = on(pool, || resize::resize(&px, rw, rh, ow, oh));
                }
                if yuv {
                    // (a third of the bytes through the pipe and in FFmpeg's queues)
                    px = on(pool, || to_yuv(&px, rw, rh));
                }
                for d in forget {
                    if let Some(c) = p.take_drawing_copy(d) {
                        sh.offer(c.into_buffers());
                    }
                    held.remove(&d);
                }
                let took = t.elapsed();
                st.render += took;
                st.render_max = st.render_max.max(took);
                st.rendered += 1;
                sh.rendered.store(st.rendered, Ordering::Relaxed);
                if out.send(Out::Frame(px, repeat)).is_err() {
                    return (st, None);
                }
            }
            Msg::End => return (st, None),
        }
    }
}

/// This thread runs below the window's (an export takes every core it's given; the window
/// must still get one at once whenever it has a frame to draw).
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain system call; on Linux it changes the calling thread only
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, pencilpilot_export::BACKGROUND_NICE);
    }
}

/// A frame as YUV 4:2:0 for FFmpeg, its row pairs on many threads.
fn to_yuv(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    use rayon::prelude::*;
    let (wu, hu) = (w as usize, h as usize);
    let cw = wu.div_ceil(2);
    let mut out = vec![0u8; pencilpilot_export::yuv420_len(w, h)];
    let (ys, uv) = out.split_at_mut(wu * hu);
    let (us, vs) = uv.split_at_mut(cw * hu.div_ceil(2));
    ys.par_chunks_mut(2 * wu)
        .zip(us.par_chunks_mut(cw))
        .zip(vs.par_chunks_mut(cw))
        .enumerate()
        .for_each(|(pair, ((y, u), v))| {
            pencilpilot_export::yuv420_row_pair(rgba, w, h, pair, y, u, v)
        });
    out
}

/// `f` on the export's threads (rayon's work inside it goes there).
fn on<T: Send>(pool: Option<&rayon::ThreadPool>, f: impl FnOnce() -> T + Send) -> T {
    match pool {
        Some(p) => p.install(f),
        None => f(),
    }
}

/// The writer: the file, from the frames as they're rendered.
fn write(
    spec: pencilpilot_export::Spec,
    rx: &mpsc::Receiver<Out>,
    sh: &Shared,
) -> (Result<PathBuf, String>, Stats) {
    let mut job = match pencilpilot_export::Job::begin(spec) {
        Ok(j) => j,
        Err(e) => {
            sh.cancel.store(true, Ordering::Relaxed);
            // (the renderer's numbers, once it's stopped)
            let stats = drain(rx);
            return (Err(e), stats);
        }
    };
    let mut write = Duration::ZERO;
    let mut write_max = Duration::ZERO;
    let mut written = 0;
    let finish = |mut s: Stats, write, write_max, written| {
        s.write = write;
        s.write_max = write_max;
        s.written = written;
        s
    };
    loop {
        let msg = rx.recv();
        if sh.cancel.load(Ordering::Relaxed) {
            job.cancel();
            let s = match msg {
                Ok(Out::Done(s, _)) => s,
                _ => drain(rx),
            };
            return (Err(String::new()), finish(s, write, write_max, written));
        }
        match msg {
            Ok(Out::Frame(px, repeat)) => {
                let t = Instant::now();
                if let Err(e) = job.frame(&px, repeat) {
                    sh.cancel.store(true, Ordering::Relaxed);
                    job.cancel();
                    let s = drain(rx);
                    return (Err(e), finish(s, write, write_max, written));
                }
                let took = t.elapsed();
                write += took;
                write_max = write_max.max(took);
                written += repeat;
                sh.written.store(written, Ordering::Relaxed);
            }
            Ok(Out::Done(s, err)) => {
                let s = finish(s, write, write_max, written);
                if let Some(e) = err {
                    job.cancel();
                    return (Err(e), s);
                }
                return (job.end().map(PathBuf::from), s);
            }
            Err(_) => {
                job.cancel();
                return (Err(String::new()), Stats::default());
            }
        }
    }
}

/// What's left from the renderer, until it says it's done: its numbers.
fn drain(rx: &mpsc::Receiver<Out>) -> Stats {
    while let Ok(m) = rx.recv() {
        if let Out::Done(s, _) = m {
            return s;
        }
    }
    Stats::default()
}
