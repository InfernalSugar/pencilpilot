//! The PencilPilot project file, `.ppstudio` (format version 1).
//!
//! A zip archive (see `FORMAT.md` next to this crate for the full description):
//!
//! | Entry | What |
//! |---|---|
//! | `manifest.json` | format and version, canvas, layers, groups, timeline, drawings (as lists of tiles) |
//! | `tiles/<hash>` | one 64×64 tile: RGBA8 (16 384 bytes) or a mask tile, grey (4 096 bytes) |
//! | `strokes.json` | the app's stroke log (kept as given) |
//! | `thumbnail.png` | a small picture for the project browser (optional) |
//!
//! Pixels are cut into 64×64 tiles. Empty tiles (fully transparent, or a mask that
//! shows everything) are left out, and identical tiles are stored once, under the
//! BLAKE3 hash of their bytes. That keeps drawings small, and animation frames that
//! repeat a cel cost almost nothing.
//!
//! Layers are timeline tracks; a frame says which drawing each layer shows. Version 1
//! files written by the app have one frame; the shape is ready for animation (S2).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The `format` field of every manifest.
pub const FORMAT: &str = "ppstudio";
/// The newest version this crate reads and the one it writes.
pub const VERSION: u32 = 1;
/// Tile side, in pixels.
pub const TILE: u32 = 64;
const RGBA_TILE: usize = (TILE * TILE * 4) as usize;
const MASK_TILE: usize = (TILE * TILE) as usize;
/// Largest canvas side accepted (matches the paint engine).
pub const SIDE_MAX: u32 = 8192;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    /// Not a project file, or damaged.
    Invalid(String),
    /// Made by a newer PencilPilot.
    TooNew(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Zip(e) => write!(f, "not a PencilPilot project (zip: {e})"),
            Error::Json(e) => write!(f, "damaged project (manifest: {e})"),
            Error::Invalid(why) => write!(f, "damaged project: {why}"),
            Error::TooNew(v) => write!(
                f,
                "this project was made by a newer PencilPilot (format {v}); update the app to open it"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Zip(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    /// 0–1
    pub opacity: f32,
    /// normal, multiply, screen, overlay, darken, lighten, add
    pub blend: String,
    pub locked: bool,
    pub alpha_lock: bool,
    /// clipped to the layer below
    pub clip: bool,
    /// holds an imported picture
    #[serde(default)]
    pub imported: bool,
    /// its group (0 = none)
    #[serde(default)]
    pub group: u32,
    /// an adjustment layer's settings (the app's JSON); none for paint layers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adjust: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
}

/// One frame: which drawing each layer shows (a layer missing here is empty).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Frame {
    /// layer id → drawing id
    pub cels: BTreeMap<u32, String>,
    /// how many frames it is held for (1 = "on ones")
    #[serde(default = "one")]
    pub hold: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub fps: u32,
    /// what shows on each frame, every cut one after another (for simple readers)
    pub frames: Vec<Frame>,
    /// the animation as it is edited: scenes → cuts → each layer's exposures. When
    /// present, `frames` is its flattening ([`Timeline::flatten`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Scene>,
    /// static layers (backgrounds, overlays): each shows its first drawing in a cut all
    /// cut long, and doesn't count towards the cut's length
    #[serde(
        default,
        rename = "staticLayers",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub static_layers: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    pub cuts: Vec<Cut>,
}

/// A cut (shot): each layer's exposures, from its first frame.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cut {
    pub name: String,
    /// layer id → its exposures in order; a layer missing here is empty all cut long
    pub tracks: BTreeMap<u32, Vec<Exposure>>,
    /// the part of the canvas the cut shows; none: all of it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<Camera>,
}

/// A rectangle of the canvas, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One drawing held for some frames (2 = "on twos").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Exposure {
    /// the drawing shown; none = an empty cel
    pub drawing: Option<String>,
    pub length: u32,
}

