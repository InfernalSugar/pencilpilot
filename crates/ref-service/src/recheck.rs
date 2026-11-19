//! `ref-service recheck [--dry-run]`: run the topic check (ADR-0009) over
//! every picture already in the pool, re-filing or removing the ones that
//! are in the wrong place. Stop the running service first: it keeps the pool
//! in memory and would overwrite the result on its next save.

use std::collections::BTreeMap;
use std::path::Path;

use crate::model::kind_allowed;
use crate::pool::PoolState;
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
/// or with `only_unsorted` just those not in `pool.sorted` yet. With `apply`,
/// re-file and remove (deleting the files of removed pictures) and remember
/// the checked ones in `pool.sorted`. Unreadable pictures and model errors
/// leave a picture alone.
pub fn recheck(
    pool: &mut PoolState,
    images: &Path,
    check: &dyn TopicCheck,
    apply: bool,
    only_unsorted: bool,
    mut progress: impl FnMut(usize, usize),
) -> Report {
    let mut rep = Report::default();
    let mut ids: Vec<String> = pool
        .refs
        .keys()
        .filter(|id| !only_unsorted || !pool.sorted.contains(*id))
        .cloned()
        .collect();
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
            Ok((c, t)) if kind_allowed(&c, &r.page_url) => {
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
