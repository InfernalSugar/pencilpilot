//! Filters (Procreate's Adjustments, Photoshop's Filter menu): sharpen, motion
//! blur, noise, halftone, chromatic aberration, bloom and glitch. Like the
//! adjustments in [`crate::adjust`], pure functions from a layer's original
//! tiles to new ones, so they preview live and undo as one step; the selection
//! and alpha lock apply the same way.

use hokusai::TILE_SIZE;
use hokusai::tile::empty_tile;

use crate::adjust::{Area, finish};
use crate::layers::{premul_fix15, unpremul_srgb};
use crate::{TileKey, TileMap};

const TS: i64 = TILE_SIZE as i64;

/// A filter and its settings. Zero amounts change nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Filter {
    /// unsharp mask: `amount` 0… (1 = 100 %), blur `radius` in px
    Sharpen { amount: f32, radius: f32 },
    /// smear along `angle` (degrees) over `distance` px
    MotionBlur { angle: f32, distance: f32 },
    /// grain: `amount` 0–1, grey (`mono`) or coloured; `seed` picks the pattern
    Noise { amount: f32, mono: bool, seed: u32 },
    /// dots `size` px apart; ink dots per channel, or one black screen (`mono`)
    Halftone { size: f32, mono: bool },
    /// red and blue pulled apart from the canvas centre, `amount` px at the corners
    Chromatic { amount: f32 },
    /// glow around light parts: lightness `threshold` 0–1, `size` px, `strength` 0…
    Bloom {
        threshold: f32,
        size: f32,
        strength: f32,
    },
    /// bands shifted sideways with their channels split: `amount` 0–1, bands
    /// `block` px tall; the same `seed` gives the same glitch
    Glitch { amount: f32, block: f32, seed: u32 },
}

impl Filter {
    /// A filter from its name and numbers (as the app passes them): `a`, `b`, `c`
    /// are its settings in the order above, `flag` is `mono`.
    pub fn from_parts(kind: &str, a: f32, b: f32, c: f32, flag: bool, seed: u32) -> Option<Self> {
        Some(match kind {
            "sharpen" => Filter::Sharpen {
                amount: a,
                radius: b,
            },
            "motion" => Filter::MotionBlur {
                angle: a,
                distance: b,
            },
            "noise" => Filter::Noise {
                amount: a,
                mono: flag,
                seed,
            },
            "halftone" => Filter::Halftone {
                size: a,
                mono: flag,
            },
            "chromatic" => Filter::Chromatic { amount: a },
            "bloom" => Filter::Bloom {
                threshold: a,
                size: b,
                strength: c,
            },
            "glitch" => Filter::Glitch {
                amount: a,
                block: b,
                seed,
            },
            _ => return None,
        })
    }

    /// Changes nothing at these settings.
    fn is_noop(&self) -> bool {
        match *self {
            Filter::Sharpen { amount, radius } => amount <= 0.0 || radius < 1.0,
            Filter::MotionBlur { distance, .. } => distance < 1.0,
            Filter::Noise { amount, .. } => amount <= 0.0,
            Filter::Halftone { size, .. } => size < 2.0,
            Filter::Chromatic { amount } => amount < 0.05,
            Filter::Bloom { size, strength, .. } => strength <= 0.0 || size < 1.0,
            Filter::Glitch { amount, block, .. } => amount <= 0.0 || block < 1.0,
        }
    }
}

