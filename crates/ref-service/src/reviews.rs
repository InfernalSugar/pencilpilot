//! The site owner's picture review (`/dev/pictures`, dev only).
//!
//! Every decision is appended to `data/ref-service/reviews.jsonl` (git-ignored
//! like the rest of `data/`). The log is the source of truth: at startup the
//! ids in it become `PoolState::reviewed`, which rule changes never re-sort or
//! remove, and `ref-service recheck --labels <file>` measures the sorter
//! against it.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pool::Removed;

/// Where a picture is filed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub category: String,
    pub topic: String,
    pub tier: u8,
}

impl Place {
    /// "category/topic" (the tier is not part of sorting)
    pub fn key(&self) -> String {
        format!("{}/{}", self.category, self.topic)
    }
}

/// One line of `reviews.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub seq: u64,
    pub id: String,
    /// `keep`, `reject`, `move`, `tier`, or `undo` (of the record `undoes`)
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<u64>,
    /// where the picture was when the decision was made
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Place>,
    /// where it went (`move`, `tier`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Place>,
    /// unix seconds
    pub at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// photo / vector / illustration / artwork (for the style rules)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// The sorter's verdict at the time: `placed` = whether the sorter put
    /// it at `from` (false: the search did, e.g. pictures from before
    /// ADR-0009 or uncovered topics), `verdict`/`to`/`reason`/`p` = its fresh
    /// check of the picture at `from`, `rules` = the rules fingerprint.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub sorter: Value,
}

/// The owner's final word on a picture, measured against where the sorter
/// filed it.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// left in the topic the sorter chose (a tier change still counts)
    Keep,
    Move(Place),
    Reject,
}

impl Outcome {
    pub fn index(&self) -> usize {
        match self {
            Outcome::Keep => 0,
            Outcome::Move(_) => 1,
            Outcome::Reject => 2,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Outcome::Keep => "keep".into(),
            Outcome::Move(p) => format!("move→{}", p.key()),
            Outcome::Reject => "reject".into(),
        }
    }
}

/// A reviewed picture's state after replaying the log.
#[derive(Debug, Clone)]
pub struct Effective {
    /// where it was at its first (still valid) review: the sorter's placement
    pub from: Place,
    pub outcome: Outcome,
    /// where it is now (`None`: rejected)
    pub place: Option<Place>,
    pub last_seq: u64,
    pub last_decision: String,
    pub last_at: u64,
    pub kind: String,
    /// the sorter's verdict at the first review
    pub sorter: Value,
}

/// Replay `records` (undone ones skipped) into one entry per picture.
pub fn effective(records: &[Record]) -> HashMap<String, Effective> {
    let undone: HashSet<u64> = records.iter().filter_map(|r| r.undoes).collect();
    let mut out: HashMap<String, Effective> = HashMap::new();
    for r in records {
        if r.decision == "undo" || undone.contains(&r.seq) {
            continue;
        }
        let Some(from) = r.from.clone() else {
            continue;
        };
        let place = match r.decision.as_str() {
            "reject" => None,
            "keep" => Some(from.clone()),
            "move" | "tier" => r.to.clone(),
            _ => continue,
        };
        let e = out.entry(r.id.clone()).or_insert_with(|| Effective {
            from: from.clone(),
            outcome: Outcome::Keep,
            place: None,
            last_seq: 0,
            last_decision: String::new(),
            last_at: 0,
            kind: r.kind.clone(),
            sorter: r.sorter.clone(),
        });
        e.place = place;
        e.last_seq = r.seq;
        e.last_decision = r.decision.clone();
        e.last_at = r.at;
        e.outcome = match &e.place {
            None => Outcome::Reject,
            Some(p) if p.key() == e.from.key() => Outcome::Keep,
            Some(p) => Outcome::Move(p.clone()),
        };
    }
    out
}

/// Read a review log; unreadable lines are skipped.
pub fn read_log(path: &Path) -> Vec<Record> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// How to take a decision back (this session only).
#[derive(Debug)]
pub enum UndoAction {
    /// a rejected picture: put it back (and its file)
    Restore(Box<Removed>),
    /// a kept, moved or re-tiered one: back to this place (and block state)
    Place { place: Place, blocked: bool },
}

