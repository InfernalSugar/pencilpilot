//! The canvas's answers to the session's thread ([`super::port`]), on the window's thread
//! between frames: each is quick (layers made, moved, drawings swapped in, tiles patched,
//! the timeline put in); anything heavy was done on the other thread.
//!
//! - **The timeline:** the session's is kept whole as it came ([`Window::full_tl`]); the cut
//!   this app shows (the first) is the sheet's, and only its tracks are read back from here,
//!   so every scene, cut, marker, sound clip and note it doesn't show goes round unchanged.
//! - **Looks:** a layer's look as the session set it is kept, and given back with only what
//!   can change here (its eye, opacity, blend where this app has it), so a blend mode or
//!   style this app doesn't have is never rewritten.
//! - **Tiles:** others' tiles are kept exactly as they came for saving and passing on
//!   ([`super::super::frames::store::Store::exact`]); a drawing not in the painter is kept
//!   compressed as it came.

use std::collections::HashMap;
use std::sync::Arc;

use pencilpilot_together::timeline::{Cut, Exposure, Scene};
use pencilpilot_together::{GroupSettings, LayerPatch, Look, PortGroup, PortLayer, Timeline};

use super::super::Engine;
use super::super::frames::store::Kept;
use super::port::{Pixels, Req, Resp};
use crate::state::State;

/// The window's side of the canvas the session sees.
#[derive(Default)]
pub struct Window {
    /// the session's timeline as it was last put in (all of it)
    pub full_tl: Option<Timeline>,
    /// each layer's look as the session set it
    looks: HashMap<u32, Look>,
    /// adjustment layers made for the session (their settings, kept as they came)
    adjusts: HashMap<u32, String>,
    /// masks as the session put them (packed RGBA)
    masks: HashMap<u32, Vec<u8>>,
    /// the sheet as it was after the last timeline put in or sent (to tell a change here)
    pub sheet_sig: u64,
    /// the scene and cut this app shows (the sheet), in the session's timeline
    pub shown: (usize, usize),
}

impl Window {
    pub fn is_adjust(&self, id: u32) -> bool {
        self.adjusts.contains_key(&id)
    }
}

/// A sign of the sheet's tracks (the layers' exposures and marks): it changes when the
/// timeline does.
pub fn sheet_sig(e: &Engine, s: &State) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for t in &s.sheet.tracks {
        if e.frames.previews.contains(&t.layer) {
            continue;
        }
        (t.layer, t.kind).hash(&mut h);
        for c in &t.cels {
            (c.drawing, c.hold, c.mark as u8).hash(&mut h);
        }
    }
    h.finish()
}

fn mark_name(m: super::super::frames::Mark) -> Option<&'static str> {
    use super::super::frames::Mark;
    match m {
        Mark::None => None,
        Mark::Key => Some("key"),
        Mark::Breakdown => Some("breakdown"),
        Mark::InBetween => Some("inbetween"),
    }
}

fn mark_of(k: &str) -> super::super::frames::Mark {
    use super::super::frames::Mark;
    match k {
        "key" => Mark::Key,
        "breakdown" => Mark::Breakdown,
        "inbetween" => Mark::InBetween,
        _ => Mark::None,
    }
}

/// The timeline as the session sees this app's: the session's, with the cut this app
/// shows read from the sheet.
/// Hosting a project from its file: the session's timeline is the whole of its, every
/// scene and cut.
fn ensure_full(e: &mut Engine, s: &State) {
    if e.together.win.full_tl.is_none()
        && let Some((tl, shown)) = from_file(e, s)
    {
        e.together.win.full_tl = Some(tl);
        e.together.win.shown = shown;
    }
}