/// A small, well mixed hash (the same inputs always give the same number).
fn hash(a: u32, b: u32, seed: u32) -> u32 {
    let mut h = a
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(b.wrapping_mul(0x85EB_CA77))
        .wrapping_add(seed.wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// 0–1 from a hash.
fn unit(h: u32) -> f32 {
    h as f32 / u32::MAX as f32
}

/// Premultiplied fix15 pixels as floats over a box of the canvas (transparent
/// outside it). Each filter reads the original from one and writes another.
#[derive(Clone)]
struct Buf {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Buf {
    /// The layer over the box its paint covers, grown by `pad` px for the paint
    /// to spread into, kept to the canvas (None: nothing on the canvas).
    fn load(orig: &TileMap, pad: i64, area: &Area<'_>) -> Option<Buf> {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for &(tx, ty) in orig.keys() {
            x0 = x0.min(tx as i64 * TS);
            y0 = y0.min(ty as i64 * TS);
            x1 = x1.max((tx as i64 + 1) * TS);
            y1 = y1.max((ty as i64 + 1) * TS);
        }
        let (x0, y0) = (
            (x0.saturating_sub(pad)).max(0),
            (y0.saturating_sub(pad)).max(0),
        );
        let (x1, y1) = (
            (x1.saturating_add(pad)).min(area.width as i64),
            (y1.saturating_add(pad)).min(area.height as i64),
        );
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut px = vec![[0f32; 4]; w * h];
        for (&(tx, ty), t) in orig {
            for (py, row) in t.iter().enumerate() {
                let y = ty as i64 * TS + py as i64;
                if y < y0 || y >= y1 {
                    continue;
                }
                for (pxi, p) in row.iter().enumerate() {
                    let x = tx as i64 * TS + pxi as i64;
                    if x >= x0 && x < x1 {
                        px[(y - y0) as usize * w + (x - x0) as usize] = p.map(|v| v as f32);
                    }
                }
            }
        }
        Some(Buf { x0, y0, w, h, px })
    }

    /// The pixel at canvas (x, y); transparent outside the box.
    fn at(&self, x: i64, y: i64) -> [f32; 4] {
        let (bx, by) = (x - self.x0, y - self.y0);
        if bx < 0 || by < 0 || bx >= self.w as i64 || by >= self.h as i64 {
            return [0.0; 4];
        }
        self.px[by as usize * self.w + bx as usize]
    }

    /// Read between pixels (canvas coordinates; pixel x's centre is x + 0.5).
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i64, iy as i64);
        let (a, b) = (self.at(ix, iy), self.at(ix + 1, iy));
        let (c, d) = (self.at(ix, iy + 1), self.at(ix + 1, iy + 1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            top + (bottom - top) * ty
        })
    }

    /// A new picture over the same box: `f(x, y)` for each pixel.
    fn map(&self, f: impl Fn(i64, i64) -> [f32; 4]) -> Buf {
        let mut px = Vec::with_capacity(self.px.len());
        for y in 0..self.h {
            for x in 0..self.w {
                px.push(f(self.x0 + x as i64, self.y0 + y as i64));
            }
        }
        Buf { px, ..*self }
    }

    /// Three box blurs (≈ Gaussian) of radius `r` px, in place.
    #[allow(clippy::needless_range_loop)]
    fn blur(&mut self, r: usize) {
        let (w, h) = (self.w, self.h);
        let mut tmp = vec![[0f32; 4]; w.max(h)];
        let k = 1.0 / (2 * r + 1) as f32;
        let buf = &mut self.px;
        for _ in 0..3 {
            for y in 0..h {
                let row = &mut buf[y * w..(y + 1) * w];
                let mut sum = [0f32; 4];
                for x in 0..=r.min(w - 1) {
                    for c in 0..4 {
                        sum[c] += row[x][c];
                    }
                }
                for x in 0..w {
                    tmp[x] = sum.map(|v| v * k);
                    if x + r + 1 < w {
                        for c in 0..4 {
                            sum[c] += row[x + r + 1][c];
                        }
                    }
                    if x >= r {
                        for c in 0..4 {
                            sum[c] -= row[x - r][c];
                        }
                    }
                }
                row.copy_from_slice(&tmp[..w]);
            }
            for x in 0..w {
                let mut sum = [0f32; 4];
                for y in 0..=r.min(h - 1) {
                    for c in 0..4 {
                        sum[c] += buf[y * w + x][c];
                    }
                }
                for y in 0..h {
                    tmp[y] = sum.map(|v| v * k);
                    if y + r + 1 < h {
                        for c in 0..4 {
                            sum[c] += buf[(y + r + 1) * w + x][c];
                        }
                    }
                    if y >= r {
                        for c in 0..4 {
                            sum[c] -= buf[(y - r) * w + x][c];
                        }
                    }
                }
                for y in 0..h {
                    buf[y * w + x] = tmp[y];
                }
            }
        }
    }

    /// Back to tiles (empty ones left out).
    fn tiles(&self) -> TileMap {
        let mut out = TileMap::new();
        let (x1, y1) = (self.x0 + self.w as i64, self.y0 + self.h as i64);
        for ty in (self.y0 / TS)..=((y1 - 1) / TS) {
            for tx in (self.x0 / TS)..=((x1 - 1) / TS) {
                let mut t = empty_tile();
                let mut any = false;
                for (py, row) in t.iter_mut().enumerate() {
                    for (pxi, p) in row.iter_mut().enumerate() {
                        let v = self.at(tx * TS + pxi as i64, ty * TS + py as i64);
                        *p = to_px(v);
                        any |= p[3] > 0;
                    }
                }
                if any {
                    out.insert((tx as i32, ty as i32), t);
                }
            }
        }
        out
    }
}

