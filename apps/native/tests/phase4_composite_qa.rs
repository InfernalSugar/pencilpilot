//! QA for the picture the canvas shows (Phase 4, GPU layers): a project with every blend
//! mode at several opacities, clipping (onto a visible, a hidden and a partly transparent
//! base), a hidden layer, a layer at opacity 0, a layer mask, and onion skin, opened through
//! the File menu as the app opens it.
//!
//! - `reference_matches_cpu`: the engine's picture (`Engine::picture`, the CPU compositor)
//!   against an independent compositor written here from the documented rules (sRGB-space
//!   blending over white paper, `dst += (B(dst, src) - dst) * alpha * opacity * mask`,
//!   clipped layers through the alpha of the nearest unclipped layer below), every pixel.
//! - `gpu_matches_cpu`: the GPU compositor's picture against the CPU's, ±1 a channel, for
//!   every frame and with the onion skin on and off (see the test for how it is read back).
//!
//! Ignored by a plain `cargo test` (the GPU one needs a GPU); run with
//! `cargo test --release -p pencilpilot-native --test phase4_composite_qa -- --include-ignored --test-threads 1`
//! inside `env -u DISPLAY -u WAYLAND_DISPLAY dbus-run-session -- …`.
//! `PP_QA_OUT` = a folder for the pictures (PNG) and the measurements (JSON).

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::{Engine, files};
use pencilpilot_native::state::{Action, State};

/// Not a multiple of the 64 px tile: the edges' tiles are partial.
const W: u32 = 333;
const H: u32 = 245;

const BLENDS: [&str; 7] = [
    "normal", "multiply", "screen", "overlay", "darken", "lighten", "add",
];
const OPACITIES: [f32; 7] = [1.0, 0.73, 0.5, 0.31, 0.9, 0.62, 1.0];

fn out_dir() -> PathBuf {
    let d = std::env::var_os("PP_QA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("pp-p4-qa-{}", std::process::id())));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let d = out_dir();
        // (the tests run on one thread: --test-threads 1)
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
            std::env::set_var("XDG_STATE_HOME", d.join("state"));
            std::env::set_var("PENCILPILOT_NO_SYSTEM_WINDOWS", "1");
            std::env::set_var("PENCILPILOT_NO_SOUND", "1");
        }
    });
}

fn save_png(path: &Path, rgba: &[u8], w: u32, h: u32) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

// ------------------------------------------------------------------------------------------
// the project

/// One layer of the test project: its file settings and its straight RGBA per frame.
struct L {
    name: String,
    blend: &'static str,
    opacity: f32,
    visible: bool,
    clip: bool,
    /// one picture a frame (None: the layer is empty there)
    frames: Vec<Option<Vec<u8>>>,
}

/// Straight RGBA, canvas-sized, from a function of (x, y) → Option<rgba>.
fn pic(f: impl Fn(u32, u32) -> Option<[u8; 4]>) -> Vec<u8> {
    let mut v = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            if let Some(p) = f(x, y) {
                let i = ((y * W + x) * 4) as usize;
                v[i..i + 4].copy_from_slice(&p);
            }
        }
    }
    v
}

