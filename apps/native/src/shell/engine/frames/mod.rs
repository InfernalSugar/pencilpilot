//! Layers, drawings and their exposures on the timeline, onion skin, playback, and the
//! picture the canvas shows.
//!
//! - [`sheet`]: the exposure sheet (each layer's cels), as the interface sees it in
//!   [`State::sheet`], and its edits;
//! - this file: the painter kept in step with it. The painter's layers are the layers
//!   (the interface's eye, lock and opacity go to them, their undo comes back); going to a
//!   frame shows each layer's drawing exposed there (`show_drawings`), so strokes land on
//!   it; painting a blank frame starts a drawing there; playback moves the frame on time;
//!   the onion skin is laid under the picture.
//! - [`thumbs`]: the drawings' small pictures for the strips; [`demo`]: the demo's drawings;
//! - [`store`]: drawings not showing, kept compressed out of the painter ([`load_drawings`]
//!   brings them back, [`trim`] puts away what isn't wanted).

mod demo;
mod onion;
pub mod sheet;
pub mod store;
mod thumbs;

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

pub use sheet::{Cel, Mark, Sheet, Thumb, Track, derive};

use super::Engine;
use crate::state::{Action, Layer, State};

/// The actions this file carries out that the interface asks for by name
/// ([`Action::Other`]).
pub mod ops {
    pub const DUPLICATE_DRAWING: &str = "Duplicate drawing";
    pub const BLANK_FRAME: &str = "Blank frame";
    pub const HOLD: [&str; 3] = ["Hold on 1s", "Hold on 2s", "Hold on 3s"];
    pub const KEY: &str = "Key";
    pub const BREAKDOWN: &str = "Breakdown";
    pub const IN_BETWEEN: &str = "In-between";
    pub const CLEAR_MARK: &str = "Clear mark";
    pub const INSERT_FRAME: &str = "Insert frame";
    pub const REMOVE_FRAME: &str = "Remove frame";
    pub const EXTEND_HOLD: &str = "Extend hold";
    pub const REVERSE: &str = "Reverse frames";
    pub const REMOVE_DRAWING: &str = "Remove drawing";
    pub const HOLD_LONGER: &str = "Hold longer";
    pub const HOLD_SHORTER: &str = "Hold shorter";
    pub const COPY_FRAMES: &str = "Copy frames";
    pub const PASTE_FRAMES: &str = "Paste frames";
    pub const BOIL: &str = "Boil…";
    pub const ALL_ON: [&str; 3] = ["All on ones", "All on twos", "All on threes"];
    pub const DELETE_LAYER: &str = "Delete layer";
    pub const DUPLICATE_LAYER: &str = "Duplicate layer";
    pub const LAYER_UP: &str = "Move layer up";
    pub const LAYER_DOWN: &str = "Move layer down";
    pub const LOOP_FROM: &str = "Loop from here";
    pub const LOOP_TO: &str = "Loop to here";
    pub const CLEAR_LOOP: &str = "Clear the loop range";
}

/// Onion skin's colours (before, after) and the nearest one's opacity.
const ONION_BEFORE: u32 = 0xE0453A;
const ONION_AFTER: u32 = 0x22A05A;
const ONION_OPACITY: f32 = 0.6;
/// An opacity drag is one undo step: it's recorded once the slider rests this long.
const OPACITY_SETTLES: Duration = Duration::from_millis(300);

/// Playback: from when and which frame, at what rate, and how many frames on.
#[derive(Debug, Clone, Copy)]
struct Play {
    t0: Instant,
    from: u32,
    fps: u32,
    n: u64,
}

/// What playback and the drawings keep between frames.
#[derive(Debug, Default)]
pub struct Frames {
    ready: bool,
    /// the painter's layers made at the start (top first), until the state has their ids
    boot: Vec<u32>,
    /// the layers as the interface had them after the last sync (and each opacity)
    last: Vec<(Layer, f32)>,
    /// tracks of layers that are gone (an undo may bring them back)
    graveyard: HashMap<u32, Track>,
    /// (layer, drawing) pairs last shown
    asked: Vec<u32>,
    /// the last `show` changed the drawings (the next flush is a frame change, not paint)
    showed: bool,
    /// the onion skin: its drawings, its picture, the picture with it underneath
    onion_key: Vec<u32>,
    onion: onion::Onion,
    out: Vec<u8>,
    onion_shown: bool,
    /// the whole picture must go up next
    full: bool,
    play: Option<Play>,
    /// an opacity change not recorded yet: the layer, its settings before, when last moved
    opacity: Option<(u32, brush::LayerProps, Instant)>,
    /// drawings whose thumbnails are out of date
    stale: BTreeSet<u32>,
    /// the painter's history as last seen (undo / redo change drawings)
    history: (bool, bool, u32, u32),
    /// copied cels and the layer they're from
    clipboard: Option<(u32, Vec<Cel>)>,
    /// a drawing painted on since the last tick
    painted: Option<u32>,
    /// what the last tick saw (when it's all the same and nothing is due, a tick does
    /// nothing): the frame, the active layer, the layers' count
    seen: (u32, usize, usize),
    /// drawings not showing, kept compressed ([`store`])
    pub store: store::Store,
    /// the painter's history count when last seen (an edit: that drawing's kept copy is
    /// out of date)
    edits_seen: u64,
    /// the next and previous frames' drawings, being decoded on another thread
    prefetch: Option<(Vec<u32>, std::sync::mpsc::Receiver<Prefetched>)>,
    /// when to put away drawings not wanted any more
    trim_at: Option<Instant>,
    /// drawings around the frame are to be asked for ahead (next tick)
    ahead_due: bool,
}

