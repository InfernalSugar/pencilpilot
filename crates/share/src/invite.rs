//! Invite codes: what a member shares so a friend can join.

use data_encoding::BASE64URL_NOPAD;
use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, err};

const PREFIX: &str = "pp1-";

/// A shared project's invite: which project, how to reach the host, and the project's
/// secret. Anyone holding it can join, like a house key: the host can make a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    /// the project's id (hex)
    pub project: String,
    /// the project's secret (hex); a joiner proves it holds it without sending it
    pub key: String,
    /// the host's id and the addresses (direct and relay) it can be reached at
    pub host: EndpointAddr,
}

impl Invite {
    pub(crate) fn new(host: EndpointAddr) -> Self {
        Self {
            project: hex(&rand::random::<[u8; 16]>()),
            key: hex(&rand::random::<[u8; 32]>()),
            host,
        }
    }

    /// The code to share: one line of letters, digits, `-` and `_`.
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("an invite serializes");
        format!("{PREFIX}{}", BASE64URL_NOPAD.encode(&json))
    }

    /// Read a code (spaces and line breaks from pasting are ignored).
    pub fn decode(code: &str) -> Result<Self> {
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let body = code
            .strip_prefix(PREFIX)
            .ok_or_else(|| Error("That isn't a PencilPilot invite code.".into()))?;
        let bytes = BASE64URL_NOPAD
            .decode(body.as_bytes())
            .map_err(|_| Error("The invite code is damaged: copy it again.".into()))?;
        let inv: Invite = serde_json::from_slice(&bytes)
            .map_err(|_| Error("The invite code is damaged: copy it again.".into()))?;
        inv.secret()?;
        Ok(inv)
    }

    /// What a joiner sends to show it was invited: a keyed hash of who is joining whom,
    /// so a proof overheard on one connection is useless on any other.
    pub(crate) fn proof(&self, joiner: EndpointId, host: EndpointId) -> Result<String> {
        let key = self.secret()?;
        let mut h = blake3::Hasher::new_keyed(&key);
        h.update(b"pencilpilot-share join v1");
        h.update(self.project.as_bytes());
        h.update(joiner.as_bytes());
        h.update(host.as_bytes());
        Ok(h.finalize().to_hex().to_string())
    }

    /// Whether `proof` came from someone holding this invite (constant-time).
    pub(crate) fn check(&self, proof: &str, joiner: EndpointId, host: EndpointId) -> bool {
        let Ok(want) = self.proof(joiner, host) else {
            return false;
        };
        let (Ok(a), Ok(b)) = (blake3::Hash::from_hex(&want), blake3::Hash::from_hex(proof)) else {
            return false;
        };
        a == b
    }

    fn secret(&self) -> Result<[u8; 32]> {
        let bytes = unhex(&self.key).ok_or_else(|| Error("The invite's key is damaged.".into()))?;
        bytes
            .try_into()
            .map_err(|_| Error("The invite's key is damaged.".into()))
    }
}

fn hex(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(bytes)
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    data_encoding::HEXLOWER
        .decode(s.as_bytes())
        .map_err(err)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn round_trips_and_proves_membership() {
        let host = SecretKey::generate().public();
        let joiner = SecretKey::generate().public();
        let inv = Invite::new(EndpointAddr::new(host));
        let code = inv.encode();
        assert!(code.starts_with("pp1-"));
        // pasted with a line break in the middle
        let pasted = format!("{}\n {}", &code[..20], &code[20..]);
        assert_eq!(Invite::decode(&pasted).unwrap(), inv);

        let proof = inv.proof(joiner, host).unwrap();
        assert!(inv.check(&proof, joiner, host));
        // not for another joiner, another host, or another project's key
        let other = SecretKey::generate().public();
        assert!(!inv.check(&proof, other, host));
        assert!(!inv.check(&proof, joiner, other));
        let mut wrong = inv.clone();
        wrong.key = hex(&[7u8; 32]);
        assert!(!wrong.check(&proof, joiner, host));
        assert!(!inv.check("not a proof", joiner, host));

        assert!(Invite::decode("hello").is_err());
        assert!(Invite::decode("pp1-@@@").is_err());
    }
}
