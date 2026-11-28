//! Apps drawing together in one process, on loopback (no relay): hosting, joining, the
//! project arriving, strokes going both ways, access, chat, leaving and coming back; what
//! this app doesn't show going round unchanged.

use std::time::{Duration, Instant};

use super::net::{Cmd, Start};
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

    fn shared(&self) -> usize {
        self.e.together.view.shared.layers.len()
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
            "{what}: {n} pixels differ, the first at {first:?}; frames {} / {}",
            a.s.frame,
            b.s.frame
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

/// Ticks a while (edits on their way arrive).
fn settle(apps: &mut [&mut App], ms: u64) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        for a in apps.iter_mut() {
            a.tick();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn host_and_guest() -> (App, App) {
    let mut host = App::new("Ana");
    host.e.together.auto = Some(Start::Host);
    until(&mut [&mut host], 20, "hosting", |a| {
        a[0].s.together.info.is_some() && a[0].e.together.synced()
    });
    let code = host.s.together.info.as_ref().unwrap().invite.clone();
    assert!(code.starts_with("pp1-"));
    let mut guest = App::new("Ben");
    guest.e.together.auto = Some(Start::Join(code));
    until(&mut [&mut host, &mut guest], 30, "joining", |a| {
        a[1].e.together.synced() && a[0].shared() == 5 && a[1].shared() == 5
    });
    settle(&mut [&mut host, &mut guest], 200);
    (host, guest)
}

#[test]
fn two_apps_draw_together() {
    let (mut host, mut guest) = host_and_guest();
    // the project arrived: the host's four layers and the guest's own, the same pictures
    assert_eq!(
        guest.s.frame, host.s.frame,
        "the guest starts on the host's frame"
    );
    same(&mut host, &mut guest, "the same picture in both");
    // the guest may draw on its own layer only; the host's show locked there
    let own = guest.e.painter.active_layer();
    assert!(
        guest.e.together.may_draw(own),
        "its own layer is the one picked"
    );
    let clean = guest
        .s
        .layers
        .iter()
        .position(|l| l.name == "Clean")
        .unwrap();
    assert!(guest.s.layers[clean].locked, "{:?}", guest.s.layers);

    // a stroke on the guest's layer (a new drawing on a blank frame) reaches the host
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
    same(&mut host, &mut guest, "the host's stroke is on the guest");

    // asking to draw on the host's layer: once let, it's unlocked there
    settle(&mut [&mut host, &mut guest], 200);
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
    loop {
        let clean = guest
            .s
            .layers
            .iter()
            .position(|l| l.name == "Clean")
            .unwrap();
        if !guest.s.layers[clean].locked {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "the answer: the guest may draw: {} (view), writers {:?}; painter {:?}; asks waiting {}",
            guest.e.together.view.shared.can_draw(&lid, &who),
            guest
                .e
                .together
                .view
                .shared
                .layers
                .get(&lid)
                .map(|l| &l.writers),
            guest
                .e
                .painter
                .layers()
                .iter()
                .map(|l| (l.id, &l.props.name, l.props.locked))
                .collect::<Vec<_>>(),
            guest.e.together.asks.len()
        );
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

    // the guest leaves: the host sees it go; the guest can come back
    guest.s.actions.push(Action::Other(ops::LEAVE.into()));
    until(&mut [&mut host, &mut guest], 20, "leaving", |a| {
        a[0].s.together.info.as_ref().unwrap().members.len() == 1
    });
    assert!(!guest.e.together.live());
    assert!(guest.s.together.can_rejoin, "it can come back");
    guest.s.actions.push(Action::Other(ops::REJOIN.into()));
    until(&mut [&mut host, &mut guest], 30, "rejoining", |a| {
        a[1].e.together.synced() && a[0].s.together.info.as_ref().unwrap().members.len() == 2
    });
    same(&mut host, &mut guest, "the same picture after coming back");
}

/// What this app doesn't show (other scenes and cuts, markers, sound clips, notes, the
/// camera, palettes) comes back unchanged from it: it only changes the tracks it changed;
/// and a joiner gets all of it from it as host.
#[test]
fn what_this_app_doesnt_show_goes_round_unchanged() {
    use serde_json::json;
    let (mut host, mut guest) = host_and_guest();
    // a timeline as the current app makes them: three scenes, cuts, and all it has
    let rich = json!({
        "scenes": [
            {"name": "Scene 1", "markers": [{"frame": 3, "label": "hit"}],
             "audio": [{"name": "Wind", "file": "s1-abc.mp3", "start": 12, "gain": 0.8, "muted": false}],
             "cuts": [
                {"name": "Cut 2", "camera": {"x": 10, "y": 20, "width": 1600, "height": 900},
                 "action": {"4": "She turns"}, "dialogue": {"6": "Hello"}, "transition": "dissolve",
                 "tracks": {}},
                {"name": "Cut 9", "tracks": {}}
             ]},
            {"name": "Scene 2", "cuts": [{"name": "A", "tracks": {}}, {"name": "B", "tracks": {}, "transition": "flash"}]},
            {"name": "Scene 3", "markers": [{"frame": 1, "label": "in"}], "cuts": [{"name": "C", "tracks": {}}]}
        ],
        "statics": [], "mouths": [], "kinds": {},
        "stages": {}, "palettes": [{"name": "Ana", "colours": [{"name": "Ink", "hex": "#16161a"}]}]
    });
    host.e
        .together
        .send(Cmd::RawEdit(json!({"k": "timeline", "tl": rich})));
    until(&mut [&mut host, &mut guest], 20, "the timeline", |a| {
        a.iter().all(|x| {
            x.e.together
                .win
                .full_tl
                .as_ref()
                .is_some_and(|t| t.scenes.len() == 3)
        })
    });
    settle(&mut [&mut host, &mut guest], 300);
    let before = host.e.together.win.full_tl.clone().unwrap();
    // the guest draws on its own layer (a new drawing on a blank frame) and holds it longer
    guest.stroke(700.0);
    guest.s.actions.push(Action::Other(
        crate::shell::engine::frames::ops::HOLD_LONGER.into(),
    ));
    let own_lid = guest
        .e
        .together
        .lid_of(guest.e.painter.active_layer())
        .cloned()
        .unwrap();
    let own = host
        .e
        .together
        .view
        .ids
        .iter()
        .find(|(l, _)| *l == own_lid)
        .map(|(_, id)| *id)
        .unwrap();
    until(
        &mut [&mut host, &mut guest],
        20,
        "the guest's timeline",
        |a| {
            a[0].e.together.win.full_tl.as_ref().is_some_and(|t| {
                t.scenes[0].cuts[0]
                    .tracks
                    .get(&own)
                    .is_some_and(|x| x.iter().any(|e| e.drawing != 0))
            })
        },
    );
    settle(&mut [&mut host, &mut guest], 300);
    let mut after = host.e.together.win.full_tl.clone().unwrap();
    let cels = after.scenes[0].cuts[0].tracks.remove(&own).unwrap();
    let mut was = before.clone();
    was.scenes[0].cuts[0].tracks.remove(&own);
    // (the guest's new drawing's kind, if any, aside)
    after.kinds.retain(|d, _| before.kinds.contains_key(d));
    assert_eq!(after, was, "only the guest's own track changed");
    assert!(
        cels.iter().any(|c| c.drawing != 0 && c.length >= 2),
        "held longer: {cels:?}"
    );
    same(&mut host, &mut guest, "the same pictures");

    // a third joins (from the host) and gets the whole timeline
    let code = host.s.together.info.as_ref().unwrap().invite.clone();
    let mut third = App::new("Cy");
    third.e.together.auto = Some(Start::Join(code));
    until(
        &mut [&mut host, &mut guest, &mut third],
        30,
        "the third joining",
        |a| a[2].e.together.synced(),
    );
    settle(&mut [&mut host, &mut guest, &mut third], 300);
    let t = third.e.together.win.full_tl.as_ref().unwrap();
    assert_eq!(t.scenes.len(), 3, "every scene");
    assert_eq!(t.scenes[0].cuts.len(), 2, "every cut");
    assert_eq!(t.scenes[0].markers.len(), 1);
    assert_eq!(t.scenes[0].audio.len(), 1);
    assert_eq!(t.scenes[0].cuts[0].transition.as_deref(), Some("dissolve"));
    assert_eq!(t.palettes.len(), 1);
}

/// A project from a file with scenes and cuts, hosted here: a joiner gets its drawings'
/// pixels too (kept as the file has them), in every cut.
#[test]
fn a_project_from_a_file_is_shared_whole() {
    let Ok(path) = std::env::var("PP_MULTI") else {
        return;
    };
    let mut host = App::new("Ana");
    assert!(crate::shell::engine::files::open_path(
        &mut host.e,
        &mut host.s,
        std::path::Path::new(&path)
    ));
    host.tick();
    host.e.together.auto = Some(Start::Host);
    until(&mut [&mut host], 20, "hosting", |a| {
        a[0].e.together.synced()
    });
    let code = host.s.together.info.as_ref().unwrap().invite.clone();
    let mut guest = App::new("Ben");
    guest.e.together.auto = Some(Start::Join(code));
    until(&mut [&mut host, &mut guest], 30, "joining", |a| {
        a[1].e.together.synced()
    });
    settle(&mut [&mut host, &mut guest], 500);
    eprintln!(
        "host frame {} guest frame {}; host store {} guest store {}",
        host.s.frame,
        guest.s.frame,
        host.e.frames.store.drawings.len(),
        guest.e.frames.store.drawings.len()
    );
    same(&mut host, &mut guest, "the same picture");
}