/// Drawings decoded ahead (on another thread).
type Prefetched = Vec<(u32, Vec<((i32, i32), brush::DecodedTile)>)>;

impl Frames {
    /// A drawing changed: its thumbnail and onion skin are made again.
    fn mark(&mut self, drawing: u32) {
        self.stale.insert(drawing);
        self.onion.forget(drawing);
        // (the onion skin showing it is made again too)
        if self.onion_key.chunks(3).any(|e| e[0] == drawing) {
            self.onion_key.clear();
        }
    }

    /// The painter's id for the interface's layer `i`.
    fn id(&self, s: &State, i: usize) -> u32 {
        match s.sheet.tracks.get(i) {
            Some(t) if t.layer != 0 => t.layer,
            _ => self.boot.get(i).copied().unwrap_or(0),
        }
    }
}

/// The painter's layers and drawings made from the state's (the demo's drawings painted).
fn setup(e: &mut Engine, s: &State) {
    let mut st = s.clone();
    derive(&mut st);
    let n = st.layers.len();
    let mut ids = vec![0u32; n];
    // bottom first: each new layer goes above the one before
    for (k, i) in (0..n).rev().enumerate() {
        let name = st.layers[i].name.clone();
        ids[i] = if k == 0 {
            let first = e.painter.layers()[0].id;
            let mut p = e.painter.layers()[0].props.clone();
            p.name = name;
            e.painter.set_layer_props(first, p, false);
            e.painter.set_active_layer(first);
            first
        } else {
            e.painter.add_layer(&name)
        };
    }
    for (t, id) in st.sheet.tracks.iter_mut().zip(&ids) {
        t.layer = *id;
    }
    e.painter.reserve_drawings(st.sheet.max_drawing());
    if st.sheet.demo {
        demo::paint(e, &st, &st.sheet.tracks);
    }
    for (i, l) in st.layers.iter().enumerate() {
        let Some(pl) = e.painter.layers().iter().find(|p| p.id == ids[i]) else {
            continue;
        };
        let mut p = pl.props.clone();
        p.visible = l.visible;
        p.locked = l.locked;
        p.opacity = st.sheet.tracks[i].opacity;
        e.painter.set_layer_props(ids[i], p, false);
    }
    e.painter.clear_history();
    if let Some(&a) = ids.get(st.active_layer) {
        e.painter.set_active_layer(a);
    }
    let h = history(e);
    let f = &mut e.frames;
    f.stale = st.sheet.tracks.iter().flat_map(|t| t.drawings()).collect();
    f.boot = ids;
    f.ready = true;
    f.full = true;
    f.history = h;
    show(e, &st);
}

fn ensure(e: &mut Engine, s: &mut State) {
    if !e.frames.ready {
        setup(e, s);
        if std::env::var_os("PENCILPILOT_PLAY").is_some() {
            // (for checks: play from the start, on a headless display)
            s.playing = true;
        }
    }
    sync(e, s);
}

fn history(e: &Engine) -> (bool, bool, u32, u32) {
    let p = &e.painter;
    (
        p.can_undo(),
        p.can_redo(),
        p.undo_drawing(),
        p.redo_drawing(),
    )
}

