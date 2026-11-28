//! The monotonic clock (the compositor's presentation times and the pen's timestamps are on
//! it) and waiting on a descriptor until a deadline, to the nanosecond.

use std::os::fd::{AsRawFd, BorrowedFd};

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
    fn ppoll(
        fds: *mut PollFd,
        n: u64,
        timeout: *const Timespec,
        sigmask: *const std::ffi::c_void,
    ) -> i32;
}

const CLOCK_MONOTONIC: i32 = 1;
const POLLIN: i16 = 1;

/// CLOCK_MONOTONIC, ns.
pub fn now_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // SAFETY: into a local
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    ts.sec as u64 * 1_000_000_000 + ts.nsec as u64
}

/// Wait up to `ns` for `fd` to have something to read. True if it does.
pub fn wait_readable(fd: &BorrowedFd<'_>, ns: u64) -> bool {
    let mut p = PollFd {
        fd: fd.as_raw_fd(),
        events: POLLIN,
        revents: 0,
    };
    let t = Timespec {
        sec: (ns / 1_000_000_000) as i64,
        nsec: (ns % 1_000_000_000) as i64,
    };
    loop {
        // SAFETY: one live pollfd, a timeout on the stack, no signal mask change
        let r = unsafe { ppoll(&mut p, 1, &t, std::ptr::null()) };
        if r >= 0 {
            return r > 0 && p.revents & POLLIN != 0;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
    }
}

/// Wait up to `ns` for `a` or `b` to have something to read: (a has, b has).
pub fn wait_readable2(a: &BorrowedFd<'_>, b: &BorrowedFd<'_>, ns: u64) -> (bool, bool) {
    let mut p = [
        PollFd {
            fd: a.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        },
        PollFd {
            fd: b.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        },
    ];
    let t = Timespec {
        sec: (ns / 1_000_000_000).min(i64::MAX as u64) as i64,
        nsec: (ns % 1_000_000_000) as i64,
    };
    // (u64::MAX: no deadline)
    let tp: *const Timespec = if ns == u64::MAX { std::ptr::null() } else { &t };
    loop {
        // SAFETY: two live pollfds, a timeout on the stack (or none), no signal mask change
        let r = unsafe { ppoll(p.as_mut_ptr(), 2, tp, std::ptr::null()) };
        if r >= 0 {
            return (
                r > 0 && p[0].revents & POLLIN != 0,
                r > 0 && p[1].revents & POLLIN != 0,
            );
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return (false, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn waiting_ends_at_the_deadline_or_when_data_comes() {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let t = now_ns();
        assert!(!wait_readable(&b.as_fd(), 3_000_000));
        let waited = now_ns() - t;
        assert!(
            (2_500_000..50_000_000).contains(&waited),
            "waited {waited} ns for 3 ms"
        );
        use std::io::Write;
        (&a).write_all(b"x").unwrap();
        assert!(wait_readable(&b.as_fd(), 1_000_000_000));
    }
}
