//! Art Institute of Chicago (<https://api.artic.edu/docs/>): public-domain
//! paintings, drawings and prints for Master studies.
//!
//! Only works the museum marks `is_public_domain` (their images are CC0).
//! Images come from the museum's IIIF server at 843 px wide, the size the
//! museum recommends and caches. The API allows 60 calls a minute; we make
//! at most one every 2 s.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{Cache, Candidate, Hit, SearchPage, SourceId, clip, http_client, plain_text, send};
use crate::budget::Budget;

pub const API: &str = "https://api.artic.edu/api/v1/artworks/search";
pub const IIIF: &str = "https://www.artic.edu/iiif/2";
pub const LICENSE: &str = "Public domain (CC0)";
pub const LICENSE_URL: &str = "https://creativecommons.org/publicdomain/zero/1.0/";
const FIELDS: &str = "id,title,artist_title,artist_id,artist_display,image_id,is_public_domain,\
artwork_type_title,classification_titles,subject_titles,term_titles,thumbnail,date_display";

/// Artwork types we use: flat works a student can copy.
pub const TYPES: &[&str] = &["Painting", "Drawing and Watercolor", "Print"];

pub struct Artic {
    base: String,
    /// IIIF image server base, from the API's `config.iiif_url`
    pub iiif_url: String,
    http: reqwest::Client,
    cache: Cache,
    pub budget: Budget,
}

impl Artic {
    pub fn new(cache_dir: PathBuf, budget_file: Option<PathBuf>) -> Self {
        Self {
            base: API.into(),
            iiif_url: IIIF.into(),
            http: http_client(),
            cache: Cache::new(cache_dir),
            budget: Budget::new(
                "artic",
                Duration::from_secs(2),
                vec![(60, 25), (24 * 3600, 3000)],
                budget_file,
            ),
        }
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.into();
        self
    }

    pub async fn search(&self, q: &str, page: u32) -> Result<SearchPage, String> {
        let key = format!("artic-{q}-{page}");
        let v = match self.cache.get(&key) {
            Some(v) => v,
            None => {
                let req = self
                    .http
                    .get(&self.base)
                    .header("AIC-User-Agent", super::USER_AGENT)
                    .query(&[
                        ("q", q),
                        ("query[term][is_public_domain]", "true"),
                        ("fields", FIELDS),
                        ("limit", "20"),
                        ("page", &page.to_string()),
                    ]);
                let v: Value = send(&self.budget, req)
                    .await?
                    .error_for_status()
                    .map_err(|e| e.to_string())?
                    .json()
                    .await
                    .map_err(|e| e.to_string())?;
                self.cache.put(&key, &v);
                v
            }
        };
        let iiif = v["config"]["iiif_url"].as_str().unwrap_or(&self.iiif_url);
        let hits = v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let mut raw = r.clone();
                raw["_iiif_url"] = Value::String(iiif.to_string());
                Some(Hit {
                    key: format!("artic:{}", r["id"].as_u64()?),
                    raw,
                })
            })
            .collect();
        let pages = v["pagination"]["total_pages"].as_u64().unwrap_or(0);
        Ok(SearchPage {
            hits,
            last: u64::from(page) >= pages.min(10),
        })
    }
}

