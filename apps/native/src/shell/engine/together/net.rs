//! The live session's own thread: `crates/share`'s [`Session`] on a small runtime of its
//! own, so nothing on the window's thread ever waits on the network.
//!
//! - **In:** the session's events, in order; tiles in edits (and in the project sent to a
//!   joiner) are decoded here (base64, inflate, the painter's own pixels) before they reach
//!   the window, which only puts them in between frames.
//! - **Out:** the window's commands (edits, direct messages, cursors); tiles it sends go as
//!   raw pixels and are compressed here, in edits of at most [`TILES_PER_EDIT`].
//! - **Waking:** each event wakes the window ([`pencilpilot_window::Waker`]), so it neither
//!   polls nor sleeps past it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use pencilpilot_share::{Event, Identity, Invite, Member, Relays, Session};
use serde_json::{Value, json};

/// Tiles per edit (an edit stays around a megabyte at most), as the current app sends them.
pub const TILES_PER_EDIT: usize = 48;
/// A tile's pixels: 64 × 64 straight RGBA8.
pub const TILE_BYTES: usize = 64 * 64 * 4;

/// How to start.
#[derive(Debug, Clone)]
pub enum Start {
    Host,
    Join(String),
}

/// The session as the window knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// the code to share (pp1-…)
    pub invite: String,
    pub me: Member,
    /// in the order they came (the host first)
    pub members: Vec<Member>,
    pub host: bool,
}

/// Tiles of one drawing, decoded on this thread.
pub struct Tiles {
    pub lid: String,
    pub did: String,
    pub set: Vec<((i32, i32), brush::DecodedTile)>,
    pub clear: Vec<(i32, i32)>,
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tiles({} {} set {} clear {})",
            self.lid,
            self.did,
            self.set.len(),
            self.clear.len()
        )
    }
}

/// A drawing's tiles as they came (straight RGBA8, raw deflate or stored): a joiner keeps
/// the project's drawings so, compressed, until they're shown.
pub struct WireTiles {
    pub lid: String,
    pub did: String,
    pub tiles: Vec<((i32, i32), Arc<FileTile>)>,
}

impl std::fmt::Debug for WireTiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WireTiles({} {} {})",
            self.lid,
            self.did,
            self.tiles.len()
        )
    }
}

use crate::shell::engine::frames::store::FileTile;

/// What reaches the window.
#[derive(Debug)]
pub enum In {
    Started(Info),
    Failed(String),
    /// edit `seq` by `from` (tiles in it decoded); when it came, ns (CLOCK_MONOTONIC)
    Edit {
        seq: u64,
        from: String,
        body: Value,
        tiles: Option<Tiles>,
        at_ns: u64,
    },
    Presence {
        from: String,
        body: Value,
        at_ns: u64,
    },
    Direct {
        from: String,
        body: Value,
        /// a snapshot's tiles ("snap.tiles"), as they came
        wire: Option<WireTiles>,
        at_ns: u64,
    },
    Joined(Member),
    Left(String),
    /// the host left and `host` took over (`you`: this app); who's here; the new code
    HostChanged {
        host: String,
        you: bool,
        members: Vec<Member>,
        invite: String,
    },
    Closed(String),
}

/// What the window asks.
pub enum Cmd {
    Edit(Value),
    Direct(String, Value),
    Presence(Value),
    /// a drawing's tiles (the painter's own pixels) as "tiles" edits, or to one member
    /// (`to`) as "snap.tiles": encoded and compressed here. `t_ns`: when the change was
    /// made (checks only: `--stats` puts it on the wire)
    Tiles {
        to: Option<String>,
        lid: String,
        did: String,
        set: Vec<((i32, i32), brush::DecodedTile)>,
        clear: Vec<(i32, i32)>,
        t_ns: Option<u64>,
    },
    /// a drawing kept compressed (not in the painter): decoded here, then as [`Cmd::Tiles`]
    Kept {
        to: Option<String>,
        lid: String,
        did: String,
        job: crate::shell::engine::frames::store::Job,
    },
    Leave,
}

