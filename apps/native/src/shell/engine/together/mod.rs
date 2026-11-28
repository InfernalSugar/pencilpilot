//! Drawing together (ADR-0013): a live session with others, the native app's side of it,
//! compatible with the current app's (`together.ts`, `share.rs`): the same edits, the same
//! join, the same live strokes, cursors and chat.
//!
//! - [`net`]: the session on its own thread (nothing here waits on the network); tiles
//!   decoded and compressed there.
//! - [`model`]: shared layers and who may draw on each.
//! - [`live`]: strokes as they're drawn, both ways.
//! - this file: hosting, joining, leaving; the session's edits put into the painter
//!   between frames (layers, tiles, the timeline, access), this app's changes sent (tiles
//!   of its strokes, layers, the timeline), members, cursors, chat, and what `--stats`
//!   records.
//!
//! **Solo projects never connect:** nothing here runs until Host or Join.

pub mod live;
pub mod model;
pub mod net;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Engine;
use crate::state::{Action, State};

/// The actions this part carries out ([`Action::Other`]).
pub mod ops {
    /// the header's button: the dialog
    pub const OPEN: &str = "Draw together";
    pub const CLOSE: &str = "Together: close";
    pub const HOST: &str = "Together: host";
    pub const JOIN: &str = "Together: join";
    pub const LEAVE: &str = "Together: leave";
    pub const COPY: &str = "Together: copy the code";
    pub const SAY: &str = "Together: say";
    pub const CHAT: &str = "Together: chat";
    /// + a layer's id: ask to draw on it
    pub const ASK: &str = "Together: ask ";
    /// + a layer's id, a space, a member's id: let them draw
    pub const LET: &str = "Together: let ";
    /// + the same: hand it over
    pub const HAND: &str = "Together: hand ";
    pub const DENY: &str = "Together: deny ";
    pub const REVOKE: &str = "Together: revoke ";
    /// the layers panel's lock on someone else's layer: ask for the active one
    pub const ASK_ACTIVE: &str = "Together: ask for this layer";
}

/// Chat lines kept (the newest), and the longest message.
pub const KEEP_CHAT: usize = 200;
pub const MAX_CHAT_CHARS: usize = 500;
/// Cursors go at most this often; a cursor without news this long goes.
const CURSOR_GAP: Duration = Duration::from_millis(50);
const CURSOR_STALE: Duration = Duration::from_secs(4);
/// The timeline is compared with what was sent this often (not mid-stroke).
const TIMELINE_EVERY: Duration = Duration::from_millis(150);
/// Work a tick may do with the session's edits and the tiles going out (the rest waits
/// for the next tick: frames stay on time).
const TICK_BUDGET: Duration = Duration::from_millis(3);
/// Tiles copied out of the painter for sending, a tick at most.
const TILES_A_TICK: usize = 192;

/// A member's colour: picked from their id, the same in every app (share.ts's).
pub fn member_colour(id: &str) -> [f32; 4] {
    const C: [u32; 8] = [
        0xe4572e, 0x2e86ab, 0x7b2cbf, 0x2a9d8f, 0xd1495b, 0xef8a17, 0x3a5a40, 0xc9184a,
    ];
    let mut h: u32 = 0;
    for u in id.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as u32);
    }
    pencilpilot_ui::theme::hex(C[(h % 8) as usize])
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub from: String,
    pub name: String,
    pub mine: bool,
    /// someone joined or left (no sender)
    pub system: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorView {
    pub id: String,
    pub name: String,
    pub colour: [f32; 4],
    /// document pixels
    pub x: f32,
    pub y: f32,
}

/// A shared layer as the layers panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
    pub lid: String,
    pub mine: bool,
    pub can_draw: bool,
    pub owner: String,
    pub asked: bool,
}

/// A shared layer as the dialog lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub lid: String,
    pub name: String,
    pub owner: String,
    pub mine: bool,
    pub can_draw: bool,
    pub asked: bool,
    /// others the owner lets draw (id, name)
    pub writers: Vec<(String, String)>,
    /// asks for the owner: (id, name, what they ask, to have it handed over, to draw)
    pub asks: Vec<(String, String, String, bool, bool)>,
}

/// What the interface shows and changes ([`State::together`]).
#[derive(Debug, Clone, Default)]
pub struct TogetherUi {
    pub open: bool,
    /// the dialog was up at the last pass (a click elsewhere puts it away)
    was_open: bool,
    /// this person's name, as others see it
    pub name: String,
    /// the code typed or pasted to join
    pub code: String,
    /// connecting
    pub busy: bool,
    pub error: String,
    /// the session in progress
    pub info: Option<net::Info>,
    /// the project has arrived (joining)
    pub synced: bool,
    /// the code was just copied
    pub copied: bool,
    pub chat: Vec<ChatLine>,
    pub chat_open: bool,
    pub chat_text: String,
    pub unread: u32,
    /// others' cursors
    pub cursors: Vec<CursorView>,
    /// per layer as [`State::layers`] has them (top first): its sharing, if shared
    pub access: Vec<Option<Access>>,
    /// the shared layers, top first (the dialog's list)
    pub rows: Vec<Row>,
    /// something to tell this person, and until when
    pub note: Option<(String, Instant)>,
    /// text for the window to put on the system's clipboard
    pub copy_out: Option<String>,
    /// this person's pointer over the canvas (document pixels), from the shell
    pub pointer: Option<(f32, f32)>,
}

impl TogetherUi {
    pub fn was_open(&self) -> bool {
        self.was_open
    }

    pub fn set_was_open(&mut self, on: bool) {
        self.was_open = on;
    }

    pub fn live(&self) -> bool {
        self.info.is_some()
    }

    fn say(&mut self, text: &str) {
        self.note = Some((text.to_string(), Instant::now() + Duration::from_secs(4)));
    }

    fn line(&mut self, l: ChatLine) {
        if !l.system && !l.mine && !self.chat_open {
            self.unread += 1;
        }
        self.chat.push(l);
        let over = self.chat.len().saturating_sub(KEEP_CHAT);
        self.chat.drain(..over);
    }

