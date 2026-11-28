//! The live session's own thread: `crates/share`'s [`Session`] on a small runtime of its
//! own, and `crates/together`'s [`Together`] (the shared project, edit for edit as the
//! current app keeps it) beside it, so nothing on the window's thread waits on the network
//! or does the session's bookkeeping.
//!
//! - **The canvas** is asked through [`port::RemotePort`]: the window answers between frames
//!   ([`super::serve`]); tiles are encoded, decoded and compared here.
//! - **The window's commands** (its changes, asks and answers, chat, cursors, live strokes)
//!   come in as [`Cmd`]s; what it should know goes out as [`In`]s.
//! - **Waking:** each message wakes the window ([`pencilpilot_window::Waker`]), so it
//!   neither polls nor sleeps past it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use pencilpilot_share::{EndpointAddr, Event, Identity, Invite, Member, Relays, Session};
use pencilpilot_together::{Notice, Out as TogetherOut, SavedMember, SavedShare, Together};
use serde_json::{Value, json};

use super::port::{self, RemotePort, Stamp};

/// How to start.
#[derive(Debug, Clone)]
pub enum Start {
    Host,
    Join(String),
    /// back to a session after a break: join whoever of its members is online, or host it
    /// again (the current app's `rejoin`)
    Rejoin(Box<Saved>),
}

