//! What else members send each other while drawing together, none of it part of the
//! project: chat lines (direct messages, `{k: 'chat', text}`), cursors and live strokes
//! (presence: `{x, y}`, `{live: …}`), each member's colour, and the invite in pasted text
//! (the studio's chat.ts and share.ts).

use indexmap::IndexMap;
use serde_json::Value;

use crate::js::{self, finite, slice16};

/// The longest chat message, and the lines a chat keeps.
pub const MAX_CHAT_CHARS: usize = 500;
pub const KEEP_CHAT: usize = 200;

/// `chatText`: the text of an incoming chat body; None when it isn't one. Never markup:
/// show it as text. Control characters (but tabs and line breaks) are dropped.
pub fn chat_text(body: &Value) -> Option<String> {
    if body.get("k").and_then(Value::as_str) != Some("chat") {
        return None;
    }
    let text = body.get("text")?.as_str()?;
    let text: String = text
        .chars()
        .filter(
            |c| !matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}'),
        )
        .collect();
    let text = js::trim(&text);
    (!text.is_empty()).then(|| slice16(text, MAX_CHAT_CHARS))
}

/// `chatBody`: what to send for typed text (to each other member, directly); None when
/// there's nothing to say.
pub fn chat_body(typed: &str) -> Option<Value> {
    let text = slice16(js::trim(typed), MAX_CHAT_CHARS);
    (!text.is_empty()).then(|| js::object(vec![("k", "chat".into()), ("text", text.into())]))
}

/// `capLines`: the newest `keep` lines.
pub fn cap_lines<T>(mut lines: Vec<T>, keep: usize) -> Vec<T> {
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }
    lines
}

const COLOURS: [&str; 8] = [
    "#e4572e", "#2e86ab", "#7b2cbf", "#2a9d8f", "#d1495b", "#ef8a17", "#3a5a40", "#c9184a",
];

/// `memberColour`: a member's colour, picked from their id (the same on every app).
pub fn member_colour(id: &str) -> &'static str {
    let h = id
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    COLOURS[h as usize % COLOURS.len()]
}

/// `cleanInvite`: the invite code in pasted text (one unbroken token).
pub fn clean_invite(text: &str) -> Option<String> {
    let at = text.find("pp1-")?;
    let rest = &text[at + 4..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    if end == 0 {
        // ("pp1-" with nothing after it: look further on)
        return clean_invite(rest);
    }
    Some(format!("pp1-{}", &rest[..end]))
}

/// A cursor (presence `{x, y}`, canvas pixels).
pub fn cursor(v: &Value) -> Option<(f64, f64)> {
    Some((finite(v.get("x"))?, finite(v.get("y"))?))
}

/// A cursor for presence.
pub fn cursor_body(x: f64, y: f64) -> Value {
    js::object(vec![("x", js::num(x)), ("y", js::num(y))])
}

/// Others' cursors: the newest position of each member, dropped after `stale_ms`
/// without news.
#[derive(Debug, Clone)]
pub struct Cursors {
    at: IndexMap<String, (f64, f64, f64)>,
    pub stale_ms: f64,
}

/// A cursor to show.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownCursor {
    pub id: String,
    pub name: String,
    pub color: &'static str,
    pub x: f64,
    pub y: f64,
}

impl Default for Cursors {
    fn default() -> Self {
        Cursors::new(4000.0)
    }
}

impl Cursors {
    pub fn new(stale_ms: f64) -> Self {
        Cursors {
            at: IndexMap::new(),
            stale_ms,
        }
    }
    /// Whether anything changed.
    pub fn set(&mut self, id: &str, x: f64, y: f64, now: f64) -> bool {
        let old = self.at.insert(id.into(), (x, y, now));
        old.is_none_or(|(ox, oy, _)| ox != x || oy != y)
    }
    pub fn remove(&mut self, id: &str) -> bool {
        self.at.shift_remove(id).is_some()
    }
    pub fn clear(&mut self) {
        self.at.clear();
    }
    /// Drops stale cursors; whether any went.
    pub fn prune(&mut self, now: f64) -> bool {
        let before = self.at.len();
        let stale = self.stale_ms;
        self.at.retain(|_, c| now - c.2 <= stale);
        self.at.len() != before
    }
    /// The cursors, named from `members` (id, name).
    pub fn list(&self, members: &[(String, String)]) -> Vec<ShownCursor> {
        self.at
            .iter()
            .map(|(id, (x, y, _))| ShownCursor {
                id: id.clone(),
                name: members
                    .iter()
                    .find(|m| m.0 == *id)
                    .map_or("Someone".into(), |m| m.1.clone()),
                color: member_colour(id),
                x: *x,
                y: *y,
            })
            .collect()
    }
}

