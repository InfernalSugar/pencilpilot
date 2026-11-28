//! The canvas as `pencilpilot_together`'s [`Together`](pencilpilot_together::Together) sees
//! it, from the session's thread: each call is asked of the window's thread (which answers
//! between frames, [`super::serve`]) and waited for here, never there.
//!
//! What's heavy stays on this thread: a drawing's tiles are encoded to the wire's RGBA here
//! from the painter's own (copied out by the window), and tiles put in are decoded here, only
//! those that changed (each drawing's tiles are known by a hash), and kept compressed
//! exactly as they came (saving and passing them on uses those bytes).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use pencilpilot_together::{
    CanvasPort, GroupSettings, LayerPatch, PortGroup, PortLayer, TileMap, Timeline, pack, unpack,
};

use crate::shell::engine::frames::store::{FileTile, Job};

/// A tile as kept exactly (straight RGBA8, raw deflate).
pub type Exact = Arc<FileTile>;

/// The session's own threads for tiles (decoding, encoding, hashing): few, and behind the
/// window's in the queue for the processor (the pen's work, the painter's on rayon's
/// shared threads, never waits for a remote picture's tiles).
pub fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(2, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads((n / 2).clamp(1, 4))
            .thread_name(|i| format!("together-tiles-{i}"))
            .start_handler(|_| behind())
            .build()
            .expect("the session's tile threads")
    })
}

/// This thread goes behind the window's for the processor (a little: nice 5).
pub fn behind() {
    // SAFETY: setpriority on the calling thread (who 0 is this thread on Linux)
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 5);
    }
}

/// A tile as it came over the wire (raw deflate), and where.
type Wire = ((i32, i32), Arc<[u8]>);
/// A tile from the wire decoded for the painter, and as it came (None: nothing in it).
type Decoded = ((i32, i32), Option<(brush::DecodedTile, Exact)>);

/// What a remote edit's tiles were, for `--stats` (who, when it came, the sender's time).
#[derive(Debug, Clone, Default)]
pub struct Stamp {
    pub kind: &'static str,
    pub from: String,
    pub received_ns: u64,
    pub sent_ns: Option<u64>,
}

/// A question to the window's thread.
pub enum Req {
    AddLayer(String, Option<String>),
    SetAdjust(u32, String),
    MaskPacked(u32),
    PutMask(u32, Vec<u8>),
    SetLayer(u32, LayerPatch),
    DrawingOf(u32),
    NewDrawing,
    LayerOfDrawing(u32),
    /// a drawing's tiles: the painter's copy, or its kept copy (and tiles kept exactly)
    Packed(u32),
    /// tiles that changed (decoded here, and as they came), and those cleared
    Put {
        d: u32,
        set: Vec<((i32, i32), brush::DecodedTile, Exact)>,
        clear: Vec<(i32, i32)>,
        stamp: Option<Stamp>,
    },
    Select(u32),
    RemoveLayer(u32),
    Place(u32, u32, bool, u32),
    AddGroup(u32),
    SetGroup(u32, GroupSettings),
    Ungroup(u32),
    MergeDown(u32),
    MergeGroup(u32),
    Groups,
    Layers,
    Timeline,
    SetTimeline(Timeline),
    DrawingsOfLayer(u32),
    /// joining: a new, empty project (of this size; else this one's), its layers put aside
    NewDoc(Option<(u32, u32)>),
    /// the project's size and the frame showing (a native host's snapshot says them)
    Doc,
    /// a sound file the session sent (kept to save with the project)
    PutSound(String, Arc<Vec<u8>>),
}

/// A drawing's tiles as the window has them.
pub enum Pixels {
    /// copied out of the painter
    Painter(Vec<((i32, i32), brush::DecodedTile)>),
    /// kept compressed (decoded here)
    Kept(Job),
    None,
}

pub enum Resp {
    Unit,
    U32(u32),
    Bool(bool),
    Bytes(Vec<u8>),
    OptU32(Option<u32>),
    Pixels(Pixels, Vec<((i32, i32), Exact)>),
    Groups(Vec<PortGroup>),
    Layers(Vec<PortLayer>),
    Timeline(Timeline),
    Ids(Vec<u32>),
    Doc(u32, u32, u32),
}

/// A question and where its answer goes.
/// (and when it was asked, ns: for `--stats`)
pub struct Ask(pub Req, pub mpsc::Sender<Resp>, pub u64);