/// The interface's layer changes go to the painter, then the painter's layers (as undo
/// leaves them) come back to the interface, and drawings started on blank frames join the
/// sheet.
fn sync(e: &mut Engine, s: &mut State) {
    for (i, t) in s.sheet.tracks.iter_mut().enumerate() {
        if t.layer == 0
            && let Some(&id) = e.frames.boot.get(i)
        {
            t.layer = id;
        }
    }
    derive(s);
    let now = Instant::now();
    // 1. what the interface changed
    if e.frames.last.len() == s.layers.len() {
        for i in 0..s.layers.len() {
            let id = s.sheet.tracks[i].layer;
            let Some(pl) = e.painter.layers().iter().find(|p| p.id == id) else {
                continue;
            };
            let mut p = pl.props.clone();
            let (was, was_opacity) = &e.frames.last[i];
            let l = &s.layers[i];
            if l.visible != was.visible || l.locked != was.locked || l.name != was.name {
                p.visible = l.visible;
                p.locked = l.locked;
                p.name = l.name.clone();
                e.painter.set_layer_props(id, p.clone(), true);
            }
            let o = s.sheet.tracks[i].opacity.clamp(0.0, 1.0);
            if (o - was_opacity).abs() > 1e-4 {
                let before = match e.frames.opacity.take() {
                    Some((pid, before, _)) if pid == id => before,
                    Some((pid, before, _)) => {
                        e.painter.commit_layer_props(pid, before);
                        p.clone()
                    }
                    None => p.clone(),
                };
                p.opacity = o;
                e.painter.set_layer_props(id, p, false);
                e.frames.opacity = Some((id, before, now));
            }
        }
    }
    // 2. the painter's layers, top first
    let active_id = s.sheet.tracks.get(s.active_layer).map(|t| t.layer);
    let mut old: HashMap<u32, Track> = s.sheet.tracks.drain(..).map(|t| (t.layer, t)).collect();
    let mut tracks = Vec::new();
    let mut layers = Vec::new();
    for pl in e.painter.layers().iter().rev() {
        let mut t = old
            .remove(&pl.id)
            .or_else(|| e.frames.graveyard.remove(&pl.id))
            .unwrap_or_else(|| Track::new(pl.id, 'C'));
        t.opacity = pl.props.opacity;
        layers.push(Layer {
            name: pl.props.name.clone(),
            visible: pl.props.visible,
            locked: pl.props.locked,
            kind: t.kind,
        });
        tracks.push(t);
    }
    e.frames
        .graveyard
        .extend(old.into_iter().filter(|(id, _)| *id != 0));
    s.sheet.tracks = tracks;
    s.layers = layers;
    s.active_layer = active_id
        .and_then(|a| s.sheet.tracks.iter().position(|t| t.layer == a))
        .unwrap_or(s.active_layer.min(s.layers.len().saturating_sub(1)));
    if let Some(t) = s.sheet.tracks.get(s.active_layer)
        && e.painter.active_layer() != t.layer
    {
        e.painter.set_active_layer(t.layer);
    }
    // 3. drawings the painter started on blank frames (painting where nothing was)
    if !e.is_stroking() {
        let hold = s.strips.picked[3] as u32 + 1;
        for t in &mut s.sheet.tracks {
            let asked_blank = e.frames.asked.chunks(2).any(|p| p == [t.layer, 0]);
            let d = e.painter.layer_drawing(t.layer);
            if asked_blank && d != 0 && t.drawing_at(s.frame) == 0 {
                let hold = if s.frame < s.frames {
                    hold.min(s.frames - s.frame)
                } else {
                    hold
                };
                t.insert_after(s.frame, Cel::new(d, hold, Mark::None));
                for p in e.frames.asked.chunks_mut(2) {
                    if p[0] == t.layer {
                        p[1] = d;
                    }
                }
                e.frames.mark(d);
            }
        }
    }
    derive(s);
    e.frames.last = s
        .layers
        .iter()
        .cloned()
        .zip(s.sheet.tracks.iter().map(|t| t.opacity))
        .collect();
}

/// Each layer shows its drawing exposed on the current frame. Whether the painter changed.
fn show(e: &mut Engine, s: &State) -> bool {
    e.frames.showed = false;
    if e.is_stroking() {
        return false;
    }
    let mut pairs = Vec::new();
    for (i, t) in s.sheet.tracks.iter().enumerate() {
        let id = e.frames.id(s, i);
        let mut d = t.drawing_at(s.frame);
        // a drawing just started on a blank frame stays until the sheet takes it
        let shows = e.painter.layer_drawing(id);
        if d == 0 && shows != 0 && e.frames.asked.chunks(2).any(|p| p == [id, 0]) {
            d = shows;
        }
        pairs.extend([id, d]);
    }
    let need = pairs
        .chunks(2)
        .any(|p| e.painter.layer_drawing(p[0]) != p[1]);
    if !need {
        e.frames.asked = pairs;
        return false;
    }
    let wanted: Vec<u32> = pairs.chunks(2).map(|p| p[1]).collect();
    load_drawings(e, &wanted);
    if e.painter.show_drawings(&pairs) {
        e.frames.asked = pairs;
        e.frames.showed = true;
        // (putting away waits for a pause: frame changes stay quick while scrubbing; unless
        // too much is held meanwhile)
        if held_tiles(e) > HELD_TILES {
            trim(e, s);
        } else {
            e.frames.trim_at = Some(Instant::now() + TRIM_AFTER);
        }
        return true;
    }
    false
}

/// Drawings brought back into the painter from the store (those it hasn't), decoded on
/// every core.
pub fn load_drawings(e: &mut Engine, drawings: &[u32]) {
    use rayon::prelude::*;
    let mut missing: Vec<u32> = drawings
        .iter()
        .copied()
        .filter(|&d| {
            d != 0 && !e.painter.has_drawing(d) && e.frames.store.drawings.contains_key(&d)
        })
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if missing.is_empty() {
        return;
    }
    let st = &e.frames.store;
    let back: Prefetched = store::pool().install(|| {
        missing
            .par_iter()
            .filter_map(|&d| Some((d, st.decode(d)?)))
            .collect()
    });
    for (d, tiles) in back {
        e.painter.put_drawing_decoded(d, tiles);
    }
}

impl Engine {
    /// A drawing's pixels (straight RGBA8, document size), wherever it is: brought back into
    /// the painter if it was put away ([`store`]). For anything reading drawings not
    /// showing (exporting, checks).
    pub fn drawing_rgba(&mut self, d: u32) -> Vec<u8> {
        load_drawings(self, &[d]);
        self.painter.drawing_rgba(d)
    }