/// A float pixel as fix15 (premultiplied: colour never exceeds alpha).
fn to_px(v: [f32; 4]) -> [u16; 4] {
    let a = v[3].round().clamp(0.0, 32768.0) as u16;
    if a == 0 {
        return [0; 4];
    }
    let c = |x: f32| (x.round().max(0.0) as u16).min(a);
    [c(v[0]), c(v[1]), c(v[2]), a]
}

/// sRGB colour (0–1) and alpha of a float pixel.
fn colour_of(v: [f32; 4]) -> ([f32; 3], f32) {
    unpremul_srgb(to_px(v))
}

/// Blur radius as the adjust blur takes it (three box blurs of half the size).
fn box_radius(size: f32) -> usize {
    (size / 2.0).round().max(1.0) as usize
}

/// The layer with `f` applied.
pub fn filter_tiles(orig: &TileMap, f: &Filter, area: &Area<'_>) -> TileMap {
    if f.is_noop() || orig.is_empty() {
        return orig.clone();
    }
    // filters that keep each pixel where it is: per pixel, on the original
    match *f {
        Filter::Noise { amount, mono, seed } => {
            return finish(orig, orig, orig.keys().copied(), area, |x, y, p| {
                if p[3] == 0 {
                    return p;
                }
                let (c, a) = unpremul_srgb(p);
                let (xu, yu) = (x as u32, y as u32);
                let n = |k: u32| (unit(hash(xu, yu, seed.wrapping_add(k))) - 0.5) * amount;
                let c = if mono {
                    let d = n(0);
                    c.map(|v| v + d)
                } else {
                    [c[0] + n(0), c[1] + n(1), c[2] + n(2)]
                };
                premul_fix15(c, a)
            });
        }
        Filter::Halftone { size, mono } => {
            let Some(mut soft) = Buf::load(orig, size.ceil() as i64 * 2, area) else {
                return orig.clone();
            };
            soft.blur((size / 3.0).round().max(1.0) as usize);
            // screen angles: one black screen, or one per channel (as cyan, magenta, yellow)
            let angles: [f32; 3] = if mono { [45.0; 3] } else { [15.0, 75.0, 0.0] };
            return finish(orig, orig, orig.keys().copied(), area, |x, y, p| {
                if p[3] == 0 {
                    return p;
                }
                let (_, a) = unpremul_srgb(p);
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let mut out = [1.0f32; 3];
                for (ch, deg) in angles.iter().enumerate() {
                    if mono && ch > 0 {
                        out[ch] = out[0];
                        continue;
                    }
                    let (s, c) = deg.to_radians().sin_cos();
                    // into the screen's grid, to the middle of this dot's cell, and back
                    let (u, v) = (px * c + py * s, -px * s + py * c);
                    let (cu, cv) = (
                        ((u / size).floor() + 0.5) * size,
                        ((v / size).floor() + 0.5) * size,
                    );
                    let (cx, cy) = (cu * c - cv * s, cu * s + cv * c);
                    let (col, ca) = colour_of(soft.sample(cx, cy));
                    let val = if ca <= 0.0 {
                        1.0
                    } else if mono {
                        0.2126 * col[0] + 0.7152 * col[1] + 0.0722 * col[2]
                    } else {
                        col[ch]
                    };
                    // a dot's area follows the ink (1 − lightness)
                    let ink = (1.0 - val).clamp(0.0, 1.0);
                    let r = size * (ink / std::f32::consts::PI).sqrt();
                    let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt();
                    out[ch] = if r > 0.0 {
                        1.0 - (r - d + 0.5).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                }
                premul_fix15(out, a)
            });
        }
        _ => {}
    }
    let pad = match *f {
        Filter::Sharpen { radius, .. } => 3 * box_radius(radius) as i64 + 1,
        Filter::MotionBlur { distance, .. } => (distance / 2.0).ceil() as i64 + 2,
        Filter::Chromatic { amount } => amount.ceil() as i64 + 2,
        Filter::Bloom { size, .. } => 3 * box_radius(size) as i64 + 1,
        Filter::Glitch { amount, block, .. } => (amount * block * 5.0).ceil() as i64 + 2,
        Filter::Noise { .. } | Filter::Halftone { .. } => 0,
    };
    let Some(src) = Buf::load(orig, pad, area) else {
        return orig.clone();
    };
    let out = match *f {
        Filter::Sharpen { amount, radius } => {
            let mut soft = src.clone();
            soft.blur(box_radius(radius));
            src.map(|x, y| {
                let (p, b) = (src.at(x, y), soft.at(x, y));
                let c = |k: usize| (p[k] + (p[k] - b[k]) * amount).clamp(0.0, p[3]);
                [c(0), c(1), c(2), p[3]]
            })
        }
        Filter::MotionBlur { angle, distance } => {
            let (dy, dx) = angle.to_radians().sin_cos();
            let n = distance.ceil() as usize + 1;
            src.map(|x, y| {
                let mut sum = [0f32; 4];
                for i in 0..n {
                    let t = (i as f32 / (n - 1) as f32 - 0.5) * distance;
                    // screen y points down: a positive angle smears up and right
                    let s = src.sample(x as f32 + 0.5 + dx * t, y as f32 + 0.5 - dy * t);
                    for k in 0..4 {
                        sum[k] += s[k];
                    }
                }
                sum.map(|v| v / n as f32)
            })
        }
        Filter::Chromatic { amount } => {
            let (cx, cy) = (area.width as f32 / 2.0, area.height as f32 / 2.0);
            let k = amount / (cx * cx + cy * cy).sqrt().max(1.0);
            src.map(|x, y| {
                let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let r = src.sample(cx + px * (1.0 - k), cy + py * (1.0 - k));
                let g = src.at(x, y);
                let b = src.sample(cx + px * (1.0 + k), cy + py * (1.0 + k));
                [r[0], g[1], b[2], r[3].max(g[3]).max(b[3])]
            })
        }
        Filter::Bloom {
            threshold,
            size,
            strength,
        } => {
            // the light parts only, then spread
            let mut glow = src.map(|x, y| {
                let p = src.at(x, y);
                let (c, _) = colour_of(p);
                let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                let w = ((l - threshold) / (1.0 - threshold).max(1e-3)).clamp(0.0, 1.0);
                p.map(|v| v * w)
            });
            glow.blur(box_radius(size));
            src.map(|x, y| {
                let (p, g) = (src.at(x, y), glow.at(x, y));
                let ga = (g[3] * strength).min(32768.0);
                let a = p[3] + (32768.0 - p[3]) * ga / 32768.0;
                let c = |k: usize| (p[k] + g[k] * strength).min(a);
                [c(0), c(1), c(2), a]
            })
        }
        Filter::Glitch {
            amount,
            block,
            seed,
        } => {
            let block = block.max(1.0);
            src.map(|x, y| {
                let band = (y as f32 / block).floor() as i32 as u32;
                let h = hash(band, 0, seed);
                if unit(h) >= amount {
                    return src.at(x, y);
                }
                // this band moves sideways, its red and blue a little apart
                let shift =
                    ((unit(hash(band, 1, seed)) * 2.0 - 1.0) * amount * block * 4.0).round();
                let split = (unit(hash(band, 2, seed)) * amount * block).round();
                let at = |dx: f32| src.at(x - (shift + dx) as i64, y);
                let (r, g, b) = (at(split), at(0.0), at(-split));
                [r[0], g[1], b[2], r[3].max(g[3]).max(b[3])]
            })
        }
        Filter::Noise { .. } | Filter::Halftone { .. } => unreachable!(),
    };
    let next = out.tiles();
    let mut keys: Vec<TileKey> = next.keys().chain(orig.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    finish(orig, &next, keys.into_iter(), area, |_, _, p| p)
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

    const ALL_OFF: [Filter; 7] = [
        Filter::Sharpen {
            amount: 0.0,
            radius: 4.0,
        },
        Filter::MotionBlur {
            angle: 30.0,
            distance: 0.0,
        },
        Filter::Noise {
            amount: 0.0,
            mono: false,
            seed: 1,
        },
        Filter::Halftone {
            size: 0.0,
            mono: true,
        },
        Filter::Chromatic { amount: 0.0 },
        Filter::Bloom {
            threshold: 0.5,
            size: 10.0,
            strength: 0.0,
        },
        Filter::Glitch {
            amount: 0.0,
            block: 8.0,
            seed: 1,
        },
    ];

    #[test]
    fn nothing_changes_at_zero() {
        let m = solid([0.4, 0.6, 0.2], 1.0, &[(0, 0), (1, 1)]);
        for f in ALL_OFF {
            assert!(filter_tiles(&m, &f, &AREA) == m, "{f:?}");
        }
    }

    #[test]
    fn names_and_numbers() {
        assert_eq!(
            Filter::from_parts("bloom", 0.6, 12.0, 1.0, false, 0),
            Some(Filter::Bloom {
                threshold: 0.6,
                size: 12.0,
                strength: 1.0
            })
        );
        assert_eq!(Filter::from_parts("nope", 0.0, 0.0, 0.0, false, 0), None);
    }

    #[test]
    fn sharpen_strengthens_edges() {
        let mut m = solid([0.3; 3], 1.0, &[(0, 0)]);
        m.extend(solid([0.7; 3], 1.0, &[(1, 0)]));
        let f = Filter::Sharpen {
            amount: 1.0,
            radius: 6.0,
        };
        let s = filter_tiles(&m, &f, &AREA);
        assert!(at(&s, 62, 30).0[0] < 0.28, "dark side darker");
        assert!(at(&s, 66, 30).0[0] > 0.72, "light side lighter");
        assert!((at(&s, 20, 30).0[0] - 0.3).abs() < 0.01, "flat parts stay");
        // only the left tile selected: the light side is untouched
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
        let s = filter_tiles(&m, &f, &area);
        assert!(at(&s, 62, 30).0[0] < 0.28);
        assert!((at(&s, 66, 30).0[0] - 0.7).abs() < 0.01);
    }

    #[test]
    fn motion_blur_smears_one_way() {
        let block = solid([0.0; 3], 1.0, &[(1, 0)]);
        let f = Filter::MotionBlur {
            angle: 0.0,
            distance: 20.0,
        };
        let b = filter_tiles(&block, &f, &AREA);
        assert!(at(&b, 60, 30).1 > 0.1, "spreads sideways");
        assert!(at(&b, 96, 30).1 > 0.95, "middle stays");
        // the block spans the tile's full height: nothing above it to check, so
        // check it doesn't spread down
        assert_eq!(at(&b, 96, 70).1, 0.0, "not up or down");
    }

    #[test]
    fn noise_is_grain_and_repeatable() {
        let grey = solid([0.5; 3], 0.8, &[(0, 0)]);
        let f = Filter::Noise {
            amount: 0.4,
            mono: true,
            seed: 7,
        };
        let n = filter_tiles(&grey, &f, &AREA);
        assert!(n == filter_tiles(&grey, &f, &AREA));
        let mut changed = 0;
        for x in 0..64 {
            let (c, a) = at(&n, x, 5);
            assert!((a - 0.8).abs() < 0.01, "transparency kept");
            assert!((c[0] - c[1]).abs() < 0.02, "grey grain");
            if (c[0] - 0.5).abs() > 0.02 {
                changed += 1;
            }
        }
        assert!(changed > 30);
        let coloured = filter_tiles(
            &grey,
            &Filter::Noise {
                amount: 0.4,
                mono: false,
                seed: 7,
            },
            &AREA,
        );
        assert!((0..64).any(|x| {
            let c = at(&coloured, x, 5).0;
            (c[0] - c[2]).abs() > 0.05
        }));
    }

    #[test]
    fn halftone_makes_dots() {
        let grey = solid([0.5; 3], 1.0, &[(0, 0)]);
        let h = filter_tiles(
            &grey,
            &Filter::Halftone {
                size: 8.0,
                mono: true,
            },
            &AREA,
        );
        let (mut dark, mut light) = (0, 0);
        for y in 8..56 {
            for x in 8..56 {
                let (c, a) = at(&h, x, y);
                assert!(a > 0.99 && (c[0] - c[2]).abs() < 0.01);
                if c[0] < 0.1 {
                    dark += 1;
                }
                if c[0] > 0.9 {
                    light += 1;
                }
            }
        }
        assert!(dark > 200 && light > 200, "{dark} {light}");
        // colour: a red picture prints no cyan (red stays full)
        let red = solid([1.0, 0.0, 0.0], 1.0, &[(0, 0)]);
        let h = filter_tiles(
            &red,
            &Filter::Halftone {
                size: 8.0,
                mono: false,
            },
            &AREA,
        );
        assert!(at(&h, 30, 30).0[0] > 0.99);
    }

    #[test]
    fn chromatic_aberration_splits_outwards() {
        // white block on the right of the canvas (centre 128, 64)
        let white = solid([1.0; 3], 1.0, &[(3, 1)]);
        let f = Filter::Chromatic { amount: 12.0 };
        let c = filter_tiles(&white, &f, &AREA);
        // the block's edge nearest the centre: red pulls outwards, leaving cyan
        let (inner, _) = at(&c, 193, 65);
        assert!(inner[0] < 0.5 && inner[2] > 0.9, "{inner:?}");
        // and blue spills in towards the centre
        let (col, a) = at(&c, 190, 65);
        assert!(a > 0.5 && col[2] > 0.9 && col[0] < 0.1, "{col:?} {a}");
    }

    #[test]
    fn bloom_glows_from_light_parts_only() {
        let white = solid([1.0; 3], 1.0, &[(1, 0)]);
        let f = Filter::Bloom {
            threshold: 0.5,
            size: 16.0,
            strength: 1.0,
        };
        let b = filter_tiles(&white, &f, &AREA);
        assert!(at(&b, 58, 30).1 > 0.05, "glows outside");
        let dark = solid([0.1; 3], 1.0, &[(1, 0)]);
        let b = filter_tiles(&dark, &f, &AREA);
        assert_eq!(at(&b, 58, 30).1, 0.0, "dark parts don't glow");
    }

    #[test]
    fn glitch_repeats_with_its_seed() {
        let mut m = solid([0.2, 0.5, 0.9], 1.0, &[(1, 0)]);
        m.extend(solid([0.9, 0.3, 0.1], 1.0, &[(2, 0)]));
        let f = |seed| Filter::Glitch {
            amount: 0.6,
            block: 8.0,
            seed,
        };
        let a = filter_tiles(&m, &f(3), &AREA);
        assert!(
            a == filter_tiles(&m, &f(3), &AREA),
            "same seed, same glitch"
        );
        assert!(
            a != filter_tiles(&m, &f(4), &AREA),
            "another seed, another glitch"
        );
        assert!(a != m, "something moved");
    }
}
