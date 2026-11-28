//! A scripted member of a shared session, for the Phase 6 QA (tools/native-qa/phase6.py):
//! it hosts or joins like an app does (no relays: same machine only), logs everything it
//! receives with the time it arrived, and sends what it's told. It speaks the session's own
//! protocol (this crate), so to the apps it is just another member; what the edits say is
//! up to the script, which is how it sends kinds an app doesn't know, AI-made content, a
//! snapshot request, chat, cursors and live strokes.
//!
//! cargo run --release -p pencilpilot-share --example qa_peer -- host <name> <log.jsonl>
//! cargo run --release -p pencilpilot-share --example qa_peer -- join <invite> <name> <log.jsonl>
//!
//! stdout: one JSON line `{"ready":…,"invite":…,"me":…,"members":[…]}`, then one line per
//! `members` command. The log: one JSON line per event, `t` its arrival in CLOCK_MONOTONIC
//! nanoseconds.
//!
//! stdin, one JSON per line:
//! `{"cmd":"edit","body":…}`, `{"cmd":"direct","to":"<id>","body":…}`,
//! `{"cmd":"presence","body":…}`, `{"cmd":"members"}`, `{"cmd":"leave"}`.
//! `{"cmd":"edits","bodies":[…],"gap_ms":n}` sends several, `gap_ms` apart.
//! End of input leaves too.

use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use pencilpilot_share::{Event, Identity, Invite, Relays, Session};
use serde_json::{Value, json};

/// CLOCK_MONOTONIC now, in ns (std's `Instant` is that clock on Linux; its debug form
/// shows the raw value, read once and carried forward).
struct Mono {
    at: Instant,
    ns: u128,
}

impl Mono {
    fn new() -> Self {
        let at = Instant::now();
        let s = format!("{at:?}");
        let num = |key: &str| -> u128 {
            let i = s.find(key).map(|i| i + key.len()).unwrap_or(s.len());
            s[i..]
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        };
        Self {
            at,
            ns: num("tv_sec") * 1_000_000_000 + num("tv_nsec"),
        }
    }
    fn now(&self) -> u128 {
        self.ns + self.at.elapsed().as_nanos()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run(args));
}

async fn run(args: Vec<String>) {
    let mono = Mono::new();
    let (s, log_path) = match args.first().map(String::as_str) {
        Some("host") if args.len() == 3 => (
            Session::host(&Identity::generate(), &args[1], Relays::Off).await,
            args[2].clone(),
        ),
        Some("join") if args.len() == 4 => {
            let invite = Invite::decode(&args[1]).expect("invite");
            (
                Session::join(&Identity::generate(), &invite, &args[2], Relays::Off).await,
                args[3].clone(),
            )
        }
        _ => {
            eprintln!("usage: qa_peer host <name> <log> | join <invite> <name> <log>");
            std::process::exit(2);
        }
    };
    let mut s = match s {
        Ok(s) => s,
        Err(e) => {
            println!("{}", json!({"ready": false, "error": e.to_string()}));
            std::process::exit(1);
        }
    };
    let mut log = std::io::BufWriter::new(std::fs::File::create(&log_path).expect("log"));
    let say = |v: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    say(json!({
        "ready": true,
        "t": mono.now() as u64,
        "invite": s.invite().encode(),
        "me": s.me(),
        "host": s.is_host(),
        "members": s.members(),
    }));

    // stdin on a thread of its own (the runtime has no stdin here)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
                Err(e) => eprintln!("qa_peer: bad command: {e}"),
            }
        }
        let _ = tx.send(json!({"cmd": "leave"}));
    });

    let mut sent = 0u64;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                let t = mono.now() as u64;
                match cmd["cmd"].as_str() {
                    Some("edit") => {
                        let r = s.edit(cmd["body"].clone());
                        sent += 1;
                        line(&mut log, json!({"t": t, "ev": "sent", "n": sent, "ok": r.is_ok()}));
                    }
                    Some("edits") => {
                        let gap = Duration::from_millis(cmd["gap_ms"].as_u64().unwrap_or(0));
                        for b in cmd["bodies"].as_array().cloned().unwrap_or_default() {
                            let r = s.edit(b);
                            sent += 1;
                            line(&mut log, json!({"t": mono.now() as u64, "ev": "sent", "n": sent, "ok": r.is_ok()}));
                            if !gap.is_zero() {
                                tokio::time::sleep(gap).await;
                            }
                        }
                    }
                    Some("direct") => {
                        let r = s.direct(cmd["to"].as_str().unwrap_or(""), cmd["body"].clone());
                        line(&mut log, json!({"t": t, "ev": "sent_direct", "ok": r.is_ok(), "err": r.err().map(|e| e.to_string())}));
                    }
                    Some("presence") => s.presence(cmd["body"].clone()),
                    Some("members") => say(json!({
                        "t": t, "members": s.members(), "host": s.is_host(),
                        "invite": s.invite().encode(),
                    })),
                    Some("leave") => break,
                    other => eprintln!("qa_peer: unknown command {other:?}"),
                }
                let _ = log.flush();
            }
            ev = s.next_event() => {
                let t = mono.now() as u64;
                let Some(ev) = ev else { break };
                let closed = matches!(ev, Event::Closed { .. });
                line(&mut log, match ev {
                    Event::Edit { seq, from, body } => json!({"t": t, "ev": "edit", "seq": seq, "from": from, "body": body}),
                    Event::Presence { from, body } => json!({"t": t, "ev": "presence", "from": from, "body": body}),
                    Event::Direct { from, body } => json!({"t": t, "ev": "direct", "from": from, "body": body}),
                    Event::Joined(m) => json!({"t": t, "ev": "joined", "member": m}),
                    Event::Left { id } => json!({"t": t, "ev": "left", "id": id}),
                    Event::HostChanged { host, members } => json!({"t": t, "ev": "host_changed", "host": host, "members": members}),
                    Event::Closed { reason } => json!({"t": t, "ev": "closed", "reason": reason}),
                });
                let _ = log.flush();
                if closed {
                    break;
                }
            }
        }
    }
    let _ = log.flush();
    s.close().await;
}

fn line(w: &mut impl Write, v: Value) {
    let _ = writeln!(w, "{v}");
}
