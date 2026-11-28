//! The timeline, shared: this app's (layers and drawings by their ids here) and the
//! session's (layers by their shared id, drawings by theirs: "<member>:<n>"; "" = an
//! empty cel), and the way between them, as together.ts's `toShared` / `fromShared`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::edit::is_sound_key;
use crate::js::{self, finite, integer, is_lid, num, or, slice16};
use crate::lines::js_round;

/// One drawing held for some frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exposure {
    /// the drawing shown (0 = an empty cel)
    pub drawing: u32,
    /// frames it is held for (1–999)
    pub length: u32,
}

/// A rectangle of the canvas, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cut {
    pub id: u32,
    pub name: String,
    /// layer id → its exposures; a layer missing here is empty all cut long
    pub tracks: BTreeMap<u32, Vec<Exposure>>,
    pub camera: Option<Camera>,
    /// exposure-sheet notes, frame → text
    pub action: BTreeMap<u32, String>,
    pub dialogue: BTreeMap<u32, String>,
    /// "flash" or "dissolve" (none: a hard cut)
    pub transition: Option<String>,
}

/// A sound file placed in a scene.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioClip {
    pub id: u32,
    pub name: String,
    /// the sound file's key (the studio keeps the bytes)
    pub file: String,
    /// the scene frame it starts on (negative: before the scene)
    pub start: f64,
    /// loudness (1 = as recorded; 0–16)
    pub gain: f64,
    pub muted: bool,
}

/// A marker on a scene frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marker {
    pub frame: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub id: u32,
    pub name: String,
    pub cuts: Vec<Cut>,
    pub audio: Vec<AudioClip>,
    pub markers: Vec<Marker>,
}

/// A lip-sync mouth chart: the drawing of a layer for each mouth shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MouthChart {
    pub layer: u32,
    /// "A"…"H", "X" → drawing
    pub shapes: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaletteColour {
    pub name: String,
    /// `#rrggbb`, lower case
    pub hex: String,
}

/// A character palette.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Palette {
    pub name: String,
    pub colours: Vec<PaletteColour>,
}

/// This app's timeline (the studio's `Timeline`; empty lists and maps are "none").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    pub scenes: Vec<Scene>,
    /// static layers: one drawing all cut long
    pub statics: Vec<u32>,
    pub mouths: Vec<MouthChart>,
    /// drawing → "key", "breakdown" or "inbetween"
    pub kinds: BTreeMap<u32, String>,
    /// layer → its production stage
    pub stages: BTreeMap<u32, String>,
    pub palettes: Vec<Palette>,
}

pub const DRAWING_KINDS: [&str; 3] = ["key", "breakdown", "inbetween"];
pub const STAGES: [&str; 8] = [
    "board", "rough", "tiedown", "clean", "tone", "flats", "shade", "comp",
];
pub const MOUTH_SHAPES: [&str; 9] = ["A", "B", "C", "D", "E", "F", "G", "H", "X"];
pub const MAX_PALETTES: usize = 32;
pub const MAX_COLOURS: usize = 64;

fn clean_name(v: Option<&Value>, fallback: &str) -> String {
    let s = match v.and_then(Value::as_str) {
        Some(s) => slice16(js::trim(s), 40),
        None => String::new(),
    };
    if s.is_empty() { fallback.into() } else { s }
}

/// `readPalettes`: palettes from a file or a peer, anything unusable dropped, sizes capped.
pub fn read_palettes(raw: Option<&Value>) -> Vec<Palette> {
    let Some(raw) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<Palette> = Vec::new();
    for p in raw.iter().take(MAX_PALETTES) {
        if !(p.is_object() || p.is_array()) {
            continue;
        }
        let mut colours = Vec::new();
        for c in p
            .get("colours")
            .and_then(Value::as_array)
            .map_or(&[][..], |a| &a[..a.len().min(MAX_COLOURS)])
        {
            let hex = c
                .get("hex")
                .and_then(Value::as_str)
                .map(str::to_lowercase)
                .unwrap_or_default();
            if hex.len() == 7
                && hex.starts_with('#')
                && hex[1..]
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                colours.push(PaletteColour {
                    name: clean_name(c.get("name"), &hex),
                    hex,
                });
            }
        }
        let fallback = format!("Palette {}", out.len() + 1);
        out.push(Palette {
            name: clean_name(p.get("name"), &fallback),
            colours,
        });
    }
    out
}

