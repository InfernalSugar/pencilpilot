//! The frame store at real sizes: 1080p and 4K line drawings, held frames, a small
//! memory budget that sends tiles to disk, and project files.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use anim::{FrameStore, TILE, TILE_BYTES, hash, pack, unpack};
use ppstudio::Project;

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pp-anim-{name}-{}", std::process::id()))
}

/// A line drawing on a `w`×`h` canvas: a diagonal stroke that moves with `frame`, as
/// tiles (only the ones it touches).
fn drawing(w: usize, h: usize, frame: usize) -> Vec<(i32, i32, Vec<u8>)> {
    let mut tiles: std::collections::BTreeMap<(i32, i32), Vec<u8>> = Default::default();
    let steps = w.max(h) * 2;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let x = ((t * 0.8 + 0.1) * w as f32) as usize + frame * 7;
        let y = ((0.2 + 0.6 * t) * h as f32) as usize;
        for dy in 0..3 {
            for dx in 0..3 {
                let (px, py) = (x + dx, y + dy);
                if px >= w || py >= h {
                    continue;
                }
                let key = ((px / TILE) as i32, (py / TILE) as i32);
                let tile = tiles.entry(key).or_insert_with(|| vec![0; TILE_BYTES]);
                let o = ((py % TILE) * TILE + px % TILE) * 4;
                tile[o..o + 4].copy_from_slice(&[20, 20, 30, 255]);
            }
        }
    }
    tiles.into_iter().map(|((x, y), t)| (x, y, t)).collect()
}

fn put(s: &mut FrameStore, id: u32, d: &[(i32, i32, Vec<u8>)]) {
    s.put(id, d.iter().map(|(x, y, t)| (*x, *y, t.as_slice())))
        .unwrap();
}

/// The drawing's tiles back, sorted, as owned bytes.
fn got(s: &mut FrameStore, id: u32) -> Vec<(i32, i32, Vec<u8>)> {
    let mut t: Vec<_> = s
        .get(id)
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|(x, y, px)| (x, y, px.to_vec()))
        .collect();
    t.sort_by_key(|(x, y, _)| (*y, *x));
    t
}

fn sorted(mut d: Vec<(i32, i32, Vec<u8>)>) -> Vec<(i32, i32, Vec<u8>)> {
    d.sort_by_key(|(x, y, _)| (*y, *x));
    d
}

#[test]
fn a_1080p_cut_keeps_each_tile_once() {
    let mut s = FrameStore::new(usize::MAX, None).unwrap();
    // 48 frames: 24 drawings, each held for two (copies share every tile)
    for f in 0..24 {
        put(&mut s, f * 2 + 1, &drawing(1920, 1080, f as usize));
        assert!(s.copy(f * 2 + 1, f * 2 + 2));
    }
    let st = s.stats();
    assert_eq!(st.drawings, 48);
    // held frames cost nothing (and identical tiles in different frames are shared too)
    assert!(st.tiles * 2 <= st.placed, "{st:?}");
    // a line drawing is a small part of the canvas (1080p is 510 tiles a frame)
    assert!(st.tiles < 24 * 120, "{st:?}");
    assert_eq!(got(&mut s, 7), sorted(drawing(1920, 1080, 3)));
    // removing the copies frees nothing; removing both frees the tiles
    for f in 0..24 {
        s.remove(f * 2 + 2);
    }
    assert_eq!(s.stats().tiles, st.tiles);
    s.retain(&HashSet::from([1]));
    assert_eq!(s.stats().drawings, 1);
    assert!(s.stats().tiles < st.tiles / 10);
}

#[test]
fn a_4k_cut_past_the_memory_budget_goes_to_disk_and_comes_back() {
    let dir = temp_dir("4k");
    // 4 MB in memory: 256 tiles
    let budget = 256 * TILE_BYTES;
    let mut s = FrameStore::new(budget, Some(dir.clone())).unwrap();
    let frames = 40;
    let start = Instant::now();
    for f in 0..frames {
        put(&mut s, f as u32 + 1, &drawing(3840, 2160, f));
        assert!(s.stats().bytes_in_memory <= budget, "{:?}", s.stats());
    }
    let st = s.stats();
    assert!(st.bytes_on_disk > 0, "{st:?}");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count() * TILE_BYTES,
        st.bytes_on_disk
    );
    // every frame comes back exactly, from memory or disk
    for f in 0..frames {
        assert_eq!(got(&mut s, f as u32 + 1), sorted(drawing(3840, 2160, f)));
        assert!(s.stats().bytes_in_memory <= budget);
    }
    eprintln!(
        "4K × {frames}: {:?} ({} tiles, {} KB on disk)",
        start.elapsed(),
        st.tiles,
        st.bytes_on_disk / 1024
    );
    // a damaged tile on disk is noticed, not shown
    let on_disk = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&on_disk, vec![1u8; TILE_BYTES]).unwrap();
    let any_err = (1..=frames as u32).any(|id| s.get(id).is_err());
    assert!(any_err);
    drop(s);
    assert!(!dir.exists(), "the store cleans up after itself");
}

#[test]
fn drawings_cross_as_packets_and_go_into_project_files() {
    let mut s = FrameStore::new(usize::MAX, None).unwrap();
    let a = drawing(1920, 1080, 0);
    put(&mut s, 5, &a);
    put(&mut s, 6, &[]); // an empty drawing
    let packet = pack(&[(5, s.get(5).unwrap().unwrap()), (6, vec![])]);
    let back = unpack(&packet).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(
        (back[0].0, back[0].1.len(), back[1].1.len()),
        (5, a.len(), 0)
    );
    assert!(unpack(&packet[..packet.len() - 1]).is_err());

    // into a project and out again, under the same hashes
    let mut p = Project::new("test", "2026-11-21T00:00:00Z", 1920, 1080, 24);
    s.write_into(&mut p, 5, "D5", None).unwrap();
    let d = &p.manifest.drawings["D5"];
    assert_eq!(d.tiles.len(), a.len());
    assert_eq!(
        d.tiles[0].hash,
        anim::hex(&hash(&p.tiles[&d.tiles[0].hash]))
    );
    let (rgba, _) = p.drawing("D5").unwrap();
    assert!(rgba.as_chunks::<4>().0.iter().any(|px| px[3] == 255));
    let mut t = FrameStore::new(usize::MAX, None).unwrap();
    t.read_from(&p, "D5", 9).unwrap();
    assert_eq!(got(&mut t, 9), sorted(a));
    // fully transparent tiles are left out; a wrong-sized tile is refused
    let clear = vec![0u8; TILE_BYTES];
    t.put(1, [(0, 0, clear.as_slice())]).unwrap();
    assert_eq!(t.get(1).unwrap().unwrap().len(), 0);
    assert!(t.put(2, [(0, 0, &clear[..10])]).is_err());
}
