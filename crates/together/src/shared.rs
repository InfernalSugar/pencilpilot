//! The shared layers and who may draw on them, the same in every app: each app puts
//! every numbered edit through [`Shared::apply`], in the session's order.

use indexmap::IndexMap;
use serde_json::Value;

use crate::edit::{Edit, Look, Why, read_spec};
use crate::js::{self, is_lid, num, slice16};

/// A shared layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedLayer {
    pub lid: String,
    /// who made it (or was handed it)
    pub owner: String,
    pub name: String,
    /// who may draw on it (the owner first)
    pub writers: Vec<String>,
    /// how it shows (set by whoever may draw on it)
    pub look: Option<Look>,
    /// an adjustment layer: its settings (the app's JSON)
    pub adjust: Option<String>,
    /// the group it's in
    pub group: Option<String>,
    /// a line (vector) layer: its strokes travel too (`lines` edits)
    pub lines: bool,
}

/// A shared group of layers (its id is made like a layer's, so ids never clash).
#[derive(Debug, Clone, PartialEq)]
pub struct SharedGroup {
    pub gid: String,
    pub owner: String,
    pub name: String,
    /// who may change it: its settings, and which of their layers are in it
    pub writers: Vec<String>,
    pub visible: bool,
    /// 0–1
    pub opacity: f64,
}

/// An ask waiting for a layer's (or group's) owner.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// the layer or group asked for
    pub lid: String,
    pub from: String,
    pub why: Why,
    /// asks for it to be handed over (merging or deleting it), not just to draw on it
    pub own: bool,
    /// the other layer of a merge
    pub with: Option<String>,
}

impl SharedLayer {
    pub fn to_json(&self) -> Value {
        let mut v = js::object(vec![
            ("lid", self.lid.clone().into()),
            ("owner", self.owner.clone().into()),
            ("name", self.name.clone().into()),
            ("writers", self.writers.clone().into()),
        ]);
        if let Some(l) = &self.look {
            v["look"] = l.to_json();
        }
        if let Some(a) = &self.adjust {
            v["adjust"] = a.clone().into();
        }
        if let Some(g) = &self.group {
            v["group"] = g.clone().into();
        }
        if self.lines {
            v["lines"] = true.into();
        }
        v
    }
}

impl SharedGroup {
    pub fn to_json(&self) -> Value {
        js::object(vec![
            ("gid", self.gid.clone().into()),
            ("owner", self.owner.clone().into()),
            ("name", self.name.clone().into()),
            ("writers", self.writers.clone().into()),
            ("visible", self.visible.into()),
            ("opacity", num(self.opacity)),
        ])
    }
}

impl Ask {
    pub fn to_json(&self) -> Value {
        let mut v = js::object(vec![
            ("lid", self.lid.clone().into()),
            ("from", self.from.clone().into()),
            ("why", self.why.as_str().into()),
        ]);
        if self.own {
            v["own"] = true.into();
        }
        if let Some(w) = &self.with {
            v["with"] = w.clone().into();
        }
        v
    }
}

/// The shared project's layers, groups and asks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shared {
    pub layers: IndexMap<String, SharedLayer>,
    /// bottom → top: the order they were made (and moved)
    pub order: Vec<String>,
    pub groups: IndexMap<String, SharedGroup>,
    /// asks waiting for their owners
    pub asks: Vec<Ask>,
}

/// `id.startsWith(member.slice(0, 12) + ':')`: an id `member` makes.
pub(crate) fn made_by(id: &str, member: &str) -> bool {
    let p: String = member.chars().take(12).collect();
    id.len() > p.len() && id.starts_with(&p) && id.as_bytes()[p.len()] == b':'
}

fn place_in(order: &mut Vec<String>, lid: &str, to: u32) {
    order.retain(|x| x != lid);
    let at = (to as usize).min(order.len());
    order.insert(at, lid.into());
}

