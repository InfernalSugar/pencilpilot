//! The animation frame store: every drawing (animation cel) as 64×64 tiles, each tile
//! kept once however many drawings use it (held frames, copies, unchanged corners).
//!
//! Tiles are the project file's colour tiles (straight-alpha sRGB RGBA8, 16 384 bytes,
//! named by the first 128 bits of their BLAKE3 hash; see `crates/ppstudio/FORMAT.md`), so
//! saving copies hashes, not pixels. Tiles stay in memory up to a budget; past it, the
//! least recently used go to a folder on disk and come back when asked for. Room is
//! made before tiles come in, so the tiles held never pass the budget (only a single
//! drawing bigger than the whole budget can, until it's put away).
//!
//! Drawings cross between the window and here packed ([`pack`] / [`unpack`]):
//!
//! ```text
//! u32 drawings, then per drawing: u32 id, u32 tiles, then per tile: i32 x, i32 y, 16 384 bytes
//! ```
//! (all little-endian; a drawing with no tiles is empty).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ppstudio::{Drawing, Project, TileRef};

/// Pixels a side.
pub const TILE: usize = 64;
/// Bytes in a colour tile.
pub const TILE_BYTES: usize = TILE * TILE * 4;

/// A tile's name: the first 128 bits of its BLAKE3 hash.
pub type Hash = [u8; 16];

pub fn hash(bytes: &[u8]) -> Hash {
    blake3::hash(bytes).as_bytes()[..16].try_into().unwrap()
}

/// The hash as the project file writes it (32 hex digits).
pub fn hex(h: &Hash) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Hash> {
    if s.len() != 32 {
        return None;
    }
    let mut h = [0u8; 16];
    for (i, b) in h.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(h)
}

/// One tile of a drawing: its column, row and pixels.
pub type TileData = (i32, i32, Arc<[u8]>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed {
    x: i32,
    y: i32,
    hash: Hash,
}

struct Entry {
    /// drawings using it (a drawing using it twice counts twice)
    refs: u32,
    /// the pixels, while in memory
    data: Option<Arc<[u8]>>,
    /// a copy is on disk
    on_disk: bool,
    /// when it was last used (for choosing what goes to disk)
    used: u64,
}

/// How much the store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub drawings: usize,
    /// different tiles
    pub tiles: usize,
    /// tiles used by drawings, counting repeats (what they'd take without sharing)
    pub placed: usize,
    pub bytes_in_memory: usize,
    pub bytes_on_disk: usize,
    /// the most tile bytes kept in memory
    pub budget: usize,
    /// the most tile bytes ever held in memory at once
    pub peak_in_memory: usize,
}

#[derive(Debug)]
pub enum Error {
    /// a tile that isn't 16 384 bytes, or a packet that doesn't add up
    Invalid(String),
    Io(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(s) => write!(f, "{s}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct FrameStore {
    drawings: HashMap<u32, Vec<Placed>>,
    tiles: HashMap<Hash, Entry>,
    /// bytes of tiles kept in memory before the oldest go to disk
    budget: usize,
    in_memory: usize,
    /// the most `in_memory` has been
    peak: usize,
    on_disk: usize,
    clock: u64,
    /// where tiles go past the budget (none: everything stays in memory)
    dir: Option<PathBuf>,
}

impl FrameStore {
    /// A store keeping up to `budget` bytes of tiles in memory; past that, tiles go
    /// to `dir` (made if missing; emptied when the store is dropped).
    pub fn new(budget: usize, dir: Option<PathBuf>) -> io::Result<Self> {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d)?;
        }
        Ok(FrameStore {
            drawings: HashMap::new(),
            tiles: HashMap::new(),
            budget,
            in_memory: 0,
            peak: 0,
            on_disk: 0,
            clock: 0,
            dir,
        })
    }

    pub fn stats(&self) -> Stats {
        Stats {
            drawings: self.drawings.len(),
            tiles: self.tiles.len(),
            placed: self.drawings.values().map(Vec::len).sum(),
            bytes_in_memory: self.in_memory,
            bytes_on_disk: self.on_disk,
            budget: self.budget,
            peak_in_memory: self.peak,
        }
    }