    /// Whether a drawing has no paint, wherever it is.
    pub fn drawing_empty(&mut self, d: u32) -> bool {
        load_drawings(self, &[d]);
        self.painter.drawing_empty(d)
    }
}

/// The drawings the painter must hold now: those showing, the onion skin's, and those its
/// undo history changes (an undo on another frame changes them in place).
fn wanted(e: &Engine, s: &State) -> std::collections::HashSet<u32> {
    let mut keep: std::collections::HashSet<u32> =
        e.painter.history_drawings().into_iter().collect();
    keep.extend(e.frames.asked.chunks(2).map(|p| p[1]));
    if s.onion && !s.playing {
        keep.extend(onion_entries(s).chunks(3).map(|e| e[0]));
    }
    keep.extend(neighbours(e, s));
    keep
}

/// Tiles of drawings brought in ahead, at most (64 MB in the painter).
const AHEAD_TILES: usize = 2048;
/// Drawings not wanted any more are put away this long after the last frame change, or at
/// once past this many tiles held that the store has too.
const TRIM_AFTER: Duration = Duration::from_millis(60);
const HELD_TILES: usize = 4 * AHEAD_TILES;

/// Tiles the painter holds of drawings not showing that the store has too.
fn held_tiles(e: &Engine) -> usize {
    e.painter
        .kept_drawings()
        .iter()
        .map(|d| e.frames.store.tiles_of(*d))
        .sum()
}

/// The drawings of the frames around this one, nearest first (ahead only while playing):
/// brought in ahead, on other threads, so going there needn't wait. The next and previous
/// frames' always; farther ones while they fit in [`AHEAD_TILES`].
fn neighbours(e: &Engine, s: &State) -> Vec<u32> {
    let n = s.frames.max(1);
    let steps: Vec<i64> = if s.playing {
        (1..=8).collect()
    } else {
        vec![1, -1, 2, -2, 3, -3, 4, 5]
    };
    let mut v: Vec<u32> = Vec::new();
    let mut tiles = 0;
    for (i, k) in steps.into_iter().enumerate() {
        let f = (s.frame as i64 + k).rem_euclid(n as i64) as u32;
        let here: Vec<u32> = s
            .sheet
            .tracks
            .iter()
            .map(|t| t.drawing_at(f))
            .filter(|d| *d != 0 && !v.contains(d))
            .collect();
        let cost: usize = here.iter().map(|d| e.frames.store.tiles_of(*d)).sum();
        if i >= 2 && tiles + cost > AHEAD_TILES {
            break;
        }
        tiles += cost;
        v.extend(here);
    }
    v
}