    pub fn member_name(&self, id: &str) -> String {
        self.info
            .as_ref()
            .and_then(|i| i.members.iter().find(|m| m.id == id))
            .map(|m| m.name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Someone".into())
    }
}

/// A remote edit's times, for `--stats` (CLOCK_MONOTONIC ns).
#[derive(Debug, Clone)]
pub struct EditStat {
    /// "tiles", "live", "snapshot"
    pub kind: &'static str,
    pub from: String,
    pub tiles: usize,
    pub received_ns: u64,
    pub applied_ns: u64,
    /// the sender's own time (its stroke's end, or the live point's), when it sent one
    pub sent_ns: Option<u64>,
}

/// What `--stats` records of a session.
#[derive(Debug, Default)]
pub struct Stats {
    pub edits: Vec<EditStat>,
    /// this app's: (live points sent, tiles sent)
    pub live_sent: u64,
    pub tiles_sent: u64,
    pub joined_ns: Option<u64>,
    pub synced_ns: Option<u64>,
    pub started_ns: Option<u64>,
    /// the most members seen at once
    pub members_peak: usize,
    /// remote tile edits that waited (the painter busy) before going in
    pub deferred: u64,
}

/// A drawing's tiles to send.
#[derive(Debug)]
enum Job {
    Drawing {
        /// to one member (a snapshot), else to everyone (an edit)
        to: Option<String>,
        lid: String,
        did: String,
        d: u32,
        /// which tiles (None: all it has)
        keys: Option<Vec<(i32, i32)>>,
        /// when the change was made (its stroke's end), ns
        t_ns: u64,
    },
    /// the snapshot's end (after its tiles)
    End(String),
}

/// Remote tiles waiting to go in (the painter was busy with that drawing).
struct Waiting {
    from: String,
    tiles: net::Tiles,
    received_ns: u64,
    sent_ns: Option<u64>,
}

/// The engine's share of drawing together.
#[derive(Default)]
pub struct Together {
    net: Option<net::Net>,
    /// wakes the window when the session has something (set by the shell)
    pub waker: Option<pencilpilot_window::Waker>,
    /// a check: no relays (loopback only), and `--stats` times on the wire
    pub offline: bool,
    pub stats_on: bool,
    /// host or join as soon as the engine is ready (`--host`, `--join`)
    pub auto: Option<net::Start>,
    /// where to write the invite once hosting (`--host`): stdout
    pub print_invite: bool,
    /// this computer's key, when not the one kept on disk (tests)
    pub identity: Option<pencilpilot_share::Identity>,
    me: String,
    shared: model::Shared,
    /// shared layer → the painter's layer
    ids: HashMap<String, u32>,
    /// shared drawing ↔ the painter's
    dids: HashMap<String, u32>,
    did_of: HashMap<u32, String>,
    synced: bool,
    /// edits before the project arrived (joining)
    buffer: Vec<(u64, String, Value, Option<net::Tiles>, u64)>,
    snap_seq: u64,
    snap_tl: Option<Value>,
    snap_frame: Option<u32>,
    last_seq: u64,
    n: u64,
    /// layers made here, made active once the session numbers them
    mine: HashSet<String>,
    /// the painter's layers that aren't shared (from before joining)
    local: HashSet<u32>,
    pub live_in: live::LiveIn,
    pub live_out: live::LiveOut,
    waiting: VecDeque<Waiting>,
    outbox: VecDeque<Job>,
    cursors: HashMap<String, (f32, f32, Instant)>,
    cursor_sent: Option<((f32, f32), Instant)>,
    tl_sent: String,
    tl_check: Option<Instant>,
    looks_sent: HashMap<String, (bool, f32)>,
    /// layer removals and moves that couldn't happen yet (the painter busy)
    structure: Vec<String>,
    /// the interface's lists need making again
    ui_dirty: bool,
    /// when this app's last stroke ended (for the tiles it sends), ns
    stroke_end_ns: u64,
    pub stats: Stats,
}

fn now_ns() -> u64 {
    pencilpilot_window::clock::now_ns()
}

impl Together {
    pub fn live(&self) -> bool {
        self.net.is_some()
    }

    /// A new id for a layer made here (the maker's first 12 id characters and a number).
    fn new_lid(&mut self) -> String {
        if self.n == 0 {
            // (a random start, as the current app's: ids from before a break don't clash)
            self.n = (now_ns() % 100_000_000).max(1);
        }
        self.n += 1;
        format!("{}:{}", &self.me[..12.min(self.me.len())], self.n)
    }

    /// A drawing's shared id (given one the first time it's shared).
    fn did_of(&mut self, d: u32) -> String {
        if let Some(x) = self.did_of.get(&d) {
            return x.clone();
        }
        let did = format!("{}:{d}", &self.me[..12.min(self.me.len())]);
        self.did_of.insert(d, did.clone());
        self.dids.insert(did.clone(), d);
        did
    }

    fn lid_of(&self, layer: u32) -> Option<&String> {
        self.ids.iter().find(|(_, v)| **v == layer).map(|(k, _)| k)
    }

    fn send(&self, c: net::Cmd) {
        if let Some(n) = &self.net {
            n.send(c);
        }
    }

    fn edit(&self, v: Value) {
        self.send(net::Cmd::Edit(v));
    }
}

/// A shared drawing's id here (a new one the first time).
fn local_of(e: &mut Engine, did: &str) -> u32 {
    if let Some(&d) = e.together.dids.get(did) {
        return d;
    }
    let d = e.painter.new_drawing();
    e.together.dids.insert(did.to_string(), d);
    e.together.did_of.insert(d, did.to_string());
    d
}

/// Starts hosting or joining (from the dialog or `--host` / `--join`).
fn start(e: &mut Engine, s: &mut State, how: net::Start) {
    if e.together.live() {
        return;
    }
    let id = match e.together.identity.clone().map_or_else(net::identity, Ok) {
        Ok(id) => id,
        Err(err) => {
            s.together.error = err;
            return;
        }
    };
    let name = s.together.name.trim().chars().take(24).collect::<String>();
    s.together.busy = true;
    s.together.error.clear();
    e.together.stats.started_ns = Some(now_ns());
    let relays = net::relays(e.together.offline);
    e.together.net = Some(net::Net::start(
        how,
        id,
        name,
        relays,
        e.together.waker.clone(),
    ));
}

/// What the interface asked for. Returns whether it was this part's (and the picture may
/// have changed).
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    let live = e.together.live() && e.together.synced;
    let name = match a {
        Action::Other(n) => n.as_str(),
        // a new layer while drawing together is a shared one, made when the session numbers
        // it (on top, for everyone)
        Action::NewLayer if live => {
            let lid = e.together.new_lid();
            e.together.mine.insert(lid.clone());
            let name = format!("{}’s layer", s.together.member_name(&e.together.me));
            e.together
                .edit(json!({"k": "layer", "lid": lid, "name": name}));
            return true;
        }
        _ => return false,
    };
    use super::frames::ops as fops;
    if live {
        let active = s.sheet.tracks.get(s.active_layer).map_or(0, |t| t.layer);
        let lid = e.together.lid_of(active).cloned();
        let mine = lid
            .as_ref()
            .is_some_and(|l| e.together.shared.owns(l, &e.together.me));
        match name {
            fops::DELETE_LAYER => {
                match (&lid, mine) {
                    (Some(l), true) => e.together.edit(json!({"k": "remove", "lid": l})),
                    (Some(_), false) => s
                        .together
                        .say("Only the person who made a shared layer can delete it."),
                    _ => return false,
                }
                return true;
            }
            fops::LAYER_UP | fops::LAYER_DOWN => {
                let Some(l) = lid else { return false };
                if !mine {
                    s.together
                        .say("Only the person who made a shared layer can move it.");
                    return true;
                }
                let order = e.together.shared.order();
                let at = order.iter().position(|x| *x == l).unwrap_or(0);
                let to = if name == fops::LAYER_UP {
                    (at + 1).min(order.len().saturating_sub(1))
                } else {
                    at.saturating_sub(1)
                };
                if to != at {
                    e.together.edit(json!({"k": "move", "lid": l, "to": to}));
                }
                return true;
            }
            fops::DUPLICATE_LAYER => {
                s.together
                    .say("Duplicating layers waits until you leave the shared session.");
                return true;
            }
            _ => {}
        }
    }
    match name {
        ops::OPEN => {
            s.together.open = !s.together.open;
            s.together.copied = false;
        }
        ops::CLOSE => s.together.open = false,
        ops::HOST => {
            if s.together.name.trim().is_empty() {
                s.together.error = "Type your name first: it's what the others see.".into();
            } else {
                start(e, s, net::Start::Host);
            }
        }
        ops::JOIN => {
            let Some(code) = net::clean_invite(&s.together.code) else {
                s.together.error = "That isn't an invite code (they start with pp1-).".into();
                return true;
            };
            if s.together.name.trim().is_empty() {
                s.together.error = "Type your name first: it's what the others see.".into();
            } else if s.project.unsaved {
                s.together.error =
                    "Save this project first: joining opens the shared project in its place."
                        .into();
            } else {
                start(e, s, net::Start::Join(code));
            }
        }
        ops::LEAVE => leave(e, s, None),
        ops::COPY => {
            if let Some(i) = &s.together.info {
                s.together.copy_out = Some(i.invite.clone());
                s.together.copied = true;
            }
        }
        ops::CHAT => {
            s.together.chat_open = !s.together.chat_open;
            if s.together.chat_open {
                s.together.unread = 0;
            }
        }
        ops::SAY => {
            let text: String = s
                .together
                .chat_text
                .trim()
                .chars()
                .take(MAX_CHAT_CHARS)
                .collect();
            s.together.chat_text.clear();
            let Some(info) = &s.together.info else {
                return true;
            };
            if text.is_empty() {
                return true;
            }
            for m in &info.members {
                if m.id != info.me.id {
                    e.together.send(net::Cmd::Direct(
                        m.id.clone(),
                        json!({"k": "chat", "text": text}),
                    ));
                }
            }
            let me = info.me.clone();
            s.together.line(ChatLine {
                from: me.id,
                name: me.name,
                mine: true,
                system: false,
                text,
            });
        }
        ops::ASK_ACTIVE => {
            let active = s.sheet.tracks.get(s.active_layer).map_or(0, |t| t.layer);
            if let Some(l) = e.together.lid_of(active).cloned() {
                ask(e, s, &l);
            }
        }
        n if n.starts_with(ops::ASK) => {
            let l = n[ops::ASK.len()..].to_string();
            ask(e, s, &l);
        }
        n if [ops::LET, ops::HAND, ops::DENY, ops::REVOKE]
            .iter()
            .any(|p| n.starts_with(p)) =>
        {
            let (p, rest) = [ops::LET, ops::HAND, ops::DENY, ops::REVOKE]
                .iter()
                .find_map(|p| n.strip_prefix(p).map(|r| (*p, r)))
                .unwrap_or(("", ""));
            let Some((lid, to)) = rest.split_once(' ') else {
                return true;
            };
            let v = match p {
                ops::LET => json!({"k": "grant", "lid": lid, "to": to, "handover": false}),
                ops::HAND => json!({"k": "grant", "lid": lid, "to": to, "handover": true}),
                ops::DENY => json!({"k": "deny", "lid": lid, "to": to}),
                _ => json!({"k": "revoke", "lid": lid, "to": to}),
            };
            e.together.edit(v);
        }
        _ => return false,
    }
    true
}