/// Bytes through the session (the edits' and messages' JSON, both ways), for `--stats`.
#[derive(Debug, Default)]
pub struct Bytes {
    pub sent: AtomicU64,
    pub received: AtomicU64,
}

/// The session's thread, from the window's side.
pub struct Net {
    tx: tokio::sync::mpsc::UnboundedSender<Cmd>,
    rx: mpsc::Receiver<In>,
    pub bytes: Arc<Bytes>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Where this computer's key is kept: the current app's place (the same computer is the
/// same member in either app), or `PENCILPILOT_SHARE_DIR` (checks).
pub fn share_dir() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("PENCILPILOT_SHARE_DIR") {
        return Some(d.into());
    }
    // (Tauri's app data dir on Linux: $XDG_DATA_HOME, else ~/.local/share, then the id)
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share"))
        })?;
    Some(base.join("app.pencilpilot.studio").join("share"))
}

/// This computer's key (`PENCILPILOT_SHARE_PROFILE` picks another, to try two apps on one
/// computer), as the current app keeps it.
pub fn identity() -> Result<Identity, String> {
    let file = match std::env::var("PENCILPILOT_SHARE_PROFILE") {
        Ok(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("key-{p}")
        }
        _ => "key".into(),
    };
    let dir = share_dir().ok_or("There's no place to keep this computer's key.")?;
    Identity::load_or_create(&dir.join(file)).map_err(|e| e.to_string())
}

/// The relays: `PENCILPILOT_RELAY` (comma-separated URLs, or `off`: same network only),
/// else iroh's public ones, as the current app with no relay set. Checks always run with
/// none (`offline`: loopback only, never a public relay).
pub fn relays(offline: bool) -> Relays {
    if offline {
        return Relays::Off;
    }
    match std::env::var("PENCILPILOT_RELAY") {
        Ok(v) if v.trim() == "off" => Relays::Off,
        Ok(v) if !v.trim().is_empty() => {
            Relays::Custom(v.split(',').map(|s| s.trim().to_string()).collect())
        }
        _ => Relays::Iroh,
    }
}

