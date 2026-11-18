//! Reference importer for PencilPilot (see docs/planning/adr/0003-reference-images.md).
//!
//! ```text
//! cargo run -p ref-importer -- fetch <category> <query> [--type photo|illustration|vector] [--count N]
//! cargo run -p ref-importer -- sync
//! cargo run -p ref-importer -- status
//! ```
//!
//! * `fetch` asks Pixabay (SafeSearch on) and adds new **candidates** to
//!   `content/references/candidates.json`. Images flagged AI-generated, not
//!   G-rated, or low quality are skipped outright. Every candidate still needs
//!   a person to approve it on the dev review page (`/dev/references`).
//! * `sync` re-downloads image files for approved references and candidates
//!   (images are never committed: Pixabay forbids hotlinking and standalone
//!   redistribution, so each machine/deploy downloads its own copies).
//!
//! The API key is read from `PIXABAY_API_KEY` (environment or the repo's
//! git-ignored `.env`).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const CATEGORIES: &[&str] = &[
    "anime",
    "real-life",
    "animals",
    "environment",
    "fundamentals",
];
const LICENSE: &str = "Pixabay Content License";
const LICENSE_URL: &str = "https://pixabay.com/service/license-summary/";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    id: String,
    source: String,
    source_id: u64,
    category: String,
    page_url: String,
    author: String,
    author_url: String,
    license: String,
    license_url: String,
    tags: Vec<String>,
    width: u32,
    height: u32,
    query: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    keypoints: Vec<Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RefList {
    references: Vec<Reference>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Rejected {
    ids: Vec<String>,
}

/// Shared HTTP client with timeouts, so one stalled download can't hang the run.
fn http() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(45)))
            .user_agent("pencilpilot-ref-importer")
            .build()
            .into()
    })
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn refs_dir() -> PathBuf {
    root().join("content/references")
}
fn images_dir() -> PathBuf {
    root().join("apps/web/static/references")
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(p: &Path, v: &T) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
}

fn api_key() -> String {
    if let Ok(k) = std::env::var("PIXABAY_API_KEY") {
        return k;
    }
    let env = fs::read_to_string(root().join(".env")).unwrap_or_default();
    env.lines()
        .find_map(|l| l.strip_prefix("PIXABAY_API_KEY="))
        .map(|s| s.trim().to_string())
        .expect("PIXABAY_API_KEY not set (environment or .env)")
}

fn known_ids() -> HashSet<String> {
    let mut ids: HashSet<String> = read_json::<Rejected>(&refs_dir().join("rejected.json"))
        .ids
        .into_iter()
        .collect();
    for f in ["candidates.json"]
        .into_iter()
        .map(String::from)
        .chain(CATEGORIES.iter().map(|c| format!("{c}.json")))
    {
        ids.extend(
            read_json::<RefList>(&refs_dir().join(f))
                .references
                .into_iter()
                .map(|r| r.id),
        );
    }
    ids
}

/// Pixabay's "large" rendition (max 1280 px). The signed `/get/` links only
/// exist in the sizes the API hands out, so we can't ask for other sizes.
fn sized_url(hit: &Value) -> Option<String> {
    hit["largeImageURL"]
        .as_str()
        .or_else(|| hit["webformatURL"].as_str())
        .map(String::from)
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let mut resp = http().get(url).call().map_err(|e| e.to_string())?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(dest, bytes).map_err(|e| e.to_string())
}

/// Why a hit is rejected automatically, if it is.
fn auto_reject(hit: &Value) -> Option<&'static str> {
    if hit["isAiGenerated"].as_bool() != Some(false) {
        return Some("AI-generated (or unknown)");
    }
    if hit["isGRated"].as_bool() != Some(true) {
        return Some("not G-rated");
    }
    if hit["isLowQuality"].as_bool() == Some(true) {
        return Some("low quality");
    }
    let tags = hit["tags"].as_str().unwrap_or("").to_lowercase();
    const BANNED: &[&str] = &[
        "ai generated",
        "ai-generated",
        "generative",
        "midjourney",
        "stable diffusion",
        "dall-e",
        "nude",
        "naked",
        "sexy",
        "lingerie",
        "bikini",
        "erotic",
        "blood",
        "gore",
        "weapon",
        "gun",
        "pokemon",
        "naruto",
        "disney",
        "marvel",
        "mickey",
        "pikachu",
    ];
    BANNED
        .iter()
        .any(|b| tags.contains(b))
        .then_some("banned tag (AI / NSFW / franchise)")
}

fn search(key: &str, query: &str, kind: &str, page: u32) -> Value {
    let mut resp = http()
        .get("https://pixabay.com/api/")
        .query("key", key)
        .query("q", query)
        .query("image_type", kind)
        .query("safesearch", "true")
        .query("per_page", "50")
        .query("page", page.to_string())
        .call()
        .unwrap_or_else(|e| panic!("Pixabay search failed: {e}"));
    resp.body_mut().read_json().expect("Pixabay JSON")
}

