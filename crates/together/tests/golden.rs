//! Golden tests: the studio's together.ts ran the scripts in `golden/scenarios.json`
//! (`golden/record.ts`) and wrote what every app sent and ended with to `fixtures/`.
//!
//! - **The same edits:** the same scripts run here, with Rust apps only, send the same
//!   edits and messages, in the same order, tile and stroke payloads byte for byte, and
//!   every app ends the same (canvas, timeline, shared state).
//! - **Mixed sessions:** each app in turn is a Rust app among the studio's: the others
//!   send exactly what they sent in the recording, and the Rust app ends as the studio's
//!   did. (So a Rust app takes in the studio's edits, and the studio's took in what the
//!   Rust one sends, which is the same.)

mod common;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use common::{FakeCanvas, base_pack, normalise, stroke, tile, timeline_json};
use pencilpilot_together::js::{self, to_base64};
use pencilpilot_together::lines::{LineStroke, encode_lines};
use pencilpilot_together::timeline::{
    AudioClip, Camera, Cut, Exposure, Marker, MouthChart, Scene, Timeline,
};
use pencilpilot_together::{
    CanvasPort, GroupSettings, LayerStyle, Notice, Out, SavedMember, SavedShare, TileMap, Together,
};
use serde_json::{Map, Value, json};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn member(k: &str) -> String {
    k.to_lowercase().repeat(64)
}

fn key_of(id: &str) -> String {
    id[..1].to_uppercase()
}

fn fixture(name: &str) -> Value {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

struct Real {
    t: Together,
    c: FakeCanvas,
    name: String,
    notes: Vec<String>,
    tiles: Vec<String>,
    drawings: HashMap<String, u32>,
    saved: Option<Value>,
}

enum Member {
    Real(Box<Real>),
    /// the studio's app, as recorded: what it sent each time it ran
    Stub(VecDeque<Vec<Value>>),
}

struct Session {
    seq: u64,
    members: BTreeMap<String, Member>,
    parked: BTreeMap<String, Member>,
    /// in the order they came in (deliveries go to each)
    joined: Vec<String>,
    queue: VecDeque<(String, Option<u64>, String, Value)>,
    log: Vec<(String, Value)>,
    directs: Vec<(String, String, Value)>,
    saves: BTreeMap<String, Value>,
    /// the recording (what stubs send)
    rec_log: Vec<Value>,
    rec_directs: Vec<Value>,
}

impl Session {
    fn number(&mut self, from: &str, body: Value) {
        self.seq += 1;
        self.log.push((from.into(), body.clone()));
        for to in &self.joined {
            self.queue
                .push_back((to.clone(), Some(self.seq), from.into(), body.clone()));
        }
    }

    fn direct(&mut self, from: &str, to: &str, body: Value) {
        self.directs.push((from.into(), to.into(), body.clone()));
        self.queue.push_back((to.into(), None, from.into(), body));
    }

    /// Run app `k` once (its action, or a message to it) and send what it sent.
    fn act(&mut self, k: &str, f: impl FnOnce(&mut Real)) {
        let outs = match self.members.get_mut(k).unwrap() {
            Member::Real(r) => {
                f(r);
                for n in r.t.take_notices() {
                    match n {
                        Notice::Note(t) => r.notes.push(t),
                        Notice::Tiles { from } => r.tiles.push(from),
                        Notice::Changed => {}
                    }
                }
                r.t.take_out()
                    .into_iter()
                    .map(|o| match o {
                        Out::Edit(b) => ("e".to_string(), String::new(), b),
                        Out::Direct { to, body } => ("d".to_string(), key_of(&to), body),
                    })
                    .collect::<Vec<_>>()
            }
            Member::Stub(acts) => {
                let outs = acts
                    .pop_front()
                    .expect("the recording ran out: the apps went differently");
                outs.into_iter()
                    .map(|o| {
                        let i = o[1].as_u64().unwrap() as usize;
                        if o[0] == "e" {
                            (
                                "e".to_string(),
                                String::new(),
                                self.rec_log[i]["body"].clone(),
                            )
                        } else {
                            let d = &self.rec_directs[i];
                            (
                                "d".to_string(),
                                d["to"].as_str().unwrap().to_string(),
                                d["body"].clone(),
                            )
                        }
                    })
                    .collect()
            }
        };
        for (kind, to, body) in outs {
            if kind == "e" {
                self.number(k, body);
            } else {
                self.direct(k, &to, body);
            }
        }
    }

    fn step(&mut self, n: usize) {
        for _ in 0..n {
            let Some((to, seq, from, body)) = self.queue.pop_front() else {
                return;
            };
            if !self.members.contains_key(&to) {
                continue;
            }
            let from_id = member(&from);
            self.act(&to, |r| match seq {
                Some(seq) => r.t.on_edit(&mut r.c, seq, &from_id, &body),
                None => r.t.on_direct(&mut r.c, &from_id, &body),
            });
        }
    }

    fn settle(&mut self) {
        while !self.queue.is_empty() {
            self.step(1);
        }
    }
}

fn layer_id(c: &FakeCanvas, name: &str) -> u32 {
    c.layer_list
        .iter()
        .rfind(|l| l.name == name)
        .unwrap_or_else(|| panic!("no layer {name}"))
        .id
}

fn group_id(c: &FakeCanvas, name: &str) -> u32 {
    c.group_list
        .iter()
        .find(|g| g.settings.name == name)
        .unwrap_or_else(|| panic!("no group {name}"))
        .id
}

fn lid_of(r: &Real, st: &Value) -> String {
    match st.get("group").and_then(Value::as_str) {
        Some(g) => r.t.gid_of(group_id(&r.c, g)).unwrap(),
        None => {
            r.t.lid_of(layer_id(&r.c, st["layer"].as_str().unwrap()))
                .unwrap()
        }
    }
}

fn drawing(r: &mut Real, v: &Value) -> u32 {
    match v {
        Value::Number(n) => n.as_u64().unwrap() as u32,
        Value::String(s) => {
            if let Some(d) = r.drawings.get(s) {
                return *d;
            }
            let d = r.c.new_drawing();
            r.drawings.insert(s.clone(), d);
            d
        }
        _ => 0,
    }
}

fn keys(v: &Value) -> Vec<(i32, i32)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|k| (k[0].as_i64().unwrap() as i32, k[1].as_i64().unwrap() as i32))
        .collect()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// Run a script; `real`: which apps are Rust ones (the others play the recording).
