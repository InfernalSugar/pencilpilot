//! Phase 6 QA: the native app's engine as a member of a live session, without its window,
//! for `tools/native-qa/phase6.py` (many native members at once, access, chat, leaving, and
//! the session's state read back exactly). The engine, the painter and the session are the
//! app's own; only the window and its frames are left out (pen to screen is measured on the
//! windowed app).
//!
//! ```sh
//! PP_QA_PEER_CMDS=<commands file> PP_QA_PEER_LOG=<log> \
//!   cargo test --release -p pencilpilot-native --test phase6_peer_qa -- --include-ignored
//! ```
//!
//! Commands, one JSON per line, read as the file grows (the script appends to it):
//! `{"cmd":"open","path":…}`, `{"cmd":"host","name":…}`, `{"cmd":"join","code":…,"name":…}`,
//! `{"cmd":"stroke","y":…,"x0":…,"x1":…,"layer":<index or null>}`,
//! `{"cmd":"new_layer"}`, `{"cmd":"undo"}`, `{"cmd":"op","op":<an ops string>}`,
//! `{"cmd":"say","text":…}`, `{"cmd":"state"}`, `{"cmd":"save","path":…}`,
//! `{"cmd":"wait","ms":…}`, `{"cmd":"quit"}`.
//! The log: one JSON line per command's answer and per `state`, with `t` (CLOCK_MONOTONIC ns).

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::together::net::Start;
use pencilpilot_native::shell::engine::{Engine, Phase, Sample, files};
use pencilpilot_native::state::{Action, State, Tool, Workspace};
use serde_json::{Value, json};

fn now_ns() -> u64 {
    pencilpilot_window::clock::now_ns()
}

struct Peer {
    e: Engine,
    s: State,
}

impl Peer {
    fn new() -> Peer {
        let mut s = State::demo();
        s.workspace = Workspace::Draw;
        let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
        e.together.offline = true;
        e.together.stats_on = true;
        e.tick(&mut s, Instant::now());
        Peer { e, s }
    }

    fn tick(&mut self) {
        let acts = std::mem::take(&mut self.s.actions);
        for a in acts {
            self.e.apply(&mut self.s, &a);
        }
        self.e.tick(&mut self.s, Instant::now());
    }

