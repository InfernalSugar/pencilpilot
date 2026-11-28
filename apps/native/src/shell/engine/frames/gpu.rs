//! The picture composited on the GPU ([`pencilpilot_gpu::Compositor`]): each layer's drawing
//! for the frame, its mask and the onion skin's drawings kept there as tiles, so going to
//! another frame mostly points the shader at other tiles instead of compositing on the CPU
//! and sending the whole picture up.
//!
//! - **What goes up:** a drawing once (all its tiles) when first shown; after that only
//!   tiles the painter says changed ([`brush::Painter::take_changes`]), each checked
//!   against what's there (a hash) so unchanged ones stay.
//! - **When a kept copy is out of date:** a drawing changed while not showing (undo on
//!   another frame, a new drawing in its place) is forgotten where the thumbnails and the
//!   onion skin learn of it ([`super::Frames::mark`]), and so is a drawing a layer stopped
//!   showing with changes not yet taken.
//! - **What stays on the CPU:** adjustment layers, layer styles, a mask being painted, more
//!   than 32 layers: the painter composites, as before ([`super::picture`]). Exports and
//!   PNGs are always the CPU's.

use pencilpilot_gpu::{Compositor, Gpu, GpuLayer, OnionLayer};

use super::{Engine, load_drawings, onion_entries, setup, show};
use crate::state::State;

/// The GPU's share of [`super::Frames`].
#[derive(Debug, Default)]
pub struct GpuFrames {
    /// the GPU makes the picture now
    pub on: bool,
    /// the painter's history count when last composited
    edits: u64,
    /// the document it's for (`State::doc_id`)
    doc: u32,
    /// each layer's source (and mask's) last composited
    last: Vec<(u64, Option<u64>)>,
    /// drawings that changed (thumbnails made again): forgotten before the next frame unless
    /// a layer shows them (then their changes come tile by tile)
    pub(super) forget: Vec<u64>,
    /// sources to forget before the next frame whatever shows (changes not taken)
    forget_now: Vec<u64>,
    /// tiles changed some other way since the last frame (`Changes::other`)
    other: Vec<(i32, i32)>,
    /// drawings going up ahead
    pub warm: Warm,
    /// the frame's (layer, drawing) pairs as the GPU shows them, when the painter doesn't
    /// show them yet ([`super::catch_up`]), and since when
    pub view: Option<Vec<u32>>,
    pub view_at: Option<std::time::Instant>,
    /// the painter is catching up (it shows the drawings, whatever the GPU has)
    pub catching_up: bool,
    /// the GPU couldn't make a frame: the CPU does until then
    cpu_until: Option<std::time::Instant>,
    /// when the pen or mouse last reached the canvas (catching up waits for a pause)
    pub input_at: Option<std::time::Instant>,
}

impl GpuFrames {
    /// A layer's mask changed from outside (drawing together): the GPU's copy goes up again.
    pub fn forget_mask(&mut self, layer: u32) {
        self.forget_now.push(mask_source(layer));
    }
}

/// How long the pen rests before the painter's own picture catches up (it takes the
/// window's thread a few milliseconds a tick: not between strokes).
pub const INPUT_QUIET: std::time::Duration = std::time::Duration::from_millis(400);

/// How long the CPU composites after the GPU couldn't (no room for a frame's tiles).
const BACK_OFF: std::time::Duration = std::time::Duration::from_secs(3);

/// Tiles of the painter's own picture composited a tick while it catches up (about 0.8 ms
/// of the window's thread: a press landing behind it waits no longer than that).
pub const CATCH_UP_TILES: usize = 32;

/// Whether the painter's own picture is to catch up now: the GPU makes the picture, the
/// frame rests (not playing, not scrubbing, no stroke) and tiles are left.
pub fn painter_behind(e: &Engine, s: &State) -> bool {
    e.frames.gpu.on
        && !s.playing
        && e.frames.gpu.view.is_none()
        && !e.is_stroking()
        && e.painter.stale_tiles() > 0
        && e.frames
            .gpu
            .input_at
            .is_none_or(|t| t.elapsed() >= INPUT_QUIET)
}

/// Whether the pen or mouse has rested on the canvas long enough for work that would take
/// the window's thread between strokes.
pub fn pen_quiet(e: &Engine) -> bool {
    e.frames
        .gpu
        .input_at
        .is_none_or(|t| t.elapsed() >= INPUT_QUIET)
}

