//! The GPU's picture against the CPU's, pixel for pixel (within one level a channel): every
//! blend mode, opacities, hidden layers, groups, clipping, masks, the onion skin, going from
//! frame to frame (the kept tiles reused), painting and undo. Needs a GPU: skipped without
//! one (as on CI).

use std::time::Instant;

use pencilpilot_gpu::{Compositor, Gpu};

use super::super::{apply, picture, tick};
use super::REST;
use super::*;
use crate::state::{Action, Tool};

/// Not multiples of 64: the right and bottom tiles are cut.
const W: u32 = 300;
const H: u32 = 170;

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(g) => Some(g),
        Err(err) => {
            eprintln!("skipped: no GPU ({err})");
            None
        }
    }
}

/// The demo's sheet on a small document, every drawing filled with made-up pixels.
fn engine() -> (Engine, State) {
    let mut s = State::demo();
    s.sheet.demo = false;
    s.frame = 0;
    s.onion = false;
    let mut e = Engine::new(W, H);
    tick(&mut e, &mut s, Instant::now());
    let ds: Vec<u32> = s.sheet.tracks.iter().flat_map(|t| t.drawings()).collect();
    for d in ds {
        assert!(e.painter.put_drawing(d, &paint(d)));
    }
    picture(&mut e, &s);
    (e, s)
}

/// A drawing's made-up pixels: blobs of colour with soft and hard edges, partly
/// transparent, faint single pixels, and tiles left empty.
fn paint(seed: u32) -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    let mut r = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
    let mut next = || {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        r
    };
    for _ in 0..5 {
        let (cx, cy) = ((next() % W) as f32, (next() % H) as f32);
        let rad = 20.0 + (next() % 60) as f32;
        let c = [next() as u8, next() as u8, next() as u8];
        let peak = 60 + next() % 196;
        let soft = next() % 2 == 0;
        for y in 0..H {
            for x in 0..W {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                if d > rad {
                    continue;
                }
                let a = if soft {
                    (peak as f32 * (1.0 - d / rad)) as u32
                } else {
                    peak
                };
                let i = ((y * W + x) * 4) as usize;
                let keep = px[i + 3] as u32;
                for k in 0..3 {
                    px[i + k] = ((c[k] as u32 * a + px[i + k] as u32 * (255 - a)) / 255) as u8;
                }
                px[i + 3] = (a + keep * (255 - a) / 255).min(255) as u8;
            }
        }
    }
    for x in (0..W).step_by(3) {
        let i = (((H / 2) * W + x) * 4) as usize;
        px[i..i + 4].copy_from_slice(&[10, 200, 90, 3 + (x % 40) as u8]);
    }
    px
}

fn on_gpu(e: &mut Engine, s: &State, g: &Gpu, comp: &mut Compositor) -> bool {
    matches!(super::picture(e, s, g, comp), Pic::Gpu(_))
}

/// The picture on the GPU and on the CPU: the most a channel differs, and how many differ.
fn compare(e: &mut Engine, s: &State, g: &Gpu, comp: &mut Compositor) -> (u8, usize) {
    assert!(on_gpu(e, s, g, comp), "made on the GPU");
    let gp = comp.read_back(g).expect("read back");
    // the painter's own, as the canvas shows it (the GPU's state left as it is: the next
    // frame goes on from it); the painter first shows the frame's drawings, as it does
    // once the frame rests
    super::super::catch_up(e, s);
    e.frames.full = true;
    picture(e, s);
    let cp = if e.frames.onion_shown {
        e.frames.out.clone()
    } else {
        e.painter.rgba().to_vec()
    };
    assert_eq!(gp.len(), cp.len());
    let mut most = 0u8;
    let mut n = 0;
    for (a, b) in gp.chunks(4).zip(cp.chunks(4)) {
        for c in 0..3 {
            let d = a[c].abs_diff(b[c]);
            most = most.max(d);
            n += (d > 0) as usize;
        }
    }
    (most, n)
}

fn check(what: &str, e: &mut Engine, s: &State, g: &Gpu, comp: &mut Compositor) {
    let (most, n) = compare(e, s, g, comp);
    eprintln!("{what}: at most {most} a channel ({n} channels differ)");
    assert!(most <= 1, "{what}: the GPU's picture differs by {most}");
}

fn new_comp(g: &Gpu) -> Compositor {
    Compositor::new(g, W, H, brush::srgb_table(), 64 << 20).expect("compositor")
}

fn set(e: &mut Engine, i: usize, f: impl FnOnce(&mut brush::LayerProps)) {
    let id = e.painter.layers()[i].id;
    let mut p = e.painter.layers()[i].props.clone();
    f(&mut p);
    e.painter.set_layer_props(id, p, false);
}