impl Shared {
    /// Whether `who` may draw on a layer (or change a group).
    pub fn can_draw(&self, lid: &str, who: &str) -> bool {
        match self.layers.get(lid) {
            Some(l) => l.writers.iter().any(|w| w == who),
            None => self
                .groups
                .get(lid)
                .is_some_and(|g| g.writers.iter().any(|w| w == who)),
        }
    }

    /// Whether `who` owns a layer (or group).
    pub fn owns(&self, lid: &str, who: &str) -> bool {
        self.owner_of(lid) == Some(who)
    }

    /// A layer's (or group's) owner.
    pub fn owner_of(&self, lid: &str) -> Option<&str> {
        match self.layers.get(lid) {
            Some(l) => Some(&l.owner),
            None => self.groups.get(lid).map(|g| g.owner.as_str()),
        }
    }

    /// A group's layers, bottom → top.
    pub fn members(&self, gid: &str) -> Vec<String> {
        self.order
            .iter()
            .filter(|x| self.layers.get(*x).and_then(|l| l.group.as_deref()) == Some(gid))
            .cloned()
            .collect()
    }

    /// Groups left with no layers go.
    fn drop_empty(&mut self) {
        let empty: Vec<String> = self
            .groups
            .keys()
            .filter(|g| self.members(g).is_empty())
            .cloned()
            .collect();
        for gid in empty {
            self.groups.shift_remove(&gid);
            self.asks.retain(|a| a.lid != gid);
        }
    }

    fn writers_mut(&mut self, lid: &str) -> Option<(&mut String, &mut Vec<String>)> {
        match self.layers.get_mut(lid) {
            Some(l) => Some((&mut l.owner, &mut l.writers)),
            None => self
                .groups
                .get_mut(lid)
                .map(|g| (&mut g.owner, &mut g.writers)),
        }
    }

