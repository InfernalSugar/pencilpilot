//! The edits drawing together carries: a closed list. Anything else (AI-made content
//! included) is dropped on arrival ([`parse_edit`] returns `None`).
//!
//! Every edit is JSON with a `k` (its kind) and, for most, a `lid` (the layer or group
//! it's about), exactly as together.ts writes and reads them.

use serde_json::{Map, Value};

use crate::js::{self, finite, int6, integer, is_lid, is_member, num, slice16, str_if};
use crate::lines::read_line_edit;

/// Tiles per edit (an edit stays around a megabyte at most).
pub const TILES_PER_EDIT: usize = 48;
/// A sound file's pieces are this many bytes.
pub const SOUND_CHUNK: usize = 256 * 1024;
/// The largest sound file shared.
pub const SOUND_MAX: usize = 20 * 1024 * 1024;

/// Why someone asks: to draw, or for a change that needs the owner's leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Why {
    #[default]
    Draw,
    Merge,
    Group,
    Delete,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::Draw => "draw",
            Why::Merge => "merge",
            Why::Group => "group",
            Why::Delete => "delete",
        }
    }
    /// The reason named (anything else: none of these).
    pub fn read(v: Option<&Value>) -> Option<Why> {
        match v.and_then(Value::as_str)? {
            "draw" => Some(Why::Draw),
            "merge" => Some(Why::Merge),
            "group" => Some(Why::Group),
            "delete" => Some(Why::Delete),
            _ => None,
        }
    }
}

/// Where a layer style's outline sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokePosition {
    #[default]
    Outside,
    Inside,
    Centre,
}

impl StrokePosition {
    pub fn as_str(self) -> &'static str {
        match self {
            StrokePosition::Outside => "outside",
            StrokePosition::Inside => "inside",
            StrokePosition::Centre => "centre",
        }
    }
}

/// A layer style's outline.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleStroke {
    pub on: bool,
    /// px, 1–100
    pub size: f64,
    /// `#rrggbb`, lower case
    pub colour: String,
    pub position: StrokePosition,
    /// 0–1
    pub opacity: f64,
}

/// A layer style's drop shadow.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleShadow {
    pub on: bool,
    pub colour: String,
    pub opacity: f64,
    /// where the light comes from, degrees anticlockwise from the right (0–360)
    pub angle: f64,
    /// px, 0–200
    pub distance: f64,
    /// px, 0–100
    pub blur: f64,
}

/// A layer style's outer glow.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleGlow {
    pub on: bool,
    pub colour: String,
    /// px, 1–100
    pub size: f64,
    pub opacity: f64,
}

/// A layer style (`@pencilpilot/paint/layer-style`): outline, drop shadow, outer glow.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStyle {
    pub stroke: StyleStroke,
    pub shadow: StyleShadow,
    pub glow: StyleGlow,
}

impl Default for LayerStyle {
    /// `NO_STYLE`: all off.
    fn default() -> Self {
        LayerStyle {
            stroke: StyleStroke {
                on: false,
                size: 4.0,
                colour: "#000000".into(),
                position: StrokePosition::Outside,
                opacity: 1.0,
            },
            shadow: StyleShadow {
                on: false,
                colour: "#000000".into(),
                opacity: 0.5,
                angle: 120.0,
                distance: 10.0,
                blur: 8.0,
            },
            glow: StyleGlow {
                on: false,
                colour: "#ffed87".into(),
                size: 12.0,
                opacity: 0.75,
            },
        }
    }
}

fn clamp_or(v: Option<&Value>, lo: f64, hi: f64, d: f64) -> f64 {
    finite(v).map_or(d, |x| x.max(lo).min(hi))
}

fn colour_or(v: Option<&Value>, d: &str) -> String {
    match v.and_then(Value::as_str) {
        Some(s) if is_hex_colour(s) => s.to_ascii_lowercase(),
        _ => d.into(),
    }
}

/// `^#[0-9a-f]{6}$` (either case).
pub(crate) fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

