//! The live session: the host numbers edits and passes them on; cursors go as datagrams.
//!
//! - **Any member lets a joiner in:** a member that isn't the host passes the joiner on to
//!   the host, so an invite from anyone keeps working after the host changes.
//! - **Edits are signed and numbered by their author** ([`Signed`]): no one can pass one on
//!   as someone else's, and none is applied twice.
//! - **Host hand-over:** when the host leaves, the member who has been in longest takes over
//!   (everyone works it out the same way from the member list, which is in the order people
//!   came). The others reconnect to it, it sends them the edits they missed, they send it the
//!   edits it missed (their own not yet numbered, and others' it hadn't seen), and the session
//!   carries on, numbering from where it was.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMap, RelayMode, RelayUrl, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::invite::{self, Invite};
use crate::wire::{self, Frame, Ordered, Presence, Resume, Signed};
use crate::{ALPN, Error, Identity, Result, err};

/// How long a joiner has to say hello, and a member to answer.
const HANDSHAKE: Duration = Duration::from_secs(15);
/// How long to keep trying to reach a member who should take over as host.
const TAKEOVER: Duration = Duration::from_secs(12);
/// Recent numbered edits kept, to catch up members after a hand-over: at most this many,
/// and at most this many bytes (edits can carry pictures).
const LOG: usize = 1024;
const LOG_BYTES: usize = 32 << 20;

/// Which relays introduce members and carry traffic they can't send directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relays {
    /// iroh's public relays (free, no uptime promise: for testing).
    Iroh,
    /// Our own relays (e.g. `https://relay.example.com`).
    Custom(Vec<String>),
    /// No relay: only members that can reach each other directly (same network, tests).
    Off,
}

/// Someone in the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// their [`Identity::id`]
    pub id: String,
    pub name: String,
    /// how to reach them (their relay; IP addresses only without relays)
    pub addr: EndpointAddr,
}

/// What happens in the session, in the order the app should apply it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Edit number `seq`, by member `from`. Your own edits come back here too, numbered:
    /// apply edits in this order. An edit is never delivered twice.
    Edit {
        seq: u64,
        from: String,
        body: Value,
    },
    /// A cursor or other passing state from another member.
    Presence {
        from: String,
        body: Value,
    },
    Joined(Member),
    Left {
        id: String,
    },
    /// A message from one member to this one (see [`Session::direct`]).
    Direct {
        from: String,
        body: Value,
    },
    /// The host left and `host` took over (it may be you); who is here now.
    HostChanged {
        host: String,
        members: Vec<Member>,
    },
    /// The session ended (the host left and no one could take over, or you left).
    Closed {
        reason: String,
    },
}

/// A live shared session.
pub struct Session {
    core: Arc<Core>,
    events: UnboundedReceiver<Event>,
}

struct Core {
    endpoint: Endpoint,
    secret: SecretKey,
    me: Member,
    key: [u8; 32],
    project: String,
    events: UnboundedSender<Event>,
    st: Mutex<St>,
}

#[derive(Default)]
struct St {
    hosting: bool,
    /// the host (when not hosting)
    host: Option<EndpointAddr>,
    peers: HashMap<EndpointId, Peer>,
    next_conn: u64,
    link: Option<Link>,
    next_link: u64,
    /// in the order they came (the host first)
    members: Vec<Member>,
    seq: u64,
    log: VecDeque<Ordered>,
    log_bytes: usize,
    /// per author, the last edit number applied
    seen: HashMap<String, u64>,
    my_n: u64,
    /// this app's edits not yet seen numbered
    unacked: Vec<Signed>,
    closed: bool,
}

struct Peer {
    n: u64,
    conn: Connection,
    out: UnboundedSender<Frame>,
}

struct Link {
    n: u64,
    host: EndpointId,
    conn: Connection,
    out: UnboundedSender<Frame>,
}

enum Answer {
    In,
    Redirect(EndpointAddr),
    Refused(String),
}

impl Session {
    /// Start a new shared session with this app as the host. The invite (see
    /// [`Session::invite`]) is what others join with.
    pub async fn host(id: &Identity, name: &str, relays: Relays) -> Result<Self> {
        let (core, events) = Core::start(id, name, &relays, Invite::random_key()).await?;
        {
            let mut st = core.st.lock().unwrap();
            st.hosting = true;
            st.members = vec![core.me.clone()];
        }
        Ok(Self { core, events })
    }