    /// Keep up to `budget` bytes of tiles in memory from now on (less than is held now:
    /// the oldest go to disk straight away).
    pub fn set_budget(&mut self, budget: usize) -> Result<()> {
        self.budget = budget;
        self.spill()
    }

    /// Tiles past the budget go to `dir` from now on; the ones already on disk move
    /// there (the old folder is removed).
    pub fn set_dir(&mut self, dir: PathBuf) -> Result<()> {
        if self.dir.as_deref() == Some(dir.as_path()) {
            return Ok(());
        }
        std::fs::create_dir_all(&dir)?;
        if let Some(old) = self.dir.take() {
            let mut result = Ok(());
            for (h, e) in &self.tiles {
                if e.on_disk && result.is_ok() {
                    result = move_file(&old.join(hex(h)), &dir.join(hex(h)));
                }
            }
            if let Err(e) = result {
                // stay where the tiles are
                self.dir = Some(old);
                return Err(e.into());
            }
            let _ = std::fs::remove_dir(&old);
        }
        self.dir = Some(dir);
        self.spill()
    }

    /// Where tiles past the budget go.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.drawings.contains_key(&id)
    }

    /// Every drawing's id.
    pub fn ids(&self) -> Vec<u32> {
        self.drawings.keys().copied().collect()
    }

    /// Keep a drawing's tiles (replacing what it had). Fully transparent tiles are left
    /// out; a drawing with none is empty.
    pub fn put<'a>(
        &mut self,
        id: u32,
        tiles: impl IntoIterator<Item = (i32, i32, &'a [u8])>,
    ) -> Result<()> {
        let mut placed = Vec::new();
        let mut new_tiles = Vec::new();
        for (x, y, bytes) in tiles {
            if bytes.len() != TILE_BYTES {
                return Err(Error::Invalid(format!(
                    "drawing {id}: a tile of {} bytes",
                    bytes.len()
                )));
            }
            if bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                continue;
            }
            let h = hash(bytes);
            placed.push(Placed { x, y, hash: h });
            new_tiles.push((h, bytes));
        }
        // room first, so the tiles held never pass the budget
        let fresh: HashSet<Hash> = new_tiles
            .iter()
            .map(|(h, _)| *h)
            .filter(|h| !self.tiles.contains_key(h))
            .collect();
        self.make_room(fresh.len() * TILE_BYTES)?;
        // count the new references before dropping the old ones (shared tiles stay)
        for (h, bytes) in new_tiles {
            self.add_ref(h, || Arc::from(bytes));
        }
        if let Some(old) = self.drawings.insert(id, placed) {
            self.release(&old);
        }
        self.spill()
    }

    fn add_ref(&mut self, h: Hash, data: impl FnOnce() -> Arc<[u8]>) {
        self.clock += 1;
        let clock = self.clock;
        match self.tiles.get_mut(&h) {
            Some(e) => {
                e.refs += 1;
                e.used = clock;
            }
            None => {
                self.in_memory += TILE_BYTES;
                self.peak = self.peak.max(self.in_memory);
                self.tiles.insert(
                    h,
                    Entry {
                        refs: 1,
                        data: Some(data()),
                        on_disk: false,
                        used: clock,
                    },
                );
            }
        }
    }

    fn release(&mut self, placed: &[Placed]) {
        for p in placed {
            let Some(e) = self.tiles.get_mut(&p.hash) else {
                continue;
            };
            e.refs -= 1;
            if e.refs == 0 {
                let e = self.tiles.remove(&p.hash).unwrap();
                if e.data.is_some() {
                    self.in_memory -= TILE_BYTES;
                }
                if e.on_disk {
                    self.on_disk -= TILE_BYTES;
                    if let Some(path) = self.path(&p.hash) {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }

    fn path(&self, h: &Hash) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(hex(h)))
    }

    /// Past the budget: the least recently used tiles go to disk until it's 90% full.
    fn spill(&mut self) -> Result<()> {
        if self.in_memory <= self.budget {
            return Ok(());
        }
        self.spill_to(self.budget / 10 * 9)
    }

    /// Make room for `bytes` more without passing the budget.
    fn make_room(&mut self, bytes: usize) -> Result<()> {
        if self.in_memory + bytes <= self.budget {
            return Ok(());
        }
        self.spill_to((self.budget / 10 * 9).saturating_sub(bytes))
    }

    /// The least recently used tiles go to disk until `target` bytes are left in memory.
    fn spill_to(&mut self, target: usize) -> Result<()> {
        if self.in_memory <= target || self.dir.is_none() {
            return Ok(());
        }
        let mut held: Vec<(u64, Hash)> = self
            .tiles
            .iter()
            .filter(|(_, e)| e.data.is_some())
            .map(|(h, e)| (e.used, *h))
            .collect();
        held.sort_unstable();
        for (_, h) in held {
            if self.in_memory <= target {
                break;
            }
            let path = self.path(&h).unwrap();
            let e = self.tiles.get_mut(&h).unwrap();
            if !e.on_disk {
                std::fs::write(&path, e.data.as_deref().unwrap())?;
                e.on_disk = true;
                self.on_disk += TILE_BYTES;
            }
            e.data = None;
            self.in_memory -= TILE_BYTES;
        }
        Ok(())
    }

    /// A tile's pixels (from disk if it went there).
    fn tile(&mut self, h: &Hash) -> Result<Arc<[u8]>> {
        self.clock += 1;
        let clock = self.clock;
        let path = self.path(h);
        let e = self
            .tiles
            .get_mut(h)
            .ok_or_else(|| Error::Invalid(format!("no tile {}", hex(h))))?;
        e.used = clock;
        if let Some(d) = &e.data {
            return Ok(d.clone());
        }
        let path = path.ok_or_else(|| Error::Invalid("a tile went missing".into()))?;
        let bytes = std::fs::read(path)?;
        if bytes.len() != TILE_BYTES || hash(&bytes) != *h {
            return Err(Error::Invalid(format!(
                "tile {} is damaged on disk",
                hex(h)
            )));
        }
        let data: Arc<[u8]> = Arc::from(bytes);
        e.data = Some(data.clone());
        self.in_memory += TILE_BYTES;
        self.peak = self.peak.max(self.in_memory);
        Ok(data)
    }

    /// These tiles are wanted now: they go to disk last, and room is made for the ones
    /// coming back from disk.
    fn wanted(&mut self, placed: &[Placed]) -> Result<()> {
        self.clock += 1;
        let mut away = HashSet::new();
        for p in placed {
            if let Some(e) = self.tiles.get_mut(&p.hash) {
                e.used = self.clock;
                if e.data.is_none() {
                    away.insert(p.hash);
                }
            }
        }
        self.make_room(away.len() * TILE_BYTES)
    }

    /// A drawing's tiles (none: no such drawing). Tiles on disk come back to memory.
    pub fn get(&mut self, id: u32) -> Result<Option<Vec<TileData>>> {
        let Some(placed) = self.drawings.get(&id).cloned() else {
            return Ok(None);
        };
        self.wanted(&placed)?;
        let mut out = Vec::with_capacity(placed.len());
        for p in placed {
            out.push((p.x, p.y, self.tile(&p.hash)?));
        }
        self.spill()?;
        Ok(Some(out))
    }

    /// `to` becomes a copy of `from` (sharing every tile). False if `from` is unknown.
    pub fn copy(&mut self, from: u32, to: u32) -> bool {
        let Some(placed) = self.drawings.get(&from).cloned() else {
            return false;
        };
        for p in &placed {
            self.tiles.get_mut(&p.hash).unwrap().refs += 1;
        }
        if let Some(old) = self.drawings.insert(to, placed) {
            self.release(&old);
        }
        true
    }

    /// `a` and `b` trade their tiles (one that doesn't exist: the other becomes
    /// missing). Nothing is copied: a canvas size change keeps every drawing's other
    /// size under a second id and swaps the two on undo and redo.
    pub fn swap(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let (x, y) = (self.drawings.remove(&a), self.drawings.remove(&b));
        if let Some(y) = y {
            self.drawings.insert(a, y);
        }
        if let Some(x) = x {
            self.drawings.insert(b, x);
        }
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(old) = self.drawings.remove(&id) {
            self.release(&old);
        }
    }

    /// Forget every drawing not in `keep`.
    pub fn retain(&mut self, keep: &HashSet<u32>) {
        let gone: Vec<u32> = self
            .drawings
            .keys()
            .filter(|id| !keep.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.remove(id);
        }
    }

    /// Put a drawing into a project as `name` (its tiles under their hashes). Its layer's
    /// mask, if any, comes from `mask`.
    pub fn write_into(
        &mut self,
        p: &mut Project,
        id: u32,
        name: &str,
        mask: Option<Vec<TileRef>>,
    ) -> Result<()> {
        let placed = self
            .drawings
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("no drawing {id}")))?;
        self.wanted(&placed)?;
        let mut refs = Vec::with_capacity(placed.len());
        for t in placed {
            if t.x < 0 || t.y < 0 {
                continue; // (off the canvas: the file has no room for it)
            }
            let name = hex(&t.hash);
            if !p.tiles.contains_key(&name) {
                p.tiles.insert(name.clone(), self.tile(&t.hash)?.to_vec());
            }
            refs.push(TileRef {
                x: t.x as u32,
                y: t.y as u32,
                hash: name,
            });
        }
        p.manifest
            .drawings
            .insert(name.into(), Drawing { tiles: refs, mask });
        self.spill()
    }

    /// Take a project's drawing `name` in as `id`.
    pub fn read_from(&mut self, p: &Project, name: &str, id: u32) -> Result<()> {
        let d = p
            .manifest
            .drawings
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("no drawing {name}")))?;
        let mut tiles = Vec::with_capacity(d.tiles.len());
        for r in &d.tiles {
            let bytes = p
                .tiles
                .get(&r.hash)
                .ok_or_else(|| Error::Invalid(format!("missing tile {}", r.hash)))?;
            if from_hex(&r.hash).is_none() {
                return Err(Error::Invalid(format!("tile name {}", r.hash)));
            }
            tiles.push((r.x as i32, r.y as i32, bytes.as_slice()));
        }
        self.put(id, tiles)
    }
}

