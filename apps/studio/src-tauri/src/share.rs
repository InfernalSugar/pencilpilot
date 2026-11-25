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
//! **The cloud** (ADR-0014): when a cloud is set (Settings, or built in), every shared
//! session asks it to keep the project, and its always-on member joins. Coming back to a
//! shared project then works when no one else is online: the app catches up from the cloud
//! (`share_catch_up`), hosts the session again and lets the cloud in. The home screen's
//! Cloud tab lists the projects the account is in (`cloud_list`) and opens one on any
//! computer (`cloud_open`). Solo projects never reach it.
//!
//! **AI generation is a solo act** (the owner's rule, ADR-0013): nothing made by AI is
//! generated during a shared session. Every AI feature in the shell starts with
//! [`Sharing::solo_only`], which refuses while a session is live; the session's edits
//! carry drawing, layers and the timeline only.

use std::path::Path;

use pencilpilot_share::cloud::{CatchUp, Cloud, CloudProject, OpenState, Opened};
use pencilpilot_share::{EndpointAddr, Event, Identity, Invite, Member, Relays, Session};

use crate::account::Account;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, mpsc, oneshot};

/// What the window learns on hosting or joining.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    /// the code to share
    pub invite: String,
    pub me: Member,
    pub members: Vec<Member>,
    pub host: bool,
    /// the cloud: "off" (none set), "on" (it keeps this project), "caught_up" (no one was
    /// online: the cloud's copy is waiting in `share_catch_up`), "unreachable"
    pub cloud: &'static str,
}

/// Where the window says to connect (Settings): our own relay, and the cloud's address
/// (empty: the defaults).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Net {
    #[serde(default)]
    pub relay: Option<String>,
    #[serde(default)]
    pub cloud: Option<String>,
}

/// What the cloud lists a project as (from the studio: its name and canvas size).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct About {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

/// The session in progress (one at a time), and the cloud's copy after catching up.
#[derive(Default)]
pub struct Sharing(Mutex<Option<Live>>, Mutex<Option<CatchUp>>);

pub struct Live {
    /// kept current by the pump (who's here, who hosts, the code after a hand-over)
    info: std::sync::Arc<std::sync::Mutex<Info>>,
    tx: mpsc::UnboundedSender<Cmd>,
}

fn info_of(s: &Session, cloud: &'static str) -> Info {
    Info {
        invite: s.invite().encode(),
        me: s.me().clone(),
        members: s.members(),
        host: s.is_host(),
        cloud,
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

/// The cloud's address as text: the app's setting, else `PENCILPILOT_CLOUD` when the app
/// runs, else `PENCILPILOT_CLOUD_ADDR` when it was built; `off` (or nothing) for none.
fn cloud_text(setting: Option<&str>) -> Option<String> {
    let pick = setting
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            std::env::var("PENCILPILOT_CLOUD")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            option_env!("PENCILPILOT_CLOUD_ADDR")
                .filter(|s| !s.is_empty())
                .map(String::from)
        })?;
    (pick.trim() != "off").then(|| pick.trim().to_string())
}

