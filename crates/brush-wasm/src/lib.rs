//! PencilPilot painting engine.
//!
//! Wraps [hokusai](https://github.com/reearth/hokusai), a pure-Rust port of
//! libmypaint (the brush engine Krita also ships), and adds what a drawing
//! app needs on top:
//!
//! - **Undo / redo** that stores only the tiles a stroke touched.
//! - **Eraser mode** for any brush.
//! - **Dirty-rect flattening**: only changed tiles are converted to RGBA8, and
//!   the caller gets the rectangle to repaint.
//!
//! The engine logic lives in [`Painter`] (plain Rust, unit-tested natively);
//! [`wasm`] exposes it to JavaScript.

use std::collections::{HashMap, HashSet};

use hokusai::color::linear_to_srgb;
use hokusai::tile::empty_tile;
use hokusai::{Brush, BrushSetting, BrushState, TILE_SIZE, TilePixels, TiledSurface, fix15};

pub mod wasm;

type TileKey = (i32, i32);
type TileMap = HashMap<TileKey, Box<TilePixels>>;
/// Tiles as they were before an edit. `None` = the tile did not exist.
type Snapshot = HashMap<TileKey, Option<Box<TilePixels>>>;

/// Maximum number of undo steps kept.
pub const UNDO_LIMIT: usize = 60;

/// Tile surface that can snapshot the tiles a stroke touches.
#[derive(Default)]
struct UndoSurface {
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

/// The canvas: a tiled surface, the in-flight stroke state, history, and an
/// RGBA8 (sRGB, composited over white) output buffer.
pub struct Painter {
    surface: UndoSurface,
    state: BrushState,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    stroking: bool,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            surface: UndoSurface::default(),
            state: BrushState::default(),
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
            undo: Vec::new(),
            redo: Vec::new(),
            stroking: false,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Start a stroke. The next [`Painter::stroke_to`] only positions the pen.
    pub fn begin_stroke(&mut self) {
        self.state.reset();
        self.surface.recording = Some(Snapshot::new());
        self.stroking = true;
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
        if !self.stroking {
            self.begin_stroke();
        }
        brush.stroke_to(
            &mut self.state,
            &mut self.surface,
            x,
            y,
            pressure,
            xtilt,
            ytilt,
            dtime,
        );
    }

    /// Finish the stroke (flushes slow-tracking lag) and record it for undo.
    pub fn end_stroke(&mut self, brush: &Brush) {
        if !self.stroking {
            return;
        }
        brush.finish_stroke(&mut self.state, &mut self.surface);
        self.state.reset();
        self.stroking = false;
        // Always record a history step, even if no pixels changed, so the
        // web app's stroke log and this undo stack stay one-to-one.
        if let Some(snap) = self.surface.recording.take() {
            self.push_undo(snap);
        }
    }

    fn push_undo(&mut self, snap: Snapshot) {
        self.undo.push(snap);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Erase everything (undoable).
    pub fn clear(&mut self) {
        if self.surface.tiles.is_empty() {
            return;
        }
        let snap: Snapshot = self
            .surface
            .tiles
            .drain()
            .map(|(k, t)| (k, Some(t)))
            .collect();
        self.surface.dirty.extend(snap.keys().copied());
        self.push_undo(snap);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.undo.pop() else {
            return false;
        };
        let replaced = self.surface.swap_in(snap);
        self.redo.push(replaced);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.redo.pop() else {
            return false;
        };
        let replaced = self.surface.swap_in(snap);
        self.undo.push(replaced);
        true
    }

    /// Whether anything is painted on the canvas.
    pub fn is_blank(&self) -> bool {
        self.surface.tiles.is_empty()
    }

    /// Convert changed tiles into the RGBA buffer. Returns the area that
    /// changed, or `None` if nothing did.
    pub fn flush(&mut self) -> Option<Rect> {
        if self.surface.dirty.is_empty() {
            return None;
        }
        let ts = TILE_SIZE as i32;
        let (w, h) = (self.width as i32, self.height as i32);
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let dirty: Vec<TileKey> = self.surface.dirty.drain().collect();
        for (tx, ty) in dirty {
            let (px0, py0) = (tx * ts, ty * ts);
            let (px1, py1) = ((px0 + ts).min(w), (py0 + ts).min(h));
            let (cx0, cy0) = (px0.max(0), py0.max(0));
            if cx0 >= px1 || cy0 >= py1 {
                continue; // tile entirely off-canvas
            }
            let tile = self.surface.tiles.get(&(tx, ty));
            for y in cy0..py1 {
                for x in cx0..px1 {
                    let idx = ((y * w + x) * 4) as usize;
                    let px = match tile {
                        Some(t) => composite(t[(y - py0) as usize][(x - px0) as usize]),
                        None => [255, 255, 255],
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
}

/// Premultiplied linear fix15 RGBA → sRGB8, composited over white.
fn composite(p: [u16; 4]) -> [u8; 3] {
    let a = fix15::to_f32(p[3]);
    if a <= 0.0 {
        return [255, 255, 255];
    }
    let ch = |c: u16| {
        // premultiplied: c_out = c + white * (1 - a)
        let lin = (fix15::to_f32(c) + (1.0 - a)).clamp(0.0, 1.0);
        (linear_to_srgb(lin).clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [ch(p[0]), ch(p[1]), ch(p[2])]
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

    #[test]
    fn hsv_conversion() {
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        assert_eq!((h, s, v), (0.0, 1.0, 1.0));
        let (h, _, _) = rgb_to_hsv(0.0, 0.0, 1.0);
        assert!((h - 2.0 / 3.0).abs() < 1e-6);
    }
}