impl LayerStyle {
    /// `readStyle`: a style from anywhere, every field checked and kept in range, missing
    /// ones as [`LayerStyle::default`]. None when it isn't an object, or is the same as
    /// no style.
    pub fn read(v: &Value) -> Option<LayerStyle> {
        let o = v.as_object()?;
        let empty = Map::new();
        let part = |k: &str| o.get(k).and_then(Value::as_object).unwrap_or(&empty);
        let (k, d, g) = (part("stroke"), part("shadow"), part("glow"));
        let n = LayerStyle::default();
        let angle = clamp_or(d.get("angle"), -1e6, 1e6, n.shadow.angle);
        // ((a % 360) + 360) % 360 || 0
        let angle = ((angle % 360.0) + 360.0) % 360.0;
        let s = LayerStyle {
            stroke: StyleStroke {
                on: k.get("on") == Some(&Value::Bool(true)),
                size: clamp_or(k.get("size"), 1.0, 100.0, n.stroke.size),
                colour: colour_or(k.get("colour"), &n.stroke.colour),
                position: match k.get("position").and_then(Value::as_str) {
                    Some("outside") => StrokePosition::Outside,
                    Some("inside") => StrokePosition::Inside,
                    Some("centre") => StrokePosition::Centre,
                    _ => n.stroke.position,
                },
                opacity: clamp_or(k.get("opacity"), 0.0, 1.0, n.stroke.opacity),
            },
            shadow: StyleShadow {
                on: d.get("on") == Some(&Value::Bool(true)),
                colour: colour_or(d.get("colour"), &n.shadow.colour),
                opacity: clamp_or(d.get("opacity"), 0.0, 1.0, n.shadow.opacity),
                angle: if angle == 0.0 || angle.is_nan() {
                    0.0
                } else {
                    angle
                },
                distance: clamp_or(d.get("distance"), 0.0, 200.0, n.shadow.distance),
                blur: clamp_or(d.get("blur"), 0.0, 100.0, n.shadow.blur),
            },
            glow: StyleGlow {
                on: g.get("on") == Some(&Value::Bool(true)),
                colour: colour_or(g.get("colour"), &n.glow.colour),
                size: clamp_or(g.get("size"), 1.0, 100.0, n.glow.size),
                opacity: clamp_or(g.get("opacity"), 0.0, 1.0, n.glow.opacity),
            },
        };
        if same_style(Some(&s), None) {
            None
        } else {
            Some(s)
        }
    }

    pub fn to_json(&self) -> Value {
        let (k, d, g) = (&self.stroke, &self.shadow, &self.glow);
        js::object(vec![
            (
                "stroke",
                js::object(vec![
                    ("on", k.on.into()),
                    ("size", num(k.size)),
                    ("colour", k.colour.clone().into()),
                    ("position", k.position.as_str().into()),
                    ("opacity", num(k.opacity)),
                ]),
            ),
            (
                "shadow",
                js::object(vec![
                    ("on", d.on.into()),
                    ("colour", d.colour.clone().into()),
                    ("opacity", num(d.opacity)),
                    ("angle", num(d.angle)),
                    ("distance", num(d.distance)),
                    ("blur", num(d.blur)),
                ]),
            ),
            (
                "glow",
                js::object(vec![
                    ("on", g.on.into()),
                    ("colour", g.colour.clone().into()),
                    ("size", num(g.size)),
                    ("opacity", num(g.opacity)),
                ]),
            ),
        ])
    }
}

/// `sameStyle`: the same look (None = no style).
pub fn same_style(a: Option<&LayerStyle>, b: Option<&LayerStyle>) -> bool {
    let none = LayerStyle::default();
    let x = a.unwrap_or(&none);
    let y = b.unwrap_or(&none);
    let close = |p: f64, q: f64| (p - q).abs() < 1e-3;
    x.stroke.on == y.stroke.on
        && close(x.stroke.size, y.stroke.size)
        && x.stroke.colour == y.stroke.colour
        && x.stroke.position == y.stroke.position
        && close(x.stroke.opacity, y.stroke.opacity)
        && x.shadow.on == y.shadow.on
        && x.shadow.colour == y.shadow.colour
        && close(x.shadow.opacity, y.shadow.opacity)
        && close(x.shadow.angle, y.shadow.angle)
        && close(x.shadow.distance, y.shadow.distance)
        && close(x.shadow.blur, y.shadow.blur)
        && x.glow.on == y.glow.on
        && x.glow.colour == y.glow.colour
        && close(x.glow.size, y.glow.size)
        && close(x.glow.opacity, y.glow.opacity)
}

/// A layer's settings that everyone sees the same (its lock stays each app's own: it's
/// how access is shown).
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub visible: bool,
    /// 0–1
    pub opacity: f64,
    /// `^[a-z-]{1,20}$` (normal, multiply…)
    pub blend: String,
    pub alpha_lock: bool,
    pub clip: bool,
    /// outline, drop shadow and outer glow. `None`: from an app without styles (the
    /// style stays as it is); `Some(None)`: no style; `Some(Some(s))`: this style.
    pub style: Option<Option<LayerStyle>>,
}

