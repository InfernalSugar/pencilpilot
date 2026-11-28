//! Opening and saving projects: round trips (pixels, sheet, settings), what's kept of what
//! the native app doesn't edit, unsaved changes, New, recent files, the recovery copy.
//! (The file dialog's part is in `tests/files_flow.rs`: it sets the environment.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ppstudio::{AudioClip, Cut, Exposure, Layer, Palette, PaletteColour, Project, Scene};

use super::*;
use crate::state::State;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join(format!("pp-files-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn layer(id: u32, name: &str) -> Layer {
    Layer {
        id,
        name: name.into(),
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

/// A drawing: soft dots in a colour, see-through at their edges (as brushes leave them).
fn picture(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut r = seed.wrapping_mul(2_654_435_761).max(1);
    let mut next = || {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        r
    };
    for _ in 0..40 {
        let (cx, cy) = ((next() % w) as i32, (next() % h) as i32);
        let rad = 3 + (next() % 14) as i32;
        let col = [next() as u8, next() as u8, next() as u8];
        for y in (cy - rad).max(0)..(cy + rad).min(h as i32) {
            for x in (cx - rad).max(0)..(cx + rad).min(w as i32) {
                let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt() / rad as f32;
                if d < 1.0 {
                    let a = ((1.0 - d) * 255.0) as u8;
                    let i = ((y as u32 * w + x as u32) * 4) as usize;
                    if px[i + 3] < a {
                        px[i..i + 4].copy_from_slice(&[col[0], col[1], col[2], a]);
                    }
                }
            }
        }
    }
    px
}

fn ex(d: Option<&str>, length: u32) -> Exposure {
    Exposure {
        drawing: d.map(Into::into),
        length,
    }
}

/// A project as the web app writes them: 300 × 170 (not a multiple of 64), three layers
/// (a static background), two scenes (the second's second cut open, as `extra` says),
/// marks, palettes, a sound, line data, a text layer's settings, and a drawing in a cut not
/// opened.
fn web_project(path: &Path) -> Project {
    let (w, h) = (300, 170);
    let mut p = Project::new("PencilPilot 0.1.0", "2026-11-01T10:00:00Z", w, h, 12);
    let mut bg = layer(1, "Paper");
    bg.locked = true;
    let mut ink = layer(2, "Ink");
    ink.opacity = 0.75;
    ink.blend = "multiply".into();
    ink.text = Some("{\"words\":\"hi\"}".into());
    let mut col = layer(3, "Colour");
    col.visible = false;
    p.manifest.layers = vec![bg, col, ink];
    for (i, d) in [1, 2, 3, 4, 5, 6, 9].iter().enumerate() {
        p.put_drawing(&format!("D{d}"), &picture(w, h, i as u32 + 1), None)
            .unwrap();
    }
    let audio = p.put_audio(b"RIFF fake sound", "wav").unwrap();
    p.lines.insert("D9".into(), b"line data".to_vec());
    let other = Scene {
        name: "Opening".into(),
        cuts: vec![Cut {
            name: "Cut 1".into(),
            tracks: [(2, vec![ex(Some("D9"), 3)])].into(),
            ..Default::default()
        }],
        audio: vec![AudioClip {
            name: "beat".into(),
            file: audio,
            start: 0,
            gain: 1.0,
            muted: false,
        }],
        ..Default::default()
    };
    let open = Scene {
        name: "Chase".into(),
        cuts: vec![
            Cut {
                name: "Cut 1".into(),
                ..Default::default()
            },
            Cut {
                name: "Cut 2".into(),
                tracks: [
                    (1, vec![ex(Some("D1"), 8)]),
                    (
                        2,
                        vec![
                            ex(Some("D2"), 2),
                            ex(None, 1),
                            ex(Some("D3"), 3),
                            ex(Some("D2"), 2),
                        ],
                    ),
                    (
                        3,
                        vec![ex(Some("D4"), 4), ex(Some("D5"), 4), ex(Some("D6"), 1)],
                    ),
                ]
                .into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    p.manifest.timeline.scenes = vec![other, open];
    p.manifest.timeline.static_layers = vec![1];
    p.manifest.timeline.drawing_kinds = [
        ("D2".into(), "key".into()),
        ("D3".into(), "breakdown".into()),
    ]
    .into();
    p.manifest.timeline.layer_stages = [("2".into(), "clean".into())].into();
    p.manifest.timeline.palettes = vec![
        Palette {
            name: "Fox".into(),
            colours: vec![
                PaletteColour {
                    name: "Fur".into(),
                    hex: "#d2691e".into(),
                },
                PaletteColour {
                    name: "Eye".into(),
                    hex: "#102030".into(),
                },
            ],
        },
        Palette {
            name: "Crow".into(),
            colours: vec![],
        },
    ];
    p.manifest.timeline.frames = ppstudio::Timeline::flatten(&p.manifest.timeline.scenes, &[1]);
    p.manifest.active_layer = 3;
    p.manifest.extra = serde_json::json!({
        "brushId": "pen", "color": "#d2691e", "size": 9, "scene": 1, "cut": 1, "frame": 5,
        "guides": [1, 2],
    });
    p.strokes = b"[{\"stroke\":1}]".to_vec();
    p.thumbnail = Some(b"old thumbnail".to_vec());
    p.save(path).unwrap();
    p
}

fn engine() -> (Engine, State) {
    let mut s = State::demo();
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    e.tick(&mut s, Instant::now());
    (e, s)
}

/// A layer's name, kind, opacity and cels (drawing, hold, mark).
type SheetRow = (String, char, f32, Vec<(u32, u32, Mark)>);

/// Every drawing on the sheet, its pixels; the composite; the sheet without ids.
#[derive(Debug, PartialEq)]
struct Seen {
    composite: Vec<u8>,
    drawings: BTreeMap<u32, Vec<u8>>,
    sheet: Vec<SheetRow>,
    layers: Vec<crate::state::Layer>,
    frames: u32,
    fps: u32,
}

fn seen(e: &mut Engine, s: &State) -> Seen {
    let _ = e.picture(s);
    e.painter.flush();
    let mut drawings = BTreeMap::new();
    for t in &s.sheet.tracks {
        for d in t.drawings() {
            drawings.insert(d, e.drawing_rgba(d));
        }
    }
    Seen {
        composite: e.painter.rgba().to_vec(),
        drawings,
        sheet: s
            .sheet
            .tracks
            .iter()
            .zip(&s.layers)
            .map(|(t, l)| {
                (
                    l.name.clone(),
                    t.kind,
                    t.opacity,
                    t.cels.iter().map(|c| (c.drawing, c.hold, c.mark)).collect(),
                )
            })
            .collect(),
        layers: s.layers.clone(),
        frames: s.frames,
        fps: s.fps,
    }
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// How far two drawings differ where they show (colour under alpha below 16 can't
/// survive premultiplying), and in alpha anywhere.
fn drawing_diff(want: &[u8], got: &[u8]) -> (u8, u8) {
    let colour = want
        .chunks(4)
        .zip(got.chunks(4))
        .filter(|(a, _)| a[3] >= 16)
        .map(|(a, b)| (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap())
        .max()
        .unwrap_or(0);
    let alpha = want
        .chunks(4)
        .zip(got.chunks(4))
        .map(|(a, b)| a[3].abs_diff(b[3]))
        .max()
        .unwrap_or(0);
    (colour, alpha)
}

#[test]
fn a_web_project_opens_as_it_was_saved() {
    let d = dir("web");
    let path = d.join("chase.ppstudio");
    let p = web_project(&path);
    let (mut e, mut s) = engine();
    let doc = s.doc_id;
    assert!(open_path(&mut e, &mut s, &path), "{}", s.project.note);
    assert_eq!(s.doc_id, doc + 1);
    // the size, the cut the file had open, its layers (top first), the frame
    assert_eq!((e.painter.width(), e.painter.height()), (300, 170));
    assert_eq!((s.view.doc_w, s.view.doc_h), (300, 170));
    assert_eq!(s.project.name, "chase");
    assert_eq!(s.project.scene, "Chase");
    assert_eq!(s.cut_name, "Cut 2");
    assert_eq!(s.fps, 12);
    assert_eq!(s.frames, 9, "the cut's longest cel layer");
    assert_eq!(s.frame, 5);
    let names: Vec<&str> = s.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Ink", "Colour", "Paper"]);
    assert_eq!(s.layers[2].kind, 'S');
    assert!(s.layers[2].locked && !s.layers[1].visible);
    assert_eq!(s.active_layer, 1, "the file's active layer (Colour)");
    assert!((s.sheet.tracks[0].opacity - 0.75).abs() < 1e-6);
    let ink: Vec<(u32, u32, Mark)> = s.sheet.tracks[0]
        .cels
        .iter()
        .map(|c| (c.drawing, c.hold, c.mark))
        .collect();
    assert_eq!(
        ink,
        [
            (2, 2, Mark::Key),
            (0, 1, Mark::None),
            (3, 3, Mark::Breakdown),
            (2, 2, Mark::Key)
        ]
    );
    let pl = e
        .painter
        .layers()
        .iter()
        .find(|l| l.id == s.sheet.tracks[0].layer)
        .unwrap();
    assert_eq!(pl.props.blend.as_str(), "multiply");
    assert!((pl.props.opacity - 0.75).abs() < 1e-6);
    // every drawing's pixels, within a level (the painter keeps them premultiplied)
    for dr in [1u32, 2, 3, 4, 5, 6] {
        let want = p.drawing(&format!("D{dr}")).unwrap().0;
        let (colour, alpha) = drawing_diff(&want, &e.drawing_rgba(dr));
        assert!(
            colour <= 1 && alpha <= 1,
            "drawing {dr}: {colour} / {alpha}"
        );
    }
    // the palette, colour and size from the file; no demo left
    assert_eq!(s.palette_name, "Fox");
    assert_eq!(s.palette.len(), 2);
    assert_eq!(s.colour_name, "Fur");
    assert_eq!(s.brush_size, 9.0);
    assert!(s.notes.is_empty(), "the demo's notes are gone");
    assert!(!s.sheet.demo);
    assert!(!s.project.unsaved);
    assert!(
        e.painter.layers().len() == 3,
        "the demo's four layers are gone"
    );
    // drawings elsewhere (the other scene's) don't get their ids taken by new ones
    assert!(e.painter.new_drawing() > 9);
}

/// open → save → open: the same pixels, sheet and settings; and what the native app
/// doesn't edit is kept.
#[test]
fn round_trip_keeps_everything() {
    let d = dir("round");
    let path = d.join("chase.ppstudio");
    let original = web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    let before = seen(&mut e, &s);
    let out = d.join("again.ppstudio");
    assert!(save_to(&mut e, &mut s, &out), "{}", s.project.note);
    assert_eq!(s.project.path.as_deref(), Some(out.as_path()));
    assert_eq!(s.project.name, "again");
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &out), "{}", s2.project.note);
    let after = seen(&mut e2, &s2);
    assert_eq!(before.layers, after.layers);
    assert_eq!(before.sheet, after.sheet);
    assert_eq!((before.frames, before.fps), (after.frames, after.fps));
    assert_eq!(
        before.drawings.keys().collect::<Vec<_>>(),
        after.drawings.keys().collect::<Vec<_>>()
    );
    for (k, v) in &before.drawings {
        assert!(*v == after.drawings[k], "drawing {k} changed");
    }
    assert!(before.composite == after.composite, "the composite changed");
    assert_eq!(s2.palette_name, "Fox");
    assert_eq!(s2.frame, s.frame);

    // the file, read as the web app reads it (all of it, checked)
    let back = Project::open(&out).unwrap();
    let m = &back.manifest;
    let om = &original.manifest;
    assert_eq!(m.created, om.created, "created kept");
    assert_eq!(m.timeline.scenes.len(), 2);
    assert_eq!(
        m.timeline.scenes[0], om.timeline.scenes[0],
        "the other scene, as it was"
    );
    assert_eq!(m.timeline.scenes[1].cuts[0], om.timeline.scenes[1].cuts[0]);
    let (got, want) = (
        &m.timeline.scenes[1].cuts[1].tracks,
        &om.timeline.scenes[1].cuts[1].tracks,
    );
    assert_eq!(
        (&got[&2], &got[&3]),
        (&want[&2], &want[&3]),
        "the open cut's exposures (the file's layer ids, D names, as the web app's)"
    );
    // (a static layer's one drawing, held all cut long)
    assert_eq!(got[&1], vec![ex(Some("D1"), 9)]);
    assert_eq!(m.timeline.static_layers, vec![1]);
    assert_eq!(m.timeline.drawing_kinds, om.timeline.drawing_kinds);
    assert_eq!(m.timeline.layer_stages, om.timeline.layer_stages);
    assert_eq!(m.timeline.palettes, om.timeline.palettes);
    assert_eq!(
        m.timeline.frames,
        ppstudio::Timeline::flatten(&m.timeline.scenes, &m.timeline.static_layers)
    );
    let ink = m.layers.iter().find(|l| l.id == 2).unwrap();
    assert_eq!(ink.text.as_deref(), Some("{\"words\":\"hi\"}"), "kept");
    assert_eq!(ink.blend, "multiply");
    assert!((ink.opacity - 0.75).abs() < 1e-6);
    let ids: Vec<u32> = m.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids, [1, 3, 2], "bottom first, the file's ids");
    assert_eq!(m.active_layer, 3);
    assert_eq!(back.audio, original.audio, "the sound, copied over");
    assert_eq!(back.lines, original.lines, "line data, copied over");
    assert_eq!(
        back.strokes, original.strokes,
        "the stroke log, copied over"
    );
    assert!(
        back.thumbnail
            .as_ref()
            .is_some_and(|t| t.starts_with(b"\x89PNG")),
        "a new thumbnail"
    );
    assert_eq!(back.drawing("D9").unwrap(), original.drawing("D9").unwrap());
    assert_eq!(m.extra["scene"], 1);
    assert_eq!(m.extra["cut"], 1);
    assert_eq!(m.extra["color"], "#d2691e");
    assert_eq!(m.extra["guides"], serde_json::json!([1, 2]));
    // every drawing the scenes show is an app drawing ("D<n>") with pixels
    for c in m.timeline.scenes.iter().flat_map(|s| &s.cuts) {
        for t in c.tracks.values() {
            for n in t.iter().filter_map(|x| x.drawing.as_ref()) {
                assert!(
                    n.strip_prefix('D')
                        .is_some_and(|k| k.parse::<u32>().is_ok())
                );
                back.drawing(n).unwrap();
            }
        }
    }
    // saved again unchanged: the same drawings, no tile encoded again
    let third = d.join("third.ppstudio");
    assert!(save_to(&mut e2, &mut s2, &third));
    let t = e2.files.last_save.clone().unwrap();
    assert_eq!(t.new_tiles, 0, "{t:?}");
    assert_eq!(
        Project::open(&third).unwrap().manifest.drawings,
        back.manifest.drawings
    );
}

/// A file with no scenes (one frame of layers, "L<n>" drawings: the web app's stills,
/// simple tools) opens as one cut, and saves as an animation the web app opens.
#[test]
fn a_still_opens_as_one_cut() {
    let d = dir("still");
    let path = d.join("still.ppstudio");
    let mut p = Project::new("x", "2026-01-01T00:00:00Z", 128, 64, 24);
    p.manifest.layers = vec![layer(5, "Base"), layer(7, "Top")];
    p.put_drawing("L5", &picture(128, 64, 3), None).unwrap();
    p.put_drawing("L7", &picture(128, 64, 4), None).unwrap();
    p.manifest.timeline.frames[0].cels = [(5, "L5".into()), (7, "L7".into())].into();
    p.save(&path).unwrap();
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path), "{}", s.project.note);
    assert_eq!(s.layers.len(), 2);
    assert_eq!(s.frames, 1);
    let top = s.sheet.tracks[0].drawing_at(0);
    assert!(top != 0);
    let (c, a) = drawing_diff(&p.drawing("L7").unwrap().0, &e.drawing_rgba(top));
    assert!(c <= 1 && a <= 1);
    let out = d.join("out.ppstudio");
    assert!(save_to(&mut e, &mut s, &out));
    let back = Project::open(&out).unwrap();
    assert_eq!(back.manifest.timeline.scenes.len(), 1);
    assert_eq!(back.manifest.drawings.len(), 2);
    assert!(back.manifest.drawings.keys().all(|k| k.starts_with('D')));
}

#[test]
fn a_damaged_file_changes_nothing() {
    let d = dir("bad");
    let path = d.join("bad.ppstudio");
    std::fs::write(&path, b"not a project").unwrap();
    let (mut e, mut s) = engine();
    let layers = s.layers.clone();
    assert!(!open_path(&mut e, &mut s, &path));
    assert!(
        s.project.note.starts_with("Couldn’t open bad"),
        "{}",
        s.project.note
    );
    assert_eq!(s.layers, layers);
    assert_eq!(s.project.name, "Ember");
    assert!(s.project.subtitle(&s.cut_name).starts_with("Couldn’t open"));
}

fn stroke(e: &mut Engine, s: &State, x: f32, y: f32) {
    e.down(s, x, y, 0.9, (0.0, 0.0), 0);
    for i in 1..30 {
        e.moved(x + i as f32 * 2.0, y + 2.0, 0.9, (0.0, 0.0), i * 4);
    }
    e.up();
}

/// Painting or editing the sheet leaves changes unsaved; saving saves them (only the
/// changed tiles encoded).
#[test]
fn edits_are_unsaved_until_saved() {
    let d = dir("unsaved");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    e.tick(&mut s, Instant::now());
    assert!(!s.project.unsaved);
    assert_eq!(s.project.subtitle(&s.cut_name), "Chase · Cut 2 · saved");
    // a stroke on Ink at frame 0 (drawing 2)
    s.active_layer = 0;
    s.frame = 0;
    s.tool = crate::state::Tool::Brush;
    s.brush_size = 6.0;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    stroke(&mut e, &s, 10.0, 10.0);
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved, "a stroke is an unsaved change");
    assert!(s.project.subtitle(&s.cut_name).ends_with("unsaved"));
    let painted = e.drawing_rgba(2);
    assert!(save_to(&mut e, &mut s, &path));
    assert!(!s.project.unsaved);
    let t = e.files.last_save.clone().unwrap();
    assert!((1..=2).contains(&t.new_tiles), "the stroke's tiles: {t:?}");
    let back = Project::open(&path).unwrap();
    let (c, a) = drawing_diff(&back.drawing("D2").unwrap().0, &painted);
    assert!(c <= 1 && a <= 1, "the stroke saved");
    // an undo back past the save: unsaved again
    e.apply(&mut s, &Action::Undo);
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved);
    assert!(save_to(&mut e, &mut s, &path));
    // a sheet edit (a hold) is one too, and a layer's eye
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved, "a hold changed");
    assert!(save_to(&mut e, &mut s, &path));
    s.layers[2].visible = false;
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved, "a layer hidden");
    assert!(save_to(&mut e, &mut s, &path));
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &path));
    assert!(!s2.layers[2].visible);
    assert_eq!(s2.sheet.tracks[0].cels[0].hold, 3);
    // a new layer and a new drawing on it survive too
    e.apply(&mut s, &Action::NewLayer);
    e.tick(&mut s, Instant::now());
    s.frame = 0;
    e.apply(&mut s, &Action::NewDrawing);
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    stroke(&mut e, &s, 100.0, 100.0);
    e.tick(&mut s, Instant::now());
    let nd = s.sheet.tracks[s.active_layer].drawing_at(s.frame);
    assert!(nd > 9, "a new drawing: {nd}");
    let px = e.drawing_rgba(nd);
    assert!(px.chunks(4).any(|p| p[3] > 0));
    assert!(save_to(&mut e, &mut s, &path));
    let (mut e3, mut s3) = engine();
    assert!(open_path(&mut e3, &mut s3, &path));
    assert_eq!(s3.layers.len(), 4);
    // (painted here: a level off at most where the paint is faint, as the file quantises)
    let (c, a) = drawing_diff(&px, &e3.drawing_rgba(nd));
    assert!(c <= 1 && a <= 1, "{c} {a}");
    let back = Project::open(&path).unwrap();
    assert_eq!(back.manifest.layers.len(), 4);
    assert!(back.manifest.layers.iter().any(|l| l.id == 4), "a new id");
}

