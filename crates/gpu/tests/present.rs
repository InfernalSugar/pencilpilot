//! The whole way to the screen: the GPU renders a frame into a dmabuf, the app hands it to
//! the compositor (wayland-client), and the compositor says when it was shown. Needs a
//! display; use a headless one, so no window opens on any screen:
//!
//!   mutter --headless --no-x11 --wayland-display pp-bench-wl --virtual-monitor 2560x1440@165 &
//!   PP_WL_LIVE=pp-bench-wl cargo test -p pencilpilot-gpu --test present -- --nocapture

#![cfg(target_os = "linux")]

use std::os::fd::AsFd;

use pencilpilot_gpu::{DRM_FORMAT_XRGB8888, DmabufImage, Gpu};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

#[derive(Default)]
struct State {
    modifiers: Vec<u64>,
    configured: bool,
    presented: Option<u32>,
    discarded: bool,
    refused: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore xdg_toplevel::XdgToplevel);
delegate_noop!(State: ignore wp_presentation::WpPresentation);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        e: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = e {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(
        s: &mut Self,
        xs: &xdg_surface::XdgSurface,
        e: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = e {
            xs.ack_configure(serial);
            s.configured = true;
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        e: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_dmabuf_v1::Event::Modifier {
            format,
            modifier_hi,
            modifier_lo,
        } = e
            && format == DRM_FORMAT_XRGB8888
        {
            s.modifiers
                .push(((modifier_hi as u64) << 32) | modifier_lo as u64);
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        e: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_buffer_params_v1::Event::Failed = e {
            s.refused = true;
        }
    }
}

impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, ()> for State {
    fn event(
        s: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        e: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wp_presentation_feedback::Event::Presented { refresh, .. } => {
                s.presented = Some(refresh)
            }
            wp_presentation_feedback::Event::Discarded => s.discarded = true,
            _ => {}
        }
    }
}

#[test]
fn a_gpu_frame_reaches_the_compositor() {
    let Ok(display_name) = std::env::var("PP_WL_LIVE") else {
        eprintln!("skipped: set PP_WL_LIVE to a Wayland display (a headless one)");
        return;
    };
    // SAFETY: before anything else reads the environment
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display_name) };
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut queue) = registry_queue_init::<State>(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).unwrap();
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
    // version 3: the modifiers come as events (later versions send a feedback table)
    let dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 = globals.bind(&qh, 3..=3, ()).unwrap();
    let presentation: wp_presentation::WpPresentation = globals.bind(&qh, 1..=2, ()).unwrap();
    let mut state = State::default();
    queue.roundtrip(&mut state).unwrap();
    eprintln!(
        "the compositor takes XRGB8888 with {} modifiers",
        state.modifiers.len()
    );

    let gpu = Gpu::new().expect("a GPU");
    let surface = compositor.create_surface(&qh, ());
    let xs = wm.get_xdg_surface(&surface, &qh, ());
    let top = xs.get_toplevel(&qh, ());
    top.set_title("PencilPilot GPU test".into());
    surface.commit();
    while !state.configured {
        queue.blocking_dispatch(&mut state).unwrap();
    }

    let (w, h) = (800, 500);
    let img = DmabufImage::new(&gpu, w, h, &state.modifiers).unwrap();
    gpu.clear(&img, [232.0 / 255.0, 73.0 / 255.0, 47.0 / 255.0, 1.0])
        .unwrap();
    eprintln!(
        "{}: modifier {:#x}, stride {}",
        gpu.info.name, img.modifier, img.stride
    );
    let fd = img.export(&gpu).unwrap();
    let params = dmabuf.create_params(&qh, ());
    params.add(
        fd.as_fd(),
        0,
        img.offset,
        img.stride,
        (img.modifier >> 32) as u32,
        img.modifier as u32,
    );
    let buffer = params.create_immed(
        w as i32,
        h as i32,
        DRM_FORMAT_XRGB8888,
        zwp_linux_buffer_params_v1::Flags::empty(),
        &qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w as i32, h as i32);
    let _feedback = presentation.feedback(&surface, &qh, ());
    surface.commit();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while state.presented.is_none() {
        assert!(std::time::Instant::now() < deadline, "not presented in 5 s");
        assert!(!state.refused, "the compositor refused the dmabuf");
        assert!(!state.discarded, "the frame was discarded");
        queue.blocking_dispatch(&mut state).unwrap();
    }
    eprintln!(
        "the GPU's frame was presented (refresh {} ns)",
        state.presented.unwrap()
    );
    top.destroy();
    xs.destroy();
    surface.destroy();
    queue.roundtrip(&mut state).unwrap();
}
