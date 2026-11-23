//! How many people can draw together: N members on this machine (direct, no relay), each
//! sending strokes and cursors like the app does, measuring how long every stroke takes to
//! reach everyone and how much the host has to send.
//!
//! cargo run --release -p pencilpilot-share --example stress -- [sizes] [secs] [stroke KB] [strokes/s]
//! e.g. cargo run --release -p pencilpilot-share --example stress -- 2,4,8,16 20 40 1

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pencilpilot_share::{Event, Identity, Invite, Relays, Session};
use rand::Rng;
use serde_json::json;

struct Stats {
    /// ms from a stroke being sent to it arriving, one per (stroke, receiver)
    lat: Vec<f64>,
    received: usize,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sizes: Vec<usize> = args
        .get(1)
        .map(|s| s.split(',').map(|n| n.parse().unwrap()).collect())
        .unwrap_or(vec![2, 4, 8, 16]);
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let kb: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let rate: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    println!("{secs} s each, every member drawing {rate} stroke/s of {kb} KB, cursors at 30/s\n");
    println!(
        "{:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>12} {:>12}",
        "people", "strokes", "arrived", "p50 ms", "p95 ms", "max ms", "host up", "member down"
    );
    for n in sizes {
        run(n, secs, kb, rate).await;
    }
}

async fn run(n: usize, secs: u64, kb: usize, rate: f64) {
    let start = Instant::now();
    let host = Session::host(&Identity::generate(), "Host", Relays::Off)
        .await
        .unwrap();
    let invite = Invite::decode(&host.invite().encode()).unwrap();
    let mut sessions = vec![host];
    for i in 1..n {
        sessions.push(
            Session::join(
                &Identity::generate(),
                &invite,
                &format!("M{i}"),
                Relays::Off,
            )
            .await
            .unwrap(),
        );
    }
    // let the joins settle
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stats = Arc::new(Mutex::new(Stats {
        lat: Vec::new(),
        received: 0,
    }));
    let sent = Arc::new(Mutex::new(0usize));
    let t0 = Instant::now();
    let stop = t0 + Duration::from_secs(secs);
    let drain = stop + Duration::from_secs(10);
    let mut tasks = Vec::new();
    for (i, mut s) in sessions.into_iter().enumerate() {
        let (stats, sent) = (stats.clone(), sent.clone());
        tasks.push(tokio::spawn(async move {
            let me = s.me().id.clone();
            let gap = Duration::from_secs_f64(1.0 / rate);
            // spread the members out over a stroke's time
            let mut next_stroke = t0 + gap.mul_f64(i as f64 / n as f64);
            let mut next_cursor = t0;
            loop {
                let now = Instant::now();
                if now >= drain {
                    break;
                }
                if now < stop && now >= next_stroke {
                    let pixels: String = {
                        let mut r = rand::rng();
                        (0..kb * 1024)
                            .map(|_| (b'A' + r.random_range(0..26u8)) as char)
                            .collect()
                    };
                    let at = t0.elapsed().as_secs_f64() * 1000.0;
                    s.edit(json!({"k": "tiles", "at": at, "px": pixels}))
                        .unwrap();
                    *sent.lock().unwrap() += 1;
                    next_stroke += gap;
                }
                if now < stop && now >= next_cursor {
                    s.presence(json!({"x": i, "y": 1}));
                    next_cursor += Duration::from_millis(33);
                }
                let wake = next_stroke.min(next_cursor).min(drain);
                let wait = wake.saturating_duration_since(Instant::now());
                match tokio::time::timeout(wait, s.next_event()).await {
                    Ok(Some(Event::Edit { from, body, .. })) => {
                        if from != me {
                            let at = body["at"].as_f64().unwrap();
                            let ms = t0.elapsed().as_secs_f64() * 1000.0 - at;
                            let mut st = stats.lock().unwrap();
                            st.lat.push(ms);
                            st.received += 1;
                        }
                    }
                    Ok(Some(_)) | Err(_) => {}
                    Ok(None) => break,
                }
            }
            drop(s);
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let sent = *sent.lock().unwrap();
    let mut st = stats.lock().unwrap();
    st.lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| {
        st.lat
            .get(((st.lat.len() as f64 * p) as usize).min(st.lat.len().saturating_sub(1)))
            .copied()
            .unwrap_or(f64::NAN)
    };
    let expected = sent * (n - 1);
    // every stroke goes to the host, which passes it to the n-1 others (the author's own
    // comes back numbered too): the host sends each stroke n-1 times (n if it isn't theirs)
    let per_sec = sent as f64 / secs as f64;
    let host_up = per_sec * kb as f64 * (n - 1) as f64 / 1024.0 * 8.0;
    let member_down = per_sec * kb as f64 / 1024.0 * 8.0;
    println!(
        "{:>7} {:>9} {:>8.1}% {:>9.0} {:>9.0} {:>9.0} {:>8.1} Mb/s {:>8.1} Mb/s",
        n,
        sent,
        100.0 * st.received as f64 / expected.max(1) as f64,
        pct(0.5),
        pct(0.95),
        st.lat.last().copied().unwrap_or(f64::NAN),
        host_up,
        member_down,
    );
    let _ = start;
}