/// A cheap deterministic hash (for colours that vary pixel to pixel).
fn hash(x: u32, y: u32, k: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ k.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

const FRAMES: usize = 3;
const BAND: u32 = H / 10;

fn layers() -> Vec<L> {
    let mut v = Vec::new();
    // the paper: opaque, every colour somewhere (bottom)
    let paper = pic(|x, y| {
        Some([
            (x * 255 / (W - 1)) as u8,
            (y * 255 / (H - 1)) as u8,
            ((x + y) * 7 % 256) as u8,
            255,
        ])
    });
    v.push(L {
        name: "Paper".into(),
        blend: "normal",
        opacity: 1.0,
        visible: true,
        clip: false,
        frames: vec![Some(paper); FRAMES],
    });
    // each blend mode in a band of its own, over the whole width; alpha rises left to right,
    // colours vary pixel to pixel; a second, half-strength wash over every band
    for (i, (&b, &o)) in BLENDS.iter().zip(OPACITIES.iter()).enumerate() {
        let k = i as u32;
        let p = pic(move |x, y| {
            let (y0, y1) = (k * BAND, (k + 1) * BAND + BAND / 2);
            (y >= y0 && y < y1).then(|| {
                let h = hash(x, y, k);
                [
                    h as u8,
                    (h >> 8) as u8,
                    (h >> 16) as u8,
                    (x * 255 / (W - 1)) as u8,
                ]
            })
        });
        v.push(L {
            name: format!("Blend {b}"),
            blend: b,
            opacity: o,
            visible: true,
            clip: false,
            frames: vec![Some(p); FRAMES],
        });
    }
    // clipping: a soft disc base, two clipped layers on it (multiply, normal)
    let base_y = 7 * BAND + BAND;
    let disc = pic(move |x, y| {
        let (dx, dy) = (x as f32 - W as f32 * 0.3, y as f32 - base_y as f32);
        let d = (dx * dx + dy * dy).sqrt();
        (d < 45.0).then(|| {
            [
                30,
                160,
                220,
                (255.0 * (1.0 - d / 45.0)).min(255.0) as u8 | 1,
            ]
        })
    });
    v.push(L {
        name: "Clip base".into(),
        blend: "normal",
        opacity: 0.85,
        visible: true,
        clip: false,
        frames: vec![Some(disc); FRAMES],
    });
    let stripes = pic(move |x, y| {
        (y >= base_y - 50 && y < base_y + 50).then_some({
            if (x / 6) % 2 == 0 {
                [240, 40, 40, 255]
            } else {
                [250, 230, 30, 160]
            }
        })
    });
    v.push(L {
        name: "Clipped multiply".into(),
        blend: "multiply",
        opacity: 0.8,
        visible: true,
        clip: true,
        frames: vec![Some(stripes.clone()); FRAMES],
    });
    v.push(L {
        name: "Clipped normal".into(),
        blend: "normal",
        opacity: 0.5,
        visible: true,
        clip: true,
        frames: vec![
            Some(pic(move |x, y| {
                (x > W / 4 && y > base_y - 60 && y < base_y + 60).then_some([10, 200, 60, 200])
            }));
            FRAMES
        ],
    });
    // a hidden base and a layer clipped to it: neither shows
    v.push(L {
        name: "Hidden base".into(),
        blend: "normal",
        opacity: 1.0,
        visible: false,
        clip: false,
        frames: vec![Some(pic(|x, _| (x > W / 2).then_some([0, 0, 0, 255]))); FRAMES],
    });
    v.push(L {
        name: "Clipped to hidden".into(),
        blend: "normal",
        opacity: 1.0,
        visible: true,
        clip: true,
        frames: vec![Some(pic(|_, _| Some([255, 0, 255, 255]))); FRAMES],
    });
    // opacity 0: doesn't show
    v.push(L {
        name: "Opacity zero".into(),
        blend: "normal",
        opacity: 0.0,
        visible: true,
        clip: false,
        frames: vec![Some(pic(|_, _| Some([0, 255, 0, 255]))); FRAMES],
    });
    // the masked layer (its mask is given after opening: the file's masks aren't read)
    v.push(L {
        name: "Masked".into(),
        blend: "normal",
        opacity: 1.0,
        visible: true,
        clip: false,
        frames: vec![
            Some(pic(|x, y| (y > H - BAND * 2).then_some([
                20,
                20,
                30,
                (x * 3 % 256) as u8 | 128
            ])));
            FRAMES
        ],
    });
    // the animated layer (top): a different drawing each frame, for the onion skin
    let anim: Vec<Option<Vec<u8>>> = (0..FRAMES as u32)
        .map(|f| {
            Some(pic(move |x, y| {
                let cx = 60.0 + f as f32 * 90.0;
                let (dx, dy) = (x as f32 - cx, y as f32 - H as f32 * 0.55);
                (dx.abs() < 30.0 && dy.abs() < 22.0).then_some([40, 30, 25, 230])
            }))
        })
        .collect();
    v.push(L {
        name: "Anim".into(),
        blend: "normal",
        opacity: 1.0,
        visible: true,
        clip: false,
        frames: anim,
    });
    v
}

/// The mask given to the "Masked" layer: a grey ramp top to bottom, with a black hole.
fn mask() -> Vec<u8> {
    let mut m = vec![0u8; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let hole =
                (x as i32 - 250).abs() < 20 && (y as i32 - (H as i32 - BAND as i32)).abs() < 12;
            m[(y * W + x) as usize] = if hole { 0 } else { (x * 255 / (W - 1)) as u8 };
        }
    }
    m
}