fn ask(e: &mut Engine, s: &mut State, lid: &str) {
    let t = &e.together;
    if t.shared.can_draw(lid, &t.me) {
        return;
    }
    if t.shared.asks.iter().any(|a| a.lid == lid && a.from == t.me) {
        s.together.say("You've asked: it's theirs to say yes.");
        return;
    }
    t.edit(json!({"k": "ask", "lid": lid, "why": "draw", "own": false}));
    let owner = t
        .shared
        .get(lid)
        .map(|l| l.owner.clone())
        .unwrap_or_default();
    s.together.say(&format!(
        "Asked {}: you can draw once they say yes.",
        s.together.member_name(&owner)
    ));
}

/// Leaves the session (`why`: it ended on its own, and why). The shared layers stay
/// (everyone keeps a full copy), unlocked.
pub fn leave(e: &mut Engine, s: &mut State, why: Option<String>) {
    let t = &mut e.together;
    if let Some(n) = t.net.take() {
        n.leave(Duration::from_millis(500));
    }
    t.live_in.clear(&mut e.painter);
    for &id in t.ids.values() {
        if let Some(l) = e.painter.layers().iter().find(|l| l.id == id) {
            let mut p = l.props.clone();
            if p.locked {
                p.locked = false;
                e.painter.set_layer_props(id, p, false);
            }
        }
    }
    e.painter.track_edits(false);
    let stats = std::mem::take(&mut t.stats);
    let (offline, stats_on, waker, identity) =
        (t.offline, t.stats_on, t.waker.clone(), t.identity.clone());
    *t = Together {
        offline,
        stats_on,
        waker,
        identity,
        stats,
        ..Default::default()
    };
    let ui = &mut s.together;
    ui.info = None;
    ui.busy = false;
    ui.synced = false;
    ui.chat.clear();
    ui.unread = 0;
    ui.chat_open = false;
    ui.cursors.clear();
    ui.access.clear();
    ui.rows.clear();
    if let Some(w) = why {
        ui.error = w;
        ui.open = true;
    }
}

/// Once a loop, before drawing: the session's news put in, this app's changes sent.
/// Returns whether the picture changed.
pub fn tick(e: &mut Engine, s: &mut State, now: Instant) -> bool {
    // (`--host` / `--join`: once the project given at the start is open)
    if s.project.startup.is_none()
        && let Some(how) = e.together.auto.take()
    {
        start(e, s, how);
    }
    if !e.together.live() {
        return false;
    }
    let until = Instant::now() + TICK_BUDGET;
    let mut changed = false;
    // what the session sent
    while Instant::now() < until {
        let Some(msg) = e.together.net.as_ref().and_then(net::Net::try_recv) else {
            break;
        };
        changed |= take(e, s, msg);
        if !e.together.live() {
            return true;
        }
    }
    if e.together.synced {
        changed |= retry(e, s);
        local_changes(e, s, now);
    }
    // this app's live stroke and cursor
    for m in e.together.live_out.take() {
        e.together.stats.live_sent += 1;
        e.together.send(net::Cmd::Presence(m));
    }
    send_out(e, s, until);
    changed |= e.together.live_in.prune(&mut e.painter);
    if let Some(p) = s.together.pointer
        && e.together
            .cursor_sent
            .is_none_or(|(q, t)| q != p && t.elapsed() >= CURSOR_GAP)
    {
        let (x, y) = (p.0.round(), p.1.round());
        e.together.send(net::Cmd::Presence(json!({"x": x, "y": y})));
        e.together.cursor_sent = Some((p, Instant::now()));
    }
    let before = e.together.cursors.len();
    e.together
        .cursors
        .retain(|_, c| c.2.elapsed() < CURSOR_STALE);
    if e.together.cursors.len() != before {
        e.together.ui_dirty = true;
    }
    if let Some((_, until)) = &s.together.note
        && Instant::now() >= *until
    {
        s.together.note = None;
        changed = true;
    }
    if std::mem::take(&mut e.together.ui_dirty) {
        refresh_ui(e, s);
        changed = true;
    }
    changed
}

