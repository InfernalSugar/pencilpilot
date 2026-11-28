//! Drawings not showing, kept out of the painter and compressed (the painter keeps a
//! drawing at 8 bytes a pixel: a 4K cut's drawings all in it took 600 MB).
//!
//! Two kinds:
//! - as the project file has it: its tiles' compressed bytes, straight from the file (a
//!   drawing not changed since it was opened costs what it costs on disk, and comes back
//!   exactly as it was read);
//! - the painter's own pixels (premultiplied 15-bit), deflated: a drawing changed here,
//!   kept without rounding (going through RGBA8 would round faint colour a little each
//!   time).
//!
//! The painter holds the drawings showing, the onion skin's, and those its undo history
//! changes ([`super::trim`]); the rest come back when wanted ([`super::load_drawings`]).

use std::collections::HashMap;
use std::sync::Arc;

use brush::DecodedTile;
use rayon::prelude::*;

const RGBA_TILE: usize = 64 * 64 * 4;

/// A tile's column and row.
pub type Key = (i32, i32);

/// A tile as the project file stores it.
#[derive(Debug)]
pub struct FileTile {
    /// raw deflate (else stored)
    pub deflated: bool,
    /// (shared: the session's own copy of a tile from the wire is this one)
    pub bytes: Arc<[u8]>,
}

/// A drawing kept out of the painter.
#[derive(Debug)]
pub enum Kept {
    /// as the project file has it: each tile's column, row and hash (in [`Store::file`])
    File(Vec<(i32, i32, String)>),
    /// the painter's own pixels, each tile deflated
    Raw(Vec<(Key, Box<[u8]>)>),
    /// as they came from others (drawing together): straight RGBA8, each tile raw deflate
    /// (or stored), as the wire carries them
    Wire(Vec<(Key, Arc<FileTile>)>),
}

#[derive(Debug, Default)]
pub struct Store {
    /// drawings kept here: not in the painter, or in it unchanged since they came back
    pub drawings: HashMap<u32, Arc<Kept>>,
    /// tiles of drawings in the painter as they came from others (drawing together):
    /// straight RGBA8, compressed, exactly as sent; saving and passing them on uses these
    /// (the painter's own pixels would round them), until a tile is changed here
    pub exact: HashMap<u32, HashMap<Key, Arc<FileTile>>>,
    /// the project file's tiles, compressed, by hash
    pub file: HashMap<String, Arc<FileTile>>,
    /// where each drawing read from the file had its tiles (kept through changes: a save
    /// keeps the file's own bytes for a tile that didn't change)
    pub origin: HashMap<u32, Vec<(i32, i32, String)>>,
}

/// The threads drawings wanted now are decoded and packed on (a jump to a frame waits
/// for them): their own, not the shared pool the window composites with.
pub fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(4, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads((n / 2).clamp(2, 8))
            .thread_name(|i| format!("pp-drawings-{i}"))
            .build()
            .expect("the drawings' threads")
    })
}

/// The threads drawings are decoded ahead on: two, at a lower priority (the window's
/// drawing and the decoding wanted now go first).
pub fn ahead_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("pp-ahead-{i}"))
            .start_handler(|_| {
                // SAFETY: plain system calls on this thread's own id
                unsafe {
                    let tid = libc::gettid();
                    libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, 10);
                }
            })
            .build()
            .expect("the drawings' threads ahead")
    })
}

fn inflate(b: &[u8], limit: usize) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_with_limit(b, limit).ok()
}

/// A tile kept as the file (or the wire) has it, as RGBA8 (16 384 bytes).
pub fn file_rgba(t: &FileTile) -> Option<Vec<u8>> {
    let px = if t.deflated {
        inflate(&t.bytes, RGBA_TILE)?
    } else {
        t.bytes.to_vec()
    };
    (px.len() == RGBA_TILE).then_some(px)
}

/// A drawing to decode, with what it needs (for another thread).
pub struct Job {
    pub d: u32,
    kept: Arc<Kept>,
    file: HashMap<String, Arc<FileTile>>,
}

