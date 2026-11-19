//! `ref-importer sort [--dry-run]`: check the committed manifests with the
//! picture sorter (ADR-0009, a local CLIP model; sorting only, never
//! grading). Tag rules (`topic_for`) give the first guess, the model has the
//! final say: each reference is kept, re-filed (topic, or category within the
//! same group, moving it to that category's file) or removed (its id goes to
//! `rejected.json` so it is never fetched again).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ref_filters::clip::{ClipText, ClipVision};
use ref_filters::sort::{Decision, SortSpec, Sorter};

use crate::{
    CATEGORIES, RefList, Rejected, images_dir, kind_allowed, read_json, refs_dir, root, topics,
    write_json,
};

/// One change to a manifest.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Refiled { to: String },
    Removed { reason: String },
}

/// Apply `decide` to every reference in `lists` (file name → list).
/// `decide` returns `None` when the picture can't be checked (left alone).
/// Category files hold one category each, so a re-filed approved reference
/// moves to `<new category>.json`; candidates stay in candidates.json.
/// Returns the changes as (file, id, from, change).
pub fn apply(
    lists: &mut BTreeMap<String, RefList>,
    mut decide: impl FnMut(&crate::Reference) -> Option<Decision>,
) -> Vec<(String, String, String, Change)> {
    let mut changes = Vec::new();
    let mut moved: Vec<(String, crate::Reference)> = Vec::new();
    for (file, list) in lists.iter_mut() {
        let mut keep = Vec::new();
        for mut r in std::mem::take(&mut list.references) {
            let from = format!("{}/{}", r.category, r.topic);
            let d = decide(&r);
            let dest = match d {
                None | Some(Decision::Keep { .. }) | Some(Decision::Unchecked) => {
                    keep.push(r);
                    continue;
                }
                Some(Decision::Refile {
                    category, topic, ..
                }) if kind_allowed(&category, &r.page_url) => Ok((category, topic)),
                Some(Decision::Refile {
                    category, topic, ..
                }) => Err(format!("wrong_kind_for:{category}/{topic}")),
                Some(Decision::Reject { reason }) => Err(reason),
            };
            match dest {
                Ok((c, t)) => {
                    changes.push((
                        file.clone(),
                        r.id.clone(),
                        from,
                        Change::Refiled {
                            to: format!("{c}/{t}"),
                        },
                    ));
                    let other_file = file != "candidates.json" && c != r.category;
                    r.category = c;
                    r.topic = t;
                    if other_file {
                        moved.push((format!("{}.json", r.category), r));
                    } else {
                        keep.push(r);
                    }
                }
                Err(reason) => {
                    changes.push((file.clone(), r.id.clone(), from, Change::Removed { reason }))
                }
            }
        }
        list.references = keep;
    }
    for (file, r) in moved {
        lists.entry(file).or_default().references.push(r);
    }
    changes
}

