//! The onion skin's picture: the drawings before and after, tinted, laid over each other on
//! transparency, and the canvas's picture with it multiplied underneath.
//!
//! Made tile by tile (only where those drawings have paint) on every core, from drawings
//! unpacked once and kept until they change, so going to another frame costs little.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use brush::Painter;
use rayon::prelude::*;

const TILE: usize = 64;
type Key = (i32, i32);
/// A drawing's painted tiles, straight-alpha RGBA8, 64 × 64 each.
type Tiles = Arc<HashMap<Key, Box<[u8]>>>;

#[derive(Debug, Default)]
pub struct Onion {
    /// drawings unpacked, by id
    cache: HashMap<u32, Tiles>,
    /// the onion skin (straight RGBA8, document size)
    pub px: Vec<u8>,
    /// the tiles it has paint in
    touched: Vec<Key>,
}

fn unpack(packed: &[u8]) -> HashMap<Key, Box<[u8]>> {
    let count = packed
        .get(..4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) as usize;
    let bytes = TILE * TILE * 4;
    let mut out = HashMap::with_capacity(count);
    for k in 0..count {
        let at = 4 + k * (8 + bytes);
        let Some(head) = packed.get(at..at + 8) else {
            break;
        };
        let tx = i32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let ty = i32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        if let Some(px) = packed.get(at + 8..at + 8 + bytes) {
            out.insert((tx, ty), px.into());
        }
    }
    out
}

impl Onion {
    /// A drawing changed: unpack it again when it's next needed.
    pub fn forget(&mut self, drawing: u32) {
        self.cache.remove(&drawing);
    }

    fn tiles(&mut self, p: &Painter, drawing: u32) -> Tiles {
        self.cache
            .entry(drawing)
            .or_insert_with(|| Arc::new(unpack(&p.drawing_tiles_packed(drawing))))
            .clone()
    }

    /// The onion skin for `entries` ((drawing, colour 0xRRGGBB, opacity 0–255) triples,
    /// farthest first, each laid over the ones before). Returns where it changed (x, y, w,
    /// h), if anywhere.
    pub fn build(&mut self, p: &Painter, entries: &[u32]) -> Option<[u32; 4]> {
        let (w, h) = (p.width() as usize, p.height() as usize);
        if self.px.len() != w * h * 4 {
            self.px = vec![0; w * h * 4];
            self.touched.clear();
        }
        // the last one's tiles go
        for &(tx, ty) in &self.touched {
            for_rows(w, h, tx, ty, |row, x0, x1| {
                self.px[(row * w + x0) * 4..(row * w + x1) * 4].fill(0);
            });
        }
        let layers: Vec<(Tiles, [f32; 3], f32)> = entries
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[d, c, o]| {
                let tint = [
                    ((c >> 16) & 255) as f32 / 255.0,
                    ((c >> 8) & 255) as f32 / 255.0,
                    (c & 255) as f32 / 255.0,
                ];
                (self.tiles(p, d), tint, o.min(255) as f32 / 255.0)
            })
            .collect();
        let keys: Vec<Key> = layers
            .iter()
            .flat_map(|(t, ..)| t.keys().copied())
            .collect::<HashSet<Key>>()
            .into_iter()
            .filter(|&(tx, ty)| {
                tx >= 0 && ty >= 0 && (tx as usize) * TILE < w && (ty as usize) * TILE < h
            })
            .collect();
        let done: Vec<(Key, Vec<u8>)> = keys
            .par_iter()
            .map(|&k| {
                let mut rgb = vec![[0f32; 3]; TILE * TILE];
                let mut alpha = vec![0f32; TILE * TILE];
                for (tiles, tint, op) in &layers {
                    let Some(src) = tiles.get(&k) else { continue };
                    for i in 0..TILE * TILE {
                        let sa = src[i * 4 + 3];
                        if sa == 0 {
                            continue;
                        }
                        let a = sa as f32 / 255.0 * op;
                        let keep = alpha[i] * (1.0 - a);
                        let out = a + keep;
                        for c in 0..3 {
                            rgb[i][c] = (tint[c] * a + rgb[i][c] * keep) / out.max(1e-6);
                        }
                        alpha[i] = out;
                    }
                }
                let mut px = vec![0u8; TILE * TILE * 4];
                for i in 0..TILE * TILE {
                    if alpha[i] > 0.0 {
                        for c in 0..3 {
                            px[i * 4 + c] = (rgb[i][c] * 255.0).round() as u8;
                        }
                        px[i * 4 + 3] = (alpha[i] * 255.0).round() as u8;
                    }
                }
                (k, px)
            })
            .collect();
        for ((tx, ty), px) in &done {
            for_rows(w, h, *tx, *ty, |row, x0, x1| {
                let r = row - *ty as usize * TILE;
                let n = x1 - x0;
                self.px[(row * w + x0) * 4..(row * w + x1) * 4]
                    .copy_from_slice(&px[r * TILE * 4..(r * TILE + n) * 4]);
            });
        }
        let before = std::mem::replace(&mut self.touched, keys);
        bounds(before.iter().chain(&self.touched), w, h)
    }
}

