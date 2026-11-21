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
//! The engine is plain Rust ([`Painter`]), unit-tested natively. The app uses
//! it directly; the `pencilpilot-brush-wasm` crate exposes it to JavaScript for
//! the website.

use std::collections::{HashMap, HashSet};

use hokusai::tile::empty_tile;
use hokusai::{Brush, BrushSetting, BrushState, TILE_SIZE, TilePixels, TiledSurface};

pub mod adjust;
pub mod layers;
pub mod texture;

pub use layers::{Blend, LayerProps};
use layers::{
    TileSource, composite_pixel_on, json_str, mask_value, merge_pixel, premul_fix15, unpremul_srgb,
};

pub(crate) type TileKey = (i32, i32);
pub(crate) type TileMap = HashMap<TileKey, Box<TilePixels>>;
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
    /// grain / tip / dual brush for the stroke being painted (see [`texture`])
    style: Option<std::sync::Arc<texture::DabStyle>>,
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

    /// libmypaint's dab, then (with a texture) each pixel's change scaled by
    /// how much the grain / tip / dual brush lets through there.
    fn draw_dab(&mut self, dab: &hokusai::Dab) -> bool {
        let Some(style) = self.style.clone() else {
            return hokusai::brushmodes::draw_dab_default(self, dab);
        };
        let r = dab.radius + 2.0;
        let (x0, y0) = ((dab.x - r).floor() as i64, (dab.y - r).floor() as i64);
        let (x1, y1) = ((dab.x + r).ceil() as i64, (dab.y + r).ceil() as i64);
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let ts = TILE_SIZE as i64;
        let px_at = |s: &Self, x: i64, y: i64| -> [u16; 4] {
            s.tiles
                .get(&(x.div_euclid(ts) as i32, y.div_euclid(ts) as i32))
                .map_or([0; 4], |t| {
                    t[y.rem_euclid(ts) as usize][x.rem_euclid(ts) as usize]
                })
        };
        let mut before = Vec::with_capacity(w * h);
        for y in y0..y1 {
            for x in x0..x1 {
                before.push(px_at(self, x, y));
            }
        }
        if !hokusai::brushmodes::draw_dab_default(self, dab) {
            return false;
        }
        let stamps = style.stamps(dab);
        for y in y0..y1 {
            for x in x0..x1 {
                let key = (x.div_euclid(ts) as i32, y.div_euclid(ts) as i32);
                let Some(tile) = self.tiles.get_mut(&key) else {
                    continue;
                };
                let p = &mut tile[y.rem_euclid(ts) as usize][x.rem_euclid(ts) as usize];
                let b = before[(y - y0) as usize * w + (x - x0) as usize];
                if *p == b {
                    continue;
                }
                let f = style.factor(dab, &stamps, x as f32 + 0.5, y as f32 + 0.5);
                if f < 0.999 {
                    *p = lerp_px(b, *p, f);
                }
            }
        }
        true
    }
}

/// A composited tile: its area on the canvas and its pixels (sRGB, row by row).
struct ComposedTile {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    px: Vec<[u8; 3]>,
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
    /// the pixels tools paint on: the colour, or the mask while editing it
    surface: UndoSurface,
    /// layer mask (white shows, black hides): holds the colour while editing the mask
    mask: Option<UndoSurface>,
    /// `surface` and `mask` are swapped: tools paint on the mask
    editing_mask: bool,
    /// the group this layer is in (0 = none); a group's layers are always next to each other
    group: u32,
    /// the drawing (animation cel) showing on this layer; 0 = none yet (the first
    /// change gives it one). Other drawings wait in [`Painter::cels`].
    drawing: u32,
}

/// A layer group ("pass-through": its visibility and opacity apply to each of its layers).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
}