/// New: one empty layer at 1920 × 1080, 24 fps; with unsaved changes it asks first.
#[test]
fn new_asks_first_when_unsaved() {
    let (mut e, mut s) = engine();
    let first_doc = s.doc_id;
    // the demo has nothing unsaved: New at once
    assert!(e.apply(&mut s, &Action::File(NEW.into())));
    assert!(s.project.asking.is_none());
    assert_eq!(s.project.name, "Untitled");
    assert_eq!((e.painter.width(), e.painter.height()), (1920, 1080));
    assert_eq!(s.layers.len(), 1);
    assert_eq!(e.painter.layers().len(), 1);
    assert_eq!(s.sheet.tracks[0].cels, vec![Cel::new(0, 1, Mark::None)]);
    assert_eq!((s.frames, s.fps), (1, 24));
    assert!(s.notes.is_empty() && !s.sheet.demo);
    assert_eq!(s.doc_id, first_doc + 1);
    // paint: unsaved; New asks
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    s.tool = crate::state::Tool::Brush;
    stroke(&mut e, &s, 50.0, 50.0);
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved);
    e.apply(&mut s, &Action::File(NEW.into()));
    assert_eq!(
        s.project.asking.as_ref().map(|q| q.title.as_str()),
        Some("Save the changes to Untitled first?")
    );
    // Cancel: nothing happens
    e.apply(&mut s, &Action::File(CONFIRM_CANCEL.into()));
    assert!(s.project.asking.is_none() && s.project.unsaved);
    let d = s.sheet.tracks[0].drawing_at(0);
    assert!(e.drawing_rgba(d).chunks(4).any(|p| p[3] > 0));
    // Don't save: a new one
    e.apply(&mut s, &Action::File(NEW.into()));
    e.apply(&mut s, &Action::File(CONFIRM_DISCARD.into()));
    assert!(s.project.asking.is_none() && !s.project.unsaved);
    assert_eq!(s.sheet.tracks[0].drawing_at(0), 0, "empty again");
    assert_eq!(s.doc_id, first_doc + 2);
}

