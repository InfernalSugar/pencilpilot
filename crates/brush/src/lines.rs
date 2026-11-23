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

use std::collections::HashSet;

use hokusai::{Brush, BrushState, TILE_SIZE, TilePixels};

use crate::{Edit, Painter, Snapshot, TileKey, UndoSurface};

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
}

/// A line edit not in the history yet: its layer, the drawing it's on (0: none yet)
/// and the tiles it replaced, as they were first.
pub(crate) struct Pending {
    layer: u32,
    drawing: u32,
    before: Snapshot,
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
        self.line_redraw = Some(Box::new(Redraw {
            layer,
            scratch: UndoSurface {
                style: self.style.clone(),
                only: Some(Box::new(Area::new(keys))),
                ..Default::default()
            },
        }));
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
        let Redraw { layer, mut scratch } = *r;
        let Some(area) = scratch.only.take() else {
            return false;
        };
        let pending = self.line_pending.take().filter(|p| p.layer == layer);
        let Some(l) = self.layer_mut(layer) else {
            return false;
        };
        let mut pending = pending.unwrap_or(Pending {
            layer,
            drawing: l.drawing,
            before: Snapshot::new(),
        });
        for key in area.tiles {
            let new = scratch.tiles.remove(&key).filter(|t| !transparent(t));
            let old = match new {
                Some(t) => l.surface.tiles.insert(key, t),
                None => l.surface.tiles.remove(&key),
            };
            pending.before.entry(key).or_insert(old);
            l.surface.dirty.insert(key);
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
    }

    /// Line edits are waiting for `lines_push`.
    pub fn lines_pending(&self) -> bool {
        self.line_pending.is_some()
    }
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
