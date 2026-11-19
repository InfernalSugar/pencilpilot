//! `ref-service recheck [--dry-run]`: run the topic check (ADR-0009) over
//! every picture already in the pool, re-filing or removing the ones that
//! are in the wrong place. Stop the running service first: it keeps the pool
//! in memory and would overwrite the result on its next save.

use std::collections::BTreeMap;
use std::path::Path;

use crate::model::kind_allowed;
use crate::pool::PoolState;
use crate::reviews::{Effective, Outcome, Place};
use crate::sorting::{TopicCheck, destination};
use ref_filters::sort::Decision;

/// What happened to one picture.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Refiled { to: String },
    Removed { reason: String },
}

#[derive(Debug, Default)]
pub struct Report {
    /// "category/topic" → (before, kept, refiled out, refiled in, removed)
    pub per_topic: BTreeMap<String, [usize; 5]>,
    /// (id, from "category/topic", change, first tags)
    pub changes: Vec<(String, String, Change, String)>,
    /// pictures whose file is missing or unreadable (left alone)
    pub unreadable: Vec<String>,
    /// the pool's pictures were sorted under other rules (or none), so all were checked
    pub rules_changed: bool,
}

impl Report {
    pub fn after(&self, key: &str) -> usize {
        self.per_topic
            .get(key)
            .map_or(0, |c| c[0] - c[2] - c[4] + c[3])
    }

    pub fn print(&self) {
        println!(
            "{:<26} {:>6} {:>6} {:>8} {:>8} {:>8} {:>6}",
            "category/topic", "before", "kept", "out", "in", "removed", "after"
        );
        let mut tot = [0usize; 5];
        for (k, c) in &self.per_topic {
            println!(
                "{k:<26} {:>6} {:>6} {:>8} {:>8} {:>8} {:>6}",
                c[0],
                c[1],
                c[2],
                c[3],
                c[4],
                self.after(k)
            );
            for (t, x) in tot.iter_mut().zip(c) {
                *t += x;
            }
        }
        println!(
            "{:<26} {:>6} {:>6} {:>8} {:>8} {:>8} {:>6}",
            "total",
            tot[0],
            tot[1],
            tot[2],
            tot[3],
            tot[4],
            tot[0] - tot[4]
        );
        if !self.unreadable.is_empty() {
            println!("{} unreadable (left alone)", self.unreadable.len());
        }
    }
}

/// Check the pictures in `pool` (images in `images/<id>.jpg`): all of them,
/// or with `only_unsorted` just those not in `pool.sorted` yet (all of them
/// again if `pool.sorted_rules` isn't `check`'s current rules). With
/// `apply`, re-file and remove (deleting the files of removed pictures) and
/// remember the checked ones and the rules in `pool.sorted` and
/// `pool.sorted_rules`. Unreadable pictures and model errors leave a picture
/// alone.
pub fn recheck(
    pool: &mut PoolState,
    images: &Path,
    check: &dyn TopicCheck,
    apply: bool,
    only_unsorted: bool,
    mut progress: impl FnMut(usize, usize),
) -> Report {
    let mut rep = Report::default();
    // New sorting rules (prompts, cut-offs): what was checked under the old
    // ones counts as unchecked, so a restart re-checks the whole pool once.
    let rules = check.rules();
    rep.rules_changed = rules.is_some() && pool.sorted_rules != rules;
    let all = !only_unsorted || rep.rules_changed;
    let mut ids: Vec<String> = pool
        .refs
        .keys()
        .filter(|id| all || !pool.sorted.contains(*id))
        // the site owner's decision wins over any rule change
        .filter(|id| !pool.reviewed.contains(*id))
        .cloned()
        .collect();
    if apply && all {
        pool.sorted.clear();
    }
    if apply && rules.is_some() {
        pool.sorted_rules = rules;
    }
    ids.sort();
    let mut remove = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        progress(i, ids.len());
        let r = pool.refs[id].clone();
        let from = format!("{}/{}", r.category, r.topic);
        rep.per_topic.entry(from.clone()).or_default()[0] += 1;
        let decision = std::fs::read(images.join(format!("{id}.jpg")))
            .ok()
            .and_then(|b| ref_filters::decode(&b).ok())
            .and_then(|img| check.check(&img, &r.category, &r.topic).ok());
        let Some(decision) = decision else {
            rep.unreadable.push(id.clone());
            rep.per_topic.get_mut(&from).unwrap()[1] += 1;
            continue;
        };
        let checked = decision != Decision::Unchecked;
        let tags = r
            .tags
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        match destination(decision, &r.category, &r.topic) {
            Ok((c, t)) if c == r.category && t == r.topic => {
                rep.per_topic.get_mut(&from).unwrap()[1] += 1;
            }
            Ok((c, t)) if kind_allowed(&c, r.kind()) => {
                let to = format!("{c}/{t}");
                rep.per_topic.get_mut(&from).unwrap()[2] += 1;
                rep.per_topic.entry(to.clone()).or_default()[3] += 1;
                rep.changes
                    .push((id.clone(), from, Change::Refiled { to }, tags));
                if apply {
                    pool.refile(id, &c, &t);
                }
            }
            other => {
                let reason = match other {
                    Err(why) => why,
                    Ok((c, t)) => format!("wrong_kind_for:{c}/{t}"),
                };
                rep.per_topic.get_mut(&from).unwrap()[4] += 1;
                rep.changes
                    .push((id.clone(), from, Change::Removed { reason }, tags));
                remove.push(id.clone());
                continue;
            }
        }
        if apply && checked {
            pool.sorted.insert(id.clone());
        }
    }
    if apply && !remove.is_empty() {
        let gone = pool.remove_where(|r| remove.contains(&r.id));
        for r in gone {
            std::fs::remove_file(images.join(format!("{}.jpg", r.id))).ok();
        }
    }
    rep
}

