//! Where live reference pictures come from (ADR-0003, ADR-0005).
//!
//! | source    | what                         | licence                         | categories            |
//! |-----------|------------------------------|---------------------------------|-----------------------|
//! | Pixabay   | photos, vectors, drawings    | Pixabay Content License         | all but Master studies |
//! | Openverse | museum + Wikimedia pictures  | CC0, Public Domain Mark, CC BY  | any topic that lists Openverse queries |
//! | Art Institute of Chicago | artworks      | public domain (CC0)             | Master studies        |
//! | The Met   | artworks                     | public domain (CC0)             | Master studies        |
//!
//! A source is used for a topic only if the topic lists search queries for
//! it in content/topics.json (`queries` for Pixabay, `sourceQueries.<source>`
//! for the rest). Each source checks its own licence and provider rules in
//! [`Sources::resolve`]; everything after that (banned words, NSFW guard,
//! picture sorter, near-duplicates) is the same for every source.

pub mod artic;
pub mod met;
pub mod openverse;
pub mod pixabay;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::budget::{Budget, retry_after};
use crate::model::PHOTO_ONLY;

/// User agent for every API and image request. Wikimedia and the Art
/// Institute ask clients to say who they are.
pub const USER_AGENT: &str =
    "pencilpilot-ref-service (+https://github.com/InfernalSugar/pencilpilot)";

/// Search results are cached this long (Pixabay requires 24 h; the others
/// just save calls).
pub const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceId {
    Pixabay,
    Openverse,
    Artic,
    Met,
}

impl SourceId {
    pub const ALL: [SourceId; 4] = [Self::Pixabay, Self::Openverse, Self::Artic, Self::Met];

    /// Stored in `LiveRef::source` and used as the topics.json key.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pixabay => "pixabay",
            Self::Openverse => "openverse",
            Self::Artic => "artic",
            Self::Met => "met",
        }
    }

    /// May this source fill `category` at all? (The topic must also list
    /// queries for it.)
    pub fn allows(self, category: &str) -> bool {
        match self {
            Self::Pixabay => category != "masters",
            Self::Openverse => category != "fundamentals",
            Self::Artic | Self::Met => category == "masters",
        }
    }
}

/// One search result, not yet checked. `key` is unique across sources
/// (`openverse:<uuid>`) and goes into the pool's `seen` set.
#[derive(Debug, Clone)]
pub struct Hit {
    pub key: String,
    pub raw: Value,
}

/// One page of search results.
#[derive(Debug, Clone, Default)]
pub struct SearchPage {
    pub hits: Vec<Hit>,
    /// no pages after this one
    pub last: bool,
}

/// A picture that passed its source's licence and provider rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub source: SourceId,
    /// our id, e.g. `ov-3759b17d84604335af618b1c47921a12`
    pub id: String,
    pub source_id: u64,
    /// the file to download
    pub image_url: String,
    pub page_url: String,
    pub author: String,
    pub author_url: String,
    pub license: String,
    pub license_url: String,
    pub title: String,
    pub tags: Vec<String>,
    /// full credit line from the source (Openverse), or empty
    pub attribution: String,
    /// who holds the picture: "Pixabay", "Wikimedia Commons", "The Met", …
    pub provider: String,
    /// "photo", "vector", "illustration" or "artwork"
    pub kind: String,
    /// size reported by the source (0 = unknown)
    pub width: u32,
    pub height: u32,
}

