//! The Clone tool (Photoshop's Clone Stamp) and its Heal option (a simple
//! Healing Brush). A clone stroke is an ordinary stroke of the current brush
//! (its size, softness and pressure), but each dab copies the picture from a
//! fixed offset away instead of laying down colour, so it goes into the undo
//! history, clips to the selection and respects locks like any other stroke.
//!
//! What's copied is the picture as it was when the stroke began: the layer
//! itself, or what you see (all layers). Heal shifts the copied colours by the
//! difference between the average colour around the source and around the
//! destination (a ring just outside the dab), so the copied texture takes on
//! the brightness and colour of where it lands. A light stand-in for Photoshop's
//! Poisson blend: it matches the surroundings' mean, not their gradient.
//!
//! The brush's texture (grain, tip picture, dual brush: see [`crate::texture`]) thins
//! the copy just as it thins paint, so a grainy pencil clones grainy.

use std::sync::Arc;

use hokusai::brushmodes::draw_dab_default;
use hokusai::color::RgbaF32;
use hokusai::tile::empty_tile;
use hokusai::{Dab, TILE_SIZE, TilePixels, TiledSurface};

use crate::layers::{Blend, merge_pixel, premul_fix15, unpremul_srgb};
use crate::{Painter, TileKey, TileMap, UndoSurface};

/// Where a clone stroke copies from.
pub(crate) struct CloneSource {
    /// the source is this far from the brush (canvas px)
    dx: i64,
    dy: i64,
    heal: bool,
    width: i64,
    height: i64,
    /// what you see (straight sRGB, canvas-sized) when sampling all layers;
    /// `None`: the layer itself, as it was when the stroke began
    seen: Option<Vec<[u8; 4]>>,
}

impl CloneSource {
    /// The source picture at canvas pixel (x, y), premultiplied (transparent off the canvas).
    fn at(&self, s: &UndoSurface, x: i64, y: i64) -> [u16; 4] {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return [0; 4];
        }
        if let Some(seen) = &self.seen {
            let p = seen[(y * self.width + x) as usize];
            let c = [p[0], p[1], p[2]].map(|v| v as f32 / 255.0);
            return premul_fix15(c, p[3] as f32 / 255.0);
        }
        let ts = TILE_SIZE as i64;
        let key: TileKey = (x.div_euclid(ts) as i32, y.div_euclid(ts) as i32);
        let (px, py) = (x.rem_euclid(ts) as usize, y.rem_euclid(ts) as usize);
        // the stroke keeps each tile as it was before it painted there
        let before = s.recording.as_ref().and_then(|r| r.get(&key));
        let tile = match before {
            Some(t) => t.as_deref(),
            None => s.tiles.get(&key).map(|t| &**t),
        };
        tile.map_or([0; 4], |t| t[py][px])
    }

    /// Mean straight colour (sRGB 0–1) of the ring just outside a dab of
    /// radius `r` at (cx, cy), seen from (ox, oy) away; None where it's empty.
    fn ring_mean(
        &self,
        s: &UndoSurface,
        cx: f32,
        cy: f32,
        r: f32,
        ox: i64,
        oy: i64,
    ) -> Option<[f32; 3]> {
        let (r0, r1) = (r, r * 1.6 + 2.0);
        // big dabs: every few pixels is plenty for an average
        let step = ((r / 12.0) as usize).max(1);
        let (mut sum, mut wsum) = ([0f32; 3], 0f32);
        let (y0, y1) = ((cy - r1).floor() as i64, (cy + r1).ceil() as i64);
        let (x0, x1) = ((cx - r1).floor() as i64, (cx + r1).ceil() as i64);
        for y in (y0..=y1).step_by(step) {
            for x in (x0..=x1).step_by(step) {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if d < r0 || d > r1 {
                    continue;
                }
                let (c, a) = unpremul_srgb(self.at(s, x + ox, y + oy));
                for k in 0..3 {
                    sum[k] += c[k] * a;
                }
                wsum += a;
            }
        }
        (wsum > 0.5).then(|| sum.map(|v| v / wsum))
    }
}

