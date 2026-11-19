//! PencilPilot painting engine.
//!
//! Wraps [hokusai](https://github.com/reearth/hokusai), a pure-Rust port of
//! libmypaint (the brush engine Krita also ships), and adds what a drawing
//! app needs on top:
//!
//! - **Layers** with opacity, blend modes, lock and alpha lock (see [`layers`]).
//! - **Undo / redo** of strokes and layer changes; strokes store only the tiles
//!   they touched.
//! - **Eraser mode** for any brush.
//! - **Dirty-rect flattening**: only changed tiles are converted to RGBA8, and
//!   the caller gets the rectangle to repaint.
//!
//! The engine logic lives in [`Painter`] (plain Rust, unit-tested natively);
//! [`wasm`] exposes it to JavaScript.

use std::collections::{HashMap, HashSet};

use hokusai::tile::empty_tile;
use hokusai::{Brush, BrushSetting, BrushState, TILE_SIZE, TilePixels, TiledSurface};

pub mod layers;
pub mod wasm;

pub use layers::{Blend, LayerProps};
use layers::{TileSource, composite_pixel, json_str, merge_pixel, premul_fix15, unpremul_srgb};

type TileKey = (i32, i32);
type TileMap = HashMap<TileKey, Box<TilePixels>>;
/// Tiles as they were before an edit. `None` = the tile did not exist.
type Snapshot = HashMap<TileKey, Option<Box<TilePixels>>>;

/// Maximum number of undo steps kept.
pub const UNDO_LIMIT: usize = 60;

/// Tile surface that can snapshot the tiles a stroke touches.
#[derive(Default)]
pub struct UndoSurface {
    tiles: TileMap,
    /// `Some` while a stroke is recording.
    recording: Option<Snapshot>,
    dirty: HashSet<TileKey>,
    /// tiles painted since the last `touched.clear()` (for clipping to a selection)
    touched: HashSet<TileKey>,
}

impl UndoSurface {
    fn remember(&mut self, key: TileKey) {
        if let Some(rec) = self.recording.as_mut() {
            rec.entry(key)
                .or_insert_with(|| self.tiles.get(&key).cloned());
        }
    }

    /// Swap the current tiles for `snap`, returning what was replaced.
    fn swap_in(&mut self, snap: Snapshot) -> Snapshot {
        let mut previous = Snapshot::with_capacity(snap.len());
        for (key, tile) in snap {
            let old = match tile {
                Some(t) => self.tiles.insert(key, t),
                None => self.tiles.remove(&key),
            };
            previous.insert(key, old);
            self.dirty.insert(key);
        }
        previous
    }
}

impl TiledSurface for UndoSurface {
    fn tile_request_start(&mut self, tx: i32, ty: i32) -> &mut TilePixels {
        self.remember((tx, ty));
        self.dirty.insert((tx, ty));
        self.touched.insert((tx, ty));
        self.tiles.entry((tx, ty)).or_insert_with(empty_tile)
    }

    fn tile_request_end(&mut self, _tx: i32, _ty: i32) {}

    fn tile_lookup(&self, tx: i32, ty: i32) -> Option<&TilePixels> {
        self.tiles.get(&(tx, ty)).map(|b| &**b)
    }
}

/// A rectangle in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Most layers a drawing can have.
pub const MAX_LAYERS: usize = 32;

/// One layer: its id, its properties and its pixels.
#[derive(Clone)]
pub struct Layer {
    pub id: u32,
    pub props: LayerProps,
    surface: UndoSurface,
}

impl Layer {
    fn new(id: u32, props: LayerProps) -> Self {
        Self {
            id,
            props,
            surface: UndoSurface::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.surface.tiles.is_empty()
    }
}

impl Clone for UndoSurface {
    fn clone(&self) -> Self {
        Self {
            tiles: self.tiles.clone(),
            recording: None,
            dirty: HashSet::new(),
            touched: HashSet::new(),
        }
    }
}

/// A transform in progress: the lifted pixels float above the rest of the layer.
struct TransformSession {
    layer: u32,
    /// the layer's tiles before the transform (for cancel / undo)
    original: TileMap,
    /// the layer without the lifted pixels
    base: TileMap,
    /// the lifted pixels (premultiplied), `fw`×`fh` from canvas (fx0, fy0)
    float: Vec<[u16; 4]>,
    fx0: i64,
    fy0: i64,
    fw: usize,
    fh: usize,
    /// destination box drawn by the last preview (to repaint it next time)
    last: Option<(i64, i64, i64, i64)>,
    /// current matrix: dest = (a x + c y + e, b x + d y + f)
    m: [f32; 6],
}

/// One undoable change. Applying an edit returns its inverse.
enum Edit {
    /// Swap tiles of one layer (strokes, clearing, merging).
    Tiles {
        layer: u32,
        snap: Snapshot,
    },
    Insert {
        index: usize,
        layer: Box<Layer>,
    },
    Remove {
        index: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    Props {
        layer: u32,
        props: LayerProps,
    },
    Batch(Vec<Edit>),
}

/// The canvas: a stack of layers (bottom → top) over white paper, the
/// in-flight stroke, history, and an RGBA8 (sRGB) output buffer.
pub struct Painter {
    layers: Vec<Layer>,
    active: u32,
    next_id: u32,
    state: BrushState,
    /// strokes from other people (shared canvases), drawn alongside the local
    /// one: key → (layer id, brush state). Never in the undo history.
    remote: HashMap<u32, (u32, BrushState)>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// the layer the current stroke paints on
    stroking: Option<u32>,
    /// alpha-locked strokes paint with a copy of the brush that has lock_alpha on
    stroke_brush: Option<Brush>,
    alpha_lock_stroke: bool,
    /// tiles to recomposite (structural changes); strokes use each surface's own set
    dirty: HashSet<TileKey>,
    /// the selection: 255 = selected, per canvas pixel (None = no selection)
    selection: Option<Vec<u8>>,
    /// a transform in progress (see `transform_begin`)
    transform: Option<Box<TransformSession>>,
    /// a liquify stroke in progress: (layer, its tiles before the stroke)
    liquify: Option<(u32, TileMap)>,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            layers: vec![Layer::new(1, LayerProps::named("Layer 1"))],
            active: 1,
            next_id: 2,
            state: BrushState::default(),
            remote: HashMap::new(),
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
            undo: Vec::new(),
            redo: Vec::new(),
            stroking: None,
            stroke_brush: None,
            alpha_lock_stroke: false,
            dirty: HashSet::new(),
            selection: None,
            transform: None,
            liquify: None,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    // ---- layers -------------------------------------------------------

    /// Layers bottom → top.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    fn layer_mut(&mut self, id: u32) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn active_layer(&self) -> u32 {
        self.active
    }

    pub fn set_active_layer(&mut self, id: u32) -> bool {
        if self.stroking.is_some() || self.index_of(id).is_none() {
            return false;
        }
        self.active = id;
        true
    }

    fn mark_layer(&mut self, idx: usize) {
        let keys: Vec<TileKey> = self.layers[idx].surface.tiles.keys().copied().collect();
        self.dirty.extend(keys);
    }

    /// Apply an edit and return its inverse.
    fn apply(&mut self, e: Edit) -> Edit {
        match e {
            Edit::Tiles { layer, snap } => {
                let Some(l) = self.layer_mut(layer) else {
                    return Edit::Batch(Vec::new());
                };
                let replaced = l.surface.swap_in(snap);
                Edit::Tiles {
                    layer,
                    snap: replaced,
                }
            }
            Edit::Insert { index, layer } => {
                let index = index.min(self.layers.len());
                self.layers.insert(index, *layer);
                self.mark_layer(index);
                Edit::Remove { index }
            }
            Edit::Remove { index } => {
                self.mark_layer(index);
                let layer = self.layers.remove(index);
                if layer.id == self.active {
                    let i = index
                        .saturating_sub(1)
                        .min(self.layers.len().saturating_sub(1));
                    self.active = self.layers.get(i).map_or(0, |l| l.id);
                }
                Edit::Insert {
                    index,
                    layer: Box::new(layer),
                }
            }
            Edit::Move { from, to } => {
                let l = self.layers.remove(from);
                self.layers.insert(to, l);
                self.mark_layer(to);
                Edit::Move { from: to, to: from }
            }
            Edit::Props { layer, props } => {
                let Some(idx) = self.index_of(layer) else {
                    return Edit::Batch(Vec::new());
                };
                let old = std::mem::replace(&mut self.layers[idx].props, props);
                self.mark_layer(idx);
                Edit::Props { layer, props: old }
            }
            Edit::Batch(edits) => {
                let mut inv: Vec<Edit> = edits.into_iter().map(|e| self.apply(e)).collect();
                inv.reverse();
                Edit::Batch(inv)
            }
        }
    }

    fn perform(&mut self, e: Edit) {
        let inv = self.apply(e);
        self.push_undo(inv);
    }

