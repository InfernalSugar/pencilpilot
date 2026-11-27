//! The interface renderer on the GPU: shapes, borders, shadows and atlas pictures land where
//! and how they should. Skips without Vulkan. `PP_SAVE` = a folder to save the picture in.

#![cfg(target_os = "linux")]

use pencilpilot_gpu::{DmabufImage, Error, Gpu, Quad, Renderer2d};

const PAPER: [f32; 4] = [246.0 / 255.0, 244.0 / 255.0, 239.0 / 255.0, 1.0];
const INK: [f32; 4] = [22.0 / 255.0, 22.0 / 255.0, 26.0 / 255.0, 1.0];
const CORAL: [f32; 4] = [232.0 / 255.0, 73.0 / 255.0, 47.0 / 255.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(g) => Some(g),
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn shapes_land_where_they_should() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (400u32, 240u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    // a 2×2 picture in the atlas: coral, ink / white, transparent
    r.atlas_put(
        0,
        0,
        2,
        2,
        &[
            232, 73, 47, 255, 22, 22, 26, 255, 255, 255, 255, 255, 0, 0, 0, 0,
        ],
    );
    let quads = [
        // a card on a soft shadow
        Quad::rect(28.0, 32.0, 200.0, 160.0, [0.0, 0.0, 0.0, 0.25])
            .radius(28.0)
            .shadow(10.0),
        Quad::rect(20.0, 20.0, 200.0, 160.0, WHITE).radius(28.0),
        // a coral button with an ink border
        Quad::rect(250.0, 40.0, 120.0, 44.0, CORAL)
            .radius(22.0)
            .border(2.0, INK),
        // the picture, scaled up (top-left texel only, so no filtering between texels)
        Quad::image(
            260.0,
            120.0,
            40.0,
            40.0,
            [0.0, 0.0, 0.5 / 2048.0, 0.5 / 2048.0],
            1.0,
        ),
        // clipped to its left half
        Quad::rect(320.0, 120.0, 60.0, 40.0, INK).clip([320.0, 0.0, 350.0, 240.0]),
    ];
    r.draw(&gpu, &frame, PAPER, &quads).unwrap();
    let px = gpu.read_back(&frame).unwrap();
    if let Ok(dir) = std::env::var("PP_SAVE") {
        let rgb: Vec<u8> = px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        ppm.extend(rgb);
        std::fs::write(format!("{dir}/draw2d.ppm"), ppm).unwrap();
    }
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i + 2], px[i + 1], px[i]]
    };
    let near = |a: [u8; 3], b: [u8; 3]| {
        a.iter()
            .zip(b)
            .all(|(x, y)| (*x as i32 - y as i32).abs() <= 2)
    };
    assert!(near(at(5, 5), [246, 244, 239]), "paper {:?}", at(5, 5));
    assert!(
        near(at(120, 100), [255, 255, 255]),
        "the card {:?}",
        at(120, 100)
    );
    assert!(
        near(at(22, 22), [246, 244, 239]),
        "outside the card's rounded corner {:?}",
        at(22, 22)
    );
    let shadow = at(120, 190);
    assert!(
        shadow[0] < 240 && shadow[0] > 150,
        "the shadow below the card {shadow:?}"
    );
    assert!(
        near(at(310, 62), [232, 73, 47]),
        "the button {:?}",
        at(310, 62)
    );
    assert!(
        near(at(310, 41), [22, 22, 26]),
        "the button's border {:?}",
        at(310, 41)
    );
    assert!(
        near(at(280, 140), [232, 73, 47]),
        "the picture {:?}",
        at(280, 140)
    );
    assert!(
        near(at(330, 140), [22, 22, 26]),
        "inside the clip {:?}",
        at(330, 140)
    );
    assert!(
        near(at(365, 140), [246, 244, 239]),
        "clipped away {:?}",
        at(365, 140)
    );
}

#[test]
fn the_canvas_sits_between_the_layers() {
    use pencilpilot_gpu::Canvas;
    let Some(gpu) = gpu() else { return };
    let (w, h) = (300u32, 200u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    // a 100×60 canvas: ink, with a coral square from (10, 10)
    let (cw, ch) = (100u32, 60u32);
    let mut px: Vec<u8> = (0..cw * ch).flat_map(|_| [22, 22, 26, 255]).collect();
    for y in 10..20 {
        for x in 10..20 {
            let i = ((y * cw + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[232, 73, 47, 255]);
        }
    }
    let mut canvas = Canvas::new(&gpu, cw, ch).unwrap();
    canvas.upload_all(&px);
    let quads = [
        // below: a white card
        Quad::rect(20.0, 20.0, 200.0, 120.0, WHITE).radius(24.0),
        // the canvas at (40, 40), pixel for pixel; the card shows outside it
        Quad::canvas(
            [40.0, 40.0, 100.0, 60.0],
            [1.0, 0.0, 0.0, 1.0, -40.0, -40.0],
            [cw as f32, ch as f32],
            [0.0; 4],
            true,
        ),
        // above: a pill over the canvas's bottom-right
        Quad::rect(120.0, 90.0, 60.0, 24.0, CORAL).radius(12.0),
    ];
    r.draw_layered(&gpu, &frame, PAPER, &quads, Some(&mut canvas))
        .unwrap();
    let out = gpu.read_back(&frame).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [out[i + 2], out[i + 1], out[i]]
    };
    assert_eq!(at(5, 5), [246, 244, 239], "paper");
    assert_eq!(at(30, 100), [255, 255, 255], "the card, below");
    assert_eq!(at(45, 45), [22, 22, 26], "the canvas, over the card");
    assert_eq!(at(55, 55), [232, 73, 47], "the canvas's own coral square");
    assert_eq!(at(150, 102), [232, 73, 47], "the pill, above the canvas");
    assert_eq!(
        at(130, 70),
        [22, 22, 26],
        "the canvas where nothing is above it"
    );
}

#[test]
fn gradients_run_the_way_they_are_told() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (200u32, 100u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    let clear = [0.0, 0.0, 0.0, 0.0];
    let quads = [
        // a colour field: white to coral left to right, then clear to black top to bottom
        Quad::gradient(0.0, 0.0, 200.0, 100.0, WHITE, CORAL, (1.0, 0.0)),
        Quad::gradient(
            0.0,
            0.0,
            200.0,
            100.0,
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            (0.0, 1.0),
        ),
    ];
    r.draw(&gpu, &frame, PAPER, &quads).unwrap();
    let _ = clear;
    let px = gpu.read_back(&frame).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i + 2] as i32, px[i + 1] as i32, px[i] as i32]
    };
    let near =
        |a: [i32; 3], b: [i32; 3], tol: i32| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol);
    assert!(
        near(at(0, 0), [255, 255, 255], 4),
        "top-left: white {:?}",
        at(0, 0)
    );
    assert!(
        near(at(199, 0), [232, 73, 47], 4),
        "top-right: coral {:?}",
        at(199, 0)
    );
    let mid = at(100, 0);
    assert!(near(mid, [244, 164, 151], 6), "top-middle: halfway {mid:?}");
    assert!(
        near(at(100, 99), [0, 0, 0], 6),
        "bottom: black {:?}",
        at(100, 99)
    );
}