impl Look {
    /// `readLook`: checked; None when it isn't a look.
    pub fn read(v: &Value) -> Option<Look> {
        let o = v.as_object()?;
        let blend = o.get("blend")?.as_str()?;
        if !(1..=20).contains(&blend.len())
            || !blend.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
        {
            return None;
        }
        let opacity = finite(o.get("opacity"))?;
        Some(Look {
            visible: o.get("visible") != Some(&Value::Bool(false)),
            opacity: opacity.clamp(0.0, 1.0),
            blend: blend.into(),
            alpha_lock: o.get("alphaLock") == Some(&Value::Bool(true)),
            clip: o.get("clip") == Some(&Value::Bool(true)),
            style: o.get("style").map(LayerStyle::read),
        })
    }

    pub fn to_json(&self) -> Value {
        let mut v = js::object(vec![
            ("visible", self.visible.into()),
            ("opacity", num(self.opacity)),
            ("blend", self.blend.clone().into()),
            ("alphaLock", self.alpha_lock.into()),
            ("clip", self.clip.into()),
        ]);
        if let Some(s) = &self.style {
            v["style"] = s.as_ref().map_or(Value::Null, LayerStyle::to_json);
        }
        v
    }
}

/// `sameLook`: both there, and the same.
pub fn same_look(a: Option<&Look>, b: Option<&Look>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    a.visible == b.visible
        && (a.opacity - b.opacity).abs() < 1e-3
        && a.blend == b.blend
        && a.alpha_lock == b.alpha_lock
        && a.clip == b.clip
        && same_style(
            a.style.as_ref().and_then(Option::as_ref),
            b.style.as_ref().and_then(Option::as_ref),
        )
}

/// A tile to put in: column, row, and its pixels as [`crate::tiles::encode_tile`] writes them.
pub type TileSet = (i32, i32, String);
/// A tile to clear: column, row.
pub type TileClear = (i32, i32);

/// One edit of the closed list.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// A new layer (on top), its id the maker's. `lines`: a line (vector) layer.
    Layer {
        lid: String,
        name: String,
        adjust: Option<String>,
        lines: bool,
    },
    /// A drawing's editable strokes changed: the strokes gone or changed (ids), and the
    /// new or changed ones at their place (each a base64 of the line bytes). On a plain
    /// layer, `base`: what changed in the pixels kept under its strokes (base64 of the
    /// engine's packed base changes).
    Lines {
        lid: String,
        did: String,
        del: Vec<String>,
        put: Vec<(u32, String)>,
        base: Option<String>,
    },
    /// A line layer became a plain one (Convert to pixels).
    Pixels {
        lid: String,
    },
    /// An adjustment layer's settings (the app's JSON).
    Adjust {
        lid: String,
        spec: String,
    },
    /// A layer's mask changed (`on: false`: it has none any more).
    Mask {
        lid: String,
        on: bool,
        set: Vec<TileSet>,
        clear: Vec<TileClear>,
    },
    Rename {
        lid: String,
        name: String,
    },
    Look {
        lid: String,
        look: Look,
    },
    Remove {
        lid: String,
    },
    /// A layer to `to` in the shared order (0: the bottom).
    Move {
        lid: String,
        to: u32,
    },
    /// A drawing's tiles changed.
    Tiles {
        lid: String,
        did: String,
        set: Vec<TileSet>,
        clear: Vec<TileClear>,
    },
    /// The whole timeline (everyone's: the later one wins). Checked as it's put in
    /// ([`crate::timeline::from_shared`]).
    Timeline {
        tl: Value,
    },
    /// A piece of a sound file (anyone's; the timeline says where it plays).
    Sound {
        file: String,
        name: String,
        i: u32,
        n: u32,
        data: String,
    },
    /// Asking a layer's (or group's) owner: to draw on it, or (`own`) for it to be handed
    /// over; `with`: the other layer of a merge.
    Ask {
        lid: String,
        why: Why,
        own: bool,
        with: Option<String>,
    },
    /// A new group (id `gid`, the maker's) holding these layers, in their places.
    Group {
        lid: String,
        gid: String,
        name: String,
        lids: Vec<String>,
    },
    /// A group's settings (`lid`: the group).
    GroupSet {
        lid: String,
        name: String,
        visible: bool,
        opacity: f64,
    },
    /// A layer to `to` in the shared order, and into a group (`gid` empty: none).
    Place {
        lid: String,
        to: u32,
        gid: String,
    },
    /// Take a group's layers out of it and remove it (`lid`: the group).
    Ungroup {
        lid: String,
    },
    /// Move a group's layers together, so the lowest is at `to` among the others.
    Block {
        lid: String,
        to: u32,
    },
    /// The owner lets `to` draw on it, or (`handover`) hands it over.
    Grant {
        lid: String,
        to: String,
        handover: bool,
    },
    Deny {
        lid: String,
        to: String,
    },
    Revoke {
        lid: String,
        to: String,
    },
}