impl Cut {
    /// Frames long: its longest track, static layers' aside (at least one frame).
    pub fn length(&self, statics: &[u32]) -> u32 {
        self.tracks
            .iter()
            .filter(|(l, _)| !statics.contains(l))
            .map(|(_, t)| t.iter().map(|e| e.length).sum::<u32>())
            .max()
            .unwrap_or(0)
            .max(1)
    }
}

impl Timeline {
    /// The scenes as plain frames: one per frame, with runs of identical frames
    /// joined into holds.
    /// A static layer shows its first drawing on every frame of a cut.
    pub fn flatten(scenes: &[Scene], statics: &[u32]) -> Vec<Frame> {
        let mut frames: Vec<Frame> = Vec::new();
        for cut in scenes.iter().flat_map(|s| &s.cuts) {
            for f in 0..cut.length(statics) {
                let mut cels = BTreeMap::new();
                for (&layer, track) in &cut.tracks {
                    if statics.contains(&layer) {
                        if let Some(d) = track.iter().find_map(|e| e.drawing.as_ref()) {
                            cels.insert(layer, d.clone());
                        }
                        continue;
                    }
                    let mut at = 0;
                    for e in track {
                        if f < at + e.length {
                            if let Some(d) = &e.drawing {
                                cels.insert(layer, d.clone());
                            }
                            break;
                        }
                        at += e.length;
                    }
                }
                match frames.last_mut() {
                    Some(last) if last.cels == cels => last.hold += 1,
                    _ => frames.push(Frame { cels, hold: 1 }),
                }
            }
        }
        frames
    }
}

/// A tile of a drawing: its column and row, and the hash it is stored under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileRef {
    pub x: u32,
    pub y: u32,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Drawing {
    pub tiles: Vec<TileRef>,
    /// the layer mask's tiles, when the layer has a mask (grey, 255 = shown)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Vec<TileRef>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// what wrote it, e.g. "PencilPilot 0.1.0"
    pub app: String,
    /// RFC 3339 times, given by the app
    pub created: String,
    pub modified: String,
    pub canvas: Canvas,
    /// bottom → top
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub active_layer: u32,
    pub timeline: Timeline,
    pub drawings: BTreeMap<String, Drawing>,
    /// the app's own settings (current brush, colour, view…), kept as given
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// A project in memory: the manifest, the tiles it uses, the stroke log and thumbnail.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub manifest: Manifest,
    /// hash → tile bytes
    pub tiles: HashMap<String, Vec<u8>>,
    /// the stroke log, as JSON (kept as given)
    pub strokes: Vec<u8>,
    /// PNG
    pub thumbnail: Option<Vec<u8>>,
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_string()
}

impl Project {
    /// A new, empty project.
    pub fn new(app: &str, now: &str, width: u32, height: u32, fps: u32) -> Self {
        Project {
            manifest: Manifest {
                format: FORMAT.into(),
                version: VERSION,
                app: app.into(),
                created: now.into(),
                modified: now.into(),
                canvas: Canvas { width, height },
                layers: Vec::new(),
                groups: Vec::new(),
                active_layer: 0,
                timeline: Timeline {
                    fps,
                    frames: vec![Frame::default()],
                    scenes: Vec::new(),
                    static_layers: Vec::new(),
                },
                drawings: BTreeMap::new(),
                extra: serde_json::Value::Null,
            },
            tiles: HashMap::new(),
            strokes: b"[]".to_vec(),
            thumbnail: None,
        }
    }