    /// Add an empty layer above the active one and make it active. Returns its id (0 = limit reached).
    pub fn add_layer(&mut self, name: &str) -> u32 {
        if self.stroking.is_some() || self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        let id = self.next_id;
        self.next_id += 1;
        let index = self
            .index_of(self.active)
            .map_or(self.layers.len(), |i| i + 1);
        let name = if name.trim().is_empty() {
            format!("Layer {id}")
        } else {
            name.chars().take(60).collect()
        };
        self.perform(Edit::Insert {
            index,
            layer: Box::new(Layer::new(id, LayerProps::named(name))),
        });
        self.active = id;
        id
    }

    /// Copy a layer (pixels and settings) just above it. Returns the copy's id.
    pub fn duplicate_layer(&mut self, id: u32) -> u32 {
        if self.stroking.is_some() || self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        let Some(idx) = self.index_of(id) else {
            return 0;
        };
        let new_id = self.next_id;
        self.next_id += 1;
        let mut copy = self.layers[idx].clone();
        copy.id = new_id;
        copy.props.name = format!("{} copy", copy.props.name)
            .chars()
            .take(60)
            .collect();
        self.perform(Edit::Insert {
            index: idx + 1,
            layer: Box::new(copy),
        });
        self.active = new_id;
        new_id
    }

    /// Delete a layer (never the last one).
    pub fn delete_layer(&mut self, id: u32) -> bool {
        if self.stroking.is_some() || self.layers.len() <= 1 {
            return false;
        }
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.perform(Edit::Remove { index });
        true
    }

    /// Move a layer to `to` (0 = bottom).
    pub fn move_layer(&mut self, id: u32, to: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let to = to.min(self.layers.len() - 1);
        if self.stroking.is_some() || from == to {
            return false;
        }
        self.perform(Edit::Move { from, to });
        true
    }

    /// Change a layer's settings. `record` = false previews (e.g. while dragging
    /// an opacity slider); finish with `record` = true and the value from
    /// before the drag in `before`, so one undo step covers the whole drag.
    pub fn set_layer_props(&mut self, id: u32, props: LayerProps, record: bool) -> bool {
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        let mut props = props;
        props.opacity = props.opacity.clamp(0.0, 1.0);
        props.name = props.name.chars().take(60).collect();
        if props.name.trim().is_empty() {
            props.name = self.layers[idx].props.name.clone();
        }
        if self.layers[idx].props == props {
            return false;
        }
        let inv = self.apply(Edit::Props { layer: id, props });
        if record {
            self.push_undo(inv);
        }
        true
    }

    /// Record a finished live change: `before` is the value from before the preview.
    pub fn commit_layer_props(&mut self, id: u32, before: LayerProps) -> bool {
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        if self.layers[idx].props == before {
            return false;
        }
        self.push_undo(Edit::Props {
            layer: id,
            props: before,
        });
        true
    }

    /// Merge a layer into the one below it (using its opacity and blend mode).
    pub fn merge_down(&mut self, id: u32) -> bool {
        if self.stroking.is_some() {
            return false;
        }
        let Some(upper_idx) = self.index_of(id) else {
            return false;
        };
        if upper_idx == 0 {
            return false;
        }
        let (lower, upper) = (&self.layers[upper_idx - 1], &self.layers[upper_idx]);
        let (opacity, blend) = (upper.props.opacity, upper.props.blend);
        let mut merged = Snapshot::new();
        for (key, src) in &upper.surface.tiles {
            let mut out = lower
                .surface
                .tiles
                .get(key)
                .cloned()
                .unwrap_or_else(empty_tile);
            for (orow, srow) in out.iter_mut().zip(src.iter()) {
                for (o, s) in orow.iter_mut().zip(srow.iter()) {
                    *o = merge_pixel(*o, *s, opacity, blend);
                }
            }
            merged.insert(*key, Some(out));
        }
        let lower_id = lower.id;
        self.perform(Edit::Batch(vec![
            Edit::Tiles {
                layer: lower_id,
                snap: merged,
            },
            Edit::Remove { index: upper_idx },
        ]));
        self.active = lower_id;
        true
    }

    // ---- painting -----------------------------------------------------

    /// Start a stroke on the active layer. Returns false (and paints nothing)
    /// if that layer is locked or hidden.
    pub fn begin_stroke(&mut self) -> bool {
        if self.transform.is_some() {
            return false;
        }
        let Some(l) = self.layer_mut(self.active) else {
            return false;
        };
        if l.props.locked || !l.props.visible {
            return false;
        }
        l.surface.recording = Some(Snapshot::new());
        let (id, alpha_lock) = (l.id, l.props.alpha_lock);
        self.state.reset();
        self.stroking = Some(id);
        self.stroke_brush = None;
        self.alpha_lock_stroke = alpha_lock;
        true
    }

    /// For alpha-locked layers: a copy of `brush` with lock_alpha on, made once per stroke.
    fn locked_copy(&mut self, brush: &Brush) {
        if self.alpha_lock_stroke && self.stroke_brush.is_none() {
            let mut b = brush.clone();
            b.get_mut(BrushSetting::LockAlpha).base_value = 1.0;
            self.stroke_brush = Some(b);
        }
    }

