//! Text on the system's clipboard (`wl_data_device`: what Ctrl+C and Ctrl+V share between
//! apps), and waking the window from another thread.
//!
//! - **Copy:** [`crate::Window::set_clipboard`] offers the text as the selection; the
//!   compositor asks for it when another app pastes, and it's written then.
//! - **Paste:** [`crate::Window::request_paste`] asks the selection's owner for its text; it
//!   arrives later as [`crate::InputEvent::Paste`] (read on its own thread, so the window
//!   never waits on another app).
//! - **Waking:** [`Waker::wake`] ends the window's wait from any thread (a pipe the wait
//!   watches as well as the compositor's socket), so work that arrives from elsewhere (the
//!   network) is handled at once without the window polling.

use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use wayland_client::protocol::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source,
};
use wayland_client::{Connection, Dispatch, QueueHandle, event_created_child};

use crate::State;

/// The text types offered and taken, best first.
const TEXT: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

/// The most text a paste takes (an invite code is ~150 bytes; nothing pasted needs more).
const MAX_PASTE: u64 = 1 << 20;

/// Ends the window's wait from any thread ([`crate::Window::waker`]).
#[derive(Clone, Debug)]
pub struct Waker(Arc<UnixStream>);

impl Waker {
    /// The window wakes (at once if it's waiting, else at its next wait).
    pub fn wake(&self) {
        // (a full pipe is already a wake)
        let _ = (&*self.0).write(&[1]);
    }
}

/// The window's side of the pipe.
pub(crate) struct Wake {
    rx: UnixStream,
    tx: Arc<UnixStream>,
}

impl Wake {
    pub(crate) fn new() -> std::io::Result<Wake> {
        let (rx, tx) = UnixStream::pair()?;
        rx.set_nonblocking(true)?;
        tx.set_nonblocking(true)?;
        Ok(Wake {
            rx,
            tx: Arc::new(tx),
        })
    }

    pub(crate) fn waker(&self) -> Waker {
        Waker(self.tx.clone())
    }

    pub(crate) fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.rx.as_fd()
    }

    /// Empties the pipe; whether anything was in it.
    pub(crate) fn drain(&self) -> bool {
        let mut buf = [0u8; 64];
        let mut any = false;
        while let Ok(n) = (&self.rx).read(&mut buf) {
            if n == 0 {
                break;
            }
            any = true;
        }
        any
    }
}

/// What the clipboard keeps between the compositor's events.
#[derive(Default)]
pub(crate) struct Clip {
    pub(crate) device: Option<wl_data_device::WlDataDevice>,
    /// the text this app offers, and its source
    pub(crate) mine: Option<(wl_data_source::WlDataSource, String)>,
    /// offers being described (their text types so far)
    pub(crate) offers: Vec<(wl_data_offer::WlDataOffer, Vec<String>)>,
    /// the selection now (another app's, or this one's), and its text types
    pub(crate) selection: Option<(wl_data_offer::WlDataOffer, Vec<String>)>,
    /// pastes read on their threads, waiting to be taken as input
    pub(crate) arrived: Arc<Mutex<Vec<String>>>,
    /// the last input's serial (setting the selection needs one)
    pub(crate) serial: u32,
}

impl Clip {
    /// The best text type an offer has.
    fn text_type(types: &[String]) -> Option<&'static str> {
        TEXT.into_iter().find(|t| types.iter().any(|x| x == t))
    }
}

pub(crate) fn set(
    st: &mut State,
    manager: Option<&wl_data_device_manager::WlDataDeviceManager>,
    qh: &QueueHandle<State>,
    text: &str,
) -> bool {
    let (Some(m), Some(dev)) = (manager, st.clip.device.as_ref()) else {
        return false;
    };
    let src = m.create_data_source(qh, ());
    for t in TEXT {
        src.offer(t.into());
    }
    dev.set_selection(Some(&src), st.clip.serial);
    if let Some((old, _)) = st.clip.mine.replace((src, text.to_string())) {
        old.destroy();
    }
    true
}

/// Asks for the selection's text: it arrives as a paste (true), or there's none.
pub(crate) fn request(st: &mut State, waker: Waker) -> bool {
    // this app's own text (until another app takes the selection): no need to ask
    if let Some((_, text)) = &st.clip.mine {
        st.input.push(crate::InputEvent::Paste(text.clone()));
        return true;
    }
    let Some((offer, types)) = &st.clip.selection else {
        return false;
    };
    let Some(t) = Clip::text_type(types) else {
        return false;
    };
    let Ok((rx, tx)) = UnixStream::pair() else {
        return false;
    };
    offer.receive(t.into(), tx.as_fd());
    drop(tx);
    let arrived = st.clip.arrived.clone();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = rx.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        if (&rx).take(MAX_PASTE).read_to_string(&mut text).is_ok() && !text.is_empty() {
            if let Ok(mut a) = arrived.lock() {
                a.push(text);
            }
            waker.wake();
        }
    });
    true
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_data_device_manager::WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_data_device::WlDataDevice,
        e: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wl_data_device::Event as E;
        match e {
            E::DataOffer { id } => s.clip.offers.push((id, Vec::new())),
            E::Selection { id } => {
                let next = id.and_then(|id| {
                    let i = s.clip.offers.iter().position(|(o, _)| *o == id)?;
                    Some(s.clip.offers.remove(i))
                });
                if let Some((old, _)) = std::mem::replace(&mut s.clip.selection, next) {
                    old.destroy();
                }
                // (offers that were for dragging are let go too)
                for (o, _) in s.clip.offers.drain(..) {
                    o.destroy();
                }
            }
            _ => {}
        }
    }

    event_created_child!(State, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn event(
        s: &mut Self,
        offer: &wl_data_offer::WlDataOffer,
        e: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = e {
            for (o, types) in s.clip.offers.iter_mut().chain(s.clip.selection.iter_mut()) {
                if o == offer {
                    types.push(mime_type.clone());
                }
            }
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for State {
    fn event(
        s: &mut Self,
        src: &wl_data_source::WlDataSource,
        e: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_data_source::Event::Send { fd, .. } => {
                if let Some((mine, text)) = &s.clip.mine
                    && mine == src
                {
                    write_out(fd, text.clone());
                }
            }
            wl_data_source::Event::Cancelled => {
                if s.clip.mine.as_ref().is_some_and(|(m, _)| m == src) {
                    s.clip.mine = None;
                }
                src.destroy();
            }
            _ => {}
        }
    }
}

/// Writes the text to the pasting app (on its own thread: the other app may read slowly).
fn write_out(fd: OwnedFd, text: String) {
    std::thread::spawn(move || {
        let mut f = std::fs::File::from(fd);
        let _ = f.write_all(text.as_bytes());
    });
}