/// Move a file, copying it when the new place is on another disk.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

impl Drop for FrameStore {
    fn drop(&mut self) {
        if let Some(d) = &self.dir {
            for (h, e) in &self.tiles {
                if e.on_disk {
                    let _ = std::fs::remove_file(d.join(hex(h)));
                }
            }
            let _ = std::fs::remove_dir(d);
        }
    }
}

// ---- packets ----

/// Drawings packed for crossing to the window.
pub fn pack(drawings: &[(u32, Vec<TileData>)]) -> Vec<u8> {
    let size: usize = drawings
        .iter()
        .map(|(_, t)| 8 + t.len() * (8 + TILE_BYTES))
        .sum();
    let mut out = Vec::with_capacity(4 + size);
    out.extend_from_slice(&(drawings.len() as u32).to_le_bytes());
    for (id, tiles) in drawings {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(tiles.len() as u32).to_le_bytes());
        for (x, y, px) in tiles {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.extend_from_slice(px);
        }
    }
    out
}

/// One drawing in a packet: its id and tiles (borrowed from the packet).
pub type Unpacked<'a> = (u32, Vec<(i32, i32, &'a [u8])>);

/// Drawings from the window.
pub fn unpack(b: &[u8]) -> Result<Vec<Unpacked<'_>>> {
    let bad = || Error::Invalid("the drawings packet doesn't add up".into());
    let u32_at = |at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            b.get(at..at + 4).ok_or_else(bad)?.try_into().unwrap(),
        ))
    };
    let n = u32_at(0)? as usize;
    let mut at = 4;
    let mut out = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let id = u32_at(at)?;
        let count = u32_at(at + 4)? as usize;
        at += 8;
        let mut tiles = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            let x = u32_at(at)? as i32;
            let y = u32_at(at + 4)? as i32;
            let px = b.get(at + 8..at + 8 + TILE_BYTES).ok_or_else(bad)?;
            tiles.push((x, y, px));
            at += 8 + TILE_BYTES;
        }
        out.push((id, tiles));
    }
    if at != b.len() {
        return Err(bad());
    }
    Ok(out)
}
