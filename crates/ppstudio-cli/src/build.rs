//! `ppstudio build`: a project from a description of it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ppstudio::{AudioClip, Cut, Exposure, Layer, Marker, Project, Scene, Timeline};
use serde::Deserialize;

/// What to build (JSON). Paths are relative to the spec file's folder.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub width: u32,
    pub height: u32,
    #[serde(default = "fps24")]
    pub fps: u32,
    /// bottom → top
    pub layers: Vec<LayerSpec>,
    pub scenes: Vec<SceneSpec>,
}

fn fps24() -> u32 {
    24
}
fn one() -> u32 {
    1
}
fn one_f() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn normal() -> String {
    "normal".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSpec {
    pub name: String,
    #[serde(default = "one_f")]
    pub opacity: f32,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default = "normal")]
    pub blend: String,
    /// clipped to the layer below
    #[serde(default)]
    pub clip: bool,
    #[serde(default)]
    pub locked: bool,
    /// one drawing all cut long (a background)
    #[serde(default, rename = "static")]
    pub is_static: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSpec {
    pub name: String,
    pub cuts: Vec<CutSpec>,
    #[serde(default)]
    pub audio: Vec<AudioSpec>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutSpec {
    pub name: String,
    /// into the next cut: "flash" or "dissolve" (none: a hard cut)
    #[serde(default)]
    pub transition: Option<String>,
    #[serde(default)]
    pub action: BTreeMap<u32, String>,
    #[serde(default)]
    pub dialogue: BTreeMap<u32, String>,
    /// layer name → its exposures, in order
    #[serde(default)]
    pub tracks: BTreeMap<String, Vec<ExposureSpec>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureSpec {
    /// the picture (none: an empty cel)
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default = "one")]
    pub length: u32,
    /// "key", "breakdown" or "inbetween"
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSpec {
    pub file: String,
    #[serde(default)]
    pub name: Option<String>,
    /// the scene frame it starts on
    #[serde(default)]
    pub start: i64,
    #[serde(default = "one_f")]
    pub gain: f32,
    #[serde(default)]
    pub muted: bool,
}

/// A picture as canvas-sized straight RGBA: fitted inside, centred, transparent around.
pub fn picture(path: &Path, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgba8();
    if img.width() == w && img.height() == h {
        return Ok(img.into_raw());
    }
    let k = (w as f64 / img.width() as f64).min(h as f64 / img.height() as f64);
    let fw = ((img.width() as f64 * k).round() as u32).clamp(1, w);
    let fh = ((img.height() as f64 * k).round() as u32).clamp(1, h);
    let small = image::imageops::resize(&img, fw, fh, image::imageops::FilterType::Lanczos3);
    let mut out = image::RgbaImage::new(w, h);
    image::imageops::overlay(
        &mut out,
        &small,
        ((w - fw) / 2) as i64,
        ((h - fh) / 2) as i64,
    );
    Ok(out.into_raw())
}

fn now() -> String {
    // (RFC 3339 is wanted; the date is good enough to the second from the system clock)
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn build(spec: &Spec, base: &Path, out: &Path) -> Result<serde_json::Value, String> {
    let (w, h) = (spec.width, spec.height);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("canvas {w}×{h}"));
    }
    if spec.layers.is_empty() {
        return Err("no layers".into());
    }
    let resolve = |p: &str| -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    };
    let mut p = Project::new("PencilPilot director", &now(), w, h, spec.fps);
    let ids: HashMap<&str, u32> = spec
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.name.as_str(), i as u32 + 1))
        .collect();
    p.manifest.layers = spec
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| Layer {
            id: i as u32 + 1,
            name: l.name.clone(),
            visible: l.visible,
            opacity: l.opacity.clamp(0.0, 1.0),
            blend: l.blend.clone(),
            locked: l.locked,
            alpha_lock: false,
            clip: l.clip,
            imported: false,
            group: 0,
            adjust: None,
            style: None,
            text: None,
            lines: false,
        })
        .collect();
    p.manifest.active_layer = spec.layers.len() as u32;
    let statics: Vec<u32> = spec
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.is_static)
        .map(|(i, _)| i as u32 + 1)
        .collect();

    // each picture once, as a drawing "D<n>"
    let mut drawings: HashMap<PathBuf, String> = HashMap::new();
    let mut kinds = BTreeMap::new();
    let mut scenes = Vec::new();
    for s in &spec.scenes {
        let mut cuts = Vec::new();
        for c in &s.cuts {
            if let Some(t) = &c.transition
                && t != "flash"
                && t != "dissolve"
            {
                return Err(format!("cut {}: transition {t}", c.name));
            }
            let mut tracks = BTreeMap::new();
            for (layer, exposures) in &c.tracks {
                let id = *ids.get(layer.as_str()).ok_or_else(|| {
                    format!(
                        "cut {} names layer {layer}, which isn't in `layers`",
                        c.name
                    )
                })?;
                let mut track = Vec::new();
                for e in exposures {
                    let drawing = match &e.image {
                        None => None,
                        Some(img) => {
                            let path = resolve(img);
                            let name = match drawings.get(&path) {
                                Some(n) => n.clone(),
                                None => {
                                    let n = format!("D{}", drawings.len() + 1);
                                    p.put_drawing(&n, &picture(&path, w, h)?, None)
                                        .map_err(|e| e.to_string())?;
                                    drawings.insert(path, n.clone());
                                    n
                                }
                            };
                            if let Some(k) = &e.kind {
                                if !ppstudio::DRAWING_KINDS.contains(&k.as_str()) {
                                    return Err(format!("drawing kind {k}"));
                                }
                                kinds.insert(name.clone(), k.clone());
                            }
                            Some(name)
                        }
                    };
                    track.push(Exposure {
                        drawing,
                        length: e.length.clamp(1, 100_000),
                    });
                }
                tracks.insert(id, track);
            }
            cuts.push(Cut {
                name: c.name.clone(),
                tracks,
                camera: None,
                action: c.action.clone(),
                dialogue: c.dialogue.clone(),
                transition: c.transition.clone(),
            });
        }
        let mut audio = Vec::new();
        for a in &s.audio {
            let path = resolve(&a.file);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
            let file = p.put_audio(&bytes, ext).map_err(|e| e.to_string())?;
            audio.push(AudioClip {
                name: a.name.clone().unwrap_or_else(|| {
                    path.file_stem()
                        .map_or("Sound".into(), |s| s.to_string_lossy().into_owned())
                }),
                file,
                start: a.start,
                gain: a.gain,
                muted: a.muted,
            });
        }
        scenes.push(Scene {
            name: s.name.clone(),
            cuts,
            audio,
            markers: s.markers.clone(),
        });
    }
    if scenes.iter().all(|s| s.cuts.is_empty()) {
        return Err("no cuts".into());
    }
    p.manifest.timeline.frames = Timeline::flatten(&scenes, &statics);
    let lengths: Vec<serde_json::Value> = scenes
        .iter()
        .flat_map(|s| &s.cuts)
        .map(|c| serde_json::json!({ "cut": c.name, "frames": c.length(&statics) }))
        .collect();
    p.manifest.timeline.scenes = scenes;
    p.manifest.timeline.static_layers = statics;
    p.manifest.timeline.drawing_kinds = kinds;
    p.save(out).map_err(|e| e.to_string())?;
    // read back: what the app will open is valid
    Project::open(out).map_err(|e| format!("the written project doesn't open: {e}"))?;
    Ok(serde_json::json!({
        "project": out.display().to_string(),
        "drawings": drawings.len(),
        "cuts": lengths,
        "fps": spec.fps,
    }))
}
