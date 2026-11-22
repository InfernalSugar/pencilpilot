//! Saving and opening `.ppstudio` projects for the window.
//!
//! Layer pixels are large (8 MB a 1080p layer), so they cross between the window and
//! here as raw bytes, never JSON. Both directions use one packet:
//!
//! ```text
//! "PPS1"  u32 (little-endian) header length  header (JSON)  blobs…
//! ```
//!
//! The header describes the document; each layer's `rgba` / `mask` and the thumbnail
//! are `[offset, length]` into the blobs. `apps/studio/src/lib/project.ts` is the other
//! end.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anim::FrameStore;
use ppstudio::{Canvas, Frame, Group, Layer, MouthChart, Project, Scene, Timeline};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"PPS1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketLayer {
    #[serde(flatten)]
    pub layer: Layer,
    /// [offset, length] of whole-canvas RGBA8 in the blobs
    pub rgba: [usize; 2],
    /// [offset, length] of the whole-canvas grey mask, if it has one
    #[serde(default)]
    pub mask: Option<[usize; 2]>,
}

/// One animation drawing (cel): its id ("D<id>" in the file), its layer, its pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketDrawing {
    pub id: u32,
    pub layer: u32,
    /// [offset, length] of whole-canvas RGBA8 in the blobs; none: it's in the frame
    /// store (the app's [`FrameStore`])
    #[serde(default)]
    pub rgba: Option<[usize; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub canvas: Canvas,
    pub fps: u32,
    pub layers: Vec<PacketLayer>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub active_layer: u32,
    /// the stroke log (a JSON array)
    #[serde(default)]
    pub strokes: serde_json::Value,
    #[serde(default)]
    pub extra: serde_json::Value,
    /// when the project was first saved (kept across saves)
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<[usize; 2]>,
    /// the animation (drawings named "D<id>"); none: one frame of the layers as they are
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Scene>,
    /// every drawing the scenes show
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drawings: Vec<PacketDrawing>,
    /// static layers (one drawing all cut long)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_layers: Vec<u32>,
    /// the sound files the scenes play
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<PacketAudio>,
    /// lip-sync mouth charts (their drawings are among `drawings`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mouth_charts: Vec<MouthChart>,
    /// drawing marks: drawing name → "key", "breakdown" or "inbetween"
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub drawing_kinds: std::collections::BTreeMap<String, String>,
    /// the timelapse: JPEG pictures and when each was taken (ms since 1970)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timelapse: Vec<PacketTimelapse>,
    /// font files imported for text layers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<PacketFont>,
}

/// A font file text layers use: the family name they know it by, the file's name as
/// imported (for its extension; saved, it's named by its content), and its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketFont {
    pub family: String,
    pub file: String,
    pub bytes: [usize; 2],
}

/// A timelapse picture: when it was taken, and its JPEG bytes in the blobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketTimelapse {
    pub t: u64,
    pub bytes: [usize; 2],
}

/// A sound file: the name the scenes use for it, and its bytes in the blobs. (Saved,
/// it's renamed by its content; scenes are updated to match.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketAudio {
    pub file: String,
    pub bytes: [usize; 2],
}

pub fn encode(header: &Header, blobs: &[u8]) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(header).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(8 + json.len() + blobs.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(blobs);
    Ok(out)
}

pub fn decode(packet: &[u8]) -> Result<(Header, &[u8]), String> {
    if packet.len() < 8 || &packet[..4] != MAGIC {
        return Err("not a project packet".into());
    }
    let n = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    let json = packet
        .get(8..8 + n)
        .ok_or("the packet ends inside its header")?;
    let header: Header = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    Ok((header, &packet[8 + n..]))
}

fn slice(blobs: &[u8], at: [usize; 2]) -> Result<&[u8], String> {
    blobs
        .get(at[0]..at[0].saturating_add(at[1]))
        .ok_or_else(|| "a layer points past the end of the packet".into())
}

