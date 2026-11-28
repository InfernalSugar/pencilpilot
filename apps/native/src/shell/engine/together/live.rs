//! Live strokes: each stroke as it's drawn goes to the others as passing state (fast and
//! lossy, like cursors: `{"live": …}`), and theirs are drawn here with the same brush on a
//! layer of their own ("Ana (drawing now)") until their real pixels (tiles) arrive.
//!
//! The messages are the current app's (`LiveEvent` in the paint package):
//! `begin` (`k`, the stroke's number, and its style: brush, colour, size, opacity, eraser),
//! `points` (`pts`: x, y, pressure, ms since the last point, four numbers a point), `end`,
//! `cancel`.

use std::collections::HashMap;

use hokusai::BrushSetting;
use serde_json::{Value, json};

use super::super::{BRUSHES, ERASER, parsed};

/// The most points a datagram carries (four numbers each, 96 numbers: the current app's
/// pieces). A datagram must fit in one packet (about 1200 bytes; a bigger one is dropped).
pub const POINTS_PER_MESSAGE: usize = 24;

/// A brush's id on the wire: its file's name, as the current app names brushes.
fn brush_id(name: &str) -> &'static str {
    match name {
        "Rough pencil" => "pencilpilot-rough-pencil",
        "Liner" => "deevad-liner",
        "Marker" => "aotz-040-marker",
        "Airbrush" => "deevad-airbrush",
        _ => "deevad-2b-pencil",
    }
}

/// A brush by its id on the wire (unknown ones paint with the 2B pencil, as in the current
/// app).
fn brush_src(id: &str) -> &'static str {
    let name = match id {
        "pencilpilot-rough-pencil" => "Rough pencil",
        "deevad-liner" => "Liner",
        "aotz-040-marker" => "Marker",
        "deevad-airbrush" => "Airbrush",
        "deevad-large-hard-eraser" => return ERASER,
        _ => "2B pencil",
    };
    BRUSHES
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(BRUSHES[0].1, |(_, b)| *b)
}

/// The brush's own radius (log), as its file has it.
fn natural_radius(src: &'static str) -> f32 {
    parsed(src).map_or(1.0, |b| b.get(BrushSetting::Radius).base_value)
}

/// The style of a stroke starting here (the current app's `begin`): `size` is how much
/// bigger than the brush's own size it paints (its log radius plus log2 of it).
pub fn style(s: &crate::state::State) -> Value {
    let name = s.brushes.get(s.brush).map_or("", String::as_str);
    let eraser = s.tool == crate::state::Tool::Eraser;
    let src = if eraser {
        ERASER
    } else {
        brush_src(brush_id(name))
    };
    let radius = (s.brush_size.max(0.5) / 2.0).ln();
    let scale = 2f32.powf(radius - natural_radius(src));
    let [r, g, b, _] = s.colour.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    json!({
        "brushId": if eraser { "deevad-large-hard-eraser" } else { brush_id(name) },
        "color": format!("#{r:02x}{g:02x}{b:02x}"),
        "size": (scale * 1000.0).round() / 1000.0,
        "opacity": super::super::tools::stroke_opacity(s),
        "eraser": eraser,
    })
}

/// This app's stroke as it's drawn, waiting to go (each tick sends what's gathered).
#[derive(Debug, Default)]
pub struct LiveOut {
    /// sending is on (a session, and the stroke is on a layer this app may draw on)
    pub on: bool,
    k: u64,
    /// messages ready to go
    pub out: Vec<Value>,
    pts: Vec<f64>,
    /// checks: each piece of points says when its first point was drawn ("t", ns)
    pub stamp: bool,
    first_ns: Option<u64>,
    /// the stroke's style (to say its start again)
    style: Option<Value>,
}

impl LiveOut {
    pub fn begin(&mut self, style: Value) {
        if !self.on {
            return;
        }
        self.flush();
        self.k += 1;
        self.style = Some(style.clone());
        self.out
            .push(json!({"live": {"type": "begin", "k": self.k, "style": style}}));
    }