/// "Save first?" → Save, on a project with a file: saved, then the other one opened.
#[test]
fn save_first_saves_then_opens() {
    let d = dir("savefirst");
    let a = d.join("a.ppstudio");
    let b = d.join("b.ppstudio");
    web_project(&a);
    web_project(&b);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &a));
    e.tick(&mut s, Instant::now());
    s.active_layer = 0;
    s.frame = 0;
    e.apply(&mut s, &Action::Other("Hold on 1s".into()));
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved);
    let recent = format!("{OPEN_RECENT}{}", b.display());
    e.apply(&mut s, &Action::File(recent));
    assert!(s.project.asking.is_some());
    e.apply(&mut s, &Action::File(CONFIRM_SAVE.into()));
    assert!(s.project.saving, "saving…");
    assert!(s.project.subtitle(&s.cut_name).ends_with("saving…"));
    wait_for_save(&mut e, &mut s);
    assert_eq!(s.project.name, "b");
    assert!(!s.project.unsaved);
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &a));
    assert_eq!(s2.sheet.tracks[0].cels[0].hold, 1, "a was saved first");
}

#[test]
fn recent_files_newest_first_ten_at_most() {
    let d = dir("recent");
    let mut last = Vec::new();
    for i in 0..12 {
        let p = d.join(format!("p{i}.ppstudio"));
        std::fs::write(&p, b"x").unwrap();
        last = recent::add(&p);
    }
    assert_eq!(last.len(), 10);
    assert!(last[0].ends_with("p11.ppstudio"));
    // (other checks add files too, in parallel: just that ours are in order)
    let at = |v: &[PathBuf], n: &str| v.iter().position(|x| x.ends_with(n));
    assert!(at(&last, "p11.ppstudio") < at(&last, "p10.ppstudio"));
    let p = d.join("p5.ppstudio");
    let v = recent::add(&p);
    assert!(v[0].ends_with("p5.ppstudio"));
    assert_eq!(v.iter().filter(|x| x.ends_with("p5.ppstudio")).count(), 1);
    assert!(recent::load().iter().any(|x| x.ends_with("p5.ppstudio")));
    // a file gone isn't listed
    std::fs::remove_file(&p).unwrap();
    assert!(!recent::load().iter().any(|x| x.ends_with("p5.ppstudio")));
    assert!(recent::config_dir().join("recent").is_file());
}

