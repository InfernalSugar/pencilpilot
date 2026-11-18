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
    "cartoon",
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
    /// topic id inside the category (see content/topics.json)
    #[serde(default)]
    topic: String,
    /// 0–100 from `grader::difficulty` (higher = harder to draw)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    difficulty: Option<f32>,
    /// automatic tier: 1 Beginner, 2 Easy, 3 Medium, 4 Hard
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier: Option<u8>,
    /// a reviewer's tier, which wins over the automatic one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier_override: Option<u8>,
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
        "gun",
        "rifle",
        "pistol",
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

fn fetch(category: &str, query: &str, kind: &str, count: usize, topic: Option<&str>) {
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
                topic: topic.unwrap_or("").into(),
                difficulty: None,
                tier: None,
                tier_override: None,
            });
            let r = cands.references.last_mut().unwrap();
            rate(r);
            assign_topic(r, &topics(), false);
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

/// The drawing canvas size references are fitted into (keep in sync with the web app).
const CANVAS: (u32, u32) = (800, 600);

/// Load a reference image and fit it (contain, centred, white background) into
/// the canvas exactly as the web app shows it, as grayscale.
fn fitted_gray(path: &Path) -> Result<grader::Gray, String> {
    // Pixabay sometimes serves PNG/BMP under a .jpg name: sniff the real format.
    let img = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    // transparent backgrounds (common in vectors) are white paper, not black
    let img = if img.color().has_alpha() {
        let mut rgba = img.to_rgba8();
        for p in rgba.pixels_mut() {
            let a = p[3] as u32;
            for c in 0..3 {
                p[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
            }
            p[3] = 255;
        }
        image::DynamicImage::ImageRgba8(rgba).to_luma8()
    } else {
        img.to_luma8()
    };
    let (w, h) = CANVAS;
    let s = (w as f32 / img.width() as f32).min(h as f32 / img.height() as f32);
    let (dw, dh) = (
        ((img.width() as f32 * s).round() as u32).clamp(1, w),
        ((img.height() as f32 * s).round() as u32).clamp(1, h),
    );
    let small = image::imageops::resize(&img, dw, dh, image::imageops::FilterType::Triangle);
    let mut canvas = image::GrayImage::from_pixel(w, h, image::Luma([255]));
    image::imageops::overlay(
        &mut canvas,
        &small,
        ((w - dw) / 2) as i64,
        ((h - dh) / 2) as i64,
    );
    grader::Gray::new(w, h, canvas.into_raw()).map_err(|e| format!("{e:?}"))
}

/// Compute difficulty + automatic tier from the downloaded image.
fn rate(r: &mut Reference) {
    match fitted_gray(&images_dir().join(format!("{}.jpg", r.id))) {
        Ok(g) => {
            let d = grader::difficulty::difficulty(&g);
            r.difficulty = Some((d.score * 10.0).round() / 10.0);
            r.tier = Some(d.tier);
        }
        Err(e) => eprintln!("  {}: can't rate ({e})", r.id),
    }
}

fn topics() -> Value {
    serde_json::from_str(&fs::read_to_string(root().join("content/topics.json")).unwrap())
        .expect("content/topics.json")
}

/// Pick the first topic whose tags match the reference's tags (topics are
/// ordered specific → general; the last one is the fallback).
fn topic_for(category: &str, tags: &[String], topics: &Value) -> String {
    let list = topics[category].as_array().cloned().unwrap_or_default();
    let tags: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    list.iter()
        .find(|t| {
            t["tags"].as_array().is_some_and(|ks| {
                ks.iter()
                    .filter_map(Value::as_str)
                    .any(|k| tags.iter().any(|g| g == k))
            })
        })
        .or(list.last())
        .and_then(|t| t["id"].as_str())
        .unwrap_or("")
        .to_string()
}

fn assign_topic(r: &mut Reference, topics: &Value, force: bool) {
    let valid = topics[r.category.as_str()]
        .as_array()
        .is_some_and(|l| l.iter().any(|t| t["id"] == r.topic.as_str()));
    if force || !valid {
        r.topic = topic_for(&r.category, &r.tags, topics);
    }
}

/// (Re)compute difficulty, tier and (missing) topics for every reference.
fn classify(report: bool, retopic: bool) {
    let topics = topics();
    let mut rows = Vec::new();
    let files: Vec<String> = CATEGORIES
        .iter()
        .map(|c| format!("{c}.json"))
        .chain(["candidates.json".to_string()])
        .collect();
    for f in files {
        let path = refs_dir().join(&f);
        if !path.exists() {
            continue;
        }
        let mut list: RefList = read_json(&path);
        for r in &mut list.references {
            rate(r);
            assign_topic(r, &topics, retopic);
            rows.push((
                r.difficulty.unwrap_or(-1.0),
                r.tier_override.or(r.tier).unwrap_or(0),
                format!("{}/{}", r.category, r.topic),
                r.id.clone(),
                r.tags
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        write_json(&path, &list);
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    if report {
        for (d, t, c, id, tags) in &rows {
            println!("{d:>5.1}  T{t}  {c:<24} {id:<12} {tags}");
        }
    }
    let mut per = [0usize; 5];
    for r in &rows {
        per[r.1 as usize] += 1;
    }
    let n = grader::difficulty::TIER_NAMES;
    println!(
        "{} rated: {} {} · {} {} · {} {} · {} {}",
        rows.len(),
        n[0],
        per[1],
        n[1],
        per[2],
        n[2],
        per[3],
        n[3],
        per[4]
    );
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
            flag("--topic").as_deref(),
        ),
        Some("sync") => sync(),
        Some("status") => status(),
        Some("classify") => classify(
            args.iter().any(|a| a == "--report"),
            args.iter().any(|a| a == "--retopic"),
        ),
        _ => {
            eprintln!(
                "usage:\n  ref-importer fetch <category> <query> [--type photo|illustration|vector] [--count N] [--topic ID]\n  ref-importer sync\n  ref-importer classify [--report] [--retopic]\n  ref-importer status"
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
    fn topics_match_tags_specific_first() {
        let t = json!({"animals": [
            {"id": "birds", "tags": ["bird", "owl"]},
            {"id": "pets", "tags": ["cat", "dog"]},
            {"id": "wildlife", "tags": ["fox"]}
        ]});
        let tags = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(topic_for("animals", &tags(&["Cat", "pet"]), &t), "pets");
        assert_eq!(topic_for("animals", &tags(&["owl", "cat"]), &t), "birds");
        assert_eq!(
            topic_for("animals", &tags(&["rock"]), &t),
            "wildlife",
            "falls back to the last topic"
        );
        assert_eq!(topic_for("nope", &tags(&["cat"]), &t), "");
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