#[derive(Debug)]
pub struct UndoEntry {
    pub seq: u64,
    pub id: String,
    pub decision: String,
    pub action: UndoAction,
}

/// The review log, plus this session's undo stack.
pub struct Log {
    path: PathBuf,
    pub records: Vec<Record>,
    next_seq: u64,
    pub undo: Vec<UndoEntry>,
}

impl Log {
    pub fn open(path: PathBuf) -> Self {
        let records = read_log(&path);
        let next_seq = records.iter().map(|r| r.seq).max().map_or(1, |m| m + 1);
        Self {
            path,
            records,
            next_seq,
            undo: Vec::new(),
        }
    }

    /// Give `r` the next sequence number and append it to the file.
    pub fn append(&mut self, mut r: Record) -> std::io::Result<u64> {
        r.seq = self.next_seq;
        let line = serde_json::to_string(&r).map_err(std::io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(format!("{line}\n").as_bytes())?;
        self.next_seq += 1;
        let seq = r.seq;
        self.records.push(r);
        Ok(seq)
    }

    pub fn effective(&self) -> HashMap<String, Effective> {
        effective(&self.records)
    }

    /// Every picture with a standing review.
    pub fn reviewed_ids(&self) -> HashSet<String> {
        self.effective().into_keys().collect()
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(c: &str, t: &str, tier: u8) -> Place {
        Place {
            category: c.into(),
            topic: t.into(),
            tier,
        }
    }

    fn rec(seq: u64, id: &str, d: &str, from: Place, to: Option<Place>) -> Record {
        Record {
            seq,
            id: id.into(),
            decision: d.into(),
            undoes: None,
            from: Some(from),
            to,
            at: seq,
            source: "pixabay".into(),
            kind: "illustration".into(),
            sorter: Value::Null,
        }
    }

    #[test]
    fn replay_follows_the_last_valid_decision() {
        let chars = place("anime", "characters", 2);
        let scenery = place("anime", "scenery", 2);
        let mut log = vec![
            rec(1, "a", "keep", chars.clone(), None),
            rec(2, "b", "move", chars.clone(), Some(scenery.clone())),
            rec(3, "c", "reject", chars.clone(), None),
            // re-tiered only: still a keep for the sorter
            rec(
                4,
                "d",
                "tier",
                chars.clone(),
                Some(place("anime", "characters", 3)),
            ),
            // moved, then moved back: a keep
            rec(5, "e", "move", chars.clone(), Some(scenery.clone())),
            rec(6, "e", "move", scenery.clone(), Some(chars.clone())),
            rec(7, "f", "reject", chars.clone(), None),
        ];
        log.push(Record {
            undoes: Some(7),
            from: None,
            ..rec(8, "f", "undo", chars.clone(), None)
        });
        let e = effective(&log);
        assert_eq!(e["a"].outcome, Outcome::Keep);
        assert_eq!(e["b"].outcome, Outcome::Move(scenery.clone()));
        assert_eq!(e["c"].outcome, Outcome::Reject);
        assert_eq!(e["d"].outcome, Outcome::Keep);
        assert_eq!(e["d"].place.as_ref().unwrap().tier, 3);
        assert_eq!(e["e"].outcome, Outcome::Keep);
        assert_eq!(e["e"].from, chars);
        assert!(!e.contains_key("f"), "undone");
    }

    #[test]
    fn log_appends_and_reloads() {
        let dir = std::env::temp_dir().join(format!("pp-reviews-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("reviews.jsonl");
        let mut log = Log::open(path.clone());
        let s1 = log
            .append(rec(0, "a", "keep", place("animals", "pets", 1), None))
            .unwrap();
        let s2 = log
            .append(rec(0, "b", "reject", place("animals", "pets", 1), None))
            .unwrap();
        assert_eq!((s1, s2), (1, 2));
        let again = Log::open(path);
        assert_eq!(again.records.len(), 2);
        assert_eq!(again.next_seq, 3);
        assert_eq!(
            again.reviewed_ids(),
            HashSet::from(["a".to_string(), "b".to_string()])
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
