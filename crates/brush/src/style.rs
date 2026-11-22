//! Layer styles: a stroke (outline), drop shadow and outer glow drawn around a
//! layer's paint without changing it. Rendered from the layer's tiles into new
//! tiles that show in its place; the canvas keeps them per layer and redraws
//! only around what changed (see `Painter::flush`).

use hokusai::TILE_SIZE;
use hokusai::tile::empty_tile;

use crate::layers::{mask_value, premul_fix15, unpremul_srgb};
use crate::{TileKey, TileMap};

/// Where a stroke sits on the paint's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokePosition {
    #[default]
    Outside,
    Inside,
    Centre,
}

impl StrokePosition {
    pub fn as_str(self) -> &'static str {
        match self {
            StrokePosition::Outside => "outside",
            StrokePosition::Inside => "inside",
            StrokePosition::Centre => "centre",
        }
    }
}

/// An outline around the paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub on: bool,
    /// px, 1–100
    pub size: f32,
    /// sRGB 0–1
    pub colour: [f32; 3],
    pub position: StrokePosition,
    /// 0–1
    pub opacity: f32,
}

/// A soft copy of the paint's shape, offset away from the light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub on: bool,
    pub colour: [f32; 3],
    pub opacity: f32,
    /// where the light comes from, degrees anticlockwise from the right (120 =
    /// upper left: the shadow falls down and to the right)
    pub angle: f32,
    /// px, 0–200
    pub distance: f32,
    /// softness, px, 0–100
    pub blur: f32,
}

/// A soft halo around the paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub on: bool,
    pub colour: [f32; 3],
    /// px, 1–100
    pub size: f32,
    pub opacity: f32,
}

/// A layer's style. Each effect keeps its settings while it's off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerStyle {
    pub stroke: Stroke,
    pub shadow: Shadow,
    pub glow: Glow,
}

impl Default for LayerStyle {
    fn default() -> Self {
        Self {
            stroke: Stroke {
                on: false,
                size: 4.0,
                colour: [0.0; 3],
                position: StrokePosition::Outside,
                opacity: 1.0,
            },
            shadow: Shadow {
                on: false,
                colour: [0.0; 3],
                opacity: 0.5,
                angle: 120.0,
                distance: 10.0,
                blur: 8.0,
            },
            glow: Glow {
                on: false,
                colour: [1.0, 237.0 / 255.0, 135.0 / 255.0],
                size: 12.0,
                opacity: 0.75,
            },
        }
    }
}

/// How many numbers [`LayerStyle::from_numbers`] reads.
pub const NUMBERS: usize = 21;

const TS: i64 = TILE_SIZE as i64;

impl LayerStyle {
    /// Any effect on.
    pub fn any(&self) -> bool {
        self.stroke.on || self.shadow.on || self.glow.on
    }

    /// Read from numbers (how the app passes it): stroke on, size, r, g, b,
    /// position (0 outside, 1 inside, 2 centre), opacity; shadow on, r, g, b,
    /// opacity, angle, distance, blur; glow on, r, g, b, size, opacity. On = not
    /// 0, colours 0–1. Out-of-range values are clamped; `None` if the count is wrong.
    pub fn from_numbers(n: &[f32]) -> Option<Self> {
        if n.len() != NUMBERS || n.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let rgb = |i: usize| [n[i], n[i + 1], n[i + 2]].map(|v| v.clamp(0.0, 1.0));
        let unit = |v: f32| v.clamp(0.0, 1.0);
        Some(Self {
            stroke: Stroke {
                on: n[0] != 0.0,
                size: n[1].clamp(1.0, 100.0),
                colour: rgb(2),
                position: match n[5] as i32 {
                    1 => StrokePosition::Inside,
                    2 => StrokePosition::Centre,
                    _ => StrokePosition::Outside,
                },
                opacity: unit(n[6]),
            },
            shadow: Shadow {
                on: n[7] != 0.0,
                colour: rgb(8),
                opacity: unit(n[11]),
                angle: n[12].rem_euclid(360.0),
                distance: n[13].clamp(0.0, 200.0),
                blur: n[14].clamp(0.0, 100.0),
            },
            glow: Glow {
                on: n[15] != 0.0,
                colour: rgb(16),
                size: n[19].clamp(1.0, 100.0),
                opacity: unit(n[20]),
            },
        })
    }

