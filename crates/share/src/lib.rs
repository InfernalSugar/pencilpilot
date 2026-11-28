//! Shared projects, peer to peer (ADR-0013, milestone S12).
//!
//! The apps of a shared project's members connect to each other directly over
//! [iroh](https://iroh.computer) (QUIC, encrypted end to end, hole punching through home
//! routers), with a relay only to introduce them and to carry traffic when a direct
//! connection can't be made. Nobody forwards ports.
//!
//! - [`Identity`]: this computer's key. Its public half is the member's id; changes and
//!   credits are tied to it.
//! - [`Invite`]: the code a member shares: the project, the host's address and the
//!   project's secret, which proves a joiner was invited.
//! - [`Session`]: the live connection. One member's app is the **host**: everyone connects
//!   to it, it numbers every edit and passes it on, so all apps apply edits in the same
//!   order. Cursors (presence) go as datagrams: unreliable, newest wins, never stored.
//!
//! The session carries edits as JSON values; what they mean (strokes, tile changes,
//! timeline edits) is up to the app.

mod identity;
mod invite;
mod session;
mod wire;

pub use identity::Identity;
pub use invite::Invite;
pub use iroh::{EndpointAddr, SecretKey};
pub use session::{Event, Member, Presencer, Relays, Session};
pub use wire::Signed;

/// The protocol's name in the connection handshake. Apps with another version don't connect.
pub const ALPN: &[u8] = b"pencilpilot/share/1";

/// What went wrong, in words the app can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}
