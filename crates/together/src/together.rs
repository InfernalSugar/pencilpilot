//! One app's side of drawing together ([`Together`]): turns the session's edits into
//! canvas and timeline changes, and this app's changes into edits, as together.ts's
//! `Together` does.
//!
//! It does no networking and owns no canvas: the app hands it the canvas
//! ([`CanvasPort`]) on each call, and sends what it queued ([`Together::take_out`]) to
//! the session (`pencilpilot_share::Session::edit` / `direct`). Everything happens in
//! the call (no `async`): edits are put in one at a time, in the session's order.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use serde_json::Value;

use crate::cache::{Changes, Tile, Tiles, deflate};
use crate::edit::{
    Edit, Look, SOUND_CHUNK, SOUND_MAX, TILES_PER_EDIT, TileClear, TileSet, Why, parse_edit,
    same_look,
};
use crate::js::{self, from_base64, is_lid, to_base64};
use crate::lines::{LineStroke, apply_line_edit, line_edits, read_line_edit, with_base};
use crate::saved::{SavedMember, SavedShare};
use crate::shared::{Shared, made_by};
use crate::tiles::{TileKey, TileMap, pack, unpack};
use crate::timeline::{Timeline, from_shared, to_shared};

/// A drawing's line strokes, as the app keeps them (shared, so an unchanged list is
/// seen as unchanged without comparing it stroke by stroke).
pub type Strokes = Arc<Vec<LineStroke>>;

/// A layer as the app has it.
#[derive(Debug, Clone, PartialEq)]
pub struct PortLayer {
    pub id: u32,
    pub name: String,
    pub look: Option<Look>,
    /// an adjustment layer's settings
    pub adjust: Option<String>,
    /// its group (0: none)
    pub group: u32,
}

/// A group's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSettings {
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
}

impl GroupSettings {
    fn same(&self, o: &GroupSettings) -> bool {
        self.name == o.name && self.visible == o.visible && (self.opacity - o.opacity).abs() < 1e-3
    }
}

/// A group as the app has it.
#[derive(Debug, Clone, PartialEq)]
pub struct PortGroup {
    pub id: u32,
    pub settings: GroupSettings,
}

/// A change to a layer's settings (what's `None` stays as it is).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayerPatch {
    pub name: Option<String>,
    /// (each app's own: it's how access is shown)
    pub locked: Option<bool>,
    pub visible: Option<bool>,
    /// the shared settings (its `style: None` leaves the style as it is)
    pub look: Option<Look>,
}

/// What drawing together needs from the app. None of it is an undo step. A `false` (or
/// 0) from something that changes the canvas means "busy (mid-stroke): try again
/// later"; it's tried again on the next [`Together::local_changed`].
pub trait CanvasPort {
    /// A new layer on top; its id (0: couldn't).
    fn add_layer(&mut self, name: &str, adjust: Option<&str>) -> u32;
    /// Change an adjustment layer's settings.
    fn set_adjust(&mut self, id: u32, spec: &str) -> bool;
    /// A layer's mask as packed tiles ([`crate::tiles::pack`]); empty when it has none.
    fn mask_packed(&mut self, id: u32) -> Vec<u8>;
    /// Put a layer's mask (empty: none).
    fn put_mask(&mut self, id: u32, packed: &[u8]) -> bool;
    fn set_layer(&mut self, id: u32, patch: LayerPatch);
    /// The drawing the layer shows now (made, and put on the timeline, if it has none).
    fn drawing_of(&mut self, id: u32) -> u32;
    /// A new, empty drawing.
    fn new_drawing(&mut self) -> u32;
    /// The layer a drawing belongs to (showing, or on the timeline).
    fn layer_of_drawing(&self, drawing: u32) -> Option<u32>;
    /// A drawing's tiles, packed (fetched back from the frame store if need be).
    fn packed(&mut self, drawing: u32) -> Vec<u8>;
    /// Put a drawing's tiles (packed).
    fn put(&mut self, drawing: u32, packed: &[u8]) -> bool;
    /// Put a drawing's tiles as they came over the wire (each tile's raw deflate, as "z:"
    /// carries it), for an app that keeps tiles compressed or decodes them on threads of its
    /// own. `None`: not taken this way (the default): they come through [`CanvasPort::put`].
    fn put_deflated(&mut self, _drawing: u32, _tiles: &[(TileKey, Arc<[u8]>)]) -> Option<bool> {
        None
    }
    fn select(&mut self, id: u32);
    /// Remove a layer (clears undo).
    fn remove_layer(&mut self, id: u32) -> bool;
    /// Move a layer right above / below `reference` (0: where it is) and into a group
    /// (0: none); clears undo.
    fn place(&mut self, id: u32, reference: u32, above: bool, group: u32) -> bool;
    /// A new group holding a layer: its id (0: busy); clears undo.
    fn add_group(&mut self, layer: u32) -> u32;
    fn set_group(&mut self, id: u32, s: &GroupSettings);
    /// Take a group's layers out and remove it; clears undo.
    fn ungroup(&mut self, id: u32) -> bool;
    /// Merge a layer into the one below (false: couldn't); clears undo.
    fn merge_down(&mut self, id: u32) -> bool;
    /// Flatten a group into its lowest layer: that layer (0: couldn't); clears undo.
    fn merge_group(&mut self, id: u32) -> u32;
    fn groups(&self) -> Vec<PortGroup>;
    /// Every layer, bottom → top.
    fn layers(&self) -> Vec<PortLayer>;
    /// A sound file the studio has (by key): its bytes and name.
    fn sound(&self, file: &str) -> Option<(Arc<Vec<u8>>, String)>;
    /// Add a sound file under its key.
    fn put_sound(&mut self, file: &str, bytes: Vec<u8>, name: &str);
    fn timeline(&self) -> Timeline;
    /// Put the session's timeline in (shows the frame again).
    fn set_timeline(&mut self, tl: Timeline);
    /// Every drawing on a layer on the timeline.
    fn drawings_of_layer(&self, id: u32) -> Vec<u32>;

    // ---- line layers (an app without them leaves these as they are) ----

    /// Whether the app has line layers at all.
    fn has_lines(&self) -> bool {
        false
    }
    fn is_line_layer(&self, _id: u32) -> bool {
        false
    }
    /// Make a layer a line layer, or a plain one.
    fn set_line_layer(&mut self, _id: u32, _on: bool) {}
    /// A drawing's line strokes.
    fn line_strokes(&self, _drawing: u32) -> Strokes {
        Strokes::default()
    }
    /// Set a drawing's line strokes (its pixels come as tiles); `plain`: a plain
    /// layer's editable strokes.
    fn set_line_strokes(&mut self, _drawing: u32, _list: Strokes, _plain: bool) {}
    /// Whether a drawing's strokes are a plain layer's (they have a base).
    fn plain_strokes(&self, _drawing: u32) -> bool {
        false
    }
    /// A plain layer drawing's base: what changed since last asked.
    fn base_changes(&mut self, _drawing: u32) -> Option<Vec<u8>> {
        None
    }
    /// All of a plain layer drawing's base (for someone joining).
    fn base_all(&mut self, _drawing: u32) -> Option<Vec<u8>> {
        None
    }
    /// Someone else's base changes for a drawing.
    fn put_base_changes(&mut self, _drawing: u32, _bytes: &[u8]) {}
}

/// Something to send.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// to everyone, numbered by the session (`Session::edit`)
    Edit(Value),
    /// to one member (`Session::direct`)
    Direct { to: String, body: Value },
}

/// Something for the app to know.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// tell the person this (a change asked for went ahead, or was refused)
    Note(String),
    /// someone's real pixels arrived (their live stroke preview can go)
    Tiles { from: String },
    /// the shared state changed (the layers panel, the Together dialog)
    Changed,
}

/// A change to a group on the canvas (its settings), made once it's allowed.
pub type GroupChange = Box<dyn FnOnce(&mut dyn CanvasPort)>;

/// A change that waits for owners to say yes.
enum Pending {
    MergeDown { a: String, b: String },
    Remove { lid: String },
    AddGroup { lid: String },
    Place { lid: String, to: u32, gid: String },
    ChangeGroup(GroupChange),
    Ungroup { gid: String },
    DeleteGroup { gid: String },
    MergeGroup { gid: String, m: Vec<String> },
    MoveGroup { gid: String, up: bool },
}

#[derive(Clone)]
struct Need {
    lid: String,
    own: bool,
    with: Option<String>,
}

struct Waiting {
    needs: Vec<Need>,
    run: Pending,
}

/// A canvas change that waits while the canvas is busy.
#[derive(Clone)]
enum Op {
    Remove(u32),
    Adjust(u32, String),
    Ungroup(u32),
    /// right above / below `reference` (0: where it is), into the shared group's here
    Place {
        id: u32,
        reference: u32,
        above: bool,
        gid: Option<String>,
    },
    MakeGroup {
        gid: String,
        first: String,
        s: GroupSettings,
    },
}

struct Later {
    op: Op,
    tries: u32,
}

/// Edits to send once this app's changed tiles have gone.
enum AfterSend {
    Remove(String),
    MergedGroup {
        bottom: String,
        name: String,
        others: Vec<String>,
    },
}

/// `tileParts`: tiles to set and clear cut into edits every app takes: at most
/// [`TILES_PER_EDIT`] set and, with them, at most four times that in all (more clears go
/// in edits of their own). Always one edit at least.
pub fn tile_parts<'a, S, C>(set: &'a [S], clear: &'a [C]) -> Vec<(&'a [S], &'a [C])> {
    let clears = 4 * TILES_PER_EDIT - TILES_PER_EDIT;
    let n = set
        .len()
        .div_ceil(TILES_PER_EDIT)
        .max(clear.len().div_ceil(clears))
        .max(1);
    (0..n)
        .map(|i| {
            let s = &set
                [(i * TILES_PER_EDIT).min(set.len())..((i + 1) * TILES_PER_EDIT).min(set.len())];
            let c = &clear[(i * clears).min(clear.len())..((i + 1) * clears).min(clear.len())];
            (s, c)
        })
        .collect()
}