    /// As JSON for the app (colours as #rrggbb).
    pub fn json(&self) -> String {
        let hex = |c: [f32; 3]| {
            let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            format!("\"#{:02x}{:02x}{:02x}\"", b[0], b[1], b[2])
        };
        let num = |v: f32| (v * 1000.0).round() / 1000.0;
        let (s, d, g) = (&self.stroke, &self.shadow, &self.glow);
        format!(
            r#"{{"stroke":{{"on":{},"size":{},"colour":{},"position":"{}","opacity":{}}},"shadow":{{"on":{},"colour":{},"opacity":{},"angle":{},"distance":{},"blur":{}}},"glow":{{"on":{},"colour":{},"size":{},"opacity":{}}}}}"#,
            s.on,
            num(s.size),
            hex(s.colour),
            s.position.as_str(),
            num(s.opacity),
            d.on,
            hex(d.colour),
            num(d.opacity),
            num(d.angle),
            num(d.distance),
            num(d.blur),
            g.on,
            hex(g.colour),
            num(g.size),
            num(g.opacity),
        )
    }

    /// Stroke widths outside and inside the paint's edge.
    fn stroke_widths(&self) -> (f32, f32) {
        let s = &self.stroke;
        if !s.on {
            return (0.0, 0.0);
        }
        match s.position {
            StrokePosition::Outside => (s.size, 0.0),
            StrokePosition::Inside => (0.0, s.size),
            StrokePosition::Centre => (s.size / 2.0, s.size / 2.0),
        }
    }

    /// The shadow's offset in whole pixels.
    fn offset(&self) -> (i64, i64) {
        let a = self.shadow.angle.to_radians();
        (
            (-a.cos() * self.shadow.distance).round() as i64,
            (a.sin() * self.shadow.distance).round() as i64,
        )
    }

    /// How far (px) the style can reach from the paint (or from a change to it).
    pub fn reach(&self) -> i64 {
        let (out, inside) = self.stroke_widths();
        let stroke = if self.stroke.on {
            out.max(inside).ceil() as i64 + 2
        } else {
            0
        };
        let mut r = stroke;
        if self.shadow.on {
            let (ox, oy) = self.offset();
            r = r.max(stroke + ox.abs().max(oy.abs()) + spread(self.shadow.blur) + 1);
        }
        if self.glow.on {
            r = r.max(stroke + spread(self.glow.size) + 1);
        }
        r
    }

    /// [`Self::reach`] in whole tiles.
    pub fn reach_tiles(&self) -> i32 {
        (self.reach() + TS - 1).div_euclid(TS) as i32
    }
}

/// Box blur radius for a softness of `size` px (three passes ≈ Gaussian, as the
/// adjust blur).
fn box_radius(size: f32) -> usize {
    (size / 2.0).round().max(1.0) as usize
}

/// How far a blur of `size` spreads.
fn spread(size: f32) -> i64 {
    if size < 1.0 {
        0
    } else {
        3 * box_radius(size) as i64 + 1
    }
}

/// Three box blurs of a one-channel picture, in place.
#[allow(clippy::needless_range_loop)]
fn blur(buf: &mut [f32], w: usize, h: usize, size: f32) {
    if size < 1.0 || w == 0 || h == 0 {
        return;
    }
    let r = box_radius(size);
    let k = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0f32; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            let row = &mut buf[y * w..(y + 1) * w];
            let mut sum: f32 = row[..=r.min(w - 1)].iter().sum();
            for x in 0..w {
                tmp[x] = sum * k;
                if x + r + 1 < w {
                    sum += row[x + r + 1];
                }
                if x >= r {
                    sum -= row[x - r];
                }
            }
            row.copy_from_slice(&tmp[..w]);
        }
        for x in 0..w {
            let mut sum = 0.0;
            for y in 0..=r.min(h - 1) {
                sum += buf[y * w + x];
            }
            for y in 0..h {
                tmp[y] = sum * k;
                if y + r + 1 < h {
                    sum += buf[(y + r + 1) * w + x];
                }
                if y >= r {
                    sum -= buf[(y - r) * w + x];
                }
            }
            for y in 0..h {
                buf[y * w + x] = tmp[y];
            }
        }
    }
}