fn palettes_json(p: &[Palette]) -> Value {
    p.iter()
        .map(|p| {
            js::object(vec![
                ("name", p.name.clone().into()),
                (
                    "colours",
                    p.colours
                        .iter()
                        .map(|c| {
                            js::object(vec![
                                ("name", c.name.clone().into()),
                                ("hex", c.hex.clone().into()),
                            ])
                        })
                        .collect(),
                ),
            ])
        })
        .collect()
}

fn notes_json(n: &BTreeMap<u32, String>) -> Value {
    Value::Object(
        n.iter()
            .map(|(f, t)| (f.to_string(), Value::from(t.clone())))
            .collect(),
    )
}

/// `toShared`: this app's timeline for the session (only shared layers: `lid_of` gives
/// a layer's shared id, `did_of` a drawing's).
pub fn to_shared(
    tl: &Timeline,
    lid_of: &dyn Fn(u32) -> Option<String>,
    did_of: &mut dyn FnMut(u32) -> String,
) -> Value {
    let mut did = |d: u32| if d != 0 { did_of(d) } else { String::new() };
    let scenes: Vec<Value> = tl
        .scenes
        .iter()
        .map(|s| {
            let mut sc = js::object(vec![("name", s.name.clone().into())]);
            if !s.markers.is_empty() {
                sc["markers"] = s
                    .markers
                    .iter()
                    .map(|m| {
                        js::object(vec![
                            ("frame", m.frame.into()),
                            ("label", m.label.clone().into()),
                        ])
                    })
                    .collect();
            }
            if !s.audio.is_empty() {
                sc["audio"] = s
                    .audio
                    .iter()
                    .map(|a| {
                        js::object(vec![
                            ("name", a.name.clone().into()),
                            ("file", a.file.clone().into()),
                            ("start", num(a.start)),
                            ("gain", num(a.gain)),
                            ("muted", a.muted.into()),
                        ])
                    })
                    .collect();
            }
            let cuts: Vec<Value> = s
                .cuts
                .iter()
                .map(|c| {
                    let mut cut = js::object(vec![("name", c.name.clone().into())]);
                    if let Some(cam) = &c.camera {
                        cut["camera"] = js::object(vec![
                            ("x", num(cam.x)),
                            ("y", num(cam.y)),
                            ("width", num(cam.width)),
                            ("height", num(cam.height)),
                        ]);
                    }
                    // (the studio keeps an empty notes object when its notes are cleared)
                    if !c.action.is_empty() {
                        cut["action"] = notes_json(&c.action);
                    }
                    if !c.dialogue.is_empty() {
                        cut["dialogue"] = notes_json(&c.dialogue);
                    }
                    if let Some(t) = &c.transition {
                        cut["transition"] = t.clone().into();
                    }
                    let mut tracks = Map::new();
                    for (l, t) in &c.tracks {
                        if let Some(lid) = lid_of(*l) {
                            tracks.insert(
                                lid,
                                t.iter()
                                    .map(|e| {
                                        js::object(vec![
                                            ("d", did(e.drawing).into()),
                                            ("n", e.length.into()),
                                        ])
                                    })
                                    .collect(),
                            );
                        }
                    }
                    cut["tracks"] = Value::Object(tracks);
                    cut
                })
                .collect();
            sc["cuts"] = cuts.into();
            sc
        })
        .collect();
    let statics: Vec<Value> = tl
        .statics
        .iter()
        .filter_map(|l| lid_of(*l))
        .map(Value::from)
        .collect();
    let mouths: Vec<Value> = tl
        .mouths
        .iter()
        .filter_map(|m| {
            let lid = lid_of(m.layer)?;
            let mut shapes = Map::new();
            for (k, d) in &m.shapes {
                if *d != 0 {
                    shapes.insert(k.clone(), did(*d).into());
                }
            }
            Some(js::object(vec![
                ("lid", lid.into()),
                ("shapes", Value::Object(shapes)),
            ]))
        })
        .collect();
    let mut kinds = Map::new();
    for (d, k) in &tl.kinds {
        if *d != 0 {
            kinds.insert(did(*d), k.clone().into());
        }
    }
    let mut stages = Map::new();
    for (l, st) in &tl.stages {
        if let Some(lid) = lid_of(*l) {
            stages.insert(lid, st.clone().into());
        }
    }
    js::object(vec![
        ("scenes", scenes.into()),
        ("statics", statics.into()),
        ("mouths", mouths.into()),
        ("kinds", Value::Object(kinds)),
        ("stages", Value::Object(stages)),
        ("palettes", palettes_json(&tl.palettes)),
    ])
}