/// A session kept to come back to (as the current app keeps it in the project file).
#[derive(Debug, Clone)]
pub struct Saved {
    pub share: SavedShare,
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

/// The shared state the interface shows (layers, who may draw, asks).
#[derive(Debug, Clone, Default)]
pub struct SharedView {
    pub shared: pencilpilot_together::Shared,
    /// shared layer → the painter's layer
    pub ids: Vec<(String, u32)>,
    pub synced: bool,
    /// bytes `Together` keeps of the project's pixels (its copy of every drawing)
    pub cache_bytes: usize,
}

/// What reaches the window.
pub enum In {
    Started(Info),
    Failed(String),
    /// the canvas asked (answer it between frames)
    Ask(port::Ask),
    Shared(SharedView),
    Note(String),
    /// someone's tiles came (their live preview can go)
    TilesCame(String),
    Presence {
        from: String,
        body: Value,
        at_ns: u64,
    },
    Chat {
        from: String,
        text: String,
    },
    Joined(Member),
    Left(String),
    HostChanged {
        you: bool,
        members: Vec<Member>,
        invite: String,
    },
    /// the project came (joining): synced, and the frame the host is on (a native host's)
    Synced(Option<u32>),
    /// the session ended (`why`), and what to come back with
    Closed {
        why: Option<String>,
        saved: Option<Box<Saved>>,
    },
}

/// What the window asks.
pub enum Cmd {
    Presence(Value),
    Chat(String),
    /// the canvas changed here: the timeline (first), drawings that may have changed (an
    /// undo elsewhere), when the stroke ended (checks)
    Changed {
        timeline: bool,
        touched: Vec<u32>,
        t_ns: Option<u64>,
    },
    AddOwnLayer(String),
    DeleteLayer(u32),
    MoveOwn(String, u32),
    Ask(String),
    Grant(String, String, bool),
    Deny(String, String),
    Revoke(String, String),
    /// an edit as it is (checks: what another app would send)
    RawEdit(Value),
    Leave,
}

/// Bytes through the session (the edits' and messages' JSON, both ways), for `--stats`.
#[derive(Debug, Default)]
pub struct Bytes {
    pub sent: AtomicU64,
    pub received: AtomicU64,
    /// the canvas asked (calls), and how long the session's thread waited for its answers
    pub asks: AtomicU64,
    pub ask_wait_ns: AtomicU64,
    /// passing state (live strokes, cursors) goes from the window's thread straight out:
    /// never behind the session thread's work
    pub presencer: std::sync::Mutex<Option<pencilpilot_share::Presencer>>,
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
    /// Hosts, joins or rejoins on a new thread; [`In::Started`] or [`In::Failed`] comes
    /// first. `stamp`: checks put the time each stroke ended on its tiles.
    pub fn start(
        start: Start,
        id: Identity,
        name: String,
        relays: Relays,
        waker: Option<pencilpilot_window::Waker>,
        stamp: bool,
    ) -> Net {
        let (tx, crx) = tokio::sync::mpsc::unbounded_channel();
        let (itx, rx) = mpsc::channel();
        let bytes = Arc::new(Bytes::default());
        let b = bytes.clone();
        let thread = std::thread::Builder::new()
            .name("together".into())
            .spawn(move || {
                super::port::behind();
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("together-io")
                    .on_thread_start(super::port::behind)
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
                rt.block_on(run(start, id, name, relays, crx, out, b, stamp));
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
        if let Cmd::Presence(body) = c {
            let p = self.bytes.presencer.lock().unwrap().clone();
            return match p {
                Some(p) => {
                    p.send(body);
                    true
                }
                None => self.tx.send(Cmd::Presence(body)).is_ok(),
            };
        }
        self.tx.send(c).is_ok()
    }

    /// What's come (never waits).
    pub fn try_recv(&self) -> Option<In> {
        self.rx.try_recv().ok()
    }

    /// The next message, waited for at most `d`.
    pub fn recv_timeout(&self, d: Duration) -> Option<In> {
        self.rx.recv_timeout(d).ok()
    }

    /// Leaves: the thread closes the session (waited for at most `wait`; the canvas's
    /// questions meanwhile go unanswered). What to come back with, if it said.
    pub fn leave(mut self, wait: Duration) -> Option<Box<Saved>> {
        let _ = self.tx.send(Cmd::Leave);
        let mut saved = None;
        let until = std::time::Instant::now() + wait;
        loop {
            while let Ok(m) = self.rx.try_recv() {
                if let In::Closed { saved: Some(s), .. } = m {
                    saved = Some(s);
                }
            }
            let done = self.thread.as_ref().is_none_or(|t| t.is_finished());
            if done || std::time::Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(t) = self.thread.take()
            && t.is_finished()
        {
            let _ = t.join();
        }
        saved
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

impl Clone for Out {
    fn clone(&self) -> Out {
        Out {
            tx: self.tx.clone(),
            waker: self.waker.clone(),
        }
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

/// Back to a session: whoever of its members is online, or hosting it again (share.rs's
/// `rejoin`).
async fn rejoin(
    id: &Identity,
    saved: &Saved,
    name: &str,
    relays: Relays,
) -> pencilpilot_share::Result<Session> {
    let mut invite = Invite::decode(&saved.share.code)?;
    for m in &saved.share.members {
        if let Ok(a) = serde_json::from_value::<EndpointAddr>(m.addr.clone())
            && a.id.to_string() != id.id()
            && !invite.addrs.iter().any(|x| x.id == a.id)
        {
            invite.addrs.push(a);
        }
    }
    invite.addrs.retain(|a| a.id.to_string() != id.id());
    if !invite.addrs.is_empty()
        && let Ok(Ok(s)) = tokio::time::timeout(
            Duration::from_secs(12),
            Session::join(id, &invite, name, relays.clone()),
        )
        .await
    {
        return Ok(s);
    }
    Session::host_again(id, &invite, name, relays).await
}

/// What `Together` queued: to the session (a native host's snapshot says the project's
/// size and frame too), its notices to the window.
struct Pump {
    out: Out,
    bytes: Arc<Bytes>,
    /// checks: when this app's last stroke ended, put on the tiles it sends
    stamp: bool,
    t_ns: Option<u64>,
}

impl Pump {
    fn flush(&mut self, t: &mut Together, s: &Session, port: &RemotePort) -> bool {
        let size = |v: &Value| v.to_string().len() as u64;
        let mut ok = true;
        let outs = t.take_out();
        wire_log(&outs);
        for o in outs {
            match o {
                TogetherOut::Edit(mut body) => {
                    if self.stamp
                        && let Some(ns) = self.t_ns
                        && body.get("k").and_then(Value::as_str) == Some("tiles")
                    {
                        body["t"] = json!(ns);
                    }
                    self.bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                    if s.edit(body).is_err() {
                        ok = false;
                    }
                }
                TogetherOut::Direct { to, mut body } => {
                    if body.get("k").and_then(Value::as_str) == Some("snap.begin")
                        && let Some((w, h, f)) = port.doc()
                    {
                        // (a native host's extras: the project's size and frame; the
                        // current app reads none of them)
                        body["w"] = json!(w);
                        body["h"] = json!(h);
                        body["frame"] = json!(f);
                    }
                    self.bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                    let _ = s.direct(&to, body);
                }
            }
        }
        let mut changed = false;
        for n in t.take_notices() {
            match n {
                Notice::Note(text) => {
                    self.out.send(In::Note(text));
                }
                Notice::Tiles { from } => {
                    self.out.send(In::TilesCame(from));
                }
                Notice::Changed => changed = true,
            }
        }
        if changed {
            self.view(t);
        }
        ok
    }

    fn view(&self, t: &Together) {
        self.out.send(In::Shared(SharedView {
            shared: t.shared.clone(),
            ids: t.ids.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            synced: t.synced(),
            cache_bytes: t.cache_bytes(),
        }));
    }
}

/// Checks: what this app sends, one line each (`PENCILPILOT_WIRE_LOG`: where), without the
/// pixels: the kind, layer, drawing, how many tiles, and the timeline's tracks.
fn wire_log(outs: &[TogetherOut]) {
    use std::io::Write;
    let Some(path) = std::env::var_os("PENCILPILOT_WIRE_LOG") else {
        return;
    };
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    for o in outs {
        let (to, b) = match o {
            TogetherOut::Edit(b) => (None, b),
            TogetherOut::Direct { to, body } => (Some(&to[..12.min(to.len())]), body),
        };
        let n = |k: &str| b.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        let line = json!({
            "to": to, "k": b.get("k"), "lid": b.get("lid"), "did": b.get("did"),
            "set": n("set"), "clear": n("clear"),
            "tl": b.get("tl").map(|t| t["scenes"].as_array().map(|sc| sc.iter().map(|s|
                s["cuts"].as_array().map(|c| c.iter().map(|c| c["tracks"].clone()).collect::<Vec<_>>())
            ).collect::<Vec<_>>())),
            "layers": b.get("layers").map(|l| l.as_array().map(|a| a.iter().map(|x| (x["lid"].clone(), x["name"].clone())).collect::<Vec<_>>())),
            "ns": now_ns(),
        });
        let _ = writeln!(f, "{line}");
    }
}

fn saved_of(t: &Together, s: &Session, everyone: &[Member]) -> Box<Saved> {
    let members: Vec<SavedMember> = everyone
        .iter()
        .map(|m| SavedMember {
            id: m.id.clone(),
            name: m.name.clone(),
            addr: serde_json::to_value(&m.addr).unwrap_or(Value::Null),
        })
        .collect();
    Box::new(Saved {
        share: t.save(&s.invite().encode(), &members),
    })
}

#[allow(clippy::too_many_arguments)]
async fn run(
    start: Start,
    id: Identity,
    name: String,
    relays: Relays,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    out: Out,
    bytes: Arc<Bytes>,
    stamp: bool,
) {
    let (joining, restore) = match &start {
        Start::Host => (false, None),
        Start::Join(_) => (true, None),
        Start::Rejoin(saved) => (false, Some(saved.clone())),
    };
    let s = match &start {
        Start::Host => Session::host(&id, &name, relays).await,
        Start::Join(code) => match Invite::decode(code) {
            Ok(inv) => Session::join(&id, &inv, &name, relays).await,
            Err(e) => Err(e),
        },
        Start::Rejoin(saved) => rejoin(&id, saved, &name, relays).await,
    };
    let mut s = match s {
        Ok(s) => s,
        Err(e) => {
            out.send(In::Failed(e.to_string()));
            return;
        }
    };
    let info = info_of(&s);
    *bytes.presencer.lock().unwrap() = Some(s.presencer());
    if !out.send(In::Started(info.clone())) {
        s.close().await;
        return;
    }
    let asker = out.clone();
    let mut port = RemotePort::new(move |a| asker.send(In::Ask(a)));
    port.bytes = Some(bytes.clone());
    let mut t = Together::new(&info.me.id, &info.me.name);
    let mut pump = Pump {
        out: out.clone(),
        bytes: bytes.clone(),
        stamp,
        t_ns: None,
    };
    // everyone who's been in it, to come back to later
    let mut everyone: Vec<Member> = info.members.clone();
    let host = info.members.first().map(|m| m.id.clone());
    tokio::task::block_in_place(|| {
        if let Some(saved) = &restore {
            t.restore(&mut port, &saved.share);
            match &host {
                Some(h) if !info.host => t.rejoin(h),
                _ => t.resume_host(&mut port),
            }
        } else if joining {
            // (the project here stays, put aside: hidden and locked, as the current app
            // keeps its own when joining)
            if let Some(h) = &host {
                t.start_join(&mut port, h);
            }
        } else {
            t.start_host(&mut port);
        }
        pump.flush(&mut t, &s, &port);
        pump.view(&t);
    });
    let mut was_synced = t.synced();
    if was_synced {
        out.send(In::Synced(None));
    }
    let size = |v: &Value| v.to_string().len() as u64;
    let mut snap_frame: Option<u32> = None;
    let mut why: Option<String> = None;
    // what couldn't go into a busy canvas is tried again (a stroke ended, …)
    let mut again = tokio::time::interval(Duration::from_millis(30));
    again.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = again.tick() => {
                // (a big picture's tiles came, decoded on threads of their own and freed on
                // the window's: what they used goes back to the system once they've stopped)
                if port.untrimmed > 16 << 20
                    && port.put_at.is_some_and(|t| t.elapsed() > Duration::from_millis(300))
                {
                    port.untrimmed = 0;
                    // SAFETY: glibc's own call, no arguments that matter
                    unsafe {
                        libc::malloc_trim(0);
                    }
                }
                if t.waiting() {
                    tokio::task::block_in_place(|| {
                        t.retry(&mut port);
                        pump.flush(&mut t, &s, &port);
                    });
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                if matches!(cmd, Cmd::Leave) {
                    break;
                }
                match cmd {
                    Cmd::Presence(body) => s.presence(body),
                    Cmd::RawEdit(body) => {
                        let _ = s.edit(body);
                    }
                    Cmd::Chat(text) => {
                        for m in s.members() {
                            if m.id != s.me().id {
                                let body = json!({"k": "chat", "text": text});
                                bytes.sent.fetch_add(size(&body), Ordering::Relaxed);
                                let _ = s.direct(&m.id, body);
                            }
                        }
                    }
                    cmd => tokio::task::block_in_place(|| {
                        match cmd {
                            Cmd::Changed { timeline, touched, t_ns } => {
                                if t_ns.is_some() {
                                    pump.t_ns = t_ns;
                                }
                                if timeline {
                                    t.timeline_changed(&mut port);
                                }
                                t.touch(&touched);
                                t.local_changed(&mut port);
                            }
                            Cmd::AddOwnLayer(n) => t.add_own_layer(&n, false),
                            Cmd::DeleteLayer(id) => {
                                let _ = t.delete_layer(&mut port, id);
                            }
                            Cmd::MoveOwn(lid, to) => t.move_own(&lid, to),
                            Cmd::Ask(lid) => t.ask(&lid),
                            Cmd::Grant(lid, to, h) => t.grant(&lid, &to, h),
                            Cmd::Deny(lid, to) => t.deny(&lid, &to),
                            Cmd::Revoke(lid, to) => t.revoke(&lid, &to),
                            _ => {}
                        }
                        if !pump.flush(&mut t, &s, &port) {
                            why = Some("The session has ended.".into());
                        }
                    }),
                }
                if why.is_some() {
                    break;
                }
            }
            ev = s.next_event() => {
                let Some(e) = ev else { break };
                let at_ns = now_ns();
                match e {
                    Event::Edit { seq, from, body } => {
                        bytes.received.fetch_add(size(&body), Ordering::Relaxed);
                        let sent_ns = body.get("t").and_then(Value::as_u64);
                        port.stamp = Some(Stamp { kind: "tiles", from: from.clone(), received_ns: at_ns, sent_ns });
                        tokio::task::block_in_place(|| {
                            t.on_edit(&mut port, seq, &from, &body);
                            pump.flush(&mut t, &s, &port);
                            // (access and asks shown as they are now)
                            if body.get("k").and_then(Value::as_str) != Some("tiles") {
                                pump.view(&t);
                            }
                        });
                        port.stamp = None;
                    }
                    Event::Direct { from, body } => {
                        bytes.received.fetch_add(size(&body), Ordering::Relaxed);
                        if let Some(text) = pencilpilot_together::chat::chat_text(&body) {
                            out.send(In::Chat { from, text });
                        } else {
                            let k = body.get("k").and_then(Value::as_str).unwrap_or("");
                            if k == "snap.begin" && !t.synced() {
                                // a native host says the project's size and frame
                                let w = body.get("w").and_then(Value::as_u64);
                                let h = body.get("h").and_then(Value::as_u64);
                                if let (Some(w), Some(h)) = (w, h)
                                    && joining
                                    && (1..=16384).contains(&w)
                                    && (1..=16384).contains(&h)
                                    && port.doc().is_some_and(|(cw, ch, _)| (cw, ch) != (w as u32, h as u32))
                                {
                                    tokio::task::block_in_place(|| port.new_doc(Some((w as u32, h as u32))));
                                }
                                snap_frame = body.get("frame").and_then(Value::as_u64).map(|f| f as u32);
                            }
                            port.stamp = Some(Stamp { kind: "snapshot", from: from.clone(), received_ns: at_ns, sent_ns: None });
                            tokio::task::block_in_place(|| {
                                t.on_direct(&mut port, &from, &body);
                                pump.flush(&mut t, &s, &port);
                            });
                            port.stamp = None;
                        }
                    }
                    Event::Presence { from, body } => {
                        out.send(In::Presence { from, body, at_ns });
                    }
                    Event::Joined(m) => {
                        if !everyone.iter().any(|x| x.id == m.id) {
                            everyone.push(m.clone());
                        }
                        out.send(In::Joined(m));
                    }
                    Event::Left { id } => {
                        out.send(In::Left(id));
                    }
                    Event::HostChanged { host, members } => {
                        out.send(In::HostChanged {
                            you: host == s.me().id,
                            members,
                            invite: s.invite().encode(),
                        });
                    }
                    Event::Closed { reason } => {
                        why = Some(reason);
                        break;
                    }
                }
                if !was_synced && t.synced() {
                    was_synced = true;
                    // (joining went through every drawing once: what it used for that goes
                    // back to the system)
                    // SAFETY: glibc's own call, no arguments that matter
                    unsafe {
                        libc::malloc_trim(0);
                    }
                    out.send(In::Synced(snap_frame.take()));
                }
            }
        }
    }
    let saved = saved_of(&t, &s, &everyone);
    *bytes.presencer.lock().unwrap() = None;
    out.send(In::Closed {
        why,
        saved: Some(saved),
    });
    s.close().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invites_are_found_in_pasted_text() {
        assert_eq!(
            clean_invite("join me: pp1-AbC_d-9 thanks").as_deref(),
            Some("pp1-AbC_d-9")
        );
        assert_eq!(clean_invite("nothing here"), None);
    }
}
