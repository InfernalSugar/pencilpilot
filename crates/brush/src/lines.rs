//! Line (vector) layers. The app keeps each stroke drawn on a line layer (its points,
//! brush and seed) and the engine paints them: live like any stroke, but seeded so the
//! same points paint the same pixels again. To change lines (move, delete, restyle…)
//! the app redraws the tiles they cover from the strokes that touch them
//! (`lines_begin` → `lines_stroke`… → `lines_commit`), as often as it likes during a
//! drag, then makes it one undo step (`lines_push`). Painting on a line layer any other
//! way turns it into a plain layer (undo brings the lines back), as for text.
//!
//! A replay matches the live stroke where every stroke under it is replayed too: dabs
//! composite pixel by pixel. (Smudging brushes read the pixels around them, which a
//! redraw of a few tiles doesn't have: they come out close, not bit-identical.)
//!
//! **Editable strokes on plain layers.** The same strokes can be kept on any layer. The
//! pixels stay the truth; for each drawing the engine keeps a *base*: for every tile an
//! editable stroke painted, the pixels as they were under all of them. A redraw then
//! starts from the base (where there is none, from the pixels as they are: no editable
//! stroke is there) and replays the strokes on top. The base only grows where a stroke
//! paints (copy on write), and every change to it is part of an undo step.
//!
//! Paint of any other kind (a fill, a filter, the eraser, a transform…) on base tiles
//! removes them from the base (in the same undo step) and lists them for the app
//! (`take_tainted`): the strokes there become plain pixels. When some strokes become
//! plain pixels while others on the same tiles stay editable, the app replays them into
//! the base (`base_begin` → `lines_stroke`… → `lines_commit`) or drops tiles no stroke
//! needs any more (`base_drop`); both join the last undo step.

use std::collections::{HashMap, HashSet};

use hokusai::tile::empty_tile;
use hokusai::{Brush, BrushState, TILE_SIZE, TilePixels};

use crate::{Edit, Painter, Snapshot, TileKey, TileMap, UndoSurface};

/// One tile of a base, kept small: most are under line art, so equal pixels in a row
/// are stored once (`Runs`: per run a little-endian u16 length and the pixel's four
/// u16s); tiles that don't shrink that way are kept as they are.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BaseTile {
    Clear,
    Runs(Box<[u8]>),
    Raw(Box<TilePixels>),
}

const TILE_PX: usize = TILE_SIZE * TILE_SIZE;
const RAW_BYTES: usize = TILE_PX * 8;

impl BaseTile {
    pub(crate) fn of(t: Option<&TilePixels>) -> Self {
        let Some(t) = t.filter(|t| !transparent(t)) else {
            return BaseTile::Clear;
        };
        let mut out: Vec<u8> = Vec::new();
        let push = |out: &mut Vec<u8>, p: [u16; 4], n: u16| {
            out.extend_from_slice(&n.to_le_bytes());
            for c in p {
                out.extend_from_slice(&c.to_le_bytes());
            }
        };
        let mut run: Option<([u16; 4], u16)> = None;
        for p in t.iter().flatten() {
            run = match run {
                Some((q, n)) if q == *p => Some((q, n + 1)),
                Some((q, n)) => {
                    push(&mut out, q, n);
                    if out.len() >= RAW_BYTES {
                        return BaseTile::Raw(Box::new(*t));
                    }
                    Some((*p, 1))
                }
                None => Some((*p, 1)),
            };
        }
        if let Some((q, n)) = run {
            push(&mut out, q, n);
        }
        if out.len() >= RAW_BYTES {
            return BaseTile::Raw(Box::new(*t));
        }
        BaseTile::Runs(out.into_boxed_slice())
    }

    /// The pixels (None: transparent).
    pub(crate) fn pixels(&self) -> Option<Box<TilePixels>> {
        match self {
            BaseTile::Clear => None,
            BaseTile::Raw(t) => Some(t.clone()),
            BaseTile::Runs(b) => {
                let mut t = empty_tile();
                let mut at = 0usize;
                for r in b.as_chunks::<10>().0 {
                    let n = u16::from_le_bytes([r[0], r[1]]) as usize;
                    let p = [0, 1, 2, 3].map(|i| u16::from_le_bytes([r[2 + i * 2], r[3 + i * 2]]));
                    for k in at..(at + n).min(TILE_PX) {
                        t[k / TILE_SIZE][k % TILE_SIZE] = p;
                    }
                    at += n;
                }
                Some(t)
            }
        }
    }

    /// Bytes it takes (roughly: what it holds, and its place in the map).
    pub(crate) fn bytes(&self) -> usize {
        32 + match self {
            BaseTile::Clear => 0,
            BaseTile::Runs(b) => b.len(),
            BaseTile::Raw(_) => RAW_BYTES,
        }
    }
}

/// A drawing's base: per tile, the pixels under its editable strokes.
pub(crate) type Base = HashMap<TileKey, BaseTile>;
/// Base entries as they were before an edit (`None`: not there).
pub(crate) type BaseSnap = HashMap<TileKey, Option<BaseTile>>;

/// A base moved like the canvas: its pixels by `redo`, its tiles by `keys` (the tiles a
/// tile's area lands on), so clear entries move too.
pub(crate) fn transform_base(
    b: &Base,
    redo: impl Fn(&TileMap) -> TileMap,
    keys: impl Fn(TileKey) -> Vec<TileKey>,
) -> Base {
    let tiles: TileMap = b
        .iter()
        .filter_map(|(k, t)| t.pixels().map(|p| (*k, p)))
        .collect();
    let moved = redo(&tiles);
    let mut out: Base = b
        .keys()
        .flat_map(|&k| keys(k))
        .map(|k| (k, BaseTile::Clear))
        .collect();
    for (k, t) in moved {
        out.insert(k, BaseTile::of(Some(&t)));
    }
    out
}

/// The tiles a tile's area covers after a canvas size change (see `resize_canvas`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn moved_keys(
    k: TileKey,
    ow: u32,
    oh: u32,
    w: u32,
    h: u32,
    dx: i32,
    dy: i32,
    scale: bool,
) -> Vec<TileKey> {
    let ts = TILE_SIZE as f64;
    let (mut x0, mut y0) = (k.0 as f64 * ts, k.1 as f64 * ts);
    let (mut x1, mut y1) = (x0 + ts, y0 + ts);
    if scale {
        let (sx, sy) = (w as f64 / ow as f64, h as f64 / oh as f64);
        (x0, x1, y0, y1) = (x0 * sx, x1 * sx, y0 * sy, y1 * sy);
    } else {
        (x0, x1, y0, y1) = (
            x0 + dx as f64,
            x1 + dx as f64,
            y0 + dy as f64,
            y1 + dy as f64,
        );
    }
    let cols = (w as f64 / ts).ceil() as i32;
    let rows = (h as f64 / ts).ceil() as i32;
    let tx0 = ((x0 / ts).floor() as i32).max(-1);
    let ty0 = ((y0 / ts).floor() as i32).max(-1);
    let tx1 = (((x1 - 1e-6) / ts).floor() as i32).min(cols);
    let ty1 = (((y1 - 1e-6) / ts).floor() as i32).min(rows);
    let mut out = Vec::new();
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            out.push((tx, ty));
        }
    }
    out
}