/// The invite code in pasted text (one unbroken token; anything around it is dropped).
pub fn clean_invite(text: &str) -> Option<String> {
    let i = text.find("pp1-")?;
    let code: String = text[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (code.len() > 4).then_some(code)
}

impl Net {
    /// Hosts or joins on a new thread; [`In::Started`] or [`In::Failed`] comes first.
    pub fn start(
        start: Start,
        id: Identity,
        name: String,
        relays: Relays,
        waker: Option<pencilpilot_window::Waker>,
    ) -> Net {
        let (tx, crx) = tokio::sync::mpsc::unbounded_channel();
        let (itx, rx) = mpsc::channel();
        let bytes = Arc::new(Bytes::default());
        let b = bytes.clone();
        let thread = std::thread::Builder::new()
            .name("together".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("together-io")
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = itx.send(In::Failed(e.to_string()));
                        return;
                    }
                };
                let out = Out { tx: itx, waker };
                rt.block_on(run(start, id, name, relays, crx, out, b));
            })
            .ok();
        Net {
            tx,
            rx,
            bytes,
            thread,
        }
    }

    pub fn send(&self, c: Cmd) -> bool {
        self.tx.send(c).is_ok()
    }

    /// What's come (never waits).
    pub fn try_recv(&self) -> Option<In> {
        self.rx.try_recv().ok()
    }

    /// Leaves (the thread closes the session; waits for it at most `wait`).
    pub fn leave(mut self, wait: std::time::Duration) {
        let _ = self.tx.send(Cmd::Leave);
        if let Some(t) = self.thread.take() {
            let until = std::time::Instant::now() + wait;
            while !t.is_finished() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

struct Out {
    tx: mpsc::Sender<In>,
    waker: Option<pencilpilot_window::Waker>,
}

impl Out {
    fn send(&self, i: In) -> bool {
        let ok = self.tx.send(i).is_ok();
        if let Some(w) = &self.waker {
            w.wake();
        }
        ok
    }
}

fn info_of(s: &Session) -> Info {
    Info {
        invite: s.invite().encode(),
        me: s.me().clone(),
        members: s.members(),
        host: s.is_host(),
    }
}

fn now_ns() -> u64 {
    pencilpilot_window::clock::now_ns()
}

async fn run(
    start: Start,
    id: Identity,
    name: String,
    relays: Relays,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    out: Out,
    bytes: Arc<Bytes>,
) {
    let s = match start {
        Start::Host => Session::host(&id, &name, relays).await,
        Start::Join(code) => match Invite::decode(&code) {
            Ok(inv) => Session::join(&id, &inv, &name, relays).await,
            Err(e) => Err(e),
        },
    };
    let mut s = match s {
        Ok(s) => s,
        Err(e) => {
            out.send(In::Failed(e.to_string()));
            return;
        }
    };
    if !out.send(In::Started(info_of(&s))) {
        s.close().await;
        return;
    }
    let size = |v: &Value| v.to_string().len() as u64;
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Edit(body)) => {
                    bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                    if let Err(e) = s.edit(body) {
                        out.send(In::Closed(e.to_string()));
                        break;
                    }
                }
                Some(Cmd::Direct(to, body)) => {
                    bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                    let _ = s.direct(&to, body);
                }
                Some(Cmd::Presence(body)) => s.presence(body),
                Some(c @ (Cmd::Tiles { .. } | Cmd::Kept { .. })) => {
                    let (to, bodies, t_ns) = tokio::task::block_in_place(|| tile_bodies(c));
                    let mut ended = false;
                    for mut body in bodies {
                        if let Some(t) = t_ns {
                            body["t"] = json!(t);
                        }
                        bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                        let r = match &to {
                            Some(m) => s.direct(m, body),
                            None => s.edit(body),
                        };
                        if r.is_err() && to.is_none() {
                            ended = true;
                            break;
                        }
                    }
                    if ended {
                        out.send(In::Closed("The session has ended.".into()));
                        break;
                    }
                }
                Some(Cmd::Leave) | None => break,
            },
            ev = s.next_event() => {
                let Some(e) = ev else { break };
                let at_ns = now_ns();
                let closed = matches!(e, Event::Closed { .. });
                let i = match e {
                    Event::Edit { seq, from, body } => {
                        bytes.received.fetch_add(size(&body), Ordering::Relaxed);
                        let tiles = decode_tiles(&body, "tiles");
                        In::Edit { seq, from, body: strip_tiles(body, &tiles), tiles, at_ns }
                    }
                    Event::Direct { from, body } => {
                        bytes.received.fetch_add(size(&body), Ordering::Relaxed);
                        // (the project for a joiner: its tiles kept as they came, compressed)
                        let wire = tokio::task::block_in_place(|| wire_tiles(&body));
                        let body = if wire.is_some() { strip(body) } else { body };
                        In::Direct { from, body, wire, at_ns }
                    }
                    Event::Presence { from, body } => In::Presence { from, body, at_ns },
                    Event::Joined(m) => In::Joined(m),
                    Event::Left { id } => In::Left(id),
                    Event::HostChanged { host, members } => In::HostChanged {
                        you: host == s.me().id,
                        host,
                        members,
                        invite: s.invite().encode(),
                    },
                    Event::Closed { reason } => In::Closed(reason),
                };
                if !out.send(i) || closed {
                    break;
                }
            }
        }
    }
    s.close().await;
}