fn timeline_of(e: &mut Engine, s: &State) -> Timeline {
    ensure_full(e, s);
    let mut tl = e.together.win.full_tl.clone().unwrap_or_default();
    let (si, ci) = e.together.win.shown;
    while tl.scenes.len() <= si {
        let n = tl.scenes.len() as u32 + 1;
        tl.scenes.push(Scene {
            id: n,
            name: s.project.scene.clone(),
            ..Default::default()
        });
    }
    while tl.scenes[si].cuts.len() <= ci {
        let n = tl.scenes[si].cuts.len() as u32 + 1;
        tl.scenes[si].cuts.push(Cut {
            id: n,
            name: s.cut_name.clone(),
            ..Default::default()
        });
    }
    let cut = &mut tl.scenes[si].cuts[ci];
    for t in &s.sheet.tracks {
        if t.layer == 0 || e.frames.previews.contains(&t.layer) {
            continue;
        }
        let ex: Vec<Exposure> = t
            .cels
            .iter()
            .map(|c| Exposure {
                drawing: c.drawing,
                length: c.hold.max(1),
            })
            .collect();
        // a static layer's one drawing shows all cut long here: the session's own length
        // for it stays
        let keep = t.kind == 'S'
            && ex.len() == 1
            && cut
                .tracks
                .get(&t.layer)
                .is_some_and(|w| w.len() == 1 && w[0].drawing == ex[0].drawing);
        // (nothing on it, and the session has no track for it: none)
        let none = ex.is_empty() && !cut.tracks.contains_key(&t.layer);
        if !keep && !none {
            cut.tracks.insert(t.layer, ex);
        }
        let is_static = tl.statics.contains(&t.layer);
        if t.kind == 'S' && !is_static {
            tl.statics.push(t.layer);
        } else if t.kind != 'S' && is_static {
            tl.statics.retain(|l| *l != t.layer);
        }
        for c in &t.cels {
            if c.drawing == 0 {
                continue;
            }
            match mark_name(c.mark) {
                Some(k) => {
                    tl.kinds.insert(c.drawing, k.to_string());
                }
                None => {
                    tl.kinds.remove(&c.drawing);
                }
            }
        }
    }
    tl
}

