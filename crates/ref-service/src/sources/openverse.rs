//! Openverse (<https://api.openverse.org/v1/images/>): openly licensed
//! pictures from museums, NASA and Wikimedia Commons.
//!
//! Licence rule (ADR-0003): **CC0, Public Domain Mark and CC BY only**. No
//! NC, ND or SA licence, ever. The search asks for those licences, and every
//! result's own `license` field is checked again.
//!
//! Provider rule: only the institutions in [`PHOTO_PROVIDERS`] and
//! [`ART_PROVIDERS`]. No Flickr, iNaturalist or other sites where anyone can
//! upload anything under any licence ("licence laundering"). Wikimedia
//! Commons is allowed: its volunteers check licences and delete copyright
//! violations.
//!
//! Anonymous use allows 20 calls a minute and 200 a day; we stay well under
//! that (one call every 4 s, at most 150 a day). With `OPENVERSE_TOKEN` (an
//! OAuth access token for a registered app) the daily budget is higher.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{Cache, Candidate, Hit, SearchPage, SourceId, clip, http_client, plain_text, send};
use crate::budget::Budget;
use crate::model::PHOTO_ONLY;

pub const API: &str = "https://api.openverse.org/v1/images/";

/// Licences we accept (Openverse's codes).
pub const LICENSES: &[&str] = &["cc0", "pdm", "by"];

/// Providers for real-life, animals and environment: photographers whose
/// licence is checked (Wikimedia) or public bodies (NASA, US National Park
/// Service, Smithsonian).
pub const PHOTO_PROVIDERS: &[&str] = &[
    "wikimedia",
    "nasa",
    "katmai_nps",
    "smithsonian_zoo_and_conservation",
    "smithsonian_gardens",
];

/// Providers for Master studies (and drawn categories): museums and
/// libraries that publish their public-domain works. Not Wikimedia: photos
/// of modern paintings there carry the photographer's licence, not the
/// painter's (a 1960s painting is still under copyright).
pub const ART_PROVIDERS: &[&str] = &[
    "met",
    "clevelandmuseum",
    "rijksmuseum",
    "smk",
    "brooklynmuseum",
    "nypl",
    "wellcome_collection",
    "bio_diversity",
    "smithsonian_american_art_museum",
    "smithsonian_portrait_gallery",
    "smithsonian_freer_gallery_of_art",
    "smithsonian_cooper_hewitt_museum",
];

/// Display names for the credit line.
fn provider_name(source: &str) -> String {
    match source {
        "wikimedia" => "Wikimedia Commons",
        "nasa" => "NASA",
        "katmai_nps" => "Katmai National Park (NPS)",
        "smithsonian_zoo_and_conservation" => "Smithsonian's National Zoo",
        "smithsonian_gardens" => "Smithsonian Gardens",
        "met" => "The Metropolitan Museum of Art",
        "clevelandmuseum" => "Cleveland Museum of Art",
        "rijksmuseum" => "Rijksmuseum",
        "smk" => "National Gallery of Denmark (SMK)",
        "brooklynmuseum" => "Brooklyn Museum",
        "nypl" => "New York Public Library",
        "wellcome_collection" => "Wellcome Collection",
        "bio_diversity" => "Biodiversity Heritage Library",
        "smithsonian_american_art_museum" => "Smithsonian American Art Museum",
        "smithsonian_portrait_gallery" => "National Portrait Gallery (Smithsonian)",
        "smithsonian_freer_gallery_of_art" => "Freer Gallery of Art (Smithsonian)",
        "smithsonian_cooper_hewitt_museum" => "Cooper Hewitt (Smithsonian)",
        other => other,
    }
    .to_string()
}

/// The providers searched for a picture in `category`.
pub fn providers(category: &str) -> &'static [&'static str] {
    if PHOTO_ONLY.contains(&category) {
        PHOTO_PROVIDERS
    } else {
        ART_PROVIDERS
    }
}

pub struct Openverse {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
    cache: Cache,
    pub budget: Budget,
}

impl Openverse {
    pub fn new(token: Option<String>, cache_dir: PathBuf, budget_file: Option<PathBuf>) -> Self {
        let windows = if token.is_some() {
            vec![(60, 60), (24 * 3600, 5000)]
        } else {
            vec![(60, 12), (24 * 3600, 150)]
        };
        Self {
            base: API.into(),
            token,
            http: http_client(),
            cache: Cache::new(cache_dir),
            budget: Budget::new("openverse", Duration::from_secs(4), windows, budget_file),
        }
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.into();
        self
    }