/// When [`tick`] next has something to do (news from the session wakes the window).
pub fn next_tick_at(e: &Engine, s: &State) -> Option<Instant> {
    let t = &e.together;
    if t.auto.is_some() {
        return Some(Instant::now());
    }
    if !t.live() {
        return None;
    }
    let now = Instant::now();
    let mut at: Vec<Instant> = Vec::new();
    if !t.outbox.is_empty() || !t.waiting.is_empty() || !t.live_out.out.is_empty() {
        at.push(now);
    }
    if let Some(p) = s.together.pointer
        && let Some((q, sent)) = t.cursor_sent
        && q != p
    {
        at.push(sent + CURSOR_GAP);
    }
    if let Some(c) = t.cursors.values().map(|c| c.2).min() {
        at.push(c + CURSOR_STALE);
    }
    if let Some(c) = t.tl_check {
        at.push(c);
    }
    if let Some(c) = t.live_in.prune_at() {
        at.push(c);
    }
    if let Some((_, u)) = &s.together.note {
        at.push(*u);
    }
    if !t.structure.is_empty() {
        at.push(now + Duration::from_millis(20));
    }
    at.into_iter().min()
}

/// One message from the session. Whether the picture changed.
fn take(e: &mut Engine, s: &mut State, msg: net::In) -> bool {
    use net::In;
    match msg {
        In::Started(info) => {
            e.together.me = info.me.id.clone();
            e.together.stats.joined_ns = Some(now_ns());
            e.painter.track_edits(true);
            let _ = e.painter.take_edited();
            s.together.busy = false;
            s.together.error.clear();
            let host = info.host;
            let host_id = info.members.first().map(|m| m.id.clone());
            if e.together.print_invite && host {
                use std::io::Write;
                let mut o = std::io::stdout();
                let _ = writeln!(o, "{}", info.invite);
                let _ = o.flush();
            }
            s.together.info = Some(info);
            if host {
                start_host(e, s);
            } else if let Some(h) = host_id {
                e.together
                    .send(net::Cmd::Direct(h, json!({"k": "snap.req"})));
            }
            e.together.ui_dirty = true;
            true
        }
        In::Failed(err) => {
            e.together.net = None;
            s.together.busy = false;
            s.together.error = err;
            true
        }
        In::Edit {
            seq,
            from,
            body,
            tiles,
            at_ns,
        } => {
            if !e.together.synced {
                e.together.buffer.push((seq, from, body, tiles, at_ns));
                return false;
            }
            on_edit(e, s, seq, &from, body, tiles, at_ns)
        }
        In::Direct {
            from,
            body,
            tiles,
            at_ns,
        } => {
            if body.get("k").and_then(Value::as_str) == Some("chat") {
                if let Some(text) = chat_text(&body) {
                    let name = s.together.member_name(&from);
                    s.together.line(ChatLine {
                        from,
                        name,
                        mine: false,
                        system: false,
                        text,
                    });
                }
                return true;
            }
            on_direct(e, s, &from, body, tiles, at_ns)
        }
        In::Presence { from, body, at_ns } => {
            if let Some(ev) = body.get("live") {
                if !e.together.synced {
                    return false;
                }
                let name = format!("{} (drawing now)", s.together.member_name(&from));
                let t = &mut e.together;
                let mut made = None;
                let took = t.live_in.take(&mut e.painter, &from, ev, |p| {
                    let id = p.add_layer_silent(&name);
                    if id != 0 {
                        let mut props = p
                            .layers()
                            .iter()
                            .find(|l| l.id == id)
                            .unwrap()
                            .props
                            .clone();
                        props.locked = true;
                        p.set_layer_props(id, props, false);
                    }
                    made = Some(id);
                    id
                });
                if let Some(id) = made.filter(|id| *id != 0) {
                    t.local.insert(id);
                    e.frames.previews.insert(id);
                    t.ui_dirty = true;
                }
                if took == live::Took::Painted {
                    t.live_in.last_at.insert(from.clone(), at_ns);
                    if t.stats_on && ev.get("type").and_then(Value::as_str) == Some("points") {
                        let n = ev.get("pts").and_then(Value::as_array).map_or(0, Vec::len) / 4;
                        t.stats.edits.push(EditStat {
                            kind: "live",
                            from,
                            tiles: n,
                            received_ns: at_ns,
                            applied_ns: now_ns(),
                            sent_ns: ev.get("t").and_then(Value::as_u64),
                        });
                    }
                    return true;
                }
                return false;
            }
            let x = body.get("x").and_then(Value::as_f64);
            let y = body.get("y").and_then(Value::as_f64);
            if let (Some(x), Some(y)) = (x, y)
                && x.is_finite()
                && y.is_finite()
            {
                e.together
                    .cursors
                    .insert(from, (x as f32, y as f32, Instant::now()));
                e.together.ui_dirty = true;
            }
            false
        }
        In::Joined(m) => {
            if let Some(info) = &mut s.together.info
                && !info.members.iter().any(|x| x.id == m.id)
            {
                let name = if m.name.trim().is_empty() {
                    "Someone".to_string()
                } else {
                    m.name.clone()
                };
                info.members.push(m);
                s.together.line(ChatLine {
                    from: String::new(),
                    name: String::new(),
                    mine: false,
                    system: true,
                    text: format!("{name} joined"),
                });
            }
            note_members(e, s);
            true
        }
        In::Left(id) => {
            let name = s.together.member_name(&id);
            if let Some(info) = &mut s.together.info {
                info.members.retain(|m| m.id != id);
            }
            s.together.line(ChatLine {
                from: String::new(),
                name: String::new(),
                mine: false,
                system: true,
                text: format!("{name} left"),
            });
            e.together.cursors.remove(&id);
            if let Some(l) = e.together.live_in.layers.get(&id).copied() {
                e.together.local.remove(&l);
                e.frames.previews.remove(&l);
            }
            e.together.live_in.forget(&mut e.painter, &id);
            note_members(e, s);
            true
        }
        In::HostChanged {
            you,
            members,
            invite,
            ..
        } => {
            if let Some(info) = &mut s.together.info {
                info.members = members;
                info.host = you;
                info.invite = invite;
            }
            note_members(e, s);
            true
        }
        In::Closed(why) => {
            leave(e, s, Some(why));
            true
        }
    }
}

fn note_members(e: &mut Engine, s: &State) {
    let n = s.together.info.as_ref().map_or(0, |i| i.members.len());
    e.together.stats.members_peak = e.together.stats.members_peak.max(n);
    e.together.ui_dirty = true;
}

/// An incoming chat body's text, or None when it isn't one (never markup: shown as text).
fn chat_text(body: &Value) -> Option<String> {
    let t = body.get("text")?.as_str()?;
    let t: String = t
        .chars()
        .filter(|c| !c.is_control() || *c == '\t' || *c == '\n' || *c == '\r')
        .collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.chars().take(MAX_CHAT_CHARS).collect())
}

// ---- hosting ----

/// Hosting: what's on the canvas is shared (every layer, every drawing, the timeline).
fn start_host(e: &mut Engine, s: &mut State) {
    super::frames::catch_up(e, s);
    let layers: Vec<(u32, String)> = e
        .painter
        .layers()
        .iter()
        .filter(|l| !e.frames.previews.contains(&l.id))
        .map(|l| (l.id, l.props.name.clone()))
        .collect();
    for (id, name) in &layers {
        let lid = e.together.new_lid();
        e.together.ids.insert(lid.clone(), *id);
        e.together
            .edit(json!({"k": "layer", "lid": lid, "name": name}));
    }
    for (id, _) in &layers {
        let lid = e.together.lid_of(*id).cloned().unwrap_or_default();
        for d in drawings_of(e, s, *id) {
            let did = e.together.did_of(d);
            e.together.outbox.push_back(Job::Drawing {
                to: None,
                lid: lid.clone(),
                did,
                d,
                keys: None,
                t_ns: 0,
            });
        }
    }
    let tl = shared_timeline(e, s);
    e.together.tl_sent = tl.to_string();
    e.together.edit(json!({"k": "timeline", "tl": tl}));
    e.together.synced = true;
    s.together.synced = true;
    e.together.stats.synced_ns = Some(now_ns());
    e.together.ui_dirty = true;
}