fn write_project(path: &Path, ls: &[L]) {
    let mut p = ppstudio::Project::new("qa", "2026-11-28T00:00:00Z", W, H, 24);
    let mut tracks = std::collections::BTreeMap::new();
    for (i, l) in ls.iter().enumerate() {
        let id = i as u32 + 1;
        p.manifest.layers.push(ppstudio::Layer {
            id,
            name: l.name.clone(),
            visible: l.visible,
            opacity: l.opacity,
            blend: l.blend.into(),
            locked: false,
            alpha_lock: false,
            clip: l.clip,
            imported: false,
            group: 0,
            adjust: None,
            style: None,
            text: None,
            lines: false,
        });
        let mut track = Vec::new();
        for (f, px) in l.frames.iter().enumerate() {
            // the same picture on every frame: one drawing, held
            let same = f > 0 && l.frames[f - 1] == *px;
            if same && let Some(e) = track.last_mut() {
                let e: &mut ppstudio::Exposure = e;
                e.length += 1;
                continue;
            }
            let drawing = px.as_ref().map(|px| {
                let n = format!("D{id}-{f}");
                p.put_drawing(&n, px, None).unwrap();
                n
            });
            track.push(ppstudio::Exposure { drawing, length: 1 });
        }
        tracks.insert(id, track);
    }
    p.manifest.timeline.scenes = vec![ppstudio::Scene {
        name: "Scene 1".into(),
        cuts: vec![ppstudio::Cut {
            name: "Cut 1".into(),
            tracks,
            ..Default::default()
        }],
        ..Default::default()
    }];
    p.manifest.timeline.frames = ppstudio::Timeline::flatten(&p.manifest.timeline.scenes, &[]);
    p.save(path).unwrap();
}

// ------------------------------------------------------------------------------------------
// the independent compositor

fn srgb(v: u8) -> f64 {
    v as f64 / 255.0
}

