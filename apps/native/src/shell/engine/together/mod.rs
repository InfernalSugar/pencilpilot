//! Drawing together (ADR-0013): a live session with others, the native app's side of it,
//! compatible with the current app's (`together.ts`, `share.rs`).
//!
//! - [`net`]: the session on its own thread, with `crates/together`'s `Together` there (the
//!   shared project edit for edit as the current app keeps it: layers, groups, access, the
//!   timeline, masks, line strokes, sound, the join, coming back after a break).
//! - [`port`] / [`serve`]: the canvas as `Together` sees it, asked from that thread and
//!   answered here between frames (only quick work here).
//! - [`live`]: strokes as they're drawn, both ways (fast and lossy, beside the session).
//! - this file: hosting, joining, leaving, rejoining; this app's changes told to the
//!   session's thread; members, cursors, chat; what `--stats` records.
//!
//! **Solo projects never connect:** nothing here runs until Host or Join.

pub mod live;
pub mod net;
pub mod port;
pub mod serve;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
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
    /// back to the session after a break
    pub const REJOIN: &str = "Together: rejoin";
}

/// Chat lines kept (the newest), and the longest message.
pub const KEEP_CHAT: usize = 200;
pub const MAX_CHAT_CHARS: usize = 500;
/// Cursors go at most this often; a cursor without news this long goes.
const CURSOR_GAP: Duration = Duration::from_millis(50);
const CURSOR_STALE: Duration = Duration::from_secs(4);
/// The canvas's answers to the session's thread a tick may give (the rest wait a tick).
const TICK_BUDGET: Duration = Duration::from_millis(3);
const TICK_BUDGET_DRAWING: Duration = Duration::from_millis(1);
/// Taking over as host with others in the session, how long until they've come over
/// (usually moments, but over a relay or on a slow machine it can take seconds; past this,
/// this app is the one that lost touch).
const ALONE_GRACE: Duration = Duration::from_secs(15);
/// How long a tick waits for the session thread's next question after answering one.
const ASK_FOLLOWS: Duration = Duration::from_millis(1);
/// A live stroke's points go at most this often (as the current app's, a frame's worth).
const LIVE_EVERY: Duration = Duration::from_millis(16);
/// A live stroke's start is said again this often while it goes (one that was lost, or
/// came before someone was ready, still shows).
const LIVE_BEGIN_AGAIN: Duration = Duration::from_millis(500);

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
    /// a kind of layer this version doesn't draw (made in the current app): what it is
    pub other: Option<&'static str>,
    /// the group it's in (shown; groups are changed in the current app)
    pub group: Option<String>,
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
    /// a session was left or lost: it can be rejoined
    pub can_rejoin: bool,
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
    /// the window's time answering the session's thread, and how long questions waited
    /// for it, ns
    pub serve_ns: u64,
    pub ask_queue_ns: u64,
    /// joining: the project's tiles, and when the last came
    pub snapshot_tiles: u64,
    pub snapshot_last_ns: Option<u64>,
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
    /// print the invite to stdout once hosting (`--host`)
    pub print_invite: bool,
    /// this computer's key, when not the one kept on disk (tests)
    pub identity: Option<pencilpilot_share::Identity>,
    pub(crate) me: String,
    /// the shared state as the session's thread last told it
    pub(crate) view: net::SharedView,
    /// the canvas the session's thread sees ([`serve`])
    pub(crate) win: serve::Window,
    pub live_in: live::LiveIn,
    pub live_out: live::LiveOut,
    live_sent_at: Option<Instant>,
    begin_at: Option<Instant>,
    /// a stroke's start said again so many times
    begins: u32,
    cursors: HashMap<String, (f32, f32, Instant)>,
    cursor_sent: Option<((f32, f32), Instant)>,
    /// the layers' settings as last told (a change here is told again)
    props_sig: u64,
    /// questions from the session's thread waiting for a tick with room
    asks: std::collections::VecDeque<port::Ask>,
    /// what to come back with after a break (the session as it was left)
    pub saved: Option<Box<net::Saved>>,
    /// came back once after losing touch (not again on its own)
    reconnected: bool,
    /// took over as host with others still in the session: since when, waiting for them
    /// to come over (a hand-over: they do within moments; if not, this app lost them)
    alone_since: Option<Instant>,
    /// the interface's lists need making again
    ui_dirty: bool,
    /// when this app's last stroke ended (for the tiles it sends), ns
    stroke_end_ns: Option<u64>,
    pub stats: Stats,
}

