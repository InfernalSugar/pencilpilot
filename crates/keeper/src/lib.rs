//! The keeper: the cloud's always-on member (ADR-0014).
//!
//! It joins each shared project's session as a member who never leaves (on one endpoint
//! for all of them: [`pencilpilot_share::Hub`]), keeps the project as the last snapshot a
//! member sent plus every edit since ([`store`]), and answers apps over the cloud protocol
//! ([`pencilpilot_share::cloud`]): which projects someone is in, how to open one, and the
//! project itself for catching up when no one else is online.
//!
//! - **Members only.** Every app shows its sign-in pass and proves it holds the computer
//!   key the pass names. The projects someone may list, open or catch up on are those
//!   their account has been in (in a session the keeper was in, or by having it kept with
//!   its invite).
//! - **Never in the way.** It never hosts, never draws and is never handed the session;
//!   cursors don't reach it. Live drawing stays between the members' apps.
//! - **Compaction.** Now and then (and when the log grows past a size), it asks the
//!   session's host for a snapshot (the same one a joiner gets); a whole snapshot replaces
//!   every edit up to it.
//! - **Space.** Each account has a limit on what its projects take. Past it, a project's
//!   new edits aren't kept (it's marked full) until a snapshot makes room; no new project
//!   is taken on.

pub mod store;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pencilpilot_share::cloud::{
    CLOUD_ALPN, CloudProject, KEEPER_USER, OpenState, Opened, Reply, Request, check_proof,
};
use pencilpilot_share::{
    Connection, EndpointAddr, Event, Hub, Identity, Invite, Member, Pass, Relays, Session, SiteKey,
    now, read_json, write,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use store::{Meta, SnapWriter, Store};

/// The keeper's settings (`keeper.toml`, see deploy/keeper).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// where projects are kept
    pub data: PathBuf,
    /// the keeper's computer key (made on the first start)
    pub key: PathBuf,
    /// its sign-in pass from the website (JSON `{ "body", "sig" }`)
    pub pass: PathBuf,
    /// the website's public key (64 hex characters; the apps' PENCILPILOT_PASS_KEY)
    pub site_key: String,
    /// the UDP port it listens on
    #[serde(default = "default_port")]
    pub port: u16,
    /// relays, like the apps': our own (`https://…`), `[]` for iroh's public ones, or
    /// `["off"]` for none (only apps that can reach it directly)
    #[serde(default)]
    pub relays: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
}

fn default_port() -> u16 {
    7843
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// what one account's projects may take, in MB
    pub account_mb: u64,
    /// projects one account may have kept
    pub projects_per_account: usize,
    /// ask for a snapshot when the edits since the last one pass this many MB
    pub compact_after_mb: u64,
    /// and at least this often while people draw (minutes)
    pub compact_every_minutes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            account_mb: 1024,
            projects_per_account: 200,
            compact_after_mb: 32,
            compact_every_minutes: 30,
        }
    }
}

/// The limits in the units the keeper works in (tests set bytes and seconds directly).
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub account_bytes: u64,
    pub projects_per_account: usize,
    pub compact_after_bytes: u64,
    pub compact_every: Duration,
    /// how long someone opening a project with no one in it has it to themselves
    pub claim: Duration,
}

impl From<&Limits> for Rules {
    fn from(l: &Limits) -> Self {
        Self {
            account_bytes: l.account_mb << 20,
            projects_per_account: l.projects_per_account,
            compact_after_bytes: l.compact_after_mb << 20,
            compact_every: Duration::from_secs(l.compact_every_minutes.max(1) * 60),
            claim: Duration::from_secs(45),
        }
    }
}

impl Config {
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn relays(&self) -> Relays {
        match self.relays.as_slice() {
            [] => Relays::Iroh,
            [one] if one == "off" => Relays::Off,
            urls => Relays::Custom(urls.to_vec()),
        }
    }
}