/// 24 h JSON cache of search results (and Met object records).
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    fn path(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let path = self.path(key);
        let age = SystemTime::now()
            .duration_since(std::fs::metadata(&path).ok()?.modified().ok()?)
            .ok()?;
        if age >= CACHE_TTL {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn put(&self, key: &str, v: &Value) {
        std::fs::write(self.path(key), v.to_string()).ok();
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
        .expect("http client")
}

/// Take one call from `budget`, send `req`, and pause the source on HTTP 429.
pub async fn send(
    budget: &Budget,
    req: reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    budget.take().await?;
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status().as_u16() == 429 {
        budget.pause(retry_after(resp.headers(), 3600)).await;
        return Err(format!("{}: rate limited (429)", budget.name()));
    }
    Ok(resp)
}

pub async fn fetch_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let mut req = http.get(url);
    if url.starts_with("https://www.artic.edu/") {
        // the Art Institute's image server turns away clients that don't say
        // who they are in this header
        req = req.header("AIC-User-Agent", USER_AGENT);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// Text without HTML tags and with entities for `&<>"'` decoded; whitespace
/// collapsed. Openverse titles from Wikimedia sometimes are raw HTML.
pub fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cut `s` to at most `max` characters (on a char boundary), adding "…".
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Every configured source. `None` = disabled (no key, or turned off).
pub struct Sources {
    pub pixabay: Option<pixabay::Pixabay>,
    pub openverse: Option<openverse::Openverse>,
    pub artic: Option<artic::Artic>,
    pub met: Option<met::Met>,
    http: reqwest::Client,
}

impl Sources {
    pub fn new(
        pixabay: Option<pixabay::Pixabay>,
        openverse: Option<openverse::Openverse>,
        artic: Option<artic::Artic>,
        met: Option<met::Met>,
    ) -> Self {
        Self {
            pixabay,
            openverse,
            artic,
            met,
            http: http_client(),
        }
    }

    pub fn enabled(&self, s: SourceId) -> bool {
        match s {
            SourceId::Pixabay => self.pixabay.is_some(),
            SourceId::Openverse => self.openverse.is_some(),
            SourceId::Artic => self.artic.is_some(),
            SourceId::Met => self.met.is_some(),
        }
    }

    fn budget(&self, s: SourceId) -> Option<&Budget> {
        match s {
            SourceId::Pixabay => self.pixabay.as_ref().map(|x| &x.budget),
            SourceId::Openverse => self.openverse.as_ref().map(|x| &x.budget),
            SourceId::Artic => self.artic.as_ref().map(|x| &x.budget),
            SourceId::Met => self.met.as_ref().map(|x| &x.budget),
        }
    }

    /// Enabled, and not paused or out of budget.
    pub async fn ready(&self, s: SourceId) -> bool {
        match self.budget(s) {
            Some(b) => b.available().await,
            None => false,
        }
    }

    /// One page of hits for `query`, for a picture in `category`/`tier`.
    pub async fn search(
        &self,
        s: SourceId,
        query: &str,
        category: &str,
        tier: u8,
        page: u32,
    ) -> Result<SearchPage, String> {
        let off = || format!("{} is disabled", s.name());
        match s {
            SourceId::Pixabay => {
                let px = self.pixabay.as_ref().ok_or_else(off)?;
                px.search(query, crate::model::image_type(category, tier), page)
                    .await
            }
            SourceId::Openverse => {
                let ov = self.openverse.as_ref().ok_or_else(off)?;
                ov.search(query, category, page).await
            }
            SourceId::Artic => {
                self.artic
                    .as_ref()
                    .ok_or_else(off)?
                    .search(query, page)
                    .await
            }
            SourceId::Met => self.met.as_ref().ok_or_else(off)?.search(query, page).await,
        }
    }

    /// Check one hit against its source's rules (licence, provider, flags,
    /// public domain, artwork type). `Ok(Err(reason code))` = skip it for
    /// good; `Err` = couldn't check it now (network, budget), try later.
    pub async fn resolve(
        &self,
        s: SourceId,
        hit: &Hit,
        category: &str,
    ) -> Result<Result<Candidate, String>, String> {
        Ok(match s {
            SourceId::Pixabay => pixabay::candidate(&hit.raw).map_err(String::from),
            SourceId::Openverse => openverse::candidate(&hit.raw, category).map_err(String::from),
            SourceId::Artic => {
                let a = self.artic.as_ref().ok_or("artic is disabled")?;
                artic::candidate(&hit.raw, &a.iiif_url).map_err(String::from)
            }
            SourceId::Met => {
                let m = self.met.as_ref().ok_or("met is disabled")?;
                m.candidate(&hit.raw).await?
            }
        })
    }

    /// The shared client for image downloads.
    pub fn client(&self) -> reqwest::Client {
        self.http.clone()
    }
}

/// The picture kind a photo-only category needs, for sources that don't say.
pub fn wanted_kind(category: &str) -> &'static str {
    if PHOTO_ONLY.contains(&category) {
        "photo"
    } else if category == "masters" {
        "artwork"
    } else {
        "illustration"
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A tiny HTTP server for tests: `routes` maps a path (without query)
    /// to (content type, body). Returns the base URL.
    pub async fn mock_server(routes: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
        use axum::http::header;
        use axum::response::IntoResponse;
        let mut app = axum::Router::new();
        for (path, ctype, body) in routes {
            app = app.route(
                path,
                axum::routing::get(move || {
                    let body = body.clone();
                    async move { ([(header::CONTENT_TYPE, ctype)], body).into_response() }
                }),
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        format!("http://{addr}")
    }

    #[test]
    fn plain_text_strips_html() {
        assert_eq!(
            plain_text("<div class='fn'> <b>Still life</b> &amp; fruit</div>"),
            "Still life & fruit"
        );
        assert_eq!(plain_text("a  b\n c"), "a b c");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abc", 4), "abc");
    }

    #[test]
    fn source_categories() {
        assert!(SourceId::Pixabay.allows("anime"));
        assert!(!SourceId::Pixabay.allows("masters"));
        assert!(SourceId::Met.allows("masters"));
        assert!(!SourceId::Artic.allows("animals"));
        assert!(SourceId::Openverse.allows("animals"));
        assert!(!SourceId::Openverse.allows("fundamentals"));
        assert_eq!(wanted_kind("animals"), "photo");
        assert_eq!(wanted_kind("masters"), "artwork");
    }

    #[tokio::test]
    async fn cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("pp-cache-{}", std::process::id()));
        let c = Cache::new(dir);
        assert!(c.get("x y").is_none());
        c.put("x y", &serde_json::json!({"a": 1}));
        assert_eq!(c.get("x y").unwrap()["a"], 1);
    }
}