/// The rectangle around these tiles, clipped to the canvas.
fn bounds<'a>(keys: impl Iterator<Item = &'a Key>, w: usize, h: usize) -> Option<[u32; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for &(tx, ty) in keys {
        let (x, y) = (tx.max(0) as usize * TILE, ty.max(0) as usize * TILE);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max((x + TILE).min(w));
        y1 = y1.max((y + TILE).min(h));
    }
    (x0 < x1 && y0 < y1).then(|| [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

/// The rectangle around two.
pub fn union(a: Option<[u32; 4]>, b: Option<[u32; 4]>) -> Option<[u32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
            let (x1, y1) = (
                (a[0] + a[2]).max(b[0] + b[2]),
                (a[1] + a[3]).max(b[1] + b[3]),
            );
            Some([x0, y0, x1 - x0, y1 - y0])
        }
        (a, None) => a,
        (None, b) => b,
    }
}

/// Each canvas row of tile (tx, ty): (row, first x, end x), clipped to the canvas.
fn for_rows(w: usize, h: usize, tx: i32, ty: i32, mut f: impl FnMut(usize, usize, usize)) {
    let (x0, y0) = (tx as usize * TILE, ty as usize * TILE);
    let x1 = (x0 + TILE).min(w);
    for row in y0..(y0 + TILE).min(h) {
        f(row, x0, x1);
    }
}

/// `out` = the picture with the onion skin multiplied under it, in `r` (rows on every core).
pub fn compose(out: &mut [u8], pic: &[u8], onion: &[u8], w: u32, r: [u32; 4]) {
    let [x, y, rw, rh] = r;
    let stride = (w * 4) as usize;
    let (a, b) = ((x * 4) as usize, ((x + rw) * 4) as usize);
    let rows = (y as usize)..(y + rh) as usize;
    out[rows.start * stride..rows.end * stride]
        .par_chunks_mut(stride)
        .zip(pic[rows.start * stride..rows.end * stride].par_chunks(stride))
        .zip(onion[rows.start * stride..rows.end * stride].par_chunks(stride))
        .for_each(|((o, p), n)| {
            for ((o, p), n) in o[a..b]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(p[a..b].as_chunks::<4>().0)
                .zip(n[a..b].as_chunks::<4>().0)
            {
                let k = n[3] as u32;
                if k == 0 {
                    *o = *p;
                    continue;
                }
                for c in 0..3 {
                    // p × (1 − k + k·tint), in 0–255 integers
                    let m = 255 * 255 - k * (255 - n[c] as u32);
                    o[c] = ((p[c] as u32 * m + 255 * 255 / 2) / (255 * 255)) as u8;
                }
                o[3] = 255;
            }
        });
}