/// Two minutes after an unsaved change, a recovery copy (not over the project's file) that
/// opens as the project with the change; saving lets it go.
#[test]
fn a_recovery_copy_every_two_minutes() {
    let d = dir("recovery");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let before = std::fs::read(&path).unwrap();
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    e.tick(&mut s, Instant::now());
    s.active_layer = 0;
    s.frame = 0;
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved);
    let at = next_tick_at(&e, &s).unwrap();
    assert!(at > Instant::now() + Duration::from_secs(100), "not yet");
    e.tick(&mut s, at + Duration::from_millis(1));
    wait_for_save(&mut e, &mut s);
    let rec = s.project.recovery.clone().expect("a recovery copy");
    assert!(s.project.unsaved, "a recovery copy isn't a save");
    assert!(rec.starts_with(recent::state_dir().join("recovery")));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the project's file untouched"
    );
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &rec));
    assert_eq!(s2.sheet.tracks[0].cels[0].hold, 3, "the change is in it");
    assert!(next_tick_at(&e, &s).unwrap() > Instant::now() + Duration::from_secs(100));
    assert!(save_to(&mut e, &mut s, &path));
    assert!(!rec.exists(), "saved: the recovery copy goes");
}

#[test]
fn a_file_given_at_startup_opens() {
    let d = dir("startup");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let mut s = State::demo();
    s.project.startup = Some(path.clone());
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    e.tick(&mut s, Instant::now());
    assert_eq!(s.project.name, "chase");
    assert_eq!((e.painter.width(), e.painter.height()), (300, 170));
    assert!(
        s.project
            .recent
            .first()
            .is_some_and(|p| p.ends_with("chase.ppstudio"))
    );
}

