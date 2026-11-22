//! Session passes: the website's signed word that a computer belongs to a PencilPilot
//! account (ADR-0013, "Sign-in and passes").
//!
//! Drawing together needs a PencilPilot account. Before hosting or joining, the app asks
//! the website for a pass: a short statement {account id, display name, this computer's
//! member id, issued, expires}, signed with the site's ed25519 key. Members check each
//! other's passes with the site's public key, built into the app: the host refuses a joiner
//! without a good one, and a joiner refuses a host without one. Names in a session come
//! from passes, so no one can pose as someone else.
//!
//! On the wire a pass is `{ "body": <JSON text>, "sig": <hex> }`; the signature covers
//! `"pencilpilot-pass v1\n"` followed by the body, byte for byte (the website signs exactly
//! that, see apps/web/src/lib/server/pass.ts). No email address is ever in a pass.

use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, err};

/// The website's public key (passes are checked with it).
pub type SiteKey = PublicKey;

const CONTEXT: &[u8] = b"pencilpilot-pass v1\n";

/// A pass as it travels: the statement exactly as signed, and the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pass {
    pub body: String,
    /// hex
    pub sig: String,
}

/// What a pass says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassInfo {
    /// the account's id on the website
    pub user: String,
    /// the account's display name
    pub name: String,
    /// the computer's member id ([`crate::Identity::id`])
    pub endpoint: String,
    /// seconds since 1970
    pub issued: u64,
    pub expires: u64,
}

impl Pass {
    /// A pass signed with `key` (the website does this; tests and tools too).
    pub fn sign(key: &SecretKey, info: &PassInfo) -> Self {
        #[derive(Serialize)]
        struct Body<'a> {
            v: u8,
            #[serde(flatten)]
            info: &'a PassInfo,
        }
        let body = serde_json::to_string(&Body { v: 1, info }).expect("a pass is plain JSON");
        let sig = key.sign(&message(&body));
        Self {
            body,
            sig: data_encoding::HEXLOWER.encode(&sig.to_bytes()),
        }
    }

    /// What the pass says, if the site signed it (expiry and computer not checked).
    pub fn read(&self, site: &SiteKey) -> Result<PassInfo> {
        let bad = || Error("That sign-in pass isn't genuine.".into());
        let bytes = data_encoding::HEXLOWER
            .decode(self.sig.as_bytes())
            .map_err(|_| bad())?;
        let bytes = <[u8; 64]>::try_from(bytes.as_slice()).map_err(|_| bad())?;
        site.verify(&message(&self.body), &Signature::from_bytes(&bytes))
            .map_err(|_| bad())?;
        #[derive(Deserialize)]
        struct Body {
            v: u8,
            #[serde(flatten)]
            info: PassInfo,
        }
        let b: Body = serde_json::from_str(&self.body).map_err(|_| bad())?;
        if b.v != 1 {
            return Err(Error(
                "That sign-in pass is from a newer PencilPilot: update the app.".into(),
            ));
        }
        Ok(b.info)
    }

    /// What the pass says, if the site signed it for the computer `endpoint` and it's still
    /// good at `now` (seconds since 1970).
    pub fn check(&self, site: &SiteKey, endpoint: &str, now: u64) -> Result<PassInfo> {
        let info = self.check_owner(site, endpoint)?;
        if now >= info.expires {
            return Err(Error("That sign-in pass has expired.".into()));
        }
        Ok(info)
    }

    /// As [`Pass::check`], but an expired pass is fine: for someone already in the session
    /// (a pass is needed to join, not to stay).
    pub fn check_owner(&self, site: &SiteKey, endpoint: &str) -> Result<PassInfo> {
        let info = self.read(site)?;
        if info.endpoint != endpoint {
            return Err(Error(
                "That sign-in pass belongs to another computer.".into(),
            ));
        }
        Ok(info)
    }
}

fn message(body: &str) -> Vec<u8> {
    let mut m = CONTEXT.to_vec();
    m.extend_from_slice(body.as_bytes());
    m
}

/// The site's public key from its text form (64 hex characters).
pub fn site_key(hex: &str) -> Result<SiteKey> {
    let bytes = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.trim().as_bytes())
        .map_err(|_| Error("The website's key isn't valid.".into()))?;
    let bytes = <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| Error("The website's key isn't valid.".into()))?;
    PublicKey::from_bytes(&bytes).map_err(err)
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The test site's key pair (fixed, so the tests read the same every run).
    pub fn test_site() -> SecretKey {
        SecretKey::from_bytes(&[7; 32])
    }

    pub fn info(endpoint: &str, name: &str, expires: u64) -> PassInfo {
        PassInfo {
            user: format!("u_{}", name.to_lowercase()),
            name: name.into(),
            endpoint: endpoint.into(),
            issued: 1_000,
            expires,
        }
    }

    #[test]
    fn a_good_pass_is_accepted() {
        let site = test_site();
        let me = SecretKey::generate().public().to_string();
        let p = Pass::sign(&site, &info(&me, "Ana", 2_000));
        let got = p.check(&site.public(), &me, 1_500).unwrap();
        assert_eq!((got.name.as_str(), got.user.as_str()), ("Ana", "u_ana"));
        assert!(p.body.starts_with(r#"{"v":1,"#), "{}", p.body);
        // the key as text, as the app has it built in
        let hex = data_encoding::HEXLOWER.encode(site.public().as_bytes());
        assert_eq!(site_key(&hex).unwrap(), site.public());
        assert!(site_key("nope").is_err());
    }

    #[test]
    fn a_forged_pass_is_refused() {
        let site = test_site();
        let me = SecretKey::generate().public().to_string();
        // signed by someone else
        let fake = Pass::sign(&SecretKey::generate(), &info(&me, "Ana", 2_000));
        let e = fake.check(&site.public(), &me, 1_500).unwrap_err();
        assert!(e.0.contains("genuine"), "{e}");
        // the statement changed after signing
        let mut p = Pass::sign(&site, &info(&me, "Ana", 2_000));
        p.body = p.body.replace("Ana", "Bob");
        assert!(p.check(&site.public(), &me, 1_500).is_err());
        // not a signature at all
        let mut p = Pass::sign(&site, &info(&me, "Ana", 2_000));
        p.sig = "zz".into();
        assert!(p.read(&site.public()).is_err());
    }

    #[test]
    fn an_expired_pass_is_refused_for_joining() {
        let site = test_site();
        let me = SecretKey::generate().public().to_string();
        let p = Pass::sign(&site, &info(&me, "Ana", 2_000));
        let e = p.check(&site.public(), &me, 2_000).unwrap_err();
        assert!(e.0.contains("expired"), "{e}");
        // but it still says who someone already in the session is
        assert_eq!(p.check_owner(&site.public(), &me).unwrap().name, "Ana");
    }

    #[test]
    fn a_pass_for_another_computer_is_refused() {
        let site = test_site();
        let me = SecretKey::generate().public().to_string();
        let other = SecretKey::generate().public().to_string();
        let p = Pass::sign(&site, &info(&other, "Ana", 2_000));
        let e = p.check(&site.public(), &me, 1_500).unwrap_err();
        assert!(e.0.contains("another computer"), "{e}");
        assert!(p.check_owner(&site.public(), &me).is_err());
    }
}