fn mix(blend: &str, cb: f64, cs: f64) -> f64 {
    match blend {
        "normal" => cs,
        "multiply" => cb * cs,
        "screen" => cb + cs - cb * cs,
        "overlay" => {
            if cb <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        "darken" => cb.min(cs),
        "lighten" => cb.max(cs),
        "add" => (cb + cs).min(1.0),
        _ => unreachable!(),
    }
}

/// Frame `f`, composited bottom to top over white paper, sRGB8 (alpha 255).
fn reference(ls: &[L], f: usize, masks: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![255u8; (W * H * 4) as usize];
    for p in 0..(W * H) as usize {
        let mut dst = [1.0f64; 3];
        let mut base = 0.0f64;
        for (li, l) in ls.iter().enumerate() {
            let Some(px) = &l.frames[f] else {
                if !l.clip {
                    base = 0.0;
                }
                continue;
            };
            let q = &px[p * 4..p * 4 + 4];
            let m = masks
                .iter()
                .find(|(i, _)| *i == li)
                .map_or(1.0, |(_, m)| m[p] as f64 / 255.0);
            let mut a = srgb(q[3]) * l.opacity as f64 * m;
            if l.clip {
                a *= base;
            } else {
                base = if l.visible { a } else { 0.0 };
            }
            if !l.visible || a <= 0.0 {
                continue;
            }
            for c in 0..3 {
                let b = mix(l.blend, dst[c], srgb(q[c]));
                dst[c] += (b - dst[c]) * a;
            }
        }
        for c in 0..3 {
            out[p * 4 + c] = (dst[c].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    out
}

/// Per channel: the largest difference, how many channels differ by more than `tol`, and
/// where the worst one is.
fn diff(a: &[u8], b: &[u8], tol: u8) -> (u8, usize, (u32, u32)) {
    let (mut worst, mut n, mut at) = (0u8, 0usize, (0, 0));
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if i % 4 == 3 {
            continue;
        }
        let d = x.abs_diff(*y);
        if d > tol {
            n += 1;
        }
        if d > worst {
            worst = d;
            let p = (i / 4) as u32;
            at = (p % W, p / W);
        }
    }
    (worst, n, at)
}

// ------------------------------------------------------------------------------------------
// the engine

fn settle(e: &mut Engine, s: &mut State) {
    let t = Instant::now();
    for _ in 0..5 {
        e.tick(s, Instant::now());
        let _ = e.picture(s);
        std::thread::sleep(Duration::from_millis(5));
    }
    while t.elapsed() < Duration::from_secs(10) && (s.project.saving || s.project.path.is_none()) {
        e.tick(s, Instant::now());
        let _ = e.picture(s);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The test project opened through File ▸ Open, as the app opens it; the mask given to
/// the "Masked" layer through the painter.
/// Layer masks given after opening: (layer index, bottom first; grey per pixel).
type Masks = Vec<(usize, Vec<u8>)>;

fn opened() -> (Engine, State, Vec<L>, Masks) {
    init();
    let ls = layers();
    let path = out_dir().join("composite.ppstudio");
    write_project(&path, &ls);
    let mut s = State::demo();
    s.onion = false;
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    settle(&mut e, &mut s);
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", &path) };
    assert!(e.apply(&mut s, &Action::File(files::OPEN.into())));
    settle(&mut e, &mut s);
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", "") };
    assert_eq!(
        (e.painter.width(), e.painter.height()),
        (W, H),
        "{}",
        s.project.note
    );
    assert_eq!(s.layers.len(), ls.len());
    // (the interface's layers are top first)
    let masked = ls.iter().position(|l| l.name == "Masked").unwrap();
    let id = s.sheet.tracks[ls.len() - 1 - masked].layer;
    assert_eq!(s.layers[ls.len() - 1 - masked].name, "Masked");
    let m = mask();
    let masks = if e.painter.restore_mask(id, &m) {
        vec![(masked, m)]
    } else {
        eprintln!("the painter took no mask on the Masked layer: masks left out");
        Vec::new()
    };
    s.frame = 0;
    e.tick(&mut s, Instant::now());
    (e, s, ls, masks)
}

fn cpu_picture(e: &mut Engine, s: &mut State, f: u32) -> Vec<u8> {
    assert!(e.apply(s, &Action::GoToFrame(f)));
    e.tick(s, Instant::now());
    shown(e, s)
}

/// The picture the canvas shows now (onion skin included), a copy: the picture is the whole
/// document (the rectangle is only what changed); None: nothing changed, and then (with no
/// onion skin) it's the painter's.
fn shown(e: &mut Engine, s: &State) -> Vec<u8> {
    match e.picture(s) {
        Some((p, _)) => p.to_vec(),
        None => e.painter.rgba().to_vec(),
    }
}

#[test]
#[ignore]
fn reference_matches_cpu() {
    let (mut e, mut s, ls, masks) = opened();
    let d = out_dir();
    let mut report = Vec::new();
    let mut worst_all = 0;
    for f in 0..FRAMES {
        let cpu = cpu_picture(&mut e, &mut s, f as u32);
        let want = reference(&ls, f, &masks);
        let (worst, over, at) = diff(&cpu, &want, 1);
        save_png(&d.join(format!("cpu-{f}.png")), &cpu, W, H);
        save_png(&d.join(format!("reference-{f}.png")), &want, W, H);
        // per band (which blend mode, clipping, the mask) where they differ most
        let mut bands = Vec::new();
        for b in 0..10 {
            let (y0, y1) = (b * BAND, ((b + 1) * BAND).min(H));
            let r = (y0 * W * 4) as usize..(y1 * W * 4) as usize;
            bands.push(diff(&cpu[r.clone()], &want[r], 1).0);
        }
        report.push(format!(
            "{{\"frame\": {f}, \"maxDiff\": {worst}, \"channelsOver1\": {over}, \"worstAt\": [{}, {}], \"maxDiffPerBand\": {bands:?}}}",
            at.0, at.1
        ));
        worst_all = worst_all.max(worst);
    }
    let json = format!(
        "{{\"masks\": {}, \"frames\": [{}]}}",
        !masks.is_empty(),
        report.join(", ")
    );
    std::fs::write(d.join("reference_vs_cpu.json"), &json).unwrap();
    eprintln!("reference vs cpu: {json}");
    assert!(
        worst_all <= 1,
        "the CPU compositor differs from the reference by {worst_all}: {json}"
    );
}
