//! Sorting reference pictures into category and topic by zero-shot matching
//! against short text prompts (CLIP, see [`crate::clip`]). Used **only** to
//! decide where a reference picture belongs, never to score or critique a
//! drawing (ADR-0009, ADR-0002).
//!
//! The prompts and cut-offs live in `content/topics.json`: each topic has a
//! `prompts` list, and the `_sorting` block has the style prompts (photo,
//! anime, cartoon, …), which styles each category allows, which categories
//! may swap pictures (a `group`), and the thresholds.
//!
//! Two checks, both on softmax probabilities of 100 × cosine similarity (the
//! CLIP logit scale), with prompt ensembling (a class is the normalised mean
//! of its prompts' embeddings):
//!
//! 1. **Style**: over all styles; reject if the category's allowed styles
//!    together get less than `minStyle` (e.g. a drawing in Real life).
//! 2. **Topic**: over every topic of every category in the same group. Keep
//!    if the picture's own topic wins. Re-file to the winner if it beats the
//!    own topic by at least `margin` and has at least `minBest`. Otherwise
//!    reject if the own topic has less than `rejectBelow`, else keep (not
//!    sure enough to move it).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cut-offs (camelCase in topics.json).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SortThresholds {
    /// Re-file only if the best topic's probability exceeds the own topic's by this.
    pub margin: f32,
    /// …and the best topic has at least this probability.
    pub min_best: f32,
    /// Reject if the own topic gets less than this (and no confident re-file).
    pub reject_below: f32,
    /// Reject if the category's allowed styles together get less than this.
    pub min_style: f32,
}

