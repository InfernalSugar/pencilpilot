//! Sorting reference pictures into category and topic by zero-shot matching
//! against short text prompts (CLIP, see [`crate::clip`]). Used **only** to
//! decide where a reference picture belongs, never to score or critique a
//! drawing (ADR-0009, ADR-0002).
//!
//! The prompts and cut-offs live in `content/topics.json`: each topic has a
//! `prompts` list, and the `_sorting` block has the style prompts (photo,
//! anime, cartoon, …), which styles each category allows, which categories
//! may swap pictures (a `group`), the subject check, and the thresholds.
//!
//! Three checks, all on softmax probabilities of 100 × cosine similarity (the
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
//! 3. **One drawable subject** (`_sorting.subject`), on the topic the
//!    picture ends up in: is it one clear subject (topic `"subject":
//!    "single"`, the default) or one coherent scene (`"scene"`), and not a
//!    sheet, grid, icon set, pattern, text or logo? The "keep" class is the
//!    mean of the topic's own prompts and its kind's prompts; it competes
//!    with every `reject` class and every prompt in the topic's `avoid`
//!    list. Reject if the best of those beats the keep class by the kind's
//!    margin. A hair-style sheet of twenty small heads is not a character
//!    to draw, however anime it looks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bump when the decision logic changes, so pools re-check every picture
/// (the prompts and thresholds are part of [`SortSpec::fingerprint`] anyway).
pub const RULES_VERSION: u32 = 3;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KindRaw {
    prompts: Vec<String>,
    #[serde(default)]
    margin: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubjectRaw {
    margin: f32,
    kinds: BTreeMap<String, KindRaw>,
    #[serde(default)]
    reject: BTreeMap<String, Vec<String>>,
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
    #[serde(default)]
    subject: Option<SubjectRaw>,
    #[serde(default)]
    franchise: Option<FranchiseSpec>,
}

/// The franchise / fan-art check (`_sorting.franchise`, ADR-0003): each
/// prompt names a well-known franchise character and is its own class; each
/// decoy describes a look-alike original ("an anime girl with long blue
/// hair") and is its own class too. Reject if the strongest franchise class
/// beats both the topic's keep class and every decoy by `margin`. Without
/// decoys, every blue-haired anime girl would be Hatsune Miku.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FranchiseSpec {
    pub margin: f32,
    /// categories checked (empty: every checked category)
    #[serde(default)]
    pub categories: Vec<String>,
    pub prompts: Vec<String>,
    #[serde(default)]
    pub decoys: Vec<String>,
}

/// A kind of picture a topic wants: one clear subject, or one scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SubjectKind {
    pub prompts: Vec<String>,
    /// Reject if a reject class beats the keep class by at least this.
    pub margin: f32,
}

/// The "one drawable subject" check (`_sorting.subject`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubjectSpec {
    /// kind id ("single", "scene") → prompts and margin
    pub kinds: BTreeMap<String, SubjectKind>,
    /// reject class ("sheet", "icons", …) → prompts
    pub reject: BTreeMap<String, Vec<String>>,
}

/// Default subject kind of a topic.
pub const DEFAULT_KIND: &str = "single";

/// One topic as the sorter sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicSpec {
    pub category: String,
    pub id: String,
    pub prompts: Vec<String>,
    /// subject kind ("single" unless the topic says `"subject": "scene"`)
    pub kind: String,
    /// what this topic must not show (each prompt is its own reject class)
    pub avoid: Vec<String>,
}

/// Everything the sorter needs from `content/topics.json`.
#[derive(Debug, Clone, Default)]
pub struct SortSpec {
    pub thresholds: SortThresholds,
    /// style id → prompts
    pub styles: BTreeMap<String, Vec<String>>,
    /// category → (group, allowed style ids)
    pub categories: BTreeMap<String, (String, Vec<String>)>,
    /// every checked topic, in file order
    pub topics: Vec<TopicSpec>,
    /// `None`: no subject check
    pub subject: Option<SubjectSpec>,
    /// `None`: no franchise check
    pub franchise: Option<FranchiseSpec>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(String::from))
        .collect()
}

