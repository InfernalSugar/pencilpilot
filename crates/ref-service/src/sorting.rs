//! Topic check: is a picture really what its category and topic promise?
//!
//! Search terms alone file pictures in the wrong place (an anime landscape
//! under "Characters", a cartoon courier under Real life › Vehicles). A local
//! CLIP model (`ref_filters::clip`) matches each picture against the prompts
//! in content/topics.json and keeps, re-files or rejects it (ADR-0009). It
//! sorts reference pictures only; it never grades or critiques drawings.

use std::path::Path;

use image::DynamicImage;
use ref_filters::clip::{ClipText, ClipVision};
use ref_filters::sort::{Decision, SortSpec, Sorter};
use serde_json::Value;

pub trait TopicCheck: Send + Sync + 'static {
    /// Where does `img`, found for `category`/`topic`, belong?
    fn check(&self, img: &DynamicImage, category: &str, topic: &str) -> Result<Decision, String>;

    /// Id of the sorting rules (prompts, cut-offs, rule version). When it
    /// differs from the one stored with the pool's `sorted` set, every
    /// picture is checked again. `None`: no model, nothing is checked.
    fn rules(&self) -> Option<String> {
        None
    }
}

/// No model (dev/tests only): every picture stays where its search put it.
pub struct NoTopicCheck;

impl TopicCheck for NoTopicCheck {
    fn check(&self, _: &DynamicImage, _: &str, _: &str) -> Result<Decision, String> {
        Ok(Decision::Unchecked)
    }
}

/// The CLIP picture sorter. Only the vision tower stays in memory; the text
/// tower is dropped once the prompts are embedded.
pub struct ClipTopicCheck {
    vision: ClipVision,
    sorter: Sorter,
}

impl ClipTopicCheck {
    pub fn load(model_dir: &Path, topics: &Value) -> Result<Self, String> {
        let spec = SortSpec::from_topics_json(topics)?;
        let text = ClipText::load(model_dir).map_err(|e| e.to_string())?;
        let sorter = Sorter::build(spec, |p| text.embed(p)).map_err(|e| e.to_string())?;
        drop(text);
        let vision = ClipVision::load(model_dir).map_err(|e| e.to_string())?;
        Ok(Self { vision, sorter })
    }
}

impl TopicCheck for ClipTopicCheck {
    fn check(&self, img: &DynamicImage, category: &str, topic: &str) -> Result<Decision, String> {
        let e = self.vision.embed(img).map_err(|e| e.to_string())?;
        Ok(self.sorter.decide(&e, category, topic))
    }

    fn rules(&self) -> Option<String> {
        Some(self.sorter.fingerprint())
    }
}

/// Where a picture goes after the check: `Ok((category, topic))` to keep or
/// re-file it, `Err(reason)` to drop it.
pub fn destination(d: Decision, category: &str, topic: &str) -> Result<(String, String), String> {
    match d {
        Decision::Keep { .. } | Decision::Unchecked => Ok((category.into(), topic.into())),
        Decision::Refile {
            category, topic, ..
        } => Ok((category, topic)),
        Decision::Reject { reason } => Err(reason),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Test double: a fixed decision for every picture, under rules "fixed".
    pub struct FixedCheck(pub Decision);

    impl TopicCheck for FixedCheck {
        fn check(&self, _: &DynamicImage, _: &str, _: &str) -> Result<Decision, String> {
            Ok(self.0.clone())
        }

        fn rules(&self) -> Option<String> {
            Some("fixed".into())
        }
    }

    #[test]
    fn destinations() {
        assert_eq!(
            destination(Decision::Keep { p: 0.9 }, "anime", "characters"),
            Ok(("anime".into(), "characters".into()))
        );
        assert_eq!(
            destination(Decision::Unchecked, "fundamentals", "shapes"),
            Ok(("fundamentals".into(), "shapes".into()))
        );
        assert_eq!(
            destination(
                Decision::Refile {
                    category: "anime".into(),
                    topic: "scenery".into(),
                    p_from: 0.1,
                    p_to: 0.8
                },
                "anime",
                "characters"
            ),
            Ok(("anime".into(), "scenery".into()))
        );
        assert_eq!(
            destination(
                Decision::Reject {
                    reason: "wrong_style:cartoon".into()
                },
                "real-life",
                "vehicles"
            ),
            Err("wrong_style:cartoon".into())
        );
    }

    #[test]
    fn missing_model_is_an_error() {
        let topics: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::repo_root().join("content/topics.json")).unwrap(),
        )
        .unwrap();
        assert!(ClipTopicCheck::load(Path::new("/nonexistent"), &topics).is_err());
    }

    #[test]
    fn real_topics_json_has_prompts_for_every_checked_topic() {
        let topics: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::repo_root().join("content/topics.json")).unwrap(),
        )
        .unwrap();
        let spec = SortSpec::from_topics_json(&topics).expect("valid _sorting block");
        for c in ["anime", "real-life", "animals", "environment", "cartoon"] {
            let n = topics[c].as_array().unwrap().len();
            assert_eq!(
                spec.topics.iter().filter(|t| t.category == c).count(),
                n,
                "{c}"
            );
        }
        // the subject check is on: wide-view topics are scenes, the rest
        // (characters above all) want one clear subject
        let sub = spec.subject.as_ref().expect("_sorting.subject");
        assert!(sub.reject.contains_key("sheet"));
        let scenes = [
            "anime/scenery",
            "environment/city",
            "environment/interiors",
            "environment/plants",
            "environment/skies-water",
            "environment/landscapes",
        ];
        for t in &spec.topics {
            let key = format!("{}/{}", t.category, t.id);
            let want = if scenes.contains(&key.as_str()) {
                "scene"
            } else {
                "single"
            };
            assert_eq!(t.kind, want, "{key}");
        }
    }
}