/// The port: asks the window, keeps what's needed to send only what changed.
pub struct RemotePort {
    ask: Box<dyn Fn(Ask) -> bool + Send>,
    /// each drawing's tiles as last put in or read (a hash each)
    hashes: HashMap<u32, HashMap<(i32, i32), u64>>,
    /// …and as last put in as they came over the wire (a hash of their bytes)
    zhashes: HashMap<u32, HashMap<(i32, i32), u64>>,
    /// sound files the session has (from others)
    sounds: HashMap<String, (Arc<Vec<u8>>, String)>,
    /// the remote edit being put in (for `--stats`)
    pub stamp: Option<Stamp>,
    /// bytes of tiles decoded here since memory was last given back, and when the last came
    pub untrimmed: usize,
    pub put_at: Option<std::time::Instant>,
    /// calls and their waits counted here (for `--stats`)
    pub bytes: Option<Arc<super::net::Bytes>>,
    /// the window's thread has gone (leaving): every answer is "no"
    gone: std::cell::Cell<bool>,
}

fn hash(px: &[u8]) -> u64 {
    let h = blake3::hash(px);
    u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap())
}

impl RemotePort {
    pub fn new(ask: impl Fn(Ask) -> bool + Send + 'static) -> RemotePort {
        RemotePort {
            ask: Box::new(ask),
            hashes: HashMap::new(),
            zhashes: HashMap::new(),
            sounds: HashMap::new(),
            stamp: None,
            bytes: None,
            untrimmed: 0,
            put_at: None,
            gone: std::cell::Cell::new(false),
        }
    }

