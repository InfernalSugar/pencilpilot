//! A project file into the engine: a painter the file's size with the open cut's drawings,
//! the layers, the exposure sheet, the frame rate, the palette and what the app keeps in
//! the file's `extra` (as the web app does: the scene, cut and frame open, the colour…).

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};
use std::path::Path;

use brush::{Blend, Painter};
use ppstudio::{Cut, Exposure, Manifest, Opened, Scene};
use rayon::prelude::*;

use super::super::Engine;
use super::super::frames::{Cel, Mark, Sheet, Track};
use super::Doc;
use crate::state::{ColourStatus, Layer, State};

/// How long each part of opening took (ms), for checks.
#[derive(Debug, Clone, Default)]
pub struct OpenTimes {
    pub read_ms: f64,
    pub tiles_ms: f64,
    pub painter_ms: f64,
    pub setup_ms: f64,
    /// the old document let go of, and the file kept open
    pub rest_ms: f64,
    /// all of it
    pub total_ms: f64,
    pub tiles: usize,
}

/// A drawing's name in the file for a painter drawing id.
pub fn name_of(d: u32) -> String {
    format!("D{d}")
}

/// A painter drawing id from a file's name ("D12" → 12).
fn id_of(name: &str) -> Option<u32> {
    name.strip_prefix('D')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|&n| n > 0)
}

/// The scene and cut the file had open (its `extra`, as the web app keeps them), within
/// what's there.
fn open_cut(m: &Manifest) -> (usize, usize) {
    let at = |k: &str| m.extra.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let scenes = &m.timeline.scenes;
    if scenes.is_empty() {
        return (0, 0);
    }
    let sc = at("scene").min(scenes.len() - 1);
    let cut = at("cut").min(scenes[sc].cuts.len().saturating_sub(1));
    (sc, cut)
}

/// A file with no scenes (a still, or written by a simple tool): its frames as one cut.
fn cut_from_frames(m: &Manifest) -> Cut {
    let mut tracks: BTreeMap<u32, Vec<Exposure>> = BTreeMap::new();
    for l in &m.layers {
        let mut t: Vec<Exposure> = Vec::new();
        for f in &m.timeline.frames {
            let d = f.cels.get(&l.id).cloned();
            match t.last_mut() {
                Some(e) if e.drawing == d => e.length += f.hold.max(1),
                _ => t.push(Exposure {
                    drawing: d,
                    length: f.hold.max(1),
                }),
            }
        }
        // (a layer that shows nothing at all: no exposures)
        if t.iter().any(|e| e.drawing.is_some()) {
            tracks.insert(l.id, t);
        }
    }
    Cut {
        name: "Cut 1".into(),
        tracks,
        ..Default::default()
    }
}

pub fn colour_hex(c: [f32; 4]) -> String {
    let [r, g, b, _] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn hex_colour(s: &str) -> Option<[f32; 4]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(pencilpilot_ui::theme::hex(v))
}

pub fn status_name(c: ColourStatus) -> &'static str {
    match c {
        ColourStatus::ToDo => "todo",
        ColourStatus::Flatted => "flatted",
        ColourStatus::Done => "done",
    }
}

fn status_of(s: &str) -> Option<ColourStatus> {
    match s {
        "todo" => Some(ColourStatus::ToDo),
        "flatted" => Some(ColourStatus::Flatted),
        "done" => Some(ColourStatus::Done),
        _ => None,
    }
}

pub fn mark_name(m: Mark) -> Option<&'static str> {
    match m {
        Mark::None => None,
        Mark::Key => Some("key"),
        Mark::Breakdown => Some("breakdown"),
        Mark::InBetween => Some("inbetween"),
    }
}

fn mark_of(s: &str) -> Mark {
    match s {
        "key" => Mark::Key,
        "breakdown" => Mark::Breakdown,
        "inbetween" => Mark::InBetween,
        _ => Mark::None,
    }
}