    fn cut(&mut self, px: &[u8], channels: usize, skip: impl Fn(&[u8]) -> bool) -> Vec<TileRef> {
        let Canvas { width, height } = self.manifest.canvas;
        let (w, h) = (width as usize, height as usize);
        let t = TILE as usize;
        let mut out = Vec::new();
        let mut tile = vec![0u8; t * t * channels];
        for ty in 0..h.div_ceil(t) {
            for tx in 0..w.div_ceil(t) {
                tile.fill(if channels == 1 { 255 } else { 0 });
                for row in 0..t {
                    let y = ty * t + row;
                    if y >= h {
                        break;
                    }
                    let x0 = tx * t;
                    let n = t.min(w - x0);
                    let src = (y * w + x0) * channels;
                    let dst = row * t * channels;
                    tile[dst..dst + n * channels].copy_from_slice(&px[src..src + n * channels]);
                }
                if skip(&tile) {
                    continue;
                }
                let key = hash(&tile);
                self.tiles
                    .entry(key.clone())
                    .or_insert_with(|| tile.clone());
                out.push(TileRef {
                    x: tx as u32,
                    y: ty as u32,
                    hash: key,
                });
            }
        }
        out
    }

    /// Store a drawing from whole-canvas pixels: straight-alpha RGBA8, and an optional
    /// grey mask (one byte per pixel, 255 = shown).
    pub fn put_drawing(&mut self, id: &str, rgba: &[u8], mask: Option<&[u8]>) -> Result<()> {
        let Canvas { width, height } = self.manifest.canvas;
        let n = (width * height) as usize;
        if rgba.len() != n * 4 {
            return Err(Error::Invalid(format!("drawing {id}: wrong pixel count")));
        }
        if mask.is_some_and(|m| m.len() != n) {
            return Err(Error::Invalid(format!("drawing {id}: wrong mask size")));
        }
        let tiles = self.cut(rgba, 4, |t| t.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        let mask = mask.map(|m| self.cut(m, 1, |t| t.iter().all(|&v| v == 255)));
        self.manifest
            .drawings
            .insert(id.into(), Drawing { tiles, mask });
        Ok(())
    }

    /// Cut a layer mask (one grey byte a canvas pixel) into tiles, kept in the project;
    /// returns them for a [`Drawing`]'s `mask`.
    pub fn put_mask(&mut self, mask: &[u8]) -> Result<Vec<TileRef>> {
        let Canvas { width, height } = self.manifest.canvas;
        if mask.len() != (width * height) as usize {
            return Err(Error::Invalid("wrong mask size".into()));
        }
        Ok(self.cut(mask, 1, |t| t.iter().all(|&v| v == 255)))
    }

    fn paste(&self, refs: &[TileRef], channels: usize, fill: u8) -> Result<Vec<u8>> {
        let Canvas { width, height } = self.manifest.canvas;
        let (w, h) = (width as usize, height as usize);
        let t = TILE as usize;
        let mut px = vec![fill; w * h * channels];
        for r in refs {
            let tile = self
                .tiles
                .get(&r.hash)
                .ok_or_else(|| Error::Invalid(format!("missing tile {}", r.hash)))?;
            if tile.len() != t * t * channels {
                return Err(Error::Invalid(format!(
                    "tile {} has the wrong size",
                    r.hash
                )));
            }
            let (x0, y0) = (r.x as usize * t, r.y as usize * t);
            if x0 >= w || y0 >= h {
                return Err(Error::Invalid(format!("tile {} is off the canvas", r.hash)));
            }
            let n = t.min(w - x0);
            for row in 0..t.min(h - y0) {
                let dst = ((y0 + row) * w + x0) * channels;
                let src = row * t * channels;
                px[dst..dst + n * channels].copy_from_slice(&tile[src..src + n * channels]);
            }
        }
        Ok(px)
    }

    /// A drawing as whole-canvas pixels: RGBA8 (transparent where empty) and its mask.
    pub fn drawing(&self, id: &str) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let d = self
            .manifest
            .drawings
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("no drawing {id}")))?;
        let rgba = self.paste(&d.tiles, 4, 0)?;
        let mask = d.mask.as_ref().map(|m| self.paste(m, 1, 255)).transpose()?;
        Ok((rgba, mask))
    }

    /// Check what a file can't be trusted to get right.
    fn validate(&self) -> Result<()> {
        let m = &self.manifest;
        if m.format != FORMAT {
            return Err(Error::Invalid("not a PencilPilot project".into()));
        }
        if m.version > VERSION {
            return Err(Error::TooNew(m.version));
        }
        let Canvas { width, height } = m.canvas;
        if width == 0 || height == 0 || width > SIDE_MAX || height > SIDE_MAX {
            return Err(Error::Invalid(format!("canvas {width}×{height}")));
        }
        if m.timeline.frames.is_empty() || m.timeline.fps == 0 || m.timeline.fps > 120 {
            return Err(Error::Invalid("timeline".into()));
        }
        for layer in &m.timeline.static_layers {
            if !m.layers.iter().any(|l| l.id == *layer) {
                return Err(Error::Invalid(format!("static layer {layer} isn't there")));
            }
        }
        for cut in m.timeline.scenes.iter().flat_map(|s| &s.cuts) {
            if let Some(c) = cut.camera
                && !([c.x, c.y, c.width, c.height].iter().all(|v| v.is_finite())
                    && c.width > 0.0
                    && c.height > 0.0)
            {
                return Err(Error::Invalid("a cut's camera".into()));
            }
            for (layer, track) in &cut.tracks {
                if !m.layers.iter().any(|l| l.id == *layer) {
                    return Err(Error::Invalid(format!("a cut names layer {layer}")));
                }
                for e in track {
                    if e.length == 0 || e.length > 100_000 {
                        return Err(Error::Invalid("an exposure's length".into()));
                    }
                    if let Some(d) = &e.drawing
                        && !m.drawings.contains_key(d)
                    {
                        return Err(Error::Invalid(format!("a cut names drawing {d}")));
                    }
                }
            }
        }
        for f in &m.timeline.frames {
            for (layer, d) in &f.cels {
                if !m.layers.iter().any(|l| l.id == *layer) {
                    return Err(Error::Invalid(format!("a frame names layer {layer}")));
                }
                if !m.drawings.contains_key(d) {
                    return Err(Error::Invalid(format!("a frame names drawing {d}")));
                }
            }
        }
        for d in m.drawings.values() {
            let refs = d.tiles.iter().chain(d.mask.iter().flatten());
            for r in refs {
                if !self.tiles.contains_key(&r.hash) {
                    return Err(Error::Invalid(format!("missing tile {}", r.hash)));
                }
            }
        }
        Ok(())
    }

    /// Write the project as a zip.
    pub fn write<W: Write + Seek>(&self, w: W) -> Result<()> {
        use zip::write::SimpleFileOptions;
        let mut z = zip::ZipWriter::new(w);
        let deflate =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        // the manifest first, so a reader can check the format before anything else
        z.start_file("manifest.json", deflate)?;
        z.write_all(&serde_json::to_vec_pretty(&self.manifest)?)?;
        z.start_file("strokes.json", deflate)?;
        z.write_all(&self.strokes)?;
        if let Some(t) = &self.thumbnail {
            z.start_file("thumbnail.png", stored)?;
            z.write_all(t)?;
        }
        // only the tiles some drawing uses, in a stable order
        let mut used: Vec<&String> = self
            .manifest
            .drawings
            .values()
            .flat_map(|d| d.tiles.iter().chain(d.mask.iter().flatten()))
            .map(|r| &r.hash)
            .collect();
        used.sort();
        used.dedup();
        for h in used {
            let tile = self
                .tiles
                .get(h)
                .ok_or_else(|| Error::Invalid(format!("missing tile {h}")))?;
            z.start_file(format!("tiles/{h}"), deflate)?;
            z.write_all(tile)?;
        }
        z.finish()?;
        Ok(())
    }

    /// Read a project from a zip.
    pub fn read<R: Read + Seek>(r: R) -> Result<Self> {
        let mut z = zip::ZipArchive::new(r)?;
        let manifest: Manifest = {
            let f = z.by_name("manifest.json").map_err(|_| {
                Error::Invalid("not a PencilPilot project (no manifest.json)".into())
            })?;
            serde_json::from_reader(f.take(64 << 20))?
        };
        if manifest.format == FORMAT && manifest.version > VERSION {
            return Err(Error::TooNew(manifest.version));
        }
        let mut read_all = |name: &str, limit: u64| -> Result<Option<Vec<u8>>> {
            match z.by_name(name) {
                Ok(f) => {
                    let mut v = Vec::new();
                    f.take(limit).read_to_end(&mut v)?;
                    Ok(Some(v))
                }
                Err(zip::result::ZipError::FileNotFound) => Ok(None),
                Err(e) => Err(e.into()),
            }
        };
        let strokes = read_all("strokes.json", 512 << 20)?.unwrap_or_else(|| b"[]".to_vec());
        let thumbnail = read_all("thumbnail.png", 16 << 20)?;
        let mut tiles = HashMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            let Some(h) = f.name()?.strip_prefix("tiles/").map(str::to_owned) else {
                continue;
            };
            let mut v = Vec::with_capacity(RGBA_TILE);
            (&mut f).take(RGBA_TILE as u64 + 1).read_to_end(&mut v)?;
            if v.len() != RGBA_TILE && v.len() != MASK_TILE {
                return Err(Error::Invalid(format!("tile {h} has the wrong size")));
            }
            tiles.insert(h, v);
        }
        let p = Project {
            manifest,
            tiles,
            strokes,
            thumbnail,
        };
        p.validate()?;
        Ok(p)
    }

    /// Save to a file safely: written next to it first, then moved into place, so a
    /// crash never leaves a half-written project.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("ppstudio.saving");
        let mut w = std::io::BufWriter::new(fs::File::create(&tmp)?);
        self.write(&mut w)?;
        // on the disk before it replaces the old file
        w.into_inner()
            .map_err(|e| Error::Io(e.into_error()))?
            .sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Just the manifest and thumbnail, without reading any pixels (for lists of files).
    pub fn peek(path: &Path) -> Result<(Manifest, Option<Vec<u8>>)> {
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(fs::File::open(path)?))?;
        let manifest: Manifest = {
            let f = z.by_name("manifest.json").map_err(|_| {
                Error::Invalid("not a PencilPilot project (no manifest.json)".into())
            })?;
            serde_json::from_reader(f.take(64 << 20))?
        };
        if manifest.format != FORMAT {
            return Err(Error::Invalid("not a PencilPilot project".into()));
        }
        if manifest.version > VERSION {
            return Err(Error::TooNew(manifest.version));
        }
        let thumbnail = match z.by_name("thumbnail.png") {
            Ok(f) => {
                let mut v = Vec::new();
                f.take(16 << 20).read_to_end(&mut v)?;
                Some(v)
            }
            Err(zip::result::ZipError::FileNotFound) => None,
            Err(e) => return Err(e.into()),
        };
        Ok((manifest, thumbnail))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let f = fs::File::open(path)?;
        Project::read(std::io::BufReader::new(f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn layer(id: u32) -> Layer {
        Layer {
            id,
            name: format!("Layer {id}"),
            visible: true,
            opacity: 1.0,
            blend: "normal".into(),
            locked: false,
            alpha_lock: false,
            clip: false,
            imported: false,
            group: 0,
            adjust: None,
        }
    }

    /// a canvas that isn't a multiple of 64, with a red square at (70, 10)
    fn sample() -> (Project, Vec<u8>) {
        let (w, h) = (150u32, 100u32);
        let mut p = Project::new("test", "2026-11-21T00:00:00Z", w, h, 24);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 10..30 {
            for x in 70..90 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[220, 30, 30, 255]);
            }
        }
        p.manifest.layers.push(layer(1));
        p.put_drawing("d1", &rgba, None).unwrap();
        p.manifest.timeline.frames[0].cels.insert(1, "d1".into());
        (p, rgba)
    }

    #[test]
    fn round_trips_pixels_and_settings() {
        let (mut p, rgba) = sample();
        p.strokes = br#"[{"brushId":"pen","points":[]}]"#.to_vec();
        p.thumbnail = Some(vec![137, 80, 78, 71]);
        p.manifest.extra = serde_json::json!({ "color": "#16161a" });
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest, p.manifest);
        assert_eq!(back.strokes, p.strokes);
        assert_eq!(back.thumbnail, p.thumbnail);
        assert_eq!(back.drawing("d1").unwrap().0, rgba);
    }

    #[test]
    fn empty_tiles_are_skipped_and_repeats_stored_once() {
        let (mut p, rgba) = sample();
        // the square sits in tile (1, 0) only: one tile of six
        assert_eq!(p.manifest.drawings["d1"].tiles.len(), 1);
        // the same picture again (a held cel): no new tiles
        let before = p.tiles.len();
        p.manifest.layers.push(layer(2));
        p.put_drawing("d2", &rgba, None).unwrap();
        assert_eq!(p.tiles.len(), before);
        // an empty drawing costs nothing
        p.put_drawing("blank", &vec![0; rgba.len()], None).unwrap();
        assert!(p.manifest.drawings["blank"].tiles.is_empty());
    }

    #[test]
    fn masks_round_trip_and_full_masks_cost_nothing() {
        let (mut p, rgba) = sample();
        let n = (150 * 100) as usize;
        let mut mask = vec![255u8; n];
        mask[0] = 0; // hide one pixel
        p.put_drawing("d1", &rgba, Some(&mask)).unwrap();
        assert_eq!(p.manifest.drawings["d1"].mask.as_ref().unwrap().len(), 1);
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.drawing("d1").unwrap().1.unwrap(), mask);
    }

    #[test]
    fn refuses_other_files_newer_versions_and_damage() {
        assert!(matches!(
            Project::read(Cursor::new(b"not a zip".to_vec())),
            Err(Error::Zip(_))
        ));
        let (mut p, _) = sample();
        p.manifest.version = VERSION + 1;
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        assert!(matches!(
            Project::read(Cursor::new(buf.into_inner())),
            Err(Error::TooNew(v)) if v == VERSION + 1
        ));
        // a frame naming a drawing that isn't there
        let (mut p, _) = sample();
        p.manifest.timeline.frames[0].cels.insert(1, "nope".into());
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        assert!(matches!(
            Project::read(Cursor::new(buf.into_inner())),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn saves_atomically_to_disk() {
        let (p, rgba) = sample();
        let dir = std::env::temp_dir().join(format!("ppstudio-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drawing.ppstudio");
        p.save(&path).unwrap();
        assert!(!path.with_extension("ppstudio.saving").exists());
        assert_eq!(Project::open(&path).unwrap().drawing("d1").unwrap().0, rgba);
        let (m, thumb) = Project::peek(&path).unwrap();
        assert_eq!(m, p.manifest);
        assert_eq!(thumb, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_1080p_line_drawing_stays_small() {
        let (w, h) = (1920u32, 1080u32);
        let mut p = Project::new("test", "now", w, h, 24);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for x in 200..1700 {
            for y in 500..504 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[20, 20, 26, 255]);
            }
        }
        p.manifest.layers.push(layer(1));
        p.put_drawing("d1", &rgba, None).unwrap();
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let size = buf.into_inner().len();
        assert!(size < 64 * 1024, "{size} bytes");
    }

    #[test]
    fn scenes_flatten_into_frames_and_are_checked() {
        let e = |d: Option<&str>, length| Exposure {
            drawing: d.map(String::from),
            length,
        };
        let cut = |tracks: Vec<(u32, Vec<Exposure>)>| Cut {
            name: "C".into(),
            tracks: tracks.into_iter().collect(),
            camera: None,
        };
        // cut 1: layer 1 on twos (A A B B), layer 2 held all along; cut 2: one frame
        let scenes = vec![Scene {
            name: "S1".into(),
            cuts: vec![
                cut(vec![
                    (1, vec![e(Some("A"), 2), e(Some("B"), 2)]),
                    (2, vec![e(Some("BG"), 4)]),
                ]),
                cut(vec![(1, vec![e(None, 1)])]),
            ],
        }];
        let frames = Timeline::flatten(&scenes, &[]);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            (frames[0].cels.get(&1).unwrap().as_str(), frames[0].hold),
            ("A", 2)
        );
        assert_eq!(
            (frames[1].cels.get(&1).unwrap().as_str(), frames[1].hold),
            ("B", 2)
        );
        assert!(frames[2].cels.is_empty() && frames[2].hold == 1);

        let mut p = Project::new("test", "2026-11-21T00:00:00Z", 64, 64, 24);
        p.manifest.layers = vec![layer(1), layer(2)];
        for d in ["A", "B", "BG"] {
            p.put_drawing(d, &vec![0u8; 64 * 64 * 4], None).unwrap();
        }
        p.manifest.timeline.frames = frames;
        p.manifest.timeline.scenes = scenes;
        let mut buf = std::io::Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.timeline, p.manifest.timeline);
        // a cut naming a drawing that isn't there is damage
        p.manifest.timeline.scenes[0].cuts[1]
            .tracks
            .insert(1, vec![e(Some("Z"), 1)]);
        let mut buf = std::io::Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        assert!(Project::read(std::io::Cursor::new(buf.into_inner())).is_err());
    }

    #[test]
    fn static_layers_hold_one_drawing_and_cuts_keep_their_camera() {
        let e = |d: Option<&str>, length| Exposure {
            drawing: d.map(String::from),
            length,
        };
        // layer 1 animates A B C; layer 2 is a static background (its later drawings
        // and lengths don't matter)
        let scenes = vec![Scene {
            name: "S1".into(),
            cuts: vec![Cut {
                name: "C1".into(),
                tracks: [
                    (1, vec![e(Some("A"), 1), e(Some("B"), 1), e(Some("C"), 1)]),
                    (2, vec![e(None, 1), e(Some("BG"), 9), e(Some("X"), 1)]),
                ]
                .into_iter()
                .collect(),
                camera: Some(Camera {
                    x: 8.0,
                    y: 4.0,
                    width: 48.0,
                    height: 27.0,
                }),
            }],
        }];
        assert_eq!(scenes[0].cuts[0].length(&[]), 11);
        assert_eq!(scenes[0].cuts[0].length(&[2]), 3);
        let frames = Timeline::flatten(&scenes, &[2]);
        assert_eq!(frames.len(), 3);
        for (f, d) in frames.iter().zip(["A", "B", "C"]) {
            assert_eq!(f.cels.get(&1).unwrap(), d);
            assert_eq!(f.cels.get(&2).unwrap(), "BG");
        }

        let mut p = Project::new("test", "2026-11-21T00:00:00Z", 64, 64, 24);
        p.manifest.layers = vec![layer(1), layer(2)];
        for d in ["A", "B", "C", "BG", "X"] {
            p.put_drawing(d, &vec![0u8; 64 * 64 * 4], None).unwrap();
        }
        p.manifest.timeline.frames = frames;
        p.manifest.timeline.scenes = scenes;
        p.manifest.timeline.static_layers = vec![2];
        let mut buf = std::io::Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.timeline, p.manifest.timeline);

        // a static layer that isn't there, or a camera with no size, is damage
        for bad in 0..2 {
            let mut q = p.clone();
            if bad == 0 {
                q.manifest.timeline.static_layers = vec![9];
            } else {
                q.manifest.timeline.scenes[0].cuts[0]
                    .camera
                    .as_mut()
                    .unwrap()
                    .width = 0.0;
            }
            let mut buf = std::io::Cursor::new(Vec::new());
            q.write(&mut buf).unwrap();
            assert!(Project::read(std::io::Cursor::new(buf.into_inner())).is_err());
        }
    }
}
