//! Shared layers and who may draw on each: `pencilpilot_together`'s [`Shared`] (the
//! current app's rules, edit for edit), and what each edit it lets through changes for the
//! native app to follow.

use pencilpilot_together::{Edit, SharedLayer, parse_edit};
use serde_json::{Value, json};

pub use pencilpilot_together::Ask;

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
    /// a layer's mask (kept as it came; shown)
    Mask(String),
    /// a drawing's line strokes (kept as they came: its pixels come as tiles)
    Lines(String, String),
    /// a piece of a sound file (kept as it came)
    Sound,
    /// anything else that's allowed (kept in the shared state, not drawn here: groups,
    /// adjustment settings)
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct Shared(pub pencilpilot_together::Shared);

impl Shared {
    pub fn get(&self, lid: &str) -> Option<&SharedLayer> {
        self.0.layers.get(lid)
    }

    /// Bottom → top.
    pub fn order(&self) -> Vec<String> {
        self.0.order.clone()
    }

    pub fn len(&self) -> usize {
        self.0.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.layers.is_empty()
    }

    pub fn asks(&self) -> &[Ask] {
        &self.0.asks
    }

    pub fn can_draw(&self, lid: &str, who: &str) -> bool {
        self.0.can_draw(lid, who)
    }

    pub fn owns(&self, lid: &str, who: &str) -> bool {
        self.0.owns(lid, who)
    }

    /// Takes an edit in (checked against the closed list, then the rules): what it changed,
    /// or None when it isn't one or isn't allowed.
    pub fn apply(&mut self, from: &str, body: &Value) -> Option<Change> {
        let e = parse_edit(body)?;
        if !self.0.apply(from, &e) {
            return None;
        }
        Some(match e {
            Edit::Layer { lid, .. } => Change::Made(lid),
            Edit::Rename { lid, .. } => Change::Renamed(lid),
            Edit::Look { lid, .. } => Change::Look(lid),
            Edit::Remove { lid } => Change::Removed(lid),
            Edit::Move { lid, .. } | Edit::Place { lid, .. } => Change::Moved(lid),
            Edit::Tiles { lid, .. } => Change::Tiles(lid),
            Edit::Timeline { .. } => Change::Timeline,
            Edit::Grant { lid, .. } | Edit::Revoke { lid, .. } => Change::Access(lid),
            Edit::Ask { lid, .. } => Change::Asked(lid),
            Edit::Deny { lid, to } => Change::Denied { lid, to },
            Edit::Mask { lid, .. } => Change::Mask(lid),
            Edit::Lines { lid, did, .. } => Change::Lines(lid, did),
            Edit::Sound { .. } => Change::Sound,
            _ => Change::Other,
        })
    }

    /// The layers, groups and asks as `snap.begin` carries them.
    pub fn snapshot(&self) -> Value {
        let (layers, groups, asks) = self.0.snapshot();
        json!({"layers": layers, "groups": groups, "asks": asks})
    }

    /// From a `snap.begin`.
    pub fn load(&mut self, v: &Value) {
        let list = |k: &str| {
            v.get(k)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        self.0.load(&list("layers"), &list("groups"), &list("asks"));
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
        assert_eq!(sh.asks().len(), 1);
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
        assert!(sh.asks().is_empty() && sh.can_draw(l, B));
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
        assert_eq!(other.0.order, sh.0.order);
        assert_eq!(other.get(l), sh.get(l));
    }
}
