//! Whole-layer adjustments (hue / saturation, brightness / contrast, blur,
//! recolour) and gradient fills. Pure functions from a layer's original tiles
//! to new tiles, so the painter can preview them live and undo them as one step.

use hokusai::TILE_SIZE;
use hokusai::tile::empty_tile;

use crate::layers::{Blend, merge_pixel, premul_fix15, unpremul_srgb};
use crate::{TileKey, TileMap};

/// Colour adjustments. All zero / `None` = unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ColourAdjust {
    /// degrees, −180…180
    pub hue: f32,
    /// −1 (grey) … 1 (twice as saturated)
    pub saturation: f32,
    /// −1 … 1
    pub brightness: f32,
    /// −1 (flat grey) … 1 (much stronger)
    pub contrast: f32,
    /// blur radius, canvas pixels
    pub blur: f32,
    /// paint everything in this colour (sRGB 0–1), keeping its transparency
    pub recolour: Option<[f32; 3]>,
    /// curves / levels: a look-up table per channel (R, G, B), sRGB 0–255 → 0–255,
    /// applied after the other changes
    pub tone: Option<[[u8; 256]; 3]>,
    /// gradient map: each pixel's lightness (0–255) → a colour, applied last
    pub map: Option<[[u8; 3]; 256]>,
}

impl ColourAdjust {
    /// Changes colours (not just blurs).
    pub fn recolours(&self) -> bool {
        self.recolour.is_some()
            || self.hue != 0.0
            || self.saturation != 0.0
            || self.brightness != 0.0
            || self.contrast != 0.0
            || self.tone.is_some()
            || self.map.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear,
    Radial,
}

/// A gradient from (x0, y0) in `c0` (with alpha `a0`) to (x1, y1) in `c1` / `a1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub c0: [f32; 3],
    pub a0: f32,
    pub c1: [f32; 3],
    pub a1: f32,
}

/// Where the changes may land.
pub struct Area<'a> {
    pub width: u32,
    pub height: u32,
    /// 255 = selected, per canvas pixel (`None`: everywhere)
    pub selection: Option<&'a [u8]>,
    /// alpha lock: transparency stays as it was
    pub keep_alpha: bool,
}

impl Area<'_> {
    /// How much a change counts here: 0 outside the selection, 1 inside, in
    /// between on a feathered edge.
    fn weight(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return 0.0;
        }
        self.selection.map_or(1.0, |s| {
            s[y as usize * self.width as usize + x as usize] as f32 / 255.0
        })
    }
}

