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