impl SortSpec {
    /// Read the `_sorting` block and every topic's `prompts` (plus optional
    /// `subject` and `avoid`) from the parsed topics.json. Categories missing
    /// from `_sorting.categories` are not checked.
    pub fn from_topics_json(v: &Value) -> Result<Self, String> {
        let s: SortingSpec = serde_json::from_value(v["_sorting"].clone())
            .map_err(|e| format!("topics.json _sorting: {e}"))?;
        let subject = s.subject.map(|raw| SubjectSpec {
            kinds: raw
                .kinds
                .into_iter()
                .map(|(k, kr)| {
                    (
                        k,
                        SubjectKind {
                            prompts: kr.prompts,
                            margin: kr.margin.unwrap_or(raw.margin),
                        },
                    )
                })
                .collect(),
            reject: raw.reject,
        });
        if let Some(sub) = &subject {
            if let Some((k, _)) = sub.kinds.iter().find(|(_, k)| k.prompts.is_empty()) {
                return Err(format!("topics.json: subject kind {k} has no prompts"));
            }
            if let Some((k, _)) = sub.reject.iter().find(|(_, ps)| ps.is_empty()) {
                return Err(format!(
                    "topics.json: subject reject class {k} has no prompts"
                ));
            }
        }
        let mut topics = Vec::new();
        for cat in s.categories.keys() {
            for t in v[cat.as_str()].as_array().into_iter().flatten() {
                let id = t["id"].as_str().unwrap_or_default().to_string();
                let prompts = strings(&t["prompts"]);
                if prompts.is_empty() {
                    return Err(format!("topics.json: {cat}/{id} has no prompts"));
                }
                let kind = t["subject"].as_str().unwrap_or(DEFAULT_KIND).to_string();
                if let Some(sub) = &subject
                    && !sub.kinds.contains_key(&kind)
                {
                    return Err(format!(
                        "topics.json: {cat}/{id} has unknown subject kind {kind}"
                    ));
                }
                topics.push(TopicSpec {
                    category: cat.clone(),
                    id,
                    prompts,
                    kind,
                    avoid: strings(&t["avoid"]),
                });
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
            subject,
            franchise: s.franchise.filter(|f| !f.prompts.is_empty()),
        })
    }

    /// Every prompt, for embedding up front.
    pub fn prompts(&self) -> impl Iterator<Item = &str> {
        let sub = self.subject.iter().flat_map(|s| {
            s.kinds
                .values()
                .flat_map(|k| &k.prompts)
                .chain(s.reject.values().flatten())
        });
        self.styles
            .values()
            .flatten()
            .chain(
                self.topics
                    .iter()
                    .flat_map(|t| t.prompts.iter().chain(&t.avoid)),
            )
            .chain(sub)
            .chain(
                self.franchise
                    .iter()
                    .flat_map(|f| f.prompts.iter().chain(&f.decoys)),
            )
            .map(String::as_str)
    }

    /// A short id of these rules (prompts, thresholds, [`RULES_VERSION`]).
    /// A pool remembers it with its `sorted` set; when it changes, every
    /// picture is checked again once.
    pub fn fingerprint(&self) -> String {
        // FNV-1a over the Debug form: stable for the same rules (BTreeMaps,
        // Vecs in file order), no extra dependency.
        let text = format!("v{RULES_VERSION} {self:?}");
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:016x}")
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
    /// Drop it. `reason` is `wrong_style:<style>`, `off_topic:<best topic>`,
    /// `not_one_subject:<reject class>` or `unwanted:<topic's avoid prompt>`.
    Reject { reason: String },
    /// The category or topic isn't covered by the sorter (e.g. fundamentals).
    Unchecked,
}

struct Class {
    category: String,
    topic: String,
    emb: Vec<f32>,
    /// subject check: the keep class (topic + kind) and its margin
    keep: Vec<f32>,
    margin: f32,
    /// the topic's avoid prompts, one class each
    avoid: Vec<(String, Vec<f32>)>,
}