fn now_ns() -> u64 {
    pencilpilot_window::clock::now_ns()
}

impl Together {
    pub fn live(&self) -> bool {
        self.net.is_some()
    }

    pub fn synced(&self) -> bool {
        self.view.synced
    }

    /// The session's thread is likely to ask the canvas soon (joining, or questions
    /// waiting): slow work on the window's thread waits.
    pub fn busy(&self) -> bool {
        self.live() && (!self.view.synced || !self.asks.is_empty())
    }

    pub(crate) fn send(&self, c: net::Cmd) {
        if let Some(n) = &self.net {
            n.send(c);
        }
    }

    pub fn lid_of(&self, layer: u32) -> Option<&String> {
        self.view
            .ids
            .iter()
            .find(|(_, v)| *v == layer)
            .map(|(k, _)| k)
    }

    /// Whether this app may draw on a painter layer (shared ones: as the session says; a
    /// line or adjustment layer made in the current app: never here).
    pub fn may_draw(&self, layer: u32) -> bool {
        let Some(lid) = self.lid_of(layer) else {
            return true;
        };
        let sh = &self.view.shared;
        sh.can_draw(lid, &self.me)
            && !sh
                .layers
                .get(lid)
                .is_some_and(|l| l.lines || l.adjust.is_some())
    }
}

/// Starts hosting, joining or coming back (from the dialog or `--host` / `--join`).
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
        e.together.stats_on,
    ));
}