const TS: i64 = TILE_SIZE as i64;

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn rgb_to_hsv(c: [f32; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c;
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

/// One colour (sRGB 0–1) with the colour changes of `a` applied.
pub fn colour_op(a: &ColourAdjust, mut c: [f32; 3]) -> [f32; 3] {
    if let Some(rc) = a.recolour {
        c = rc;
    }
    if a.hue != 0.0 || a.saturation != 0.0 {
        let (h, s, v) = rgb_to_hsv(c);
        c = hsv_to_rgb(
            h + a.hue / 360.0,
            (s * (1.0 + a.saturation)).clamp(0.0, 1.0),
            v,
        );
    }
    if a.brightness != 0.0 {
        c = c.map(|v| v + a.brightness * 0.5);
    }
    if a.contrast != 0.0 {
        let f = if a.contrast >= 0.0 {
            1.0 / (1.0 - a.contrast.min(0.95))
        } else {
            1.0 + a.contrast
        };
        c = c.map(|v| (v - 0.5) * f + 0.5);
    }
    let mut c = c.map(|v| v.clamp(0.0, 1.0));
    if let Some(t) = &a.tone {
        c = [0, 1, 2].map(|i| lut_at(&t[i], c[i]));
    }
    if let Some(m) = &a.map {
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let f = l.clamp(0.0, 1.0) * 255.0;
        let (i, t) = (f.floor() as usize, f.fract());
        let j = (i + 1).min(255);
        c = [0, 1, 2].map(|k| (m[i][k] as f32 * (1.0 - t) + m[j][k] as f32 * t) / 255.0);
    }
    c
}

/// A 0–255 table read at v (0–1), between entries.
fn lut_at(t: &[u8; 256], v: f32) -> f32 {
    let f = v.clamp(0.0, 1.0) * 255.0;
    let (i, k) = (f.floor() as usize, f.fract());
    let j = (i + 1).min(255);
    (t[i] as f32 * (1.0 - k) + t[j] as f32 * k) / 255.0
}

/// Three box blurs (≈ Gaussian) of the premultiplied layer, over the area the
/// paint can spread to.
#[allow(clippy::needless_range_loop)]
fn blur_map(orig: &TileMap, radius: f32, w: u32, h: u32) -> TileMap {
    let r = (radius / 2.0).round().max(1.0) as i64;
    if orig.is_empty() {
        return TileMap::new();
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &(tx, ty) in orig.keys() {
        x0 = x0.min(tx as i64 * TS);
        y0 = y0.min(ty as i64 * TS);
        x1 = x1.max((tx as i64 + 1) * TS);
        y1 = y1.max((ty as i64 + 1) * TS);
    }
    let pad = 3 * r + 1;
    let (x0, y0) = ((x0 - pad).max(0), (y0 - pad).max(0));
    let (x1, y1) = ((x1 + pad).min(w as i64), (y1 + pad).min(h as i64));
    if x0 >= x1 || y0 >= y1 {
        return TileMap::new();
    }
    let (rw, rh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut buf = vec![[0f32; 4]; rw * rh];
    for (&(tx, ty), t) in orig {
        for py in 0..TILE_SIZE {
            for px in 0..TILE_SIZE {
                let (x, y) = (tx as i64 * TS + px as i64, ty as i64 * TS + py as i64);
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    let p = t[py][px];
                    buf[(y - y0) as usize * rw + (x - x0) as usize] =
                        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32];
                }
            }
        }
    }
    let mut tmp = vec![[0f32; 4]; rw.max(rh)];
    let k = 1.0 / (2 * r + 1) as f32;
    for _ in 0..3 {
        // rows
        for y in 0..rh {
            let row = &mut buf[y * rw..(y + 1) * rw];
            let mut sum = [0f32; 4];
            for x in 0..=(r as usize).min(rw - 1) {
                for c in 0..4 {
                    sum[c] += row[x][c];
                }
            }
            for x in 0..rw {
                tmp[x] = sum.map(|v| v * k);
                let add = x + r as usize + 1;
                if add < rw {
                    for c in 0..4 {
                        sum[c] += row[add][c];
                    }
                }
                if x >= r as usize {
                    for c in 0..4 {
                        sum[c] -= row[x - r as usize][c];
                    }
                }
            }
            row.copy_from_slice(&tmp[..rw]);
        }
        // columns
        for x in 0..rw {
            let mut sum = [0f32; 4];
            for y in 0..=(r as usize).min(rh - 1) {
                for c in 0..4 {
                    sum[c] += buf[y * rw + x][c];
                }
            }
            for y in 0..rh {
                tmp[y] = sum.map(|v| v * k);
                let add = y + r as usize + 1;
                if add < rh {
                    for c in 0..4 {
                        sum[c] += buf[add * rw + x][c];
                    }
                }
                if y >= r as usize {
                    for c in 0..4 {
                        sum[c] -= buf[(y - r as usize) * rw + x][c];
                    }
                }
            }
            for y in 0..rh {
                buf[y * rw + x] = tmp[y];
            }
        }
    }
    let mut out = TileMap::new();
    for ty in (y0 / TS)..=((y1 - 1) / TS) {
        for tx in (x0 / TS)..=((x1 - 1) / TS) {
            let mut t = empty_tile();
            let mut any = false;
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let (x, y) = (tx * TS + px as i64, ty * TS + py as i64);
                    if x >= x0 && x < x1 && y >= y0 && y < y1 {
                        let v = buf[(y - y0) as usize * rw + (x - x0) as usize];
                        let a = v[3].round().clamp(0.0, 32768.0) as u16;
                        if a > 0 {
                            any = true;
                            // premultiplied: colour never exceeds alpha
                            t[py][px] = [
                                (v[0].round() as u16).min(a),
                                (v[1].round() as u16).min(a),
                                (v[2].round() as u16).min(a),
                                a,
                            ];
                        }
                    }
                }
            }
            if any {
                out.insert((tx as i32, ty as i32), t);
            }
        }
    }
    out
}

/// Run `f(x, y, original pixel, new pixel) → pixel` over `keys`; tiles that end up
/// empty are dropped. Outside the selection the original stays.
pub(crate) fn finish(
    orig: &TileMap,
    src: &TileMap,
    keys: impl Iterator<Item = TileKey>,
    area: &Area<'_>,
    f: impl Fn(i64, i64, [u16; 4]) -> [u16; 4],
) -> TileMap {
    let mut out = TileMap::new();
    for key in keys {
        let o = orig.get(&key);
        let s = src.get(&key);
        let mut t = empty_tile();
        let mut any = false;
        for py in 0..TILE_SIZE {
            for px in 0..TILE_SIZE {
                let (x, y) = (key.0 as i64 * TS + px as i64, key.1 as i64 * TS + py as i64);
                let op = o.map_or([0; 4], |t| t[py][px]);
                let wt = area.weight(x, y);
                let mut p = if wt > 0.0 {
                    crate::lerp_px(op, f(x, y, s.map_or([0; 4], |t| t[py][px])), wt)
                } else {
                    op
                };
                if area.keep_alpha && p[3] != op[3] {
                    let (c, _) = unpremul_srgb(p);
                    let a = op[3] as f32 / 32768.0;
                    p = if p[3] == 0 { op } else { premul_fix15(c, a) };
                }
                any |= p[3] > 0;
                t[py][px] = p;
            }
        }
        if any {
            out.insert(key, t);
        }
    }
    out
}

