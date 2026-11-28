//! What the synced tiles take, for a big project (QA's 4K, 20-layer one): the host's and a
//! joiner's, against their pixels (what together.ts, and this crate before, kept), and how
//! long each side takes. Ignored by default (it needs the file):
//!
//!   PP_TOGETHER_PROJECT=…/layers20-4k.ppstudio cargo test --release -p pencilpilot-together \
//!     --test memory -- --ignored --nocapture

mod common;

use common::{A, B, FakeCanvas, FakeSession};
use pencilpilot_together::CanvasPort;

#[test]
#[ignore]
fn a_big_projects_tiles_kept_small() {
    let Ok(path) = std::env::var("PP_TOGETHER_PROJECT") else {
        eprintln!("PP_TOGETHER_PROJECT not set: skipped");
        return;
    };
    let mut f = ppstudio::Opened::read(std::fs::File::open(&path).unwrap()).unwrap();
    let m = f.manifest.clone();
    let names: Vec<&str> = m.layers.iter().map(|l| l.name.as_str()).collect();
    let mut hc = FakeCanvas::new(&names);
    // every frame's drawings, on their layers' tracks (one exposure a frame)
    let mut tiles = 0usize;
    for (i, l) in m.layers.iter().enumerate() {
        let id = (i + 1) as u32;
        let mut seen: Vec<&String> = Vec::new();
        let mut track = Vec::new();
        for fr in &m.timeline.frames {
            let Some(dname) = fr.cels.get(&l.id) else {
                continue;
            };
            let d = match seen.iter().position(|x| *x == dname) {
                Some(_) if seen.last() == Some(&dname) => {
                    if let Some(e) = track.last_mut() {
                        let e: &mut pencilpilot_together::timeline::Exposure = e;
                        e.length += fr.hold;
                    }
                    continue;
                }
                _ => {
                    let d = if seen.is_empty() {
                        hc.drawing_of(id)
                    } else {
                        hc.new_drawing()
                    };
                    seen.push(dname);
                    for t in &m.drawings[dname].tiles {
                        let px = f.tile(&t.hash).unwrap();
                        hc.drawings
                            .get_mut(&d)
                            .unwrap()
                            .insert((t.x as i32, t.y as i32), px);
                        tiles += 1;
                    }
                    d
                }
            };
            track.push(pencilpilot_together::timeline::Exposure {
                drawing: d,
                length: fr.hold,
            });
        }
        if !track.is_empty() {
            hc.tl.scenes[0].cuts[0].tracks.insert(id, track);
        }
    }
    hc.show_frame(0);
    let mut s = FakeSession::default();
    s.add(A, "Ana", hc);
    let t0 = std::time::Instant::now();
    s.with(A, |t, c| t.start_host(c));
    s.settle();
    let host_ms = t0.elapsed().as_millis();
    let host_bytes = s.t(A).cache_bytes();
    s.add(B, "Bo", FakeCanvas::new(&[]));
    let t0 = std::time::Instant::now();
    s.with(B, |t, c| t.start_join(c, A));
    s.settle();
    let join_ms = t0.elapsed().as_millis();
    assert!(s.t(B).synced());
    let got: usize = s.c(B).drawings.values().map(|m| m.len()).sum();
    assert_eq!(got, tiles);
    let mib = |b: usize| (b as f64 / (1 << 20) as f64 * 10.0).round() / 10.0;
    println!(
        "{{\"tiles\": {tiles}, \"pixelsMiB\": {}, \"hostCacheMiB\": {}, \"joinerCacheMiB\": {}, \"hostStartMs\": {host_ms}, \"joinMs\": {join_ms}}}",
        mib(tiles * 16384),
        mib(host_bytes),
        mib(s.t(B).cache_bytes())
    );
}