/// What the interface asked for. Returns whether it was this part's (and the picture may
/// have changed).
pub fn apply(e: &mut Engine, s: &mut State, a: &Action) -> bool {
    let live = e.together.live() && e.together.synced();
    let name = match a {
        Action::Other(n) => n.as_str(),
        // a new layer while drawing together is a shared one, made when the session numbers
        // it (on top, for everyone)
        Action::NewLayer if live => {
            let name = format!("{}’s layer", s.together.member_name(&e.together.me));
            e.together.send(net::Cmd::AddOwnLayer(name));
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
            .is_some_and(|l| e.together.view.shared.owns(l, &e.together.me));
        match name {
            fops::DELETE_LAYER if lid.is_some() => {
                // (someone else's: their leave is asked for first)
                e.together.send(net::Cmd::DeleteLayer(active));
                return true;
            }
            fops::LAYER_UP | fops::LAYER_DOWN => {
                let Some(l) = lid else { return false };
                if !mine {
                    s.together
                        .say("Only the person who made a shared layer can move it.");
                    return true;
                }
                let order = &e.together.view.shared.order;
                let at = order.iter().position(|x| *x == l).unwrap_or(0);
                let to = if name == fops::LAYER_UP {
                    (at + 1).min(order.len().saturating_sub(1))
                } else {
                    at.saturating_sub(1)
                };
                if to != at {
                    e.together.send(net::Cmd::MoveOwn(l, to as u32));
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
        ops::REJOIN => {
            if let Some(saved) = e.together.saved.clone() {
                start(e, s, net::Start::Rejoin(saved));
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
            e.together.send(net::Cmd::Chat(text.clone()));
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
            let (lid, to) = (lid.to_string(), to.to_string());
            e.together.send(match p {
                ops::LET => net::Cmd::Grant(lid, to, false),
                ops::HAND => net::Cmd::Grant(lid, to, true),
                ops::DENY => net::Cmd::Deny(lid, to),
                _ => net::Cmd::Revoke(lid, to),
            });
        }
        _ => return false,
    }
    true
}

fn ask(e: &mut Engine, s: &mut State, lid: &str) {
    let t = &e.together;
    if t.view.shared.can_draw(lid, &t.me) {
        return;
    }
    if t.view
        .shared
        .asks
        .iter()
        .any(|a| a.lid == lid && a.from == t.me)
    {
        s.together.say("You've asked: it's theirs to say yes.");
        return;
    }
    t.send(net::Cmd::Ask(lid.to_string()));
    let owner = t
        .view
        .shared
        .owner_of(lid)
        .map(str::to_string)
        .unwrap_or_default();
    s.together.say(&format!(
        "Asked {}: you can draw once they say yes.",
        s.together.member_name(&owner)
    ));
}

/// Leaves the session (`why`: it ended on its own, and why). The shared layers stay
/// (everyone keeps a full copy), unlocked; the session is kept to come back to.
pub fn leave(e: &mut Engine, s: &mut State, why: Option<String>) {
    let t = &mut e.together;
    let mut saved = None;
    if let Some(n) = t.net.take() {
        saved = n.leave(Duration::from_millis(800));
    }
    // (questions left unanswered: the thread has gone)
    t.asks.clear();
    let previews: Vec<u32> = t.live_in.layers.values().copied().collect();
    t.live_in.clear(&mut e.painter);
    for id in previews {
        e.frames.previews.remove(&id);
    }
    for &(_, id) in &t.view.ids {
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
    let keep = Together {
        offline: t.offline,
        stats_on: t.stats_on,
        waker: t.waker.clone(),
        identity: t.identity.clone(),
        saved: saved.or_else(|| t.saved.take()),
        // (the session's timeline as it was: coming back goes on from it)
        win: std::mem::take(&mut t.win),
        reconnected: t.reconnected,
        stats,
        ..Default::default()
    };
    *t = keep;
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
    ui.can_rejoin = e.together.saved.is_some();
    if let Some(w) = why {
        ui.error = format!("{w} You can rejoin: whoever's still there, or you'll host it again.");
        ui.open = true;
    }
}

/// Once a loop, before drawing: the session thread's questions answered, its news put
/// in, this app's changes told. Returns whether the picture changed.
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
    // (the pen down: the canvas's answers take at most a millisecond of each tick)
    let until = Instant::now()
        + if e.is_stroking() {
            TICK_BUDGET_DRAWING
        } else {
            TICK_BUDGET
        };
    let mut changed = false;
    let mut served = false;
    // what the session's thread sent (its questions answered within the tick's room)
    loop {
        if let Some(a) = e.together.asks.pop_front() {
            if Instant::now() >= until {
                e.together.asks.push_front(a);
                break;
            }
            let port::Ask(r, reply, asked) = a;
            e.together.stats.ask_queue_ns += now_ns().saturating_sub(asked);
            // (a question that only reads draws no frame: joining asks hundreds)
            let shows = !matches!(
                r,
                port::Req::Layers
                    | port::Req::Groups
                    | port::Req::Timeline
                    | port::Req::Doc
                    | port::Req::DrawingsOfLayer(_)
                    | port::Req::LayerOfDrawing(_)
                    | port::Req::Packed(_)
                    | port::Req::MaskPacked(_)
                    | port::Req::NewDrawing
                    | port::Req::DrawingOf(_)
                    | port::Req::PutSound(..)
            );
            let t0 = Instant::now();
            let resp = serve::serve(e, s, r);
            e.together.stats.serve_ns += t0.elapsed().as_nanos() as u64;
            let _ = reply.send(resp);
            // (joining: the project shows once it's all in, not a frame per question)
            changed |= shows && e.together.view.synced;
            served = true;
            continue;
        }
        // (a question answered: the next one usually follows within a fraction of a
        // millisecond; waiting for it here, briefly, saves it a whole loop of the window —
        // joining asks hundreds, one after another. Never while the pen draws.)
        // (joining, nothing else to do: the whole tick's room)
        let room = until.saturating_duration_since(Instant::now());
        // (and none while the pen is in use: the frames' time is the pen's)
        let wait = if !e.together.view.synced {
            room
        } else if crate::shell::engine::frames::gpu::pen_quiet(e) {
            room.min(ASK_FOLLOWS)
        } else {
            Duration::ZERO
        };
        let msg = if served && !e.is_stroking() && !wait.is_zero() {
            e.together.net.as_ref().and_then(|n| n.recv_timeout(wait))
        } else {
            e.together.net.as_ref().and_then(net::Net::try_recv)
        };
        let Some(msg) = msg else {
            break;
        };
        changed |= take(e, s, msg);
        if !e.together.live() {
            return true;
        }
    }
    if e.together
        .alone_since
        .is_some_and(|t| t.elapsed() >= ALONE_GRACE)
    {
        e.together.alone_since = None;
        if s.together
            .info
            .as_ref()
            .is_some_and(|i| i.members.len() <= 1)
        {
            reconnect(e, s, "Lost touch with the others: finding them again…");
            return true;
        }
    }
    if e.together.synced() {
        local_changes(e, s);
    }
    // this app's live stroke (a frame's worth of points a datagram) and cursor
    let due = e
        .together
        .live_sent_at
        .is_none_or(|t| t.elapsed() >= LIVE_EVERY)
        || !e.together.live_out.on;
    if due {
        // (soon after the start, then now and then: a stroke's first datagram can be lost
        // on a link that has been quiet)
        let gap = match e.together.begins {
            0 => Duration::from_millis(20),
            1 => Duration::from_millis(40),
            2 => Duration::from_millis(100),
            3 => Duration::from_millis(200),
            _ => LIVE_BEGIN_AGAIN,
        };
        if e.together.live_out.on && e.together.begin_at.is_some_and(|t| t.elapsed() >= gap) {
            e.together.live_out.begin_again();
            e.together.begin_at = Some(Instant::now());
            e.together.begins += 1;
        }
        let msgs = e.together.live_out.take();
        if !msgs.is_empty() {
            e.together.live_sent_at = Some(Instant::now());
        }
        for m in msgs {
            e.together.stats.live_sent += 1;
            e.together.send(net::Cmd::Presence(m));
        }
    }
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
    changed |= e.together.live_in.prune(&mut e.painter);
    if let Some((_, until)) = &s.together.note
        && Instant::now() >= *until
    {
        s.together.note = None;
        changed = true;
    }
    // (the layers changed: their sharing is listed again)
    if s.together.access.len() != s.sheet.tracks.len() {
        e.together.ui_dirty = true;
    }
    if std::mem::take(&mut e.together.ui_dirty) {
        refresh_ui(e, s);
        changed = true;
    }
    let _ = now;
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
    if !t.asks.is_empty() {
        at.push(now);
    }
    if !t.live_out.out.is_empty() || t.live_out.has_points() {
        at.push(t.live_sent_at.map_or(now, |x| x + LIVE_EVERY));
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
    if let Some(c) = t.live_in.prune_at() {
        at.push(c);
    }
    if let Some((_, u)) = &s.together.note {
        at.push(*u);
    }
    if let Some(a) = t.alone_since {
        at.push(a + ALONE_GRACE);
    }
    at.into_iter().min()
}

/// One message from the session's thread. Whether the picture changed.
fn take(e: &mut Engine, s: &mut State, msg: net::In) -> bool {
    use net::In;
    match msg {
        In::Ask(a) => {
            e.together.asks.push_back(a);
            false
        }
        In::Started(info) => {
            e.together.me = info.me.id.clone();
            e.together.stats.joined_ns = Some(now_ns());
            e.painter.track_edits(true);
            let _ = e.painter.take_edited();
            e.together.win.sheet_sig = serve::sheet_sig(e, s);
            s.together.busy = false;
            s.together.error.clear();
            s.together.can_rejoin = false;
            if e.together.print_invite && info.host {
                use std::io::Write;
                let mut o = std::io::stdout();
                let _ = writeln!(o, "{}", info.invite);
                let _ = o.flush();
            }
            s.together.info = Some(info);
            e.together.ui_dirty = true;
            true
        }
        In::Failed(err) => {
            e.together.net = None;
            s.together.busy = false;
            s.together.error = err;
            true
        }
        In::Shared(v) => {
            e.together.view = v;
            s.together.synced = e.together.view.synced;
            e.together.ui_dirty = true;
            refresh_locks(e);
            true
        }
        In::Synced(frame) => {
            e.together.stats.synced_ns = Some(now_ns());
            e.together.reconnected = false;
            if let Some(f) = frame {
                s.frame = f.min(s.frames.saturating_sub(1));
            }
            s.together.synced = true;
            e.together.win.sheet_sig = serve::sheet_sig(e, s);
            e.together.ui_dirty = true;
            true
        }
        In::Note(text) => {
            s.together.say(&text);
            true
        }
        In::TilesCame(from) => e.together.live_in.tiles_came(&mut e.painter, &from),
        In::Chat { from, text } => {
            let name = s.together.member_name(&from);
            s.together.line(ChatLine {
                from,
                name,
                mine: false,
                system: false,
                text,
            });
            true
        }
        In::Presence { from, body, at_ns } => presence(e, s, from, body, at_ns),
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
                // (someone came over: a hand-over, not a lost session)
                e.together.alone_since = None;
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
        } => {
            // (this app took over as host, and no one else is here: the host (and the others)
            // may have dropped it, not the session)
            let was_host = s.together.info.as_ref().is_some_and(|i| i.host);
            let alone = you && !was_host && members.len() == 1;
            // (the others this app knew, the old host gone: on a hand-over they come to it)
            let others = s.together.info.as_ref().map_or(0, |i| {
                i.members.iter().filter(|m| m.id != e.together.me).count()
            });
            if let Some(info) = &mut s.together.info {
                info.members = members;
                info.host = you;
                info.invite = invite;
            }
            note_members(e, s);
            // took over alone: the others may still be there (this app lost touch with
            // them, not they with each other): back to them if they are
            if alone && others > 0 {
                e.together.alone_since = Some(Instant::now());
            } else if alone {
                reconnect(e, s, "Lost touch with the others: finding them again…");
            }
            true
        }
        In::Closed { why, saved } => {
            if saved.is_some() {
                e.together.saved = saved;
            }
            // lost (not left): back once, to whoever is still there
            if why.is_some() && !e.together.reconnected && e.together.saved.is_some() {
                reconnect(e, s, "The session was lost: finding the others again…");
                return true;
            }
            let why = why.map(|w| format!("The session was lost ({w})."));
            leave(
                e,
                s,
                Some(why.unwrap_or_else(|| "The session has ended.".into())),
            );
            true
        }
    }
}

/// Lost touch: leave, and come back to whoever of the session is online (or host it again
/// if no one is), once; the person is told.
fn reconnect(e: &mut Engine, s: &mut State, say: &str) {
    leave(e, s, None);
    if let Some(saved) = e.together.saved.clone() {
        e.together.reconnected = true;
        s.together.say(say);
        start(e, s, net::Start::Rejoin(saved));
    }
}

/// Someone's cursor or live stroke.
fn presence(e: &mut Engine, s: &mut State, from: String, body: Value, at_ns: u64) -> bool {
    if let Some(ev) = body.get("live") {
        if !e.together.synced() {
            return false;
        }
        let name = format!("{} (drawing now)", s.together.member_name(&from));
        let t = &mut e.together;
        let mut made = None;
        let took = t.live_in.take(&mut e.painter, &from, ev, |p| {
            let id = p.add_layer_silent(&name);
            if id != 0
                && let Some(l) = p.layers().iter().find(|l| l.id == id)
            {
                let mut props = l.props.clone();
                props.locked = true;
                p.set_layer_props(id, props, false);
            }
            made = Some(id);
            id
        });
        if let Some(id) = made.filter(|id| *id != 0) {
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
    if let Some((x, y)) = pencilpilot_together::chat::cursor(&body) {
        e.together
            .cursors
            .insert(from, (x as f32, y as f32, Instant::now()));
        e.together.ui_dirty = true;
    }
    false
}

fn note_members(e: &mut Engine, s: &State) {
    let n = s.together.info.as_ref().map_or(0, |i| i.members.len());
    e.together.stats.members_peak = e.together.stats.members_peak.max(n);
    e.together.ui_dirty = true;
}

/// Layers this app can't draw on faithfully (a line or adjustment layer from the current
/// app) stay locked, whatever access says.
fn refresh_locks(e: &mut Engine) {
    let ids: Vec<u32> = e.together.view.ids.iter().map(|(_, v)| *v).collect();
    for id in ids {
        if !e.together.may_draw(id)
            && let Some(l) = e.painter.layers().iter().find(|l| l.id == id)
            && !l.props.locked
        {
            let mut p = l.props.clone();
            p.locked = true;
            e.painter.set_layer_props(id, p, false);
        }
    }
}

/// A sign of the layers' settings (a change here goes to the others).
fn props_sig(e: &Engine) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in e.painter.layers() {
        if e.frames.previews.contains(&l.id) {
            continue;
        }
        (
            l.id,
            l.props.visible,
            (l.props.opacity * 1000.0) as i32,
            l.props.blend.as_str(),
            &l.props.name,
            l.group(),
        )
            .hash(&mut h);
    }
    h.finish()
}

/// What changed here goes to the session's thread: the timeline first (as the current app
/// sends it), then the drawings strokes changed; the layers' settings.
fn local_changes(e: &mut Engine, s: &mut State) {
    // a stroke in progress: its changes go when it ends
    if e.is_stroking() {
        return;
    }
    let edited = e.painter.take_edited();
    // (tiles changed here aren't as they came any more)
    for (d, keys) in &edited {
        if let Some(m) = e.frames.store.exact.get_mut(d) {
            for k in keys {
                m.remove(k);
            }
        }
    }
    let sig = serve::sheet_sig(e, s);
    let timeline = sig != e.together.win.sheet_sig;
    let props = props_sig(e);
    let looks = props != e.together.props_sig;
    if !timeline && !looks && edited.is_empty() {
        return;
    }
    e.together.win.sheet_sig = sig;
    e.together.props_sig = props;
    let touched: Vec<u32> = edited.iter().map(|(d, _)| *d).filter(|d| *d != 0).collect();
    let tiles: usize = edited.iter().map(|(_, k)| k.len()).sum();
    e.together.stats.tiles_sent += tiles as u64;
    let t_ns = e.together.stroke_end_ns.take();
    e.together.send(net::Cmd::Changed {
        timeline,
        touched,
        t_ns,
    });
}

/// This app's stroke starts: it goes to the others as it's drawn when it's on a shared
/// layer this app may draw on.
pub fn stroke_begins(e: &mut Engine, s: &State) {
    let t = &mut e.together;
    let active = e.painter.active_layer();
    // (a hidden or locked layer takes no paint: nothing to show the others)
    let paints = e
        .painter
        .layers()
        .iter()
        .find(|l| l.id == active)
        .is_some_and(|l| l.props.visible && !l.props.locked);
    let on = t.live() && t.synced() && paints && t.lid_of(active).is_some() && t.may_draw(active);
    t.live_out.on = on;
    t.live_out.stamp = t.stats_on;
    if on {
        t.live_out.begin(live::style(s));
        t.begin_at = Some(Instant::now());
        t.begins = 0;
        // (the start goes at once)
        t.live_sent_at = None;
    }
}

/// The stroke's start (and its first point) go to the others now, at pen-down: not with
/// the window's next loop.
pub fn send_live_now(e: &mut Engine) {
    let t = &mut e.together;
    if !t.live_out.on {
        return;
    }
    let msgs = t.live_out.take();
    if !msgs.is_empty() {
        t.live_sent_at = Some(Instant::now());
    }
    for m in msgs {
        t.stats.live_sent += 1;
        t.send(net::Cmd::Presence(m));
    }
}

/// This app's stroke ended (for the time its tiles say they were made).
pub fn stroke_ended(e: &mut Engine) {
    if e.together.live() {
        e.together.stroke_end_ns = Some(now_ns());
        e.together.live_sent_at = None;
        e.together.begin_at = None;
    }
}

// ---- the interface's lists ----

fn refresh_ui(e: &Engine, s: &mut State) {
    let t = &e.together;
    let sh = &t.view.shared;
    let ui_names = |id: &str| s.together.member_name(id);
    let access: Vec<Option<Access>> = s
        .sheet
        .tracks
        .iter()
        .map(|tr| {
            let lid = t.lid_of(tr.layer)?;
            let l = sh.layers.get(lid)?;
            Some(Access {
                lid: lid.clone(),
                mine: l.owner == t.me,
                can_draw: l.writers.contains(&t.me),
                owner: ui_names(&l.owner),
                asked: sh.asks.iter().any(|a| a.lid == *lid && a.from == t.me),
                other: if l.adjust.is_some() {
                    Some("Adjustment layer, made in another version: not shown here")
                } else if l.lines {
                    Some("Line layer, made in another version: shown, not editable here")
                } else {
                    None
                },
                group: e
                    .painter
                    .layers()
                    .iter()
                    .find(|x| x.id == tr.layer)
                    .map(|x| x.group())
                    .filter(|g| *g != 0)
                    .and_then(|g| e.painter.groups().iter().find(|x| x.id == g))
                    .map(|g| g.name.clone()),
            })
        })
        .collect();
    let rows: Vec<Row> = sh
        .order
        .iter()
        .rev()
        .filter_map(|lid| sh.layers.get(lid))
        .map(|l| Row {
            lid: l.lid.clone(),
            name: l.name.clone(),
            owner: ui_names(&l.owner),
            mine: l.owner == t.me,
            can_draw: l.writers.contains(&t.me),
            asked: sh.asks.iter().any(|a| a.lid == l.lid && a.from == t.me),
            writers: l
                .writers
                .iter()
                .filter(|w| **w != l.owner)
                .map(|w| (w.clone(), ui_names(w)))
                .collect(),
            asks: sh
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
                        a.why.as_str() == "draw",
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
    s.together.synced = t.view.synced;
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
    let tl = t.win.full_tl.as_ref().map_or(0, |tl| tl.scenes.len());
    format!(
        "{{\"host\": {}, \"members\": {members}, \"membersPeak\": {}, \"startedNs\": {}, \"joinedNs\": {}, \"syncedNs\": {}, \"bytesSent\": {sent}, \"bytesReceived\": {received}, \"liveSent\": {}, \"tilesSent\": {}, \"deferred\": {}, \"cacheMiB\": {:.1}, \"asks\": {}, \"askWaitMs\": {:.1}, \"serveMs\": {:.1}, \"askQueueMs\": {:.1}, \"sharedLayers\": {}, \"scenes\": {}, \"edits\": [{}]}}",
        s.together.info.as_ref().is_some_and(|i| i.host),
        t.stats.members_peak,
        opt(t.stats.started_ns),
        opt(t.stats.joined_ns),
        opt(t.stats.synced_ns),
        t.stats.live_sent,
        t.stats.tiles_sent,
        t.stats.deferred,
        t.view.cache_bytes as f64 / (1 << 20) as f64,
        t.net.as_ref().map_or(0, |n| n
            .bytes
            .asks
            .load(std::sync::atomic::Ordering::Relaxed)),
        t.net.as_ref().map_or(0.0, |n| n
            .bytes
            .ask_wait_ns
            .load(std::sync::atomic::Ordering::Relaxed)
            as f64
            / 1e6),
        t.stats.serve_ns as f64 / 1e6,
        t.stats.ask_queue_ns as f64 / 1e6,
        t.view.shared.layers.len(),
        tl,
        edits.join(", ")
    )
}
