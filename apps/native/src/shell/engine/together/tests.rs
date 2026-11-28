//! Two apps drawing together in one process, on loopback (no relay): hosting, joining,
//! the project arriving, strokes going both ways, access, leaving.

use std::time::{Duration, Instant};

use super::net::Start;
use super::ops;
use crate::shell::engine::{Engine, Phase, Sample};
use crate::state::{Action, State, Tool, Workspace};

struct App {
    e: Engine,
    s: State,
}

impl App {
    fn new(name: &str) -> App {
        let mut s = State::demo();
        s.workspace = Workspace::Draw;
        s.together.name = name.into();
        let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
        e.together.offline = true;
        e.together.stats_on = true;
        e.together.identity = Some(pencilpilot_share::Identity::generate());
        e.tick(&mut s, Instant::now());
        App { e, s }
    }

    fn tick(&mut self) {
        let acts = std::mem::take(&mut self.s.actions);
        for a in acts {
            self.e.apply(&mut self.s, &a);
        }
        self.e.tick(&mut self.s, Instant::now());
    }

    fn picture(&mut self) -> Vec<u8> {
        crate::shell::engine::frames::catch_up(&mut self.e, &self.s);
        self.e.painter.flush();
        self.e.painter.rgba().to_vec()
    }

    fn stroke(&mut self, y: f32) {
        self.s.tool = Tool::Brush;
        let at = |x: f32, ms: u32| Sample {
            x,
            y,
            pressure: 0.8,
            tilt: (0.0, 0.0),
            ms,
        };
        self.e.pointer(&mut self.s, Phase::Down, at(300.0, 0));
        for i in 1..80 {
            self.e
                .pointer(&mut self.s, Phase::Move, at(300.0 + i as f32 * 8.0, i * 4));
        }
        self.e.pointer(&mut self.s, Phase::Up, at(940.0, 320));
    }
}

/// Pixels that differ between two pictures (by more than 3 a channel), and where.
fn differ(a: &[u8], b: &[u8], w: usize) -> (usize, Option<(usize, usize)>) {
    let mut n = 0;
    let mut first = None;
    for (i, (p, q)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
        if p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > 3) {
            n += 1;
            first.get_or_insert((i % w, i / w));
        }
    }
    (n, first)
}