fn run(rec: &Value, real: &[&str]) -> Session {
    let mut acts: HashMap<String, VecDeque<Vec<Value>>> = rec["acts"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_array().unwrap().clone())
                    .collect(),
            )
        })
        .collect();
    let mut ses = Session {
        seq: 0,
        members: BTreeMap::new(),
        parked: BTreeMap::new(),
        joined: Vec::new(),
        queue: VecDeque::new(),
        log: Vec::new(),
        directs: Vec::new(),
        saves: BTreeMap::new(),
        rec_log: rec["log"].as_array().unwrap().clone(),
        rec_directs: rec["directs"].as_array().unwrap().clone(),
    };
    for st in rec["steps"].as_array().unwrap() {
        let k = st
            .get("app")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let what = s(&st["do"]);
        let is_real = real.contains(&k.as_str());
        // the canvas, for steps on it (a stub has none: skipped)
        let canvas_step = !matches!(
            what,
            "app"
                | "settle"
                | "step"
                | "forge"
                | "leave"
                | "back"
                | "host"
                | "join"
                | "local"
                | "timeline"
                | "ask"
                | "grant"
                | "deny"
                | "revoke"
                | "mergeDown"
                | "deleteLayer"
                | "addGroup"
                | "moveLayerTo"
                | "changeGroup"
                | "ungroup"
                | "deleteGroup"
                | "mergeGroup"
                | "moveGroup"
                | "moveOwn"
                | "removeOwn"
                | "shareAbove"
                | "addOwnLayer"
                | "save"
                | "restore"
                | "rejoin"
                | "resumeHost"
        );
        if canvas_step {
            if !is_real {
                continue;
            }
            let m = ses
                .members
                .get_mut(&k)
                .or_else(|| ses.parked.get_mut(&k))
                .unwrap();
            let Member::Real(r) = m else { unreachable!() };
            canvas(r, what, st);
            continue;
        }
        match what {
            "app" => {
                let m = if is_real {
                    let names: Vec<&str> = st["layers"].as_array().unwrap().iter().map(s).collect();
                    let mut r = Real {
                        t: Together::with_start(&member(&k), s(&st["name"]), 0),
                        c: FakeCanvas::new(&names),
                        name: s(&st["name"]).into(),
                        notes: vec![],
                        tiles: vec![],
                        drawings: HashMap::new(),
                        saved: None,
                    };
                    r.c.layer_list.iter_mut().for_each(|_| {});
                    Member::Real(Box::new(r))
                } else {
                    Member::Stub(acts.remove(&k).unwrap_or_default())
                };
                ses.members.insert(k.clone(), m);
                ses.joined.push(k.clone());
            }
            "settle" => ses.settle(),
            "step" => ses.step(st["n"].as_u64().unwrap() as usize),
            "forge" => ses.number(s(&st["from"]), st["body"].clone()),
            "leave" => {
                let m = ses.members.remove(&k).unwrap();
                ses.parked.insert(k.clone(), m);
                ses.joined.retain(|x| *x != k);
            }
            "back" => {
                let mut m = ses.parked.remove(&k).unwrap();
                if let Member::Real(r) = &mut m {
                    r.t = Together::with_start(&member(&k), &r.name, 0);
                    r.notes.clear();
                    r.tiles.clear();
                }
                ses.members.insert(k.clone(), m);
                ses.joined.push(k.clone());
            }
            "save" => {
                let members: Vec<SavedMember> = st
                    .get("members")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|m| SavedMember {
                                id: member(s(m)),
                                name: s(m).into(),
                                addr: json!({}),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(Member::Real(r)) = ses.members.get_mut(&k) {
                    let saved = r.t.save("pp1-code", &members).to_json();
                    let saved: Value = serde_json::from_str(&saved.to_string()).unwrap();
                    r.saved = Some(saved.clone());
                    ses.saves.insert(k.clone(), saved);
                }
            }
            _ => {
                let st = st.clone();
                ses.act(&k, |r| together(r, &st));
            }
        }
    }
    ses
}

