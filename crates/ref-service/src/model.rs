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
    #[serde(default)]
    pub keypoints: Vec<Value>,
    pub difficulty: f32,
    pub tier: u8,
    /// served by this service at /refimg/<id>.jpg
    pub image_url: String,
    pub live: bool,
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
pub struct Topic {
    pub id: String,
    #[serde(default)]
    pub queries: Vec<String>,
}

pub const CATEGORIES: &[&str] = &[
    "anime",
    "real-life",
    "animals",
    "environment",
    "cartoon",
    "fundamentals",
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

/// May this reference be shown in its category? Photo-only categories need a
/// page URL that says it's a photo (unknown counts as no).
pub fn kind_allowed(category: &str, page_url: &str) -> bool {
    !PHOTO_ONLY.contains(&category) || kind_from_page_url(page_url) == Some("photo")
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
        assert!(kind_allowed("real-life", "https://pixabay.com/photos/a-1/"));
        assert!(!kind_allowed(
            "real-life",
            "https://pixabay.com/vectors/a-1/"
        ));
        assert!(!kind_allowed("animals", ""));
        assert!(kind_allowed("anime", "https://pixabay.com/vectors/a-1/"));
    }
}
