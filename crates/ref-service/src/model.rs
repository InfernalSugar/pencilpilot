//! Shared data types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One live reference, in the same shape as the web app's curated `Reference`
/// (apps/web/src/lib/references.ts) plus `imageUrl` and `live`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveRef {
    pub id: String,
    pub source: String,
    pub source_id: u64,
    pub category: String,
    pub topic: String,
    pub page_url: String,
    pub author: String,
    pub author_url: String,
    pub license: String,
    pub license_url: String,
    pub tags: Vec<String>,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub title: String,
    /// the source's full credit line (Openverse), or empty
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub attribution: String,
    /// who holds the picture ("Pixabay", "Wikimedia Commons", "The Met", …);
    /// empty in pools from before there were other sources (= Pixabay)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    /// "photo", "vector", "illustration" or "artwork"; empty in old pools
    /// (then read from the Pixabay page URL)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default)]
    pub keypoints: Vec<Value>,
    pub difficulty: f32,
    pub tier: u8,
    /// served by this service at /refimg/<id>.jpg
    pub image_url: String,
    pub live: bool,
}

impl LiveRef {
    /// The picture kind: stored, or read from a Pixabay page URL.
    pub fn kind(&self) -> &str {
        if self.kind.is_empty() {
            kind_from_page_url(&self.page_url).unwrap_or("")
        } else {
            &self.kind
        }
    }
}

/// A pool slot: what the student asked for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Slot {
    pub category: String,
    pub topic: String,
    pub tier: u8,
}

impl Slot {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.category, self.topic, self.tier)
    }
}

/// Topic definition from content/topics.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: String,
    /// Pixabay search queries
    #[serde(default)]
    pub queries: Vec<String>,
    /// search queries for the other sources, by source name
    #[serde(default)]
    pub source_queries: std::collections::BTreeMap<String, Vec<String>>,
}

impl Topic {
    /// Search queries for `source` (empty: don't use that source here).
    pub fn queries_for(&self, source: &str) -> &[String] {
        if source == "pixabay" {
            return &self.queries;
        }
        self.source_queries.get(source).map_or(&[], Vec::as_slice)
    }
}

pub const CATEGORIES: &[&str] = &[
    "anime",
    "real-life",
    "animals",
    "environment",
    "cartoon",
    "fundamentals",
    "masters",
];

/// Categories that show real photos only, at every tier (user decision,
/// 2026-11-19). Beginner-friendliness comes from the difficulty tier, never
/// from swapping in drawings.
pub const PHOTO_ONLY: &[&str] = &["real-life", "animals", "environment"];

/// Pixabay `image_type` to search for, by category and tier. Real life,
/// animals and environment are photos at every tier; anime and cartoon stay
/// drawn (vector for Beginner anime and all cartoon, illustration otherwise).
pub fn image_type(category: &str, tier: u8) -> &'static str {
    if PHOTO_ONLY.contains(&category) {
        return "photo";
    }
    match (category, tier) {
        ("cartoon", _) | ("fundamentals", _) | ("anime", 1) => "vector",
        _ => "illustration",
    }
}

/// The Pixabay image kind ("photo", "vector", "illustration") read from a
/// page URL like `https://pixabay.com/photos/cat-123/`. `None` if unknown.
pub fn kind_from_page_url(url: &str) -> Option<&'static str> {
    let rest = url.split("pixabay.com/").nth(1)?;
    match rest.split('/').next()? {
        "photos" => Some("photo"),
        "vectors" => Some("vector"),
        "illustrations" => Some("illustration"),
        _ => None,
    }
}

/// May a picture of this kind be shown in `category`? Photo-only categories
/// need photos, Master studies needs artworks, anime and cartoon anything but
/// photos. Unknown kinds count as no where a kind is required.
pub fn kind_allowed(category: &str, kind: &str) -> bool {
    if PHOTO_ONLY.contains(&category) {
        kind == "photo"
    } else if category == "masters" {
        kind == "artwork"
    } else {
        kind != "photo" || category == "fundamentals"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_only_categories_are_photos_at_every_tier() {
        for c in PHOTO_ONLY {
            for t in 1..=4 {
                assert_eq!(image_type(c, t), "photo", "{c} tier {t}");
            }
        }
    }

    #[test]
    fn drawn_categories_stay_drawn() {
        assert_eq!(image_type("anime", 1), "vector");
        for t in 2..=4 {
            assert_eq!(image_type("anime", t), "illustration");
        }
        for t in 1..=4 {
            assert_eq!(image_type("cartoon", t), "vector");
            assert_eq!(image_type("fundamentals", t), "vector");
        }
    }

    #[test]
    fn kind_from_url() {
        assert_eq!(
            kind_from_page_url("https://pixabay.com/photos/cat-pet-123/"),
            Some("photo")
        );
        assert_eq!(
            kind_from_page_url("https://pixabay.com/vectors/pizza-courier-delivery-fast-1454239/"),
            Some("vector")
        );
        assert_eq!(
            kind_from_page_url("https://pixabay.com/illustrations/girl-1/"),
            Some("illustration")
        );
        assert_eq!(kind_from_page_url("https://example.com/x"), None);
    }

    #[test]
    fn kinds_per_category() {
        assert!(kind_allowed("real-life", "photo"));
        assert!(!kind_allowed("real-life", "vector"));
        assert!(!kind_allowed("animals", ""));
        assert!(!kind_allowed("animals", "artwork"));
        assert!(kind_allowed("anime", "vector"));
        assert!(kind_allowed("anime", "illustration"));
        assert!(!kind_allowed("anime", "photo"));
        assert!(kind_allowed("masters", "artwork"));
        assert!(!kind_allowed("masters", "photo"));
        assert!(!kind_allowed("masters", "illustration"));
        assert!(!kind_allowed("masters", ""));
    }

    #[test]
    fn old_pool_entries_read_their_kind_from_the_url() {
        let mut r: LiveRef = serde_json::from_value(serde_json::json!({
            "id": "px-1", "source": "pixabay", "sourceId": 1, "category": "animals", "topic": "pets",
            "pageUrl": "https://pixabay.com/photos/cat-1/", "author": "a", "authorUrl": "",
            "license": "l", "licenseUrl": "", "tags": [], "width": 1, "height": 1,
            "difficulty": 1.0, "tier": 1, "imageUrl": "/refimg/px-1.jpg", "live": true
        }))
        .unwrap();
        assert_eq!(r.kind(), "photo");
        r.kind = "artwork".into();
        assert_eq!(r.kind(), "artwork");
    }

    #[test]
    fn topic_queries_per_source() {
        let t: Topic = serde_json::from_value(serde_json::json!({
            "id": "x", "queries": ["cat"], "sourceQueries": {"openverse": ["tabby cat"]}
        }))
        .unwrap();
        assert_eq!(t.queries_for("pixabay"), ["cat"]);
        assert_eq!(t.queries_for("openverse"), ["tabby cat"]);
        assert!(t.queries_for("met").is_empty());
    }
}