/// A step of this app's side of drawing together.
fn together(r: &mut Real, st: &Value) {
    let Real { t, c, .. } = r;
    let c: &mut FakeCanvas = c;
    let to = || member(s(&st["to"]));
    match s(&st["do"]) {
        "host" => t.start_host(c),
        "join" => t.start_join(c, &member(s(&st["host"]))),
        "local" => t.local_changed(c),
        "timeline" => t.timeline_changed(c),
        "ask" => {
            let lid = lid_of(r, st);
            r.t.ask(&lid)
        }
        "grant" | "deny" | "revoke" => {
            let lid = lid_of(r, st);
            match s(&st["do"]) {
                "grant" => {
                    r.t.grant(&lid, &to(), st.get("handover") == Some(&Value::Bool(true)))
                }
                "deny" => r.t.deny(&lid, &to()),
                _ => r.t.revoke(&lid, &to()),
            }
        }
        "mergeDown" => {
            let id = layer_id(c, s(&st["layer"]));
            t.merge_down(c, id);
        }
        "deleteLayer" => {
            let id = layer_id(c, s(&st["layer"]));
            t.delete_layer(c, id);
        }
        "addGroup" => {
            let id = layer_id(c, s(&st["layer"]));
            t.add_group(c, id);
        }
        "moveLayerTo" => {
            let id = layer_id(c, s(&st["layer"]));
            let g = st
                .get("group")
                .and_then(Value::as_str)
                .map_or(0, |g| group_id(c, g));
            t.move_layer_to(c, id, st["to"].as_u64().unwrap() as u32, g);
        }
        "changeGroup" => {
            let g = group_id(c, s(&st["group"]));
            let set = GroupSettings {
                name: s(&st["name"]).into(),
                visible: st["visible"].as_bool().unwrap(),
                opacity: f(&st["opacity"]),
            };
            t.change_group(
                c,
                g,
                Box::new(move |c: &mut dyn CanvasPort| c.set_group(g, &set)),
            );
        }
        "ungroup" => {
            let g = group_id(c, s(&st["group"]));
            t.ungroup(c, g);
        }
        "deleteGroup" => {
            let g = group_id(c, s(&st["group"]));
            t.delete_group(c, g);
        }
        "mergeGroup" => {
            let g = group_id(c, s(&st["group"]));
            t.merge_group(c, g);
        }
        "moveGroup" => {
            let g = group_id(c, s(&st["group"]));
            t.move_group(c, g, st.get("up") == Some(&Value::Bool(true)));
        }
        "moveOwn" => {
            let lid = lid_of(r, st);
            r.t.move_own(&lid, st["to"].as_u64().unwrap() as u32)
        }
        "removeOwn" => {
            let lid = lid_of(r, st);
            r.t.remove_own(&lid)
        }
        "shareAbove" => {
            let (id, above) = (layer_id(c, s(&st["layer"])), layer_id(c, s(&st["above"])));
            t.share_above(c, id, above);
        }
        "addOwnLayer" => t.add_own_layer(
            st.get("name").and_then(Value::as_str).unwrap_or(""),
            st.get("lines") == Some(&Value::Bool(true)),
        ),
        "restore" => {
            let saved = SavedShare::read(r.saved.as_ref().unwrap()).unwrap();
            r.t.restore(&mut r.c, &saved)
        }
        "rejoin" => t.rejoin(&member(s(&st["host"]))),
        "resumeHost" => t.resume_host(c),
        other => panic!("unknown step {other}"),
    }
}

