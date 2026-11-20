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

use ppstudio::{Canvas, Frame, Group, Layer, Project};
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
pub fn to_project(packet: &[u8], app: &str, now: &str) -> Result<Project, String> {
    let (h, blobs) = decode(packet)?;
    let mut p = Project::new(app, now, h.canvas.width, h.canvas.height, h.fps.max(1));
    if let Some(created) = h.created {
        p.manifest.created = created;
    }
    let mut frame = Frame::default();
    for l in &h.layers {
        let id = format!("L{}", l.layer.id);
        let mask = l.mask.map(|m| slice(blobs, m)).transpose()?;
        p.put_drawing(&id, slice(blobs, l.rgba)?, mask)
            .map_err(|e| e.to_string())?;
        frame.cels.insert(l.layer.id, id);
        p.manifest.layers.push(l.layer.clone());
    }
    p.manifest.timeline.frames = vec![frame];
    p.manifest.groups = h.groups;
    p.manifest.active_layer = h.active_layer;
    p.manifest.extra = h.extra;
    p.strokes = serde_json::to_vec(&if h.strokes.is_null() {
        serde_json::json!([])
    } else {
        h.strokes
    })
    .map_err(|e| e.to_string())?;
    p.thumbnail = h
        .thumbnail
        .map(|t| slice(blobs, t).map(<[u8]>::to_vec))
        .transpose()?;
    Ok(p)
}

/// A project file's contents → the packet the window restores from (its first frame).
pub fn from_project(p: &Project) -> Result<Vec<u8>, String> {
    let m = &p.manifest;
    let frame = &m.timeline.frames[0];
    let mut blobs = Vec::new();
    let mut put = |bytes: &[u8]| {
        let at = [blobs.len(), bytes.len()];
        blobs.extend_from_slice(bytes);
        at
    };
    let n = (m.canvas.width * m.canvas.height) as usize;
    let mut layers = Vec::new();
    for l in &m.layers {
        let (rgba, mask) = match frame.cels.get(&l.id) {
            Some(d) => p.drawing(d).map_err(|e| e.to_string())?,
            None => (vec![0; n * 4], None),
        };
        layers.push(PacketLayer {
            layer: l.clone(),
            rgba: put(&rgba),
            mask: mask.as_deref().map(&mut put),
        });
    }
    let thumbnail = p.thumbnail.as_deref().map(&mut put);
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

pub fn save(path: &Path, packet: &[u8], app: &str) -> Result<(), String> {
    let p = to_project(packet, app, &now_rfc3339())?;
    p.save(path).map_err(|e| e.to_string())
}

pub fn open(path: &Path) -> Result<Vec<u8>, String> {
    let p = Project::open(path).map_err(|e| e.to_string())?;
    from_project(&p)
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
        };
        (encode(&header, &blobs).unwrap(), rgba, mask)
    }

    #[test]
    fn a_document_survives_save_and_open() {
        let (pk, rgba, mask) = packet();
        let dir = std::env::temp_dir().join(format!("pp-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.ppstudio");
        save(&path, &pk, "PencilPilot test").unwrap();
        let back = open(&path).unwrap();
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
        assert!(to_project(&pk, "t", "now").is_err(), "a layer past the end");
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