/// Every drawing a layer has: on the timeline, and the one it shows.
fn drawings_of(e: &Engine, s: &State, layer: u32) -> Vec<u32> {
    let mut v: Vec<u32> = s
        .sheet
        .tracks
        .iter()
        .filter(|t| t.layer == layer)
        .flat_map(|t| t.drawings())
        .collect();
    let shows = e.painter.layer_drawing(layer);
    if shows != 0 && !v.contains(&shows) {
        v.push(shows);
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// The open cut's timeline as it travels (together.ts's `SharedTimeline`): one scene,
/// one cut, the shared layers' tracks by their ids, drawings by theirs.
fn shared_timeline(e: &mut Engine, s: &State) -> Value {
    let mut tracks = serde_json::Map::new();
    let mut statics = Vec::new();
    let mut kinds = serde_json::Map::new();
    for t in &s.sheet.tracks {
        let Some(lid) = e.together.lid_of(t.layer).cloned() else {
            continue;
        };
        let cels: Vec<Value> = t
            .cels
            .iter()
            .map(|c| {
                let d = if c.drawing != 0 {
                    e.together.did_of(c.drawing)
                } else {
                    String::new()
                };
                if c.drawing != 0
                    && let Some(k) = mark_name(c.mark)
                {
                    kinds.insert(d.clone(), json!(k));
                }
                json!({"d": d, "n": c.hold.max(1)})
            })
            .collect();
        if t.kind == 'S' {
            statics.push(json!(lid));
        }
        tracks.insert(lid, Value::Array(cels));
    }
    json!({
        "scenes": [{"name": s.project.scene, "cuts": [{"name": s.cut_name, "tracks": tracks}]}],
        "statics": statics,
        "mouths": [],
        "kinds": kinds,
    })
}

fn mark_name(m: super::frames::Mark) -> Option<&'static str> {
    use super::frames::Mark;
    match m {
        Mark::None => None,
        Mark::Key => Some("key"),
        Mark::Breakdown => Some("breakdown"),
        Mark::InBetween => Some("inbetween"),
    }
}

fn mark_of(k: &str) -> super::frames::Mark {
    use super::frames::Mark;
    match k {
        "key" => Mark::Key,
        "breakdown" => Mark::Breakdown,
        "inbetween" => Mark::InBetween,
        _ => Mark::None,
    }
}

/// The session's timeline put in: the shared layers' tracks (the first cut: this app
/// works on one); this app's own layers keep theirs.
fn put_timeline(e: &mut Engine, s: &mut State, tl: &Value) {
    let Some(cut) = tl
        .get("scenes")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("cuts"))
        .and_then(|v| v.get(0))
    else {
        return;
    };
    let kinds = tl.get("kinds").cloned().unwrap_or(Value::Null);
    let statics: HashSet<String> = tl
        .get("statics")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let Some(tracks) = cut.get("tracks").and_then(Value::as_object) else {
        return;
    };
    let mut new: Vec<(u32, char, Vec<super::frames::Cel>)> = Vec::new();
    for (lid, cels) in tracks {
        let Some(&layer) = e.together.ids.get(lid) else {
            continue;
        };
        let Some(cels) = cels.as_array() else {
            continue;
        };
        let mut v = Vec::new();
        for c in cels.iter().take(5000) {
            let did = c.get("d").and_then(Value::as_str).unwrap_or("");
            let d = if model::is_lid(did) {
                local_of(e, did)
            } else {
                0
            };
            let n = c
                .get("n")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 999) as u32;
            let mark = kinds
                .get(did)
                .and_then(Value::as_str)
                .map_or(super::frames::Mark::None, mark_of);
            v.push(super::frames::Cel::new(d, n, mark));
        }
        let kind = if statics.contains(lid) { 'S' } else { 'C' };
        new.push((layer, kind, v));
    }
    super::frames::catch_up(e, s);
    for (layer, kind, cels) in new {
        if let Some(i) = s.sheet.tracks.iter().position(|t| t.layer == layer) {
            s.sheet.tracks[i].cels = cels;
            s.sheet.tracks[i].kind = kind;
            if let Some(l) = s.layers.get_mut(i) {
                l.kind = kind;
            }
        }
    }
    super::frames::derive(s);
    e.together.tl_sent = shared_timeline(e, s).to_string();
}

// ---- joining ----

fn on_direct(
    e: &mut Engine,
    s: &mut State,
    from: &str,
    body: Value,
    tiles: Option<net::Tiles>,
    at_ns: u64,
) -> bool {
    let k = body.get("k").and_then(Value::as_str).unwrap_or("");
    match k {
        "snap.req" if e.together.synced => {
            send_snapshot(e, s, from);
            false
        }
        "snap.begin" if !e.together.synced => {
            e.together.shared.load(&body);
            e.together.snap_seq = body.get("seq").and_then(Value::as_u64).unwrap_or(0);
            e.together.snap_tl = body.get("tl").cloned().filter(Value::is_object);
            e.together.snap_frame = body.get("frame").and_then(Value::as_u64).map(|f| f as u32);
            // a native host says the project's size; the current app's doesn't (its
            // canvas: this app's own size then)
            let size = match (
                body.get("w").and_then(Value::as_u64),
                body.get("h").and_then(Value::as_u64),
            ) {
                (Some(w), Some(h)) if (1..=16384).contains(&w) && (1..=16384).contains(&h) => {
                    Some((w as u32, h as u32))
                }
                _ => None,
            };
            open_shared(e, s, size);
            true
        }
        "snap.tiles" if !e.together.synced => {
            if let Some(t) = tiles
                && e.together.shared.get(&t.lid).is_some()
                && model::is_lid(&t.did)
            {
                put_tiles(e, s, from, t, at_ns, None, "snapshot");
            }
            false
        }
        "snap.kept" if !e.together.synced => false,
        "snap.end" if !e.together.synced => {
            finish_join(e, s);
            true
        }
        _ => false,
    }
}

/// Joining: the shared project opens in place of the one here (its size, an empty
/// layer), and the shared layers are made.
fn open_shared(e: &mut Engine, s: &mut State, size: Option<(u32, u32)>) {
    let (w, h) = size.unwrap_or((e.painter.width(), e.painter.height()));
    super::files::new_sized(e, s, w, h);
    e.painter.track_edits(true);
    // (the empty layer stays aside until the shared ones are in)
    for l in e.painter.layers() {
        e.together.local.insert(l.id);
    }
    for lid in e.together.shared.order() {
        make_layer(e, &lid);
    }
    for lid in e.together.shared.order() {
        place(e, &lid);
    }
    e.painter.clear_history();
}

