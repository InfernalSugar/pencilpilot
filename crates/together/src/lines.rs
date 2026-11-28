//! Line strokes (`@pencilpilot/paint/lines`) as drawing together carries them: the
//! binary form ("PPL1"/"PPL2": u32 JSON length, the strokes' settings as JSON, padding to
//! 4 bytes, every stroke's points as little-endian f32), byte for byte as lines.ts writes
//! it; the diff between two lists of strokes and its patch; and the `lines` edits.

use serde_json::{Map, Value};

use crate::edit::Edit;
use crate::js::{self, finite, from_base64, integer, len16, to_base64};
use crate::tiles::TILE_BYTES;

/// Numbers per point: x, y (canvas px), pressure (0–1), x and y tilt (−1–1), dtime (s).
pub const STRIDE: usize = 6;
/// Most strokes a drawing may hold, and points a stroke may have.
pub const MAX_STROKES: usize = 20000;
pub const MAX_POINTS: usize = 100000;
/// Strokes in one `lines` edit (more go in more edits), and the most text they may take.
pub const LINES_PER_EDIT: usize = 400;
pub const LINE_EDIT_CHARS: usize = 1_500_000;

const MAGIC: [u8; 4] = *b"PPL1";
const MAGIC2: [u8; 4] = *b"PPL2";

/// Brush strands (a stroke painted as several thin ones).
#[derive(Debug, Clone, PartialEq)]
pub struct BrushStrands {
    pub count: f64,
    pub spread: f64,
    pub wobble: f64,
    pub taper: f64,
}

/// A stroke's grain, picture tip and dual brush.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushTexture {
    pub grain: Option<String>,
    pub grain_scale: f64,
    pub grain_strength: f64,
    pub tip: Option<String>,
    pub dual: Option<String>,
    pub dual_size: f64,
    pub dual_scatter: f64,
    pub dual_count: f64,
    pub strands: Option<BrushStrands>,
}

/// A QuickShape a stroke was made as (its corners or axes come back to edit).
#[derive(Debug, Clone, PartialEq)]
pub enum EditShape {
    Poly {
        pts: Vec<[f64; 2]>,
        closed: bool,
        pressure: f64,
        smooth: bool,
    },
    Ellipse {
        c: [f64; 2],
        a: f64,
        b: f64,
        theta: f64,
        circle: bool,
        pressure: f64,
    },
}

/// Strokes made with symmetry on are linked: `g` names the group, `k` the copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub g: String,
    pub k: u32,
    /// vertical, horizontal, quadrant or radial
    pub mode: String,
    pub n: f64,
    pub cx: f64,
    pub cy: f64,
}

/// A line stroke: every stroke on a line layer (and a plain layer's editable ones).
#[derive(Debug, Clone, PartialEq)]
pub struct LineStroke {
    /// unique (shared drawings mix strokes from several apps): `^[0-9a-z~._-]{1,40}$`
    pub id: String,
    /// points, [`STRIDE`] numbers each
    pub pts: Vec<f32>,
    pub brush: String,
    /// `#rrggbb`
    pub color: String,
    pub size: f64,
    pub opacity: f64,
    pub radius: f64,
    pub seed: u32,
    pub texture: Option<BrushTexture>,
    pub shape: Option<EditShape>,
    pub link: Option<Link>,
    /// none (0): 2.5
    pub reach: Option<f64>,
    pub smudge: bool,
    /// an eraser stroke kept as one (an erase mark)
    pub erase: bool,
    pub span: Option<(u32, u32)>,
    pub dabs: Option<(u32, u32)>,
    /// erase marks that erase only this stroke
    pub clip: Vec<LineStroke>,
}

impl LineStroke {
    pub fn point_count(&self) -> usize {
        self.pts.len() / STRIDE
    }
}

