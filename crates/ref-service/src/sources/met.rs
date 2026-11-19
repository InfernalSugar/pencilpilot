//! The Metropolitan Museum of Art collection API
//! (<https://metmuseum.github.io/>): public-domain paintings, drawings and
//! prints for Master studies.
//!
//! The search (`/public/collection/v1.1/search`, paginated) returns object
//! ids only; each object record (`/public/collection/v1/objects/{id}`) is
//! fetched when the refill worker looks at it, and cached. Only objects with
//! `isPublicDomain == true` (Open Access, CC0) are used. The API allows 80
//! calls a second; we make at most 4.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{Cache, Candidate, Hit, SearchPage, SourceId, clip, http_client, plain_text, send};
use crate::budget::Budget;

pub const API: &str = "https://collectionapi.metmuseum.org/public/collection";
pub const LICENSE: &str = "Public domain (CC0)";
pub const LICENSE_URL: &str = "https://creativecommons.org/publicdomain/zero/1.0/";

pub struct Met {
    base: String,
    http: reqwest::Client,
    cache: Cache,
    pub budget: Budget,
}

impl Met {
    pub fn new(cache_dir: PathBuf, budget_file: Option<PathBuf>) -> Self {
        Self {
            base: API.into(),
            http: http_client(),
            cache: Cache::new(cache_dir),
            budget: Budget::new(
                "met",
                Duration::from_millis(250),
                vec![(60, 120), (24 * 3600, 20_000)],
                budget_file,
            ),
        }
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.into();
        self
    }

    async fn get_json(
        &self,
        key: &str,
        url: String,
        query: &[(&str, String)],
    ) -> Result<Value, String> {
        if let Some(v) = self.cache.get(key) {
            return Ok(v);
        }
        let resp = send(&self.budget, self.http.get(url).query(query)).await?;
        if resp.status().as_u16() == 404 {
            return Ok(Value::Null);
        }
        let v: Value = resp
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        self.cache.put(key, &v);
        Ok(v)
    }