#[test]
fn the_canvas_shows_through_a_view() {
    use pencilpilot_gpu::Canvas;
    let Some(gpu) = gpu() else { return };
    let (w, h) = (200u32, 100u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    // a 20×10 canvas: its left half coral, its right half ink
    let (cw, ch) = (20u32, 10u32);
    let px: Vec<u8> = (0..cw * ch)
        .flat_map(|i| {
            if i % cw < 10 {
                [232, 73, 47, 255]
            } else {
                [22, 22, 26, 255]
            }
        })
        .collect();
    let mut canvas = Canvas::new(&gpu, cw, ch).unwrap();
    canvas.upload_all(&px);
    let grey = [0.5, 0.5, 0.5, 1.0];
    let quads = [
        // zoomed ×4 at (0, 0): canvas = frame / 4; the rest of the rectangle is outside it
        Quad::canvas(
            [0.0, 0.0, 100.0, 100.0],
            [0.25, 0.0, 0.0, 0.25, 0.0, 0.0],
            [20.0, 10.0],
            grey,
            true,
        ),
        // mirrored ×4 at (100, 0): canvas.x = 20 − (x − 100) / 4
        Quad::canvas(
            [100.0, 0.0, 80.0, 40.0],
            [-0.25, 0.0, 0.0, 0.25, 45.0, 0.0],
            [20.0, 10.0],
            grey,
            true,
        ),
    ];
    r.draw_layered(&gpu, &frame, PAPER, &quads, Some(&mut canvas))
        .unwrap();
    let out = gpu.read_back(&frame).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [out[i + 2], out[i + 1], out[i]]
    };
    assert_eq!(at(10, 10), [232, 73, 47], "zoomed: the left half");
    assert_eq!(at(60, 10), [22, 22, 26], "zoomed: the right half");
    assert_eq!(at(10, 60), [128, 128, 128], "below the canvas: outside");
    assert_eq!(
        at(110, 10),
        [22, 22, 26],
        "mirrored: the right half shows on the left"
    );
    assert_eq!(
        at(170, 10),
        [232, 73, 47],
        "mirrored: the left half shows on the right"
    );
}

#[test]
fn a_second_picture_moves_over_the_canvas() {
    use pencilpilot_gpu::Canvas;
    let Some(gpu) = gpu() else { return };
    let (w, h) = (100u32, 50u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    // the canvas: ink all over
    let mut canvas = Canvas::new(&gpu, w, h).unwrap();
    canvas.upload_all(
        &(0..w * h)
            .flat_map(|_| [22, 22, 26, 255])
            .collect::<Vec<u8>>(),
    );
    // the second picture: 10×10, coral, its right half see-through
    let px: Vec<u8> = (0..100u32)
        .flat_map(|i| {
            if i % 10 < 5 {
                [232, 73, 47, 255]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect();
    let mut second = Canvas::new(&gpu, 10, 10).unwrap();
    second.upload_all(&px);
    let quads = [
        Quad::canvas(
            [0.0, 0.0, 100.0, 50.0],
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [100.0, 50.0],
            [0.0; 4],
            true,
        ),
        // moved to (60, 20)
        Quad::canvas(
            [0.0, 0.0, 100.0, 50.0],
            [1.0, 0.0, 0.0, 1.0, -60.0, -20.0],
            [10.0, 10.0],
            [0.0; 4],
            true,
        )
        .second(),
    ];
    r.draw_canvases(
        &gpu,
        &frame,
        PAPER,
        &quads,
        Some(&mut canvas),
        Some(&mut second),
    )
    .unwrap();
    let out = gpu.read_back(&frame).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [out[i + 2], out[i + 1], out[i]]
    };
    assert_eq!(at(62, 25), [232, 73, 47], "the moved picture");
    assert_eq!(
        at(67, 25),
        [22, 22, 26],
        "its see-through half shows the canvas"
    );
    assert_eq!(at(30, 25), [22, 22, 26], "the canvas elsewhere");
}
