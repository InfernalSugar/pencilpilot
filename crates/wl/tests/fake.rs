//! The connection against a scripted compositor on a socket pair (no display needed).

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use pencilpilot_wl::*;

/// A message as the compositor would write it.
fn msg(object: u32, opcode: u16, body: &[u32]) -> Vec<u8> {
    let size = 8 + body.len() * 4;
    let mut v = Vec::new();
    v.extend_from_slice(&object.to_ne_bytes());
    v.extend_from_slice(&(((size as u32) << 16) | opcode as u32).to_ne_bytes());
    for w in body {
        v.extend_from_slice(&w.to_ne_bytes());
    }
    v
}

fn string_words(s: &str) -> Vec<u32> {
    let mut b = s.as_bytes().to_vec();
    b.push(0);
    let len = b.len() as u32;
    b.resize(b.len().div_ceil(4) * 4, 0);
    let mut w = vec![len];
    w.extend(
        b.chunks(4)
            .map(|c| u32::from_ne_bytes(c.try_into().unwrap())),
    );
    w
}

/// Read the client's messages: (object, opcode, body words).
fn read_requests(s: &mut UnixStream, want: usize) -> Vec<(u32, u16, Vec<u32>)> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    while out.len() < want {
        let mut chunk = [0u8; 4096];
        let n = s.read(&mut chunk).unwrap();
        assert!(n > 0, "the client closed");
        buf.extend_from_slice(&chunk[..n]);
        while buf.len() >= 8 {
            let object = u32::from_ne_bytes(buf[0..4].try_into().unwrap());
            let word = u32::from_ne_bytes(buf[4..8].try_into().unwrap());
            let size = (word >> 16) as usize;
            if buf.len() < size {
                break;
            }
            let body = buf[8..size]
                .chunks(4)
                .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
                .collect();
            out.push((object, (word & 0xffff) as u16, body));
            buf.drain(..size);
        }
    }
    out
}

#[test]
fn a_roundtrip_brings_the_globals() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let t = std::thread::spawn(move || {
        // get_registry (new id 2), sync (new id 3)
        let req = read_requests(&mut server, 2);
        assert_eq!((req[0].0, req[0].1), (1, 1));
        assert_eq!((req[1].0, req[1].1), (1, 0));
        let (registry, callback) = (req[0].2[0], req[1].2[0]);
        let mut global = vec![7];
        global.extend(string_words("wl_compositor"));
        global.push(6);
        server.write_all(&msg(registry, 0, &global)).unwrap();
        server.write_all(&msg(callback, 0, &[42])).unwrap(); // done
        server.write_all(&msg(1, 1, &[callback])).unwrap(); // delete_id
        server
    });
    let (mut c, display) = Connection::from_stream(client);
    let registry = display.get_registry(&mut c).unwrap();
    c.roundtrip(display).unwrap();
    let ev = c.next_event().expect("the global");
    assert!(ev.is(registry));
    match ev.parse::<wl_registry::Event>().unwrap() {
        wl_registry::Event::Global {
            name,
            interface,
            version,
        } => {
            assert_eq!((name, interface.as_str(), version), (7, "wl_compositor", 6));
        }
        e => panic!("{e:?}"),
    }
    assert!(
        c.next_event().is_none(),
        "the sync's done isn't an event of its own"
    );
    let _server = t.join().unwrap();
}

#[test]
fn an_id_is_reused_only_after_the_compositor_frees_it() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let (mut c, display) = Connection::from_stream(client);
    let a = display.sync(&mut c).unwrap(); // id 2
    c.flush().unwrap();
    read_requests(&mut server, 1);
    server.write_all(&msg(a.0, 0, &[1])).unwrap(); // done, no delete_id yet
    c.read(true).unwrap();
    let b = display.sync(&mut c).unwrap();
    assert_ne!(a.0, b.0, "id {} reused before delete_id", a.0);
    server.write_all(&msg(1, 1, &[a.0])).unwrap(); // delete_id
    c.read(true).unwrap();
    let d = display.sync(&mut c).unwrap();
    assert_eq!(d.0, a.0, "a freed id is used again");
}

#[test]
fn a_compositor_error_ends_with_its_message() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let (mut c, display) = Connection::from_stream(client);
    let registry = display.get_registry(&mut c).unwrap();
    c.flush().unwrap();
    let mut err = vec![registry.0, 3];
    err.extend(string_words("invalid thing"));
    server.write_all(&msg(1, 0, &err)).unwrap();
    match c.read(true) {
        Err(Error::Server {
            object,
            interface,
            code,
            message,
        }) => {
            assert_eq!(
                (object, interface.as_str(), code, message.as_str()),
                (registry.0, "wl_registry", 3, "invalid thing")
            );
        }
        r => panic!("{r:?}"),
    }
}

#[test]
fn a_closed_socket_says_so() {
    let (client, server) = UnixStream::pair().unwrap();
    let (mut c, _display) = Connection::from_stream(client);
    drop(server);
    assert!(matches!(c.read(true), Err(Error::Closed)));
}