/// The tiles being redrawn: dabs that touch none of them aren't drawn at all.
pub(crate) struct Area {
    tiles: HashSet<TileKey>,
    /// the tiles' bounds (inclusive), to turn most dabs away at once
    min: TileKey,
    max: TileKey,
}

impl Area {
    fn new(tiles: HashSet<TileKey>) -> Self {
        let min = tiles
            .iter()
            .fold((i32::MAX, i32::MAX), |m, k| (m.0.min(k.0), m.1.min(k.1)));
        let max = tiles
            .iter()
            .fold((i32::MIN, i32::MIN), |m, k| (m.0.max(k.0), m.1.max(k.1)));
        Self { tiles, min, max }
    }

    /// Whether a dab can change any of the tiles. (libmypaint's dabs reach `radius + 1`
    /// from their centre whatever their shape; textures only weaken what's painted.)
    pub(crate) fn touches(&self, dab: &hokusai::Dab) -> bool {
        let r = dab.radius + 2.0;
        let ts = TILE_SIZE as f32;
        let tile = |v: f32| (v / ts).floor() as i32;
        let (x0, x1) = (
            tile(dab.x - r).max(self.min.0),
            tile(dab.x + r).min(self.max.0),
        );
        let (y0, y1) = (
            tile(dab.y - r).max(self.min.1),
            tile(dab.y + r).min(self.max.1),
        );
        if x0 > x1 || y0 > y1 {
            return false;
        }
        let span = (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64;
        if span > self.tiles.len() as i64 {
            return self
                .tiles
                .iter()
                .any(|k| (x0..=x1).contains(&k.0) && (y0..=y1).contains(&k.1));
        }
        (y0..=y1).any(|ty| (x0..=x1).any(|tx| self.tiles.contains(&(tx, ty))))
    }
}

/// A redraw in progress: the layer, and the strokes replayed so far (only the area's
/// tiles count).
pub(crate) struct Redraw {
    layer: u32,
    scratch: UndoSurface,
    /// replaying into this drawing's base, not the layer (see `base_begin`)
    into_base: Option<u32>,
}

/// A line edit not in the history yet: its layer, the drawing it's on (0: none yet)
/// and the tiles it replaced, as they were first.
pub(crate) struct Pending {
    layer: u32,
    drawing: u32,
    before: Snapshot,
    /// base entries the edit added on a plain layer (tiles strokes reached first), as
    /// they were: not there
    base_before: BaseSnap,
}

fn transparent(t: &TilePixels) -> bool {
    t.iter().flatten().all(|p| p[3] == 0)
}

impl Painter {
    /// Add an empty line layer above the active one and make it active (one undo
    /// step, as `add_layer`). Returns its id (0 = no room).
    pub fn add_line_layer(&mut self, name: &str) -> u32 {
        let name = if name.trim().is_empty() {
            format!("Lines {}", self.next_id)
        } else {
            name.to_string()
        };
        let id = self.add_layer(&name);
        if let Some(l) = self.layer_mut(id) {
            // (undoing the insert takes the layer away, flag and all)
            l.props.lines = true;
        }
        id
    }

    /// Make a layer a line layer (or not) without an undo step (opening a project,
    /// drawing together). Adjustment layers can't be.
    pub fn set_lines_silent(&mut self, id: u32, on: bool) -> bool {
        match self.layer_mut(id) {
            Some(l) if l.props.adjust.is_none() => {
                l.props.lines = on;
                true
            }
            _ => false,
        }
    }

