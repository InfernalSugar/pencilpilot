//! Exporting the frame showing as a Photoshop file (`pencilpilot_export::psd`): its layers
//! as the document has them (names, opacity, blend, visibility, clipping, masks, groups) and
//! the flattened picture, as the current app's "Photoshop file (.psd)".
//!
//! The window copies each layer's drawing and mask (memory copies of their tiles) and lets
//! go; a thread of its own puts them in a painter of its own, and writes the file from it a
//! layer at a time.

use std::path::PathBuf;
use std::sync::mpsc;

use brush::{DrawingCopy, LayerProps, Painter};
use pencilpilot_export::psd::{LayerPixels, PsdItem, PsdLayer, write_psd};

use super::super::Engine;

/// A layer as the file's thread needs it.
struct Copied {
    props: LayerProps,
    group: u32,
    drawing: Option<DrawingCopy>,
    mask: Option<DrawingCopy>,
}

/// An opacity as the current app gets it from the engine (to three decimals), so the file's
/// bytes are its.
fn decimal(o: f32) -> f64 {
    (o as f64 * 1000.0).round() / 1000.0
}

/// An adjustment's kind, as the current app names it (`specName`).
fn adjust_name(spec: &str) -> &'static str {
    let v: serde_json::Value = serde_json::from_str(spec).unwrap_or_default();
    let set = |k: &str| v.get(k).is_some_and(|x| !x.is_null());
    let nonzero = |k: &str| v.get(k).and_then(|x| x.as_f64()).is_some_and(|x| x != 0.0);
    if set("map") {
        "Gradient map"
    } else if set("curves") {
        "Curves"
    } else if set("levels") {
        "Levels"
    } else if nonzero("hue") || nonzero("saturation") {
        "Hue / saturation"
    } else if nonzero("brightness") || nonzero("contrast") {
        "Brightness / contrast"
    } else {
        "Adjustment"
    }
}

/// Starts writing the frame showing to `path`; the result comes back once.
pub fn start(e: &Engine, path: PathBuf) -> mpsc::Receiver<Result<PathBuf, String>> {
    let p = &e.painter;
    let (w, h) = (p.width(), p.height());
    // each layer's group (the painter says it in its list)
    let groups_of: Vec<u32> = serde_json::from_str::<serde_json::Value>(&p.layers_json())
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.iter()
                .map(|l| l["group"].as_u64().unwrap_or(0) as u32)
                .collect()
        })
        .unwrap_or_default();
    let layers: Vec<Copied> = p
        .layers()
        .iter()
        .enumerate()
        .map(|(i, l)| Copied {
            props: l.props.clone(),
            group: groups_of.get(i).copied().unwrap_or(0),
            drawing: p.drawing_copy(p.layer_drawing(l.id)),
            mask: p.mask_copy(l.id),
        })
        .collect();
    let groups = p.groups().to_vec();
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("export-psd".into())
        .spawn(move || {
            let r = write(&path, w, h, layers, &groups);
            if r.is_err() {
                let _ = std::fs::remove_file(&path);
            }
            let _ = tx.send(r.map(|_| path));
        });
    if let Err(err) = spawned {
        let (tx, rx2) = mpsc::channel();
        let _ = tx.send(Err(err.to_string()));
        return rx2;
    }
    rx
}

fn write(
    path: &std::path::Path,
    w: u32,
    h: u32,
    layers: Vec<Copied>,
    groups: &[brush::Group],
) -> Result<(), String> {
    // the layers in a painter of this thread's own, each showing its drawing
    let mut p = Painter::new(w, h);
    let mut ids = Vec::new();
    let mut pairs = Vec::new();
    let mut meta = Vec::new();
    for (i, l) in layers.into_iter().enumerate() {
        let id = if i == 0 {
            p.layers()[0].id
        } else {
            p.add_layer(&l.props.name)
        };
        p.set_layer_props(id, l.props.clone(), false);
        let d = i as u32 + 1;
        if let Some(c) = l.drawing {
            p.put_drawing_copy(d, c);
            pairs.extend([id, d]);
        } else {
            pairs.extend([id, 0]);
        }
        if let Some(m) = l.mask {
            p.put_mask_copy(id, m);
        }
        ids.push(id);
        meta.push((l.props, l.group));
    }
    p.show_drawings(&pairs);
    // the groups, as the window's painter has them
    let mut made: std::collections::HashMap<u32, u32> = Default::default();
    for (i, (_, g)) in meta.iter().enumerate() {
        if *g == 0 {
            continue;
        }
        let Some(info) = groups.iter().find(|x| x.id == *g) else {
            continue;
        };
        match made.get(g) {
            Some(&gid) => {
                p.move_layer_to(ids[i], i, gid);
            }
            None => {
                let gid = p.add_group(ids[i]);
                p.set_group(gid, &info.name, info.visible, info.opacity, false);
                made.insert(*g, gid);
            }
        }
    }
    p.clear_history();
    // the flattened picture: the frame as the document shows it
    let composite = p.render_frame(&[], 1, &[]);
    // a group's layers sit next to each other: consecutive layers of one group, a folder
    let mut items: Vec<PsdItem> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    let mut open: Option<u32> = None;
    for (i, (props, g)) in meta.iter().enumerate() {
        let layer = if props.adjust.is_some() {
            // an adjustment isn't written as one: the flattened picture shows it, the layer
            // is kept (hidden, empty) under its name
            let name = format!(
                "{} ({}, not editable)",
                props.name,
                adjust_name(&props.adjust_spec)
            );
            let units: Vec<u16> = name.encode_utf16().take(60).collect();
            PsdLayer {
                name: String::from_utf16_lossy(&units),
                opacity: decimal(props.opacity),
                blend: props.blend.as_str().into(),
                visible: false,
                clip: props.clip,
            }
        } else {
            PsdLayer {
                name: props.name.clone(),
                opacity: decimal(props.opacity),
                blend: props.blend.as_str().into(),
                visible: props.visible,
                clip: props.clip,
            }
        };
        order.push(i);
        let info = groups.iter().find(|x| x.id == *g).filter(|_| *g != 0);
        match info {
            None => {
                open = None;
                items.push(PsdItem::Layer(layer));
            }
            Some(_) if open == Some(*g) => {
                if let Some(PsdItem::Group { children, .. }) = items.last_mut() {
                    children.push(layer);
                }
            }
            Some(gi) => {
                open = Some(*g);
                items.push(PsdItem::Group {
                    name: gi.name.clone(),
                    opacity: decimal(gi.opacity),
                    visible: gi.visible,
                    children: vec![layer],
                });
            }
        }
    }
    let n = (w * h) as usize;
    let mut pixels = |k: usize| -> LayerPixels {
        let i = order[k];
        let (props, _) = &meta[i];
        let id = ids[i];
        if props.adjust.is_some() {
            let mask = mask_of(&p, id, w, h);
            return (vec![0; n * 4], mask);
        }
        // a style has no place in the file: drawn into the pixels (with the mask)
        if props.style.any() {
            return (p.layer_styled_rgba(id, 0), None);
        }
        (p.layer_rgba(id), mask_of(&p, id, w, h))
    };
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    write_psd(
        std::io::BufWriter::new(f),
        w,
        h,
        &composite,
        &items,
        &mut pixels,
    )
    .map_err(|e| e.to_string())
}

/// A layer's mask at full size, one byte a pixel (None: it has none).
fn mask_of(p: &Painter, id: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    let m = p.mask_thumbnail(id, w, h);
    (!m.is_empty()).then(|| m.as_chunks::<4>().0.iter().map(|px| px[0]).collect())
}
