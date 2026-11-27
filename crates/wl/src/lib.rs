//! A Wayland client written from scratch: the wire protocol over the compositor's Unix
//! socket (with file descriptors passed alongside), object ids, and typed requests and
//! events generated from the protocol files in `protocols/` (see build.rs). No libwayland.
//!
//! ```no_run
//! use pencilpilot_wl::{Connection, wl_registry};
//! let (mut c, display) = Connection::connect()?;
//! let registry = display.get_registry(&mut c)?;
//! c.roundtrip(display)?;
//! while let Some(ev) = c.next_event() {
//!     if let Ok(wl_registry::Event::Global { interface, version, .. }) = ev.parse::<wl_registry::Event>() {
//!         println!("{interface} v{version}");
//!     }
//! }
//! # Ok::<(), pencilpilot_wl::Error>(())
//! ```

#![cfg(target_os = "linux")]

mod socket;
mod wire;

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub use wire::Arg;

/// The protocols: a module per interface (`wl_surface`, `xdg_toplevel`, `zwp_tablet_tool_v2`…).
#[allow(clippy::all)]
mod protocols {
    include!(concat!(env!("OUT_DIR"), "/protocols.rs"));
}
pub use protocols::*;

/// An object's id on the connection.
pub type Id = u32;

/// What an interface's messages carry, for reading and writing them.
#[derive(Debug)]
pub struct Interface {
    pub name: &'static str,
    pub version: u32,
    pub requests: &'static [MessageDesc],
    pub events: &'static [MessageDesc],
}

