//! The live session: the host numbers edits and passes them on; cursors go as datagrams.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointId, RelayMap, RelayMode, RelayUrl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::wire::{self, Frame, Presence};
use crate::{ALPN, Error, Identity, Invite, Result, err};

/// How long a joiner has to say hello, and a host to answer.
const HANDSHAKE: Duration = Duration::from_secs(15);

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
}

/// What happens in the session, in the order the app should apply it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Edit number `seq` (counting from 1), by member `from`. Your own edits come back here
    /// too, numbered: apply edits in this order and everyone ends up the same.
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
    /// The session ended (for a joiner: the host left or the connection dropped).
    Closed {
        reason: String,
    },
}

/// A live shared session, as the host or as a joiner.
pub struct Session {
    endpoint: Endpoint,
    me: Member,
    invite: Invite,
    members: Arc<Mutex<Vec<Member>>>,
    role: Role,
    events: UnboundedReceiver<Event>,
}

enum Role {
    Host(Arc<Hub>),
    Guest {
        conn: Connection,
        out: UnboundedSender<Frame>,
    },
}

impl Session {
    /// Start a new shared session with this app as the host. The invite (see
    /// [`Session::invite`]) is what others join with.
    pub async fn host(id: &Identity, name: &str, relays: Relays) -> Result<Self> {
        let endpoint = bind(id, &relays).await?;
        if relays != Relays::Off {
            // reachable through the relay before handing out an address
            let _ = tokio::time::timeout(Duration::from_secs(10), endpoint.online()).await;
        }
        let invite = Invite::new(endpoint.addr());
        Self::host_with(endpoint, invite, name)
    }

    fn host_with(endpoint: Endpoint, invite: Invite, name: &str) -> Result<Self> {
        let me = Member {
            id: endpoint.id().to_string(),
            name: clean_name(name),
        };
        let (tx, events) = unbounded_channel();
        let members = Arc::new(Mutex::new(vec![me.clone()]));
        let hub = Arc::new(Hub {
            me: me.clone(),
            invite: invite.clone(),
            host_id: endpoint.id(),
            events: tx,
            members: members.clone(),
            state: Mutex::new(HubState::default()),
        });
        tokio::spawn(accept_loop(endpoint.clone(), hub.clone()));
        Ok(Self {
            endpoint,
            me,
            invite,
            members,
            role: Role::Host(hub),
            events,
        })
    }

    /// Join the session an invite points to.
    pub async fn join(id: &Identity, invite: &Invite, name: &str, relays: Relays) -> Result<Self> {
        let endpoint = bind(id, &relays).await?;
        let me = Member {
            id: endpoint.id().to_string(),
            name: clean_name(name),
        };
        let host = invite.host.id;
        let conn = tokio::time::timeout(HANDSHAKE, endpoint.connect(invite.host.clone(), ALPN))
            .await
            .map_err(|_| Error("Couldn't reach the host: is their app open?".into()))?
            .map_err(|e| err(format!("Couldn't reach the host: {e}")))?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(err)?;
        let hello = Frame::Hello {
            project: invite.project.clone(),
            proof: invite.proof(endpoint.id(), host)?,
            name: me.name.clone(),
        };
        wire::write(&mut send, &hello).await?;
        let answer = tokio::time::timeout(HANDSHAKE, wire::read(&mut recv))
            .await
            .map_err(|_| Error("The host didn't answer.".into()))??;
        let (list, _seq) = match answer {
            Some(Frame::Welcome { members, seq }) => (members, seq),
            Some(Frame::Refused { reason }) => return Err(Error(reason)),
            _ => return Err(Error("The host answered something unexpected.".into())),
        };
        let members = Arc::new(Mutex::new(list));
        let (tx, events) = unbounded_channel();
        let (out, out_rx) = unbounded_channel();
        tokio::spawn(writer(send, out_rx));
        tokio::spawn(guest_reader(recv, tx.clone(), members.clone()));
        tokio::spawn(guest_datagrams(conn.clone(), tx, me.id.clone()));
        Ok(Self {
            endpoint,
            me,
            invite: invite.clone(),
            members,
            role: Role::Guest { conn, out },
            events,
        })
    }

    /// The code others join with (the host's, also for joiners).
    pub fn invite(&self) -> &Invite {
        &self.invite
    }