    /// Feed one pointer sample. `pressure` 0–1, tilt −1–1, `dtime` seconds.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_to(
        &mut self,
        brush: &Brush,
        x: f32,
        y: f32,
        pressure: f32,
        xtilt: f32,
        ytilt: f32,
        dtime: f64,
    ) {
        if self.stroking.is_none() && !self.begin_stroke() {
            return;
        }
        let id = self.stroking.unwrap_or(0);
        self.locked_copy(brush);
        let b = self.stroke_brush.as_ref().unwrap_or(brush);
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
            l.surface.touched.clear();
            b.stroke_to(
                &mut self.state,
                &mut l.surface,
                x,
                y,
                pressure,
                xtilt,
                ytilt,
                dtime,
            );
        }
        self.clip_to_selection(id);
    }

    /// Undo paint that landed outside the selection on the tiles just touched
    /// (restored from the stroke's own snapshot of how they were before).
    fn clip_to_selection(&mut self, layer: u32) {
        let Some(sel) = self.selection.as_ref() else {
            return;
        };
        let (w, h) = (self.width as usize, self.height as usize);
        let ts = TILE_SIZE;
        let Some(l) = self.layers.iter_mut().find(|l| l.id == layer) else {
            return;
        };
        let keys: Vec<TileKey> = l.surface.touched.drain().collect();
        for key in keys {
            let orig = l
                .surface
                .recording
                .as_ref()
                .and_then(|r| r.get(&key))
                .cloned();
            let Some(tile) = l.surface.tiles.get_mut(&key) else {
                continue;
            };
            for (py, row) in tile.iter_mut().enumerate() {
                for (px, p) in row.iter_mut().enumerate() {
                    let (cx, cy) = (
                        key.0 as i64 * ts as i64 + px as i64,
                        key.1 as i64 * ts as i64 + py as i64,
                    );
                    let inside = cx >= 0
                        && cy >= 0
                        && (cx as usize) < w
                        && (cy as usize) < h
                        && sel[cy as usize * w + cx as usize] > 0;
                    if !inside {
                        *p = match &orig {
                            Some(Some(o)) => o[py][px],
                            _ => [0; 4],
                        };
                    }
                }
            }
        }
    }

    /// Finish the stroke (flushes slow-tracking lag) and record it for undo.
    pub fn end_stroke(&mut self, brush: &Brush) {
        let Some(id) = self.stroking.take() else {
            return;
        };
        self.locked_copy(brush);
        let b = self.stroke_brush.take();
        let b = b.as_ref().unwrap_or(brush);
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
            l.surface.touched.clear();
            b.finish_stroke(&mut self.state, &mut l.surface);
        }
        self.clip_to_selection(id);
        let snap = self
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .and_then(|l| l.surface.recording.take());
        self.state.reset();
        self.alpha_lock_stroke = false;
        // Always record a history step, even if no pixels changed, so the
        // web app's history and this undo stack stay one-to-one.
        self.push_undo(Edit::Tiles {
            layer: id,
            snap: snap.unwrap_or_default(),
        });
    }

    /// Abandon the stroke in progress (e.g. a second finger turned it into a
    /// pinch): its pixels are restored and nothing goes into the history.
    pub fn cancel_stroke(&mut self) {
        let Some(id) = self.stroking.take() else {
            return;
        };
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id)
            && let Some(snap) = l.surface.recording.take()
        {
            l.surface.swap_in(snap);
        }
        self.state.reset();
        self.stroke_brush = None;
        self.alpha_lock_stroke = false;
    }

    fn push_undo(&mut self, e: Edit) {
        self.undo.push(e);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Erase the active layer (undoable). Returns false if it was already empty.
    pub fn clear(&mut self) -> bool {
        let active = self.active;
        let Some(l) = self.layer_mut(active) else {
            return false;
        };
        if l.surface.tiles.is_empty() || l.props.locked {
            return false;
        }
        if let Some(sel) = self.selection.clone() {
            // clear only what's selected
            let (w, h) = (self.width as usize, self.height as usize);
            let ts = TILE_SIZE;
            let l = self.layers.iter().find(|l| l.id == active).unwrap();
            let mut snap = Snapshot::new();
            for (&key, tile) in &l.surface.tiles {
                let mut t = tile.clone();
                let mut changed = false;
                for (py, row) in t.iter_mut().enumerate() {
                    for (px, p) in row.iter_mut().enumerate() {
                        let (cx, cy) = (
                            key.0 as i64 * ts as i64 + px as i64,
                            key.1 as i64 * ts as i64 + py as i64,
                        );
                        if cx >= 0
                            && cy >= 0
                            && (cx as usize) < w
                            && (cy as usize) < h
                            && sel[cy as usize * w + cx as usize] > 0
                            && p[3] > 0
                        {
                            *p = [0; 4];
                            changed = true;
                        }
                    }
                }
                if changed {
                    snap.insert(key, Some(t));
                }
            }
            if snap.is_empty() {
                return false;
            }
            self.perform(Edit::Tiles {
                layer: active,
                snap,
            });
            return true;
        }
        let l = self.layers.iter_mut().find(|l| l.id == active).unwrap();
        let snap: Snapshot = l.surface.tiles.drain().map(|(k, t)| (k, Some(t))).collect();
        l.surface.dirty.extend(snap.keys().copied());
        self.push_undo(Edit::Tiles {
            layer: active,
            snap,
        });
        true
    }

    // ---- fill ----

    /// Flood-fill the active layer from canvas point (x, y) with sRGB colour
    /// `rgb` at `opacity`. The region is every connected pixel within
    /// `tolerance` (0–1, per channel) of the clicked colour, sampled from what
    /// you see (`all_layers`) or the active layer alone; it then grows by
    /// `grow` px so it tucks under soft line edges. Paint goes *behind* what
    /// the layer already has, so lines stay on top. One undo step; false if
    /// nothing changed (or the layer is locked/hidden).
    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    pub fn fill(
        &mut self,
        x: u32,
        y: u32,
        rgb: [u8; 3],
        opacity: f32,
        tolerance: f32,
        all_layers: bool,
        grow: u32,
        gap: u32,
    ) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        if self.stroking.is_some() || x as usize >= w || y as usize >= h {
            return false;
        }
        let active = self.active;
        let Some(li) = self.index_of(active) else {
            return false;
        };
        if self.layers[li].props.locked || !self.layers[li].props.visible {
            return false;
        }
        let source = self.sample_source(li, all_layers);
        let tol = (tolerance.clamp(0.0, 1.0) * 255.0) as i32;
        let mask = flood_closed(&source, w, h, x as usize, y as usize, tol, gap as usize);
        let mut mask = grow_mask(&mask, w, h, grow as usize);
        if let Some(sel) = &self.selection {
            for (m, &s) in mask.iter_mut().zip(sel) {
                *m &= s > 0;
            }
        }
        // 2. paint behind the layer's pixels, tile by tile
        let fill = premul_fix15(
            [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ],
            opacity.clamp(0.0, 1.0),
        );
        let alpha_lock = self.layers[li].props.alpha_lock;
        let ts = TILE_SIZE;
        let mut keys: HashSet<TileKey> = HashSet::new();
        for (i, &m) in mask.iter().enumerate() {
            if m {
                keys.insert(((i % w / ts) as i32, (i / w / ts) as i32));
            }
        }
        let mut snap = Snapshot::new();
        let l = &mut self.layers[li];
        for key in keys {
            let mut tile = l
                .surface
                .tiles
                .get(&key)
                .cloned()
                .unwrap_or_else(empty_tile);
            let mut changed = false;
            for py in 0..ts {
                for px in 0..ts {
                    let (cx, cy) = (key.0 as usize * ts + px, key.1 as usize * ts + py);
                    if cx >= w || cy >= h || !mask[cy * w + cx] {
                        continue;
                    }
                    let d = tile[py][px];
                    if alpha_lock && d[3] == 0 {
                        continue;
                    }
                    // "destination over": the existing pixel stays on top of the fill
                    let out = if alpha_lock {
                        let (_, a) = unpremul_srgb(d);
                        let c = unpremul_srgb(fill).0;
                        premul_fix15(c, a)
                    } else {
                        merge_pixel(fill, d, 1.0, Blend::Normal)
                    };
                    if out != d {
                        tile[py][px] = out;
                        changed = true;
                    }
                }
            }
            if changed {
                snap.insert(key, Some(tile));
            }
        }
        if snap.is_empty() {
            return false;
        }
        self.perform(Edit::Tiles {
            layer: active,
            snap,
        });
        true
    }

    /// Straight RGBA of every canvas pixel: what you see, or one layer's own pixels.
    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    fn sample_source(&mut self, li: usize, all_layers: bool) -> Vec<[u8; 4]> {
        let (w, h) = (self.width as usize, self.height as usize);
        if all_layers {
            self.flush();
            self.rgba.as_chunks::<4>().0.to_vec()
        } else {
            let l = &self.layers[li];
            let ts = TILE_SIZE;
            let mut v = vec![[0u8; 4]; w * h];
            for (&(tx, ty), tile) in &l.surface.tiles {
                for py in 0..ts {
                    for px in 0..ts {
                        let (cx, cy) = (tx * ts as i32 + px as i32, ty * ts as i32 + py as i32);
                        if cx < 0 || cy < 0 || cx as usize >= w || cy as usize >= h {
                            continue;
                        }
                        let (c, a) = unpremul_srgb(tile[py][px]);
                        v[cy as usize * w + cx as usize] = [
                            (c[0] * 255.0) as u8,
                            (c[1] * 255.0) as u8,
                            (c[2] * 255.0) as u8,
                            (a * 255.0) as u8,
                        ];
                    }
                }
            }
            v
        }
    }

    // ---- selection ----

    /// Combine `new` (per canvas pixel) into the selection: 0 replace, 1 add,
    /// 2 subtract, 3 intersect. An empty result means no selection.
    fn combine_selection(&mut self, new: Vec<bool>, op: u8) -> bool {
        let n = (self.width * self.height) as usize;
        if new.len() != n {
            return false;
        }
        let cur = self.selection.take().unwrap_or_else(|| vec![0; n]);
        let out: Vec<u8> = cur
            .iter()
            .zip(&new)
            .map(|(&c, &m)| {
                let c = c > 0;
                let v = match op {
                    1 => c || m,
                    2 => c && !m,
                    3 => c && m,
                    _ => m,
                };
                if v { 255 } else { 0 }
            })
            .collect();
        if out.iter().any(|&v| v > 0) {
            self.selection = Some(out);
        }
        true
    }

    /// Set the selection from a mask (any byte > 127 = selected), combined by `op`.
    pub fn set_selection(&mut self, mask: &[u8], op: u8) -> bool {
        self.combine_selection(mask.iter().map(|&v| v > 127).collect(), op)
    }

    /// Magic wand: select the connected area of similar colour at (x, y).
    pub fn select_wand(
        &mut self,
        x: u32,
        y: u32,
        tolerance: f32,
        all_layers: bool,
        op: u8,
    ) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        let Some(li) = self.index_of(self.active) else {
            return false;
        };
        if x as usize >= w || y as usize >= h {
            return false;
        }
        let src = self.sample_source(li, all_layers);
        let tol = (tolerance.clamp(0.0, 1.0) * 255.0) as i32;
        // the same gap closing as fill, so grainy pencil outlines hold the selection in
        let m = flood_closed(&src, w, h, x as usize, y as usize, tol, 3);
        self.combine_selection(m, op)
    }

    pub fn select_all(&mut self) {
        self.selection = Some(vec![255; (self.width * self.height) as usize]);
    }

    pub fn deselect(&mut self) {
        self.selection = None;
    }

    pub fn invert_selection(&mut self) {
        self.selection = match self.selection.take() {
            None => Some(vec![255; (self.width * self.height) as usize]),
            Some(s) => {
                let inv: Vec<u8> = s.iter().map(|&v| 255 - v).collect();
                inv.iter().any(|&v| v > 0).then_some(inv)
            }
        };
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    /// The selection as bytes (empty when there is none).
    pub fn selection_mask(&self) -> Vec<u8> {
        self.selection.clone().unwrap_or_default()
    }

    // ---- transform ----

    /// Lift the selected pixels of the active layer (or all of it, with no
    /// selection) so they can be moved, scaled, rotated and flipped. Returns
    /// the lifted box `[x0, y0, x1, y1]` (empty if there's nothing to lift).
    pub fn transform_begin(&mut self) -> Vec<f32> {
        if self.transform.is_some() || self.stroking.is_some() {
            return Vec::new();
        }
        let (w, h) = (self.width as i64, self.height as i64);
        let active = self.active;
        let Some(li) = self.index_of(active) else {
            return Vec::new();
        };
        if self.layers[li].props.locked || !self.layers[li].props.visible {
            return Vec::new();
        }
        let sel = self.selection.clone();
        let ts = TILE_SIZE as i64;
        let picked = |cx: i64, cy: i64| match &sel {
            None => true,
            Some(s) => cx >= 0 && cy >= 0 && cx < w && cy < h && s[(cy * w + cx) as usize] > 0,
        };
        let l = &self.layers[li];
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (&(tx, ty), tile) in &l.surface.tiles {
            for (py, row) in tile.iter().enumerate() {
                for (px, p) in row.iter().enumerate() {
                    let (cx, cy) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                    if p[3] > 0 && picked(cx, cy) {
                        x0 = x0.min(cx);
                        y0 = y0.min(cy);
                        x1 = x1.max(cx);
                        y1 = y1.max(cy);
                    }
                }
            }
        }
        if x0 > x1 {
            return Vec::new();
        }
        let (fw, fh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let mut float = vec![[0u16; 4]; fw * fh];
        let original = l.surface.tiles.clone();
        let mut base = original.clone();
        for (&(tx, ty), tile) in base.iter_mut() {
            for (py, row) in tile.iter_mut().enumerate() {
                for (px, p) in row.iter_mut().enumerate() {
                    let (cx, cy) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                    if p[3] > 0 && picked(cx, cy) && cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
                        float[((cy - y0) as usize) * fw + (cx - x0) as usize] = *p;
                        *p = [0; 4];
                    }
                }
            }
        }
        self.transform = Some(Box::new(TransformSession {
            layer: active,
            original,
            base,
            float,
            fx0: x0,
            fy0: y0,
            fw,
            fh,
            last: None,
            m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }));
        self.transform_preview([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        vec![x0 as f32, y0 as f32, (x1 + 1) as f32, (y1 + 1) as f32]
    }

    pub fn transforming(&self) -> bool {
        self.transform.is_some()
    }

    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    /// Show the lifted pixels under matrix `m` (dest = a·x + c·y + e, b·x + d·y + f).
    pub fn transform_preview(&mut self, m: [f32; 6]) -> bool {
        let Some(mut t) = self.transform.take() else {
            return false;
        };
        let [a, b, c, d, e, f] = m;
        let det = a * d - b * c;
        if det.abs() < 1e-6 {
            self.transform = Some(t);
            return false;
        }
        t.m = m;
        let (w, h) = (self.width as i64, self.height as i64);
        let ts = TILE_SIZE as i64;
        // where the floating box lands
        let (fx0, fy0) = (t.fx0 as f32, t.fy0 as f32);
        let (fx1, fy1) = (fx0 + t.fw as f32, fy0 + t.fh as f32);
        let pts = [(fx0, fy0), (fx1, fy0), (fx0, fy1), (fx1, fy1)]
            .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        let dx0 = (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i64 - 1).max(0);
        let dy0 = (pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i64 - 1).max(0);
        let dx1 = (pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i64 + 1).min(w - 1);
        let dy1 = (pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i64 + 1).min(h - 1);
        // repaint the old and new boxes from the base
        let mut region = (dx0, dy0, dx1, dy1);
        if let Some(o) = t.last {
            region = (
                region.0.min(o.0),
                region.1.min(o.1),
                region.2.max(o.2),
                region.3.max(o.3),
            );
        }
        t.last = Some((dx0, dy0, dx1, dy1));
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        let sample = |sx: f32, sy: f32| -> [u16; 4] {
            // bilinear, premultiplied; outside the float is transparent
            let (x, y) = (sx - fx0 - 0.5, sy - fy0 - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (tx, ty) = (x - x0, y - y0);
            let get = |xi: i64, yi: i64| -> [f32; 4] {
                if xi < 0 || yi < 0 || xi >= t.fw as i64 || yi >= t.fh as i64 {
                    return [0.0; 4];
                }
                t.float[yi as usize * t.fw + xi as usize].map(|v| v as f32)
            };
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (p00, p10, p01, p11) = (
                get(x0, y0),
                get(x0 + 1, y0),
                get(x0, y0 + 1),
                get(x0 + 1, y0 + 1),
            );
            let mut out = [0u16; 4];
            for k in 0..4 {
                let v = p00[k] * (1.0 - tx) * (1.0 - ty)
                    + p10[k] * tx * (1.0 - ty)
                    + p01[k] * (1.0 - tx) * ty
                    + p11[k] * tx * ty;
                out[k] = v.round().clamp(0.0, 32768.0) as u16;
            }
            out
        };
        let Some(li) = self.index_of(t.layer) else {
            self.transform = Some(t);
            return false;
        };
        let tiles = &mut self.layers[li].surface.tiles;
        for ty in region.1.div_euclid(ts)..=region.3.div_euclid(ts) {
            for tx in region.0.div_euclid(ts)..=region.2.div_euclid(ts) {
                let key = (tx as i32, ty as i32);
                let mut tile = t.base.get(&key).cloned().unwrap_or_else(empty_tile);
                for py in 0..TILE_SIZE {
                    for px in 0..TILE_SIZE {
                        let (cx, cy) = (tx * ts + px as i64, ty * ts + py as i64);
                        if cx < dx0 || cx > dx1 || cy < dy0 || cy > dy1 {
                            continue;
                        }
                        let (ux, uy) = (cx as f32 + 0.5 - e, cy as f32 + 0.5 - f);
                        let s = sample(ia * ux + ic * uy, ib * ux + id * uy);
                        if s[3] > 0 {
                            tile[py][px] = merge_pixel(tile[py][px], s, 1.0, Blend::Normal);
                        }
                    }
                }
                if tile.iter().flatten().any(|p| p[3] > 0) {
                    tiles.insert(key, tile);
                } else {
                    tiles.remove(&key);
                }
                self.dirty.insert(key);
            }
        }
        self.transform = Some(t);
        true
    }

    /// Keep the transform (one undo step); the selection follows the pixels.
    pub fn transform_commit(&mut self) -> bool {
        let Some(t) = self.transform.take() else {
            return false;
        };
        let Some(li) = self.index_of(t.layer) else {
            return false;
        };
        let now = &self.layers[li].surface.tiles;
        let mut snap = Snapshot::new();
        for key in t.original.keys().chain(now.keys()) {
            let before = t.original.get(key);
            let after = now.get(key);
            let same = match (before, after) {
                (Some(x), Some(y)) => x == y,
                (None, None) => true,
                _ => false,
            };
            if !same {
                snap.insert(*key, before.cloned());
            }
        }
        if let Some(sel) = self.selection.take() {
            let (w, h) = (self.width as usize, self.height as usize);
            let [a, b, c, d, e, f] = t.m;
            let det = a * d - b * c;
            let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
            let mut out = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let (ux, uy) = (x as f32 + 0.5 - e, y as f32 + 0.5 - f);
                    let (sx, sy) = ((ia * ux + ic * uy).floor(), (ib * ux + id * uy).floor());
                    if sx >= 0.0 && sy >= 0.0 && (sx as usize) < w && (sy as usize) < h {
                        out[y * w + x] = sel[sy as usize * w + sx as usize];
                    }
                }
            }
            if out.iter().any(|&v| v > 0) {
                self.selection = Some(out);
            }
        }
        if snap.is_empty() {
            return false;
        }
        self.push_undo(Edit::Tiles {
            layer: t.layer,
            snap,
        });
        true
    }

    /// Put everything back as it was before `transform_begin`.
    pub fn transform_cancel(&mut self) {
        let Some(t) = self.transform.take() else {
            return;
        };
        if let Some(li) = self.index_of(t.layer) {
            let tiles = &mut self.layers[li].surface.tiles;
            self.dirty.extend(tiles.keys().copied());
            *tiles = t.original;
            self.dirty.extend(tiles.keys().copied());
        }
    }

    // ---- liquify: push pixels around like wet paint ----

    /// Start a liquify stroke on the active layer (false if locked/hidden/busy).
    pub fn liquify_begin(&mut self) -> bool {
        if self.liquify.is_some() || self.transform.is_some() || self.stroking.is_some() {
            return false;
        }
        let Some(li) = self.index_of(self.active) else {
            return false;
        };
        let l = &self.layers[li];
        if l.props.locked || !l.props.visible {
            return false;
        }
        self.liquify = Some((l.id, l.surface.tiles.clone()));
        true
    }

    /// Push the pixels around (x, y) by (dx, dy): a forward warp, strongest in
    /// the middle and fading to nothing at `radius`. Only selected pixels move.
    pub fn liquify_push(&mut self, x: f32, y: f32, dx: f32, dy: f32, radius: f32, strength: f32) {
        let Some((layer, _)) = &self.liquify else {
            return;
        };
        let layer = *layer;
        let Some(li) = self.index_of(layer) else {
            return;
        };
        let (w, h) = (self.width as i64, self.height as i64);
        let r = radius.max(2.0);
        let reach = r + dx.abs().max(dy.abs()) + 2.0;
        let (x0, y0) = (
            ((x - reach).floor() as i64).max(0),
            ((y - reach).floor() as i64).max(0),
        );
        let (x1, y1) = (
            ((x + reach).ceil() as i64).min(w - 1),
            ((y + reach).ceil() as i64).min(h - 1),
        );
        if x0 > x1 || y0 > y1 {
            return;
        }
        let ts = TILE_SIZE as i64;
        let tiles = &mut self.layers[li].surface.tiles;
        // read the area first, so every pixel samples the same (old) picture
        let (bw, bh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let get = |tiles: &TileMap, cx: i64, cy: i64| -> [u16; 4] {
            let key = (cx.div_euclid(ts) as i32, cy.div_euclid(ts) as i32);
            tiles.get(&key).map_or([0; 4], |t| {
                t[cy.rem_euclid(ts) as usize][cx.rem_euclid(ts) as usize]
            })
        };
        let mut old = vec![[0u16; 4]; bw * bh];
        for yy in 0..bh {
            for xx in 0..bw {
                old[yy * bw + xx] = get(tiles, x0 + xx as i64, y0 + yy as i64);
            }
        }
        let sample = |sx: f32, sy: f32| -> [u16; 4] {
            let (fx, fy) = (sx - x0 as f32 - 0.5, sy - y0 as f32 - 0.5);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - ix, fy - iy);
            let at = |xi: i64, yi: i64| -> [f32; 4] {
                if xi < 0 || yi < 0 || xi >= bw as i64 || yi >= bh as i64 {
                    return [0.0; 4];
                }
                old[yi as usize * bw + xi as usize].map(|v| v as f32)
            };
            let (ix, iy) = (ix as i64, iy as i64);
            let (a, b, c, d) = (
                at(ix, iy),
                at(ix + 1, iy),
                at(ix, iy + 1),
                at(ix + 1, iy + 1),
            );
            let mut o = [0u16; 4];
            for k in 0..4 {
                o[k] = (a[k] * (1.0 - tx) * (1.0 - ty)
                    + b[k] * tx * (1.0 - ty)
                    + c[k] * (1.0 - tx) * ty
                    + d[k] * tx * ty)
                    .round()
                    .clamp(0.0, 32768.0) as u16;
            }
            o
        };
        let sel = self.selection.as_ref();
        let s = strength.clamp(0.0, 1.0);
        for yy in 0..bh {
            for xx in 0..bw {
                let (cx, cy) = (x0 + xx as i64, y0 + yy as i64);
                if sel.is_some_and(|m| m[(cy * w + cx) as usize] == 0) {
                    continue;
                }
                let (px, py) = (cx as f32 + 0.5, cy as f32 + 0.5);
                let d2 = ((px - x).powi(2) + (py - y).powi(2)) / (r * r);
                if d2 >= 1.0 {
                    continue;
                }
                let fall = (1.0 - d2).powi(2) * s;
                let v = sample(px - dx * fall, py - dy * fall);
                if v == old[yy * bw + xx] {
                    continue;
                }
                let key = (cx.div_euclid(ts) as i32, cy.div_euclid(ts) as i32);
                let t = tiles.entry(key).or_insert_with(empty_tile);
                t[cy.rem_euclid(ts) as usize][cx.rem_euclid(ts) as usize] = v;
                self.dirty.insert(key);
            }
        }
    }

    /// Finish the liquify stroke: one undo step (false if nothing changed).
    pub fn liquify_end(&mut self) -> bool {
        let Some((layer, original)) = self.liquify.take() else {
            return false;
        };
        let Some(li) = self.index_of(layer) else {
            return false;
        };
        let now = &self.layers[li].surface.tiles;
        let mut snap = Snapshot::new();
        for key in original.keys().chain(now.keys()) {
            if original.get(key) != now.get(key) {
                snap.insert(*key, original.get(key).cloned());
            }
        }
        if snap.is_empty() {
            return false;
        }
        self.push_undo(Edit::Tiles { layer, snap });
        true
    }

    // ---- shared canvases: other people's strokes ----

    /// Add a layer without an undo step (e.g. a layer for another person). Returns its id.
    pub fn add_layer_silent(&mut self, name: &str) -> u32 {
        if self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.layers.push(Layer::new(
            id,
            LayerProps::named(name.chars().take(60).collect::<String>()),
        ));
        id
    }

    /// Erase a layer without an undo step (before redrawing someone's strokes).
    pub fn clear_layer_silent(&mut self, id: u32) {
        if let Some(idx) = self.index_of(id) {
            self.mark_layer(idx);
            self.layers[idx].surface.tiles.clear();
        }
    }

    /// Start another person's stroke `key` on `layer` (any number at once,
    /// alongside the local stroke; never undoable here).
    pub fn remote_begin(&mut self, key: u32, layer: u32) -> bool {
        if self.index_of(layer).is_none() {
            return false;
        }
        self.remote.insert(key, (layer, BrushState::default()));
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub fn remote_to(
        &mut self,
        key: u32,
        brush: &Brush,
        x: f32,
        y: f32,
        pressure: f32,
        dtime: f64,
    ) {
        let Some((layer, state)) = self.remote.get_mut(&key) else {
            return;
        };
        let layer = *layer;
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == layer) {
            // a remote stroke must not land in the local stroke's undo snapshot
            let rec = l.surface.recording.take();
            brush.stroke_to(state, &mut l.surface, x, y, pressure, 0.0, 0.0, dtime);
            l.surface.recording = rec;
        }
    }

    pub fn remote_end(&mut self, key: u32, brush: &Brush) {
        let Some((layer, mut state)) = self.remote.remove(&key) else {
            return;
        };
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == layer) {
            let rec = l.surface.recording.take();
            brush.finish_stroke(&mut state, &mut l.surface);
            l.surface.recording = rec;
        }
    }

    /// Forget the undo/redo history (e.g. after setting up starter layers,
    /// so the first undo doesn't take the setup apart).
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        if self.stroking.is_some() {
            return false;
        }
        if self.transform.is_some() {
            // undo during a transform just cancels it
            self.transform_cancel();
            return false;
        }
        let Some(e) = self.undo.pop() else {
            return false;
        };
        let inv = self.apply(e);
        self.redo.push(inv);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.stroking.is_some() {
            return false;
        }
        let Some(e) = self.redo.pop() else {
            return false;
        };
        let inv = self.apply(e);
        self.undo.push(inv);
        true
    }

    /// Whether nothing visible is painted (hidden or fully transparent layers don't count).
    pub fn is_blank(&self) -> bool {
        !self
            .layers
            .iter()
            .any(|l| l.props.visible && l.props.opacity > 0.0 && !l.is_empty())
    }

    /// Recomposite changed tiles into the RGBA buffer. Returns the area that
    /// changed, or `None` if nothing did.
    pub fn flush(&mut self) -> Option<Rect> {
        for l in &mut self.layers {
            self.dirty.extend(l.surface.dirty.drain());
        }
        if self.dirty.is_empty() {
            return None;
        }
        let ts = TILE_SIZE as i32;
        let (w, h) = (self.width as i32, self.height as i32);
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let dirty: Vec<TileKey> = self.dirty.drain().collect();
        for (tx, ty) in dirty {
            let (px0, py0) = (tx * ts, ty * ts);
            let (px1, py1) = ((px0 + ts).min(w), (py0 + ts).min(h));
            let (cx0, cy0) = (px0.max(0), py0.max(0));
            if cx0 >= px1 || cy0 >= py1 {
                continue; // tile entirely off-canvas
            }
            let sources: Vec<TileSource<'_>> = self
                .layers
                .iter()
                .filter(|l| l.props.visible && l.props.opacity > 0.0)
                .filter_map(|l| {
                    l.surface.tiles.get(&(tx, ty)).map(|t| TileSource {
                        tile: t,
                        opacity: l.props.opacity,
                        blend: l.props.blend,
                    })
                })
                .collect();
            for y in cy0..py1 {
                for x in cx0..px1 {
                    let idx = ((y * w + x) * 4) as usize;
                    let px = if sources.is_empty() {
                        [255, 255, 255]
                    } else {
                        composite_pixel(&sources, (y - py0) as usize, (x - px0) as usize)
                    };
                    self.rgba[idx..idx + 3].copy_from_slice(&px);
                    self.rgba[idx + 3] = 255;
                }
            }
            x0 = x0.min(cx0);
            y0 = y0.min(cy0);
            x1 = x1.max(px1);
            y1 = y1.max(py1);
        }
        (x0 < x1).then(|| Rect {
            x: x0 as u32,
            y: y0 as u32,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        })
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// A small preview of one layer on its own (straight-alpha sRGB RGBA8,
    /// transparent where the layer is empty), `tw`×`th`. Each preview pixel
    /// averages its area (thin pencil lines would vanish with point sampling),
    /// with coverage boosted so faint lines still show. Only painted tiles are
    /// visited, so cost follows the painted area.
    pub fn layer_thumbnail(&self, id: u32, tw: u32, th: u32) -> Vec<u8> {
        let n = (tw * th) as usize;
        let mut out = vec![0u8; n * 4];
        let Some(l) = self.layers.iter().find(|l| l.id == id) else {
            return out;
        };
        if tw == 0 || th == 0 {
            return out;
        }
        // sum of alpha, and of alpha-weighted colour, per preview pixel
        let mut sa = vec![0f32; n];
        let mut sc = vec![[0f32; 3]; n];
        let ts = TILE_SIZE as u32;
        let step = ((self.width / tw).max(1) / 4).max(1);
        for (&(kx, ky), tile) in &l.surface.tiles {
            if kx < 0 || ky < 0 {
                continue;
            }
            let (x0, y0) = (kx as u32 * ts, ky as u32 * ts);
            for py in (0..ts).step_by(step as usize) {
                let y = y0 + py;
                if y >= self.height {
                    break;
                }
                let row = ((y * th) / self.height) as usize * tw as usize;
                for px in (0..ts).step_by(step as usize) {
                    let x = x0 + px;
                    if x >= self.width {
                        break;
                    }
                    let (c, a) = unpremul_srgb(tile[py as usize][px as usize]);
                    if a <= 0.0 {
                        continue;
                    }
                    let i = row + ((x * tw) / self.width) as usize;
                    sa[i] += a;
                    for k in 0..3 {
                        sc[i][k] += c[k] * a;
                    }
                }
            }
        }
        // samples per preview pixel
        let per = ((self.width as f32 / tw as f32) / step as f32).max(1.0)
            * ((self.height as f32 / th as f32) / step as f32).max(1.0);
        for i in 0..n {
            if sa[i] <= 0.0 {
                continue;
            }
            let o = i * 4;
            for k in 0..3 {
                out[o + k] = ((sc[i][k] / sa[i]) * 255.0).round() as u8;
            }
            out[o + 3] = ((sa[i] / per * 6.0).min(1.0) * 255.0).round() as u8;
        }
        out
    }

    /// The layer list as JSON, bottom → top (for the web app's layers panel).
    pub fn layers_json(&self) -> String {
        let items: Vec<String> = self
            .layers
            .iter()
            .map(|l| {
                let p = &l.props;
                format!(
                    r#"{{"id":{},"name":{},"visible":{},"opacity":{},"blend":"{}","locked":{},"alphaLock":{},"empty":{}}}"#,
                    l.id,
                    json_str(&p.name),
                    p.visible,
                    (p.opacity * 1000.0).round() / 1000.0,
                    p.blend.as_str(),
                    p.locked,
                    p.alpha_lock,
                    l.is_empty()
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }
}

/// Pixels connected to (x, y) whose colour is within `tol` (per channel, 0–255)
/// of the seed pixel (scanline flood fill).
fn flood(src: &[[u8; 4]], w: usize, h: usize, x: usize, y: usize, tol: i32) -> Vec<bool> {
    let seed = src[y * w + x];
    let near = |p: [u8; 4]| (0..4).all(|k| (p[k] as i32 - seed[k] as i32).abs() <= tol);
    let mut mask = vec![false; w * h];
    let mut stack = vec![(x, y)];
    while let Some((sx, sy)) = stack.pop() {
        let row = sy * w;
        if mask[row + sx] || !near(src[row + sx]) {
            continue;
        }
        let (mut l, mut r) = (sx, sx);
        while l > 0 && !mask[row + l - 1] && near(src[row + l - 1]) {
            l -= 1;
        }
        while r + 1 < w && !mask[row + r + 1] && near(src[row + r + 1]) {
            r += 1;
        }
        for i in l..=r {
            mask[row + i] = true;
        }
        for ny in [sy.wrapping_sub(1), sy + 1] {
            if ny >= h {
                continue;
            }
            let nrow = ny * w;
            let mut i = l;
            while i <= r {
                if !mask[nrow + i] && near(src[nrow + i]) {
                    stack.push((i, ny));
                    // skip the rest of this run; it's reached from the pushed seed
                    while i <= r && near(src[nrow + i]) {
                        i += 1;
                    }
                }
                i += 1;
            }
        }
    }
    mask
}

/// Flood fill with gap closing: lines count as `gap` px thicker while
/// flooding, so small gaps in grainy pencil lines don't let it leak; the
/// region then grows back by `gap` to meet the lines. (`gap` 0 = plain flood.)
fn flood_closed(
    src: &[[u8; 4]],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    tol: i32,
    gap: usize,
) -> Vec<bool> {
    if gap == 0 {
        return flood(src, w, h, x, y, tol);
    }
    let seed = src[y * w + x];
    let near = |p: [u8; 4]| (0..4).all(|k| (p[k] as i32 - seed[k] as i32).abs() <= tol);
    let wall: Vec<bool> = src.iter().map(|&p| !near(p)).collect();
    let wall = grow_mask(&wall, w, h, gap);
    if wall[y * w + x] {
        // clicked right by a line
        return flood(src, w, h, x, y, tol);
    }
    let open: Vec<[u8; 4]> = wall
        .iter()
        .map(|&b| if b { [0, 0, 0, 0] } else { [255; 4] })
        .collect();
    grow_mask(&flood(&open, w, h, x, y, 0), w, h, gap)
}

/// Grow a mask by `r` pixels (square neighbourhood, two separable passes).
fn grow_mask(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    if r == 0 {
        return m.to_vec();
    }
    let mut a = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r).min(w - 1));
            a[y * w + x] = (x0..=x1).any(|i| m[y * w + i]);
        }
    }
    let mut b = vec![false; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            b[y * w + x] = (y0..=y1).any(|j| a[j * w + x]);
        }
    }
    b
}