/// The window's document → a project file's contents.
pub fn to_project(
    packet: &[u8],
    app: &str,
    now: &str,
    mut frames: Option<&mut FrameStore>,
) -> Result<Project, String> {
    let (h, blobs) = decode(packet)?;
    let mut p = Project::new(app, now, h.canvas.width, h.canvas.height, h.fps.max(1));
    if let Some(created) = h.created {
        p.manifest.created = created;
    }
    if h.scenes.is_empty() {
        // one frame: the layers as they are
        let mut frame = Frame::default();
        for l in &h.layers {
            let id = format!("L{}", l.layer.id);
            let mask = l.mask.map(|m| slice(blobs, m)).transpose()?;
            p.put_drawing(&id, slice(blobs, l.rgba)?, mask)
                .map_err(|e| e.to_string())?;
            frame.cels.insert(l.layer.id, id);
        }
        p.manifest.timeline.frames = vec![frame];
    } else {
        // an animation: every drawing (each with its layer's mask), and the scenes
        let mut masks: std::collections::HashMap<u32, Option<Vec<ppstudio::TileRef>>> =
            Default::default();
        for d in &h.drawings {
            let layer = h
                .layers
                .iter()
                .find(|l| l.layer.id == d.layer)
                .ok_or_else(|| format!("drawing {} names layer {}", d.id, d.layer))?;
            let name = format!("D{}", d.id);
            let Some(at) = d.rgba else {
                // in the frame store: its tiles go straight in
                let store = frames
                    .as_deref_mut()
                    .ok_or_else(|| format!("drawing {} isn't in the packet", d.id))?;
                let mask = match masks.get(&d.layer) {
                    Some(m) => m.clone(),
                    None => {
                        let m = match layer.mask {
                            Some(m) => {
                                Some(p.put_mask(slice(blobs, m)?).map_err(|e| e.to_string())?)
                            }
                            None => None,
                        };
                        masks.insert(d.layer, m.clone());
                        m
                    }
                };
                if store.contains(d.id) {
                    store
                        .write_into(&mut p, d.id, &name, mask)
                        .map_err(|e| e.to_string())?;
                } else {
                    // never painted
                    p.manifest.drawings.insert(
                        name,
                        ppstudio::Drawing {
                            tiles: vec![],
                            mask,
                        },
                    );
                }
                continue;
            };
            let mask = layer.mask.map(|m| slice(blobs, m)).transpose()?;
            // an empty drawing comes as no bytes at all
            let blank;
            let rgba = match slice(blobs, at)? {
                [] => {
                    blank = vec![0u8; (h.canvas.width * h.canvas.height * 4) as usize];
                    &blank[..]
                }
                px => px,
            };
            p.put_drawing(&name, rgba, mask)
                .map_err(|e| e.to_string())?;
        }
        // the sound files, named by their content
        let mut names = std::collections::HashMap::new();
        for a in &h.audio {
            let ext = a.file.rsplit_once('.').map_or("bin", |(_, e)| e);
            let name = p
                .put_audio(slice(blobs, a.bytes)?, ext)
                .map_err(|e| e.to_string())?;
            names.insert(a.file.clone(), name);
        }
        let mut scenes = h.scenes;
        for clip in scenes.iter_mut().flat_map(|s| s.audio.iter_mut()) {
            clip.file = names
                .get(&clip.file)
                .ok_or_else(|| format!("sound {} isn't in the packet", clip.file))?
                .clone();
        }
        p.manifest.timeline.frames = Timeline::flatten(&scenes, &h.static_layers);
        p.manifest.timeline.scenes = scenes;
        p.manifest.timeline.static_layers = h.static_layers;
        p.manifest.timeline.mouth_charts = h.mouth_charts;
        p.manifest.timeline.drawing_kinds = h.drawing_kinds;
    }
    for f in &h.fonts {
        let ext = f.file.rsplit_once('.').map_or("ttf", |(_, e)| e);
        p.put_font(&f.family, slice(blobs, f.bytes)?, ext)
            .map_err(|e| e.to_string())?;
    }
    p.manifest.layers = h.layers.iter().map(|l| l.layer.clone()).collect();
    p.manifest.groups = h.groups;
    p.manifest.active_layer = h.active_layer;
    p.manifest.extra = h.extra;
    p.strokes = serde_json::to_vec(&if h.strokes.is_null() {
        serde_json::json!([])
    } else {
        h.strokes
    })
    .map_err(|e| e.to_string())?;
    p.timelapse = h
        .timelapse
        .iter()
        .map(|f| slice(blobs, f.bytes).map(<[u8]>::to_vec))
        .collect::<Result<_, _>>()?;
    p.manifest.timelapse = h.timelapse.iter().map(|f| f.t).collect();
    p.thumbnail = h
        .thumbnail
        .map(|t| slice(blobs, t).map(<[u8]>::to_vec))
        .transpose()?;
    Ok(p)
}

