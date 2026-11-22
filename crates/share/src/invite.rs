//! Invite codes: what a member shares so a friend can join.
//!
//! A code is `pp1-` and then, in base64url: a version byte, the project's secret (32 bytes)
//! and one or more members' addresses (who made the code, then the host). Each address is
//! the member's id (32 bytes) and how to reach it: a relay URL, or IP addresses only when
//! running without relays (on one network). Home IP addresses stay out of codes, so a code
//! that travels further than meant doesn't give away where anyone is.
//!
//! The project's id is derived from its secret, so it doesn't need its own place.

use std::net::{IpAddr, SocketAddr};

use data_encoding::{BASE64URL_NOPAD, HEXLOWER};
use iroh::{EndpointAddr, EndpointId, PublicKey, RelayUrl, TransportAddr};

use crate::{Error, Result};

const PREFIX: &str = "pp1-";
const VERSION: u8 = 2;
/// addresses per code at most (the code stays short)
const MAX_ADDRS: usize = 4;

/// A shared project's invite: the project's secret, and members to ask to join (any
/// member passes a joiner on to the host). Anyone holding it can join, like a house key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// the project's secret; a joiner proves it holds it without sending it
    pub key: [u8; 32],
    /// members that can let a joiner in, the code's maker first
    pub addrs: Vec<EndpointAddr>,
}

impl Invite {
    pub(crate) fn new(key: [u8; 32], addrs: Vec<EndpointAddr>) -> Self {
        Self { key, addrs }
    }

    pub(crate) fn random_key() -> [u8; 32] {
        rand::random()
    }

    /// The project's id (hex), the same for every member.
    pub fn project(&self) -> String {
        project_of(&self.key)
    }

    /// The code to share: one line of letters, digits, `-` and `_`.
    pub fn encode(&self) -> String {
        let mut b = vec![VERSION];
        b.extend_from_slice(&self.key);
        let addrs = &self.addrs[..self.addrs.len().min(MAX_ADDRS)];
        b.push(addrs.len() as u8);
        for a in addrs {
            b.extend_from_slice(a.id.as_bytes());
            let relays: Vec<&RelayUrl> = a.relay_urls().collect();
            let ips: Vec<&SocketAddr> = a.ip_addrs().collect();
            // a relay is enough: the apps find a direct way through it
            if let Some(r) = relays.first() {
                let url = r.to_string();
                let url = url.trim_end_matches('/');
                b.push(0);
                b.push(url.len().min(255) as u8);
                b.extend_from_slice(&url.as_bytes()[..url.len().min(255)]);
            } else {
                b.push(1);
                let ips: Vec<_> = ips.into_iter().take(8).collect();
                b.push(ips.len() as u8);
                for ip in ips {
                    match ip.ip() {
                        IpAddr::V4(v4) => {
                            b.push(4);
                            b.extend_from_slice(&v4.octets());
                        }
                        IpAddr::V6(v6) => {
                            b.push(6);
                            b.extend_from_slice(&v6.octets());
                        }
                    }
                    b.extend_from_slice(&ip.port().to_be_bytes());
                }
            }
        }
        format!("{PREFIX}{}", BASE64URL_NOPAD.encode(&b))
    }

    /// Read a code (spaces and line breaks from pasting are ignored).
    pub fn decode(code: &str) -> Result<Self> {
        let damaged = || Error("The invite code is damaged: copy it again.".into());
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let body = code
            .strip_prefix(PREFIX)
            .ok_or_else(|| Error("That isn't a PencilPilot invite code.".into()))?;
        let b = BASE64URL_NOPAD
            .decode(body.as_bytes())
            .map_err(|_| damaged())?;
        let mut r = Reader(&b);
        if r.u8()? != VERSION {
            return Err(Error(
                "That invite is from another version of PencilPilot: update both apps.".into(),
            ));
        }
        let key: [u8; 32] = r.take(32)?.try_into().map_err(|_| damaged())?;
        let n = r.u8()? as usize;
        if n == 0 || n > MAX_ADDRS {
            return Err(damaged());
        }
        let mut addrs = Vec::with_capacity(n);
        for _ in 0..n {
            let id: [u8; 32] = r.take(32)?.try_into().map_err(|_| damaged())?;
            let id = PublicKey::from_bytes(&id).map_err(|_| damaged())?;
            let mut via = Vec::new();
            match r.u8()? {
                0 => {
                    let len = r.u8()? as usize;
                    let url = std::str::from_utf8(r.take(len)?).map_err(|_| damaged())?;
                    let url: RelayUrl = url.parse().map_err(|_| damaged())?;
                    via.push(TransportAddr::Relay(url));
                }
                1 => {
                    for _ in 0..r.u8()? {
                        let ip = match r.u8()? {
                            4 => {
                                let o: [u8; 4] = r.take(4)?.try_into().map_err(|_| damaged())?;
                                IpAddr::from(o)
                            }
                            6 => {
                                let o: [u8; 16] = r.take(16)?.try_into().map_err(|_| damaged())?;
                                IpAddr::from(o)
                            }
                            _ => return Err(damaged()),
                        };
                        let port =
                            u16::from_be_bytes(r.take(2)?.try_into().map_err(|_| damaged())?);
                        via.push(TransportAddr::Ip(SocketAddr::new(ip, port)));
                    }
                }
                _ => return Err(damaged()),
            }
            addrs.push(EndpointAddr::from_parts(id, via));
        }
        if !r.0.is_empty() {
            return Err(damaged());
        }
        Ok(Self { key, addrs })
    }
}