/// A project's whole timeline as its file has it (every scene and cut), in this app's ids:
/// the drawings of the cuts it doesn't show are given ids here and kept as the file has
/// them (to go to joiners as they are). None when the project has no file, or no scenes.
fn from_file(e: &mut Engine, s: &State) -> Option<(Timeline, (usize, usize))> {
    use pencilpilot_together::timeline::{
        AudioClip, Camera, Marker, MouthChart, Palette, PaletteColour,
    };
    let m = e.files.doc.base.clone()?;
    if m.timeline.scenes.is_empty() {
        return None;
    }
    let (si, ci) = (e.files.doc.scene, e.files.doc.cut);
    let open = m.timeline.scenes.get(si)?.cuts.get(ci)?.clone();
    // file layer → painter layer
    let pl: HashMap<u32, u32> = e
        .files
        .doc
        .layer_ids
        .iter()
        .map(|(p, f)| (*f, *p))
        .collect();
    // the open cut's drawings: their names → the painter's ids (the sheet, in order)
    let mut ids: HashMap<(u32, String), u32> = HashMap::new();
    for (fl, ex) in &open.tracks {
        let Some(p) = pl.get(fl) else { continue };
        let Some(t) = s.sheet.tracks.iter().find(|t| t.layer == *p) else {
            continue;
        };
        let named: Vec<&String> = ex.iter().filter_map(|x| x.drawing.as_ref()).collect();
        let drawn: Vec<u32> = t
            .cels
            .iter()
            .map(|c| c.drawing)
            .filter(|d| *d != 0)
            .collect();
        for (n, d) in named.iter().zip(&drawn) {
            ids.entry((*fl, (*n).clone())).or_insert(*d);
        }
    }
    // the rest: new ids, kept as the file has them
    let mut need: Vec<String> = Vec::new();
    let mut id_for = |e: &mut Engine, fl: u32, name: &str| -> u32 {
        if let Some(d) = ids.get(&(fl, name.to_string())) {
            return *d;
        }
        let Some(dr) = m.drawings.get(name) else {
            return 0;
        };
        let d = e.painter.new_drawing();
        let refs: Vec<(i32, i32, String)> = dr
            .tiles
            .iter()
            .map(|r| (r.x as i32, r.y as i32, r.hash.clone()))
            .collect();
        need.extend(refs.iter().map(|r| r.2.clone()));
        e.frames
            .store
            .drawings
            .insert(d, Arc::new(Kept::File(refs)));
        ids.insert((fl, name.to_string()), d);
        d
    };
    let mut tl = Timeline::default();
    for (i, sc) in m.timeline.scenes.iter().enumerate() {
        let mut scene = Scene {
            id: i as u32 + 1,
            name: sc.name.clone(),
            cuts: Vec::new(),
            audio: sc
                .audio
                .iter()
                .enumerate()
                .map(|(k, a)| AudioClip {
                    id: k as u32 + 1,
                    name: a.name.clone(),
                    file: a.file.clone(),
                    start: a.start as f64,
                    gain: a.gain as f64,
                    muted: a.muted,
                })
                .collect(),
            markers: sc
                .markers
                .iter()
                .map(|x| Marker {
                    frame: x.frame,
                    label: x.label.clone(),
                })
                .collect(),
        };
        for (j, c) in sc.cuts.iter().enumerate() {
            let mut cut = Cut {
                id: j as u32 + 1,
                name: c.name.clone(),
                camera: c.camera.map(|k| Camera {
                    x: k.x,
                    y: k.y,
                    width: k.width,
                    height: k.height,
                }),
                action: c.action.clone(),
                dialogue: c.dialogue.clone(),
                transition: c.transition.clone(),
                ..Default::default()
            };
            for (fl, ex) in &c.tracks {
                let Some(p) = pl.get(fl) else { continue };
                let v: Vec<Exposure> = ex
                    .iter()
                    .map(|x| Exposure {
                        drawing: x.drawing.as_deref().map_or(0, |n| id_for(e, *fl, n)),
                        length: x.length.max(1),
                    })
                    .collect();
                cut.tracks.insert(*p, v);
            }
            scene.cuts.push(cut);
        }
        tl.scenes.push(scene);
    }
    tl.statics = m
        .timeline
        .static_layers
        .iter()
        .filter_map(|f| pl.get(f).copied())
        .collect();
    tl.mouths = m
        .timeline
        .mouth_charts
        .iter()
        .filter_map(|c| {
            let p = *pl.get(&c.layer)?;
            let shapes = c
                .shapes
                .iter()
                .map(|(k, n)| (k.clone(), id_for(e, c.layer, n)))
                .filter(|(_, d)| *d != 0)
                .collect();
            Some(MouthChart { layer: p, shapes })
        })
        .collect();
    for ((_, name), d) in &ids {
        if let Some(k) = m.timeline.drawing_kinds.get(name) {
            tl.kinds.insert(*d, k.clone());
        }
    }
    for (f, st) in &m.timeline.layer_stages {
        if let Some(p) = f.parse::<u32>().ok().and_then(|f| pl.get(&f)) {
            tl.stages.insert(*p, st.clone());
        }
    }
    tl.palettes = m
        .timeline
        .palettes
        .iter()
        .map(|p| Palette {
            name: p.name.clone(),
            colours: p
                .colours
                .iter()
                .map(|c| PaletteColour {
                    name: c.name.clone(),
                    hex: c.hex.clone(),
                })
                .collect(),
        })
        .collect();
    // the tiles of the drawings just kept, read from the file (once)
    need.sort_unstable();
    need.dedup();
    need.retain(|h| !e.frames.store.file.contains_key(h));
    if !need.is_empty()
        && let Some(path) = &s.project.path
        && let Ok(bytes) = std::fs::read(path)
        && let Ok(o) = ppstudio::Opened::read(std::io::Cursor::new(bytes))
    {
        let mut t = o.tiles();
        for h in need {
            if let Ok((deflated, b)) = t.tile_stored(&h) {
                e.frames.store.file.insert(
                    h,
                    Arc::new(super::super::frames::store::FileTile {
                        deflated,
                        bytes: b.into_boxed_slice(),
                    }),
                );
            }
        }
    }
    Some((tl, (si, ci)))
}

