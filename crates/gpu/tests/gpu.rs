//! On the machine's GPU: an image the compositor could show, cleared and read back.
//! Skips (passes) where there's no Vulkan, as on CI's machines.

#![cfg(target_os = "linux")]

use pencilpilot_gpu::{DmabufImage, Error, Gpu};

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
fn an_exportable_image_clears_and_reads_back() {
    let Some(gpu) = gpu() else { return };
    eprintln!("{:?}", gpu.info);
    let mods = gpu.modifiers();
    eprintln!(
        "{} single-plane modifiers for BGRA8: {:x?}",
        mods.len(),
        mods
    );
    assert!(!mods.is_empty());
    let img = DmabufImage::new(&gpu, 256, 128, &[]).unwrap();
    assert!(img.stride >= 256 * 4, "stride {}", img.stride);
    eprintln!(
        "modifier {:#x}, offset {}, stride {}",
        img.modifier, img.offset, img.stride
    );
    // the website's coral, #E8492F
    gpu.clear(&img, [232.0 / 255.0, 73.0 / 255.0, 47.0 / 255.0, 1.0])
        .unwrap();
    let px = gpu.read_back(&img).unwrap();
    assert_eq!(px.len(), 256 * 128 * 4);
    for p in px.chunks(4).step_by(997) {
        assert_eq!(p, [0x2f, 0x49, 0xe8, 0xff], "BGRA");
    }
    let fd = img.export(&gpu).unwrap();
    use std::os::fd::AsRawFd;
    assert!(fd.as_raw_fd() >= 0);
}

#[test]
fn a_linear_image_is_always_possible() {
    let Some(gpu) = gpu() else { return };
    // DRM_FORMAT_MOD_LINEAR: every compositor and GPU take it
    let img = DmabufImage::new(&gpu, 100, 50, &[0]).unwrap();
    assert_eq!(img.modifier, 0);
    gpu.clear(&img, [0.0, 0.0, 1.0, 1.0]).unwrap();
    let px = gpu.read_back(&img).unwrap();
    assert_eq!(&px[..4], [0xff, 0, 0, 0xff]);
}

#[test]
fn the_canvas_shows_what_was_painted() {
    use pencilpilot_gpu::Canvas;
    let Some(gpu) = gpu() else { return };
    let (w, h) = (64u32, 32u32);
    // paper, then two changes: a red square and a blue one
    let mut rgba: Vec<u8> = (0..w * h).flat_map(|_| [246, 244, 239, 255]).collect();
    let mut canvas = Canvas::new(&gpu, w, h).unwrap();
    canvas.upload_all(&rgba);
    let frame = DmabufImage::new(&gpu, 80, 40, &[]).unwrap(); // a bit bigger than the canvas
    gpu.show(
        &mut canvas,
        &frame,
        [26.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 1.0],
    )
    .unwrap();
    let put = |rgba: &mut Vec<u8>, x0: u32, y0: u32, c: [u8; 4]| {
        for y in y0..y0 + 4 {
            for x in x0..x0 + 4 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    };
    put(&mut rgba, 2, 2, [232, 73, 47, 255]);
    canvas.upload(&rgba, 2, 2, 4, 4);
    gpu.show(
        &mut canvas,
        &frame,
        [26.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 1.0],
    )
    .unwrap();
    put(&mut rgba, 40, 20, [61, 107, 255, 255]);
    canvas.upload(&rgba, 40, 20, 4, 4);
    gpu.show(
        &mut canvas,
        &frame,
        [26.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 1.0],
    )
    .unwrap();
    let px = gpu.read_back(&frame).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * 80 + x) * 4) as usize;
        [px[i + 2], px[i + 1], px[i], px[i + 3]] // BGRA → RGBA
    };
    assert_eq!(at(3, 3), [232, 73, 47, 255], "the first change");
    assert_eq!(at(41, 21), [61, 107, 255, 255], "the second change");
    assert_eq!(at(20, 10), [246, 244, 239, 255], "the paper");
    assert_eq!(at(70, 35), [26, 26, 26, 255], "around the canvas");
}