/// Only the drawings wanted now are in the painter (showing, onion skin, undo); the rest
/// are kept compressed, come back when shown, and save as they were; an undo on another
/// frame still works, and a drawing put away after a change keeps it exactly.
#[test]
fn drawings_not_showing_stay_out_of_the_painter() {
    let d = dir("store");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    s.onion = false;
    s.frame = 0;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    let resident = |e: &Engine| {
        let mut v: Vec<u32> = (1..=9).filter(|d| e.painter.has_drawing(*d)).collect();
        v.sort();
        v
    };
    // what frames f - 3 to f + 5 show (frames around are brought in ahead, as far as they
    // fit: all of these, here)
    let near = |s: &State, f: u32| -> Vec<u32> {
        let n = s.frames as i64;
        let mut v: Vec<u32> = [0i64, 1, -1, 2, -2, 3, -3, 4, 5]
            .iter()
            .map(|k| (f as i64 + k).rem_euclid(n) as u32)
            .collect::<Vec<u32>>()
            .iter()
            .flat_map(|&g| s.sheet.tracks.iter().map(move |t| t.drawing_at(g)))
            .filter(|d| *d != 0)
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let first = seen(&mut e, &s);
    for f in 0..s.frames {
        s.frame = f;
        e.tick(&mut s, Instant::now());
        let _ = e.picture(&s);
        // (what isn't wanted is put away after a pause)
        e.tick(&mut s, Instant::now() + Duration::from_millis(100));
        let showing: Vec<u32> = s
            .sheet
            .tracks
            .iter()
            .map(|t| t.drawing_at(f))
            .filter(|d| *d != 0)
            .collect();
        let held = resident(&e);
        for dd in &showing {
            assert!(held.contains(dd), "frame {f}: {dd} showing");
        }
        // (what the frame before had near stays until the drawings showing change)
        let mut ok = near(&s, f);
        ok.extend(near(&s, f.saturating_sub(1)));
        assert!(
            held.iter().all(|d| ok.contains(d)),
            "frame {f}: {held:?}, only {ok:?}"
        );
    }
    // (and ahead: the next frame's, decoded on another thread, arrive)
    s.frame = 0;
    e.tick(&mut s, Instant::now());
    let t = Instant::now();
    while !near(&s, 0).iter().all(|d| e.painter.has_drawing(*d)) {
        assert!(t.elapsed() < Duration::from_secs(5), "brought in ahead");
        std::thread::sleep(Duration::from_millis(2));
        e.tick(&mut s, Instant::now());
    }
    assert_eq!(resident(&e), near(&s, 0));
    // the onion skin's neighbours come in too
    s.onion = true;
    s.active_layer = 0;
    s.frame = 3;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    for x in super::super::frames::onion_entries(&s).chunks(3) {
        assert!(e.painter.has_drawing(x[0]), "onion {}", x[0]);
    }
    s.onion = false;
    // a stroke on frame 0 (Ink's drawing 2), then away: an undo there still works
    s.frame = 0;
    s.tool = crate::state::Tool::Brush;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    let before = e.drawing_rgba(2);
    stroke(&mut e, &s, 30.0, 30.0);
    e.tick(&mut s, Instant::now());
    let painted = e.drawing_rgba(2);
    assert_ne!(before, painted);
    s.frame = 4;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    e.tick(&mut s, Instant::now() + Duration::from_millis(100));
    assert!(e.painter.has_drawing(2), "undo still changes it: kept");
    e.apply(&mut s, &Action::Undo);
    e.tick(&mut s, Instant::now());
    assert_eq!(e.drawing_rgba(2), before, "undone on another frame");
    e.apply(&mut s, &Action::Redo);
    e.tick(&mut s, Instant::now());
    assert_eq!(e.drawing_rgba(2), painted);
    // the history let go: the changed drawing is put away, as it is (no rounding), once
    // far enough (Ink's drawing 3 held 18 frames longer: frame 12 is far from drawing 2)
    s.frame = 3;
    for _ in 0..18 {
        e.apply(&mut s, &Action::Other("Insert frame".into()));
    }
    e.painter.clear_history();
    s.frame = 12;
    e.tick(&mut s, Instant::now());
    let _ = e.picture(&s);
    e.tick(&mut s, Instant::now() + Duration::from_millis(100));
    assert!(!e.painter.has_drawing(2), "put away");
    assert!(matches!(
        e.frames.store.drawings.get(&2).map(|k| &**k),
        Some(super::super::frames::store::Kept::Raw(_))
    ));
    for _ in 0..3 {
        for f in [0, 12] {
            s.frame = f;
            e.tick(&mut s, Instant::now());
            let _ = e.picture(&s);
            e.tick(&mut s, Instant::now() + Duration::from_millis(100));
        }
    }
    assert_eq!(
        e.drawing_rgba(2),
        painted,
        "back and forth: exactly the same"
    );
    // saved: every drawing, the changed one as it is, the others as the file had them
    let out = d.join("out.ppstudio");
    assert!(save_to(&mut e, &mut s, &out));
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &out));
    let after = seen(&mut e2, &s2);
    for (k, v) in &first.drawings {
        if *k == 2 {
            let (c, a) = drawing_diff(&painted, &after.drawings[k]);
            assert!(c <= 1 && a <= 1);
        } else {
            assert!(*v == after.drawings[k], "drawing {k}");
        }
    }
    // thumbnails: every drawing has one
    for _ in 0..40 {
        e2.tick(&mut s2, Instant::now());
    }
    for t in &s2.sheet.tracks {
        for dd in t.drawings() {
            assert!(s2.sheet.thumbs.contains_key(&dd), "thumbnail {dd}");
        }
    }
}