impl Default for SortThresholds {
    fn default() -> Self {
        Self {
            margin: 0.25,
            min_best: 0.5,
            reject_below: 0.05,
            min_style: 0.25,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CategorySpec {
    #[serde(default)]
    group: String,
    #[serde(default)]
    styles: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SortingSpec {
    #[serde(flatten)]
    thresholds: SortThresholds,
    #[serde(default)]
    styles: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    categories: BTreeMap<String, CategorySpec>,
}

/// Everything the sorter needs from `content/topics.json`.
#[derive(Debug, Clone, Default)]
pub struct SortSpec {
    pub thresholds: SortThresholds,
    /// style id → prompts
    pub styles: BTreeMap<String, Vec<String>>,
    /// category → (group, allowed style ids)
    pub categories: BTreeMap<String, (String, Vec<String>)>,
    /// (category, topic id, prompts), in file order
    pub topics: Vec<(String, String, Vec<String>)>,
}

impl SortSpec {
    /// Read the `_sorting` block and every topic's `prompts` from the parsed
    /// topics.json. Categories missing from `_sorting.categories` are not checked.
    pub fn from_topics_json(v: &Value) -> Result<Self, String> {
        let s: SortingSpec = serde_json::from_value(v["_sorting"].clone())
            .map_err(|e| format!("topics.json _sorting: {e}"))?;
        let mut topics = Vec::new();
        for cat in s.categories.keys() {
            for t in v[cat.as_str()].as_array().into_iter().flatten() {
                let id = t["id"].as_str().unwrap_or_default().to_string();
                let prompts: Vec<String> = t["prompts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect();
                if prompts.is_empty() {
                    return Err(format!("topics.json: {cat}/{id} has no prompts"));
                }
                topics.push((cat.clone(), id, prompts));
            }
        }
        for (cat, c) in &s.categories {
            if let Some(bad) = c.styles.iter().find(|st| !s.styles.contains_key(*st)) {
                return Err(format!("topics.json: {cat} allows unknown style {bad}"));
            }
        }
        Ok(Self {
            thresholds: s.thresholds,
            styles: s.styles,
            categories: s
                .categories
                .into_iter()
                .map(|(k, c)| (k, (c.group, c.styles)))
                .collect(),
            topics,
        })
    }

    /// Every prompt, for embedding up front.
    pub fn prompts(&self) -> impl Iterator<Item = &str> {
        self.styles
            .values()
            .flatten()
            .chain(self.topics.iter().flat_map(|t| &t.2))
            .map(String::as_str)
    }
}

/// What to do with a picture.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// It belongs where it is. `p` = own topic's probability.
    Keep { p: f32 },
    /// Move it to another topic (possibly another category of the same group).
    Refile {
        category: String,
        topic: String,
        p_from: f32,
        p_to: f32,
    },
    /// Drop it. `reason` is `wrong_style:<style>` or `off_topic:<best topic>`.
    Reject { reason: String },
    /// The category or topic isn't covered by the sorter (e.g. fundamentals).
    Unchecked,
}

struct Class {
    category: String,
    topic: String,
    emb: Vec<f32>,
}

/// Class embeddings, ready to match picture embeddings against.
pub struct Sorter {
    spec: SortSpec,
    styles: Vec<(String, Vec<f32>)>,
    classes: Vec<Class>,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn mean_unit(vs: &[Vec<f32>]) -> Vec<f32> {
    let n = vs.first().map_or(0, Vec::len);
    let mut m = vec![0f32; n];
    for v in vs {
        for (a, b) in m.iter_mut().zip(v) {
            *a += b;
        }
    }
    let norm = dot(&m, &m).sqrt();
    if norm > 0.0 {
        m.iter_mut().for_each(|x| *x /= norm);
    }
    m
}

/// Softmax of 100 × cosine (CLIP's logit scale).
fn softmax100(img: &[f32], classes: impl Iterator<Item = impl AsRef<[f32]>>) -> Vec<f32> {
    let logits: Vec<f32> = classes.map(|c| 100.0 * dot(img, c.as_ref())).collect();
    let m = logits.iter().copied().fold(f32::MIN, f32::max);
    let e: Vec<f32> = logits.iter().map(|l| (l - m).exp()).collect();
    let z: f32 = e.iter().sum();
    e.into_iter().map(|x| x / z).collect()
}

impl Sorter {
    /// Embed every prompt with `embed_text` (unit-length vectors).
    pub fn build<E>(
        spec: SortSpec,
        mut embed_text: impl FnMut(&str) -> Result<Vec<f32>, E>,
    ) -> Result<Self, E> {
        let mut embed_all = |ps: &[String]| -> Result<Vec<f32>, E> {
            let vs = ps
                .iter()
                .map(|p| embed_text(p))
                .collect::<Result<Vec<_>, E>>()?;
            Ok(mean_unit(&vs))
        };
        let mut styles = Vec::new();
        for (id, ps) in &spec.styles {
            styles.push((id.clone(), embed_all(ps)?));
        }
        let mut classes = Vec::new();
        for (c, t, ps) in &spec.topics {
            classes.push(Class {
                category: c.clone(),
                topic: t.clone(),
                emb: embed_all(ps)?,
            });
        }
        Ok(Self {
            spec,
            styles,
            classes,
        })
    }

    pub fn thresholds(&self) -> &SortThresholds {
        &self.spec.thresholds
    }

    /// Style probabilities (style id, p), highest first.
    pub fn styles(&self, img: &[f32]) -> Vec<(String, f32)> {
        let p = softmax100(img, self.styles.iter().map(|s| &s.1));
        let mut out: Vec<(String, f32)> = self.styles.iter().map(|s| s.0.clone()).zip(p).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    /// Topic probabilities over `category`'s group ((category, topic), p), highest first.
    pub fn topics(&self, img: &[f32], category: &str) -> Vec<((String, String), f32)> {
        let Some((group, _)) = self.spec.categories.get(category) else {
            return vec![];
        };
        let in_group: Vec<&Class> = self
            .classes
            .iter()
            .filter(|c| self.spec.categories.get(&c.category).map(|g| &g.0) == Some(group))
            .collect();
        let p = softmax100(img, in_group.iter().map(|c| &c.emb));
        let mut out: Vec<_> = in_group
            .iter()
            .map(|c| (c.category.clone(), c.topic.clone()))
            .zip(p)
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    /// Decide where a picture with embedding `img`, filed under
    /// `category`/`topic`, belongs.
    pub fn decide(&self, img: &[f32], category: &str, topic: &str) -> Decision {
        let th = &self.spec.thresholds;
        let Some((_, allowed)) = self.spec.categories.get(category) else {
            return Decision::Unchecked;
        };
        if !self
            .classes
            .iter()
            .any(|c| c.category == category && c.topic == topic)
        {
            return Decision::Unchecked;
        }
        if !allowed.is_empty() {
            let styles = self.styles(img);
            let ok: f32 = styles
                .iter()
                .filter(|(s, _)| allowed.contains(s))
                .map(|s| s.1)
                .sum();
            if ok < th.min_style {
                return Decision::Reject {
                    reason: format!("wrong_style:{}", styles[0].0),
                };
            }
        }
        let ranked = self.topics(img, category);
        let p_own = ranked
            .iter()
            .find(|((c, t), _)| c == category && t == topic)
            .map_or(0.0, |r| r.1);
        let ((bc, bt), p_best) = ranked[0].clone();
        if bc == category && bt == topic {
            return Decision::Keep { p: p_own };
        }
        if p_best - p_own >= th.margin && p_best >= th.min_best {
            return Decision::Refile {
                category: bc,
                topic: bt,
                p_from: p_own,
                p_to: p_best,
            };
        }
        if p_own < th.reject_below {
            return Decision::Reject {
                reason: format!("off_topic:{bc}/{bt}"),
            };
        }
        Decision::Keep { p: p_own }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A fake text "model": each prompt is a one-hot vector by its first word.
    fn fake_embed(p: &str) -> Result<Vec<f32>, ()> {
        let axes = ["photo", "anime", "cartoon", "face", "scenery", "cat", "car"];
        let w = p.split_whitespace().next().unwrap();
        let mut v = vec![0.0; axes.len()];
        v[axes.iter().position(|a| *a == w).expect("known word")] = 1.0;
        Ok(v)
    }

    fn spec() -> SortSpec {
        let v = json!({
            "_sorting": {
                "margin": 0.25, "minBest": 0.5, "rejectBelow": 0.05, "minStyle": 0.25,
                "styles": {"photo": ["photo"], "anime": ["anime"], "cartoon": ["cartoon"]},
                "categories": {
                    "anime": {"group": "drawn", "styles": ["anime"]},
                    "animals": {"group": "photo", "styles": ["photo"]},
                    "real-life": {"group": "photo", "styles": ["photo"]}
                }
            },
            "anime": [
                {"id": "characters", "prompts": ["face"]},
                {"id": "scenery", "prompts": ["scenery"]}
            ],
            "animals": [{"id": "pets", "prompts": ["cat"]}],
            "real-life": [{"id": "vehicles", "prompts": ["car"]}],
            "fundamentals": [{"id": "shapes"}]
        });
        SortSpec::from_topics_json(&v).unwrap()
    }

    /// Picture embedding: weights on the fake axes, normalised.
    fn img(w: [f32; 7]) -> Vec<f32> {
        mean_unit(&[w.to_vec()])
    }

    #[test]
    fn reads_prompts_and_thresholds() {
        let s = spec();
        assert_eq!(s.thresholds, SortThresholds::default());
        assert_eq!(s.topics.len(), 4);
        assert_eq!(s.categories["animals"].0, "photo");
        assert_eq!(s.prompts().count(), 7);
        let bad =
            json!({"_sorting": {"categories": {"anime": {"group": "d"}}}, "anime": [{"id": "x"}]});
        assert!(
            SortSpec::from_topics_json(&bad)
                .unwrap_err()
                .contains("no prompts")
        );
    }

    #[test]
    fn keeps_refiles_and_rejects() {
        let s = Sorter::build(spec(), fake_embed).unwrap();
        // an anime face filed as a face: keep
        let face = img([0.0, 0.3, 0.0, 0.3, 0.0, 0.0, 0.0]);
        assert!(matches!(
            s.decide(&face, "anime", "characters"),
            Decision::Keep { .. }
        ));
        // anime scenery filed under characters (px-9599953): re-file
        let scene = img([0.0, 0.3, 0.0, 0.0, 0.3, 0.0, 0.0]);
        match s.decide(&scene, "anime", "characters") {
            Decision::Refile {
                category, topic, ..
            } => {
                assert_eq!((category.as_str(), topic.as_str()), ("anime", "scenery"))
            }
            d => panic!("{d:?}"),
        }
        // a cat photo filed under real-life vehicles: moves to animals/pets
        let cat = img([0.3, 0.0, 0.0, 0.0, 0.0, 0.3, 0.0]);
        assert_eq!(
            s.decide(&cat, "real-life", "vehicles"),
            Decision::Refile {
                category: "animals".into(),
                topic: "pets".into(),
                p_from: s.topics(&cat, "real-life")[1].1,
                p_to: s.topics(&cat, "real-life")[0].1,
            }
        );
        // a cartoon car in real-life (px-1454239): wrong style
        let toon = img([0.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.3]);
        assert_eq!(
            s.decide(&toon, "real-life", "vehicles"),
            Decision::Reject {
                reason: "wrong_style:cartoon".into()
            }
        );
        // not covered
        assert_eq!(
            s.decide(&face, "fundamentals", "shapes"),
            Decision::Unchecked
        );
        assert_eq!(s.decide(&face, "anime", "nope"), Decision::Unchecked);
    }

    #[test]
    fn unsure_pictures_stay_and_hopeless_ones_go() {
        let s = Sorter::build(spec(), fake_embed).unwrap();
        // nearly a tie between face and scenery: not sure enough to move
        let tie = img([0.0, 0.3, 0.0, 0.300, 0.302, 0.0, 0.0]);
        assert!(matches!(
            s.decide(&tie, "anime", "characters"),
            Decision::Keep { .. }
        ));
        // with a strict min_best, a clear loser that nothing claims confidently is rejected
        let mut sp = spec();
        sp.thresholds.min_best = 1.1;
        let s = Sorter::build(sp, fake_embed).unwrap();
        let scene = img([0.0, 0.3, 0.0, 0.0, 0.3, 0.0, 0.0]);
        assert!(matches!(
            s.decide(&scene, "anime", "characters"),
            Decision::Reject { reason } if reason == "off_topic:anime/scenery"
        ));
    }
}