fn cut0(tl: &mut Timeline) -> &mut Cut {
    &mut tl.scenes[0].cuts[0]
}

/// A step on the canvas.
fn canvas(r: &mut Real, what: &str, st: &Value) {
    let lay = |r: &Real| layer_id(&r.c, s(&st["layer"]));
    match what {
        "busy" => r.c.busy = st["on"].as_bool().unwrap(),
        "paint" => {
            let id = lay(r);
            r.c.paint(
                id,
                st["x"].as_i64().unwrap() as i32,
                st["y"].as_i64().unwrap() as i32,
                st["v"].as_u64().unwrap() as u8,
            );
        }
        "paintD" => {
            let d = drawing(r, &st["d"]);
            r.c.drawings.get_mut(&d).unwrap().insert(
                (
                    st["x"].as_i64().unwrap() as i32,
                    st["y"].as_i64().unwrap() as i32,
                ),
                tile(st["v"].as_u64().unwrap() as u8),
            );
        }
        "addLayer" => {
            r.c.add_layer(s(&st["name"]), st.get("adjust").and_then(Value::as_str));
        }
        "set" => {
            let id = lay(r);
            let l = r.c.layer_list.iter_mut().find(|l| l.id == id).unwrap();
            for (k, v) in st["patch"].as_object().unwrap() {
                match k.as_str() {
                    "opacity" => l.opacity = f(v),
                    "blend" => l.blend = s(v).into(),
                    "visible" => l.visible = v.as_bool().unwrap(),
                    "style" => l.style = LayerStyle::read(v),
                    _ => panic!("patch {k}"),
                }
            }
        }
        "mask" => {
            let id = lay(r);
            let l = r.c.layer_list.iter_mut().find(|l| l.id == id).unwrap();
            l.mask = st["tiles"].as_array().map(|t| {
                t.iter()
                    .map(|x| {
                        (
                            (x[0].as_i64().unwrap() as i32, x[1].as_i64().unwrap() as i32),
                            tile(x[2].as_u64().unwrap() as u8),
                        )
                    })
                    .collect()
            });
        }
        "adjust" => {
            let id = lay(r);
            r.c.set_adjust(id, s(&st["spec"]));
        }
        "group" => {
            let id = lay(r);
            r.c.add_group(id);
        }
        "groupSet" => {
            let g = group_id(&r.c, s(&st["group"]));
            r.c.set_group(
                g,
                &GroupSettings {
                    name: s(&st["name"]).into(),
                    visible: st["visible"].as_bool().unwrap(),
                    opacity: f(&st["opacity"]),
                },
            );
        }
        "lineLayer" => {
            let id = lay(r);
            r.c.set_line_layer(id, st["on"].as_bool().unwrap());
        }
        "strokes" => {
            let id = lay(r);
            let d = r.c.drawing_of(id);
            let list: Vec<LineStroke> = st["list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| stroke(s(&x[0]), (f(&x[1]) + x.get(2).map_or(0.0, f)) as f32))
                .collect();
            r.c.set_line_strokes(
                d,
                Arc::new(list),
                st.get("plain") == Some(&Value::Bool(true)),
            );
        }
        "base" => {
            let id = lay(r);
            let d = r.c.drawing_of(id);
            if let Some(o) = st.get("out") {
                r.c.base_out
                    .insert(d, base_pack(&keys(&o[0]), &keys(&o[1])));
            }
            if let Some(o) = st.get("full") {
                r.c.base_full
                    .insert(d, base_pack(&keys(&o[0]), &keys(&o[1])));
            }
        }
        "drawingOf" => {
            let id = lay(r);
            r.c.drawing_of(id);
        }
        "track" => {
            let id = lay(r);
            let track: Vec<Exposure> = st["track"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| Exposure {
                    drawing: drawing(r, &e[0]),
                    length: e[1].as_u64().unwrap() as u32,
                })
                .collect();
            cut0(&mut r.c.tl).tracks.insert(id, track);
        }
        "showFrame" => r.c.show_frame(st["f"].as_u64().unwrap() as u32),
        "fresh" => {
            let d = drawing(r, &st["d"]);
            let id = lay(r);
            r.c.layer_list
                .iter_mut()
                .find(|l| l.id == id)
                .unwrap()
                .drawing = d;
            cut0(&mut r.c.tl).tracks.insert(
                id,
                vec![Exposure {
                    drawing: d,
                    length: 1,
                }],
            );
        }
        "follow" => {
            let (from, to) = (layer_id(&r.c, s(&st["from"])), layer_id(&r.c, s(&st["to"])));
            let mut tl = std::mem::take(&mut r.c.tl);
            common::follow_timing(&mut tl, from, to, &mut || r.c.new_drawing());
            r.c.tl = tl;
        }
        "sound" => {
            let bytes: Vec<u8> = (0..st["size"].as_u64().unwrap())
                .map(|i| (i % 251) as u8)
                .collect();
            r.c.sounds.insert(
                s(&st["file"]).into(),
                (Arc::new(bytes), s(&st["name"]).into()),
            );
        }
        "audio" => {
            r.c.tl.scenes[0].audio = st["clips"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, a)| AudioClip {
                    id: i as u32 + 1,
                    name: s(&a["name"]).into(),
                    file: s(&a["file"]).into(),
                    start: f(&a["start"]),
                    gain: f(&a["gain"]),
                    muted: a["muted"].as_bool().unwrap(),
                })
                .collect();
        }
        "stage" => {
            let id = lay(r);
            r.c.tl.stages.insert(id, s(&st["stage"]).into());
        }
        "palettes" => {
            r.c.tl.palettes = pencilpilot_together::timeline::read_palettes(Some(&st["list"]))
        }
        "markers" => {
            r.c.tl.scenes[0].markers = st["list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| Marker {
                    frame: m["frame"].as_u64().unwrap() as u32,
                    label: s(&m["label"]).into(),
                })
                .collect();
        }
        "cut" => {
            let set = &st["set"];
            let notes = |v: &Value| -> BTreeMap<u32, String> {
                v.as_object()
                    .map(|o| {
                        o.iter()
                            .map(|(k, t)| (k.parse().unwrap(), s(t).to_string()))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let c = cut0(&mut r.c.tl);
            if let Some(cam) = set.get("camera") {
                c.camera = Some(Camera {
                    x: f(&cam["x"]),
                    y: f(&cam["y"]),
                    width: f(&cam["width"]),
                    height: f(&cam["height"]),
                });
            }
            if let Some(a) = set.get("action") {
                c.action = notes(a);
            }
            if let Some(a) = set.get("dialogue") {
                c.dialogue = notes(a);
            }
            if let Some(t) = set.get("transition") {
                c.transition = Some(s(t).into());
            }
        }
        "scene" => {
            let id = r.c.tl.scenes.len() as u32 + 1;
            r.c.tl.scenes.push(Scene {
                id,
                name: s(&st["name"]).into(),
                cuts: vec![Cut {
                    id: 1,
                    name: "Cut 1".into(),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        "mouths" => {
            let list: Vec<MouthChart> = st["list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    let layer = layer_id(&r.c, s(&m["layer"]));
                    let shapes = m["shapes"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(sh, d)| (sh.clone(), drawing(r, d)))
                        .collect();
                    MouthChart { layer, shapes }
                })
                .collect();
            r.c.tl.mouths = list;
        }
        "kinds" => {
            let list: Vec<(u32, String)> = st["list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (drawing(r, &x[0]), s(&x[1]).to_string()))
                .collect();
            r.c.tl.kinds = list.into_iter().collect();
        }
        "statics" => {
            r.c.tl.statics = st["layers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| layer_id(&r.c, s(n)))
                .collect();
        }
        other => panic!("unknown step {other}"),
    }
}

// ---- what an app ends with, as record.ts writes it ----

fn tiles_json(m: &TileMap) -> Value {
    Value::Object(
        m.iter()
            .map(|((x, y), px)| {
                (
                    format!("{x},{y}"),
                    json!([px[0], px.iter().all(|v| *v == px[0])]),
                )
            })
            .collect(),
    )
}

fn state(r: &Real) -> Value {
    let c = &r.c;
    let (layers, groups, asks) = r.t.shared.snapshot();
    let mut plain: Vec<u32> = c.plain.iter().copied().collect();
    plain.sort();
    let mut v = json!({
        "layers": c.layer_list.iter().map(|l| json!({
            "id": l.id,
            "name": l.name,
            "locked": l.locked,
            "visible": l.visible,
            "opacity": js::num(l.opacity),
            "blend": l.blend,
            "adjust": l.adjust,
            "group": l.group,
            "style": l.style.as_ref().map_or(Value::Null, LayerStyle::to_json),
            "drawing": l.drawing,
            "lines": c.line_layers.contains(&l.id),
            "mask": l.mask.as_ref().map_or(Value::Null, tiles_json),
        })).collect::<Vec<_>>(),
        "groups": c.group_list.iter().map(|g| json!({"id": g.id, "name": g.settings.name, "visible": g.settings.visible, "opacity": js::num(g.settings.opacity)})).collect::<Vec<_>>(),
        "selected": c.selected,
        "drawings": Value::Object(c.drawings.iter().filter(|(_, m)| !m.is_empty()).map(|(d, m)| (d.to_string(), tiles_json(m))).collect()),
        "strokes": Value::Object(c.strokes.iter().map(|(d, l)| (d.to_string(), Value::from(to_base64(&encode_lines(l))))).collect()),
        "plain": plain,
        "baseIn": c.base_in.iter().map(|(d, b)| json!([d, to_base64(b)])).collect::<Vec<_>>(),
        "sounds": Value::Object(c.sounds.iter().map(|(f, (b, n))| (f.clone(), json!([n, b.len(), b.iter().map(|x| *x as u64).sum::<u64>()]))).collect()),
        "tl": timeline_json(&c.tl),
        "shared": {"layers": layers, "groups": groups, "asks": asks},
        "ids": r.t.ids.iter().map(|(l, i)| json!([l, i])).collect::<Vec<_>>(),
        "gids": r.t.gids.iter().map(|(l, i)| json!([l, i])).collect::<Vec<_>>(),
        "synced": r.t.synced(),
        "dids": r.t.save("pp1-x", &[]).dids.iter().map(|(d, n)| json!([d, n])).collect::<Vec<_>>(),
        "notes": r.notes,
        "tilesFrom": r.tiles,
    });
    normalise(&mut v);
    canon(v)
}

/// The drawings in an app's state by their shared ids where they have one: each app
/// numbers the drawings it gets in the order it meets them, and JSON objects reach the
/// Rust app with their keys sorted (the studio meets them in the sender's order).
fn canon(mut v: Value) -> Value {
    let names: HashMap<u64, String> = v["dids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[1].as_u64().unwrap(), p[0].as_str().unwrap().to_string()))
        .collect();
    let name = |n: &Value| match n.as_u64().and_then(|n| names.get(&n)) {
        Some(d) => Value::from(d.clone()),
        None => n.clone(),
    };
    let key = |k: &str| {
        k.parse::<u64>()
            .ok()
            .and_then(|n| names.get(&n).cloned())
            .unwrap_or(k.to_string())
    };
    let rekey = |o: &mut Value| {
        if let Some(m) = o.as_object_mut() {
            *m = std::mem::take(m)
                .into_iter()
                .map(|(k, x)| (key(&k), x))
                .collect();
        }
    };
    for l in v["layers"].as_array_mut().unwrap() {
        l["drawing"] = name(&l["drawing"]);
    }
    for k in ["drawings", "strokes"] {
        rekey(&mut v[k]);
    }
    for p in v["plain"].as_array_mut().unwrap() {
        *p = name(p);
    }
    for b in v["baseIn"].as_array_mut().unwrap() {
        b[0] = name(&b[0]);
    }
    let tl = &mut v["tl"];
    for sc in tl["scenes"].as_array_mut().unwrap() {
        for c in sc["cuts"].as_array_mut().unwrap() {
            for t in c["tracks"].as_object_mut().unwrap().values_mut() {
                for e in t.as_array_mut().unwrap() {
                    e["drawing"] = name(&e["drawing"]);
                }
            }
        }
    }
    if let Some(m) = tl.get_mut("mouths").and_then(Value::as_array_mut) {
        for m in m {
            for d in m["shapes"].as_object_mut().unwrap().values_mut() {
                *d = name(d);
            }
        }
    }
    if let Some(k) = tl.get_mut("kinds") {
        rekey(k);
    }
    v["dids"] = Value::Null;
    v
}

/// Where two values differ (paths), for a readable failure.
fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(
                    &format!("{path}.{k}"),
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), p, q, out);
            }
        }
        (Value::Number(p), Value::Number(q)) if p.as_f64() == q.as_f64() => {}
        _ if a == b => {}
        _ => {
            let short = |v: &Value| {
                let t = v.to_string();
                if t.len() > 160 {
                    format!("{}…", &t[..160])
                } else {
                    t
                }
            };
            out.push(format!("{path}: rust {} / studio {}", short(a), short(b)));
        }
    }
}

fn assert_same(what: &str, rust: &Value, studio: &Value) {
    let mut out = Vec::new();
    diff(what, rust, studio, &mut out);
    assert!(
        out.is_empty(),
        "{} difference(s):\n{}",
        out.len(),
        out[..out.len().min(30)].join("\n")
    );
}

fn studio_state(rec: &Value, k: &str) -> Value {
    let mut v = rec["final"][k].clone();
    v["tl"] = {
        let mut t = v["tl"].clone();
        // (the studio's timeline object: its optional parts as the Rust one writes them)
        if t.get("statics").is_none() {
            t["statics"] = json!([]);
        }
        t
    };
    normalise(&mut v);
    canon(v)
}

/// The same edits: Rust apps only, the same script.
fn same_edits(name: &str) {
    let rec = fixture(name);
    let apps: Vec<String> = rec["acts"].as_object().unwrap().keys().cloned().collect();
    let real: Vec<&str> = apps.iter().map(String::as_str).collect();
    let ses = run(&rec, &real);
    let studio_log: Vec<Value> = rec["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| json!([e["from"], e["body"]]))
        .collect();
    let rust_log: Vec<Value> = ses.log.iter().map(|(f, b)| json!([f, b])).collect();
    for (i, (r, t)) in rust_log.iter().zip(&studio_log).enumerate() {
        assert_same(&format!("{name}: edit {i} ({})", t[1]["k"]), r, t);
    }
    assert_eq!(rust_log.len(), studio_log.len(), "{name}: edits sent");
    let studio_directs: Vec<Value> = rec["directs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| json!([d["from"], d["to"], d["body"]]))
        .collect();
    let rust_directs: Vec<Value> = ses
        .directs
        .iter()
        .map(|(f, t, b)| json!([f, t, b]))
        .collect();
    for (i, (r, t)) in rust_directs.iter().zip(&studio_directs).enumerate() {
        assert_same(&format!("{name}: direct {i} ({})", t[2]["k"]), r, t);
    }
    assert_eq!(
        rust_directs.len(),
        studio_directs.len(),
        "{name}: direct messages"
    );
    for (k, saved) in &ses.saves {
        assert_same(
            &format!("{name}: {k}'s saved session"),
            saved,
            &rec["saves"][k],
        );
    }
    for (k, m) in &ses.members {
        let Member::Real(r) = m else { unreachable!() };
        assert_same(
            &format!("{name}: {k} ends"),
            &state(r),
            &studio_state(&rec, k),
        );
    }
}

/// Mixed sessions: each app in turn a Rust one among the studio's.
fn mixed(name: &str) {
    let rec = fixture(name);
    for k in rec["final"].as_object().unwrap().keys() {
        let ses = run(&rec, &[k.as_str()]);
        let Some(Member::Real(r)) = ses.members.get(k) else {
            panic!("{k} isn't in the session at the end")
        };
        assert_same(
            &format!("{name}: {k} (Rust, among the studio's apps) ends"),
            &state(r),
            &studio_state(&rec, k),
        );
        assert_eq!(
            ses.log.len(),
            rec["log"].as_array().unwrap().len(),
            "{name} with {k} in Rust: edits"
        );
    }
}

macro_rules! goldens {
    ($($name:ident),*) => {
        mod same_edits {
            $(#[test] fn $name() { super::same_edits(stringify!($name)); })*
        }
        mod mixed_sessions {
            $(#[test] fn $name() { super::mixed(stringify!($name)); })*
        }
    };
}

goldens!(basics, groups, masks_sound, timeline, lines, rejoin);

#[allow(dead_code)]
fn _unused(_: Map<String, Value>) {}