/// Quit: at once with nothing unsaved; else "Save first?": Cancel stays, Don't save quits,
/// Save quits once the file is written.
#[test]
fn quitting_asks_to_save_first() {
    let d = dir("quit");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    e.tick(&mut s, Instant::now());
    assert!(e.apply(&mut s, &Action::File(QUIT.into())));
    assert!(s.quit_now, "nothing unsaved: quits");
    s.quit_now = false;
    s.active_layer = 0;
    s.frame = 0;
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    e.tick(&mut s, Instant::now());
    quit_requested(&mut e, &mut s);
    let q = s.project.asking.clone().expect("asks");
    assert_eq!(q.title, "Save the changes to chase before quitting?");
    assert!(!s.quit_now);
    e.apply(&mut s, &Action::File(CONFIRM_CANCEL.into()));
    assert!(!s.quit_now && s.project.asking.is_none(), "Cancel stays");
    quit_requested(&mut e, &mut s);
    e.apply(&mut s, &Action::File(CONFIRM_SAVE.into()));
    assert!(!s.quit_now, "not until it's written");
    wait_for_save(&mut e, &mut s);
    assert!(s.quit_now, "written: quits");
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &path));
    assert_eq!(s2.sheet.tracks[0].cels[0].hold, 3, "saved before quitting");
    s.quit_now = false;
    e.apply(&mut s, &Action::Other("Hold on 1s".into()));
    e.tick(&mut s, Instant::now());
    quit_requested(&mut e, &mut s);
    e.apply(&mut s, &Action::File(CONFIRM_DISCARD.into()));
    assert!(s.quit_now, "Don't save: quits");
}