/// The layer with `a` applied.
pub fn adjust_tiles(orig: &TileMap, a: &ColourAdjust, area: &Area<'_>) -> TileMap {
    let src = if a.blur >= 1.0 {
        blur_map(orig, a.blur, area.width, area.height)
    } else {
        orig.clone()
    };
    let colour = a.recolours();
    let mut keys: Vec<TileKey> = src.keys().chain(orig.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    finish(orig, &src, keys.into_iter(), area, |_, _, p| {
        if !colour || p[3] == 0 {
            return p;
        }
        let (c, al) = unpremul_srgb(p);
        premul_fix15(colour_op(a, c), al)
    })
}

/// The layer with a gradient painted over it (normal blending).
pub fn gradient_tiles(orig: &TileMap, g: &Gradient, area: &Area<'_>) -> TileMap {
    let (dx, dy) = (g.to.0 - g.from.0, g.to.1 - g.from.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let len = len2.sqrt();
    let (tw, th) = (
        (area.width as i64 + TS - 1) / TS,
        (area.height as i64 + TS - 1) / TS,
    );
    let keys = (0..th).flat_map(|ty| (0..tw).map(move |tx| (tx as i32, ty as i32)));
    finish(orig, orig, keys, area, |x, y, p| {
        let (fx, fy) = (x as f32 + 0.5 - g.from.0, y as f32 + 0.5 - g.from.1);
        let t = match g.kind {
            GradientKind::Linear => (fx * dx + fy * dy) / len2,
            GradientKind::Radial => (fx * fx + fy * fy).sqrt() / len,
        }
        .clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let c = [
            lerp(g.c0[0], g.c1[0]),
            lerp(g.c0[1], g.c1[1]),
            lerp(g.c0[2], g.c1[2]),
        ];
        let src = premul_fix15(c, lerp(g.a0, g.a1));
        merge_pixel(p, src, 1.0, Blend::Normal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(c: [f32; 3], a: f32, keys: &[TileKey]) -> TileMap {
        keys.iter()
            .map(|&k| {
                let mut t = empty_tile();
                for row in t.iter_mut() {
                    for p in row.iter_mut() {
                        *p = premul_fix15(c, a);
                    }
                }
                (k, t)
            })
            .collect()
    }

    fn at(m: &TileMap, x: i64, y: i64) -> ([f32; 3], f32) {
        m.get(&((x / TS) as i32, (y / TS) as i32))
            .map_or(([0.0; 3], 0.0), |t| {
                unpremul_srgb(t[(y % TS) as usize][(x % TS) as usize])
            })
    }

    const AREA: Area<'static> = Area {
        width: 256,
        height: 128,
        selection: None,
        keep_alpha: false,
    };

    #[test]
    fn colour_adjustments() {
        let red = solid([0.8, 0.1, 0.1], 1.0, &[(0, 0)]);
        // curves: an inverting table on every channel
        let mut inv = [0u8; 256];
        for (i, v) in inv.iter_mut().enumerate() {
            *v = 255 - i as u8;
        }
        let t = adjust_tiles(
            &red,
            &ColourAdjust {
                tone: Some([inv; 3]),
                ..Default::default()
            },
            &AREA,
        );
        let (c, _) = at(&t, 5, 5);
        assert!(
            (c[0] - 0.2).abs() < 0.02 && (c[1] - 0.9).abs() < 0.02,
            "{c:?}"
        );
        // gradient map: dark → blue, light → yellow
        let mut map = [[0u8; 3]; 256];
        for (i, m) in map.iter_mut().enumerate() {
            let k = i as f32 / 255.0;
            *m = [
                (k * 255.0) as u8,
                (k * 255.0) as u8,
                ((1.0 - k) * 255.0) as u8,
            ];
        }
        let g = adjust_tiles(
            &red,
            &ColourAdjust {
                map: Some(map),
                ..Default::default()
            },
            &AREA,
        );
        let (c, _) = at(&g, 5, 5);
        assert!(c[2] > c[0], "dark red maps towards blue: {c:?}");
        // hue +120°: red → green
        let g = adjust_tiles(
            &red,
            &ColourAdjust {
                hue: 120.0,
                ..Default::default()
            },
            &AREA,
        );
        let (c, a) = at(&g, 5, 5);
        assert!(c[1] > 0.7 && c[0] < 0.2 && a > 0.99, "{c:?}");
        // saturation −1: grey
        let grey = adjust_tiles(
            &red,
            &ColourAdjust {
                saturation: -1.0,
                ..Default::default()
            },
            &AREA,
        );
        let (c, _) = at(&grey, 5, 5);
        assert!(
            (c[0] - c[1]).abs() < 0.02 && (c[1] - c[2]).abs() < 0.02,
            "{c:?}"
        );
        // brightness / contrast
        let b = adjust_tiles(
            &red,
            &ColourAdjust {
                brightness: 0.4,
                ..Default::default()
            },
            &AREA,
        );
        assert!(at(&b, 5, 5).0[1] > 0.25);
        let flat = adjust_tiles(
            &red,
            &ColourAdjust {
                contrast: -1.0,
                ..Default::default()
            },
            &AREA,
        );
        assert!((at(&flat, 5, 5).0[0] - 0.5).abs() < 0.02);
        // recolour keeps transparency
        let half = solid([0.0; 3], 0.5, &[(0, 0)]);
        let blue = adjust_tiles(
            &half,
            &ColourAdjust {
                recolour: Some([0.0, 0.0, 1.0]),
                ..Default::default()
            },
            &AREA,
        );
        let (c, a) = at(&blue, 5, 5);
        assert!(c[2] > 0.98 && (a - 0.5).abs() < 0.01);
    }

    #[test]
    fn blur_spreads_paint_and_selection_limits_it() {
        // a hard-edged block in tile (1, 0)
        let block = solid([0.0; 3], 1.0, &[(1, 0)]);
        let b = adjust_tiles(
            &block,
            &ColourAdjust {
                blur: 12.0,
                ..Default::default()
            },
            &AREA,
        );
        assert!(at(&b, 60, 30).1 > 0.05, "spreads left of the block");
        assert!(at(&b, 96, 30).1 > 0.95, "middle stays solid");
        assert!(at(&b, 20, 30).1 == 0.0, "not far");
        // only the left half of the canvas selected: the block is untouched
        let mut sel = vec![0u8; 256 * 128];
        for y in 0..128 {
            for x in 0..64 {
                sel[y * 256 + x] = 255;
            }
        }
        let area = Area {
            selection: Some(&sel),
            ..AREA
        };
        let b = adjust_tiles(
            &block,
            &ColourAdjust {
                blur: 12.0,
                ..Default::default()
            },
            &area,
        );
        assert!(at(&b, 60, 30).1 > 0.05);
        assert!(
            (at(&b, 66, 30).1 - 1.0).abs() < 0.01,
            "outside the selection: as before"
        );
    }

    #[test]
    fn gradients() {
        let g = Gradient {
            kind: GradientKind::Linear,
            from: (0.0, 0.0),
            to: (256.0, 0.0),
            c0: [1.0, 0.0, 0.0],
            a0: 1.0,
            c1: [0.0, 0.0, 1.0],
            a1: 1.0,
        };
        let m = gradient_tiles(&TileMap::new(), &g, &AREA);
        assert!(at(&m, 2, 60).0[0] > 0.95);
        assert!(at(&m, 253, 60).0[2] > 0.95);
        let mid = at(&m, 128, 60).0;
        assert!(mid[0] > 0.3 && mid[2] > 0.3);
        // to transparent: fades out
        let fade = Gradient {
            c1: [1.0, 0.0, 0.0],
            a1: 0.0,
            ..g
        };
        let m = gradient_tiles(&TileMap::new(), &fade, &AREA);
        assert!(at(&m, 2, 60).1 > 0.95 && at(&m, 250, 60).1 < 0.05);
        // radial: centre is c0
        let r = Gradient {
            kind: GradientKind::Radial,
            from: (128.0, 64.0),
            to: (128.0, 0.0),
            ..g
        };
        let m = gradient_tiles(&TileMap::new(), &r, &AREA);
        assert!(at(&m, 128, 64).0[0] > 0.95 && at(&m, 128, 2).0[2] > 0.9);
        // alpha lock: only where there was paint
        let dot = solid([0.0; 3], 1.0, &[(0, 0)]);
        let m = gradient_tiles(
            &dot,
            &g,
            &Area {
                keep_alpha: true,
                ..AREA
            },
        );
        assert!(at(&m, 2, 2).0[0] > 0.95);
        assert_eq!(at(&m, 200, 60).1, 0.0);
    }
}