/// The two pictures become the same (the edits still on their way arrive) within 10 s.
fn same(a: &mut App, b: &mut App, what: &str) {
    let t = Instant::now();
    loop {
        let w = a.e.painter.width() as usize;
        let (pa, pb) = (a.picture(), b.picture());
        assert_eq!(pa.len(), pb.len(), "{what}: sizes");
        let (n, first) = differ(&pa, &pb, w);
        if n == 0 {
            return;
        }
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "{what}: {n} pixels differ, the first at {first:?}"
        );
        a.tick();
        b.tick();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Ticks the apps until `done` (or panics after `secs`).
fn until(apps: &mut [&mut App], secs: u64, what: &str, done: impl Fn(&[&mut App]) -> bool) {
    let t = Instant::now();
    while !done(apps) {
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out: {what}");
        for a in apps.iter_mut() {
            a.tick();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn two_apps_draw_together() {
    let mut host = App::new("Ana");
    host.e.together.auto = Some(Start::Host);
    until(&mut [&mut host], 20, "hosting", |a| {
        a[0].s.together.info.is_some()
    });
    let code = host.s.together.info.as_ref().unwrap().invite.clone();
    assert!(code.starts_with("pp1-"));

    let mut guest = App::new("Ben");
    guest.e.together.auto = Some(Start::Join(code));
    until(&mut [&mut host, &mut guest], 30, "joining", |a| {
        a[1].s.together.synced && a[0].e.together.shared.len() == 5
    });
    // the project arrived: the host's four layers and the guest's own, the same pictures
    assert_eq!(guest.e.together.shared.len(), 5);
    assert_eq!(
        guest.s.frame, host.s.frame,
        "the guest starts on the host's frame"
    );
    same(&mut host, &mut guest, "the same picture in both");
    // the guest may draw on its own layer only; the host's show locked there
    let locked: Vec<bool> = guest.s.layers.iter().map(|l| l.locked).collect();
    assert_eq!(
        locked,
        vec![false, true, true, true, true],
        "{:?}",
        guest.s.layers
    );
    assert_eq!(guest.s.active_layer, 0, "its own layer is the one picked");

    // a stroke on the guest's layer reaches the host (live, then its tiles)
    guest.stroke(700.0);
    until(
        &mut [&mut host, &mut guest],
        20,
        "the guest's stroke",
        |a| {
            a[0].e
                .together
                .stats
                .edits
                .iter()
                .any(|x| x.kind == "tiles")
        },
    );
    assert!(
        host.e.together.stats.edits.iter().any(|x| x.kind == "live"),
        "it was seen as it was drawn"
    );
    same(&mut host, &mut guest, "the guest's stroke is on the host");

    // the host's stroke reaches the guest
    host.stroke(500.0);
    until(&mut [&mut host, &mut guest], 20, "the host's stroke", |a| {
        a[1].e
            .together
            .stats
            .edits
            .iter()
            .any(|x| x.kind == "tiles")
    });
    same(&mut host, &mut guest, "the host's stroke is on the guest");

    // asking to draw on the host's layer: once let, it's unlocked there
    let clean = guest
        .s
        .layers
        .iter()
        .position(|l| l.name == "Clean")
        .unwrap();
    let lid = guest.s.together.access[clean].as_ref().unwrap().lid.clone();
    guest
        .s
        .actions
        .push(Action::Other(format!("{}{lid}", ops::ASK)));
    until(&mut [&mut host, &mut guest], 20, "the ask", |a| {
        a[0].s.together.rows.iter().any(|r| !r.asks.is_empty())
    });
    let who = guest.e.together.me.clone();
    host.s
        .actions
        .push(Action::Other(format!("{}{lid} {who}", ops::LET)));
    let t = Instant::now();
    while guest.s.layers[clean].locked {
        if t.elapsed() > Duration::from_secs(20) {
            panic!(
                "the answer: the guest may draw {}, the host thinks {}; {:?}; who {who}",
                guest.e.together.shared.can_draw(&lid, &who),
                host.e.together.shared.can_draw(&lid, &who),
                guest.s.layers
            );
        }
        host.tick();
        guest.tick();
        std::thread::sleep(Duration::from_millis(2));
    }

    // chat
    guest.s.together.chat_text = "hello".into();
    guest.s.actions.push(Action::Other(ops::SAY.into()));
    until(&mut [&mut host, &mut guest], 20, "the chat", |a| {
        a[0].s
            .together
            .chat
            .iter()
            .any(|l| l.text == "hello" && l.name == "Ben")
    });

    // the guest leaves: the host sees it go
    guest.s.actions.push(Action::Other(ops::LEAVE.into()));
    until(&mut [&mut host, &mut guest], 20, "leaving", |a| {
        a[0].s.together.info.as_ref().unwrap().members.len() == 1
    });
    assert!(!guest.e.together.live());
    assert!(
        guest.s.layers.iter().all(|l| !l.locked),
        "unlocked after leaving"
    );
}

/// What this app doesn't show (other scenes and cuts, markers, sound clips, notes, the
/// camera, mouth charts, palettes; masks, line strokes and sound files from another app)
/// comes back unchanged from it, and a joiner gets it from it as host: it only changes
/// the tracks it changed.
#[test]
fn what_this_app_doesnt_show_goes_round_unchanged() {
    use serde_json::{Value, json};
    let mut host = App::new("Ana");
    host.e.together.auto = Some(Start::Host);
    until(&mut [&mut host], 20, "hosting", |a| {
        a[0].s.together.info.is_some()
    });
    let code = host.s.together.info.as_ref().unwrap().invite.clone();
    let mut guest = App::new("Ben");
    guest.e.together.auto = Some(Start::Join(code));
    until(&mut [&mut host, &mut guest], 30, "joining", |a| {
        a[1].s.together.synced && a[0].e.together.shared.len() == 5
    });
    // a timeline as the current app makes them: three scenes, cuts, and all it has
    let mut rich = host.e.together.session_tl.clone().unwrap();
    let tracks = rich["scenes"][0]["cuts"][0]["tracks"].clone();
    let first = tracks.as_object().unwrap().keys().next().unwrap().clone();
    let did = tracks[&first][0]["d"].clone();
    rich["scenes"][0]["markers"] = json!([{"frame": 3, "label": "hit"}]);
    rich["scenes"][0]["audio"] =
        json!([{"name": "Wind", "file": "s1-abc.mp3", "start": 12, "gain": 0.8, "muted": false}]);
    let cut0 = &mut rich["scenes"][0]["cuts"][0];
    cut0["camera"] = json!({"x": 10, "y": 20, "width": 1600, "height": 900});
    cut0["action"] = json!({"4": "She turns"});
    cut0["dialogue"] = json!({"6": "Hello"});
    cut0["transition"] = json!("dissolve");
    let more_cut = json!({"name": "Cut 9", "tracks": {first.clone(): [{"d": did, "n": 5}, {"d": "", "n": 2}]}});
    rich["scenes"][0]["cuts"]
        .as_array_mut()
        .unwrap()
        .push(more_cut.clone());
    for n in 2..=3 {
        rich["scenes"].as_array_mut().unwrap().push(json!({
            "name": format!("Scene {n}"),
            "markers": [{"frame": 1, "label": "in"}],
            "cuts": [more_cut.clone(), {"name": "Cut 2", "tracks": {}, "transition": "flash"}]
        }));
    }
    rich["mouths"] = json!([{"lid": first, "shapes": {"A": did}}]);
    rich["stages"] = json!({first.clone(): "clean"});
    rich["palettes"] = json!([{"name": "Ana", "colours": [{"name": "Ink", "hex": "#16161a"}]}]);
    host.e.together.edit(json!({"k": "timeline", "tl": rich}));
    until(&mut [&mut host, &mut guest], 20, "the timeline", |a| {
        a[1].e.together.session_tl.as_ref() == Some(&rich)
    });
    // a mask, line strokes and a sound file, as the current app sends them: kept
    let lid = first.clone();
    let px = super::net::encode_tile(&[255u8; 64 * 64 * 4]);
    host.e
        .together
        .edit(json!({"k": "mask", "lid": lid, "on": true, "set": [[0, 0, px]], "clear": []}));
    host.e.together.edit(
        json!({"k": "sound", "file": "s1-abc.mp3", "name": "Wind", "i": 0, "n": 1, "data": "AAAA"}),
    );
    until(
        &mut [&mut host, &mut guest],
        20,
        "the mask and the sound",
        |a| a[1].e.together.masks.contains_key(&lid) && !a[1].e.together.sounds.is_empty(),
    );

    // the guest draws on its own layer (a new drawing on a blank frame) and holds it longer
    guest.stroke(700.0);
    guest.s.actions.push(Action::Other(
        crate::shell::engine::frames::ops::HOLD_LONGER.into(),
    ));
    let own = guest
        .e
        .together
        .lid_of(guest.e.painter.active_layer())
        .cloned()
        .unwrap();
    until(
        &mut [&mut host, &mut guest],
        20,
        "the guest's timeline",
        |a| {
            a[0].e.together.session_tl.as_ref().is_some_and(|tl| {
                tl["scenes"][0]["cuts"][0]["tracks"][own.as_str()]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|x| x["d"] != json!("")))
            })
        },
    );
    for _ in 0..100 {
        host.tick();
        guest.tick();
        std::thread::sleep(Duration::from_millis(2));
    }
    let back = host.e.together.session_tl.clone().unwrap();
    let without = |mut tl: Value| {
        tl["scenes"][0]["cuts"][0]["tracks"]
            .as_object_mut()
            .unwrap()
            .remove(&own);
        tl
    };
    assert_eq!(
        without(back.clone()),
        without(rich.clone()),
        "only the guest's own track changed"
    );
    let cels = back["scenes"][0]["cuts"][0]["tracks"][own.as_str()].clone();
    assert!(
        cels.as_array()
            .unwrap()
            .iter()
            .any(|c| c["d"] != json!("") && c["n"].as_u64() >= Some(2)),
        "held longer: {cels}"
    );

    // a third joins with the guest hosting (the host leaves: the guest takes over) and gets
    // all of it
    host.s.actions.push(Action::Other(ops::LEAVE.into()));
    until(&mut [&mut host, &mut guest], 30, "the hand-over", |a| {
        a[1].s.together.info.as_ref().is_some_and(|i| i.host)
    });
    let code = guest.s.together.info.as_ref().unwrap().invite.clone();
    let mut third = App::new("Cy");
    third.e.together.auto = Some(Start::Join(code));
    until(
        &mut [&mut guest, &mut third],
        30,
        "the third joining",
        |a| a[1].s.together.synced,
    );
    let t = &third.e.together;
    assert_eq!(
        t.session_tl, guest.e.together.session_tl,
        "the whole timeline"
    );
    assert!(
        t.masks.get(&lid).is_some_and(|m| m.0 && m.1.len() == 1),
        "the mask"
    );
    assert_eq!(t.sounds.len(), 1, "the sound file");
}
