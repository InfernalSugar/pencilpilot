//! `ppstudio render` and `ppstudio info`: look at a project without the app.

use std::collections::HashMap;
use std::path::Path;

use ppstudio::Project;

/// Each drawing change (or, with `every`, each frame) as a PNG over white.
pub fn render(
    project: &Path,
    out: &Path,
    only: Option<&[String]>,
    every: bool,
) -> Result<serde_json::Value, String> {
    let p = Project::open(project).map_err(|e| format!("{}: {e}", project.display()))?;
    let m = &p.manifest;
    let (w, h) = (m.canvas.width, m.canvas.height);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let layers: Vec<_> = m
        .layers
        .iter()
        .filter(|l| l.visible && l.adjust.is_none())
        .filter(|l| only.is_none_or(|names| names.iter().any(|n| n == &l.name)))
        .collect();
    // decoded drawings, the few most recent kept
    let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
    let mut written = Vec::new();
    let mut n = 0;
    for f in &m.timeline.frames {
        let mut px = vec![255f32; (w * h * 3) as usize];
        for l in &layers {
            let Some(d) = f.cels.get(&l.id) else { continue };
            if !cache.contains_key(d) {
                if cache.len() > 8 {
                    cache.clear();
                }
                let rgba = p.drawing(d).map_err(|e| e.to_string())?.0;
                cache.insert(d.clone(), rgba);
            }
            let rgba = &cache[d];
            for (i, c) in rgba.as_chunks::<4>().0.iter().enumerate() {
                let a = c[3] as f32 / 255.0 * l.opacity;
                if a <= 0.0 {
                    continue;
                }
                for k in 0..3 {
                    let o = &mut px[i * 3 + k];
                    *o = c[k] as f32 * a + *o * (1.0 - a);
                }
            }
        }
        let bytes: Vec<u8> = px
            .iter()
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
            .collect();
        let img = image::RgbImage::from_raw(w, h, bytes).ok_or("a frame of the wrong size")?;
        let copies = if every { f.hold } else { 1 };
        for _ in 0..copies {
            n += 1;
            let file = out.join(format!("{n:04}.png"));
            img.save(&file).map_err(|e| e.to_string())?;
            written.push(serde_json::json!({
                "file": file.display().to_string(),
                "hold": if every { 1 } else { f.hold },
            }));
        }
    }
    Ok(serde_json::json!({ "width": w, "height": h, "fps": m.timeline.fps, "frames": written }))
}

/// The project's timeline, briefly.
pub fn info(project: &Path) -> Result<serde_json::Value, String> {
    let p = Project::open(project).map_err(|e| format!("{}: {e}", project.display()))?;
    let m = &p.manifest;
    let statics = &m.timeline.static_layers;
    let names: HashMap<u32, &str> = m.layers.iter().map(|l| (l.id, l.name.as_str())).collect();
    let scenes: Vec<serde_json::Value> = m
        .timeline
        .scenes
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "sounds": s.audio.iter().map(|a| &a.name).collect::<Vec<_>>(),
                "cuts": s.cuts.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "frames": c.length(statics),
                    "seconds": c.length(statics) as f64 / m.timeline.fps as f64,
                    "transition": c.transition,
                    "layers": c.tracks.iter().filter(|(_, t)| t.iter().any(|e| e.drawing.is_some()))
                        .map(|(l, t)| serde_json::json!({
                            "layer": names.get(l).copied().unwrap_or("?"),
                            "drawings": t.iter().filter(|e| e.drawing.is_some()).count(),
                        })).collect::<Vec<_>>(),
                    "dialogue": c.dialogue,
                    "action": c.action,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "canvas": [m.canvas.width, m.canvas.height],
        "fps": m.timeline.fps,
        "layers": m.layers.iter().map(|l| &l.name).collect::<Vec<_>>(),
        "drawings": m.drawings.len(),
        "scenes": scenes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{Spec, build};

    #[test]
    fn builds_a_project_the_app_opens_and_renders_it_back() {
        let dir = std::env::temp_dir().join(format!("ppstudio-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // two pictures: a red one (half the canvas's width: fitted, with bars) and a blue one
        image::RgbaImage::from_pixel(32, 64, image::Rgba([255, 0, 0, 255]))
            .save(dir.join("red.png"))
            .unwrap();
        image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 255, 128]))
            .save(dir.join("blue.png"))
            .unwrap();
        let spec: Spec = serde_json::from_value(serde_json::json!({
            "width": 64, "height": 64, "fps": 24,
            "layers": [{ "name": "Board" }, { "name": "Guide", "opacity": 0.5 }],
            "scenes": [{ "name": "S1", "cuts": [
                { "name": "Shot 1", "transition": "flash", "dialogue": { "0": "hi" },
                  "tracks": { "Board": [{ "image": "red.png", "length": 2, "kind": "key" }] } },
                { "name": "Shot 2", "tracks": {
                    "Board": [{ "image": "red.png", "length": 1 }, { "length": 1 }],
                    "Guide": [{ "image": "blue.png", "length": 2 }] } }
            ]}]
        }))
        .unwrap();
        let out = dir.join("film.ppstudio");
        let r = build(&spec, &dir, &out).unwrap();
        assert_eq!(r["drawings"], 2, "the same picture is one drawing");
        assert_eq!(r["cuts"][0]["frames"], 2);
        let p = Project::open(&out).unwrap();
        let t = &p.manifest.timeline;
        assert_eq!(t.scenes[0].cuts[0].transition.as_deref(), Some("flash"));
        assert_eq!(t.drawing_kinds.get("D1").map(String::as_str), Some("key"));
        assert_eq!(t.frames.iter().map(|f| f.hold).sum::<u32>(), 4);

        let info = info(&out).unwrap();
        assert_eq!(info["scenes"][0]["cuts"][1]["frames"], 2);

        let r = render(&out, &dir.join("frames"), None, false).unwrap();
        let frames = r["frames"].as_array().unwrap();
        // shot 1 (held 2), shot 2 frame 1 (red + blue), frame 2 (blue only)
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["hold"], 2);
        let px = |i: usize, x, y| {
            let img = image::open(frames[i]["file"].as_str().unwrap())
                .unwrap()
                .to_rgb8();
            *img.get_pixel(x, y)
        };
        assert_eq!(
            px(0, 32, 32).0,
            [255, 0, 0],
            "the red picture, in the middle"
        );
        assert_eq!(px(0, 2, 32).0, [255, 255, 255], "white beside it (fitted)");
        // blue at half alpha on a half-opacity layer: a quarter blue over white
        let b = px(2, 2, 32).0;
        assert!(b[2] == 255 && (b[0] as i32 - 191).abs() <= 2, "{b:?}");
        let r = render(&out, &dir.join("every"), Some(&["Guide".to_string()]), true).unwrap();
        assert_eq!(r["frames"].as_array().unwrap().len(), 4);

        // a cut naming a layer that isn't there is refused
        let bad: Spec = serde_json::from_value(serde_json::json!({
            "width": 64, "height": 64, "layers": [{ "name": "A" }],
            "scenes": [{ "name": "S", "cuts": [{ "name": "C", "tracks": { "B": [] } }] }]
        }))
        .unwrap();
        assert!(build(&bad, &dir, &dir.join("bad.ppstudio")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
