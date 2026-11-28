//! Two apps drawing together over a real session (`pencilpilot-share`, no relay: on this
//! computer only): the host's drawing reaches the joiner, the joiner's own layer reaches
//! the host, and access asked for and given works, with the session's own member ids.

mod common;

use std::time::Duration;

use common::FakeCanvas;
use pencilpilot_share::{Event, Identity, Invite, Relays, Session};
use pencilpilot_together::{Out, Together};

struct App {
    s: Session,
    t: Together,
    c: FakeCanvas,
}

impl App {
    /// Send what drawing together queued.
    fn send(&mut self) {
        for o in self.t.take_out() {
            match o {
                Out::Edit(b) => self.s.edit(b).unwrap(),
                Out::Direct { to, body } => self.s.direct(&to, body).unwrap(),
            }
        }
    }

    /// Take in what the session has for this app.
    fn take(&mut self) -> bool {
        let mut any = false;
        while let Some(e) = self.s.try_event() {
            any = true;
            match e {
                Event::Edit { seq, from, body } => self.t.on_edit(&mut self.c, seq, &from, &body),
                Event::Direct { from, body } => self.t.on_direct(&mut self.c, &from, &body),
                _ => {}
            }
            self.send();
        }
        any
    }
}

/// Pass messages both ways until `done` (or give up after 20 s).
async fn until(apps: &mut [&mut App], done: impl Fn(&[&mut App]) -> bool) {
    let start = std::time::Instant::now();
    loop {
        for a in apps.iter_mut() {
            a.send();
            a.take();
        }
        if done(apps) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the session didn't get there"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_apps_draw_together_over_a_real_session() {
    let host = Session::host(&Identity::generate(), "Ana", Relays::Off)
        .await
        .unwrap();
    let code = host.invite().encode();
    let host_id = host.me().id.clone();
    let mut hc = FakeCanvas::new(&["Sky"]);
    hc.paint(1, 0, 0, 50);
    let mut a = App {
        t: Together::new(&host_id, "Ana"),
        s: host,
        c: hc,
    };
    a.t.start_host(&mut a.c);
    a.send();

    let joiner = Session::join(
        &Identity::generate(),
        &Invite::decode(&code).unwrap(),
        "Bo",
        Relays::Off,
    )
    .await
    .unwrap();
    let bo_id = joiner.me().id.clone();
    let mut b = App {
        t: Together::new(&bo_id, "Bo"),
        s: joiner,
        c: FakeCanvas::new(&[]),
    };
    b.t.start_join(&mut b.c, &host_id);
    b.send();

    // Bo gets the project, and a layer of his own that Ana sees too
    until(&mut [&mut a, &mut b], |x| {
        x[1].t.synced() && x[0].c.has("Bo’s layer")
    })
    .await;
    let sky = b.c.by_name("Sky").id;
    assert_eq!(b.c.pixel(sky, 0, 0), Some(50));
    assert!(b.c.by_name("Sky").locked);

    // Bo draws on his layer: Ana sees it
    let mine = b.c.by_name("Bo’s layer").id;
    b.c.paint(mine, 3, 3, 77);
    b.t.timeline_changed(&mut b.c);
    b.t.local_changed(&mut b.c);
    until(&mut [&mut a, &mut b], |x| {
        let l = x[0].c.by_name("Bo’s layer").id;
        x[0].c.pixel(l, 3, 3) == Some(77)
    })
    .await;

    // he asks to draw on the sky; she lets him; his stroke there reaches her
    let sky_lid = b.t.lid_of(sky).unwrap();
    b.t.ask(&sky_lid);
    until(&mut [&mut a, &mut b], |x| !x[0].t.shared.asks.is_empty()).await;
    assert_eq!(a.t.shared.asks[0].from, bo_id);
    a.t.grant(&sky_lid, &bo_id, false);
    until(&mut [&mut a, &mut b], |x| !x[1].c.by_name("Sky").locked).await;
    b.c.paint(sky, 1, 1, 33);
    b.t.local_changed(&mut b.c);
    until(&mut [&mut a, &mut b], |x| {
        let s = x[0].c.by_name("Sky").id;
        x[0].c.pixel(s, 1, 1) == Some(33)
    })
    .await;
    assert_eq!(a.c.names(), b.c.names());

    b.s.close().await;
    a.s.close().await;
}