/// Drawings decoded ahead arrive; more are asked for. Whether some arrived.
fn prefetch(e: &mut Engine, s: &State, more: bool) -> bool {
    let mut arrived = false;
    if let Some((_, rx)) = &e.frames.prefetch {
        match rx.try_recv() {
            Ok(done) => {
                for (d, tiles) in done {
                    // (still wanted, still as kept: not changed meanwhile)
                    if !e.painter.has_drawing(d) && e.frames.store.drawings.contains_key(&d) {
                        e.painter.put_drawing_decoded(d, tiles);
                        arrived = true;
                    }
                }
                e.frames.prefetch = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(_) => e.frames.prefetch = None,
        }
    }
    if e.is_stroking() || !more {
        e.frames.ahead_due = !more;
        return arrived;
    }
    e.frames.ahead_due = false;
    let jobs: Vec<store::Job> = neighbours(e, s)
        .into_iter()
        .filter(|d| !e.painter.has_drawing(*d))
        .filter_map(|d| e.frames.store.job(d))
        .collect();
    if !jobs.is_empty() {
        let ds = jobs.iter().map(|j| j.d).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        store::ahead_pool().spawn(move || {
            let done: Prefetched = jobs.iter().map(|j| (j.d, j.decode())).collect();
            let _ = tx.send(done);
        });
        e.frames.prefetch = Some((ds, rx));
    }
    arrived
}

/// Drawings the painter holds but nothing wants now go to the store (compressed; those
/// already kept there, unchanged, are just let go).
pub fn trim(e: &mut Engine, s: &State) {
    use rayon::prelude::*;
    if e.is_stroking() || e.painter.transforming() || e.float.is_some() {
        return;
    }
    // (changed since they were kept: the painter's is the one)
    for d in e.painter.history_drawings() {
        e.frames.store.drawings.remove(&d);
    }
    let keep = wanted(e, s);
    let mut pack = Vec::new();
    for d in e.painter.kept_drawings() {
        if keep.contains(&d) {
            continue;
        }
        if e.frames.store.drawings.contains_key(&d) {
            e.painter.forget_drawing(d);
        } else if let Some(t) = e.painter.take_drawing(d) {
            pack.push((d, t));
        }
    }
    let packed: Vec<(u32, std::sync::Arc<store::Kept>)> = store::pool().install(|| {
        pack.into_par_iter()
            .map(|(d, t)| (d, std::sync::Arc::new(store::Store::pack(t))))
            .collect()
    });
    e.frames.store.drawings.extend(packed);
}

/// Where playback is `n` frames after `from` in `a..=b`, and whether it stopped there.
fn play_position(from: u32, n: u64, a: u32, b: u32, mode: usize) -> (u32, bool) {
    let len = (b - a + 1) as u64;
    let off = (from.clamp(a, b) - a) as u64 + n;
    match mode {
        0 if off >= len => (b, true),
        0 => (a + off as u32, false),
        2 if len > 1 => {
            let period = 2 * (len - 1);
            let m = off % period;
            let f = if m < len { m } else { period - m };
            (a + f as u32, false)
        }
        _ => (a + (off % len) as u32, false),
    }
}

/// The frames playback goes over: the loop range, else the cut.
fn play_range(s: &State) -> (u32, u32) {
    let last = s.frames.saturating_sub(1);
    match s.sheet.loop_range {
        Some((a, b)) if a.min(last) <= b.min(last) => (a.min(last), b.min(last)),
        _ => (0, last),
    }
}

/// The drawings before and after the current one on the active layer, for the onion
/// skin: (drawing, colour, opacity 0–255) triples, farthest first.
pub fn onion_entries(s: &State) -> Vec<u32> {
    let Some(t) = s.sheet.tracks.get(s.active_layer) else {
        return Vec::new();
    };
    let (before, after) = match s.strips.onion_range {
        0 => (1, 1),
        1 => (2, 1),
        _ => (2, 2),
    };
    let here = t.at(s.frame).map_or(t.cels.len(), |(i, _)| i);
    let showing = t.drawing_at(s.frame);
    let pick = |it: &mut dyn Iterator<Item = usize>, n: usize| -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        for i in it {
            let d = t.cels[i].drawing;
            if d != 0 && d != showing && !v.contains(&d) {
                v.push(d);
                if v.len() == n {
                    break;
                }
            }
        }
        v
    };
    let prev = pick(&mut (0..here).rev(), before);
    let next = pick(&mut (here + 1..t.cels.len()), after);
    let fade = |i: usize, n: usize| {
        ((ONION_OPACITY * (1.0 - 0.5 * i as f32 / n.max(1) as f32)) * 255.0).round() as u32
    };
    let mut out = Vec::new();
    for i in (0..prev.len().max(next.len())).rev() {
        if let Some(&d) = prev.get(i) {
            out.extend([d, ONION_BEFORE, fade(i, before)]);
        }
        if let Some(&d) = next.get(i)
            && !prev.contains(&d)
        {
            out.extend([d, ONION_AFTER, fade(i, after)]);
        }
    }
    out
}

/// An action about layers, drawings or frames. Returns whether it was one and the picture
/// may have changed.
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    ensure(e, s);
    let frame = s.frame;
    let li = s.active_layer;
    let id = s.sheet.tracks.get(li).map_or(0, |t| t.layer);
    // a layer to make active once the layers follow the painter
    let mut activate: Option<u32> = None;
    let handled = match a {
        Action::NewDrawing => {
            let d = e.painter.new_drawing();
            new_cel(s, Cel::new(d, new_hold(s), Mark::None))
        }
        Action::Play => {
            let (a0, b) = play_range(s);
            if s.strips.play_mode == 0 && s.frame >= b {
                s.frame = a0;
            }
            s.playing = true;
            e.frames.play = None;
            true
        }
        Action::Stop => {
            s.playing = false;
            e.frames.play = None;
            true
        }
        Action::PreviousFrame => {
            s.frame = if frame == 0 { s.frames - 1 } else { frame - 1 };
            true
        }
        Action::NextFrame => {
            s.frame = if frame + 1 >= s.frames { 0 } else { frame + 1 };
            true
        }
        Action::GoToFrame(f) => {
            s.frame = (*f).min(s.frames - 1);
            true
        }
        Action::PreviousDrawing | Action::NextDrawing => {
            let Some(t) = s.sheet.tracks.get(li) else {
                return false;
            };
            let here = t.at(frame).map(|(_, a)| a).unwrap_or(t.end());
            let starts: Vec<u32> = t
                .spans()
                .into_iter()
                .filter(|(_, c)| c.drawing != 0)
                .map(|(a, _)| a)
                .collect();
            let to = if *a == Action::PreviousDrawing {
                starts.iter().rev().find(|a| **a < here).copied()
            } else {
                starts.iter().find(|a| **a > here).copied()
            };
            if let Some(f) = to {
                s.frame = f;
            }
            true
        }
        Action::NewLayer => {
            let n = e.painter.layers().len() + 1;
            let new = e.painter.add_layer(&format!("Layer {n}"));
            if new != 0 {
                activate = Some(new);
            }
            true
        }
        Action::Other(name) => match other(e, s, name, id) {
            Some(new) => {
                activate = new;
                true
            }
            None => false,
        },
        _ => false,
    };
    if !handled {
        return false;
    }
    sync(e, s);
    if let Some(new) = activate
        && let Some(i) = s.sheet.tracks.iter().position(|t| t.layer == new)
    {
        s.active_layer = i;
        e.painter.set_active_layer(new);
        derive(s);
    }
    show(e, s);
    true
}