/// Opens `path` into the engine and the state. On an error nothing has changed.
pub fn open(e: &mut Engine, s: &mut State, path: &Path) -> Result<(Doc, OpenTimes), String> {
    let mut times = OpenTimes::default();
    let t = std::time::Instant::now();
    let t_all = t;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let o = Opened::read(std::io::Cursor::new(&bytes[..])).map_err(|e| e.to_string())?;
    times.read_ms = t.elapsed().as_secs_f64() * 1e3;
    let m = o.manifest.clone();
    let (w, h) = (m.canvas.width, m.canvas.height);

    // ---- the cut open, and its drawings' painter ids ----
    let (scene_i, cut_i) = open_cut(&m);
    let (scene_name, cut) = match m.timeline.scenes.get(scene_i) {
        Some(sc) => (
            sc.name.clone(),
            sc.cuts.get(cut_i).cloned().unwrap_or_default(),
        ),
        None => ("Scene 1".to_string(), cut_from_frames(&m)),
    };
    // (new ids go past every drawing the file has, in any cut)
    let mut next = m
        .drawings
        .keys()
        .filter_map(|n| id_of(n))
        .max()
        .unwrap_or(0)
        + 1;
    // file layer id → (painter drawing id → the file's drawing name), each drawing on one
    // layer only (one shown on two gets a copy)
    let mut used: HashMap<u32, u32> = HashMap::new();
    let mut ids: HashMap<(u32, String), u32> = HashMap::new();
    let mut loads: Vec<(u32, String)> = Vec::new();
    for (&layer, track) in &cut.tracks {
        if !m.layers.iter().any(|l| l.id == layer) {
            continue;
        }
        for name in track.iter().filter_map(|x| x.drawing.as_ref()) {
            if ids.contains_key(&(layer, name.clone())) {
                continue;
            }
            let id = match id_of(name) {
                Some(id) if !used.contains_key(&id) => id,
                _ => {
                    next += 1;
                    next - 1
                }
            };
            used.insert(id, layer);
            ids.insert((layer, name.clone()), id);
            loads.push((id, name.clone()));
        }
    }

    // ---- the drawings, kept compressed as the file has them (the painter gets the ones
    // showing, decoded when shown: a 4K cut's drawings all in it are hundreds of MB) ----
    let t = std::time::Instant::now();
    let (cols, rows) = (w.div_ceil(64), h.div_ceil(64));
    let mut store = super::super::frames::store::Store::default();
    let mut hashes: Vec<&str> = Vec::new();
    for (id, name) in &loads {
        let Some(d) = m.drawings.get(name) else {
            continue;
        };
        let refs: Vec<(i32, i32, String)> = d
            .tiles
            .iter()
            .filter(|r| r.x < cols && r.y < rows)
            .map(|r| (r.x as i32, r.y as i32, r.hash.clone()))
            .collect();
        hashes.extend(d.tiles.iter().map(|r| r.hash.as_str()));
        store.origin.insert(*id, refs.clone());
        store.drawings.insert(
            *id,
            std::sync::Arc::new(super::super::frames::store::Kept::File(refs)),
        );
    }
    hashes.sort_unstable();
    hashes.dedup();
    times.tiles = hashes.len();
    use super::super::frames::store::FileTile;
    type Read<'a> = (&'a str, std::sync::Arc<FileTile>);
    let read: Vec<Read> = hashes
        .par_iter()
        .map_init(
            || o.tiles(),
            |o, h| -> Result<Read, String> {
                let (deflated, bytes) = o.tile_stored(h).map_err(|e| e.to_string())?;
                Ok((
                    *h,
                    std::sync::Arc::new(FileTile {
                        deflated,
                        bytes: bytes.into_boxed_slice(),
                    }),
                ))
            },
        )
        .collect::<Result<_, _>>()?;
    store.file = read.into_iter().map(|(h, t)| (h.to_string(), t)).collect();
    times.tiles_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let mut painter = Painter::new(w, h);
    painter.reserve_drawings(next);
    times.painter_ms = t.elapsed().as_secs_f64() * 1e3;

    // ---- the state: layers top first, their tracks, the rest ----
    let t = std::time::Instant::now();
    let statics = &m.timeline.static_layers;
    let kinds = &m.timeline.drawing_kinds;
    let mut layers = Vec::new();
    let mut tracks = Vec::new();
    let mut file_ids = Vec::new();
    for l in m.layers.iter().rev() {
        let kind = if statics.contains(&l.id) { 'S' } else { 'C' };
        layers.push(Layer {
            name: l.name.clone(),
            visible: l.visible,
            locked: l.locked,
            kind,
        });
        let mut track = Track::new(0, kind);
        track.opacity = l.opacity.clamp(0.0, 1.0);
        for x in cut.tracks.get(&l.id).into_iter().flatten() {
            let d = x
                .drawing
                .as_ref()
                .and_then(|n| ids.get(&(l.id, n.clone())).copied())
                .unwrap_or(0);
            let mark = x
                .drawing
                .as_ref()
                .and_then(|n| kinds.get(n))
                .map_or(Mark::None, |k| mark_of(k));
            track.cels.push(Cel::new(d, x.length.max(1), mark));
        }
        if kind == 'S' {
            let d = track.drawings().first().copied().unwrap_or(0);
            let mark = track
                .cels
                .iter()
                .find(|c| c.drawing == d)
                .map_or(Mark::None, |c| c.mark);
            track.cels = vec![Cel::new(d, 1, mark)];
        }
        tracks.push(track);
        file_ids.push(l.id);
    }
    let extra = &m.extra;
    let mut status = BTreeMap::new();
    if let Some(cs) = extra.get("colourStatus").and_then(|v| v.as_object()) {
        for (n, v) in cs {
            if let (Some(id), Some(st)) = (
                ids.iter().find(|((_, nm), _)| nm == n).map(|(_, id)| *id),
                v.as_str().and_then(status_of),
            ) {
                status.insert(id, st);
            }
        }
    }
    let loop_range = extra
        .get("loop")
        .and_then(|v| v.as_array())
        .and_then(|a| Some((a.first()?.as_u64()? as u32, a.get(1)?.as_u64()? as u32)))
        .filter(|(a, b)| a <= b);
    let cut_len = cut.length(statics);

    // the engine: a new painter, frames and tools set up for it
    let t_rest = std::time::Instant::now();
    super::replace(e, s, painter);
    e.frames.store = store;
    times.rest_ms = t_rest.elapsed().as_secs_f64() * 1e3;
    s.layers = layers;
    s.sheet = Sheet {
        tracks,
        status,
        thumbs: BTreeMap::new(),
        loop_range,
        demo: false,
    };
    s.frames = cut_len;
    s.frame = extra.get("frame").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    s.fps = m.timeline.fps.max(1);
    s.active_layer = file_ids
        .iter()
        .position(|&id| id == m.active_layer)
        .unwrap_or(0);
    s.cut_name = cut.name.clone();
    match m.timeline.palettes.first() {
        Some(p) => {
            s.palette_name = p.name.clone();
            s.palette = p
                .colours
                .iter()
                .filter_map(|c| Some((c.name.clone(), hex_colour(&c.hex)?)))
                .collect();
        }
        None => (s.palette_name, s.palette) = super::default_palette(),
    }
    if let Some(c) = extra
        .get("color")
        .and_then(|v| v.as_str())
        .and_then(hex_colour)
    {
        s.colour = c;
        s.colour_name = s
            .palette
            .iter()
            .find(|(_, p)| colour_hex(*p) == colour_hex(c))
            .map_or_else(|| colour_hex(c), |(n, _)| n.clone());
    }
    if let Some(v) = extra.get("size").and_then(|v| v.as_f64()) {
        s.brush_size = (v as f32).clamp(0.5, 2000.0);
    }
    s.notes = super::notes_from(extra);
    super::super::frames::derive(s);
    // frames sets the painter's layers up from the state (its first tick)
    super::super::frames::tick(e, s, std::time::Instant::now());
    // the painter's ids for the file's layers (top first), and what the sheet doesn't hold
    let mut layer_ids = HashMap::new();
    for (i, t) in s.sheet.tracks.iter().enumerate() {
        let fid = file_ids[i];
        layer_ids.insert(t.layer, fid);
        if let Some(l) = m.layers.iter().find(|l| l.id == fid)
            && let Some(pl) = e.painter.layers().iter().find(|p| p.id == t.layer)
        {
            let mut p = pl.props.clone();
            p.blend = Blend::parse(&l.blend).unwrap_or(p.blend);
            p.alpha_lock = l.alpha_lock;
            p.clip = l.clip;
            e.painter.set_layer_props(t.layer, p, false);
        }
    }
    e.painter.clear_history();
    times.setup_ms = t.elapsed().as_secs_f64() * 1e3;

    // the file stays open: a save copies what it doesn't change from it
    use std::io::SeekFrom;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let source = o
        .with_reader(std::io::BufReader::new(file))
        .map_err(|e| e.to_string())?;
    times.total_ms = t_all.elapsed().as_secs_f64() * 1e3;
    Ok((
        Doc {
            source: Some(source),
            base: Some(m),
            scene: scene_i,
            cut: cut_i,
            scene_name,
            had_scenes: !o.manifest.timeline.scenes.is_empty(),
            layer_ids,
        },
        times,
    ))
}

/// A scene as the file keeps it, for one with none yet.
pub fn new_scene(name: &str, cut: Cut) -> Scene {
    Scene {
        name: name.into(),
        cuts: vec![cut],
        ..Default::default()
    }
}