    fn call(&self, r: Req) -> Option<Resp> {
        if self.gone.get() {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        if !(self.ask)(Ask(r, tx, pencilpilot_window::clock::now_ns())) {
            self.gone.set(true);
            return None;
        }
        // (waiting is fine here: this is the session's thread, never the window's)
        let t = std::time::Instant::now();
        let got = tokio::task::block_in_place(|| rx.recv().ok());
        if let Some(b) = &self.bytes {
            use std::sync::atomic::Ordering::Relaxed;
            b.asks.fetch_add(1, Relaxed);
            b.ask_wait_ns
                .fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
        }
        if got.is_none() {
            self.gone.set(true);
        }
        got
    }

    fn u32(&mut self, r: Req) -> u32 {
        match self.call(r) {
            Some(Resp::U32(v)) => v,
            _ => 0,
        }
    }

    fn bool(&mut self, r: Req) -> bool {
        matches!(self.call(r), Some(Resp::Bool(true)))
    }
}

/// A tile as RGBA8 (None: nothing in it).
fn rgba(t: &brush::DecodedTile) -> Option<Vec<u8>> {
    let mut px = vec![0u8; pencilpilot_together::TILE_BYTES];
    brush::encode_tile(t, &mut px);
    px.chunks(4).any(|p| p[3] > 0).then_some(px)
}

/// RGBA8 as it's kept exactly (raw deflate, fast).
pub fn exact(px: &[u8]) -> Exact {
    Arc::new(FileTile {
        deflated: true,
        bytes: miniz_oxide::deflate::compress_to_vec(px, 1).into(),
    })
}

impl CanvasPort for RemotePort {
    fn add_layer(&mut self, name: &str, adjust: Option<&str>) -> u32 {
        self.u32(Req::AddLayer(name.into(), adjust.map(str::to_string)))
    }
    fn set_adjust(&mut self, id: u32, spec: &str) -> bool {
        self.bool(Req::SetAdjust(id, spec.into()))
    }
    fn mask_packed(&mut self, id: u32) -> Vec<u8> {
        match self.call(Req::MaskPacked(id)) {
            Some(Resp::Bytes(b)) => b,
            _ => Vec::new(),
        }
    }
    fn put_mask(&mut self, id: u32, packed: &[u8]) -> bool {
        self.bool(Req::PutMask(id, packed.to_vec()))
    }
    fn set_layer(&mut self, id: u32, patch: LayerPatch) {
        self.call(Req::SetLayer(id, patch));
    }
    fn drawing_of(&mut self, id: u32) -> u32 {
        self.u32(Req::DrawingOf(id))
    }
    fn new_drawing(&mut self) -> u32 {
        self.u32(Req::NewDrawing)
    }
    fn layer_of_drawing(&self, drawing: u32) -> Option<u32> {
        // (the trait asks through `&self`: a call that changes nothing here)
        match self.call(Req::LayerOfDrawing(drawing)) {
            Some(Resp::OptU32(v)) => v,
            _ => None,
        }
    }
    fn packed(&mut self, drawing: u32) -> Vec<u8> {
        use rayon::prelude::*;
        let Some(Resp::Pixels(px, exact)) = self.call(Req::Packed(drawing)) else {
            return pack(&TileMap::new());
        };
        let tiles: Vec<((i32, i32), brush::DecodedTile)> = match px {
            Pixels::Painter(t) => t,
            Pixels::Kept(job) => {
                // kept as the file or the wire has them: those bytes, nothing rounded
                if let Some(w) = job.rgba_tiles() {
                    let mut m: Vec<((i32, i32), Vec<u8>)> = pool().install(|| {
                        w.par_iter()
                            .filter_map(|(k, t)| {
                                Some((*k, crate::shell::engine::frames::store::file_rgba(t)?))
                            })
                            .collect()
                    });
                    m.sort_by_key(|(k, _)| (k.1, k.0));
                    return self.packed_from(drawing, m, &exact);
                }
                pool().install(|| job.decode())
            }
            Pixels::None => Vec::new(),
        };
        let mut m: Vec<((i32, i32), Vec<u8>)> = pool().install(|| {
            tiles
                .par_iter()
                .filter_map(|(k, t)| Some((*k, rgba(t)?)))
                .collect()
        });
        m.sort_by_key(|(k, _)| (k.1, k.0));
        self.packed_from(drawing, m, &exact)
    }
    fn put(&mut self, drawing: u32, packed: &[u8]) -> bool {
        use rayon::prelude::*;
        let m = unpack(packed);
        let was = self.hashes.get(&drawing).cloned().unwrap_or_default();
        let list: Vec<(&(i32, i32), &Vec<u8>)> = m.iter().collect();
        let now: HashMap<(i32, i32), u64> =
            pool().install(|| list.par_iter().map(|(k, px)| (**k, hash(px))).collect());
        let changed: Vec<(&(i32, i32), &Vec<u8>)> =
            m.iter().filter(|(k, _)| was.get(k) != now.get(k)).collect();
        let set: Vec<((i32, i32), brush::DecodedTile, Exact)> = pool().install(|| {
            changed
                .par_iter()
                .filter_map(|(k, px)| Some((**k, brush::decode_tile(px)?, exact(px))))
                .collect()
        });
        let mut clear: Vec<(i32, i32)> = self
            .known(drawing)
            .filter(|k| !now.contains_key(k))
            .collect();
        // (a tile with nothing in it is cleared)
        for (k, _) in &changed {
            if !set.iter().any(|(s, _, _)| s == *k) {
                clear.push(**k);
            }
        }
        self.untrimmed += set.len() * (64 * 64 * 8 + 64 * 64 * 4);
        self.put_at = Some(std::time::Instant::now());
        if set.is_empty() && clear.is_empty() {
            return true;
        }
        let stamp = self.stamp.clone();
        let ok = self.bool(Req::Put {
            d: drawing,
            set,
            clear,
            stamp,
        });
        if ok {
            self.hashes.insert(drawing, now);
            self.zhashes.remove(&drawing);
        }
        ok
    }
    fn put_deflated(&mut self, drawing: u32, tiles: &[((i32, i32), Arc<[u8]>)]) -> Option<bool> {
        use rayon::prelude::*;
        // (as they came over the wire: kept as they are, inflated and decoded here only
        // where they changed; known by a hash of their bytes as they came)
        let was = self.zhashes.get(&drawing).cloned().unwrap_or_default();
        let now: HashMap<(i32, i32), u64> =
            pool().install(|| tiles.par_iter().map(|(k, b)| (*k, hash(b))).collect());
        let changed: Vec<&Wire> = tiles
            .iter()
            .filter(|(k, _)| was.get(k) != now.get(k))
            .collect();
        let decoded: Vec<Decoded> = pool().install(|| {
            changed
                .par_iter()
                .map(|(k, b)| {
                    let px = miniz_oxide::inflate::decompress_to_vec_with_limit(
                        b,
                        pencilpilot_together::TILE_BYTES,
                    )
                    .ok()
                    .filter(|p| p.len() == pencilpilot_together::TILE_BYTES);
                    let t = px.and_then(|p| brush::decode_tile(&p)).map(|t| {
                        (
                            t,
                            Arc::new(FileTile {
                                deflated: true,
                                bytes: Arc::clone(b),
                            }),
                        )
                    });
                    (*k, t)
                })
                .collect()
        });
        let mut clear: Vec<(i32, i32)> = self
            .known(drawing)
            .filter(|k| !now.contains_key(k))
            .collect();
        let mut set = Vec::with_capacity(decoded.len());
        for (k, t) in decoded {
            match t {
                Some((t, x)) => set.push((k, t, x)),
                None => clear.push(k),
            }
        }
        self.untrimmed += set.len() * (64 * 64 * 8 + 64 * 64 * 4);
        self.put_at = Some(std::time::Instant::now());
        if !set.is_empty() || !clear.is_empty() {
            let stamp = self.stamp.clone();
            if !self.bool(Req::Put {
                d: drawing,
                set,
                clear,
                stamp,
            }) {
                return Some(false);
            }
        }
        self.zhashes.insert(drawing, now);
        // (the pixels' hashes no longer say what's there)
        self.hashes.remove(&drawing);
        Some(true)
    }
    fn select(&mut self, id: u32) {
        self.call(Req::Select(id));
    }
    fn remove_layer(&mut self, id: u32) -> bool {
        self.bool(Req::RemoveLayer(id))
    }
    fn place(&mut self, id: u32, reference: u32, above: bool, group: u32) -> bool {
        self.bool(Req::Place(id, reference, above, group))
    }
    fn add_group(&mut self, layer: u32) -> u32 {
        self.u32(Req::AddGroup(layer))
    }
    fn set_group(&mut self, id: u32, s: &GroupSettings) {
        self.call(Req::SetGroup(id, s.clone()));
    }
    fn ungroup(&mut self, id: u32) -> bool {
        self.bool(Req::Ungroup(id))
    }
    fn merge_down(&mut self, id: u32) -> bool {
        self.bool(Req::MergeDown(id))
    }
    fn merge_group(&mut self, id: u32) -> u32 {
        self.u32(Req::MergeGroup(id))
    }
    fn groups(&self) -> Vec<PortGroup> {
        match self.call(Req::Groups) {
            Some(Resp::Groups(v)) => v,
            _ => Vec::new(),
        }
    }
    fn layers(&self) -> Vec<PortLayer> {
        match self.call(Req::Layers) {
            Some(Resp::Layers(v)) => v,
            _ => Vec::new(),
        }
    }
    fn sound(&self, file: &str) -> Option<(Arc<Vec<u8>>, String)> {
        self.sounds.get(file).cloned()
    }
    fn put_sound(&mut self, file: &str, bytes: Vec<u8>, name: &str) {
        let b = Arc::new(bytes);
        self.call(Req::PutSound(file.into(), b.clone()));
        self.sounds.insert(file.into(), (b, name.into()));
    }
    fn timeline(&self) -> Timeline {
        match self.call(Req::Timeline) {
            Some(Resp::Timeline(t)) => t,
            _ => Timeline::default(),
        }
    }
    fn set_timeline(&mut self, tl: Timeline) {
        self.call(Req::SetTimeline(tl));
    }
    fn drawings_of_layer(&self, id: u32) -> Vec<u32> {
        match self.call(Req::DrawingsOfLayer(id)) {
            Some(Resp::Ids(v)) => v,
            _ => Vec::new(),
        }
    }
}

impl RemotePort {
    /// Joining: a new, empty project in place of the one open (of this size).
    pub fn new_doc(&mut self, size: Option<(u32, u32)>) {
        self.hashes.clear();
        self.zhashes.clear();
        self.call(Req::NewDoc(size));
    }

