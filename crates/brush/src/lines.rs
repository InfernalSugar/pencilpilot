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

use crate::strands::{Pen, Strands};
use crate::{Edit, H_IDENTITY, Painter, Snapshot, TileKey, TileMap, TransformSession, UndoSurface};

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

/// How far past a dab a brush that smudges reads the paint around it, as a multiple of
/// the dab's radius (0: it doesn't smudge). Its strokes' redraws must take in every tile
/// that far away, drawn as it was (see `lines`'s smudge notes).
pub fn smudge_reach(b: &Brush) -> f32 {
    use hokusai::BrushSetting as S;
    let smudge = b.get(S::Smudge);
    let length = b.get(S::SmudgeLength);
    let active = (smudge.base_value != 0.0 || !smudge.inputs.is_empty())
        && (length.base_value < 1.0 || !length.inputs.is_empty());
    if !active {
        return 0.0;
    }
    // the most the sampling radius's log can reach with its inputs
    let r = b.get(S::SmudgeRadiusLog);
    let most = r.base_value
        + r.inputs
            .iter()
            .map(|m| m.points.iter().map(|p| p.1.abs()).fold(0.0, f32::max))
            .sum::<f32>();
    most.exp().max(1.0)
}

/// The most a setting reaches: its base and every input's largest output.
fn most(b: &Brush, s: hokusai::BrushSetting) -> f32 {
    let v = b.get(s);
    v.base_value.abs()
        + v.inputs
            .iter()
            .map(|m| m.points.iter().map(|p| p.1.abs()).fold(0.0, f32::max))
            .sum::<f32>()
}

/// How far from the path a brush's strokes can paint, or read paint to smudge, as a
/// multiple of its radius: its dabs grow with their inputs and random size, move with
/// their offsets and tracking noise, and smudging reads around them. A redraw must take
/// in every tile that far from a stroke for it to come out as it was painted.
pub fn paint_reach(b: &Brush) -> f32 {
    use hokusai::BrushSetting as S;
    let grow = (most(b, S::Radius) - b.get(S::Radius).base_value.abs()).max(0.0)
        + most(b, S::RadiusByRandom);
    let dab = grow.exp();
    let offsets = (most(b, S::OffsetByRandom)
        + most(b, S::OffsetX)
        + most(b, S::OffsetY)
        + most(b, S::OffsetAngle)
        + most(b, S::OffsetAngleAsc)
        + most(b, S::OffsetAngleView)
        + most(b, S::OffsetAngle2)
        + most(b, S::OffsetAngle2Asc)
        + most(b, S::OffsetAngle2View))
        * most(b, S::OffsetMultiplier).exp()
        + most(b, S::OffsetBySpeed)
        + most(b, S::TrackingNoise);
    let pick = smudge_reach(b).max(1.0);
    // (summed, the inputs' extremes can make it absurd: past this, the whole canvas)
    (dab * (offsets + pick) + 1.0).min(MOST_REACH)
}

/// The farthest `paint_reach` goes: any farther is the whole canvas anyway.
pub const MOST_REACH: f32 = 1000.0;

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
#[derive(Clone)]
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
    /// strokes of one group (made together with symmetry), waiting to be replayed
    /// point by point together, as they were painted (see `lines_stroke`)
    queue: Vec<Queued>,
    queue_group: u32,
    /// replaying into the last transform's stash (`xf_without_begin`)
    into_stash: bool,
    /// replaying the strokes to lift onto nothing (`lines_lift_begin`)
    into_lift: bool,
}

/// The last transform kept (see `transform_commit_keeping`), for editable strokes to
/// follow it: the layer as it was, its base then, what was lifted (the box, and the
/// selection), and the matrix.
pub(crate) struct XfStash {
    pub(crate) layer: u32,
    pub(crate) drawing: u32,
    pub(crate) original: TileMap,
    pub(crate) base: Base,
    pub(crate) m: [f32; 9],
    /// the lifted box: x0, y0, width, height (canvas px)
    pub(crate) lifted: (i64, i64, usize, usize),
    pub(crate) sel: Option<Vec<u8>>,
    /// the layer as it was without the strokes that follow the transform (their tiles)
    pub(crate) without: TileMap,
}

/// A stroke waiting in a group: its brush, seed, points and dab style; the dabs it
/// draws (a piece: `lines_span`) and the erasers it carries (`lines_clip`).
struct Queued {
    brush: Brush,
    seed: u32,
    pts: Vec<f32>,
    style: Option<std::sync::Arc<crate::texture::DabStyle>>,
    strands: Option<Strands>,
    span: Option<(u32, u32)>,
    clips: Vec<Clip>,
}

/// An eraser stroke that erases only the stroke carrying it (see `lines_clip`).
struct Clip {
    brush: Brush,
    seed: u32,
    pts: Vec<f32>,
    style: Option<std::sync::Arc<crate::texture::DabStyle>>,
    strands: Option<Strands>,
}

impl Redraw {
    fn new(layer: u32, scratch: UndoSurface, into_base: Option<u32>) -> Self {
        Self {
            layer,
            scratch: UndoSurface {
                fixed_sampling: true,
                ..scratch
            },
            into_base,
            queue: Vec::new(),
            queue_group: 0,
            into_stash: false,
            into_lift: false,
        }
    }

    /// Replay the strokes waiting: point by point, each stroke in turn (as symmetry
    /// paints its copies), then each finished. A piece draws only its span of the
    /// stroke's dabs (the rest move its brush as they did). A stroke carrying erasers
    /// is painted on its own (see `clipped`).
    fn flush(&mut self) {
        let queue = std::mem::take(&mut self.queue);
        self.queue_group = 0;
        if queue.iter().any(|q| !q.clips.is_empty()) {
            for q in &queue {
                self.clipped(q);
            }
            return;
        }
        let mut states: Vec<Pen> = queue.iter().map(|q| Pen::new(q.seed, q.strands)).collect();
        let mut counts = vec![0u32; queue.len()];
        let most = queue.iter().map(|q| q.pts.len() / 6).max().unwrap_or(0);
        let s = &mut self.scratch;
        for i in 0..most {
            for (k, (q, st)) in queue.iter().zip(states.iter_mut()).enumerate() {
                let Some(p) = q.pts.get(i * 6..i * 6 + 6) else {
                    continue;
                };
                s.style = q.style.clone();
                (s.span, s.dab_no) = (q.span, counts[k]);
                st.stroke_to(&q.brush, s, p);
                counts[k] = s.dab_no;
            }
        }
        for (k, (q, st)) in queue.iter().zip(states.iter_mut()).enumerate() {
            s.style = q.style.clone();
            (s.span, s.dab_no) = (q.span, counts[k]);
            st.finish(&q.brush, s);
        }
        s.span = None;
    }

