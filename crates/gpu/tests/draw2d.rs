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
