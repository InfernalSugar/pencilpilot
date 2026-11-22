//! Drawing together, peer to peer (ADR-0013, S12-T03): the live session runs here in the
//! shell, never in the window (whose CSP keeps it offline). Nothing connects until the
//! person hosts or joins; solo projects never do.
//!
//! The window calls `share_host` / `share_join`, sends edits and cursors, and listens to
//! "share-event" for what the others do.
//!
//! **Signed in only:** hosting and joining need this computer's sign-in pass from the
//! website ([`crate::account`]); names in the session come from passes.
//!
//! **AI generation is a solo act** (the owner's rule, ADR-0013): nothing made by AI is
//! generated during a shared session. Every AI feature in the shell starts with
//! [`Sharing::solo_only`], which refuses while a session is live; the session's edits
//! carry drawing, layers and the timeline only.

use std::path::Path;

use pencilpilot_share::{Event, Identity, Invite, Member, Relays, Session};

use crate::account::Account;
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
    /// kept current by the pump (who's here, who hosts, the code after a hand-over)
    info: std::sync::Arc<std::sync::Mutex<Info>>,
    tx: mpsc::UnboundedSender<Cmd>,
}

fn info_of(s: &Session) -> Info {
    Info {
        invite: s.invite().encode(),
        me: s.me().clone(),
        members: s.members(),
        host: s.is_host(),
    }
}

enum Cmd {
    Edit(Value),
    Direct(String, Value),
    Presence(Value),
    Leave(oneshot::Sender<()>),
}