/// `^[A-Za-z0-9._-]{1,80}$`: a sound file's key, as the studio makes them.
pub(crate) fn is_sound_key(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// An adjustment layer's settings: a JSON object, 8000 characters at most.
pub(crate) fn read_spec(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    if js::len16(s) > 8000 {
        return None;
    }
    match serde_json::from_str::<Value>(s) {
        Ok(Value::Object(_)) => Some(s.into()),
        _ => None,
    }
}

/// Tiles to set and clear, checked.
fn read_tiles(b: &Map<String, Value>) -> Option<(Vec<TileSet>, Vec<TileClear>)> {
    let set = b.get("set")?.as_array()?;
    let clear = b.get("clear")?.as_array()?;
    if set.len() + clear.len() > 4 * TILES_PER_EDIT {
        return None;
    }
    let set = set
        .iter()
        .filter_map(|t| {
            let t = t.as_array()?;
            Some((
                int6(t.first())? as i32,
                int6(t.get(1))? as i32,
                t.get(2)?.as_str()?.to_string(),
            ))
        })
        .collect();
    let clear = clear
        .iter()
        .filter_map(|t| {
            let t = t.as_array()?;
            Some((int6(t.first())? as i32, int6(t.get(1))? as i32))
        })
        .collect();
    Some((set, clear))
}

/// A name as edits carry it: control characters out, trimmed, 60 characters at most.
fn read_name(v: Option<&Value>) -> String {
    match v.and_then(Value::as_str) {
        Some(s) => {
            let s: String = s.chars().filter(|c| !c.is_control()).collect();
            slice16(js::trim(&s), 60)
        }
        None => String::new(),
    }
}

fn to_at(v: Option<&Value>) -> Option<u32> {
    int6(v).filter(|x| *x >= 0).map(|x| x as u32)
}

/// `parseEdit`: an edit from the session, checked; None for anything not on the list.
pub fn parse_edit(body: &Value) -> Option<Edit> {
    let b = body.as_object()?;
    let k = b.get("k").and_then(Value::as_str);
    if k == Some("timeline") {
        let tl = b.get("tl")?;
        return (tl.is_object() && tl.get("scenes").is_some_and(Value::is_array))
            .then(|| Edit::Timeline { tl: tl.clone() });
    }
    if k == Some("sound") {
        let file = str_if(b.get("file"), is_sound_key)?;
        let i = integer(b.get("i"))?;
        let n = integer(b.get("n"))?;
        let most = SOUND_MAX.div_ceil(SOUND_CHUNK) as f64;
        if !(n > 0.0 && n <= most && i >= 0.0 && i < n) {
            return None;
        }
        let data = b.get("data")?.as_str()?;
        let name = js::string(Some(
            js::or(b.get("name")).unwrap_or(&Value::String(file.into())),
        ));
        return Some(Edit::Sound {
            file: file.into(),
            name: slice16(&name, 100),
            i: i as u32,
            n: n as u32,
            data: data.into(),
        });
    }
    let lid = str_if(b.get("lid"), is_lid)?.to_string();
    let name = read_name(b.get("name"));
    let to = str_if(b.get("to"), is_member).map(str::to_string);
    let did = || str_if(b.get("did"), is_lid).map(str::to_string);
    Some(match k? {
        "layer" => {
            let adjust = read_spec(b.get("adjust"));
            let lines = adjust.is_none() && b.get("lines") == Some(&Value::Bool(true));
            Edit::Layer {
                lid,
                name: if name.is_empty() {
                    "Layer".into()
                } else {
                    name
                },
                adjust,
                lines,
            }
        }
        "lines" => {
            let did = did()?;
            let (del, put, base) = read_line_edit(b)?;
            Edit::Lines {
                lid,
                did,
                del,
                put,
                base,
            }
        }
        "pixels" => Edit::Pixels { lid },
        "adjust" => Edit::Adjust {
            lid,
            spec: read_spec(b.get("spec"))?,
        },
        "mask" => {
            let (set, clear) = read_tiles(b)?;
            Edit::Mask {
                lid,
                on: b.get("on") != Some(&Value::Bool(false)),
                set,
                clear,
            }
        }
        "rename" if !name.is_empty() => Edit::Rename { lid, name },
        "look" => Edit::Look {
            lid,
            look: Look::read(b.get("look")?)?,
        },
        "remove" => Edit::Remove { lid },
        "move" => Edit::Move {
            lid,
            to: to_at(b.get("to"))?,
        },
        "tiles" => {
            let did = did()?;
            let (set, clear) = read_tiles(b)?;
            Edit::Tiles {
                lid,
                did,
                set,
                clear,
            }
        }
        "ask" => Edit::Ask {
            lid,
            why: Why::read(b.get("why")).unwrap_or_default(),
            own: b.get("own") == Some(&Value::Bool(true)),
            with: str_if(b.get("with"), is_lid).map(str::to_string),
        },
        "group" => {
            let gid = str_if(b.get("gid"), is_lid)?.to_string();
            let lids: Vec<String> = b
                .get("lids")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| str_if(Some(x), is_lid).map(str::to_string))
                        .take(500)
                        .collect()
                })
                .unwrap_or_default();
            if lids.is_empty() {
                return None;
            }
            Edit::Group {
                lid,
                gid,
                name: if name.is_empty() {
                    "Group".into()
                } else {
                    name
                },
                lids,
            }
        }
        "gset" => Edit::GroupSet {
            lid,
            name: if name.is_empty() {
                "Group".into()
            } else {
                name
            },
            visible: b.get("visible") != Some(&Value::Bool(false)),
            opacity: finite(b.get("opacity"))?.clamp(0.0, 1.0),
        },
        "place" => {
            let gid = match b.get("gid") {
                Some(Value::String(s)) if s.is_empty() || is_lid(s) => s.clone(),
                _ => return None,
            };
            Edit::Place {
                lid,
                to: to_at(b.get("to"))?,
                gid,
            }
        }
        "ungroup" => Edit::Ungroup { lid },
        "block" => Edit::Block {
            lid,
            to: to_at(b.get("to"))?,
        },
        "grant" => Edit::Grant {
            lid,
            to: to?,
            handover: b.get("handover") == Some(&Value::Bool(true)),
        },
        "deny" => Edit::Deny { lid, to: to? },
        "revoke" => Edit::Revoke { lid, to: to? },
        _ => return None,
    })
}