fn model_dir() -> PathBuf {
    std::env::var("REF_CLIP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("crates/ref-filters/models/clip-vit-b32"))
}

pub fn run(dry_run: bool) {
    let dir = model_dir();
    let spec = SortSpec::from_topics_json(&topics()).unwrap_or_else(|e| panic!("{e}"));
    let text = ClipText::load(&dir).unwrap_or_else(|e| {
        panic!("{e}\nRun crates/ref-filters/fetch-model.sh clip first (or set REF_CLIP_DIR).")
    });
    let sorter = Sorter::build(spec, |p| text.embed(p)).expect("embed prompts");
    drop(text);
    let vision = ClipVision::load(&dir).expect("CLIP vision tower");

    let files: Vec<String> = CATEGORIES
        .iter()
        .map(|c| format!("{c}.json"))
        .chain(["candidates.json".to_string()])
        .collect();
    let mut lists: BTreeMap<String, RefList> = files
        .iter()
        .filter(|f| refs_dir().join(f).exists())
        .map(|f| (f.clone(), read_json(&refs_dir().join(f))))
        .collect();
    let mut before: BTreeMap<String, usize> = BTreeMap::new();
    for r in lists.values().flat_map(|l| &l.references) {
        *before
            .entry(format!("{}/{}", r.category, r.topic))
            .or_default() += 1;
    }
    let mut unreadable = Vec::new();
    let changes = apply(&mut lists, |r| {
        let img = image_of(&images_dir(), &r.id);
        let Some(img) = img else {
            unreadable.push(r.id.clone());
            return None;
        };
        let e = vision.embed(&img).ok()?;
        Some(sorter.decide(&e, &r.category, &r.topic))
    });
    for (file, id, from, c) in &changes {
        println!("{file:<17} {id:<12} {from:<24} {c:?}");
    }
    let mut after: BTreeMap<String, usize> = BTreeMap::new();
    for r in lists.values().flat_map(|l| &l.references) {
        *after
            .entry(format!("{}/{}", r.category, r.topic))
            .or_default() += 1;
    }
    println!("\n{:<26} {:>6} {:>6}", "category/topic", "before", "after");
    let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for k in keys {
        println!(
            "{k:<26} {:>6} {:>6}",
            before.get(k).copied().unwrap_or(0),
            after.get(k).copied().unwrap_or(0)
        );
    }
    let removed = changes
        .iter()
        .filter(|c| matches!(c.3, Change::Removed { .. }))
        .count();
    println!(
        "{} checked, {} re-filed, {removed} removed, {} without a picture file (run `sync`)",
        before.values().sum::<usize>(),
        changes.len() - removed,
        unreadable.len()
    );
    if dry_run {
        println!("dry run: nothing written");
        return;
    }
    for (f, l) in &lists {
        write_json(&refs_dir().join(f), l);
    }
    if removed > 0 {
        let path = refs_dir().join("rejected.json");
        let mut rej: Rejected = read_json(&path);
        for (_, id, _, c) in &changes {
            if matches!(c, Change::Removed { .. }) && !rej.ids.contains(id) {
                rej.ids.push(id.clone());
            }
        }
        write_json(&path, &rej);
    }
}

fn image_of(dir: &Path, id: &str) -> Option<image::DynamicImage> {
    let bytes = std::fs::read(dir.join(format!("{id}.jpg"))).ok()?;
    ref_filters::decode(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, cat: &str, topic: &str, kind: &str) -> crate::Reference {
        serde_json::from_value(serde_json::json!({
            "id": id, "source": "pixabay", "sourceId": 1, "category": cat, "topic": topic,
            "pageUrl": format!("https://pixabay.com/{kind}/x-1/"), "author": "a", "authorUrl": "",
            "license": "l", "licenseUrl": "", "tags": [], "width": 1, "height": 1, "query": ""
        }))
        .unwrap()
    }

    #[test]
    fn keeps_refiles_moves_and_removes() {
        let mut lists = BTreeMap::new();
        lists.insert(
            "anime.json".to_string(),
            RefList {
                references: vec![
                    r("keep", "anime", "characters", "vectors"),
                    r("scene", "anime", "characters", "vectors"),
                    r("toon", "anime", "chibi", "vectors"),
                    r("bad", "anime", "action", "vectors"),
                    r("nofile", "anime", "action", "vectors"),
                ],
            },
        );
        lists.insert(
            "candidates.json".to_string(),
            RefList {
                references: vec![r("cand", "anime", "chibi", "vectors")],
            },
        );
        let refile = |c: &str, t: &str| Decision::Refile {
            category: c.into(),
            topic: t.into(),
            p_from: 0.0,
            p_to: 1.0,
        };
        let changes = apply(&mut lists, |r| match r.id.as_str() {
            "keep" => Some(Decision::Keep { p: 0.9 }),
            "scene" => Some(refile("anime", "scenery")),
            "toon" | "cand" => Some(refile("cartoon", "animals")),
            "bad" => Some(refile("real-life", "people")), // a drawing can't go to a photo category
            _ => None,
        });
        assert_eq!(changes.len(), 4);
        let ids = |f: &str| {
            lists[f]
                .references
                .iter()
                .map(|r| format!("{}:{}/{}", r.id, r.category, r.topic))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids("anime.json"),
            [
                "keep:anime/characters",
                "scene:anime/scenery",
                "nofile:anime/action"
            ]
        );
        assert_eq!(ids("cartoon.json"), ["toon:cartoon/animals"]);
        assert_eq!(ids("candidates.json"), ["cand:cartoon/animals"]);
        assert!(matches!(&changes.iter().find(|c| c.1 == "bad").unwrap().3,
            Change::Removed { reason } if reason == "wrong_kind_for:real-life/people"));
    }
}