/// The session's timeline put in: kept whole; the cut this app shows goes to the sheet.
fn put_timeline(e: &mut Engine, s: &mut State, tl: Timeline) {
    super::super::frames::catch_up(e, s);
    let shared: std::collections::HashSet<u32> =
        e.together.view.ids.iter().map(|(_, v)| *v).collect();
    let (si, ci) = e.together.win.shown;
    if let Some(cut) = tl.scenes.get(si).and_then(|sc| sc.cuts.get(ci)) {
        let none = Vec::new();
        for t in s.sheet.tracks.iter_mut() {
            // (a shared layer the cut has no track for has nothing on it; this app's own
            // keep theirs)
            let ex = match cut.tracks.get(&t.layer) {
                Some(ex) => ex,
                None if shared.contains(&t.layer) => &none,
                None => continue,
            };
            t.cels = ex
                .iter()
                .take(5000)
                .map(|x| {
                    let mark = tl
                        .kinds
                        .get(&x.drawing)
                        .map_or(super::super::frames::Mark::None, |k| mark_of(k));
                    super::super::frames::Cel::new(x.drawing, x.length.clamp(1, 999), mark)
                })
                .collect();
            t.kind = if tl.statics.contains(&t.layer) {
                'S'
            } else {
                'C'
            };
        }
        for (i, t) in s.sheet.tracks.iter().enumerate() {
            if let Some(l) = s.layers.get_mut(i) {
                l.kind = t.kind;
            }
        }
    }
    e.together.win.full_tl = Some(tl);
    super::super::frames::derive(s);
    super::super::frames::sheet_changed(e, s);
    e.together.win.sheet_sig = sheet_sig(e, s);
}

/// A layer's look: as the session set it, with what can change here.
fn look_of(e: &Engine, id: u32) -> Option<Look> {
    let l = e.painter.layers().iter().find(|l| l.id == id)?;
    let p = &l.props;
    let mut look = e.together.win.looks.get(&id).cloned().unwrap_or(Look {
        visible: true,
        opacity: 1.0,
        blend: "normal".into(),
        alpha_lock: false,
        clip: false,
        style: None,
    });
    look.visible = p.visible;
    if (look.opacity - p.opacity as f64).abs() > 1e-3 {
        look.opacity = p.opacity as f64;
    }
    look.alpha_lock = p.alpha_lock;
    look.clip = p.clip;
    // (a blend mode this app hasn't stays as it came)
    if brush::Blend::parse(&look.blend).is_some() {
        look.blend = p.blend.as_str().into();
    }
    Some(look)
}

/// Changes a layer's settings (not an undo step).
fn set_props(e: &mut Engine, id: u32, f: impl FnOnce(&mut brush::LayerProps)) {
    if let Some(l) = e.painter.layers().iter().find(|l| l.id == id) {
        let mut p = l.props.clone();
        f(&mut p);
        if p != l.props {
            e.painter.set_layer_props(id, p, false);
        }
    }
}

/// The layers following the painter now (a layer made, moved or gone).
fn layers_changed(e: &mut Engine, s: &mut State) {
    e.painter.clear_history();
    super::super::frames::sheet_changed(e, s);
    e.together.ui_dirty = true;
}