fn finish_join(e: &mut Engine, s: &mut State) {
    let t = &mut e.together;
    // the empty layer it opened with goes (the shared ones are the project)
    let boot: Vec<u32> = e
        .painter
        .layers()
        .iter()
        .filter(|l| t.local.contains(&l.id) && !e.frames.previews.contains(&l.id))
        .map(|l| l.id)
        .collect();
    if e.painter.layers().len() > boot.len() {
        for id in boot {
            if e.painter.remove_layer_silent(id) {
                t.local.remove(&id);
            }
        }
    }
    for lid in t.shared.order() {
        refresh_lock(e, &lid);
    }
    super::frames::tick(e, s, Instant::now());
    if let Some(tl) = e.together.snap_tl.take() {
        put_timeline(e, s, &tl);
    }
    if let Some(f) = e.together.snap_frame.take() {
        s.frame = f.min(s.frames.saturating_sub(1));
    }
    e.painter.clear_history();
    e.together.synced = true;
    s.together.synced = true;
    e.together.stats.synced_ns = Some(now_ns());
    let snap = e.together.snap_seq;
    let later: Vec<_> = std::mem::take(&mut e.together.buffer)
        .into_iter()
        .filter(|b| b.0 > snap)
        .collect();
    for (seq, from, body, tiles, at) in later {
        on_edit(e, s, seq, &from, body, tiles, at);
    }
    // a layer of one's own to draw on, on top, for everyone
    let lid = e.together.new_lid();
    e.together.mine.insert(lid.clone());
    let name = format!("{}’s layer", s.together.member_name(&e.together.me));
    e.together
        .edit(json!({"k": "layer", "lid": lid, "name": name}));
    e.together.ui_dirty = true;
}

/// A joiner asked for the project: the layers and who may draw on them, the timeline,
/// then every shared drawing's tiles, then the end.
fn send_snapshot(e: &mut Engine, s: &mut State, to: &str) {
    let mut snap = e.together.shared.snapshot();
    let tl = shared_timeline(e, s);
    snap["k"] = json!("snap.begin");
    snap["seq"] = json!(e.together.last_seq);
    snap["tl"] = tl;
    // (a native host's extras: the project's size, and the frame it's on, where the joiner
    // starts)
    snap["w"] = json!(e.painter.width());
    snap["h"] = json!(e.painter.height());
    snap["frame"] = json!(s.frame);
    e.together.send(net::Cmd::Direct(to.to_string(), snap));
    // the drawings showing first (the joiner can draw sooner), then the rest
    let mut jobs: Vec<(String, String, u32, bool)> = Vec::new();
    let ids: Vec<(String, u32)> = e
        .together
        .ids
        .iter()
        .map(|(a, b)| (a.clone(), *b))
        .collect();
    for (lid, layer) in ids {
        let shows = e.painter.layer_drawing(layer);
        for d in drawings_of(e, s, layer) {
            let did = e.together.did_of(d);
            jobs.push((lid.clone(), did, d, d == shows));
        }
    }
    jobs.sort_by_key(|j| !j.3);
    for (lid, did, d, _) in jobs {
        e.together.outbox.push_back(Job::Drawing {
            to: Some(to.to_string()),
            lid,
            did,
            d,
            keys: None,
            t_ns: 0,
        });
    }
    e.together.outbox.push_back(Job::End(to.to_string()));
}

// ---- the session's edits ----

fn on_edit(
    e: &mut Engine,
    s: &mut State,
    seq: u64,
    from: &str,
    body: Value,
    tiles: Option<net::Tiles>,
    at_ns: u64,
) -> bool {
    e.together.last_seq = e.together.last_seq.max(seq);
    let Some(change) = e.together.shared.apply(from, &body) else {
        return false;
    };
    let me = e.together.me.clone();
    e.together.ui_dirty = true;
    use model::Change;
    match change {
        Change::Made(lid) => {
            if !e.together.ids.contains_key(&lid) {
                make_layer(e, &lid);
                place(e, &lid);
            } else {
                refresh_lock(e, &lid);
            }
            if e.together.mine.remove(&lid)
                && let Some(&id) = e.together.ids.get(&lid)
            {
                super::frames::catch_up(e, s);
                e.painter.set_active_layer(id);
                super::frames::tick(e, s, Instant::now());
                if let Some(i) = s.sheet.tracks.iter().position(|t| t.layer == id) {
                    s.active_layer = i;
                }
            }
            true
        }
        Change::Renamed(lid) => {
            if let (Some(&id), Some(l)) = (e.together.ids.get(&lid), e.together.shared.get(&lid)) {
                let name = l.name.clone();
                set_props(e, id, |p| p.name = name);
            }
            true
        }
        Change::Look(lid) => {
            if from != me
                && let (Some(&id), Some(look)) = (
                    e.together.ids.get(&lid),
                    e.together.shared.get(&lid).and_then(|l| l.look.clone()),
                )
            {
                let visible = look.get("visible").and_then(Value::as_bool).unwrap_or(true);
                let opacity = look
                    .get("opacity")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0) as f32;
                let blend = look
                    .get("blend")
                    .and_then(Value::as_str)
                    .and_then(brush::Blend::parse);
                e.together
                    .looks_sent
                    .insert(lid.clone(), (visible, opacity));
                set_props(e, id, |p| {
                    p.visible = visible;
                    p.opacity = opacity;
                    if let Some(b) = blend {
                        p.blend = b;
                    }
                });
            }
            true
        }
        Change::Removed(lid) => {
            if let Some(id) = e.together.ids.remove(&lid) {
                if !e.painter.remove_layer_silent(id) {
                    e.together.structure.push(format!("remove {id}"));
                } else {
                    e.painter.clear_history();
                }
            }
            true
        }
        Change::Moved(lid) => {
            place(e, &lid);
            true
        }
        Change::Tiles(_) => {
            if from == me {
                // (this app's own changes are on its canvas already)
                return false;
            }
            let sent = body.get("t").and_then(Value::as_u64);
            match tiles {
                Some(t) => put_tiles(e, s, from, t, at_ns, sent, "tiles"),
                None => false,
            }
        }
        Change::Timeline => {
            if from != me
                && let Some(tl) = body.get("tl")
            {
                put_timeline(e, s, tl);
                return true;
            }
            false
        }
        Change::Access(lid) => {
            refresh_lock(e, &lid);
            true
        }
        Change::Asked(lid) => {
            if e.together.shared.owns(&lid, &me) {
                let name = s.together.member_name(from);
                let layer = e
                    .together
                    .shared
                    .get(&lid)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                s.together.say(&format!(
                    "{name} asks to draw on “{layer}”: see Draw together."
                ));
            }
            false
        }
        Change::Denied { to, .. } => {
            if to == me {
                s.together.say("They said no: nothing was changed.");
            }
            false
        }
        Change::Other => false,
    }
}

/// Changes a layer's settings (not an undo step).
fn set_props(e: &mut Engine, id: u32, f: impl FnOnce(&mut brush::LayerProps)) {
    if let Some(l) = e.painter.layers().iter().find(|l| l.id == id) {
        let mut p = l.props.clone();
        f(&mut p);
        if p != l.props {
            e.painter.set_layer_props(id, p, false);
        }
    }
}

/// A shared layer made here (on top; [`place`] puts it where the session has it).
fn make_layer(e: &mut Engine, lid: &str) {
    if e.together.ids.contains_key(lid) {
        return;
    }
    let Some(l) = e.together.shared.get(lid).cloned() else {
        return;
    };
    let id = e.painter.add_layer_silent(&l.name);
    if id == 0 {
        return;
    }
    e.together.ids.insert(lid.to_string(), id);
    if let Some(look) = &l.look {
        let visible = look.get("visible").and_then(Value::as_bool).unwrap_or(true);
        let opacity = look.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        set_props(e, id, |p| {
            p.visible = visible;
            p.opacity = opacity.clamp(0.0, 1.0);
        });
    }
    refresh_lock(e, lid);
}