    /// Take an edit in; false when it isn't allowed (and changes nothing).
    pub fn apply(&mut self, from: &str, e: &Edit) -> bool {
        let lid = e.lid();
        let writer = self
            .layers
            .get(lid)
            .is_some_and(|l| l.writers.iter().any(|w| w == from));
        let owner = self.layers.get(lid).is_some_and(|l| l.owner == from);
        match e {
            Edit::Timeline { .. } | Edit::Sound { .. } => true,
            Edit::Layer {
                lid,
                name,
                adjust,
                lines,
            } => {
                // only the member whose id starts the layer's id can make it, once
                if self.layers.contains_key(lid)
                    || self.groups.contains_key(lid)
                    || !made_by(lid, from)
                {
                    return false;
                }
                self.layers.insert(
                    lid.clone(),
                    SharedLayer {
                        lid: lid.clone(),
                        owner: from.into(),
                        name: name.clone(),
                        writers: vec![from.into()],
                        look: None,
                        adjust: adjust.clone(),
                        group: None,
                        lines: *lines,
                    },
                );
                self.order.push(lid.clone());
                true
            }
            Edit::Lines { .. } => {
                writer && self.layers.get(lid).is_some_and(|l| l.adjust.is_none())
            }
            Edit::Pixels { .. } => {
                let Some(l) = self.layers.get_mut(lid).filter(|l| l.lines) else {
                    return false;
                };
                if !writer {
                    return false;
                }
                l.lines = false;
                true
            }
            Edit::Remove { lid } => {
                if !owner {
                    return false;
                }
                self.layers.shift_remove(lid);
                self.order.retain(|x| x != lid);
                self.asks
                    .retain(|a| a.lid != *lid && a.with.as_deref() != Some(lid));
                self.drop_empty();
                true
            }
            Edit::Move { lid, to } => {
                if !owner {
                    return false;
                }
                place_in(&mut self.order, lid, *to);
                true
            }
            Edit::Rename { .. } | Edit::Look { .. } | Edit::Tiles { .. } | Edit::Mask { .. } => {
                if !writer {
                    return false;
                }
                let l = self.layers.get_mut(lid).unwrap();
                match e {
                    Edit::Rename { name, .. } => l.name = name.clone(),
                    Edit::Look { look, .. } => l.look = Some(look.clone()),
                    _ => {}
                }
                true
            }
            Edit::Adjust { spec, .. } => {
                let Some(l) = self.layers.get_mut(lid).filter(|l| l.adjust.is_some()) else {
                    return false;
                };
                if !writer {
                    return false;
                }
                l.adjust = Some(spec.clone());
                true
            }
            Edit::Ask {
                lid,
                why,
                own,
                with,
            } => {
                let Some(owner) = self.owner_of(lid) else {
                    return false;
                };
                if if *own {
                    owner == from
                } else {
                    self.can_draw(lid, from)
                } {
                    return false;
                }
                let ask = Ask {
                    lid: lid.clone(),
                    from: from.into(),
                    why: *why,
                    own: *own,
                    with: with.clone(),
                };
                // (asked again: the newer reason counts)
                self.asks.retain(|a| !(a.lid == *lid && a.from == from));
                self.asks.push(ask);
                true
            }
            Edit::Grant { lid, to, handover } => {
                if self.owner_of(lid) != Some(from) {
                    return false;
                }
                self.asks.retain(|a| !(a.lid == *lid && a.from == *to));
                let (o, w) = self.writers_mut(lid).unwrap();
                if *handover {
                    *o = to.clone();
                    let rest: Vec<String> = w
                        .iter()
                        .filter(|x| *x != to && *x != from)
                        .cloned()
                        .collect();
                    *w = std::iter::once(to.clone()).chain(rest).collect();
                } else if !w.contains(to) {
                    w.push(to.clone());
                }
                true
            }
            Edit::Deny { lid, to } => {
                if self.owner_of(lid) != Some(from) {
                    return false;
                }
                self.asks.retain(|a| !(a.lid == *lid && a.from == *to));
                true
            }
            Edit::Revoke { lid, to } => {
                if self.owner_of(lid) != Some(from) || to == from {
                    return false;
                }
                let (_, w) = self.writers_mut(lid).unwrap();
                w.retain(|x| x != to);
                true
            }
            Edit::Group {
                gid, name, lids, ..
            } => {
                // a new id, the maker's; every layer one the maker may draw on, in one group or none
                if self.layers.contains_key(gid)
                    || self.groups.contains_key(gid)
                    || !made_by(gid, from)
                {
                    return false;
                }
                let mut seen = Vec::new();
                for x in lids {
                    if !seen.contains(x) {
                        seen.push(x.clone());
                    }
                }
                for x in &seen {
                    let Some(m) = self.layers.get(x) else {
                        return false;
                    };
                    if !m.writers.iter().any(|w| w == from) {
                        return false;
                    }
                    if let Some(g) = &m.group
                        && !self.can_draw(g, from)
                    {
                        return false;
                    }
                }
                self.groups.insert(
                    gid.clone(),
                    SharedGroup {
                        gid: gid.clone(),
                        owner: from.into(),
                        name: name.clone(),
                        writers: vec![from.into()],
                        visible: true,
                        opacity: 1.0,
                    },
                );
                for x in &seen {
                    self.layers.get_mut(x).unwrap().group = Some(gid.clone());
                }
                self.drop_empty();
                true
            }
            Edit::GroupSet {
                lid,
                name,
                visible,
                opacity,
            } => {
                let Some(g) = self
                    .groups
                    .get_mut(lid)
                    .filter(|g| g.writers.iter().any(|w| w == from))
                else {
                    return false;
                };
                g.name = name.clone();
                g.visible = *visible;
                g.opacity = *opacity;
                true
            }
            Edit::Place { lid, to, gid } => {
                if !writer {
                    return false;
                }
                if !gid.is_empty() && !self.can_draw(gid, from) {
                    return false;
                }
                let now = self.layers[lid.as_str()].group.clone();
                if let Some(g) = &now
                    && g != gid
                    && !self.can_draw(g, from)
                {
                    return false;
                }
                place_in(&mut self.order, lid, *to);
                self.layers.get_mut(lid).unwrap().group = (!gid.is_empty()).then(|| gid.clone());
                self.drop_empty();
                true
            }
            Edit::Ungroup { lid } => {
                if self.groups.get(lid).is_none_or(|g| g.owner != from) {
                    return false;
                }
                for x in self.members(lid) {
                    self.layers.get_mut(&x).unwrap().group = None;
                }
                self.groups.shift_remove(lid);
                self.asks.retain(|a| a.lid != *lid);
                true
            }
            Edit::Block { lid, to } => {
                if self
                    .groups
                    .get(lid)
                    .is_none_or(|g| !g.writers.iter().any(|w| w == from))
                {
                    return false;
                }
                let m = self.members(lid);
                let mut rest: Vec<String> = self
                    .order
                    .iter()
                    .filter(|x| !m.contains(x))
                    .cloned()
                    .collect();
                let at = (*to as usize).min(rest.len());
                rest.splice(at..at, m);
                self.order = rest;
                true
            }
        }
    }