/// Class embeddings, ready to match picture embeddings against.
pub struct Sorter {
    spec: SortSpec,
    styles: Vec<(String, Vec<f32>)>,
    classes: Vec<Class>,
    /// subject check reject classes (empty: no subject check)
    reject: Vec<(String, Vec<f32>)>,
    /// franchise check classes, one per prompt (empty: no franchise check)
    franchise: Vec<(String, Vec<f32>)>,
    /// look-alike originals for the franchise check, one class each
    decoys: Vec<Vec<f32>>,
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

/// Result of the subject check for one picture and topic.
#[derive(Debug, Clone, PartialEq)]
pub struct SubjectScore {
    /// probability of the keep class (one subject / one scene matching the topic)
    pub p_keep: f32,
    /// the strongest reject class: `not_one_subject:<class>` or `unwanted:<prompt>`
    pub worst: String,
    pub p_worst: f32,
    /// the kind's margin: reject if `p_worst - p_keep >= margin`
    pub margin: f32,
}

impl SubjectScore {
    pub fn rejects(&self) -> bool {
        self.p_worst - self.p_keep >= self.margin
    }
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
        let mut kinds = BTreeMap::new();
        let mut reject = Vec::new();
        if let Some(sub) = &spec.subject {
            for (k, kind) in &sub.kinds {
                kinds.insert(k.clone(), (embed_all(&kind.prompts)?, kind.margin));
            }
            for (k, ps) in &sub.reject {
                reject.push((format!("not_one_subject:{k}"), embed_all(ps)?));
            }
        }
        let mut classes = Vec::new();
        for t in &spec.topics {
            let emb = embed_all(&t.prompts)?;
            let (keep, margin) = match kinds.get(&t.kind) {
                Some((k, m)) => (mean_unit(&[emb.clone(), k.clone()]), *m),
                None => (emb.clone(), f32::INFINITY),
            };
            let mut avoid = Vec::new();
            for p in &t.avoid {
                avoid.push((format!("unwanted:{p}"), embed_all(std::slice::from_ref(p))?));
            }
            classes.push(Class {
                category: t.category.clone(),
                topic: t.id.clone(),
                emb,
                keep,
                margin,
                avoid,
            });
        }
        let mut franchise = Vec::new();
        let mut decoys = Vec::new();
        if let Some(f) = &spec.franchise {
            for p in &f.prompts {
                franchise.push((
                    format!("franchise:{p}"),
                    embed_all(std::slice::from_ref(p))?,
                ));
            }
            for p in &f.decoys {
                decoys.push(embed_all(std::slice::from_ref(p))?);
            }
        }
        Ok(Self {
            spec,
            styles,
            classes,
            reject,
            franchise,
            decoys,
        })
    }

    pub fn thresholds(&self) -> &SortThresholds {
        &self.spec.thresholds
    }