/// How long a new drawing is held, as the web app's New drawing has it: as long as the
/// drawing it follows; past the layer's end, one frame (on a blank, the blank's rest).
fn new_hold(s: &State) -> u32 {
    let Some(t) = s.sheet.tracks.get(s.active_layer) else {
        return 1;
    };
    match t.at(s.frame) {
        Some((i, start)) if t.cels[i].drawing == 0 => t.cels[i].hold - (s.frame - start),
        Some((i, _)) => t.cels[i].hold,
        None => 1,
    }
}

/// A cel on the active layer after the exposure on the current frame; the current frame
/// goes to it. (A static layer's one drawing is replaced.)
fn new_cel(s: &mut State, cel: Cel) -> bool {
    let li = s.active_layer;
    let Some(t) = s.sheet.tracks.get_mut(li) else {
        return false;
    };
    if t.kind == 'S' {
        if let Some(c) = t.cels.first_mut() {
            c.drawing = cel.drawing;
        }
        return true;
    }
    s.frame = t.insert_after(s.frame, cel);
    derive(s);
    true
}

/// The named actions. None: not one of them; Some(layer): done, and that layer is to be
/// made active (if any).
fn other(e: &mut Engine, s: &mut State, name: &str, id: u32) -> Option<Option<u32>> {
    let frame = s.frame;
    let li = s.active_layer;
    let hold = s.strips.picked[3] as u32 + 1;
    let cut = s.frames;
    let t = s.sheet.tracks.get_mut(li)?;
    let at = t.at(frame);
    let cur = t.drawing_at(frame);
    let mark = |t: &mut Track, m| {
        t.set_mark(frame, m);
    };
    match name {
        ops::DUPLICATE_DRAWING => {
            let (d, m) = if cur != 0 {
                let m = at.map_or(Mark::None, |(i, _)| t.cels[i].mark);
                (e.painter.copy_drawing(cur), m)
            } else {
                (e.painter.new_drawing(), Mark::None)
            };
            e.frames.mark(d);
            if let Some(st) = s.sheet.status.get(&cur).copied() {
                s.sheet.status.insert(d, st);
            }
            new_cel(s, Cel::new(d, new_hold(s), m));
        }
        ops::BLANK_FRAME => {
            if t.kind != 'S' {
                s.frame = t.insert_after(frame, Cel::new(0, hold, Mark::None));
            }
        }
        n if ops::HOLD.contains(&n) => {
            let h = ops::HOLD.iter().position(|x| *x == n).unwrap_or(0) as u32 + 1;
            t.set_hold(frame, h);
        }
        ops::KEY => mark(t, Mark::Key),
        ops::BREAKDOWN => mark(t, Mark::Breakdown),
        ops::IN_BETWEEN => mark(t, Mark::InBetween),
        ops::CLEAR_MARK => mark(t, Mark::None),
        ops::INSERT_FRAME | ops::HOLD_LONGER => t.insert_frame(frame),
        ops::REMOVE_FRAME | ops::HOLD_SHORTER => {
            t.remove_frame(frame);
        }
        ops::EXTEND_HOLD => {
            t.extend_hold(frame, cut);
        }
        ops::REVERSE => t.reverse(),
        ops::REMOVE_DRAWING => {
            if let Some((i, _)) = at {
                t.cels.remove(i);
            }
        }
        ops::COPY_FRAMES => {
            if let Some((i, _)) = at {
                e.frames.clipboard = Some((id, vec![t.cels[i]]));
            }
        }
        ops::PASTE_FRAMES => {
            let (from, cels) = e.frames.clipboard.clone()?;
            let ds: Vec<u32> = cels.iter().map(|c| c.drawing).collect();
            load_drawings(e, &ds);
            let mut f = frame;
            for mut c in cels {
                // a drawing shows on one layer only: another layer gets copies
                if from != id && c.drawing != 0 {
                    c.drawing = e.painter.copy_drawing(c.drawing);
                    e.frames.mark(c.drawing);
                }
                let t = s.sheet.tracks.get_mut(li)?;
                f = t.insert_after(f, c);
            }
            s.frame = f;
        }
        ops::BOIL => {
            let (i, _) = at?;
            let c = t.cels[i];
            if c.drawing == 0 || c.hold < 2 {
                return Some(None);
            }
            let boils: Vec<u32> = (1..=2)
                .map(|seed| e.painter.boil_drawing(c.drawing, 2.0, seed))
                .collect();
            if boils.contains(&0) {
                return Some(None);
            }
            for b in &boils {
                e.frames.mark(*b);
            }
            // on twos, the drawing and its boils taking turns
            let cycle = [c.drawing, boils[0], boils[1]];
            let mut left = c.hold;
            let mut put = Vec::new();
            let mut k = 0;
            while left > 0 {
                let h = left.min(2);
                let m = if k == 0 { c.mark } else { Mark::None };
                put.push(Cel::new(cycle[k % 3], h, m));
                left -= h;
                k += 1;
            }
            t.cels.splice(i..=i, put);
        }
        n if ops::ALL_ON.contains(&n) => {
            let k = ops::ALL_ON.iter().position(|x| *x == n).unwrap_or(0) as u32 + 1;
            if t.kind != 'S' {
                t.retime(k);
            }
        }
        ops::DELETE_LAYER => {
            e.painter.delete_layer(id);
        }
        ops::DUPLICATE_LAYER => {
            let ds = t.drawings();
            load_drawings(e, &ds);
            let t = s.sheet.tracks.get_mut(li)?;
            let copy = e.painter.duplicate_layer(id);
            if copy == 0 {
                return Some(None);
            }
            // its own copy of every drawing
            let mut map: HashMap<u32, u32> = HashMap::new();
            let mut nt = t.clone();
            for c in &mut nt.cels {
                if c.drawing != 0 {
                    let d = *map
                        .entry(c.drawing)
                        .or_insert_with(|| e.painter.copy_drawing(c.drawing));
                    c.drawing = d;
                }
            }
            nt.layer = copy;
            for (from, to) in &map {
                e.frames.mark(*to);
                if let Some(st) = s.sheet.status.get(from).copied() {
                    s.sheet.status.insert(*to, st);
                }
            }
            e.frames.graveyard.insert(copy, nt);
            return Some(Some(copy));
        }
        ops::LAYER_UP | ops::LAYER_DOWN => {
            let at = e.painter.layers().iter().position(|l| l.id == id)?;
            let to = if name == ops::LAYER_UP {
                at + 1
            } else {
                at.checked_sub(1)?
            };
            e.painter.move_layer(id, to);
        }
        ops::LOOP_FROM => {
            let b = s.sheet.loop_range.map_or(cut - 1, |r| r.1).max(frame);
            s.sheet.loop_range = Some((frame, b));
        }
        ops::LOOP_TO => {
            let a = s.sheet.loop_range.map_or(0, |r| r.0).min(frame);
            s.sheet.loop_range = Some((a, frame));
        }
        ops::CLEAR_LOOP => s.sheet.loop_range = None,
        _ => return None,
    }
    derive(s);
    Some(None)
}