/// When the pen will have rested long enough ([`pen_quiet`]).
pub fn quiet_at(e: &Engine) -> std::time::Instant {
    let now = std::time::Instant::now();
    e.frames
        .gpu
        .input_at
        .map_or(now, |t| (t + INPUT_QUIET).max(now))
}

/// When the painter's own picture is next to catch up (see [`painter_behind`]).
pub fn catch_up_at(e: &Engine, s: &State) -> Option<std::time::Instant> {
    let g = &e.frames.gpu;
    let waiting =
        g.on && !s.playing && g.view.is_none() && !e.is_stroking() && e.painter.stale_tiles() > 0;
    let now = std::time::Instant::now();
    waiting.then(|| {
        (now + std::time::Duration::from_millis(8)).max(g.input_at.map_or(now, |t| t + INPUT_QUIET))
    })
}

/// How long a frame rests before the painter shows its drawings too.
pub const REST: std::time::Duration = std::time::Duration::from_millis(80);

/// Whether a frame change may be shown by the GPU alone for now (the painter catching up
/// later).
pub fn lazy(e: &Engine) -> bool {
    e.frames.gpu.on && !e.frames.gpu.catching_up && can(e) && !e.painter.transforming()
}

/// The picture: made on the GPU (whether anything is to be composited), or the painter's
/// (as [`super::picture`] gives it).
pub enum Pic<'a> {
    Gpu(bool),
    Cpu(Option<(&'a [u8], [u32; 4])>),
}

/// A drawing's id as a source; a layer showing no drawing yet, and a layer's mask, have ids
/// of their own.
fn drawing_source(d: u32) -> u64 {
    d as u64
}
fn layer_source(layer: u32) -> u64 {
    1 << 32 | layer as u64
}
fn mask_source(layer: u32) -> u64 {
    2 << 32 | layer as u64
}

/// An empty cel (the GPU's view of a frame where a layer shows nothing).
const NOTHING: u64 = u64::MAX;

fn source_of(drawing: u32, layer: u32) -> u64 {
    if drawing != 0 {
        drawing_source(drawing)
    } else {
        layer_source(layer)
    }
}

/// Before the painter shows other drawings: the changes not taken yet belong to the
/// drawings showing now, which stop showing, so their GPU copies are forgotten.
pub(super) fn before_show(e: &mut Engine) {
    if !e.frames.gpu.on {
        return;
    }
    let ch = e.painter.take_changes();
    if ch.colour.is_empty() && ch.mask.is_empty() && ch.other.is_empty() {
        return;
    }
    let active = e.painter.active_layer();
    let layers = e.painter.composite_layers();
    let src = |id: u32| {
        layers
            .iter()
            .find(|l| l.id == id)
            .map(|l| source_of(l.drawing, l.id))
    };
    let g = &mut e.frames.gpu;
    for (id, _) in &ch.colour {
        g.forget_now.extend(src(*id));
    }
    for (id, _) in &ch.mask {
        g.forget_now.push(mask_source(*id));
    }
    if !ch.other.is_empty() {
        g.forget_now.extend(src(active));
        g.forget_now.push(mask_source(active));
        g.other.extend(ch.other);
    }
}

/// After the painter showed other drawings: what it marked is only where they are (the
/// compositor finds that itself).
pub(super) fn after_show(e: &mut Engine) {
    if e.frames.gpu.on {
        let _ = e.painter.take_changes();
    }
}

/// Whether the GPU can composite the painter's layers as they are.
fn can(e: &Engine) -> bool {
    let layers = e.painter.composite_layers();
    layers.len() <= pencilpilot_gpu::MAX_GPU_LAYERS
        && layers.iter().all(|l| l.plain)
        && !e.painter.is_editing_mask()
        && std::env::var_os("PENCILPILOT_CPU_LAYERS").is_none()
}

/// The canvas's picture, composited on the GPU into `comp` where it can be (see the
/// module's notes), the painter's otherwise.
pub fn picture<'a>(e: &'a mut Engine, s: &State, gpu: &Gpu, comp: &mut Compositor) -> Pic<'a> {
    if !e.frames.ready {
        setup(e, s);
    }
    show(e, s);
    let resting = e
        .frames
        .gpu
        .cpu_until
        .is_some_and(|t| std::time::Instant::now() < t);
    if resting || !can(e) || (comp.width, comp.height) != (e.painter.width(), e.painter.height()) {
        return cpu(e, s);
    }
    match compose(e, s, gpu, comp) {
        Ok(changed) => Pic::Gpu(changed),
        // (no room for this frame's tiles, or the GPU failed: the CPU makes it, for a while,
        // so a frame too big for the GPU isn't sent up again and again)
        Err(_) => {
            comp.forget_all();
            e.frames.gpu.cpu_until = Some(std::time::Instant::now() + BACK_OFF);
            cpu(e, s)
        }
    }
}