    pub fn me(&self) -> &Member {
        &self.me
    }

    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host(_))
    }

    /// Who is here now (this app included).
    pub fn members(&self) -> Vec<Member> {
        self.members.lock().unwrap().clone()
    }

    /// Send an edit. It comes back as an [`Event::Edit`] with its number once the host
    /// has ordered it.
    pub fn edit(&self, body: Value) -> Result<()> {
        match &self.role {
            Role::Host(hub) => {
                hub.order(hub.me.id.clone(), body);
                Ok(())
            }
            Role::Guest { out, .. } => out
                .send(Frame::Edit { body })
                .map_err(|_| Error("Not connected to the host.".into())),
        }
    }

    /// Send passing state (a cursor). It may be lost; send the newest often instead.
    pub fn presence(&self, body: Value) {
        let p = Presence {
            from: self.me.id.clone(),
            body,
        };
        match &self.role {
            Role::Host(hub) => hub.spread_presence(&p, None),
            Role::Guest { conn, .. } => send_datagram(conn, &p),
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
        self.endpoint.close().await;
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
            Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Custom(urls.into_iter().collect::<RelayMap>()))
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

// ---- the host ----

struct Hub {
    me: Member,
    invite: Invite,
    host_id: EndpointId,
    events: UnboundedSender<Event>,
    members: Arc<Mutex<Vec<Member>>>,
    state: Mutex<HubState>,
}

#[derive(Default)]
struct HubState {
    seq: u64,
    /// connection number (to tell a rejoin from the connection it replaces)
    next: u64,
    peers: HashMap<EndpointId, Peer>,
}

struct Peer {
    n: u64,
    member: Member,
    conn: Connection,
    out: UnboundedSender<Frame>,
}

impl Hub {
    /// Number an edit and pass it to everyone (in the same order to each).
    fn order(&self, from: String, body: Value) {
        let mut s = self.state.lock().unwrap();
        s.seq += 1;
        let seq = s.seq;
        for p in s.peers.values() {
            let _ = p.out.send(Frame::Ordered {
                seq,
                from: from.clone(),
                body: body.clone(),
            });
        }
        let _ = self.events.send(Event::Edit { seq, from, body });
    }

    fn spread_presence(&self, p: &Presence, except: Option<EndpointId>) {
        let s = self.state.lock().unwrap();
        for (id, peer) in &s.peers {
            if Some(*id) != except {
                send_datagram(&peer.conn, p);
            }
        }
    }

    fn sync_members(&self, s: &HubState) {
        let mut list = vec![self.me.clone()];
        list.extend(s.peers.values().map(|p| p.member.clone()));
        *self.members.lock().unwrap() = list;
    }
}

async fn accept_loop(endpoint: Endpoint, hub: Arc<Hub>) {
    while let Some(incoming) = endpoint.accept().await {
        let hub = hub.clone();
        tokio::spawn(async move {
            if let Ok(conn) = incoming.await {
                let _ = serve(conn, hub).await;
            }
        });
    }
}

/// One joiner, from hello to goodbye.
async fn serve(conn: Connection, hub: Arc<Hub>) -> Result<()> {
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
    }) = hello
    else {
        conn.close(1u32.into(), b"hello first");
        return Ok(());
    };
    if project != hub.invite.project || !hub.invite.check(&proof, remote, hub.host_id) {
        let reason = "This invite isn't valid any more: ask for a new one.".to_string();
        let _ = wire::write(&mut send, &Frame::Refused { reason }).await;
        let _ = send.finish();
        // let the refusal arrive before closing
        let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
        return Ok(());
    }
    let member = Member {
        id: remote.to_string(),
        name: clean_name(&name),
    };
    let (out, out_rx) = unbounded_channel();
    let n = {
        let mut s = hub.state.lock().unwrap();
        s.next += 1;
        let n = s.next;
        // a rejoin (same member, new connection) replaces the old one
        if let Some(old) = s.peers.remove(&remote) {
            old.conn.close(0u32.into(), b"replaced");
        }
        for p in s.peers.values() {
            let _ = p.out.send(Frame::Joined {
                member: member.clone(),
            });
        }
        let mut members = vec![hub.me.clone()];
        members.extend(s.peers.values().map(|p| p.member.clone()));
        members.push(member.clone());
        // first on this member's queue, so nothing ordered after it arrives before it
        let _ = out.send(Frame::Welcome {
            members,
            seq: s.seq,
        });
        s.peers.insert(
            remote,
            Peer {
                n,
                member: member.clone(),
                conn: conn.clone(),
                out,
            },
        );
        hub.sync_members(&s);
        n
    };
    let _ = hub.events.send(Event::Joined(member.clone()));
    tokio::spawn(writer(send, out_rx));
    tokio::spawn(host_datagrams(conn.clone(), hub.clone(), remote));

    // their edits, until they leave
    while let Ok(Some(frame)) = wire::read(&mut recv).await {
        if let Frame::Edit { body } = frame {
            hub.order(member.id.clone(), body);
        }
    }

    let mut s = hub.state.lock().unwrap();
    if s.peers.get(&remote).is_some_and(|p| p.n == n) {
        s.peers.remove(&remote);
        for p in s.peers.values() {
            let _ = p.out.send(Frame::Left {
                id: member.id.clone(),
            });
        }
        hub.sync_members(&s);
        let _ = hub.events.send(Event::Left { id: member.id });
    }
    Ok(())
}