    /// The project's size and the frame showing.
    pub fn doc(&self) -> Option<(u32, u32, u32)> {
        match self.call(Req::Doc) {
            Some(Resp::Doc(w, h, f)) => Some((w, h, f)),
            _ => None,
        }
    }

    /// Packed from RGBA tiles, those kept exactly in their place; the hashes noted.
    /// The tiles last known in a drawing, put in either way.
    fn known(&self, drawing: u32) -> impl Iterator<Item = (i32, i32)> + '_ {
        let a = self.hashes.get(&drawing).into_iter().flat_map(|m| m.keys());
        let b = self
            .zhashes
            .get(&drawing)
            .into_iter()
            .flat_map(|m| m.keys());
        let mut all: Vec<(i32, i32)> = a.chain(b).copied().collect();
        all.sort_unstable();
        all.dedup();
        all.into_iter()
    }

    fn packed_from(
        &mut self,
        drawing: u32,
        mut m: Vec<((i32, i32), Vec<u8>)>,
        exact: &[((i32, i32), Exact)],
    ) -> Vec<u8> {
        for (k, t) in exact {
            if let Some(px) = crate::shell::engine::frames::store::file_rgba(t) {
                match m.iter_mut().find(|(x, _)| x == k) {
                    Some(e) => e.1 = px,
                    None => m.push((*k, px)),
                }
            }
        }
        let map: TileMap = m.into_iter().collect();
        self.hashes
            .insert(drawing, map.iter().map(|(k, px)| (*k, hash(px))).collect());
        pack(&map)
    }
}