impl Layer {
    fn new(id: u32, props: LayerProps) -> Self {
        Self {
            id,
            props,
            surface: UndoSurface::default(),
            mask: None,
            editing_mask: false,
            group: 0,
            drawing: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.colour().tiles.is_empty()
    }

    pub fn has_mask(&self) -> bool {
        self.mask.is_some()
    }

    fn colour(&self) -> &UndoSurface {
        match (&self.mask, self.editing_mask) {
            (Some(m), true) => m,
            _ => &self.surface,
        }
    }

    fn colour_mut(&mut self) -> &mut UndoSurface {
        match (&mut self.mask, self.editing_mask) {
            (Some(m), true) => m,
            _ => &mut self.surface,
        }
    }

    fn mask_surface(&self) -> Option<&UndoSurface> {
        if self.editing_mask {
            Some(&self.surface)
        } else {
            self.mask.as_ref()
        }
    }

    /// Point the tools at the mask (or back at the colour).
    fn set_editing(&mut self, on: bool) {
        if on != self.editing_mask
            && let Some(m) = self.mask.as_mut()
        {
            std::mem::swap(&mut self.surface, m);
            self.editing_mask = on;
        }
    }
}

/// Colour tiles with a mask applied (premultiplied, so every channel scales).
fn bake(colour: &TileMap, mask: Option<&TileMap>) -> TileMap {
    let Some(mask) = mask else {
        return colour.clone();
    };
    colour
        .iter()
        .map(|(k, t)| {
            let mut t = t.clone();
            if let Some(m) = mask.get(k) {
                for (row, mrow) in t.iter_mut().zip(m.iter()) {
                    for (p, mp) in row.iter_mut().zip(mrow.iter()) {
                        let v = mask_value(*mp);
                        for c in p.iter_mut() {
                            *c = (*c as f32 * v).round() as u16;
                        }
                    }
                }
            }
            (*k, t)
        })
        .collect()
}

impl Clone for UndoSurface {
    fn clone(&self) -> Self {
        Self {
            tiles: self.tiles.clone(),
            recording: None,
            dirty: HashSet::new(),
            touched: HashSet::new(),
            style: None,
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
    /// current projective matrix, row-major: dest = (h0 x + h1 y + h2, h3 x + h4 y + h5) / (h6 x + h7 y + h8)
    m: [f32; 9],
    /// after a warp: the box (inclusive) and which of its pixels the lifted paint
    /// now covers, row by row (the selection follows it)
    cover: Option<(PixelBox, Vec<bool>)>,
}

/// A box of canvas pixels: (x0, y0, x1, y1), inclusive.
type PixelBox = (i64, i64, i64, i64);

/// Part of a transform drawn with one projective map (see `render_float`).
struct Piece {
    /// destination → source
    inv: [f32; 9],
    /// destination box it may draw in (canvas px, inclusive)
    bbox: (i64, i64, i64, i64),
    /// source rectangle it owns (canvas px)
    src: (f32, f32, f32, f32),
}

/// Mirror / rotation copies of every stroke, about (cx, cy).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymmetryMode {
    Vertical,
    Horizontal,
    Quadrant,
    /// `n` copies rotated evenly around the centre (2–24)
    Radial(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Symmetry {
    pub mode: SymmetryMode,
    pub cx: f32,
    pub cy: f32,
}

impl Symmetry {
    /// Where the copies of point (x, y) go (not including (x, y) itself).
    pub fn copies(&self, x: f32, y: f32) -> Vec<(f32, f32)> {
        let (cx, cy) = (self.cx, self.cy);
        match self.mode {
            SymmetryMode::Vertical => vec![(2.0 * cx - x, y)],
            SymmetryMode::Horizontal => vec![(x, 2.0 * cy - y)],
            SymmetryMode::Quadrant => vec![
                (2.0 * cx - x, y),
                (x, 2.0 * cy - y),
                (2.0 * cx - x, 2.0 * cy - y),
            ],
            SymmetryMode::Radial(n) => {
                let n = n.clamp(2, 24);
                (1..n)
                    .map(|k| {
                        let a = k as f32 * std::f32::consts::TAU / n as f32;
                        let (dx, dy) = (x - cx, y - cy);
                        (
                            cx + dx * a.cos() - dy * a.sin(),
                            cy + dx * a.sin() + dy * a.cos(),
                        )
                    })
                    .collect()
            }
        }
    }
}

/// One undoable change. Applying an edit returns its inverse.
enum Edit {
    /// Swap tiles of one layer (strokes, clearing, merging).
    Tiles {
        layer: u32,
        snap: Snapshot,
        /// the tiles are the layer's mask, not its colour
        mask: bool,
        /// the drawing (animation cel) the colour tiles belong to; 0 = not tagged yet
        /// ([`Painter::push_undo`] tags it with the layer's drawing)
        drawing: u32,
    },
    /// Replace the whole group list (add / remove / change a group).
    Groups(Vec<Group>),
    /// Put a layer in a group (0 = none).
    Member {
        layer: u32,
        group: u32,
    },
    /// Swap a layer's mask (add / delete one).
    SetMask {
        layer: u32,
        mask: Option<Box<UndoSurface>>,
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
    /// A new canvas size: every layer's colour and mask tiles, by layer id.
    Canvas {
        width: u32,
        height: u32,
        tiles: Vec<(u32, TileMap, Option<TileMap>)>,
    },
    Batch(Vec<Edit>),
    /// A step of the app's own (the animation timeline changed): no pixels, just its
    /// place in the history, so undo and redo come back to the app in order.
    Note(u32),
}

/// The canvas: a stack of layers (bottom → top) over white paper, the
/// in-flight stroke, history, and an RGBA8 (sRGB) output buffer.
pub struct Painter {
    layers: Vec<Layer>,
    active: u32,
    next_id: u32,
    groups: Vec<Group>,
    next_group: u32,
    state: BrushState,
    /// strokes from other people (shared canvases), drawn alongside the local
    /// one: key → (layer id, brush state). Never in the undo history.
    remote: HashMap<u32, (u32, BrushState)>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// counts changes to the history (new edits, undo, redo)
    edits: u64,
    /// the last selection fill, while it's still the latest edit (`edits` then)
    sel_fill_at: Option<u64>,
    /// drawings (animation cels) that aren't showing on their layer right now
    cels: HashMap<u32, TileMap>,
    next_drawing: u32,
    /// the layer the current stroke paints on
    stroking: Option<u32>,
    /// alpha-locked strokes paint with a copy of the brush that has lock_alpha on
    stroke_brush: Option<Brush>,
    alpha_lock_stroke: bool,
    /// tiles to recomposite (structural changes); strokes use each surface's own set
    dirty: HashSet<TileKey>,
    /// the selection: 255 = selected, per canvas pixel (None = no selection)
    selection: Option<Vec<u8>>,
    /// an adjustment / gradient being previewed: (layer, its tiles before)
    adjusting: Option<(u32, TileMap)>,
    /// a transform in progress (see `transform_begin`)
    transform: Option<Box<TransformSession>>,
    /// a liquify stroke in progress: (layer, its tiles before the stroke)
    liquify: Option<(u32, TileMap)>,
    /// symmetry: every stroke is also drawn mirrored / rotated (see `set_symmetry`)
    symmetry: Option<Symmetry>,
    /// brush states for the mirrored copies of the current stroke
    sym_states: Vec<BrushState>,
    /// grain / tip pictures by id (see `set_texture`)
    textures: HashMap<u32, std::sync::Arc<texture::Gray>>,
    /// how the next strokes are textured (see `set_dab_style`)
    style: Option<std::sync::Arc<texture::DabStyle>>,
    /// an adjustment layer being changed live: (layer, its settings before)
    adjust_layer_before: Option<(u32, LayerProps)>,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            layers: vec![Layer::new(1, LayerProps::named("Layer 1"))],
            active: 1,
            next_id: 2,
            groups: Vec::new(),
            next_group: 1,
            state: BrushState::default(),
            remote: HashMap::new(),
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
            undo: Vec::new(),
            redo: Vec::new(),
            edits: 0,
            sel_fill_at: None,
            cels: HashMap::new(),
            next_drawing: 1,
            stroking: None,
            stroke_brush: None,
            alpha_lock_stroke: false,
            dirty: HashSet::new(),
            selection: None,
            adjusting: None,
            transform: None,
            liquify: None,
            symmetry: None,
            sym_states: Vec::new(),
            textures: HashMap::new(),
            style: None,
            adjust_layer_before: None,
        }
    }

    // ---- brush textures -------------------------------------------------

    /// Store a grey picture (255 = paint) as texture `id` for grain, tips and
    /// dual brushes. An empty picture removes it.
    pub fn set_texture(&mut self, id: u32, w: u32, h: u32, px: Vec<u8>) -> bool {
        match texture::Gray::new(w, h, px) {
            Some(g) => {
                self.textures.insert(id, std::sync::Arc::new(g));
                true
            }
            None => {
                self.textures.remove(&id);
                false
            }
        }
    }

    /// Texture the next strokes: grain (texture id, canvas px per texel,
    /// strength 0–1), tip picture, and dual brush (tip, size and scatter as
    /// fractions of the brush radius, stamps per dab). Id 0 = none.
    #[allow(clippy::too_many_arguments)]
    pub fn set_dab_style(
        &mut self,
        grain: u32,
        grain_scale: f32,
        grain_strength: f32,
        tip: u32,
        dual: u32,
        dual_size: f32,
        dual_scatter: f32,
        dual_count: u32,
    ) {
        let tex = |id: u32| self.textures.get(&id).cloned();
        let style = texture::DabStyle {
            grain: tex(grain)
                .filter(|_| grain_strength > 0.0)
                .map(|t| texture::Grain {
                    tex: t,
                    scale: grain_scale,
                    strength: grain_strength,
                }),
            tip: tex(tip),
            dual: tex(dual).map(|t| texture::Dual {
                tip: t,
                size: dual_size.clamp(0.05, 2.0),
                scatter: dual_scatter.clamp(0.0, 2.0),
                count: dual_count.clamp(1, 12),
            }),
        };
        self.style = (!style.is_plain()).then(|| std::sync::Arc::new(style));
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
        if id != self.active && self.idle() {
            self.stop_mask_editing();
        }
        self.active = id;
        true
    }

    fn mark_layer(&mut self, idx: usize) {
        if self.layers[idx].props.adjust.is_some() {
            self.mark_all();
            return;
        }
        let l = &self.layers[idx];
        let mut keys: Vec<TileKey> = l.surface.tiles.keys().copied().collect();
        if let Some(m) = &l.mask {
            keys.extend(m.tiles.keys().copied());
        }
        self.dirty.extend(keys);
    }

    /// Recomposite every tile of the canvas.
    fn mark_all(&mut self) {
        let ts = TILE_SIZE as u32;
        let (tw, th) = (
            self.width.div_ceil(ts) as i32,
            self.height.div_ceil(ts) as i32,
        );
        for ty in 0..th {
            for tx in 0..tw {
                self.dirty.insert((tx, ty));
            }
        }
    }

    /// A layer's group's (visible, opacity); (true, 1) outside a group.
    fn group_of(&self, l: &Layer) -> (bool, f32) {
        self.groups
            .iter()
            .find(|g| g.id == l.group)
            .map_or((true, 1.0), |g| (g.visible, g.opacity))
    }

    /// Shown on the canvas (its own eye and its group's).
    fn shown(&self, idx: usize) -> bool {
        let l = &self.layers[idx];
        l.props.visible && self.group_of(l).0
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Indices of a group's layers (bottom → top, next to each other).
    fn members(&self, gid: u32) -> Vec<usize> {
        (0..self.layers.len())
            .filter(|&i| gid != 0 && self.layers[i].group == gid)
            .collect()
    }

    /// Put layer `id` in a new group. Returns the group's id (0 = couldn't).
    pub fn add_group(&mut self, id: u32) -> u32 {
        if !self.idle() || self.index_of(id).is_none() {
            return 0;
        }
        let gid = self.next_group;
        self.next_group += 1;
        let mut list = self.groups.clone();
        list.push(Group {
            id: gid,
            name: format!("Group {gid}"),
            visible: true,
            opacity: 1.0,
        });
        self.perform(Edit::Batch(vec![
            Edit::Groups(list),
            Edit::Member {
                layer: id,
                group: gid,
            },
        ]));
        gid
    }

    /// Change a group's name, visibility or opacity. `record` = false previews
    /// (a slider drag); finish with `commit_group` and the values from before.
    pub fn set_group(
        &mut self,
        gid: u32,
        name: &str,
        visible: bool,
        opacity: f32,
        record: bool,
    ) -> bool {
        let Some(i) = self.groups.iter().position(|g| g.id == gid) else {
            return false;
        };
        let mut list = self.groups.clone();
        let name: String = name.chars().take(60).collect();
        if !name.trim().is_empty() {
            list[i].name = name;
        }
        list[i].visible = visible;
        list[i].opacity = opacity.clamp(0.0, 1.0);
        if list == self.groups {
            return false;
        }
        if record {
            self.perform(Edit::Groups(list));
        } else {
            self.apply(Edit::Groups(list));
        }
        true
    }

    /// One undo step for a previewed change: `before` values are what it was.
    pub fn commit_group(&mut self, gid: u32, name: &str, visible: bool, opacity: f32) -> bool {
        let Some(i) = self.groups.iter().position(|g| g.id == gid) else {
            return false;
        };
        let mut before = self.groups.clone();
        before[i].name = name.chars().take(60).collect();
        before[i].visible = visible;
        before[i].opacity = opacity;
        if before == self.groups {
            return false;
        }
        self.push_undo(Edit::Groups(before));
        true
    }

    /// Take the layers out of a group and remove it.
    pub fn ungroup(&mut self, gid: u32) -> bool {
        if !self.idle() || !self.groups.iter().any(|g| g.id == gid) {
            return false;
        }
        let mut edits: Vec<Edit> = self
            .members(gid)
            .into_iter()
            .map(|i| Edit::Member {
                layer: self.layers[i].id,
                group: 0,
            })
            .collect();
        edits.push(Edit::Groups(
            self.groups
                .iter()
                .filter(|g| g.id != gid)
                .cloned()
                .collect(),
        ));
        self.perform(Edit::Batch(edits));
        true
    }

    /// Delete a group and its layers (not if that would leave no layers).
    pub fn delete_group(&mut self, gid: u32) -> bool {
        let m = self.members(gid);
        if !self.idle() || m.len() >= self.layers.len() || !self.groups.iter().any(|g| g.id == gid)
        {
            return false;
        }
        self.stop_mask_editing();
        let mut edits: Vec<Edit> = m
            .iter()
            .rev()
            .map(|&index| Edit::Remove { index })
            .collect();
        edits.push(Edit::Groups(
            self.groups
                .iter()
                .filter(|g| g.id != gid)
                .cloned()
                .collect(),
        ));
        self.perform(Edit::Batch(edits));
        if self.index_of(self.active).is_none() {
            self.active = self.layers[m[0].saturating_sub(1).min(self.layers.len() - 1)].id;
        }
        true
    }

    /// Flatten a group's visible layers into one layer (named after the group,
    /// with the group's opacity). One undo step. Returns the layer's id.
    pub fn merge_group(&mut self, gid: u32) -> u32 {
        let m = self.members(gid);
        let Some(g) = self.groups.iter().find(|g| g.id == gid).cloned() else {
            return 0;
        };
        if !self.idle() || m.is_empty() {
            return 0;
        }
        self.stop_mask_editing();
        // composite the members onto transparency
        let mut out: TileMap = TileMap::new();
        let mut base: Option<TileMap> = None;
        for &i in &m {
            let l = &self.layers[i];
            let baked = bake(&l.surface.tiles, l.mask.as_ref().map(|mk| &mk.tiles));
            if !l.props.clip {
                base = Some(baked.clone());
            }
            if !l.props.visible || l.props.opacity <= 0.0 {
                continue;
            }
            if let Some(adj) = l.props.adjust.as_deref() {
                out = Self::adjusted(
                    &out,
                    adj,
                    l.props.opacity,
                    l.mask.as_ref().map(|m| &m.tiles),
                );
                continue;
            }
            for (key, src) in &baked {
                let o = out.entry(*key).or_insert_with(empty_tile);
                let bt = base.as_ref().and_then(|b| b.get(key));
                for (r, (orow, srow)) in o.iter_mut().zip(src.iter()).enumerate() {
                    for (c, (op, sp)) in orow.iter_mut().zip(srow.iter()).enumerate() {
                        let k = if l.props.clip {
                            bt.map_or(0.0, |t| t[r][c][3] as f32 / 32768.0)
                        } else {
                            1.0
                        };
                        *op = merge_pixel(*op, *sp, l.props.opacity * k, l.props.blend);
                    }
                }
            }
        }
        let bottom = &self.layers[m[0]];
        let bottom_id = bottom.id;
        let mut snap: Snapshot = bottom.surface.tiles.keys().map(|k| (*k, None)).collect();
        for (k, t) in out {
            snap.insert(k, Some(t));
        }
        let mut props = LayerProps::named(g.name.clone());
        props.opacity = g.opacity;
        props.visible = g.visible;
        props.imported = m.iter().any(|&i| self.layers[i].props.imported);
        let mut edits = vec![Edit::Tiles {
            drawing: 0,
            layer: bottom_id,
            snap,
            mask: false,
        }];
        if bottom.mask.is_some() {
            edits.push(Edit::SetMask {
                layer: bottom_id,
                mask: None,
            });
        }
        edits.push(Edit::Props {
            layer: bottom_id,
            props,
        });
        edits.push(Edit::Member {
            layer: bottom_id,
            group: 0,
        });
        edits.extend(m[1..].iter().rev().map(|&index| Edit::Remove { index }));
        edits.push(Edit::Groups(
            self.groups
                .iter()
                .filter(|x| x.id != gid)
                .cloned()
                .collect(),
        ));
        self.perform(Edit::Batch(edits));
        self.active = bottom_id;
        bottom_id
    }

    /// Move a group's layers together past the next layer (or whole group) above / below.
    pub fn move_group(&mut self, gid: u32, up: bool) -> bool {
        let m = self.members(gid);
        if !self.idle() || m.is_empty() {
            return false;
        }
        let (a, b) = (m[0], *m.last().unwrap());
        let n = self.layers.len();
        let mut edits = Vec::new();
        if up {
            if b + 1 >= n {
                return false;
            }
            let h = self.layers[b + 1].group;
            let mut c = b + 1;
            while h != 0 && c + 1 < n && self.layers[c + 1].group == h {
                c += 1;
            }
            for k in 0..(c - b) {
                edits.push(Edit::Move {
                    from: b + 1 + k,
                    to: a + k,
                });
            }
        } else {
            if a == 0 {
                return false;
            }
            let h = self.layers[a - 1].group;
            let mut d = a - 1;
            while h != 0 && d > 0 && self.layers[d - 1].group == h {
                d -= 1;
            }
            for _ in 0..(a - d) {
                edits.push(Edit::Move { from: d, to: b });
            }
        }
        self.perform(Edit::Batch(edits));
        true
    }

    /// Move a layer to `to` (0 = bottom) and into `group` (0 = none), as one step.
    pub fn move_layer_to(&mut self, id: u32, to: usize, group: u32) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let to = to.min(self.layers.len() - 1);
        let group = if self.groups.iter().any(|g| g.id == group) {
            group
        } else {
            0
        };
        let same_group = self.layers[from].group == group;
        if self.stroking.is_some() || (from == to && same_group) {
            return false;
        }
        let mut edits = Vec::new();
        if from != to {
            edits.push(Edit::Move { from, to });
        }
        if !same_group {
            edits.push(Edit::Member { layer: id, group });
        }
        self.perform(Edit::Batch(edits));
        true
    }

    /// The groups as JSON (for the layers panel).
    pub fn groups_json(&self) -> String {
        let items: Vec<String> = self
            .groups
            .iter()
            .map(|g| {
                format!(
                    r#"{{"id":{},"name":{},"visible":{},"opacity":{}}}"#,
                    g.id,
                    json_str(&g.name),
                    g.visible,
                    (g.opacity * 1000.0).round() / 1000.0
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    fn editing_mask(&self, id: u32) -> bool {
        self.layers.iter().any(|l| l.id == id && l.editing_mask)
    }

    /// No tool is busy with pixels (so surfaces can be swapped).
    fn idle(&self) -> bool {
        self.stroking.is_none()
            && self.transform.is_none()
            && self.liquify.is_none()
            && self.adjusting.is_none()
    }

    fn stop_mask_editing(&mut self) {
        for l in &mut self.layers {
            l.set_editing(false);
        }
    }

    // ---- layer masks ----

    /// Give a layer an empty mask (everything shown). Undoable.
    pub fn add_mask(&mut self, id: u32) -> bool {
        if !self.idle() || self.layers.iter().any(|l| l.id == id && l.mask.is_some()) {
            return false;
        }
        if self.index_of(id).is_none() {
            return false;
        }
        self.perform(Edit::SetMask {
            layer: id,
            mask: Some(Box::default()),
        });
        true
    }

    /// Remove a layer's mask (everything shown again). Undoable.
    pub fn delete_mask(&mut self, id: u32) -> bool {
        if !self.idle() || !self.layers.iter().any(|l| l.id == id && l.mask.is_some()) {
            return false;
        }
        self.perform(Edit::SetMask {
            layer: id,
            mask: None,
        });
        true
    }

    /// Bake the mask into the layer's pixels and remove it. Undoable, one step.
    pub fn apply_mask(&mut self, id: u32) -> bool {
        if !self.idle() {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        self.layers[idx].set_editing(false);
        let l = &self.layers[idx];
        let Some(m) = &l.mask else {
            return false;
        };
        let baked = bake(&l.surface.tiles, Some(&m.tiles));
        let snap: Snapshot = baked.into_iter().map(|(k, t)| (k, Some(t))).collect();
        self.perform(Edit::Batch(vec![
            Edit::Tiles {
                drawing: 0,
                layer: id,
                snap,
                mask: false,
            },
            Edit::SetMask {
                layer: id,
                mask: None,
            },
        ]));
        true
    }

    /// Paint on the layer's mask (`true`) or its colour. Makes the layer active.
    pub fn set_mask_editing(&mut self, id: u32, on: bool) -> bool {
        if !self.idle() || self.index_of(id).is_none() {
            return false;
        }
        for l in &mut self.layers {
            l.set_editing(on && l.id == id);
        }
        self.active = id;
        self.layers
            .iter()
            .any(|l| l.id == id && l.editing_mask == on)
    }

    pub fn is_editing_mask(&self) -> bool {
        self.editing_mask(self.active)
    }

    /// Apply an edit and return its inverse.
    fn apply(&mut self, e: Edit) -> Edit {
        match e {
            Edit::Tiles {
                layer,
                snap,
                mask,
                drawing,
            } => {
                // a drawing that isn't showing (another frame): change it where it's kept
                let shown = self.layer_mut(layer).map(|l| l.drawing);
                if !mask && drawing != 0 && shown != Some(drawing) {
                    let tiles = self.cels.entry(drawing).or_default();
                    let replaced = swap_tiles(tiles, snap);
                    return Edit::Tiles {
                        layer,
                        snap: replaced,
                        mask,
                        drawing,
                    };
                }
                let Some(l) = self.layer_mut(layer) else {
                    return Edit::Batch(Vec::new());
                };
                let target = if mask == l.editing_mask {
                    Some(&mut l.surface)
                } else {
                    l.mask.as_mut()
                };
                let Some(target) = target else {
                    return Edit::Batch(Vec::new());
                };
                let replaced = target.swap_in(snap);
                Edit::Tiles {
                    drawing,
                    layer,
                    snap: replaced,
                    mask,
                }
            }
            Edit::Groups(list) => {
                let old = std::mem::replace(&mut self.groups, list);
                for i in 0..self.layers.len() {
                    self.mark_layer(i);
                }
                Edit::Groups(old)
            }
            Edit::Member { layer, group } => {
                let Some(idx) = self.index_of(layer) else {
                    return Edit::Batch(Vec::new());
                };
                let old = std::mem::replace(&mut self.layers[idx].group, group);
                self.mark_layer(idx);
                Edit::Member { layer, group: old }
            }
            Edit::SetMask { layer, mask } => {
                let Some(idx) = self.index_of(layer) else {
                    return Edit::Batch(Vec::new());
                };
                self.mark_layer(idx);
                let l = &mut self.layers[idx];
                l.set_editing(false);
                let old = std::mem::replace(&mut l.mask, mask.map(|b| *b));
                self.mark_layer(idx);
                Edit::SetMask {
                    layer,
                    mask: old.map(Box::new),
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
            Edit::Canvas {
                width,
                height,
                tiles,
            } => {
                let mut old = Vec::with_capacity(tiles.len());
                for (id, colour, mask) in tiles {
                    let Some(l) = self.layers.iter_mut().find(|l| l.id == id) else {
                        continue;
                    };
                    let c = std::mem::replace(&mut l.surface.tiles, colour);
                    let m = match (l.mask.as_mut(), mask) {
                        (Some(s), Some(t)) => Some(std::mem::replace(&mut s.tiles, t)),
                        _ => None,
                    };
                    old.push((id, c, m));
                }
                let (ow, oh) = (self.width, self.height);
                self.width = width;
                self.height = height;
                self.rgba = vec![255; (width * height * 4) as usize];
                self.selection = None;
                self.mark_all();
                Edit::Canvas {
                    width: ow,
                    height: oh,
                    tiles: old,
                }
            }
            Edit::Note(id) => Edit::Note(id),
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
        let mut layer = Layer::new(id, LayerProps::named(name));
        // a new layer goes into the selected layer's group
        layer.group = index
            .checked_sub(1)
            .and_then(|i| self.layers.get(i))
            .map_or(0, |l| l.group);
        self.perform(Edit::Insert {
            index,
            layer: Box::new(layer),
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
        copy.set_editing(false);
        copy.id = new_id;
        // its own drawing (a copy of the pixels, not the same cel)
        copy.drawing = if copy.is_empty() {
            0
        } else {
            self.new_drawing()
        };
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
        props.imported = self.layers[idx].props.imported; // not the caller's to change
        props.adjust = self.layers[idx].props.adjust.clone();
        props.adjust_spec = self.layers[idx].props.adjust_spec.clone();
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
        let mut before = before;
        before.imported = self.layers[idx].props.imported;
        before.adjust = self.layers[idx].props.adjust.clone();
        before.adjust_spec = self.layers[idx].props.adjust_spec.clone();
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
        if upper_idx == 0 || !self.idle() {
            return false;
        }
        self.stop_mask_editing();
        let (lower, upper) = (&self.layers[upper_idx - 1], &self.layers[upper_idx]);
        if lower.props.adjust.is_some() {
            return false;
        }
        if let Some(adj) = upper.props.adjust.as_deref() {
            let lower_id = lower.id;
            let adjusted = Self::adjusted(
                &lower.surface.tiles,
                adj,
                upper.props.opacity,
                upper.mask.as_ref().map(|m| &m.tiles),
            );
            let snap: Snapshot = adjusted.into_iter().map(|(k, t)| (k, Some(t))).collect();
            self.perform(Edit::Batch(vec![
                Edit::Tiles {
                    drawing: 0,
                    layer: lower_id,
                    mask: false,
                    snap,
                },
                Edit::Remove { index: upper_idx },
            ]));
            self.active = lower_id;
            return true;
        }
        let (opacity, blend, clipped) = (upper.props.opacity, upper.props.blend, upper.props.clip);
        // masks are baked in: the merged layer looks the same and has none
        let upper_tiles = bake(&upper.surface.tiles, upper.mask.as_ref().map(|m| &m.tiles));
        let lower_baked = lower
            .mask
            .as_ref()
            .map(|m| bake(&lower.surface.tiles, Some(&m.tiles)));
        let lower_tiles = lower_baked.as_ref().unwrap_or(&lower.surface.tiles);
        let mut merged = Snapshot::new();
        if lower_baked.is_some() {
            for (k, t) in lower_tiles {
                merged.insert(*k, Some(t.clone()));
            }
        }
        for (key, src) in &upper_tiles {
            let mut out = lower_tiles.get(key).cloned().unwrap_or_else(empty_tile);
            for (orow, srow) in out.iter_mut().zip(src.iter()) {
                for (o, s) in orow.iter_mut().zip(srow.iter()) {
                    // a clipped layer only lands where the layer below has paint
                    let k = if clipped { o[3] as f32 / 32768.0 } else { 1.0 };
                    *o = merge_pixel(*o, *s, opacity * k, blend);
                }
            }
            merged.insert(*key, Some(out));
        }
        let lower_id = lower.id;
        let lower_masked = lower.mask.is_some();
        let taint = upper.props.imported && !lower.props.imported;
        let lower_props = LayerProps {
            imported: true,
            ..lower.props.clone()
        };
        let mut edits = vec![Edit::Tiles {
            drawing: 0,
            layer: lower_id,
            mask: self.editing_mask(lower_id),
            snap: merged,
        }];
        if lower_masked {
            edits.push(Edit::SetMask {
                layer: lower_id,
                mask: None,
            });
        }
        if taint {
            edits.push(Edit::Props {
                layer: lower_id,
                props: lower_props,
            });
        }
        edits.push(Edit::Remove { index: upper_idx });
        self.perform(Edit::Batch(edits));
        self.active = lower_id;
        true
    }

    // ---- painting -----------------------------------------------------

    /// Start a stroke on the active layer. Returns false (and paints nothing)
    /// if that layer is locked or hidden.
    pub fn begin_stroke(&mut self) -> bool {
        if self.transform.is_some() || self.adjusting.is_some() {
            return false;
        }
        let Some(idx) = self.index_of(self.active) else {
            return false;
        };
        if self.layers[idx].props.locked || !self.shown(idx) {
            return false;
        }
        // an adjustment layer is painted on through its mask only
        if self.layers[idx].props.adjust.is_some() && !self.layers[idx].editing_mask {
            return false;
        }
        let l = &mut self.layers[idx];
        l.surface.recording = Some(Snapshot::new());
        l.surface.style = self.style.clone();
        let (id, alpha_lock) = (l.id, l.props.alpha_lock);
        self.state.reset();
        let copies = self.symmetry.map_or(0, |sym| sym.copies(0.0, 0.0).len());
        self.sym_states = (0..copies).map(|_| BrushState::default()).collect();
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
            if let Some(sym) = self.symmetry {
                for (st, (mx, my)) in self.sym_states.iter_mut().zip(sym.copies(x, y)) {
                    b.stroke_to(st, &mut l.surface, mx, my, pressure, 0.0, 0.0, dtime);
                }
            }
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
                    let weight = if cx >= 0 && cy >= 0 && (cx as usize) < w && (cy as usize) < h {
                        sel[cy as usize * w + cx as usize]
                    } else {
                        0
                    };
                    if weight < 255 {
                        // outside: as before; a feathered edge: part of the new paint
                        let before = match &orig {
                            Some(Some(o)) => o[py][px],
                            _ => [0; 4],
                        };
                        *p = lerp_px(before, *p, weight as f32 / 255.0);
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
            for st in self.sym_states.iter_mut() {
                b.finish_stroke(st, &mut l.surface);
            }
        }
        self.sym_states.clear();
        self.clip_to_selection(id);
        let snap = self.layers.iter_mut().find(|l| l.id == id).and_then(|l| {
            l.surface.style = None;
            l.surface.recording.take()
        });
        self.state.reset();
        self.alpha_lock_stroke = false;
        // Always record a history step, even if no pixels changed, so the
        // web app's history and this undo stack stay one-to-one.
        self.push_undo(Edit::Tiles {
            drawing: 0,
            layer: id,
            mask: self.editing_mask(id),
            snap: snap.unwrap_or_default(),
        });
    }

    /// Abandon the stroke in progress (e.g. a second finger turned it into a
    /// pinch): its pixels are restored and nothing goes into the history.
    pub fn cancel_stroke(&mut self) {
        let Some(id) = self.stroking.take() else {
            return;
        };
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
            l.surface.style = None;
            if let Some(snap) = l.surface.recording.take() {
                l.surface.swap_in(snap);
            }
        }
        self.state.reset();
        self.stroke_brush = None;
        self.alpha_lock_stroke = false;
    }

    fn push_undo(&mut self, mut e: Edit) {
        self.edits += 1;
        self.tag_drawings(&mut e);
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
                            // a feathered edge clears only partly
                            let k = sel[cy as usize * w + cx as usize] as f32 / 255.0;
                            *p = lerp_px(*p, [0; 4], k);
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
                drawing: 0,
                layer: active,
                mask: self.editing_mask(active),
                snap,
            });
            return true;
        }
        let l = self.layers.iter_mut().find(|l| l.id == active).unwrap();
        let snap: Snapshot = l.surface.tiles.drain().map(|(k, t)| (k, Some(t))).collect();
        l.surface.dirty.extend(snap.keys().copied());
        self.push_undo(Edit::Tiles {
            drawing: 0,
            layer: active,
            mask: self.editing_mask(active),
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
        self.fill_from(x, y, rgb, opacity, tolerance, all_layers, grow, gap, None)
    }

    /// `fill`, but the region comes from `reference`'s own pixels (a line-art
    /// layer, as in Procreate's reference layer) while the paint still goes on
    /// the active layer. `None`, or a missing layer, falls back to `fill`.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    pub fn fill_from(
        &mut self,
        x: u32,
        y: u32,
        rgb: [u8; 3],
        opacity: f32,
        tolerance: f32,
        all_layers: bool,
        grow: u32,
        gap: u32,
        reference: Option<u32>,
    ) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        if self.stroking.is_some() || x as usize >= w || y as usize >= h {
            return false;
        }
        let active = self.active;
        let Some(li) = self.index_of(active) else {
            return false;
        };
        if self.layers[li].props.locked || !self.shown(li) {
            return false;
        }
        let source = match reference.and_then(|id| self.index_of(id)) {
            Some(ri) => self.sample_source(ri, false),
            None => self.sample_source(li, all_layers),
        };
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
        let soft = self.selection.clone();
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
                    // a feathered selection edge gets part of the fill
                    let out = soft
                        .as_ref()
                        .map_or(out, |s| lerp_px(d, out, s[cy * w + cx] as f32 / 255.0));
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
            drawing: 0,
            layer: active,
            mask: self.editing_mask(active),
            snap,
        });
        true
    }

    /// Paint `rgb` at `opacity` over the active layer wherever `coverage`
    /// (one byte per canvas pixel, 0–255, anti-aliased edges allowed) says so:
    /// the lasso fill. Normal blending, on top of what's there; respects the
    /// selection and alpha lock. One undo step; false if nothing changed.
    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    pub fn fill_mask(&mut self, coverage: &[u8], rgb: [u8; 3], opacity: f32) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        if self.stroking.is_some() || coverage.len() != w * h || !self.idle() {
            return false;
        }
        let active = self.active;
        let Some(li) = self.index_of(active) else {
            return false;
        };
        if self.layers[li].props.locked || !self.shown(li) {
            return false;
        }
        let color = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        ];
        let opacity = opacity.clamp(0.0, 1.0);
        let alpha_lock = self.layers[li].props.alpha_lock;
        let sel = self.selection.clone();
        let ts = TILE_SIZE;
        let mut keys: HashSet<TileKey> = HashSet::new();
        for (i, &c) in coverage.iter().enumerate() {
            if c > 0 && sel.as_ref().is_none_or(|s| s[i] > 0) {
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
                    if cx >= w || cy >= h {
                        continue;
                    }
                    let i = cy * w + cx;
                    if coverage[i] == 0 || sel.as_ref().is_some_and(|s| s[i] == 0) {
                        continue;
                    }
                    let d = tile[py][px];
                    if alpha_lock && d[3] == 0 {
                        continue;
                    }
                    let a = opacity * coverage[i] as f32 / 255.0
                        * sel.as_ref().map_or(1.0, |s| s[i] as f32 / 255.0);
                    let src = premul_fix15(color, a);
                    let mut out = merge_pixel(d, src, 1.0, Blend::Normal);
                    if alpha_lock {
                        // keep the layer's own transparency: only the colour changes
                        let (c, _) = unpremul_srgb(out);
                        out = premul_fix15(c, unpremul_srgb(d).1);
                    }
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
            drawing: 0,
            layer: active,
            mask: self.editing_mask(active),
            snap,
        });
        true
    }

    /// Fill the selection with `rgb` at `opacity` on the active layer (soft edges
    /// partly): one undo step. False if there's no selection or nothing changed.
    pub fn fill_selection(&mut self, rgb: [u8; 3], opacity: f32) -> bool {
        if self.selection.is_none() {
            return false;
        }
        let n = (self.width * self.height) as usize;
        let ok = self.fill_mask(&vec![255; n], rgb, opacity);
        if ok {
            self.sel_fill_at = Some(self.edits);
        }
        ok
    }

    /// Join the last two undo steps into one (a stroke and the fill that closed it).
    /// Put a step of the app's own in the history (see [`Edit::Note`]); `id` > 0.
    pub fn push_note(&mut self, id: u32) -> bool {
        if id == 0 || self.stroking.is_some() {
            return false;
        }
        self.push_undo(Edit::Note(id));
        true
    }

    /// The app's step the next undo / redo goes back over (0: none).
    pub fn undo_note(&self) -> u32 {
        self.undo.last().map_or(0, edit_note)
    }
    pub fn redo_note(&self) -> u32 {
        self.redo.last().map_or(0, edit_note)
    }

    pub fn join_last_edits(&mut self) -> bool {
        if self.undo.len() < 2 || self.stroking.is_some() {
            return false;
        }
        let newer = self.undo.pop().unwrap();
        let older = self.undo.pop().unwrap();
        // undoing: the newer one first
        self.undo.push(Edit::Batch(vec![newer, older]));
        self.edits += 1;
        true
    }

    /// Whether the last selection fill is still the latest edit (so it can be
    /// filled again with another colour or the selection as it is now).
    pub fn selection_fill_live(&self) -> bool {
        self.sel_fill_at == Some(self.edits) && self.stroking.is_none()
    }

    /// Fill again, in place of the last selection fill (Procreate's Color Fill: a new
    /// colour, or shapes added to the selection, change the fill): still one undo
    /// step. False (and nothing done) if it isn't the latest edit any more.
    pub fn refill_selection(&mut self, rgb: [u8; 3], opacity: f32) -> bool {
        if !self.selection_fill_live() || !self.idle() {
            return false;
        }
        let Some(e) = self.undo.pop() else {
            return false;
        };
        // put the pixels back as they were before it (no redo for it)
        self.apply(e);
        self.edits += 1;
        self.sel_fill_at = None;
        self.fill_selection(rgb, opacity);
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

    /// Select every pixel of a similar colour to the one at (x, y), joined up
    /// or not (Photoshop's Color Range, Procreate's non-contiguous wand).
    pub fn select_colour_range(
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
        let seed = src[y as usize * w + x as usize];
        let m = src
            .iter()
            .map(|p| (0..4).all(|i| (p[i] as i32 - seed[i] as i32).abs() <= tol))
            .collect();
        self.combine_selection(m, op)
    }

    // ---- canvas size ----------------------------------------------------

    /// Change the canvas size (one undo step). Without `scale`, the drawing
    /// keeps its size and moves by (`dx`, `dy`): a crop, or more room; paint
    /// off the new canvas is dropped. With `scale`, the drawing is resized to
    /// fill the new canvas (`dx`, `dy` ignored).
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        dx: i32,
        dy: i32,
        scale: bool,
    ) -> bool {
        if !self.idle() || width == 0 || height == 0 || width > 8192 || height > 8192 {
            return false;
        }
        // (an animation's other frames would keep the old size: not yet)
        if !self.cels.is_empty() {
            return false;
        }
        if width == self.width && height == self.height && (scale || (dx == 0 && dy == 0)) {
            return false;
        }
        self.stop_mask_editing();
        let (ow, oh) = (self.width, self.height);
        let redo = |t: &TileMap| {
            if scale {
                scale_tiles(t, ow, oh, width, height)
            } else {
                shift_tiles(t, dx, dy, width, height)
            }
        };
        let tiles = self
            .layers
            .iter()
            .map(|l| {
                (
                    l.id,
                    redo(&l.surface.tiles),
                    l.mask.as_ref().map(|m| redo(&m.tiles)),
                )
            })
            .collect();
        self.perform(Edit::Canvas {
            width,
            height,
            tiles,
        });
        true
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

    /// Soften the selection's edge over about `radius` px (a feathered
    /// selection): paint, fills and adjustments then fade out across it.
    pub fn feather_selection(&mut self, radius: f32) -> bool {
        let Some(sel) = self.selection.as_ref() else {
            return false;
        };
        let (w, h) = (self.width as usize, self.height as usize);
        // three box blurs ≈ a gaussian; each pass has a third of the radius
        let r = (radius / 3.0).round().max(1.0) as usize;
        let mut v: Vec<f32> = sel.iter().map(|&b| b as f32).collect();
        let mut tmp = vec![0f32; w * h];
        for _ in 0..3 {
            box_blur_line(&v, &mut tmp, w, h, r, true);
            box_blur_line(&tmp, &mut v, w, h, r, false);
        }
        let out: Vec<u8> = v
            .iter()
            .map(|x| x.round().clamp(0.0, 255.0) as u8)
            .collect();
        if out.iter().all(|&b| b == 0) {
            return false;
        }
        self.selection = Some(out);
        true
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
        if self.transform.is_some() || self.stroking.is_some() || self.adjusting.is_some() {
            return Vec::new();
        }
        let (w, h) = (self.width as i64, self.height as i64);
        let active = self.active;
        let Some(li) = self.index_of(active) else {
            return Vec::new();
        };
        if self.layers[li].props.locked || !self.shown(li) {
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
            m: H_IDENTITY,
            cover: None,
        }));
        self.transform_preview_h(H_IDENTITY);
        vec![x0 as f32, y0 as f32, (x1 + 1) as f32, (y1 + 1) as f32]
    }

    pub fn transforming(&self) -> bool {
        self.transform.is_some()
    }

    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    /// Show the lifted pixels under matrix `m` (dest = a·x + c·y + e, b·x + d·y + f).
    pub fn transform_preview(&mut self, m: [f32; 6]) -> bool {
        let [a, b, c, d, e, f] = m;
        self.transform_preview_h([a, c, e, b, d, f, 0.0, 0.0, 1.0])
    }

    /// Show the lifted pixels under a projective matrix `h` (row-major 3×3,
    /// see `TransformSession::m`): distort and perspective. The lifted box must
    /// stay in front of the "camera" (no corner may flip through infinity).
    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    pub fn transform_preview_h(&mut self, hm: [f32; 9]) -> bool {
        let Some(mut t) = self.transform.take() else {
            return false;
        };
        let Some(inv) = h_invert(hm) else {
            self.transform = Some(t);
            return false;
        };
        let (fx0, fy0) = (t.fx0 as f32, t.fy0 as f32);
        let (fx1, fy1) = (fx0 + t.fw as f32, fy0 + t.fh as f32);
        let mut quad = [(0.0f32, 0.0f32); 4];
        for (k, (x, y)) in [(fx0, fy0), (fx1, fy0), (fx1, fy1), (fx0, fy1)]
            .into_iter()
            .enumerate()
        {
            let Some(q) = h_apply(hm, x, y) else {
                self.transform = Some(t);
                return false;
            };
            quad[k] = q;
        }
        t.m = hm;
        t.cover = None;
        // the whole float is one piece (samples outside it are transparent anyway)
        let piece = Piece {
            inv,
            bbox: quad_bbox(&quad),
            src: (fx0 - 1.0, fy0 - 1.0, fx1 + 1.0, fy1 + 1.0),
        };
        self.render_float(t, vec![piece], false)
    }

    /// Warp: the lifted box is cut into a `cols`×`rows` grid whose
    /// (cols+1)×(rows+1) corner points land at `nodes` (x, y pairs, row by row,
    /// in canvas px). Each cell is drawn with its own projective map, so a fine
    /// grid gives a smooth bend.
    pub fn transform_preview_mesh(&mut self, nodes: &[f32], cols: usize, rows: usize) -> bool {
        let Some(t) = self.transform.take() else {
            return false;
        };
        if cols == 0 || rows == 0 || nodes.len() != 2 * (cols + 1) * (rows + 1) {
            self.transform = Some(t);
            return false;
        }
        let (fx0, fy0) = (t.fx0 as f32, t.fy0 as f32);
        let (cw, ch) = (t.fw as f32 / cols as f32, t.fh as f32 / rows as f32);
        let node = |c: usize, r: usize| {
            let i = 2 * (r * (cols + 1) + c);
            (nodes[i], nodes[i + 1])
        };
        let mut pieces = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let quad = [
                    node(c, r),
                    node(c + 1, r),
                    node(c + 1, r + 1),
                    node(c, r + 1),
                ];
                let src = (
                    fx0 + c as f32 * cw,
                    fy0 + r as f32 * ch,
                    fx0 + (c + 1) as f32 * cw,
                    fy0 + (r + 1) as f32 * ch,
                );
                // a folded cell is skipped rather than drawn inside out
                let Some(inv) = rect_to_quad(src, quad).and_then(h_invert) else {
                    continue;
                };
                // cells meet edge to edge: let each reach half a pixel over
                pieces.push(Piece {
                    inv,
                    bbox: quad_bbox(&quad),
                    src: (
                        src.0 - if c == 0 { 1.0 } else { 0.5 },
                        src.1 - if r == 0 { 1.0 } else { 0.5 },
                        src.2 + if c + 1 == cols { 1.0 } else { 0.5 },
                        src.3 + if r + 1 == rows { 1.0 } else { 0.5 },
                    ),
                });
            }
        }
        if pieces.is_empty() {
            self.transform = Some(t);
            return false;
        }
        self.render_float(t, pieces, true)
    }

    /// Repaint the transform's old and new areas: the layer without the lifted
    /// pixels, then the lifted pixels through each piece's map (a pixel is drawn
    /// by the first piece that covers it). `track` remembers which pixels got
    /// paint, for the selection to follow a warp.
    // pixel loops index tiles by (row, col) and map them to canvas coordinates
    #[allow(clippy::needless_range_loop)]
    fn render_float(
        &mut self,
        mut t: Box<TransformSession>,
        pieces: Vec<Piece>,
        track: bool,
    ) -> bool {
        let (w, h) = (self.width as i64, self.height as i64);
        let ts = TILE_SIZE as i64;
        let clip =
            |b: (i64, i64, i64, i64)| (b.0.max(0), b.1.max(0), b.2.min(w - 1), b.3.min(h - 1));
        let mut now = clip(pieces[0].bbox);
        for p in &pieces[1..] {
            let b = clip(p.bbox);
            now = (
                now.0.min(b.0),
                now.1.min(b.1),
                now.2.max(b.2),
                now.3.max(b.3),
            );
        }
        let mut region = now;
        if let Some(o) = t.last {
            region = (
                region.0.min(o.0),
                region.1.min(o.1),
                region.2.max(o.2),
                region.3.max(o.3),
            );
        }
        t.last = Some(now);
        let (fx0, fy0) = (t.fx0 as f32, t.fy0 as f32);
        let float = &t.float;
        let (fw, fh) = (t.fw, t.fh);
        let sample = |sx: f32, sy: f32| -> [u16; 4] {
            // bilinear, premultiplied; outside the float is transparent
            let (x, y) = (sx - fx0 - 0.5, sy - fy0 - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (tx, ty) = (x - x0, y - y0);
            let get = |xi: i64, yi: i64| -> [f32; 4] {
                if xi < 0 || yi < 0 || xi >= fw as i64 || yi >= fh as i64 {
                    return [0.0; 4];
                }
                float[yi as usize * fw + xi as usize].map(|v| v as f32)
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
        // what each piece draws, per canvas pixel of the new area (first piece wins)
        let (nw, nh) = ((now.2 - now.0 + 1) as usize, (now.3 - now.1 + 1) as usize);
        let mut drawn: Vec<Option<[u16; 4]>> = vec![None; nw * nh];
        for p in &pieces {
            let b = clip(p.bbox);
            for cy in b.1..=b.3 {
                for cx in b.0..=b.2 {
                    let i = (cy - now.1) as usize * nw + (cx - now.0) as usize;
                    if drawn[i].is_some() {
                        continue;
                    }
                    let Some((sx, sy)) = h_apply(p.inv, cx as f32 + 0.5, cy as f32 + 0.5) else {
                        continue;
                    };
                    if sx < p.src.0 || sy < p.src.1 || sx > p.src.2 || sy > p.src.3 {
                        continue;
                    }
                    drawn[i] = Some(sample(sx, sy));
                }
            }
        }
        t.cover = track.then(|| {
            (
                now,
                drawn.iter().map(|d| d.is_some_and(|s| s[3] > 0)).collect(),
            )
        });
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
                        if cx < now.0 || cx > now.2 || cy < now.1 || cy > now.3 {
                            continue;
                        }
                        let i = (cy - now.1) as usize * nw + (cx - now.0) as usize;
                        if let Some(s) = drawn[i]
                            && s[3] > 0
                        {
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
        if let Some((b, cover)) = t.cover.as_ref().filter(|_| self.selection.is_some()) {
            let w = self.width as usize;
            let bw = (b.2 - b.0 + 1) as usize;
            let mut out = vec![0u8; w * self.height as usize];
            for (i, _) in cover.iter().enumerate().filter(|(_, c)| **c) {
                let (x, y) = (b.0 as usize + i % bw, b.1 as usize + i / bw);
                out[y * w + x] = 255;
            }
            self.selection = cover.iter().any(|&c| c).then_some(out);
        } else if let Some(sel) = self.selection.take() {
            let (w, h) = (self.width as usize, self.height as usize);
            let inv = h_invert(t.m).unwrap_or(H_IDENTITY);
            let mut out = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let Some((sx, sy)) = h_apply(inv, x as f32 + 0.5, y as f32 + 0.5) else {
                        continue;
                    };
                    let (sx, sy) = (sx.floor(), sy.floor());
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
            drawing: 0,
            layer: t.layer,
            mask: self.editing_mask(t.layer),
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

    // ---- symmetry ----

    /// Draw every stroke mirrored / rotated about (cx, cy) (`None` = off).
    pub fn set_symmetry(&mut self, sym: Option<Symmetry>) {
        if self.stroking.is_none() {
            self.symmetry = sym;
        }
    }

    // ---- liquify: push pixels around like wet paint ----

    /// Start a liquify stroke on the active layer (false if locked/hidden/busy).
    pub fn liquify_begin(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        let Some(li) = self.index_of(self.active) else {
            return false;
        };
        if self.layers[li].props.locked || !self.shown(li) {
            return false;
        }
        let l = &self.layers[li];
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
                // a feathered selection edge moves only partly
                let v = sel.map_or(v, |m| {
                    lerp_px(
                        old[yy * bw + xx],
                        v,
                        m[(cy * w + cx) as usize] as f32 / 255.0,
                    )
                });
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
        self.push_undo(Edit::Tiles {
            drawing: 0,
            layer,
            mask: self.editing_mask(layer),
            snap,
        });
        true
    }

    // ---- imported pictures ----

    /// Tiles for a canvas-size straight-alpha sRGB RGBA8 picture (None: wrong size).
    #[allow(clippy::needless_range_loop)]
    fn picture_tiles(&self, rgba: &[u8]) -> Option<TileMap> {
        let (w, h) = (self.width as usize, self.height as usize);
        if rgba.len() != w * h * 4 {
            return None;
        }
        let ts = TILE_SIZE;
        let mut tiles = TileMap::new();
        for ty in 0..h.div_ceil(ts) {
            for tx in 0..w.div_ceil(ts) {
                let mut t = empty_tile();
                let mut any = false;
                for py in 0..ts {
                    for px in 0..ts {
                        let (x, y) = (tx * ts + px, ty * ts + py);
                        if x >= w || y >= h {
                            continue;
                        }
                        let i = (y * w + x) * 4;
                        if rgba[i + 3] == 0 {
                            continue;
                        }
                        let c = [rgba[i], rgba[i + 1], rgba[i + 2]].map(|v| v as f32 / 255.0);
                        t[py][px] = premul_fix15(c, rgba[i + 3] as f32 / 255.0);
                        any = true;
                    }
                }
                if any {
                    tiles.insert((tx as i32, ty as i32), t);
                }
            }
        }
        Some(tiles)
    }

    fn add_pixels_layer(&mut self, name: String, rgba: &[u8], imported: bool) -> u32 {
        if !self.idle() || self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        let Some(tiles) = self.picture_tiles(rgba) else {
            return 0;
        };
        let id = self.next_id;
        self.next_id += 1;
        let index = self
            .index_of(self.active)
            .map_or(self.layers.len(), |i| i + 1);
        let mut props = LayerProps::named(if name.trim().is_empty() {
            format!("Layer {id}")
        } else {
            name.chars().take(60).collect()
        });
        props.imported = imported;
        let mut layer = Layer::new(id, props);
        layer.group = index
            .checked_sub(1)
            .and_then(|i| self.layers.get(i))
            .map_or(0, |l| l.group);
        layer.surface.tiles = tiles;
        self.perform(Edit::Insert {
            index,
            layer: Box::new(layer),
        });
        self.active = id;
        id
    }

    /// Add a layer holding a picture (straight-alpha sRGB RGBA8, canvas size),
    /// above the active layer and in its group. One undo step. Returns its id (0 = couldn't).
    pub fn add_image_layer(&mut self, name: &str, rgba: &[u8]) -> u32 {
        let name = if name.trim().is_empty() {
            format!("Picture {}", self.next_id)
        } else {
            name.to_string()
        };
        self.add_pixels_layer(name, rgba, true)
    }

    /// Bring back a saved layer's pixels (the drawing history). `imported` keeps a
    /// layer that held an imported picture marked as such.
    pub fn restore_layer(&mut self, name: &str, rgba: &[u8], imported: bool) -> u32 {
        self.add_pixels_layer(name.to_string(), rgba, imported)
    }

    /// Give a layer a mask from a saved greyscale picture (one byte per canvas
    /// pixel, 255 = shown). Not an undo step: for restoring a drawing.
    pub fn restore_mask(&mut self, id: u32, gray: &[u8]) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        if !self.idle() || gray.len() != w * h {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        // a mask pixel of grey g is black paint with alpha 1 - g (white = nothing painted)
        let rgba: Vec<u8> = gray.iter().flat_map(|&g| [0, 0, 0, 255 - g]).collect();
        let Some(tiles) = self.picture_tiles(&rgba) else {
            return false;
        };
        let l = &mut self.layers[idx];
        l.set_editing(false);
        l.mask = Some(UndoSurface {
            tiles,
            ..UndoSurface::default()
        });
        self.mark_layer(idx);
        true
    }

    // ---- adjustments and gradients: previewed live, one undo step ----

    /// Start adjusting the active layer (or painting a gradient on it).
    pub fn adjust_begin(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        let Some(li) = self.index_of(self.active) else {
            return false;
        };
        if self.layers[li].props.locked || !self.shown(li) || self.layers[li].props.adjust.is_some()
        {
            return false;
        }
        let l = &self.layers[li];
        self.adjusting = Some((l.id, l.surface.tiles.clone()));
        true
    }

    // ---- adjustment layers ----------------------------------------------

    /// Add an adjustment layer above the active one (with a mask, which tools
    /// then paint on). `spec` is the app's own description, kept as given.
    /// Returns its id (0 = limit reached).
    pub fn add_adjustment_layer(
        &mut self,
        name: &str,
        adj: adjust::ColourAdjust,
        spec: &str,
    ) -> u32 {
        if !self.idle() || self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        self.stop_mask_editing();
        let id = self.next_id;
        self.next_id += 1;
        let index = self
            .index_of(self.active)
            .map_or(self.layers.len(), |i| i + 1);
        let mut props = LayerProps::named(name.chars().take(60).collect::<String>());
        props.adjust = Some(Box::new(adj));
        props.adjust_spec = spec.into();
        let mut layer = Layer::new(id, props);
        layer.group = index
            .checked_sub(1)
            .and_then(|i| self.layers.get(i))
            .map_or(0, |l| l.group);
        layer.mask = Some(UndoSurface::default());
        layer.set_editing(true);
        self.perform(Edit::Insert {
            index,
            layer: Box::new(layer),
        });
        self.active = id;
        id
    }

    /// Change an adjustment layer's settings live (no undo step yet: see
    /// [`Self::adjustment_commit`]).
    pub fn adjustment_preview(&mut self, id: u32, adj: adjust::ColourAdjust, spec: &str) -> bool {
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        if self.layers[idx].props.adjust.is_none() {
            return false;
        }
        if self
            .adjust_layer_before
            .as_ref()
            .is_none_or(|(l, _)| *l != id)
        {
            self.adjustment_commit();
            self.adjust_layer_before = Some((id, self.layers[idx].props.clone()));
        }
        let mut props = self.layers[idx].props.clone();
        props.adjust = Some(Box::new(adj));
        props.adjust_spec = spec.into();
        self.apply(Edit::Props { layer: id, props });
        true
    }

    /// Record the previewed adjustment-layer change as one undo step.
    pub fn adjustment_commit(&mut self) -> bool {
        let Some((id, before)) = self.adjust_layer_before.take() else {
            return false;
        };
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

    /// `tiles` with an adjustment applied (through `mask`, at `opacity`).
    fn adjusted(
        tiles: &TileMap,
        adj: &adjust::ColourAdjust,
        opacity: f32,
        mask: Option<&TileMap>,
    ) -> TileMap {
        tiles
            .iter()
            .map(|(k, t)| {
                let mut t = t.clone();
                let m = mask.and_then(|m| m.get(k));
                for (r, row) in t.iter_mut().enumerate() {
                    for (c, p) in row.iter_mut().enumerate() {
                        if p[3] == 0 {
                            continue;
                        }
                        let w = opacity * m.map_or(1.0, |m| mask_value(m[r][c]));
                        if w <= 0.0 {
                            continue;
                        }
                        let (col, a) = unpremul_srgb(*p);
                        let to = adjust::colour_op(adj, col);
                        let mix = [0, 1, 2].map(|i| col[i] + (to[i] - col[i]) * w);
                        *p = premul_fix15(mix, a);
                    }
                }
                (*k, t)
            })
            .collect()
    }

    pub fn is_adjusting(&self) -> bool {
        self.adjusting.is_some()
    }

    fn adjust_show(&mut self, make: impl FnOnce(&TileMap, &adjust::Area<'_>) -> TileMap) {
        let Some((layer, original)) = &self.adjusting else {
            return;
        };
        let Some(li) = self.index_of(*layer) else {
            return;
        };
        let area = adjust::Area {
            width: self.width,
            height: self.height,
            selection: self.selection.as_deref(),
            keep_alpha: self.layers[li].props.alpha_lock,
        };
        let next = make(original, &area);
        let s = &mut self.layers[li].surface;
        let keys: Vec<TileKey> = s.tiles.keys().chain(next.keys()).copied().collect();
        s.dirty.extend(keys);
        s.tiles = next;
    }

    /// Show the layer with these colour adjustments (from the original each time).
    pub fn adjust_preview(&mut self, a: adjust::ColourAdjust) {
        self.adjust_show(|o, area| adjust::adjust_tiles(o, &a, area));
    }

    /// Show the layer with this gradient painted over it.
    pub fn gradient_preview(&mut self, g: adjust::Gradient) {
        self.adjust_show(|o, area| adjust::gradient_tiles(o, &g, area));
    }

    /// Keep what's previewed as one undo step. False if nothing changed.
    pub fn adjust_commit(&mut self) -> bool {
        let Some((layer, original)) = self.adjusting.take() else {
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
        self.push_undo(Edit::Tiles {
            drawing: 0,
            layer,
            mask: self.editing_mask(layer),
            snap,
        });
        true
    }

    /// Put the layer back as it was.
    pub fn adjust_cancel(&mut self) {
        let Some((layer, original)) = self.adjusting.take() else {
            return;
        };
        if let Some(li) = self.index_of(layer) {
            let s = &mut self.layers[li].surface;
            let keys: Vec<TileKey> = s.tiles.keys().chain(original.keys()).copied().collect();
            s.dirty.extend(keys);
            s.tiles = original;
        }
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
            self.layers[idx].colour_mut().tiles.clear();
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
            let c = l.colour_mut();
            let rec = c.recording.take();
            brush.stroke_to(state, c, x, y, pressure, 0.0, 0.0, dtime);
            c.recording = rec;
        }
    }

    pub fn remote_end(&mut self, key: u32, brush: &Brush) {
        let Some((layer, mut state)) = self.remote.remove(&key) else {
            return;
        };
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == layer) {
            let c = l.colour_mut();
            let rec = c.recording.take();
            brush.finish_stroke(&mut state, c);
            c.recording = rec;
        }
    }

    /// Forget the undo/redo history (e.g. after setting up starter layers,
    /// so the first undo doesn't take the setup apart).
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    // ---- drawings (animation cels) ---------------------------------------
    //
    // Each layer shows one drawing at a time; the others wait in `cels`. Moving to
    // another frame swaps them (`show_drawings`): nothing is copied. Undo steps
    // remember their drawing, so undoing a stroke from another frame changes that
    // drawing, not the one showing ([`Painter::undo_drawing`] says which it is).

    /// Give tile edits that don't know their drawing the one their layer shows (a
    /// layer showing none gets a new drawing: drawing on an empty cel starts one).
    fn tag_drawings(&mut self, e: &mut Edit) {
        match e {
            Edit::Tiles {
                layer,
                mask: false,
                drawing,
                ..
            } if *drawing == 0 => {
                let next = self.next_drawing;
                if let Some(l) = self.layer_mut(*layer) {
                    if l.drawing == 0 {
                        l.drawing = next;
                        self.next_drawing += 1;
                    }
                    *drawing = self.layer_mut(*layer).map_or(0, |l| l.drawing);
                }
            }
            Edit::Batch(list) => {
                for e in list {
                    self.tag_drawings(e);
                }
            }
            _ => {}
        }
    }

    /// Give every layer that has paint but no drawing yet (an imported or pasted
    /// picture, a restored layer) a drawing of its own.
    pub fn ensure_drawings(&mut self) {
        for i in 0..self.layers.len() {
            if self.layers[i].drawing == 0 && !self.layers[i].surface.tiles.is_empty() {
                let id = self.new_drawing();
                self.layers[i].drawing = id;
            }
        }
    }

    /// The drawing a layer shows (0 = none yet).
    pub fn layer_drawing(&self, id: u32) -> u32 {
        self.layers
            .iter()
            .find(|l| l.id == id)
            .map_or(0, |l| l.drawing)
    }

    /// Show other drawings: `pairs` is (layer id, drawing id) flattened; drawing 0
    /// shows an empty cel. Not an undo step. False if busy (a stroke, transform,
    /// adjustment or mask being edited) or a layer is missing; nothing changes then.
    pub fn show_drawings(&mut self, pairs: &[u32]) -> bool {
        if !self.idle()
            || self.layers.iter().any(|l| l.editing_mask)
            || !pairs.len().is_multiple_of(2)
        {
            return false;
        }
        let pairs: Vec<(u32, u32)> = pairs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| (c[0], c[1]))
            .collect();
        if pairs.iter().any(|&(l, _)| self.index_of(l).is_none()) {
            return false;
        }
        // a drawing can show on one layer only
        for (i, &(_, d)) in pairs.iter().enumerate() {
            if d != 0 && pairs[i + 1..].iter().any(|&(_, e)| e == d) {
                return false;
            }
        }
        self.ensure_drawings();
        for &(id, drawing) in &pairs {
            let idx = self.index_of(id).unwrap();
            let l = &mut self.layers[idx];
            if l.drawing == drawing {
                continue;
            }
            let old = std::mem::take(&mut l.surface.tiles);
            // a layer's dirty tiles: everywhere either drawing has paint
            let mut keys: HashSet<TileKey> = old.keys().copied().collect();
            if l.drawing != 0 {
                self.cels.insert(l.drawing, old);
            }
            let new = if drawing == 0 {
                TileMap::new()
            } else {
                self.cels.remove(&drawing).unwrap_or_default()
            };
            keys.extend(new.keys().copied());
            let l = &mut self.layers[idx];
            l.surface.tiles = new;
            l.drawing = drawing;
            l.surface.dirty.extend(keys);
            self.next_drawing = self.next_drawing.max(drawing + 1);
        }
        true
    }

    /// A new, empty drawing's id (it shows nowhere until `show_drawings`).
    pub fn new_drawing(&mut self) -> u32 {
        let id = self.next_drawing;
        self.next_drawing += 1;
        id
    }

    /// The tiles of a drawing, wherever it is (showing on a layer, or kept).
    fn drawing_tiles(&self, drawing: u32) -> Option<&TileMap> {
        if drawing == 0 {
            return None;
        }
        self.layers
            .iter()
            .find(|l| l.drawing == drawing)
            .map(|l| &l.surface.tiles)
            .or_else(|| self.cels.get(&drawing))
    }

    /// A copy of a drawing (for a duplicated frame); returns the copy's id.
    pub fn copy_drawing(&mut self, drawing: u32) -> u32 {
        let tiles = self.drawing_tiles(drawing).cloned().unwrap_or_default();
        let id = self.new_drawing();
        self.cels.insert(id, tiles);
        id
    }

    /// A "boiled" copy of a drawing: the same lines nudged by a smooth wobble of up to
    /// `amount` pixels, different for each `seed`, as if traced again by hand. Held
    /// frames alternating between a drawing and its boils look alive. Returns the
    /// copy's id (0: no such drawing, or busy).
    pub fn boil_drawing(&mut self, drawing: u32, amount: f32, seed: u32) -> u32 {
        if !self.idle() {
            return 0;
        }
        let Some(tiles) = self.drawing_tiles(drawing) else {
            return 0;
        };
        let (w, h) = (self.width as usize, self.height as usize);
        let src = self.tiles_rgba(tiles);
        // where there's paint (in tiles), and a margin it can move into
        let ts = TILE_SIZE as i64;
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for &(tx, ty) in tiles.keys() {
            x0 = x0.min(tx as i64 * ts);
            y0 = y0.min(ty as i64 * ts);
            x1 = x1.max((tx as i64 + 1) * ts);
            y1 = y1.max((ty as i64 + 1) * ts);
        }
        let amount = amount.clamp(0.0, 16.0);
        let pad = amount.ceil() as i64 + 1;
        let clampx = |v: i64| v.clamp(0, w as i64) as usize;
        let clampy = |v: i64| v.clamp(0, h as i64) as usize;
        let (x0, x1) = (clampx(x0 - pad), clampx(x1 + pad));
        let (y0, y1) = (clampy(y0 - pad), clampy(y1 + pad));
        let mut out = vec![0u8; w * h * 4];
        // the wobble: smooth noise, a cell every ~40 px (about a stroke's curve)
        let cell = 40.0f32;
        let noise = |x: f32, y: f32, k: u32| -> f32 {
            let hash = |ix: i32, iy: i32| -> f32 {
                let mut v = (ix as u32)
                    .wrapping_mul(0x9e37_79b1)
                    .wrapping_add((iy as u32).wrapping_mul(0x85eb_ca77))
                    .wrapping_add(seed.wrapping_mul(0xc2b2_ae3d))
                    .wrapping_add(k.wrapping_mul(0x27d4_eb2f));
                v ^= v >> 15;
                v = v.wrapping_mul(0x2c1b_3c6d);
                v ^= v >> 12;
                v = v.wrapping_mul(0x297a_2d39);
                v ^= v >> 15;
                v as f32 / u32::MAX as f32 * 2.0 - 1.0
            };
            let (gx, gy) = (x / cell, y / cell);
            let (ix, iy) = (gx.floor() as i32, gy.floor() as i32);
            let (fx, fy) = (gx - ix as f32, gy - iy as f32);
            let s = |t: f32| t * t * (3.0 - 2.0 * t);
            let (sx, sy) = (s(fx), s(fy));
            let a = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * sx;
            let b = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * sx;
            a + (b - a) * sy
        };
        // sample the original, premultiplied (so edges don't pick up dark fringes)
        let at = |x: i64, y: i64| -> [f32; 4] {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return [0.0; 4];
            }
            let i = (y as usize * w + x as usize) * 4;
            let a = src[i + 3] as f32 / 255.0;
            [
                src[i] as f32 * a,
                src[i + 1] as f32 * a,
                src[i + 2] as f32 * a,
                a,
            ]
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let (fx, fy) = (x as f32, y as f32);
                let sx = fx + noise(fx, fy, 1) * amount;
                let sy = fy + noise(fx, fy, 2) * amount;
                let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
                let (tx, ty) = (sx - ix as f32, sy - iy as f32);
                let (p00, p10, p01, p11) = (
                    at(ix, iy),
                    at(ix + 1, iy),
                    at(ix, iy + 1),
                    at(ix + 1, iy + 1),
                );
                let mut c = [0.0f32; 4];
                for k in 0..4 {
                    let top = p00[k] + (p10[k] - p00[k]) * tx;
                    let bot = p01[k] + (p11[k] - p01[k]) * tx;
                    c[k] = top + (bot - top) * ty;
                }
                if c[3] <= 0.5 / 255.0 {
                    continue;
                }
                let i = (y * w + x) * 4;
                out[i] = (c[0] / c[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 1] = (c[1] / c[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 2] = (c[2] / c[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 3] = (c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        let Some(boiled) = self.picture_tiles(&out) else {
            return 0;
        };
        let id = self.new_drawing();
        self.cels.insert(id, boiled);
        id
    }

    /// A drawing's pixels at full size (straight-alpha sRGB RGBA8); transparent if
    /// it's empty or unknown.
    pub fn drawing_rgba(&self, drawing: u32) -> Vec<u8> {
        match self.drawing_tiles(drawing) {
            Some(t) => self.tiles_rgba(t),
            None => vec![0; (self.width * self.height * 4) as usize],
        }
    }

    /// Whether a drawing has no paint.
    pub fn drawing_empty(&self, drawing: u32) -> bool {
        self.drawing_tiles(drawing).is_none_or(|t| t.is_empty())
    }

    /// Load a drawing's pixels (straight-alpha sRGB RGBA8, canvas size), replacing
    /// it if it exists. Not an undo step: for opening a project.
    pub fn put_drawing(&mut self, drawing: u32, rgba: &[u8]) -> bool {
        if drawing == 0 || !self.idle() {
            return false;
        }
        let Some(tiles) = self.picture_tiles(rgba) else {
            return false;
        };
        self.next_drawing = self.next_drawing.max(drawing + 1);
        if let Some(idx) = self.layers.iter().position(|l| l.drawing == drawing) {
            let l = &mut self.layers[idx];
            let keys: Vec<TileKey> = l
                .surface
                .tiles
                .keys()
                .chain(tiles.keys())
                .copied()
                .collect();
            l.surface.tiles = tiles;
            l.surface.dirty.extend(keys);
        } else {
            self.cels.insert(drawing, tiles);
        }
        true
    }

    /// Forget kept drawings that aren't in `keep` (frames deleted for good). Drawings
    /// still in the undo history are kept too.
    pub fn drop_drawings_except(&mut self, keep: &[u32]) {
        let mut used: HashSet<u32> = keep.iter().copied().collect();
        fn collect(e: &Edit, used: &mut HashSet<u32>) {
            match e {
                Edit::Tiles { drawing, .. } => {
                    used.insert(*drawing);
                }
                Edit::Batch(list) => list.iter().for_each(|e| collect(e, used)),
                _ => {}
            }
        }
        for e in self.undo.iter().chain(&self.redo) {
            collect(e, &mut used);
        }
        self.cels.retain(|d, _| used.contains(d));
    }

    /// A drawing's tiles packed for the frame store: `u32` count, then per tile `i32`
    /// x, `i32` y and 64×64 straight-alpha sRGB RGBA8 (the project file's tiles). Only
    /// tiles with paint; empty for an unknown drawing.
    pub fn drawing_tiles_packed(&self, drawing: u32) -> Vec<u8> {
        let tiles = self.drawing_tiles(drawing);
        let ts = TILE_SIZE;
        let list: Vec<(&TileKey, &Box<TilePixels>)> = tiles
            .map(|t| {
                t.iter()
                    .filter(|(_, px)| px.iter().flatten().any(|p| p[3] > 0))
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::with_capacity(4 + list.len() * (8 + ts * ts * 4));
        out.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for (&(tx, ty), px) in list {
            out.extend_from_slice(&tx.to_le_bytes());
            out.extend_from_slice(&ty.to_le_bytes());
            for row in px.iter() {
                for p in row {
                    if p[3] == 0 {
                        out.extend_from_slice(&[0; 4]);
                        continue;
                    }
                    let (c, a) = unpremul_srgb(*p);
                    out.extend_from_slice(&[
                        (c[0] * 255.0).round() as u8,
                        (c[1] * 255.0).round() as u8,
                        (c[2] * 255.0).round() as u8,
                        (a * 255.0).round() as u8,
                    ]);
                }
            }
        }
        out
    }

    /// Load a drawing from packed tiles (as [`Painter::drawing_tiles_packed`] makes them),
    /// replacing it if it exists. Not an undo step. False if busy or the bytes don't
    /// add up.
    pub fn put_drawing_tiles(&mut self, drawing: u32, packed: &[u8]) -> bool {
        let ts = TILE_SIZE;
        let tile_bytes = ts * ts * 4;
        if drawing == 0 || !self.idle() || packed.len() < 4 {
            return false;
        }
        let n = u32::from_le_bytes(packed[..4].try_into().unwrap()) as usize;
        if packed.len() != 4 + n * (8 + tile_bytes) {
            return false;
        }
        let mut tiles = TileMap::new();
        for i in 0..n {
            let at = 4 + i * (8 + tile_bytes);
            let tx = i32::from_le_bytes(packed[at..at + 4].try_into().unwrap());
            let ty = i32::from_le_bytes(packed[at + 4..at + 8].try_into().unwrap());
            let src = &packed[at + 8..at + 8 + tile_bytes];
            let mut t = empty_tile();
            for (k, px) in src.as_chunks::<4>().0.iter().enumerate() {
                if px[3] == 0 {
                    continue;
                }
                let c = [px[0], px[1], px[2]].map(|v| v as f32 / 255.0);
                t[k / ts][k % ts] = premul_fix15(c, px[3] as f32 / 255.0);
            }
            tiles.insert((tx, ty), t);
        }
        self.next_drawing = self.next_drawing.max(drawing + 1);
        if let Some(idx) = self.layers.iter().position(|l| l.drawing == drawing) {
            let l = &mut self.layers[idx];
            let keys: Vec<TileKey> = l
                .surface
                .tiles
                .keys()
                .chain(tiles.keys())
                .copied()
                .collect();
            l.surface.tiles = tiles;
            l.surface.dirty.extend(keys);
        } else {
            self.cels.insert(drawing, tiles);
        }
        true
    }

    /// Whether the engine holds a drawing's pixels (showing or kept).
    pub fn has_drawing(&self, drawing: u32) -> bool {
        drawing != 0
            && (self.cels.contains_key(&drawing)
                || self.layers.iter().any(|l| l.drawing == drawing))
    }

    /// Drawings kept here that aren't showing.
    pub fn kept_drawings(&self) -> Vec<u32> {
        self.cels.keys().copied().collect()
    }

    /// Drawings the undo / redo history changes (they must stay here to be undone).
    pub fn history_drawings(&self) -> Vec<u32> {
        let mut used = HashSet::new();
        fn collect(e: &Edit, used: &mut HashSet<u32>) {
            match e {
                Edit::Tiles { drawing, .. } if *drawing != 0 => {
                    used.insert(*drawing);
                }
                Edit::Batch(list) => list.iter().for_each(|e| collect(e, used)),
                _ => {}
            }
        }
        for e in self.undo.iter().chain(&self.redo) {
            collect(e, &mut used);
        }
        used.into_iter().collect()
    }

    /// Let go of a kept drawing (the frame store has it). False if it isn't kept here
    /// (showing, or unknown). If the history still changes it, it must be back
    /// ([`Painter::put_drawing_tiles`]) before that undo / redo: see
    /// [`Painter::undo_drawing`].
    pub fn forget_drawing(&mut self, drawing: u32) -> bool {
        self.cels.remove(&drawing).is_some()
    }

    /// The drawing the next undo changes (0: none, or not a drawing's pixels).
    pub fn undo_drawing(&self) -> u32 {
        self.undo.last().map_or(0, edit_drawing)
    }

    /// The drawing the next redo changes (0: none, or not a drawing's pixels).
    pub fn redo_drawing(&self) -> u32 {
        self.redo.last().map_or(0, edit_drawing)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.adjust_cancel();
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
        self.edits += 1;
        let inv = self.apply(e);
        self.redo.push(inv);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.adjust_cancel();
        if self.stroking.is_some() {
            return false;
        }
        let Some(e) = self.redo.pop() else {
            return false;
        };
        self.edits += 1;
        let inv = self.apply(e);
        self.undo.push(inv);
        true
    }

    /// Whether nothing visible is painted (hidden or fully transparent layers don't count).
    pub fn is_blank(&self) -> bool {
        !(0..self.layers.len()).any(|i| {
            let l = &self.layers[i];
            self.shown(i) && l.props.opacity * self.group_of(l).1 > 0.0 && !l.is_empty()
        })
    }

    /// Recomposite changed tiles into the RGBA buffer. Returns the area that
    /// changed, or `None` if nothing did.
    /// One tile of the picture, composited (None if it's off the canvas).
    fn composite_tile(&self, key: TileKey) -> Option<ComposedTile> {
        self.composite_tile_with(key, None)
    }

    /// One tile composited, with some layers showing other drawings (`over`: layer id →
    /// that drawing's tiles; none = an empty cel).
    fn composite_tile_with(
        &self,
        key: TileKey,
        over: Option<&HashMap<u32, Option<&TileMap>>>,
    ) -> Option<ComposedTile> {
        self.composite_tile_on(key, over, [1.0; 3])
    }

    /// One tile composited over a paper of colour `paper` (white for the canvas).
    fn composite_tile_on(
        &self,
        (tx, ty): TileKey,
        over: Option<&HashMap<u32, Option<&TileMap>>>,
        paper_rgb: [f32; 3],
    ) -> Option<ComposedTile> {
        let ts = TILE_SIZE as i32;
        let (w, h) = (self.width as i32, self.height as i32);
        let (px0, py0) = (tx * ts, ty * ts);
        let (px1, py1) = ((px0 + ts).min(w), (py0 + ts).min(h));
        let (cx0, cy0) = (px0.max(0), py0.max(0));
        if cx0 >= px1 || cy0 >= py1 {
            return None; // tile entirely off-canvas
        }
        // with clipping masks every layer matters (a hidden or empty base hides
        // what's clipped to it); otherwise only visible layers with paint here
        let clipping = self.layers.iter().any(|l| l.props.clip);
        let groups = &self.groups;
        let sources: Vec<TileSource<'_>> = self
            .layers
            .iter()
            .filter_map(|l| {
                // pass-through groups: their eye and opacity apply to each layer
                let (gv, go) = groups
                    .iter()
                    .find(|g| g.id == l.group)
                    .map_or((true, 1.0), |g| (g.visible, g.opacity));
                let opacity = l.props.opacity * go;
                let visible = l.props.visible && gv && opacity > 0.0;
                let adjust = l.props.adjust.as_deref();
                let tile = if adjust.is_some() {
                    None
                } else {
                    match over.and_then(|o| o.get(&l.id)) {
                        Some(tiles) => tiles.and_then(|t| t.get(&(tx, ty))),
                        None => l.colour().tiles.get(&(tx, ty)),
                    }
                };
                (clipping || (visible && (tile.is_some() || adjust.is_some()))).then(|| {
                    TileSource {
                        adjust,
                        tile: tile.map(|t| &**t),
                        opacity,
                        blend: l.props.blend,
                        visible,
                        clip: l.props.clip,
                        mask: l
                            .mask_surface()
                            .map(|m| m.tiles.get(&(tx, ty)).map(|t| &**t)),
                    }
                })
            })
            .collect();
        let paper = sources
            .iter()
            .all(|s| s.tile.is_none() && s.adjust.is_none());
        let mut px = Vec::with_capacity(((px1 - cx0) * (py1 - cy0)) as usize);
        for y in cy0..py1 {
            for x in cx0..px1 {
                px.push(if paper {
                    paper_rgb.map(|v| (v * 255.0).round() as u8)
                } else {
                    composite_pixel_on(&sources, (y - py0) as usize, (x - px0) as usize, paper_rgb)
                });
            }
        }
        Some(ComposedTile {
            x0: cx0,
            y0: cy0,
            x1: px1,
            y1: py1,
            px,
        })
    }

    pub fn flush(&mut self) -> Option<Rect> {
        for l in &mut self.layers {
            self.dirty.extend(l.surface.dirty.drain());
            if let Some(m) = l.mask.as_mut() {
                self.dirty.extend(m.dirty.drain());
            }
        }
        if self.dirty.is_empty() {
            return None;
        }
        let w = self.width as i32;
        let dirty: Vec<TileKey> = self.dirty.drain().collect();
        // each tile on its own (on every core natively), then copied into the picture
        #[cfg(not(target_arch = "wasm32"))]
        let done: Vec<ComposedTile> = {
            use rayon::prelude::*;
            dirty
                .par_iter()
                .filter_map(|&k| self.composite_tile(k))
                .collect()
        };
        #[cfg(target_arch = "wasm32")]
        let done: Vec<ComposedTile> = dirty
            .iter()
            .filter_map(|&k| self.composite_tile(k))
            .collect();
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for t in done {
            let tw = (t.x1 - t.x0) as usize;
            for (row, y) in (t.y0..t.y1).enumerate() {
                let dst = ((y * w + t.x0) * 4) as usize;
                for (i, px) in t.px[row * tw..(row + 1) * tw].iter().enumerate() {
                    self.rgba[dst + i * 4..dst + i * 4 + 3].copy_from_slice(px);
                    self.rgba[dst + i * 4 + 3] = 255;
                }
            }
            x0 = x0.min(t.x0);
            y0 = y0.min(t.y0);
            x1 = x1.max(t.x1);
            y1 = y1.max(t.y1);
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

    /// Onion skin: drawings as tinted lines over each other, on transparency, for showing
    /// over the canvas. `entries` are (drawing, colour 0xRRGGBB, opacity 0–255) triples,
    /// farthest first (each is laid over the ones before). Made `shrink` times smaller
    /// ([`Painter::frame_size`]); thin lines keep their strength when shrunk. Straight-alpha
    /// RGBA8. Drawings must be here; a missing one is skipped.
    pub fn onion_rgba(&self, entries: &[u32], shrink: u32) -> Vec<u8> {
        let k = shrink.max(1) as usize;
        let ts = TILE_SIZE;
        let (w, h) = (self.width as usize, self.height as usize);
        let (ow, oh) = (w.div_ceil(k), h.div_ceil(k));
        // straight-alpha colour and alpha, laid one over another
        let mut rgb = vec![[0f32; 3]; ow * oh];
        let mut alpha = vec![0f32; ow * oh];
        let mut cover = vec![0f32; ow * oh];
        let per = 1.0 / (k * k) as f32;
        for e in entries.as_chunks::<3>().0 {
            let [drawing, colour, opacity] = *e;
            let Some(tiles) = self.drawing_tiles(drawing) else {
                continue;
            };
            let tint = [
                ((colour >> 16) & 255) as f32 / 255.0,
                ((colour >> 8) & 255) as f32 / 255.0,
                (colour & 255) as f32 / 255.0,
            ];
            let op = (opacity.min(255)) as f32 / 255.0;
            cover.iter_mut().for_each(|c| *c = 0.0);
            let mut touched = Vec::new();
            for (&(tx, ty), px) in tiles {
                for (py, row) in px.iter().enumerate() {
                    let y = ty as i64 * ts as i64 + py as i64;
                    if y < 0 || y >= h as i64 {
                        continue;
                    }
                    for (pxi, p) in row.iter().enumerate() {
                        if p[3] == 0 {
                            continue;
                        }
                        let x = tx as i64 * ts as i64 + pxi as i64;
                        if x < 0 || x >= w as i64 {
                            continue;
                        }
                        let o = (y as usize / k) * ow + x as usize / k;
                        if cover[o] == 0.0 {
                            touched.push(o);
                        }
                        cover[o] += p[3] as f32 / 32768.0 * per;
                    }
                }
            }
            for o in touched {
                // shrunk lines get a boost (a 1 px line in a 4×4 square is still a line)
                let a = (cover[o] * k as f32).min(1.0) * op;
                let keep = alpha[o] * (1.0 - a);
                let out = a + keep;
                for c in 0..3 {
                    rgb[o][c] = (tint[c] * a + rgb[o][c] * keep) / out.max(1e-6);
                }
                alpha[o] = out;
            }
        }
        let mut img = vec![0u8; ow * oh * 4];
        for (i, px) in img.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if alpha[i] > 0.0 {
                for c in 0..3 {
                    px[c] = (rgb[i][c] * 255.0).round() as u8;
                }
                px[3] = (alpha[i] * 255.0).round() as u8;
            }
        }
        img
    }

    /// Size of a [`Painter::render_frame`] picture made `shrink` times smaller.
    pub fn frame_size(&self, shrink: u32) -> (u32, u32) {
        let k = shrink.max(1);
        (self.width.div_ceil(k), self.height.div_ceil(k))
    }

    /// The whole picture as it would look with other drawings showing (an animation
    /// frame), without changing anything: `pairs` is (layer id, drawing id) flattened
    /// (drawing 0 = an empty cel; layers not named show what they show). Made `shrink`
    /// times smaller (each pixel the average of its square), as RGBA8 over the paper.
    /// Drawings must be here (showing or kept); a missing one shows as empty.
    pub fn render_frame(&self, pairs: &[u32], shrink: u32) -> Vec<u8> {
        self.render_frame_on(pairs, shrink, [1.0; 3])
    }

    /// A frame as `render_frame` makes it, but on transparency (straight-alpha RGBA8):
    /// for exports with an alpha channel. It's composited over white and over black,
    /// and the difference gives each pixel's alpha (exact for normal layers; blend
    /// modes that need the paper show as they would over black).
    pub fn render_frame_alpha(&self, pairs: &[u32], shrink: u32) -> Vec<u8> {
        let white = self.render_frame_on(pairs, shrink, [1.0; 3]);
        let mut out = self.render_frame_on(pairs, shrink, [0.0; 3]);
        for (b, w) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(white.as_chunks::<4>().0)
        {
            // over white w = c·a + (1 − a), over black b = c·a: so a = 1 − (w − b)
            let diff = (0..3)
                .map(|i| w[i] as i32 - b[i] as i32)
                .sum::<i32>()
                .clamp(0, 765);
            let a = 255 - (diff + 1) / 3;
            if a <= 0 {
                *b = [0; 4];
                continue;
            }
            for v in &mut b[..3] {
                *v = ((*v as i32 * 255 + a / 2) / a).min(255) as u8;
            }
            b[3] = a as u8;
        }
        out
    }

    fn render_frame_on(&self, pairs: &[u32], shrink: u32, paper: [f32; 3]) -> Vec<u8> {
        let k = shrink.max(1) as usize;
        let over: HashMap<u32, Option<&TileMap>> = pairs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[l, d]| (l, self.drawing_tiles(d)))
            .collect();
        let ts = TILE_SIZE;
        let (w, h) = (self.width as usize, self.height as usize);
        let keys: Vec<TileKey> = (0..h.div_ceil(ts))
            .flat_map(|ty| (0..w.div_ceil(ts)).map(move |tx| (tx as i32, ty as i32)))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let done: Vec<ComposedTile> = {
            use rayon::prelude::*;
            keys.par_iter()
                .filter_map(|&key| self.composite_tile_on(key, Some(&over), paper))
                .collect()
        };
        #[cfg(target_arch = "wasm32")]
        let done: Vec<ComposedTile> = keys
            .iter()
            .filter_map(|&key| self.composite_tile_on(key, Some(&over), paper))
            .collect();
        let (ow, oh) = (w.div_ceil(k), h.div_ceil(k));
        let mut sum = vec![[0u32; 4]; ow * oh];
        for t in done {
            let tw = (t.x1 - t.x0) as usize;
            for (row, y) in (t.y0..t.y1).enumerate() {
                let o = (y as usize / k) * ow;
                for (i, px) in t.px[row * tw..(row + 1) * tw].iter().enumerate() {
                    let s = &mut sum[o + (t.x0 as usize + i) / k];
                    s[0] += px[0] as u32;
                    s[1] += px[1] as u32;
                    s[2] += px[2] as u32;
                    s[3] += 1;
                }
            }
        }
        let mut out = vec![255u8; ow * oh * 4];
        for (px, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(&sum) {
            for c in 0..3 {
                if let Some(v) = (s[c] + s[3] / 2).checked_div(s[3]) {
                    px[c] = v as u8;
                }
            }
        }
        out
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
        for (&(kx, ky), tile) in &l.colour().tiles {
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

    /// A layer's own pixels at full size (straight-alpha sRGB RGBA8, no mask
    /// applied); empty if there's no such layer. For exporting with layers.
    pub fn layer_rgba(&self, id: u32) -> Vec<u8> {
        let Some(l) = self.layers.iter().find(|l| l.id == id) else {
            return Vec::new();
        };
        self.tiles_rgba(&l.colour().tiles)
    }

    /// Tiles at full canvas size (straight-alpha sRGB RGBA8).
    fn tiles_rgba(&self, tiles: &TileMap) -> Vec<u8> {
        let (w, h) = (self.width as i64, self.height as i64);
        let ts = TILE_SIZE as i64;
        let mut out = vec![0u8; (w * h * 4) as usize];
        for (&(tx, ty), tile) in tiles {
            for (py, row) in tile.iter().enumerate() {
                let y = ty as i64 * ts + py as i64;
                if y < 0 || y >= h {
                    continue;
                }
                for (px, p) in row.iter().enumerate() {
                    let x = tx as i64 * ts + px as i64;
                    if x < 0 || x >= w || p[3] == 0 {
                        continue;
                    }
                    let (c, a) = unpremul_srgb(*p);
                    let i = ((y * w + x) * 4) as usize;
                    out[i] = (c[0] * 255.0).round() as u8;
                    out[i + 1] = (c[1] * 255.0).round() as u8;
                    out[i + 2] = (c[2] * 255.0).round() as u8;
                    out[i + 3] = (a * 255.0).round() as u8;
                }
            }
        }
        out
    }

    /// A small greyscale preview of a layer's mask (RGBA, `tw`×`th`; empty if it has none).
    pub fn mask_thumbnail(&self, id: u32, tw: u32, th: u32) -> Vec<u8> {
        let Some(m) = self
            .layers
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.mask_surface())
        else {
            return Vec::new();
        };
        let ts = TILE_SIZE as u32;
        let mut out = Vec::with_capacity((tw * th * 4) as usize);
        for j in 0..th {
            for i in 0..tw {
                let x = (i * self.width + self.width / (2 * tw.max(1))) / tw.max(1);
                let y = (j * self.height + self.height / (2 * th.max(1))) / th.max(1);
                let key = ((x / ts) as i32, (y / ts) as i32);
                let v = m
                    .tiles
                    .get(&key)
                    .map_or(1.0, |t| mask_value(t[(y % ts) as usize][(x % ts) as usize]));
                let g = (v * 255.0).round() as u8;
                out.extend_from_slice(&[g, g, g, 255]);
            }
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
                    r#"{{"id":{},"name":{},"visible":{},"opacity":{},"blend":"{}","locked":{},"alphaLock":{},"clip":{},"mask":{},"editingMask":{},"group":{},"imported":{},"empty":{},"drawing":{},"adjust":{}}}"#,
                    l.id,
                    json_str(&p.name),
                    p.visible,
                    (p.opacity * 1000.0).round() / 1000.0,
                    p.blend.as_str(),
                    p.locked,
                    p.alpha_lock,
                    p.clip,
                    l.mask.is_some(),
                    l.editing_mask,
                    l.group,
                    p.imported,
                    l.is_empty(),
                    l.drawing,
                    if p.adjust.is_some() {
                        json_str(&p.adjust_spec)
                    } else {
                        "null".into()
                    }
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }
}

/// Swap tiles in a kept drawing, returning what was replaced (as [`UndoSurface::swap_in`]).
fn swap_tiles(tiles: &mut TileMap, snap: Snapshot) -> Snapshot {
    snap.into_iter()
        .map(|(key, tile)| {
            let old = match tile {
                Some(t) => tiles.insert(key, t),
                None => tiles.remove(&key),
            };
            (key, old)
        })
        .collect()
}

/// The drawing an undo step changes (the first one, for a batch; 0 for none).
fn edit_note(e: &Edit) -> u32 {
    match e {
        Edit::Note(id) => *id,
        Edit::Batch(list) => list.iter().map(edit_note).find(|&d| d != 0).unwrap_or(0),
        _ => 0,
    }
}

fn edit_drawing(e: &Edit) -> u32 {
    match e {
        Edit::Tiles {
            drawing,
            mask: false,
            ..
        } => *drawing,
        Edit::Batch(list) => list.iter().map(edit_drawing).find(|&d| d != 0).unwrap_or(0),
        _ => 0,
    }
}

/// `t` moved by (dx, dy) pixels, keeping only what's on a `w`×`h` canvas.
fn shift_tiles(t: &TileMap, dx: i32, dy: i32, w: u32, h: u32) -> TileMap {
    let ts = TILE_SIZE as i64;
    let mut out = TileMap::new();
    for (&(tx, ty), tile) in t {
        for (py, row) in tile.iter().enumerate() {
            for (px, p) in row.iter().enumerate() {
                if p[3] == 0 {
                    continue;
                }
                let x = tx as i64 * ts + px as i64 + dx as i64;
                let y = ty as i64 * ts + py as i64 + dy as i64;
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                let o = out
                    .entry(((x / ts) as i32, (y / ts) as i32))
                    .or_insert_with(empty_tile);
                o[(y % ts) as usize][(x % ts) as usize] = *p;
            }
        }
    }
    out
}

/// `t` (on an `ow`×`oh` canvas) resized to fill `w`×`h`: each new pixel
/// averages a few samples over its footprint (premultiplied, so edges stay clean).
fn scale_tiles(t: &TileMap, ow: u32, oh: u32, w: u32, h: u32) -> TileMap {
    let ts = TILE_SIZE as i64;
    let (sx, sy) = (ow as f32 / w as f32, oh as f32 / h as f32);
    let get = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= ow as i64 || y >= oh as i64 {
            return [0.0; 4];
        }
        t.get(&((x / ts) as i32, (y / ts) as i32))
            .map_or([0.0; 4], |tile| {
                tile[(y % ts) as usize][(x % ts) as usize].map(|v| v as f32)
            })
    };
    // bilinear at a source point (pixel centres at .5)
    let bil = |x: f32, y: f32| -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            get(x0, y0),
            get(x0 + 1, y0),
            get(x0, y0 + 1),
            get(x0 + 1, y0 + 1),
        );
        [0, 1, 2, 3].map(|i| {
            (a[i] * (1.0 - ax) + b[i] * ax) * (1.0 - ay) + (c[i] * (1.0 - ax) + d[i] * ax) * ay
        })
    };
    let (nx, ny) = (
        (sx.ceil() as usize).clamp(1, 4),
        (sy.ceil() as usize).clamp(1, 4),
    );
    // the new tiles that can have paint: each old tile's area, scaled
    let mut keys = HashSet::new();
    for &(tx, ty) in t.keys() {
        let (x0, y0) = ((tx as i64 * ts) as f32 / sx, (ty as i64 * ts) as f32 / sy);
        let (x1, y1) = (
            ((tx as i64 + 1) * ts) as f32 / sx,
            ((ty as i64 + 1) * ts) as f32 / sy,
        );
        let k0 = (
            ((x0 - 2.0).max(0.0) as i64) / ts,
            ((y0 - 2.0).max(0.0) as i64) / ts,
        );
        let k1 = (
            ((x1 + 2.0).min(w as f32 - 1.0) as i64) / ts,
            ((y1 + 2.0).min(h as f32 - 1.0) as i64) / ts,
        );
        for ky in k0.1..=k1.1 {
            for kx in k0.0..=k1.0 {
                keys.insert((kx as i32, ky as i32));
            }
        }
    }
    let mut out = TileMap::new();
    for (kx, ky) in keys {
        let mut tile = empty_tile();
        let mut any = false;
        for (py, row) in tile.iter_mut().enumerate() {
            for (px, p) in row.iter_mut().enumerate() {
                let (x, y) = (kx as i64 * ts + px as i64, ky as i64 * ts + py as i64);
                if x >= w as i64 || y >= h as i64 {
                    continue;
                }
                let mut acc = [0.0f32; 4];
                for j in 0..ny {
                    for i in 0..nx {
                        let s = bil(
                            (x as f32 + (i as f32 + 0.5) / nx as f32) * sx,
                            (y as f32 + (j as f32 + 0.5) / ny as f32) * sy,
                        );
                        for c in 0..4 {
                            acc[c] += s[c];
                        }
                    }
                }
                let n = (nx * ny) as f32;
                *p = acc.map(|v| (v / n).round().clamp(0.0, 32768.0) as u16);
                any |= p[3] > 0;
            }
        }
        if any {
            out.insert((kx, ky), tile);
        }
    }
    out
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
/// The identity projective matrix (row-major 3×3).
const H_IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// `h` applied to (x, y); None behind the camera (w ≤ 0) or at infinity.
fn h_apply(h: [f32; 9], x: f32, y: f32) -> Option<(f32, f32)> {
    let w = h[6] * x + h[7] * y + h[8];
    if w <= 1e-6 {
        return None;
    }
    Some((
        (h[0] * x + h[1] * y + h[2]) / w,
        (h[3] * x + h[4] * y + h[5]) / w,
    ))
}

/// Bounding box (inclusive, padded by a pixel) of four points.
fn quad_bbox(q: &[(f32, f32); 4]) -> (i64, i64, i64, i64) {
    let xs = q.map(|p| p.0);
    let ys = q.map(|p| p.1);
    (
        xs.iter().copied().fold(f32::MAX, f32::min).floor() as i64 - 1,
        ys.iter().copied().fold(f32::MAX, f32::min).floor() as i64 - 1,
        xs.iter().copied().fold(f32::MIN, f32::max).ceil() as i64 + 1,
        ys.iter().copied().fold(f32::MIN, f32::max).ceil() as i64 + 1,
    )
}

/// The projective map taking rectangle (x0, y0, x1, y1)'s corners (top-left,
/// top-right, bottom-right, bottom-left) to `q` (Heckbert's square-to-quad).
fn rect_to_quad(r: (f32, f32, f32, f32), q: [(f32, f32); 4]) -> Option<[f32; 9]> {
    let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = q.map(|(x, y)| (x as f64, y as f64));
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let sq: [f64; 9] = if sx.abs() < 1e-9 && sy.abs() < 1e-9 {
        [x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0, 0.0, 0.0, 1.0]
    } else {
        let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
        let den = dx1 * dy2 - dx2 * dy1;
        if den.abs() < 1e-12 {
            return None;
        }
        let g = (sx * dy2 - dx2 * sy) / den;
        let h = (dx1 * sy - sx * dy1) / den;
        [
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            y0,
            g,
            h,
            1.0,
        ]
    };
    let (rw, rh) = ((r.2 - r.0) as f64, (r.3 - r.1) as f64);
    if rw <= 0.0 || rh <= 0.0 {
        return None;
    }
    // unit square ← rectangle: u = (x - x0) / w, v = (y - y0) / h
    let s = [
        1.0 / rw,
        0.0,
        -r.0 as f64 / rw,
        0.0,
        1.0 / rh,
        -r.1 as f64 / rh,
        0.0,
        0.0,
        1.0,
    ];
    let mut m = [0.0f32; 9];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 3 + j] = (0..3).map(|k| sq[i * 3 + k] * s[k * 3 + j]).sum::<f64>() as f32;
        }
    }
    Some(m)
}

/// One box-blur pass along rows (`horizontal`) or columns, radius `r`; edges clamp.
fn box_blur_line(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (n, lines) = if horizontal { (w, h) } else { (h, w) };
    let at = |line: usize, i: usize| {
        if horizontal {
            line * w + i
        } else {
            i * w + line
        }
    };
    let k = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let get = |i: isize| src[at(line, i.clamp(0, n as isize - 1) as usize)];
        let mut acc: f32 = (-(r as isize)..=r as isize).map(get).sum();
        for i in 0..n {
            dst[at(line, i)] = acc * k;
            acc += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
        }
    }
}

/// Premultiplied blend from `a` (t = 0) to `b` (t = 1): soft selections.
pub(crate) fn lerp_px(a: [u16; 4], b: [u16; 4], t: f32) -> [u16; 4] {
    if t >= 1.0 {
        return b;
    }
    if t <= 0.0 {
        return a;
    }
    let mut o = [0u16; 4];
    for k in 0..4 {
        o[k] = (a[k] as f32 + (b[k] as f32 - a[k] as f32) * t)
            .round()
            .clamp(0.0, 32768.0) as u16;
    }
    o
}

/// Inverse of a 3×3 matrix; None if singular.
fn h_invert(m: [f32; 9]) -> Option<[f32; 9]> {
    let m = m.map(|v| v as f64);
    let c0 = m[4] * m[8] - m[5] * m[7];
    let c1 = m[5] * m[6] - m[3] * m[8];
    let c2 = m[3] * m[7] - m[4] * m[6];
    let det = m[0] * c0 + m[1] * c1 + m[2] * c2;
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = [
        c0,
        m[2] * m[7] - m[1] * m[8],
        m[1] * m[5] - m[2] * m[4],
        c1,
        m[0] * m[8] - m[2] * m[6],
        m[2] * m[3] - m[0] * m[5],
        c2,
        m[1] * m[6] - m[0] * m[7],
        m[0] * m[4] - m[1] * m[3],
    ]
    // the exact inverse: a point in front of the camera maps back with w > 0
    .map(|v| (v / det) as f32);
    Some(inv)
}

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

    /// a fat line for texture tests (radius ≈ 12 px)
    fn fat_line(p: &mut Painter, y: f32) {
        let mut b = pen();
        b.get_mut(BrushSetting::Radius).base_value = 2.5;
        line(p, &b, y);
    }

    #[test]
    fn grain_leaves_gaps_in_the_paint() {
        let mut plain = Painter::new(256, 128);
        fat_line(&mut plain, 60.0);
        plain.flush();
        let mut grainy = Painter::new(256, 128);
        // stripes: 2 px painted, 2 px bare
        let tex: Vec<u8> = (0..16).map(|i| if i % 4 < 2 { 255 } else { 0 }).collect();
        assert!(grainy.set_texture(7, 4, 4, tex));
        grainy.set_dab_style(7, 1.0, 1.0, 0, 0, 0.0, 0.0, 1);
        fat_line(&mut grainy, 60.0);
        grainy.flush();
        let (a, b) = (dark_pixels(&plain), dark_pixels(&grainy));
        assert!(b * 10 < a * 7 && b > 0, "grain thins the line: {a} → {b}");
        // the bare columns stay paper-white in the middle of the line
        assert!(px(&grainy, 102, 60)[0] > 200 || px(&grainy, 103, 60)[0] > 200);
        // the style is per stroke: undo and the plain style again paint solid
        grainy.set_dab_style(0, 1.0, 0.0, 0, 0, 0.0, 0.0, 1);
        grainy.undo();
        fat_line(&mut grainy, 60.0);
        grainy.flush();
        assert_eq!(dark_pixels(&grainy), a);
    }

    #[test]
    fn a_tip_picture_shapes_each_dab() {
        let mut p = Painter::new(128, 128);
        // the top half of the tip is painted, the bottom half bare
        let n = 16;
        let tex: Vec<u8> = (0..n * n)
            .map(|i| if i / n < 8 { 255 } else { 0 })
            .collect();
        p.set_texture(1, n as u32, n as u32, tex);
        p.set_dab_style(0, 1.0, 0.0, 1, 0, 0.0, 0.0, 1);
        let mut b = pen();
        b.get_mut(BrushSetting::Radius).base_value = 2.5;
        p.begin_stroke();
        for i in 0..=20 {
            p.stroke_to(&b, 20.0 + i as f32 * 4.0, 64.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&b);
        p.flush();
        assert!(px(&p, 60, 58)[0] < 100, "above the line: painted");
        assert!(px(&p, 60, 70)[0] > 200, "below: bare");
    }

    #[test]
    fn adjustment_layers_change_whats_below_and_undo_in_one_step() {
        let mut p = Painter::new(128, 64);
        let mut red = pen();
        set_brush_rgb(&mut red, 220, 0, 0);
        line(&mut p, &red, 30.0);
        p.flush();
        assert!(px(&p, 60, 30)[0] > 150);
        let id = p.add_adjustment_layer(
            "Hue",
            adjust::ColourAdjust {
                hue: 120.0,
                ..Default::default()
            },
            "{}",
        );
        assert!(id > 0);
        p.flush();
        let c = px(&p, 60, 30);
        assert!(c[1] > c[0] + 60, "red looks green: {c:?}");
        assert!(p.layers_json().contains(r#""adjust":"{}""#));
        // live changes, then one undo step back to the first setting
        for h in [60.0, 240.0] {
            p.adjustment_preview(
                id,
                adjust::ColourAdjust {
                    hue: h,
                    ..Default::default()
                },
                "{\"h\":1}",
            );
        }
        assert!(p.adjustment_commit());
        p.flush();
        let c = px(&p, 60, 30);
        assert!(c[2] > c[0] + 60, "blue now: {c:?}");
        p.undo();
        p.flush();
        let c = px(&p, 60, 30);
        assert!(c[1] > c[0] + 60, "green again: {c:?}");
        // no painting on it except on its mask
        assert!(p.begin_stroke(), "paints on the mask");
        p.cancel_stroke();
        // merging it down bakes the change into the layer below
        assert!(p.merge_down(id));
        p.flush();
        let c = px(&p, 60, 30);
        assert!(c[1] > c[0] + 60, "still green, now in the pixels: {c:?}");
        assert_eq!(p.layers().len(), 1);
        p.undo();
        p.flush();
        assert_eq!(p.layers().len(), 2);
    }

    #[test]
    fn canvas_crop_extend_and_scale_are_undoable() {
        let mut p = Painter::new(200, 100);
        line(&mut p, &pen(), 50.0); // x 20 … 180
        p.flush();
        let dark = dark_pixels(&p);
        // crop the left 100 px off
        assert!(p.resize_canvas(100, 100, -100, 0, false));
        p.flush();
        assert_eq!((p.width(), p.height()), (100, 100));
        assert_eq!(p.rgba().len(), 100 * 100 * 4);
        assert!(
            px(&p, 50, 50)[0] < 100,
            "the line's right part is still there"
        );
        assert!(dark_pixels(&p) < dark);
        p.undo();
        p.flush();
        assert_eq!((p.width(), p.height()), (200, 100));
        assert_eq!(dark_pixels(&p), dark);
        p.redo();
        p.undo();
        // scale to half size: about half the ink
        assert!(p.resize_canvas(100, 50, 0, 0, true));
        p.flush();
        assert!(px(&p, 50, 25)[0] < 160, "{:?}", px(&p, 50, 25));
        let half = dark_pixels(&p);
        assert!(half > 0 && half < dark, "{half} vs {dark}");
        // more room: moves the drawing over
        p.undo();
        assert!(p.resize_canvas(300, 100, 50, 0, false));
        p.flush();
        assert!(px(&p, 120, 50)[0] < 100 && px(&p, 30, 50)[0] > 200);
    }

    #[test]
    fn colour_range_selects_unconnected_areas() {
        let mut p = Painter::new(200, 100);
        line(&mut p, &pen(), 20.0);
        line(&mut p, &pen(), 80.0);
        p.flush();
        assert!(p.select_colour_range(60, 20, 0.2, true, 0));
        let m = p.selection_mask();
        assert!(m[80 * 200 + 60] > 0, "the other line too");
        assert_eq!(m[50 * 200 + 60], 0, "not the paper");
        p.deselect();
        assert!(p.select_wand(60, 20, 0.2, true, 0));
        assert_eq!(
            p.selection_mask()[80 * 200 + 60],
            0,
            "the wand only takes what's joined"
        );
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

    // ---- drawings (animation cels) ----

    /// dark pixels in rows y0..y1
    fn dark_rows(p: &Painter, y0: u32, y1: u32) -> usize {
        let w = p.width() as usize;
        p.rgba()[y0 as usize * w * 4..y1 as usize * w * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] < 128)
            .count()
    }

    #[test]
    fn frames_swap_drawings_without_losing_them() {
        let mut p = Painter::new(256, 128);
        let layer = p.active_layer();
        let b = pen();
        line(&mut p, &b, 30.0); // frame 1: a line at the top
        let first = p.layer_drawing(layer);
        assert_ne!(first, 0, "drawing on an empty cel starts a drawing");
        p.flush();
        let top = dark_rows(&p, 20, 40);
        assert!(top > 50);
        // frame 2: an empty cel, then a line at the bottom
        assert!(p.show_drawings(&[layer, 0]));
        p.flush();
        assert_eq!(dark_pixels(&p), 0);
        line(&mut p, &b, 100.0);
        let second = p.layer_drawing(layer);
        assert!(second != 0 && second != first);
        p.flush();
        assert_eq!(dark_rows(&p, 20, 40), 0);
        // back to frame 1: its line, not frame 2's
        assert!(p.show_drawings(&[layer, first]));
        p.flush();
        assert_eq!(dark_rows(&p, 20, 40), top);
        assert_eq!(dark_rows(&p, 90, 110), 0);
        assert!(p.drawing_rgba(second).chunks(4).any(|px| px[3] > 0));
        assert!(!p.drawing_empty(second) && p.drawing_empty(999));
    }

    #[test]
    fn undo_changes_the_drawing_it_was_made_on() {
        let mut p = Painter::new(256, 128);
        let layer = p.active_layer();
        let b = pen();
        line(&mut p, &b, 30.0);
        let first = p.layer_drawing(layer);
        p.show_drawings(&[layer, 0]);
        line(&mut p, &b, 100.0);
        let second = p.layer_drawing(layer);
        assert_eq!(p.undo_drawing(), second);
        p.undo();
        assert_eq!(p.undo_drawing(), first, "the next undo is frame 1's stroke");
        // undoing it while frame 2 shows: frame 2 stays as it is, frame 1 loses its line
        p.undo();
        assert!(p.drawing_empty(first));
        assert_eq!(p.redo_drawing(), first);
        p.redo();
        assert!(!p.drawing_empty(first));
        p.show_drawings(&[layer, first]);
        p.flush();
        assert!(dark_rows(&p, 20, 40) > 50);
    }

    #[test]
    fn drawings_copy_load_and_stay_with_their_layer() {
        let mut p = Painter::new(128, 64);
        let layer = p.active_layer();
        line(&mut p, &pen(), 30.0);
        let first = p.layer_drawing(layer);
        let copy = p.copy_drawing(first);
        assert_eq!(p.drawing_rgba(copy), p.drawing_rgba(first));
        // loading a drawing (opening a project)
        let mut rgba = vec![0u8; 128 * 64 * 4];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        assert!(p.put_drawing(40, &rgba));
        assert_eq!(&p.drawing_rgba(40)[..4], &[255, 0, 0, 255]);
        assert!(p.new_drawing() > 40);
        // one drawing can't show on two layers at once
        let other = p.add_layer("B");
        assert!(!p.show_drawings(&[layer, 40, other, 40]));
        assert!(p.show_drawings(&[other, 40]));
        // a duplicated layer gets its own drawing
        let dup = p.duplicate_layer(layer);
        assert!(p.layer_drawing(dup) != 0 && p.layer_drawing(dup) != first);
        // forgetting unused drawings keeps what's showing and what undo needs
        p.drop_drawings_except(&[]);
        assert!(!p.drawing_empty(first) && !p.drawing_empty(40));
    }

    #[test]
    fn a_pasted_picture_gets_a_drawing_before_frames_change() {
        let mut p = Painter::new(64, 64);
        let rgba = vec![200u8; 64 * 64 * 4];
        let id = p.add_image_layer("Picture", &rgba);
        assert_eq!(p.layer_drawing(id), 0);
        assert!(p.show_drawings(&[id, 0]));
        // its pixels were kept as a drawing of their own
        let kept = p.layers().iter().find(|l| l.id == id).unwrap();
        assert!(kept.is_empty());
        assert_eq!(p.cels.len(), 1);
        let d = *p.cels.keys().next().unwrap();
        assert!(p.show_drawings(&[id, d]));
        assert!(!p.layers().iter().find(|l| l.id == id).unwrap().is_empty());
    }

    #[test]
    fn drawings_pack_into_file_tiles_and_back() {
        let mut p = Painter::new(200, 100);
        let layer = p.active_layer();
        line(&mut p, &pen(), 40.0);
        let d = p.layer_drawing(layer);
        let packed = p.drawing_tiles_packed(d);
        let n = u32::from_le_bytes(packed[..4].try_into().unwrap()) as usize;
        assert!(n > 0 && packed.len() == 4 + n * (8 + 64 * 64 * 4));
        // the same pixels, under another id
        assert!(p.put_drawing_tiles(50, &packed));
        assert_eq!(p.drawing_rgba(50), p.drawing_rgba(d));
        assert!(!p.put_drawing_tiles(51, &packed[..packed.len() - 1]));
        assert_eq!(p.drawing_tiles_packed(999), 0u32.to_le_bytes());
        // a kept drawing can go (the store has it), unless undo needs it
        p.show_drawings(&[layer, 0]);
        assert!(p.has_drawing(50) && p.kept_drawings().contains(&50));
        assert!(p.forget_drawing(50));
        assert!(!p.has_drawing(50));
        assert!(p.history_drawings().contains(&d));
        // one the history needs can go too, if it's back before its undo
        let back = p.drawing_tiles_packed(d);
        assert!(p.forget_drawing(d) && !p.has_drawing(d));
        assert_eq!(p.undo_drawing(), d);
        assert!(p.put_drawing_tiles(d, &back));
        p.undo();
        assert!(p.drawing_empty(d));
    }

    #[test]
    fn frames_render_without_changing_the_canvas() {
        let mut p = Painter::new(256, 128);
        let layer = p.active_layer();
        line(&mut p, &pen(), 30.0);
        let first = p.layer_drawing(layer);
        p.show_drawings(&[layer, 0]);
        line(&mut p, &pen(), 100.0);
        p.flush();
        let before = p.rgba().to_vec();
        // frame 1 at full size: the top line, not the bottom one
        let f1 = p.render_frame(&[layer, first], 1);
        let dark = |img: &[u8], w: usize, y0: usize, y1: usize| {
            img[y0 * w * 4..y1 * w * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[0] < 128)
                .count()
        };
        assert!(dark(&f1, 256, 20, 40) > 50);
        assert_eq!(dark(&f1, 256, 90, 110), 0);
        // what shows now, at a quarter of the size (each side): about the same ink area
        let small = p.render_frame(&[], 4);
        assert_eq!(p.frame_size(4), (64, 32));
        assert_eq!(small.len(), 64 * 32 * 4);
        let grey = small
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] < 230)
            .count();
        assert!(grey > 5, "{grey}");
        // an empty cel, and the canvas untouched by any of it
        let blank = p.render_frame(&[layer, 0], 2);
        assert!(blank.iter().all(|&v| v == 255));
        assert_eq!(p.rgba(), &before[..]);
    }

    #[test]
    fn onion_skins_tint_drawings_over_each_other() {
        let mut p = Painter::new(256, 128);
        let layer = p.active_layer();
        line(&mut p, &pen(), 30.0);
        let before = p.layer_drawing(layer);
        p.show_drawings(&[layer, 0]);
        line(&mut p, &pen(), 100.0);
        let after = p.layer_drawing(layer);
        let img = p.onion_rgba(&[before, 0xff0000, 128, after, 0x00ff00, 255], 1);
        let at = |x: usize, y: usize| {
            let i = (y * 256 + x) * 4;
            [img[i], img[i + 1], img[i + 2], img[i + 3]]
        };
        // red, half see-through, where the earlier line is; green where the later one is
        let r = at(100, 30);
        assert!(
            r[0] > 200 && r[1] < 50 && (100..=140).contains(&r[3]),
            "{r:?}"
        );
        let g = at(100, 100);
        assert!(g[1] > 200 && g[0] < 50 && g[3] > 240, "{g:?}");
        assert_eq!(at(100, 70)[3], 0, "clear between them");
        // shrunk: the lines are still there
        let small = p.onion_rgba(&[before, 0xff0000, 255], 4);
        assert_eq!(small.len(), 64 * 32 * 4);
        assert!(
            small
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 128)
                .count()
                > 10
        );
        // an unknown drawing is skipped
        assert!(p.onion_rgba(&[999, 0, 255], 1).iter().all(|&v| v == 0));
    }

    #[test]
    fn frames_render_on_transparency() {
        let mut p = Painter::new(64, 64);
        let layer = p.active_layer();
        line(&mut p, &pen(), 32.0);
        let d = p.layer_drawing(layer);
        let img = p.render_frame_alpha(&[layer, d], 1);
        let at = |x: usize, y: usize| {
            let i = (y * 64 + x) * 4;
            [img[i], img[i + 1], img[i + 2], img[i + 3]]
        };
        // the line is there and opaque-ish and dark; elsewhere is see-through
        assert_eq!(at(32, 5)[3], 0, "no paper");
        let on = at(32, 32);
        assert!(on[3] > 200 && on[0] < 80, "{on:?}");
        // half-transparent paint keeps its colour and gets half alpha
        let mut q = Painter::new(16, 16);
        let mut sel = vec![0u8; 256];
        sel.iter_mut().for_each(|v| *v = 255);
        q.set_selection(&sel, 0);
        q.fill_selection([200, 40, 40], 0.5);
        let l = q.active_layer();
        let im = q.render_frame_alpha(&[l, q.layer_drawing(l)], 1);
        let px = [im[0], im[1], im[2], im[3]];
        assert!((px[3] as i32 - 128).abs() <= 3, "{px:?}");
        assert!(
            (px[0] as i32 - 200).abs() <= 4 && (px[1] as i32 - 40).abs() <= 4,
            "{px:?}"
        );
    }

    #[test]
    fn two_edits_join_into_one_undo_step() {
        let mut p = Painter::new(64, 64);
        line(&mut p, &pen(), 20.0);
        line(&mut p, &pen(), 40.0);
        p.flush();
        let one = p.rgba().to_vec();
        assert!(p.join_last_edits());
        assert!(p.undo());
        p.flush();
        assert!(!p.can_undo(), "both went in one step");
        assert!(p.redo());
        p.flush();
        assert_eq!(p.rgba(), &one[..]);
        assert!(!p.join_last_edits(), "only one step left to join");
    }

    #[test]
    fn the_apps_steps_take_their_place_in_the_history() {
        let mut p = Painter::new(64, 64);
        line(&mut p, &pen(), 20.0);
        assert!(p.push_note(7));
        assert!(!p.push_note(0));
        assert_eq!(p.undo_note(), 7);
        assert_eq!(p.undo_drawing(), 0, "a note changes no drawing");
        p.flush();
        let drawn = p.rgba().to_vec();
        // undoing the note changes no pixels, and comes back on redo
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &drawn[..]);
        assert_eq!(p.redo_note(), 7);
        assert_eq!(p.undo_note(), 0);
        assert!(p.redo());
        // joined with the stroke before it: one step that names both
        line(&mut p, &pen(), 40.0);
        assert!(p.push_note(8));
        assert!(p.join_last_edits());
        assert_eq!(p.undo_note(), 8);
        assert_ne!(p.undo_drawing(), 0);
        assert!(p.undo());
        assert_eq!(p.redo_note(), 8);
        assert_eq!(p.undo_note(), 7);
    }

    #[test]
    fn colour_fill_selections_can_be_filled_again() {
        let mut p = Painter::new(128, 128);
        let mut mask = vec![0u8; 128 * 128];
        for y in 10..40 {
            for x in 10..40 {
                mask[y * 128 + x] = 255;
            }
        }
        assert!(!p.fill_selection([255, 0, 0], 1.0), "no selection, no fill");
        p.set_selection(&mask, 0);
        assert!(p.fill_selection([255, 0, 0], 1.0));
        assert!(p.selection_fill_live());
        p.flush();
        assert_eq!(px(&p, 20, 20), [255, 0, 0]);
        // another colour, and a second square added: one fill, one undo step
        assert!(p.refill_selection([0, 0, 255], 1.0));
        let mut more = vec![0u8; 128 * 128];
        for y in 60..90 {
            for x in 60..90 {
                more[y * 128 + x] = 255;
            }
        }
        p.set_selection(&more, 1);
        assert!(p.refill_selection([0, 0, 255], 1.0));
        p.flush();
        assert_eq!(px(&p, 20, 20), [0, 0, 255]);
        assert_eq!(px(&p, 70, 70), [0, 0, 255]);
        assert!(p.undo());
        p.flush();
        assert_eq!(px(&p, 20, 20), [255, 255, 255]);
        assert_eq!(px(&p, 70, 70), [255, 255, 255]);
        assert!(!p.can_undo());
        // after another edit, the fill stays as it is
        assert!(p.redo());
        assert!(!p.selection_fill_live());
        assert!(!p.refill_selection([0, 255, 0], 1.0));
    }

    #[test]
    fn boiled_drawings_wobble_a_little() {
        let mut p = Painter::new(256, 128);
        let layer = p.active_layer();
        line(&mut p, &pen(), 64.0);
        let d = p.layer_drawing(layer);
        let rgba = p.drawing_rgba(d);
        let ink = |v: &[u8]| v.as_chunks::<4>().0.iter().filter(|px| px[3] > 128).count();
        let b1 = p.boil_drawing(d, 1.5, 1);
        let b2 = p.boil_drawing(d, 1.5, 2);
        assert!(b1 != 0 && b2 != 0 && b1 != b2 && b1 != d);
        let (r1, r2) = (p.drawing_rgba(b1), p.drawing_rgba(b2));
        // about as much ink, in different places, never far from the line
        for r in [&r1, &r2] {
            let (a, b) = (ink(&rgba) as f32, ink(r) as f32);
            assert!((b - a).abs() / a < 0.25, "{a} vs {b}");
            assert_ne!(r, &rgba);
            for y in 0..128usize {
                for x in 0..256usize {
                    if r[(y * 256 + x) * 4 + 3] > 128 {
                        assert!((y as f32 - 64.0).abs() < 12.0, "ink at {x},{y}");
                    }
                }
            }
        }
        assert_ne!(r1, r2);
        // the same seed boils the same way; the original is untouched
        let again = p.boil_drawing(d, 1.5, 1);
        assert_eq!(p.drawing_rgba(again), r1);
        assert_eq!(p.drawing_rgba(d), rgba);
        // nothing to boil
        assert_eq!(p.boil_drawing(999, 1.0, 1), 0);
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

    #[test]
    fn fill_from_a_reference_layer() {
        let mut p = Painter::new(256, 128);
        dense_square(&mut p);
        let lines = p.active_layer();
        let colour = p.add_layer("Colour");
        // hide the lines: a normal fill sampling what you see can't find the square
        let mut hidden = props(&p, lines);
        hidden.visible = false;
        p.set_layer_props(lines, hidden, true);
        assert!(p.fill_from(80, 60, [0, 200, 0], 1.0, 0.1, true, 0, 0, Some(lines)));
        assert_eq!(p.active_layer(), colour, "painted on the active layer");
        let mut shown = props(&p, lines);
        shown.visible = true;
        p.set_layer_props(lines, shown, true);
        p.flush();
        assert!(
            px(&p, 80, 60)[1] > 150,
            "inside the reference's square is filled"
        );
        assert!(px(&p, 200, 60)[0] > 240, "outside it is not");
        // a reference that doesn't exist falls back to the usual sampling (what you see)
        assert!(p.fill_from(200, 60, [200, 0, 0], 1.0, 0.1, true, 0, 0, Some(999)));
        p.flush();
        assert!(
            px(&p, 200, 60)[1] < 60,
            "the outside is filled: {:?}",
            px(&p, 200, 60)
        );
        assert!(px(&p, 80, 60)[1] > 150, "the inside keeps its green");
    }

    #[test]
    fn fill_mask_paints_the_lasso_shape_with_soft_edges() {
        let mut p = Painter::new(256, 128);
        let mut cov = rect_mask(&p, 40, 30, 120, 90);
        cov[(60 * 256 + 120) as usize] = 128; // a half-covered edge pixel
        assert!(p.fill_mask(&cov, [0, 0, 220], 1.0));
        p.flush();
        assert!(
            px(&p, 80, 60)[2] > 200 && px(&p, 80, 60)[0] < 40,
            "inside is blue"
        );
        assert!(px(&p, 200, 60)[0] > 240, "outside untouched");
        let edge = px(&p, 120, 60);
        assert!(edge[0] > 60 && edge[0] < 200, "edge is half way: {edge:?}");
        assert!(p.undo());
        p.flush();
        assert!(px(&p, 80, 60)[0] > 240);
        // the selection limits it
        p.set_selection(&rect_mask(&p, 0, 0, 80, 128), 0);
        assert!(p.fill_mask(&rect_mask(&p, 40, 30, 120, 90), [0, 0, 220], 1.0));
        p.flush();
        assert!(px(&p, 60, 60)[0] < 40 && px(&p, 100, 60)[0] > 240);
        // nothing to paint: no undo step
        assert!(!p.fill_mask(&vec![0; 256 * 128], [0, 0, 220], 1.0));
    }

    #[test]
    fn projective_transform_distorts_and_undoes() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 30.0);
        p.flush();
        let before = p.rgba().to_vec();
        assert_eq!(p.transform_begin().len(), 4);
        // a matrix scaled by 2 is the same move: down 50
        assert!(p.transform_preview_h([2.0, 0.0, 0.0, 0.0, 2.0, 100.0, 0.0, 0.0, 2.0]));
        p.flush();
        assert!(px(&p, 100, 30)[0] > 240 && px(&p, 100, 80)[0] < 100);
        // real perspective: the right end shrinks towards the middle row
        assert!(p.transform_preview_h([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.004, 0.0, 1.0]));
        p.flush();
        // x' = x / (1 + 0.004 x): the line's right end (x = 180) lands near x = 105
        assert!(px(&p, 60, 22)[0] < 120, "left part stays");
        assert!(px(&p, 160, 18)[0] > 240, "right end pulled in");
        // a corner behind the camera is refused (and the last good preview stays)
        assert!(!p.transform_preview_h([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.02, 0.0, 1.0]));
        assert!(p.transform_commit());
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
    }

    #[test]
    fn warp_mesh_bends_and_the_selection_follows() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0);
        p.flush();
        let before = p.rgba().to_vec();
        p.set_selection(&rect_mask(&p, 0, 0, 256, 128), 0);
        let bbox = p.transform_begin();
        let (x0, y0, x1, y1) = (bbox[0], bbox[1], bbox[2], bbox[3]);
        // a 2×1 mesh: lift the middle column of points by 30 px (a tent)
        let grid = |lift: f32| {
            let mut n = Vec::new();
            for r in 0..2 {
                for c in 0..3 {
                    let x = x0 + (x1 - x0) * c as f32 / 2.0;
                    let y = if r == 0 { y0 } else { y1 };
                    n.extend([x, y - if c == 1 { lift } else { 0.0 }]);
                }
            }
            n
        };
        // the identity mesh changes nothing
        assert!(p.transform_preview_mesh(&grid(0.0), 2, 1));
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        assert!(p.transform_preview_mesh(&grid(30.0), 2, 1));
        p.flush();
        let mid = ((x0 + x1) / 2.0) as u32;
        assert!(
            px(&p, mid, 30)[0] < 120,
            "the middle rose: {:?}",
            px(&p, mid, 30)
        );
        assert!(px(&p, mid, 60)[0] > 240, "and left its old place");
        assert!(px(&p, 24, 60)[0] < 120, "the ends stay put");
        // a bad mesh is refused
        assert!(!p.transform_preview_mesh(&[1.0, 2.0], 2, 1));
        assert!(p.transform_commit());
        let sel = p.selection_mask();
        assert!(
            sel[30 * 256 + mid as usize] > 0,
            "the selection followed the paint"
        );
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
    }

    #[test]
    fn feathered_selection_paints_partly_at_its_edge() {
        let mut p = Painter::new(256, 128);
        assert!(!p.feather_selection(10.0), "nothing to feather");
        p.set_selection(&rect_mask(&p, 0, 0, 128, 128), 0);
        assert!(p.feather_selection(12.0));
        let sel = p.selection_mask();
        let at = |x: usize| sel[64 * 256 + x];
        assert_eq!(at(40), 255, "deep inside: fully selected");
        assert_eq!(at(220), 0, "far outside: not selected");
        assert!(
            at(128) > 60 && at(128) < 200,
            "the edge is soft: {}",
            at(128)
        );
        // a lasso fill across the edge fades out over it
        assert!(p.fill_mask(&vec![255; 256 * 128], [0, 0, 0], 1.0));
        p.flush();
        let (inside, edge, outside) = (px(&p, 40, 64)[0], px(&p, 128, 64)[0], px(&p, 220, 64)[0]);
        assert!(inside < 20 && outside > 240, "{inside} {outside}");
        assert!(edge > 40 && edge < 220, "half painted at the edge: {edge}");
        // strokes too
        p.undo();
        let b = pen();
        line(&mut p, &b, 64.0);
        p.flush();
        assert!(px(&p, 40, 64)[0] < 60);
        assert!(px(&p, 160, 64)[0] > 240);
    }

    #[test]
    fn h_invert_round_trips() {
        let h = [1.2, 0.1, 5.0, -0.2, 0.9, 3.0, 0.001, 0.002, 1.0];
        let inv = h_invert(h).unwrap();
        let (x, y) = h_apply(h, 40.0, 70.0).unwrap();
        let (bx, by) = h_apply(inv, x, y).unwrap();
        assert!((bx - 40.0).abs() < 1e-2 && (by - 70.0).abs() < 1e-2);
        assert!(h_invert([0.0; 9]).is_none());
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
    fn symmetry_mirrors_strokes_in_one_undo_step() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Vertical,
            cx: 128.0,
            cy: 64.0,
        }));
        p.begin_stroke();
        for i in 0..=20 {
            p.stroke_to(&b, 20.0 + i as f32 * 2.0, 30.0, 1.0, 0.0, 0.0, 0.01); // x 20..60
        }
        p.end_stroke(&b);
        p.flush();
        assert!(px(&p, 40, 30)[0] < 100, "original");
        assert!(px(&p, 216, 30)[0] < 100, "mirror at 256 - 40");
        assert!(px(&p, 128, 30)[0] > 240);
        assert!(p.undo());
        p.flush();
        assert_eq!(dark_pixels(&p), 0, "one step removes both");
        // radial: 4 copies of a short line right of centre land around it
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Radial(4),
            cx: 128.0,
            cy: 64.0,
        }));
        p.begin_stroke();
        for i in 0..=8 {
            p.stroke_to(&b, 160.0 + i as f32 * 2.0, 64.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&b);
        p.flush();
        for (x, y) in [(168, 64), (128, 104), (88, 64), (128, 24)] {
            assert!(
                px(&p, x, y)[0] < 160,
                "copy at ({x},{y}): {:?}",
                px(&p, x, y)
            );
        }
        p.set_symmetry(None);
        assert_eq!(
            Symmetry {
                mode: SymmetryMode::Quadrant,
                cx: 10.0,
                cy: 10.0
            }
            .copies(0.0, 0.0),
            vec![(20.0, 0.0), (0.0, 20.0), (20.0, 20.0)]
        );
    }