    /// The layers (in order), groups and asks, as a joiner gets them (`snap.begin`) and
    /// the project file keeps them.
    pub fn snapshot(&self) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
        (
            self.order
                .iter()
                .map(|lid| self.layers[lid.as_str()].to_json())
                .collect(),
            self.groups.values().map(SharedGroup::to_json).collect(),
            self.asks.iter().map(Ask::to_json).collect(),
        )
    }

    /// Back from a snapshot (someone else's: everything checked).
    pub fn load(&mut self, layers: &[Value], groups: &[Value], asks: &[Value]) {
        self.layers.clear();
        self.groups.clear();
        self.order.clear();
        for g in groups {
            let Some(gid) = js::str_if(g.get("gid"), is_lid) else {
                continue;
            };
            let Some(writers) = g.get("writers").and_then(Value::as_array) else {
                continue;
            };
            self.groups.insert(
                gid.into(),
                SharedGroup {
                    gid: gid.into(),
                    owner: js::string(g.get("owner")),
                    name: slice16(&js::string(g.get("name")), 60),
                    writers: writers.iter().map(|w| js::string(Some(w))).collect(),
                    visible: g.get("visible") != Some(&Value::Bool(false)),
                    opacity: js::finite(g.get("opacity")).map_or(1.0, |o| o.clamp(0.0, 1.0)),
                },
            );
        }
        for l in layers {
            let Some(lid) = js::str_if(l.get("lid"), is_lid) else {
                continue;
            };
            if self.layers.contains_key(lid) || self.groups.contains_key(lid) {
                continue;
            }
            // (together.ts stops reading here, mid-snapshot; a layer without writers is
            // left out instead)
            let Some(writers) = l.get("writers").and_then(Value::as_array) else {
                continue;
            };
            let adjust = read_spec(l.get("adjust"));
            let group = l
                .get("group")
                .and_then(Value::as_str)
                .filter(|g| self.groups.contains_key(*g))
                .map(str::to_string);
            self.layers.insert(
                lid.into(),
                SharedLayer {
                    lid: lid.into(),
                    owner: js::string(l.get("owner")),
                    name: slice16(&js::string(l.get("name")), 60),
                    writers: writers.iter().map(|w| js::string(Some(w))).collect(),
                    look: l.get("look").and_then(Look::read),
                    lines: l.get("lines") == Some(&Value::Bool(true)) && adjust.is_none(),
                    adjust,
                    group,
                },
            );
            self.order.push(lid.into());
        }
        self.drop_empty();
        self.asks = asks
            .iter()
            .filter(|a| {
                a.get("lid")
                    .and_then(Value::as_str)
                    .is_some_and(|l| self.layers.contains_key(l) || self.groups.contains_key(l))
            })
            .map(|a| Ask {
                lid: a["lid"].as_str().unwrap().into(),
                from: js::string(a.get("from")),
                why: Why::read(a.get("why")).unwrap_or_default(),
                own: js::truthy(a.get("own")),
                with: js::str_if(a.get("with"), is_lid).map(str::to_string),
            })
            .collect();
    }
}