/// A save asked for while one is written waits for it; the window's part is short.
#[test]
fn saves_are_written_on_another_thread() {
    let d = dir("background");
    let path = d.join("chase.ppstudio");
    web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    e.tick(&mut s, Instant::now());
    s.active_layer = 0;
    s.frame = 0;
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    e.apply(&mut s, &Action::File(SAVE.into()));
    assert!(s.project.saving);
    e.apply(&mut s, &Action::Other("Hold on 1s".into()));
    e.apply(&mut s, &Action::File(SAVE.into()));
    let t = Instant::now();
    while s.project.saving || e.files.saving.is_some() {
        e.tick(&mut s, Instant::now());
        assert!(t.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    e.tick(&mut s, Instant::now());
    assert!(!s.project.unsaved, "the second save took the second change");
    let (mut e2, mut s2) = engine();
    assert!(open_path(&mut e2, &mut s2, &path));
    assert_eq!(s2.sheet.tracks[0].cels[0].hold, 1);
    // a change made while a save is written stays unsaved
    e.apply(&mut s, &Action::Other("Hold on 2s".into()));
    e.apply(&mut s, &Action::File(SAVE.into()));
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    wait_for_save(&mut e, &mut s);
    e.tick(&mut s, Instant::now());
    assert!(s.project.unsaved, "changed after the save started");
}

/// At start, a recovery copy newer than its file is offered: opened, it's the project with
/// its changes unsaved (a save goes to the project's file); discarded, it goes.
#[test]
fn a_recovery_copy_is_offered_at_start() {
    OFFER_RECOVERY.with(|o| o.set(true));
    let d = dir("offer");
    recent::TEST_HOME.with(|h| *h.borrow_mut() = Some(d.join("home")));
    let path = d.join("offered.ppstudio");
    web_project(&path);
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path));
    e.tick(&mut s, Instant::now());
    s.active_layer = 0;
    s.frame = 0;
    e.apply(&mut s, &Action::Other("Hold on 3s".into()));
    e.tick(&mut s, Instant::now());
    let at = next_tick_at(&e, &s).unwrap();
    e.tick(&mut s, at + Duration::from_millis(1));
    wait_for_save(&mut e, &mut s);
    let rec = s.project.recovery.clone().unwrap();
    // the app starts again (the old one closed without saving)
    drop(e);
    let (mut e, mut s) = engine();
    let q = s.project.asking.clone().expect("offered");
    assert_eq!(q.title, "Recover your unsaved changes to offered?");
    e.apply(&mut s, &Action::File(RECOVER_OPEN.into()));
    assert_eq!(s.project.path.as_deref(), Some(path.as_path()));
    assert_eq!(s.project.name, "offered");
    assert!(s.project.unsaved);
    assert_eq!(s.sheet.tracks[0].cels[0].hold, 3, "the changes are back");
    assert!(save_to(&mut e, &mut s, &path));
    assert!(!rec.exists(), "saved: the copy goes");
    let (_e, s) = engine();
    assert!(s.project.asking.is_none(), "nothing more to offer");
    OFFER_RECOVERY.with(|o| o.set(false));
    recent::TEST_HOME.with(|h| *h.borrow_mut() = None);
}