/// A tiles edit's pixels aren't needed again once decoded (the window gets them as tiles).
fn strip_tiles(mut body: Value, tiles: &Option<Tiles>) -> Value {
    if tiles.is_some()
        && let Some(o) = body.as_object_mut()
    {
        // (the pixels went to the tiles: the edit is checked without them)
        o.insert("set".into(), Value::Array(Vec::new()));
    }
    body
}

/// The edits a [`Cmd::Tiles`] or [`Cmd::Kept`] goes as (to one member, or everyone), and
/// its time.
fn tile_bodies(c: Cmd) -> (Option<String>, Vec<Value>, Option<u64>) {
    match c {
        Cmd::Tiles {
            to,
            lid,
            did,
            set,
            clear,
            t_ns,
        } => {
            let (rgba, blank) = to_rgba(set);
            let mut clear = clear;
            clear.extend(blank);
            let b = tile_edits(to.is_some(), &lid, &did, rgba, clear);
            (to, b, t_ns)
        }
        Cmd::Kept { to, lid, did, job } => {
            // kept as the file has them (or as they came): sent as they are
            if let Some(w) = job.rgba_tiles() {
                let enc: Vec<Value> = w
                    .iter()
                    .map(|((x, y), t)| {
                        let p = if t.deflated { "z:" } else { "r:" };
                        json!([
                            x,
                            y,
                            format!("{p}{}", data_encoding::BASE64.encode(&t.bytes))
                        ])
                    })
                    .collect();
                return (
                    to.clone(),
                    chunked(to.is_some(), &lid, &did, enc, Vec::new()),
                    None,
                );
            }
            let (rgba, _) = to_rgba(job.decode());
            (
                to.clone(),
                tile_edits(to.is_some(), &lid, &did, rgba, Vec::new()),
                None,
            )
        }
        _ => (None, Vec::new(), None),
    }
}

/// A snapshot's tiles as they came: base64 undone, nothing inflated (checked when shown).
fn wire_tiles(body: &Value) -> Option<WireTiles> {
    use rayon::prelude::*;
    if body.get("k")?.as_str()? != "snap.tiles" {
        return None;
    }
    let lid = body.get("lid")?.as_str()?.to_string();
    let did = body.get("did")?.as_str()?.to_string();
    let int = |v: &Value| v.as_i64().filter(|n| n.abs() < 1_000_000).map(|n| n as i32);
    let set: Vec<(i32, i32, &str)> = body
        .get("set")?
        .as_array()?
        .iter()
        .filter_map(|t| {
            let t = t.as_array()?;
            Some((int(t.first()?)?, int(t.get(1)?)?, t.get(2)?.as_str()?))
        })
        .collect();
    let tiles = set
        .par_iter()
        .filter_map(|(x, y, s)| {
            let (deflated, b) = if let Some(z) = s.strip_prefix("z:") {
                (true, z)
            } else {
                (false, s.strip_prefix("r:")?)
            };
            let bytes = data_encoding::BASE64.decode(b.as_bytes()).ok()?;
            if !deflated && bytes.len() != TILE_BYTES {
                return None;
            }
            Some((
                (*x, *y),
                Arc::new(FileTile {
                    deflated,
                    bytes: bytes.into_boxed_slice(),
                }),
            ))
        })
        .collect();
    Some(WireTiles { lid, did, tiles })
}

/// An edit without its tiles' pixels (they went elsewhere).
fn strip(mut body: Value) -> Value {
    if let Some(o) = body.as_object_mut() {
        o.insert("set".into(), Value::Array(Vec::new()));
    }
    body
}

/// A tile's pixels (or None: nothing in it), and where.
type Rgba = ((i32, i32), Option<Vec<u8>>);