/// Where the cloud is, if one is set (`Err`: set, but the address is wrong).
async fn cloud_addr(net: &Net) -> Option<Result<EndpointAddr, String>> {
    let text = cloud_text(net.cloud.as_deref())?;
    let relay = net.relay.as_deref().filter(|r| !r.trim().is_empty());
    Some(
        pencilpilot_share::cloud::resolve(&text, relay)
            .await
            .map_err(|e| e.to_string()),
    )
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

/// Connect to the cloud as this computer (signed in).
async fn cloud_connect(data: &Path, account: &Account, net: &Net) -> Result<Option<Cloud>, String> {
    let Some(addr) = cloud_addr(net).await else {
        return Ok(None);
    };
    let addr = addr?;
    let id = identity(data)?;
    let (pass, _) = account.pass(data, &id).await?;
    Cloud::connect(addr, &id, &pass, relays(net.relay.as_deref()))
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Have the cloud keep a session's project and join it (in the background: it never holds
/// up drawing).
fn keep_in_cloud(app: &AppHandle, data: &Path, net: &Net, code: String, about: &About) {
    let (app, data, net, about) = (app.clone(), data.to_path_buf(), net.clone(), about.clone());
    tauri::async_runtime::spawn(async move {
        let account = app.state::<Account>();
        if let Ok(Some(mut c)) = cloud_connect(&data, &account, &net).await {
            let _ = c.keep(&code, &about.name, about.width, about.height).await;
            c.close().await;
        }
    });
}

/// The home screen's Cloud tab: the projects, and whether the list is fresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudList {
    /// "ok", "off" (no cloud set), "signed_out", "unreachable"
    pub status: String,
    pub projects: Vec<CloudProject>,
    /// when the list was fetched (seconds since 1970; 0: never)
    pub fetched: u64,
    #[serde(default)]
    pub error: String,
}

fn list_file(data: &Path) -> std::path::PathBuf {
    data.join("share").join("cloud-list.json")
}

/// The projects in the cloud this account is a member of. When the cloud can't be reached,
/// the list from last time.
pub async fn cloud_list(data: &Path, account: &Account, net: &Net) -> CloudList {
    let last = || -> CloudList {
        std::fs::read(list_file(data))
            .ok()
            .and_then(|b| serde_json::from_slice::<CloudList>(&b).ok())
            .unwrap_or(CloudList {
                status: String::new(),
                projects: Vec::new(),
                fetched: 0,
                error: String::new(),
            })
    };
    if cloud_text(net.cloud.as_deref()).is_none() {
        return CloudList {
            status: "off".into(),
            ..last()
        };
    }
    if account.status(data).await.is_none() {
        return CloudList {
            status: "signed_out".into(),
            projects: Vec::new(),
            fetched: 0,
            error: String::new(),
        };
    }
    let got = async {
        let mut c = cloud_connect(data, account, net)
            .await?
            .ok_or("No cloud is set.")?;
        let list = c.list().await.map_err(|e| e.to_string());
        c.close().await;
        list
    }
    .await;
    match got {
        Ok(projects) => {
            let fresh = CloudList {
                status: "ok".into(),
                projects,
                fetched: pencilpilot_share::now(),
                error: String::new(),
            };
            let _ = std::fs::create_dir_all(data.join("share"));
            if let Ok(b) = serde_json::to_vec(&fresh) {
                let _ = std::fs::write(list_file(data), b);
            }
            fresh
        }
        Err(e) => CloudList {
            status: "unreachable".into(),
            error: e,
            ..last()
        },
    }
}

/// How to open a project from the cloud (on any computer): its invite and size.
pub async fn cloud_open(
    data: &Path,
    account: &Account,
    net: &Net,
    project: &str,
) -> Result<Opened, String> {
    let mut c = cloud_connect(data, account, net)
        .await?
        .ok_or("No cloud is set: Settings → Drawing together → Cloud.")?;
    let o = c.open(project).await.map_err(|e| e.to_string());
    c.close().await;
    o
}

/// A shared project's new name or picture, for the cloud's list (best effort).
pub async fn cloud_touch(
    data: &Path,
    account: &Account,
    net: &Net,
    code: &str,
    name: Option<String>,
    thumb: Option<String>,
) -> Result<(), String> {
    let project = Invite::decode(code).map_err(|e| e.to_string())?.project();
    let Some(mut c) = cloud_connect(data, account, net).await? else {
        return Ok(());
    };
    let r = c
        .touch(&project, name, thumb)
        .await
        .map_err(|e| e.to_string());
    c.close().await;
    r
}

impl Sharing {
    pub async fn host(
        &self,
        app: AppHandle,
        data: &Path,
        account: &Account,
        net: &Net,
        about: &About,
    ) -> Result<Info, String> {
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        let s = Session::host(&id, &pass, &site, relays(net.relay.as_deref()))
            .await
            .map_err(|e| e.to_string())?;
        let cloud = self.keep(&app, data, net, &s, about);
        Ok(self.start(app, s, cloud).await)
    }

    pub async fn join(
        &self,
        app: AppHandle,
        data: &Path,
        code: &str,
        account: &Account,
        net: &Net,
        about: &About,
    ) -> Result<Info, String> {
        let invite = Invite::decode(code).map_err(|e| e.to_string())?;
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        let s = Session::join(&id, &invite, &pass, &site, relays(net.relay.as_deref()))
            .await
            .map_err(|e| e.to_string())?;
        let cloud = self.keep(&app, data, net, &s, about);
        Ok(self.start(app, s, cloud).await)
    }

    /// The cloud keeps a live session's project (if one is set).
    fn keep(
        &self,
        app: &AppHandle,
        data: &Path,
        net: &Net,
        s: &Session,
        about: &About,
    ) -> &'static str {
        if cloud_text(net.cloud.as_deref()).is_none() {
            return "off";
        }
        keep_in_cloud(app, data, net, s.invite().encode(), about);
        "on"
    }

    /// Come back to a shared project after a break: join whoever of its members is online
    /// (`code`: its saved invite; `addrs`: the members' addresses as saved), found directly
    /// or through the cloud. When no one is online, catch up from the cloud (if one is
    /// set) and host it again.
    #[allow(clippy::too_many_arguments)]
    pub async fn rejoin(
        &self,
        app: AppHandle,
        data: &Path,
        code: &str,
        addrs: Vec<Value>,
        account: &Account,
        net: &Net,
        about: &About,
    ) -> Result<Info, String> {
        let mut invite = Invite::decode(code).map_err(|e| e.to_string())?;
        let id = identity(data)?;
        let (pass, site) = account.pass(data, &id).await?;
        self.leave().await;
        *self.1.lock().await = None;
        for a in addrs {
            if let Ok(a) = serde_json::from_value::<EndpointAddr>(a)
                && a.id.to_string() != id.id()
                && !invite.addrs.iter().any(|x| x.id == a.id)
            {
                invite.addrs.push(a);
            }
        }
        // the cloud first: it knows whether anyone is in, and passes us on to them
        let mut cloud_state = "off";
        let mut cloud = None;
        if cloud_text(net.cloud.as_deref()).is_some() {
            cloud_state = "unreachable";
            if let Ok(Some(mut c)) = cloud_connect(data, account, net).await {
                let project = invite.project();
                let mut opened = c.open(&project).await;
                // someone else is opening it this moment: meet them in their session
                let until = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                while matches!(&opened, Ok(o) if o.state == OpenState::Starting)
                    && tokio::time::Instant::now() < until
                {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    opened = c.open(&project).await;
                }
                match opened {
                    Ok(o) => {
                        if let Ok(via) = Invite::decode(&o.code) {
                            for a in via.addrs.into_iter().rev() {
                                invite.addrs.retain(|x| x.id != a.id);
                                invite.addrs.insert(0, a);
                            }
                        }
                        cloud_state = if o.state == OpenState::Idle {
                            "idle"
                        } else {
                            "on"
                        };
                        cloud = Some(c);
                    }
                    // not kept (yet), or not a member: it's kept once the session starts
                    Err(_) => {
                        cloud_state = "on";
                        c.close().await;
                    }
                }
            }
        }
        invite.addrs.retain(|a| a.id.to_string() != id.id());
        // (the cloud says no one is in: a short try, for a session it was never let into)
        let wait = if cloud_state == "idle" { 6 } else { 12 };
        let joined = if invite.addrs.is_empty() {
            None
        } else {
            tokio::time::timeout(
                std::time::Duration::from_secs(wait),
                Session::join(&id, &invite, &pass, &site, relays(net.relay.as_deref())),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
        };
        let s = match joined {
            Some(s) => {
                if let Some(c) = cloud.take() {
                    c.close().await;
                }
                s
            }
            None => {
                // no one is in: the cloud's copy, then this app hosts it again
                if let Some(mut c) = cloud.take() {
                    match c.catch_up(&invite.project()).await {
                        Ok(got) => {
                            *self.1.lock().await = Some(got);
                            cloud_state = "caught_up";
                        }
                        Err(_) => cloud_state = "unreachable",
                    }
                    c.close().await;
                }
                Session::host_again(&id, &invite, &pass, &site, relays(net.relay.as_deref()))
                    .await
                    .map_err(|e| e.to_string())?
            }
        };
        let kept = self.keep(&app, data, net, &s, about);
        if cloud_state == "idle" || (cloud_state == "off" && kept == "on") {
            cloud_state = kept;
        }
        Ok(self.start(app, s, cloud_state).await)
    }

    /// The cloud's copy from coming back when no one was online (once: it's forgotten as
    /// it's taken).
    pub async fn take_catch_up(&self) -> Option<CatchUp> {
        self.1.lock().await.take()
    }

    async fn start(&self, app: AppHandle, s: Session, cloud: &'static str) -> Info {
        let info = info_of(&s, cloud);
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
                        let mut i = info.lock().unwrap_or_else(|e| e.into_inner());
                        *i = info_of(&s, i.cloud);
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
                cloud: "off",
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

    #[test]
    fn the_setting_picks_the_cloud_and_off_turns_it_off() {
        assert_eq!(
            cloud_text(Some(" abc@cloud.example.com:7843 ")).as_deref(),
            Some("abc@cloud.example.com:7843")
        );
        assert_eq!(cloud_text(Some("off")), None);
    }

    #[tokio::test]
    async fn no_cloud_set_means_no_cloud_list() {
        let dir = std::env::temp_dir().join(format!("pp-cloud-list-{}", std::process::id()));
        let net = Net {
            relay: None,
            cloud: Some("off".into()),
        };
        let l = cloud_list(&dir, &Account::default(), &net).await;
        assert_eq!(l.status, "off");
        assert!(l.projects.is_empty());
    }
}