/// Open and save times for the baseline's projects (`tools/baseline/fixtures.py` into
/// `PP_FIXTURES`), their round trips, and PNGs of what opened into `PP_SAVE`.
#[test]
#[ignore = "PP_FIXTURES=dir cargo test --release -p pencilpilot-native -- --ignored fixtures --nocapture"]
fn fixtures_open_save_and_round_trip() {
    let Ok(dir_s) = std::env::var("PP_FIXTURES") else {
        return;
    };
    let fixtures = PathBuf::from(dir_s);
    let out = std::env::var("PP_SAVE").ok().map(PathBuf::from);
    for name in ["empty", "layers20-4k", "drawings500"] {
        let path = fixtures.join(format!("{name}.ppstudio"));
        let (mut e, mut s) = engine();
        let mut opens = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            assert!(open_path(&mut e, &mut s, &path), "{}", s.project.note);
            opens.push((t.elapsed().as_secs_f64() * 1e3).round());
        }
        let ot = e.files.last_open.clone().unwrap();
        let t = Instant::now();
        let _ = e.picture(&s);
        let first_picture = t.elapsed().as_secs_f64() * 1e3;
        // scrubbing at 165 Hz (a frame change each refresh, 6 ms apart): each frame in
        // turn, both ways; then jumps anywhere. A change: the tick and the picture; and of
        // that, bringing drawings back from the store
        for _ in 0..600 {
            e.tick(&mut s, Instant::now());
        }
        let p95 = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let at = |q: usize| v[(v.len() * q / 100).min(v.len() - 1)];
            (at(50), at(95), at(99))
        };
        let scrub = |e: &mut Engine, s: &mut State, order: Vec<u32>| {
            let (mut all, mut back) = (Vec::new(), Vec::new());
            for f in order {
                // (what arrived meanwhile: the loop ticks between frames)
                e.tick(s, Instant::now());
                let start = Instant::now();
                s.frame = f;
                let ds: Vec<u32> = s.sheet.tracks.iter().map(|t| t.drawing_at(f)).collect();
                let t = Instant::now();
                super::super::frames::load_drawings(e, &ds);
                back.push(t.elapsed().as_secs_f64() * 1e3);
                e.tick(s, Instant::now());
                let _ = e.picture(s);
                all.push(start.elapsed().as_secs_f64() * 1e3);
                if let Some(rest) = Duration::from_micros(6060).checked_sub(start.elapsed()) {
                    std::thread::sleep(rest);
                }
                e.tick(s, Instant::now());
            }
            (p95(&mut all), p95(&mut back))
        };
        let n = s.frames;
        let seq = scrub(&mut e, &mut s, (0..n).chain((0..n).rev()).collect());
        let jumps = scrub(&mut e, &mut s, (0..n).map(|k| (k * 7919 + 3) % n).collect());
        eprintln!(
            "{name}: scrubbing at 165 Hz, frame change p50/p95/p99 (of which bringing drawings \
             back): frame by frame {:.1?} ms ({:.1?}), jumps {:.1?} ms ({:.1?})",
            seq.0, seq.1, jumps.0, jumps.1
        );
        let resident = e
            .painter
            .layers()
            .iter()
            .filter(|l| e.painter.layer_drawing(l.id) != 0)
            .count()
            + e.painter.kept_drawings().len();
        eprintln!(
            "{name}: the painter holds {resident} drawings, the store {} KB compressed",
            e.frames.store.bytes() / 1024
        );
        let saved = fixtures.join(format!("{name}.native.ppstudio"));
        let t = Instant::now();
        assert!(save_to(&mut e, &mut s, &saved), "{}", s.project.note);
        let first_save = t.elapsed().as_secs_f64() * 1e3;
        let st1 = e.files.last_save.clone().unwrap();
        // a stroke, saved again as the interface saves: the window's longest wait (Save
        // asked, then each tick until it's written)
        s.tool = crate::state::Tool::Brush;
        let _ = e.picture(&s);
        stroke(&mut e, &s, 200.0, 200.0);
        e.tick(&mut s, Instant::now());
        let t = Instant::now();
        e.apply(&mut s, &Action::File(SAVE.into()));
        let mut longest = t.elapsed().as_secs_f64() * 1e3;
        while e.files.saving.is_some() {
            let t = Instant::now();
            e.tick(&mut s, Instant::now());
            let _ = e.picture(&s);
            longest = longest.max(t.elapsed().as_secs_f64() * 1e3);
            std::thread::sleep(Duration::from_millis(1));
        }
        let st2 = e.files.last_save.clone().unwrap();
        let resave = st2.window_ms + st2.write_ms;
        eprintln!("{name}: saving, the window's longest wait {longest:.1} ms");
        // (what's open now, the stroke in it, against what the file opens as)
        let before = seen(&mut e, &s);
        let (mut e2, mut s2) = engine();
        let t = Instant::now();
        assert!(open_path(&mut e2, &mut s2, &saved));
        let reopen = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{name} reopened: {:?}", e2.files.last_open);
        let after = seen(&mut e2, &s2);
        let same_drawings = before.drawings == after.drawings;
        let worst = before
            .drawings
            .iter()
            .map(|(d, px)| drawing_diff(px, &after.drawings[d]))
            .fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
        let composite = max_diff(&before.composite, &after.composite);
        let same = before.sheet == after.sheet && before.layers == after.layers;
        Project::open(&saved).unwrap();
        if let Some(o) = &out {
            let (w, h) = (e2.painter.width(), e2.painter.height());
            let f = std::fs::File::create(o.join(format!("{name}-reopened.png"))).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&after.composite)
                .unwrap();
        }
        eprintln!(
            "{name}: open {opens:?} ms (read {:.1}, tiles {:.1} [{} tiles], painter {:.1}, \
             setup {:.1}, old one dropped {:.1}); first picture {first_picture:.1} ms; save {first_save:.0} ms \
             {st1:?}; after a stroke {resave:.0} ms {st2:?}; reopen {reopen:.0} ms; round \
             trip: drawings identical {same_drawings} (worst colour/alpha {worst:?}), composite \
             within {composite}, sheet and layers identical {same}; {} drawings, {} layers, \
             file {} KB",
            ot.read_ms,
            ot.tiles_ms,
            ot.tiles,
            ot.painter_ms,
            ot.setup_ms,
            ot.rest_ms,
            before.drawings.len(),
            before.layers.len(),
            std::fs::metadata(&saved).unwrap().len() / 1024
        );
        assert!(
            same && worst.0 <= 1 && worst.1 <= 1,
            "{name}: the round trip changed something"
        );
    }
}

/// A file with more layers than the painter holds opens with as many as it holds, and says
/// how many weren't opened.
#[test]
fn too_many_layers_are_said() {
    let d = dir("many-layers");
    let path = d.join("many.ppstudio");
    let mut p = Project::new("PencilPilot 0.1.0", "2026-11-28T10:00:00Z", 64, 64, 24);
    let n = brush::MAX_LAYERS as u32 + 2;
    p.manifest.layers = (1..=n).map(|i| layer(i, &format!("L{i}"))).collect();
    p.save(&path).unwrap();
    let (mut e, mut s) = engine();
    assert!(open_path(&mut e, &mut s, &path), "{}", s.project.note);
    assert!(
        s.project
            .note
            .starts_with(&format!("2 of its {n} layers weren't opened")),
        "{}",
        s.project.note
    );
    assert_eq!(s.project.subtitle("Cut 1"), s.project.note);
}