    /// See [`SortSpec::fingerprint`].
    pub fn fingerprint(&self) -> String {
        self.spec.fingerprint()
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

    fn class(&self, category: &str, topic: &str) -> Option<&Class> {
        self.classes
            .iter()
            .find(|c| c.category == category && c.topic == topic)
    }

    /// The subject check for a picture in `category`/`topic`: `None` if
    /// there is no subject check or the topic isn't covered.
    pub fn subject(&self, img: &[f32], category: &str, topic: &str) -> Option<SubjectScore> {
        let c = self.class(category, topic)?;
        if !c.margin.is_finite() || (self.reject.is_empty() && c.avoid.is_empty()) {
            return None;
        }
        let rivals: Vec<&(String, Vec<f32>)> = self.reject.iter().chain(&c.avoid).collect();
        let p = softmax100(
            img,
            std::iter::once(&c.keep).chain(rivals.iter().map(|r| &r.1)),
        );
        let (i, p_worst) = p[1..]
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        Some(SubjectScore {
            p_keep: p[0],
            worst: rivals[i].0.clone(),
            p_worst,
            margin: c.margin,
        })
    }

    /// Decide where a picture with embedding `img`, filed under
    /// `category`/`topic`, belongs.
    pub fn decide(&self, img: &[f32], category: &str, topic: &str) -> Decision {
        let d = self.decide_topic(img, category, topic);
        let (c, t) = match &d {
            Decision::Keep { .. } => (category, topic),
            Decision::Refile {
                category, topic, ..
            } => (category.as_str(), topic.as_str()),
            _ => return d,
        };
        if let Some(s) = self.subject(img, c, t)
            && s.rejects()
        {
            return Decision::Reject { reason: s.worst };
        }
        match self.franchise(img, c, t) {
            Some(s) if s.rejects() => Decision::Reject { reason: s.worst },
            _ => d,
        }
    }

    /// The franchise check for a picture in `category`/`topic`: the topic's
    /// keep class and the decoys against each franchise prompt. `p_keep` is
    /// the strongest of the keep class and the decoys. `None` if there is no
    /// franchise check, the category isn't checked or the topic isn't covered.
    pub fn franchise(&self, img: &[f32], category: &str, topic: &str) -> Option<SubjectScore> {
        let spec = self.spec.franchise.as_ref()?;
        if !spec.categories.is_empty() && !spec.categories.iter().any(|c| c == category) {
            return None;
        }
        let c = self.class(category, topic)?;
        if self.franchise.is_empty() {
            return None;
        }
        let n_safe = 1 + self.decoys.len();
        let p = softmax100(
            img,
            std::iter::once(&c.keep)
                .chain(&self.decoys)
                .chain(self.franchise.iter().map(|f| &f.1)),
        );
        let p_keep = p[..n_safe].iter().copied().fold(0.0, f32::max);
        let (i, p_worst) = p[n_safe..]
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        Some(SubjectScore {
            p_keep,
            worst: self.franchise[i].0.clone(),
            p_worst,
            margin: spec.margin,
        })
    }

    /// Checks 1 and 2 (style, topic).
    fn decide_topic(&self, img: &[f32], category: &str, topic: &str) -> Decision {
        let th = &self.spec.thresholds;
        let Some((_, allowed)) = self.spec.categories.get(category) else {
            return Decision::Unchecked;
        };
        if self.class(category, topic).is_none() {
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

    /// The fake model's vocabulary: one axis per word.
    const AXES: [&str; 13] = [
        "photo", "anime", "cartoon", "face", "scenery", "cat", "car", "one", "wide", "grid", "eye",
        "pikachu", "girl",
    ];

    /// A fake text "model": each prompt is a one-hot vector by its first word.
    fn fake_embed(p: &str) -> Result<Vec<f32>, ()> {
        let axes = AXES;
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

    /// Picture embedding: weights on the first fake axes (the rest 0), normalised.
    fn img<const N: usize>(w: [f32; N]) -> Vec<f32> {
        let mut v = w.to_vec();
        v.resize(AXES.len(), 0.0);
        mean_unit(&[v])
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

    /// `spec()` plus a subject check: "single" (prompt "one", margin 0.35)
    /// and "scene" (prompt "wide", its own margin 0.9), a "sheet" reject
    /// class ("grid"), and characters must not be a lone "eye".
    fn subject_json() -> Value {
        json!({
            "_sorting": {
                "styles": {"photo": ["photo"], "anime": ["anime"], "cartoon": ["cartoon"]},
                "categories": {
                    "anime": {"group": "drawn", "styles": ["anime"]},
                    "real-life": {"group": "photo", "styles": ["photo"]}
                },
                "subject": {
                    "margin": 0.35,
                    "kinds": {
                        "single": {"prompts": ["one"]},
                        "scene": {"prompts": ["wide"], "margin": 0.9}
                    },
                    "reject": {"sheet": ["grid"]}
                }
            },
            "anime": [
                {"id": "characters", "prompts": ["face"], "avoid": ["eye"]},
                {"id": "scenery", "prompts": ["scenery"], "subject": "scene"}
            ],
            "real-life": [{"id": "vehicles", "prompts": ["car"]}]
        })
    }

    #[test]
    fn reads_the_subject_check() {
        let s = SortSpec::from_topics_json(&subject_json()).unwrap();
        let sub = s.subject.as_ref().unwrap();
        assert_eq!(sub.kinds["single"].margin, 0.35, "global margin");
        assert_eq!(sub.kinds["scene"].margin, 0.9, "kind's own margin");
        assert_eq!(sub.reject["sheet"], ["grid"]);
        let t = |id: &str| s.topics.iter().find(|t| t.id == id).unwrap();
        assert_eq!(t("characters").kind, "single", "default kind");
        assert_eq!(t("characters").avoid, ["eye"]);
        assert_eq!(t("scenery").kind, "scene");
        assert!(t("vehicles").avoid.is_empty());
        // styles 3 + topics 3 + avoid 1 + kinds 2 + reject 1
        assert_eq!(s.prompts().count(), 10);
        assert!(spec().subject.is_none(), "no block, no check");

        let mut bad = subject_json();
        bad["anime"][1]["subject"] = json!("crowd");
        let e = SortSpec::from_topics_json(&bad).unwrap_err();
        assert!(e.contains("unknown subject kind crowd"), "{e}");
        let mut bad = subject_json();
        bad["_sorting"]["subject"]["reject"]["text"] = json!([]);
        let e = SortSpec::from_topics_json(&bad).unwrap_err();
        assert!(e.contains("reject class text has no prompts"), "{e}");
        let mut bad = subject_json();
        bad["_sorting"]["subject"]["kinds"]["single"] = json!({"margin": 0.1});
        assert!(
            SortSpec::from_topics_json(&bad).is_err(),
            "kind without prompts"
        );
    }

    #[test]
    fn fingerprint_follows_the_rules() {
        let a = SortSpec::from_topics_json(&subject_json()).unwrap();
        assert_eq!(a.fingerprint(), a.clone().fingerprint());
        assert_eq!(a.fingerprint().len(), 16);
        let mut v = subject_json();
        v["_sorting"]["subject"]["reject"]["sheet"] = json!(["grid", "set"]);
        assert_ne!(
            SortSpec::from_topics_json(&v).unwrap().fingerprint(),
            a.fingerprint()
        );
        let mut v = subject_json();
        v["_sorting"]["subject"]["margin"] = json!(0.4);
        assert_ne!(
            SortSpec::from_topics_json(&v).unwrap().fingerprint(),
            a.fingerprint()
        );
    }

    #[test]
    fn sheets_go_single_subjects_stay() {
        let spec = SortSpec::from_topics_json(&subject_json()).unwrap();
        let s = Sorter::build(spec, fake_embed).unwrap();
        // one anime face: keep
        let face = img([0.0, 0.3, 0.0, 0.3, 0.0, 0.0, 0.0, 0.3]);
        assert!(matches!(
            s.decide(&face, "anime", "characters"),
            Decision::Keep { .. }
        ));
        assert!(!s.subject(&face, "anime", "characters").unwrap().rejects());
        // a sheet of twenty small faces (px-7111967): the topic check alone
        // keeps it (faces, anime), the subject check rejects it
        let sheet = img([0.0, 0.3, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5]);
        assert!(matches!(
            s.decide_topic(&sheet, "anime", "characters"),
            Decision::Keep { .. }
        ));
        assert_eq!(
            s.decide(&sheet, "anime", "characters"),
            Decision::Reject {
                reason: "not_one_subject:sheet".into()
            }
        );
        let sc = s.subject(&sheet, "anime", "characters").unwrap();
        assert_eq!(sc.worst, "not_one_subject:sheet");
        assert!(sc.p_worst > 0.9 && sc.margin == 0.35);
        // a lone eye is not a character: the topic's avoid list
        let eye = img([0.0, 0.3, 0.0, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5]);
        assert_eq!(
            s.decide(&eye, "anime", "characters"),
            Decision::Reject {
                reason: "unwanted:eye".into()
            }
        );
        // a category the sorter doesn't cover: no subject check
        assert_eq!(s.subject(&sheet, "fundamentals", "shapes"), None);
    }

    #[test]
    fn scenes_are_judged_as_scenes_where_they_end_up() {
        let spec = SortSpec::from_topics_json(&subject_json()).unwrap();
        let s = Sorter::build(spec, fake_embed).unwrap();
        // a wide scene with a repeated motif (stars, windows): "grid" wins by
        // more than the single kind's 0.35 but less than the scene kind's 0.9
        let town = img([0.0, 0.3, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.183]);
        let sc = s.subject(&town, "anime", "scenery").unwrap();
        assert!(sc.p_worst - sc.p_keep > 0.35 && !sc.rejects(), "{sc:?}");
        assert!(matches!(
            s.decide(&town, "anime", "scenery"),
            Decision::Keep { .. }
        ));
        // the same picture filed under characters is re-filed to scenery and
        // judged by scenery's rule there
        assert!(matches!(
            s.decide(&town, "anime", "characters"),
            Decision::Refile { topic, .. } if topic == "scenery"
        ));
        // a sheet of faces found by a scenery search: re-filed to characters
        // by the topic check, then rejected as a sheet there
        let sheet = img([0.0, 0.3, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5]);
        assert!(matches!(
            s.decide_topic(&sheet, "anime", "scenery"),
            Decision::Refile { .. }
        ));
        assert_eq!(
            s.decide(&sheet, "anime", "scenery"),
            Decision::Reject {
                reason: "not_one_subject:sheet".into()
            }
        );
    }

    #[test]
    fn franchise_check_with_decoys() {
        let v = json!({
            "_sorting": {
                "styles": {"anime": ["anime"], "photo": ["photo"]},
                "categories": {
                    "anime": {"group": "drawn", "styles": ["anime"]},
                    "animals": {"group": "photo", "styles": ["photo"]}
                },
                "franchise": {"margin": 0.3, "categories": ["anime"], "prompts": ["pikachu"], "decoys": ["girl"]}
            },
            "anime": [{"id": "characters", "prompts": ["face"]}],
            "animals": [{"id": "pets", "prompts": ["cat"]}]
        });
        let spec = SortSpec::from_topics_json(&v).unwrap();
        assert_eq!(spec.prompts().filter(|p| *p == "girl").count(), 1);
        let s = Sorter::build(spec, fake_embed).unwrap();
        // anime style, looks like Pikachu
        let pika = img([0.0, 0.3, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            s.decide(&pika, "anime", "characters"),
            Decision::Reject {
                reason: "franchise:pikachu".into()
            }
        );
        // an original girl: the decoy wins
        let girl = img([
            0.0, 0.3, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.05, 1.0,
        ]);
        assert!(!s.franchise(&girl, "anime", "characters").unwrap().rejects());
        assert!(matches!(
            s.decide(&girl, "anime", "characters"),
            Decision::Keep { .. }
        ));
        // photo categories aren't checked
        assert!(s.franchise(&pika, "animals", "pets").is_none());
    }
}