/// A shared layer put where the session's order has it, below the live previews.
fn place(e: &mut Engine, lid: &str) {
    let Some(&id) = e.together.ids.get(lid) else {
        return;
    };
    let order = e.together.shared.order();
    let Some(i) = order.iter().position(|x| x == lid) else {
        return;
    };
    let pos = |e: &Engine, id: u32| e.painter.layers().iter().position(|l| l.id == id);
    let Some(from) = pos(e, id) else { return };
    // the nearest shared layer below that's here
    let below = order[..i]
        .iter()
        .rev()
        .find_map(|x| e.together.ids.get(x).copied())
        .and_then(|b| pos(e, b));
    let to = match below {
        Some(b) if from > b => b + 1,
        Some(b) => b,
        // the lowest shared layer: right above this app's own (none after joining)
        None => e
            .painter
            .layers()
            .iter()
            .filter(|l| e.together.local.contains(&l.id) && !e.frames.previews.contains(&l.id))
            .count()
            .min(from),
    };
    if to != from {
        if e.painter.move_layer_silent(id, to) {
            e.painter.clear_history();
        } else {
            e.together.structure.push(format!("place {lid}"));
        }
    }
}

/// A shared layer shows locked when this app may not draw on it.
fn refresh_lock(e: &mut Engine, lid: &str) {
    let t = &e.together;
    let Some(&id) = t.ids.get(lid) else { return };
    let locked = !t.shared.can_draw(lid, &t.me);
    set_props(e, id, |p| p.locked = locked);
}

/// Someone's tiles put into the drawing they're for (decoded already: this only swaps
/// them in). The GPU's copy follows: a drawing showing gets the changed tiles, one not
/// showing is sent up again when it shows.
fn put_tiles(
    e: &mut Engine,
    s: &State,
    from: &str,
    t: net::Tiles,
    received_ns: u64,
    sent_ns: Option<u64>,
    kind: &'static str,
) -> bool {
    let d = local_of(e, &t.did);
    // drawing on that drawing now (granted to two): theirs goes in once the stroke ends
    if e.is_stroking() && e.painter.layer_drawing(e.painter.active_layer()) == d {
        e.together.stats.deferred += 1;
        e.together.waiting.push_back(Waiting {
            from: from.to_string(),
            tiles: t,
            received_ns,
            sent_ns,
        });
        return false;
    }
    // (a drawing put away compressed comes back first: the change is to all of it)
    if !e.painter.has_drawing(d) && e.frames.store.drawings.contains_key(&d) {
        super::frames::load_drawings(e, &[d]);
    }
    let n = t.set.len() + t.clear.len();
    let ok = e.painter.patch_drawing(d, t.set, &t.clear);
    if !ok {
        return false;
    }
    e.frames.store.drawings.remove(&d);
    e.frames.remote_changed(d);
    if e.together.live_in.tiles_came(&mut e.painter, from) {
        // (their preview layer emptied)
    }
    if e.together.stats_on {
        e.together.stats.edits.push(EditStat {
            kind,
            from: from.to_string(),
            tiles: n,
            received_ns,
            applied_ns: now_ns(),
            sent_ns,
        });
    }
    let _ = s;
    true
}

/// Remote tiles that waited, and layer moves the painter couldn't make then.
fn retry(e: &mut Engine, s: &mut State) -> bool {
    let mut changed = false;
    if !e.is_stroking() {
        while let Some(w) = e.together.waiting.pop_front() {
            changed |= put_tiles(e, s, &w.from, w.tiles, w.received_ns, w.sent_ns, "tiles");
        }
        for op in std::mem::take(&mut e.together.structure) {
            if let Some(id) = op
                .strip_prefix("remove ")
                .and_then(|x| x.parse::<u32>().ok())
            {
                if e.painter.remove_layer_silent(id) {
                    e.painter.clear_history();
                    changed = true;
                }
            } else if let Some(lid) = op.strip_prefix("place ") {
                place(e, lid);
                changed = true;
            }
        }
    }
    changed
}

// ---- this app's changes ----