/// Playback and the rest that moves with time. Returns whether the interface changed.
pub fn tick(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    if idle(e, s) {
        return false;
    }
    let before = (
        s.layers.clone(),
        s.sheet.tracks.clone(),
        s.frame,
        s.frames,
        s.active_layer,
        s.playing,
    );
    ensure(e, s);
    let mut changed = false;
    // playback
    if s.playing {
        let (a, b) = play_range(s);
        match &mut e.frames.play {
            None => {
                if !(a..=b).contains(&s.frame) {
                    s.frame = a;
                }
                e.frames.play = Some(Play {
                    t0: now,
                    from: s.frame,
                    fps: s.fps,
                    n: 0,
                });
            }
            Some(p) => {
                if p.fps != s.fps {
                    *p = Play {
                        t0: now,
                        from: s.frame,
                        fps: s.fps,
                        n: 0,
                    };
                }
                let n = (now.saturating_duration_since(p.t0).as_secs_f64() * p.fps.max(1) as f64)
                    .floor() as u64;
                if n != p.n {
                    p.n = n;
                    let (f, stop) = play_position(p.from, n, a, b, s.strips.play_mode);
                    s.frame = f;
                    if stop {
                        s.playing = false;
                        e.frames.play = None;
                    }
                    derive(s);
                }
            }
        }
    } else {
        e.frames.play = None;
    }
    // an opacity drag that rested: one undo step
    if let Some((_, _, at)) = &e.frames.opacity
        && now >= *at + OPACITY_SETTLES
        && let Some((id, b, _)) = e.frames.opacity.take()
    {
        e.painter.commit_layer_props(id, b);
    }
    // an edit: the drawing's kept copy is out of date
    let hc = e.painter.history_changes();
    if hc != e.frames.edits_seen {
        e.frames.edits_seen = hc;
        for d in [e.painter.undo_drawing(), e.painter.redo_drawing()] {
            e.frames.store.drawings.remove(&d);
        }
    }
    // thumbnails
    let h = history(e);
    if h != e.frames.history {
        for d in [h.2, h.3, e.frames.history.2, e.frames.history.3] {
            if d != 0 {
                e.frames.mark(d);
            }
        }
        e.frames.history = h;
    }
    if !e.is_stroking() {
        if let Some(d) = e.frames.painted.take() {
            e.frames.mark(d);
        }
        // one thumbnail a tick (a frame), and none while playing
        if !s.playing
            && let Some(d) = e.frames.stale.pop_first()
        {
            load_drawings(e, &[d]);
            let showing = e
                .painter
                .layers()
                .iter()
                .find(|l| e.painter.layer_drawing(l.id) == d)
                .map(|l| l.id);
            let t = thumbs::of(&e.painter, d, showing);
            if s.sheet.thumbs.get(&d) != Some(&t) {
                s.sheet.thumbs.insert(d, t);
                changed = true;
            }
            if showing.is_none() {
                trim(e, s);
            }
        }
    }
    e.frames.seen = (s.frame, s.active_layer, e.painter.layers().len());
    let shown = show(e, s);
    if shown {
        changed = true;
    }
    // (more asked for once this frame is drawn, not while: their decoding would slow it)
    prefetch(e, s, !shown);
    if e.frames.trim_at.is_some_and(|t| now >= t) {
        e.frames.trim_at = None;
        trim(e, s);
    }
    changed
        || before
            != (
                s.layers.clone(),
                s.sheet.tracks.clone(),
                s.frame,
                s.frames,
                s.active_layer,
                s.playing,
            )
}