/// JSON kept in order (for bytes as `JSON.stringify` writes them).
enum J {
    N(f64),
    S(String),
    B(bool),
    Null,
    A(Vec<J>),
    O(Vec<(&'static str, J)>),
}

impl J {
    fn write(&self, out: &mut String) {
        match self {
            J::N(x) => out.push_str(&if x.is_finite() {
                js::number_text(*x)
            } else {
                "null".into()
            }),
            J::S(s) => out.push_str(&serde_json::to_string(s).unwrap()),
            J::B(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Null => out.push_str("null"),
            J::A(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            J::O(o) => {
                out.push('{');
                for (i, (k, x)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(k);
                    out.push_str("\":");
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn opt_s(s: &Option<String>) -> J {
    s.as_ref().map_or(J::Null, |s| J::S(s.clone()))
}

fn meta_of(s: &LineStroke) -> J {
    let mut o = vec![
        ("id", J::S(s.id.clone())),
        ("n", J::N(s.point_count() as f64)),
        ("brush", J::S(s.brush.clone())),
        ("color", J::S(s.color.clone())),
        ("size", J::N(s.size)),
        ("opacity", J::N(s.opacity)),
        ("radius", J::N(s.radius)),
        ("seed", J::N(s.seed as f64)),
    ];
    if let Some(t) = &s.texture {
        let mut to = vec![
            ("grain", opt_s(&t.grain)),
            ("grainScale", J::N(t.grain_scale)),
            ("grainStrength", J::N(t.grain_strength)),
            ("tip", opt_s(&t.tip)),
            ("dual", opt_s(&t.dual)),
            ("dualSize", J::N(t.dual_size)),
            ("dualScatter", J::N(t.dual_scatter)),
            ("dualCount", J::N(t.dual_count)),
        ];
        if let Some(st) = &t.strands {
            to.push((
                "strands",
                J::O(vec![
                    ("count", J::N(st.count)),
                    ("spread", J::N(st.spread)),
                    ("wobble", J::N(st.wobble)),
                    ("taper", J::N(st.taper)),
                ]),
            ));
        }
        o.push(("texture", J::O(to)));
    }
    if let Some(sh) = &s.shape {
        let pt = |p: &[f64; 2]| J::A(vec![J::N(p[0]), J::N(p[1])]);
        o.push((
            "shape",
            match sh {
                EditShape::Poly {
                    pts,
                    closed,
                    pressure,
                    smooth,
                } => {
                    let mut v = vec![
                        ("kind", J::S("poly".into())),
                        ("pts", J::A(pts.iter().map(pt).collect())),
                        ("closed", J::B(*closed)),
                        ("pressure", J::N(*pressure)),
                    ];
                    if *smooth {
                        v.push(("smooth", J::B(true)));
                    }
                    J::O(v)
                }
                EditShape::Ellipse {
                    c,
                    a,
                    b,
                    theta,
                    circle,
                    pressure,
                } => J::O(vec![
                    ("kind", J::S("ellipse".into())),
                    ("c", pt(c)),
                    ("a", J::N(*a)),
                    ("b", J::N(*b)),
                    ("theta", J::N(*theta)),
                    ("circle", J::B(*circle)),
                    ("pressure", J::N(*pressure)),
                ]),
            },
        ));
    }
    if let Some(l) = &s.link {
        o.push((
            "link",
            J::O(vec![
                ("g", J::S(l.g.clone())),
                ("k", J::N(l.k as f64)),
                ("mode", J::S(l.mode.clone())),
                ("n", J::N(l.n)),
                ("cx", J::N(l.cx)),
                ("cy", J::N(l.cy)),
            ]),
        ));
    }
    if let Some(r) = s.reach.filter(|r| *r != 0.0 && !r.is_nan()) {
        o.push(("reach", J::N(r)));
    }
    if s.smudge {
        o.push(("smudge", J::B(true)));
    }
    if s.erase {
        o.push(("erase", J::B(true)));
    }
    if let Some((a, b)) = s.span {
        o.push(("span", J::A(vec![J::N(a as f64), J::N(b as f64)])));
    }
    if let Some((a, b)) = s.dabs {
        o.push(("dabs", J::A(vec![J::N(a as f64), J::N(b as f64)])));
    }
    if !s.clip.is_empty() {
        o.push(("clip", J::A(s.clip.iter().map(meta_of).collect())));
    }
    J::O(o)
}

/// `encodeLines`: strokes as bytes (a drawing in a project file, or an edit shared).
pub fn encode_lines(strokes: &[LineStroke]) -> Vec<u8> {
    let mut json = String::new();
    J::A(strokes.iter().map(meta_of).collect()).write(&mut json);
    let json = json.into_bytes();
    let head = 8 + json.len();
    let pad = (4 - head % 4) % 4;
    let mut out = Vec::with_capacity(head + pad);
    let two = strokes
        .iter()
        .any(|s| s.erase || s.dabs.is_some() || !s.clip.is_empty());
    out.extend_from_slice(if two { &MAGIC2 } else { &MAGIC });
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(head + pad, 0);
    for s in strokes {
        for pts in std::iter::once(&s.pts).chain(s.clip.iter().map(|c| &c.pts)) {
            let n = pts.len() / STRIDE * STRIDE;
            for v in &pts[..n] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

fn num_in(v: Option<&Value>, lo: f64, hi: f64) -> Option<f64> {
    finite(v).filter(|x| *x >= lo && *x <= hi)
}

/// `^[0-9a-z~._-]{1,40}$`: a stroke's id.
pub fn is_line_id(s: &str) -> bool {
    (1..=40).contains(&s.len())
        && s.bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'z' | b'~' | b'.' | b'_' | b'-'))
}

fn is_brush(s: &str) -> bool {
    (1..=120).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'/' | b'-'))
}

fn clamp_to(v: Option<&Value>, lo: f64, hi: f64, d: f64) -> f64 {
    finite(v).map_or(d, |x| x.max(lo).min(hi))
}

fn read_strands(v: Option<&Value>) -> Option<BrushStrands> {
    let s = v?.as_object()?;
    let count = js_round(clamp_to(s.get("count"), 0.0, 24.0, 0.0));
    if count < 2.0 {
        return None;
    }
    Some(BrushStrands {
        count,
        spread: clamp_to(s.get("spread"), 1.0, 30.0, 10.0),
        wobble: clamp_to(s.get("wobble"), 0.0, 1.0, 0.35),
        taper: clamp_to(s.get("taper"), 0.0, 1.0, 0.6),
    })
}

/// `Math.round`.
pub(crate) fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

fn read_texture(v: Option<&Value>) -> Option<BrushTexture> {
    let empty = Map::new();
    // (an array is an object to JavaScript: it reads as one without those fields)
    let t = match v? {
        Value::Object(o) => o,
        Value::Array(_) => &empty,
        _ => return None,
    };
    let id = |k: &str| {
        t.get(k)
            .and_then(Value::as_str)
            .filter(|s| len16(s) <= 120)
            .map(str::to_string)
    };
    let n = |k: &str, d: f64| finite(t.get(k)).unwrap_or(d);
    Some(BrushTexture {
        grain: id("grain"),
        grain_scale: n("grainScale", 1.0),
        grain_strength: n("grainStrength", 0.7),
        tip: id("tip"),
        dual: id("dual"),
        dual_size: n("dualSize", 0.35),
        dual_scatter: n("dualScatter", 0.0),
        dual_count: n("dualCount", 1.0),
        strands: read_strands(t.get("strands")),
    })
}

fn read_pt(p: &Value) -> Option<[f64; 2]> {
    let a = p.as_array().filter(|a| a.len() == 2)?;
    Some([num_in(a.first(), -1e6, 1e6)?, num_in(a.get(1), -1e6, 1e6)?])
}

fn read_shape(v: Option<&Value>) -> Option<EditShape> {
    let s = v?.as_object()?;
    let pressure = num_in(s.get("pressure"), 0.0, 1.0)?;
    match s.get("kind").and_then(Value::as_str) {
        Some("poly") => {
            let pts = s.get("pts")?.as_array()?;
            if !(2..=64).contains(&pts.len()) {
                return None;
            }
            Some(EditShape::Poly {
                pts: pts.iter().map(read_pt).collect::<Option<_>>()?,
                closed: s.get("closed") == Some(&Value::Bool(true)),
                pressure,
                smooth: js::truthy(s.get("smooth")),
            })
        }
        Some("ellipse") => Some(EditShape::Ellipse {
            c: read_pt(s.get("c")?)?,
            a: num_in(s.get("a"), 0.0, 1e6)?,
            b: num_in(s.get("b"), 0.0, 1e6)?,
            theta: num_in(s.get("theta"), -10.0, 10.0)?,
            circle: s.get("circle") == Some(&Value::Bool(true)),
            pressure,
        }),
        _ => None,
    }
}

fn read_link(v: Option<&Value>) -> Option<Link> {
    let l = v?.as_object()?;
    let g = l.get("g")?.as_str().filter(|g| is_line_id(g))?;
    let mode = l.get("mode")?.as_str()?;
    if !["vertical", "horizontal", "quadrant", "radial"].contains(&mode) {
        return None;
    }
    let k = integer(l.get("k")).filter(|k| (0.0..=24.0).contains(k))?;
    Some(Link {
        g: g.into(),
        k: k as u32,
        mode: mode.into(),
        n: num_in(l.get("n"), 2.0, 24.0)?,
        cx: num_in(l.get("cx"), -1e6, 1e6)?,
        cy: num_in(l.get("cy"), -1e6, 1e6)?,
    })
}

/// A pair of whole numbers `a < b ≤ most`.
fn pair(v: Option<&Value>, most: f64) -> Option<(u32, u32)> {
    let a = v?.as_array().filter(|a| a.len() == 2)?;
    let x = integer(a.first())?;
    let y = integer(a.get(1))?;
    (x >= 0.0 && x < y && y <= most).then_some((x as u32, y as u32))
}

/// Why bytes aren't line data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotLines(pub &'static str);

struct Reader<'a> {
    bytes: &'a [u8],
    o: usize,
    points: usize,
}

impl Reader<'_> {
    fn read(&mut self, m: &Value, nested: bool) -> Result<LineStroke, NotLines> {
        let bad = NotLines("bad stroke in line data");
        let m = m.as_object().ok_or(bad.clone())?;
        let id = m
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| is_line_id(s));
        let n = integer(m.get("n")).filter(|n| *n >= 1.0 && *n <= MAX_POINTS as f64);
        let brush = m
            .get("brush")
            .and_then(Value::as_str)
            .filter(|s| is_brush(s));
        let color = m
            .get("color")
            .and_then(Value::as_str)
            .filter(|s| crate::edit::is_hex_colour(s));
        let size = num_in(m.get("size"), 0.01, 1000.0);
        let opacity = num_in(m.get("opacity"), 0.0, 1.0);
        let radius = num_in(m.get("radius"), 0.0, 5000.0);
        let seed = num_in(m.get("seed"), 0.0, 4294967295.0);
        let clip_ok = match m.get("clip") {
            None => true,
            Some(c) => !nested && c.as_array().is_some_and(|c| c.len() <= 64),
        };
        let (
            Some(id),
            Some(n),
            Some(brush),
            Some(color),
            Some(size),
            Some(opacity),
            Some(radius),
            Some(seed),
            true,
        ) = (id, n, brush, color, size, opacity, radius, seed, clip_ok)
        else {
            return Err(bad);
        };
        let n = n as usize;
        let count = n * STRIDE;
        self.points += n;
        if self.points > MAX_POINTS * 8 {
            return Err(NotLines("too many points in line data"));
        }
        if self.o + count * 4 > self.bytes.len() {
            return Err(NotLines("line data is cut short"));
        }
        let mut pts = Vec::with_capacity(count);
        for _ in 0..count {
            let v = f32::from_le_bytes(self.bytes[self.o..self.o + 4].try_into().unwrap());
            if !v.is_finite() {
                return Err(NotLines("bad point in line data"));
            }
            pts.push(v);
            self.o += 4;
        }
        let erase = m.get("erase") == Some(&Value::Bool(true));
        let span = if erase {
            None
        } else {
            pair(m.get("span"), n as f64)
        };
        let cut = if erase {
            None
        } else {
            pair(m.get("dabs"), 2f64.powi(31))
        };
        let mut clip = Vec::new();
        if let Some(c) = m.get("clip").and_then(Value::as_array) {
            for c in c {
                let c = self.read(c, true)?;
                if c.erase {
                    clip.push(c);
                }
            }
        }
        Ok(LineStroke {
            id: id.into(),
            pts,
            brush: brush.into(),
            color: color.to_ascii_lowercase(),
            size,
            opacity,
            radius,
            seed: seed.floor() as u32,
            texture: read_texture(m.get("texture")),
            shape: read_shape(m.get("shape")),
            link: read_link(m.get("link")),
            reach: num_in(m.get("reach"), 0.0, 1000.0),
            smudge: m.get("smudge") == Some(&Value::Bool(true)),
            erase,
            span: if cut.is_some() { span } else { None },
            dabs: cut,
            clip,
        })
    }
}

/// `decodeLines`: bytes back into strokes, checked.
pub fn decode_lines(bytes: &[u8]) -> Result<Vec<LineStroke>, NotLines> {
    if bytes.len() < 8 || (bytes[..4] != MAGIC && bytes[..4] != MAGIC2) {
        return Err(NotLines("not line data"));
    }
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if 8 + len > bytes.len() {
        return Err(NotLines("line data ends inside its header"));
    }
    let text = String::from_utf8_lossy(&bytes[8..8 + len]);
    let raw: Value = serde_json::from_str(&text).map_err(|_| NotLines("bad line data"))?;
    let raw = raw
        .as_array()
        .filter(|a| a.len() <= MAX_STROKES)
        .ok_or(NotLines("bad line data"))?;
    let head = 8 + len;
    let mut r = Reader {
        bytes,
        o: head + (4 - head % 4) % 4,
        points: 0,
    };
    raw.iter().map(|m| r.read(m, false)).collect()
}

/// `diffLines`: what turns `before` into `after`: the strokes gone (or changed) by id,
/// and the new or changed ones with their place in `after`. (lines.ts compares strokes
/// as objects; here they're compared by value, which sends the same for the same edits.)
pub fn diff_lines<'a>(
    before: &[LineStroke],
    after: &'a [LineStroke],
) -> (Vec<String>, Vec<(u32, &'a LineStroke)>) {
    use std::collections::HashMap;
    let was: HashMap<&str, &LineStroke> = before.iter().map(|s| (s.id.as_str(), s)).collect();
    let now: HashMap<&str, &LineStroke> = after.iter().map(|s| (s.id.as_str(), s)).collect();
    let del = before
        .iter()
        .filter(|s| now.get(s.id.as_str()) != Some(s))
        .map(|s| s.id.clone())
        .collect();
    let put = after
        .iter()
        .enumerate()
        .filter(|(_, s)| was.get(s.id.as_str()) != Some(s))
        .map(|(i, s)| (i as u32, s))
        .collect();
    (del, put)
}

/// `patchLines`: a diff applied to a drawing's strokes.
pub fn patch_lines(
    list: &[LineStroke],
    del: &[String],
    put: Vec<(u32, LineStroke)>,
) -> Vec<LineStroke> {
    use std::collections::HashSet;
    let gone: HashSet<&str> = del
        .iter()
        .map(String::as_str)
        .chain(put.iter().map(|(_, s)| s.id.as_str()))
        .collect();
    let mut out: Vec<LineStroke> = list
        .iter()
        .filter(|s| !gone.contains(s.id.as_str()))
        .cloned()
        .collect();
    let mut put = put;
    put.sort_by_key(|p| p.0);
    for (i, s) in put {
        let at = (i as usize).min(out.len());
        out.insert(at, s);
    }
    out
}

/// A line stroke for a `lines` edit.
pub fn stroke_to_share(s: &LineStroke) -> String {
    to_base64(&encode_lines(std::slice::from_ref(s)))
}

/// A line stroke from a `lines` edit (None: not one).
pub fn stroke_from_share(s: &str) -> Option<LineStroke> {
    let mut list = decode_lines(&from_base64(s)?).ok()?;
    (list.len() == 1).then(|| list.pop().unwrap())
}

fn is_b64_chars(s: &str) -> bool {
    s.bytes()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'='))
}

/// A `lines` edit's parts, checked: strokes gone, strokes put, and the base.
pub type LineParts = (Vec<String>, Vec<(u32, String)>, Option<String>);

/// `readLineEdit`: a `lines` edit's strokes gone and put, checked (the strokes
/// themselves are read as they're put in).
pub fn read_line_edit(b: &Map<String, Value>) -> Option<LineParts> {
    let del = b.get("del")?.as_array()?;
    let put = b.get("put")?.as_array()?;
    if del.len() > 20000 || put.len() > LINES_PER_EDIT {
        return None;
    }
    let del = del
        .iter()
        .filter_map(|x| x.as_str().filter(|s| is_line_id(s)).map(str::to_string))
        .collect();
    let base = match b.get("base") {
        None => None,
        Some(Value::String(s)) if len16(s) <= LINE_EDIT_CHARS && is_b64_chars(s) => Some(s.clone()),
        Some(_) => return None,
    };
    let mut chars = base.as_ref().map_or(0, |s| s.len());
    let mut out = Vec::new();
    for p in put {
        let p = p.as_array()?;
        let i = integer(p.first()).filter(|i| *i >= 0.0 && *i <= 1e6)?;
        let s = p
            .get(1)?
            .as_str()
            .filter(|s| !s.is_empty() && is_b64_chars(s))?;
        chars += s.len();
        if chars > LINE_EDIT_CHARS {
            return None;
        }
        out.push((i as u32, s.to_string()));
    }
    Some((del, out, base))
}

/// `withMoves`: a diff that also carries strokes moved in the order without changing
/// (brought to the front, sent back): the fewest of them, taken out and put again at their
/// place, so the strokes left keep their order. (A diff alone sends only changed strokes:
/// the order would differ from app to app.)
pub fn with_moves<'a>(
    before: &[LineStroke],
    after: &'a [LineStroke],
    d: (Vec<String>, Vec<(u32, &'a LineStroke)>),
) -> (Vec<String>, Vec<(u32, &'a LineStroke)>) {
    use std::collections::{HashMap, HashSet};
    let at: HashMap<&str, usize> = before
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();
    let changed: HashSet<u32> = d.1.iter().map(|p| p.0).collect();
    // the strokes that stay: their places before, in the order they're in now
    let stay: Vec<(usize, usize)> = after
        .iter()
        .enumerate()
        .filter(|(i, _)| !changed.contains(&(*i as u32)))
        .map(|(i, s)| (i, at[s.id.as_str()]))
        .collect();
    // the longest run of them still in order keeps its place; the others move
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; stay.len()];
    for j in 0..stay.len() {
        let lo = tails.partition_point(|&t| stay[t].1 < stay[j].1);
        prev[j] = if lo > 0 { tails[lo - 1] } else { usize::MAX };
        if lo == tails.len() {
            tails.push(j);
        } else {
            tails[lo] = j;
        }
    }
    let mut keep = HashSet::new();
    let mut j = tails.last().copied().unwrap_or(usize::MAX);
    while j != usize::MAX {
        keep.insert(j);
        j = prev[j];
    }
    if keep.len() == stay.len() {
        return d;
    }
    let moved: Vec<usize> = stay
        .iter()
        .enumerate()
        .filter(|(j, _)| !keep.contains(j))
        .map(|(_, (i, _))| *i)
        .collect();
    let (mut del, mut put) = d;
    del.extend(moved.iter().map(|i| after[*i].id.clone()));
    put.extend(moved.iter().map(|i| (*i as u32, &after[*i])));
    put.sort_by_key(|p| p.0);
    (del, put)
}

/// `lineEdits`: the `lines` edits that turn `before` into `after` (several when there's
/// a lot to send); none when nothing changed.
pub fn line_edits(lid: &str, did: &str, before: &[LineStroke], after: &[LineStroke]) -> Vec<Edit> {
    let (del, put) = with_moves(before, after, diff_lines(before, after));
    if del.is_empty() && put.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Edit> = Vec::new();
    let mut part: Vec<(u32, String)> = Vec::new();
    let mut chars = 0;
    let mut del = Some(del);
    let mut flush = |out: &mut Vec<Edit>, part: &mut Vec<(u32, String)>| {
        out.push(Edit::Lines {
            lid: lid.into(),
            did: did.into(),
            del: if out.is_empty() {
                del.take().unwrap_or_default()
            } else {
                Vec::new()
            },
            put: std::mem::take(part),
            base: None,
        });
    };
    for (i, s) in put {
        let enc = stroke_to_share(s);
        if !part.is_empty() && (part.len() >= LINES_PER_EDIT || chars + enc.len() > LINE_EDIT_CHARS)
        {
            flush(&mut out, &mut part);
            chars = 0;
        }
        chars += enc.len();
        part.push((i, enc));
    }
    if !part.is_empty() || out.is_empty() {
        flush(&mut out, &mut part);
    }
    out
}

/// `applyLineEdit`: a `lines` edit put into a drawing's strokes (strokes that aren't
/// ones are left out).
pub fn apply_line_edit(
    list: &[LineStroke],
    del: &[String],
    put: &[(u32, String)],
) -> Vec<LineStroke> {
    let put = put
        .iter()
        .filter_map(|(i, enc)| stroke_from_share(enc).map(|s| (*i, s)))
        .collect();
    patch_lines(list, del, put)
}

/// `splitBase`: a plain layer's base changes (the engine's packing: u32 entries, i32 x,
/// i32 y each; the tiles with paint as u32 count, then i32 x, i32 y, 64×64 RGBA8 each;
/// u32 entries gone, i32 x, i32 y each) cut into parts of at most `max` bytes, each a
/// packing of its own. None when the bytes don't add up.
pub fn split_base(bytes: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    use std::collections::HashMap;
    let u32_at = |at: usize| {
        (at + 4 <= bytes.len()).then(|| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()))
    };
    let i32_at = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let n = u32_at(0)? as usize;
    let mut at = 4;
    let mut keys = Vec::new();
    for _ in 0..n {
        if at + 8 > bytes.len() {
            return None;
        }
        keys.push((i32_at(at), i32_at(at + 4)));
        at += 8;
    }
    let tn = u32_at(at)? as usize;
    at += 4;
    let mut tiles: HashMap<(i32, i32), &[u8]> = HashMap::new();
    for _ in 0..tn {
        if at + 8 + TILE_BYTES > bytes.len() {
            return None;
        }
        tiles.insert(
            (i32_at(at), i32_at(at + 4)),
            &bytes[at + 8..at + 8 + TILE_BYTES],
        );
        at += 8 + TILE_BYTES;
    }
    let gn = u32_at(at)? as usize;
    if at + 4 + gn * 8 != bytes.len() {
        return None;
    }
    at += 4;
    let gone: Vec<(i32, i32)> = (0..gn)
        .map(|i| (i32_at(at + i * 8), i32_at(at + i * 8 + 4)))
        .collect();
    if bytes.len() <= max {
        return Some(vec![bytes.to_vec()]);
    }
    let pack = |part: &[(i32, i32)], g: &[(i32, i32)]| {
        let with_px: Vec<&(i32, i32)> = part.iter().filter(|k| tiles.contains_key(k)).collect();
        let mut out = Vec::with_capacity(
            12 + part.len() * 8 + with_px.len() * (8 + TILE_BYTES) + g.len() * 8,
        );
        out.extend_from_slice(&(part.len() as u32).to_le_bytes());
        for (x, y) in part {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
        out.extend_from_slice(&(with_px.len() as u32).to_le_bytes());
        for k in with_px {
            out.extend_from_slice(&k.0.to_le_bytes());
            out.extend_from_slice(&k.1.to_le_bytes());
            out.extend_from_slice(tiles[k]);
        }
        out.extend_from_slice(&(g.len() as u32).to_le_bytes());
        for (x, y) in g {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
        out
    };
    let mut parts = Vec::new();
    let mut part = Vec::new();
    let mut size = 12;
    for k in keys {
        let s = 8 + if tiles.contains_key(&k) {
            8 + TILE_BYTES
        } else {
            0
        };
        if !part.is_empty() && size + s > max {
            parts.push(pack(&part, &[]));
            part.clear();
            size = 12;
        }
        part.push(k);
        size += s;
    }
    parts.push(pack(&part, &gone));
    Some(parts)
}

/// The most bytes [`split_base`] puts in one part, as together.ts sends them.
pub const BASE_PART: usize = 900_000;

/// `JSON.stringify(put).length`.
fn put_chars(put: &[(u32, String)]) -> usize {
    if put.is_empty() {
        return 2;
    }
    2 + put.len() - 1
        + put
            .iter()
            .map(|(i, s)| 5 + js::number_text(*i as f64).len() + s.len())
            .sum::<usize>()
}

/// `withBase`: the `lines` edits that carry a plain layer's base changes (`bytes`),
/// after `edits` (the first part rides with the strokes' last edit when there's room).
pub fn with_base(mut edits: Vec<Edit>, lid: &str, did: &str, bytes: Option<&[u8]>) -> Vec<Edit> {
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
        return edits;
    };
    let parts = split_base(bytes, BASE_PART).unwrap_or_default();
    for (i, p) in parts.iter().enumerate() {
        let b64 = to_base64(p);
        if i == 0
            && let Some(Edit::Lines { put, base, .. }) = edits.last_mut()
            && base.is_none()
            && put_chars(put) + b64.len() < LINE_EDIT_CHARS
        {
            *base = Some(b64);
            continue;
        }
        edits.push(Edit::Lines {
            lid: lid.into(),
            did: did.into(),
            del: Vec::new(),
            put: Vec::new(),
            base: Some(b64),
        });
    }
    edits
}