    /// One page (20 results) of CC0 / PDM / CC BY pictures from the allowed
    /// providers for `category`.
    pub async fn search(&self, q: &str, category: &str, page: u32) -> Result<SearchPage, String> {
        let group = if PHOTO_ONLY.contains(&category) {
            "photo"
        } else {
            "art"
        };
        let key = format!("openverse-{q}-{group}-{page}");
        let v = match self.cache.get(&key) {
            Some(v) => v,
            None => {
                let mut req = self.http.get(&self.base).query(&[
                    ("q", q),
                    ("license", &LICENSES.join(",")),
                    ("license_type", "commercial,modification"),
                    ("source", &providers(category).join(",")),
                    ("mature", "false"),
                    ("page_size", "20"),
                    ("page", &page.to_string()),
                ]);
                if let Some(t) = &self.token {
                    req = req.bearer_auth(t);
                }
                let resp = send(&self.budget, req).await?;
                self.respect_headers(resp.headers()).await;
                if matches!(resp.status().as_u16(), 400 | 404) {
                    // past the last page anonymous users may see
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
        let hits = v["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Hit {
                    key: format!("openverse:{}", r["id"].as_str()?),
                    raw: r.clone(),
                })
            })
            .collect();
        let pages = v["page_count"].as_u64().unwrap_or(0);
        Ok(SearchPage {
            hits,
            last: u64::from(page) >= pages.min(10),
        })
    }

    /// Openverse says how many calls are left; pause when one runs out.
    async fn respect_headers(&self, h: &reqwest::header::HeaderMap) {
        for (name, value) in h {
            let n = name.as_str();
            if n.starts_with("x-ratelimit-available-") && value.to_str().ok() == Some("0") {
                let secs = if n.contains("sustained") {
                    6 * 3600
                } else {
                    90
                };
                self.budget.pause(secs).await;
            }
        }
    }
}

/// A smaller rendition: Wikimedia serves standard thumbnail widths, so a
/// 6000 px original becomes a 1280 px JPEG/PNG.
pub fn sized_url(url: &str, width: u32) -> String {
    const COMMONS: &str = "https://upload.wikimedia.org/wikipedia/commons/";
    if width > 1280
        && let Some(rest) = url.strip_prefix(COMMONS)
        && let [a, b, file] = rest.splitn(3, '/').collect::<Vec<_>>()[..]
        && !file.contains('/')
    {
        return format!("{COMMONS}thumb/{a}/{b}/{file}/1280px-{file}");
    }
    url.to_string()
}

fn license_label(code: &str, version: &str) -> String {
    let v = if version.is_empty() {
        String::new()
    } else {
        format!(" {version}")
    };
    match code {
        "cc0" => format!("CC0{v}"),
        "pdm" => format!("Public Domain Mark{v}"),
        "by" => format!("CC BY{v}"),
        other => other.to_uppercase(),
    }
}

