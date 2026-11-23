//! The keeper in-process, with apps on the same computer (no relay), as the share crate's
//! own tests run: members drawing, leaving, and catching up from the cloud.

use std::path::PathBuf;
use std::time::Duration;

use pencilpilot_keeper::{Keeper, Rules};
use pencilpilot_share::cloud::{Cloud, KEEPER_USER, OpenState};
use pencilpilot_share::{
    Event, Identity, Invite, Pass, PassInfo, Relays, SecretKey, Session, SiteKey, now,
};
use serde_json::{Value, json};

fn site_secret() -> SecretKey {
    SecretKey::from_bytes(&[7; 32])
}

fn site() -> SiteKey {
    site_secret().public()
}

fn pass_for(id: &Identity, user: &str, name: &str) -> Pass {
    Pass::sign(
        &site_secret(),
        &PassInfo {
            user: user.into(),
            name: name.into(),
            endpoint: id.id(),
            issued: now() - 10,
            expires: now() + 3600,
        },
    )
}

/// A computer signed in as `name` (account `u_<name>`).
fn person(name: &str) -> (Identity, Pass) {
    let id = Identity::generate();
    let p = pass_for(&id, &format!("u_{}", name.to_lowercase()), name);
    (id, p)
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pp-keeper-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn rules() -> Rules {
    Rules {
        account_bytes: 64 << 20,
        projects_per_account: 10,
        compact_after_bytes: 32 << 20,
        compact_every: Duration::from_secs(1800),
        claim: Duration::from_secs(45),
    }
}

async fn keeper(name: &str, rules: Rules) -> (Keeper, PathBuf) {
    let id = Identity::generate();
    let pass = pass_for(&id, KEEPER_USER, "Cloud");
    let dir = temp(name);
    let k = Keeper::run(&id, pass, site(), Relays::Off, 0, &dir, rules)
        .await
        .unwrap();
    (k, dir)
}

/// An app in a session: it applies edits in order and answers anyone asking for the
/// project with a snapshot (every edit so far), as the real app's `Together` does.
struct App {
    s: Session,
    id: Identity,
    pass: Pass,
    /// the project: every edit applied, in order
    applied: Vec<Value>,
    seq: u64,
}

impl App {
    fn new(s: Session, id: Identity, pass: Pass) -> Self {
        Self {
            s,
            id,
            pass,
            applied: Vec::new(),
            seq: 0,
        }
    }

    /// Take one event in (if one comes within `wait`).
    async fn step(&mut self, wait: Duration) -> Option<Event> {
        let e = tokio::time::timeout(wait, self.s.next_event())
            .await
            .ok()??;
        match &e {
            Event::Edit { seq, body, .. } => {
                self.applied.push(body.clone());
                self.seq = *seq;
            }
            Event::Direct { from, body } if body["k"] == "snap.req" => {
                let begin = json!({ "k": "snap.begin", "seq": self.seq, "edits": self.applied });
                self.s.direct(from, begin).unwrap();
                self.s.direct(from, json!({ "k": "snap.end" })).unwrap();
            }
            _ => {}
        }
        Some(e)
    }

    /// Keep taking events in until `done` (or fail after a while).
    async fn until(&mut self, mut done: impl FnMut(&Self) -> bool) {
        let end = tokio::time::Instant::now() + Duration::from_secs(30);
        while !done(self) {
            assert!(tokio::time::Instant::now() < end, "timed out");
            self.step(Duration::from_millis(100)).await;
        }
    }

    async fn draw(&mut self, v: Value) {
        self.s.edit(v.clone()).unwrap();
        self.until(|a| a.applied.contains(&v)).await;
    }

    async fn cloud(&self, k: &Keeper) -> Cloud {
        Cloud::connect(k.addr(), &self.id, &self.pass, Relays::Off)
            .await
            .unwrap()
    }
}

/// The project as the cloud hands it over: the snapshot's edits, then the edits since.
fn caught_up(c: &pencilpilot_share::cloud::CatchUp) -> Vec<Value> {
    let mut out: Vec<Value> = c
        .snap
        .iter()
        .filter(|m| m["k"] == "snap.begin")
        .flat_map(|m| m["edits"].as_array().cloned().unwrap_or_default())
        .collect();
    out.extend(c.edits.iter().map(|(_, b)| b.clone()));
    out
}

async fn wait_for(mut f: impl FnMut() -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(tokio::time::Instant::now() < end, "timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Run app `a` (answering snapshot asks) until `f` holds.
async fn pump(a: &mut App, mut f: impl FnMut() -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(tokio::time::Instant::now() < end, "timed out");
        a.step(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_catches_up_after_everyone_left_then_the_cloud_compacts() {
    let (k, dir) = keeper("catchup", rules()).await;
    // Ana hosts and has the cloud keep the project
    let (ana_id, ana_pass) = person("Ana");
    let s = Session::host(&ana_id, &ana_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let code = s.invite().encode();
    let project = s.invite().project();
    let mut ana = App::new(s, ana_id, ana_pass);
    let mut c = ana.cloud(&k).await;
    assert_eq!(
        c.keep(&code, "Walk cycle", 1920, 1080).await.unwrap(),
        project
    );
    c.close().await;
    // it joins as a member that never hosts, and gets a first snapshot from Ana
    pump(&mut ana, || {
        k.meta(&project).is_some_and(|m| m.snap.is_some())
    })
    .await;
    assert!(k.is_live(&project));
    let keeper_member = ana.s.members().into_iter().find(|m| m.is_keeper()).unwrap();
    assert_eq!(keeper_member.name, "Cloud");
    assert!(ana.s.is_host());

    // Bo joins; both draw
    let (bo_id, bo_pass) = person("Bo");
    let invite = Invite::decode(&code).unwrap();
    let bo_s = Session::join(&bo_id, &invite, &bo_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let mut bo = App::new(bo_s, bo_id, bo_pass);
    ana.draw(json!("a1")).await;
    bo.draw(json!("b1")).await;
    ana.draw(json!("a2")).await;
    bo.until(|b| b.applied.len() == 3).await;
    let drawn = ana.applied.clone();
    assert_eq!(drawn, [json!("a1"), json!("b1"), json!("a2")]);

    // everyone leaves; the keeper's session ends (it doesn't take over)
    let (bo_id, bo_pass) = (bo.id, bo.pass);
    bo.s.close().await;
    let (ana_id, ana_pass) = (ana.id, ana.pass);
    ana.s.close().await;
    wait_for(|| !k.is_live(&project)).await;

    // Bo is a member now (he was in the session): he sees it in his list
    let mut c = Cloud::connect(k.addr(), &bo_id, &bo_pass, Relays::Off)
        .await
        .unwrap();
    let list = c.list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Walk cycle");
    assert_eq!((list[0].width, list[0].height), (1920, 1080));
    let names: Vec<&str> = list[0].members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Ana", "Bo"]);
    assert!(list[0].online.is_empty());
    // no one is in it: he catches up from the cloud
    let opened = c.open(&project).await.unwrap();
    assert_eq!(opened.state, OpenState::Idle);
    let got = c.catch_up(&project).await.unwrap();
    assert_eq!(caught_up(&got), drawn, "the whole project, in order");
    c.close().await;

    // Ana, opening it at the same moment, is told to meet Bo rather than host her own copy
    let mut ca = Cloud::connect(k.addr(), &ana_id, &ana_pass, Relays::Off)
        .await
        .unwrap();
    assert_eq!(ca.open(&project).await.unwrap().state, OpenState::Starting);
    ca.close().await;

    // Bo hosts it again with what he caught up on, and lets the keeper in
    let invite = Invite::decode(&opened.code).unwrap();
    let s = Session::host_again(&bo_id, &invite, &bo_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let mut bo = App::new(s, bo_id, bo_pass);
    bo.applied = caught_up(&got);
    bo.draw(json!("b2 (after coming back)")).await;
    let mut c = bo.cloud(&k).await;
    c.keep(&bo.s.invite().encode(), "", 0, 0).await.unwrap();
    // the keeper's new snapshot replaces the whole log (compaction)
    pump(&mut bo, || {
        k.meta(&project)
            .is_some_and(|m| m.epoch == 2 && m.snap == Some((2, 1)))
    })
    .await;
    let again = c.catch_up(&project).await.unwrap();
    assert!(again.edits.is_empty(), "{:?}", again.edits);
    assert_eq!(
        caught_up(&again),
        [
            json!("a1"),
            json!("b1"),
            json!("a2"),
            json!("b2 (after coming back)")
        ]
    );
    // and now it's live: Ana is passed on to Bo through the keeper
    let mut ca = Cloud::connect(k.addr(), &ana_id, &ana_pass, Relays::Off)
        .await
        .unwrap();
    let live = ca.open(&project).await.unwrap();
    assert_eq!(live.state, OpenState::Live);
    let through_keeper = Invite::decode(&live.code).unwrap();
    assert_eq!(through_keeper.addrs[0].id, k.addr().id, "the keeper first");
    let only_keeper = Invite::new(through_keeper.key, vec![k.addr()]);
    let ana_s = Session::join(&ana_id, &only_keeper, &ana_pass, &site(), Relays::Off)
        .await
        .unwrap();
    assert_eq!(ana_s.members()[0].id, bo.s.me().id, "Bo hosts");
    bo.until(|b| b.s.members().len() == 3).await;
    let list = c.list().await.unwrap();
    assert_eq!(list[0].online, ["Bo", "Ana"]);
    for cl in [c, ca] {
        cl.close().await;
    }
    ana_s.close().await;
    bo.s.close().await;
    k.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_members_are_served() {
    let (k, dir) = keeper("members", rules()).await;
    let (ana_id, ana_pass) = person("Ana");
    let s = Session::host(&ana_id, &ana_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let project = s.invite().project();
    let mut ana = App::new(s, ana_id, ana_pass);
    let mut c = ana.cloud(&k).await;
    c.keep(&ana.s.invite().encode(), "Secret plans", 64, 64)
        .await
        .unwrap();
    pump(&mut ana, || k.is_live(&project)).await;

    // Eve is signed in, but has never been in it
    let (eve_id, eve_pass) = person("Eve");
    let mut e = Cloud::connect(k.addr(), &eve_id, &eve_pass, Relays::Off)
        .await
        .unwrap();
    assert!(e.list().await.unwrap().is_empty());
    let no = e.open(&project).await.unwrap_err();
    assert!(no.0.contains("aren't a member"), "{no}");
    assert!(e.catch_up(&project).await.is_err());
    assert!(e.touch(&project, Some("Mine".into()), None).await.is_err());
    // an invite she made up for it isn't this project's (another secret, another project)
    let fake = Invite::new([9; 32], ana.s.invite().addrs.clone()).encode();
    let kept = e.keep(&fake, "x", 1, 1).await.unwrap();
    assert_ne!(kept, project);
    e.close().await;

    // someone else's pass, or one the website didn't sign, gets nowhere
    let (other_id, _) = person("Mallory");
    let refused = Cloud::connect(k.addr(), &other_id, &ana.pass, Relays::Off).await;
    assert!(refused.err().unwrap().0.contains("another computer"));
    let forged = Pass::sign(
        &SecretKey::generate(),
        &PassInfo {
            user: "u_ana".into(),
            name: "Ana".into(),
            endpoint: other_id.id(),
            issued: now(),
            expires: now() + 60,
        },
    );
    let refused = Cloud::connect(k.addr(), &other_id, &forged, Relays::Off).await;
    assert!(refused.err().unwrap().0.contains("genuine"));

    // a member renames it and gives it a picture
    c.touch(&project, Some("Plans".into()), Some("iVBORw0KGgo=".into()))
        .await
        .unwrap();
    let l = c.list().await.unwrap();
    assert_eq!(l[0].name, "Plans");
    assert_eq!(l[0].thumb.as_deref(), Some("iVBORw0KGgo="));
    c.close().await;
    ana.s.close().await;
    k.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_growing_log_is_compacted_and_space_runs_out() {
    let mut r = rules();
    r.compact_after_bytes = 600;
    r.account_bytes = 6_000;
    let (k, dir) = keeper("space", r).await;
    let (ana_id, ana_pass) = person("Ana");
    let s = Session::host(&ana_id, &ana_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let project = s.invite().project();
    let mut ana = App::new(s, ana_id, ana_pass);
    let mut c = ana.cloud(&k).await;
    c.keep(&ana.s.invite().encode(), "Big", 64, 64)
        .await
        .unwrap();
    pump(&mut ana, || {
        k.meta(&project).is_some_and(|m| m.snap.is_some())
    })
    .await;

    // edits of ~200 bytes: past 600 bytes of log, the keeper asks Ana for a snapshot
    for i in 0..5 {
        ana.draw(json!({ "i": i, "pad": "x".repeat(200) })).await;
    }
    pump(&mut ana, || {
        k.meta(&project)
            .is_some_and(|m| m.snap.is_some_and(|(_, s)| s >= 3))
    })
    .await;
    let mut caught = c.catch_up(&project).await.unwrap();
    assert!(caught.edits.len() < 5, "compacted: {}", caught.edits.len());
    assert_eq!(caught_up(&caught), ana.applied);

    // bigger and bigger snapshots: the account's 6 kB run out, and the project says so
    for i in 5..30 {
        ana.s
            .edit(json!({ "i": i, "pad": "y".repeat(400) }))
            .unwrap();
        ana.step(Duration::from_millis(20)).await;
    }
    pump(&mut ana, || k.meta(&project).is_some_and(|m| m.full)).await;
    let m = k.meta(&project).unwrap();
    assert!(m.bytes <= 6_000, "{}", m.bytes);
    assert!(c.list().await.unwrap()[0].full);
    // no new project is taken on while it's full
    let (_, other) = (
        0,
        Invite::new([3; 32], ana.s.invite().addrs.clone()).encode(),
    );
    let no = c.keep(&other, "Another", 64, 64).await.unwrap_err();
    assert!(no.0.contains("full"), "{no}");
    caught = c.catch_up(&project).await.unwrap();
    assert!(!caught_up(&caught).is_empty());
    c.close().await;
    ana.s.close().await;
    k.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn when_the_host_leaves_a_person_takes_over_and_the_keeper_follows() {
    let (k, dir) = keeper("handover", rules()).await;
    let (ana_id, ana_pass) = person("Ana");
    let s = Session::host(&ana_id, &ana_pass, &site(), Relays::Off)
        .await
        .unwrap();
    let code = s.invite().encode();
    let project = s.invite().project();
    let mut ana = App::new(s, ana_id, ana_pass);
    let mut c = ana.cloud(&k).await;
    c.keep(&code, "Hand-over", 64, 64).await.unwrap();
    c.close().await;
    pump(&mut ana, || {
        k.meta(&project).is_some_and(|m| m.snap.is_some())
    })
    .await;
    // Bo joins after the keeper: it's in longer, but it's never asked to host
    let (bo_id, bo_pass) = person("Bo");
    let bo_s = Session::join(
        &bo_id,
        &Invite::decode(&code).unwrap(),
        &bo_pass,
        &site(),
        Relays::Off,
    )
    .await
    .unwrap();
    let mut bo = App::new(bo_s, bo_id, bo_pass);
    ana.draw(json!("before")).await;
    ana.s.close().await;
    // Bo takes over, the keeper follows him, and keeps what he draws
    bo.until(|b| b.s.is_host()).await;
    bo.draw(json!("after")).await;
    wait_for(|| {
        k.is_live(&project)
            && std::fs::read_to_string(dir.join("projects").join(&project).join("log.jsonl"))
                .is_ok_and(|l| l.contains("after"))
    })
    .await;
    bo.s.close().await;
    k.close().await;
    let _ = std::fs::remove_dir_all(dir);
}
