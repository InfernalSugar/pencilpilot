//! Messages on the session's stream: JSON, each preceded by its length (4 bytes, big endian).

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::Member;
use crate::{Error, Result, err};

/// The largest message (a batch of tiles stays well under it).
pub const MAX_FRAME: usize = 16 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// joiner → host, first
    Hello {
        project: String,
        proof: String,
        name: String,
    },
    /// host → joiner: you're in; who is here; the last edit's number
    Welcome {
        members: Vec<Member>,
        seq: u64,
    },
    /// host → joiner: not let in, and why
    Refused {
        reason: String,
    },
    /// member → host: an edit to number and pass on
    Edit {
        body: Value,
    },
    /// host → everyone: edit number `seq`, made by `from`
    Ordered {
        seq: u64,
        from: String,
        body: Value,
    },
    /// host → everyone
    Joined {
        member: Member,
    },
    Left {
        id: String,
    },
}

/// A cursor or other passing state: sent as a datagram, may be lost, never stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Presence {
    /// set by the host from the connection, never trusted from the sender
    #[serde(default)]
    pub from: String,
    pub body: Value,
}

pub async fn write(send: &mut SendStream, frame: &Frame) -> Result<()> {
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