/// Check one Openverse result (licence, provider, mature flag, file type,
/// size) and turn it into a candidate for `category`.
pub fn candidate(r: &Value, category: &str) -> Result<Candidate, &'static str> {
    let uuid = r["id"].as_str().ok_or("no_id")?;
    let license = r["license"].as_str().unwrap_or("").to_lowercase();
    if !LICENSES.contains(&license.as_str()) {
        return Err("licence");
    }
    let source = r["source"].as_str().unwrap_or("");
    let provider = r["provider"].as_str().unwrap_or(source);
    if !providers(category).contains(&source) || !providers(category).contains(&provider) {
        return Err("provider");
    }
    if r["mature"].as_bool() != Some(false) {
        return Err("mature");
    }
    if r["unstable__sensitivity"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        return Err("sensitive");
    }
    let filetype = r["filetype"].as_str().unwrap_or("").to_lowercase();
    if !matches!(filetype.as_str(), "jpg" | "jpeg" | "png" | "webp") {
        return Err("file_type");
    }
    let (w, h) = (
        r["width"].as_u64().unwrap_or(0) as u32,
        r["height"].as_u64().unwrap_or(0) as u32,
    );
    if w.min(h) < 400 {
        return Err("tiny");
    }
    let url = r["url"].as_str().ok_or("no_image")?;
    let page_url = r["foreign_landing_url"].as_str().unwrap_or("");
    if !url.starts_with("https://") || !page_url.starts_with("https://") {
        return Err("no_https");
    }
    if source != "wikimedia" && r["filesize"].as_u64().unwrap_or(0) > 20_000_000 {
        return Err("too_large");
    }
    // Wikimedia files mostly have no category; then the kind is the one the
    // category asks for (photos: JPEG only), and the picture sorter's style
    // check has the final say.
    let kind = match r["category"].as_str() {
        Some("photograph") => "photo",
        Some("digitized_artwork" | "illustration") => {
            if category == "masters" {
                "artwork"
            } else {
                "illustration"
            }
        }
        Some(_) => return Err("kind"),
        None if PHOTO_ONLY.contains(&category) => {
            if !matches!(filetype.as_str(), "jpg" | "jpeg") {
                return Err("kind");
            }
            "photo"
        }
        None => super::wanted_kind(category),
    };
    let hex: String = uuid.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != 32 {
        return Err("bad_id");
    }
    let title = clip(&plain_text(r["title"].as_str().unwrap_or("")), 120);
    let version = r["license_version"].as_str().unwrap_or("");
    let license_url = r["license_url"]
        .as_str()
        .filter(|u| u.starts_with("https://creativecommons.org/"))
        .map(String::from)
        .unwrap_or_else(|| match license.as_str() {
            "cc0" => "https://creativecommons.org/publicdomain/zero/1.0/".into(),
            "pdm" => "https://creativecommons.org/publicdomain/mark/1.0/".into(),
            _ => format!("https://creativecommons.org/licenses/by/{version}/"),
        });
    let author = clip(&plain_text(r["creator"].as_str().unwrap_or("")), 80);
    Ok(Candidate {
        source: SourceId::Openverse,
        id: format!("ov-{hex}"),
        source_id: u64::from_str_radix(&hex[..16], 16).unwrap_or(0),
        image_url: sized_url(url, w),
        page_url: page_url.into(),
        author: if author.is_empty() {
            "Unknown".into()
        } else {
            author
        },
        author_url: r["creator_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .unwrap_or("")
            .into(),
        license: license_label(&license, version),
        license_url,
        title: if title.is_empty() {
            "Untitled".into()
        } else {
            title
        },
        tags: r["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .take(20)
            .collect(),
        attribution: clip(&plain_text(r["attribution"].as_str().unwrap_or("")), 400),
        provider: provider_name(source),
        kind: kind.into(),
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub fn result() -> Value {
        json!({
            "id": "3759b17d-8460-4335-af61-8b1c47921a12",
            "title": "<div class='fn'>Tawny <b>owl</b></div>",
            "foreign_landing_url": "https://commons.wikimedia.org/w/index.php?curid=1",
            "url": "https://upload.wikimedia.org/wikipedia/commons/7/7c/Owl.jpg",
            "creator": "Jane Doe",
            "creator_url": "https://commons.wikimedia.org/wiki/User:Jane",
            "license": "by",
            "license_version": "4.0",
            "license_url": "https://creativecommons.org/licenses/by/4.0/",
            "provider": "wikimedia",
            "source": "wikimedia",
            "category": null,
            "filesize": 2_000_000,
            "filetype": "jpg",
            "tags": [{"name": "owl"}, {"name": "bird"}],
            "attribution": "\"Tawny owl\" by Jane Doe is licensed under CC BY 4.0.",
            "mature": false,
            "width": 3000,
            "height": 2000,
            "unstable__sensitivity": []
        })
    }

    #[test]
    fn accepts_clean_licences_from_allowed_providers() {
        let c = candidate(&result(), "animals").unwrap();
        assert_eq!(c.id, "ov-3759b17d84604335af618b1c47921a12");
        assert_eq!(c.id.len() + 4, 39, "fits the /refimg/ name limit");
        assert_eq!(c.title, "Tawny owl");
        assert_eq!(c.license, "CC BY 4.0");
        assert_eq!(c.provider, "Wikimedia Commons");
        assert_eq!(c.kind, "photo");
        assert_eq!(
            c.image_url,
            "https://upload.wikimedia.org/wikipedia/commons/thumb/7/7c/Owl.jpg/1280px-Owl.jpg"
        );
        assert!(c.attribution.starts_with("\"Tawny owl\" by Jane Doe"));
        for ok in ["cc0", "pdm", "by", "BY"] {
            let mut r = result();
            r["license"] = json!(ok);
            assert!(candidate(&r, "animals").is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_nc_nd_sa_and_unknown_licences() {
        for bad in [
            "by-nc",
            "by-nd",
            "by-sa",
            "by-nc-sa",
            "by-nc-nd",
            "nc",
            "sampling+",
            "",
        ] {
            let mut r = result();
            r["license"] = json!(bad);
            assert_eq!(candidate(&r, "animals"), Err("licence"), "{bad}");
        }
    }

    #[test]
    fn rejects_user_upload_sites_and_wrong_providers() {
        for bad in [
            "flickr",
            "inaturalist",
            "geographorguk",
            "wordpress",
            "rawpixel",
        ] {
            let mut r = result();
            r["source"] = json!(bad);
            r["provider"] = json!(bad);
            assert_eq!(candidate(&r, "animals"), Err("provider"), "{bad}");
        }
        // a museum is fine for Master studies but not for photo categories
        let mut r = result();
        r["source"] = json!("met");
        r["provider"] = json!("met");
        assert_eq!(candidate(&r, "animals"), Err("provider"));
        assert_eq!(candidate(&r, "masters").unwrap().kind, "artwork");
        // Wikimedia photos of paintings carry the photographer's licence: not for Master studies
        assert_eq!(candidate(&result(), "masters"), Err("provider"));
    }

    #[test]
    fn rejects_mature_small_and_non_photos_in_photo_categories() {
        let mut r = result();
        r["mature"] = json!(true);
        assert_eq!(candidate(&r, "animals"), Err("mature"));
        let mut r = result();
        r["unstable__sensitivity"] = json!(["sensitive_text"]);
        assert_eq!(candidate(&r, "animals"), Err("sensitive"));
        let mut r = result();
        r["height"] = json!(300);
        assert_eq!(candidate(&r, "animals"), Err("tiny"));
        let mut r = result();
        r["filetype"] = json!("png");
        assert_eq!(
            candidate(&r, "animals"),
            Err("kind"),
            "a PNG is rarely a photo"
        );
        let mut r = result();
        r["category"] = json!("illustration");
        assert!(candidate(&r, "animals").is_ok_and(|c| c.kind == "illustration"));
        r["filetype"] = json!("svg");
        assert_eq!(candidate(&r, "animals"), Err("file_type"));
    }

    #[test]
    fn small_wikimedia_files_are_used_as_is() {
        assert_eq!(
            sized_url(
                "https://upload.wikimedia.org/wikipedia/commons/7/7c/Owl.jpg",
                900
            ),
            "https://upload.wikimedia.org/wikipedia/commons/7/7c/Owl.jpg"
        );
        assert_eq!(
            sized_url("https://images.example.org/a.jpg", 5000),
            "https://images.example.org/a.jpg"
        );
    }

    #[tokio::test]
    async fn search_sends_licence_and_provider_filters() {
        use axum::extract::RawQuery;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let s2 = seen.clone();
        let body = json!({"result_count": 1, "page_count": 1, "results": [result()]}).to_string();
        let app = axum::Router::new().route(
            "/v1/images/",
            axum::routing::get(move |RawQuery(q): RawQuery| {
                *s2.lock().unwrap() = q.unwrap_or_default();
                let body = body.clone();
                async move { ([("content-type", "application/json")], body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });
        let dir = std::env::temp_dir().join(format!("pp-ov-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let ov = Openverse::new(None, dir, None).with_base(&format!("http://{addr}/v1/images/"));
        let page = ov.search("owl", "animals", 1).await.unwrap();
        assert_eq!(page.hits.len(), 1);
        assert!(page.last);
        assert_eq!(
            page.hits[0].key,
            "openverse:3759b17d-8460-4335-af61-8b1c47921a12"
        );
        let q = seen.lock().unwrap().clone();
        assert!(q.contains("license=cc0%2Cpdm%2Cby"), "{q}");
        assert!(q.contains("source=wikimedia%2Cnasa"), "{q}");
        assert!(!q.contains("flickr"), "{q}");
    }
}