/// Answers one of the session thread's questions.
pub fn serve(e: &mut Engine, s: &mut State, r: Req) -> Resp {
    match r {
        Req::AddLayer(name, adjust) => {
            let id = e.painter.add_layer_silent(&name);
            if id != 0 {
                if let Some(a) = adjust {
                    e.together.win.adjusts.insert(id, a);
                }
                layers_changed(e, s);
            }
            Resp::U32(id)
        }
        Req::SetAdjust(id, spec) => {
            e.together.win.adjusts.insert(id, spec);
            Resp::Bool(true)
        }
        Req::MaskPacked(id) => Resp::Bytes(
            e.together
                .win
                .masks
                .get(&id)
                .cloned()
                .unwrap_or_else(|| e.painter.mask_tiles_packed(id)),
        ),
        Req::PutMask(id, packed) => {
            let ok = e.painter.put_mask_tiles(id, &packed);
            if ok {
                e.frames.gpu.forget_mask(id);
                if packed.is_empty() {
                    e.together.win.masks.remove(&id);
                } else {
                    e.together.win.masks.insert(id, packed);
                }
            }
            Resp::Bool(ok)
        }
        Req::SetLayer(id, patch) => {
            set_layer(e, id, patch);
            // (the interface's layers follow)
            super::super::frames::sheet_changed(e, s);
            e.together.ui_dirty = true;
            Resp::Unit
        }
        Req::DrawingOf(id) => {
            super::super::frames::catch_up(e, s);
            Resp::U32(e.painter.layer_drawing(id))
        }
        Req::NewDrawing => Resp::U32(e.painter.new_drawing()),
        Req::LayerOfDrawing(d) => Resp::OptU32(layer_of_drawing(e, s, d)),
        Req::Packed(d) => {
            let exact: Vec<((i32, i32), Arc<_>)> = e
                .frames
                .store
                .exact
                .get(&d)
                .map(|m| m.iter().map(|(k, t)| (*k, t.clone())).collect())
                .unwrap_or_default();
            // (kept compressed, unchanged since: as kept, nothing rounded)
            let px = if let Some(job) = e.frames.store.job(d) {
                Pixels::Kept(job)
            } else if e.painter.has_drawing(d) {
                let keys = e.painter.drawing_tile_keys(d);
                let mut spare = brush::TileBuffers::default();
                Pixels::Painter(
                    e.painter
                        .drawing_copy_tiles(d, &keys, &mut spare)
                        .into_decoded(),
                )
            } else {
                Pixels::None
            };
            Resp::Pixels(px, exact)
        }
        Req::Put {
            d,
            set,
            clear,
            stamp,
        } => Resp::Bool(put(e, s, d, set, clear, stamp)),
        Req::Select(id) => {
            super::super::frames::catch_up(e, s);
            e.painter.set_active_layer(id);
            super::super::frames::sheet_changed(e, s);
            if let Some(i) = s.sheet.tracks.iter().position(|t| t.layer == id) {
                s.active_layer = i;
            }
            Resp::Unit
        }
        Req::RemoveLayer(id) => {
            super::super::frames::catch_up(e, s);
            let ok = e.painter.remove_layer_silent(id);
            if ok {
                layers_changed(e, s);
            }
            Resp::Bool(ok)
        }
        Req::Place(id, reference, above, group) => {
            Resp::Bool(place(e, s, id, reference, above, group))
        }
        Req::AddGroup(layer) => {
            if e.is_stroking() {
                return Resp::U32(0);
            }
            let g = e.painter.add_group(layer);
            if g != 0 {
                layers_changed(e, s);
            }
            Resp::U32(g)
        }
        Req::SetGroup(id, g) => {
            e.painter.set_group(
                id,
                &g.name,
                g.visible,
                g.opacity.clamp(0.0, 1.0) as f32,
                false,
            );
            Resp::Unit
        }
        Req::Ungroup(id) => {
            let ok = e.painter.ungroup(id);
            if ok {
                layers_changed(e, s);
            }
            Resp::Bool(ok)
        }
        Req::MergeDown(id) => {
            super::super::frames::catch_up(e, s);
            let ok = !e.is_stroking() && e.painter.merge_down(id);
            if ok {
                layers_changed(e, s);
            }
            Resp::Bool(ok)
        }
        Req::MergeGroup(id) => {
            super::super::frames::catch_up(e, s);
            let l = if e.is_stroking() {
                0
            } else {
                e.painter.merge_group(id)
            };
            if l != 0 {
                layers_changed(e, s);
            }
            Resp::U32(l)
        }
        Req::Groups => Resp::Groups(
            e.painter
                .groups()
                .iter()
                .map(|g| PortGroup {
                    id: g.id,
                    settings: GroupSettings {
                        name: g.name.clone(),
                        visible: g.visible,
                        opacity: g.opacity as f64,
                    },
                })
                .collect(),
        ),
        Req::Layers => Resp::Layers(
            e.painter
                .layers()
                .iter()
                .filter(|l| !e.frames.previews.contains(&l.id))
                .map(|l| PortLayer {
                    id: l.id,
                    name: l.props.name.clone(),
                    look: look_of(e, l.id),
                    adjust: e.together.win.adjusts.get(&l.id).cloned().or_else(|| {
                        (l.props.adjust.is_some() && !l.props.adjust_spec.is_empty())
                            .then(|| l.props.adjust_spec.clone())
                    }),
                    group: l.group(),
                })
                .collect(),
        ),
        Req::Timeline => Resp::Timeline(timeline_of(e, s)),
        Req::SetTimeline(tl) => {
            put_timeline(e, s, tl);
            Resp::Unit
        }
        Req::DrawingsOfLayer(id) => {
            ensure_full(e, s);
            let mut v: Vec<u32> = s
                .sheet
                .tracks
                .iter()
                .filter(|t| t.layer == id)
                .flat_map(|t| t.drawings())
                .collect();
            if let Some(tl) = &e.together.win.full_tl {
                for sc in &tl.scenes {
                    for c in &sc.cuts {
                        for x in c.tracks.get(&id).into_iter().flatten() {
                            if x.drawing != 0 {
                                v.push(x.drawing);
                            }
                        }
                    }
                }
            }
            v.sort_unstable();
            v.dedup();
            Resp::Ids(v)
        }
        Req::NewDoc(size) => {
            let (w, h) = size.unwrap_or((e.painter.width(), e.painter.height()));
            super::super::files::new_sized(e, s, w, h);
            e.painter.track_edits(true);
            e.frames.previews.clear();
            e.together.win = Window::default();
            // (the empty layer it opens with is put aside: the shared ones are the project)
            let ids: Vec<u32> = e.painter.layers().iter().map(|l| l.id).collect();
            for id in ids {
                set_props(e, id, |p| {
                    p.visible = false;
                    p.locked = true;
                });
            }
            super::super::frames::sheet_changed(e, s);
            e.together.win.sheet_sig = sheet_sig(e, s);
            Resp::Unit
        }
        Req::Doc => Resp::Doc(e.painter.width(), e.painter.height(), s.frame),
    }
}

