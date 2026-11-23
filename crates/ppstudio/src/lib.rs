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
//! | `audio/<hash>.<ext>` | a sound file a scene plays (as imported: WAV, MP3, OGG…) |
//! | `fonts/<hash>.<ext>` | a font file text layers use (as imported: TTF, OTF, WOFF2) |
//! | `references/<hash>.<ext>` | a reference picture the user imported to draw from (as imported) |
//! | `lines/<drawing>` | a drawing's editable strokes (the app's own data, kept as given) |
//! | `timelapse/<n>.jpg` | the timelapse: a small picture after each change (optional) |
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
    /// its layer style: stroke, drop shadow and outer glow (the app's JSON object)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<serde_json::Value>,
    /// a text layer's text (the app's JSON: words, font, size…); none for other layers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// a line (vector) layer: the app keeps its strokes (`lines/<drawing>`) and draws
    /// its pixels from them; its drawings hold those pixels too
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lines: bool,
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
    /// mouth charts for lip-sync: which drawing of a layer is each mouth shape
    #[serde(default, rename = "mouthCharts", skip_serializing_if = "Vec::is_empty")]
    pub mouth_charts: Vec<MouthChart>,
    /// what drawings are in the animation: drawing id → "key", "breakdown" or "inbetween"
    #[serde(
        default,
        rename = "drawingKinds",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub drawing_kinds: BTreeMap<String, String>,
}

/// What a drawing can be marked as.
pub const DRAWING_KINDS: [&str; 3] = ["key", "breakdown", "inbetween"];

/// The mouth shapes lip-sync uses (Rhubarb's: A–F, and the extended G, H and X).
pub const MOUTH_SHAPES: &str = "ABCDEFGHX";

/// A mouth chart: the drawing of a layer drawn for each mouth shape.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MouthChart {
    pub layer: u32,
    /// shape ("A"…"H", "X") → drawing id
    pub shapes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    pub cuts: Vec<Cut>,
    /// sound: clips placed along the scene (its cuts one after another)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<AudioClip>,
    /// sync markers on the scene's frames (beats, accents, lines)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

/// A sound file placed in a scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub name: String,
    /// the file in the archive: `audio/<file>`
    pub file: String,
    /// the scene frame it starts on (negative: it started before the scene)
    #[serde(default)]
    pub start: i64,
    /// loudness (1 = as recorded)
    #[serde(default = "one_f")]
    pub gain: f32,
    #[serde(default)]
    pub muted: bool,
}

fn one_f() -> f32 {
    1.0
}

/// A marker on a scene frame.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Marker {
    pub frame: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
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
    /// exposure-sheet notes, frame → text: what happens
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action: BTreeMap<u32, String>,
    /// exposure-sheet notes, frame → text: what's said
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dialogue: BTreeMap<u32, String>,
    /// how the cut goes into the next one: "flash" or "dissolve" (none: a hard cut)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
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
    /// when each timelapse picture was taken (ms since 1970), in order: `timelapse/<n>.jpg`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timelapse: Vec<u64>,
    /// font files imported for text layers: `fonts/<file>`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<FontFile>,
    /// reference pictures the user imported to draw from, in order: `references/<file>`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<RefFile>,
}

/// A reference picture kept in the project (never part of the drawing or its exports).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefFile {
    /// the picture's name as imported (shown to the user)
    pub name: String,
    /// the file in the archive: `references/<file>`
    pub file: String,
}

/// A font file kept in the project, and the family name text layers know it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontFile {
    pub family: String,
    /// the file in the archive: `fonts/<file>`
    pub file: String,
}

