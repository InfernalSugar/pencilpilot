//! A shared project's session as the project file keeps it (`extra.share`), so it can be
//! rejoined after a break: the invite, who was in it, the shared layers and drawings.

use serde_json::Value;

use crate::js::{self, integer, is_lid};

/// Someone who was in the session.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedMember {
    pub id: String,
    pub name: String,
    /// how to reach them (the session's address, kept as given)
    pub addr: Value,
}

/// `SavedShare`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedShare {
    /// the invite code (it holds the project's secret: anyone with the file can rejoin)
    pub code: String,
    pub members: Vec<SavedMember>,
    /// the shared layers, groups and asks ([`crate::Shared::snapshot`]'s JSON)
    pub layers: Vec<Value>,
    pub groups: Vec<Value>,
    pub asks: Vec<Value>,
    /// shared layer → its id in this document
    pub ids: Vec<(String, u32)>,
    /// shared group → its id in this document
    pub gids: Vec<(String, u32)>,
    /// shared drawing → its id in this document
    pub dids: Vec<(String, u32)>,
    /// the counter new ids are made from
    pub n: u64,
}

fn pairs(v: Option<&Value>) -> Vec<(String, u32)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let p = p.as_array()?;
                    let id = js::str_if(p.first(), is_lid)?;
                    // (ids here are whole numbers; together.ts takes any)
                    let n = integer(p.get(1)).filter(|n| *n >= 0.0 && *n <= u32::MAX as f64)?;
                    Some((id.to_string(), n as u32))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pairs_json(p: &[(String, u32)]) -> Value {
    p.iter()
        .map(|(s, n)| Value::from(vec![Value::from(s.clone()), Value::from(*n)]))
        .collect()
}

impl SavedShare {
    /// `readSavedShare`: a saved session from a file, checked (None if it isn't one).
    pub fn read(raw: &Value) -> Option<SavedShare> {
        let code = raw
            .get("code")?
            .as_str()
            .filter(|c| c.starts_with("pp1-"))?;
        let arr = |k: &str| raw.get(k).and_then(Value::as_array);
        Some(SavedShare {
            code: code.into(),
            members: arr("members")
                .map(|a| {
                    a.iter()
                        .filter_map(|m| {
                            let id = m.get("id")?.as_str()?;
                            Some(SavedMember {
                                id: id.into(),
                                name: js::string(Some(
                                    js::or(m.get("name")).unwrap_or(&Value::from("")),
                                )),
                                addr: m.get("addr").cloned().unwrap_or(Value::Null),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            layers: arr("layers")
                .map(|a| {
                    a.iter()
                        .filter(|l| {
                            js::str_if(l.get("lid"), is_lid).is_some()
                                && l.get("writers").is_some_and(Value::is_array)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            groups: arr("groups").cloned().unwrap_or_default(),
            asks: arr("asks").cloned().unwrap_or_default(),
            ids: pairs(raw.get("ids")),
            gids: pairs(raw.get("gids")),
            dids: pairs(raw.get("dids")),
            n: integer(raw.get("n"))
                .filter(|n| *n >= 0.0)
                .map_or(0, |n| n as u64),
        })
    }

    pub fn to_json(&self) -> Value {
        js::object(vec![
            ("code", self.code.clone().into()),
            (
                "members",
                self.members
                    .iter()
                    .map(|m| {
                        js::object(vec![
                            ("id", m.id.clone().into()),
                            ("name", m.name.clone().into()),
                            ("addr", m.addr.clone()),
                        ])
                    })
                    .collect(),
            ),
            ("layers", self.layers.clone().into()),
            ("groups", self.groups.clone().into()),
            ("asks", self.asks.clone().into()),
            ("ids", pairs_json(&self.ids)),
            ("gids", pairs_json(&self.gids)),
            ("dids", pairs_json(&self.dids)),
            ("n", self.n.into()),
        ])
    }
}