fn cpu<'a>(e: &'a mut Engine, s: &State) -> Pic<'a> {
    if e.frames.gpu.on {
        e.frames.gpu.on = false;
        super::catch_up(e, s);
        // (the painter's picture catches up with everything the GPU did meanwhile)
        e.frames.full = true;
    }
    Pic::Cpu(super::picture(e, s))
}

fn compose(
    e: &mut Engine,
    s: &State,
    gpu: &Gpu,
    comp: &mut Compositor,
) -> pencilpilot_gpu::Result<bool> {
    let showed = std::mem::take(&mut e.frames.showed);
    if !e.frames.gpu.on || e.frames.gpu.doc != s.doc_id {
        // starting (again): nothing kept is known to be as the painter has it
        comp.forget_all();
        let _ = e.painter.take_changes();
        let g = &mut e.frames.gpu;
        *g = GpuFrames {
            on: true,
            doc: s.doc_id,
            edits: e.painter.edits(),
            ..Default::default()
        };
    }
    comp.begin_frame();
    let ch = e.painter.take_changes();
    let layers = e.painter.composite_layers();
    let active = e.painter.active_layer();
    let active_drawing = e.painter.layer_drawing(active);
    let any = !ch.colour.is_empty() || !ch.mask.is_empty() || !ch.other.is_empty();
    if any && !showed && !s.playing && active_drawing != 0 {
        // paint (a stroke, a fill): the active layer's drawing changed (its thumbnail is made
        // again once the stroke ends)
        e.frames.painted = Some(active_drawing);
    }
    let g = &mut e.frames.gpu;
    // (a drawing a layer shows is kept up to date tile by tile from the painter's changes:
    // its strokes, its undo; only one changed out of sight is sent up again)
    for id in std::mem::take(&mut g.forget) {
        let showing = layers
            .iter()
            .any(|l| l.drawing != 0 && drawing_source(l.drawing) == id);
        if !showing {
            comp.forget(id);
        }
    }
    for id in std::mem::take(&mut g.forget_now) {
        comp.forget(id);
    }
    // each layer's source: what the painter shows, or the GPU's own view of the frame
    let view = g.view.clone().unwrap_or_default();
    let viewed = |l: &brush::LayerComposite| {
        view.chunks(2)
            .find(|p| p[0] == l.id)
            .map(|p| p[1])
            .filter(|d| *d != l.drawing)
    };
    let now: Vec<(u64, Option<u64>)> = layers
        .iter()
        .map(|l| {
            let src = match viewed(l) {
                Some(0) => NOTHING,
                Some(d) => drawing_source(d),
                None => source_of(l.drawing, l.id),
            };
            (src, l.has_mask.then(|| mask_source(l.id)))
        })
        .collect();
    // an edit and another drawing in one go: what stopped showing may hold it
    let edits = e.painter.edits();
    if edits != g.edits {
        for (was, _) in &g.last {
            if !now.iter().any(|n| n.0 == *was) {
                comp.forget(*was);
            }
        }
        g.edits = edits;
    }
    let mut other = std::mem::take(&mut g.other);
    other.extend(ch.other.iter().copied());
    let p = &e.painter;
    // tiles that changed, on layers showing what they showed
    for (id, keys) in &ch.colour {
        let Some(l) = layers.iter().find(|l| l.id == *id) else {
            continue;
        };
        let src = source_of(l.drawing, l.id);
        if comp.has(src) {
            for &k in keys {
                comp.put_tile(gpu, src, k, p.layer_tile(l.id, k))?;
            }
        }
    }
    for (id, keys) in &ch.mask {
        let src = mask_source(*id);
        if comp.has(src) {
            for &k in keys {
                comp.put_tile(gpu, src, k, p.mask_tile(*id, k))?;
            }
        }
    }
    // changed some other way: tools writing straight into the active layer (a transform,
    // liquify); anything else there is only a setting
    if !other.is_empty()
        && let Some(l) = layers.iter().find(|l| l.id == active)
    {
        let src = source_of(l.drawing, l.id);
        if comp.has(src) {
            for &k in &other {
                comp.put_tile(gpu, src, k, p.layer_tile(l.id, k))?;
            }
        }
        if l.has_mask && comp.has(mask_source(l.id)) {
            for &k in &other {
                comp.put_tile(gpu, mask_source(l.id), k, p.mask_tile(l.id, k))?;
            }
        }
    }
    comp.invalidate(&other);
    // every source this frame shows, there (drawings the painter doesn't have brought in)
    let missing: Vec<u32> = layers
        .iter()
        .filter_map(viewed)
        .filter(|&d| d != 0 && !comp.has(drawing_source(d)) && !p.has_drawing(d))
        .collect();
    if !missing.is_empty() {
        load_drawings(e, &missing);
    }
    let p = &e.painter;
    for (l, (src, _)) in layers.iter().zip(&now) {
        let src = *src;
        if src != NOTHING && !comp.has(src) {
            comp.start(src);
            let tiles = match viewed(l) {
                Some(d) => p.drawing_tiles_px(d),
                None => p.layer_tiles(l.id),
            };
            for (k, px) in tiles {
                comp.put_tile(gpu, src, k, Some(px))?;
            }
        }
        if l.has_mask && !comp.has(mask_source(l.id)) {
            comp.start(mask_source(l.id));
            for (k, px) in p.mask_tiles(l.id) {
                comp.put_tile(gpu, mask_source(l.id), k, Some(px))?;
            }
        }
    }
    let entries = if s.onion && !s.playing {
        onion_entries(s)
    } else {
        Vec::new()
    };
    let missing: Vec<u32> = entries
        .chunks(3)
        .map(|x| x[0])
        .filter(|d| !comp.has(drawing_source(*d)))
        .collect();
    if !missing.is_empty() {
        load_drawings(e, &missing);
    }
    let p = &e.painter;
    let mut onion = Vec::new();
    for x in entries.chunks(3) {
        let (d, c, o) = (x[0], x[1], x[2]);
        if !p.has_drawing(d) && !comp.has(drawing_source(d)) {
            continue;
        }
        if !comp.has(drawing_source(d)) {
            comp.start(drawing_source(d));
            for (k, px) in p.drawing_tiles_px(d) {
                comp.put_tile(gpu, drawing_source(d), k, Some(px))?;
            }
        }
        onion.push(OnionLayer {
            source: drawing_source(d),
            tint: [
                ((c >> 16) & 255) as f32 / 255.0,
                ((c >> 8) & 255) as f32 / 255.0,
                (c & 255) as f32 / 255.0,
            ],
            opacity: o,
        });
    }
    let frame: Vec<GpuLayer> = layers
        .iter()
        .zip(&now)
        .map(|(l, (src, mask))| GpuLayer {
            source: *src,
            mask: *mask,
            opacity: l.opacity,
            blend: brush::Blend::ALL
                .iter()
                .position(|b| *b == l.blend)
                .unwrap_or(0) as u32,
            visible: l.visible,
            clip: l.clip,
        })
        .collect();
    comp.set_frame(gpu, &frame, &onion);
    e.frames.gpu.last = now;
    Ok(comp.pending())
}