#[test]
fn every_blend_mode_and_opacity_matches_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, s) = engine();
    let n = e.painter.layers().len();
    for (k, b) in brush::Blend::ALL.into_iter().enumerate() {
        for i in 0..n {
            // every other layer in this mode, the rest in the others
            let mode = if i % 2 == 0 {
                b
            } else {
                brush::Blend::ALL[(k + i) % 7]
            };
            set(&mut e, i, |p| {
                p.blend = mode;
                p.opacity = [1.0, 0.73, 0.41, 0.9, 0.12][i % 5];
            });
        }
        check(&format!("{b:?}"), &mut e, &s, &g, &mut comp);
    }
}

#[test]
fn hidden_layers_groups_clipping_and_masks_match_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, s) = engine();
    let ids: Vec<u32> = e.painter.layers().iter().map(|l| l.id).collect();
    // clipped to the layer under it; then that base hidden
    set(&mut e, 1, |p| p.clip = true);
    set(&mut e, 2, |p| {
        p.clip = true;
        p.blend = brush::Blend::Multiply;
    });
    check("clipped", &mut e, &s, &g, &mut comp);
    set(&mut e, 0, |p| p.visible = false);
    check("clipped to a hidden base", &mut e, &s, &g, &mut comp);
    set(&mut e, 0, |p| p.visible = true);
    // a mask: a gradient, with a hole
    let mut gray: Vec<u8> = (0..W * H).map(|i| ((i % W) * 255 / W) as u8).collect();
    for y in 40..90 {
        for x in 100..180 {
            gray[(y * W + x) as usize] = 0;
        }
    }
    assert!(e.painter.restore_mask(ids[3], &gray));
    check("masked", &mut e, &s, &g, &mut comp);
    // a group's opacity and eye
    let gid = e.painter.add_group(ids[2]);
    assert!(gid != 0);
    e.painter.set_group(gid, "g", true, 0.5, false);
    check("in a group at half", &mut e, &s, &g, &mut comp);
    e.painter.set_group(gid, "g", false, 0.5, false);
    check("in a hidden group", &mut e, &s, &g, &mut comp);
    // the view leaving a layer out
    e.painter.set_view_hidden(&[ids[1]]);
    check("left out by the view", &mut e, &s, &g, &mut comp);
}

#[test]
fn scrubbing_with_and_without_onion_skin_matches_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, mut s) = engine();
    for onion in [false, true] {
        s.onion = onion;
        for range in [0, 2] {
            s.strips.onion_range = range;
            for f in [0, 3, 7, 13, 6, 20, 30, 5, 47, 0] {
                assert!(apply(&mut e, &mut s, &Action::GoToFrame(f)));
                check(
                    &format!("frame {f}, onion {onion} ({range})"),
                    &mut e,
                    &s,
                    &g,
                    &mut comp,
                );
            }
        }
    }
}

#[test]
fn going_back_to_a_frame_reuses_its_tiles() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, mut s) = engine();
    let go = |e: &mut Engine, s: &mut State, comp: &mut Compositor, f| {
        assert!(apply(e, s, &Action::GoToFrame(f)));
        assert!(on_gpu(e, s, &g, comp));
        comp.read_back(&g).unwrap();
        comp.uploads
    };
    for f in 0..48 {
        go(&mut e, &mut s, &mut comp, f);
    }
    let before = go(&mut e, &mut s, &mut comp, 0);
    for f in 0..48 {
        go(&mut e, &mut s, &mut comp, f);
    }
    let after = go(&mut e, &mut s, &mut comp, 0);
    assert_eq!(after, before, "a second pass uploads nothing");
}

#[test]
fn painting_undo_and_a_new_drawing_match_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, mut s) = engine();
    s.onion = true;
    assert!(apply(&mut e, &mut s, &Action::GoToFrame(8)));
    check("before", &mut e, &s, &g, &mut comp);
    s.tool = Tool::Brush;
    s.brush_size = 12.0;
    e.down(&s, 20.0, 30.0, 0.9, (0.0, 0.0), 0);
    for i in 1..40 {
        e.moved(
            20.0 + i as f32 * 6.0,
            30.0 + i as f32 * 2.0,
            0.9,
            (0.0, 0.0),
            i * 4,
        );
        if i % 10 == 0 {
            // mid-stroke: the GPU's picture follows it
            check(&format!("mid-stroke {i}"), &mut e, &s, &g, &mut comp);
        }
    }
    e.up();
    tick(&mut e, &mut s, Instant::now());
    check("a stroke", &mut e, &s, &g, &mut comp);
    assert!(apply(&mut e, &mut s, &Action::GoToFrame(20)));
    check("another frame", &mut e, &s, &g, &mut comp);
    assert!(apply(&mut e, &mut s, &Action::GoToFrame(8)));
    check("back to the stroke", &mut e, &s, &g, &mut comp);
    e.apply(&mut s, &Action::Undo);
    tick(&mut e, &mut s, Instant::now());
    check("undone", &mut e, &s, &g, &mut comp);
    assert!(apply(&mut e, &mut s, &Action::NewDrawing));
    tick(&mut e, &mut s, Instant::now());
    check("a new drawing", &mut e, &s, &g, &mut comp);
}

