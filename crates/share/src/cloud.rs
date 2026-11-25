//! The cloud: an always-on member that keeps shared projects (ADR-0014).
//!
//! Drawing together is peer to peer: someone must be online for the others to catch up.
//! The cloud fixes that with a member that never leaves. A server process (the *keeper*,
//! `crates/keeper`) joins each shared project's session like a person does, with its own
//! sign-in pass from the website ([`KEEPER_USER`]), and receives every edit. It never draws,
//! never hosts and is never handed the session: live drawing stays between the members'
//! apps. What it adds:
//!
//! - **Catch-up when no one is online:** it keeps the project as the last snapshot a member
//!   sent it plus every edit since, and hands both to a member coming back
//!   ([`Cloud::catch_up`]), who then hosts the session again and lets the keeper in.
//! - **A meeting point:** a member reaching the keeper while others are drawing is passed
//!   on to the session's host, and two members coming back at once are told to meet in one
//!   session instead of each hosting their own copy ([`OpenState::Starting`]).
//! - **A list of the projects you're in**, from any computer ([`Cloud::list`]), and a way to
//!   open one there ([`Cloud::open`]).
//!
//! This module is the protocol between apps and the keeper (its own ALPN, [`CLOUD_ALPN`],
//! on the keeper's endpoint) and the apps' side of it. An app proves who it is with its
//! sign-in pass and a signature by the computer key the pass names (the connection itself
//! uses a throwaway key, so it never clashes with the app's live session).
//!
//! The keeper only ever reads an edit's author and the kind of a snapshot message; what an
//! edit says is kept as received. (No end-to-end encryption yet: see ADR-0014, where the
//! seam for it is described.)

use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, PublicKey, SecretKey, Signature, TransportAddr};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pass::Pass;
use crate::session::Relays;
use crate::{Error, Identity, Result, err, wire};

/// The protocol's name in the handshake.
pub const CLOUD_ALPN: &[u8] = b"pencilpilot/cloud/1";

/// The account id on the keeper's pass. The website's accounts are `u_…`, so no person's
/// pass can say this.
pub const KEEPER_USER: &str = "pp:cloud";

/// How long the keeper has to answer.
const WAIT: Duration = Duration::from_secs(20);

/// An app → the keeper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    /// first: who is asking (their pass, and their computer key's signature: [`proof`])
    Hi { pass: Pass, proof: String },
    /// the shared projects I'm a member of
    List,
    /// keep this project (its invite: the secret, and members in its session now); the
    /// keeper joins the session
    Keep {
        code: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        width: u32,
        #[serde(default)]
        height: u32,
    },
    /// a project's name and picture changed (members only)
    Touch {
        project: String,
        #[serde(default)]
        name: Option<String>,
        /// a small PNG, base64
        #[serde(default)]
        thumb: Option<String>,
    },
    /// I'd like to open this project: how (members only)
    Open { project: String },
    /// the project as kept: the last snapshot, then the edits since (members only)
    CatchUp { project: String },
}

/// The keeper → an app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Reply {
    Welcome {
        user: String,
    },
    Refused {
        reason: String,
    },
    Projects {
        list: Vec<CloudProject>,
    },
    Kept {
        project: String,
    },
    Touched,
    Opened(Opened),
    /// catching up, one message at a time: the snapshot's messages first (as the member
    /// who made it sent them), then each edit since, then [`Reply::Done`]
    Snap {
        body: Value,
    },
    Edit {
        from: String,
        body: Value,
    },
    Done,
}

/// A shared project as the cloud has it (for the home screen's Cloud tab).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudProject {
    /// the project's id (from its secret; the same in every app)
    pub project: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// everyone who has been in it (accounts), oldest first
    pub members: Vec<CloudMember>,
    /// seconds since 1970
    pub changed: u64,
    /// who is drawing in it now (names)
    pub online: Vec<String>,
    /// a small PNG, base64
    #[serde(default)]
    pub thumb: Option<String>,
    /// what it takes in the cloud
    pub bytes: u64,
    /// its account is out of cloud space: newer changes aren't kept
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudMember {
    pub user: String,
    pub name: String,
}

/// How to open a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Opened {
    pub project: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// an invite to its session: the keeper (it passes you on to the host), and the host
    pub code: String,
    pub state: OpenState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenState {
    /// people are drawing: join them (through the invite)
    Live,
    /// someone else is opening it right now: try again in a moment, then join them
    Starting,
    /// no one is in it: catch up from the cloud, host it, then let the keeper in (Keep)
    Idle,
}