/// A drawing decoded for the GPU (on another thread), with what the store had.
type WarmedOne = (
    u32,
    std::sync::Arc<super::store::Kept>,
    Vec<((i32, i32), brush::DecodedTile)>,
);
type Warmed = Vec<WarmedOne>;

/// Going up ahead: the drawings of the frames around this one (nearest first, the whole cut
/// in the end, as long as the GPU has room) go up between frames and while nothing else
/// happens, decoded from the store on another thread (those the painter has, from it), and
/// let go of on the CPU once up. Showing them later then only points the shader at them.
#[derive(Default)]
pub struct Warm {
    /// drawings being decoded
    job: Option<std::sync::mpsc::Receiver<Warmed>>,
    /// decoded, to go up
    ready: std::collections::VecDeque<WarmedOne>,
    /// the GPU was full, on this frame (it's tried again on another)
    full: Option<u32>,
    /// nothing left to send (for this frame)
    pub(super) done: Option<u32>,
}

impl std::fmt::Debug for Warm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Warm")
            .field("decoding", &self.job.is_some())
            .field("ready", &self.ready.len())
            .field("full", &self.full)
            .field("done", &self.done)
            .finish()
    }
}

impl Warm {
    /// Whether it has something to do (a wake is wanted soon).
    pub fn busy(&self) -> bool {
        self.job.is_some() || !self.ready.is_empty()
    }
}

