//! The native app's window on Wayland.
//!
//! - **Size and scale:** the compositor's size, rendered at the output's real pixel density
//!   (fractional scales through `wp_fractional_scale_v1` + `wp_viewporter`).
//! - **Frames:** GPU images (dmabufs) in a ring of three, each reused only once the
//!   compositor has let it go.
//! - **Pacing:** a frame is drawn when the compositor asks (frame callbacks), so the window
//!   runs at the monitor's rate, whatever it is.
//! - **Presentation feedback** says when each frame reached the screen and the refresh of
//!   the output it reached, so the window follows the monitor it's on.
//!
//! ```no_run
//! # use pencilpilot_gpu::Gpu; use pencilpilot_window::Window;
//! let gpu = Gpu::new()?;
//! let mut win = Window::new(&gpu, "PencilPilot")?;
//! while win.is_open() {
//!     if !win.wants_frame() {
//!         win.wait()?; // until the compositor wants a frame (or something else happens)
//!     }
//!     if win.wants_frame() {
//!         let img = win.acquire(&gpu)?;
//!         gpu.clear(img, [1.0, 1.0, 1.0, 1.0])?;
//!         win.present()?;
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![cfg(target_os = "linux")]

pub mod clock;
pub mod input;
mod keyboard;
pub mod pace;

use std::collections::{HashMap, HashSet};
use std::os::fd::AsFd;

use pencilpilot_gpu::{DRM_FORMAT_XRGB8888, DmabufImage, Gpu};
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

pub use input::*;
pub use wayland_client::protocol::wl_output::WlOutput;

use keyboard::Keyboard;
use wayland_client::backend::ObjectId;
use wayland_client::event_created_child;
use wayland_client::protocol::{wl_keyboard, wl_pointer, wl_seat};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2, zwp_tablet_pad_dial_v2, zwp_tablet_pad_group_v2, zwp_tablet_pad_ring_v2,
    zwp_tablet_pad_strip_v2, zwp_tablet_pad_v2, zwp_tablet_seat_v2, zwp_tablet_tool_v2,
    zwp_tablet_v2,
};