/// Distance from each pixel to the nearest `seed` pixel (0 on one; huge with
/// none): the exact Euclidean distance transform (Felzenszwalb & Huttenlocher),
/// a row pass and a column pass.
fn distance(seed: &[bool], w: usize, h: usize) -> Vec<f32> {
    const FAR: f32 = 1e20;
    let mut d: Vec<f32> = seed.iter().map(|&s| if s { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let (mut f, mut out) = (vec![0f32; n], vec![0f32; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0f32; n + 1]);
    // squared distance along one line, in place
    let mut line = |f: &mut [f32], out: &mut [f32]| {
        let n = f.len();
        let mut k = 0;
        v[0] = 0;
        z[0] = f32::NEG_INFINITY;
        z[1] = f32::INFINITY;
        for q in 1..n {
            let fq = f[q] + (q * q) as f32;
            let cross = |p: usize| (fq - (f[p] + (p * p) as f32)) / (2 * (q - p)) as f32;
            let mut s = cross(v[k]);
            while s <= z[k] {
                k -= 1;
                s = cross(v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
        }
        k = 0;
        for (q, o) in out.iter_mut().enumerate().take(n) {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            let dq = q as f32 - p as f32;
            *o = dq * dq + f[p];
        }
        f.copy_from_slice(&out[..n]);
    };
    for y in 0..h {
        let row = &mut d[y * w..(y + 1) * w];
        line(row, &mut out[..w]);
    }
    for x in 0..w {
        for y in 0..h {
            f[y] = d[y * w + x];
        }
        line(&mut f[..h], &mut out[..h]);
        for y in 0..h {
            d[y * w + x] = f[y];
        }
    }
    d.iter_mut().for_each(|v| *v = v.sqrt());
    d
}

/// A layer pixel with its mask applied (premultiplied, so every channel scales).
fn masked(
    colour: &TileMap,
    mask: Option<&TileMap>,
    key: TileKey,
    row: usize,
    col: usize,
) -> [u16; 4] {
    let Some(p) = colour.get(&key).map(|t| t[row][col]) else {
        return [0; 4];
    };
    match mask.and_then(|m| m.get(&key)) {
        Some(m) if p[3] > 0 => {
            let v = mask_value(m[row][col]);
            p.map(|c| (c as f32 * v).round() as u16)
        }
        _ => p,
    }
}

/// The layer as it shows with its style (mask applied first): the tiles in the
/// box (tx0, ty0)–(tx1, ty1), inclusive. Tiles left empty aren't in the result.
pub fn render(
    colour: &TileMap,
    mask: Option<&TileMap>,
    style: &LayerStyle,
    (tx0, ty0, tx1, ty1): (i32, i32, i32, i32),
) -> TileMap {
    if tx0 > tx1 || ty0 > ty1 {
        return TileMap::new();
    }
    // rows of tiles a few at a time: the padding stays small next to each band
    let pad = style.reach();
    let band = ((2 * pad + TS - 1) / TS).max(4) as i32;
    let bands: Vec<(i32, i32)> = (ty0..=ty1)
        .step_by(band as usize)
        .map(|y| (y, (y + band - 1).min(ty1)))
        .collect();
    let one = |&(b0, b1): &(i32, i32)| render_band(colour, mask, style, (tx0, b0, tx1, b1), pad);
    #[cfg(not(target_arch = "wasm32"))]
    let parts: Vec<TileMap> = {
        use rayon::prelude::*;
        bands.par_iter().map(one).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let parts: Vec<TileMap> = bands.iter().map(one).collect();
    parts.into_iter().flatten().collect()
}

/// The whole layer with its style, on a `width`×`height` canvas.
pub fn render_all(
    colour: &TileMap,
    mask: Option<&TileMap>,
    style: &LayerStyle,
    width: u32,
    height: u32,
) -> TileMap {
    if colour.is_empty() {
        return TileMap::new();
    }
    let r = style.reach_tiles();
    let (mut b0, mut b1) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
    for &(x, y) in colour.keys() {
        b0 = (b0.0.min(x), b0.1.min(y));
        b1 = (b1.0.max(x), b1.1.max(y));
    }
    let last = (
        width.div_ceil(TILE_SIZE as u32) as i32 - 1,
        height.div_ceil(TILE_SIZE as u32) as i32 - 1,
    );
    render(
        colour,
        mask,
        style,
        (
            (b0.0 - r).max(0),
            (b0.1 - r).max(0),
            (b1.0 + r).min(last.0),
            (b1.1 + r).min(last.1),
        ),
    )
}

#[allow(clippy::needless_range_loop)]
fn render_band(
    colour: &TileMap,
    mask: Option<&TileMap>,
    style: &LayerStyle,
    (tx0, ty0, tx1, ty1): (i32, i32, i32, i32),
    pad: i64,
) -> TileMap {
    // the band in canvas px, and the window around it the effects read from
    let (x0, y0) = (tx0 as i64 * TS, ty0 as i64 * TS);
    let (x1, y1) = ((tx1 as i64 + 1) * TS, (ty1 as i64 + 1) * TS);
    let (wx0, wy0) = (x0 - pad, y0 - pad);
    let (ww, wh) = ((x1 - x0 + 2 * pad) as usize, (y1 - y0 + 2 * pad) as usize);
    let mut alpha = vec![0f32; ww * wh];
    let mut any = false;
    for ty in (wy0.div_euclid(TS))..=((wy0 + wh as i64 - 1).div_euclid(TS)) {
        for tx in (wx0.div_euclid(TS))..=((wx0 + ww as i64 - 1).div_euclid(TS)) {
            let key = (tx as i32, ty as i32);
            if !colour.contains_key(&key) {
                continue;
            }
            for py in 0..TILE_SIZE {
                let y = ty * TS + py as i64 - wy0;
                if y < 0 || y >= wh as i64 {
                    continue;
                }
                for px in 0..TILE_SIZE {
                    let x = tx * TS + px as i64 - wx0;
                    if x < 0 || x >= ww as i64 {
                        continue;
                    }
                    let a = masked(colour, mask, key, py, px)[3] as f32 / 32768.0;
                    any |= a > 0.0;
                    alpha[y as usize * ww + x as usize] = a;
                }
            }
        }
    }
    if !any {
        return TileMap::new();
    }
    // the stroke: outside the edge (under the paint) and inside it (over the paint)
    let (out_w, in_w) = style.stroke_widths();
    // the edge: faint paint counts too (a light pencil line is outlined whole, not
    // just its darkest grains)
    let shape: Vec<bool> = alpha.iter().map(|&a| a >= 0.2).collect();
    let under: Option<Vec<f32>> = (out_w > 0.0).then(|| {
        distance(&shape, ww, wh)
            .into_iter()
            .map(|d| (out_w + 1.0 - d).clamp(0.0, 1.0) * style.stroke.opacity)
            .collect()
    });
    let over: Option<Vec<f32>> = (in_w > 0.0).then(|| {
        let outside: Vec<bool> = shape.iter().map(|s| !s).collect();
        distance(&outside, ww, wh)
            .into_iter()
            .zip(&alpha)
            .map(|(d, a)| (in_w + 1.0 - d).clamp(0.0, 1.0) * a * style.stroke.opacity)
            .collect()
    });
    // shadow and glow follow the paint with its outline
    let body = || -> Vec<f32> {
        match &under {
            Some(u) => alpha.iter().zip(u).map(|(a, u)| a.max(*u)).collect(),
            None => alpha.clone(),
        }
    };
    let shadow = style.shadow.on.then(|| {
        let mut b = body();
        blur(&mut b, ww, wh, style.shadow.blur);
        b
    });
    let glow = style.glow.on.then(|| {
        let mut b = body();
        blur(&mut b, ww, wh, style.glow.size);
        b.iter_mut()
            .for_each(|v| *v = (*v * 2.0).min(1.0) * style.glow.opacity);
        b
    });
    let (ox, oy) = style.offset();
    let at = |buf: &Option<Vec<f32>>, x: i64, y: i64| -> f32 {
        let Some(b) = buf else { return 0.0 };
        let (x, y) = (x - wx0, y - wy0);
        if x < 0 || y < 0 || x >= ww as i64 || y >= wh as i64 {
            0.0
        } else {
            b[y as usize * ww + x as usize]
        }
    };
    let mut out = TileMap::new();
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let key = (tx, ty);
            let mut t = empty_tile();
            let mut any = false;
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let (x, y) = (tx as i64 * TS + px as i64, ty as i64 * TS + py as i64);
                    let p = masked(colour, mask, key, py, px);
                    let ao = at(&over, x, y);
                    if p[3] >= 32768 && ao <= 0.0 {
                        // solid paint, nothing on it: as it was
                        t[py][px] = p;
                        any = true;
                        continue;
                    }
                    let a_shadow = at(&shadow, x - ox, y - oy) * style.shadow.opacity;
                    let a_glow = at(&glow, x, y);
                    let a_under = at(&under, x, y);
                    if a_shadow <= 0.0 && a_glow <= 0.0 && a_under <= 0.0 && ao <= 0.0 {
                        t[py][px] = p;
                        any |= p[3] > 0;
                        continue;
                    }
                    // bottom → top, each over what's below (straight sRGB colour)
                    let (mut c, mut a) = ([0f32; 3], 0f32);
                    let mut lay = |cs: [f32; 3], as_: f32| {
                        if as_ <= 0.0 {
                            return;
                        }
                        let ao = as_ + a * (1.0 - as_);
                        for i in 0..3 {
                            c[i] = (cs[i] * as_ + c[i] * a * (1.0 - as_)) / ao;
                        }
                        a = ao;
                    };
                    lay(style.shadow.colour, a_shadow);
                    lay(style.glow.colour, a_glow);
                    lay(style.stroke.colour, a_under);
                    let (pc, pa) = unpremul_srgb(p);
                    lay(pc, pa);
                    lay(style.stroke.colour, ao);
                    let v = premul_fix15(c, a);
                    any |= v[3] > 0;
                    t[py][px] = v;
                }
            }
            if any {
                out.insert(key, t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` block of `c` at (x0, y0), as tiles.
    fn block(x0: i64, y0: i64, w: i64, h: i64, c: [f32; 3]) -> TileMap {
        let mut m = TileMap::new();
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let key = (x.div_euclid(TS) as i32, y.div_euclid(TS) as i32);
                let t = m.entry(key).or_insert_with(empty_tile);
                t[y.rem_euclid(TS) as usize][x.rem_euclid(TS) as usize] = premul_fix15(c, 1.0);
            }
        }
        m
    }

    fn at(m: &TileMap, x: i64, y: i64) -> ([f32; 3], f32) {
        m.get(&(x.div_euclid(TS) as i32, y.div_euclid(TS) as i32))
            .map_or(([0.0; 3], 0.0), |t| {
                unpremul_srgb(t[y.rem_euclid(TS) as usize][x.rem_euclid(TS) as usize])
            })
    }

    const RED: [f32; 3] = [1.0, 0.0, 0.0];

    fn style() -> LayerStyle {
        LayerStyle::default()
    }

    #[test]
    fn distances_are_exact() {
        let (w, h) = (23, 17);
        let mut seed = vec![false; w * h];
        for &(x, y) in &[(3, 4), (20, 2), (11, 15)] {
            seed[y * w + x] = true;
        }
        let d = distance(&seed, w, h);
        for y in 0..h {
            for x in 0..w {
                let want = [(3.0, 4.0), (20.0, 2.0), (11.0, 15.0)]
                    .iter()
                    .map(|&(sx, sy): &(f32, f32)| {
                        ((x as f32 - sx).powi(2) + (y as f32 - sy).powi(2)).sqrt()
                    })
                    .fold(f32::MAX, f32::min);
                assert!((d[y * w + x] - want).abs() < 1e-3, "({x}, {y})");
            }
        }
    }

    #[test]
    fn no_effect_leaves_the_paint_as_it_was() {
        let paint = block(70, 70, 40, 30, RED);
        let out = render_all(&paint, None, &style(), 256, 256);
        assert_eq!(out, paint);
    }

    #[test]
    fn outside_stroke_rings_the_paint() {
        let paint = block(70, 70, 40, 40, RED);
        let mut s = style();
        s.stroke.on = true;
        s.stroke.size = 5.0;
        s.stroke.colour = [0.0, 0.0, 1.0];
        let out = render_all(&paint, None, &s, 256, 256);
        // just outside the left edge: blue, solid
        let (c, a) = at(&out, 67, 90);
        assert!(a > 0.99 && c[2] > 0.99 && c[0] < 0.01, "{c:?} {a}");
        // the paint stays red
        let (c, a) = at(&out, 90, 90);
        assert!(a > 0.99 && c[0] > 0.99, "{c:?}");
        // beyond the stroke: nothing
        assert_eq!(at(&out, 62, 90).1, 0.0);
        // inside: the edge turns blue, the middle stays red
        s.stroke.position = StrokePosition::Inside;
        let out = render_all(&paint, None, &s, 256, 256);
        assert!(at(&out, 72, 90).0[2] > 0.99);
        assert!(at(&out, 90, 90).0[0] > 0.99);
        assert_eq!(at(&out, 68, 90).1, 0.0, "nothing outside");
        // centre: both sides
        s.stroke.position = StrokePosition::Centre;
        let out = render_all(&paint, None, &s, 256, 256);
        assert!(at(&out, 68, 90).0[2] > 0.99 && at(&out, 71, 90).0[2] > 0.99);
        assert_eq!(at(&out, 64, 90).1, 0.0);
    }

    #[test]
    fn shadow_falls_away_from_the_light() {
        let paint = block(70, 70, 30, 30, RED);
        let mut s = style();
        s.shadow.on = true;
        s.shadow.blur = 0.0;
        s.shadow.distance = 10.0;
        s.shadow.angle = 90.0; // light from above: shadow straight down
        s.shadow.opacity = 1.0;
        let out = render_all(&paint, None, &s, 256, 256);
        let (c, a) = at(&out, 85, 105);
        assert!(a > 0.99 && c[0] < 0.01, "black under the block: {c:?} {a}");
        assert_eq!(at(&out, 85, 65).1, 0.0, "nothing above");
        assert!(at(&out, 85, 85).0[0] > 0.99, "paint over the shadow");
        // soft: fades out
        s.shadow.blur = 12.0;
        s.shadow.opacity = 0.5;
        let out = render_all(&paint, None, &s, 256, 256);
        let a = at(&out, 85, 112).1;
        assert!(a > 0.0 && a < 0.5, "{a}");
    }

    #[test]
    fn glow_surrounds_and_fades() {
        let paint = block(100, 100, 20, 20, RED);
        let mut s = style();
        s.glow.on = true;
        s.glow.size = 10.0;
        let out = render_all(&paint, None, &s, 256, 256);
        let near = at(&out, 97, 110).1;
        let far = at(&out, 90, 110).1;
        assert!(near > far && far > 0.0, "{near} {far}");
        assert_eq!(at(&out, 60, 110).1, 0.0);
    }

    #[test]
    fn mask_shapes_the_style() {
        let paint = block(64, 64, 64, 64, RED);
        // the mask hides the right half of the block (black paint)
        let mask = block(96, 64, 32, 64, [0.0; 3]);
        let mut s = style();
        s.stroke.on = true;
        s.stroke.size = 3.0;
        let out = render_all(&paint, Some(&mask), &s, 256, 256);
        assert!(at(&out, 97, 90).1 > 0.99, "stroke where the mask cuts");
        assert_eq!(at(&out, 110, 90).1, 0.0, "hidden part gone");
    }

    #[test]
    fn a_part_matches_the_whole() {
        let paint = block(30, 40, 90, 60, RED);
        let mut s = style();
        s.stroke.on = true;
        s.shadow.on = true;
        s.glow.on = true;
        let all = render_all(&paint, None, &s, 256, 256);
        let part = render(&paint, None, &s, (1, 1, 2, 2));
        // the same, give or take float rounding in the blurs
        for (k, t) in &part {
            let w = all.get(k).expect("tile in both");
            for (a, b) in t.iter().flatten().zip(w.iter().flatten()) {
                for c in 0..4 {
                    assert!((a[c] as i32 - b[c] as i32).abs() <= 8, "{k:?}: {a:?} {b:?}");
                }
            }
        }
        let inside = all
            .keys()
            .filter(|k| (1..=2).contains(&k.0) && (1..=2).contains(&k.1))
            .count();
        assert_eq!(part.len(), inside);
    }

    #[test]
    fn numbers_and_json() {
        let mut n = [0f32; NUMBERS];
        n[0] = 1.0;
        n[1] = 500.0;
        n[4] = 1.0;
        n[5] = 2.0;
        n[6] = 1.0;
        n[12] = -90.0;
        let s = LayerStyle::from_numbers(&n).unwrap();
        assert!(s.stroke.on && s.stroke.size == 100.0);
        assert_eq!(s.stroke.position, StrokePosition::Centre);
        assert_eq!(s.shadow.angle, 270.0);
        assert!(LayerStyle::from_numbers(&n[1..]).is_none());
        let j = s.json();
        assert!(
            j.contains(r##""colour":"#0000ff","position":"centre""##),
            "{j}"
        );
        assert!(!LayerStyle::default().any() && s.any());
        // the app's NO_STYLE (packages/paint/src/layer-style.ts) is this
        let d = LayerStyle::default().json();
        assert!(
            d.contains(r##""glow":{"on":false,"colour":"#ffed87","size":12"##),
            "{d}"
        );
    }
}