    /// "Convert to pixels": a line layer becomes a plain one (one undo step). False if
    /// busy or it isn't a line layer.
    pub fn rasterize_lines(&mut self, id: u32) -> bool {
        if !self.idle() || self.line_pending.is_some() {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        if !self.layers[idx].props.lines {
            return false;
        }
        let before = self.layers[idx].props.clone();
        self.layers[idx].props.lines = false;
        self.push_step(Edit::Props {
            layer: id,
            props: before,
        });
        true
    }

    /// Start a line stroke on the active layer: as `begin_stroke`, but its brush state
    /// is seeded with `seed` (symmetry copy k with `seed + k + 1`), so replaying the
    /// same points with `lines_stroke` paints the same pixels. The selection doesn't
    /// clip it, alpha lock doesn't hold it, and it keeps a line layer one. Not on a
    /// mask.
    pub fn begin_line_stroke(&mut self, seed: u32) -> bool {
        if self.stroking.is_some()
            || self.line_redraw.is_some()
            || self.editing_mask(self.active)
            || !self.begin_stroke()
        {
            return false;
        }
        self.state = BrushState::new(seed);
        for (k, st) in self.sym_states.iter_mut().enumerate() {
            *st = BrushState::new(seed.wrapping_add(k as u32 + 1));
        }
        self.alpha_lock_stroke = false;
        self.line_stroke = true;
        true
    }

    /// Start redrawing tiles of layer `layer` from its strokes: `tiles` is (tx, ty)
    /// pairs of `TILE_SIZE` tiles. The strokes that touch them follow (`lines_stroke`),
    /// then `lines_commit`. False if busy, the layer is missing, locked, an adjustment
    /// layer or showing its mask, another layer has a line edit pending, or `tiles` has
    /// an odd length.
    pub fn lines_begin(&mut self, layer: u32, tiles: &[i32]) -> bool {
        if !self.idle() || !tiles.len().is_multiple_of(2) {
            return false;
        }
        let Some(idx) = self.index_of(layer) else {
            return false;
        };
        let l = &self.layers[idx];
        if l.props.locked || l.props.adjust.is_some() || l.editing_mask {
            return false;
        }
        if self.line_pending.as_ref().is_some_and(|p| p.layer != layer) {
            return false;
        }
        let keys: HashSet<TileKey> = tiles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .collect();
        // a plain layer: the strokes go over what's under them (the base, or where no
        // editable stroke is yet, the pixels as they are)
        let mut start = TileMap::new();
        if !l.props.lines {
            let base = self.bases.get(&l.drawing).filter(|_| l.drawing != 0);
            for &k in &keys {
                let t = match base.and_then(|b| b.get(&k)) {
                    Some(b) => b.pixels(),
                    None => l.surface.tiles.get(&k).cloned(),
                };
                if let Some(t) = t {
                    start.insert(k, t);
                }
            }
        }
        self.line_redraw = Some(Box::new(Redraw {
            layer,
            scratch: UndoSurface {
                tiles: start,
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            into_base: None,
        }));
        true
    }

    /// Start replaying strokes into a drawing's base (strokes becoming plain pixels while
    /// others above them on the same tiles stay editable): `tiles` as `lines_begin`;
    /// tiles not in the base are left alone. The strokes follow (`lines_stroke`), then
    /// `lines_commit`, which joins the change to the last undo step. False if busy, a
    /// line edit is pending, `layer` (whose dab style is used) is missing, or `tiles` has
    /// an odd length.
    pub fn base_begin(&mut self, drawing: u32, layer: u32, tiles: &[i32]) -> bool {
        if !self.idle() || self.line_pending.is_some() || !tiles.len().is_multiple_of(2) {
            return false;
        }
        if self.index_of(layer).is_none() || drawing == 0 {
            return false;
        }
        let base = self.bases.get(&drawing);
        let keys: HashSet<TileKey> = tiles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .filter(|k| base.is_some_and(|b| b.contains_key(k)))
            .collect();
        let start: TileMap = keys
            .iter()
            .filter_map(|k| base.and_then(|b| b.get(k)?.pixels()).map(|t| (*k, t)))
            .collect();
        self.line_redraw = Some(Box::new(Redraw {
            layer,
            scratch: UndoSurface {
                tiles: start,
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            into_base: Some(drawing),
        }));
        true
    }

    /// Forget base tiles no editable stroke needs any more (joins the last undo step).
    /// The tiles: (x, y) pairs.
    pub fn base_drop(&mut self, drawing: u32, tiles: &[i32]) -> bool {
        if !tiles.len().is_multiple_of(2) {
            return false;
        }
        let Some(base) = self.bases.get(&drawing) else {
            return false;
        };
        let snap: BaseSnap = tiles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .filter(|k| base.contains_key(k))
            .map(|k| (k, None))
            .collect();
        if snap.is_empty() {
            return false;
        }
        let old = self.base_swap(drawing, snap);
        self.join_last(Edit::Base { drawing, snap: old });
        true
    }

    /// Replay one whole stroke into the redraw: `pts` is x, y, pressure, x tilt,
    /// y tilt, seconds since the previous point (6 numbers a point; a partial point at
    /// the end is ignored), painted with `brush`, the dab style set now and a brush
    /// state seeded with `seed`. Nothing without a redraw begun.
    pub fn lines_stroke(&mut self, brush: &Brush, seed: u32, pts: &[f32]) {
        let Some(r) = self.line_redraw.as_mut() else {
            return;
        };
        let mut state = BrushState::new(seed);
        for p in pts.as_chunks::<6>().0 {
            brush.stroke_to(
                &mut state,
                &mut r.scratch,
                p[0],
                p[1],
                p[2],
                p[3],
                p[4],
                p[5] as f64,
            );
        }
        brush.finish_stroke(&mut state, &mut r.scratch);
    }

    /// The redrawn tiles replace the layer's (fully transparent ones are dropped).
    /// Not an undo step yet: the tiles as they were before the first commit are kept
    /// until `lines_push` (or `lines_forget` / `lines_cancel`). False if no redraw was
    /// begun.
    pub fn lines_commit(&mut self) -> bool {
        let Some(r) = self.line_redraw.take() else {
            return false;
        };
        let Redraw {
            layer,
            mut scratch,
            into_base,
        } = *r;
        let Some(area) = scratch.only.take() else {
            return false;
        };
        if let Some(drawing) = into_base {
            let snap: BaseSnap = area
                .tiles
                .iter()
                .map(|k| (*k, Some(BaseTile::of(scratch.tiles.get(k).map(|t| &**t)))))
                .collect();
            if !snap.is_empty() {
                let old = self.base_swap(drawing, snap);
                self.join_last(Edit::Base { drawing, snap: old });
            }
            return true;
        }
        let pending = self.line_pending.take().filter(|p| p.layer == layer);
        let Some(idx) = self.index_of(layer) else {
            return false;
        };
        let plain = !self.layers[idx].props.lines;
        // (a plain layer with editable strokes has a drawing: its base is kept by it)
        if plain && self.layers[idx].drawing == 0 {
            let d = self.new_drawing();
            self.layers[idx].drawing = d;
        }
        let drawing = self.layers[idx].drawing;
        let mut pending = pending.unwrap_or(Pending {
            layer,
            drawing,
            before: Snapshot::new(),
            base_before: BaseSnap::new(),
        });
        let mut base = if plain {
            self.bases.remove(&drawing)
        } else {
            None
        };
        let changed = self.base_changed.entry(drawing).or_default();
        let l = &mut self.layers[idx];
        for key in area.tiles {
            let new = scratch.tiles.remove(&key).filter(|t| !transparent(t));
            let old = match new {
                Some(t) => l.surface.tiles.insert(key, t),
                None => l.surface.tiles.remove(&key),
            };
            // strokes reach a tile for the first time: what was there goes under them
            if plain {
                let b = base.get_or_insert_with(Base::new);
                if let std::collections::hash_map::Entry::Vacant(v) = b.entry(key) {
                    v.insert(BaseTile::of(old.as_deref()));
                    pending.base_before.insert(key, None);
                    changed.insert(key);
                }
            }
            pending.before.entry(key).or_insert(old);
            l.surface.dirty.insert(key);
        }
        if let Some(b) = base.filter(|b| !b.is_empty()) {
            self.bases.insert(drawing, b);
        }
        self.line_pending = Some(pending);
        true
    }

    /// The line edits since the last push become one undo step. False (no step) if
    /// there are none.
    pub fn lines_push(&mut self) -> bool {
        let Some(p) = self.line_pending.take() else {
            return false;
        };
        if self.index_of(p.layer).is_none() {
            return false;
        }
        self.push_line_step(Edit::Tiles {
            layer: p.layer,
            snap: p.before,
            mask: false,
            drawing: p.drawing,
        });
        if !p.base_before.is_empty() {
            self.join_last(Edit::Base {
                drawing: p.drawing,
                snap: p.base_before,
            });
        }
        true
    }

    /// Keep the line edits since the last push without an undo step (someone else's,
    /// drawing together).
    pub fn lines_forget(&mut self) {
        self.line_pending = None;
    }

    /// Undo the line edits since the last push (a cancelled drag), and drop a redraw
    /// in progress.
    pub fn lines_cancel(&mut self) {
        self.line_redraw = None;
        let Some(p) = self.line_pending.take() else {
            return;
        };
        if let Some(l) = self.layer_mut(p.layer) {
            l.surface.swap_in(p.before);
        }
        if !p.base_before.is_empty() {
            self.base_swap(p.drawing, p.base_before);
        }
    }

    // ---- bases (editable strokes on plain layers) ----

    /// Swap base entries in (`None`: remove), returning what they replaced.
    pub(crate) fn base_swap(&mut self, drawing: u32, snap: BaseSnap) -> BaseSnap {
        let base = self.bases.entry(drawing).or_default();
        let changed = self.base_changed.entry(drawing).or_default();
        let mut old = BaseSnap::with_capacity(snap.len());
        for (k, v) in snap {
            changed.insert(k);
            let was = match v {
                Some(t) => base.insert(k, t),
                None => base.remove(&k),
            };
            old.insert(k, was);
        }
        if base.is_empty() {
            self.bases.remove(&drawing);
        }
        old
    }

    /// Put `e` (the undo of a change just made) into the last undo step.
    pub(crate) fn join_last(&mut self, e: Edit) {
        if let Some(last) = self.undo.pop() {
            self.undo.push(Edit::Batch(vec![e, last]));
        }
    }

    /// A line stroke ending on `layer`: the tiles it painted that the base doesn't have
    /// yet, as they were before it (None: a line layer, which needs no base).
    pub(crate) fn base_candidates(
        &self,
        layer: u32,
        snap: Option<&Snapshot>,
    ) -> Option<Vec<(TileKey, BaseTile)>> {
        let l = &self.layers[self.index_of(layer)?];
        if l.props.lines {
            return None;
        }
        let base = self.bases.get(&l.drawing).filter(|_| l.drawing != 0);
        Some(
            snap.into_iter()
                .flatten()
                .filter(|(k, _)| !base.is_some_and(|b| b.contains_key(k)))
                .map(|(k, t)| (*k, BaseTile::of(t.as_deref())))
                .collect(),
        )
    }

    /// Those tiles join the base of the drawing `layer` shows (in the last undo step).
    pub(crate) fn base_add(&mut self, layer: u32, under: Vec<(TileKey, BaseTile)>) {
        let d = self.layer_drawing(layer);
        if d == 0 || under.is_empty() {
            return;
        }
        let snap: BaseSnap = under.into_iter().map(|(k, t)| (k, Some(t))).collect();
        let old = self.base_swap(d, snap);
        self.join_last(Edit::Base {
            drawing: d,
            snap: old,
        });
    }

    /// Another drawing (a copy) gets the same base.
    pub(crate) fn copy_base(&mut self, from: u32, to: u32) {
        if from == 0 || to == 0 {
            return;
        }
        if let Some(b) = self.bases.get(&from).cloned() {
            self.base_changed
                .entry(to)
                .or_default()
                .extend(b.keys().copied());
            self.bases.insert(to, b);
        }
    }

    /// Paint other than editable strokes landing on base tiles (`e`, about to be an
    /// undo step): those tiles leave the base (in the same step) and are listed for the
    /// app, whose strokes there become plain pixels.
    pub(crate) fn taint(&mut self, e: &mut Edit) {
        fn walk(e: &Edit, out: &mut Vec<(u32, u32, Vec<TileKey>)>) {
            match e {
                Edit::Tiles {
                    layer,
                    snap,
                    mask: false,
                    drawing,
                } => out.push((*layer, *drawing, snap.keys().copied().collect())),
                Edit::Batch(list) => list.iter().for_each(|e| walk(e, out)),
                _ => {}
            }
        }
        if self.bases.is_empty() {
            return;
        }
        let mut found = Vec::new();
        walk(e, &mut found);
        let mut undo = Vec::new();
        for (layer, drawing, keys) in found {
            let d = if drawing != 0 {
                drawing
            } else {
                self.layer_drawing(layer)
            };
            let Some(base) = self.bases.get(&d) else {
                continue;
            };
            let hit: Vec<TileKey> = keys.into_iter().filter(|k| base.contains_key(k)).collect();
            if hit.is_empty() {
                continue;
            }
            let snap = hit.iter().map(|k| (*k, None)).collect();
            let old = self.base_swap(d, snap);
            self.tainted.push((d, hit));
            undo.push(Edit::Base {
                drawing: d,
                snap: old,
            });
        }
        if !undo.is_empty() {
            undo.push(std::mem::replace(e, Edit::Batch(Vec::new())));
            *e = Edit::Batch(undo);
        }
    }

    /// The base tiles other paint changed since the last call, as `drawing, count, x, y,
    /// …` runs (their strokes must become plain pixels).
    pub fn take_tainted(&mut self) -> Vec<i32> {
        let mut out = Vec::new();
        for (d, keys) in self.tainted.drain(..) {
            out.push(d as i32);
            out.push(keys.len() as i32);
            for k in keys {
                out.extend([k.0, k.1]);
            }
        }
        out
    }

    /// Bytes the bases take (all drawings with 0, or one).
    pub fn base_bytes(&self, drawing: u32) -> usize {
        self.bases
            .iter()
            .filter(|(d, _)| drawing == 0 || **d == drawing)
            .flat_map(|(_, b)| b.values())
            .map(BaseTile::bytes)
            .sum()
    }

    /// Tiles in a drawing's base.
    pub fn base_tiles(&self, drawing: u32) -> usize {
        self.bases.get(&drawing).map_or(0, |b| b.len())
    }

    /// Forget the bases of drawings not in `keep` (their strokes are gone for good).
    pub fn keep_bases(&mut self, keep: &[u32]) {
        let keep: HashSet<u32> = keep.iter().copied().collect();
        self.bases.retain(|d, _| keep.contains(d));
        self.base_changed.retain(|d, _| keep.contains(d));
    }

    /// A drawing's base packed (saving, sharing): u32 entries, then per entry i32 x,
    /// i32 y; then its tiles with paint as `drawing_tiles_packed` packs them; then u32 0
    /// (no entries gone).
    pub fn base_packed(&self, drawing: u32) -> Vec<u8> {
        let empty = Base::new();
        let b = self.bases.get(&drawing).unwrap_or(&empty);
        pack_base(b.iter().map(|(k, t)| (*k, Some(t))))
    }

    /// Put a drawing's base from `base_packed` bytes (opening a project): no undo step.
    /// Empty bytes clear it. False if the bytes don't add up.
    pub fn put_base_packed(&mut self, drawing: u32, packed: &[u8]) -> bool {
        if drawing == 0 {
            return false;
        }
        if packed.is_empty() {
            self.bases.remove(&drawing);
            return true;
        }
        let Some(entries) = unpack_base(packed) else {
            return false;
        };
        let b: Base = entries
            .into_iter()
            .filter_map(|(k, t)| t.map(|t| (k, t)))
            .collect();
        if b.is_empty() {
            self.bases.remove(&drawing);
        } else {
            self.bases.insert(drawing, b);
        }
        true
    }

    /// What changed in a drawing's base since the last call (drawing together sends it),
    /// as `base_packed` with the entries that went listed at the end (u32 count, then
    /// i32 x, i32 y each). Empty when nothing changed.
    pub fn base_changes_packed(&mut self, drawing: u32) -> Vec<u8> {
        let Some(keys) = self.base_changed.remove(&drawing).filter(|k| !k.is_empty()) else {
            return Vec::new();
        };
        let b = self.bases.get(&drawing);
        let mut keys: Vec<TileKey> = keys.into_iter().collect();
        keys.sort_unstable();
        pack_base(keys.into_iter().map(|k| (k, b.and_then(|b| b.get(&k)))))
    }

    /// Put changes from `base_changes_packed` (someone else's) without an undo step.
    pub fn put_base_changes(&mut self, drawing: u32, packed: &[u8]) -> bool {
        if drawing == 0 {
            return false;
        }
        let Some(entries) = unpack_base(packed) else {
            return false;
        };
        let base = self.bases.entry(drawing).or_default();
        for (k, t) in entries {
            match t {
                Some(t) => base.insert(k, t),
                None => base.remove(&k),
            };
        }
        if base.is_empty() {
            self.bases.remove(&drawing);
        }
        true
    }

    /// Line edits are waiting for `lines_push`.
    pub fn lines_pending(&self) -> bool {
        self.line_pending.is_some()
    }
}

/// Base entries packed: those there (with their tiles), then those gone.
fn pack_base<'a>(entries: impl Iterator<Item = (TileKey, Option<&'a BaseTile>)>) -> Vec<u8> {
    let (here, gone): (Vec<_>, Vec<_>) = entries.partition(|(_, t)| t.is_some());
    let mut out = Vec::new();
    out.extend_from_slice(&(here.len() as u32).to_le_bytes());
    let mut tiles = TileMap::new();
    for (k, t) in &here {
        out.extend_from_slice(&k.0.to_le_bytes());
        out.extend_from_slice(&k.1.to_le_bytes());
        if let Some(px) = t.and_then(|t| t.pixels()) {
            tiles.insert(*k, px);
        }
    }
    out.extend(crate::pack_tiles(Some(&tiles)));
    out.extend_from_slice(&(gone.len() as u32).to_le_bytes());
    for (k, _) in gone {
        out.extend_from_slice(&k.0.to_le_bytes());
        out.extend_from_slice(&k.1.to_le_bytes());
    }
    out
}

/// `pack_base`'s bytes back: the entries there (Some) and gone (None).
fn unpack_base(b: &[u8]) -> Option<Vec<(TileKey, Option<BaseTile>)>> {
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    };
    let i32_at = |at: usize| u32_at(at).map(|v| v as i32);
    let n = u32_at(0)? as usize;
    if n > 1 << 20 {
        return None;
    }
    let mut keys = Vec::with_capacity(n);
    let mut at = 4;
    for _ in 0..n {
        keys.push((i32_at(at)?, i32_at(at + 4)?));
        at += 8;
    }
    let tiles_n = u32_at(at)? as usize;
    let end = at + 4 + tiles_n.checked_mul(8 + TILE_PX * 4)?;
    let tiles = crate::unpack_tiles(b.get(at..end)?)?;
    at = end;
    let mut out: Vec<(TileKey, Option<BaseTile>)> = keys
        .into_iter()
        .map(|k| (k, Some(BaseTile::of(tiles.get(&k).map(|t| &**t)))))
        .collect();
    let gone = u32_at(at)? as usize;
    at += 4;
    if b.len() != at + gone.checked_mul(8)? {
        return None;
    }
    for _ in 0..gone {
        out.push(((i32_at(at)?, i32_at(at + 4)?), None));
        at += 8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Symmetry, SymmetryMode, TileMap, set_brush_rgb};
    use hokusai::BrushSetting;