async fn host_datagrams(conn: Connection, hub: Arc<Hub>, remote: EndpointId) {
    while let Ok(bytes) = conn.read_datagram().await {
        let Ok(mut p) = serde_json::from_slice::<Presence>(&bytes) else {
            continue;
        };
        // who sent it is the connection's id, whatever the message says
        p.from = remote.to_string();
        hub.spread_presence(&p, Some(remote));
        let _ = hub.events.send(Event::Presence {
            from: p.from,
            body: p.body,
        });
    }
}

// ---- a joiner ----

async fn guest_reader(
    mut recv: iroh::endpoint::RecvStream,
    tx: UnboundedSender<Event>,
    members: Arc<Mutex<Vec<Member>>>,
) {
    let reason = loop {
        match wire::read(&mut recv).await {
            Ok(Some(Frame::Ordered { seq, from, body })) => {
                let _ = tx.send(Event::Edit { seq, from, body });
            }
            Ok(Some(Frame::Joined { member })) => {
                let mut m = members.lock().unwrap();
                m.retain(|x| x.id != member.id);
                m.push(member.clone());
                let _ = tx.send(Event::Joined(member));
            }
            Ok(Some(Frame::Left { id })) => {
                members.lock().unwrap().retain(|x| x.id != id);
                let _ = tx.send(Event::Left { id });
            }
            Ok(Some(_)) => {}
            Ok(None) => break "The host left the session.".to_string(),
            Err(e) => break format!("The connection to the host dropped: {e}"),
        }
    };
    let _ = tx.send(Event::Closed { reason });
}

async fn guest_datagrams(conn: Connection, tx: UnboundedSender<Event>, me: String) {
    while let Ok(bytes) = conn.read_datagram().await {
        if let Ok(p) = serde_json::from_slice::<Presence>(&bytes)
            && p.from != me
        {
            let _ = tx.send(Event::Presence {
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
        tokio::time::timeout(Duration::from_secs(10), s.next_event())
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn host_orders_edits_for_everyone_and_passes_cursors() {
        let mut host = Session::host(&Identity::generate(), "Host", Relays::Off)
            .await
            .unwrap();
        let code = host.invite().encode();
        let invite = Invite::decode(&code).unwrap();

        let mut a = Session::join(&Identity::generate(), &invite, "  Ana\u{7}  ", Relays::Off)
            .await
            .unwrap();
        assert_eq!(a.me().name, "Ana");
        assert_eq!(next(&mut host).await, Event::Joined(a.me().clone()));
        let mut b = Session::join(&Identity::generate(), &invite, "Bo", Relays::Off)
            .await
            .unwrap();
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
                match next_change(s).await {
                    Event::Edit { seq, from, body } => list.push((seq, from, body)),
                    other => panic!("expected an edit, got {other:?}"),
                }
            }
            seen.push(list);
        }
        assert_eq!(seen[0], seen[1]);
        assert_eq!(seen[0], seen[2]);
        assert_eq!(
            seen[0].iter().map(|e| e.0).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        // a's own edits keep their order, and carry a's id
        let a_edits: Vec<&Value> = seen[0]
            .iter()
            .filter(|e| e.1 == a.me().id)
            .map(|e| &e.2)
            .collect();
        assert_eq!(
            a_edits,
            [&json!({"stroke": "a1"}), &json!({"stroke": "a2"})]
        );

        // a cursor from a reaches the host and b, with a's real id (not what a claims)
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

        // the host leaves: a's session ends
        host.close().await;
        assert!(matches!(next_change(&mut a).await, Event::Closed { .. }));
        a.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wrong_invite_is_refused() {
        let host = Session::host(&Identity::generate(), "Host", Relays::Off)
            .await
            .unwrap();
        let mut forged = host.invite().clone();
        forged.key = "00".repeat(32);
        let e = Session::join(&Identity::generate(), &forged, "Eve", Relays::Off)
            .await
            .err()
            .expect("refused");
        assert!(e.0.contains("isn't valid"), "{e}");
        assert_eq!(host.members().len(), 1);
        host.close().await;
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
        let invite = host.invite().clone();
        assert!(
            invite.host.relay_urls().next().is_some(),
            "the invite carries a relay"
        );
        // joiner knows only the host's id and relay: no direct address
        let mut only_relay = invite.clone();
        only_relay.host = iroh::EndpointAddr::from_parts(
            invite.host.id,
            invite
                .host
                .relay_urls()
                .cloned()
                .map(iroh::TransportAddr::Relay),
        );
        let t = std::time::Instant::now();
        let a = Session::join(&Identity::generate(), &only_relay, "Ana", Relays::Iroh)
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