    /// Join the session an invite points to (through any member in it).
    pub async fn join(id: &Identity, invite: &Invite, name: &str, relays: Relays) -> Result<Self> {
        let (core, events) = Core::start(id, name, &relays, invite.key).await?;
        let mut last = Error("The invite has no one to ask.".into());
        for addr in &invite.addrs {
            match core.clone().enter(addr.clone(), None).await {
                Ok(()) => return Ok(Self { core, events }),
                Err(e) => last = e,
            }
        }
        core.endpoint.close().await;
        Err(last)
    }

    /// The code others can join with: this app, then the host.
    pub fn invite(&self) -> Invite {
        let st = self.core.st.lock().unwrap();
        let mut addrs = vec![self.core.me.addr.clone()];
        if let Some(h) = st.host.as_ref().filter(|h| h.id != self.core.me.addr.id) {
            addrs.push(h.clone());
        }
        Invite::new(self.core.key, addrs)
    }

    pub fn me(&self) -> &Member {
        &self.core.me
    }

    pub fn is_host(&self) -> bool {
        self.core.st.lock().unwrap().hosting
    }

    /// Who is here now (this app included), in the order they came.
    pub fn members(&self) -> Vec<Member> {
        self.core.st.lock().unwrap().members.clone()
    }

    /// Send an edit. It comes back as an [`Event::Edit`] with its number once the host has
    /// ordered it. If the host is changing, it waits and goes to the new host.
    pub fn edit(&self, body: Value) -> Result<()> {
        let body = serde_json::to_string(&body).map_err(err)?;
        let (signed, hosting, out) = {
            let mut st = self.core.st.lock().unwrap();
            if st.closed {
                return Err(Error("The session has ended.".into()));
            }
            st.my_n += 1;
            let s = Signed::make(&self.core.secret, &self.core.project, st.my_n, body);
            st.unacked.push(s.clone());
            (s, st.hosting, st.link.as_ref().map(|l| l.out.clone()))
        };
        if hosting {
            self.core.order(signed);
        } else if let Some(out) = out {
            let _ = out.send(Frame::Edit { edit: signed });
        }
        Ok(())
    }

    /// Send a message to one member (`to`: their id), e.g. the project for someone who just
    /// joined. It arrives as [`Event::Direct`]; it isn't numbered or kept.
    pub fn direct(&self, to: &str, body: Value) -> Result<()> {
        let body = serde_json::to_string(&body).map_err(err)?;
        let st = self.core.st.lock().unwrap();
        let frame = Frame::Direct {
            to: to.to_string(),
            from: self.core.me.id.clone(),
            body,
        };
        if st.hosting {
            let peer = to
                .parse::<EndpointId>()
                .ok()
                .and_then(|id| st.peers.get(&id))
                .ok_or_else(|| Error("They aren't in the session.".into()))?;
            let _ = peer.out.send(frame);
        } else if let Some(l) = &st.link {
            let _ = l.out.send(frame);
        } else {
            return Err(Error("Not connected right now.".into()));
        }
        Ok(())
    }

    /// Send passing state (a cursor). It may be lost; send the newest often instead.
    pub fn presence(&self, body: Value) {
        let p = Presence {
            from: self.core.me.id.clone(),
            body,
        };
        let st = self.core.st.lock().unwrap();
        if st.hosting {
            for peer in st.peers.values() {
                send_datagram(&peer.conn, &p);
            }
        } else if let Some(l) = &st.link {
            send_datagram(&l.conn, &p);
        }
    }

    /// The next event, or `None` once the session is over and every event was read.
    pub async fn next_event(&mut self) -> Option<Event> {
        self.events.recv().await
    }