/// Each tile to send, deflated (the wire's bytes).
fn wire_of(d: &Changes) -> Vec<Arc<[u8]>> {
    d.set.iter().map(|(_, _, px)| deflate(px).into()).collect()
}

/// "z:" and the bytes in base64.
fn wire_text(z: &[u8]) -> String {
    format!("z:{}", to_base64(z))
}

fn tile_sets(d: &Changes, wire: &[Arc<[u8]>]) -> Vec<TileSet> {
    d.set
        .iter()
        .zip(wire)
        .map(|((x, y, _), z)| (*x, *y, wire_text(z)))
        .collect()
}

fn key(lid: &str, did: &str) -> String {
    format!("{lid}|{did}")
}

fn split_key(k: &str) -> (&str, &str) {
    k.split_once('|').unwrap_or((k, ""))
}

fn unique(v: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// A random start for new ids (together.ts: `Math.floor(Math.random() * 1e8)`).
fn random_start() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    h.finish() % 100_000_000
}

/// One app's side of drawing together.
pub struct Together {
    /// this member's id (64 hex digits) and name
    pub me: String,
    pub my_name: String,
    /// the shared layers and who may draw on them
    pub shared: Shared,
    /// shared layer → the canvas's layer here
    pub ids: IndexMap<String, u32>,
    /// shared group → the canvas's group here
    pub gids: IndexMap<String, u32>,
    /// each shared drawing's tiles as last synced ("lid|did" → tiles; "lid|mask": its mask)
    cache: IndexMap<String, Tiles>,
    dids: IndexMap<String, u32>,
    did_of: HashMap<u32, String>,
    pending: IndexSet<String>,
    structure: VecDeque<Later>,
    local: HashSet<u32>,
    touched: IndexSet<u32>,
    synced: bool,
    buffer: Vec<(u64, String, Value)>,
    snap_seq: f64,
    snap_tl: Option<Value>,
    last_seq: u64,
    n: u64,
    mine: HashSet<String>,
    sounds_shared: HashSet<String>,
    sound_parts: HashMap<String, (String, Vec<Option<String>>)>,
    mask_on: HashMap<String, bool>,
    adjust_sent: HashMap<String, String>,
    look_sent: HashMap<String, Look>,
    rejoining: bool,
    groups_seen: HashSet<String>,
    group_sent: HashMap<String, GroupSettings>,
    waiting: Vec<Waiting>,
    after_send: Vec<AfterSend>,
    lines_synced: IndexMap<String, Strokes>,
    out: Vec<Out>,
    notices: Vec<Notice>,
    /// joiners who asked for the project before this app had it: answered once it has
    askers: Vec<String>,
    /// edits sent from here that the session hasn't handed back yet
    unseen: usize,
}

impl Together {
    /// This app's side of a session, for member `me` called `my_name`.
    pub fn new(me: &str, my_name: &str) -> Self {
        Self::with_start(me, my_name, random_start())
    }

    /// The same, with new layers' ids counted on from `n` (tests: the same ids each run).
    pub fn with_start(me: &str, my_name: &str, n: u64) -> Self {
        Together {
            me: me.into(),
            my_name: my_name.into(),
            shared: Shared::default(),
            ids: IndexMap::new(),
            gids: IndexMap::new(),
            cache: IndexMap::new(),
            dids: IndexMap::new(),
            did_of: HashMap::new(),
            pending: IndexSet::new(),
            structure: VecDeque::new(),
            local: HashSet::new(),
            touched: IndexSet::new(),
            synced: false,
            buffer: Vec::new(),
            snap_seq: 0.0,
            snap_tl: None,
            last_seq: 0,
            n,
            mine: HashSet::new(),
            sounds_shared: HashSet::new(),
            sound_parts: HashMap::new(),
            mask_on: HashMap::new(),
            adjust_sent: HashMap::new(),
            look_sent: HashMap::new(),
            rejoining: false,
            groups_seen: HashSet::new(),
            group_sent: HashMap::new(),
            waiting: Vec::new(),
            after_send: Vec::new(),
            lines_synced: IndexMap::new(),
            out: Vec::new(),
            notices: Vec::new(),
            askers: Vec::new(),
            unseen: 0,
        }
    }

    /// Bytes the synced tiles take here (each tile's hash, and its deflated bytes where it
    /// has them).
    pub fn cache_bytes(&self) -> usize {
        self.cache.values().map(Tiles::bytes).sum()
    }

    /// Whether this app has the project (hosting, or joined and caught up).
    pub fn synced(&self) -> bool {
        self.synced
    }