/// The project as the cloud kept it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CatchUp {
    /// the snapshot's messages (`snap.begin`, `snap.tiles`…, `snap.end`), if there is one
    pub snap: Vec<Value>,
    /// the edits since, in order: (author, edit)
    pub edits: Vec<(String, Value)>,
}

const PROOF: &[u8] = b"pencilpilot-cloud hi v1\0";

fn proof_message(cloud: EndpointId, conn: EndpointId) -> Vec<u8> {
    let mut m = PROOF.to_vec();
    m.extend_from_slice(cloud.as_bytes());
    m.extend_from_slice(conn.as_bytes());
    m
}

/// The app's signature, with its computer key, over the keeper's id and the connection's
/// (throwaway) id: it holds the key its pass names, and the proof is good on this
/// connection only.
pub fn proof(secret: &SecretKey, cloud: EndpointId, conn: EndpointId) -> String {
    data_encoding::HEXLOWER.encode(&secret.sign(&proof_message(cloud, conn)).to_bytes())
}

/// Whether `member` (a computer's id) signed `proof` for this keeper and connection.
pub fn check_proof(member: &str, proof: &str, cloud: EndpointId, conn: EndpointId) -> bool {
    let Ok(key) = member.parse::<PublicKey>() else {
        return false;
    };
    let Ok(bytes) = data_encoding::HEXLOWER.decode(proof.as_bytes()) else {
        return false;
    };
    let Ok(bytes) = <[u8; 64]>::try_from(bytes.as_slice()) else {
        return false;
    };
    key.verify(&proof_message(cloud, conn), &Signature::from_bytes(&bytes))
        .is_ok()
}

/// The cloud's address as typed in Settings: its id (64 hex characters), then `@` and
/// where it listens (`host:port`), e.g. `3b43…c0d0@cloud.example.com:7843`. Without a
/// place, it's found through `relay`.
pub async fn resolve(text: &str, relay: Option<&str>) -> Result<EndpointAddr> {
    let bad = || Error("That cloud address isn't right: it's the id, then @host:port.".into());
    let text = text.trim();
    let (id, place) = match text.split_once('@') {
        Some((id, place)) => (id, Some(place)),
        None => (text, None),
    };
    let id: EndpointId = id.trim().parse().map_err(|_| bad())?;
    let mut via = Vec::new();
    if let Some(place) = place.map(str::trim).filter(|p| !p.is_empty()) {
        let found = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::lookup_host(place.to_string()),
        )
        .await
        .map_err(|_| Error("Couldn't find the cloud: is this computer online?".into()))?
        .map_err(|_| bad())?;
        via.extend(found.map(TransportAddr::Ip));
    }
    if let Some(r) = relay.map(str::trim).filter(|r| !r.is_empty()) {
        via.push(TransportAddr::Relay(r.parse().map_err(err)?));
    }
    Ok(EndpointAddr::from_parts(id, via))
}

/// A connection to the keeper.
pub struct Cloud {
    endpoint: Endpoint,
    conn: Connection,
    send: SendStream,
    recv: RecvStream,
    /// the account the keeper knows this app as
    pub user: String,
}