/// A blank surface the dab is drawn on in solid white, to read its coverage.
#[derive(Default)]
struct Coverage {
    tiles: TileMap,
}

impl TiledSurface for Coverage {
    fn tile_request_start(&mut self, tx: i32, ty: i32) -> &mut TilePixels {
        self.tiles.entry((tx, ty)).or_insert_with(empty_tile)
    }
    fn tile_request_end(&mut self, _tx: i32, _ty: i32) {}
    fn tile_lookup(&self, tx: i32, ty: i32) -> Option<&TilePixels> {
        self.tiles.get(&(tx, ty)).map(|b| &**b)
    }
}

/// One dab of a clone stroke: the source laid over the layer, as much as the
/// dab covers each pixel (its shape, softness and opacity).
pub(crate) fn clone_dab(s: &mut UndoSurface, dab: &Dab, src: &CloneSource) -> bool {
    let mut cover = Coverage::default();
    let mut solid = *dab;
    solid.color = RgbaF32::new(1.0, 1.0, 1.0, 1.0);
    solid.alpha_eraser = 1.0;
    solid.lock_alpha = 0.0;
    solid.colorize = 0.0;
    solid.posterize = 0.0;
    solid.paint = 0.0;
    if !draw_dab_default(&mut cover, &solid) {
        return false;
    }
    // heal: how far the colours around the source are from those around here
    let shift = if src.heal {
        let here = src.ring_mean(s, dab.x, dab.y, dab.radius, 0, 0);
        let there = src.ring_mean(s, dab.x, dab.y, dab.radius, src.dx, src.dy);
        here.zip(there)
            .map(|(a, b)| [a[0] - b[0], a[1] - b[1], a[2] - b[2]])
    } else {
        None
    };
    let lock_alpha = dab.lock_alpha > 0.5;
    // the brush's grain / tip / dual brush thin the copy as they thin paint
    let style = s.style.clone();
    let stamps = style.as_ref().map(|st| st.stamps(dab)).unwrap_or_default();
    let ts = TILE_SIZE as i64;
    let mut changed = false;
    for (&(tx, ty), tile) in &cover.tiles {
        // what to lay down here first (reading the source), then lay it down
        let mut put = Vec::new();
        for (py, row) in tile.iter().enumerate() {
            for (px, p) in row.iter().enumerate() {
                if p[3] == 0 {
                    continue;
                }
                let (x, y) = (tx as i64 * ts + px as i64, ty as i64 * ts + py as i64);
                let mut c = src.at(s, x + src.dx, y + src.dy);
                if c[3] == 0 {
                    continue;
                }
                if let Some(k) = shift {
                    let (col, a) = unpremul_srgb(c);
                    c = premul_fix15([col[0] + k[0], col[1] + k[1], col[2] + k[2]], a);
                }
                let mut a = p[3] as f32 / 32768.0;
                if let Some(st) = &style {
                    a *= st.factor(dab, &stamps, x as f32 + 0.5, y as f32 + 0.5);
                    if a <= 0.0 {
                        continue;
                    }
                }
                put.push((py, px, c, a));
            }
        }
        if put.is_empty() {
            continue;
        }
        let t = s.tile_request_start(tx, ty);
        for (py, px, c, a) in put {
            let d = t[py][px];
            let mut out = merge_pixel(d, c, a, Blend::Normal);
            if lock_alpha {
                // keep the layer's own transparency: only the colour changes
                out = premul_fix15(unpremul_srgb(out).0, unpremul_srgb(d).1);
            }
            if out != d {
                t[py][px] = out;
                changed = true;
            }
        }
        s.tile_request_end(tx, ty);
    }
    changed
}

