//! Drawing together, peer to peer (ADR-0013, S12-T03): the live session runs here in the
//! shell, never in the window (whose CSP keeps it offline). Nothing connects until the
//! person hosts or joins; solo projects never do.
//!
//! The window calls `share_host` / `share_join`, sends edits and cursors, and listens to
//! "share-event" for what the others do.

use std::path::Path;

use pencilpilot_share::{Event, Identity, Invite, Member, Relays, Session};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, mpsc, oneshot};

/// What the window learns on hosting or joining.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    /// the code to share
    pub invite: String,
    pub me: Member,
    pub members: Vec<Member>,
    pub host: bool,
}

/// The session in progress (one at a time).
#[derive(Default)]
pub struct Sharing(Mutex<Option<Live>>);

pub struct Live {
    info: Info,
    tx: mpsc::UnboundedSender<Cmd>,
}

enum Cmd {
    Edit(Value),
    Presence(Value),
    Leave(oneshot::Sender<()>),
}

/// Relays: `PENCILPILOT_RELAY` (comma-separated URLs, our own), `off` (same network only),
/// or iroh's public relays.
fn relays() -> Relays {
    match std::env::var("PENCILPILOT_RELAY") {
        Ok(v) if v.trim() == "off" => Relays::Off,
        Ok(v) if !v.trim().is_empty() => {
            Relays::Custom(v.split(',').map(|s| s.trim().to_string()).collect())
        }
        _ => Relays::Iroh,
    }
}

/// This computer's key (`PENCILPILOT_SHARE_PROFILE` picks another, to try two apps on one
/// computer).
fn identity(data: &Path) -> Result<Identity, String> {
    let file = match std::env::var("PENCILPILOT_SHARE_PROFILE") {
        Ok(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("key-{p}")
        }
        _ => "key".into(),
    };
    Identity::load_or_create(&data.join("share").join(file)).map_err(|e| e.to_string())
}

impl Sharing {
    pub async fn host(&self, app: AppHandle, data: &Path, name: &str) -> Result<Info, String> {
        self.leave().await;
        let id = identity(data)?;
        let s = Session::host(&id, name, relays())
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.start(app, s).await)
    }

    pub async fn join(
        &self,
        app: AppHandle,
        data: &Path,
        code: &str,
        name: &str,
    ) -> Result<Info, String> {
        let invite = Invite::decode(code).map_err(|e| e.to_string())?;
        self.leave().await;
        let id = identity(data)?;
        let s = Session::join(&id, &invite, name, relays())
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.start(app, s).await)
    }

    async fn start(&self, app: AppHandle, s: Session) -> Info {
        let info = Info {
            invite: s.invite().encode(),
            me: s.me().clone(),
            members: s.members(),
            host: s.is_host(),
        };
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(pump(app, s, rx));
        *self.0.lock().await = Some(Live {
            info: info.clone(),
            tx,
        });
        info
    }

    pub async fn status(&self) -> Option<Info> {
        let mut g = self.0.lock().await;
        // a session that ended on its own (the host left) is gone
        if g.as_ref().is_some_and(|l| l.tx.is_closed()) {
            *g = None;
        }
        g.as_ref().map(|l| l.info.clone())
    }

    pub async fn edit(&self, body: Value) -> Result<(), String> {
        self.send(Cmd::Edit(body)).await
    }

    pub async fn presence(&self, body: Value) -> Result<(), String> {
        self.send(Cmd::Presence(body)).await
    }

    async fn send(&self, cmd: Cmd) -> Result<(), String> {
        let g = self.0.lock().await;
        let live = g.as_ref().ok_or("Not drawing together right now.")?;
        live.tx
            .send(cmd)
            .map_err(|_| "The session has ended.".to_string())
    }

    pub async fn leave(&self) {
        let live = self.0.lock().await.take();
        if let Some(live) = live {
            let (done, wait) = oneshot::channel();
            if live.tx.send(Cmd::Leave(done)).is_ok() {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), wait).await;
            }
        }
    }
}

/// The event the window gets ("share-event").
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WindowEvent {
    Edit { seq: u64, from: String, body: Value },
    Presence { from: String, body: Value },
    Joined { member: Member },
    Left { id: String },
    Closed { reason: String },
}

impl From<Event> for WindowEvent {
    fn from(e: Event) -> Self {
        match e {
            Event::Edit { seq, from, body } => Self::Edit { seq, from, body },
            Event::Presence { from, body } => Self::Presence { from, body },
            Event::Joined(member) => Self::Joined { member },
            Event::Left { id } => Self::Left { id },
            Event::Closed { reason } => Self::Closed { reason },
        }
    }
}

/// Owns the session: the window's commands in, the session's events out.
async fn pump(app: AppHandle, mut s: Session, mut rx: mpsc::UnboundedReceiver<Cmd>) {
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Edit(body)) => {
                    if let Err(e) = s.edit(body) {
                        let _ = app.emit("share-event", WindowEvent::Closed { reason: e.to_string() });
                        break;
                    }
                }
                Some(Cmd::Presence(body)) => s.presence(body),
                Some(Cmd::Leave(done)) => {
                    s.close().await;
                    let _ = done.send(());
                    return;
                }
                None => break,
            },
            ev = s.next_event() => match ev {
                Some(e) => {
                    let closed = matches!(e, Event::Closed { .. });
                    let _ = app.emit("share-event", WindowEvent::from(e));
                    if closed {
                        break;
                    }
                }
                None => break,
            },
        }
    }
    s.close().await;
}