    /// A stroke carrying erasers: painted alone (on nothing), its erasers erase it
    /// there, and the result goes over the tiles being redrawn.
    fn clipped(&mut self, q: &Queued) {
        let mut own = UndoSurface {
            style: q.style.clone(),
            only: self.scratch.only.clone(),
            fixed_sampling: true,
            ..Default::default()
        };
        let mut st = Pen::new(q.seed, q.strands);
        own.span = q.span;
        for p in q.pts.as_chunks::<6>().0 {
            st.stroke_to(&q.brush, &mut own, p);
        }
        st.finish(&q.brush, &mut own);
        own.span = None;
        for c in &q.clips {
            own.style = c.style.clone();
            let mut st = Pen::new(c.seed, c.strands);
            for p in c.pts.as_chunks::<6>().0 {
                st.stroke_to(&c.brush, &mut own, p);
            }
            st.finish(&c.brush, &mut own);
        }
        for (k, t) in own.tiles {
            if self
                .scratch
                .only
                .as_ref()
                .is_some_and(|a| !a.tiles.contains(&k))
            {
                continue;
            }
            let dst = self.scratch.tiles.entry(k).or_insert_with(empty_tile);
            for (d, s) in dst.iter_mut().flatten().zip(t.iter().flatten()) {
                if s[3] > 0 {
                    *d = crate::merge_pixel(*d, *s, 1.0, crate::Blend::Normal);
                }
            }
        }
    }
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
        self.begin_strands(seed);
        // (smudge sampling as a replay will do it)
        let active = self.active;
        if let Some(l) = self.layer_mut(active) {
            l.surface.fixed_sampling = true;
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
        self.line_redraw = Some(Box::new(Redraw::new(
            layer,
            UndoSurface {
                tiles: start,
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            None,
        )));
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
        self.line_redraw = Some(Box::new(Redraw::new(
            layer,
            UndoSurface {
                tiles: start,
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            Some(drawing),
        )));
        true
    }

    /// Tiles a stroke that smudges read from without painting them (its pick-up reaches
    /// past its paint) join the drawing's base as they are now, so a replay reads what
    /// it read (joins the last undo step). The tiles: (x, y) pairs.
    pub fn base_cover(&mut self, drawing: u32, tiles: &[i32]) -> bool {
        if drawing == 0 || !tiles.len().is_multiple_of(2) {
            return false;
        }
        let Some(px) = self.drawing_tiles(drawing) else {
            return false;
        };
        let base = self.bases.get(&drawing);
        let snap: BaseSnap = tiles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .filter(|k| !base.is_some_and(|b| b.contains_key(k)))
            .map(|k| (k, Some(BaseTile::of(px.get(&k).map(|t| &**t)))))
            .collect();
        if snap.is_empty() {
            return false;
        }
        let old = self.base_swap(drawing, snap);
        self.join_last(Edit::Base { drawing, snap: old });
        true
    }

    // ---- editable strokes following a transform ----

    /// The last transform kept was of drawing `drawing` (0: none, or another).
    pub fn xf_drawing(&self) -> u32 {
        self.xf_stash.as_ref().map_or(0, |s| s.drawing)
    }

    /// Start working out the layer as it was before the last transform without its
    /// editable strokes but those that became plain pixels with it: on every tile its
    /// base then had, from that base, the strokes that became plain pixels are replayed
    /// (`lines_stroke`…, `lines_commit`). Not an undo step. False without a transform
    /// kept, or busy.
    pub fn xf_without_begin(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        let Some(st) = self.xf_stash.as_ref() else {
            return false;
        };
        let keys: HashSet<TileKey> = st.base.keys().copied().collect();
        let start: TileMap = st
            .base
            .iter()
            .filter_map(|(k, b)| b.pixels().map(|t| (*k, t)))
            .collect();
        let mut r = Redraw::new(
            st.layer,
            UndoSurface {
                tiles: start,
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            None,
        );
        r.into_stash = true;
        self.line_redraw = Some(Box::new(r));
        true
    }

    /// The pixels under the strokes that followed the last transform, at their new
    /// `tiles`: the layer as it was without them (see `xf_without_begin`), moved as the
    /// transform moved what it lifted, become the drawing's base there (joins the last
    /// undo step). The stash goes. False without one.
    #[allow(clippy::needless_range_loop)] // (pixels by row and column, mapped to the canvas)
    pub fn xf_base(&mut self, tiles: &[i32]) -> bool {
        let Some(st) = self.xf_stash.take() else {
            return false;
        };
        let Some(inv) = crate::h_invert(st.m) else {
            return false;
        };
        let (w, h) = (self.width as i64, self.height as i64);
        let ts = TILE_SIZE as i64;
        let (fx0, fy0, fw, fh) = st.lifted;
        let at = |cx: i64, cy: i64| -> [u16; 4] {
            let key = (cx.div_euclid(ts) as i32, cy.div_euclid(ts) as i32);
            st.without
                .get(&key)
                .or_else(|| st.original.get(&key))
                .map_or([0; 4], |t| {
                    t[cy.rem_euclid(ts) as usize][cx.rem_euclid(ts) as usize]
                })
        };
        // lifted as `transform_begin` lifted (its paint in the box, inside the selection)
        let lifted = |cx: i64, cy: i64| {
            cx >= fx0
                && cy >= fy0
                && cx < fx0 + fw as i64
                && cy < fy0 + fh as i64
                && st.sel.as_ref().is_none_or(|s| {
                    cx >= 0 && cy >= 0 && cx < w && cy < h && s[(cy * w + cx) as usize] > 0
                })
        };
        let float = |xi: i64, yi: i64| -> [f32; 4] {
            if lifted(xi, yi) {
                at(xi, yi).map(|v| v as f32)
            } else {
                [0.0; 4]
            }
        };
        let mut snap = BaseSnap::new();
        for &[tx, ty] in tiles.as_chunks::<2>().0 {
            let mut tile = empty_tile();
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let (cx, cy) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                    let rest = if lifted(cx, cy) { [0; 4] } else { at(cx, cy) };
                    // (as `render_float` samples: bilinear, premultiplied)
                    let mut s = [0u16; 4];
                    if cx >= 0
                        && cy >= 0
                        && cx < w
                        && cy < h
                        && let Some((sx, sy)) =
                            crate::h_apply(inv, cx as f32 + 0.5, cy as f32 + 0.5)
                    {
                        let (x, y) = (sx - 0.5, sy - 0.5);
                        let (x0, y0) = (x.floor(), y.floor());
                        let (dx, dy) = (x - x0, y - y0);
                        let (x0, y0) = (x0 as i64, y0 as i64);
                        let (p00, p10, p01, p11) = (
                            float(x0, y0),
                            float(x0 + 1, y0),
                            float(x0, y0 + 1),
                            float(x0 + 1, y0 + 1),
                        );
                        for k in 0..4 {
                            let v = p00[k] * (1.0 - dx) * (1.0 - dy)
                                + p10[k] * dx * (1.0 - dy)
                                + p01[k] * (1.0 - dx) * dy
                                + p11[k] * dx * dy;
                            s[k] = v.round().clamp(0.0, 32768.0) as u16;
                        }
                    }
                    tile[py][px] = if s[3] > 0 {
                        crate::merge_pixel(rest, s, 1.0, crate::Blend::Normal)
                    } else {
                        rest
                    };
                }
            }
            snap.insert((tx, ty), Some(BaseTile::of(Some(&tile))));
        }
        if snap.is_empty() {
            return true;
        }
        let old = self.base_swap(st.drawing, snap);
        self.join_last(Edit::Base {
            drawing: st.drawing,
            snap: old,
        });
        true
    }

    /// Forget the last transform kept.
    pub fn xf_forget(&mut self) {
        self.xf_stash = None;
    }

    // ---- strokes lifted while they're dragged ----

    /// Start lifting strokes of `layer` (a drag of heavy strokes): the strokes that
    /// follow (`lines_stroke`…, then `lines_commit`) are drawn onto nothing over `tiles`
    /// (as `lines_begin`) and kept as a float that `lines_lift_preview` slides over the
    /// layer as pixels, without drawing a dab, till `lines_lift_end`. The app takes the
    /// strokes off the layer first and draws them exactly where they land after.
    pub fn lines_lift_begin(&mut self, layer: u32, tiles: &[i32]) -> bool {
        if !self.idle() || !tiles.len().is_multiple_of(2) {
            return false;
        }
        let Some(l) = self.index_of(layer).map(|i| &self.layers[i]) else {
            return false;
        };
        if l.props.locked || l.props.adjust.is_some() || l.editing_mask {
            return false;
        }
        let keys: HashSet<TileKey> = tiles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .collect();
        let mut r = Redraw::new(
            layer,
            UndoSurface {
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
            None,
        );
        r.into_lift = true;
        self.line_redraw = Some(Box::new(r));
        true
    }

    /// The lifted strokes' pixels (`tiles`) become the float (false: nothing to lift).
    fn lift_float(&mut self, layer: u32, tiles: &TileMap) -> bool {
        let ts = TILE_SIZE as i64;
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (&(tx, ty), tile) in tiles {
            for (py, row) in tile.iter().enumerate() {
                for (px, p) in row.iter().enumerate() {
                    if p[3] > 0 {
                        let (cx, cy) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                        x0 = x0.min(cx);
                        y0 = y0.min(cy);
                        x1 = x1.max(cx);
                        y1 = y1.max(cy);
                    }
                }
            }
        }
        if x0 > x1 {
            return false;
        }
        let (fw, fh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let mut float = vec![[0u16; 4]; fw * fh];
        for (&(tx, ty), tile) in tiles {
            for (py, row) in tile.iter().enumerate() {
                for (px, p) in row.iter().enumerate() {
                    let (cx, cy) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                    if p[3] > 0 {
                        float[((cy - y0) as usize) * fw + (cx - x0) as usize] = *p;
                    }
                }
            }
        }
        self.line_lift = Some(Box::new(TransformSession {
            layer,
            original: TileMap::new(),
            base: TileMap::new(),
            float,
            fx0: x0,
            fy0: y0,
            fw,
            fh,
            last: None,
            m: H_IDENTITY,
            cover: None,
            under: Some(HashMap::new()),
        }));
        true
    }

    /// Whether strokes are lifted (`lines_lift_begin`).
    pub fn lifting(&self) -> bool {
        self.line_lift.is_some()
    }

    /// Show the lifted strokes moved by `m` (dest = a·x + c·y + e, b·x + d·y + f) over
    /// the layer as it is under them. False if none are lifted.
    pub fn lines_lift_preview(&mut self, m: [f32; 6]) -> bool {
        let Some(mut t) = self.line_lift.take() else {
            return false;
        };
        let [a, b, c, d, e, f] = m;
        let ok = self.preview_float(&mut t, [a, c, e, b, d, f, 0.0, 0.0, 1.0]);
        self.line_lift = Some(t);
        ok
    }

    /// Drop the lifted strokes: every tile they passed over goes back to how it was.
    pub fn lines_lift_end(&mut self) {
        let Some(t) = self.line_lift.take() else {
            return;
        };
        let Some(li) = self.index_of(t.layer) else {
            return;
        };
        let tiles = &mut self.layers[li].surface.tiles;
        for (key, was) in t.under.unwrap_or_default() {
            match was {
                Some(p) => tiles.insert(key, p),
                None => tiles.remove(&key),
            };
            self.dirty.insert(key);
        }
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
    /// state seeded with `seed`. Nothing without a redraw begun. (It's painted when the
    /// next stroke comes or the redraw ends, so `lines_span` and `lines_clip` can still
    /// change it.)
    ///
    /// `group` (0: none) gathers strokes made together (symmetry's copies, given one
    /// after the other with the same number): they're replayed point by point
    /// together, as they were painted, when the group ends. (It matters for brushes
    /// that smudge: each copy picks up the paint the others just put down.)
    pub fn lines_stroke(&mut self, brush: &Brush, seed: u32, pts: &[f32], group: u32) {
        let (style, strands) = (self.style.clone(), self.strands);
        let Some(r) = self.line_redraw.as_mut() else {
            return;
        };
        if group == 0 || group != r.queue_group {
            r.flush();
        }
        r.queue_group = group;
        let pts = pts[..pts.len() / 6 * 6].to_vec();
        r.queue.push(Queued {
            brush: brush.clone(),
            seed,
            pts,
            style,
            strands,
            span: None,
            clips: Vec::new(),
        });
    }

    /// The stroke just given (`lines_stroke`) is a piece of a stroke the eraser cut:
    /// only its dabs `from..to` (counted in the order it paints them, as `lines_dabs`
    /// lists them) are drawn; the others move the brush as they did, so the dabs fall
    /// exactly where the whole stroke put them. (For brushes that don't smudge: one
    /// that does reads paint the dabs not drawn never put down.)
    pub fn lines_span(&mut self, from: u32, to: u32) {
        if let Some(q) = self.line_redraw.as_mut().and_then(|r| r.queue.last_mut()) {
            q.span = Some((from, to));
        }
    }

    /// The stroke just given (`lines_stroke`) carries an eraser stroke (`brush` set to
    /// erase, the dab style set now, `seed` and `pts` as `lines_stroke`'s) that erases
    /// only it: it's painted on its own, erased, then laid over the rest (a line cut
    /// by the eraser and moved away keeps its cut ends).
    pub fn lines_clip(&mut self, brush: &Brush, seed: u32, pts: &[f32]) {
        let (style, strands) = (self.style.clone(), self.strands);
        if let Some(q) = self.line_redraw.as_mut().and_then(|r| r.queue.last_mut()) {
            q.clips.push(Clip {
                brush: brush.clone(),
                seed,
                pts: pts[..pts.len() / 6 * 6].to_vec(),
                style,
                strands,
            });
        }
    }

    /// Where a stroke's dabs fall (`brush`, `seed`, `pts` as `lines_stroke` takes
    /// them), nothing drawn: for each dab, the point whose stroke-to painted it, then
    /// its x, y and radius (4 numbers a dab). Brushes trail behind the pen, so a point's
    /// dabs can lie well back along the line (the eraser's cuts go by them). For brushes
    /// that don't smudge (one that does would read paint this doesn't have).
    pub fn lines_dabs(&self, brush: &Brush, seed: u32, pts: &[f32]) -> Vec<f32> {
        let mut s = UndoSurface {
            probe: Some(Vec::new()),
            fixed_sampling: true,
            ..Default::default()
        };
        let mut st = Pen::new(seed, self.strands);
        let mut out = Vec::new();
        let flush = |s: &mut UndoSurface, i: usize, out: &mut Vec<f32>| {
            for d in s
                .probe
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
                .chunks(3)
            {
                out.extend([i as f32, d[0], d[1], d[2]]);
            }
        };
        let points = pts.as_chunks::<6>().0;
        for (i, p) in points.iter().enumerate() {
            st.stroke_to(brush, &mut s, p);
            flush(&mut s, i, &mut out);
        }
        st.finish(brush, &mut s);
        flush(&mut s, points.len().saturating_sub(1), &mut out);
        out
    }

    /// End the redraw begun with `lines_begin` without keeping it: how many pixels of
    /// its tiles come out other than the layer has them now (0: the strokes given
    /// paint exactly what's there). Nothing changes.
    pub fn lines_diff(&mut self) -> u32 {
        let Some(mut r) = self.line_redraw.take() else {
            return u32::MAX;
        };
        r.flush();
        let Some(area) = r.scratch.only.take() else {
            return u32::MAX;
        };
        if r.into_base.is_some() || r.into_stash || r.into_lift {
            return u32::MAX;
        }
        let Some(l) = self.index_of(r.layer).map(|i| &self.layers[i]) else {
            return u32::MAX;
        };
        let clear = empty_tile();
        let mut n = 0u32;
        for k in &area.tiles {
            let a = r.scratch.tiles.get(k).map_or(&*clear, |t| &**t);
            let b = l.surface.tiles.get(k).map_or(&*clear, |t| &**t);
            n += a
                .iter()
                .flatten()
                .zip(b.iter().flatten())
                .filter(|(x, y)| x != y && (x[3] != 0 || y[3] != 0))
                .count() as u32;
        }
        n
    }

    /// The redrawn tiles replace the layer's (fully transparent ones are dropped).
    /// Not an undo step yet: the tiles as they were before the first commit are kept
    /// until `lines_push` (or `lines_forget` / `lines_cancel`). False if no redraw was
    /// begun.
    pub fn lines_commit(&mut self) -> bool {
        let Some(r) = self.line_redraw.take() else {
            return false;
        };
        let mut r = r;
        r.flush();
        let Redraw {
            layer,
            mut scratch,
            into_base,
            into_stash,
            into_lift,
            ..
        } = *r;
        let Some(area) = scratch.only.take() else {
            return false;
        };
        if into_lift {
            return self.lift_float(layer, &scratch.tiles);
        }
        if into_stash {
            let Some(st) = self.xf_stash.as_mut() else {
                return false;
            };
            for key in area.tiles {
                let t = scratch.tiles.remove(&key).unwrap_or_else(empty_tile);
                st.without.insert(key, t);
            }
            return true;
        }
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
        self.lines_lift_end();
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
            p.lines_stroke(b, *seed, pts, 0);
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
    fn lifted_strokes_slide_as_pixels_and_leave_the_layer_as_it_was() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        let stay = wave(20.0, 280.0, 50.0);
        let go = wave(30.0, 200.0, 120.0);
        live(&mut p, &b, 3, &stay);
        live(&mut p, &b, 5, &go);
        // the app takes the strokes being dragged off the layer…
        redraw(&mut p, a, &all(), &b, &[(3, &stay)]);
        let without = painted(&p, a);
        // …and lifts them
        assert!(p.lines_lift_begin(a, &all()));
        p.lines_stroke(&b, 5, &go, 0);
        assert!(p.lines_commit());
        assert!(p.lifting());
        assert!(!p.idle(), "nothing else while strokes are lifted");
        assert!(!p.lines_begin(a, &all()));
        assert_eq!(painted(&p, a), without, "lifting alone changes nothing");
        // moved by whole pixels: what drawing them there gives
        assert!(p.lines_lift_preview([1.0, 0.0, 0.0, 1.0, 7.0, 20.0]));
        let slid = painted(&p, a);
        let mut q = Painter::new(300, 200);
        let r = q.add_line_layer("");
        redraw(
            &mut q,
            r,
            &all(),
            &b,
            &[(3, &stay), (5, &shifted(&go, 7.0, 20.0))],
        );
        assert_eq!(differing(&slid, &painted(&q, r), 8), 0);
        // a turn, then let go: every tile it passed over is back
        assert!(p.lines_lift_preview([0.9, 0.3, -0.3, 0.9, 40.0, -10.0]));
        assert_ne!(painted(&p, a), without);
        p.lines_lift_end();
        assert!(!p.lifting());
        assert!(p.idle());
        assert_eq!(painted(&p, a), without);
        // a cancelled drag drops them too
        assert!(p.lines_lift_begin(a, &all()));
        p.lines_stroke(&b, 5, &go, 0);
        assert!(p.lines_commit());
        assert!(p.lines_lift_preview([1.0, 0.0, 0.0, 1.0, -30.0, 0.0]));
        p.lines_cancel();
        assert!(!p.lifting());
        // (and the redraw without them, still pending, with it)
        assert_eq!(painted(&p, a).len(), {
            let mut q = Painter::new(300, 200);
            let c = q.add_line_layer("");
            live(&mut q, &b, 3, &stay);
            live(&mut q, &b, 5, &go);
            painted(&q, c).len()
        });
    }

    #[test]
    fn nothing_to_lift_lifts_nothing() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let a = p.add_line_layer("");
        assert!(p.lines_lift_begin(a, &[0, 0]));
        // (a stroke far from the tile drawn: nothing lands on it)
        p.lines_stroke(&b, 5, &wave(200.0, 290.0, 180.0), 0);
        assert!(!p.lines_commit());
        assert!(!p.lifting());
        assert!(p.idle());
        assert!(!p.lines_lift_preview([1.0, 0.0, 0.0, 1.0, 1.0, 1.0]));
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
        p.lines_stroke(&b, 1, &wave(0.0, 100.0, 50.0), 0);
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

    #[test]
    fn a_stroke_in_strands_replays_pixel_for_pixel() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        p.set_strands(7, 14.0, 0.8, 0.9);
        let a = p.add_line_layer("");
        let pts = wave(20.0, 280.0, 100.0);
        live(&mut p, &b, 11, &pts);
        let drawn = painted(&p, a);
        assert!(drawn.len() > 3);
        // the whole stroke again
        let c = p.add_line_layer("");
        redraw(&mut p, c, &keys(&drawn), &b, &[(11, &pts)]);
        assert!(p.lines_push());
        assert_eq!(painted(&p, c), drawn);
        // a few of its tiles only
        let some: Vec<i32> = keys(&drawn).into_iter().take(4).collect();
        assert!(p.lines_begin(a, &some));
        p.lines_stroke(&b, 11, &pts, 0);
        assert_eq!(p.lines_diff(), 0);
        // the dabs it lists are the dabs it draws: a piece of all of them is the same
        let n = (p.lines_dabs(&b, 11, &pts).len() / 4) as u32;
        assert!(n > 100);
        assert!(p.lines_begin(a, &keys(&drawn)));
        p.lines_stroke(&b, 11, &pts, 0);
        p.lines_span(0, n);
        assert_eq!(p.lines_diff(), 0);
        // other settings (or none) paint something else
        p.set_strands(0, 0.0, 0.0, 0.0);
        assert!(p.lines_begin(a, &keys(&drawn)));
        p.lines_stroke(&b, 11, &pts, 0);
        assert!(p.lines_diff() > 0);
        p.set_strands(7, 14.0, 0.8, 0.5);
        assert!(p.lines_begin(a, &keys(&drawn)));
        p.lines_stroke(&b, 11, &pts, 0);
        assert!(p.lines_diff() > 0);
    }

    #[test]
    fn strands_with_symmetry_replay_copy_by_copy() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        p.set_strands(5, 10.0, 0.6, 0.7);
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Vertical,
            cx: 150.0,
            cy: 100.0,
        }));
        let a = p.add_line_layer("");
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
        assert!(p.lines_begin(c, &all()));
        p.lines_stroke(&b, 40, &pts, 3);
        p.lines_stroke(&b, 41, &mirrored, 3);
        assert!(p.lines_commit());
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
        p.lines_stroke(&b, 1, &a, 0);
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

