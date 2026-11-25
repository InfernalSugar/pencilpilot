//! Every vendored MyPaint brush must load and paint without panicking.

use std::fs;
use std::path::Path;

use brush::Painter;

#[test]
fn every_brush_in_the_pack_loads_and_paints() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/static/brushes");
    let mut count = 0;
    let mut painted = 0;
    for entry in fs::read_dir(&dir).expect("brush dir exists") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "myb") {
            continue;
        }
        let json = fs::read_to_string(&path).unwrap();
        let mut brush = hokusai::myb::from_str(&json)
            .unwrap_or_else(|e| panic!("{} failed to parse: {e}", path.display()));
        brush::set_brush_rgb(&mut brush, 20, 20, 30);

        let mut p = Painter::new(256, 256);
        p.begin_stroke();
        for i in 0..60 {
            let t = i as f32 / 59.0;
            p.stroke_to(
                &brush,
                30.0 + t * 190.0,
                128.0 + (t * 6.0).sin() * 40.0,
                0.3 + 0.6 * t,
                0.0,
                0.0,
                0.016,
            );
        }
        p.end_stroke(&brush);
        if p.flush().is_some() {
            painted += 1;
        }
        count += 1;
    }
    assert_eq!(
        count, 197,
        "expected the full mypaint-brushes v2.0.2 pack and Hair strands"
    );
    // Smudge/blend/eraser brushes leave nothing on a blank canvas; the rest must paint.
    assert!(
        painted >= 150,
        "only {painted}/{count} brushes painted anything"
    );
}
