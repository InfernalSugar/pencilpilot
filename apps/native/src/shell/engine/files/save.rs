//! The engine into a project file: the layers, the open cut's exposures and drawings, the
//! frame rate, the palette, what the app keeps in `extra`; everything the native app
//! doesn't edit (other scenes and cuts, sounds, fonts, reference pictures, line data, the
//! timelapse, layer styles…) kept as the file had it.
//!
//! Drawings are read from the painter a tile at a time on every core and named by their
//! content: a tile the old file already has isn't compressed again but copied over as
//! stored ([`ppstudio::Project::write_from`]); only changed tiles are encoded.

use std::collections::{BTreeMap, HashMap, HashSet};

use ppstudio::{Cut, Drawing, Exposure, Layer, Manifest, Palette, PaletteColour, Project, TileRef};
use rayon::prelude::*;

use super::super::Engine;
use super::Doc;
use super::load::{colour_hex, mark_name, name_of, new_scene, status_name};
use crate::state::State;

/// How saving went (ms, tiles), for checks.
#[derive(Debug, Clone, Default)]
pub struct SaveTimes {
    /// the window's part (the project made, the changed tiles read), and the rest (written
    /// on another thread)
    pub window_ms: f64,
    pub tiles_ms: f64,
    pub write_ms: f64,
    /// tiles read from the painter, and of those new to the file (compressed)
    pub tiles: usize,
    pub new_tiles: usize,
}

const TILE: usize = 64;

/// Now, as RFC 3339 UTC.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // (civil from days: Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// A small picture of what the canvas shows (at most 320 px across, straight RGBA8): each
/// pixel the average of a 4 × 4 grid of samples over its area (not every pixel: a 4K canvas
/// has 8 million, and the window waits for this). [`png`] makes the file's PNG of it.
pub fn thumbnail(e: &Engine) -> (u32, u32, Vec<u8>) {
    let (w, h) = (e.painter.width() as usize, e.painter.height() as usize);
    let k = (w.max(h)).div_ceil(320).max(1);
    let (tw, th) = (w.div_ceil(k), h.div_ceil(k));
    let src = e.painter.rgba();
    let mut out = vec![0u8; tw * th * 4];
    const N: usize = 4;
    for ty in 0..th {
        for tx in 0..tw {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for sy in 0..N {
                let y = (ty * k + (sy * k) / N).min(h - 1);
                for sx in 0..N {
                    let x = (tx * k + (sx * k) / N).min(w - 1);
                    let i = (y * w + x) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                    n += 1;
                }
            }
            let o = (ty * tw + tx) * 4;
            for c in 0..4 {
                out[o + c] = (acc[c] / n) as u8;
            }
        }
    }
    (tw as u32, th as u32, out)
}

/// The thumbnail as PNG (on the writing thread).
pub fn png((w, h, rgba): &(u32, u32, Vec<u8>)) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, *w, *h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

/// A layer as the file keeps it, for a layer the file didn't have.
fn new_layer(id: u32) -> Layer {
    Layer {
        id,
        name: String::new(),
        visible: true,
        opacity: 1.0,
        blend: "normal".into(),
        locked: false,
        alpha_lock: false,
        clip: false,
        imported: false,
        group: 0,
        adjust: None,
        style: None,
        text: None,
        lines: false,
    }
}