/// Check one artwork (public domain, has an image, a flat artwork type) and
/// turn it into a candidate.
pub fn candidate(r: &Value, default_iiif: &str) -> Result<Candidate, &'static str> {
    let id = r["id"].as_u64().ok_or("no_id")?;
    if r["is_public_domain"].as_bool() != Some(true) {
        return Err("not_public_domain");
    }
    let image_id = r["image_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .ok_or("no_image")?;
    let kind = r["artwork_type_title"].as_str().unwrap_or("");
    if !TYPES.contains(&kind) {
        return Err("not_flat_art");
    }
    let (tw, th) = (
        r["thumbnail"]["width"].as_u64().unwrap_or(0),
        r["thumbnail"]["height"].as_u64().unwrap_or(0),
    );
    // 843 px wide; very wide works would come out too short to draw from
    let (w, h) = if tw > 0 && th > 0 {
        (843, (843 * th / tw) as u32)
    } else {
        (0, 0)
    };
    if h > 0 && h < 400 {
        return Err("tiny");
    }
    let iiif = r["_iiif_url"]
        .as_str()
        .filter(|u| u.starts_with("https://"))
        .unwrap_or(default_iiif);
    let artist = r["artist_title"]
        .as_str()
        .or_else(|| r["artist_display"].as_str().and_then(|a| a.lines().next()))
        .map(plain_text)
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "Unknown artist".into());
    let mut tags: Vec<String> = ["classification_titles", "subject_titles", "term_titles"]
        .iter()
        .flat_map(|f| r[*f].as_array().into_iter().flatten())
        .filter_map(|t| t.as_str().map(String::from))
        .take(20)
        .collect();
    tags.insert(0, kind.to_lowercase());
    Ok(Candidate {
        source: SourceId::Artic,
        id: format!("ai-{id}"),
        source_id: id,
        image_url: format!("{iiif}/{image_id}/full/843,/0/default.jpg"),
        page_url: format!("https://www.artic.edu/artworks/{id}"),
        author_url: r["artist_id"]
            .as_u64()
            .map(|a| format!("https://www.artic.edu/artists/{a}"))
            .unwrap_or_default(),
        author: clip(&artist, 80),
        license: LICENSE.into(),
        license_url: LICENSE_URL.into(),
        title: clip(&plain_text(r["title"].as_str().unwrap_or("Untitled")), 120),
        tags,
        attribution: String::new(),
        provider: "Art Institute of Chicago".into(),
        kind: "artwork".into(),
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn art() -> Value {
        json!({
            "id": 80607, "title": "Self-Portrait", "artist_title": "Vincent van Gogh", "artist_id": 40610,
            "image_id": "47c5bcb8-62ef-e5d7-55e7-f5121f409a30", "is_public_domain": true,
            "artwork_type_title": "Painting", "classification_titles": ["oil on canvas"],
            "subject_titles": ["portraits"], "thumbnail": {"width": 6116, "height": 7794}
        })
    }

    #[test]
    fn public_domain_paintings_only() {
        let c = candidate(&art(), IIIF).unwrap();
        assert_eq!(c.id, "ai-80607");
        assert_eq!(
            c.image_url,
            "https://www.artic.edu/iiif/2/47c5bcb8-62ef-e5d7-55e7-f5121f409a30/full/843,/0/default.jpg"
        );
        assert_eq!(c.page_url, "https://www.artic.edu/artworks/80607");
        assert_eq!(c.author, "Vincent van Gogh");
        assert_eq!(c.kind, "artwork");
        assert_eq!(c.height, 1074);
        let mut r = art();
        r["is_public_domain"] = json!(false);
        assert_eq!(candidate(&r, IIIF), Err("not_public_domain"));
        let mut r = art();
        r.as_object_mut().unwrap().remove("is_public_domain");
        assert_eq!(candidate(&r, IIIF), Err("not_public_domain"));
        let mut r = art();
        r["artwork_type_title"] = json!("Sculpture");
        assert_eq!(candidate(&r, IIIF), Err("not_flat_art"));
        let mut r = art();
        r["image_id"] = json!(null);
        assert_eq!(candidate(&r, IIIF), Err("no_image"));
        let mut r = art();
        r["thumbnail"] = json!({"width": 9000, "height": 2000});
        assert_eq!(candidate(&r, IIIF), Err("tiny"));
    }

    #[tokio::test]
    async fn search_asks_for_public_domain() {
        let body = json!({"pagination": {"total_pages": 3}, "config": {"iiif_url": "https://www.artic.edu/iiif/2"}, "data": [art()]})
            .to_string()
            .into_bytes();
        let base =
            super::super::tests::mock_server(vec![("/search", "application/json", body)]).await;
        let dir = std::env::temp_dir().join(format!("pp-aic-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let a = Artic::new(dir, None).with_base(&format!("{base}/search"));
        let page = a.search("portrait", 1).await.unwrap();
        assert_eq!(page.hits.len(), 1);
        assert!(!page.last);
        assert!(candidate(&page.hits[0].raw, IIIF).is_ok());
    }
}