/// A project file's contents → the packet the window restores from (its first frame).
/// With a frame store, an animation's drawings go into it (emptied first) and the
/// packet names them without pixels; the window fetches the ones it shows.
pub fn from_project(p: &Project, mut frames: Option<&mut FrameStore>) -> Result<Vec<u8>, String> {
    let m = &p.manifest;
    let frame = &m.timeline.frames[0];
    let mut blobs = Vec::new();
    let mut put = |bytes: &[u8]| {
        let at = [blobs.len(), bytes.len()];
        blobs.extend_from_slice(bytes);
        at
    };
    let n = (m.canvas.width * m.canvas.height) as usize;
    let stored = frames.is_some() && !m.timeline.scenes.is_empty();
    if let Some(store) = frames.as_deref_mut()
        && stored
    {
        store.retain(&Default::default());
    }
    let mut layers = Vec::new();
    for l in &m.layers {
        let (rgba, mask) = match frame.cels.get(&l.id) {
            // (an animation's layers come empty: their drawings come from the store)
            Some(d) if stored => (Vec::new(), p.drawing(d).map_err(|e| e.to_string())?.1),
            Some(d) => p.drawing(d).map_err(|e| e.to_string())?,
            None => (vec![0; n * 4], None),
        };
        layers.push(PacketLayer {
            layer: l.clone(),
            rgba: put(&rgba),
            mask: mask.as_deref().map(&mut put),
        });
    }
    // an animation's drawings, each with the layer it's on (and mouth charts' drawings,
    // shown or not)
    let mut drawings = Vec::new();
    let mut named: Vec<(u32, &String)> = Vec::new();
    for cut in m.timeline.scenes.iter().flat_map(|s| &s.cuts) {
        for (&layer, track) in &cut.tracks {
            named.extend(
                track
                    .iter()
                    .filter_map(|e| e.drawing.as_ref())
                    .map(|d| (layer, d)),
            );
        }
    }
    for chart in &m.timeline.mouth_charts {
        named.extend(chart.shapes.values().map(|d| (chart.layer, d)));
    }
    for (layer, name) in named {
        let Some(id) = name.strip_prefix('D').and_then(|n| n.parse::<u32>().ok()) else {
            return Err(format!("drawing {name} isn't an app drawing"));
        };
        if drawings.iter().any(|d: &PacketDrawing| d.id == id) {
            continue;
        }
        let rgba = match frames.as_deref_mut() {
            Some(store) => {
                store.read_from(p, name, id).map_err(|e| e.to_string())?;
                None
            }
            None => Some(put(&p.drawing(name).map_err(|e| e.to_string())?.0)),
        };
        drawings.push(PacketDrawing { id, layer, rgba });
    }
    let thumbnail = p.thumbnail.as_deref().map(&mut put);
    let timelapse = p
        .timelapse
        .iter()
        .zip(&m.timelapse)
        .map(|(jpeg, &t)| PacketTimelapse {
            t,
            bytes: put(jpeg),
        })
        .collect();
    let mut audio: Vec<PacketAudio> = Vec::new();
    for clip in m.timeline.scenes.iter().flat_map(|s| &s.audio) {
        if audio.iter().any(|a| a.file == clip.file) {
            continue;
        }
        if let Some(bytes) = p.audio.get(&clip.file) {
            audio.push(PacketAudio {
                file: clip.file.clone(),
                bytes: put(bytes),
            });
        }
    }
    let fonts = m
        .fonts
        .iter()
        .filter_map(|f| {
            let bytes = p.fonts.get(&f.file)?;
            Some(PacketFont {
                family: f.family.clone(),
                file: f.file.clone(),
                bytes: put(bytes),
            })
        })
        .collect();
    let header = Header {
        canvas: m.canvas.clone(),
        fps: m.timeline.fps,
        layers,
        groups: m.groups.clone(),
        active_layer: m.active_layer,
        strokes: serde_json::from_slice(&p.strokes).unwrap_or(serde_json::json!([])),
        extra: m.extra.clone(),
        created: Some(m.created.clone()),
        thumbnail,
        scenes: m.timeline.scenes.clone(),
        drawings,
        static_layers: m.timeline.static_layers.clone(),
        audio,
        mouth_charts: m.timeline.mouth_charts.clone(),
        drawing_kinds: m.timeline.drawing_kinds.clone(),
        timelapse,
        fonts,
    };
    encode(&header, &blobs)
}