fn tiles_json(set: &[TileSet], clear: &[TileClear]) -> (Value, Value) {
    (
        set.iter()
            .map(|(x, y, s)| Value::from(vec![Value::from(*x), Value::from(*y), s.clone().into()]))
            .collect(),
        clear
            .iter()
            .map(|(x, y)| Value::from(vec![Value::from(*x), Value::from(*y)]))
            .collect(),
    )
}

impl Edit {
    /// The layer or group it's about ("" for the timeline and sound files).
    pub fn lid(&self) -> &str {
        match self {
            Edit::Timeline { .. } | Edit::Sound { .. } => "",
            Edit::Layer { lid, .. }
            | Edit::Lines { lid, .. }
            | Edit::Pixels { lid }
            | Edit::Adjust { lid, .. }
            | Edit::Mask { lid, .. }
            | Edit::Rename { lid, .. }
            | Edit::Look { lid, .. }
            | Edit::Remove { lid }
            | Edit::Move { lid, .. }
            | Edit::Tiles { lid, .. }
            | Edit::Ask { lid, .. }
            | Edit::Group { lid, .. }
            | Edit::GroupSet { lid, .. }
            | Edit::Place { lid, .. }
            | Edit::Ungroup { lid }
            | Edit::Block { lid, .. }
            | Edit::Grant { lid, .. }
            | Edit::Deny { lid, .. }
            | Edit::Revoke { lid, .. } => lid,
        }
    }

