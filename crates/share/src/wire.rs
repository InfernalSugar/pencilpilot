//! Messages on the session's stream: JSON, each preceded by its length (4 bytes, big endian).

use std::collections::HashMap;

use iroh::endpoint::{RecvStream, SendStream};
use iroh::{EndpointAddr, PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use crate::pass::Pass;
use crate::session::Member;
use crate::{Error, Result, err};

/// The largest message (a batch of tiles stays well under it).
pub const MAX_FRAME: usize = 16 << 20;

/// An edit as its author made it: numbered by the author (1, 2, 3…), signed with the
/// author's key, so no one can pass on an edit as someone else's and none is applied twice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Signed {
    pub from: String,
    pub n: u64,
    /// the edit (JSON), exactly as signed
    pub body: String,
    /// hex
    pub sig: String,
}

impl Signed {
    pub fn make(secret: &SecretKey, project: &str, n: u64, body: String) -> Self {
        let sig = secret.sign(&message(project, n, &body));
        Self {
            from: secret.public().to_string(),
            n,
            body,
            sig: data_encoding::HEXLOWER.encode(&sig.to_bytes()),
        }
    }

    /// Whether the author's key signed exactly this.
    pub fn verify(&self, project: &str) -> bool {
        let Ok(key) = self.from.parse::<PublicKey>() else {
            return false;
        };
        let Ok(bytes) = data_encoding::HEXLOWER.decode(self.sig.as_bytes()) else {
            return false;
        };
        let Ok(bytes) = <[u8; 64]>::try_from(bytes.as_slice()) else {
            return false;
        };
        key.verify(
            &message(project, self.n, &self.body),
            &Signature::from_bytes(&bytes),
        )
        .is_ok()
    }
}

fn message(project: &str, n: u64, body: &str) -> Vec<u8> {
    let mut m = b"pencilpilot-share edit v1\0".to_vec();
    m.extend_from_slice(project.as_bytes());
    m.extend_from_slice(&n.to_be_bytes());
    m.extend_from_slice(body.as_bytes());
    m
}

/// A member coming back to a new host after the old one left.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resume {
    /// the host that left (its id)
    pub from_host: String,
    /// per author, the last edit number this member has applied
    pub seen: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// joiner → member, first
    Hello {
        project: String,
        proof: String,
        /// the joiner's sign-in pass: their name and account come from it
        #[serde(default)]
        pass: Option<Pass>,
        /// how others can reach the joiner (for a later hand-over)
        addr: EndpointAddr,
        #[serde(default)]
        resume: Option<Resume>,
    },
    /// host → joiner: you're in; who is here (in the order they came); the last edit's
    /// number; what the host has seen of each author; edits the joiner missed
    Welcome {
        members: Vec<Member>,
        seq: u64,
        seen: HashMap<String, u64>,
        missed: Vec<Ordered>,
    },
    /// a member that isn't the host → joiner: ask the host instead
    Redirect {
        host: EndpointAddr,
    },
    /// host → joiner: not let in, and why
    Refused {
        reason: String,
    },
    /// member → host: an edit (its own, or after a hand-over one the host hadn't seen)
    Edit {
        edit: Signed,
    },
    /// host → everyone: edit number `seq`
    Ordered(Ordered),
    /// one member to another, through the host (which sets `from`): e.g. the project for
    /// someone who just joined. Not numbered, not kept.
    Direct {
        to: String,
        #[serde(default)]
        from: String,
        body: String,
    },
    /// host → everyone
    Joined {
        member: Member,
    },
    Left {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ordered {
    pub seq: u64,
    pub edit: Signed,
}

/// A cursor or other passing state: sent as a datagram, may be lost, never stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Presence {
    /// set by the host from the connection, never trusted from the sender
    #[serde(default)]
    pub from: String,
    pub body: serde_json::Value,
}

pub async fn write<T: Serialize>(send: &mut SendStream, frame: &T) -> Result<()> {
    let bytes = serde_json::to_vec(frame).map_err(err)?;
    if bytes.len() > MAX_FRAME {
        return Err(Error("That change is too big to send in one go.".into()));
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(err)?;
    send.write_all(&bytes).await.map_err(err)?;
    Ok(())
}

/// The next frame, or `None` when the other side finished the stream.
pub async fn read(recv: &mut RecvStream) -> Result<Option<Frame>> {
    read_json(recv).await
}

/// Any message, framed the same way (the cloud's protocol uses this too).
pub async fn read_json<T: serde::de::DeserializeOwned>(recv: &mut RecvStream) -> Result<Option<T>> {
    let mut len = [0u8; 4];
    match recv.read_exact(&mut len).await {
        Ok(()) => {}
        Err(iroh::endpoint::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(err(e)),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(Error("A message was too big.".into()));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.map_err(err)?;
    serde_json::from_slice(&buf).map(Some).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_signed_by_their_author() {
        let k = SecretKey::generate();
        let e = Signed::make(&k, "proj", 3, r#"{"a":1}"#.into());
        assert!(e.verify("proj"));
        assert!(!e.verify("other project"));
        let mut forged = e.clone();
        forged.body = r#"{"a":2}"#.into();
        assert!(!forged.verify("proj"));
        let mut renumbered = e.clone();
        renumbered.n = 4;
        assert!(!renumbered.verify("proj"));
        let mut someone_else = e;
        someone_else.from = SecretKey::generate().public().to_string();
        assert!(!someone_else.verify("proj"));
    }
}