impl Job {
    /// What it decodes (to tell later whether the store still has that).
    pub fn kept(&self) -> &Arc<Kept> {
        &self.kept
    }

    /// Its tiles, ready for the painter (decoded on every core).
    pub fn decode(&self) -> Vec<((i32, i32), DecodedTile)> {
        match &*self.kept {
            Kept::File(refs) => refs
                .par_iter()
                .filter_map(|(x, y, h)| {
                    Some((
                        (*x, *y),
                        brush::decode_tile(&file_rgba(self.file.get(h)?)?)?,
                    ))
                })
                .collect(),
            Kept::Raw(tiles) => tiles
                .par_iter()
                .filter_map(|(k, b)| {
                    let raw = inflate(b, DecodedTile::BYTES)?;
                    Some((*k, DecodedTile::from_bytes(&raw)?))
                })
                .collect(),
            Kept::Wire(tiles) => tiles
                .par_iter()
                .filter_map(|(k, t)| Some((*k, brush::decode_tile(&file_rgba(t)?)?)))
                .collect(),
        }
    }

    /// Its tiles as straight RGBA8, compressed, when it's kept that way (as the file or the
    /// wire has them): they go to others as they are, nothing decoded or compressed again.
    pub fn rgba_tiles(&self) -> Option<Vec<(Key, Arc<FileTile>)>> {
        match &*self.kept {
            Kept::File(refs) => Some(
                refs.iter()
                    .filter_map(|(x, y, h)| Some(((*x, *y), self.file.get(h)?.clone())))
                    .collect(),
            ),
            Kept::Wire(tiles) => Some(tiles.clone()),
            Kept::Raw(_) => None,
        }
    }
}

impl Store {
    /// A file tile's RGBA (16 384 bytes).
    pub fn file_rgba(&self, hash: &str) -> Option<Vec<u8>> {
        file_rgba(self.file.get(hash)?)
    }

    /// A kept drawing's tiles, ready for the painter (decoded on every core).
    pub fn decode(&self, d: u32) -> Option<Vec<((i32, i32), DecodedTile)>> {
        Some(self.job(d)?.decode())
    }

    /// What decoding a drawing needs, to do it on another thread.
    pub fn job(&self, d: u32) -> Option<Job> {
        let kept = self.drawings.get(&d)?.clone();
        let file = match &*kept {
            Kept::File(refs) => refs
                .iter()
                .filter_map(|(_, _, h)| Some((h.clone(), self.file.get(h)?.clone())))
                .collect(),
            Kept::Raw(_) | Kept::Wire(_) => HashMap::new(),
        };
        Some(Job { d, kept, file })
    }

    /// The painter's tiles of a drawing, packed to keep.
    pub fn pack(tiles: Vec<((i32, i32), DecodedTile)>) -> Kept {
        Kept::Raw(
            tiles
                .into_par_iter()
                .map(|(k, t)| {
                    let b = miniz_oxide::deflate::compress_to_vec(&t.to_bytes(), 1);
                    (k, b.into_boxed_slice())
                })
                .collect(),
        )
    }

    /// How many tiles a kept drawing has (0: not kept here).
    pub fn tiles_of(&self, d: u32) -> usize {
        match self.drawings.get(&d).map(|k| &**k) {
            Some(Kept::File(r)) => r.len(),
            Some(Kept::Raw(t)) => t.len(),
            Some(Kept::Wire(t)) => t.len(),
            None => 0,
        }
    }

    /// Bytes held (compressed tiles), for checks.
    pub fn bytes(&self) -> usize {
        let raw: usize = self
            .drawings
            .values()
            .map(|k| match &**k {
                Kept::Raw(t) => t.iter().map(|(_, b)| b.len()).sum(),
                Kept::File(r) => r.len() * 48,
                Kept::Wire(t) => t.iter().map(|(_, b)| b.bytes.len()).sum(),
            })
            .sum();
        raw + self.file.values().map(|t| t.bytes.len()).sum::<usize>()
    }
}
