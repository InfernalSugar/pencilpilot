//! Text on the GPU: the design's fonts, weights, right-to-left Arabic through fallback, and
//! colour emoji. Skips without Vulkan. `PP_SAVE` = a folder to save the picture in.

#![cfg(target_os = "linux")]

use pencilpilot_gpu::{DmabufImage, Error, Gpu, Quad, Renderer2d};
use pencilpilot_ui::text::{Style, Text};

const PAPER: [f32; 4] = [246.0 / 255.0, 244.0 / 255.0, 239.0 / 255.0, 1.0];
const INK: [f32; 4] = [22.0 / 255.0, 22.0 / 255.0, 26.0 / 255.0, 1.0];
const CORAL: [f32; 4] = [232.0 / 255.0, 73.0 / 255.0, 47.0 / 255.0, 1.0];

#[test]
fn text_in_the_designs_fonts_and_others() {
    let gpu = match Gpu::new() {
        Ok(g) => g,
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let (w, h) = (640u32, 360u32);
    let frame = DmabufImage::new(&gpu, w, h, &[]).unwrap();
    let mut r = Renderer2d::new(&gpu).unwrap();
    let mut text = Text::new();
    let title = text.lay_out(&mut r, "Animate", &Style::display(44.0, INK), None);
    let regular = text.lay_out(&mut r, "Cut 1 · 24 frames", &Style::body(18.0, INK), None);
    let bold = text.lay_out(
        &mut r,
        "Cut 1 · 24 frames",
        &Style::body(18.0, INK).weight(700),
        None,
    );
    let arabic = text.lay_out(
        &mut r,
        "مرحبا بالرسوم المتحركة",
        &Style::body(22.0, INK),
        None,
    );
    let emoji = text.lay_out(&mut r, "Colour 🎨", &Style::body(22.0, INK), None);
    let wrapped = text.lay_out(
        &mut r,
        "A long note that wraps onto a second line inside its card",
        &Style::body(16.0, INK),
        Some(260.0),
    );
    eprintln!(
        "title {}×{}, regular {:.1} wide, bold {:.1}, arabic {} glyphs, wrapped {:.0} tall",
        title.width,
        title.height,
        regular.width,
        bold.width,
        arabic.quads.len(),
        wrapped.height
    );
    assert!(title.width > 100.0 && !title.quads.is_empty());
    assert!(
        bold.width > regular.width + 1.0,
        "bold is wider: the weight reached the variable font"
    );
    assert!(
        arabic.quads.len() >= 10,
        "Arabic shaped through a fallback font"
    );
    assert!(wrapped.height > 30.0, "wrapped onto two lines");

    let mut quads = vec![
        Quad::rect(24.0, 28.0, 592.0, 308.0, [0.0, 0.0, 0.0, 0.12])
            .radius(28.0)
            .shadow(12.0),
        Quad::rect(20.0, 20.0, 592.0, 308.0, [1.0; 4]).radius(28.0),
        Quad::rect(452.0, 44.0, 132.0, 44.0, CORAL).radius(22.0),
    ];
    quads.extend(title.at(48.0, 40.0));
    quads.extend(
        text.lay_out(
            &mut r,
            "New drawing",
            &Style::body(16.0, [1.0; 4]).weight(600),
            None,
        )
        .at(470.0, 55.0),
    );
    quads.extend(regular.at(48.0, 108.0));
    quads.extend(bold.at(48.0, 136.0));
    quads.extend(arabic.at(48.0, 172.0));
    quads.extend(emoji.at(48.0, 214.0));
    quads.extend(wrapped.at(48.0, 252.0));
    r.draw(&gpu, &frame, PAPER, &quads).unwrap();
    let px = gpu.read_back(&frame).unwrap();
    if let Ok(dir) = std::env::var("PP_SAVE") {
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        ppm.extend(px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
        std::fs::write(format!("{dir}/text.ppm"), ppm).unwrap();
    }
    // the emoji brings colour that isn't the ink, paper or white
    let colourful = (214..240)
        .flat_map(|y| (48..200).map(move |x| (x, y)))
        .any(|(x, y)| {
            let i = ((y * w + x) * 4) as usize;
            let (b, g, rr) = (px[i] as i32, px[i + 1] as i32, px[i + 2] as i32);
            (rr - g).abs() > 60 || (g - b).abs() > 60
        });
    assert!(colourful, "the emoji came out in colour");
}
