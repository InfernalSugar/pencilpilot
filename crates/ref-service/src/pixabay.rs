//! Pixabay API client: 24 h search cache (required by Pixabay's API terms),
//! a request budget well under the 100 req/min limit, and image download.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use tokio::sync::Mutex;

const API: &str = "https://pixabay.com/api/";
const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);
/// Minimum gap between API calls (≈ 50/min, half the limit).
const API_GAP: Duration = Duration::from_millis(1200);

pub struct Pixabay {
    key: String,
    http: reqwest::Client,
    cache_dir: PathBuf,
    last_call: Mutex<Option<Instant>>,
}

impl Pixabay {
    pub fn new(key: String, cache_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            key,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("pencilpilot-ref-service")
                .build()
                .expect("http client"),
            cache_dir,
            last_call: Mutex::new(None),
        }
    }

    fn cache_path(&self, q: &str, kind: &str, page: u32) -> PathBuf {
        let safe: String = format!("{q}-{kind}-{page}")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.cache_dir.join(format!("{safe}.json"))
    }

    /// One search page (SafeSearch on, 50 per page), from cache when < 24 h old.
    pub async fn search(&self, q: &str, kind: &str, page: u32) -> Result<Vec<Value>, String> {
        let path = self.cache_path(q, kind, page);
        if let Ok(meta) = std::fs::metadata(&path)
            && meta
                .modified()
                .ok()
                .and_then(|m| SystemTime::now().duration_since(m).ok())
                .is_some_and(|age| age < CACHE_TTL)
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(v) = serde_json::from_str::<Value>(&text)
        {
            return Ok(v["hits"].as_array().cloned().unwrap_or_default());
        }
        {
            let mut last = self.last_call.lock().await;
            if let Some(t) = *last {
                let wait = API_GAP.saturating_sub(t.elapsed());
                tokio::time::sleep(wait).await;
            }
            *last = Some(Instant::now());
        }
        let resp = self
            .http
            .get(API)
            .query(&[
                ("key", self.key.as_str()),
                ("q", q),
                ("image_type", kind),
                ("safesearch", "true"),
                ("per_page", "50"),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().as_u16() == 400 {
            // past the last page
            return Ok(Vec::new());
        }
        let v: Value = resp
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        std::fs::write(&path, v.to_string()).ok();
        Ok(v["hits"].as_array().cloned().unwrap_or_default())
    }

    /// A cheap handle to the HTTP client, for parallel downloads.
    pub fn client(&self) -> reqwest::Client {
        self.http.clone()
    }
}

pub async fn fetch_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}