/// `Object.entries(v)` of a JSON value (an array's indices too, as JavaScript gives them).
fn entries(v: Option<&Value>) -> Vec<(String, &Value)> {
    match v {
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    }
}

/// A frame number as an object key (`Number.isInteger(+f) && +f >= 0`).
fn frame_key(f: &str) -> Option<u32> {
    let n = js::to_number(f);
    (n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n <= u32::MAX as f64).then_some(n as u32)
}

fn notes(n: Option<&Value>) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    if n.is_some_and(|n| n.is_object() || n.is_array()) {
        for (f, text) in entries(n) {
            if let (Some(f), Some(t)) = (frame_key(&f), text.as_str())
                && !t.is_empty()
            {
                out.insert(f, slice16(t, 500));
            }
        }
    }
    out
}

/// `fromShared`: a timeline from the session, as this app's. `id_of` gives a shared
/// layer's id here, `local_of` a drawing's (making one when it's new). `local` is this
/// app's timeline now: its unshared layers' tracks and stages stay (`is_shared` says
/// which layers are shared, from whoever sent it).
pub fn from_shared(
    s: &Value,
    id_of: &dyn Fn(&str) -> Option<u32>,
    local_of: &mut dyn FnMut(&str) -> u32,
    local: &Timeline,
    is_shared: &dyn Fn(u32) -> bool,
) -> Timeline {
    let mut drawing = |d: Option<&Value>| match d.and_then(Value::as_str) {
        Some(d) if is_lid(d) => local_of(d),
        _ => 0,
    };
    let empty = Vec::new();
    let scenes_in = s.get("scenes").and_then(Value::as_array).unwrap_or(&empty);
    let mut scenes = Vec::new();
    for (si, sc) in scenes_in.iter().take(200).enumerate() {
        let mine = local.scenes.get(si);
        let name = js::string(Some(
            or(sc.get("name")).unwrap_or(&Value::String(format!("Scene {}", si + 1))),
        ));
        let audio = match sc.get("audio").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => a
                .iter()
                .filter(|a| js::str_if(a.get("file"), is_sound_key).is_some())
                .take(200)
                .enumerate()
                .map(|(i, a)| AudioClip {
                    id: i as u32 + 1,
                    name: slice16(
                        &js::string(Some(or(a.get("name")).unwrap_or(&Value::from("Sound")))),
                        100,
                    ),
                    file: a["file"].as_str().unwrap().into(),
                    start: finite(a.get("start")).map_or(0.0, js_round),
                    gain: finite(a.get("gain")).map_or(1.0, |g| g.clamp(0.0, 16.0)),
                    muted: js::truthy(a.get("muted")),
                })
                .collect(),
            _ => Vec::new(),
        };
        let markers = match sc.get("markers").and_then(Value::as_array) {
            Some(m) if !m.is_empty() => m
                .iter()
                .filter_map(|m| {
                    let f =
                        integer(m.get("frame")).filter(|f| *f >= 0.0 && *f <= u32::MAX as f64)?;
                    Some(Marker {
                        frame: f as u32,
                        label: slice16(
                            &js::string(Some(or(m.get("label")).unwrap_or(&Value::from("")))),
                            100,
                        ),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        let cuts_in = sc.get("cuts").and_then(Value::as_array).unwrap_or(&empty);
        let mut cuts = Vec::new();
        for (ci, c) in cuts_in.iter().take(500).enumerate() {
            let mut tracks = BTreeMap::new();
            for (lid, t) in entries(or(c.get("tracks"))) {
                let (Some(id), Some(t)) = (id_of(&lid), t.as_array()) else {
                    continue;
                };
                tracks.insert(
                    id,
                    t.iter()
                        .take(5000)
                        .map(|e| Exposure {
                            drawing: drawing(e.get("d")),
                            length: integer(e.get("n")).map_or(1, |n| n.clamp(1.0, 999.0) as u32),
                        })
                        .collect(),
                );
            }
            // this app's own, unshared layers keep their tracks
            if let Some(mc) = mine.and_then(|m| m.cuts.get(ci)) {
                for (l, t) in &mc.tracks {
                    if !is_shared(*l) {
                        tracks.insert(*l, t.clone());
                    }
                }
            }
            let camera = c.get("camera").and_then(|cam| {
                let (x, y, w, h) = (
                    finite(cam.get("x"))?,
                    finite(cam.get("y"))?,
                    finite(cam.get("width"))?,
                    finite(cam.get("height"))?,
                );
                (w > 0.0 && h > 0.0).then_some(Camera {
                    x,
                    y,
                    width: w,
                    height: h,
                })
            });
            let transition = match c.get("transition").and_then(Value::as_str) {
                Some(t @ ("flash" | "dissolve")) => Some(t.to_string()),
                _ => None,
            };
            cuts.push(Cut {
                id: ci as u32 + 1,
                name: slice16(
                    &js::string(Some(
                        or(c.get("name")).unwrap_or(&Value::String(format!("Cut {}", ci + 1))),
                    )),
                    100,
                ),
                tracks,
                camera,
                action: notes(c.get("action")),
                dialogue: notes(c.get("dialogue")),
                transition,
            });
        }
        scenes.push(Scene {
            id: si as u32 + 1,
            name: slice16(&name, 100),
            cuts,
            audio,
            markers,
        });
    }
    let mut statics: Vec<u32> = s
        .get("statics")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| l.as_str().and_then(id_of))
                .collect()
        })
        .unwrap_or_default();
    for l in &local.statics {
        if !is_shared(*l) && !statics.contains(l) {
            statics.push(*l);
        }
    }
    let mut mouths = Vec::new();
    for m in s.get("mouths").and_then(Value::as_array).unwrap_or(&empty) {
        let Some(layer) = m.get("lid").and_then(Value::as_str).and_then(id_of) else {
            continue;
        };
        let mut shapes = BTreeMap::new();
        for (k, d) in entries(or(m.get("shapes"))) {
            if MOUTH_SHAPES.contains(&k.as_str()) {
                let d = drawing(Some(d));
                if d != 0 {
                    shapes.insert(k, d);
                }
            }
        }
        mouths.push(MouthChart { layer, shapes });
    }
    let mut kinds = BTreeMap::new();
    for (d, k) in entries(or(s.get("kinds"))) {
        if let Some(k) = k.as_str().filter(|k| DRAWING_KINDS.contains(k)) {
            let d = drawing(Some(&Value::String(d)));
            if d != 0 {
                kinds.insert(d, k.to_string());
            }
        }
    }
    let mut stages = BTreeMap::new();
    let sent = s.get("stages").filter(|v| js::truthy(Some(v)));
    if sent.is_some_and(|v| v.is_object() || v.is_array()) {
        for (lid, st) in entries(sent) {
            if let (Some(id), Some(st)) = (id_of(&lid), st.as_str())
                && STAGES.contains(&st)
            {
                stages.insert(id, st.to_string());
            }
        }
    }
    // this app's own, unshared layers keep their stages (all of them, from an app that
    // doesn't send any)
    for (l, st) in &local.stages {
        if !is_shared(*l) || sent.is_none() {
            stages.insert(*l, st.clone());
        }
    }
    // (an app that sends no palettes leaves this one's as they are)
    let palettes = match s.get("palettes") {
        None => local.palettes.clone(),
        p => read_palettes(p),
    };
    Timeline {
        scenes: if scenes.is_empty() {
            local.scenes.clone()
        } else {
            scenes
        },
        statics,
        mouths,
        kinds,
        stages,
        palettes,
    }
}