    /// One page of 20 object ids with images.
    pub async fn search(&self, q: &str, page: u32) -> Result<SearchPage, String> {
        let offset = (page.max(1) - 1) * 20;
        let v = self
            .get_json(
                &format!("met-{q}-{page}"),
                format!("{}/v1.1/search", self.base),
                &[
                    ("hasImages", "true".into()),
                    ("q", q.into()),
                    ("limit", "20".into()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;
        let hits: Vec<Hit> = v["objectIDs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let id = id.as_u64()?;
                Some(Hit {
                    key: format!("met:{id}"),
                    raw: Value::from(id),
                })
            })
            .collect();
        let total = v["total"].as_u64().unwrap_or(0);
        Ok(SearchPage {
            last: u64::from(offset) + 20 >= total || page >= 10,
            hits,
        })
    }

    /// Fetch the object record and check it. `Err`: the record couldn't be
    /// fetched now; `Ok(Err(reason))`: the object is not usable.
    pub async fn candidate(&self, hit: &Value) -> Result<Result<Candidate, String>, String> {
        let Some(id) = hit.as_u64() else {
            return Ok(Err("no_id".into()));
        };
        let obj = self
            .get_json(
                &format!("met-object-{id}"),
                format!("{}/v1/objects/{id}", self.base),
                &[],
            )
            .await?;
        Ok(candidate(&obj).map_err(String::from))
    }
}

/// Paintings, drawings, prints and watercolours: flat works a student can copy.
fn is_flat_art(o: &Value) -> bool {
    let text = |f: &str| o[f].as_str().unwrap_or("").to_lowercase();
    // pieces of wall paintings, sherds: not a whole picture to copy
    if text("title").contains("fragment") || text("objectName").contains("fragment") {
        return false;
    }
    let what = format!("{} {}", text("classification"), text("objectName"));
    [
        "painting",
        "drawing",
        "print",
        "watercolor",
        "pastel",
        "sketch",
    ]
    .iter()
    .any(|w| what.contains(w))
        || ["European Paintings", "Drawings and Prints"]
            .contains(&o["department"].as_str().unwrap_or(""))
}

/// Check one object record (public domain, has an image, flat art).
pub fn candidate(o: &Value) -> Result<Candidate, &'static str> {
    let id = o["objectID"].as_u64().ok_or("no_id")?;
    if o["isPublicDomain"].as_bool() != Some(true) {
        return Err("not_public_domain");
    }
    let image_url = o["primaryImageSmall"]
        .as_str()
        .filter(|u| !u.is_empty())
        .or(o["primaryImage"].as_str())
        .filter(|u| u.starts_with("https://"))
        .ok_or("no_image")?;
    if !is_flat_art(o) {
        return Err("not_flat_art");
    }
    let page_url = o["objectURL"]
        .as_str()
        .filter(|u| u.starts_with("https://www.metmuseum.org/"))
        .map(String::from)
        .unwrap_or_else(|| format!("https://www.metmuseum.org/art/collection/search/{id}"));
    let artist = plain_text(o["artistDisplayName"].as_str().unwrap_or(""));
    let mut tags: Vec<String> = o["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["term"].as_str().map(String::from))
        .take(20)
        .collect();
    for f in ["classification", "objectName"] {
        if let Some(s) = o[f].as_str().filter(|s| !s.is_empty()) {
            tags.push(s.to_lowercase());
        }
    }
    Ok(Candidate {
        source: SourceId::Met,
        id: format!("mt-{id}"),
        source_id: id,
        image_url: image_url.into(),
        page_url,
        author: if artist.is_empty() {
            "Unknown artist".into()
        } else {
            clip(&artist, 80)
        },
        author_url: o["artistWikidata_URL"]
            .as_str()
            .or(o["artistULAN_URL"].as_str())
            .filter(|u| u.starts_with("https://"))
            .unwrap_or("")
            .into(),
        license: LICENSE.into(),
        license_url: LICENSE_URL.into(),
        title: clip(&plain_text(o["title"].as_str().unwrap_or("Untitled")), 120),
        tags,
        attribution: String::new(),
        provider: "The Metropolitan Museum of Art".into(),
        kind: "artwork".into(),
        width: 0,
        height: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj() -> Value {
        json!({
            "objectID": 436305, "isPublicDomain": true, "classification": "Paintings",
            "department": "European Paintings", "objectName": "Painting", "title": "Still Life",
            "artistDisplayName": "Georg Flegel",
            "artistWikidata_URL": "https://www.wikidata.org/wiki/Q123",
            "primaryImage": "https://images.metmuseum.org/CRDImages/ep/original/DP-25893-001.jpg",
            "primaryImageSmall": "https://images.metmuseum.org/CRDImages/ep/web-large/DP-25893-001.jpg",
            "objectURL": "https://www.metmuseum.org/art/collection/search/436305",
            "tags": [{"term": "Food"}, {"term": "Still Life"}]
        })
    }

    #[test]
    fn public_domain_flat_art_only() {
        let c = candidate(&obj()).unwrap();
        assert_eq!(c.id, "mt-436305");
        assert!(c.image_url.contains("web-large"));
        assert_eq!(c.author, "Georg Flegel");
        assert_eq!(c.kind, "artwork");
        assert!(c.tags.contains(&"Food".to_string()));
        let mut o = obj();
        o["isPublicDomain"] = json!(false);
        assert_eq!(candidate(&o), Err("not_public_domain"));
        let mut o = obj();
        o["primaryImageSmall"] = json!("");
        o["primaryImage"] = json!("");
        assert_eq!(candidate(&o), Err("no_image"));
        let mut o = obj();
        o["classification"] = json!("Negatives");
        o["objectName"] = json!("Negative");
        o["department"] = json!("Photographs");
        assert_eq!(candidate(&o), Err("not_flat_art"));
        // American Wing paintings have no classification but say "Painting"
        let mut o = obj();
        o["classification"] = json!("");
        o["department"] = json!("The American Wing");
        assert!(candidate(&o).is_ok());
        assert!(candidate(&Value::Null).is_err());
    }

    #[tokio::test]
    async fn search_then_object() {
        let search = json!({"total": 25, "objectIDs": [436305]})
            .to_string()
            .into_bytes();
        let object = obj().to_string().into_bytes();
        let base = super::super::tests::mock_server(vec![
            ("/v1.1/search", "application/json", search),
            ("/v1/objects/436305", "application/json", object),
        ])
        .await;
        let dir = std::env::temp_dir().join(format!("pp-met-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let m = Met::new(dir, None).with_base(&base);
        let page = m.search("still life", 1).await.unwrap();
        assert_eq!(page.hits[0].key, "met:436305");
        assert!(!page.last, "25 results: a second page exists");
        let c = m.candidate(&page.hits[0].raw).await.unwrap().unwrap();
        assert_eq!(c.title, "Still Life");
    }
}