#[test]
fn a_layer_style_goes_back_to_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, s) = engine();
    assert!(on_gpu(&mut e, &s, &g, &mut comp));
    set(&mut e, 0, |p| p.style.stroke.on = true);
    assert!(e.painter.layers()[0].props.style.any());
    assert!(!on_gpu(&mut e, &s, &g, &mut comp));
    assert!(!e.frames.gpu.on);
    set(&mut e, 0, |p| p.style.stroke.on = false);
    check("back on the GPU", &mut e, &s, &g, &mut comp);
}

#[test]
fn scrubbing_leaves_the_painter_until_the_frame_rests() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, mut s) = engine();
    assert!(on_gpu(&mut e, &s, &g, &mut comp));
    let first = e.painter.layer_drawing(e.painter.layers()[3].id);
    for f in [6, 12, 18] {
        assert!(apply(&mut e, &mut s, &Action::GoToFrame(f)));
        assert!(on_gpu(&mut e, &s, &g, &mut comp));
    }
    // the GPU showed frame 18; the painter still shows frame 0's drawings
    assert!(e.frames.gpu.view.is_some());
    assert_eq!(e.painter.layer_drawing(e.painter.layers()[3].id), first);
    // the frame rests: the painter catches up, and the picture is the same
    let before = comp.read_back(&g).unwrap();
    tick(&mut e, &mut s, Instant::now() + REST * 2);
    assert!(e.frames.gpu.view.is_none());
    assert!(on_gpu(&mut e, &s, &g, &mut comp));
    assert_eq!(comp.read_back(&g).unwrap(), before);
    check("rested", &mut e, &s, &g, &mut comp);
    // a stroke right after scrubbing lands on the frame shown
    assert!(apply(&mut e, &mut s, &Action::GoToFrame(30)));
    assert!(on_gpu(&mut e, &s, &g, &mut comp));
    let want = s.sheet.tracks[s.active_layer].drawing_at(30);
    s.tool = Tool::Brush;
    e.down(&s, 50.0, 50.0, 0.9, (0.0, 0.0), 0);
    e.moved(120.0, 60.0, 0.9, (0.0, 0.0), 8);
    e.up();
    let active = e.painter.active_layer();
    assert_eq!(e.painter.layer_drawing(active), want);
    tick(&mut e, &mut s, Instant::now());
    check("painted after scrubbing", &mut e, &s, &g, &mut comp);
}

#[test]
fn the_bottom_layers_kept_composited_match_the_cpu() {
    let Some(g) = gpu() else { return };
    let mut comp = new_comp(&g);
    let (mut e, mut s) = engine();
    // more layers under the animation, each one drawing held through the cut: the bottom
    // ones are kept composited
    let mut still = Vec::new();
    for k in 0..4 {
        let id = e.painter.add_layer_silent(&format!("still {k}"));
        e.painter.move_layer_silent(id, 0);
        let d = e.painter.new_drawing();
        assert!(e.painter.put_drawing(d, &paint(900 + k)));
        set(&mut e, 0, |p| p.blend = brush::Blend::ALL[k as usize % 7]);
        still.push((id, d));
    }
    tick(&mut e, &mut s, Instant::now());
    for (id, d) in &still {
        let t = s.sheet.tracks.iter_mut().find(|t| t.layer == *id).unwrap();
        t.cels = vec![super::super::Cel::new(*d, 48, super::super::Mark::None)];
    }
    tick(&mut e, &mut s, Instant::now());
    for (id, d) in &still {
        assert_eq!(e.painter.layer_drawing(*id), *d);
    }
    let mut kept = 0;
    for f in (0..48).chain([3, 40, 7]) {
        assert!(apply(&mut e, &mut s, &Action::GoToFrame(f)));
        check(&format!("frame {f}"), &mut e, &s, &g, &mut comp);
        kept = kept.max(comp.prefix_layers());
    }
    assert!(kept >= 3, "the still layers were kept composited ({kept})");
    // painting on a kept layer: it's composited again there
    let bottom = e.painter.layers()[0].id;
    e.painter.set_active_layer(bottom);
    s.tool = Tool::Brush;
    e.down(&s, 30.0, 100.0, 0.9, (0.0, 0.0), 0);
    e.moved(200.0, 120.0, 0.9, (0.0, 0.0), 8);
    e.up();
    check("painted under the animation", &mut e, &s, &g, &mut comp);
}