/// Relays: the app's setting (our own relay's URL), else `PENCILPILOT_RELAY`
/// (comma-separated URLs, or `off`: same network only), else iroh's public relays.
fn relays(setting: Option<&str>) -> Relays {
    if let Some(url) = setting.map(str::trim).filter(|u| !u.is_empty()) {
        return Relays::Custom(vec![url.to_string()]);
    }
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
    pub async fn host(
        &self,
        app: AppHandle,
        data: &Path,
        account: &Account,
        relay: Option<&str>,
    ) -> Result<Info, String> {
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        let s = Session::host(&id, &pass, &site, relays(relay))
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.start(app, s).await)
    }

    pub async fn join(
        &self,
        app: AppHandle,
        data: &Path,
        code: &str,
        account: &Account,
        relay: Option<&str>,
    ) -> Result<Info, String> {
        let invite = Invite::decode(code).map_err(|e| e.to_string())?;
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        let s = Session::join(&id, &invite, &pass, &site, relays(relay))
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.start(app, s).await)
    }

    /// Come back to a shared project after a break: join whoever of its members is online
    /// (`code`: its saved invite; `addrs`: the members' addresses as saved), or, when no one
    /// answers, host it again.
    pub async fn rejoin(
        &self,
        app: AppHandle,
        data: &Path,
        code: &str,
        addrs: Vec<Value>,
        account: &Account,
        relay: Option<&str>,
    ) -> Result<Info, String> {
        let mut invite = Invite::decode(code).map_err(|e| e.to_string())?;
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        for a in addrs {
            if let Ok(a) = serde_json::from_value::<pencilpilot_share::EndpointAddr>(a)
                && a.id.to_string() != id.id()
                && !invite.addrs.iter().any(|x| x.id == a.id)
            {
                invite.addrs.push(a);
            }
        }
        invite.addrs.retain(|a| a.id.to_string() != id.id());
        let joined = if invite.addrs.is_empty() {
            None
        } else {
            tokio::time::timeout(
                std::time::Duration::from_secs(12),
                Session::join(&id, &invite, &pass, &site, relays(relay)),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
        };
        let s = match joined {
            Some(s) => s,
            None => Session::host_again(&id, &invite, &pass, &site, relays(relay))
                .await
                .map_err(|e| e.to_string())?,
        };
        Ok(self.start(app, s).await)
    }

    async fn start(&self, app: AppHandle, s: Session) -> Info {
        let info = info_of(&s);
        let shared = std::sync::Arc::new(std::sync::Mutex::new(info.clone()));
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(pump(app, s, rx, shared.clone()));
        *self.0.lock().await = Some(Live { info: shared, tx });
        info
    }

    /// AI features call this first: an error while drawing together (AI is solo only).
    pub async fn solo_only(&self) -> Result<(), String> {
        if self.status().await.is_some() {
            return Err("AI tools are for solo work: leave the shared session to use them.".into());
        }
        Ok(())
    }

    pub async fn status(&self) -> Option<Info> {
        let mut g = self.0.lock().await;
        // a session that ended on its own (the host left) is gone
        if g.as_ref().is_some_and(|l| l.tx.is_closed()) {
            *g = None;
        }
        g.as_ref()
            .map(|l| l.info.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    pub async fn edit(&self, body: Value) -> Result<(), String> {
        self.send(Cmd::Edit(body)).await
    }

    pub async fn direct(&self, to: String, body: Value) -> Result<(), String> {
        self.send(Cmd::Direct(to, body)).await
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
    Edit {
        seq: u64,
        from: String,
        body: Value,
    },
    Presence {
        from: String,
        body: Value,
    },
    Direct {
        from: String,
        body: Value,
    },
    Joined {
        member: Member,
    },
    Left {
        id: String,
    },
    /// the host left and `host` took over (`you`: this app); who's here; the new code
    HostChanged {
        host: String,
        you: bool,
        members: Vec<Member>,
        invite: String,
    },
    Closed {
        reason: String,
    },
}

fn window_event(e: Event, s: &Session) -> WindowEvent {
    match e {
        Event::Edit { seq, from, body } => WindowEvent::Edit { seq, from, body },
        Event::Presence { from, body } => WindowEvent::Presence { from, body },
        Event::Direct { from, body } => WindowEvent::Direct { from, body },
        Event::Joined(member) => WindowEvent::Joined { member },
        Event::Left { id } => WindowEvent::Left { id },
        Event::HostChanged { host, members } => WindowEvent::HostChanged {
            you: host == s.me().id,
            host,
            members,
            invite: s.invite().encode(),
        },
        Event::Closed { reason } => WindowEvent::Closed { reason },
    }
}

/// Owns the session: the window's commands in, the session's events out.
async fn pump(
    app: AppHandle,
    mut s: Session,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    info: std::sync::Arc<std::sync::Mutex<Info>>,
) {
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Edit(body)) => {
                    if let Err(e) = s.edit(body) {
                        let _ = app.emit("share-event", WindowEvent::Closed { reason: e.to_string() });
                        break;
                    }
                }
                Some(Cmd::Direct(to, body)) => {
                    let _ = s.direct(&to, body);
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
                    if matches!(e, Event::Joined(_) | Event::Left { .. } | Event::HostChanged { .. }) {
                        *info.lock().unwrap_or_else(|e| e.into_inner()) = info_of(&s);
                    }
                    let _ = app.emit("share-event", window_event(e, &s));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ai_is_refused_while_drawing_together() {
        let sharing = Sharing::default();
        assert!(sharing.solo_only().await.is_ok(), "solo: AI allowed");
        let (tx, _rx) = mpsc::unbounded_channel();
        let me = Member {
            id: "me".into(),
            user: "u_me".into(),
            name: "Me".into(),
            addr: pencilpilot_share::EndpointAddr::new(
                pencilpilot_share::SecretKey::generate().public(),
            ),
            pass: pencilpilot_share::Pass {
                body: String::new(),
                sig: String::new(),
            },
        };
        *sharing.0.lock().await = Some(Live {
            info: std::sync::Arc::new(std::sync::Mutex::new(Info {
                invite: String::new(),
                me: me.clone(),
                members: vec![me],
                host: true,
            })),
            tx,
        });
        let e = sharing.solo_only().await.unwrap_err();
        assert!(e.contains("solo"), "{e}");
    }
}

#[cfg(test)]
mod relay_tests {
    use super::*;

    #[test]
    fn the_setting_picks_our_relay() {
        assert_eq!(
            relays(Some(" https://relay.example.com ")),
            Relays::Custom(vec!["https://relay.example.com".into()])
        );
        // empty: the environment or iroh's public relays
        assert_ne!(relays(Some("")), Relays::Custom(vec![String::new()]));
    }
}