/// The project as a file's contents (pixels the old file has left out: see
/// [`Project::write_from`]). Updates `doc`'s painter → file layer ids for new layers.
pub fn project(e: &Engine, s: &State, doc: &mut Doc) -> Result<(Project, SaveTimes), String> {
    let mut times = SaveTimes::default();
    let (w, h) = (e.painter.width(), e.painter.height());
    let now = now_rfc3339();
    let app = format!("PencilPilot {}", env!("CARGO_PKG_VERSION"));
    let mut p = Project::new(&app, &now, w, h, s.fps.max(1));
    let base: Manifest = match &doc.base {
        Some(b) => b.clone(),
        None => p.manifest.clone(),
    };
    let mut m = base.clone();
    m.app = app;
    m.modified = now;
    m.canvas = ppstudio::Canvas {
        width: w,
        height: h,
    };
    m.timeline.fps = s.fps.clamp(1, 120);

    // ---- layers: the painter's, bottom first, keeping what the file had for each ----
    let mut next_id = base.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
    for pl in e.painter.layers() {
        doc.layer_ids.entry(pl.id).or_insert_with(|| {
            next_id += 1;
            next_id - 1
        });
    }
    let fid = |pid: u32| doc.layer_ids.get(&pid).copied().unwrap_or(0);
    m.layers = e
        .painter
        .layers()
        .iter()
        // (others' strokes as they draw them, drawing together, aren't the project's)
        .filter(|pl| !e.frames.previews.contains(&pl.id))
        .map(|pl| {
            let id = fid(pl.id);
            let mut l = base
                .layers
                .iter()
                .find(|l| l.id == id)
                .cloned()
                .unwrap_or_else(|| new_layer(id));
            l.name = pl.props.name.clone();
            l.visible = pl.props.visible;
            l.locked = pl.props.locked;
            l.opacity = pl.props.opacity;
            l.blend = pl.props.blend.as_str().into();
            l.alpha_lock = pl.props.alpha_lock;
            l.clip = pl.props.clip;
            // (drawing together: the session's groups, as the painter keeps them)
            if !e.together.view.ids.is_empty() {
                l.group = pl.group();
            }
            l
        })
        .collect();
    if !e.together.view.ids.is_empty() {
        m.groups = e
            .painter
            .groups()
            .iter()
            .map(|g| ppstudio::Group {
                id: g.id,
                name: g.name.clone(),
                visible: g.visible,
                opacity: g.opacity,
            })
            .collect();
    }
    let here: HashSet<u32> = m.layers.iter().map(|l| l.id).collect();
    m.active_layer = s
        .sheet
        .tracks
        .get(s.active_layer)
        .map_or(0, |t| fid(t.layer));

    // ---- the open cut: each layer's exposures ----
    let mut tracks: BTreeMap<u32, Vec<Exposure>> = BTreeMap::new();
    let mut statics = Vec::new();
    // painter drawing → (its file layer, the painter layer it's on)
    let mut drawings: Vec<(u32, u32)> = Vec::new();
    let mut kinds: BTreeMap<String, String> = BTreeMap::new();
    for t in &s.sheet.tracks {
        let layer = fid(t.layer);
        if layer == 0 || !here.contains(&layer) {
            continue;
        }
        if t.kind == 'S' {
            statics.push(layer);
        }
        let mut ex: Vec<Exposure> = Vec::new();
        for c in &t.cels {
            let hold = if t.kind == 'S' {
                s.frames.max(1)
            } else {
                c.hold.max(1)
            };
            ex.push(Exposure {
                drawing: (c.drawing != 0).then(|| name_of(c.drawing)),
                length: hold,
            });
            if c.drawing != 0 {
                if !drawings.iter().any(|(d, _)| *d == c.drawing) {
                    drawings.push((c.drawing, t.layer));
                }
                if let Some(k) = mark_name(c.mark) {
                    kinds.entry(name_of(c.drawing)).or_insert_with(|| k.into());
                }
            }
        }
        tracks.insert(layer, ex);
    }
    let cut_name = s.cut_name.clone();
    let mut scenes = m.timeline.scenes.clone();
    if !doc.had_scenes || scenes.is_empty() {
        scenes = vec![new_scene(&doc.scene_name, Cut::default())];
        doc.scene = 0;
        doc.cut = 0;
        doc.had_scenes = true;
    }
    let cut: &mut Cut = scenes
        .get_mut(doc.scene)
        .and_then(|sc| sc.cuts.get_mut(doc.cut))
        .ok_or("the open cut isn't in the project")?;
    cut.name = cut_name;
    cut.tracks = tracks;
    // the scene's sound as the timeline has it (clips moved, muted)
    if s.sound.from_file
        && let Some(sc) = scenes.get_mut(doc.scene)
    {
        sc.audio = s.sound.file_clips();
    }
    // the other cuts: only layers that are still there
    for c in scenes.iter_mut().flat_map(|sc| sc.cuts.iter_mut()) {
        c.tracks.retain(|l, _| here.contains(l));
    }
    m.timeline.static_layers = base
        .timeline
        .static_layers
        .iter()
        .copied()
        .filter(|l| here.contains(l) && !s.sheet.tracks.iter().any(|t| fid(t.layer) == *l))
        .chain(statics)
        .collect();
    m.timeline
        .layer_stages
        .retain(|l, _| l.parse::<u32>().is_ok_and(|id| here.contains(&id)));
    m.timeline.mouth_charts.retain(|c| here.contains(&c.layer));

    // ---- drawings: the open cut's from the painter, the rest as the file had them ----
    let mut keep: HashSet<String> = HashSet::new();
    for (i, c) in scenes.iter().flat_map(|sc| &sc.cuts).enumerate() {
        let open = scenes
            .iter()
            .take(doc.scene)
            .map(|sc| sc.cuts.len())
            .sum::<usize>()
            + doc.cut;
        if i == open {
            continue;
        }
        for t in c.tracks.values() {
            keep.extend(t.iter().filter_map(|x| x.drawing.clone()));
        }
    }
    for c in &m.timeline.mouth_charts {
        keep.extend(c.shapes.values().cloned());
    }
    let ours: HashSet<String> = drawings.iter().map(|(d, _)| name_of(*d)).collect();
    m.drawings = base
        .drawings
        .iter()
        .filter(|(n, _)| keep.contains(*n) && !ours.contains(*n))
        .map(|(n, d)| (n.clone(), d.clone()))
        .collect();
    // the layer masks the file had (the native app doesn't edit them: kept)
    let mask_of = |name: &str, layer: u32| -> Option<Vec<TileRef>> {
        if let Some(d) = base.drawings.get(name) {
            return d.mask.clone();
        }
        let fl = fid(layer);
        base.timeline
            .scenes
            .iter()
            .flat_map(|sc| &sc.cuts)
            .filter_map(|c| c.tracks.get(&fl))
            .flatten()
            .filter_map(|x| x.drawing.as_ref())
            .find_map(|n| base.drawings.get(n).and_then(|d| d.mask.clone()))
    };
    // every drawing's tiles, named on every core: a drawing kept as the file has it
    // (unchanged since read) keeps its tiles; one changed here is read from the painter
    // (or from where it was put away: the painter's own pixels, compressed)
    let t0 = std::time::Instant::now();
    let (cols, rows) = (
        w.div_ceil(TILE as u32) as i32,
        h.div_ceil(TILE as u32) as i32,
    );
    use super::super::frames::store::Kept;
    let store = &e.frames.store;
    // tiles past the canvas aren't written (as the current app's writer leaves them out); a
    // shared project's tiles at its edge are written as they are, as that writer writes them
    // (a solo project's are cut at the edge)
    let shared = !e.together.view.ids.is_empty();
    let on = |x: i32, y: i32| x >= 0 && y >= 0 && x < cols && y < rows;
    let mut kept_refs: HashMap<u32, Vec<TileRef>> = HashMap::new();
    // (drawing, tile, where: None the painter, Some(i) the store's i-th tile)
    let mut jobs: Vec<(u32, (i32, i32), Option<usize>)> = Vec::new();
    for (d, _) in &drawings {
        match store.drawings.get(d).map(|k| &**k) {
            Some(Kept::File(refs)) => {
                kept_refs.insert(
                    *d,
                    refs.iter()
                        .filter(|(x, y, _)| on(*x, *y))
                        .map(|(x, y, h)| TileRef {
                            x: *x as u32,
                            y: *y as u32,
                            hash: h.clone(),
                        })
                        .collect(),
                );
            }
            Some(Kept::Raw(tiles)) => jobs.extend(
                tiles
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| on(k.0, k.1))
                    .map(|(i, (k, _))| (*d, *k, Some(i))),
            ),
            // (from others, drawing together: their RGBA tiles, kept as they came, past the
            // canvas's edge too, as the current app keeps them)
            Some(Kept::Wire(tiles)) => jobs.extend(
                tiles
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| on(k.0, k.1))
                    .map(|(i, (k, _))| (*d, *k, Some(i))),
            ),
            None => jobs.extend(
                e.painter
                    .drawing_tile_keys(*d)
                    .into_iter()
                    .filter(|&(x, y)| {
                        on(x, y)
                            || (x >= 0 && y >= 0 && exact.is_some_and(|m| m.contains_key(&(x, y))))
                    })
                    .map(|k| (*d, k, None)),
            ),
        }
    }
    let source = doc.source.as_ref();
    let painter = &e.painter;
    // where drawings read from the file had each tile: one that didn't change keeps the
    // file's bytes (the painter rounds faint colour a little on the way out)
    let origin: HashMap<(u32, (i32, i32)), &String> = drawings
        .iter()
        .filter_map(|(d, _)| Some((*d, store.origin.get(d)?)))
        .flat_map(|(d, refs)| refs.iter().map(move |(x, y, h)| ((d, (*x, *y)), h)))
        .collect();
    // (drawing, tile, its name, its bytes when the old file hasn't it)
    type Named = (u32, (i32, i32), String, Option<Vec<u8>>);
    let tiles: Vec<Named> = super::super::frames::store::pool().install(|| {
        jobs.par_iter()
            .filter_map(|&(d, (x, y), at)| {
                let mut px = vec![0u8; TILE * TILE * 4];
                // (a tile as it came from someone, drawing together: its bytes exactly)
                let mut as_came = false;
                match at {
                    None => {
                        if let Some(t) = store.exact.get(&d).and_then(|m| m.get(&(x, y)))
                            && let Some(b) = super::super::frames::store::file_rgba(t)
                        {
                            px = b;
                            as_came = true;
                        } else if !painter.drawing_tile_rgba(d, (x, y), &mut px) {
                            return None;
                        }
                    }
                    Some(i) => match store.drawings.get(&d).map(|k| &**k) {
                        Some(Kept::Raw(t)) => {
                            let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(
                                &t.get(i)?.1,
                                brush::DecodedTile::BYTES,
                            )
                            .ok()?;
                            brush::encode_tile(&brush::DecodedTile::from_bytes(&raw)?, &mut px);
                        }
                        Some(Kept::Wire(t)) => {
                            px = super::super::frames::store::file_rgba(&t.get(i)?.1)?;
                            as_came = true;
                        }
                        _ => return None,
                    },
                }
                // (paint past the canvas's edge isn't part of the picture: as the file cuts
                // tiles from a whole canvas)
                let (x0, y0) = (x.max(0) as usize * TILE, y.max(0) as usize * TILE);
                let (wn, hn) = (
                    (w as usize).saturating_sub(x0).min(TILE),
                    (h as usize).saturating_sub(y0).min(TILE),
                );
                if !as_came && !shared && (wn < TILE || hn < TILE) {
                    for row in 0..TILE {
                        let from = if row < hn { wn } else { 0 };
                        px[(row * TILE + from) * 4..(row + 1) * TILE * 4].fill(0);
                    }
                }
                if px.chunks(4).all(|p| p[3] == 0) {
                    return None;
                }
                let mut hash = ppstudio::tile_hash(&px);
                let mut known = source.is_some_and(|o| o.has_tile(&hash));
                // (unchanged since it was read: the file's own bytes for it)
                if !known
                    && let Some(orig) = origin.get(&(d, (x, y)))
                    && source.is_some_and(|o| o.has_tile(orig))
                    && let Some(file) = store.file_rgba(orig)
                    && let Some(t) = brush::decode_tile(&file)
                {
                    let mut back = vec![0u8; TILE * TILE * 4];
                    brush::encode_tile(&t, &mut back);
                    if back == px {
                        hash = (*orig).clone();
                        known = true;
                    }
                }
                Some((d, (x, y), hash, (!known).then_some(px)))
            })
            .collect()
    });
    times.tiles = tiles.len();
    let mut refs: HashMap<u32, Vec<TileRef>> = kept_refs;
    for (d, (x, y), hash, px) in tiles {
        if let Some(px) = px
            && !p.tiles.contains_key(&hash)
        {
            times.new_tiles += 1;
            p.tiles.insert(hash.clone(), px);
        }
        refs.entry(d).or_default().push(TileRef {
            x: x as u32,
            y: y as u32,
            hash,
        });
    }
    for (d, layer) in &drawings {
        let mut tiles = refs.remove(d).unwrap_or_default();
        tiles.sort_by_key(|r| (r.y, r.x));
        let name = name_of(*d);
        let mask = mask_of(&name, *layer);
        m.drawings.insert(name, Drawing { tiles, mask });
    }
    times.tiles_ms = t0.elapsed().as_secs_f64() * 1e3;
    // the mask tiles kept must be written too: copied from the old file
    m.timeline
        .drawing_kinds
        .retain(|d, _| keep.contains(d) && !ours.contains(d));
    m.timeline.drawing_kinds.extend(kinds);
    m.timeline
        .drawing_kinds
        .retain(|d, _| m.drawings.contains_key(d));
    for c in &mut m.timeline.mouth_charts {
        c.shapes.retain(|_, d| m.drawings.contains_key(d));
    }
    m.timeline.frames = ppstudio::Timeline::flatten(&scenes, &m.timeline.static_layers);
    if m.timeline.frames.is_empty() {
        m.timeline.frames = vec![Default::default()];
    }
    m.timeline.scenes = scenes;

    // ---- the palette: the first of the file's, the native app's one ----
    let pal = Palette {
        name: s.palette_name.chars().take(40).collect(),
        colours: s
            .palette
            .iter()
            .take(64)
            .map(|(n, c)| PaletteColour {
                name: n.chars().take(40).collect(),
                hex: colour_hex(*c),
            })
            .collect(),
    };
    let default = super::default_palette();
    let unchanged_default = (s.palette_name.clone(), s.palette.clone()) == default;
    match m.timeline.palettes.first_mut() {
        Some(first) => *first = pal,
        None if !unchanged_default => m.timeline.palettes.push(pal),
        None => {}
    }

    // ---- what the app keeps for itself (the web app's keys, and the native app's) ----
    let mut extra = match &base.extra {
        serde_json::Value::Object(o) => o.clone(),
        _ => serde_json::Map::new(),
    };
    use serde_json::json;
    extra.insert("scene".into(), json!(doc.scene));
    extra.insert("cut".into(), json!(doc.cut));
    extra.insert("frame".into(), json!(s.frame));
    extra.insert("color".into(), json!(colour_hex(s.colour)));
    extra.insert("size".into(), json!(s.brush_size));
    let status: serde_json::Map<String, serde_json::Value> = s
        .sheet
        .status
        .iter()
        .filter(|(d, _)| ours.contains(&name_of(**d)))
        .map(|(d, st)| (name_of(*d), json!(status_name(*st))))
        .collect();
    extra.insert("colourStatus".into(), serde_json::Value::Object(status));
    match s.sheet.loop_range {
        Some((a, b)) => extra.insert("loop".into(), json!([a, b])),
        None => extra.remove("loop"),
    };
    extra.insert("notes".into(), super::notes_json(&s.notes));
    m.extra = serde_json::Value::Object(extra);

    p.manifest = m;
    // the stroke log, timelapse, sounds… stay in the old file: copied from it
    if doc.source.is_some() {
        p.strokes.clear();
    }
    // (made into a PNG by whoever writes the file)
    p.thumbnail = None;
    Ok((p, times))
}