    /// The stroke's start said again (one lost, or come before someone was ready, then
    /// still shows: a start twice is a fresh piece of the same stroke there).
    pub fn begin_again(&mut self) {
        if self.on
            && let Some(st) = self.style.clone()
        {
            self.flush();
            self.out
                .push(json!({"live": {"type": "begin", "k": self.k, "style": st}}));
        }
    }

    /// Points gathered and not gone yet.
    pub fn has_points(&self) -> bool {
        !self.pts.is_empty()
    }

    pub fn point(&mut self, x: f32, y: f32, pressure: f32, dt_ms: f64) {
        if !self.on {
            return;
        }
        let r1 = |v: f32| (v as f64 * 10.0).round() / 10.0;
        if self.stamp && self.pts.is_empty() {
            self.first_ns = Some(pencilpilot_window::clock::now_ns());
        }
        self.pts.extend([
            r1(x),
            r1(y),
            (pressure as f64 * 1000.0).round() / 1000.0,
            dt_ms.round(),
        ]);
    }

    pub fn end(&mut self, cancel: bool) {
        if !self.on {
            return;
        }
        self.flush();
        let t = if cancel { "cancel" } else { "end" };
        self.out.push(json!({"live": {"type": t, "k": self.k}}));
    }

    /// The points gathered so far become messages (in pieces of [`POINTS_PER_MESSAGE`]).
    pub fn flush(&mut self) {
        let t = self.first_ns.take();
        for c in self.pts.chunks(POINTS_PER_MESSAGE * 4) {
            let mut m = json!({"live": {"type": "points", "k": self.k, "pts": c}});
            if let Some(t) = t {
                m["live"]["t"] = json!(t);
            }
            self.out.push(m);
        }
        self.pts.clear();
    }

    pub fn take(&mut self) -> Vec<Value> {
        self.flush();
        std::mem::take(&mut self.out)
    }
}

/// Someone's stroke in progress here.
#[derive(Debug)]
struct Stroke {
    /// the painter's stroke key
    key: u32,
    brush: hokusai::Brush,
    /// when its newest message came (one whose end was lost goes after a while)
    at: std::time::Instant,
}

/// Others' live strokes: each member's preview layer and strokes going.
#[derive(Debug, Default)]
pub struct LiveIn {
    /// member → their preview layer (the painter's id)
    pub layers: HashMap<String, u32>,
    /// (member, k) → stroke
    strokes: HashMap<(String, u64), Stroke>,
    next_key: u32,
    /// when each member's newest live points came, ns (for `--stats`)
    pub last_at: HashMap<String, u64>,
    /// when each stroke's `begin` came, and when each member's tiles last came: their
    /// tiles may come before their stroke's last datagrams (the two travel apart)
    begun: HashMap<(String, u64), std::time::Instant>,
    tiles_at: HashMap<String, std::time::Instant>,
    /// members whose preview is left from a stroke that has ended, since when
    ended: HashMap<String, std::time::Instant>,
}

/// A preview left this long after its stroke ended goes (its tiles have come by then, or
/// were lost: they're the drawing either way).
pub const STALE_PREVIEW: std::time::Duration = std::time::Duration::from_secs(2);

/// What a live message did.
#[derive(Debug, PartialEq, Eq)]
pub enum Took {
    Nothing,
    /// the picture changed (and a preview layer may be new)
    Painted,
}

impl LiveIn {
    /// Whether `from` has strokes going.
    pub fn drawing(&self, from: &str) -> bool {
        self.strokes.keys().any(|(m, _)| m == from)
    }