#[derive(Debug)]
pub enum Error {
    /// no Wayland display to connect to, or it went away
    Wayland(String),
    /// the compositor lacks something the window needs
    Missing(&'static str),
    Gpu(pencilpilot_gpu::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Wayland(e) => write!(f, "Wayland: {e}"),
            Error::Missing(w) => write!(f, "the compositor has no {w}"),
            Error::Gpu(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<pencilpilot_gpu::Error> for Error {
    fn from(e: pencilpilot_gpu::Error) -> Self {
        Error::Gpu(e)
    }
}

fn wl(e: impl std::fmt::Display) -> Error {
    Error::Wayland(e.to_string())
}

pub type Result<T> = std::result::Result<T, Error>;

/// A frame the compositor showed.
#[derive(Debug, Clone, Copy)]
pub struct Presented {
    /// which frame (as `present` numbered it)
    pub frame: u64,
    /// when it reached the screen (the compositor's clock, usually CLOCK_MONOTONIC), ns
    pub at_ns: u64,
    /// the output's refresh period then, ns (0: unknown, e.g. variable refresh)
    pub refresh_ns: u32,
}

/// One monitor, as the compositor describes it.
#[derive(Debug, Clone, Default)]
pub struct Output {
    pub name: String,
    /// its current mode's refresh, mHz
    pub refresh_mhz: i32,
    pub width: i32,
    pub height: i32,
}

/// What the window's event handlers keep (Wayland's dispatch writes here).
#[derive(Default)]
struct State {
    open: bool,
    /// the size the compositor asked for (logical pixels; 0: ours to choose)
    asked: (i32, i32),
    configured: bool,
    /// a configure to ack before the next commit
    ack: Option<u32>,
    /// scale × 120 (fractional-scale), or the integer buffer scale × 120
    scale120: u32,
    /// the compositor sends fractional scales: integer ones are ignored
    fractional: bool,
    frame_due: bool,
    busy: HashSet<u64>,
    outputs: HashMap<u32, Output>,
    output_ids: HashMap<wayland_client::backend::ObjectId, u32>,
    on: Vec<u32>,
    presented: Vec<Presented>,
    discarded: u32,
    /// the clock presentation times are on (Linux clock ids; 1 = CLOCK_MONOTONIC)
    clock_id: u32,
    refused: bool,
    modifiers: Vec<u64>,
    // input
    input: Vec<InputEvent>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_gatherer: PointerGatherer,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keymap: Option<Keyboard>,
    pens: HashMap<ObjectId, PenGatherer>,
}

struct Slot {
    image: DmabufImage,
    buffer: wl_buffer::WlBuffer,
    id: u64,
}

/// A window on the screen.
pub struct Window {
    conn: Connection,
    queue: EventQueue<State>,
    qh: QueueHandle<State>,
    st: State,
    _globals: GlobalList,
    surface: wl_surface::WlSurface,
    xdg: xdg_surface::XdgSurface,
    top: xdg_toplevel::XdgToplevel,
    dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    presentation: Option<wp_presentation::WpPresentation>,
    viewport: Option<wp_viewport::WpViewport>,
    _fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    outputs: Vec<WlOutput>,
    slots: Vec<Slot>,
    /// the slot acquired for the next present
    current: Option<usize>,
    next_id: u64,
    /// frames presented so far
    frames: u64,
    /// the size the slots are (device pixels)
    slot_size: (u32, u32),
}

impl Window {
    /// Open a window on the display in `WAYLAND_DISPLAY`.
    pub fn new(gpu: &Gpu, title: &str) -> Result<Window> {
        let conn = Connection::connect_to_env().map_err(wl)?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(wl)?;
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals
            .bind(&qh, 4..=6, ())
            .map_err(|_| Error::Missing("wl_compositor 4+"))?;
        let wm: xdg_wm_base::XdgWmBase = globals
            .bind(&qh, 1..=6, ())
            .map_err(|_| Error::Missing("xdg_wm_base"))?;
        // version 3: the dmabuf modifiers come as events
        let dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 = globals
            .bind(&qh, 3..=3, ())
            .map_err(|_| Error::Missing("zwp_linux_dmabuf_v1"))?;
        let presentation: Option<wp_presentation::WpPresentation> =
            globals.bind(&qh, 1..=2, ()).ok();
        let viewporter: Option<wp_viewporter::WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
        let fractional: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1> =
            globals.bind(&qh, 1..=1, ()).ok();
        let decorations: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1> =
            globals.bind(&qh, 1..=1, ()).ok();
        // input: the seat (pointer, keyboard) and the pen (tablet v2)
        let seat: Option<wl_seat::WlSeat> = globals.bind(&qh, 1..=9, ()).ok();
        let tablets: Option<zwp_tablet_manager_v2::ZwpTabletManagerV2> =
            globals.bind(&qh, 1..=2, ()).ok();
        if let (Some(seat), Some(t)) = (&seat, &tablets) {
            t.get_tablet_seat(seat, &qh, ());
        }
        // every monitor, to know their refresh rates
        let mut outputs = Vec::new();
        let mut st = State {
            open: true,
            scale120: 120,
            ..Default::default()
        };
        for g in globals.contents().clone_list() {
            if g.interface == "wl_output" {
                let o: WlOutput = globals
                    .registry()
                    .bind(g.name, g.version.min(4), &qh, g.name);
                st.output_ids.insert(o.id(), g.name);
                st.outputs.insert(g.name, Output::default());
                outputs.push(o);
            }
        }
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm.get_xdg_surface(&surface, &qh, ());
        let top = xdg.get_toplevel(&qh, ());
        top.set_title(title.into());
        top.set_app_id("app.pencilpilot.studio".into());
        if let Some(d) = &decorations {
            // the compositor draws the frame, where it draws frames (Hyprland draws none)
            let deco = d.get_toplevel_decoration(&top, &qh, ());
            deco.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        }
        let viewport = viewporter
            .as_ref()
            .map(|v| v.get_viewport(&surface, &qh, ()));
        let fractional = match (&fractional, &viewport) {
            (Some(f), Some(_)) => Some(f.get_fractional_scale(&surface, &qh, ())),
            _ => None,
        };
        surface.commit();
        queue.roundtrip(&mut st).map_err(wl)?;
        while !st.configured {
            queue.blocking_dispatch(&mut st).map_err(wl)?;
        }
        st.frame_due = true;
        let _ = gpu;
        Ok(Window {
            conn,
            queue,
            qh,
            st,
            _globals: globals,
            surface,
            xdg,
            top,
            dmabuf,
            presentation,
            viewport,
            _fractional: fractional,
            outputs,
            slots: Vec::new(),
            current: None,
            next_id: 1,
            frames: 0,
            slot_size: (0, 0),
        })
    }

    /// Wait for the compositor (a frame due, a resize, a close…), then handle what came.
    pub fn wait(&mut self) -> Result<()> {
        self.queue.blocking_dispatch(&mut self.st).map_err(wl)?;
        if self.st.refused {
            return Err(Error::Wayland(
                "the compositor refused a frame's dmabuf".into(),
            ));
        }
        Ok(())
    }

    /// Wait for the compositor until `deadline_ns` (CLOCK_MONOTONIC) at the latest. True if
    /// something came.
    pub fn wait_until(&mut self, deadline_ns: u64) -> Result<bool> {
        self.conn.flush().map_err(wl)?;
        if self.queue.dispatch_pending(&mut self.st).map_err(wl)? > 0 {
            return Ok(true);
        }
        let Some(guard) = self.queue.prepare_read() else {
            return Ok(self.queue.dispatch_pending(&mut self.st).map_err(wl)? > 0);
        };
        let now = clock::now_ns();
        if deadline_ns > now && clock::wait_readable(&guard.connection_fd(), deadline_ns - now) {
            guard.read().map_err(wl)?;
        } else {
            drop(guard);
        }
        Ok(self.queue.dispatch_pending(&mut self.st).map_err(wl)? > 0)
    }

    /// Handle what's arrived without waiting.
    pub fn poll(&mut self) -> Result<()> {
        self.conn.flush().map_err(wl)?;
        if let Some(guard) = self.queue.prepare_read() {
            // nothing queued: read whatever is on the socket now (it doesn't block when empty)
            let _ = guard.read();
        }
        self.queue.dispatch_pending(&mut self.st).map_err(wl)?;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.st.open
    }

    /// The compositor is ready for a new frame.
    pub fn wants_frame(&self) -> bool {
        self.st.frame_due
    }

    /// The window's size in logical pixels.
    pub fn logical_size(&self) -> (u32, u32) {
        let (w, h) = self.st.asked;
        (
            if w > 0 { w as u32 } else { 1280 },
            if h > 0 { h as u32 } else { 800 },
        )
    }

    /// The scale from logical to device pixels (1.0, 1.25, 2.0…).
    pub fn scale(&self) -> f64 {
        self.st.scale120 as f64 / 120.0
    }

    /// The size frames are drawn at, in device pixels.
    pub fn size(&self) -> (u32, u32) {
        let (w, h) = self.logical_size();
        let s = self.st.scale120 as u64;
        (
            ((w as u64 * s + 60) / 120) as u32,
            ((h as u64 * s + 60) / 120) as u32,
        )
    }

    /// The monitors the window is on.
    pub fn outputs(&self) -> Vec<Output> {
        self.st
            .on
            .iter()
            .filter_map(|n| self.st.outputs.get(n).cloned())
            .collect()
    }

    /// Every monitor (for the app to put the window on one).
    pub fn all_outputs(&self) -> Vec<(WlOutput, Output)> {
        self.outputs
            .iter()
            .filter_map(|o| {
                self.st
                    .output_ids
                    .get(&o.id())
                    .and_then(|n| self.st.outputs.get(n))
                    .map(|i| (o.clone(), i.clone()))
            })
            .collect()
    }

    /// The refresh period of the monitor the window is on now, ns: from the last presented
    /// frame, or the monitor's mode before any frame has been shown.
    pub fn refresh_ns(&self) -> Option<u32> {
        if let Some(p) = self.st.presented.iter().rev().find(|p| p.refresh_ns > 0) {
            return Some(p.refresh_ns);
        }
        self.outputs()
            .iter()
            .map(|o| o.refresh_mhz)
            .filter(|&m| m > 0)
            .max()
            .map(|m| (1e12 / m as f64).round() as u32)
    }

    /// Frames shown since the last call, oldest first.
    pub fn take_presented(&mut self) -> Vec<Presented> {
        std::mem::take(&mut self.st.presented)
    }

    /// Input since the last call, in order.
    pub fn take_input(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.st.input)
    }

    /// Frames the compositor never showed (it skipped them), since the window opened.
    pub fn discarded(&self) -> u32 {
        self.st.discarded
    }

    /// Fullscreen on a monitor (None: the compositor picks), or back to a window.
    pub fn set_fullscreen(&mut self, on: Option<Option<&WlOutput>>) {
        match on {
            Some(o) => self.top.set_fullscreen(o),
            None => self.top.unset_fullscreen(),
        }
        self.surface.commit();
    }

    pub fn set_maximized(&mut self, on: bool) {
        if on {
            self.top.set_maximized()
        } else {
            self.top.unset_maximized()
        }
        self.surface.commit();
    }

    /// An image to draw the next frame into, at `size()`.
    pub fn acquire(&mut self, gpu: &Gpu) -> Result<&DmabufImage> {
        let size = self.size();
        if size != self.slot_size {
            // a new size: new images (the compositor keeps the old ones it still shows)
            for s in self.slots.drain(..) {
                s.buffer.destroy();
                self.st.busy.remove(&s.id);
            }
            self.slot_size = size;
        }
        let free = self
            .slots
            .iter()
            .position(|s| !self.st.busy.contains(&s.id));
        let i = match free {
            Some(i) => i,
            None if self.slots.len() < 3 => {
                let image = DmabufImage::new(gpu, size.0, size.1, &self.st.modifiers)?;
                let fd = image.export(gpu)?;
                let params = self.dmabuf.create_params(&self.qh, ());
                params.add(
                    fd.as_fd(),
                    0,
                    image.offset,
                    image.stride,
                    (image.modifier >> 32) as u32,
                    image.modifier as u32,
                );
                let id = self.next_id;
                self.next_id += 1;
                let buffer = params.create_immed(
                    size.0 as i32,
                    size.1 as i32,
                    DRM_FORMAT_XRGB8888,
                    zwp_linux_buffer_params_v1::Flags::empty(),
                    &self.qh,
                    id,
                );
                params.destroy();
                self.slots.push(Slot { image, buffer, id });
                self.slots.len() - 1
            }
            None => {
                // all three still on screen or queued: wait for one back
                while self.slots.iter().all(|s| self.st.busy.contains(&s.id)) {
                    self.wait()?;
                }
                return self.acquire(gpu);
            }
        };
        self.current = Some(i);
        Ok(&self.slots[i].image)
    }

    /// The clock presentation times are on (1 = CLOCK_MONOTONIC, as almost always).
    pub fn clock_id(&self) -> u32 {
        self.st.clock_id
    }

    /// Show the acquired image (drawn and handed to the compositor by the GPU). Returns the
    /// frame's number, which its `Presented` carries.
    pub fn present(&mut self) -> Result<u64> {
        let i = self
            .current
            .take()
            .ok_or(Error::Wayland("present without acquire".into()))?;
        let slot = &self.slots[i];
        if let Some(serial) = self.st.ack.take() {
            self.xdg.ack_configure(serial);
        }
        let (lw, lh) = self.logical_size();
        if let Some(v) = &self.viewport {
            // device pixels in, logical pixels on screen
            v.set_destination(lw as i32, lh as i32);
            self.surface.set_buffer_scale(1);
        } else {
            self.surface
                .set_buffer_scale((self.st.scale120 / 120).max(1) as i32);
        }
        self.surface.attach(Some(&slot.buffer), 0, 0);
        self.surface
            .damage_buffer(0, 0, slot.image.width as i32, slot.image.height as i32);
        self.surface.frame(&self.qh, ());
        self.frames += 1;
        let frame = self.frames;
        if let Some(p) = &self.presentation {
            p.feedback(&self.surface, &self.qh, frame);
        }
        self.surface.commit();
        self.st.busy.insert(slot.id);
        self.st.frame_due = false;
        self.conn.flush().map_err(wl)?;
        Ok(frame)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        for s in self.slots.drain(..) {
            s.buffer.destroy();
        }
        self.top.destroy();
        self.xdg.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
    }
}

// ---- the compositor's events ----

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
delegate_noop!(State: ignore wp_viewporter::WpViewporter);
delegate_noop!(State: ignore wp_viewport::WpViewport);
delegate_noop!(State: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(State: ignore zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);
delegate_noop!(State: ignore zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1);

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
            if s.configured {
                // acked with the next frame, so the new size and the ack land together
                s.ack = Some(serial);
                s.frame_due = true;
            } else {
                xs.ack_configure(serial);
                s.configured = true;
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(
        s: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        e: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            xdg_toplevel::Event::Configure { width, height, .. } => s.asked = (width, height),
            xdg_toplevel::Event::Close => s.open = false,
            _ => {}
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_surface::WlSurface,
        e: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_surface::Event::Enter { output } => {
                if let Some(&n) = s.output_ids.get(&output.id()) {
                    s.on.push(n);
                }
            }
            wl_surface::Event::Leave { output } => {
                if let Some(&n) = s.output_ids.get(&output.id()) {
                    s.on.retain(|&o| o != n);
                }
            }
            // without fractional scaling: the integer scale the compositor prefers
            wl_surface::Event::PreferredBufferScale { factor } if !s.fractional => {
                s.scale120 = (factor.max(1) as u32) * 120;
                s.frame_due = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        e: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = e {
            s.fractional = true;
            s.scale120 = scale.max(1);
            s.frame_due = true;
        }
    }
}

impl Dispatch<WlOutput, u32> for State {
    fn event(
        s: &mut Self,
        _: &WlOutput,
        e: wl_output::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let o = s.outputs.entry(*name).or_default();
        match e {
            wl_output::Event::Mode {
                flags: WEnum::Value(f),
                width,
                height,
                refresh,
            } if f.contains(wl_output::Mode::Current) => {
                o.refresh_mhz = refresh;
                o.width = width;
                o.height = height;
            }
            wl_output::Event::Name { name } => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_callback::WlCallback,
        e: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = e {
            s.frame_due = true;
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, u64> for State {
    fn event(
        s: &mut Self,
        _: &wl_buffer::WlBuffer,
        e: wl_buffer::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = e {
            s.busy.remove(id);
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

impl Dispatch<wp_presentation::WpPresentation, ()> for State {
    fn event(
        s: &mut Self,
        _: &wp_presentation::WpPresentation,
        e: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = e {
            s.clock_id = clk_id;
        }
    }
}

impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, u64> for State {
    fn event(
        s: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        e: wp_presentation_feedback::Event,
        frame: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                ..
            } => {
                let secs = ((tv_sec_hi as u64) << 32) | tv_sec_lo as u64;
                s.presented.push(Presented {
                    frame: *frame,
                    at_ns: secs * 1_000_000_000 + tv_nsec as u64,
                    refresh_ns: refresh,
                });
            }
            wp_presentation_feedback::Event::Discarded => s.discarded += 1,
            _ => {}
        }
    }
}

// ---- input ----

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        s: &mut Self,
        seat: &wl_seat::WlSeat,
        e: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(c),
        } = e
        {
            if c.contains(wl_seat::Capability::Pointer) && s.pointer.is_none() {
                s.pointer = Some(seat.get_pointer(qh, ()));
            }
            if c.contains(wl_seat::Capability::Keyboard) && s.keyboard.is_none() {
                s.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        s: &mut Self,
        p: &wl_pointer::WlPointer,
        e: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_pointer::Event as E;
        let piece = match e {
            E::Enter {
                surface_x,
                surface_y,
                ..
            } => PointerPiece::Enter(surface_x, surface_y),
            E::Leave { .. } => PointerPiece::Leave,
            E::Motion {
                time,
                surface_x,
                surface_y,
            } => PointerPiece::Motion(time, surface_x, surface_y),
            E::Button {
                time,
                button,
                state,
                ..
            } => PointerPiece::Button(
                time,
                button,
                state == WEnum::Value(wl_pointer::ButtonState::Pressed),
            ),
            E::Axis { time, axis, value } => PointerPiece::Axis(
                time,
                axis == WEnum::Value(wl_pointer::Axis::VerticalScroll),
                value,
            ),
            E::AxisSource {
                axis_source: WEnum::Value(wl_pointer::AxisSource::Wheel),
            } => PointerPiece::WheelSource,
            E::Frame => PointerPiece::Frame,
            _ => return,
        };
        let mut out = s.pointer_gatherer.take(piece);
        // before version 5 there are no frames: each piece is a moment
        if p.version() < 5 && piece != PointerPiece::Frame {
            out.extend(s.pointer_gatherer.take(PointerPiece::Frame));
        }
        s.input.extend(out.into_iter().map(InputEvent::Pointer));
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        e: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_keyboard::Event as E;
        match e {
            E::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                use std::io::Read;
                let mut text = String::new();
                let file = std::fs::File::from(fd);
                if file.take(size as u64).read_to_string(&mut text).is_ok() {
                    s.keymap = Keyboard::from_text(&text);
                }
            }
            E::Enter { .. } => s.input.push(InputEvent::Focus(true)),
            E::Leave { .. } => s.input.push(InputEvent::Focus(false)),
            E::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(k) = &mut s.keymap {
                    k.set_mods(mods_depressed, mods_latched, mods_locked, group);
                }
            }
            E::Key {
                time, key, state, ..
            } => {
                let pressed = state == WEnum::Value(wl_keyboard::KeyState::Pressed);
                let (keysym, name, text, mods) = match &s.keymap {
                    Some(k) => {
                        let (sym, name, text) = k.key(key);
                        (sym, name, text, k.mods())
                    }
                    None => (0, String::new(), None, Mods::default()),
                };
                s.input.push(InputEvent::Key(KeyEvent {
                    keysym,
                    name,
                    text: if pressed { text } else { None },
                    pressed,
                    mods,
                    code: key,
                    time_ms: time,
                }));
            }
            E::RepeatInfo { rate, delay } => s.input.push(InputEvent::KeyRepeat {
                rate,
                delay_ms: delay,
            }),
            _ => {}
        }
    }
}

delegate_noop!(State: ignore zwp_tablet_manager_v2::ZwpTabletManagerV2);
delegate_noop!(State: ignore zwp_tablet_v2::ZwpTabletV2);
delegate_noop!(State: ignore zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2);
delegate_noop!(State: ignore zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2);
delegate_noop!(State: ignore zwp_tablet_pad_dial_v2::ZwpTabletPadDialV2);

impl Dispatch<zwp_tablet_seat_v2::ZwpTabletSeatV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwp_tablet_seat_v2::ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // tablets, tools and pads arrive as new objects (below)
    }
    event_created_child!(State, zwp_tablet_seat_v2::ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (zwp_tablet_v2::ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (zwp_tablet_tool_v2::ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (zwp_tablet_pad_v2::ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<zwp_tablet_pad_v2::ZwpTabletPadV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwp_tablet_pad_v2::ZwpTabletPadV2,
        _: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // the tablet's own buttons and rings: later (shortcuts)
    }
    event_created_child!(State, zwp_tablet_pad_v2::ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    event_created_child!(State, zwp_tablet_pad_group_v2::ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2, ()),
        zwp_tablet_pad_group_v2::EVT_DIAL_OPCODE => (zwp_tablet_pad_dial_v2::ZwpTabletPadDialV2, ()),
    ]);
}

impl Dispatch<zwp_tablet_tool_v2::ZwpTabletToolV2, ()> for State {
    fn event(
        s: &mut Self,
        tool: &zwp_tablet_tool_v2::ZwpTabletToolV2,
        e: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_tablet_tool_v2::{ButtonState, Event as E, Type};
        let piece = match e {
            E::Type {
                tool_type: WEnum::Value(t),
            } => PenPiece::Type(match t {
                Type::Eraser => PenTool::Eraser,
                Type::Brush => PenTool::Brush,
                Type::Pencil => PenTool::Pencil,
                Type::Airbrush => PenTool::Airbrush,
                Type::Finger => PenTool::Finger,
                Type::Mouse => PenTool::Mouse,
                Type::Lens => PenTool::Lens,
                _ => PenTool::Pen,
            }),
            E::HardwareSerial {
                hardware_serial_hi,
                hardware_serial_lo,
            } => PenPiece::Serial(((hardware_serial_hi as u64) << 32) | hardware_serial_lo as u64),
            E::ProximityIn { .. } => PenPiece::ProximityIn,
            E::ProximityOut => PenPiece::ProximityOut,
            E::Down { .. } => PenPiece::Down,
            E::Up => PenPiece::Up,
            E::Motion { x, y } => PenPiece::Motion(x, y),
            E::Pressure { pressure } => PenPiece::Pressure(pressure),
            E::Distance { distance } => PenPiece::Distance(distance),
            E::Tilt { tilt_x, tilt_y } => PenPiece::Tilt(tilt_x, tilt_y),
            E::Rotation { degrees } => PenPiece::Rotation(degrees),
            E::Button { button, state, .. } => {
                PenPiece::Button(button, state == WEnum::Value(ButtonState::Pressed))
            }
            E::Frame { time } => PenPiece::Frame(time),
            E::Removed => {
                s.pens.remove(&tool.id());
                return;
            }
            _ => return,
        };
        let out = s.pens.entry(tool.id()).or_default().take(piece);
        s.input.extend(out.into_iter().map(InputEvent::Pen));
    }
}