impl Cloud {
    /// Connect and say who this is: this computer's key `id` and its sign-in `pass`.
    pub async fn connect(
        cloud: EndpointAddr,
        id: &Identity,
        pass: &Pass,
        relays: Relays,
    ) -> Result<Self> {
        // a throwaway key for the connection (the computer's own may be in a live session)
        let endpoint = crate::session::bind_client(&relays).await?;
        let gone = || Error("Couldn't reach the cloud.".into());
        let target = cloud.id;
        let conn = match tokio::time::timeout(WAIT, endpoint.connect(cloud, CLOUD_ALPN)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                endpoint.close().await;
                return Err(Error(format!("Couldn't reach the cloud: {e}")));
            }
            Err(_) => {
                endpoint.close().await;
                return Err(gone());
            }
        };
        let (mut send, mut recv) = conn.open_bi().await.map_err(err)?;
        let hi = Request::Hi {
            pass: pass.clone(),
            proof: proof(&id.secret(), target, endpoint.id()),
        };
        wire::write(&mut send, &hi).await?;
        let user = match read(&mut recv).await? {
            Reply::Welcome { user } => user,
            Reply::Refused { reason } => {
                endpoint.close().await;
                return Err(Error(reason));
            }
            _ => return Err(Error("The cloud answered something unexpected.".into())),
        };
        Ok(Self {
            endpoint,
            conn,
            send,
            recv,
            user,
        })
    }

    async fn ask(&mut self, r: &Request) -> Result<Reply> {
        wire::write(&mut self.send, r).await?;
        match read(&mut self.recv).await? {
            Reply::Refused { reason } => Err(Error(reason)),
            other => Ok(other),
        }
    }

    pub async fn list(&mut self) -> Result<Vec<CloudProject>> {
        match self.ask(&Request::List).await? {
            Reply::Projects { list } => Ok(list),
            _ => Err(unexpected()),
        }
    }

    /// Have the cloud keep a project (and join its session through `code`): its id.
    pub async fn keep(
        &mut self,
        code: &str,
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<String> {
        let r = Request::Keep {
            code: code.into(),
            name: name.into(),
            width,
            height,
        };
        match self.ask(&r).await? {
            Reply::Kept { project } => Ok(project),
            _ => Err(unexpected()),
        }
    }

    pub async fn touch(
        &mut self,
        project: &str,
        name: Option<String>,
        thumb: Option<String>,
    ) -> Result<()> {
        let r = Request::Touch {
            project: project.into(),
            name,
            thumb,
        };
        match self.ask(&r).await? {
            Reply::Touched => Ok(()),
            _ => Err(unexpected()),
        }
    }

    pub async fn open(&mut self, project: &str) -> Result<Opened> {
        match self
            .ask(&Request::Open {
                project: project.into(),
            })
            .await?
        {
            Reply::Opened(o) => Ok(o),
            _ => Err(unexpected()),
        }
    }

    pub async fn catch_up(&mut self, project: &str) -> Result<CatchUp> {
        let mut out = CatchUp::default();
        let mut next = self
            .ask(&Request::CatchUp {
                project: project.into(),
            })
            .await?;
        loop {
            match next {
                Reply::Snap { body } => out.snap.push(body),
                Reply::Edit { from, body } => out.edits.push((from, body)),
                Reply::Done => return Ok(out),
                _ => return Err(unexpected()),
            }
            next = read(&mut self.recv).await?;
        }
    }

    pub async fn close(mut self) {
        let _ = self.send.finish().ok();
        self.conn.close(0u32.into(), b"bye");
        self.endpoint.close().await;
    }
}

fn unexpected() -> Error {
    Error("The cloud answered something unexpected.".into())
}

async fn read(recv: &mut RecvStream) -> Result<Reply> {
    tokio::time::timeout(WAIT, wire::read_json::<Reply>(recv))
        .await
        .map_err(|_| Error("The cloud didn't answer.".into()))??
        .ok_or_else(|| Error("The cloud closed the connection.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proof_is_good_for_its_keeper_and_connection_only() {
        let me = SecretKey::generate();
        let cloud = SecretKey::generate().public();
        let conn = SecretKey::generate().public();
        let p = proof(&me, cloud, conn);
        let id = me.public().to_string();
        assert!(check_proof(&id, &p, cloud, conn));
        // another keeper, another connection, someone else's key
        assert!(!check_proof(&id, &p, SecretKey::generate().public(), conn));
        assert!(!check_proof(&id, &p, cloud, SecretKey::generate().public()));
        let other = SecretKey::generate().public().to_string();
        assert!(!check_proof(&other, &p, cloud, conn));
        assert!(!check_proof(&id, "zz", cloud, conn));
    }

    #[tokio::test]
    async fn the_address_is_the_id_then_where_it_listens() {
        let id = SecretKey::generate().public();
        let a = resolve(&format!(" {id}@127.0.0.1:7843 "), None)
            .await
            .unwrap();
        assert_eq!(a.id, id);
        assert_eq!(
            a.ip_addrs().next().unwrap().to_string(),
            "127.0.0.1:7843".to_string()
        );
        let b = resolve(&id.to_string(), Some("https://relay.example.com"))
            .await
            .unwrap();
        assert_eq!(b.relay_urls().count(), 1);
        assert!(resolve("nope@127.0.0.1:1", None).await.is_err());
    }

    #[test]
    fn messages_read_back() {
        let r = Reply::Opened(Opened {
            project: "p".into(),
            name: "Walk".into(),
            width: 10,
            height: 20,
            code: "pp1-x".into(),
            state: OpenState::Idle,
        });
        let s = serde_json::to_string(&r).unwrap();
        assert!(
            s.contains(r#""t":"opened""#) && s.contains(r#""state":"idle""#),
            "{s}"
        );
        assert_eq!(serde_json::from_str::<Reply>(&s).unwrap(), r);
    }
}
