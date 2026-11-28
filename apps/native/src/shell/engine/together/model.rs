//! Shared layers and who may draw on each, as every app keeps them (together.ts's
//! `Shared`): one drawer per layer at a time, unless its owner lets others in; asks wait
//! for the owner's answer. Edits that aren't allowed change nothing.
//!
//! This holds the part of the current app's rules the native app takes part in: layers
//! (made, renamed, their look, removed, moved), their tiles, asking, granting, denying and
//! revoking. Groups, masks, line strokes, adjustment layers and sound are left as they are
//! (the native app doesn't make them in a session yet), but kept in snapshots it passes on.

use serde_json::{Value, json};

/// A layer's id (and a drawing's): the maker's first 12 id characters and a number.
pub fn is_lid(s: &str) -> bool {
    let Some((a, b)) = s.split_once(':') else {
        return false;
    };
    a.len() == 12
        && a.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && (1..=12).contains(&b.len())
        && b.bytes().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub lid: String,
    pub owner: String,
    pub name: String,
    pub writers: Vec<String>,
    /// how it shows (set by whoever may draw on it): the current app's `look`, as it came
    pub look: Option<Value>,
    /// the rest as it came (an adjustment's settings, its group, line layers): passed on
    pub rest: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub lid: String,
    pub from: String,
    pub why: String,
    pub own: bool,
    pub with: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Shared {
    pub layers: Vec<Layer>,
    /// groups as they came (passed on in snapshots)
    pub groups: Vec<Value>,
    pub asks: Vec<Ask>,
}

/// What an edit changed, for the app to follow.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Made(String),
    Renamed(String),
    Look(String),
    Removed(String),
    Moved(String),
    /// tiles for a drawing of a layer (allowed)
    Tiles(String),
    Timeline,
    /// who may draw on a layer changed (grant, revoke)
    Access(String),
    Asked(String),
    /// an ask was answered no (for `to`)
    Denied {
        lid: String,
        to: String,
    },
    /// anything else that's allowed (a kind this app doesn't follow)
    Other,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

impl Shared {
    pub fn get(&self, lid: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.lid == lid)
    }

    fn get_mut(&mut self, lid: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.lid == lid)
    }

    /// Bottom → top.
    pub fn order(&self) -> Vec<String> {
        self.layers.iter().map(|l| l.lid.clone()).collect()
    }

    pub fn can_draw(&self, lid: &str, who: &str) -> bool {
        self.get(lid)
            .is_some_and(|l| l.writers.iter().any(|w| w == who))
    }

    pub fn owns(&self, lid: &str, who: &str) -> bool {
        self.get(lid).is_some_and(|l| l.owner == who)
    }

    /// Takes an edit in: what it changed, or None when it isn't allowed (or isn't one).
    pub fn apply(&mut self, from: &str, e: &Value) -> Option<Change> {
        let k = e.get("k")?.as_str()?;
        if matches!(k, "timeline") {
            return Some(Change::Timeline);
        }
        if k == "sound" {
            return Some(Change::Other);
        }
        let lid = s(e, "lid")?;
        let has = self.get(&lid).is_some();
        let writer = self.can_draw(&lid, from);
        let owner = self.owns(&lid, from);
        match k {
            "layer" => {
                let prefix = from.get(..12)?;
                if has || !is_lid(&lid) || !lid.starts_with(&format!("{prefix}:")) {
                    return None;
                }
                let name: String = s(e, "name")?.chars().take(60).collect();
                let mut rest = serde_json::Map::new();
                for key in ["adjust", "lines"] {
                    if let Some(v) = e.get(key) {
                        rest.insert(key.into(), v.clone());
                    }
                }
                self.layers.push(Layer {
                    lid: lid.clone(),
                    owner: from.to_string(),
                    name,
                    writers: vec![from.to_string()],
                    look: None,
                    rest,
                });
                Some(Change::Made(lid))
            }
            "rename" if writer => {
                let name: String = s(e, "name")?.chars().take(60).collect();
                self.get_mut(&lid)?.name = name;
                Some(Change::Renamed(lid))
            }
            "look" if writer => {
                let look = e.get("look")?.clone();
                look.get("blend")?.as_str()?;
                look.get("opacity")?.as_f64()?;
                self.get_mut(&lid)?.look = Some(look);
                Some(Change::Look(lid))
            }
            "tiles" | "mask" | "lines" if writer => Some(if k == "tiles" {
                Change::Tiles(lid)
            } else {
                Change::Other
            }),
            "adjust" | "pixels" if writer => Some(Change::Other),
            "remove" if owner => {
                self.layers.retain(|l| l.lid != lid);
                self.asks
                    .retain(|a| a.lid != lid && a.with.as_deref() != Some(&lid));
                Some(Change::Removed(lid))
            }
            "move" | "place" if (k == "move" && owner) || (k == "place" && writer) => {
                let to = e.get("to")?.as_u64()? as usize;
                let i = self.layers.iter().position(|l| l.lid == lid)?;
                let l = self.layers.remove(i);
                let to = to.min(self.layers.len());
                self.layers.insert(to, l);
                Some(Change::Moved(lid))
            }
            "ask" if has => {
                let own = e.get("own").and_then(Value::as_bool).unwrap_or(false);
                if (own && owner) || (!own && writer) {
                    return None;
                }
                let why = s(e, "why").unwrap_or_else(|| "draw".into());
                if !matches!(why.as_str(), "draw" | "merge" | "group" | "delete") {
                    return None;
                }
                self.asks.retain(|a| !(a.lid == lid && a.from == from));
                self.asks.push(Ask {
                    lid: lid.clone(),
                    from: from.to_string(),
                    why,
                    own,
                    with: s(e, "with").filter(|w| is_lid(w)),
                });
                Some(Change::Asked(lid))
            }
            "grant" if owner => {
                let to = s(e, "to")?;
                let handover = e.get("handover").and_then(Value::as_bool).unwrap_or(false);
                self.asks.retain(|a| !(a.lid == lid && a.from == to));
                let l = self.get_mut(&lid)?;
                if handover {
                    l.owner = to.clone();
                    let rest: Vec<String> = l
                        .writers
                        .iter()
                        .filter(|w| **w != to && *w != from)
                        .cloned()
                        .collect();
                    l.writers = std::iter::once(to).chain(rest).collect();
                } else if !l.writers.contains(&to) {
                    l.writers.push(to);
                }
                Some(Change::Access(lid))
            }
            "deny" if owner => {
                let to = s(e, "to")?;
                self.asks.retain(|a| !(a.lid == lid && a.from == to));
                Some(Change::Denied { lid, to })
            }
            "revoke" if owner => {
                let to = s(e, "to")?;
                let l = self.get_mut(&lid)?;
                if to == l.owner {
                    return None;
                }
                l.writers.retain(|w| *w != to);
                Some(Change::Access(lid))
            }
            // groups: allowed or not by rules this app doesn't keep; passed over
            "group" | "gset" | "ungroup" | "block" => None,
            _ => None,
        }
    }

    /// The layers, groups and asks as `snap.begin` carries them (together.ts's `snapshot`).
    pub fn snapshot(&self) -> Value {
        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|l| {
                let mut o = l.rest.clone();
                o.insert("lid".into(), json!(l.lid));
                o.insert("owner".into(), json!(l.owner));
                o.insert("name".into(), json!(l.name));
                o.insert("writers".into(), json!(l.writers));
                if let Some(look) = &l.look {
                    o.insert("look".into(), look.clone());
                }
                Value::Object(o)
            })
            .collect();
        let asks: Vec<Value> = self
            .asks
            .iter()
            .map(|a| {
                let mut o = json!({"lid": a.lid, "from": a.from, "why": a.why});
                if a.own {
                    o["own"] = json!(true);
                }
                if let Some(w) = &a.with {
                    o["with"] = json!(w);
                }
                o
            })
            .collect();
        json!({"layers": layers, "groups": self.groups, "asks": asks})
    }

    /// From a `snap.begin` (together.ts's `load`).
    pub fn load(&mut self, v: &Value) {
        self.layers.clear();
        self.asks.clear();
        self.groups = v
            .get("groups")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for l in v
            .get("layers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(lid) = s(l, "lid").filter(|x| is_lid(x)) else {
                continue;
            };
            let Some(writers) = l.get("writers").and_then(Value::as_array) else {
                continue;
            };
            if self.get(&lid).is_some() {
                continue;
            }
            let mut rest = l.as_object().cloned().unwrap_or_default();
            for k in ["lid", "owner", "name", "writers", "look"] {
                rest.remove(k);
            }
            self.layers.push(Layer {
                lid,
                owner: s(l, "owner").unwrap_or_default(),
                name: s(l, "name").unwrap_or_default().chars().take(60).collect(),
                writers: writers
                    .iter()
                    .filter_map(|w| w.as_str().map(str::to_string))
                    .collect(),
                look: l.get("look").cloned(),
                rest,
            });
        }
        for a in v
            .get("asks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(lid), Some(from)) = (s(a, "lid"), s(a, "from")) else {
                continue;
            };
            if self.get(&lid).is_none() {
                continue;
            }
            self.asks.push(Ask {
                lid,
                from,
                why: s(a, "why").unwrap_or_else(|| "draw".into()),
                own: a.get("own").and_then(Value::as_bool).unwrap_or(false),
                with: s(a, "with"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaa0000000000000000000000000000000000000000000000000000";
    const B: &str = "bbbbbbbbbbbb0000000000000000000000000000000000000000000000000000";

    #[test]
    fn one_drawer_per_layer_until_its_owner_lets_others_in() {
        let mut sh = Shared::default();
        let l = "aaaaaaaaaaaa:7";
        assert!(
            sh.apply(B, &json!({"k": "layer", "lid": l, "name": "x"}))
                .is_none(),
            "only the member whose id starts it"
        );
        assert_eq!(
            sh.apply(A, &json!({"k": "layer", "lid": l, "name": "Ana"})),
            Some(Change::Made(l.into()))
        );
        let tiles = json!({"k": "tiles", "lid": l, "did": l, "set": [], "clear": []});
        assert!(sh.apply(B, &tiles).is_none(), "locked for B");
        assert!(sh.apply(A, &tiles).is_some());
        // B asks; A lets them draw; A takes it back
        assert!(
            sh.apply(
                B,
                &json!({"k": "ask", "lid": l, "why": "draw", "own": false})
            )
            .is_some()
        );
        assert_eq!(sh.asks.len(), 1);
        assert!(
            sh.apply(
                B,
                &json!({"k": "grant", "lid": l, "to": B, "handover": false})
            )
            .is_none(),
            "only the owner answers"
        );
        sh.apply(
            A,
            &json!({"k": "grant", "lid": l, "to": B, "handover": false}),
        )
        .unwrap();
        assert!(sh.asks.is_empty() && sh.can_draw(l, B));
        sh.apply(A, &json!({"k": "revoke", "lid": l, "to": B}))
            .unwrap();
        assert!(!sh.can_draw(l, B));
        // handing over
        sh.apply(
            A,
            &json!({"k": "grant", "lid": l, "to": B, "handover": true}),
        )
        .unwrap();
        assert!(sh.owns(l, B) && !sh.can_draw(l, A));
        // a snapshot loads back the same
        let mut other = Shared::default();
        other.load(&sh.snapshot());
        assert_eq!(other.layers, sh.layers);
    }
}