    /// A live message from `from`. `layer` makes their preview layer when they have none
    /// (the painter's id, 0: couldn't).
    pub fn take(
        &mut self,
        p: &mut brush::Painter,
        from: &str,
        ev: &Value,
        layer: impl FnOnce(&mut brush::Painter) -> u32,
    ) -> Took {
        let Some(t) = ev.get("type").and_then(Value::as_str) else {
            return Took::Nothing;
        };
        let Some(k) = ev.get("k").and_then(Value::as_u64) else {
            return Took::Nothing;
        };
        let id = (from.to_string(), k);
        match t {
            "begin" => {
                let lid = match self.layers.get(from) {
                    Some(&l) if p.layers().iter().any(|x| x.id == l) => l,
                    _ => {
                        let l = layer(p);
                        if l == 0 {
                            return Took::Nothing;
                        }
                        self.layers.insert(from.to_string(), l);
                        l
                    }
                };
                let Some(b) = remote_brush(ev.get("style")) else {
                    return Took::Nothing;
                };
                self.next_key += 1;
                let key = self.next_key;
                if !p.remote_begin(key, lid) {
                    return Took::Nothing;
                }
                self.begun.insert(id.clone(), std::time::Instant::now());
                self.ended.remove(from);
                self.strokes.insert(
                    id,
                    Stroke {
                        key,
                        brush: b,
                        at: std::time::Instant::now(),
                    },
                );
                Took::Nothing
            }
            "points" => {
                let Some(st) = self.strokes.get_mut(&id) else {
                    return Took::Nothing;
                };
                st.at = std::time::Instant::now();
                let Some(pts) = ev.get("pts").and_then(Value::as_array) else {
                    return Took::Nothing;
                };
                let n = |i: usize| pts.get(i).and_then(Value::as_f64);
                let mut i = 0;
                let mut any = false;
                while i + 3 < pts.len() {
                    if let (Some(x), Some(y), Some(pr), Some(dt)) =
                        (n(i), n(i + 1), n(i + 2), n(i + 3))
                        && x.is_finite()
                        && y.is_finite()
                    {
                        p.remote_to(
                            st.key,
                            &st.brush,
                            x as f32,
                            y as f32,
                            pr.clamp(0.0, 1.0) as f32,
                            (dt / 1000.0).clamp(0.0005, 0.1),
                        );
                        any = true;
                    }
                    i += 4;
                }
                if any { Took::Painted } else { Took::Nothing }
            }
            "end" | "cancel" => {
                if let Some(st) = self.strokes.remove(&id) {
                    p.remote_end(st.key, &st.brush);
                }
                let begun = self.begun.remove(&id);
                // their tiles came already (after this stroke began): the preview goes now
                let tiles_first = matches!(
                    (begun, self.tiles_at.get(from)),
                    (Some(b), Some(t)) if *t >= b
                );
                if (t == "cancel" || tiles_first)
                    && !self.drawing(from)
                    && let Some(&l) = self.layers.get(from)
                {
                    p.clear_layer_silent(l);
                } else if !self.drawing(from) {
                    self.ended
                        .insert(from.to_string(), std::time::Instant::now());
                }
                Took::Painted
            }
            _ => Took::Nothing,
        }
    }

    /// Their real pixels arrived: their preview goes (unless they're mid-stroke).
    pub fn tiles_came(&mut self, p: &mut brush::Painter, from: &str) -> bool {
        self.tiles_at
            .insert(from.to_string(), std::time::Instant::now());
        if self.drawing(from) {
            return false;
        }
        self.ended.remove(from);
        match self.layers.get(from) {
            Some(&l) => {
                p.clear_layer_silent(l);
                true
            }
            None => false,
        }
    }

    /// Previews whose strokes ended a while ago go. Whether any did.
    pub fn prune(&mut self, p: &mut brush::Painter) -> bool {
        // strokes whose end never came (datagrams may be lost) end now
        let lost: Vec<(String, u64)> = self
            .strokes
            .iter()
            .filter(|(_, st)| st.at.elapsed() >= STALE_PREVIEW)
            .map(|(k, _)| k.clone())
            .collect();
        for k in lost {
            if let Some(st) = self.strokes.remove(&k) {
                p.remote_end(st.key, &st.brush);
            }
            self.begun.remove(&k);
            self.ended
                .entry(k.0)
                .or_insert_with(|| std::time::Instant::now() - STALE_PREVIEW);
        }
        let old: Vec<String> = self
            .ended
            .iter()
            .filter(|(_, t)| t.elapsed() >= STALE_PREVIEW)
            .map(|(m, _)| m.clone())
            .collect();
        for m in &old {
            self.ended.remove(m);
            if !self.drawing(m)
                && let Some(&l) = self.layers.get(m)
            {
                p.clear_layer_silent(l);
            }
        }
        !old.is_empty()
    }