    /// The edit's kind, as `k` says it.
    pub fn kind(&self) -> &'static str {
        match self {
            Edit::Layer { .. } => "layer",
            Edit::Lines { .. } => "lines",
            Edit::Pixels { .. } => "pixels",
            Edit::Adjust { .. } => "adjust",
            Edit::Mask { .. } => "mask",
            Edit::Rename { .. } => "rename",
            Edit::Look { .. } => "look",
            Edit::Remove { .. } => "remove",
            Edit::Move { .. } => "move",
            Edit::Tiles { .. } => "tiles",
            Edit::Timeline { .. } => "timeline",
            Edit::Sound { .. } => "sound",
            Edit::Ask { .. } => "ask",
            Edit::Group { .. } => "group",
            Edit::GroupSet { .. } => "gset",
            Edit::Place { .. } => "place",
            Edit::Ungroup { .. } => "ungroup",
            Edit::Block { .. } => "block",
            Edit::Grant { .. } => "grant",
            Edit::Deny { .. } => "deny",
            Edit::Revoke { .. } => "revoke",
        }
    }

    /// The edit as it goes to the session, in the shape together.ts sends it.
    pub fn to_json(&self) -> Value {
        let k = ("k", Value::from(self.kind()));
        let lid = ("lid", Value::from(self.lid()));
        let s = |v: &String| Value::from(v.clone());
        match self {
            Edit::Layer {
                name,
                adjust,
                lines,
                ..
            } => {
                let mut v = js::object(vec![k, lid, ("name", s(name))]);
                if let Some(a) = adjust {
                    v["adjust"] = s(a);
                } else if *lines {
                    v["lines"] = true.into();
                }
                v
            }
            Edit::Lines {
                did,
                del,
                put,
                base,
                ..
            } => {
                let mut v = js::object(vec![
                    k,
                    lid,
                    ("did", s(did)),
                    ("del", del.clone().into()),
                    (
                        "put",
                        put.iter()
                            .map(|(i, e)| Value::from(vec![Value::from(*i), s(e)]))
                            .collect(),
                    ),
                ]);
                if let Some(b) = base {
                    v["base"] = s(b);
                }
                v
            }
            Edit::Pixels { .. } | Edit::Remove { .. } | Edit::Ungroup { .. } => {
                js::object(vec![k, lid])
            }
            Edit::Adjust { spec, .. } => js::object(vec![k, lid, ("spec", s(spec))]),
            Edit::Mask { on, set, clear, .. } => {
                let (set, clear) = tiles_json(set, clear);
                js::object(vec![
                    k,
                    lid,
                    ("on", (*on).into()),
                    ("set", set),
                    ("clear", clear),
                ])
            }
            Edit::Rename { name, .. } => js::object(vec![k, lid, ("name", s(name))]),
            Edit::Look { look, .. } => js::object(vec![k, lid, ("look", look.to_json())]),
            Edit::Move { to, .. } | Edit::Block { to, .. } => {
                js::object(vec![k, lid, ("to", (*to).into())])
            }
            Edit::Tiles {
                did, set, clear, ..
            } => {
                let (set, clear) = tiles_json(set, clear);
                js::object(vec![
                    k,
                    lid,
                    ("did", s(did)),
                    ("set", set),
                    ("clear", clear),
                ])
            }
            Edit::Timeline { tl } => js::object(vec![k, ("tl", tl.clone())]),
            Edit::Sound {
                file,
                name,
                i,
                n,
                data,
            } => js::object(vec![
                k,
                ("file", s(file)),
                ("name", s(name)),
                ("i", (*i).into()),
                ("n", (*n).into()),
                ("data", s(data)),
            ]),
            Edit::Ask { why, own, with, .. } => {
                let mut v = js::object(vec![
                    k,
                    lid,
                    ("why", why.as_str().into()),
                    ("own", (*own).into()),
                ]);
                if let Some(w) = with {
                    v["with"] = s(w);
                }
                v
            }
            Edit::Group {
                gid, name, lids, ..
            } => js::object(vec![
                k,
                lid,
                ("gid", s(gid)),
                ("name", s(name)),
                ("lids", lids.clone().into()),
            ]),
            Edit::GroupSet {
                name,
                visible,
                opacity,
                ..
            } => js::object(vec![
                k,
                lid,
                ("name", s(name)),
                ("visible", (*visible).into()),
                ("opacity", num(*opacity)),
            ]),
            Edit::Place { to, gid, .. } => {
                js::object(vec![k, lid, ("to", (*to).into()), ("gid", s(gid))])
            }
            Edit::Grant { to, handover, .. } => js::object(vec![
                k,
                lid,
                ("to", s(to)),
                ("handover", (*handover).into()),
            ]),
            Edit::Deny { to, .. } | Edit::Revoke { to, .. } => {
                js::object(vec![k, lid, ("to", s(to))])
            }
        }
    }
}
