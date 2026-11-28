//! together.test.ts's fakes: a canvas of layers with drawings of tiles, and a session
//! that numbers edits and hands them to every app, in order.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use pencilpilot_together::js;
use pencilpilot_together::lines::{LineStroke, STRIDE};
use pencilpilot_together::timeline::Palette;
use pencilpilot_together::timeline::{Cut, Exposure, Scene, Timeline};
use pencilpilot_together::{
    CanvasPort, GroupSettings, LayerPatch, LayerStyle, Look, Out, PortGroup, PortLayer, Strokes,
    TILE_BYTES, TileMap, Together, pack, unpack,
};
use serde_json::{Value, json};

pub const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

pub fn tile(v: u8) -> Vec<u8> {
    vec![v; TILE_BYTES]
}

#[derive(Debug, Clone)]
pub struct FakeLayer {
    pub id: u32,
    pub name: String,
    pub locked: bool,
    pub visible: bool,
    pub drawing: u32,
    pub opacity: f64,
    pub blend: String,
    pub adjust: Option<String>,
    pub mask: Option<TileMap>,
    pub group: u32,
    pub style: Option<LayerStyle>,
}

/// `newTimeline`: one scene, one cut, each layer showing its drawing for a frame.
pub fn new_timeline(layers: &[(u32, u32)]) -> Timeline {
    let mut tracks = BTreeMap::new();
    for (id, d) in layers {
        tracks.insert(
            *id,
            if *d != 0 {
                vec![Exposure {
                    drawing: *d,
                    length: 1,
                }]
            } else {
                vec![]
            },
        );
    }
    Timeline {
        scenes: vec![Scene {
            id: 1,
            name: "Scene 1".into(),
            cuts: vec![Cut {
                id: 1,
                name: "Cut 1".into(),
                tracks,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// `setStage`.
pub fn set_stage(tl: &mut Timeline, layer: u32, stage: &str) {
    tl.stages.insert(layer, stage.into());
}

/// `followTiming`: `to` timed as `from`, with new drawings for its drawings.
pub fn follow_timing(tl: &mut Timeline, from: u32, to: u32, make: &mut dyn FnMut() -> u32) {
    let mut made: HashMap<u32, u32> = HashMap::new();
    for s in &mut tl.scenes {
        for c in &mut s.cuts {
            let Some(t) = c.tracks.get(&from).cloned() else {
                continue;
            };
            let copy: Vec<Exposure> = t
                .iter()
                .map(|e| Exposure {
                    drawing: if e.drawing == 0 {
                        0
                    } else {
                        *made.entry(e.drawing).or_insert_with(&mut *make)
                    },
                    length: e.length,
                })
                .collect();
            c.tracks.insert(to, copy);
        }
    }
}

pub struct FakeCanvas {
    pub layer_list: Vec<FakeLayer>,
    pub group_list: Vec<PortGroup>,
    next_group: u32,
    pub drawings: HashMap<u32, TileMap>,
    pub selected: u32,
    pub busy: bool,
    next: u32,
    pub tl: Timeline,
    pub sounds: HashMap<String, (Arc<Vec<u8>>, String)>,
    next_drawing: u32,
    pub line_layers: HashSet<u32>,
    pub strokes: HashMap<u32, Strokes>,
    pub plain: HashSet<u32>,
    pub base_out: HashMap<u32, Vec<u8>>,
    pub base_full: HashMap<u32, Vec<u8>>,
    pub base_in: Vec<(u32, Vec<u8>)>,
}

impl FakeCanvas {
    pub fn new(names: &[&str]) -> Self {
        let mut c = FakeCanvas {
            layer_list: vec![],
            group_list: vec![],
            next_group: 1,
            drawings: HashMap::new(),
            selected: 0,
            busy: false,
            next: 1,
            tl: new_timeline(&[]),
            sounds: HashMap::new(),
            next_drawing: 5000,
            line_layers: HashSet::new(),
            strokes: HashMap::new(),
            plain: HashSet::new(),
            base_out: HashMap::new(),
            base_full: HashMap::new(),
            base_in: vec![],
        };
        for n in names {
            c.add_layer(n, None);
        }
        c
    }

    fn layer(&mut self, id: u32) -> &mut FakeLayer {
        self.layer_list.iter_mut().find(|l| l.id == id).unwrap()
    }

    pub fn by_name(&self, name: &str) -> &FakeLayer {
        self.layer_list
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no layer {name}"))
    }

    pub fn by_name_mut(&mut self, name: &str) -> &mut FakeLayer {
        self.layer_list.iter_mut().find(|l| l.name == name).unwrap()
    }

    pub fn has(&self, name: &str) -> bool {
        self.layer_list.iter().any(|l| l.name == name)
    }

    pub fn index_of(&self, id: u32) -> usize {
        self.layer_list.iter().position(|l| l.id == id).unwrap()
    }

    pub fn names(&self) -> Vec<String> {
        self.layer_list.iter().map(|l| l.name.clone()).collect()
    }

    pub fn visible_names(&self) -> Vec<String> {
        self.layer_list
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.name.clone())
            .collect()
    }

    /// `moveNextTo`, as the fake does it (splice with -1 too).
    pub fn move_next_to(&mut self, id: u32, reference: u32, above: bool) -> bool {
        if self.busy {
            return false;
        }
        let i = self.index_of(id);
        let l = self.layer_list.remove(i);
        let r = self.layer_list.iter().position(|x| x.id == reference);
        let len = self.layer_list.len();
        let at = match (r, above) {
            (Some(r), true) => r + 1,
            (Some(r), false) => r,
            (None, true) => 0,
            (None, false) => len.saturating_sub(1),
        };
        self.layer_list.insert(at, l);
        true
    }

    /// Draw: set a tile on a layer, if it isn't locked.
    pub fn paint(&mut self, id: u32, x: i32, y: i32, v: u8) -> bool {
        if self.layer(id).locked {
            return false;
        }
        let d = self.drawing_of(id);
        self.drawings.get_mut(&d).unwrap().insert((x, y), tile(v));
        true
    }

    pub fn pixel(&self, id: u32, x: i32, y: i32) -> Option<u8> {
        let l = self.layer_list.iter().find(|l| l.id == id)?;
        self.drawings.get(&l.drawing)?.get(&(x, y)).map(|t| t[0])
    }

    /// Show the drawing on frame `f` of the first cut.
    pub fn show_frame(&mut self, f: u32) {
        let tracks = self
            .tl
            .scenes
            .first()
            .and_then(|s| s.cuts.first())
            .map(|c| c.tracks.clone())
            .unwrap_or_default();
        for l in &mut self.layer_list {
            let mut at = 0;
            for e in tracks.get(&l.id).map(|t| &t[..]).unwrap_or(&[]) {
                if f < at + e.length {
                    l.drawing = e.drawing;
                    break;
                }
                at += e.length;
            }
        }
    }

    pub fn track(&self, id: u32) -> Vec<Exposure> {
        self.tl.scenes[0].cuts[0]
            .tracks
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_group_settings(&mut self, id: u32, s: GroupSettings) {
        self.set_group(id, &s);
    }

    pub fn set_strokes(&mut self, d: u32, list: Vec<LineStroke>, plain: bool) {
        self.set_line_strokes(d, Arc::new(list), plain);
    }

    pub fn stroke_ids(&self, d: u32) -> Vec<String> {
        self.line_strokes(d).iter().map(|s| s.id.clone()).collect()
    }
}

impl CanvasPort for FakeCanvas {
    fn add_layer(&mut self, name: &str, adjust: Option<&str>) -> u32 {
        let id = self.next;
        self.next += 1;
        self.layer_list.push(FakeLayer {
            id,
            name: name.into(),
            locked: false,
            visible: true,
            drawing: 0,
            opacity: 1.0,
            blend: "normal".into(),
            adjust: adjust.map(str::to_string),
            mask: None,
            group: 0,
            style: None,
        });
        id
    }
    fn set_adjust(&mut self, id: u32, spec: &str) -> bool {
        if self.busy {
            return false;
        }
        self.layer(id).adjust = Some(spec.into());
        true
    }
    fn mask_packed(&mut self, id: u32) -> Vec<u8> {
        self.layer(id).mask.as_ref().map(pack).unwrap_or_default()
    }
    fn put_mask(&mut self, id: u32, packed: &[u8]) -> bool {
        if self.busy {
            return false;
        }
        self.layer(id).mask = (!packed.is_empty()).then(|| unpack(packed));
        true
    }
    fn set_layer(&mut self, id: u32, patch: LayerPatch) {
        let l = self.layer(id);
        if let Some(n) = patch.name {
            l.name = n;
        }
        if let Some(v) = patch.locked {
            l.locked = v;
        }
        if let Some(v) = patch.visible {
            l.visible = v;
        }
        if let Some(look) = patch.look {
            l.visible = look.visible;
            l.opacity = look.opacity;
            l.blend = look.blend;
            if let Some(s) = look.style {
                l.style = s;
            }
        }
    }
    /// (as the page does: drawing on an empty cel starts a drawing, on the timeline there)
    fn drawing_of(&mut self, id: u32) -> u32 {
        let l = self.layer(id);
        if l.drawing == 0 {
            l.drawing = 1000 + id;
            let d = l.drawing;
            self.drawings.insert(d, TileMap::new());
            let cut = &mut self.tl.scenes[0].cuts[0];
            if cut.tracks.get(&id).is_none_or(|t| t.is_empty()) {
                cut.tracks.insert(
                    id,
                    vec![Exposure {
                        drawing: d,
                        length: 1,
                    }],
                );
            }
        }
        self.layer(id).drawing
    }
    fn new_drawing(&mut self) -> u32 {
        let d = self.next_drawing;
        self.next_drawing += 1;
        self.drawings.insert(d, TileMap::new());
        d
    }
    fn layer_of_drawing(&self, d: u32) -> Option<u32> {
        if let Some(l) = self.layer_list.iter().find(|l| l.drawing == d) {
            return Some(l.id);
        }
        for sc in &self.tl.scenes {
            for c in &sc.cuts {
                for (l, t) in &c.tracks {
                    if t.iter().any(|e| e.drawing == d) {
                        return Some(*l);
                    }
                }
            }
        }
        None
    }
    fn packed(&mut self, d: u32) -> Vec<u8> {
        pack(self.drawings.get(&d).unwrap_or(&TileMap::new()))
    }
    fn put(&mut self, d: u32, p: &[u8]) -> bool {
        if self.busy {
            return false;
        }
        self.drawings.insert(d, unpack(p));
        true
    }
    fn select(&mut self, id: u32) {
        self.selected = id;
    }
    fn remove_layer(&mut self, id: u32) -> bool {
        if self.busy {
            return false;
        }
        self.layer_list.retain(|l| l.id != id);
        true
    }
    fn place(&mut self, id: u32, reference: u32, above: bool, group: u32) -> bool {
        if self.busy {
            return false;
        }
        if reference != 0 && reference != id {
            self.move_next_to(id, reference, above);
        }
        let has = self.group_list.iter().any(|g| g.id == group);
        if let Some(l) = self.layer_list.iter_mut().find(|x| x.id == id) {
            l.group = if has { group } else { 0 };
        }
        true
    }
    fn add_group(&mut self, layer: u32) -> u32 {
        if self.busy {
            return 0;
        }
        let id = self.next_group;
        self.next_group += 1;
        self.group_list.push(PortGroup {
            id,
            settings: GroupSettings {
                name: format!("Group {id}"),
                visible: true,
                opacity: 1.0,
            },
        });
        self.layer(layer).group = id;
        id
    }
    fn set_group(&mut self, id: u32, s: &GroupSettings) {
        if let Some(g) = self.group_list.iter_mut().find(|g| g.id == id) {
            g.settings = s.clone();
        }
    }
    fn ungroup(&mut self, id: u32) -> bool {
        if self.busy {
            return false;
        }
        for l in &mut self.layer_list {
            if l.group == id {
                l.group = 0;
            }
        }
        self.group_list.retain(|g| g.id != id);
        true
    }
    /// (the upper layer's tiles over the lower's)
    fn merge_down(&mut self, id: u32) -> bool {
        let i = self.index_of(id);
        if self.busy || i < 1 {
            return false;
        }
        let (lower, upper) = (self.layer_list[i - 1].id, self.layer_list[i].id);
        let into = self.drawing_of(lower);
        let from = self.drawing_of(upper);
        let px: Vec<_> = self
            .drawings
            .get(&from)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let t = self.drawings.get_mut(&into).unwrap();
        for (k, v) in px {
            t.insert(k, v);
        }
        self.layer_list.remove(i);
        true
    }
    fn merge_group(&mut self, id: u32) -> u32 {
        let m: Vec<u32> = self
            .layer_list
            .iter()
            .filter(|l| l.group == id)
            .map(|l| l.id)
            .collect();
        let Some(g) = self.group_list.iter().find(|g| g.id == id).cloned() else {
            return 0;
        };
        if self.busy || m.is_empty() {
            return 0;
        }
        let into = self.drawing_of(m[0]);
        for l in &m[1..] {
            let from = self.drawing_of(*l);
            let px: Vec<_> = self
                .drawings
                .get(&from)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            let t = self.drawings.get_mut(&into).unwrap();
            for (k, v) in px {
                t.insert(k, v);
            }
        }
        self.layer_list.retain(|l| !m[1..].contains(&l.id));
        let first = self.layer(m[0]);
        first.name = g.settings.name.clone();
        first.group = 0;
        first.opacity = g.settings.opacity;
        self.group_list.retain(|x| x.id != id);
        m[0]
    }
    fn groups(&self) -> Vec<PortGroup> {
        self.group_list.clone()
    }
    fn layers(&self) -> Vec<PortLayer> {
        self.layer_list
            .iter()
            .map(|l| PortLayer {
                id: l.id,
                name: l.name.clone(),
                adjust: l.adjust.clone(),
                group: l.group,
                look: Some(Look {
                    visible: l.visible,
                    opacity: l.opacity,
                    blend: l.blend.clone(),
                    alpha_lock: false,
                    clip: false,
                    style: Some(l.style.clone()),
                }),
            })
            .collect()
    }
    fn sound(&self, file: &str) -> Option<(Arc<Vec<u8>>, String)> {
        self.sounds.get(file).cloned()
    }
    fn put_sound(&mut self, file: &str, bytes: Vec<u8>, name: &str) {
        self.sounds
            .insert(file.into(), (Arc::new(bytes), name.into()));
    }
    fn timeline(&self) -> Timeline {
        self.tl.clone()
    }
    /// (as the page does: the frame is shown again)
    fn set_timeline(&mut self, tl: Timeline) {
        self.tl = tl;
        self.show_frame(0);
    }
    fn drawings_of_layer(&self, id: u32) -> Vec<u32> {
        self.tl
            .scenes
            .iter()
            .flat_map(|sc| &sc.cuts)
            .flat_map(|c| c.tracks.get(&id).map(|t| &t[..]).unwrap_or(&[]))
            .map(|e| e.drawing)
            .filter(|d| *d != 0)
            .collect()
    }
    fn has_lines(&self) -> bool {
        true
    }
    fn is_line_layer(&self, id: u32) -> bool {
        self.line_layers.contains(&id)
    }
    fn set_line_layer(&mut self, id: u32, on: bool) {
        if on {
            self.line_layers.insert(id);
        } else {
            self.line_layers.remove(&id);
        }
    }
    fn line_strokes(&self, d: u32) -> Strokes {
        self.strokes.get(&d).cloned().unwrap_or_default()
    }
    fn set_line_strokes(&mut self, d: u32, list: Strokes, plain: bool) {
        self.strokes.insert(d, list);
        if plain {
            self.plain.insert(d);
        }
    }
    fn plain_strokes(&self, d: u32) -> bool {
        self.plain.contains(&d)
    }
    fn base_changes(&mut self, d: u32) -> Option<Vec<u8>> {
        self.base_out.remove(&d)
    }
    fn base_all(&mut self, d: u32) -> Option<Vec<u8>> {
        self.base_full.get(&d).cloned()
    }
    fn put_base_changes(&mut self, d: u32, bytes: &[u8]) {
        self.base_in.push((d, bytes.to_vec()));
    }
}

/// A packed base (as the engine packs it): `keys` there (clear), `gone` keys gone.
pub fn base_pack(keys: &[(i32, i32)], gone: &[(i32, i32)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for (x, y) in keys {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(gone.len() as u32).to_le_bytes());
    for (x, y) in gone {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }
    out
}

/// A line stroke from (x, 0) to (x, 10).
pub fn stroke(id: &str, x: f32) -> LineStroke {
    let mut pts = Vec::with_capacity(3 * STRIDE);
    for i in 0..3 {
        pts.extend_from_slice(&[x, i as f32 * 5.0, 0.5, 0.0, 0.0, 0.01]);
    }
    LineStroke {
        id: id.into(),
        pts,
        brush: "pen".into(),
        color: "#000000".into(),
        size: 1.0,
        opacity: 1.0,
        radius: 2.0,
        seed: 7,
        texture: None,
        shape: None,
        link: None,
        reach: None,
        smudge: false,
        erase: false,
        span: None,
        dabs: None,
        clip: vec![],
    }
}

/// `transformStroke` by a move.
pub fn moved(s: &LineStroke, dx: f32, dy: f32) -> LineStroke {
    let mut s = s.clone();
    for p in s.pts.chunks_mut(STRIDE) {
        p[0] += dx;
        p[1] += dy;
    }
    s
}

pub struct App {
    pub t: Together,
    pub c: FakeCanvas,
}

enum Delivery {
    Edit {
        to: String,
        seq: u64,
        from: String,
        body: Value,
    },
    Direct {
        to: String,
        from: String,
        body: Value,
    },
}

/// A session: numbers edits and hands them to every app, in order.
#[derive(Default)]
pub struct FakeSession {
    pub seq: u64,
    pub apps: BTreeMap<String, App>,
    /// the order apps joined (deliveries go to each, as the TS fake's Map does)
    joined: Vec<String>,
    queue: VecDeque<Delivery>,
    /// every edit numbered, in order: (from, body)
    pub log: Vec<(String, Value)>,
    /// every direct message: (from, to, body)
    pub directs: Vec<(String, String, Value)>,
}

impl FakeSession {
    pub fn add(&mut self, me: &str, name: &str, c: FakeCanvas) {
        self.add_app(me, Together::with_start(me, name, 0), c);
    }

    pub fn add_app(&mut self, me: &str, t: Together, c: FakeCanvas) {
        self.apps.insert(me.into(), App { t, c });
        self.joined.retain(|x| x != me);
        self.joined.push(me.into());
    }

    pub fn remove(&mut self, me: &str) -> App {
        self.joined.retain(|x| x != me);
        self.apps.remove(me).unwrap()
    }

    pub fn c(&mut self, me: &str) -> &mut FakeCanvas {
        &mut self.apps.get_mut(me).unwrap().c
    }

    pub fn t(&mut self, me: &str) -> &mut Together {
        &mut self.apps.get_mut(me).unwrap().t
    }

    /// Call `f` on an app, and send what it queued.
    pub fn with<R>(&mut self, me: &str, f: impl FnOnce(&mut Together, &mut FakeCanvas) -> R) -> R {
        let app = self.apps.get_mut(me).unwrap();
        let r = f(&mut app.t, &mut app.c);
        self.flush(me);
        r
    }

    /// Send an edit as if from `me`'s app (forged or not).
    pub fn send_edit(&mut self, me: &str, body: Value) {
        self.number(me, body);
    }

    fn number(&mut self, from: &str, body: Value) {
        self.seq += 1;
        self.log.push((from.into(), body.clone()));
        for to in &self.joined {
            self.queue.push_back(Delivery::Edit {
                to: to.clone(),
                seq: self.seq,
                from: from.into(),
                body: body.clone(),
            });
        }
    }

    pub fn flush(&mut self, me: &str) {
        let Some(app) = self.apps.get_mut(me) else {
            return;
        };
        for o in app.t.take_out() {
            match o {
                Out::Edit(body) => self.number(me, body),
                Out::Direct { to, body } => {
                    self.directs.push((me.into(), to.clone(), body.clone()));
                    self.queue.push_back(Delivery::Direct {
                        to,
                        from: me.into(),
                        body,
                    })
                }
            }
        }
    }

    /// Hand on up to `n` queued messages.
    pub fn step(&mut self, n: usize) {
        for _ in 0..n {
            let Some(d) = self.queue.pop_front() else {
                return;
            };
            let (to, from, body, seq) = match d {
                Delivery::Edit {
                    to,
                    seq,
                    from,
                    body,
                } => (to, from, body, Some(seq)),
                Delivery::Direct { to, from, body } => (to, from, body, None),
            };
            let Some(app) = self.apps.get_mut(&to) else {
                continue;
            };
            match seq {
                Some(seq) => app.t.on_edit(&mut app.c, seq, &from, &body),
                None => app.t.on_direct(&mut app.c, &from, &body),
            }
            self.flush(&to);
        }
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn settle(&mut self) {
        while !self.queue.is_empty() {
            self.step(1);
        }
    }
}

// ---- timelines and states as JSON, as record.ts writes them ----

pub fn timeline_json(tl: &Timeline) -> Value {
    let notes = |n: &BTreeMap<u32, String>| -> Value {
        Value::Object(
            n.iter()
                .map(|(k, v)| (k.to_string(), Value::from(v.clone())))
                .collect(),
        )
    };
    let palettes: Vec<Value> = tl.palettes.iter().map(palette_json).collect();
    json!({
        "scenes": tl.scenes.iter().map(|sc| json!({
            "id": sc.id,
            "name": sc.name,
            "audio": sc.audio.iter().map(|a| json!({
                "id": a.id, "name": a.name, "file": a.file, "start": js::num(a.start), "gain": js::num(a.gain), "muted": a.muted
            })).collect::<Vec<_>>(),
            "markers": sc.markers.iter().map(|m| json!({"frame": m.frame, "label": m.label})).collect::<Vec<_>>(),
            "cuts": sc.cuts.iter().map(|c| {
                let mut v = json!({
                    "id": c.id,
                    "name": c.name,
                    "tracks": Value::Object(c.tracks.iter().map(|(l, t)| (l.to_string(), t.iter().map(|e| json!({"drawing": e.drawing, "length": e.length})).collect())).collect()),
                    "action": notes(&c.action),
                    "dialogue": notes(&c.dialogue),
                });
                if let Some(cam) = &c.camera {
                    v["camera"] = json!({"x": js::num(cam.x), "y": js::num(cam.y), "width": js::num(cam.width), "height": js::num(cam.height)});
                }
                if let Some(t) = &c.transition {
                    v["transition"] = t.clone().into();
                }
                v
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "statics": tl.statics,
        "mouths": tl.mouths.iter().map(|m| json!({"layer": m.layer, "shapes": m.shapes})).collect::<Vec<_>>(),
        "kinds": Value::Object(tl.kinds.iter().map(|(d, k)| (d.to_string(), Value::from(k.clone()))).collect()),
        "stages": Value::Object(tl.stages.iter().map(|(d, k)| (d.to_string(), Value::from(k.clone()))).collect()),
        "palettes": palettes,
    })
}

fn palette_json(p: &Palette) -> Value {
    json!({"name": p.name, "colours": p.colours.iter().map(|c| json!({"name": c.name, "hex": c.hex})).collect::<Vec<_>>()})
}

/// Keys the studio leaves out when there's nothing in them (and we write empty): out.
pub fn normalise(v: &mut Value) {
    const OPTIONAL: [&str; 8] = [
        "audio", "markers", "mouths", "kinds", "stages", "palettes", "action", "dialogue",
    ];
    match v {
        Value::Object(m) => {
            m.retain(|k, x| {
                !(OPTIONAL.contains(&k.as_str())
                    && (x.is_null()
                        || x.as_array().is_some_and(|a| a.is_empty())
                        || x.as_object().is_some_and(|o| o.is_empty())))
            });
            m.values_mut().for_each(normalise);
        }
        Value::Array(a) => a.iter_mut().for_each(normalise),
        _ => {}
    }
}