    /// What to send, in order (send it all, in this order, before the next call).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// What the app should know, in order.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    fn edit(&mut self, e: Value) {
        self.unseen += 1;
        self.out.push(Out::Edit(e));
    }

    /// Answer who asked for the project, once this app has it and has its own edits back
    /// (a snapshot taken before would leave out the layers on their way, and the tiles
    /// that go only with it).
    fn answer(&mut self, c: &mut dyn CanvasPort) {
        if !self.synced || self.unseen > 0 {
            return;
        }
        for to in std::mem::take(&mut self.askers) {
            self.send_snapshot(c, &to);
        }
    }

    fn direct(&mut self, to: &str, body: Value) {
        self.out.push(Out::Direct {
            to: to.into(),
            body,
        });
    }

    fn note(&mut self, text: &str) {
        self.notices.push(Notice::Note(text.into()));
    }

    fn changed(&mut self) {
        if self.notices.last() != Some(&Notice::Changed) {
            self.notices.push(Notice::Changed);
        }
    }

    fn me12(&self) -> String {
        self.me.chars().take(12).collect()
    }

    fn new_lid(&mut self) -> String {
        self.n += 1;
        format!("{}:{}", self.me12(), self.n)
    }

    /// A drawing's shared id (given one the first time it's shared).
    pub fn did_of(&mut self, drawing: u32) -> String {
        if let Some(d) = self.did_of.get(&drawing) {
            return d.clone();
        }
        let did = format!("{}:{}", self.me12(), drawing);
        self.did_of.insert(drawing, did.clone());
        self.dids.insert(did.clone(), drawing);
        did
    }

    /// A shared drawing's id here (a new, empty one the first time).
    pub fn local_of(&mut self, c: &mut dyn CanvasPort, did: &str) -> u32 {
        local_of(&mut self.dids, &mut self.did_of, c, did)
    }

    /// The shared layer a canvas layer is, if any.
    pub fn lid_of(&self, id: u32) -> Option<String> {
        self.ids
            .iter()
            .find(|(_, i)| **i == id)
            .map(|(l, _)| l.clone())
    }

    /// The shared group a canvas group is, if any.
    pub fn gid_of(&self, id: u32) -> Option<String> {
        self.gids
            .iter()
            .find(|(_, i)| **i == id)
            .map(|(g, _)| g.clone())
    }

    /// Send the sound files the timeline plays that the session doesn't have yet (or,
    /// to one member, all of them).
    fn share_sounds(&mut self, c: &mut dyn CanvasPort, tl: &Value, to: Option<&str>) {
        let empty = Vec::new();
        let scenes = tl["scenes"].as_array().unwrap_or(&empty);
        let files: Vec<String> = scenes
            .iter()
            .flat_map(|sc| sc.get("audio").and_then(Value::as_array).unwrap_or(&empty))
            .filter_map(|a| a.get("file").and_then(Value::as_str).map(str::to_string))
            .collect();
        for file in files {
            if to.is_none() && self.sounds_shared.contains(&file) {
                continue;
            }
            let Some((bytes, name)) = c.sound(&file) else {
                continue;
            };
            if bytes.len() > SOUND_MAX {
                continue;
            }
            if to.is_none() {
                self.sounds_shared.insert(file.clone());
            }
            let n = bytes.len().div_ceil(SOUND_CHUNK).max(1);
            for i in 0..n {
                let part = &bytes
                    [(i * SOUND_CHUNK).min(bytes.len())..((i + 1) * SOUND_CHUNK).min(bytes.len())];
                let piece = Edit::Sound {
                    file: file.clone(),
                    name: name.clone(),
                    i: i as u32,
                    n: n as u32,
                    data: to_base64(part),
                }
                .to_json();
                match to {
                    Some(to) => {
                        let mut p = piece;
                        p["k"] = "snap.sound".into();
                        self.direct(to, p);
                    }
                    None => self.edit(piece),
                }
            }
        }
    }

    fn shared_timeline(&mut self, c: &mut dyn CanvasPort) -> Value {
        let tl = c.timeline();
        let ids = self.ids.clone();
        let lid_of = |l: u32| ids.iter().find(|(_, i)| **i == l).map(|(x, _)| x.clone());
        let mut did_of = |d: u32| self.did_of(d);
        to_shared(&tl, &lid_of, &mut did_of)
    }

    /// Hosting: share what's on the canvas (every layer, every frame, the timeline).
    pub fn start_host(&mut self, c: &mut dyn CanvasPort) {
        let mut adjusts = HashSet::new();
        for l in c.layers() {
            let lid = self.new_lid();
            self.ids.insert(lid.clone(), l.id);
            if l.adjust.is_some() {
                adjusts.insert(l.id);
            }
            let lines = l.adjust.is_none() && c.is_line_layer(l.id);
            self.edit(
                Edit::Layer {
                    lid,
                    name: l.name,
                    adjust: l.adjust,
                    lines,
                }
                .to_json(),
            );
        }
        self.share_groups(c);
        for (lid, id) in self.ids.clone() {
            if adjusts.contains(&id) {
                continue;
            }
            let first = c.drawing_of(id);
            for d in unique(std::iter::once(first).chain(c.drawings_of_layer(id))) {
                let tiles = unpack(&c.packed(d));
                let did = self.did_of(d);
                // (kept, not sent: no one is in the session yet, and whoever joins gets
                // them with the project)
                self.cache.insert(key(&lid, &did), Tiles::local(&tiles));
                self.send_lines(c, &lid, id, d, true);
            }
        }
        let tl = self.shared_timeline(c);
        self.share_sounds(c, &tl, None);
        self.edit(Edit::Timeline { tl }.to_json());
        self.now_synced(c);
    }

    /// This app has the project: joiners who asked meanwhile get it.
    fn now_synced(&mut self, c: &mut dyn CanvasPort) {
        self.synced = true;
        self.answer(c);
    }

    /// This app's groups, shared (hosting): each with its layers, then its settings.
    fn share_groups(&mut self, c: &mut dyn CanvasPort) {
        let layers = c.layers();
        for g in c.groups() {
            let lids: Vec<String> = layers
                .iter()
                .filter(|l| l.group == g.id)
                .filter_map(|l| self.lid_of(l.id))
                .collect();
            if lids.is_empty() {
                continue;
            }
            let gid = self.new_lid();
            self.gids.insert(gid.clone(), g.id);
            self.edit(
                Edit::Group {
                    lid: lids[0].clone(),
                    gid: gid.clone(),
                    name: g.settings.name.clone(),
                    lids,
                }
                .to_json(),
            );
            self.group_sent.insert(gid.clone(), g.settings.clone());
            self.edit(
                Edit::GroupSet {
                    lid: gid,
                    name: g.settings.name,
                    visible: g.settings.visible,
                    opacity: g.settings.opacity,
                }
                .to_json(),
            );
        }
    }

    /// The session, to keep in the project file (`members`: everyone who's been in it).
    pub fn save(&self, code: &str, members: &[SavedMember]) -> SavedShare {
        let (layers, groups, asks) = self.shared.snapshot();
        SavedShare {
            code: code.into(),
            members: members.to_vec(),
            layers,
            groups,
            asks,
            ids: self.ids.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            gids: self.gids.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            dids: self.dids.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            n: self.n,
        }
    }

    /// Back to a saved session (its layer ids already this document's).
    pub fn restore(&mut self, c: &mut dyn CanvasPort, saved: &SavedShare) {
        self.shared.load(&saved.layers, &saved.groups, &saved.asks);
        let here: HashSet<u32> = c.layers().iter().map(|l| l.id).collect();
        self.ids = saved
            .ids
            .iter()
            .filter(|(_, id)| here.contains(id))
            .cloned()
            .collect();
        let groups_here: HashSet<u32> = c.groups().iter().map(|g| g.id).collect();
        self.gids = saved
            .gids
            .iter()
            .filter(|(_, id)| groups_here.contains(id))
            .cloned()
            .collect();
        for gid in self.gids.keys() {
            self.groups_seen.insert(gid.clone());
            if let Some(g) = self.shared.groups.get(gid) {
                self.group_sent.insert(
                    gid.clone(),
                    GroupSettings {
                        name: g.name.clone(),
                        visible: g.visible,
                        opacity: g.opacity,
                    },
                );
            }
        }
        for (did, d) in &saved.dids {
            self.dids.insert(did.clone(), *d);
            self.did_of.insert(*d, did.clone());
        }
        self.n = self.n.max(saved.n + 1);
        // layers here that aren't shared (from before joining) stay out of it
        let shared: HashSet<u32> = self.ids.values().copied().collect();
        for l in c.layers() {
            if !shared.contains(&l.id) {
                self.local.insert(l.id);
            }
        }
    }

    /// After [`Together::restore`], alone: the copy here is the project (others coming
    /// back get it).
    pub fn resume_host(&mut self, c: &mut dyn CanvasPort) {
        for (lid, id) in self.ids.clone() {
            let first = c.drawing_of(id);
            for d in unique(std::iter::once(first).chain(c.drawings_of_layer(id))) {
                let did = self.did_of(d);
                self.cache
                    .insert(key(&lid, &did), Tiles::local(&unpack(&c.packed(d))));
                // editable strokes too, so joiners get them (a line layer's; a plain
                // layer's, with the pixels under them)
                if c.has_lines() && (c.is_line_layer(id) || c.plain_strokes(d)) {
                    self.lines_synced.insert(key(&lid, &did), c.line_strokes(d));
                }
            }
        }
        for lid in self.ids.keys().cloned().collect::<Vec<_>>() {
            self.refresh_lock(c, &lid);
        }
        self.now_synced(c);
        self.changed();
    }

    /// After [`Together::restore`], with others online: get the project from `host`;
    /// this app's own layers keep what was drawn here meanwhile.
    pub fn rejoin(&mut self, host: &str) {
        self.rejoining = true;
        self.direct(host, js::object(vec![("k", "snap.req".into())]));
    }

    /// Joining: ask the host for the project; this app's own layers are put aside
    /// (hidden, locked, not shared).
    pub fn start_join(&mut self, c: &mut dyn CanvasPort, host: &str) {
        for l in c.layers() {
            self.local.insert(l.id);
            c.set_layer(
                l.id,
                LayerPatch {
                    visible: Some(false),
                    locked: Some(true),
                    ..Default::default()
                },
            );
        }
        self.direct(host, js::object(vec![("k", "snap.req".into())]));
    }

    /// A layer of this app's own that isn't shared (e.g. live stroke previews).
    pub fn exclude(&mut self, id: u32) {
        self.local.insert(id);
    }

    /// A new layer of this app's own (on top, for everyone); `lines`: a line layer. It's
    /// made here when the session numbers it (and selected).
    pub fn add_own_layer(&mut self, name: &str, lines: bool) {
        let lid = self.new_lid();
        self.mine.insert(lid.clone());
        let name = if name.is_empty() {
            format!(
                "{}\u{2019}s {}",
                self.my_name,
                if lines { "lines" } else { "layer" }
            )
        } else {
            name.into()
        };
        self.edit(
            Edit::Layer {
                lid,
                name,
                adjust: None,
                lines,
            }
            .to_json(),
        );
    }

    /// Send what changed in a drawing's editable strokes of this app's (`all`: the whole
    /// base, for a start).
    fn send_lines(&mut self, c: &mut dyn CanvasPort, lid: &str, id: u32, d: u32, all: bool) {
        if !c.has_lines() {
            return;
        }
        let line_layer = c.is_line_layer(id);
        let plain = !line_layer && c.plain_strokes(d);
        if !line_layer && !plain {
            return;
        }
        let did = self.did_of(d);
        let now = c.line_strokes(d);
        let k = key(lid, &did);
        let same = self
            .lines_synced
            .get(&k)
            .is_some_and(|w| Arc::ptr_eq(w, &now) || **w == *now);
        let mut base = if plain { c.base_changes(d) } else { None };
        if plain && all {
            base = c.base_all(d).or(base);
        }
        if same && base.as_ref().is_none_or(|b| b.is_empty()) {
            return;
        }
        let was = self.lines_synced.insert(k, now.clone()).unwrap_or_default();
        let edits = if same {
            Vec::new()
        } else {
            line_edits(lid, &did, &was, &now)
        };
        for e in with_base(edits, lid, &did, base.as_deref()) {
            self.edit(e.to_json());
        }
    }

    /// Delete a shared layer of this app's own, for everyone.
    pub fn remove_own(&mut self, lid: &str) {
        if self
            .shared
            .layers
            .get(lid)
            .is_some_and(|l| l.owner == self.me)
        {
            self.edit(Edit::Remove { lid: lid.into() }.to_json());
        }
    }

    /// Move a shared layer of this app's own to `to` in the shared order (0 = bottom).
    pub fn move_own(&mut self, lid: &str, to: u32) {
        if self
            .shared
            .layers
            .get(lid)
            .is_some_and(|l| l.owner == self.me)
        {
            self.edit(
                Edit::Move {
                    lid: lid.into(),
                    to,
                }
                .to_json(),
            );
        }
    }

    /// A layer just made here: shared now (so the timeline sent next carries its tracks)
    /// and put just above `above`, a shared layer, for everyone. Its lid (None: not shared).
    pub fn share_above(&mut self, c: &mut dyn CanvasPort, id: u32, above: u32) -> Option<String> {
        if !self.synced {
            return None;
        }
        if self.lid_of(id).is_none() {
            self.local_changed(c);
        }
        let lid = self.lid_of(id)?;
        let Some(at) = self.lid_of(above) else {
            return Some(lid);
        };
        // the session takes the new layer before the move: it's this app's by then
        let i = self
            .shared
            .order
            .iter()
            .filter(|x| **x != lid)
            .position(|x| *x == at);
        if let Some(i) = i {
            self.edit(
                Edit::Move {
                    lid: lid.clone(),
                    to: i as u32 + 1,
                }
                .to_json(),
            );
        }
        Some(lid)
    }

    /// Ask to draw on someone's layer.
    pub fn ask(&mut self, lid: &str) {
        self.edit(
            Edit::Ask {
                lid: lid.into(),
                why: Why::Draw,
                own: false,
                with: None,
            }
            .to_json(),
        );
    }

    fn ok(&self, n: &Need) -> bool {
        if n.own {
            self.shared.owns(&n.lid, &self.me)
        } else {
            self.shared.can_draw(&n.lid, &self.me)
        }
    }

    /// Do `run` once this app may: what's missing of `needs` is asked for, saying `why`.
    /// The owners asked (none: it went ahead now).
    fn request(
        &mut self,
        c: &mut dyn CanvasPort,
        why: Why,
        needs: Vec<Need>,
        run: Pending,
    ) -> Vec<String> {
        let missing: Vec<Need> = needs.iter().filter(|n| !self.ok(n)).cloned().collect();
        if missing.is_empty() {
            self.run(c, run);
            return Vec::new();
        }
        for n in &missing {
            self.edit(
                Edit::Ask {
                    lid: n.lid.clone(),
                    why,
                    own: n.own,
                    with: n.with.clone(),
                }
                .to_json(),
            );
        }
        self.waiting.push(Waiting { needs, run });
        let mut owners: Vec<String> = Vec::new();
        for n in &missing {
            if let Some(o) = self.shared.owner_of(&n.lid)
                && !owners.iter().any(|x| x == o)
            {
                owners.push(o.into());
            }
        }
        owners
    }

    /// Changes waiting for a yes: those that may go ahead now do.
    fn try_waiting(&mut self, c: &mut dyn CanvasPort) {
        let (ready, rest): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|w| w.needs.iter().all(|n| self.ok(n)));
        self.waiting = rest;
        for w in ready {
            self.run(c, w.run);
        }
    }

    fn run(&mut self, c: &mut dyn CanvasPort, p: Pending) {
        match p {
            Pending::MergeDown { a, b } => {
                let id_a = self.ids.get(&a).copied();
                // meanwhile moved apart, or gone: nothing merged
                let Some(id_a) = id_a.filter(|id| self.below(c, *id).as_deref() == Some(&b)) else {
                    self.note("The layers moved meanwhile: merge again.");
                    return;
                };
                if !c.merge_down(id_a) {
                    self.note("Couldn\u{2019}t merge those layers.");
                    return;
                }
                self.forget(&a);
                self.after_send.push(AfterSend::Remove(a));
                self.local_changed(c);
            }
            Pending::Remove { lid } => self.edit(Edit::Remove { lid }.to_json()),
            Pending::AddGroup { lid } => {
                let gid = self.new_lid();
                self.edit(
                    Edit::Group {
                        lid: lid.clone(),
                        gid,
                        name: "Group".into(),
                        lids: vec![lid],
                    }
                    .to_json(),
                );
            }
            Pending::Place { lid, to, gid } => self.edit(Edit::Place { lid, to, gid }.to_json()),
            Pending::ChangeGroup(f) => {
                f(c);
                self.local_changed(c);
            }
            Pending::Ungroup { gid } => self.edit(Edit::Ungroup { lid: gid }.to_json()),
            Pending::DeleteGroup { gid } => {
                for lid in self.shared.members(&gid) {
                    self.edit(Edit::Remove { lid }.to_json());
                }
            }
            Pending::MergeGroup { gid, m } => {
                let now = self.shared.members(&gid);
                let local = self.gids.get(&gid).copied();
                let (Some(local), true) = (local, now == m) else {
                    self.note("The group changed meanwhile: merge again.");
                    return;
                };
                let name = self
                    .shared
                    .groups
                    .get(&gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                if c.merge_group(local) == 0 {
                    self.note("Couldn\u{2019}t merge that group.");
                    return;
                }
                for lid in &m[1..] {
                    self.forget(lid);
                }
                self.gids.shift_remove(&gid);
                self.group_sent.remove(&gid);
                self.after_send.push(AfterSend::MergedGroup {
                    bottom: m[0].clone(),
                    name,
                    others: m[1..].to_vec(),
                });
                self.local_changed(c);
            }
            Pending::MoveGroup { gid, up } => {
                let m = self.shared.members(&gid);
                if m.is_empty() {
                    return;
                }
                let order = &self.shared.order;
                let rest: Vec<&String> = order.iter().filter(|x| !m.contains(x)).collect();
                let Some(mut p) = order.iter().position(|x| *x == m[0]) else {
                    return;
                };
                let group_of = |x: Option<&&String>| {
                    x.and_then(|x| self.shared.layers.get(x.as_str()))
                        .and_then(|l| l.group.clone())
                };
                if up {
                    if p >= rest.len() {
                        return;
                    }
                    let h = group_of(rest.get(p));
                    p += 1;
                    while h.is_some() && p < rest.len() && group_of(rest.get(p)) == h {
                        p += 1;
                    }
                } else {
                    if p == 0 {
                        return;
                    }
                    let h = group_of(rest.get(p - 1));
                    p -= 1;
                    while h.is_some() && p > 0 && group_of(rest.get(p - 1)) == h {
                        p -= 1;
                    }
                }
                self.edit(
                    Edit::Block {
                        lid: gid,
                        to: p as u32,
                    }
                    .to_json(),
                );
            }
        }
    }

    /// The layer below a layer here, if shared.
    fn below(&self, c: &dyn CanvasPort, id: u32) -> Option<String> {
        let all = c.layers();
        let i = all.iter().position(|l| l.id == id)?;
        if i == 0 {
            return None;
        }
        self.lid_of(all[i - 1].id)
    }

    /// Merge a shared layer into the one below it: it needs the layer handed over (it
    /// goes) and leave to draw on the one below. Owners asked (none: merged now); None:
    /// can't.
    pub fn merge_down(&mut self, c: &mut dyn CanvasPort, id: u32) -> Option<Vec<String>> {
        let a = self.lid_of(id)?;
        let b = self.below(c, id)?;
        let needs = vec![
            Need {
                lid: a.clone(),
                own: true,
                with: Some(b.clone()),
            },
            Need {
                lid: b.clone(),
                own: false,
                with: Some(a.clone()),
            },
        ];
        Some(self.request(c, Why::Merge, needs, Pending::MergeDown { a, b }))
    }

    /// Forget a layer merged away here (its removal is sent after the merged pixels).
    fn forget(&mut self, lid: &str) {
        self.ids.shift_remove(lid);
        let prefix = format!("{lid}|");
        let gone: Vec<String> = self
            .cache
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for k in gone {
            self.cache.shift_remove(&k);
            self.pending.shift_remove(&k);
        }
    }

    fn need(lid: &str, own: bool) -> Need {
        Need {
            lid: lid.into(),
            own,
            with: None,
        }
    }

    /// Delete a shared layer: needs it handed over.
    pub fn delete_layer(&mut self, c: &mut dyn CanvasPort, id: u32) -> Option<Vec<String>> {
        let lid = self.lid_of(id)?;
        Some(self.request(
            c,
            Why::Delete,
            vec![Self::need(&lid, true)],
            Pending::Remove { lid },
        ))
    }

    /// Put a shared layer in a new group: needs leave to draw on it (and on the group
    /// it's in now). Owners asked; None: not a shared layer.
    pub fn add_group(&mut self, c: &mut dyn CanvasPort, id: u32) -> Option<Vec<String>> {
        let lid = self.lid_of(id)?;
        let mut needs = vec![Self::need(&lid, false)];
        if let Some(now) = self.shared.layers.get(&lid).and_then(|l| l.group.clone()) {
            needs.push(Self::need(&now, false));
        }
        Some(self.request(c, Why::Group, needs, Pending::AddGroup { lid }))
    }

    /// Move a shared layer to `to` (the shared order) and into a group (the canvas's
    /// group, 0: none): needs leave to draw on it, on that group, and on the group it
    /// leaves.
    pub fn move_layer_to(
        &mut self,
        c: &mut dyn CanvasPort,
        id: u32,
        to: u32,
        group: u32,
    ) -> Option<Vec<String>> {
        let lid = self.lid_of(id)?;
        let gid = if group != 0 {
            self.gid_of(group)?
        } else {
            String::new()
        };
        let mut needs = vec![Self::need(&lid, false)];
        if !gid.is_empty() {
            needs.push(Self::need(&gid, false));
        }
        if let Some(now) = self.shared.layers.get(&lid).and_then(|l| l.group.clone())
            && now != gid
        {
            needs.push(Self::need(&now, false));
        }
        Some(self.request(c, Why::Group, needs, Pending::Place { lid, to, gid }))
    }

    /// Change a shared group's settings: `run` changes them on the canvas, once allowed.
    pub fn change_group(
        &mut self,
        c: &mut dyn CanvasPort,
        group: u32,
        run: GroupChange,
    ) -> Option<Vec<String>> {
        let gid = self.gid_of(group)?;
        Some(self.request(
            c,
            Why::Group,
            vec![Self::need(&gid, false)],
            Pending::ChangeGroup(run),
        ))
    }

    /// Take a group's layers out of it: needs the group handed over.
    pub fn ungroup(&mut self, c: &mut dyn CanvasPort, group: u32) -> Option<Vec<String>> {
        let gid = self.gid_of(group)?;
        Some(self.request(
            c,
            Why::Group,
            vec![Self::need(&gid, true)],
            Pending::Ungroup { gid },
        ))
    }

    /// Delete a group and its layers: needs each layer handed over.
    pub fn delete_group(&mut self, c: &mut dyn CanvasPort, group: u32) -> Option<Vec<String>> {
        let gid = self.gid_of(group)?;
        let needs = self
            .shared
            .members(&gid)
            .iter()
            .map(|l| Self::need(l, true))
            .collect();
        Some(self.request(c, Why::Delete, needs, Pending::DeleteGroup { gid }))
    }

    /// Flatten a group into its lowest layer: needs the others handed over, and leave to
    /// draw on the lowest and to change the group.
    pub fn merge_group(&mut self, c: &mut dyn CanvasPort, group: u32) -> Option<Vec<String>> {
        let gid = self.gid_of(group)?;
        let m = self.shared.members(&gid);
        if m.is_empty() {
            return None;
        }
        let mut needs = vec![Self::need(&gid, false), Self::need(&m[0], false)];
        needs.extend(m[1..].iter().map(|l| Self::need(l, true)));
        Some(self.request(c, Why::Merge, needs, Pending::MergeGroup { gid, m }))
    }

    /// Move a group's layers together, up or down past the next layer (or group).
    pub fn move_group(
        &mut self,
        c: &mut dyn CanvasPort,
        group: u32,
        up: bool,
    ) -> Option<Vec<String>> {
        let gid = self.gid_of(group)?;
        Some(self.request(
            c,
            Why::Group,
            vec![Self::need(&gid, false)],
            Pending::MoveGroup { gid, up },
        ))
    }

    /// The owner lets `to` draw on a layer (or group), or hands it over.
    pub fn grant(&mut self, lid: &str, to: &str, handover: bool) {
        self.edit(
            Edit::Grant {
                lid: lid.into(),
                to: to.into(),
                handover,
            }
            .to_json(),
        );
    }

    pub fn deny(&mut self, lid: &str, to: &str) {
        self.edit(
            Edit::Deny {
                lid: lid.into(),
                to: to.into(),
            }
            .to_json(),
        );
    }

    pub fn revoke(&mut self, lid: &str, to: &str) {
        self.edit(
            Edit::Revoke {
                lid: lid.into(),
                to: to.into(),
            }
            .to_json(),
        );
    }

    /// The timeline changed here (a step of this app's): everyone gets it.
    pub fn timeline_changed(&mut self, c: &mut dyn CanvasPort) {
        if !self.synced {
            return;
        }
        let tl = self.shared_timeline(c);
        self.share_sounds(c, &tl, None);
        self.edit(Edit::Timeline { tl }.to_json());
    }

    /// These drawings may have changed here (e.g. an undo on another frame).
    pub fn touch(&mut self, drawings: &[u32]) {
        for d in drawings {
            if *d != 0 {
                self.touched.insert(*d);
            }
        }
    }

    /// A numbered edit from the session (this app's own come back too). Put them in in
    /// the order the session numbered them.
    pub fn on_edit(&mut self, c: &mut dyn CanvasPort, seq: u64, from: &str, body: &Value) {
        if from == self.me {
            self.unseen = self.unseen.saturating_sub(1);
        }
        self.take_edit(c, seq, from, body);
        if !self.askers.is_empty() {
            self.answer(c);
        }
    }

    fn take_edit(&mut self, c: &mut dyn CanvasPort, seq: u64, from: &str, body: &Value) {
        if !self.synced {
            self.buffer.push((seq, from.into(), body.clone()));
            return;
        }
        self.last_seq = self.last_seq.max(seq);
        let Some(e) = parse_edit(body) else {
            return;
        };
        let could =
            self.shared.layers.contains_key(e.lid()) && self.shared.can_draw(e.lid(), &self.me);
        if !self.shared.apply(from, &e) {
            return;
        }
        let mine = from == self.me;
        match e {
            Edit::Layer { lid, .. } => {
                if !self.ids.contains_key(&lid) {
                    self.make_layer(c, &lid);
                } else {
                    self.refresh_lock(c, &lid);
                }
                if self.mine.remove(&lid)
                    && let Some(id) = self.ids.get(&lid)
                {
                    c.select(*id);
                }
            }
            Edit::Rename { lid, name } => {
                if let Some(id) = self.ids.get(&lid) {
                    c.set_layer(
                        *id,
                        LayerPatch {
                            name: Some(name),
                            ..Default::default()
                        },
                    );
                }
            }
            Edit::Look { lid, look } => {
                // (as if sent from here too, so it isn't sent back)
                self.look_sent.insert(lid.clone(), look.clone());
                if let Some(id) = self.ids.get(&lid)
                    && !mine
                {
                    c.set_layer(
                        *id,
                        LayerPatch {
                            look: Some(look),
                            ..Default::default()
                        },
                    );
                }
            }
            Edit::Adjust { lid, spec } => {
                self.adjust_sent.insert(lid.clone(), spec.clone());
                if let Some(id) = self.ids.get(&lid).copied()
                    && !mine
                {
                    self.later(c, Op::Adjust(id, spec));
                }
            }
            Edit::Mask {
                lid,
                on,
                set,
                clear,
            } => {
                if !mine {
                    self.mask_on.insert(lid.clone(), on);
                    let k = key(&lid, "mask");
                    if !on {
                        self.cache.shift_remove(&k);
                    } else {
                        let t = self.cache.entry(k.clone()).or_default();
                        for xy in clear {
                            t.remove(&xy);
                        }
                        for (x, y, s) in set {
                            if let Some(tile) = Tile::wire(&s) {
                                t.put((x, y), tile);
                            }
                        }
                    }
                    self.put_key(c, &k);
                }
            }
            Edit::Remove { lid } => {
                let id = self.ids.shift_remove(&lid);
                let prefix = format!("{lid}|");
                let gone: Vec<String> = self
                    .cache
                    .keys()
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect();
                for k in gone {
                    self.cache.shift_remove(&k);
                    self.pending.shift_remove(&k);
                }
                if let Some(id) = id {
                    self.later(c, Op::Remove(id));
                }
            }
            Edit::Move { lid, .. } | Edit::Place { lid, .. } => self.place(c, &lid),
            Edit::Block { lid, .. } => {
                for l in self.shared.members(&lid) {
                    self.place(c, &l);
                }
            }
            Edit::Group { gid, .. } => self.make_group(c, &gid),
            Edit::GroupSet {
                lid,
                name,
                visible,
                opacity,
            } => {
                let s = GroupSettings {
                    name,
                    visible,
                    opacity,
                };
                self.group_sent.insert(lid.clone(), s.clone());
                if let Some(g) = self.gids.get(&lid)
                    && !mine
                {
                    c.set_group(*g, &s);
                }
            }
            Edit::Tiles {
                lid,
                did,
                set,
                clear,
            } => {
                // this app's own changes are on its canvas already
                if !mine {
                    self.take_tiles(c, &lid, &did, &set, &clear);
                    self.notices.push(Notice::Tiles { from: from.into() });
                }
            }
            Edit::Lines {
                lid,
                did,
                del,
                put,
                base,
            } => {
                if !mine && !self.take_lines(c, &lid, &did, &del, &put, base.as_deref()) {
                    return;
                }
            }
            Edit::Pixels { lid } => {
                if let Some(id) = self.ids.get(&lid)
                    && !mine
                {
                    c.set_line_layer(*id, false);
                }
            }
            Edit::Sound {
                file,
                name,
                i,
                n,
                data,
            } => {
                if !mine && !self.take_sound(c, &file, &name, i, n, &data) {
                    return;
                }
            }
            Edit::Timeline { tl } => {
                // everyone's, this app's too, in the session's order: all end the same
                let tl = self.timeline_here(c, &tl, Some(from));
                c.set_timeline(tl);
            }
            Edit::Grant { lid, to, .. } => {
                self.refresh_lock(c, &lid);
                // taken back (or handed on): what was drawn here and not sent doesn't count
                if could && !self.shared.can_draw(&lid, &self.me) {
                    self.revert(c, &lid);
                }
                if to == self.me {
                    self.try_waiting(c);
                }
            }
            Edit::Revoke { lid, .. } => {
                self.refresh_lock(c, &lid);
                if could && !self.shared.can_draw(&lid, &self.me) {
                    self.revert(c, &lid);
                }
            }
            Edit::Deny { lid, to } => {
                if to == self.me {
                    let was = self.waiting.len();
                    self.waiting
                        .retain(|w| !w.needs.iter().any(|n| n.lid == lid));
                    if self.waiting.len() < was {
                        self.note("They said no: nothing was changed.");
                    }
                }
            }
            Edit::Ask { .. } | Edit::Ungroup { .. } => {}
        }
        self.drop_groups(c);
        self.changed();
    }

    /// Groups the session no longer has: taken apart here too.
    fn drop_groups(&mut self, c: &mut dyn CanvasPort) {
        // (a group made here counts once the session has it: until then it's on its way)
        for gid in self.shared.groups.keys() {
            self.groups_seen.insert(gid.clone());
        }
        let gone: Vec<(String, u32)> = self
            .gids
            .iter()
            .filter(|(gid, _)| {
                self.groups_seen.contains(*gid) && !self.shared.groups.contains_key(*gid)
            })
            .map(|(g, i)| (g.clone(), *i))
            .collect();
        for (gid, g) in gone {
            self.gids.shift_remove(&gid);
            self.group_sent.remove(&gid);
            self.later(c, Op::Ungroup(g));
        }
    }

    /// Make a shared group here (its layers first), and put its layers in it.
    fn make_group(&mut self, c: &mut dyn CanvasPort, gid: &str) {
        if self.gids.contains_key(gid) {
            return;
        }
        let Some(g) = self.shared.groups.get(gid) else {
            return;
        };
        let s = GroupSettings {
            name: g.name.clone(),
            visible: g.visible,
            opacity: g.opacity,
        };
        let Some(first) = self
            .shared
            .members(gid)
            .into_iter()
            .find(|l| self.ids.contains_key(l))
        else {
            return;
        };
        self.later(
            c,
            Op::MakeGroup {
                gid: gid.into(),
                first,
                s,
            },
        );
    }

    /// A piece of a sound file; once all are here, the studio gets the file. False when
    /// the pieces don't make one (together.ts stops there).
    fn take_sound(
        &mut self,
        c: &mut dyn CanvasPort,
        file: &str,
        name: &str,
        i: u32,
        n: u32,
        data: &str,
    ) -> bool {
        self.sounds_shared.insert(file.into());
        if c.sound(file).is_some() {
            return true;
        }
        let got = self
            .sound_parts
            .entry(file.into())
            .or_insert_with(|| (name.into(), vec![None; n as usize]));
        if got.1.len() != n as usize {
            return true;
        }
        got.1[i as usize] = Some(data.into());
        if got.1.iter().any(Option::is_none) {
            return true;
        }
        let (name, parts) = self.sound_parts.remove(file).unwrap();
        let mut bytes = Vec::new();
        for p in parts {
            match from_base64(&p.unwrap()) {
                Some(b) => bytes.extend(b),
                None => return false,
            }
        }
        c.put_sound(file, bytes, &name);
        self.changed();
        true
    }

    fn timeline_here(
        &mut self,
        c: &mut dyn CanvasPort,
        tl: &Value,
        from: Option<&str>,
    ) -> Timeline {
        let known: HashSet<u32> = self.ids.values().copied().collect();
        // (a layer's tracks come from those who may draw on it: a timeline sent before
        // the sender heard of someone's new drawing doesn't take it off their layer)
        let shared_here: HashMap<u32, bool> = self
            .ids
            .iter()
            .map(|(lid, id)| (*id, from.is_none_or(|f| self.shared.can_draw(lid, f))))
            .collect();
        let local = c.timeline();
        let ids = &self.ids;
        let id_of = |lid: &str| ids.get(lid).copied();
        let is_shared =
            |l: u32| known.contains(&l) && shared_here.get(&l).copied().unwrap_or(from.is_none());
        let (dids, did_of) = (&mut self.dids, &mut self.did_of);
        let mut lo = |did: &str| local_of(dids, did_of, c, did);
        from_shared(tl, &id_of, &mut lo, &local, &is_shared)
    }

    /// A message for this app alone (the project, when joining).
    pub fn on_direct(&mut self, c: &mut dyn CanvasPort, from: &str, body: &Value) {
        let Some(k) = body.get("k").and_then(Value::as_str) else {
            return;
        };
        let empty = Vec::new();
        match k {
            "snap.req" => {
                // (asked before this app has the project: answered once it has)
                if !self.askers.iter().any(|a| a == from) {
                    self.askers.push(from.into());
                }
                self.answer(c);
            }
            "snap.begin" if !self.synced => {
                let arr = |k: &str| body.get(k).and_then(Value::as_array).unwrap_or(&empty);
                self.shared.load(arr("layers"), arr("groups"), arr("asks"));
                // (no number: everything after the snapshot is put in, again if need be)
                self.snap_seq = body
                    .get("seq")
                    .and_then(Value::as_f64)
                    .filter(|s| s.is_finite())
                    .unwrap_or(0.0);
                self.snap_tl = body
                    .get("tl")
                    .filter(|t| t.is_object() || t.is_array())
                    .cloned();
            }
            "snap.tiles" => {
                let Some(lid) = body.get("lid").and_then(Value::as_str) else {
                    return;
                };
                let Some(did) = body
                    .get("did")
                    .and_then(Value::as_str)
                    .filter(|d| is_lid(d) || *d == "mask")
                else {
                    return;
                };
                let Some(set) = body.get("set").and_then(Value::as_array) else {
                    return;
                };
                if !self.shared.layers.contains_key(lid) {
                    return;
                }
                let k = key(lid, did);
                // a snapshot's tiles after its end (a host that sends some later): taken for
                // a drawing this app has nothing of yet (anything newer would have made it one)
                let late = self.synced;
                if late && self.cache.contains_key(&k) {
                    return;
                }
                if did == "mask" {
                    self.mask_on.insert(lid.into(), true);
                }
                let t = self.cache.entry(k.clone()).or_default();
                for s in set {
                    let (Some(x), Some(y), Some(s)) = (
                        js::int6(s.get(0)),
                        js::int6(s.get(1)),
                        s.get(2).and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if let Some(tile) = Tile::wire(s) {
                        t.put((x as i32, y as i32), tile);
                    }
                }
                if late {
                    self.put_key(c, &k);
                }
            }
            "snap.lines" if !self.synced => {
                let Some(lid) = body.get("lid").and_then(Value::as_str) else {
                    return;
                };
                if self
                    .shared
                    .layers
                    .get(lid)
                    .is_none_or(|l| l.adjust.is_some())
                {
                    return;
                }
                let Some(did) = body
                    .get("did")
                    .and_then(Value::as_str)
                    .filter(|d| is_lid(d))
                else {
                    return;
                };
                if let Some((del, put, base)) = body.as_object().and_then(read_line_edit) {
                    let (lid, did) = (lid.to_string(), did.to_string());
                    self.take_lines(c, &lid, &did, &del, &put, base.as_deref());
                }
            }
            "snap.sound" if !self.synced => {
                let mut b = body.clone();
                b["k"] = "sound".into();
                if let Some(Edit::Sound {
                    file,
                    name,
                    i,
                    n,
                    data,
                }) = parse_edit(&b)
                {
                    self.take_sound(c, &file, &name, i, n, &data);
                }
            }
            "snap.end" if !self.synced => self.snap_end(c),
            _ => {}
        }
    }

    fn snap_end(&mut self, c: &mut dyn CanvasPort) {
        // back from a break: layers deleted meanwhile go
        for (lid, id) in self.ids.clone() {
            if !self.shared.layers.contains_key(&lid) {
                self.ids.shift_remove(&lid);
                self.later(c, Op::Remove(id));
            }
        }
        for lid in self.shared.order.clone() {
            self.make_layer(c, &lid);
            self.refresh_lock(c, &lid);
        }
        // groups: those this app has (back from a break) put right, the others made
        self.drop_groups(c);
        for gid in self.shared.groups.keys().cloned().collect::<Vec<_>>() {
            let sg = &self.shared.groups[gid.as_str()];
            let s = GroupSettings {
                name: sg.name.clone(),
                visible: sg.visible,
                opacity: sg.opacity,
            };
            if let Some(g) = self.gids.get(&gid).copied() {
                c.set_group(g, &s);
                for lid in self.shared.members(&gid) {
                    self.place(c, &lid);
                }
            } else {
                self.make_group(c, &gid);
            }
        }
        for lid in self.ids.keys().cloned().collect::<Vec<_>>() {
            if self
                .shared
                .layers
                .get(&lid)
                .is_none_or(|l| l.group.is_none())
            {
                self.place(c, &lid);
            }
        }
        for k in self.cache.keys().cloned().collect::<Vec<_>>() {
            let own = self.rejoining
                && self
                    .shared
                    .layers
                    .get(split_key(&k).0)
                    .is_some_and(|l| l.owner == self.me);
            if !own {
                self.put_key(c, &k);
            }
        }
        if let Some(tl) = self.snap_tl.take() {
            let tl = self.timeline_here(c, &tl, None);
            c.set_timeline(tl);
        }
        self.synced = true;
        let snap_seq = self.snap_seq;
        let later: Vec<(u64, String, Value)> = std::mem::take(&mut self.buffer)
            .into_iter()
            .filter(|b| b.0 as f64 > snap_seq)
            .collect();
        for (seq, from, body) in later {
            self.on_edit(c, seq, &from, &body);
        }
        if self.rejoining {
            // this app's own drawings, as drawn here (compared with the session's): sent
            for (lid, id) in self.ids.clone() {
                if self
                    .shared
                    .layers
                    .get(&lid)
                    .is_some_and(|l| l.owner == self.me)
                {
                    let first = c.drawing_of(id);
                    let all: Vec<u32> = std::iter::once(first)
                        .chain(c.drawings_of_layer(id))
                        .collect();
                    self.touch(&all);
                }
            }
            self.rejoining = false;
            self.local_changed(c);
        } else {
            self.add_own_layer("", false);
        }
        self.changed();
        self.answer(c);
    }

    /// Whether something waits to go into the canvas (it was busy). While it does, call
    /// [`Together::retry`] now and then (the studio tries every 50 ms): waiting for this
    /// app's next change could be forever, as nothing may change here.
    pub fn waiting(&self) -> bool {
        !self.structure.is_empty() || !self.pending.is_empty()
    }

    /// Put in what couldn't go in yet (layer and group changes, then tiles and masks):
    /// those that can now do. Sends nothing.
    pub fn retry(&mut self, c: &mut dyn CanvasPort) {
        self.flush(c, false);
    }

    /// `count`: a change that keeps failing (e.g. the last layer can't go) is given up after
    /// 50 of this app's changes (tries while waiting for a busy canvas don't count).
    fn flush(&mut self, c: &mut dyn CanvasPort, count: bool) {
        while let Some(op) = self.structure.front().map(|l| l.op.clone()) {
            let done = self.run_op(c, &op);
            let front = self.structure.front_mut().unwrap();
            if !done {
                if !count {
                    break;
                }
                front.tries += 1;
                if front.tries < 50 {
                    break;
                }
            }
            self.structure.pop_front();
        }
        for k in self.pending.iter().cloned().collect::<Vec<_>>() {
            self.put_key(c, &k);
        }
    }

    /// The canvas changed here: send this app's changed tiles, settings, strokes and
    /// masks (and share layers made here). Call it a moment after changes (the studio
    /// waits 40 ms of quiet), and after the canvas stops being busy.
    pub fn local_changed(&mut self, c: &mut dyn CanvasPort) {
        if !self.synced {
            return;
        }
        self.flush(c, true);
        // a layer made here outside the session's way (duplicate, paste, import)
        let known: HashSet<u32> = self.ids.values().copied().collect();
        let all = c.layers();
        for l in &all {
            if known.contains(&l.id) || self.local.contains(&l.id) {
                continue;
            }
            // it goes on top, here as for everyone (the same stack everywhere)
            let top = self
                .shared
                .order
                .iter()
                .rev()
                .find_map(|x| self.ids.get(x).copied());
            match top {
                Some(top) if all.last().map(|x| x.id) != Some(l.id) || l.group != 0 => {
                    c.place(l.id, top, true, 0);
                }
                _ if l.group != 0 => {
                    c.place(l.id, 0, true, 0);
                }
                _ => {}
            }
            let lid = self.new_lid();
            self.ids.insert(lid.clone(), l.id);
            // (a line layer stays one: duplicated, pasted or imported)
            let lines = l.adjust.is_none() && c.is_line_layer(l.id);
            self.edit(
                Edit::Layer {
                    lid: lid.clone(),
                    name: l.name.clone(),
                    adjust: l.adjust.clone(),
                    lines,
                }
                .to_json(),
            );
            if let Some(a) = &l.adjust {
                self.adjust_sent.insert(lid, a.clone());
            }
        }
        // adjustment settings and masks changed here, on layers this app may change
        for l in &all {
            let Some(lid) = self.lid_of(l.id) else {
                continue;
            };
            if !self.shared.can_draw(&lid, &self.me) {
                continue;
            }
            if let Some(a) = &l.adjust {
                let sent = self
                    .adjust_sent
                    .get(&lid)
                    .or_else(|| self.shared.layers.get(&lid).and_then(|x| x.adjust.as_ref()));
                if sent != Some(a) {
                    self.adjust_sent.insert(lid.clone(), a.clone());
                    self.edit(
                        Edit::Adjust {
                            lid: lid.clone(),
                            spec: a.clone(),
                        }
                        .to_json(),
                    );
                }
            }
            let packed = c.mask_packed(l.id);
            let on = !packed.is_empty();
            let next = if on { unpack(&packed) } else { TileMap::new() };
            let mk = key(&lid, "mask");
            let d = self.cache.entry(mk.clone()).or_default().changes(&next);
            if on == self.mask_on.get(&lid).copied().unwrap_or(false) && d.is_empty() {
                if self.cache.get(&mk).is_some_and(Tiles::is_empty) && !on {
                    self.cache.shift_remove(&mk);
                }
                continue;
            }
            self.mask_on.insert(lid.clone(), on);
            let wire = wire_of(&d);
            if on {
                self.cache.get_mut(&mk).unwrap().sent(&d, &wire);
            } else {
                self.cache.shift_remove(&mk);
            }
            self.send_mask(&lid, on, &d, &wire);
        }
        // layer settings changed here (opacity, blend…) on layers this app may change
        let now: HashMap<u32, Option<Look>> =
            c.layers().into_iter().map(|l| (l.id, l.look)).collect();
        for (lid, id) in self.ids.clone() {
            let Some(Some(look)) = now.get(&id) else {
                continue;
            };
            if !self.shared.can_draw(&lid, &self.me) {
                continue;
            }
            let was = self
                .look_sent
                .get(&lid)
                .or_else(|| self.shared.layers.get(&lid).and_then(|l| l.look.as_ref()));
            if same_look(Some(look), was) {
                continue;
            }
            self.look_sent.insert(lid.clone(), look.clone());
            self.edit(
                Edit::Look {
                    lid,
                    look: look.clone(),
                }
                .to_json(),
            );
        }
        // group settings changed here, on groups this app may change
        for g in c.groups() {
            let Some(gid) = self.gid_of(g.id) else {
                continue;
            };
            if !self.shared.can_draw(&gid, &self.me) {
                continue;
            }
            if self
                .group_sent
                .get(&gid)
                .is_some_and(|w| w.same(&g.settings))
            {
                continue;
            }
            self.group_sent.insert(gid.clone(), g.settings.clone());
            self.edit(
                Edit::GroupSet {
                    lid: gid,
                    name: g.settings.name,
                    visible: g.settings.visible,
                    opacity: g.settings.opacity,
                }
                .to_json(),
            );
        }
        // what may have changed: each drawable layer's drawing showing, and drawings
        // touched elsewhere (an undo on another frame)
        let mut look: IndexMap<u32, u32> = IndexMap::new();
        let adjusts: HashSet<u32> = all
            .iter()
            .filter(|l| l.adjust.is_some())
            .map(|l| l.id)
            .collect();
        for id in self.ids.values().copied().collect::<Vec<_>>() {
            if !adjusts.contains(&id) {
                let d = c.drawing_of(id);
                look.insert(d, id);
            }
        }
        for d in std::mem::take(&mut self.touched) {
            if let Some(id) = c.layer_of_drawing(d)
                && self.lid_of(id).is_some()
            {
                look.insert(d, id);
            }
        }
        let me12 = self.me12();
        for (&d, &id) in &look {
            let Some(lid) = self.lid_of(id) else {
                continue;
            };
            // own layers, before the session has numbered them, count as drawable
            let mine = self.shared.can_draw(&lid, &self.me)
                || (!self.shared.layers.contains_key(&lid) && made_by(&lid, &me12));
            if !mine || d == 0 {
                continue;
            }
            let did = self.did_of(d);
            let next = unpack(&c.packed(d));
            let k = key(&lid, &did);
            let diff = self.cache.entry(k.clone()).or_default().changes(&next);
            if diff.is_empty() {
                continue;
            }
            let wire = wire_of(&diff);
            self.cache.get_mut(&k).unwrap().sent(&diff, &wire);
            self.send_tiles(&lid, &did, &diff, &wire);
        }
        // line layers: their strokes, and a layer turned into pixels
        for (&d, &id) in &look {
            let Some(lid) = self.lid_of(id) else {
                continue;
            };
            if d == 0 || !self.shared.can_draw(&lid, &self.me) {
                continue;
            }
            self.send_lines(c, &lid, id, d, false);
            if self.shared.layers.get(&lid).is_some_and(|l| l.lines)
                && c.has_lines()
                && !c.is_line_layer(id)
            {
                self.edit(Edit::Pixels { lid }.to_json());
            }
        }
        // (a merged layer's removal: after the merged pixels)
        for f in std::mem::take(&mut self.after_send) {
            match f {
                AfterSend::Remove(lid) => self.edit(Edit::Remove { lid }.to_json()),
                AfterSend::MergedGroup {
                    bottom,
                    name,
                    others,
                } => {
                    self.edit(
                        Edit::Rename {
                            lid: bottom.clone(),
                            name,
                        }
                        .to_json(),
                    );
                    let to = self.shared.order.iter().position(|x| *x == bottom);
                    // (together.ts sends indexOf's -1 when it's gone, which every app drops)
                    let mut e = Edit::Place {
                        lid: bottom,
                        to: to.unwrap_or(0) as u32,
                        gid: String::new(),
                    }
                    .to_json();
                    if to.is_none() {
                        e["to"] = (-1).into();
                    }
                    self.edit(e);
                    for lid in others {
                        self.edit(Edit::Remove { lid }.to_json());
                    }
                }
            }
        }
    }

    fn send_tiles(&mut self, lid: &str, did: &str, d: &Changes, wire: &[Arc<[u8]>]) {
        let set = tile_sets(d, wire);
        for (part, gone) in tile_parts(&set, &d.clear) {
            self.edit(
                Edit::Tiles {
                    lid: lid.into(),
                    did: did.into(),
                    set: part.to_vec(),
                    clear: gone.to_vec(),
                }
                .to_json(),
            );
        }
    }

    fn send_mask(&mut self, lid: &str, on: bool, d: &Changes, wire: &[Arc<[u8]>]) {
        let set = tile_sets(d, wire);
        for (part, gone) in tile_parts(&set, &d.clear) {
            self.edit(
                Edit::Mask {
                    lid: lid.into(),
                    on,
                    set: part.to_vec(),
                    clear: gone.to_vec(),
                }
                .to_json(),
            );
        }
    }

    fn send_snapshot(&mut self, c: &mut dyn CanvasPort, to: &str) {
        let (layers, groups, asks) = self.shared.snapshot();
        let tl = self.shared_timeline(c);
        self.direct(
            to,
            js::object(vec![
                ("k", "snap.begin".into()),
                ("seq", self.last_seq.into()),
                ("layers", layers.into()),
                ("groups", groups.into()),
                ("asks", asks.into()),
                ("tl", tl),
            ]),
        );
        let tl = self.shared_timeline(c);
        self.share_sounds(c, &tl, Some(to));
        let mut msgs = Vec::new();
        let keys: Vec<String> = self
            .cache
            .keys()
            .filter(|k| self.shared.layers.contains_key(split_key(k).0))
            .cloned()
            .collect();
        for k in keys {
            let (lid, did) = split_key(&k);
            // (each tile's bytes as they came or went; a tile never sent, from this app's canvas)
            let mut t = std::mem::take(self.cache.get_mut(&k).unwrap());
            let tiles = {
                let mut here = || self.canvas_tiles(c, lid, did);
                t.for_wire(&mut here)
            };
            *self.cache.get_mut(&k).unwrap() = t;
            let mut i = 0;
            while i < tiles.len().max(1) {
                let set: Vec<Value> = tiles
                    [i.min(tiles.len())..(i + TILES_PER_EDIT).min(tiles.len())]
                    .iter()
                    .map(|((x, y), z)| {
                        Value::from(vec![Value::from(*x), Value::from(*y), wire_text(z).into()])
                    })
                    .collect();
                msgs.push(js::object(vec![
                    ("k", "snap.tiles".into()),
                    ("lid", lid.into()),
                    ("did", did.into()),
                    ("set", set.into()),
                ]));
                i += TILES_PER_EDIT;
            }
        }
        for m in msgs {
            self.direct(to, m);
        }
        for (k, list) in self.lines_synced.clone() {
            let (lid, did) = split_key(&k);
            let Some(l) = self.shared.layers.get(lid).filter(|l| l.adjust.is_none()) else {
                continue;
            };
            let lines = l.lines;
            // a plain layer's strokes go with the pixels kept under them
            let d = self.local_of(c, did);
            let base = if !lines && d != 0 {
                c.base_all(d)
            } else {
                None
            };
            if !lines && base.is_none() {
                continue;
            }
            for e in with_base(line_edits(lid, did, &[], &list), lid, did, base.as_deref()) {
                let mut e = e.to_json();
                e["k"] = "snap.lines".into();
                self.direct(to, e);
            }
        }
        self.direct(to, js::object(vec![("k", "snap.end".into())]));
    }

    /// Someone's editable strokes put into a drawing here (its pixels come as tiles); a
    /// plain layer's with what changed in its base. False when the base isn't base64
    /// (together.ts stops there).
    fn take_lines(
        &mut self,
        c: &mut dyn CanvasPort,
        lid: &str,
        did: &str,
        del: &[String],
        put: &[(u32, String)],
        base: Option<&str>,
    ) -> bool {
        if !c.has_lines() {
            return true;
        }
        let d = self.local_of(c, did);
        let plain = !self.shared.layers.get(lid).is_some_and(|l| l.lines);
        if plain && let Some(b) = base.filter(|b| !b.is_empty()) {
            let Some(bytes) = from_base64(b) else {
                return false;
            };
            c.put_base_changes(d, &bytes);
        }
        let next = Arc::new(apply_line_edit(&c.line_strokes(d), del, put));
        self.lines_synced.insert(key(lid, did), next.clone());
        c.set_line_strokes(d, next, plain);
        true
    }

    fn take_tiles(
        &mut self,
        c: &mut dyn CanvasPort,
        lid: &str,
        did: &str,
        set: &[TileSet],
        clear: &[TileClear],
    ) {
        let d = self.local_of(c, did);
        let k = key(lid, did);
        // drawing on it too: send this app's own changes first, so putting theirs in keeps them
        if self.ids.contains_key(lid) && self.shared.can_draw(lid, &self.me) {
            let next = unpack(&c.packed(d));
            let diff = self.cache.entry(k.clone()).or_default().changes(&next);
            if !diff.is_empty() {
                let wire = wire_of(&diff);
                self.cache.get_mut(&k).unwrap().sent(&diff, &wire);
                self.send_tiles(lid, did, &diff, &wire);
            }
        }
        let t = self.cache.entry(k.clone()).or_default();
        for xy in clear {
            t.remove(xy);
        }
        for (x, y, s) in set {
            if let Some(tile) = Tile::wire(s) {
                t.put((*x, *y), tile);
            }
        }
        self.put_key(c, &k);
    }

    /// A layer this app may no longer draw on, back to the session's copy (its drawings'
    /// tiles, its mask and its strokes as last synced; a drawing the session has nothing
    /// of, empty). Mid-stroke, once the stroke is done.
    fn revert(&mut self, c: &mut dyn CanvasPort, lid: &str) {
        let Some(id) = self.ids.get(lid).copied() else {
            return;
        };
        let first = c.drawing_of(id);
        for d in unique(std::iter::once(first).chain(c.drawings_of_layer(id))) {
            let k = key(lid, &self.did_of(d));
            self.cache.entry(k.clone()).or_default();
            self.put_key(c, &k);
            if let Some(lines) = self.lines_synced.get(&k).cloned() {
                let plain = !self.shared.layers.get(lid).is_some_and(|l| l.lines);
                c.set_line_strokes(d, lines, plain);
            }
        }
        self.put_key(c, &key(lid, "mask"));
    }

    fn make_layer(&mut self, c: &mut dyn CanvasPort, lid: &str) {
        if self.ids.contains_key(lid) {
            return;
        }
        let Some(l) = self.shared.layers.get(lid).cloned() else {
            return;
        };
        let id = c.add_layer(&l.name, l.adjust.as_deref());
        if id == 0 {
            return;
        }
        self.ids.insert(lid.into(), id);
        if let Some(a) = &l.adjust {
            self.adjust_sent.insert(lid.into(), a.clone());
        }
        if l.lines {
            c.set_line_layer(id, true);
        }
        if let Some(look) = l.look {
            c.set_layer(
                id,
                LayerPatch {
                    look: Some(look),
                    ..Default::default()
                },
            );
        }
        self.refresh_lock(c, lid);
    }

    /// Put a shared layer next to its neighbour in the shared order.
    fn place(&mut self, c: &mut dyn CanvasPort, lid: &str) {
        let Some(id) = self.ids.get(lid).copied() else {
            return;
        };
        let Some(i) = self.shared.order.iter().position(|x| x == lid) else {
            return;
        };
        let at = |j: Option<usize>| {
            j.and_then(|j| self.shared.order.get(j))
                .and_then(|x| self.ids.get(x))
                .copied()
        };
        let below = at(i.checked_sub(1));
        let above = at(Some(i + 1));
        let gid = self.shared.layers.get(lid).and_then(|l| l.group.clone());
        let (reference, up) = match (below, above) {
            (Some(b), _) => (b, true),
            (None, Some(a)) => (a, false),
            (None, None) => (0, true),
        };
        self.later(
            c,
            Op::Place {
                id,
                reference,
                above: up,
                gid,
            },
        );
    }

    /// Do it now, or (the canvas busy mid-stroke, or others waiting) after the next change.
    fn later(&mut self, c: &mut dyn CanvasPort, op: Op) {
        if !self.structure.is_empty() || !self.run_op(c, &op) {
            self.structure.push_back(Later { op, tries: 0 });
        }
    }

    fn run_op(&mut self, c: &mut dyn CanvasPort, op: &Op) -> bool {
        match op {
            Op::Remove(id) => c.remove_layer(*id),
            Op::Adjust(id, spec) => c.set_adjust(*id, spec),
            Op::Ungroup(g) => c.ungroup(*g),
            Op::Place {
                id,
                reference,
                above,
                gid,
            } => {
                let group = gid
                    .as_ref()
                    .and_then(|g| self.gids.get(g))
                    .copied()
                    .unwrap_or(0);
                c.place(*id, *reference, *above, group)
            }
            Op::MakeGroup { gid, first, s } => {
                if self.gids.contains_key(gid) || !self.shared.groups.contains_key(gid) {
                    return true;
                }
                let Some(first) = self.ids.get(first).copied() else {
                    return true;
                };
                let local = c.add_group(first);
                if local == 0 {
                    return false;
                }
                self.gids.insert(gid.clone(), local);
                c.set_group(local, s);
                self.group_sent.insert(gid.clone(), s.clone());
                for lid in self.shared.members(gid) {
                    self.place(c, &lid);
                }
                true
            }
        }
    }

    fn refresh_lock(&mut self, c: &mut dyn CanvasPort, lid: &str) {
        if let Some(id) = self.ids.get(lid) {
            c.set_layer(
                *id,
                LayerPatch {
                    locked: Some(!self.shared.can_draw(lid, &self.me)),
                    ..Default::default()
                },
            );
        }
    }

    /// This app's own copy of a shared drawing's tiles (a mask's: did "mask").
    fn canvas_tiles(&mut self, c: &mut dyn CanvasPort, lid: &str, did: &str) -> TileMap {
        if did == "mask" {
            return self
                .ids
                .get(lid)
                .map_or_else(TileMap::new, |id| unpack(&c.mask_packed(*id)));
        }
        let d = local_of(&mut self.dids, &mut self.did_of, c, did);
        unpack(&c.packed(d))
    }

    /// Put a shared drawing's synced tiles into the canvas ("lid|did").
    fn put_key(&mut self, c: &mut dyn CanvasPort, k: &str) {
        let (lid, did) = split_key(k);
        let ok = if did == "mask" {
            let Some(id) = self.ids.get(lid).copied() else {
                return;
            };
            let on = self
                .mask_on
                .get(lid)
                .copied()
                .unwrap_or(self.cache.contains_key(k));
            let bytes = if on {
                let t = self.cache.get(k).cloned().unwrap_or_default();
                let mut here = || self.canvas_tiles(c, lid, did);
                pack(&t.pixels(&mut here))
            } else {
                Vec::new()
            };
            c.put_mask(id, &bytes)
        } else {
            let Some(t) = self.cache.get(k) else {
                return;
            };
            let d = local_of(&mut self.dids, &mut self.did_of, c, did);
            // (all as they came over the wire: the app may take them as they are)
            let quick = t.all_deflated().and_then(|z| c.put_deflated(d, &z));
            match quick {
                Some(ok) => ok,
                None => {
                    let t = t.clone();
                    let mut here = || self.canvas_tiles(c, lid, did);
                    let bytes = pack(&t.pixels(&mut here));
                    c.put(d, &bytes)
                }
            }
        };
        if ok {
            self.pending.shift_remove(k);
        } else {
            self.pending.insert(k.into());
        }
    }
}

fn local_of(
    dids: &mut IndexMap<String, u32>,
    did_of: &mut HashMap<u32, String>,
    c: &mut dyn CanvasPort,
    did: &str,
) -> u32 {
    if let Some(d) = dids.get(did).filter(|d| **d != 0) {
        return *d;
    }
    let d = c.new_drawing();
    dids.insert(did.into(), d);
    did_of.insert(d, did.into());
    d
}