    /// When [`LiveIn::prune`] next has something to do.
    pub fn prune_at(&self) -> Option<std::time::Instant> {
        self.ended
            .values()
            .chain(self.strokes.values().map(|st| &st.at))
            .map(|t| *t + STALE_PREVIEW)
            .min()
    }

    /// Someone left: their strokes and preview layer go.
    pub fn forget(&mut self, p: &mut brush::Painter, from: &str) {
        let keys: Vec<(String, u64)> = self
            .strokes
            .keys()
            .filter(|(m, _)| m == from)
            .cloned()
            .collect();
        for k in keys {
            if let Some(st) = self.strokes.remove(&k) {
                p.remote_end(st.key, &st.brush);
            }
        }
        if let Some(l) = self.layers.remove(from) {
            p.remove_layer_silent(l);
        }
        self.ended.remove(from);
        self.tiles_at.remove(from);
        self.begun.retain(|(m, _), _| m != from);
    }

    /// The session ended: every preview goes.
    pub fn clear(&mut self, p: &mut brush::Painter) {
        let who: Vec<String> = self.layers.keys().cloned().collect();
        for m in who {
            self.forget(p, &m);
        }
        self.strokes.clear();
    }
}

/// The brush a `begin`'s style asks for, set up as the current app sets it up.
fn remote_brush(style: Option<&Value>) -> Option<hokusai::Brush> {
    let st = style?;
    let id = st.get("brushId").and_then(Value::as_str).unwrap_or("");
    let src = brush_src(id);
    let mut b = parsed(src)?;
    let natural = b.get(BrushSetting::Radius).base_value;
    let scale = st
        .get("size")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(1.0) as f32;
    b.get_mut(BrushSetting::Radius).base_value = natural + scale.max(0.05).log2();
    let opacity = st
        .get("opacity")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(1.0) as f32;
    b.get_mut(BrushSetting::OpaqueMultiply).base_value = opacity.clamp(0.0, 1.0);
    let colour = st.get("color").and_then(Value::as_str).unwrap_or("#16161a");
    let n = u32::from_str_radix(colour.trim_start_matches('#'), 16).unwrap_or(0x16161a);
    brush::set_brush_rgb(&mut b, (n >> 16) as u8, (n >> 8) as u8, n as u8);
    let eraser = st.get("eraser").and_then(Value::as_bool).unwrap_or(false);
    b.get_mut(BrushSetting::Eraser).base_value = if eraser { 1.0 } else { 0.0 };
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_goes_in_small_pieces_and_is_drawn_there() {
        let s = crate::state::State::demo();
        let mut out = LiveOut {
            on: true,
            ..Default::default()
        };
        out.begin(style(&s));
        for i in 0..250 {
            out.point(20.0 + i as f32, 40.0, 0.8, 4.0);
        }
        out.end(false);
        let msgs = out.take();
        // begin, eleven pieces of points (24 each, 10), end
        assert_eq!(msgs.len(), 13, "{msgs:?}");
        assert_eq!(msgs[1]["live"]["pts"].as_array().unwrap().len(), 24 * 4);
        for m in &msgs {
            let wire = serde_json::json!({"from": "f".repeat(64), "body": m}).to_string();
            assert!(
                wire.len() < 1100,
                "a datagram fits in a packet: {}",
                wire.len()
            );
        }
        let st = &msgs[0]["live"]["style"];
        assert_eq!(st["brushId"], "deevad-2b-pencil");
        assert_eq!(st["color"], "#16161a");
        // drawn on someone's preview layer here
        let mut p = brush::Painter::new(300, 80);
        let mut live = LiveIn::default();
        for m in &msgs {
            live.take(&mut p, "them", &m["live"], |p| {
                p.add_layer_silent("Them (drawing now)")
            });
        }
        let l = live.layers["them"];
        assert!(!p.layer_tiles(l).is_empty(), "their stroke shows");
        assert!(!live.drawing("them"));
        assert!(live.tiles_came(&mut p, "them"));
        assert!(p.layer_tiles(l).is_empty(), "replaced by their tiles");
    }
}