/// The most timelapse pictures a project keeps, and the most bytes each may have.
pub const TIMELAPSE_MAX: usize = 4000;
pub const TIMELAPSE_FRAME_MAX: usize = 8 << 20;

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
    /// sound files: name (`<hash>.<ext>`) → bytes
    pub audio: HashMap<String, Vec<u8>>,
    /// the timelapse pictures (JPEG), in order; `manifest.timelapse` says when each was taken
    pub timelapse: Vec<Vec<u8>>,
    /// font files: name (`<hash>.<ext>`) → bytes
    pub fonts: HashMap<String, Vec<u8>>,
    /// reference pictures: name (`<hash>.<ext>`) → bytes
    pub references: HashMap<String, Vec<u8>>,
    /// line layers' strokes: drawing name (`D<n>`, a key of `manifest.drawings`) → the
    /// app's data for it (its own versioned binary, kept as given)
    pub lines: HashMap<String, Vec<u8>>,
}

/// Font files may be this big (each).
pub const FONT_MAX: usize = 64 << 20;

/// Reference pictures may be this big (each), and a project may keep this many.
pub const REFERENCE_MAX: usize = 64 << 20;
pub const REFERENCES_MAX: usize = 200;

/// A drawing's line data may be this big.
pub const LINES_MAX: usize = 256 << 20;

/// A drawing name line data may be kept under: `D<digits>`, no folders.
fn lines_name_ok(name: &str) -> bool {
    name.strip_prefix('D')
        .is_some_and(|n| (1..=10).contains(&n.len()) && n.chars().all(|c| c.is_ascii_digit()))
}

/// Sound files may be this big (each).
pub const AUDIO_MAX: usize = 512 << 20;