    #[test]
    fn clipping_masks_show_only_over_the_layer_below() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 60.0); // base: a line at y 60
        let top = p.add_layer("Colour");
        let mut red = pen();
        set_brush_rgb(&mut red, 220, 0, 0);
        red.get_mut(BrushSetting::Radius).base_value = 4.0;
        line(&mut p, &red, 60.0); // a fat red line over it
        p.flush();
        assert!(
            px(&p, 100, 68)[0] > 200 && px(&p, 100, 68)[1] < 60,
            "red spills past the base"
        );
        let mut c = props(&p, top);
        c.clip = true;
        p.set_layer_props(top, c.clone(), true);
        p.flush();
        assert!(
            px(&p, 100, 68)[1] > 240,
            "clipped: nothing outside the base line"
        );
        let on = px(&p, 100, 60);
        assert!(on[0] > on[1] + 60, "red on the base line: {on:?}");
        // hiding the base hides what's clipped to it
        let base = p.layers()[0].id;
        let mut hb = props(&p, base);
        hb.visible = false;
        p.set_layer_props(base, hb.clone(), true);
        p.flush();
        assert_eq!(dark_pixels(&p), 0);
        assert!(px(&p, 100, 60)[1] > 240);
        hb.visible = true;
        p.set_layer_props(base, hb, true);
        p.flush();
        let image = p.rgba().to_vec();
        // merging keeps it clipped
        assert!(p.merge_down(top));
        p.flush();
        let diff = p
            .rgba()
            .iter()
            .zip(&image)
            .filter(|(a, b)| a.abs_diff(**b) > 3)
            .count();
        assert!(diff < 40, "merged picture differs in {diff} channels");
        assert!(p.layers_json().contains(r#""clip":false"#));
    }

    #[test]
    fn layer_masks_hide_and_show_and_undo() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        let mut fat = pen();
        fat.get_mut(BrushSetting::Radius).base_value = 1.6; // logarithmic: ~5 px
        line(&mut p, &fat, 60.0); // a fat line across the middle
        p.flush();
        let id = p.active_layer();
        let full = dark_pixels(&p);
        assert!(p.add_mask(id));
        assert!(p.layers_json().contains(r#""mask":true"#));
        p.flush();
        assert_eq!(dark_pixels(&p), full, "an empty mask shows everything");
        // paint black on the mask over the left half: hidden there
        assert!(p.set_mask_editing(id, true));
        assert!(p.is_editing_mask());
        let mut wide = pen();
        wide.get_mut(BrushSetting::Radius).base_value = 3.0; // ~20 px
        p.begin_stroke();
        for i in 0..=20 {
            p.stroke_to(&wide, 10.0 + i as f32 * 5.0, 60.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&wide);
        p.flush();
        assert!(px(&p, 60, 60)[0] > 240, "hidden under black mask paint");
        assert!(px(&p, 160, 60)[0] < 120, "still shown on the right");
        // the colour wasn't touched
        assert_eq!(p.layer_thumbnail(id, 16, 8), {
            let mut q = Painter::new(256, 128);
            line(&mut q, &fat, 60.0);
            q.layer_thumbnail(q.active_layer(), 16, 8)
        });
        // undo the mask stroke after leaving mask editing still hits the mask
        assert!(p.set_mask_editing(id, false));
        assert!(p.undo());
        p.flush();
        assert_eq!(dark_pixels(&p), full);
        assert!(p.redo());
        p.flush();
        assert!(px(&p, 60, 60)[0] > 240);
        let masked = p.rgba().to_vec();
        // apply: same picture, no mask; undo brings the mask back
        assert!(p.apply_mask(id));
        p.flush();
        assert_eq!(p.rgba(), &masked[..]);
        assert!(p.layers_json().contains(r#""mask":false"#));
        assert!(p.undo());
        assert!(p.layers_json().contains(r#""mask":true"#));
        // merge down bakes the mask in
        let top = p.add_layer("top");
        line(&mut p, &b, 20.0);
        p.flush();
        let before = p.rgba().to_vec();
        assert!(p.merge_down(top));
        p.flush();
        let diff = p
            .rgba()
            .iter()
            .zip(&before)
            .filter(|(a, b)| a.abs_diff(**b) > 3)
            .count();
        assert!(diff < 40, "merge changed {diff} channels");
        assert!(p.layers_json().contains(r#""mask":false"#));
        // delete: everything shows again
        assert!(p.undo());
        assert!(p.delete_mask(id));
        p.flush();
        assert!(px(&p, 60, 60)[0] < 120);
    }

    #[test]
    fn layer_groups_hide_fade_move_merge_and_undo() {
        let mut p = Painter::new(256, 128);
        let b = pen();
        line(&mut p, &b, 20.0); // layer 1 (bottom)
        let l2 = p.add_layer("two");
        line(&mut p, &b, 60.0);
        let l3 = p.add_layer("three");
        line(&mut p, &b, 100.0);
        p.flush();
        let gid = p.add_group(l2);
        assert!(gid > 0);
        assert!(p.layers_json().contains(&format!(r#""group":{gid}"#)));
        // a layer added above a grouped layer joins it
        p.set_active_layer(l2);
        let l4 = p.add_layer("four");
        assert_eq!(p.members(gid).len(), 2);
        // hiding the group hides its layers; painting in it is refused
        assert!(p.set_group(gid, "Head", false, 1.0, true));
        p.flush();
        assert!(px(&p, 100, 60)[0] > 240);
        assert!(px(&p, 100, 100)[0] < 120);
        p.set_active_layer(l4);
        assert!(!p.begin_stroke());
        assert!(p.undo());
        p.flush();
        assert!(px(&p, 100, 60)[0] < 120);
        // group opacity fades its layers
        let solid = px(&p, 100, 60)[0];
        p.set_group(gid, "Head", true, 0.3, true);
        p.flush();
        assert!(px(&p, 100, 60)[0] > solid + 60);
        // move the group up past layer three: order is 1, three, two, four
        assert!(p.move_group(gid, true));
        let order: Vec<u32> = p.layers().iter().map(|l| l.id).collect();
        assert_eq!(order, vec![1, l3, l2, l4]);
        assert!(p.move_group(gid, false));
        assert!(!p.move_group(gid, false) || p.layers()[0].group == gid);
        p.undo();
        // move a layer out of the group with move_layer_to
        assert!(p.move_layer_to(l4, 3, 0));
        assert_eq!(p.members(gid).len(), 1);
        p.undo();
        assert_eq!(p.members(gid).len(), 2);
        // merge: one layer named after the group, same picture
        p.flush();
        let before = p.rgba().to_vec();
        let merged = p.merge_group(gid);
        assert!(merged > 0);
        p.flush();
        let diff = p
            .rgba()
            .iter()
            .zip(&before)
            .filter(|(a, b)| a.abs_diff(**b) > 4)
            .count();
        assert!(diff < 60, "merge changed {diff} channels");
        assert!(p.groups().is_empty());
        assert!(p.layers_json().contains(r#""name":"Head""#));
        assert_eq!(p.layers().len(), 3);
        assert!(p.undo());
        assert_eq!(p.layers().len(), 4);
        assert_eq!(p.groups().len(), 1);
        // ungroup keeps the layers; delete removes them
        assert!(p.ungroup(gid));
        assert!(p.groups().is_empty() && p.layers().len() == 4);
        p.undo();
        assert!(p.delete_group(gid));
        assert_eq!(p.layers().len(), 2);
        p.undo();
        assert_eq!(p.layers().len(), 4);
    }

    #[test]
    fn adjustments_preview_commit_cancel_and_undo() {
        let mut p = Painter::new(256, 128);
        let mut fat = pen();
        fat.get_mut(BrushSetting::Radius).base_value = 2.5;
        set_brush_rgb(&mut fat, 200, 30, 30);
        line(&mut p, &fat, 60.0);
        p.flush();
        let before = p.rgba().to_vec();
        assert!(p.adjust_begin());
        assert!(!p.begin_stroke(), "no painting while adjusting");
        p.adjust_preview(adjust::ColourAdjust {
            hue: 120.0,
            ..Default::default()
        });
        p.flush();
        let c = px(&p, 100, 60);
        assert!(c[1] > c[0] + 60, "red turned green: {c:?}");
        p.adjust_cancel();
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        // commit is one undo step
        assert!(p.adjust_begin());
        p.adjust_preview(adjust::ColourAdjust {
            saturation: -1.0,
            ..Default::default()
        });
        assert!(p.adjust_commit());
        p.flush();
        let c = px(&p, 100, 60);
        assert!(c[0].abs_diff(c[1]) < 8, "grey: {c:?}");
        assert!(p.undo());
        p.flush();
        assert_eq!(p.rgba(), &before[..]);
        // gradient
        assert!(p.adjust_begin());
        p.gradient_preview(adjust::Gradient {
            kind: adjust::GradientKind::Linear,
            from: (0.0, 0.0),
            to: (256.0, 0.0),
            c0: [0.0, 0.0, 1.0],
            a0: 1.0,
            c1: [0.0, 0.0, 1.0],
            a1: 0.0,
        });
        assert!(p.adjust_commit());
        p.flush();
        assert!(px(&p, 4, 10)[2] > 200 && px(&p, 4, 10)[0] < 40);
        assert!(px(&p, 250, 10)[0] > 240);
        assert!(!p.adjust_commit());
    }

    #[test]
    fn imported_pictures_are_marked_and_stay_marked() {
        let mut p = Painter::new(100, 70);
        let b = pen();
        line(&mut p, &b, 20.0);
        let mut rgba = vec![0u8; 100 * 70 * 4];
        for y in 30..60 {
            for x in 10..90 {
                let i = (y * 100 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        assert_eq!(p.add_image_layer("photo", &rgba[..10]), 0, "wrong size");
        let id = p.add_image_layer("photo", &rgba);
        assert!(id > 0);
        p.flush();
        assert_eq!(px(&p, 50, 45), [0, 0, 255]);
        assert!(p.layers_json().contains(r#""name":"photo""#));
        assert!(p.layers_json().contains(r#""imported":true"#));
        // settings can't clear the mark
        let mut pr = props(&p, id);
        pr.imported = false;
        pr.opacity = 0.5;
        p.set_layer_props(id, pr, true);
        assert!(p.layers_json().contains(r#""imported":true"#));
        // merging it down passes the mark on; undo takes it back
        assert!(p.merge_down(id));
        assert!(p.layers_json().contains(r#""imported":true"#));
        assert!(p.undo());
        assert!(p.undo());
        assert!(p.undo());
        assert!(!p.layers_json().contains(r#""imported":true"#));
        p.flush();
        assert_eq!(px(&p, 50, 45), [255, 255, 255]);
    }

    #[test]
    fn layer_rgba_is_full_size_and_straight() {
        let mut p = Painter::new(100, 70);
        let mut rgba = vec![0u8; 100 * 70 * 4];
        let i = (35 * 100 + 50) * 4;
        rgba[i..i + 4].copy_from_slice(&[200, 100, 50, 128]);
        let id = p.add_image_layer("x", &rgba);
        let out = p.layer_rgba(id);
        assert_eq!(out.len(), rgba.len());
        for k in 0..4 {
            assert!(
                out[i + k].abs_diff(rgba[i + k]) <= 2,
                "{:?}",
                &out[i..i + 4]
            );
        }
        assert_eq!(out[0..4], [0, 0, 0, 0]);
        assert!(p.layer_rgba(999).is_empty());
    }

    #[test]
    fn restored_layers_and_masks_come_back_as_saved() {
        let mut p = Painter::new(100, 70);
        let mut rgba = vec![0u8; 100 * 70 * 4];
        let i = (35 * 100 + 50) * 4;
        rgba[i..i + 4].copy_from_slice(&[200, 100, 50, 255]);
        let id = p.restore_layer("Ink", &rgba, false);
        assert!(id > 0);
        assert!(p.layers_json().contains(r#""name":"Ink""#));
        assert!(!p.layers_json().contains(r#""imported":true"#));
        let back = p.layer_rgba(id);
        assert!(back[i].abs_diff(200) <= 2 && back[i + 3] == 255);
        // a mask: left half hidden (0), right half shown (255), a grey in the middle
        let mut gray = vec![255u8; 100 * 70];
        for y in 0..70 {
            for x in 0..50 {
                gray[y * 100 + x] = 0;
            }
        }
        gray[35 * 100 + 50] = 128;
        assert!(p.restore_mask(id, &gray));
        assert!(p.layers_json().contains(r#""mask":true"#));
        let m = p.mask_thumbnail(id, 100, 70);
        assert!(m[(10 * 100 + 10) * 4] < 5, "hidden");
        assert!(m[(10 * 100 + 90) * 4] > 250, "shown");
        assert!(m[(35 * 100 + 50) * 4].abs_diff(128) <= 3, "grey");
        assert!(!p.restore_mask(id, &gray[..10]));
    }

    #[test]
    fn hsv_conversion() {
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        assert_eq!((h, s, v), (0.0, 1.0, 1.0));
        let (h, _, _) = rgb_to_hsv(0.0, 0.0, 1.0);
        assert!((h - 2.0 / 3.0).abs() < 1e-6);
    }
}