/// A stroke's brush, as a live stroke shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveStyle {
    pub brush_id: String,
    pub color: String,
    pub size: f64,
    pub opacity: f64,
    pub eraser: bool,
}

/// Someone's stroke as they draw it (presence `{live: …}`: fast and lossy, like cursors;
/// the real pixels follow as tiles). `k` numbers the stroke.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    Begin {
        k: f64,
        style: LiveStyle,
    },
    /// 4 numbers a point; at most 96 numbers a message
    Points {
        k: f64,
        pts: Vec<f64>,
    },
    End {
        k: f64,
    },
    /// the stroke was abandoned (a pinch): drop it
    Cancel {
        k: f64,
    },
}

/// Numbers per live message (datagrams stay small).
pub const LIVE_POINTS: usize = 96;

impl LiveEvent {
    /// From a presence body (`{live: …}`); None when it isn't one.
    pub fn read(body: &Value) -> Option<LiveEvent> {
        let l = body.get("live")?;
        let k = finite(l.get("k"))?;
        Some(match l.get("type")?.as_str()? {
            "begin" => {
                let s = l.get("style")?;
                LiveEvent::Begin {
                    k,
                    style: LiveStyle {
                        brush_id: s.get("brushId")?.as_str()?.into(),
                        color: s.get("color")?.as_str()?.into(),
                        size: finite(s.get("size"))?,
                        opacity: finite(s.get("opacity"))?,
                        eraser: s.get("eraser") == Some(&Value::Bool(true)),
                    },
                }
            }
            "points" => LiveEvent::Points {
                k,
                pts: l
                    .get("pts")?
                    .as_array()?
                    .iter()
                    .map(|v| finite(Some(v)))
                    .collect::<Option<_>>()?,
            },
            "end" => LiveEvent::End { k },
            "cancel" => LiveEvent::Cancel { k },
            _ => return None,
        })
    }