/// A safe sound or font file name: `<hex hash>.<ext>`, no folders.
fn audio_name_ok(name: &str) -> bool {
    let Some((h, ext)) = name.split_once('.') else {
        return false;
    };
    !h.is_empty()
        && h.chars().all(|c| c.is_ascii_hexdigit())
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
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
                    mouth_charts: Vec::new(),
                    drawing_kinds: BTreeMap::new(),
                },
                drawings: BTreeMap::new(),
                extra: serde_json::Value::Null,
                timelapse: Vec::new(),
                fonts: Vec::new(),
                references: Vec::new(),
            },
            tiles: HashMap::new(),
            strokes: b"[]".to_vec(),
            thumbnail: None,
            audio: HashMap::new(),
            timelapse: Vec::new(),
            fonts: HashMap::new(),
            references: HashMap::new(),
            lines: HashMap::new(),
        }
    }

    /// Keep a sound file (stored once, by its content); returns its name in the archive.
    pub fn put_audio(&mut self, bytes: &[u8], ext: &str) -> Result<String> {
        let ext: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(5)
            .collect::<String>()
            .to_ascii_lowercase();
        let name = format!(
            "{}.{}",
            hash(bytes),
            if ext.is_empty() { "bin" } else { &ext }
        );
        if bytes.len() > AUDIO_MAX {
            return Err(Error::Invalid(format!("sound file {name} is too big")));
        }
        self.audio
            .entry(name.clone())
            .or_insert_with(|| bytes.to_vec());
        Ok(name)
    }

    /// Keep a font file for text layers (stored once, by its content) under the family
    /// name they use for it.
    pub fn put_font(&mut self, family: &str, bytes: &[u8], ext: &str) -> Result<()> {
        let ext: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(5)
            .collect::<String>()
            .to_ascii_lowercase();
        let file = format!(
            "{}.{}",
            hash(bytes),
            if ext.is_empty() { "ttf" } else { &ext }
        );
        if bytes.len() > FONT_MAX {
            return Err(Error::Invalid(format!("font file {file} is too big")));
        }
        let family: String = family.trim().chars().take(100).collect();
        if family.is_empty() {
            return Err(Error::Invalid("a font with no name".into()));
        }
        self.fonts
            .entry(file.clone())
            .or_insert_with(|| bytes.to_vec());
        if !self.manifest.fonts.iter().any(|f| f.family == family) {
            self.manifest.fonts.push(FontFile { family, file });
        }
        Ok(())
    }

    /// Keep a reference picture (stored once, by its content) after the others, under the
    /// name it was imported with. The same picture twice is kept once.
    pub fn put_reference(&mut self, name: &str, bytes: &[u8], ext: &str) -> Result<()> {
        let ext: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(5)
            .collect::<String>()
            .to_ascii_lowercase();
        let file = format!(
            "{}.{}",
            hash(bytes),
            if ext.is_empty() { "img" } else { &ext }
        );
        if bytes.len() > REFERENCE_MAX {
            return Err(Error::Invalid(format!(
                "reference picture {file} is too big"
            )));
        }
        if self.manifest.references.iter().any(|r| r.file == file) {
            return Ok(());
        }
        if self.manifest.references.len() >= REFERENCES_MAX {
            return Err(Error::Invalid("too many reference pictures".into()));
        }
        let name: String = name.trim().chars().take(200).collect();
        self.references
            .entry(file.clone())
            .or_insert_with(|| bytes.to_vec());
        self.manifest.references.push(RefFile {
            name: if name.is_empty() {
                "Reference".into()
            } else {
                name
            },
            file,
        });
        Ok(())
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
        for s in &m.timeline.scenes {
            for a in &s.audio {
                if !audio_name_ok(&a.file) || !self.audio.contains_key(&a.file) {
                    return Err(Error::Invalid(format!(
                        "a scene names sound file {}",
                        a.file
                    )));
                }
                if !a.gain.is_finite() || a.gain < 0.0 || a.gain > 16.0 {
                    return Err(Error::Invalid("a sound's loudness".into()));
                }
            }
        }
        for f in &m.fonts {
            if !audio_name_ok(&f.file)
                || !self.fonts.contains_key(&f.file)
                || f.family.trim().is_empty()
                || f.family.chars().count() > 100
            {
                return Err(Error::Invalid(format!("font {}", f.file)));
            }
        }
        if m.references.len() > REFERENCES_MAX {
            return Err(Error::Invalid("too many reference pictures".into()));
        }
        for r in &m.references {
            if !audio_name_ok(&r.file)
                || !self.references.contains_key(&r.file)
                || r.name.chars().count() > 200
            {
                return Err(Error::Invalid(format!("reference picture {}", r.file)));
            }
        }
        for (name, data) in &self.lines {
            if !lines_name_ok(name) || !m.drawings.contains_key(name) || data.len() > LINES_MAX {
                return Err(Error::Invalid(format!("line data {name}")));
            }
        }
        if m.timelapse.len() != self.timelapse.len()
            || self.timelapse.len() > TIMELAPSE_MAX
            || self.timelapse.iter().any(|j| j.len() > TIMELAPSE_FRAME_MAX)
        {
            return Err(Error::Invalid("the timelapse".into()));
        }
        for layer in &m.timeline.static_layers {
            if !m.layers.iter().any(|l| l.id == *layer) {
                return Err(Error::Invalid(format!("static layer {layer} isn't there")));
            }
        }
        for (d, kind) in &m.timeline.drawing_kinds {
            if !m.drawings.contains_key(d) || !DRAWING_KINDS.contains(&kind.as_str()) {
                return Err(Error::Invalid(format!("drawing mark {d}: {kind}")));
            }
        }
        for chart in &m.timeline.mouth_charts {
            if !m.layers.iter().any(|l| l.id == chart.layer) {
                return Err(Error::Invalid(format!(
                    "a mouth chart names layer {}",
                    chart.layer
                )));
            }
            for (shape, d) in &chart.shapes {
                if shape.len() != 1 || !MOUTH_SHAPES.contains(shape.as_str()) {
                    return Err(Error::Invalid(format!("mouth shape {shape}")));
                }
                if !m.drawings.contains_key(d) {
                    return Err(Error::Invalid(format!("a mouth chart names drawing {d}")));
                }
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
            if let Some(t) = &cut.transition
                && t != "flash"
                && t != "dissolve"
            {
                return Err(Error::Invalid(format!("a cut's transition: {t}")));
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
        // the sound files the scenes use (already compressed, mostly: stored)
        let mut sounds: Vec<&String> = self
            .manifest
            .timeline
            .scenes
            .iter()
            .flat_map(|s| s.audio.iter().map(|a| &a.file))
            .collect();
        sounds.sort();
        sounds.dedup();
        for name in sounds {
            let bytes = self
                .audio
                .get(name)
                .ok_or_else(|| Error::Invalid(format!("missing sound file {name}")))?;
            z.start_file(format!("audio/{name}"), stored)?;
            z.write_all(bytes)?;
        }
        // the font files text layers use (compressed already, mostly: stored)
        let mut fonts: Vec<&String> = self.manifest.fonts.iter().map(|f| &f.file).collect();
        fonts.sort();
        fonts.dedup();
        for name in fonts {
            let bytes = self
                .fonts
                .get(name)
                .ok_or_else(|| Error::Invalid(format!("missing font file {name}")))?;
            z.start_file(format!("fonts/{name}"), stored)?;
            z.write_all(bytes)?;
        }
        // reference pictures (compressed already: stored)
        let mut refs: Vec<&String> = self.manifest.references.iter().map(|r| &r.file).collect();
        refs.sort();
        refs.dedup();
        for name in refs {
            let bytes = self
                .references
                .get(name)
                .ok_or_else(|| Error::Invalid(format!("missing reference picture {name}")))?;
            z.start_file(format!("references/{name}"), stored)?;
            z.write_all(bytes)?;
        }
        // line layers' strokes, for the drawings that are there
        let mut lines: Vec<(&String, &Vec<u8>)> = self
            .lines
            .iter()
            .filter(|(name, _)| lines_name_ok(name) && self.manifest.drawings.contains_key(*name))
            .collect();
        lines.sort();
        for (name, data) in lines {
            z.start_file(format!("lines/{name}"), deflate)?;
            z.write_all(data)?;
        }
        // the timelapse (JPEG: stored)
        for (i, jpeg) in self.timelapse.iter().enumerate() {
            z.start_file(format!("timelapse/{i:06}.jpg"), stored)?;
            z.write_all(jpeg)?;
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
        let mut audio = HashMap::new();
        let mut fonts = HashMap::new();
        let mut references = HashMap::new();
        let mut lines = HashMap::new();
        let mut timelapse: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if let Some(n) = f
                .name()?
                .strip_prefix("timelapse/")
                .and_then(|n| n.strip_suffix(".jpg"))
                .and_then(|n| n.parse::<usize>().ok())
            {
                let mut v = Vec::new();
                (&mut f)
                    .take(TIMELAPSE_FRAME_MAX as u64 + 1)
                    .read_to_end(&mut v)?;
                if n < TIMELAPSE_MAX && v.len() <= TIMELAPSE_FRAME_MAX {
                    timelapse.insert(n, v);
                }
                continue;
            }
            if let Some(name) = f.name()?.strip_prefix("audio/").map(str::to_owned) {
                let mut v = Vec::new();
                (&mut f).take(AUDIO_MAX as u64 + 1).read_to_end(&mut v)?;
                if v.len() > AUDIO_MAX || !audio_name_ok(&name) {
                    return Err(Error::Invalid(format!("sound file {name}")));
                }
                audio.insert(name, v);
                continue;
            }
            if let Some(name) = f.name()?.strip_prefix("fonts/").map(str::to_owned) {
                let mut v = Vec::new();
                (&mut f).take(FONT_MAX as u64 + 1).read_to_end(&mut v)?;
                if v.len() > FONT_MAX || !audio_name_ok(&name) {
                    return Err(Error::Invalid(format!("font file {name}")));
                }
                fonts.insert(name, v);
                continue;
            }
            if let Some(name) = f.name()?.strip_prefix("references/").map(str::to_owned) {
                let mut v = Vec::new();
                (&mut f)
                    .take(REFERENCE_MAX as u64 + 1)
                    .read_to_end(&mut v)?;
                if v.len() > REFERENCE_MAX || !audio_name_ok(&name) {
                    return Err(Error::Invalid(format!("reference picture {name}")));
                }
                references.insert(name, v);
                continue;
            }
            if let Some(name) = f.name()?.strip_prefix("lines/").map(str::to_owned) {
                let mut v = Vec::new();
                (&mut f).take(LINES_MAX as u64 + 1).read_to_end(&mut v)?;
                if v.len() > LINES_MAX || !lines_name_ok(&name) {
                    return Err(Error::Invalid(format!("line data {name}")));
                }
                lines.insert(name, v);
                continue;
            }
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
        // (the timelapse is a nicety: pictures that don't line up with their times are
        // dropped rather than refusing the project)
        let mut manifest = manifest;
        let mut pictures = Vec::new();
        let mut times = Vec::new();
        for (i, t) in manifest.timelapse.iter().enumerate().take(TIMELAPSE_MAX) {
            match timelapse.remove(&i) {
                Some(v) => {
                    pictures.push(v);
                    times.push(*t);
                }
                None => break,
            }
        }
        manifest.timelapse = times;
        let p = Project {
            manifest,
            tiles,
            strokes,
            thumbnail,
            audio,
            timelapse: pictures,
            fonts,
            references,
            lines,
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
            style: None,
            text: None,
            lines: false,
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
    fn line_layers_and_their_data_round_trip() {
        let (mut p, rgba) = sample();
        p.manifest.layers[0].lines = true;
        p.manifest.drawings = BTreeMap::new();
        p.put_drawing("D3", &rgba, None).unwrap();
        p.manifest.timeline.frames[0].cels.insert(1, "D3".into());
        let data = b"PPL1\x00\x01\x02 strokes".to_vec();
        p.lines.insert("D3".into(), data.clone());
        // data for a drawing that isn't there isn't written
        p.lines.insert("D9".into(), vec![1, 2, 3]);
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.layers, p.manifest.layers);
        assert!(back.manifest.layers[0].lines);
        assert_eq!(back.lines.len(), 1);
        assert_eq!(back.lines["D3"], data);
        assert_eq!(back.drawing("D3").unwrap().0, rgba);
        // plain layers don't say so; older files have no line data
        let json = serde_json::to_string(&layer(2)).unwrap();
        assert!(!json.contains("lines"));
        let old: Layer = serde_json::from_str(&json).unwrap();
        assert!(!old.lines);
        // data for a missing drawing, or under a name that isn't a drawing's, is damage
        for name in ["D9", "../D3", "x"] {
            let mut q = back.clone();
            q.lines.insert(name.into(), vec![0]);
            assert!(q.validate().is_err(), "{name}");
        }
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
            ..Default::default()
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
            ..Default::default()
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
                action: [(0, "turns".to_string())].into_iter().collect(),
                dialogue: [(1, "hel-".to_string()), (2, "lo".to_string())]
                    .into_iter()
                    .collect(),
                transition: Some("flash".into()),
            }],
            ..Default::default()
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
        // a sound file and a marker in the scene
        let file = p.put_audio(b"RIFF fake wav", "WAV").unwrap();
        assert!(file.ends_with(".wav"));
        assert_eq!(
            p.put_audio(b"RIFF fake wav", "wav").unwrap(),
            file,
            "stored once"
        );
        p.manifest.timeline.scenes[0].audio.push(AudioClip {
            name: "line.wav".into(),
            file: file.clone(),
            start: -3,
            gain: 0.8,
            muted: false,
        });
        p.manifest.timeline.scenes[0].markers.push(Marker {
            frame: 2,
            label: "beat".into(),
        });
        // a mouth chart, with a drawing ("X") no frame shows
        p.manifest.timeline.mouth_charts.push(MouthChart {
            layer: 1,
            shapes: [("A".to_string(), "A".to_string()), ("X".into(), "X".into())]
                .into_iter()
                .collect(),
        });
        // a key and a breakdown
        p.manifest.timeline.drawing_kinds = [
            ("A".to_string(), "key".to_string()),
            ("B".into(), "breakdown".into()),
        ]
        .into_iter()
        .collect();
        // a timelapse of two pictures
        p.timelapse = vec![b"\xff\xd8one".to_vec(), b"\xff\xd8two".to_vec()];
        p.manifest.timelapse = vec![1_700_000_000_000, 1_700_000_005_000];
        let mut buf = std::io::Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.timeline, p.manifest.timeline);
        assert_eq!(back.audio.get(&file).unwrap(), b"RIFF fake wav");
        assert_eq!(back.timelapse, p.timelapse);
        assert_eq!(back.manifest.timelapse, p.manifest.timelapse);
        // a text layer and the font it uses
        let mut q = p.clone();
        q.manifest.layers[0].text = Some(r#"{"text":"Hi","font":"Comic"}"#.into());
        q.put_font("Comic", b"OTTO fake font", "OTF").unwrap();
        q.put_font("Comic", b"OTTO fake font", "otf").unwrap();
        assert_eq!(q.manifest.fonts.len(), 1, "kept once");
        let mut buf = std::io::Cursor::new(Vec::new());
        q.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.layers, q.manifest.layers);
        assert_eq!(back.manifest.fonts, q.manifest.fonts);
        let file = &back.manifest.fonts[0].file;
        assert!(file.ends_with(".otf"));
        assert_eq!(back.fonts.get(file).unwrap(), b"OTTO fake font");
        // a font the file names but doesn't hold
        q.fonts.clear();
        assert!(q.validate().is_err());
        // reference pictures: kept in order, once each, and back
        let mut r = p.clone();
        r.put_reference("cat.png", b"\x89PNG cat", "PNG").unwrap();
        r.put_reference(" dog photo ", b"\xff\xd8 dog", "jpg")
            .unwrap();
        r.put_reference("cat again.png", b"\x89PNG cat", "png")
            .unwrap();
        assert_eq!(
            r.manifest.references.len(),
            2,
            "the same picture is kept once"
        );
        assert_eq!(r.manifest.references[1].name, "dog photo");
        let mut buf = std::io::Cursor::new(Vec::new());
        r.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.references, r.manifest.references);
        let file = &back.manifest.references[0].file;
        assert!(file.ends_with(".png"));
        assert_eq!(back.references.get(file).unwrap(), b"\x89PNG cat");
        // a picture the file names but doesn't hold
        r.references.clear();
        assert!(r.validate().is_err());
        // more times than pictures: the timelapse is cut short, the project still opens
        let mut q = p.clone();
        q.manifest.timelapse.push(1_700_000_009_000);
        assert!(q.validate().is_err());
        let mut buf = std::io::Cursor::new(Vec::new());
        q.write(&mut buf).unwrap();
        let back = Project::read(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(back.manifest.timelapse.len(), 2);

        // a static layer that isn't there, a camera with no size, a sound file that isn't
        // in the archive, or a mouth chart naming an unknown shape, layer or drawing is
        // damage
        for bad in 0..6 {
            let mut q = p.clone();
            let chart = &mut q.manifest.timeline.mouth_charts[0];
            if bad == 0 {
                q.manifest.timeline.static_layers = vec![9];
            } else if bad == 3 {
                chart.shapes.insert("Q".into(), "A".into());
                q.manifest
                    .timeline
                    .drawing_kinds
                    .insert("A".into(), "wobbly".into());
            } else if bad == 4 {
                chart.layer = 9;
                q.manifest.timeline.scenes[0].cuts[0].transition = Some("wipe".into());
            } else if bad == 5 {
                chart.shapes.insert("B".into(), "NOPE".into());
            } else if bad == 2 {
                // (a name that climbs out of audio/)
                q.manifest.timeline.scenes[0].audio[0].file = "../x.wav".into();
                q.audio.insert("../x.wav".into(), vec![1]);
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
