//! Drawings' small pictures for the strips, made from the painter's tiles (only the
//! painted ones are visited, so a sparse drawing costs little).

use brush::Painter;

use super::sheet::Thumb;

/// A thumbnail's size: the document's 16:9, small.
pub const TW: u32 = 52;
pub const TH: u32 = 29;
const TILE: i64 = 64;

/// A drawing as a `TW` × `TH` picture (straight RGBA8; transparent where it's empty). Each
/// pixel averages its area, its coverage boosted so thin lines still show.
pub fn of(p: &Painter, drawing: u32, showing_on: Option<u32>) -> Thumb {
    // showing on a layer: the painter's own thumbnail (it samples, and only painted tiles)
    if let Some(layer) = showing_on {
        return Thumb {
            w: TW,
            h: TH,
            px: p.layer_thumbnail(layer, TW, TH),
        };
    }
    let (w, h) = (p.width() as i64, p.height() as i64);
    let n = (TW * TH) as usize;
    let mut sa = vec![0f32; n];
    let mut sc = vec![[0f32; 3]; n];
    let packed = p.drawing_tiles_packed(drawing);
    let count = packed
        .get(..4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) as usize;
    let tile_bytes = (TILE * TILE * 4) as usize;
    // every 2nd pixel each way is enough for a picture this small
    let step = 2usize;
    for k in 0..count {
        let at = 4 + k * (8 + tile_bytes);
        let Some(head) = packed.get(at..at + 8) else {
            break;
        };
        let tx = i32::from_le_bytes([head[0], head[1], head[2], head[3]]) as i64;
        let ty = i32::from_le_bytes([head[4], head[5], head[6], head[7]]) as i64;
        let px = &packed[at + 8..at + 8 + tile_bytes];
        for py in (0..TILE as usize).step_by(step) {
            let y = ty * TILE + py as i64;
            if y < 0 || y >= h {
                continue;
            }
            let row = (y * TH as i64 / h) as usize * TW as usize;
            for pxi in (0..TILE as usize).step_by(step) {
                let x = tx * TILE + pxi as i64;
                if x < 0 || x >= w {
                    continue;
                }
                let o = (py * TILE as usize + pxi) * 4;
                let a = px[o + 3] as f32 / 255.0;
                if a <= 0.0 {
                    continue;
                }
                let i = row + (x * TW as i64 / w) as usize;
                sa[i] += a;
                for c in 0..3 {
                    sc[i][c] += px[o + c] as f32 / 255.0 * a;
                }
            }
        }
    }
    let per = (w as f32 / TW as f32 / step as f32) * (h as f32 / TH as f32 / step as f32);
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        if sa[i] <= 0.0 {
            continue;
        }
        for c in 0..3 {
            out[i * 4 + c] = (sc[i][c] / sa[i] * 255.0).round() as u8;
        }
        out[i * 4 + 3] = ((sa[i] / per.max(1.0) * 6.0).min(1.0) * 255.0).round() as u8;
    }
    Thumb {
        w: TW,
        h: TH,
        px: out,
    }
}