/// Now, as RFC 3339 UTC (no time-zone crate needed for that).
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil from days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A path from the window: absolute, and ending in .ppstudio.
pub fn project_path(raw: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(raw);
    if !p.is_absolute() {
        return Err("the project path must be absolute".into());
    }
    Ok(if p.extension().is_some_and(|e| e == "ppstudio") {
        p
    } else {
        p.with_extension("ppstudio")
    })
}

/// Undo the window's `encodeURIComponent` on a path.
pub fn percent_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or("bad path encoding")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "bad path encoding")?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "bad path encoding".into())
}

pub fn save(
    path: &Path,
    packet: &[u8],
    app: &str,
    frames: Option<&mut FrameStore>,
) -> Result<(), String> {
    let p = to_project(packet, app, &now_rfc3339(), frames)?;
    p.save(path).map_err(|e| e.to_string())
}

pub fn open(path: &Path, frames: Option<&mut FrameStore>) -> Result<Vec<u8>, String> {
    let p = Project::open(path).map_err(|e| e.to_string())?;
    from_project(&p, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: u32) -> Layer {
        Layer {
            id,
            name: format!("Layer {id}"),
            visible: true,
            opacity: 0.5,
            blend: "multiply".into(),
            locked: false,
            alpha_lock: true,
            clip: false,
            imported: false,
            group: 0,
            adjust: None,
            style: None,
            text: None,
        }
    }

    fn packet() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (w, h) = (100u32, 70u32);
        let n = (w * h) as usize;
        let mut rgba = vec![0u8; n * 4];
        rgba[(10 * w as usize + 20) * 4..][..4].copy_from_slice(&[1, 2, 3, 255]);
        let mut mask = vec![255u8; n];
        mask[5] = 40;
        let thumb = vec![137, 80, 78, 71];
        let mut blobs = rgba.clone();
        blobs.extend_from_slice(&mask);
        blobs.extend_from_slice(&thumb);
        let header = Header {
            canvas: Canvas {
                width: w,
                height: h,
            },
            fps: 24,
            layers: vec![PacketLayer {
                layer: layer(3),
                rgba: [0, n * 4],
                mask: Some([n * 4, n]),
            }],
            groups: vec![],
            active_layer: 3,
            strokes: serde_json::json!([{ "brushId": "pen", "points": [] }]),
            extra: serde_json::json!({ "color": "#ff0000" }),
            created: None,
            thumbnail: Some([n * 5, thumb.len()]),
            scenes: vec![],
            drawings: vec![],
            static_layers: vec![],
            audio: vec![],
            mouth_charts: vec![],
            drawing_kinds: Default::default(),
            timelapse: vec![],
            fonts: vec![],
        };
        (encode(&header, &blobs).unwrap(), rgba, mask)
    }

    #[test]
    fn an_animation_survives_save_and_open() {
        use ppstudio::{Cut, Exposure};
        let (pk, rgba, _) = packet();
        let (mut h, blobs) = decode(&pk).unwrap();
        let mut blobs = blobs.to_vec();
        // a second drawing: one red pixel elsewhere
        let mut second = vec![0u8; rgba.len()];
        second[..4].copy_from_slice(&[255, 0, 0, 255]);
        let at = blobs.len();
        blobs.extend_from_slice(&second);
        let ex = |d: u32, length| Exposure {
            drawing: Some(format!("D{d}")),
            length,
        };
        h.drawings = vec![
            PacketDrawing {
                id: 7,
                layer: 3,
                rgba: Some(h.layers[0].rgba),
            },
            PacketDrawing {
                id: 9,
                layer: 3,
                rgba: Some([at, second.len()]),
            },
        ];
        h.scenes = vec![Scene {
            name: "Scene 1".into(),
            cuts: vec![Cut {
                name: "Cut 1".into(),
                tracks: [(3, vec![ex(7, 2), ex(9, 3)])].into_iter().collect(),
                camera: Some(ppstudio::Camera {
                    x: 10.0,
                    y: 5.0,
                    width: 80.0,
                    height: 45.0,
                }),
                dialogue: [(3, "ah".to_string())].into_iter().collect(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        let pk = encode(&h, &blobs).unwrap();
        let p = to_project(&pk, "test", "2026-11-21T00:00:00Z", None).unwrap();
        assert_eq!(p.manifest.timeline.frames.len(), 2);
        assert_eq!(p.manifest.timeline.frames[1].hold, 3);
        let back = from_project(&p, None).unwrap();
        let (h2, b2) = decode(&back).unwrap();
        assert_eq!(h2.scenes, h.scenes);
        assert!(h2.static_layers.is_empty());
        assert_eq!(h2.drawings.len(), 2);
        let d9 = h2.drawings.iter().find(|d| d.id == 9).unwrap();
        assert_eq!(d9.layer, 3);
        assert_eq!(slice(b2, d9.rgba.unwrap()).unwrap(), &second[..]);
        // the layer shows the first frame's drawing
        assert_eq!(slice(b2, h2.layers[0].rgba).unwrap(), &rgba[..]);

        // a sound in the scene: renamed by its content, and back
        let mut ha = h.clone();
        let mut blobs_a = blobs.clone();
        let at = blobs_a.len();
        blobs_a.extend_from_slice(b"RIFF....WAVE");
        ha.audio = vec![PacketAudio {
            file: "a1.wav".into(),
            bytes: [at, 12],
        }];
        ha.scenes[0].audio.push(ppstudio::AudioClip {
            name: "line".into(),
            file: "a1.wav".into(),
            start: 4,
            gain: 1.0,
            muted: false,
        });
        let pa = to_project(&encode(&ha, &blobs_a).unwrap(), "test", "now", None).unwrap();
        let file = &pa.manifest.timeline.scenes[0].audio[0].file;
        assert!(file.ends_with(".wav") && file != "a1.wav");
        let pk_a = from_project(&pa, None).unwrap();
        let (ha2, ba2) = decode(&pk_a).unwrap();
        assert_eq!(ha2.audio[0].file, *file);
        assert_eq!(slice(ba2, ha2.audio[0].bytes).unwrap(), b"RIFF....WAVE");

        // a text layer and its imported font: kept, and back
        let mut ht = h.clone();
        let mut blobs_t = blobs.clone();
        let at = blobs_t.len();
        blobs_t.extend_from_slice(b"wOF2 fake");
        ht.layers[0].layer.text = Some(r#"{"text":"Hi","font":"Hand"}"#.into());
        ht.fonts = vec![PacketFont {
            family: "Hand".into(),
            file: "hand.woff2".into(),
            bytes: [at, 9],
        }];
        let pt = to_project(&encode(&ht, &blobs_t).unwrap(), "test", "now", None).unwrap();
        let pk_t = from_project(&pt, None).unwrap();
        let (ht2, bt2) = decode(&pk_t).unwrap();
        assert_eq!(ht2.layers[0].layer.text, ht.layers[0].layer.text);
        assert_eq!(ht2.fonts[0].family, "Hand");
        assert!(ht2.fonts[0].file.ends_with(".woff2"));
        assert_eq!(slice(bt2, ht2.fonts[0].bytes).unwrap(), b"wOF2 fake");

        // a static layer: one drawing all cut long, so one frame; kept through the file
        let mut hs = h.clone();
        hs.static_layers = vec![3];
        let ps = to_project(&encode(&hs, &blobs).unwrap(), "test", "now", None).unwrap();
        assert_eq!(ps.manifest.timeline.frames.len(), 1);
        assert_eq!(ps.manifest.timeline.static_layers, vec![3]);
        let (hs2, _) = decode(&from_project(&ps, None).unwrap()).unwrap();
        assert_eq!(hs2.static_layers, vec![3]);

        // a mouth chart whose drawing 11 no frame shows: kept, drawing and all
        let mut hm = h.clone();
        let mut blobs_m = blobs.clone();
        let at = blobs_m.len();
        blobs_m.extend_from_slice(&rgba);
        hm.drawings.push(PacketDrawing {
            id: 11,
            layer: 3,
            rgba: Some([at, rgba.len()]),
        });
        hm.mouth_charts = vec![MouthChart {
            layer: 3,
            shapes: [
                ("A".to_string(), "D11".to_string()),
                ("D".into(), "D9".into()),
            ]
            .into_iter()
            .collect(),
        }];
        let pm = to_project(&encode(&hm, &blobs_m).unwrap(), "test", "now", None).unwrap();
        assert!(pm.manifest.drawings.contains_key("D11"));
        let pk_m = from_project(&pm, None).unwrap();
        let (hm2, bm2) = decode(&pk_m).unwrap();
        assert_eq!(hm2.mouth_charts, hm.mouth_charts);
        // and drawing marks
        let mut hk = h.clone();
        hk.drawing_kinds = [("D9".to_string(), "key".to_string())]
            .into_iter()
            .collect();
        let pk = to_project(&encode(&hk, &blobs).unwrap(), "test", "now", None).unwrap();
        let (hk2, _) = decode(&from_project(&pk, None).unwrap()).unwrap();
        assert_eq!(hk2.drawing_kinds, hk.drawing_kinds);
        let d11 = hm2.drawings.iter().find(|d| d.id == 11).unwrap();

        // a timelapse: its pictures and times through the file and back
        let mut ht = h.clone();
        let mut blobs_t = blobs.clone();
        let at = blobs_t.len();
        blobs_t.extend_from_slice(b"\xff\xd8jpeg");
        ht.timelapse = vec![PacketTimelapse {
            t: 1_700_000_000_000,
            bytes: [at, 6],
        }];
        let pt = to_project(&encode(&ht, &blobs_t).unwrap(), "test", "now", None).unwrap();
        assert_eq!(pt.timelapse, vec![b"\xff\xd8jpeg".to_vec()]);
        let pk_t = from_project(&pt, None).unwrap();
        let (ht2, bt2) = decode(&pk_t).unwrap();
        assert_eq!(ht2.timelapse[0].t, 1_700_000_000_000);
        assert_eq!(slice(bt2, ht2.timelapse[0].bytes).unwrap(), b"\xff\xd8jpeg");
        assert_eq!(slice(bm2, d11.rgba.unwrap()).unwrap(), &rgba[..]);

        // with the frame store: opening puts the drawings in it, the packet carries none
        let mut store = FrameStore::new(usize::MAX, None).unwrap();
        store.put(99, []).unwrap(); // the last document's: forgotten on opening
        let back = from_project(&p, Some(&mut store)).unwrap();
        let (h3, b3) = decode(&back).unwrap();
        assert!(h3.drawings.iter().all(|d| d.rgba.is_none()));
        assert!(h3.layers[0].rgba[1] == 0, "the layer comes empty");
        assert_eq!(
            slice(b3, h3.layers[0].mask.unwrap()).unwrap().len(),
            100 * 70
        );
        assert!(store.contains(7) && store.contains(9) && !store.contains(99));
        // and saving takes them from the store
        let mut h4 = h3.clone();
        h4.layers[0].rgba = [0, 0];
        let pk4 = encode(&h4, b3).unwrap();
        let p4 = to_project(&pk4, "test", "now", Some(&mut store)).unwrap();
        assert_eq!(p4.drawing("D9").unwrap().0, second);
        assert_eq!(p4.drawing("D7").unwrap().0, rgba);
        assert!(p4.manifest.drawings["D9"].mask.is_some());
        // without a store, a packet naming stored drawings is refused
        assert!(to_project(&pk4, "test", "now", None).is_err());
    }

    #[test]
    fn a_document_survives_save_and_open() {
        let (pk, rgba, mask) = packet();
        let dir = std::env::temp_dir().join(format!("pp-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.ppstudio");
        save(&path, &pk, "PencilPilot test", None).unwrap();
        let back = open(&path, None).unwrap();
        let (h, blobs) = decode(&back).unwrap();
        assert_eq!(h.layers.len(), 1);
        let l = &h.layers[0];
        assert_eq!(l.layer, layer(3));
        assert_eq!(slice(blobs, l.rgba).unwrap(), &rgba[..]);
        assert_eq!(slice(blobs, l.mask.unwrap()).unwrap(), &mask[..]);
        assert_eq!(h.active_layer, 3);
        assert_eq!(h.extra["color"], "#ff0000");
        assert_eq!(h.strokes[0]["brushId"], "pen");
        assert!(h.created.as_deref().is_some_and(|c| c.ends_with('Z')));
        assert_eq!(
            slice(blobs, h.thumbnail.unwrap()).unwrap(),
            &[137, 80, 78, 71]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_packets_and_paths_are_refused() {
        assert!(decode(b"nope").is_err());
        let (mut pk, ..) = packet();
        pk.truncate(pk.len() - 10_000);
        assert!(
            to_project(&pk, "t", "now", None).is_err(),
            "a layer past the end"
        );
        assert!(project_path("relative/file").is_err());
        assert_eq!(
            project_path("/tmp/drawing").unwrap(),
            PathBuf::from("/tmp/drawing.ppstudio")
        );
    }

    #[test]
    fn paths_are_decoded() {
        assert_eq!(
            percent_decode("/home/a%20b/%C3%A9.ppstudio").unwrap(),
            "/home/a b/é.ppstudio"
        );
        assert!(percent_decode("%zz").is_err());
    }

    #[test]
    fn times_are_rfc3339() {
        let t = now_rfc3339();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'), "{t}");
    }
}
