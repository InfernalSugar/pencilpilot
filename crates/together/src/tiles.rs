//! Pixels as they travel: 64×64 tiles of straight RGBA8 (as in `.ppstudio`), the
//! engine's packed form, the difference between two drawings, and one tile on the wire
//! (deflate-raw, base64: "z:…"; or as is: "r:…").

use std::io::{Read, Write};

use crate::js::{from_base64, to_base64};

/// Tile side, in pixels.
pub const TILE: usize = 64;
/// Bytes in a tile: 64×64 RGBA8.
pub const TILE_BYTES: usize = TILE * TILE * 4;

/// A tile's place: column, row (tile coordinates; negative ones are off the canvas's
/// top left).
pub type TileKey = (i32, i32);

/// A drawing's tiles: place → 64×64 straight RGBA8 ([`TILE_BYTES`] bytes), in the order
/// they came (as together.ts's `Map`: a tile put again keeps its place, a new one goes
/// last; take tiles out with `shift_remove` to keep the others' order).
pub type TileMap = indexmap::IndexMap<TileKey, Vec<u8>>;

/// `unpack`: from the engine's packed form: u32 count; per tile i32 x, i32 y, 64×64
/// RGBA8 (little-endian). A short last tile is left out.
pub fn unpack(packed: &[u8]) -> TileMap {
    let mut m = TileMap::new();
    if packed.len() < 4 {
        return m;
    }
    let n = u32::from_le_bytes(packed[..4].try_into().unwrap()) as usize;
    for i in 0..n {
        let at = 4 + i * (8 + TILE_BYTES);
        if at + 8 + TILE_BYTES > packed.len() {
            break;
        }
        let x = i32::from_le_bytes(packed[at..at + 4].try_into().unwrap());
        let y = i32::from_le_bytes(packed[at + 4..at + 8].try_into().unwrap());
        m.insert((x, y), packed[at + 8..at + 8 + TILE_BYTES].to_vec());
    }
    m
}

/// `pack`: the engine's packed form of tiles.
pub fn pack(m: &TileMap) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + m.len() * (8 + TILE_BYTES));
    out.extend_from_slice(&(m.len() as u32).to_le_bytes());
    for ((x, y), px) in m {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(px);
    }
    out
}

/// What changed from one drawing to the next: tiles to set, tiles to clear.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TileDiff {
    pub set: Vec<(i32, i32, Vec<u8>)>,
    pub clear: Vec<TileKey>,
}

impl TileDiff {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.clear.is_empty()
    }
}

/// `diffTiles`: what changed from `prev` to `next`.
pub fn diff_tiles(prev: &TileMap, next: &TileMap) -> TileDiff {
    let mut d = TileDiff::default();
    for (k, px) in next {
        if prev.get(k) != Some(px) {
            d.set.push((k.0, k.1, px.clone()));
        }
    }
    for k in prev.keys() {
        if !next.contains_key(k) {
            d.clear.push(*k);
        }
    }
    d
}

/// `encodeTile`: a tile for the wire, deflated ("z:" + base64 of deflate-raw at zlib's
/// default level; the same bytes as the studio's).
pub fn encode_tile(px: &[u8]) -> String {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(px).unwrap();
    format!("z:{}", to_base64(&e.finish().unwrap()))
}

/// `decodeTile`: a tile from the wire ("z:" deflated or "r:" as is); None unless it's
/// exactly one tile.
pub fn decode_tile(s: &str) -> Option<Vec<u8>> {
    let raw = if let Some(z) = s.strip_prefix("z:") {
        let bytes = from_base64(z)?;
        let mut out = Vec::with_capacity(TILE_BYTES);
        // (no more than a tile and a byte: anything longer isn't one)
        flate2::read::DeflateDecoder::new(&bytes[..])
            .take(TILE_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .ok()?;
        out
    } else {
        from_base64(s.strip_prefix("r:")?)?
    };
    (raw.len() == TILE_BYTES).then_some(raw)
}