fn set_layer(e: &mut Engine, id: u32, patch: LayerPatch) {
    if let Some(look) = &patch.look {
        // (kept as it came: given back so, but for what changes here)
        let mut keep = look.clone();
        if let Some(was) = e.together.win.looks.get(&id)
            && keep.style.is_none()
        {
            keep.style = was.style.clone();
        }
        e.together.win.looks.insert(id, keep);
    }
    set_props(e, id, |p| {
        if let Some(n) = &patch.name {
            p.name = n.chars().take(60).collect();
        }
        if let Some(l) = patch.locked {
            p.locked = l;
        }
        if let Some(v) = patch.visible {
            p.visible = v;
        }
        if let Some(look) = &patch.look {
            p.visible = look.visible;
            p.opacity = look.opacity.clamp(0.0, 1.0) as f32;
            if let Some(b) = brush::Blend::parse(&look.blend) {
                p.blend = b;
            }
            p.alpha_lock = look.alpha_lock;
            p.clip = look.clip;
        }
    });
}

fn layer_of_drawing(e: &Engine, s: &State, d: u32) -> Option<u32> {
    if d == 0 {
        return None;
    }
    if let Some(l) = e
        .painter
        .layers()
        .iter()
        .find(|l| e.painter.layer_drawing(l.id) == d)
    {
        return Some(l.id);
    }
    if let Some(t) = s
        .sheet
        .tracks
        .iter()
        .find(|t| t.cels.iter().any(|c| c.drawing == d))
    {
        return Some(t.layer);
    }
    let tl = e.together.win.full_tl.as_ref()?;
    tl.scenes.iter().flat_map(|sc| &sc.cuts).find_map(|c| {
        c.tracks
            .iter()
            .find(|(_, ex)| ex.iter().any(|x| x.drawing == d))
            .map(|(l, _)| *l)
    })
}