fn fetch(category: &str, query: &str, kind: &str, count: usize) {
    assert!(
        CATEGORIES.contains(&category),
        "unknown category {category}; use one of {CATEGORIES:?}"
    );
    let key = api_key();
    let known = known_ids();
    let cand_path = refs_dir().join("candidates.json");
    let mut cands: RefList = read_json(&cand_path);
    let (mut added, mut skipped) = (0, 0);

    'pages: for page in 1..=5 {
        let res = search(&key, query, kind, page);
        let hits = res["hits"].as_array().cloned().unwrap_or_default();
        if hits.is_empty() {
            break;
        }
        for hit in hits {
            let sid = hit["id"].as_u64().unwrap_or(0);
            let id = format!("px-{sid}");
            if known.contains(&id) || cands.references.iter().any(|r| r.id == id) {
                continue;
            }
            if let Some(why) = auto_reject(&hit) {
                skipped += 1;
                eprintln!("  skip {id}: {why}");
                continue;
            }
            let Some(url) = sized_url(&hit) else { continue };
            if let Err(e) = download(&url, &images_dir().join(format!("{id}.jpg"))) {
                eprintln!("  download failed for {id}: {e}");
                continue;
            }
            let user = hit["user"].as_str().unwrap_or("").to_string();
            cands.references.push(Reference {
                id: id.clone(),
                source: "pixabay".into(),
                source_id: sid,
                category: category.into(),
                page_url: hit["pageURL"].as_str().unwrap_or("").into(),
                author_url: format!(
                    "https://pixabay.com/users/{}-{}/",
                    user.to_lowercase().replace(' ', "_"),
                    hit["user_id"].as_u64().unwrap_or(0)
                ),
                author: user,
                license: LICENSE.into(),
                license_url: LICENSE_URL.into(),
                tags: hit["tags"]
                    .as_str()
                    .unwrap_or("")
                    .split(", ")
                    .map(String::from)
                    .collect(),
                width: hit["imageWidth"].as_u64().unwrap_or(0) as u32,
                height: hit["imageHeight"].as_u64().unwrap_or(0) as u32,
                query: query.into(),
                title: String::new(),
                keypoints: Vec::new(),
            });
            added += 1;
            write_json(&cand_path, &cands); // save progress as we go
            println!("  + {id} by {}", cands.references.last().unwrap().author);
            if added >= count {
                break 'pages;
            }
        }
        sleep(Duration::from_millis(800)); // stay well under the 100 req/min limit
    }
    write_json(&cand_path, &cands);
    println!(
        "{added} new candidates for {category} ({skipped} auto-rejected). Review them at /dev/references."
    );
}

fn sync() {
    let key = api_key();
    let mut lists: Vec<Reference> = CATEGORIES
        .iter()
        .flat_map(|c| read_json::<RefList>(&refs_dir().join(format!("{c}.json"))).references)
        .collect();
    lists.extend(read_json::<RefList>(&refs_dir().join("candidates.json")).references);
    let (mut ok, mut missing) = (0, 0);
    for r in lists {
        let dest = images_dir().join(format!("{}.jpg", r.id));
        if dest.exists() {
            ok += 1;
            continue;
        }
        let mut resp = match http()
            .get("https://pixabay.com/api/")
            .query("key", &key)
            .query("id", r.source_id.to_string())
            .call()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  {}: lookup failed: {e}", r.id);
                missing += 1;
                continue;
            }
        };
        let v: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
        match v["hits"].get(0).and_then(sized_url) {
            Some(url) => match download(&url, &dest) {
                Ok(()) => {
                    ok += 1;
                    println!("  ↓ {}", r.id);
                }
                Err(e) => {
                    eprintln!("  {}: {e}", r.id);
                    missing += 1;
                }
            },
            None => {
                eprintln!("  {}: no longer on Pixabay", r.id);
                missing += 1;
            }
        }
        sleep(Duration::from_millis(700));
    }
    println!("{ok} images present, {missing} missing");
}

fn status() {
    for c in CATEGORIES {
        let n = read_json::<RefList>(&refs_dir().join(format!("{c}.json")))
            .references
            .len();
        println!("{c:<13} {n} approved");
    }
    let n = read_json::<RefList>(&refs_dir().join("candidates.json"))
        .references
        .len();
    println!("candidates    {n} waiting for review");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match args.first().map(String::as_str) {
        Some("fetch") if args.len() >= 3 => fetch(
            &args[1],
            &args[2],
            &flag("--type").unwrap_or_else(|| "photo".into()),
            flag("--count").and_then(|c| c.parse().ok()).unwrap_or(10),
        ),
        Some("sync") => sync(),
        Some("status") => status(),
        _ => {
            eprintln!(
                "usage:\n  ref-importer fetch <category> <query> [--type photo|illustration|vector] [--count N]\n  ref-importer sync\n  ref-importer status"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(ai: bool, g: bool, low: bool, tags: &str) -> Value {
        json!({"isAiGenerated": ai, "isGRated": g, "isLowQuality": low, "tags": tags})
    }

    #[test]
    fn auto_reject_rules() {
        assert_eq!(auto_reject(&hit(false, true, false, "cat, pet")), None);
        assert!(auto_reject(&hit(true, true, false, "cat")).is_some());
        assert!(auto_reject(&hit(false, false, false, "cat")).is_some());
        assert!(auto_reject(&hit(false, true, true, "cat")).is_some());
        assert!(auto_reject(&hit(false, true, false, "anime, pikachu")).is_some());
        assert!(
            auto_reject(&json!({"isGRated": true, "tags": "cat"})).is_some(),
            "unknown AI flag is rejected"
        );
    }

    #[test]
    fn sized_url_prefers_large() {
        let h = json!({"largeImageURL": "L.jpg", "webformatURL": "W.jpg"});
        assert_eq!(sized_url(&h).unwrap(), "L.jpg");
        assert_eq!(
            sized_url(&json!({"webformatURL": "W.jpg"})).unwrap(),
            "W.jpg"
        );
    }
}
