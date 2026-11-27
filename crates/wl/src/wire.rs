//! The wire format: a message is the object's id, then (size << 16 | opcode), then its
//! arguments, each 32-bit aligned, in the host's byte order. File descriptors travel
//! beside the bytes (see socket.rs), in the order the arguments name them.

use std::collections::VecDeque;
use std::os::fd::OwnedFd;

use crate::{ArgKind, Error, Id, Result};

/// A message argument.
#[derive(Debug)]
pub enum Arg {
    Int(i32),
    Uint(u32),
    /// 24.8 fixed point, as sent
    Fixed(i32),
    Str(Option<String>),
    Object(Option<Id>),
    NewId(Id),
    /// wl_registry.bind's: the interface and version are spelled out
    UntypedNewId {
        interface: String,
        version: u32,
        id: Id,
    },
    Array(Vec<u8>),
    /// a descriptor to send (a duplicate, made when the request is queued)
    FdOut(OwnedFd),
    /// a descriptor received
    Fd(OwnedFd),
}

impl Arg {
    pub fn int(self) -> Option<i32> {
        if let Arg::Int(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn uint(self) -> Option<u32> {
        if let Arg::Uint(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn fixed(self) -> Option<f64> {
        if let Arg::Fixed(v) = self {
            Some(v as f64 / 256.0)
        } else {
            None
        }
    }
    pub fn string(self) -> Option<Option<String>> {
        if let Arg::Str(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn object(self) -> Option<Option<Id>> {
        if let Arg::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn new_id(self) -> Option<Id> {
        if let Arg::NewId(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn array(self) -> Option<Vec<u8>> {
        if let Arg::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn fd(self) -> Option<OwnedFd> {
        if let Arg::Fd(v) = self { Some(v) } else { None }
    }
}

fn pad(n: usize) -> usize {
    (n + 3) & !3
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_ne_bytes());
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8], len_word: u32) {
    put_u32(out, len_word);
    out.extend_from_slice(b);
    out.resize(out.len() + pad(b.len()) - b.len(), 0);
}

/// Append a request to `out`; the descriptors it carries go to `fds`, to send with it.
pub fn write_message(
    out: &mut Vec<u8>,
    object: Id,
    opcode: u16,
    args: Vec<Arg>,
    fds: &mut Vec<OwnedFd>,
) -> Result<()> {
    let start = out.len();
    put_u32(out, object);
    put_u32(out, 0); // size and opcode, below
    for a in args {
        match a {
            Arg::Int(v) => put_u32(out, v as u32),
            Arg::Uint(v) => put_u32(out, v),
            Arg::Fixed(v) => put_u32(out, v as u32),
            Arg::Str(None) => put_u32(out, 0),
            Arg::Str(Some(s)) => {
                if s.as_bytes().contains(&0) {
                    return Err(Error::Protocol("a string with a NUL in it".into()));
                }
                let mut b = s.as_bytes().to_vec();
                b.push(0);
                put_bytes(out, &b, b.len() as u32);
            }
            Arg::Object(v) => put_u32(out, v.unwrap_or(0)),
            Arg::NewId(id) => put_u32(out, id),
            Arg::UntypedNewId {
                interface,
                version,
                id,
            } => {
                let mut b = interface.as_bytes().to_vec();
                b.push(0);
                put_bytes(out, &b, b.len() as u32);
                put_u32(out, version);
                put_u32(out, id);
            }
            Arg::Array(b) => put_bytes(out, &b, b.len() as u32),
            Arg::FdOut(fd) => fds.push(fd),
            Arg::Fd(_) => return Err(Error::Protocol("a received descriptor sent back".into())),
        }
    }
    let size = out.len() - start;
    if size > 0xffff {
        out.truncate(start);
        return Err(Error::Protocol(format!(
            "a message of {size} bytes (at most 65535)"
        )));
    }
    let word = ((size as u32) << 16) | opcode as u32;
    out[start + 4..start + 8].copy_from_slice(&word.to_ne_bytes());
    Ok(())
}

/// Read an event's arguments from its body (after the 8-byte header).
pub fn read_args(body: &[u8], kinds: &[ArgKind], fds: &mut VecDeque<OwnedFd>) -> Result<Vec<Arg>> {
    let mut at = 0;
    let short = || Error::Protocol("a message shorter than its arguments".into());
    let word = |at: &mut usize| -> Result<u32> {
        let b = body.get(*at..*at + 4).ok_or_else(short)?;
        *at += 4;
        Ok(u32::from_ne_bytes(b.try_into().unwrap()))
    };
    let mut out = Vec::with_capacity(kinds.len());
    for k in kinds {
        out.push(match k {
            ArgKind::Int => Arg::Int(word(&mut at)? as i32),
            ArgKind::Uint => Arg::Uint(word(&mut at)?),
            ArgKind::Fixed => Arg::Fixed(word(&mut at)? as i32),
            ArgKind::Str => {
                let len = word(&mut at)? as usize;
                if len == 0 {
                    Arg::Str(None)
                } else {
                    let b = body.get(at..at + len).ok_or_else(short)?;
                    at += pad(len);
                    let b = b.strip_suffix(&[0]).ok_or_else(|| {
                        Error::Protocol("a string without its closing NUL".into())
                    })?;
                    Arg::Str(Some(String::from_utf8_lossy(b).into_owned()))
                }
            }
            ArgKind::Object(_) => {
                let id = word(&mut at)?;
                Arg::Object((id != 0).then_some(id))
            }
            ArgKind::NewId(_) => Arg::NewId(word(&mut at)?),
            ArgKind::Array => {
                let len = word(&mut at)? as usize;
                let b = body.get(at..at + len).ok_or_else(short)?;
                at += pad(len);
                Arg::Array(b.to_vec())
            }
            ArgKind::Fd => Arg::Fd(
                fds.pop_front()
                    .ok_or_else(|| Error::Protocol("a descriptor that didn't come".into()))?,
            ),
        });
    }
    if at != body.len() {
        return Err(Error::Protocol(format!(
            "a message {} bytes longer than its arguments",
            body.len() - at
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_reads_back_as_written() {
        let args = vec![
            Arg::Int(-5),
            Arg::Uint(7),
            Arg::Fixed(-384), // -1.5
            Arg::Str(Some("wl_compositor".into())),
            Arg::Str(None),
            Arg::Object(Some(9)),
            Arg::Object(None),
            Arg::NewId(12),
            Arg::Array(vec![1, 2, 3, 4, 5]),
        ];
        let mut out = Vec::new();
        let mut fds = Vec::new();
        write_message(&mut out, 3, 2, args, &mut fds).unwrap();
        assert_eq!(out.len() % 4, 0);
        let word = u32::from_ne_bytes(out[4..8].try_into().unwrap());
        assert_eq!(word >> 16, out.len() as u32);
        assert_eq!(word & 0xffff, 2);
        let kinds = [
            ArgKind::Int,
            ArgKind::Uint,
            ArgKind::Fixed,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Object(None),
            ArgKind::Object(None),
            ArgKind::NewId(None),
            ArgKind::Array,
        ];
        let back = read_args(&out[8..], &kinds, &mut VecDeque::new()).unwrap();
        let mut it = back.into_iter();
        assert_eq!(it.next().unwrap().int(), Some(-5));
        assert_eq!(it.next().unwrap().uint(), Some(7));
        assert_eq!(it.next().unwrap().fixed(), Some(-1.5));
        assert_eq!(
            it.next().unwrap().string(),
            Some(Some("wl_compositor".into()))
        );
        assert_eq!(it.next().unwrap().string(), Some(None));
        assert_eq!(it.next().unwrap().object(), Some(Some(9)));
        assert_eq!(it.next().unwrap().object(), Some(None));
        assert_eq!(it.next().unwrap().new_id(), Some(12));
        assert_eq!(it.next().unwrap().array(), Some(vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn a_short_or_long_message_is_refused() {
        let mut out = Vec::new();
        write_message(&mut out, 3, 0, vec![Arg::Uint(1)], &mut Vec::new()).unwrap();
        assert!(
            read_args(
                &out[8..],
                &[ArgKind::Uint, ArgKind::Uint],
                &mut VecDeque::new()
            )
            .is_err()
        );
        assert!(read_args(&out[8..], &[], &mut VecDeque::new()).is_err());
    }

    #[test]
    fn a_string_needs_its_nul() {
        // length 3, "abc" with no NUL
        let mut body = Vec::new();
        body.extend_from_slice(&3u32.to_ne_bytes());
        body.extend_from_slice(b"abc\0");
        assert!(read_args(&body, &[ArgKind::Str], &mut VecDeque::new()).is_err());
    }
}