/// The keeper, running.
#[derive(Clone)]
pub struct Keeper {
    inner: Arc<Inner>,
}

struct Inner {
    hub: Arc<Hub>,
    site: SiteKey,
    pass: Pass,
    rules: Rules,
    store: Mutex<Store>,
    /// projects whose session the keeper is in
    live: Mutex<HashMap<String, Live>>,
    /// projects it's joining now
    joining: Mutex<HashSet<String>>,
    /// someone is opening a project no one was in: (their computer, until)
    claims: Mutex<HashMap<String, (String, Instant)>>,
}

struct Live {
    /// who's in the session (the keeper too), and its host
    members: Arc<Mutex<(Vec<Member>, Option<EndpointAddr>)>>,
    stop: mpsc::UnboundedSender<()>,
}

impl Keeper {
    /// Start from settings: reads the key, pass and site key, opens the store, listens.
    pub async fn start(cfg: &Config) -> Result<Self, String> {
        let id = Identity::load_or_create(&cfg.key).map_err(|e| e.to_string())?;
        let pass: Pass = serde_json::from_str(
            &std::fs::read_to_string(&cfg.pass)
                .map_err(|e| format!("{}: {e}", cfg.pass.display()))?,
        )
        .map_err(|e| format!("{}: {e}", cfg.pass.display()))?;
        let site = pencilpilot_share::site_key(&cfg.site_key).map_err(|e| e.to_string())?;
        Self::run(
            &id,
            pass,
            site,
            cfg.relays(),
            cfg.port,
            &cfg.data,
            (&cfg.limits).into(),
        )
        .await
    }

    /// Start with everything given (tests run it in-process).
    pub async fn run(
        id: &Identity,
        pass: Pass,
        site: SiteKey,
        relays: Relays,
        port: u16,
        data: &Path,
        rules: Rules,
    ) -> Result<Self, String> {
        let who = pass
            .check(&site, &id.id(), now())
            .map_err(|e| format!("The keeper's pass: {e}"))?;
        if who.user != KEEPER_USER {
            return Err(format!(
                "The keeper's pass is for the account {:?}; it should be {KEEPER_USER:?} (see deploy/keeper).",
                who.user
            ));
        }
        let left = who.expires.saturating_sub(now()) / 86_400;
        if left < 30 {
            tracing::warn!("the keeper's pass runs out in {left} days: make a new one");
        }
        let store = Store::open(data).map_err(|e| format!("{}: {e}", data.display()))?;
        let (hub, mut others) = Hub::bind(id, relays, port, vec![CLOUD_ALPN.to_vec()])
            .await
            .map_err(|e| e.to_string())?;
        let k = Self {
            inner: Arc::new(Inner {
                hub,
                site,
                pass,
                rules,
                store: Mutex::new(store),
                live: Mutex::new(HashMap::new()),
                joining: Mutex::new(HashSet::new()),
                claims: Mutex::new(HashMap::new()),
            }),
        };
        let me = k.clone();
        tokio::spawn(async move {
            while let Some(conn) = others.recv().await {
                if conn.alpn() == CLOUD_ALPN {
                    let me = me.clone();
                    tokio::spawn(async move {
                        if let Err(e) = me.serve(conn).await {
                            tracing::debug!("cloud connection: {e}");
                        }
                    });
                }
            }
        });
        tracing::info!("keeper {} listening", k.inner.hub.id());
        Ok(k)
    }

    /// How apps reach it.
    pub fn addr(&self) -> EndpointAddr {
        self.inner.hub.addr()
    }

    /// Whether it's in a project's session now.
    pub fn is_live(&self, project: &str) -> bool {
        self.inner.live.lock().unwrap().contains_key(project)
    }

    /// A project's meta (tests look).
    pub fn meta(&self, project: &str) -> Option<Meta> {
        self.inner.store.lock().unwrap().get(project).cloned()
    }

