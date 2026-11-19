//! Pixabay API client: 24 h search cache (required by Pixabay's API terms)
//! and a request budget well under the 100 req/min limit.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{Cache, Candidate, Hit, SearchPage, SourceId, http_client, send};
use crate::budget::Budget;
use crate::model::kind_from_page_url;
use crate::vet::pixabay_flags_ok;

pub const API: &str = "https://pixabay.com/api/";
pub const LICENSE: &str = "Pixabay Content License";
pub const LICENSE_URL: &str = "https://pixabay.com/service/license-summary/";

pub struct Pixabay {
    key: String,
    base: String,
    http: reqwest::Client,
    cache: Cache,
    pub budget: Budget,
}

impl Pixabay {
    /// `budget_file`: where to remember calls (`None`: memory only).
    pub fn new(key: String, cache_dir: PathBuf, budget_file: Option<PathBuf>) -> Self {
        Self {
            key,
            base: API.into(),
            http: http_client(),
            cache: Cache::new(cache_dir),
            // ≈ 50 calls/min, half the limit
            budget: Budget::new(
                "pixabay",
                Duration::from_millis(1200),
                vec![(60, 60)],
                budget_file,
            ),
        }
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.into();
        self
    }

    /// One search page (SafeSearch on, 50 per page), from cache when < 24 h old.
    pub async fn search(&self, q: &str, kind: &str, page: u32) -> Result<SearchPage, String> {
        // the cache key predates the other sources; keep it so old caches still count
        let key = format!("{q}-{kind}-{page}");
        let v = match self.cache.get(&key) {
            Some(v) => v,
            None => {
                let resp = send(
                    &self.budget,
                    self.http.get(&self.base).query(&[
                        ("key", self.key.as_str()),
                        ("q", q),
                        ("image_type", kind),
                        ("safesearch", "true"),
                        ("per_page", "50"),
                        ("page", &page.to_string()),
                    ]),
                )
                .await?;
                if resp.status().as_u16() == 400 {
                    // past the last page
                    return Ok(SearchPage {
                        hits: vec![],
                        last: true,
                    });
                }
                let v: Value = resp
                    .error_for_status()
                    .map_err(|e| e.to_string())?
                    .json()
                    .await
                    .map_err(|e| e.to_string())?;
                self.cache.put(&key, &v);
                v
            }
        };
        let hits: Vec<Hit> = v["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| {
                Some(Hit {
                    key: format!("pixabay:{}", h["id"].as_u64()?),
                    raw: h.clone(),
                })
            })
            .collect();
        Ok(SearchPage {
            last: hits.len() < 50 || page >= 10,
            hits,
        })
    }
}

/// Pixabay's own flags and banned tags, then the picture's credit.
pub fn candidate(hit: &Value) -> Result<Candidate, &'static str> {
    pixabay_flags_ok(hit)?;
    let sid = hit["id"].as_u64().ok_or("no_id")?;
    let page_url = hit["pageURL"].as_str().unwrap_or("").to_string();
    // The 640 px rendition is plenty for a reference and ~5x smaller.
    let image_url = hit["webformatURL"]
        .as_str()
        .or(hit["largeImageURL"].as_str())
        .ok_or("no_image")?
        .to_string();
    let user = hit["user"].as_str().unwrap_or("").to_string();
    Ok(Candidate {
        source: SourceId::Pixabay,
        id: format!("px-{sid}"),
        source_id: sid,
        image_url,
        author_url: format!(
            "https://pixabay.com/users/{}-{}/",
            user.to_lowercase().replace(' ', "_"),
            hit["user_id"].as_u64().unwrap_or(0)
        ),
        author: user,
        license: LICENSE.into(),
        license_url: LICENSE_URL.into(),
        title: String::new(),
        tags: hit["tags"]
            .as_str()
            .unwrap_or("")
            .split(", ")
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect(),
        attribution: String::new(),
        provider: "Pixabay".into(),
        kind: kind_from_page_url(&page_url).unwrap_or("").into(),
        page_url,
        width: hit["imageWidth"].as_u64().unwrap_or(0) as u32,
        height: hit["imageHeight"].as_u64().unwrap_or(0) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn candidate_from_hit() {
        let hit = json!({"id": 7, "pageURL": "https://pixabay.com/photos/cat-7/", "webformatURL": "https://cdn/x.jpg",
            "user": "Some One", "user_id": 9, "tags": "cat, pet", "isAiGenerated": false, "isGRated": true, "isLowQuality": false});
        let c = candidate(&hit).unwrap();
        assert_eq!(c.id, "px-7");
        assert_eq!(c.kind, "photo");
        assert_eq!(c.author_url, "https://pixabay.com/users/some_one-9/");
        assert_eq!(c.tags, ["cat", "pet"]);
        let mut ai = hit.clone();
        ai["isAiGenerated"] = json!(true);
        assert_eq!(candidate(&ai), Err("ai_generated"));
    }

    #[tokio::test]
    async fn search_reads_hits_and_caches() {
        let body = json!({"hits": [{"id": 1}, {"id": 2}]})
            .to_string()
            .into_bytes();
        let base =
            super::super::tests::mock_server(vec![("/api/", "application/json", body)]).await;
        let dir = std::env::temp_dir().join(format!("pp-px-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let px = Pixabay::new("k".into(), dir, None).with_base(&format!("{base}/api/"));
        let page = px.search("cat", "photo", 1).await.unwrap();
        assert_eq!(page.hits.len(), 2);
        assert_eq!(page.hits[0].key, "pixabay:1");
        assert!(page.last, "fewer than 50 hits");
        // second call comes from the cache: no budget used
        px.search("cat", "photo", 1).await.unwrap();
        assert_eq!(px.budget.calls().await, 1);
    }
}
