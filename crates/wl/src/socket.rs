//! Bytes and file descriptors over the Unix socket: sendmsg / recvmsg with SCM_RIGHTS,
//! declared here against the C library (Linux, 64-bit: the layouts below are the kernel's).

use std::ffi::c_void;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

#[repr(C)]
struct IoVec {
    base: *mut c_void,
    len: usize,
}

#[repr(C)]
struct MsgHdr {
    name: *mut c_void,
    namelen: u32,
    iov: *mut IoVec,
    iovlen: usize,
    control: *mut c_void,
    controllen: usize,
    flags: i32,
}

#[repr(C)]
struct CmsgHdr {
    len: usize,
    level: i32,
    kind: i32,
}

const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;
const MSG_DONTWAIT: i32 = 0x40;
const MSG_NOSIGNAL: i32 = 0x4000;
const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;
const MSG_CTRUNC: i32 = 0x8;
const HDR: usize = std::mem::size_of::<CmsgHdr>();
/// at most this many descriptors arrive with one read (libwayland sends at most 28)
const MAX_FDS: usize = 28;

unsafe extern "C" {
    fn sendmsg(fd: i32, msg: *const MsgHdr, flags: i32) -> isize;
    fn recvmsg(fd: i32, msg: *mut MsgHdr, flags: i32) -> isize;
}

const fn align(n: usize) -> usize {
    (n + 7) & !7
}

/// Send `bytes` with `fds` beside them; how many bytes went (the descriptors all go with
/// the first byte).
pub fn send(sock: RawFd, bytes: &[u8], fds: &[RawFd]) -> io::Result<usize> {
    let mut iov = IoVec {
        base: bytes.as_ptr() as *mut c_void,
        len: bytes.len(),
    };
    let mut control = [0u64; align(HDR + MAX_FDS * 4) / 8];
    let mut msg = MsgHdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: std::ptr::null_mut(),
        controllen: 0,
        flags: 0,
    };
    if !fds.is_empty() {
        assert!(fds.len() <= MAX_FDS);
        let data = fds.len() * 4;
        let p = control.as_mut_ptr() as *mut u8;
        // SAFETY: `control` is 8-aligned and big enough for a header and MAX_FDS ints
        unsafe {
            (p as *mut CmsgHdr).write(CmsgHdr {
                len: HDR + data,
                level: SOL_SOCKET,
                kind: SCM_RIGHTS,
            });
            std::ptr::copy_nonoverlapping(fds.as_ptr() as *const u8, p.add(HDR), data);
        }
        msg.control = p as *mut c_void;
        msg.controllen = align(HDR + data);
    }
    loop {
        // SAFETY: msg points at live buffers for the call
        let n = unsafe { sendmsg(sock, &msg, MSG_NOSIGNAL) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Read into `buf`, with any descriptors that came. None: nothing there (not blocking);
/// Some(0): the other end closed.
pub fn recv(sock: RawFd, buf: &mut [u8], block: bool) -> io::Result<(Option<usize>, Vec<OwnedFd>)> {
    let mut iov = IoVec {
        base: buf.as_mut_ptr() as *mut c_void,
        len: buf.len(),
    };
    let mut control = [0u64; align(HDR + MAX_FDS * 4) / 8 * 2];
    let mut msg = MsgHdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: control.as_mut_ptr() as *mut c_void,
        controllen: std::mem::size_of_val(&control),
        flags: 0,
    };
    let flags = MSG_CMSG_CLOEXEC | if block { 0 } else { MSG_DONTWAIT };
    let n = loop {
        // SAFETY: msg points at live buffers for the call
        let n = unsafe { recvmsg(sock, &mut msg, flags) };
        if n >= 0 {
            break n as usize;
        }
        let e = io::Error::last_os_error();
        match e.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok((None, Vec::new())),
            _ => return Err(e),
        }
    };
    let mut fds = Vec::new();
    let mut off = 0;
    let base = control.as_ptr() as *const u8;
    while off + HDR <= msg.controllen {
        // SAFETY: within the control buffer the kernel filled
        let h = unsafe { (base.add(off) as *const CmsgHdr).read() };
        if h.len < HDR || off + h.len > msg.controllen {
            break;
        }
        if h.level == SOL_SOCKET && h.kind == SCM_RIGHTS {
            let count = (h.len - HDR) / 4;
            for k in 0..count {
                // SAFETY: SCM_RIGHTS data is an array of descriptors now ours
                let fd = unsafe { (base.add(off + HDR + k * 4) as *const i32).read_unaligned() };
                fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
        off += align(h.len);
    }
    if msg.flags & MSG_CTRUNC != 0 {
        return Err(io::Error::other(
            "more descriptors came than fit: some were lost",
        ));
    }
    Ok((Some(n), fds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn descriptors_travel_with_the_bytes() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut f = std::env::temp_dir();
        f.push(format!("pp-wl-fd-{}", std::process::id()));
        let mut file = std::fs::File::options()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&f)
            .unwrap();
        std::fs::remove_file(&f).unwrap();
        file.write_all(b"through the socket").unwrap();
        let sent = send(a.as_raw_fd(), b"hello", &[file.as_raw_fd()]).unwrap();
        assert_eq!(sent, 5);
        let mut buf = [0u8; 64];
        let (n, fds) = recv(b.as_raw_fd(), &mut buf, true).unwrap();
        assert_eq!(&buf[..n.unwrap()], b"hello");
        assert_eq!(fds.len(), 1);
        let mut got = std::fs::File::from(fds.into_iter().next().unwrap());
        got.rewind().unwrap();
        let mut s = String::new();
        got.read_to_string(&mut s).unwrap();
        assert_eq!(s, "through the socket");
    }

    #[test]
    fn nothing_there_doesnt_block() {
        let (_a, b) = UnixStream::pair().unwrap();
        let mut buf = [0u8; 8];
        let (n, fds) = recv(b.as_raw_fd(), &mut buf, false).unwrap();
        assert!(n.is_none() && fds.is_empty());
    }
}