    /// An event if one is waiting.
    pub fn try_event(&mut self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Leave (closes every connection of this app's session).
    pub async fn close(self) {
        self.core.st.lock().unwrap().closed = true;
        self.core.endpoint.close().await;
    }
}

impl Core {
    async fn start(
        id: &Identity,
        name: &str,
        relays: &Relays,
        key: [u8; 32],
    ) -> Result<(Arc<Self>, UnboundedReceiver<Event>)> {
        let endpoint = bind(id, relays).await?;
        if *relays != Relays::Off {
            // reachable through the relay before handing out an address
            let _ = tokio::time::timeout(Duration::from_secs(10), endpoint.online()).await;
        }
        let full = endpoint.addr();
        let addr = if *relays == Relays::Off || full.relay_urls().next().is_none() {
            full
        } else {
            // a relay is enough to be found; home addresses stay private
            EndpointAddr::from_parts(
                full.id,
                full.relay_urls().cloned().map(iroh::TransportAddr::Relay),
            )
        };
        let me = Member {
            id: endpoint.id().to_string(),
            name: clean_name(name),
            addr,
        };
        let (events, rx) = unbounded_channel();
        let core = Arc::new(Core {
            endpoint: endpoint.clone(),
            secret: id.secret(),
            me,
            key,
            project: invite::project_of(&key),
            events,
            st: Mutex::new(St::default()),
        });
        tokio::spawn(accept_loop(core.clone()));
        Ok((core, rx))
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    /// As host: number an edit and pass it to everyone (in the same order to each).
    fn order(&self, edit: Signed) {
        if !edit.verify(&self.project) {
            return;
        }
        let mut st = self.st.lock().unwrap();
        if !st.hosting || st.seen.get(&edit.from).is_some_and(|&n| edit.n <= n) {
            return;
        }
        st.seq += 1;
        let o = Ordered { seq: st.seq, edit };
        for p in st.peers.values() {
            let _ = p.out.send(Frame::Ordered(o.clone()));
        }
        self.apply(&mut st, o);
    }

    /// Take a numbered edit in (unless it's here already) and tell the app.
    fn apply(&self, st: &mut St, o: Ordered) {
        st.seq = st.seq.max(o.seq);
        let e = &o.edit;
        if st.seen.get(&e.from).is_some_and(|&n| e.n <= n) {
            return;
        }
        st.seen.insert(e.from.clone(), e.n);
        if e.from == self.me.id {
            let n = e.n;
            st.unacked.retain(|u| u.n > n);
        }
        let body = serde_json::from_str(&e.body).unwrap_or(Value::Null);
        self.emit(Event::Edit {
            seq: o.seq,
            from: e.from.clone(),
            body,
        });
        st.log_bytes += o.edit.body.len();
        st.log.push_back(o);
        while st.log.len() > LOG || (st.log_bytes > LOG_BYTES && st.log.len() > 1) {
            if let Some(old) = st.log.pop_front() {
                st.log_bytes -= old.edit.body.len();
            }
        }
    }

    /// Become the host (the old one left and this app is next).
    fn take_over(&self) {
        let members = {
            let mut st = self.st.lock().unwrap();
            if st.hosting || st.closed {
                return;
            }
            st.hosting = true;
            st.host = None;
            st.link = None;
            st.members = vec![self.me.clone()];
            st.members.clone()
        };
        self.emit(Event::HostChanged {
            host: self.me.id.clone(),
            members,
        });
        // edits this app made that the old host never numbered
        let pending = std::mem::take(&mut self.st.lock().unwrap().unacked);
        for e in pending {
            self.order(e);
        }
    }

    /// Ask the member at `addr` to let this app in (following one redirect to the host).
    /// (Boxed: a reader that loses its host enters again, which starts a new reader.)
    fn enter(
        self: Arc<Self>,
        addr: EndpointAddr,
        resume: Option<Resume>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> {
        Box::pin(async move {
            let mut to = addr;
            for _ in 0..3 {
                match self.clone().dial(to.clone(), resume.clone()).await? {
                    Answer::In => return Ok(()),
                    Answer::Redirect(h) => to = h,
                    Answer::Refused(reason) => return Err(Error(reason)),
                }
            }
            Err(Error("Couldn't find the session's host.".into()))
        })
    }

    async fn dial(self: Arc<Self>, addr: EndpointAddr, resume: Option<Resume>) -> Result<Answer> {
        let target = addr.id;
        let conn = tokio::time::timeout(HANDSHAKE, self.endpoint.connect(addr.clone(), ALPN))
            .await
            .map_err(|_| Error("Couldn't reach the session: is someone in it online?".into()))?
            .map_err(|e| err(format!("Couldn't reach the session: {e}")))?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(err)?;
        let hello = Frame::Hello {
            project: self.project.clone(),
            proof: invite::proof(&self.key, self.endpoint.id(), target),
            name: self.me.name.clone(),
            addr: self.me.addr.clone(),
            resume,
        };
        wire::write(&mut send, &hello).await?;
        let answer = tokio::time::timeout(HANDSHAKE, wire::read(&mut recv))
            .await
            .map_err(|_| Error("The session didn't answer.".into()))??;
        let (members, seq, host_seen, missed) = match answer {
            Some(Frame::Welcome {
                members,
                seq,
                seen,
                missed,
            }) => (members, seq, seen, missed),
            Some(Frame::Redirect { host }) => return Ok(Answer::Redirect(host)),
            Some(Frame::Refused { reason }) => return Ok(Answer::Refused(reason)),
            _ => return Err(Error("The session answered something unexpected.".into())),
        };
        let (out, out_rx) = unbounded_channel();
        let n = {
            let mut st = self.st.lock().unwrap();
            if st.closed {
                return Err(Error("The session has ended.".into()));
            }
            st.next_link += 1;
            let n = st.next_link;
            st.link = Some(Link {
                n,
                host: target,
                conn: conn.clone(),
                out: out.clone(),
            });
            st.host = Some(addr);
            st.members = members;
            for o in missed {
                self.apply(&mut st, o);
            }
            st.seq = st.seq.max(seq);
            // what the host hasn't seen: this app's edits it never numbered, and others'
            // edits this app has but the host doesn't (signed by their authors)
            let unseen = |e: &Signed| host_seen.get(&e.from).is_none_or(|&h| e.n > h);
            for o in st.log.iter().filter(|o| unseen(&o.edit)) {
                let _ = out.send(Frame::Edit {
                    edit: o.edit.clone(),
                });
            }
            for e in st.unacked.iter().filter(|e| unseen(e)) {
                let _ = out.send(Frame::Edit { edit: e.clone() });
            }
            n
        };
        tokio::spawn(writer(send, out_rx));
        let reader: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
            Box::pin(self.clone().guest_reader(recv, n));
        tokio::spawn(reader);
        tokio::spawn(guest_datagrams(self.clone(), conn));
        Ok(Answer::In)
    }

    async fn guest_reader(self: Arc<Self>, mut recv: iroh::endpoint::RecvStream, n: u64) {
        loop {
            let frame = wire::read(&mut recv).await;
            let mut st = self.st.lock().unwrap();
            if st.closed || st.link.as_ref().is_none_or(|l| l.n != n) {
                return;
            }
            match frame {
                Ok(Some(Frame::Ordered(o))) => self.apply(&mut st, o),
                Ok(Some(Frame::Joined { member })) => {
                    st.members.retain(|x| x.id != member.id);
                    st.members.push(member.clone());
                    self.emit(Event::Joined(member));
                }
                Ok(Some(Frame::Left { id })) => {
                    st.members.retain(|x| x.id != id);
                    self.emit(Event::Left { id });
                }
                Ok(Some(Frame::Direct { from, body, .. })) => {
                    self.emit(Event::Direct {
                        from,
                        body: serde_json::from_str(&body).unwrap_or(Value::Null),
                    });
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        // the host is gone: hand over
        let (old, candidates, seen) = {
            let mut st = self.st.lock().unwrap();
            let Some(old) = st.link.take().map(|l| l.host) else {
                return;
            };
            let old_id = old.to_string();
            let candidates: Vec<Member> = st
                .members
                .iter()
                .filter(|m| m.id != old_id)
                .cloned()
                .collect();
            (old_id, candidates, st.seen.clone())
        };
        self.emit(Event::Left { id: old.clone() });
        for c in candidates {
            if c.id == self.me.id {
                self.take_over();
                return;
            }
            let resume = Resume {
                from_host: old.clone(),
                seen: seen.clone(),
            };
            let deadline = tokio::time::Instant::now() + TAKEOVER;
            while tokio::time::Instant::now() < deadline {
                if self.st.lock().unwrap().closed {
                    return;
                }
                if self
                    .clone()
                    .enter(c.addr.clone(), Some(resume.clone()))
                    .await
                    .is_ok()
                {
                    let members = self.st.lock().unwrap().members.clone();
                    let host = self
                        .st
                        .lock()
                        .unwrap()
                        .link
                        .as_ref()
                        .map(|l| l.host.to_string())
                        .unwrap_or(c.id.clone());
                    self.emit(Event::HostChanged { host, members });
                    return;
                }
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
        self.st.lock().unwrap().closed = true;
        self.emit(Event::Closed {
            reason: "The host left, and no one else could carry the session on.".into(),
        });
    }
}

async fn bind(id: &Identity, relays: &Relays) -> Result<Endpoint> {
    let builder = match relays {
        Relays::Iroh => Endpoint::builder(presets::N0),
        Relays::Custom(urls) => {
            let urls = urls
                .iter()
                .map(|u| u.parse::<RelayUrl>().map_err(|e| err(format!("{u}: {e}"))))
                .collect::<Result<Vec<_>>>()?;
            let b = Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Custom(urls.into_iter().collect::<RelayMap>()));
            // tests run their own relay, with a self-signed certificate
            #[cfg(test)]
            let b = b.ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify());
            b
        }
        Relays::Off => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
    };
    builder
        .secret_key(id.secret())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| err(format!("Couldn't start the connection: {e}")))
}

/// 1–24 visible characters, or "Someone".
fn clean_name(raw: &str) -> String {
    let n: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(24)
        .collect();
    if n.is_empty() { "Someone".into() } else { n }
}

fn send_datagram(conn: &Connection, p: &Presence) {
    let Ok(bytes) = serde_json::to_vec(p) else {
        return;
    };
    if conn
        .max_datagram_size()
        .is_some_and(|max| bytes.len() <= max)
    {
        let _ = conn.send_datagram(Bytes::from(bytes));
    }
}

async fn writer(mut send: iroh::endpoint::SendStream, mut rx: UnboundedReceiver<Frame>) {
    while let Some(f) = rx.recv().await {
        if wire::write(&mut send, &f).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
}

async fn accept_loop(core: Arc<Core>) {
    while let Some(incoming) = core.endpoint.accept().await {
        let core = core.clone();
        tokio::spawn(async move {
            if let Ok(conn) = incoming.await {
                let _ = serve(conn, core).await;
            }
        });
    }
}

/// Someone asking to join this app (or, after a hand-over, to rejoin it as host).
async fn serve(conn: Connection, core: Arc<Core>) -> Result<()> {
    let remote = conn.remote_id();
    let (mut send, mut recv) = tokio::time::timeout(HANDSHAKE, conn.accept_bi())
        .await
        .map_err(err)?
        .map_err(err)?;
    let hello = tokio::time::timeout(HANDSHAKE, wire::read(&mut recv))
        .await
        .map_err(err)??;
    let Some(Frame::Hello {
        project,
        proof,
        name,
        addr,
        resume,
    }) = hello
    else {
        conn.close(1u32.into(), b"hello first");
        return Ok(());
    };
    let refuse = |reason: &str| Frame::Refused {
        reason: reason.into(),
    };
    let answer = if project != core.project
        || !invite::check(&core.key, &proof, remote, core.endpoint.id())
    {
        Some(refuse(
            "This invite isn't valid any more: ask for a new one.",
        ))
    } else {
        // asked to take over by someone whose host left: if it was this app's host too
        let host_gone = {
            let st = core.st.lock().unwrap();
            !st.hosting
                && resume.as_ref().is_some_and(|r| {
                    st.host
                        .as_ref()
                        .is_none_or(|h| h.id.to_string() == r.from_host)
                })
        };
        if host_gone {
            core.take_over();
        }
        let st = core.st.lock().unwrap();
        if st.closed {
            Some(refuse("This session has ended."))
        } else if !st.hosting {
            st.host.clone().map(|host| Frame::Redirect { host })
        } else {
            None
        }
    };
    if let Some(answer) = answer {
        let _ = wire::write(&mut send, &answer).await;
        let _ = send.finish();
        // let the answer arrive before closing
        let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
        return Ok(());
    }

    let member = Member {
        id: remote.to_string(),
        name: clean_name(&name),
        addr: EndpointAddr::from_parts(remote, addr.addrs),
    };
    let (out, out_rx) = unbounded_channel();
    let n = {
        let mut st = core.st.lock().unwrap();
        st.next_conn += 1;
        let n = st.next_conn;
        // a rejoin (same member, new connection) replaces the old one
        if let Some(old) = st.peers.remove(&remote) {
            old.conn.close(0u32.into(), b"replaced");
        }
        st.members.retain(|m| m.id != member.id);
        for p in st.peers.values() {
            let _ = p.out.send(Frame::Joined {
                member: member.clone(),
            });
        }
        st.members.push(member.clone());
        let seen = resume.map(|r| r.seen);
        let missed = match &seen {
            Some(seen) => st
                .log
                .iter()
                .filter(|o| seen.get(&o.edit.from).is_none_or(|&s| o.edit.n > s))
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        // first on this member's queue, so nothing numbered after it arrives before it
        let _ = out.send(Frame::Welcome {
            members: st.members.clone(),
            seq: st.seq,
            seen: st.seen.clone(),
            missed,
        });
        st.peers.insert(
            remote,
            Peer {
                n,
                conn: conn.clone(),
                out,
            },
        );
        n
    };
    core.emit(Event::Joined(member.clone()));
    tokio::spawn(writer(send, out_rx));
    tokio::spawn(host_datagrams(conn.clone(), core.clone(), remote));

    // their edits (and, after a hand-over, others' this app missed), until they leave
    while let Ok(Some(frame)) = wire::read(&mut recv).await {
        match frame {
            Frame::Edit { edit } => core.order(edit),
            Frame::Direct { to, body, .. } => {
                let from = remote.to_string();
                if to == core.me.id {
                    core.emit(Event::Direct {
                        from,
                        body: serde_json::from_str(&body).unwrap_or(Value::Null),
                    });
                } else if let Some(p) = to.parse::<EndpointId>().ok().and_then(|id| {
                    core.st
                        .lock()
                        .unwrap()
                        .peers
                        .get(&id)
                        .map(|p| p.out.clone())
                }) {
                    let _ = p.send(Frame::Direct { to, from, body });
                }
            }
            _ => {}
        }
    }

    let mut st = core.st.lock().unwrap();
    if st.peers.get(&remote).is_some_and(|p| p.n == n) {
        st.peers.remove(&remote);
        st.members.retain(|m| m.id != member.id);
        for p in st.peers.values() {
            let _ = p.out.send(Frame::Left {
                id: member.id.clone(),
            });
        }
        core.emit(Event::Left { id: member.id });
    }
    Ok(())
}

async fn host_datagrams(conn: Connection, core: Arc<Core>, remote: EndpointId) {
    while let Ok(bytes) = conn.read_datagram().await {
        let Ok(mut p) = serde_json::from_slice::<Presence>(&bytes) else {
            continue;
        };
        // who sent it is the connection's id, whatever the message says
        p.from = remote.to_string();
        {
            let st = core.st.lock().unwrap();
            for (id, peer) in &st.peers {
                if *id != remote {
                    send_datagram(&peer.conn, &p);
                }
            }
        }
        core.emit(Event::Presence {
            from: p.from,
            body: p.body,
        });
    }
}

async fn guest_datagrams(core: Arc<Core>, conn: Connection) {
    while let Ok(bytes) = conn.read_datagram().await {
        if let Ok(p) = serde_json::from_slice::<Presence>(&bytes)
            && p.from != core.me.id
        {
            core.emit(Event::Presence {
                from: p.from,
                body: p.body,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn next(s: &mut Session) -> Event {
        tokio::time::timeout(Duration::from_secs(20), s.next_event())
            .await
            .expect("an event in time")
            .expect("the session is open")
    }

    /// The next event that isn't presence.
    async fn next_change(s: &mut Session) -> Event {
        loop {
            let e = next(s).await;
            if !matches!(e, Event::Presence { .. }) {
                return e;
            }
        }
    }

    /// The next edit, skipping joins, leaves and hand-overs.
    async fn next_edit(s: &mut Session) -> (u64, String, Value) {
        loop {
            if let Event::Edit { seq, from, body } = next_change(s).await {
                return (seq, from, body);
            }
        }
    }

    async fn host() -> Session {
        Session::host(&Identity::generate(), "Host", Relays::Off)
            .await
            .unwrap()
    }

    async fn join(code: &str, name: &str) -> Session {
        let invite = Invite::decode(code).unwrap();
        Session::join(&Identity::generate(), &invite, name, Relays::Off)
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn host_orders_edits_for_everyone_and_passes_cursors() {
        let mut host = host().await;
        let code = host.invite().encode();

        let mut a = join(&code, "  Ana\u{7}  ").await;
        assert_eq!(a.me().name, "Ana");
        assert_eq!(next(&mut host).await, Event::Joined(a.me().clone()));
        let mut b = join(&code, "Bo").await;
        assert_eq!(next(&mut host).await, Event::Joined(b.me().clone()));
        assert_eq!(next(&mut a).await, Event::Joined(b.me().clone()));
        let names: Vec<String> = b.members().into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["Host", "Ana", "Bo"]);
        assert_eq!(host.members().len(), 3);

        // edits from three apps at once: everyone sees the same numbered order
        a.edit(json!({"stroke": "a1"})).unwrap();
        b.edit(json!({"stroke": "b1"})).unwrap();
        host.edit(json!({"stroke": "h1"})).unwrap();
        a.edit(json!({"stroke": "a2"})).unwrap();
        let mut seen = Vec::new();
        for s in [&mut host, &mut a, &mut b] {
            let mut list = Vec::new();
            for _ in 0..4 {
                list.push(next_edit(s).await);
            }
            seen.push(list);
        }
        assert_eq!(seen[0], seen[1]);
        assert_eq!(seen[0], seen[2]);
        assert_eq!(
            seen[0].iter().map(|e| e.0).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        let a_edits: Vec<&Value> = seen[0]
            .iter()
            .filter(|e| e.1 == a.me().id)
            .map(|e| &e.2)
            .collect();
        assert_eq!(
            a_edits,
            [&json!({"stroke": "a1"}), &json!({"stroke": "a2"})]
        );

        // a cursor from a reaches the host and b, with a's real id
        let a_id = a.me().id.clone();
        for _ in 0..20 {
            a.presence(json!({"x": 10, "y": 20}));
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        for s in [&mut host, &mut b] {
            match next(s).await {
                Event::Presence { from, body } => {
                    assert_eq!(from, a_id);
                    assert_eq!(body, json!({"x": 10, "y": 20}));
                }
                other => panic!("expected a cursor, got {other:?}"),
            }
        }

        // b leaves: the others hear it
        let b_id = b.me().id.clone();
        b.close().await;
        assert_eq!(
            next_change(&mut host).await,
            Event::Left { id: b_id.clone() }
        );
        assert_eq!(next_change(&mut a).await, Event::Left { id: b_id });
        assert_eq!(host.members().len(), 2);
        host.close().await;
        a.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn when_the_host_leaves_the_longest_member_carries_on() {
        let mut h = host().await;
        let code = h.invite().encode();
        let mut a = join(&code, "Ana").await;
        let mut b = join(&code, "Bo").await;
        let (a_id, b_id) = (a.me().id.clone(), b.me().id.clone());
        a.edit(json!(1)).unwrap();
        b.edit(json!(2)).unwrap();
        for s in [&mut a, &mut b] {
            assert_eq!(next_edit(s).await.0, 1);
            assert_eq!(next_edit(s).await.0, 2);
        }
        let _ = next_edit(&mut h).await;
        let _ = next_edit(&mut h).await;

        // the host goes; b draws while the session is changing hands
        h.close().await;
        b.edit(json!("during")).unwrap();

        // a (in longest) takes over, b reconnects to it
        let mut a_host = false;
        loop {
            match next_change(&mut a).await {
                Event::HostChanged { host, .. } => {
                    assert_eq!(host, a_id);
                    a_host = true;
                }
                Event::Edit { seq, from, body } => {
                    assert!(a_host, "the edit came through the new host");
                    assert_eq!(
                        (seq, from.as_str(), body),
                        (3, b_id.as_str(), json!("during"))
                    );
                    break;
                }
                _ => {}
            }
        }
        assert!(a.is_host());
        let mut b_moved = false;
        loop {
            match next_change(&mut b).await {
                Event::HostChanged { host, members } => {
                    assert_eq!(host, a_id);
                    assert_eq!(members.len(), 2);
                    b_moved = true;
                }
                Event::Edit { seq, body, .. } => {
                    assert!(b_moved);
                    assert_eq!((seq, body), (3, json!("during")), "once, numbered by a");
                    break;
                }
                _ => {}
            }
        }
        // and it carries on
        b.edit(json!("after")).unwrap();
        assert_eq!(next_edit(&mut a).await.0, 4);
        assert_eq!(next_edit(&mut b).await, (4, b_id.clone(), json!("after")));

        // an invite made before the hand-over still works: c gets passed to a
        let mut c = join(&code_from(&b), "Cy").await;
        assert!(matches!(next_change(&mut a).await, Event::Joined(m) if m.name == "Cy"));
        c.edit(json!("hi")).unwrap();
        assert_eq!(next_edit(&mut a).await.0, 5);
        assert_eq!(next_edit(&mut c).await.0, 5);
        for s in [a, b, c] {
            s.close().await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn direct_messages_go_to_one_member_with_the_real_sender() {
        let mut h = host().await;
        let code = h.invite().encode();
        let mut a = join(&code, "Ana").await;
        let mut b = join(&code, "Bo").await;
        let (h_id, a_id, b_id) = (h.me().id.clone(), a.me().id.clone(), b.me().id.clone());
        // guest → guest (through the host), guest → host, host → guest
        a.direct(&b_id, json!({"k": "hi b"})).unwrap();
        a.direct(&h_id, json!({"k": "hi host"})).unwrap();
        h.direct(&a_id, json!({"k": "hi a"})).unwrap();
        async fn direct(s: &mut Session) -> (String, Value) {
            loop {
                if let Event::Direct { from, body } = next_change(s).await {
                    return (from, body);
                }
            }
        }
        assert_eq!(direct(&mut b).await, (a_id.clone(), json!({"k": "hi b"})));
        assert_eq!(direct(&mut h).await, (a_id, json!({"k": "hi host"})));
        assert_eq!(direct(&mut a).await, (h_id, json!({"k": "hi a"})));
        assert!(h.direct("nobody", json!(1)).is_err());
        for s in [h, a, b] {
            s.close().await;
        }
    }

    fn code_from(s: &Session) -> String {
        s.invite().encode()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_last_one_left_carries_on_alone() {
        let h = host().await;
        let mut a = join(&h.invite().encode(), "Ana").await;
        h.close().await;
        loop {
            if let Event::HostChanged { host, members } = next_change(&mut a).await {
                assert_eq!(host, a.me().id);
                assert_eq!(members.len(), 1);
                break;
            }
        }
        a.edit(json!("still here")).unwrap();
        assert_eq!(next_edit(&mut a).await.2, json!("still here"));
        a.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wrong_invite_is_refused() {
        let host = host().await;
        let mut forged = host.invite();
        forged.key = [0; 32];
        let e = Session::join(&Identity::generate(), &forged, "Eve", Relays::Off)
            .await
            .err()
            .expect("refused");
        assert!(e.0.contains("isn't valid"), "{e}");
        assert_eq!(host.members().len(), 1);
        host.close().await;
    }
}

/// Through our own relay (a relay server run in the test, as on the VPS): members that know
/// each other only by id and relay find each other.
#[cfg(test)]
mod own_relay {
    use super::*;
    use serde_json::json;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn joins_and_hands_over_through_our_own_relay() {
        let (_map, url, _server) = iroh::test_utils::run_relay_server().await.unwrap();
        let relays = Relays::Custom(vec![url.to_string()]);
        let mut h = Session::host(&Identity::generate(), "Host", relays.clone())
            .await
            .unwrap();
        let invite = h.invite();
        let via: Vec<String> = invite.addrs[0]
            .relay_urls()
            .map(|u| u.to_string())
            .collect();
        assert_eq!(via, [url.to_string()], "the code points at our relay");
        assert_eq!(invite.addrs[0].ip_addrs().count(), 0);
        let code = invite.encode();
        let a = Session::join(
            &Identity::generate(),
            &Invite::decode(&code).unwrap(),
            "Ana",
            relays.clone(),
        )
        .await
        .unwrap();
        let mut b = Session::join(
            &Identity::generate(),
            &Invite::decode(&code).unwrap(),
            "Bo",
            relays,
        )
        .await
        .unwrap();
        a.edit(json!("a")).unwrap();
        for s in [&mut h, &mut b] {
            loop {
                if let Some(Event::Edit { seq, .. }) =
                    tokio::time::timeout(Duration::from_secs(20), s.next_event())
                        .await
                        .unwrap()
                {
                    assert_eq!(seq, 1);
                    break;
                }
            }
        }
        h.close().await;
        b.edit(json!("b")).unwrap();
        loop {
            match tokio::time::timeout(Duration::from_secs(30), b.next_event())
                .await
                .unwrap()
            {
                Some(Event::Edit { seq: 2, body, .. }) => {
                    assert_eq!(body, json!("b"));
                    break;
                }
                Some(Event::Closed { reason }) => panic!("{reason}"),
                _ => {}
            }
        }
        assert!(a.is_host());
        a.close().await;
        b.close().await;
    }
}

/// Over the internet, through iroh's public relays (needs a connection; not run in CI):
/// `cargo test -p pencilpilot-share -- --ignored`
#[cfg(test)]
mod online {
    use super::*;
    use serde_json::json;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "uses the internet and iroh's public relays"]
    async fn joins_through_the_public_relays() {
        let mut host = Session::host(&Identity::generate(), "Host", Relays::Iroh)
            .await
            .unwrap();
        let invite = host.invite();
        assert!(
            invite.addrs[0].relay_urls().next().is_some(),
            "the invite carries a relay"
        );
        assert_eq!(invite.addrs[0].ip_addrs().count(), 0, "and no home address");
        let code = invite.encode();
        println!("code: {} characters", code.len());
        let t = std::time::Instant::now();
        let a = Session::join(
            &Identity::generate(),
            &Invite::decode(&code).unwrap(),
            "Ana",
            Relays::Iroh,
        )
        .await
        .unwrap();
        println!("joined through the relay in {:?}", t.elapsed());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), host.next_event())
                .await
                .unwrap(),
            Some(Event::Joined(a.me().clone()))
        );
        a.edit(json!({"hi": 1})).unwrap();
        let e = tokio::time::timeout(Duration::from_secs(10), host.next_event())
            .await
            .unwrap();
        assert!(matches!(e, Some(Event::Edit { seq: 1, .. })), "{e:?}");
        a.close().await;
        host.close().await;
    }
}
