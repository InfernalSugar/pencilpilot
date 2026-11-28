//! Each shared drawing's tiles as last synced, kept small: a tile is its hash (to tell
//! whether the canvas still has it) and, where it has them, its deflated bytes (a tile
//! from the wire keeps the bytes it came in; one this app sent keeps what it sent). A
//! tile this app never sent (a host's project as it was when it started) is only its hash:
//! its pixels are on this app's own canvas, and are read from there when a joiner needs
//! them.
//!
//! (together.ts keeps every tile's pixels: 16 KiB each, 232 MiB for 20 layers of 4K.)

use std::io::Read;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::js::from_base64;
use crate::tiles::{TILE_BYTES, TileKey, TileMap};

/// A tile's pixels, hashed (BLAKE3, the first 16 bytes).
pub type TileHash = [u8; 16];

pub fn hash(px: &[u8]) -> TileHash {
    blake3::hash(px).as_bytes()[..16].try_into().unwrap()
}

/// One synced tile.
#[derive(Debug, Clone, Default)]
pub struct Tile {
    /// its pixels' hash (none yet: a tile from the wire not looked at yet)
    hash: Option<TileHash>,
    /// its deflated pixels (raw deflate, as "z:" tiles carry them)
    z: Option<Arc<[u8]>>,
    /// came in, but isn't a tile (its bytes don't inflate to one)
    bad: bool,
}

/// Inflate deflated bytes to exactly one tile (None: they aren't one).
pub fn inflate(z: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(TILE_BYTES);
    flate2::read::DeflateDecoder::new(z)
        .take(TILE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() == TILE_BYTES).then_some(out)
}

impl Tile {
    /// A tile on this app's canvas (not sent yet): its hash.
    pub fn local(px: &[u8]) -> Tile {
        Tile {
            hash: Some(hash(px)),
            ..Tile::default()
        }
    }

    /// A tile this app sent: its hash and the bytes it sent.
    pub fn sent(px: &[u8], z: Arc<[u8]>) -> Tile {
        Tile {
            hash: Some(hash(px)),
            z: Some(z),
            bad: false,
        }
    }

    /// A tile from the wire ("z:" or "r:"); None when it can't be one. Its bytes are
    /// looked at when it's needed (a "z:" tile that doesn't inflate to a tile then counts
    /// as none).
    pub fn wire(s: &str) -> Option<Tile> {
        if let Some(z) = s.strip_prefix("z:") {
            let z = from_base64(z)?;
            return Some(Tile {
                hash: None,
                z: Some(z.into()),
                bad: false,
            });
        }
        let raw = from_base64(s.strip_prefix("r:")?)?;
        (raw.len() == TILE_BYTES).then(|| Tile::sent(&raw, deflate(&raw).into()))
    }

    /// Its hash (inflating it the first time, for a tile from the wire); None: not a tile.
    pub fn hash(&mut self) -> Option<TileHash> {
        if self.hash.is_none() && !self.bad {
            match self.z.as_deref().and_then(inflate) {
                Some(px) => self.hash = Some(hash(&px)),
                None => self.bad = true,
            }
        }
        self.hash
    }

    /// Its deflated bytes, where it has them.
    pub fn deflated(&self) -> Option<&Arc<[u8]>> {
        self.z.as_ref().filter(|_| !self.bad)
    }

    /// Its pixels, where it has them (inflated); None for one only on the canvas, or bad.
    pub fn pixels(&self) -> Option<Vec<u8>> {
        if self.bad {
            return None;
        }
        self.z.as_deref().and_then(inflate)
    }

    /// Bytes it takes here.
    pub fn bytes(&self) -> usize {
        16 + self.z.as_ref().map_or(0, |z| z.len())
    }

    fn with_z(&mut self, z: Arc<[u8]>) {
        self.z = Some(z);
    }
}

/// Raw deflate at zlib's default level (the wire's).
pub fn deflate(px: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(px).unwrap();
    e.finish().unwrap()
}

