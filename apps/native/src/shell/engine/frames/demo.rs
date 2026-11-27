//! The demo cut's drawings: a ball bouncing across the cut (Clean), its tie-down and
//! rough passes, and a floor (the static Background). Painted with the brushes, so the
//! demo plays, onion-skins and shows thumbnails like a real cut.

use super::super::Engine;
use super::sheet::Track;
use crate::state::State;

/// Paint each demo drawing on its layer (`tracks` top first, with their painter ids).
pub fn paint(e: &mut Engine, s: &State, tracks: &[Track]) {
    let (w, h) = (e.painter.width() as f32, e.painter.height() as f32);
    let mut st = s.clone();
    st.tool = crate::state::Tool::Brush;
    let floor = 0.82 * h;
    // where the ball is in each of a track's drawings (k of n): along the cut, bouncing
    let ball = |k: usize, n: usize| {
        let u = (k as f32 + 0.5) / n as f32;
        let x = (0.12 + 0.76 * u) * w;
        let bounce = (u * 2.0 * std::f32::consts::PI).sin().abs();
        let r = 0.07 * h;
        (x, floor - r - bounce * 0.45 * h, r)
    };
    let layer_brush = |name: &str| -> (&'static str, u32, f32) {
        match name {
            "Clean" => ("Liner", 0x16161A, (h / 220.0).max(1.5)),
            "Tie-down" => ("2B pencil", 0x3D6BFF, (h / 160.0).max(1.5)),
            _ => ("Rough pencil", 0x8C7B9E, (h / 130.0).max(1.5)),
        }
    };
    for (li, t) in tracks.iter().enumerate() {
        let name = s.layers.get(li).map_or("", |l| l.name.as_str());
        let ds = t.drawings();
        for (k, &d) in ds.iter().enumerate() {
            if !e.painter.show_drawings(&[t.layer, d]) || !e.painter.set_active_layer(t.layer) {
                continue;
            }
            let mut lines: Vec<Vec<(f32, f32)>> = Vec::new();
            if t.kind == 'S' {
                // the floor, and a hill behind it
                lines.push(vec![(0.04 * w, floor), (0.96 * w, floor)]);
                lines.push(
                    (0..=48)
                        .map(|i| {
                            let u = i as f32 / 48.0;
                            let x = (0.55 + 0.4 * u) * w;
                            (x, floor - (u * std::f32::consts::PI).sin() * 0.18 * h)
                        })
                        .collect(),
                );
            } else {
                let (cx, cy, r) = ball(k, ds.len());
                let rough = name != "Clean";
                let r = if rough { r * 1.12 } else { r };
                let turns = if rough { 1.3 } else { 1.0 };
                lines.push(
                    (0..=64)
                        .map(|i| {
                            let a = i as f32 / 64.0 * turns * std::f32::consts::TAU;
                            let wob = if rough {
                                1.0 + 0.04 * (a * 3.0).sin()
                            } else {
                                1.0
                            };
                            (cx + a.cos() * r * wob, cy + a.sin() * r * wob)
                        })
                        .collect(),
                );
                if name == "Clean" {
                    // a highlight
                    lines.push(
                        (0..=12)
                            .map(|i| {
                                let a = 3.6 + i as f32 / 12.0 * 1.2;
                                (cx + a.cos() * r * 0.6, cy + a.sin() * r * 0.6)
                            })
                            .collect(),
                    );
                }
            }
            let (brush, rgb, size) = if t.kind == 'S' {
                ("Liner", 0x8C8B92, (h / 260.0).max(1.0))
            } else {
                layer_brush(name)
            };
            st.brush = st.brushes.iter().position(|b| b == brush).unwrap_or(0);
            st.brush_size = size;
            st.colour = pencilpilot_ui::theme::hex(rgb);
            for line in lines {
                // samples a few pixels apart, as a hand would give them
                let mut ms = 0;
                e.down(&st, line[0].0, line[0].1, 0.8, (0.0, 0.0), ms);
                for pair in line.windows(2) {
                    let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                    let steps = ((x1 - x0).hypot(y1 - y0) / 4.0).ceil().max(1.0) as u32;
                    for k in 1..=steps {
                        let u = k as f32 / steps as f32;
                        ms += 4;
                        e.moved(x0 + (x1 - x0) * u, y0 + (y1 - y0) * u, 0.8, (0.0, 0.0), ms);
                    }
                }
                e.up();
            }
        }
    }
}