    // ---- brushes that smudge ----

    fn shipped(name: &str) -> Brush {
        // (the website's copies are in the repository; the app's are fetched at build)
        let path = format!(
            "{}/../../apps/web/static/brushes/{name}.myb",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut b = hokusai::myb::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        set_brush_rgb(&mut b, 200, 40, 30);
        b
    }

    /// the brush creator's "Blend with the paint underneath" on a round brush
    fn blending() -> Brush {
        let mut b = pen();
        b.get_mut(BrushSetting::Radius).base_value = 2.5;
        b.get_mut(BrushSetting::Smudge).base_value = 0.6;
        b.get_mut(BrushSetting::SmudgeLength).base_value = 0.5;
        b.get_mut(BrushSetting::SmudgeRadiusLog).base_value = 0.7;
        b
    }

    /// the tiles within `reach` px of a stroke's points
    fn near(pts: &[f32], reach: f32) -> Vec<i32> {
        let ts = TILE_SIZE as f32;
        let mut keys = HashSet::new();
        // (a 400×300 canvas and the ring of tiles round it)
        let tx = |v: f32| ((v / ts).floor() as i32).clamp(-1, 7);
        let ty = |v: f32| ((v / ts).floor() as i32).clamp(-1, 5);
        for p in pts.as_chunks::<6>().0 {
            let (x0, x1) = (tx(p[0] - reach), tx(p[0] + reach));
            let (y0, y1) = (ty(p[1] - reach), ty(p[1] + reach));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    keys.insert((x, y));
                }
            }
        }
        keys.into_iter().flat_map(|k| [k.0, k.1]).collect()
    }

    /// how far a brush's paint and pick-up reach past the path (px), as the app works it out
    fn reach_of(b: &Brush) -> f32 {
        b.get(BrushSetting::Radius).base_value.exp() * paint_reach(b).max(2.5) + 3.0
    }

    #[test]
    fn strokes_that_smudge_replay_exactly_over_the_paint_under_them() {
        for (name, b) in [
            ("blend", blending()),
            ("blend, mixing as pigment", {
                // (wide pick-up mixed as pigment: its sampling skips pixels at random)
                let mut b = blending();
                b.get_mut(BrushSetting::Paint).base_value = 1.0;
                b
            }),
            ("classic-blend-paint", shipped("classic-blend-paint")),
            ("classic-blending-knife", shipped("classic-blending-knife")),
            ("classic-impressionism", shipped("classic-impressionism")),
            ("dieterle-blender", shipped("dieterle-blender")),
            ("dieterle-fan-1", shipped("dieterle-fan-1")),
        ] {
            assert!(smudge_reach(&b) > 0.0, "{name} smudges");
            let mut p = Painter::new(400, 300);
            let layer = p.active_layer();
            // paint to smudge, of other colours, then some smudging first (the sampling's
            // random pattern moves on)
            let mut under = pen();
            set_brush_rgb(&mut under, 20, 60, 220);
            plain(&mut p, &under, 140.0);
            plain(&mut p, &b, 170.0);
            let first = wave(20.0, 380.0, 150.0);
            live(&mut p, &b, 5, &first);
            let second = wave(60.0, 340.0, 160.0);
            live(&mut p, &b, 6, &second);
            let drawn = painted(&p, layer);
            // every tile the strokes reach, redrawn from the base
            let reach = reach_of(&b);
            let mut tiles = near(&first, reach);
            tiles.extend(near(&second, reach));
            redraw(&mut p, layer, &tiles, &b, &[(5, &first), (6, &second)]);
            p.lines_push();
            assert_eq!(painted(&p, layer), drawn, "{name}: replayed as painted");
            // moved and moved back: as it was
            let moved = shifted(&second, 15.0, 20.0);
            let mut both = tiles.clone();
            both.extend(near(&moved, reach));
            redraw(&mut p, layer, &both, &b, &[(5, &first), (6, &moved)]);
            p.lines_push();
            assert_ne!(painted(&p, layer), drawn);
            redraw(&mut p, layer, &both, &b, &[(5, &first), (6, &second)]);
            p.lines_push();
            assert_eq!(painted(&p, layer), drawn, "{name}: moved back");
        }
    }

    #[test]
    fn smudging_symmetry_copies_replay_together_as_they_were_painted() {
        let b = blending();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        plain(&mut p, &pen(), 100.0);
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Vertical,
            cx: 150.0,
            cy: 100.0,
        }));
        // across the axis: each copy smudges into the other's fresh paint
        let pts = wave(60.0, 200.0, 100.0);
        live(&mut p, &b, 40, &pts);
        p.set_symmetry(None);
        let drawn = painted(&p, layer);
        let mirrored: Vec<f32> = pts
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|q| [300.0 - q[0], q[1], q[2], 0.0, 0.0, q[5]])
            .collect();
        let tiles = tiles_of(300, 200);
        assert!(p.lines_begin(layer, &tiles));
        p.lines_stroke(&b, 40, &pts, 7);
        p.lines_stroke(&b, 41, &mirrored, 7);
        assert!(p.lines_commit());
        p.lines_push();
        assert_eq!(painted(&p, layer), drawn);
    }

    #[test]
    fn a_smudging_strokes_pick_up_joins_the_base() {
        let b = blending();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        let s = wave(20.0, 280.0, 100.0);
        live(&mut p, &b, 3, &s);
        let d = p.layer_drawing(layer);
        let n = p.base_tiles(d);
        let around = tiles_of(300, 200);
        assert!(p.base_cover(d, &around));
        assert!(p.base_tiles(d) >= around.len() / 2);
        assert!(!p.base_cover(d, &around));
        p.undo();
        assert_eq!(p.base_tiles(d), 0);
        p.redo();
        assert!(p.base_tiles(d) >= around.len() / 2);
        assert!(n > 0);
        assert_eq!(smudge_reach(&pen()), 0.0);
        // a plain pen reaches about its radius and its scatter
        assert!((paint_reach(&pen()) - 2.6).abs() < 0.01);
    }

    // ---- strokes following a transform ----

    /// stroke points under an affine (a, b, c, d, e, f as canvas 2D takes it)
    fn mapped(pts: &[f32], m: [f32; 6]) -> Vec<f32> {
        pts.as_chunks::<6>()
            .0
            .iter()
            .flat_map(|q| {
                [
                    m[0] * q[0] + m[2] * q[1] + m[4],
                    m[1] * q[0] + m[3] * q[1] + m[5],
                    q[2],
                    q[3],
                    q[4],
                    q[5],
                ]
            })
            .collect()
    }

    /// Transform the active layer by `m` (with `sel`, a selection), as the app does with
    /// editable strokes `moving` following it: returns the tiles redrawn.
    fn transform_with_strokes(
        p: &mut Painter,
        b: &Brush,
        m: [f32; 6],
        all: &[(u32, Vec<f32>)],
        moving: &[usize],
    ) -> Vec<i32> {
        let layer = p.active_layer();
        assert!(p.transform_begin().len() == 4);
        assert!(p.transform_preview(m));
        assert!(p.transform_commit_keeping(false));
        assert_eq!(p.xf_drawing(), p.layer_drawing(layer));
        let _ = p.take_tainted();
        // the layer without its editable strokes (none became plain pixels: none replayed)
        let reach = b.get(BrushSetting::Radius).base_value.exp() * 2.5 + 3.0;
        assert!(p.xf_without_begin());
        assert!(p.lines_commit());
        // where they go: the base, then the strokes drawn there
        let now: Vec<(u32, Vec<f32>)> = all
            .iter()
            .enumerate()
            .map(|(i, (s, pts))| {
                (
                    *s,
                    if moving.contains(&i) {
                        mapped(pts, m)
                    } else {
                        pts.clone()
                    },
                )
            })
            .collect();
        let new: Vec<i32> = moving
            .iter()
            .flat_map(|&i| near(&now[i].1, reach))
            .collect();
        assert!(p.xf_base(&new));
        let list: Vec<(u32, &[f32])> = now.iter().map(|(s, v)| (*s, v.as_slice())).collect();
        redraw(p, layer, &new, b, &list);
        assert!(p.lines_push());
        assert!(p.join_last_edits());
        new
    }

    #[test]
    fn strokes_follow_a_transform_and_stay_editable() {
        let b = pen();
        let mut under = pen();
        set_brush_rgb(&mut under, 30, 160, 60);
        // turned a little, a bit bigger, moved
        let (c, s) = (0.2f32.cos() * 1.1, 0.2f32.sin() * 1.1);
        let m = [c, s, -s, c, 20.0, -10.0];
        for selected in [false, true] {
            let mut p = Painter::new(400, 300);
            let layer = p.active_layer();
            plain(&mut p, &under, 120.0);
            let a = wave(60.0, 200.0, 110.0);
            let d2 = wave(250.0, 380.0, 240.0);
            let all = vec![(1, a.clone()), (2, d2.clone())];
            live(&mut p, &b, 1, &a);
            live(&mut p, &b, 2, &d2);
            let before = painted(&p, layer);
            // the same plain paint, moved alone (what's under the strokes afterwards)
            let mut q = Painter::new(400, 300);
            let ql = q.active_layer();
            plain(&mut q, &under, 120.0);
            let mut sel = vec![0u8; 400 * 300];
            if selected {
                // the top part: the first stroke and the paint, not the second
                for v in &mut sel[..400 * 200] {
                    *v = 255;
                }
                p.set_selection(&sel, 0);
                q.set_selection(&sel, 0);
            }
            q.transform_begin();
            q.transform_preview(m);
            q.transform_commit();
            let moving: Vec<usize> = if selected { vec![0] } else { vec![0, 1] };
            let steps = p.undo.len();
            let new = transform_with_strokes(&mut p, &b, m, &all, &moving);
            assert_eq!(p.undo.len(), steps + 1, "one undo step");
            let moved = painted(&p, layer);
            // drawn again as they are: nothing changes (they're editable)
            let mut now = all.clone();
            for &i in &moving {
                now[i].1 = mapped(&all[i].1, m);
            }
            let list: Vec<(u32, &[f32])> = now.iter().map(|(s, v)| (*s, v.as_slice())).collect();
            redraw(&mut p, layer, &new, &b, &list);
            p.lines_push();
            assert_eq!(painted(&p, layer), moved, "selected: {selected}");
            // the moved strokes deleted: what's left is the paint, moved alone
            let rest: Vec<(u32, &[f32])> = list
                .iter()
                .enumerate()
                .filter(|(i, _)| !moving.contains(i))
                .map(|(_, x)| *x)
                .collect();
            redraw(&mut p, layer, &new, &b, &rest);
            p.lines_push();
            let got = painted(&p, layer);
            let want = painted(&q, ql);
            for k in new.as_chunks::<2>().0 {
                let k = (k[0], k[1]);
                if selected && k.1 >= 3 {
                    continue; // (the second stroke stays there)
                }
                assert_eq!(
                    got.get(&k),
                    want.get(&k),
                    "tile {k:?}, selected: {selected}"
                );
            }
            // undo: as before the transform
            p.undo();
            p.undo();
            p.undo();
            assert_eq!(painted(&p, layer), before);
        }
    }

    #[test]
    fn a_line_layer_can_stay_one_through_a_transform() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let l = p.add_line_layer("");
        live(&mut p, &b, 1, &wave(20.0, 200.0, 60.0));
        p.transform_begin();
        p.transform_preview([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
        assert!(p.transform_commit_keeping(true));
        assert!(is_lines(&p, l));
        p.xf_forget();
        assert_eq!(p.xf_drawing(), 0);
        p.transform_begin();
        p.transform_preview([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
        assert!(p.transform_commit());
        assert!(!is_lines(&p, l));
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

    // ---- the eraser cutting editable strokes ----

    /// a round eraser
    fn rubber(radius: f32, hardness: f32) -> Brush {
        let mut b = Brush::new();
        b.get_mut(BrushSetting::Radius).base_value = radius.ln();
        b.get_mut(BrushSetting::Opaque).base_value = 1.0;
        b.get_mut(BrushSetting::Hardness).base_value = hardness;
        b.get_mut(BrushSetting::DabsPerActualRadius).base_value = 3.0;
        b.get_mut(BrushSetting::Eraser).base_value = 1.0;
        b
    }

    /// a vertical eraser stroke at x, from y0 to y1
    fn down_at(x: f32, y0: f32, y1: f32) -> Vec<f32> {
        (0..30)
            .flat_map(|i| {
                let t = i as f32 / 29.0;
                [x, y0 + (y1 - y0) * t, 0.5 + 0.5 * t, 0.0, 0.0, 0.01]
            })
            .collect()
    }

    /// pixels of `a` and `b` farther apart than `tol` in any channel
    fn differing(a: &TileMap, b: &TileMap, tol: u16) -> usize {
        let clear = empty_tile();
        let mut k: Vec<TileKey> = a.keys().chain(b.keys()).copied().collect();
        k.sort_unstable();
        k.dedup();
        k.iter()
            .map(|k| {
                let (x, y) = (a.get(k).unwrap_or(&clear), b.get(k).unwrap_or(&clear));
                x.iter()
                    .flatten()
                    .zip(y.iter().flatten())
                    .filter(|(p, q)| p.iter().zip(q.iter()).any(|(u, v)| u.abs_diff(*v) > tol))
                    .count()
            })
            .sum()
    }

    #[test]
    fn a_stroke_replayed_in_pieces_paints_what_it_did_whole() {
        let b = pen();
        let mut p = Painter::new(300, 200);
        let layer = p.active_layer();
        let a = wave(20.0, 280.0, 100.0);
        live(&mut p, &b, 7, &a);
        let want = painted(&p, layer);
        // where its dabs fall: each once, along the line, from the points that painted them
        let dabs = p.lines_dabs(&b, 7, &a);
        let n = (dabs.len() / 4) as u32;
        assert!(n > 60);
        for d in dabs.as_chunks::<4>().0 {
            let i = d[0] as usize;
            assert!(i < 60);
            assert!((d[1] - a[i * 6]).abs() < 20.0 && (d[2] - a[i * 6 + 1]).abs() < 40.0);
        }
        // pieces: every dab once, where it was
        let (k1, k2) = (n / 3, n * 2 / 3);
        assert!(p.lines_begin(layer, &all()));
        for (from, to) in [(0, k1), (k1, k2), (k2, n)] {
            p.lines_stroke(&b, 7, &a, 0);
            p.lines_span(from, to);
        }
        assert_eq!(p.lines_diff(), 0);
        // and a piece left out leaves a gap
        assert!(p.lines_begin(layer, &all()));
        p.lines_stroke(&b, 7, &a, 0);
        p.lines_span(0, k1);
        p.lines_stroke(&b, 7, &a, 0);
        p.lines_span(k2, n);
        assert!(p.lines_diff() > 0);
        assert_eq!(painted(&p, layer), want, "nothing changed");
    }

    #[test]
    fn an_eraser_kept_as_a_stroke_replays_exactly_over_strokes_and_paint() {
        let b = pen();
        for (hard, opaque) in [(0.95, 1.0), (0.3, 1.0), (0.6, 0.5)] {
            let mut e = rubber(9.0, hard);
            e.get_mut(BrushSetting::Opaque).base_value = opaque;
            for lines in [false, true] {
                let mut p = Painter::new(300, 200);
                let layer = if lines {
                    p.add_line_layer("")
                } else {
                    p.active_layer()
                };
                if !lines {
                    plain(&mut p, &b, 100.0);
                }
                let a = wave(20.0, 280.0, 90.0);
                let c = wave(60.0, 280.0, 110.0);
                live(&mut p, &b, 1, &a);
                live(&mut p, &b, 2, &c);
                let cut = down_at(150.0, 40.0, 160.0);
                live(&mut p, &e, 3, &cut);
                assert_eq!(is_lines(&p, layer), lines, "a line layer stays one");
                // the strokes, then the eraser: what's there
                assert!(p.lines_begin(layer, &all()));
                p.lines_stroke(&b, 1, &a, 0);
                p.lines_stroke(&b, 2, &c, 0);
                p.lines_stroke(&e, 3, &cut, 0);
                assert_eq!(p.lines_diff(), 0, "hardness {hard}, opacity {opaque}");
                // the strokes cut in two (every dab kept), then the eraser: the same
                assert!(p.lines_begin(layer, &all()));
                for (seed, pts) in [(1, &a), (2, &c)] {
                    let n = (p.lines_dabs(&b, seed, pts).len() / 4) as u32;
                    p.lines_stroke(&b, seed, pts, 0);
                    p.lines_span(0, n / 2);
                    p.lines_stroke(&b, seed, pts, 0);
                    p.lines_span(n / 2, n);
                }
                p.lines_stroke(&e, 3, &cut, 0);
                assert_eq!(p.lines_diff(), 0);
                // without the eraser: not the same
                assert!(p.lines_begin(layer, &all()));
                p.lines_stroke(&b, 1, &a, 0);
                p.lines_stroke(&b, 2, &c, 0);
                assert!(p.lines_diff() > 0);
            }
        }
    }

    #[test]
    fn an_eraser_a_stroke_carries_erases_only_it_and_moves_with_it() {
        let b = pen();
        let e = rubber(9.0, 0.9);
        let a = wave(20.0, 280.0, 100.0);
        let cut = down_at(150.0, 40.0, 160.0);
        // on its own, erased: as the stroke then the eraser (but for rounding)
        let mut p = Painter::new(300, 200);
        let layer = p.add_line_layer("");
        live(&mut p, &b, 1, &a);
        live(&mut p, &e, 3, &cut);
        let want = painted(&p, layer);
        let mut q = Painter::new(300, 200);
        let ql = q.add_line_layer("");
        assert!(q.lines_begin(ql, &all()));
        q.lines_stroke(&b, 1, &a, 0);
        q.lines_clip(&e, 3, &cut);
        assert!(q.lines_commit());
        assert_eq!(differing(&want, &painted(&q, ql), 2), 0);
        // over other paint: only the stroke is erased
        let mut r = Painter::new(300, 200);
        let rl = r.active_layer();
        plain(&mut r, &b, 100.0);
        let under = painted(&r, rl);
        assert!(r.lines_begin(rl, &all()));
        r.lines_stroke(&b, 1, &shifted(&a, 0.0, 300.0), 0);
        r.lines_clip(&e, 3, &cut);
        assert!(r.lines_commit());
        assert_eq!(
            painted(&r, rl),
            under,
            "nothing of the plain line is erased"
        );
        // moved, it keeps its cut where it went
        let mut m = Painter::new(300, 200);
        let ml = m.add_line_layer("");
        assert!(m.lines_begin(ml, &all()));
        m.lines_stroke(&b, 1, &shifted(&a, 10.0, 20.0), 0);
        m.lines_clip(&e, 3, &shifted(&cut, 10.0, 20.0));
        assert!(m.lines_commit());
        let moved = painted(&m, ml);
        let alpha = |x: usize, y: usize| {
            moved
                .get(&((x / TILE_SIZE) as i32, (y / TILE_SIZE) as i32))
                .map_or(0, |t| t[y % TILE_SIZE][x % TILE_SIZE][3])
        };
        assert!((60..180).all(|y| alpha(160, y) == 0), "the cut moved along");
        assert!(!moved.is_empty());
    }
}