/// The painter's tiles as straight RGBA8 (in parallel), and those with nothing in them.
#[allow(clippy::type_complexity)]
fn to_rgba(
    set: Vec<((i32, i32), brush::DecodedTile)>,
) -> (Vec<(i32, i32, Vec<u8>)>, Vec<(i32, i32)>) {
    use rayon::prelude::*;
    let all: Vec<Rgba> = set
        .into_par_iter()
        .map(|((x, y), t)| {
            let mut px = vec![0u8; TILE_BYTES];
            brush::encode_tile(&t, &mut px);
            ((x, y), px.chunks(4).any(|p| p[3] > 0).then_some(px))
        })
        .collect();
    let mut set = Vec::with_capacity(all.len());
    let mut blank = Vec::new();
    for ((x, y), px) in all {
        match px {
            Some(px) => set.push((x, y, px)),
            None => blank.push((x, y)),
        }
    }
    (set, blank)
}

/// The edits carrying a drawing's tiles: compressed, at most [`TILES_PER_EDIT`] each (the
/// cleared ones with the first).
pub fn tile_edits(
    snap: bool,
    lid: &str,
    did: &str,
    set: Vec<(i32, i32, Vec<u8>)>,
    clear: Vec<(i32, i32)>,
) -> Vec<Value> {
    use rayon::prelude::*;
    let enc: Vec<Value> = set
        .par_iter()
        .map(|(x, y, px)| json!([x, y, encode_tile(px)]))
        .collect();
    chunked(snap, lid, did, enc, clear)
}

/// Tiles (on the wire already) in edits of at most [`TILES_PER_EDIT`] set and
/// [`CLEARS_PER_EDIT`] cleared (a snapshot's carry no clearing).
fn chunked(
    snap: bool,
    lid: &str,
    did: &str,
    enc: Vec<Value>,
    clear: Vec<(i32, i32)>,
) -> Vec<Value> {
    let k = if snap { "snap.tiles" } else { "tiles" };
    if snap {
        return enc
            .chunks(TILES_PER_EDIT)
            .map(|c| json!({"k": k, "lid": lid, "did": did, "set": c}))
            .collect();
    }
    // (an edit takes 4 × 48 tiles in all at most, set and cleared: the rest go in more)
    let mut sets = enc.chunks(TILES_PER_EDIT);
    let mut clears = clear.chunks(CLEARS_PER_EDIT);
    let mut out = Vec::new();
    loop {
        let (s, c) = (sets.next(), clears.next());
        if s.is_none() && c.is_none() && !out.is_empty() {
            break;
        }
        out.push(json!({
            "k": k, "lid": lid, "did": did,
            "set": s.unwrap_or_default(), "clear": c.unwrap_or_default()
        }));
    }
    out
}

/// Tiles cleared per edit at most (with [`TILES_PER_EDIT`] set, under the 4 × 48 an edit
/// may carry).
pub const CLEARS_PER_EDIT: usize = 3 * TILES_PER_EDIT;

/// A tile for the wire: deflated ("z:") as the current app deflates them.
pub fn encode_tile(px: &[u8]) -> String {
    pencilpilot_together::encode_tile(px)
}

/// A tile from the wire (16 384 bytes of straight RGBA8), or None.
pub fn decode_tile(s: &str) -> Option<Vec<u8>> {
    pencilpilot_together::decode_tile(s).filter(|b| b.len() == TILE_BYTES)
}