    /// The presence bodies to send it in (long point lists in pieces, as the studio does).
    pub fn bodies(&self) -> Vec<Value> {
        let wrap = |l: Value| js::object(vec![("live", l)]);
        match self {
            LiveEvent::Begin { k, style } => vec![wrap(js::object(vec![
                ("type", "begin".into()),
                ("k", js::num(*k)),
                (
                    "style",
                    js::object(vec![
                        ("brushId", style.brush_id.clone().into()),
                        ("color", style.color.clone().into()),
                        ("size", js::num(style.size)),
                        ("opacity", js::num(style.opacity)),
                        ("eraser", style.eraser.into()),
                    ]),
                ),
            ]))],
            LiveEvent::Points { k, pts } => {
                let chunks: Vec<&[f64]> = if pts.is_empty() {
                    vec![&pts[..]]
                } else {
                    pts.chunks(LIVE_POINTS).collect()
                };
                chunks
                    .into_iter()
                    .map(|c| {
                        wrap(js::object(vec![
                            ("type", "points".into()),
                            ("k", js::num(*k)),
                            ("pts", c.iter().map(|v| js::num(*v)).collect()),
                        ]))
                    })
                    .collect()
            }
            LiveEvent::End { k } => vec![wrap(js::object(vec![
                ("type", "end".into()),
                ("k", js::num(*k)),
            ]))],
            LiveEvent::Cancel { k } => {
                vec![wrap(js::object(vec![
                    ("type", "cancel".into()),
                    ("k", js::num(*k)),
                ]))]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // chat.test.ts
    #[test]
    fn reads_only_well_formed_chat_bodies() {
        assert_eq!(
            chat_text(&json!({"k": "chat", "text": " hi "})).as_deref(),
            Some("hi")
        );
        assert_eq!(chat_text(&json!({"k": "snap.req"})), None);
        assert_eq!(chat_text(&json!({"k": "chat", "text": 5})), None);
        assert_eq!(chat_text(&json!({"k": "chat", "text": "   "})), None);
        assert_eq!(chat_text(&Value::Null), None);
        assert_eq!(chat_text(&json!("chat")), None);
    }

    #[test]
    fn cuts_long_messages_and_drops_control_characters() {
        let long = chat_text(&json!({"k": "chat", "text": "x".repeat(900)})).unwrap();
        assert_eq!(long.len(), MAX_CHAT_CHARS);
        assert_eq!(
            chat_text(&json!({"k": "chat", "text": "a\u{0}b\u{7}c"})).as_deref(),
            Some("abc")
        );
        assert_eq!(
            chat_text(&json!({"k": "chat", "text": "<b>x</b>"})).as_deref(),
            Some("<b>x</b>")
        );
    }

    #[test]
    fn makes_what_to_send_from_typed_text() {
        assert_eq!(chat_body("  "), None);
        assert_eq!(
            chat_body(" hello "),
            Some(json!({"k": "chat", "text": "hello"}))
        );
        assert_eq!(
            chat_body(&"y".repeat(600)).unwrap()["text"]
                .as_str()
                .unwrap()
                .len(),
            MAX_CHAT_CHARS
        );
    }

    #[test]
    fn keeps_the_newest_lines() {
        let lines: Vec<usize> = (0..KEEP_CHAT + 5).collect();
        let kept = cap_lines(lines, KEEP_CHAT);
        assert_eq!(kept.len(), KEEP_CHAT);
        assert_eq!(kept[0], 5);
        assert_eq!(cap_lines(vec![1, 2], KEEP_CHAT), vec![1, 2]);
    }

    // share.test.ts (the parts that aren't the window's)
    #[test]
    fn gives_each_member_the_same_colour_everywhere() {
        assert_eq!(member_colour("abc"), member_colour("abc"));
        // (the studio's pick for these ids)
        assert_eq!(member_colour("abc"), COLOURS[96354 % 8]);
    }

    #[test]
    fn finds_the_invite_code_in_pasted_text() {
        assert_eq!(
            clean_invite("join me:\n pp1-eyJw_b3-xyz  thanks!").as_deref(),
            Some("pp1-eyJw_b3-xyz")
        );
        assert_eq!(clean_invite("hello"), None);
    }

    #[test]
    fn checks_cursor_messages() {
        assert_eq!(cursor(&json!({"x": 1, "y": 2})), Some((1.0, 2.0)));
        assert_eq!(cursor(&json!({"x": "a", "y": 2})), None);
        assert_eq!(cursor(&Value::Null), None);
    }

    #[test]
    fn keeps_the_newest_cursor_of_each_member_and_drops_stale_ones() {
        let mut c = Cursors::new(1000.0);
        assert!(c.set("a", 1.0, 2.0, 0.0));
        assert!(!c.set("a", 1.0, 2.0, 10.0));
        assert!(c.set("b", 5.0, 5.0, 500.0));
        let list = c.list(&[("a".into(), "Ana".into())]);
        let names: Vec<(String, String)> = list
            .iter()
            .map(|x| (x.id.clone(), x.name.clone()))
            .collect();
        assert_eq!(
            names,
            vec![("a".into(), "Ana".into()), ("b".into(), "Someone".into())]
        );
        assert!(c.prune(1200.0));
        assert_eq!(
            c.list(&[])
                .iter()
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        assert!(c.remove("b"));
        assert!(c.list(&[]).is_empty());
    }

    #[test]
    fn live_strokes_go_in_pieces() {
        let e = LiveEvent::Points {
            k: 3.0,
            pts: (0..200).map(f64::from).collect(),
        };
        let b = e.bodies();
        assert_eq!(b.len(), 3);
        assert_eq!(b[2]["live"]["pts"].as_array().unwrap().len(), 8);
        assert_eq!(
            LiveEvent::read(&b[0]),
            Some(LiveEvent::Points {
                k: 3.0,
                pts: (0..96).map(f64::from).collect()
            })
        );
        assert_eq!(
            LiveEvent::read(&json!({"live": {"type": "end", "k": 1}})),
            Some(LiveEvent::End { k: 1.0 })
        );
        assert_eq!(LiveEvent::read(&json!({"x": 1, "y": 2})), None);
    }
}