/// Drawings sent up ahead (see [`Warm`]) for at most `budget`. Whether there's more to do.
pub fn ahead(
    e: &mut Engine,
    s: &State,
    gpu: &Gpu,
    comp: &mut Compositor,
    budget: std::time::Duration,
) -> bool {
    if !e.frames.gpu.on || e.is_stroking() || !can(e) {
        return false;
    }
    let until = std::time::Instant::now() + budget;
    let w = &mut e.frames.gpu.warm;
    if let Some(rx) = &w.job {
        match rx.try_recv() {
            Ok(v) => {
                w.ready.extend(v);
                w.job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => w.job = None,
        }
    }
    // what's decoded goes up (if still as the store has it)
    while let Some((d, kept, tiles)) = e.frames.gpu.warm.ready.pop_front() {
        if std::time::Instant::now() > until {
            e.frames.gpu.warm.ready.push_front((d, kept, tiles));
            return true;
        }
        let src = drawing_source(d);
        let current = e
            .frames
            .store
            .drawings
            .get(&d)
            .is_some_and(|k| std::sync::Arc::ptr_eq(k, &kept));
        if comp.holds(src) || !current || e.painter.has_drawing(d) {
            continue;
        }
        if !comp.room(tiles.len()) {
            e.frames.gpu.warm.full = Some(s.frame);
            e.frames.gpu.warm.ready.clear();
            return false;
        }
        comp.start(src);
        for (k, t) in &tiles {
            if comp.put_tile(gpu, src, *k, Some(t.pixels())).is_err() {
                comp.forget(src);
                e.frames.gpu.warm.full = Some(s.frame);
                return false;
            }
        }
    }
    let w = &e.frames.gpu.warm;
    if w.job.is_some() || w.full == Some(s.frame) || w.done == Some(s.frame) {
        return w.job.is_some();
    }
    // the next ones: nearest frames first
    let n = s.frames.max(1);
    let mut want: Vec<u32> = Vec::new();
    let mut tiles = 0;
    'frames: for i in 0..n as i64 {
        for k in [i, -i] {
            if k == -i && i == 0 {
                continue;
            }
            let f = (s.frame as i64 + k).rem_euclid(n as i64) as u32;
            for t in &s.sheet.tracks {
                let d = t.drawing_at(f);
                if d == 0 || want.contains(&d) || comp.holds(drawing_source(d)) {
                    continue;
                }
                want.push(d);
                tiles += e.frames.store.tiles_of(d).max(1);
                if want.len() >= 16 || tiles >= 1024 {
                    break 'frames;
                }
            }
        }
    }
    if want.is_empty() {
        e.frames.gpu.warm.done = Some(s.frame);
        return false;
    }
    if !comp.room(tiles) || comp.reserve(gpu, tiles).is_err() {
        e.frames.gpu.warm.full = Some(s.frame);
        return false;
    }
    let mut jobs = Vec::new();
    for d in want {
        if e.painter.has_drawing(d) {
            // (the painter has it: straight from there)
            let src = drawing_source(d);
            comp.start(src);
            for (k, px) in e.painter.drawing_tiles_px(d) {
                if comp.put_tile(gpu, src, k, Some(px)).is_err() {
                    comp.forget(src);
                    e.frames.gpu.warm.full = Some(s.frame);
                    return false;
                }
            }
        } else if let Some(j) = e.frames.store.job(d) {
            jobs.push(j);
        }
    }
    if jobs.is_empty() {
        return true;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    super::store::ahead_pool().spawn(move || {
        let done: Warmed = jobs
            .iter()
            .map(|j| (j.d, j.kept().clone(), j.decode()))
            .collect();
        let _ = tx.send(done);
    });
    e.frames.gpu.warm.job = Some(rx);
    true
}

#[cfg(test)]
mod tests;