/// The tiles in an edit of kind `k`, decoded for the painter (in parallel), or None when
/// it isn't one.
fn decode_tiles(body: &Value, k: &str) -> Option<Tiles> {
    use rayon::prelude::*;
    if body.get("k")?.as_str()? != k {
        return None;
    }
    let lid = body.get("lid")?.as_str()?.to_string();
    let did = body.get("did")?.as_str()?.to_string();
    let int = |v: &Value| v.as_i64().filter(|n| n.abs() < 1_000_000).map(|n| n as i32);
    let set: Vec<(i32, i32, &str)> = body
        .get("set")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let t = t.as_array()?;
                    Some((int(t.first()?)?, int(t.get(1)?)?, t.get(2)?.as_str()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut clear: Vec<(i32, i32)> = body
        .get("clear")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let t = t.as_array()?;
                    Some((int(t.first()?)?, int(t.get(1)?)?))
                })
                .collect()
        })
        .unwrap_or_default();
    let decoded: Vec<((i32, i32), Option<brush::DecodedTile>)> = set
        .par_iter()
        .filter_map(|(x, y, s)| {
            let px = decode_tile(s)?;
            // (a tile with nothing in it decodes to none: it's cleared)
            Some(((*x, *y), brush::decode_tile(&px)))
        })
        .collect();
    let mut set = Vec::with_capacity(decoded.len());
    for (k, t) in decoded {
        match t {
            Some(t) => set.push((k, t)),
            None => clear.push(k),
        }
    }
    Some(Tiles {
        lid,
        did,
        set,
        clear,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_go_and_come_back() {
        let mut px = vec![0u8; TILE_BYTES];
        for (i, p) in px.chunks_mut(4).enumerate() {
            p.copy_from_slice(&[(i % 256) as u8, 40, 200, if i % 3 == 0 { 255 } else { 0 }]);
        }
        let s = encode_tile(&px);
        assert!(s.starts_with("z:"));
        assert_eq!(decode_tile(&s).unwrap(), px);
        let edits = tile_edits(
            false,
            "aaaaaaaaaaaa:1",
            "aaaaaaaaaaaa:2",
            (0..50).map(|i| (i, 0, px.clone())).collect(),
            vec![(9, 9)],
        );
        assert_eq!(edits.len(), 2, "48 tiles an edit");
        assert_eq!(edits[0]["clear"], json!([[9, 9]]));
        assert_eq!(edits[1]["clear"], json!([]));
        let t = decode_tiles(&edits[0], "tiles").unwrap();
        assert_eq!(t.set.len(), 48);
        assert_eq!(t.clear, vec![(9, 9)]);
        // nothing but clearing still goes
        let e = tile_edits(false, "a", "b", vec![], vec![(1, 2)]);
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn invites_are_found_in_pasted_text() {
        assert_eq!(
            clean_invite("join me: pp1-AbC_d-9 thanks").as_deref(),
            Some("pp1-AbC_d-9")
        );
        assert_eq!(clean_invite("nothing here"), None);
    }

    /// Tiles cleared go as the current app sends them (all in the first edit, with its
    /// first 48 set) while that's an edit its own check takes (set and cleared 4 × 48 at
    /// most); past that the current app's own edit would be refused by every app, so they
    /// go in more edits, each one both take.
    #[test]
    fn cleared_tiles_go_as_the_current_app_sends_them() {
        let px = vec![255u8; TILE_BYTES];
        let set: Vec<(i32, i32, Vec<u8>)> = (0..60).map(|i| (i, 0, px.clone())).collect();
        let clear: Vec<(i32, i32)> = (0..100).map(|i| (i, 1)).collect();
        let e = tile_edits(
            false,
            "aaaaaaaaaaaa:1",
            "aaaaaaaaaaaa:2",
            set.clone(),
            clear,
        );
        assert_eq!(e.len(), 2);
        assert_eq!(e[0]["set"].as_array().unwrap().len(), 48);
        assert_eq!(e[0]["clear"].as_array().unwrap().len(), 100);
        assert_eq!(e[1]["clear"], json!([]));
        // a big erase: every edit within the limit, and checked as edits
        let clear: Vec<(i32, i32)> = (0..400).map(|i| (i, 2)).collect();
        let e = tile_edits(false, "aaaaaaaaaaaa:1", "aaaaaaaaaaaa:2", set, clear);
        let mut cleared = 0;
        for x in &e {
            let n = x["set"].as_array().unwrap().len() + x["clear"].as_array().unwrap().len();
            assert!(n <= 4 * TILES_PER_EDIT);
            assert!(pencilpilot_together::parse_edit(x).is_some());
            cleared += x["clear"].as_array().unwrap().len();
        }
        assert_eq!(cleared, 400);
    }
}