/// Moves a layer right above / below `reference` (0: where it is) and into a group.
fn place(e: &mut Engine, s: &mut State, id: u32, reference: u32, above: bool, group: u32) -> bool {
    if e.is_stroking() {
        return false;
    }
    super::super::frames::catch_up(e, s);
    let pos = |e: &Engine, id: u32| e.painter.layers().iter().position(|l| l.id == id);
    let Some(from) = pos(e, id) else {
        return false;
    };
    let to = match (reference, pos(e, reference)) {
        (0, _) => from,
        (_, Some(r)) => match (above, from > r) {
            (true, true) => r + 1,
            (true, false) => r,
            (false, true) => r,
            (false, false) => r.saturating_sub(1),
        },
        (_, None) => from,
    };
    let in_group = e
        .painter
        .layers()
        .get(from)
        .is_some_and(|l| l.group() == group);
    if to == from && in_group {
        return true;
    }
    let ok = e.painter.move_layer_to(id, to, group);
    if ok {
        layers_changed(e, s);
    }
    ok
}

/// Someone's tiles put in: patched into the drawing in the painter (only those that
/// changed), or into its copy kept as it came; the exact bytes kept either way.
fn put(
    e: &mut Engine,
    s: &mut State,
    d: u32,
    set: Vec<((i32, i32), brush::DecodedTile, super::port::Exact)>,
    clear: Vec<(i32, i32)>,
    stamp: Option<super::port::Stamp>,
) -> bool {
    if d == 0 {
        return false;
    }
    // drawing on that drawing now (granted to two): it goes in once the stroke ends
    if e.is_stroking() && e.painter.layer_drawing(e.painter.active_layer()) == d {
        e.together.stats.deferred += 1;
        return false;
    }
    let n = set.len() + clear.len();
    let in_painter = e.painter.has_drawing(d);
    let kept = e.frames.store.drawings.get(&d).cloned();
    match (&kept, in_painter) {
        // kept as it came, not in the painter: changed there
        (Some(k), false) if matches!(**k, Kept::Wire(_)) => {
            let Kept::Wire(tiles) = &**k else {
                unreachable!()
            };
            let mut m: Vec<((i32, i32), super::port::Exact)> = tiles.clone();
            m.retain(|(k, _)| !clear.contains(k) && !set.iter().any(|(s, _, _)| s == k));
            m.extend(set.into_iter().map(|(k, _, x)| (k, x)));
            e.frames.store.drawings.insert(d, Arc::new(Kept::Wire(m)));
        }
        // nowhere yet: kept as it came
        (None, false) => {
            let m: Vec<((i32, i32), super::port::Exact)> =
                set.into_iter().map(|(k, _, x)| (k, x)).collect();
            e.frames.store.drawings.insert(d, Arc::new(Kept::Wire(m)));
        }
        _ => {
            if !in_painter {
                super::super::frames::load_drawings(e, &[d]);
            }
            let exact = e.frames.store.exact.entry(d).or_default();
            for k in &clear {
                exact.remove(k);
            }
            for (k, _, x) in &set {
                exact.insert(*k, x.clone());
            }
            let decoded: Vec<((i32, i32), brush::DecodedTile)> =
                set.into_iter().map(|(k, t, _)| (k, t)).collect();
            if !e.painter.patch_drawing(d, decoded, &clear) {
                return false;
            }
            e.frames.store.drawings.remove(&d);
        }
    }
    e.frames.remote_changed(d);
    let _ = s;
    if e.together.stats_on
        && let Some(st) = stamp
    {
        e.together.stats.edits.push(super::EditStat {
            kind: st.kind,
            from: st.from,
            tiles: n,
            received_ns: st.received_ns,
            applied_ns: pencilpilot_window::clock::now_ns(),
            sent_ns: st.sent_ns,
        });
    }
    true
}