/// What changed here goes to the others: the tiles strokes changed (once a stroke ends),
/// the layers' look, the timeline.
fn local_changes(e: &mut Engine, s: &mut State, now: Instant) {
    // a stroke in progress: its tiles go when it ends
    if e.is_stroking() {
        return;
    }
    let me = e.together.me.clone();
    let edited = e.painter.take_edited();
    let t_ns = std::mem::take(&mut e.together.stroke_end_ns);
    for (d, keys) in edited {
        if d == 0 || keys.is_empty() {
            continue;
        }
        // the layer the drawing is on: showing, or on its track
        let layer = e
            .painter
            .layers()
            .iter()
            .find(|l| e.painter.layer_drawing(l.id) == d)
            .map(|l| l.id)
            .or_else(|| {
                s.sheet
                    .tracks
                    .iter()
                    .find(|t| t.cels.iter().any(|c| c.drawing == d))
                    .map(|t| t.layer)
            });
        let Some(lid) = layer.and_then(|l| e.together.lid_of(l).cloned()) else {
            continue;
        };
        let own_new =
            e.together.shared.get(&lid).is_none() && lid.starts_with(&me[..12.min(me.len())]);
        if !e.together.shared.can_draw(&lid, &me) && !own_new {
            continue;
        }
        let did = e.together.did_of(d);
        e.together.outbox.push_back(Job::Drawing {
            to: None,
            lid,
            did,
            d,
            keys: Some(keys),
            t_ns,
        });
    }
    // the eye and opacity of layers this app may change
    let looks: Vec<(String, bool, f32, &'static str)> = e
        .together
        .ids
        .iter()
        .filter(|(lid, _)| e.together.shared.can_draw(lid, &me))
        .filter_map(|(lid, id)| {
            let l = e.painter.layers().iter().find(|l| l.id == *id)?;
            Some((
                lid.clone(),
                l.props.visible,
                l.props.opacity,
                l.props.blend.as_str(),
            ))
        })
        .collect();
    for (lid, visible, opacity, blend) in looks {
        let was = e.together.looks_sent.get(&lid).copied();
        let base = e.together.shared.get(&lid).and_then(|l| l.look.clone());
        let was = was.or_else(|| {
            base.map(|b| {
                (
                    b.get("visible").and_then(Value::as_bool).unwrap_or(true),
                    b.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                )
            })
        });
        let was = was.unwrap_or((true, 1.0));
        if was.0 != visible || (was.1 - opacity).abs() > 1e-3 {
            e.together
                .looks_sent
                .insert(lid.clone(), (visible, opacity));
            e.together.edit(json!({"k": "look", "lid": lid, "look": {
                "visible": visible, "opacity": opacity, "blend": blend,
                "alphaLock": false, "clip": false
            }}));
        }
    }
    // the layers' locks show access (unlocking someone else's layer here changes nothing)
    let lids: Vec<String> = e.together.ids.keys().cloned().collect();
    for lid in &lids {
        refresh_lock(e, lid);
    }
    // the timeline, now and then
    if e.together.tl_check.is_none_or(|t| now >= t) {
        e.together.tl_check = Some(now + TIMELINE_EVERY);
        let tl = shared_timeline(e, s);
        let text = tl.to_string();
        if text != e.together.tl_sent {
            e.together.tl_sent = text;
            e.together.edit(json!({"k": "timeline", "tl": tl}));
        }
    }
}

/// This app's stroke starts: it goes to the others as it's drawn when it's on a shared
/// layer this app may draw on.
pub fn stroke_begins(e: &mut Engine, s: &State) {
    let t = &mut e.together;
    let on = t.live()
        && t.synced
        && t.lid_of(e.painter.active_layer())
            .is_some_and(|l| t.shared.can_draw(l, &t.me));
    t.live_out.on = on;
    t.live_out.stamp = t.stats_on;
    if on {
        t.live_out.begin(live::style(s));
    }
}

/// This app's stroke ended (for the time its tiles say they were made).
pub fn stroke_ended(e: &mut Engine) {
    if e.together.live() {
        e.together.stroke_end_ns = now_ns();
    }
}

/// Tiles waiting to go: copied out of the painter a few at a time (the network thread
/// compresses them), within the tick's budget.
fn send_out(e: &mut Engine, _s: &State, until: Instant) {
    let mut copied = 0usize;
    while let Some(job) = e.together.outbox.pop_front() {
        if Instant::now() >= until || copied >= TILES_A_TICK {
            e.together.outbox.push_front(job);
            break;
        }
        match job {
            Job::End(to) => {
                e.together
                    .send(net::Cmd::Direct(to, json!({"k": "snap.end"})));
            }
            Job::Drawing {
                to,
                lid,
                did,
                d,
                keys,
                t_ns,
            } => {
                // a drawing put away compressed: decoded and sent from the network thread
                if !e.painter.has_drawing(d) {
                    if let Some(job) = e.frames.store.job(d) {
                        e.together.send(net::Cmd::Kept { to, lid, did, job });
                    }
                    continue;
                }
                let mut keys = keys.unwrap_or_else(|| e.painter.drawing_tile_keys(d));
                let rest = if keys.len() > TILES_A_TICK {
                    keys.split_off(TILES_A_TICK)
                } else {
                    Vec::new()
                };
                let mut spare = brush::TileBuffers::default();
                let copy = e.painter.drawing_copy_tiles(d, &keys, &mut spare);
                let set = copy.into_decoded();
                copied += keys.len();
                let have: HashSet<(i32, i32)> = set.iter().map(|(k, _)| *k).collect();
                // (asked for all it has: none to clear)
                let clear: Vec<(i32, i32)> =
                    keys.into_iter().filter(|k| !have.contains(k)).collect();
                e.together.stats.tiles_sent += set.len() as u64;
                let stamp = (e.together.stats_on && t_ns != 0).then_some(t_ns);
                e.together.send(net::Cmd::Tiles {
                    to: to.clone(),
                    lid: lid.clone(),
                    did: did.clone(),
                    set,
                    clear,
                    t_ns: stamp,
                });
                if !rest.is_empty() {
                    e.together.outbox.push_front(Job::Drawing {
                        to,
                        lid,
                        did,
                        d,
                        keys: Some(rest),
                        t_ns,
                    });
                }
            }
        }
    }
}

// ---- the interface's lists ----

fn refresh_ui(e: &Engine, s: &mut State) {
    let t = &e.together;
    let ui_names = |id: &str| s.together.member_name(id);
    let access: Vec<Option<Access>> = s
        .sheet
        .tracks
        .iter()
        .map(|tr| {
            let lid = t.lid_of(tr.layer)?;
            let l = t.shared.get(lid)?;
            Some(Access {
                lid: lid.clone(),
                mine: l.owner == t.me,
                can_draw: l.writers.contains(&t.me),
                owner: ui_names(&l.owner),
                asked: t
                    .shared
                    .asks
                    .iter()
                    .any(|a| a.lid == *lid && a.from == t.me),
            })
        })
        .collect();
    let rows: Vec<Row> = t
        .shared
        .layers
        .iter()
        .rev()
        .map(|l| Row {
            lid: l.lid.clone(),
            name: l.name.clone(),
            owner: ui_names(&l.owner),
            mine: l.owner == t.me,
            can_draw: l.writers.contains(&t.me),
            asked: t
                .shared
                .asks
                .iter()
                .any(|a| a.lid == l.lid && a.from == t.me),
            writers: l
                .writers
                .iter()
                .filter(|w| **w != l.owner)
                .map(|w| (w.clone(), ui_names(w)))
                .collect(),
            asks: t
                .shared
                .asks
                .iter()
                .filter(|a| a.lid == l.lid && l.owner == t.me)
                .map(|a| {
                    let text = match a.why.as_str() {
                        "merge" => "asks to merge it",
                        "delete" => "asks for it, to delete it",
                        "group" => "asks to put it in a group",
                        _ => "asks to draw here",
                    };
                    (
                        a.from.clone(),
                        ui_names(&a.from),
                        text.to_string(),
                        a.own,
                        a.why == "draw",
                    )
                })
                .collect(),
        })
        .collect();
    let cursors: Vec<CursorView> = t
        .cursors
        .iter()
        .map(|(id, c)| CursorView {
            id: id.clone(),
            name: ui_names(id),
            colour: member_colour(id),
            x: c.0,
            y: c.1,
        })
        .collect();
    s.together.access = access;
    s.together.rows = rows;
    s.together.cursors = cursors;
    s.together.synced = t.synced;
}

/// The interface's lists follow the layers (after the frames part synced them).
pub fn after_sync(e: &mut Engine) {
    if e.together.live() {
        e.together.ui_dirty = true;
    }
}

/// `--stats`: the session's record, as JSON (null when there was none).
pub fn stats_json(e: &Engine, s: &State, shown_after: &dyn Fn(u64) -> Option<u64>) -> String {
    let t = &e.together;
    if t.stats.started_ns.is_none() {
        return "null".into();
    }
    let opt = |v: Option<u64>| v.map_or("null".to_string(), |v| v.to_string());
    let edits: Vec<String> = t
        .stats
        .edits
        .iter()
        .map(|x| {
            format!(
                "{{\"kind\": \"{}\", \"from\": \"{}\", \"tiles\": {}, \"receivedNs\": {}, \"appliedNs\": {}, \"shownNs\": {}, \"sentNs\": {}}}",
                x.kind,
                &x.from[..12.min(x.from.len())],
                x.tiles,
                x.received_ns,
                x.applied_ns,
                opt(shown_after(x.applied_ns)),
                opt(x.sent_ns)
            )
        })
        .collect();
    let (sent, received) = t.net.as_ref().map_or((0, 0), |n| {
        (
            n.bytes.sent.load(std::sync::atomic::Ordering::Relaxed),
            n.bytes.received.load(std::sync::atomic::Ordering::Relaxed),
        )
    });
    let members = s.together.info.as_ref().map_or(0, |i| i.members.len());
    format!(
        "{{\"host\": {}, \"members\": {members}, \"membersPeak\": {}, \"startedNs\": {}, \"joinedNs\": {}, \"syncedNs\": {}, \"bytesSent\": {sent}, \"bytesReceived\": {received}, \"liveSent\": {}, \"tilesSent\": {}, \"deferred\": {}, \"sharedLayers\": {}, \"edits\": [{}]}}",
        s.together.info.as_ref().is_some_and(|i| i.host),
        t.stats.members_peak,
        opt(t.stats.started_ns),
        opt(t.stats.joined_ns),
        opt(t.stats.synced_ns),
        t.stats.live_sent,
        t.stats.tiles_sent,
        t.stats.deferred,
        t.shared.layers.len(),
        edits.join(", ")
    )
}