    /// a small round brush with some scatter, so the seed matters
    fn pen() -> Brush {
        let mut b = Brush::new();
        b.get_mut(BrushSetting::Radius).base_value = 1.5;
        b.get_mut(BrushSetting::Opaque).base_value = 0.8;
        b.get_mut(BrushSetting::Hardness).base_value = 0.7;
        b.get_mut(BrushSetting::DabsPerActualRadius).base_value = 2.0;
        b.get_mut(BrushSetting::OffsetByRandom).base_value = 0.6;
        set_brush_rgb(&mut b, 20, 40, 200);
        b
    }

    /// a wavy stroke from (x0, y) to (x1, y), as `lines_stroke` takes it
    fn wave(x0: f32, x1: f32, y: f32) -> Vec<f32> {
        (0..60)
            .flat_map(|i| {
                let t = i as f32 / 59.0;
                let x = x0 + (x1 - x0) * t;
                [x, y + 30.0 * (t * 9.0).sin(), 0.4 + 0.5 * t, 0.0, 0.0, 0.01]
            })
            .collect()
    }

    fn shifted(pts: &[f32], dx: f32, dy: f32) -> Vec<f32> {
        pts.as_chunks::<6>()
            .0
            .iter()
            .flat_map(|p| [p[0] + dx, p[1] + dy, p[2], p[3], p[4], p[5]])
            .collect()
    }