/// `recheck --labels`: how often the sorter, run now, agrees with the site
/// owner's review decisions.
#[derive(Debug, Default)]
pub struct Agreement {
    /// "category/topic" (where the sorter had filed it) → counts[owner][sorter],
    /// each 0 keep, 1 move, 2 reject
    pub per_topic: BTreeMap<String, [[usize; 3]; 3]>,
    /// "category/topic" → pictures where the sorter does exactly what the
    /// owner did (a move must go to the same topic)
    pub exact: BTreeMap<String, usize>,
    /// (id, from, owner's outcome, sorter's outcome)
    pub disagreements: Vec<(String, String, String, String)>,
    /// reviewed pictures whose file is gone
    pub missing: Vec<String>,
}

/// Run `check` over every reviewed picture (file from `image_for`) at the
/// place the sorter had put it, and compare with the owner's outcome.
pub fn agreement(
    labels: &std::collections::HashMap<String, Effective>,
    image_for: impl Fn(&str) -> Option<std::path::PathBuf>,
    check: &dyn TopicCheck,
) -> Agreement {
    let mut out = Agreement::default();
    let mut ids: Vec<&String> = labels.keys().collect();
    ids.sort();
    for id in ids {
        let e = &labels[id];
        let from = e.from.key();
        let decision = image_for(id)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| ref_filters::decode(&b).ok())
            .and_then(|img| check.check(&img, &e.from.category, &e.from.topic).ok());
        let Some(decision) = decision else {
            out.missing.push(id.clone());
            continue;
        };
        let sorter = match destination(decision, &e.from.category, &e.from.topic) {
            Ok((c, t)) if c == e.from.category && t == e.from.topic => Outcome::Keep,
            Ok((c, t)) if kind_allowed(&c, &e.kind) => Outcome::Move(Place {
                category: c,
                topic: t,
                tier: e.from.tier,
            }),
            _ => Outcome::Reject,
        };
        out.per_topic.entry(from.clone()).or_default()[e.outcome.index()][sorter.index()] += 1;
        let same = match (&e.outcome, &sorter) {
            (Outcome::Move(a), Outcome::Move(b)) => a.key() == b.key(),
            (a, b) => a.index() == b.index(),
        };
        let n = out.exact.entry(from.clone()).or_default();
        if same {
            *n += 1;
        } else {
            out.disagreements
                .push((id.clone(), from, e.outcome.label(), sorter.label()));
        }
    }
    out
}