    fn run_for(&mut self, d: Duration) {
        let t = Instant::now();
        while t.elapsed() < d {
            self.tick();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn stroke(&mut self, y: f32, x0: f32, x1: f32) -> (u64, u64) {
        self.s.tool = Tool::Brush;
        let at = |x: f32, ms: u32| Sample {
            x,
            y,
            pressure: 0.8,
            tilt: (0.0, 0.0),
            ms,
        };
        let t0 = now_ns();
        self.e.pointer(&mut self.s, Phase::Down, at(x0, 0));
        for i in 1..80u32 {
            self.e.pointer(
                &mut self.s,
                Phase::Move,
                at(x0 + (x1 - x0) * i as f32 / 80.0, i * 4),
            );
            // (points as a pen gives them: a few a frame)
            if i % 4 == 0 {
                self.tick();
            }
        }
        self.e.pointer(&mut self.s, Phase::Up, at(x1, 320));
        (t0, now_ns())
    }

    fn state(&self) -> Value {
        let t = &self.s.together;
        json!({
            "live": self.e.together.live(),
            "synced": t.synced,
            "info": t.info.as_ref().map(|i| json!({
                "invite": i.invite, "me": i.me.id, "host": i.host,
                "members": i.members.iter().map(|m| json!({"id": m.id, "name": m.name})).collect::<Vec<_>>(),
            })),
            "layers": self.s.layers.iter().enumerate().map(|(i, l)| json!({
                "name": l.name, "locked": l.locked, "visible": l.visible,
                "lid": t.access.get(i).and_then(|a| a.as_ref()).map(|a| a.lid.clone()),
            })).collect::<Vec<_>>(),
            "active": self.s.active_layer,
            "rows": t.rows.iter().map(|r| json!({
                "lid": r.lid, "name": r.name, "owner": r.owner, "mine": r.mine,
                "canDraw": r.can_draw, "asked": r.asked,
                "writers": r.writers.iter().map(|w| w.0.clone()).collect::<Vec<_>>(),
                "asks": r.asks.iter().map(|a| json!({"id": a.0, "name": a.1, "text": a.2, "own": a.3, "draw": a.4})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "chat": t.chat.iter().map(|c| json!({"name": c.name, "text": c.text, "mine": c.mine, "system": c.system})).collect::<Vec<_>>(),
            "note": t.note.as_ref().map(|n| n.0.clone()),
            "error": t.error,
            "stats": {
                "edits": self.e.together.stats.edits.len(),
                "tiles": self.e.together.stats.edits.iter().filter(|x| x.kind == "tiles").count(),
                "live": self.e.together.stats.edits.iter().filter(|x| x.kind == "live").count(),
                "tilesSent": self.e.together.stats.tiles_sent,
                "membersPeak": self.e.together.stats.members_peak,
                "deferred": self.e.together.stats.deferred,
            },
        })
    }
}

#[test]
#[ignore = "a member for tools/native-qa/phase6.py: needs PP_QA_PEER_CMDS and PP_QA_PEER_LOG"]
fn native_peer() {
    let (Ok(cmds), Ok(log)) = (
        std::env::var("PP_QA_PEER_CMDS"),
        std::env::var("PP_QA_PEER_LOG"),
    ) else {
        eprintln!("PP_QA_PEER_CMDS and PP_QA_PEER_LOG not set: nothing to do");
        return;
    };
    let mut out = std::io::BufWriter::new(std::fs::File::create(&log).unwrap());
    let mut say = |v: Value| {
        let mut v = v;
        v["t"] = json!(now_ns());
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    };
    let mut p = Peer::new();
    let mut input = std::io::BufReader::new(std::fs::File::open(&cmds).unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap() == 0 {
            // nothing new yet: keep the session going
            p.run_for(Duration::from_millis(4));
            continue;
        }
        if !line.ends_with('\n') {
            // a line still being written: read it again whole
            let back = line.len() as i64;
            std::io::Seek::seek(&mut input, std::io::SeekFrom::Current(-back)).unwrap();
            p.run_for(Duration::from_millis(2));
            continue;
        }
        let c: Value = match serde_json::from_str(line.trim()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let cmd = c["cmd"].as_str().unwrap_or("");
        match cmd {
            "open" => {
                let ok =
                    files::open_path(&mut p.e, &mut p.s, Path::new(c["path"].as_str().unwrap()));
                p.run_for(Duration::from_millis(200));
                say(json!({"cmd": cmd, "ok": ok, "layers": p.s.layers.len()}));
            }
            "host" | "join" => {
                p.s.together.name = c["name"].as_str().unwrap_or("Native").into();
                let t0 = now_ns();
                p.e.together.auto = Some(if cmd == "host" {
                    Start::Host
                } else {
                    Start::Join(c["code"].as_str().unwrap().into())
                });
                let t = Instant::now();
                while t.elapsed() < Duration::from_secs(30)
                    && !(p.s.together.info.is_some() && (cmd == "host" || p.s.together.synced))
                {
                    p.tick();
                    std::thread::sleep(Duration::from_millis(1));
                }
                say(json!({"cmd": cmd, "startNs": t0, "readyNs": now_ns(), "state": p.state()}));
            }
            "stroke" => {
                if let Some(l) = c["layer"].as_u64() {
                    // as the layers panel picks one: the next passes make it the painter's
                    p.s.active_layer = l as usize;
                    p.run_for(Duration::from_millis(60));
                }
                let (t0, t1) = p.stroke(
                    c["y"].as_f64().unwrap_or(400.0) as f32,
                    c["x0"].as_f64().unwrap_or(300.0) as f32,
                    c["x1"].as_f64().unwrap_or(900.0) as f32,
                );
                say(json!({"cmd": cmd, "startNs": t0, "endNs": t1, "active": p.s.active_layer}));
            }
            "new_layer" => {
                p.s.actions.push(Action::NewLayer);
                p.run_for(Duration::from_millis(50));
                say(json!({"cmd": cmd, "layers": p.s.layers.len()}));
            }
            "undo" => {
                p.s.actions.push(Action::Undo);
                p.run_for(Duration::from_millis(50));
                say(json!({"cmd": cmd}));
            }
            "op" => {
                p.s.actions
                    .push(Action::Other(c["op"].as_str().unwrap_or("").into()));
                p.run_for(Duration::from_millis(20));
                say(json!({"cmd": cmd, "op": c["op"]}));
            }
            "say" => {
                p.s.together.chat_text = c["text"].as_str().unwrap_or("").into();
                p.s.actions.push(Action::Other(
                    pencilpilot_native::shell::engine::together::ops::SAY.into(),
                ));
                p.run_for(Duration::from_millis(20));
                say(json!({"cmd": cmd}));
            }
            "state" => say(json!({"cmd": cmd, "state": p.state()})),
            "save" => {
                let ok = files::save_to(&mut p.e, &mut p.s, Path::new(c["path"].as_str().unwrap()));
                files::wait_for_save(&mut p.e, &mut p.s);
                say(json!({"cmd": cmd, "ok": ok}));
            }
            "wait" => {
                p.run_for(Duration::from_millis(c["ms"].as_u64().unwrap_or(100)));
                say(json!({"cmd": cmd}));
            }
            "quit" => {
                say(json!({"cmd": cmd, "state": p.state()}));
                break;
            }
            _ => say(json!({"cmd": cmd, "error": "unknown command"})),
        }
    }
}