/// Set a brush's base colour from sRGB 0–255 (converted to HSV, as MyPaint stores it).
pub fn set_brush_rgb(brush: &mut Brush, r: u8, g: u8, b: u8) {
    let (h, s, v) = rgb_to_hsv(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    brush.get_mut(BrushSetting::ColorH).base_value = h;
    brush.get_mut(BrushSetting::ColorS).base_value = s;
    brush.get_mut(BrushSetting::ColorV).base_value = v;
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen() -> Brush {
        let mut b = Brush::new();
        b.get_mut(BrushSetting::Radius).base_value = 2.0; // log2 → 4 px
        b.get_mut(BrushSetting::Opaque).base_value = 1.0;
        b.get_mut(BrushSetting::Hardness).base_value = 1.0;
        b.get_mut(BrushSetting::DabsPerActualRadius).base_value = 2.0;
        set_brush_rgb(&mut b, 0, 0, 0);
        b
    }

    fn line(p: &mut Painter, b: &Brush, y: f32) {
        p.begin_stroke();
        for i in 0..=40 {
            p.stroke_to(b, 20.0 + i as f32 * 4.0, y, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(b);
    }

    fn dark_pixels(p: &Painter) -> usize {
        p.rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] < 128)
            .count()
    }

    #[test]
    fn stroke_paints_and_flush_reports_rect() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 50.0);
        let r = p.flush().expect("something changed");
        assert!(r.w > 0 && r.h > 0 && r.x + r.w <= 256 && r.y + r.h <= 128);
        assert!(dark_pixels(&p) > 100, "the line should be visible");
        assert!(p.flush().is_none(), "second flush has nothing new");
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 30.0);
        p.flush();
        let one = dark_pixels(&p);
        line(&mut p, &b, 90.0);
        p.flush();
        let two = dark_pixels(&p);
        assert!(two > one);

        assert!(p.undo());
        p.flush();
        assert_eq!(dark_pixels(&p), one);
        assert!(p.redo());
        p.flush();
        assert_eq!(dark_pixels(&p), two);
        assert!(p.undo() && p.undo());
        p.flush();
        assert_eq!(dark_pixels(&p), 0);
        assert!(!p.can_undo());
    }

    #[test]
    fn clear_is_undoable_and_new_stroke_drops_redo() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0);
        p.flush();
        let before = dark_pixels(&p);
        p.clear();
        p.flush();
        assert!(p.is_blank());
        assert_eq!(dark_pixels(&p), 0);
        p.undo();
        p.flush();
        assert_eq!(dark_pixels(&p), before);
        p.undo();
        assert!(p.can_redo());
        line(&mut p, &b, 20.0);
        assert!(!p.can_redo());
    }

    #[test]
    fn eraser_removes_paint() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0);
        p.flush();
        let before = dark_pixels(&p);
        let mut e = pen();
        e.get_mut(BrushSetting::Eraser).base_value = 1.0;
        e.get_mut(BrushSetting::Radius).base_value = 3.0;
        line(&mut p, &e, 60.0);
        p.flush();
        assert!(dark_pixels(&p) < before / 4);
    }

    #[test]
    fn undo_limit_is_respected() {
        let mut p = Painter::new(128, 64);
        let b = pen();
        for i in 0..(UNDO_LIMIT + 5) {
            line(&mut p, &b, (i % 50) as f32 + 5.0);
        }
        let mut n = 0;
        while p.undo() {
            n += 1;
        }
        assert_eq!(n, UNDO_LIMIT);
    }

    fn px(p: &Painter, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * p.width() + x) * 4) as usize;
        [p.rgba()[i], p.rgba()[i + 1], p.rgba()[i + 2]]
    }

    fn props(p: &Painter, id: u32) -> LayerProps {
        p.layers()
            .iter()
            .find(|l| l.id == id)
            .unwrap()
            .props
            .clone()
    }

    #[test]
    fn layers_stack_hide_and_undo() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let first = p.active_layer();
        line(&mut p, &b, 30.0);
        let second = p.add_layer("Ink");
        assert_ne!(second, first);
        assert_eq!(p.active_layer(), second);
        line(&mut p, &b, 90.0);
        p.flush();
        let both = dark_pixels(&p);

        let mut hidden = props(&p, second);
        hidden.visible = false;
        assert!(p.set_layer_props(second, hidden, true));
        p.flush();
        let one = dark_pixels(&p);
        assert!(one < both && one > 0);
        // hidden layers can't be painted on
        assert!(!p.begin_stroke());

        assert!(p.undo()); // show again
        p.flush();
        assert_eq!(dark_pixels(&p), both);
        assert!(p.undo()); // second stroke
        assert!(p.undo()); // add layer
        assert_eq!(p.layers().len(), 1);
        assert_eq!(p.active_layer(), first);
        p.redo();
        p.redo();
        p.flush();
        assert_eq!(p.layers().len(), 2);
        assert_eq!(dark_pixels(&p), both);
    }

    #[test]
    fn opacity_and_blend_modes_change_the_mix() {
        let mut p = Painter::new(128, 64);
        let mut grey = pen();
        set_brush_rgb(&mut grey, 128, 128, 128);
        grey.get_mut(BrushSetting::Radius).base_value = 4.0;
        line(&mut p, &grey, 32.0);
        let top = p.add_layer("");
        line(&mut p, &grey, 32.0);
        p.flush();
        let normal = px(&p, 60, 32)[0];

        let mut m = props(&p, top);
        m.blend = Blend::Multiply;
        p.set_layer_props(top, m.clone(), true);
        p.flush();
        let multiply = px(&p, 60, 32)[0];
        assert!(
            multiply < normal,
            "multiply {multiply} should be darker than {normal}"
        );

        m.blend = Blend::Screen;
        p.set_layer_props(top, m.clone(), true);
        p.flush();
        assert!(px(&p, 60, 32)[0] > normal);

        m.blend = Blend::Normal;
        m.opacity = 0.0;
        p.set_layer_props(top, m, true);
        p.flush();
        // only the bottom grey line is left: same as a single layer of it
        assert_eq!(px(&p, 60, 32)[0], normal);
    }

    #[test]
    fn locked_layers_refuse_paint_and_alpha_lock_keeps_the_shape() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let id = p.active_layer();
        line(&mut p, &b, 60.0);
        p.flush();
        let before = dark_pixels(&p);

        let mut lock = props(&p, id);
        lock.locked = true;
        p.set_layer_props(id, lock.clone(), true);
        let steps = p.undo.len();
        assert!(!p.begin_stroke());
        assert!(!p.clear());
        assert_eq!(p.undo.len(), steps, "nothing recorded on a locked layer");

        lock.locked = false;
        lock.alpha_lock = true;
        p.set_layer_props(id, lock, true);
        // a vertical red stroke across the black line: only the overlap may change
        let mut red = pen();
        set_brush_rgb(&mut red, 220, 0, 0);
        p.begin_stroke();
        for i in 0..=30 {
            p.stroke_to(&red, 100.0, 20.0 + i as f32 * 3.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&red);
        p.flush();
        let painted = p
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|c| c[0] < 250 || c[1] < 250)
            .count();
        assert!(painted <= before + before / 10, "{painted} vs {before}");
        assert!(px(&p, 100, 30)[1] > 240, "no paint above the line");
        let on_line = px(&p, 100, 60);
        assert!(
            on_line[0] > on_line[1] + 40,
            "the line turned red: {on_line:?}"
        );
    }

    #[test]
    fn delete_duplicate_move_and_merge() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let bottom = p.active_layer();
        line(&mut p, &b, 30.0);
        let top = p.add_layer("Top");
        line(&mut p, &b, 90.0);
        p.flush();
        let image = p.rgba().to_vec();

        let copy = p.duplicate_layer(top);
        assert_eq!(p.layers().len(), 3);
        assert_eq!(props(&p, copy).name, "Top copy");
        assert!(p.delete_layer(copy));
        assert_eq!(p.layers().len(), 2);
        assert!(p.undo());
        assert_eq!(p.layers().len(), 3);
        assert!(p.undo()); // duplicate
        assert_eq!(p.layers().len(), 2);

        assert!(p.move_layer(top, 0));
        assert_eq!(p.layers()[0].id, top);
        assert!(p.undo());
        assert_eq!(p.layers()[1].id, top);

        // merging keeps the picture identical
        assert!(p.merge_down(top));
        assert_eq!(p.layers().len(), 1);
        assert_eq!(p.active_layer(), bottom);
        p.flush();
        let diff = p
            .rgba()
            .iter()
            .zip(&image)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert_eq!(diff, 0, "merged image differs in {diff} channels");
        assert!(p.undo());
        assert_eq!(p.layers().len(), 2);
        assert!(!p.layers()[0].is_empty() && !p.layers()[1].is_empty());

        // the last layer can't be deleted
        assert!(p.delete_layer(top));
        assert!(!p.delete_layer(bottom));
    }

    #[test]
    fn live_props_commit_as_one_step() {
        let mut p = Painter::new(64, 64);
        let id = p.active_layer();
        let before = props(&p, id);
        for o in [0.9, 0.7, 0.5] {
            let mut q = before.clone();
            q.opacity = o;
            p.set_layer_props(id, q, false);
        }
        assert!(!p.can_undo());
        assert!(p.commit_layer_props(id, before.clone()));
        assert_eq!(props(&p, id).opacity, 0.5);
        p.undo();
        assert_eq!(props(&p, id), before);
    }

    #[test]
    fn json_and_thumbnails() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0);
        p.add_layer("say \"hi\"");
        let j = p.layers_json();
        assert!(j.starts_with(r#"[{"id":1,"name":"Layer 1""#), "{j}");
        assert!(j.contains(r#""name":"say \"hi\"""#), "{j}");
        let t = p.layer_thumbnail(1, 32, 16);
        assert_eq!(t.len(), 32 * 16 * 4);
        assert!(
            t.chunks(4).any(|c| c[3] > 200),
            "the line shows in the thumbnail"
        );
        // a 1-px hairline on a big canvas still shows at preview size
        let mut big = Painter::new(800, 600);
        let mut hair = pen();
        hair.get_mut(BrushSetting::Radius).base_value = 0.0; // 1 px
        big.begin_stroke();
        for i in 0..=60 {
            big.stroke_to(&hair, 100.0 + i as f32 * 10.0, 300.0, 1.0, 0.0, 0.0, 0.01);
        }
        big.end_stroke(&hair);
        let t = big.layer_thumbnail(1, 48, 36);
        assert!(
            t.chunks(4).filter(|c| c[3] > 60).count() >= 20,
            "hairline visible"
        );
        assert!(p.layer_thumbnail(2, 8, 8).iter().all(|&v| v == 0));
    }

    #[test]
    fn cancelled_strokes_leave_no_trace() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 30.0);
        p.flush();
        let before = p.rgba().to_vec();
        p.begin_stroke();
        for i in 0..=20 {
            p.stroke_to(&b, 20.0 + i as f32 * 8.0, 90.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.cancel_stroke();
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        assert!(
            p.undo() && !p.can_undo(),
            "only the first stroke is in the history"
        );
    }

    #[test]
    fn remote_strokes_paint_alongside_the_local_one() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let theirs = p.add_layer_silent("Sam");
        assert!(!p.can_undo(), "a layer for someone else isn't an undo step");
        // local stroke on layer 1 and a remote stroke on Sam's layer, interleaved
        p.begin_stroke();
        assert!(p.remote_begin(7, theirs));
        for i in 0..=30 {
            p.stroke_to(&b, 20.0 + i as f32 * 6.0, 30.0, 1.0, 0.0, 0.0, 0.01);
            p.remote_to(7, &b, 20.0 + i as f32 * 6.0, 90.0, 1.0, 0.01);
        }
        p.remote_end(7, &b);
        p.end_stroke(&b);
        p.flush();
        assert!(
            px(&p, 100, 30)[0] < 100 && px(&p, 100, 90)[0] < 100,
            "both lines show"
        );
        // undo removes only the local line
        p.undo();
        p.flush();
        assert!(px(&p, 100, 30)[0] > 240 && px(&p, 100, 90)[0] < 100);
        p.clear_layer_silent(theirs);
        p.flush();
        assert!(px(&p, 100, 90)[0] > 240);
        assert!(!p.can_undo());
    }

    /// A closed square outline from (40,20) to (120,100).
    fn dense_square(p: &mut Painter) {
        let mut b = pen();
        b.get_mut(BrushSetting::Radius).base_value = 1.0;
        let corners = [
            (40.0f32, 20.0f32),
            (120.0, 20.0),
            (120.0, 100.0),
            (40.0, 100.0),
            (40.0, 20.0),
        ];
        p.begin_stroke();
        for w in corners.windows(2) {
            for k in 0..=40 {
                let t = k as f32 / 40.0;
                let (x, y) = (
                    w[0].0 + (w[1].0 - w[0].0) * t,
                    w[0].1 + (w[1].1 - w[0].1) * t,
                );
                p.stroke_to(&b, x, y, 1.0, 0.0, 0.0, 0.002);
            }
        }
        p.end_stroke(&b);
    }

    #[test]
    fn fill_stays_inside_lines_and_tucks_under_them() {
        let mut p = Painter::new(256, 128);
        dense_square(&mut p);
        p.flush();
        assert!(p.fill(80, 60, [220, 0, 0], 1.0, 0.1, true, 2, 0));
        p.flush();
        let inside = px(&p, 80, 60);
        assert!(
            inside[0] > 200 && inside[1] < 40,
            "inside is red: {inside:?}"
        );
        assert!(px(&p, 200, 60)[1] > 240, "outside untouched");
        // the line is still dark on top
        assert!(px(&p, 40, 60)[0] < 90, "line on top: {:?}", px(&p, 40, 60));
        assert!(p.undo());
        p.flush();
        assert!(px(&p, 80, 60)[1] > 240);
        // clicking outside fills the outside, not the inside
        assert!(p.fill(200, 60, [0, 0, 220], 1.0, 0.1, true, 0, 0));
        p.flush();
        assert!(
            px(&p, 80, 60)[2] > 240 && px(&p, 80, 60)[0] > 240,
            "inside stays white"
        );
        assert!(px(&p, 200, 60)[0] < 40);
    }

    #[test]
    fn gap_closing_stops_leaks_through_small_gaps() {
        let mut p = Painter::new(256, 128);
        dense_square(&mut p);
        // cut a 3 px gap in the left side of the square
        let mut e = pen();
        e.get_mut(BrushSetting::Eraser).base_value = 1.0;
        e.get_mut(BrushSetting::Radius).base_value = 0.6;
        p.begin_stroke();
        for k in 0..=6 {
            p.stroke_to(&e, 34.0 + k as f32 * 2.0, 60.0, 1.0, 0.0, 0.0, 0.002);
        }
        p.end_stroke(&e);
        p.flush();
        // without gap closing the fill escapes through the gap
        assert!(p.fill(80, 60, [220, 0, 0], 1.0, 0.1, true, 1, 0));
        p.flush();
        assert!(
            px(&p, 200, 60)[1] < 60,
            "leaked (as expected without gap closing)"
        );
        p.undo();
        // with it, the outside stays white and the inside is filled
        assert!(p.fill(80, 60, [220, 0, 0], 1.0, 0.1, true, 1, 3));
        p.flush();
        assert!(px(&p, 200, 60)[1] > 240, "no leak");
        assert!(px(&p, 80, 60)[1] < 60, "inside filled");
        assert!(
            px(&p, 44, 40)[1] < 90,
            "fill reaches the lines: {:?}",
            px(&p, 44, 40)
        );
    }

    #[test]
    fn fill_respects_lock_and_layer_sampling() {
        let mut p = Painter::new(256, 128);
        dense_square(&mut p);
        let top = p.add_layer("Colour");
        // sampling only the empty top layer: everything is one region
        assert!(p.fill(80, 60, [0, 200, 0], 1.0, 0.1, false, 0, 0));
        p.flush();
        assert!(px(&p, 200, 60)[1] > 150 && px(&p, 200, 60)[0] < 60);
        p.undo();
        let mut lock = props(&p, top);
        lock.locked = true;
        p.set_layer_props(top, lock, true);
        assert!(!p.fill(80, 60, [0, 200, 0], 1.0, 0.1, true, 0, 0));
    }

    /// a selection mask for the rectangle x0..x1, y0..y1
    fn rect_mask(p: &Painter, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
        let (w, h) = (p.width(), p.height());
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    255
                } else {
                    0
                }
            })
            .collect()
    }

    #[test]
    fn strokes_stay_inside_the_selection_and_undo_cleanly() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let m = rect_mask(&p, 0, 0, 128, 128);
        assert!(p.set_selection(&m, 0));
        line(&mut p, &b, 60.0); // from x 20 to 180
        p.flush();
        assert!(px(&p, 60, 60)[0] < 100, "inside painted");
        assert!(px(&p, 160, 60)[0] > 240, "outside untouched");
        p.undo();
        p.flush();
        assert_eq!(dark_pixels(&p), 0);
        // with no selection, the whole line paints
        p.deselect();
        line(&mut p, &b, 60.0);
        p.flush();
        assert!(px(&p, 160, 60)[0] < 100);
    }

    #[test]
    fn selection_ops_wand_fill_and_clear() {
        let mut p = Painter::new(256, 128);
        dense_square(&mut p);
        // magic wand inside the square selects its inside only
        assert!(p.select_wand(80, 60, 0.1, true, 0));
        let sel = p.selection_mask();
        assert!(sel[60 * 256 + 80] > 0 && sel[60 * 256 + 200] == 0);
        // subtract the left half of the canvas
        let left = rect_mask(&p, 0, 0, 80, 128);
        p.set_selection(&left, 2);
        assert!(p.selection_mask()[60 * 256 + 60] == 0 && p.selection_mask()[60 * 256 + 100] > 0);
        // fill only paints selected pixels
        assert!(p.fill(100, 60, [220, 0, 0], 1.0, 0.1, true, 2, 0));
        p.flush();
        assert!(px(&p, 100, 60)[1] < 60 && px(&p, 60, 60)[1] > 240);
        // invert, then clear: only the unselected... i.e. now-selected outside clears the line
        p.invert_selection();
        assert!(p.clear());
        p.flush();
        assert!(
            px(&p, 40, 60)[0] > 240,
            "the left edge of the square was cleared"
        );
        assert!(
            px(&p, 100, 60)[1] < 60,
            "the red inside (not selected now) stays"
        );
        p.undo();
        p.flush();
        assert!(px(&p, 40, 60)[0] < 100);
        // select all / deselect
        p.select_all();
        assert!(p.has_selection());
        p.deselect();
        assert!(!p.has_selection() && p.selection_mask().is_empty());
        // subtracting everything leaves no selection
        p.select_all();
        p.set_selection(&rect_mask(&p, 0, 0, 256, 128), 2);
        assert!(!p.has_selection());
    }

    #[test]
    fn transform_moves_and_undoes() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 30.0);
        p.flush();
        let before = p.rgba().to_vec();
        let bbox = p.transform_begin();
        assert_eq!(bbox.len(), 4);
        assert!(bbox[1] < 30.0 && bbox[3] > 30.0);
        assert!(p.transform_preview([1.0, 0.0, 0.0, 1.0, 0.0, 50.0]));
        p.flush();
        assert!(
            px(&p, 100, 30)[0] > 240 && px(&p, 100, 80)[0] < 100,
            "moved down 50"
        );
        assert!(!p.begin_stroke(), "no painting mid-transform");
        assert!(p.transform_commit());
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        assert!(p.redo());
        p.flush();
        assert!(px(&p, 100, 80)[0] < 100);
    }

    #[test]
    fn transform_lifts_only_the_selection_and_cancel_restores() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0); // x 20..180
        p.flush();
        let before = p.rgba().to_vec();
        p.set_selection(&rect_mask(&p, 0, 0, 100, 128), 0);
        assert!(!p.transform_begin().is_empty());
        p.transform_preview([1.0, 0.0, 0.0, 1.0, 0.0, -40.0]);
        p.flush();
        assert!(px(&p, 60, 20)[0] < 100, "left part moved up");
        assert!(px(&p, 60, 60)[0] > 240, "and left its old place");
        assert!(px(&p, 150, 60)[0] < 100, "right part stayed");
        p.transform_cancel();
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        // flip horizontally about x = 128: the line's left end (x 20) lands near x 236
        p.deselect();
        p.transform_begin();
        p.transform_preview([-1.0, 0.0, 0.0, 1.0, 256.0, 0.0]);
        assert!(p.transform_commit());
        p.flush();
        assert!(px(&p, 230, 60)[0] < 100 && px(&p, 30, 60)[0] > 240);
        // the selection follows the pixels
        let mut q = Painter::new(256, 128);
        line(&mut q, &b, 60.0);
        q.set_selection(&rect_mask(&q, 0, 0, 50, 128), 0);
        q.transform_begin();
        q.transform_preview([1.0, 0.0, 0.0, 1.0, 100.0, 0.0]);
        q.transform_commit();
        let sel = q.selection_mask();
        assert!(sel[60 * 256 + 120] > 0 && sel[60 * 256 + 20] == 0);
        // a locked layer can't be transformed
        let id = q.active_layer();
        let mut lock = props(&q, id);
        lock.locked = true;
        q.set_layer_props(id, lock, true);
        assert!(q.transform_begin().is_empty());
    }

    #[test]
    fn liquify_pushes_paint_and_undoes() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0); // horizontal at y 60
        p.flush();
        let before = p.rgba().to_vec();
        assert!(p.liquify_begin());
        for _ in 0..10 {
            p.liquify_push(100.0, 60.0, 0.0, 3.0, 25.0, 1.0); // push down at x 100
        }
        assert!(p.liquify_end());
        p.flush();
        assert!(
            px(&p, 100, 60)[0] > 200,
            "the line moved away from y 60 at x 100"
        );
        let moved = (61..=100).any(|y| px(&p, 100, y)[0] < 120);
        assert!(moved, "…to somewhere below");
        assert!(px(&p, 30, 60)[0] < 120, "far away it didn't move");
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        // nothing to push: no undo step
        assert!(p.liquify_begin());
        p.liquify_push(200.0, 10.0, 3.0, 0.0, 10.0, 1.0);
        assert!(!p.liquify_end());
    }

    #[test]
    fn hsv_conversion() {
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        assert_eq!((h, s, v), (0.0, 1.0, 1.0));
        let (h, _, _) = rgb_to_hsv(0.0, 0.0, 1.0);
        assert!((h - 2.0 / 3.0).abs() < 1e-6);
    }
}