impl Agreement {
    pub fn print(&self) {
        for (id, from, owner, sorter) in &self.disagreements {
            println!("{id:<14} {from:<24} owner {owner:<28} sorter {sorter}");
        }
        if !self.disagreements.is_empty() {
            println!();
        }
        println!("rows: the owner's decision; columns: the sorter's (k keep, m move, r reject)");
        println!(
            "{:<26} {:>5} {:>7}   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}",
            "category/topic",
            "n",
            "agree",
            "k→k",
            "k→m",
            "k→r",
            "m→k",
            "m→m",
            "m→r",
            "r→k",
            "r→m",
            "r→r"
        );
        let mut tot = [[0usize; 3]; 3];
        let mut tot_exact = 0;
        for (k, m) in &self.per_topic {
            let n: usize = m.iter().flatten().sum();
            let ex = self.exact.get(k).copied().unwrap_or(0);
            tot_exact += ex;
            println!(
                "{k:<26} {n:>5} {:>6.0}%   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}",
                100.0 * ex as f64 / n.max(1) as f64,
                m[0][0],
                m[0][1],
                m[0][2],
                m[1][0],
                m[1][1],
                m[1][2],
                m[2][0],
                m[2][1],
                m[2][2]
            );
            for (i, row) in m.iter().enumerate() {
                for (j, x) in row.iter().enumerate() {
                    tot[i][j] += x;
                }
            }
        }
        let n: usize = tot.iter().flatten().sum();
        println!(
            "{:<26} {n:>5} {:>6.0}%   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}   {:>4} {:>4} {:>4}",
            "total",
            100.0 * tot_exact as f64 / n.max(1) as f64,
            tot[0][0],
            tot[0][1],
            tot[0][2],
            tot[1][0],
            tot[1][1],
            tot[1][2],
            tot[2][0],
            tot[2][1],
            tot[2][2]
        );
        if !self.missing.is_empty() {
            println!(
                "{} reviewed pictures without a file (skipped)",
                self.missing.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LiveRef;
    use crate::sorting::tests::FixedCheck;
    use image::DynamicImage;

    fn r(id: &str, cat: &str, topic: &str) -> LiveRef {
        LiveRef {
            id: id.into(),
            source: "pixabay".into(),
            source_id: 1,
            category: cat.into(),
            topic: topic.into(),
            page_url: "https://pixabay.com/illustrations/x-1/".into(),
            author: "a".into(),
            author_url: String::new(),
            license: "l".into(),
            license_url: String::new(),
            tags: vec!["girl".into(), "anime".into()],
            width: 800,
            height: 600,
            title: String::new(),
            attribution: String::new(),
            provider: String::new(),
            kind: String::new(),
            keypoints: vec![],
            difficulty: 20.0,
            tier: 2,
            image_url: String::new(),
            live: true,
        }
    }

    fn setup(tag: &str) -> (PoolState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pp-recheck-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = PoolState::default();
        for id in ["a", "b"] {
            p.add(r(id, "anime", "characters"), None);
            DynamicImage::new_rgb8(64, 64)
                .save_with_format(dir.join(format!("{id}.jpg")), image::ImageFormat::Jpeg)
                .unwrap();
        }
        p.add(r("nofile", "anime", "characters"), None);
        (p, dir)
    }

    #[test]
    fn refiles_with_a_fake_scorer() {
        let (mut p, dir) = setup("refile");
        let check = FixedCheck(Decision::Refile {
            category: "anime".into(),
            topic: "scenery".into(),
            p_from: 0.1,
            p_to: 0.9,
        });
        let dry = recheck(&mut p, &dir, &check, false, false, |_, _| {});
        assert_eq!(dry.changes.len(), 2);
        assert_eq!(p.refs["a"].topic, "characters", "dry run changes nothing");
        let rep = recheck(&mut p, &dir, &check, true, false, |_, _| {});
        assert_eq!(rep.per_topic["anime/characters"], [3, 1, 2, 0, 0]);
        assert_eq!(rep.per_topic["anime/scenery"], [0, 0, 0, 2, 0]);
        assert_eq!(rep.after("anime/scenery"), 2);
        assert_eq!(rep.unreadable, ["nofile"]);
        assert_eq!(p.refs["a"].topic, "scenery");
        assert!(p.sorted.contains("a") && !p.sorted.contains("nofile"));
        // a second pass over unsorted pictures only looks at the unreadable one
        let again = recheck(&mut p, &dir, &check, true, true, |_, _| {});
        assert_eq!(again.unreadable, ["nofile"]);
        assert!(again.changes.is_empty());
        // without a model nothing is marked as sorted
        let (mut p2, dir2) = setup("unchecked");
        recheck(
            &mut p2,
            &dir2,
            &FixedCheck(Decision::Unchecked),
            true,
            true,
            |_, _| {},
        );
        assert!(p2.sorted.is_empty());
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(dir2).ok();
    }

    /// Keeps everything, under rules with the given id.
    struct KeepUnder(&'static str);

    impl TopicCheck for KeepUnder {
        fn check(&self, _: &DynamicImage, _: &str, _: &str) -> Result<Decision, String> {
            Ok(Decision::Keep { p: 0.9 })
        }

        fn rules(&self) -> Option<String> {
            Some(self.0.into())
        }
    }

    #[test]
    fn new_rules_recheck_everything_once() {
        let (mut p, dir) = setup("rules");
        // a pool sorted before rules were remembered (or under older ones)
        p.sorted.extend(["a".to_string(), "b".to_string()]);
        let v1 = KeepUnder("v1");
        let dry = recheck(&mut p, &dir, &v1, false, true, |_, _| {});
        assert!(dry.rules_changed);
        assert_eq!(
            dry.per_topic["anime/characters"][0], 3,
            "all, not just unsorted"
        );
        assert_eq!(p.sorted_rules, None, "dry run changes nothing");

        let rep = recheck(&mut p, &dir, &v1, true, true, |_, _| {});
        assert!(rep.rules_changed);
        assert_eq!(rep.per_topic["anime/characters"][0], 3);
        assert_eq!(p.sorted_rules.as_deref(), Some("v1"));
        assert!(p.sorted.contains("a") && !p.sorted.contains("nofile"));
        // same rules on the next start: only the unreadable one again
        let rep = recheck(&mut p, &dir, &v1, true, true, |_, _| {});
        assert!(!rep.rules_changed);
        assert_eq!(rep.per_topic["anime/characters"][0], 1);
        // edited prompts: everything once more
        let rep = recheck(&mut p, &dir, &KeepUnder("v2"), true, true, |_, _| {});
        assert!(rep.rules_changed);
        assert_eq!(rep.per_topic["anime/characters"][0], 3);
        assert_eq!(p.sorted_rules.as_deref(), Some("v2"));
        // no model: the rules and the sorted set stay as they are
        recheck(
            &mut p,
            &dir,
            &crate::sorting::NoTopicCheck,
            true,
            true,
            |_, _| {},
        );
        assert_eq!(p.sorted_rules.as_deref(), Some("v2"));
        assert!(p.sorted.contains("a"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reviewed_pictures_survive_a_rules_change() {
        let (mut p, dir) = setup("reviewed");
        p.reviewed.insert("a".into());
        let check = FixedCheck(Decision::Reject {
            reason: "off_topic:anime/scenery".into(),
        });
        let rep = recheck(&mut p, &dir, &check, true, true, |_, _| {});
        assert!(rep.rules_changed);
        assert!(p.refs.contains_key("a"), "the owner kept it");
        assert!(dir.join("a.jpg").exists());
        assert!(!p.refs.contains_key("b"), "unreviewed: the rules apply");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agreement_with_the_owner() {
        use crate::reviews::{Record, effective};
        let (_, dir) = setup("agree");
        let chars = Place {
            category: "anime".into(),
            topic: "characters".into(),
            tier: 2,
        };
        let rec = |seq, id: &str, d: &str, to: Option<Place>| Record {
            seq,
            id: id.into(),
            decision: d.into(),
            undoes: None,
            from: Some(chars.clone()),
            to,
            at: 0,
            source: String::new(),
            kind: "illustration".into(),
            sorter: serde_json::Value::Null,
        };
        let scenery = Place {
            topic: "scenery".into(),
            ..chars.clone()
        };
        let labels = effective(&[
            rec(1, "a", "move", Some(scenery)),
            rec(2, "b", "keep", None),
            rec(3, "nofile", "reject", None),
        ]);
        let check = FixedCheck(Decision::Refile {
            category: "anime".into(),
            topic: "scenery".into(),
            p_from: 0.1,
            p_to: 0.9,
        });
        let ag = agreement(&labels, |id| Some(dir.join(format!("{id}.jpg"))), &check);
        // owner moved a → scenery (sorter agrees), kept b (sorter would move it)
        assert_eq!(ag.per_topic["anime/characters"][1][1], 1);
        assert_eq!(ag.per_topic["anime/characters"][0][1], 1);
        assert_eq!(ag.exact["anime/characters"], 1);
        assert_eq!(ag.disagreements.len(), 1);
        assert_eq!(ag.disagreements[0].0, "b");
        assert_eq!(ag.missing, ["nofile"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn removes_rejected_and_wrong_kind() {
        let (mut p, dir) = setup("remove");
        let check = FixedCheck(Decision::Reject {
            reason: "off_topic:anime/scenery".into(),
        });
        let rep = recheck(&mut p, &dir, &check, true, false, |_, _| {});
        assert_eq!(rep.per_topic["anime/characters"], [3, 1, 0, 0, 2]);
        assert!(!p.refs.contains_key("a"));
        assert!(!dir.join("a.jpg").exists(), "file deleted");
        assert!(p.refs.contains_key("nofile"));

        // a drawing re-filed into a photo-only category is removed instead
        let (mut p, dir2) = setup("kind");
        let check = FixedCheck(Decision::Refile {
            category: "real-life".into(),
            topic: "people".into(),
            p_from: 0.1,
            p_to: 0.9,
        });
        let rep = recheck(&mut p, &dir2, &check, true, false, |_, _| {});
        assert!(
            matches!(&rep.changes[0].2, Change::Removed { reason } if reason.starts_with("wrong_kind_for"))
        );
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(dir2).ok();
    }
}