    /// draw `pts` live as a line stroke on the active layer
    fn live(p: &mut Painter, b: &Brush, seed: u32, pts: &[f32]) {
        assert!(p.begin_line_stroke(seed));
        for q in pts.as_chunks::<6>().0 {
            p.stroke_to(b, q[0], q[1], q[2], q[3], q[4], q[5] as f64);
        }
        p.end_stroke(b);
    }

    /// a layer's painted tiles (left-over transparent ones aside)
    fn painted(p: &Painter, id: u32) -> TileMap {
        let l = &p.layers[p.index_of(id).unwrap()];
        l.surface
            .tiles
            .iter()
            .filter(|(_, t)| !transparent(t))
            .map(|(k, t)| (*k, t.clone()))
            .collect()
    }

    /// every tile of a `w`×`h` canvas, as `lines_begin` takes them
    fn tiles_of(w: u32, h: u32) -> Vec<i32> {
        let ts = TILE_SIZE as u32;
        let (w, h) = (w.div_ceil(ts) as i32, h.div_ceil(ts) as i32);
        (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x, y]))
            .collect()
    }

    /// every tile of the 300×200 canvas the tests use
    fn all() -> Vec<i32> {
        tiles_of(300, 200)
    }

    fn keys(tiles: &TileMap) -> Vec<i32> {
        tiles.keys().flat_map(|k| [k.0, k.1]).collect()
    }

    /// redraw `tiles` of `layer` from `strokes` (seed, points) and push it
    fn redraw(p: &mut Painter, layer: u32, tiles: &[i32], b: &Brush, strokes: &[(u32, &[f32])]) {
        assert!(p.lines_begin(layer, tiles));
        for (seed, pts) in strokes {
            p.lines_stroke(b, *seed, pts);
        }
        assert!(p.lines_commit());
    }

    fn is_lines(p: &Painter, id: u32) -> bool {
        p.layers[p.index_of(id).unwrap()].props.lines
    }

    #[test]
    fn a_replayed_line_stroke_paints_the_same_pixels() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let pts = wave(20.0, 280.0, 100.0);
        live(&mut p, &b, 7, &pts);
        let drawn = painted(&p, a);
        assert!(drawn.len() > 3);
        let c = p.add_line_layer("");
        redraw(&mut p, c, &keys(&drawn), &b, &[(7, &pts)]);
        assert!(p.lines_push());
        assert_eq!(painted(&p, c), drawn);
        // another seed scatters differently
        let d = p.add_line_layer("");
        redraw(&mut p, d, &keys(&drawn), &b, &[(8, &pts)]);
        assert_ne!(painted(&p, d), drawn);
    }

    #[test]
    fn redrawing_some_tiles_leaves_the_others_and_skips_nothing_that_counts() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let pts = wave(20.0, 280.0, 100.0);
        live(&mut p, &b, 3, &pts);
        let before = painted(&p, a);
        // the moved stroke, drawn over the whole canvas on another layer
        let moved = shifted(&pts, 7.0, 20.0);
        let full = p.add_line_layer("");
        redraw(&mut p, full, &all(), &b, &[(3, &moved)]);
        p.lines_forget();
        let want = painted(&p, full);
        // just two tiles of the first layer, from the moved stroke
        let some = [(1, 1), (3, 2)];
        let flat: Vec<i32> = some.iter().flat_map(|k| [k.0, k.1]).collect();
        redraw(&mut p, a, &flat, &b, &[(3, &moved)]);
        let got = painted(&p, a);
        let mut keys: HashSet<TileKey> = before.keys().chain(want.keys()).copied().collect();
        keys.extend(some);
        for k in keys {
            let expect = if some.contains(&k) { &want } else { &before };
            assert_eq!(got.get(&k), expect.get(&k), "tile {k:?}");
        }
        assert!(some.iter().any(|k| got.get(k) != before.get(k)));
    }

    #[test]
    fn moving_a_stroke_is_one_undo_step() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let pts = wave(20.0, 200.0, 60.0);
        live(&mut p, &b, 1, &pts);
        let original = painted(&p, a);
        let steps = p.undo.len();
        // a drag: many redraws, from the original place to (40, 50) away
        for i in 1..=5 {
            let moved = shifted(&pts, 8.0 * i as f32, 10.0 * i as f32);
            redraw(&mut p, a, &all(), &b, &[(1, &moved)]);
            assert!(p.idle(), "pending isn't busy");
        }
        let moved = painted(&p, a);
        assert!(p.lines_pending());
        assert!(p.lines_push());
        assert!(!p.lines_pending() && !p.lines_push());
        assert_eq!(p.undo.len(), steps + 1);
        assert!(is_lines(&p, a));
        assert!(p.undo());
        assert_eq!(painted(&p, a), original);
        assert!(p.redo());
        assert_eq!(painted(&p, a), moved);
    }

    #[test]
    fn a_cancelled_drag_puts_the_lines_back() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let pts = wave(20.0, 200.0, 60.0);
        live(&mut p, &b, 1, &pts);
        let original = painted(&p, a);
        let steps = p.undo.len();
        redraw(&mut p, a, &all(), &b, &[(1, &shifted(&pts, 30.0, 30.0))]);
        redraw(&mut p, a, &all(), &b, &[]);
        assert!(painted(&p, a).is_empty());
        p.lines_cancel();
        assert_eq!(painted(&p, a), original);
        assert!(!p.lines_pending() && !p.lines_push());
        assert_eq!(p.undo.len(), steps);
        // undo in the middle of a drag cancels it too
        redraw(&mut p, a, &all(), &b, &[]);
        assert!(!p.undo());
        assert_eq!(painted(&p, a), original);
    }

    #[test]
    fn redraws_are_busy_and_refused_where_they_cant_be() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let c = p.add_line_layer("");
        assert!(!p.lines_begin(a, &[0, 0, 1]));
        assert!(p.lines_begin(a, &[0, 0]));
        assert!(!p.idle());
        assert!(!p.lines_begin(a, &[0, 0]), "one at a time");
        assert!(p.lines_commit());
        assert!(!p.lines_commit());
        // a line edit pending on another layer
        assert!(!p.lines_begin(c, &[0, 0]));
        p.lines_forget();
        assert!(p.lines_begin(c, &[0, 0]));
        p.lines_cancel();
        assert!(p.idle());
        // nothing begun: replaying is a no-op
        p.lines_stroke(&b, 1, &wave(0.0, 100.0, 50.0));
        assert!(!p.lines_pending());
        // locked layers refuse
        let mut props = p.layers[p.index_of(a).unwrap()].props.clone();
        props.locked = true;
        p.set_layer_props(a, props, true);
        assert!(!p.lines_begin(a, &[0, 0]));
        assert!(is_lines(&p, a), "settings changes keep the lines");
        assert!(!p.lines_begin(99, &[0, 0]));
    }

    #[test]
    fn other_paint_rasterizes_line_layers_and_undo_brings_them_back() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        live(&mut p, &b, 1, &wave(20.0, 200.0, 60.0));
        assert!(is_lines(&p, a));
        redraw(&mut p, a, &all(), &b, &[(2, &wave(20.0, 200.0, 90.0))]);
        p.lines_push();
        assert!(is_lines(&p, a));
        // a plain stroke
        p.begin_stroke();
        p.stroke_to(&b, 50.0, 50.0, 1.0, 0.0, 0.0, 0.01);
        p.stroke_to(&b, 90.0, 50.0, 1.0, 0.0, 0.0, 0.01);
        p.end_stroke(&b);
        assert!(!is_lines(&p, a));
        p.undo();
        assert!(is_lines(&p, a));
        // clearing
        assert!(p.clear());
        assert!(!is_lines(&p, a));
        p.undo();
        assert!(is_lines(&p, a));
        // merging a layer down into it
        let top = p.add_layer("");
        p.stroke_to(&b, 50.0, 150.0, 1.0, 0.0, 0.0, 0.01);
        p.end_stroke(&b);
        assert!(p.merge_down(top));
        assert!(!is_lines(&p, a));
        p.undo();
        assert!(is_lines(&p, a));
    }

    #[test]
    fn line_layers_come_and_go_in_single_steps() {
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        assert_eq!(p.layers.len(), 2);
        assert!(p.layers_json().contains(&format!(r#""name":"Lines {a}""#)));
        assert!(p.layers_json().contains(r#""lines":true"#));
        assert!(p.layers_json().contains(r#""lines":false"#));
        let copy = p.duplicate_layer(a);
        assert!(is_lines(&p, copy));
        p.undo();
        assert!(p.rasterize_lines(a));
        assert!(!p.rasterize_lines(a));
        assert!(!p.layers_json().contains(r#""lines":true"#));
        p.undo();
        assert!(is_lines(&p, a));
        p.undo();
        assert_eq!(p.layers.len(), 1);
        // silently, as opening a project does
        assert!(p.set_lines_silent(1, true));
        assert!(is_lines(&p, 1));
        assert!(!p.set_lines_silent(99, true));
    }

    #[test]
    fn line_strokes_ignore_the_selection_and_alpha_lock() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let mut props = p.layers[p.index_of(a).unwrap()].props.clone();
        props.alpha_lock = true;
        p.set_layer_props(a, props, true);
        // a selection far from the stroke
        let mut sel = vec![0u8; 300 * 200];
        sel[0] = 255;
        p.set_selection(&sel, 0);
        let pts = wave(100.0, 200.0, 100.0);
        live(&mut p, &b, 4, &pts);
        let drawn = painted(&p, a);
        assert!(!drawn.is_empty());
        let c = p.add_line_layer("");
        redraw(&mut p, c, &all(), &b, &[(4, &pts)]);
        assert_eq!(painted(&p, c), drawn);
    }

    #[test]
    fn symmetry_copies_are_seeded_one_after_the_other() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Vertical,
            cx: 150.0,
            cy: 100.0,
        }));
        let a = p.add_line_layer("");
        // (on one side of the axis, so the copy doesn't cross it)
        let pts = wave(20.0, 110.0, 100.0);
        live(&mut p, &b, 40, &pts);
        p.set_symmetry(None);
        let mirrored: Vec<f32> = pts
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|q| [300.0 - q[0], q[1], q[2], 0.0, 0.0, q[5]])
            .collect();
        let c = p.add_line_layer("");
        redraw(&mut p, c, &all(), &b, &[(40, &pts), (41, &mirrored)]);
        assert_eq!(painted(&p, c), painted(&p, a));
    }

    // ---- editable strokes on plain layers ----

    /// a plain (not line) stroke across the layer
    fn plain(p: &mut Painter, b: &Brush, y: f32) {
        assert!(p.begin_stroke());
        for i in 0..40 {
            p.stroke_to(b, 10.0 + i as f32 * 7.0, y, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(b);
    }

    fn flat(keys: &[TileKey]) -> Vec<i32> {
        keys.iter().flat_map(|k| [k.0, k.1]).collect()
    }

    /// tiles where either map has paint
    fn union_keys(a: &TileMap, b: &TileMap) -> Vec<i32> {
        let mut k: Vec<TileKey> = a.keys().chain(b.keys()).copied().collect();
        k.sort_unstable();
        k.dedup();
        flat(&k)
    }

    #[test]
    fn a_stroke_on_a_plain_layer_moves_over_whats_under_it() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        // paint that was there first (not editable), then two editable strokes crossing
        plain(&mut p, &b, 100.0);
        let a = wave(20.0, 200.0, 90.0);
        let c = wave(60.0, 280.0, 110.0);
        live(&mut p, &b, 1, &a);
        live(&mut p, &b, 2, &c);
        let d = p.layer_drawing(layer);
        assert!(p.base_tiles(d) > 0);
        let before = painted(&p, layer);
        // the same, drawn with the first stroke where it's moved to
        let moved = shifted(&a, 10.0, 25.0);
        let mut q = Painter::new(300, 200);
        let ql = q.active_layer();
        plain(&mut q, &b, 100.0);
        live(&mut q, &b, 1, &moved);
        live(&mut q, &b, 2, &c);
        let want = painted(&q, ql);
        // move it: redraw where it was and where it goes, from the base
        let tiles = union_keys(&before, &want);
        redraw(&mut p, layer, &tiles, &b, &[(1, &moved), (2, &c)]);
        assert!(p.lines_push());
        assert_eq!(painted(&p, layer), want, "the layer under it is as it was");
        // undo puts it back, and it can be moved again
        assert!(p.undo());
        assert_eq!(painted(&p, layer), before);
        redraw(&mut p, layer, &tiles, &b, &[(1, &moved), (2, &c)]);
        assert!(p.lines_push());
        assert_eq!(painted(&p, layer), want);
        // the second stroke deleted: the first and the old paint stay
        redraw(&mut p, layer, &tiles, &b, &[(1, &moved)]);
        p.lines_push();
        let mut r = Painter::new(300, 200);
        let rl = r.active_layer();
        plain(&mut r, &b, 100.0);
        live(&mut r, &b, 1, &moved);
        assert_eq!(painted(&p, layer), painted(&r, rl));
    }

    #[test]
    fn other_paint_on_strokes_takes_their_tiles_out_of_the_base_until_undone() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        live(&mut p, &b, 1, &wave(20.0, 280.0, 60.0));
        let d = p.layer_drawing(layer);
        let n = p.base_tiles(d);
        assert!(n > 0);
        assert!(p.take_tainted().is_empty(), "editable strokes don't taint");
        let steps = p.undo.len();
        plain(&mut p, &b, 60.0);
        assert_eq!(p.undo.len(), steps + 1, "still one step");
        let t = p.take_tainted();
        assert_eq!(t[0], d as i32);
        assert!(t[1] > 0 && t.len() == 2 + t[1] as usize * 2);
        assert!(p.base_tiles(d) < n);
        assert!(p.take_tainted().is_empty());
        p.undo();
        assert_eq!(p.base_tiles(d), n);
        assert!(p.take_tainted().is_empty(), "undo doesn't taint");
        p.redo();
        assert!(p.base_tiles(d) < n);
        // a line layer has no base
        let l = p.add_line_layer("");
        live(&mut p, &b, 3, &wave(20.0, 280.0, 60.0));
        assert_eq!(p.base_tiles(p.layer_drawing(l)), 0);
    }

    #[test]
    fn undoing_a_stroke_takes_its_tiles_out_of_the_base() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        live(&mut p, &b, 1, &wave(20.0, 280.0, 60.0));
        let d = p.layer_drawing(layer);
        assert!(p.base_tiles(d) > 0);
        p.undo();
        assert_eq!(p.base_tiles(d), 0);
        // plain paint there now, and a stroke over it: the base is that paint
        plain(&mut p, &b, 60.0);
        let under = painted(&p, layer);
        let s = wave(20.0, 280.0, 70.0);
        live(&mut p, &b, 2, &s);
        let all = union_keys(&under, &painted(&p, layer));
        redraw(&mut p, layer, &all, &b, &[]);
        p.lines_push();
        assert_eq!(painted(&p, layer), under);
    }

    #[test]
    fn strokes_baked_into_the_base_and_dropped_tiles_join_the_last_step() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        let a = wave(20.0, 200.0, 90.0);
        let c = wave(60.0, 280.0, 110.0);
        live(&mut p, &b, 1, &a);
        live(&mut p, &b, 2, &c);
        let d = p.layer_drawing(layer);
        let done = painted(&p, layer);
        let steps = p.undo.len();
        // the first stroke becomes plain pixels: replayed into the base
        let all = keys(&done);
        assert!(p.base_begin(d, layer, &all));
        p.lines_stroke(&b, 1, &a);
        assert!(p.lines_commit());
        assert_eq!(p.undo.len(), steps);
        // deleting the second now leaves the first, as pixels
        redraw(&mut p, layer, &all, &b, &[]);
        p.lines_push();
        let mut q = Painter::new(300, 200);
        let ql = q.active_layer();
        live(&mut q, &b, 1, &a);
        assert_eq!(painted(&p, layer), painted(&q, ql));
        p.undo();
        p.undo();
        // the base is as it was: deleting both clears the layer
        redraw(&mut p, layer, &all, &b, &[]);
        p.lines_push();
        assert!(painted(&p, layer).is_empty());
        p.undo();
        let n = p.base_tiles(d);
        assert!(p.push_note(1));
        assert!(p.base_drop(d, &all));
        assert_eq!(p.base_tiles(d), 0);
        assert!(!p.base_drop(d, &all));
        p.undo();
        assert_eq!(p.base_tiles(d), n);
    }

    #[test]
    fn bases_pack_copy_and_travel() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        plain(&mut p, &b, 60.0);
        live(&mut p, &b, 1, &wave(20.0, 280.0, 70.0));
        let d = p.layer_drawing(layer);
        let packed = p.base_packed(d);
        let mut q = Painter::new(300, 200);
        assert!(q.put_base_packed(5, &packed));
        assert_eq!(q.base_tiles(5), p.base_tiles(d));
        assert!(!q.put_base_packed(5, &packed[..packed.len() - 1]));
        // (8-bit in files: close, as the pixels are)
        let (pb, qb) = (&p.bases[&d], &q.bases[&5]);
        for (k, t) in pb {
            let (x, y) = (t.pixels(), qb[k].pixels());
            assert_eq!(x.is_some(), y.is_some());
            if let (Some(x), Some(y)) = (x, y) {
                for (a, b) in x.iter().flatten().zip(y.iter().flatten()) {
                    assert!((a[3] as i32 - b[3] as i32).abs() < 300);
                }
            }
        }
        // a copy of the drawing has it too
        let c = p.copy_drawing(d);
        assert_eq!(p.base_tiles(c), p.base_tiles(d));
        // changes travel: someone else's copy follows
        p.base_changes_packed(d);
        assert!(p.base_changes_packed(d).is_empty());
        plain(&mut p, &b, 70.0);
        let ch = p.base_changes_packed(d);
        assert!(!ch.is_empty());
        assert!(q.put_base_changes(5, &ch));
        assert_eq!(q.base_tiles(5), p.base_tiles(d));
        p.keep_bases(&[c]);
        assert_eq!(p.base_tiles(d), 0);
        assert!(p.base_tiles(c) > 0);
        assert!(p.base_bytes(0) > 0);
    }

    #[test]
    fn base_tiles_pack_small_and_come_back_the_same() {
        let mut t = empty_tile();
        assert_eq!(BaseTile::of(Some(&t)), BaseTile::Clear);
        for (i, row) in t.iter_mut().enumerate() {
            row[i] = [100, 200, 300, 32768];
            row[(i * 7) % 64] = [1, 2, 3, 4];
        }
        let b = BaseTile::of(Some(&t));
        assert!(matches!(b, BaseTile::Runs(_)));
        assert!(b.bytes() < RAW_BYTES / 4);
        assert_eq!(*b.pixels().unwrap(), *t);
        // noise doesn't shrink: kept as it is
        for (i, p) in t.iter_mut().flatten().enumerate() {
            *p = [i as u16, 0, 0, i as u16 * 3];
        }
        let b = BaseTile::of(Some(&t));
        assert!(matches!(b, BaseTile::Raw(_)));
        assert_eq!(*b.pixels().unwrap(), *t);
    }

    #[test]
    fn the_canvas_size_moves_bases_with_the_drawing() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        let a = wave(20.0, 200.0, 90.0);
        live(&mut p, &b, 1, &a);
        let d = p.layer_drawing(layer);
        assert!(p.resize_canvas(364, 264, 64, 64, false, &[]));
        // deleting the stroke (moved with the canvas) clears it
        assert!(p.base_tiles(d) > 0);
        let mut k = keys(&painted(&p, layer));
        k.extend(tiles_of(364, 264));
        redraw(&mut p, layer, &k, &b, &[]);
        p.lines_push();
        assert!(painted(&p, layer).is_empty());
        p.undo();
        p.undo();
        assert_eq!(p.width(), 300);
        assert!(p.base_tiles(d) > 0);
    }

    /// Memory and speed on a plain 1920×1080 layer: `cargo test -p pencilpilot-brush
    /// --release -- --ignored --nocapture plain_strokes_speed`.
    #[test]
    #[ignore]
    fn plain_strokes_speed() {
        let b = pen();
        for count in [200usize, 2000] {
            let mut p = Painter::new(1920, 1080);
            let layer = p.active_layer();
            let strokes: Vec<Vec<f32>> = (0..count)
                .map(|i| {
                    let x = 20.0 + (i * 37 % 1800) as f32;
                    let y = 20.0 + (i * 53 % 1000) as f32;
                    (0..80)
                        .flat_map(|j| {
                            let t = j as f32 / 79.0;
                            [
                                x + 120.0 * t,
                                y + 30.0 * (t * 6.0).sin(),
                                0.7,
                                0.0,
                                0.0,
                                0.008,
                            ]
                        })
                        .collect()
                })
                .collect();
            for (i, s) in strokes.iter().enumerate() {
                live(&mut p, &b, i as u32, s);
            }
            let d = p.layer_drawing(layer);
            // move stroke 0: redraw its old and new tiles from every stroke there
            let moved = shifted(&strokes[0], 30.0, 20.0);
            let mut list: Vec<(u32, &[f32])> = strokes
                .iter()
                .enumerate()
                .map(|(i, s)| (i as u32, s.as_slice()))
                .collect();
            list[0].1 = &moved;
            let tiles = vec![0, 0, 1, 0, 2, 0, 0, 1, 1, 1, 2, 1];
            let t = std::time::Instant::now();
            redraw(&mut p, layer, &tiles, &b, &list);
            p.lines_push();
            eprintln!(
                "{count} strokes: base {} tiles, {} KB; one edit {:?}",
                p.base_tiles(d),
                p.base_bytes(d) / 1024,
                t.elapsed()
            );
        }
    }

    /// How long redraws take: `cargo test -p brush --release -- --ignored --nocapture
    /// lines_redraw_speed`.
    #[test]
    #[ignore]
    fn lines_redraw_speed() {
        let b = pen();
        let mut p = Painter::new(2048, 2048);
        let a = p.add_line_layer("");
        let strokes: Vec<Vec<f32>> = (0..200)
            .map(|i| {
                let y = 10.0 + i as f32 * 10.0;
                (0..150)
                    .flat_map(|j| {
                        let t = j as f32 / 149.0;
                        [
                            10.0 + 2000.0 * t,
                            y + 20.0 * (t * 20.0).sin(),
                            0.7,
                            0.0,
                            0.0,
                            0.008,
                        ]
                    })
                    .collect()
            })
            .collect();
        let list: Vec<(u32, &[f32])> = strokes
            .iter()
            .enumerate()
            .map(|(i, s)| (i as u32, s.as_slice()))
            .collect();
        for (name, tiles) in [
            ("2 tiles", vec![10, 10, 11, 10]),
            ("whole canvas", tiles_of(2048, 2048)),
        ] {
            let t = std::time::Instant::now();
            redraw(&mut p, a, &tiles, &b, &list);
            eprintln!("200 strokes × 150 points into {name}: {:?}", t.elapsed());
        }
    }
}
