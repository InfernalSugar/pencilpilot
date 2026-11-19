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
use layers::{TileSource, composite_pixel, json_str, merge_pixel, unpremul_srgb};

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
        }
    }
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
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            layers: vec![Layer::new(1, LayerProps::named("Layer 1"))],
            active: 1,
            next_id: 2,
            state: BrushState::default(),
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
            undo: Vec::new(),
            redo: Vec::new(),
            stroking: None,
            stroke_brush: None,
            alpha_lock_stroke: false,
            dirty: HashSet::new(),
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
    }

    /// Finish the stroke (flushes slow-tracking lag) and record it for undo.
    pub fn end_stroke(&mut self, brush: &Brush) {
        let Some(id) = self.stroking.take() else {
            return;
        };
        self.locked_copy(brush);
        let b = self.stroke_brush.take();
        let b = b.as_ref().unwrap_or(brush);
        let snap = self.layers.iter_mut().find(|l| l.id == id).and_then(|l| {
            b.finish_stroke(&mut self.state, &mut l.surface);
            l.surface.recording.take()
        });
        self.state.reset();
        self.alpha_lock_stroke = false;
        // Always record a history step, even if no pixels changed, so the
        // web app's history and this undo stack stay one-to-one.
        self.push_undo(Edit::Tiles {
            layer: id,
            snap: snap.unwrap_or_default(),
        });
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
        let snap: Snapshot = l.surface.tiles.drain().map(|(k, t)| (k, Some(t))).collect();
        l.surface.dirty.extend(snap.keys().copied());
        self.push_undo(Edit::Tiles {
            layer: active,
            snap,
        });
        true
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
    fn hsv_conversion() {
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        assert_eq!((h, s, v), (0.0, 1.0, 1.0));
        let (h, _, _) = rgb_to_hsv(0.0, 0.0, 1.0);
        assert!((h - 2.0 / 3.0).abs() < 1e-6);
    }
}