pub(crate) fn project_of(key: &[u8; 32]) -> String {
    let h = blake3::keyed_hash(key, b"pencilpilot-share project id");
    HEXLOWER.encode(&h.as_bytes()[..16])
}

/// What a joiner sends to show it was invited: a keyed hash of who is joining whom, so a
/// proof overheard on one connection is useless on any other.
pub(crate) fn proof(key: &[u8; 32], joiner: EndpointId, host: EndpointId) -> String {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(b"pencilpilot-share join v2");
    h.update(joiner.as_bytes());
    h.update(host.as_bytes());
    h.finalize().to_hex().to_string()
}

/// Whether `given` came from someone holding `key` (constant-time).
pub(crate) fn check(key: &[u8; 32], given: &str, joiner: EndpointId, host: EndpointId) -> bool {
    let want = proof(key, joiner, host);
    match (blake3::Hash::from_hex(&want), blake3::Hash::from_hex(given)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(Error("The invite code is damaged: copy it again.".into()));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn short_codes_that_round_trip_and_prove_membership() {
        let host = SecretKey::generate().public();
        let maker = SecretKey::generate().public();
        let joiner = SecretKey::generate().public();
        let relay: RelayUrl = "https://euc1-1.relay.n0.iroh-canary.iroh.link./"
            .parse()
            .unwrap();
        let lan = "192.168.1.20:5000".parse::<SocketAddr>().unwrap();
        let v6 = "[fe80::1]:5001".parse::<SocketAddr>().unwrap();
        let inv = Invite::new(
            Invite::random_key(),
            vec![
                // a relay and a home address: only the relay goes in the code
                EndpointAddr::from_parts(
                    maker,
                    [TransportAddr::Relay(relay.clone()), TransportAddr::Ip(lan)],
                ),
                EndpointAddr::from_parts(host, [TransportAddr::Ip(lan), TransportAddr::Ip(v6)]),
            ],
        );
        let code = inv.encode();
        assert!(code.starts_with("pp1-"));
        assert!(code.len() < 260, "{} characters", code.len());
        let pasted = format!("{}\n {}", &code[..20], &code[20..]);
        let back = Invite::decode(&pasted).unwrap();
        assert_eq!(back.key, inv.key);
        assert_eq!(back.project(), inv.project());
        assert_eq!(back.addrs[0].id, maker);
        assert_eq!(back.addrs[0].ip_addrs().count(), 0, "no home address");
        assert_eq!(back.addrs[0].relay_urls().count(), 1);
        // without a relay (one network), the IP addresses are what there is
        assert_eq!(back.addrs[1].ip_addrs().count(), 2);

        let p = proof(&back.key, joiner, host);
        assert!(check(&inv.key, &p, joiner, host));
        let other = SecretKey::generate().public();
        assert!(!check(&inv.key, &p, other, host));
        assert!(!check(&inv.key, &p, joiner, other));
        assert!(!check(&[7; 32], &p, joiner, host));
        assert!(!check(&inv.key, "nope", joiner, host));

        assert!(Invite::decode("hello").is_err());
        assert!(Invite::decode("pp1-@@@").is_err());
        assert!(Invite::decode(&code[..code.len() - 4]).is_err());
    }

    #[test]
    fn a_typical_code_is_short() {
        let relay: RelayUrl = "https://relay.pencilpilot.app".parse().unwrap();
        let inv = Invite::new(
            [1; 32],
            vec![EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Relay(relay)],
            )],
        );
        let code = inv.encode();
        println!("{code}");
        assert!(code.len() < 140, "{} characters", code.len());
    }
}