    pub async fn close(&self) {
        let lives: Vec<_> = self.inner.live.lock().unwrap().drain().collect();
        for (_, l) in lives {
            let _ = l.stop.send(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.inner.hub.close().await;
    }

    /// One app's connection: who they are, then their questions, one at a time.
    async fn serve(&self, conn: Connection) -> Result<(), String> {
        let (mut send, mut recv) = tokio::time::timeout(Duration::from_secs(15), conn.accept_bi())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let hi = tokio::time::timeout(Duration::from_secs(15), read_json::<Request>(&mut recv))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let who = match hi {
            Some(Request::Hi { pass, proof }) => self.admit(&pass, &proof, &conn),
            _ => Err("Say who you are first.".to_string()),
        };
        let (user, name, computer) = match who {
            Ok(w) => w,
            Err(reason) => {
                let _ = write(&mut send, &Reply::Refused { reason }).await;
                let _ = send.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
                return Ok(());
            }
        };
        write(&mut send, &Reply::Welcome { user: user.clone() })
            .await
            .map_err(|e| e.to_string())?;
        while let Ok(Some(req)) = read_json::<Request>(&mut recv).await {
            let reply = match req {
                Request::Hi { .. } => Reply::Refused {
                    reason: "Already said hello.".into(),
                },
                Request::List => Reply::Projects {
                    list: self.list(&user),
                },
                Request::Keep {
                    code,
                    name: title,
                    width,
                    height,
                } => match self.keep(&user, &name, &code, &title, width, height) {
                    Ok(project) => Reply::Kept { project },
                    Err(reason) => Reply::Refused { reason },
                },
                Request::Touch {
                    project,
                    name: title,
                    thumb,
                } => match self.touch(&user, &project, title, thumb) {
                    Ok(()) => Reply::Touched,
                    Err(reason) => Reply::Refused { reason },
                },
                Request::Open { project } => match self.open(&user, &computer, &project) {
                    Ok(o) => Reply::Opened(o),
                    Err(reason) => Reply::Refused { reason },
                },
                Request::CatchUp { project } => match self.member_of(&user, &project) {
                    Ok(()) => {
                        let reading = self.inner.store.lock().unwrap().read(&project);
                        for l in reading.lines() {
                            let r = match l {
                                Ok(body) => Reply::Snap { body },
                                Err((from, body)) => Reply::Edit { from, body },
                            };
                            write(&mut send, &r).await.map_err(|e| e.to_string())?;
                        }
                        Reply::Done
                    }
                    Err(reason) => Reply::Refused { reason },
                },
            };
            write(&mut send, &reply).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Who an app is: a good pass, and proof it holds the computer key the pass names.
    fn admit(
        &self,
        pass: &Pass,
        proof: &str,
        conn: &Connection,
    ) -> Result<(String, String, String), String> {
        let again = |e: String| format!("The cloud didn't let you in: {e} Sign in again.");
        let info = pass
            .read(&self.inner.site)
            .map_err(|e| again(e.to_string()))?;
        pass.check(&self.inner.site, &info.endpoint, now())
            .map_err(|e| again(e.to_string()))?;
        if !check_proof(&info.endpoint, proof, self.inner.hub.id(), conn.remote_id()) {
            return Err(again(
                "That sign-in pass belongs to another computer.".into(),
            ));
        }
        Ok((info.user, info.name, info.endpoint))
    }

    fn member_of(&self, user: &str, project: &str) -> Result<(), String> {
        let store = self.inner.store.lock().unwrap();
        match store.get(project) {
            Some(m) if m.is_member(user) => Ok(()),
            _ => Err("You aren't a member of that project.".into()),
        }
    }

    fn list(&self, user: &str) -> Vec<CloudProject> {
        let store = self.inner.store.lock().unwrap();
        let live = self.inner.live.lock().unwrap();
        let mut out: Vec<CloudProject> = store
            .all()
            .filter(|m| m.is_member(user))
            .map(|m| CloudProject {
                project: m.project.clone(),
                name: m.name.clone(),
                width: m.width,
                height: m.height,
                members: m.members.clone(),
                changed: m.changed,
                online: live
                    .get(&m.project)
                    .map(|l| {
                        l.members
                            .lock()
                            .unwrap()
                            .0
                            .iter()
                            .filter(|x| !x.is_keeper())
                            .map(|x| x.name.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                thumb: store.thumb(&m.project),
                bytes: m.bytes,
                full: m.full,
            })
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.changed));
        out
    }

    /// Keep a project (its invite `code`), and join its session.
    fn keep(
        &self,
        user: &str,
        name: &str,
        code: &str,
        title: &str,
        width: u32,
        height: u32,
    ) -> Result<String, String> {
        let invite = Invite::decode(code).map_err(|e| e.to_string())?;
        let project = invite.project();
        let key = data_encoding::HEXLOWER.encode(&invite.key);
        let rules = self.inner.rules;
        {
            let mut store = self.inner.store.lock().unwrap();
            match store.get(&project) {
                // (the code holds the project's secret: whoever has it was invited)
                Some(m) if m.key != key => return Err("That invite isn't this project's.".into()),
                Some(_) => {}
                None => {
                    if store.owned(user) >= rules.projects_per_account {
                        return Err(
                            "Your cloud has as many projects as it can keep: remove one first."
                                .into(),
                        );
                    }
                    let full = store.all().any(|m| m.owner == user && m.full);
                    if full || store.usage(user) >= rules.account_bytes {
                        return Err(
                            "Your cloud space is full, so this project can't be kept there.".into(),
                        );
                    }
                    let t = now();
                    store
                        .create(Meta {
                            project: project.clone(),
                            key,
                            name: clean_title(title),
                            width,
                            height,
                            owner: user.to_string(),
                            members: Vec::new(),
                            created: t,
                            changed: t,
                            epoch: 0,
                            snap: None,
                            bytes: 0,
                            full: false,
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            store
                .add_member(&project, user, name)
                .map_err(|e| e.to_string())?;
            if width > 0 && height > 0 {
                store
                    .update(&project, |m| {
                        m.width = width;
                        m.height = height;
                        if !title.trim().is_empty() {
                            m.name = clean_title(title);
                        }
                    })
                    .map_err(|e| e.to_string())?;
            }
        }
        self.inner.claims.lock().unwrap().remove(&project);
        if !self.is_live(&project) {
            let me = self.clone();
            tokio::spawn(async move { me.join(invite).await });
        }
        Ok(project)
    }

    fn touch(
        &self,
        user: &str,
        project: &str,
        title: Option<String>,
        thumb: Option<String>,
    ) -> Result<(), String> {
        self.member_of(user, project)?;
        let mut store = self.inner.store.lock().unwrap();
        if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
            store
                .update(project, |m| m.name = clean_title(&t))
                .map_err(|e| e.to_string())?;
        }
        if let Some(b64) = thumb {
            // a small picture only (base64 letters)
            if b64.len() > 256 << 10
                || !b64
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
            {
                return Err("That picture is too big.".into());
            }
            store.put_thumb(project, &b64).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn open(&self, user: &str, computer: &str, project: &str) -> Result<Opened, String> {
        self.member_of(user, project)?;
        let m = self.meta(project).ok_or("That project isn't kept.")?;
        let key: [u8; 32] = data_encoding::HEXLOWER
            .decode(m.key.as_bytes())
            .ok()
            .and_then(|k| k.try_into().ok())
            .ok_or("That project's secret is damaged.")?;
        let me = self.addr();
        let host = self
            .inner
            .live
            .lock()
            .unwrap()
            .get(project)
            .map(|l| l.members.lock().unwrap().1.clone());
        let (state, addrs) = match host {
            Some(Some(h)) => (OpenState::Live, vec![me, h]),
            Some(None) => (OpenState::Live, vec![me]),
            None => {
                let mut claims = self.inner.claims.lock().unwrap();
                let t = Instant::now();
                claims.retain(|_, (_, until)| *until > t);
                match claims.get(project) {
                    Some((who, _)) if who != computer => (OpenState::Starting, vec![me]),
                    _ => {
                        claims.insert(
                            project.to_string(),
                            (computer.to_string(), t + self.inner.rules.claim),
                        );
                        (OpenState::Idle, vec![me])
                    }
                }
            }
        };
        Ok(Opened {
            project: project.to_string(),
            name: m.name,
            width: m.width,
            height: m.height,
            code: Invite::new(key, addrs).encode(),
            state,
        })
    }

    /// Join a project's session and stay until it ends.
    async fn join(self, invite: Invite) {
        let project = invite.project();
        if !self.inner.joining.lock().unwrap().insert(project.clone()) || self.is_live(&project) {
            return;
        }
        let joined = self
            .inner
            .hub
            .keep(&invite, &self.inner.pass, &self.inner.site)
            .await;
        let s = match joined {
            Ok(s) => s,
            Err(e) => {
                tracing::info!("couldn't join {project}: {e}");
                self.inner.joining.lock().unwrap().remove(&project);
                return;
            }
        };
        let members = Arc::new(Mutex::new((s.members(), s.host_addr())));
        let (stop, stopped) = mpsc::unbounded_channel();
        self.inner.live.lock().unwrap().insert(
            project.clone(),
            Live {
                members: members.clone(),
                stop,
            },
        );
        self.inner.joining.lock().unwrap().remove(&project);
        tracing::info!("in {project}'s session");
        self.pump(project.clone(), s, members, stopped).await;
        self.inner.live.lock().unwrap().remove(&project);
        tracing::info!("{project}'s session ended");
    }

    /// In a session: every edit kept, members noted, snapshots asked for and kept.
    async fn pump(
        &self,
        project: String,
        mut s: Session,
        members: Arc<Mutex<(Vec<Member>, Option<EndpointAddr>)>>,
        mut stopped: mpsc::UnboundedReceiver<()>,
    ) {
        let rules = self.inner.rules;
        let epoch = self
            .inner
            .store
            .lock()
            .unwrap()
            .new_epoch(&project)
            .unwrap_or(0);
        for m in s.members() {
            self.note_member(&project, &m);
        }
        let mut snap: Option<(String, u64, SnapWriter)> = None;
        let mut asked: Option<Instant> = None;
        let mut last_snap = Instant::now();
        let mut unsnapped = 0u64;
        let mut dirty = false;
        // a snapshot first: the keeper's copy is whole from here on
        ask_snapshot(&s, &mut asked);
        let mut tick = tokio::time::interval(Duration::from_secs(5).min(rules.compact_every));
        loop {
            tokio::select! {
                _ = stopped.recv() => break,
                _ = tick.tick() => {
                    if dirty {
                        let _ = self.inner.store.lock().unwrap().flush(&project);
                        dirty = false;
                    }
                    // a snapshot asked for that never came: ask again
                    if asked.is_some_and(|t| t.elapsed() > Duration::from_secs(120)) {
                        asked = None;
                        snap = None;
                    }
                    if unsnapped > 0 && last_snap.elapsed() >= rules.compact_every {
                        ask_snapshot(&s, &mut asked);
                    }
                }
                ev = s.next_event() => match ev {
                    Some(Event::Edit { seq, from, body }) => {
                        let log = self.inner.store.lock().unwrap().append(
                            &project, epoch, seq, &from, &body, rules.account_bytes, now(),
                        );
                        dirty = true;
                        unsnapped += 1;
                        match log {
                            Ok(Some(bytes)) if bytes >= rules.compact_after_bytes => {
                                ask_snapshot(&s, &mut asked)
                            }
                            // out of space: a snapshot may make room
                            Ok(None) => ask_snapshot(&s, &mut asked),
                            Err(e) => tracing::warn!("{project}: couldn't keep an edit: {e}"),
                            _ => {}
                        }
                    }
                    Some(Event::Direct { from, body }) => {
                        let k = body.get("k").and_then(Value::as_str).unwrap_or("");
                        match k {
                            "snap.begin" => {
                                let seq = body.get("seq").and_then(Value::as_u64).unwrap_or(0);
                                let w = self.inner.store.lock().unwrap().snap_writer(&project);
                                if let Ok(mut w) = w && w.push(&body).is_ok() {
                                    snap = Some((from, seq, w));
                                }
                            }
                            "snap.tiles" | "snap.sound" | "snap.end" => {
                                let mine = snap.as_ref().is_some_and(|(f, _, _)| *f == from);
                                if !mine {
                                    continue;
                                }
                                let (_, seq, w) = snap.as_mut().expect("checked");
                                if w.push(&body).is_err() || w.bytes() > rules.account_bytes {
                                    snap = None;
                                    continue;
                                }
                                if k == "snap.end" {
                                    let (_, seq, w) = snap.take().expect("checked");
                                    let kept = self.inner.store.lock().unwrap().commit_snap(
                                        &project, w, epoch, seq, rules.account_bytes, now(),
                                    );
                                    asked = None;
                                    if matches!(kept, Ok(true)) {
                                        last_snap = Instant::now();
                                        unsnapped = 0;
                                        tracing::debug!("{project}: snapshot at {epoch}/{seq}");
                                    }
                                } else {
                                    let _ = seq;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Event::Joined(m)) => {
                        self.note_member(&project, &m);
                        *members.lock().unwrap() = (s.members(), s.host_addr());
                    }
                    Some(Event::Left { .. } | Event::HostChanged { .. }) => {
                        *members.lock().unwrap() = (s.members(), s.host_addr());
                    }
                    Some(Event::Presence { .. }) => {}
                    Some(Event::Closed { .. }) | None => break,
                },
            }
        }
        let _ = self.inner.store.lock().unwrap().flush(&project);
        s.close().await;
    }

    /// Someone in the session is a member (the keeper itself isn't).
    fn note_member(&self, project: &str, m: &Member) {
        if m.is_keeper() {
            return;
        }
        let _ = self
            .inner
            .store
            .lock()
            .unwrap()
            .add_member(project, &m.user, &m.name);
    }
}

/// Ask the session's host for a snapshot (unless one is on its way).
fn ask_snapshot(s: &Session, asked: &mut Option<Instant>) {
    if asked.is_some() {
        return;
    }
    if let Some(host) = s.host_addr() {
        let _ = s.direct(&host.id.to_string(), json!({ "k": "snap.req" }));
        *asked = Some(Instant::now());
    }
}

/// 1–80 visible characters.
fn clean_title(raw: &str) -> String {
    let t: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(80)
        .collect();
    if t.is_empty() {
        "Shared project".into()
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deployed_settings_read() {
        let c: Config = toml::from_str(include_str!("../../../deploy/keeper/keeper.toml")).unwrap();
        assert_eq!(c.port, 7843);
        assert_eq!(
            c.relays(),
            Relays::Custom(vec!["https://relay.example.com".into()])
        );
        let r = Rules::from(&c.limits);
        assert_eq!(r.account_bytes, 1024 << 20);
        assert_eq!(r.compact_every, Duration::from_secs(30 * 60));
        // a mistyped setting is an error, not ignored
        assert!(
            toml::from_str::<Config>(
                "data = \"x\"\nkey = \"k\"\npass = \"p\"\nsite_key = \"s\"\nprot = 1"
            )
            .is_err()
        );
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("  Walk\u{7} cycle "), "Walk cycle");
        assert_eq!(clean_title(""), "Shared project");
    }
}
