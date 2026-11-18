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

/// Pixabay `image_type` to search for, by category and tier. Beginners get
/// vectors/illustrations, which are simpler to draw than photos.
pub fn image_type(category: &str, tier: u8) -> &'static str {
    match (category, tier) {
        ("cartoon", _) => "vector",
        ("anime", 1) | ("fundamentals", _) => "vector",
        ("anime", _) => "illustration",
        (_, 1) => "vector",
        (_, 2) => "illustration",
        _ => "photo",
    }
}