#[derive(Debug)]
pub struct MessageDesc {
    pub name: &'static str,
    pub since: u32,
    pub destructor: bool,
    pub args: &'static [ArgKind],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Int,
    Uint,
    Fixed,
    Str,
    /// an object (of this interface, when the protocol says)
    Object(Option<&'static str>),
    /// a new object (of this interface; none: the request says which, as wl_registry.bind)
    NewId(Option<&'static str>),
    Array,
    Fd,
}

/// A typed handle to an object (generated for each interface).
pub trait Proxy: Copy {
    const INTERFACE: &'static Interface;
    fn from_id(id: Id) -> Self;
    fn id(&self) -> Id;
}

/// Something that reads an event's arguments (each interface's generated `Event`).
pub trait EventSet: Sized {
    const INTERFACE: &'static Interface;
    fn parse(opcode: u16, args: Vec<Arg>) -> Result<Self>;
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// a message that doesn't fit the protocol
    Protocol(String),
    /// the compositor reported an error (the connection is over)
    Server {
        object: Id,
        interface: String,
        code: u32,
        message: String,
    },
    /// the compositor closed the connection
    Closed,
    /// no WAYLAND_DISPLAY (or XDG_RUNTIME_DIR) to connect to
    NoDisplay,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Protocol(m) => write!(f, "protocol: {m}"),
            Error::Server {
                object,
                interface,
                code,
                message,
            } => {
                write!(
                    f,
                    "the compositor: {interface}@{object} error {code}: {message}"
                )
            }
            Error::Closed => write!(f, "the compositor closed the connection"),
            Error::NoDisplay => write!(f, "no Wayland display (WAYLAND_DISPLAY isn't set)"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// An event from the compositor, its arguments read.
#[derive(Debug)]
pub struct RawEvent {
    pub object: Id,
    pub interface: &'static Interface,
    pub opcode: u16,
    pub args: Vec<Arg>,
}

impl RawEvent {
    /// Its typed form, when it's from an object of that interface.
    pub fn parse<E: EventSet>(self) -> Result<E> {
        if !std::ptr::eq(self.interface, E::INTERFACE) {
            return Err(Error::Protocol(format!(
                "a {} event read as {}",
                self.interface.name,
                E::INTERFACE.name
            )));
        }
        E::parse(self.opcode, self.args)
    }

    /// Whether it's from this object.
    pub fn is(&self, p: impl Proxy) -> bool {
        self.object == p.id()
    }
}

struct Object {
    interface: &'static Interface,
    version: u32,
    /// destroyed here, waiting for the compositor's delete_id (its id isn't free yet)
    gone: bool,
}

const DISPLAY: Id = 1;
const MAX_CLIENT_ID: Id = 0xFEFF_FFFF;

/// A connection to the compositor.
pub struct Connection {
    sock: UnixStream,
    objects: HashMap<Id, Object>,
    next_id: Id,
    free_ids: Vec<Id>,
    out: Vec<u8>,
    out_fds: Vec<OwnedFd>,
    inbuf: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
    events: VecDeque<RawEvent>,
    /// syncs being waited on (roundtrip), and those done
    pending_syncs: Vec<Id>,
    syncs_done: Vec<Id>,
}

impl Connection {
    /// Connect to the compositor in `WAYLAND_DISPLAY` (a name in XDG_RUNTIME_DIR, or a path).
    pub fn connect() -> Result<(Connection, wl_display::WlDisplay)> {
        let name = std::env::var_os("WAYLAND_DISPLAY").ok_or(Error::NoDisplay)?;
        let path = PathBuf::from(&name);
        let path = if path.is_absolute() {
            path
        } else {
            PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::NoDisplay)?).join(path)
        };
        Ok(Self::from_stream(UnixStream::connect(path)?))
    }

    /// A connection over a socket already open (tests use a socket pair).
    pub fn from_stream(sock: UnixStream) -> (Connection, wl_display::WlDisplay) {
        let mut objects = HashMap::new();
        objects.insert(
            DISPLAY,
            Object {
                interface: &wl_display::INTERFACE,
                version: 1,
                gone: false,
            },
        );
        let c = Connection {
            sock,
            objects,
            next_id: 2,
            free_ids: Vec::new(),
            out: Vec::with_capacity(4096),
            out_fds: Vec::new(),
            inbuf: Vec::with_capacity(4096),
            in_fds: VecDeque::new(),
            events: VecDeque::new(),
            pending_syncs: Vec::new(),
            syncs_done: Vec::new(),
        };
        (c, wl_display::WlDisplay(DISPLAY))
    }

    /// The socket, to wait on with poll() alongside other things.
    pub fn fd(&self) -> BorrowedFd<'_> {
        use std::os::fd::AsFd;
        self.sock.as_fd()
    }

    /// A new object's id, for a request that makes one (the generated code calls this).
    pub fn new_object(&mut self, interface: &'static Interface, version: u32) -> Id {
        let id = self.free_ids.pop().unwrap_or_else(|| {
            let id = self.next_id;
            assert!(id <= MAX_CLIENT_ID, "out of object ids");
            self.next_id += 1;
            id
        });
        self.objects.insert(
            id,
            Object {
                interface,
                version,
                gone: false,
            },
        );
        id
    }

    /// The version an object was made with (the objects it makes get the same).
    pub fn version_of(&self, id: Id) -> u32 {
        self.objects.get(&id).map_or(1, |o| o.version)
    }

    /// An object destroyed here: its events are dropped until the compositor frees its id.
    pub fn forget(&mut self, id: Id) {
        if let Some(o) = self.objects.get_mut(&id) {
            o.gone = true;
        }
    }

    /// Queue a request (the generated methods call this); sent at the next flush, or now if
    /// the buffer is full or it carries file descriptors.
    pub fn send(&mut self, object: Id, opcode: u16, args: Vec<Arg>) -> Result<()> {
        let o = self
            .objects
            .get(&object)
            .ok_or_else(|| Error::Protocol(format!("a request on unknown object {object}")))?;
        let desc = o.interface.requests.get(opcode as usize).ok_or_else(|| {
            Error::Protocol(format!("{} has no request {opcode}", o.interface.name))
        })?;
        if desc.since > o.version {
            return Err(Error::Protocol(format!(
                "{}.{} needs version {}, the object is version {}",
                o.interface.name, desc.name, desc.since, o.version
            )));
        }
        let mut fds = Vec::new();
        wire::write_message(&mut self.out, object, opcode, args, &mut fds)?;
        let has_fds = !fds.is_empty();
        self.out_fds.extend(fds);
        if has_fds || self.out.len() > 3072 {
            self.flush()?;
        }
        Ok(())
    }

    /// Send what's queued.
    pub fn flush(&mut self) -> Result<()> {
        while !self.out.is_empty() {
            // the compositor takes at most 28 descriptors a message (as libwayland)
            let n = self.out_fds.len().min(28);
            let fds: Vec<i32> = self.out_fds[..n].iter().map(|f| f.as_raw_fd()).collect();
            let sent = socket::send(self.sock.as_raw_fd(), &self.out, &fds)?;
            self.out_fds.drain(..n);
            self.out.drain(..sent);
        }
        Ok(())
    }

    /// Read what the compositor sent (waiting for something if `block`) and turn whole
    /// messages into events. False when nothing was there (not blocking).
    pub fn read(&mut self, block: bool) -> Result<bool> {
        self.flush()?;
        let mut buf = [0u8; 16384];
        let (n, fds) = socket::recv(self.sock.as_raw_fd(), &mut buf, block)?;
        match n {
            None => return Ok(false),
            Some(0) => return Err(Error::Closed),
            Some(n) => self.inbuf.extend_from_slice(&buf[..n]),
        }
        self.in_fds.extend(fds);
        self.decode()?;
        Ok(true)
    }

    fn decode(&mut self) -> Result<()> {
        let mut at = 0;
        while self.inbuf.len() - at >= 8 {
            let object = u32::from_ne_bytes(self.inbuf[at..at + 4].try_into().unwrap());
            let word = u32::from_ne_bytes(self.inbuf[at + 4..at + 8].try_into().unwrap());
            let (size, opcode) = ((word >> 16) as usize, (word & 0xffff) as u16);
            if size < 8 || size % 4 != 0 {
                return Err(Error::Protocol(format!("a message of {size} bytes")));
            }
            if self.inbuf.len() - at < size {
                break;
            }
            let obj = self
                .objects
                .get(&object)
                .ok_or_else(|| Error::Protocol(format!("an event for unknown object {object}")))?;
            let (interface, version, gone) = (obj.interface, obj.version, obj.gone);
            let desc = interface.events.get(opcode as usize).ok_or_else(|| {
                Error::Protocol(format!("{} has no event {opcode}", interface.name))
            })?;
            let need_fds = desc.args.iter().filter(|k| **k == ArgKind::Fd).count();
            if self.in_fds.len() < need_fds {
                break; // its descriptors haven't arrived yet
            }
            let body = &self.inbuf[at + 8..at + size];
            let args = wire::read_args(body, desc.args, &mut self.in_fds)?;
            at += size;
            // objects the compositor makes: known from here on
            for (k, a) in desc.args.iter().zip(&args) {
                if let (ArgKind::NewId(Some(name)), Arg::NewId(id)) = (k, a) {
                    let i = ALL.iter().find(|i| i.name == *name).ok_or_else(|| {
                        Error::Protocol(format!("an object of unknown interface {name}"))
                    })?;
                    self.objects.insert(
                        *id,
                        Object {
                            interface: i,
                            version,
                            gone: false,
                        },
                    );
                }
            }
            if object == DISPLAY {
                self.display_event(opcode, args)?;
            } else if interface.name == "wl_callback" && opcode == 0 && !gone {
                // done ends a callback (a sync's or a frame's); its id is free at delete_id
                self.forget(object);
                if self.pending_syncs.contains(&object) {
                    self.syncs_done.push(object);
                } else {
                    self.events.push_back(RawEvent {
                        object,
                        interface,
                        opcode,
                        args,
                    });
                }
            } else if !gone {
                self.events.push_back(RawEvent {
                    object,
                    interface,
                    opcode,
                    args,
                });
            }
        }
        self.inbuf.drain(..at);
        Ok(())
    }

    fn display_event(&mut self, opcode: u16, args: Vec<Arg>) -> Result<()> {
        match wl_display::Event::parse(opcode, args)? {
            wl_display::Event::Error {
                object_id,
                code,
                message,
            } => {
                let object = object_id.unwrap_or(0);
                let interface = self
                    .objects
                    .get(&object)
                    .map_or("unknown", |o| o.interface.name)
                    .to_string();
                Err(Error::Server {
                    object,
                    interface,
                    code,
                    message,
                })
            }
            wl_display::Event::DeleteId { id } => {
                if self.objects.remove(&id).is_some() {
                    self.free_ids.push(id);
                }
                Ok(())
            }
        }
    }

    /// The next event read, if any (not waiting).
    pub fn next_event(&mut self) -> Option<RawEvent> {
        self.events.pop_front()
    }

    /// The next event, waiting for one.
    pub fn wait_event(&mut self) -> Result<RawEvent> {
        loop {
            if let Some(e) = self.events.pop_front() {
                return Ok(e);
            }
            self.read(true)?;
        }
    }

    /// Wait until the compositor has handled everything sent so far (wl_display.sync).
    /// Events that arrive meanwhile stay queued.
    pub fn roundtrip(&mut self, display: wl_display::WlDisplay) -> Result<()> {
        let cb = display.sync(self)?;
        self.pending_syncs.push(cb.0);
        self.flush()?;
        while !self.syncs_done.contains(&cb.0) {
            self.read(true)?;
        }
        self.syncs_done.retain(|&i| i != cb.0);
        self.pending_syncs.retain(|&i| i != cb.0);
        Ok(())
    }
}