impl Painter {
    /// Start a clone stroke on the active layer: it copies from (`dx`, `dy`)
    /// away (canvas px, rounded), from the layer itself or what you see
    /// (`all_layers`), healed into its surroundings with `heal`. Then
    /// `stroke_to` / `end_stroke` as usual. False (and nothing starts) if the
    /// layer can't be painted on.
    pub fn begin_clone_stroke(&mut self, dx: f32, dy: f32, all_layers: bool, heal: bool) -> bool {
        if self.stroking.is_some() {
            return false;
        }
        let seen = match self.index_of(self.active) {
            Some(li) if all_layers => Some(self.sample_source(li, true)),
            _ => None,
        };
        if !self.begin_stroke() {
            return false;
        }
        let src = CloneSource {
            dx: dx.round() as i64,
            dy: dy.round() as i64,
            heal,
            width: self.width as i64,
            height: self.height as i64,
            seen,
        };
        let id = self.stroking.unwrap_or(0);
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
            l.surface.clone = Some(Arc::new(src));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use hokusai::{Brush, BrushSetting};

    use super::*;
    use crate::set_brush_rgb;

    /// a fairly hard round brush, radius ≈ 7 px
    fn stamp() -> Brush {
        let mut b = Brush::new();
        b.get_mut(BrushSetting::Radius).base_value = 2.0;
        b.get_mut(BrushSetting::Opaque).base_value = 1.0;
        b.get_mut(BrushSetting::Hardness).base_value = 0.9;
        b.get_mut(BrushSetting::DabsPerActualRadius).base_value = 4.0;
        set_brush_rgb(&mut b, 255, 0, 0);
        b
    }

    /// a canvas with a flat colour on its left and right halves, and a dark dot at (20, 30)
    fn halves(left: [u8; 4], right: [u8; 4]) -> Painter {
        let (w, h) = (120, 60);
        let mut p = Painter::new(w, h);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let c = if x < 60 { left } else { right };
                rgba[(y * w as usize + x) * 4..][..4].copy_from_slice(&c);
            }
        }
        for y in 28..33 {
            for x in 18..23 {
                rgba[(y * w as usize + x) * 4..][..4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        p.add_image_layer("pic", &rgba);
        p
    }

    fn at(p: &mut Painter, x: usize, y: usize) -> [u8; 4] {
        p.flush();
        let i = (y * p.width as usize + x) * 4;
        p.rgba()[i..i + 4].try_into().unwrap()
    }

    fn dab_at(p: &mut Painter, b: &Brush, x: f32, y: f32) {
        for i in 0..6 {
            p.stroke_to(b, x - 1.0 + i as f32 * 0.4, y, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(b);
    }

    #[test]
    fn a_clone_stroke_copies_a_whole_line() {
        let mut p = Painter::new(200, 100);
        let b = stamp();
        p.begin_stroke();
        for i in 0..=40 {
            p.stroke_to(&b, 20.0 + i as f32 * 4.0, 20.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&b);
        assert!(p.begin_clone_stroke(0.0, -50.0, false, false));
        for i in 0..=40 {
            p.stroke_to(&b, 20.0 + i as f32 * 4.0, 70.0, 0.5, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&b);
        for x in [30, 100, 170] {
            let c = at(&mut p, x, 70);
            assert!(c[1] < 60, "copied at {x}: {c:?}");
        }
    }

    #[test]
    fn clone_copies_the_source_and_undoes() {
        let mut p = halves([200, 200, 200, 255], [200, 200, 200, 255]);
        let b = stamp();
        let undo = p.undo.len();
        // the dot, copied 40 px to the right
        assert!(p.begin_clone_stroke(-40.0, 0.0, false, false));
        dab_at(&mut p, &b, 60.0, 30.0);
        assert_eq!(p.undo.len(), undo + 1, "one undo step");
        assert!(
            at(&mut p, 60, 30)[0] < 30,
            "the dot is copied, not the brush's red"
        );
        assert!(at(&mut p, 75, 30)[0] > 190, "nothing outside the dab");
        assert!(at(&mut p, 20, 30)[0] < 30, "the dot is still there");
        p.undo();
        assert!(at(&mut p, 60, 30)[0] > 190, "undone");
        // after the stroke, strokes are ordinary brush strokes again
        p.begin_stroke();
        dab_at(&mut p, &b, 100.0, 50.0);
        let c = at(&mut p, 100, 50);
        assert!(c[0] > 200 && c[1] < 50, "{c:?}");
    }

    #[test]
    fn clone_reads_all_layers_and_refuses_locked_layers() {
        let mut p = halves([200, 200, 200, 255], [200, 200, 200, 255]);
        let b = stamp();
        let top = p.add_layer("empty");
        // the layer itself is empty: nothing to copy
        assert!(p.begin_clone_stroke(-40.0, 0.0, false, false));
        dab_at(&mut p, &b, 60.0, 30.0);
        assert!(p.layer_rgba(top).iter().all(|&v| v == 0));
        // what you see: the dot lands on the empty layer
        assert!(p.begin_clone_stroke(-40.0, 0.0, true, false));
        dab_at(&mut p, &b, 60.0, 30.0);
        let i = (30 * 120 + 60) * 4;
        assert!(p.layer_rgba(top)[i + 3] > 200 && p.layer_rgba(top)[i] < 30);
        // a locked layer can't be cloned on
        let mut props = p.layers.iter().find(|l| l.id == top).unwrap().props.clone();
        props.locked = true;
        p.set_layer_props(top, props, false);
        assert!(!p.begin_clone_stroke(-40.0, 0.0, true, false));
    }

    #[test]
    fn a_grainy_brush_clones_grainy() {
        // the dark dot, cloned with a brush whose grain is stripes: 2 px painted, 2 bare
        let copied = |grain: bool| {
            let mut p = halves([200, 200, 200, 255], [200, 200, 200, 255]);
            let b = stamp();
            if grain {
                let tex: Vec<u8> = (0..16).map(|i| if i % 4 < 2 { 255 } else { 0 }).collect();
                assert!(p.set_texture(7, 4, 4, tex));
                p.set_dab_style(7, 1.0, 1.0, 0, 0, 0.0, 0.0, 1);
            }
            assert!(p.begin_clone_stroke(-40.0, 0.0, false, false));
            dab_at(&mut p, &b, 60.0, 30.0);
            // dark pixels in the copy, and the columns the grain leaves bare
            let dark: Vec<(usize, usize)> = (26..35)
                .flat_map(|y| (55..66).map(move |x| (x, y)))
                .filter(|&(x, y)| at(&mut p, x, y)[0] < 100)
                .collect();
            dark
        };
        let (plain, grainy) = (copied(false), copied(true));
        assert!(plain.len() >= 20, "the dot is copied: {}", plain.len());
        assert!(
            !grainy.is_empty() && grainy.len() * 10 < plain.len() * 7,
            "the grain thins the copy: {} → {}",
            plain.len(),
            grainy.len()
        );
        // only the grain's painted columns get the copy
        assert!(grainy.iter().all(|&(x, _)| x % 4 < 2), "{grainy:?}");
    }

    #[test]
    fn heal_takes_on_the_colour_around_it() {
        // a dark-grey left half, a light-grey right half; clone the dot onto the right
        let mut p = halves([60, 60, 60, 255], [200, 200, 200, 255]);
        let b = stamp();
        assert!(p.begin_clone_stroke(-70.0, 0.0, false, false));
        dab_at(&mut p, &b, 90.0, 30.0);
        let plain = at(&mut p, 96, 30)[0];
        p.undo();
        assert!(p.begin_clone_stroke(-70.0, 0.0, false, true));
        dab_at(&mut p, &b, 90.0, 30.0);
        let healed = at(&mut p, 96, 30)[0];
        // around the dab plain cloning brings the dark grey; healed, it's light like here
        assert!(plain < 120, "plain {plain}");
        assert!(healed > 160, "healed {healed}");
        // the dot itself stays darker than its surroundings
        assert!(at(&mut p, 90, 30)[0] < healed);
    }
}