/// A drawing's synced tiles, in the order they came (as the studio's `Map`).
#[derive(Debug, Clone, Default)]
pub struct Tiles(pub(crate) IndexMap<TileKey, Tile>);

/// What changed on the canvas since the tiles were synced: the tiles to send (their
/// pixels) and the places cleared.
pub struct Changes {
    pub set: Vec<(i32, i32, Vec<u8>)>,
    pub clear: Vec<TileKey>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.clear.is_empty()
    }
}

impl Tiles {
    /// The canvas's tiles, as not sent yet.
    pub fn local(m: &TileMap) -> Tiles {
        Tiles(m.iter().map(|(k, px)| (*k, Tile::local(px))).collect())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.0.values().map(Tile::bytes).sum()
    }

    pub fn remove(&mut self, k: &TileKey) {
        self.0.shift_remove(k);
    }

    pub fn put(&mut self, k: TileKey, t: Tile) {
        self.0.insert(k, t);
    }

    /// `diffTiles`: what changed from these to `next` (the canvas now).
    pub fn changes(&mut self, next: &TileMap) -> Changes {
        let mut set = Vec::new();
        for (k, px) in next {
            let same = self.0.get_mut(k).and_then(Tile::hash) == Some(hash(px));
            if !same {
                set.push((k.0, k.1, px.clone()));
            }
        }
        let clear = self
            .0
            .keys()
            .filter(|k| !next.contains_key(*k))
            .copied()
            .collect();
        Changes { set, clear }
    }

    /// After sending `c` (each tile with the bytes it went as): these are the canvas's.
    pub fn sent(&mut self, c: &Changes, wire: &[Arc<[u8]>]) {
        for k in &c.clear {
            self.0.shift_remove(k);
        }
        for ((x, y, px), z) in c.set.iter().zip(wire) {
            self.0.insert((*x, *y), Tile::sent(px, z.clone()));
        }
    }

    /// The tiles as they go to a joiner: each one's deflated bytes, made from `canvas`
    /// (this app's own copy, read only if some tile has none) where it has none yet, and
    /// kept. A tile only on the canvas that the canvas no longer has as it was goes as the
    /// canvas has it now (this app's change, sent to everyone soon after), not kept.
    pub fn for_wire(&mut self, canvas: &mut dyn FnMut() -> TileMap) -> Vec<(TileKey, Arc<[u8]>)> {
        let mut here: Option<TileMap> = None;
        let mut out = Vec::with_capacity(self.0.len());
        for (k, t) in self.0.iter_mut() {
            if let Some(z) = t.deflated() {
                out.push((*k, z.clone()));
                continue;
            }
            if t.bad {
                continue;
            }
            let c = here.get_or_insert_with(&mut *canvas);
            let Some(px) = c.get(k) else {
                continue;
            };
            let z: Arc<[u8]> = deflate(px).into();
            if t.hash == Some(hash(px)) {
                t.with_z(z.clone());
            }
            out.push((*k, z));
        }
        out
    }

    /// The tiles' pixels, for the canvas: inflated, or (a tile only on the canvas) as the
    /// canvas has it.
    pub fn pixels(&self, canvas: &mut dyn FnMut() -> TileMap) -> TileMap {
        let mut here: Option<TileMap> = None;
        let mut out = TileMap::with_capacity(self.0.len());
        for (k, t) in &self.0 {
            if let Some(px) = t.pixels() {
                out.insert(*k, px);
            } else if t.z.is_none() && !t.bad {
                let c = here.get_or_insert_with(&mut *canvas);
                if let Some(px) = c.get(k) {
                    out.insert(*k, px.clone());
                }
            }
        }
        out
    }

    /// Every tile's deflated bytes, when each has them (a drawing that came over the wire).
    pub fn all_deflated(&self) -> Option<Vec<(TileKey, Arc<[u8]>)>> {
        self.0
            .iter()
            .map(|(k, t)| t.deflated().map(|z| (*k, z.clone())))
            .collect()
    }
}