/// Whether a tick has nothing to do: not playing, nothing pending, the interface's layers
/// and frame as the last tick left them, the painter's history unchanged. (A few
/// comparisons: no scans of the sheet or the drawings.)
fn idle(e: &Engine, s: &State) -> bool {
    let f = &e.frames;
    f.ready
        && f.prefetch.is_none()
        && f.trim_at.is_none()
        && !f.ahead_due
        && !s.playing
        && f.play.is_none()
        && f.opacity.is_none()
        && f.painted.is_none()
        && f.stale.is_empty()
        && f.seen == (s.frame, s.active_layer, e.painter.layers().len())
        && history(e) == f.history
        && f.last.len() == s.layers.len()
        && f.last
            .iter()
            .zip(&s.layers)
            .zip(&s.sheet.tracks)
            .all(|(((l, o), m), t)| l == m && *o == t.opacity)
        && !f
            .asked
            .chunks(2)
            .any(|p| p[1] == 0 && e.painter.layer_drawing(p[0]) != 0)
}

/// When [`tick`] next has something to do.
pub fn next_tick_at(e: &Engine, s: &State) -> Option<Instant> {
    let f = &e.frames;
    let now = Instant::now();
    if !f.ready {
        return Some(now);
    }
    let mut at: Vec<Instant> = Vec::new();
    if s.playing {
        at.push(match f.play {
            Some(p) => p.t0 + Duration::from_secs_f64((p.n + 1) as f64 / p.fps.max(1) as f64),
            None => now,
        });
    }
    if let Some((_, _, t)) = &f.opacity {
        at.push(*t + OPACITY_SETTLES);
    }
    if f.prefetch.is_some() || f.ahead_due {
        at.push(now + Duration::from_millis(1));
    }
    if let Some(t) = f.trim_at {
        at.push(t);
    }
    if !e.is_stroking() {
        let adopt = s.sheet.tracks.iter().any(|t| {
            f.asked.chunks(2).any(|p| p == [t.layer, 0]) && e.painter.layer_drawing(t.layer) != 0
        });
        if (!f.stale.is_empty() && !s.playing)
            || f.painted.is_some()
            || adopt
            || history(e) != f.history
        {
            at.push(now);
        }
    }
    at.into_iter().min()
}

/// The picture the canvas shows and what changed in it: the painter's, with the onion
/// skin laid under it (multiplied, so lines stay on top).
pub fn picture<'a>(e: &'a mut Engine, s: &State) -> Option<(&'a [u8], [u32; 4])> {
    if !e.frames.ready {
        setup(e, s);
    }
    show(e, s);
    let d = e.painter.flush();
    let active = e.painter.layer_drawing(e.painter.active_layer());
    let (w, h) = (e.painter.width(), e.painter.height());
    let f = &mut e.frames;
    if d.is_some() && !f.showed && !s.playing && active != 0 {
        // paint (a stroke, a fill): the active layer's drawing changed (its thumbnail is
        // made again once the stroke ends)
        f.painted = Some(active);
    }
    f.showed = false;
    let whole = [0, 0, w, h];
    let full = std::mem::take(&mut f.full);
    let entries = if s.onion && !s.playing {
        onion_entries(s)
    } else {
        Vec::new()
    };
    if !entries.is_empty() && entries != f.onion_key {
        let ds: Vec<u32> = entries.chunks(3).map(|x| x[0]).collect();
        load_drawings(e, &ds);
    }
    let f = &mut e.frames;
    if entries.is_empty() {
        f.onion_key.clear();
        let was = std::mem::take(&mut f.onion_shown);
        if was || full {
            return Some((e.painter.rgba(), whole));
        }
        return d.map(|r| (e.painter.rgba(), [r.x, r.y, r.w, r.h]));
    }
    let mut rect = d.map(|r| [r.x, r.y, r.w, r.h]);
    if entries != f.onion_key || f.onion.px.is_empty() {
        // only where the onion skin was or is now changes (and where the painter did)
        let changed = f.onion.build(&e.painter, &entries);
        f.onion_key = entries;
        rect = onion::union(rect, changed);
    }
    if !f.onion_shown || full || f.out.len() != (w * h * 4) as usize {
        f.out.resize((w * h * 4) as usize, 255);
        rect = Some(whole);
    }
    f.onion_shown = true;
    let r = rect?;
    onion::compose(&mut f.out, e.painter.rgba(), &f.onion.px, w, r);
    Some((&e.frames.out, r))
}

#[cfg(test)]
mod tests;
