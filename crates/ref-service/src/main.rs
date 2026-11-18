//! Endless references service (milestone M5b).
//!
//! ```text
//! GET  /api/ref?category=animals&topic=pets&tier=1&exclude=px-1,px-2
//!        → 200 LiveRef JSON, or 204 when nothing is ready (the web app then
//!          falls back to the hand-approved references)
//! GET  /refimg/<id>.jpg   → the picture (downloaded + re-encoded on our server,
//!                           as Pixabay requires; never hotlinked)
//! POST /api/report {id, reason} → block a picture for everyone
//! GET  /api/pool          → pool status
//! GET  /api/health
//! ```
//!
//! Config (env, or the repo's git-ignored `.env`): `PIXABAY_API_KEY`;
//! optional `REF_DATA_DIR` (default `data/ref-service`), `REF_PORT` (8787),
//! `REF_TOPICS` (content/topics.json), `REF_TARGET` (fresh pictures kept
//! ready per requested slot, default 6).

mod model;
mod pixabay;
mod pool;
mod vet;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

use model::{CATEGORIES, LiveRef, Slot, Topic, image_type};
use pixabay::Pixabay;
use pool::PoolState;
use vet::{FiltersVetter, Vetter, pixabay_flags_ok};

const LICENSE: &str = "Pixabay Content License";
const LICENSE_URL: &str = "https://pixabay.com/service/license-summary/";
/// How many search hits to examine per refill step before yielding.
const HITS_PER_STEP: usize = 12;

struct App {
    pool: Mutex<PoolState>,
    wake: Notify,
    pixabay: Pixabay,
    vetter: Box<dyn Vetter>,
    topics: HashMap<String, Vec<Topic>>,
    data: PathBuf,
    target: usize,
    /// (query, image_type) → next search page to try
    cursors: Mutex<HashMap<(String, String), u32>>,
}

impl App {
    fn images(&self) -> PathBuf {
        self.data.join("images")
    }

    async fn save(&self) {
        let text = {
            let p = self.pool.lock().await;
            serde_json::to_string(&*p).unwrap_or_default()
        };
        let tmp = self.data.join("pool.json.tmp");
        if tokio::fs::write(&tmp, text).await.is_ok() {
            tokio::fs::rename(&tmp, self.data.join("pool.json"))
                .await
                .ok();
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_or_dotenv(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| {
        std::fs::read_to_string(repo_root().join(".env"))
            .ok()?
            .lines()
            .find_map(|l| {
                l.strip_prefix(&format!("{name}="))
                    .map(|v| v.trim().to_string())
            })
    })
}

fn load_topics(path: &Path) -> HashMap<String, Vec<Topic>> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).expect("topics.json"))
        .expect("topics.json is JSON");
    CATEGORIES
        .iter()
        .filter_map(|c| {
            let list: Vec<Topic> = serde_json::from_value(v[*c].clone()).ok()?;
            Some((c.to_string(), list))
        })
        .collect()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let key = env_or_dotenv("PIXABAY_API_KEY").expect("PIXABAY_API_KEY not set");
    let data = env_or_dotenv("REF_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("data/ref-service"));
    std::fs::create_dir_all(data.join("images")).expect("data dir");
    let topics_path = env_or_dotenv("REF_TOPICS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("content/topics.json"));
    let port: u16 = env_or_dotenv("REF_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    // NSFW safety guard (ADR-0005). Refuse to run without it unless explicitly allowed.
    let model = env_or_dotenv("REF_NSFW_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("crates/ref-filters/models/open_nsfw.onnx"));
    let filters = match ref_filters::Filters::new(&model) {
        Ok(f) => {
            info!(model = %model.display(), "NSFW guard loaded");
            f
        }
        Err(e) if env_or_dotenv("REF_ALLOW_NO_NSFW").as_deref() == Some("1") => {
            warn!("NSFW guard NOT loaded ({e}); REF_ALLOW_NO_NSFW=1, dev only!");
            ref_filters::Filters::without_nsfw()
        }
        Err(e) => panic!(
            "NSFW model not available ({e}). Run crates/ref-filters/fetch-model.sh \
             (or set REF_ALLOW_NO_NSFW=1 for local experiments)."
        ),
    };
    let target = env_or_dotenv("REF_TARGET")
        .and_then(|t| t.parse().ok())
        .unwrap_or(6);

    let pool: PoolState = std::fs::read_to_string(data.join("pool.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    info!(refs = pool.refs.len(), "loaded pool");

    let app = Arc::new(App {
        pool: Mutex::new(pool),
        wake: Notify::new(),
        pixabay: Pixabay::new(key, data.join("search-cache")),
        vetter: Box::new(FiltersVetter(filters)),
        topics: load_topics(&topics_path),
        data,
        target,
        cursors: Mutex::new(HashMap::new()),
    });

    tokio::spawn(refill_worker(app.clone()));

    let router = routes(app);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    info!("ref-service on http://127.0.0.1:{port}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await
        .expect("server");
}

fn routes(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/ref", get(get_ref))
        .route("/api/report", post(report))
        .route("/api/pool", get(pool_status))
        .route("/refimg/{file}", get(image))
        .with_state(app)
}

#[derive(Deserialize)]
struct RefQuery {
    category: String,
    topic: Option<String>,
    tier: Option<u8>,
    exclude: Option<String>,
}

async fn get_ref(State(app): State<Arc<App>>, Query(q): Query<RefQuery>) -> Response {
    if !CATEGORIES.contains(&q.category.as_str()) {
        return (StatusCode::BAD_REQUEST, "unknown category").into_response();
    }
    let topic = q
        .topic
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "*".into());
    if topic != "*"
        && !app
            .topics
            .get(&q.category)
            .is_some_and(|l| l.iter().any(|t| t.id == topic))
    {
        return (StatusCode::BAD_REQUEST, "unknown topic").into_response();
    }
    let slot = Slot {
        category: q.category,
        topic,
        tier: q.tier.unwrap_or(1).clamp(1, 4),
    };
    let exclude: Vec<String> = q
        .exclude
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let got = {
        let mut p = app.pool.lock().await;
        // "any topic" requests count as demand for every topic in the category
        if slot.topic == "*" {
            for t in app.topics.get(&slot.category).into_iter().flatten() {
                p.note_demand(&Slot {
                    topic: t.id.clone(),
                    ..slot.clone()
                });
            }
        } else {
            p.note_demand(&slot);
        }
        p.take(&slot, &exclude, &mut rand::rng())
    };
    app.wake.notify_one();
    match got {
        Some(r) => Json(r).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

#[derive(Deserialize)]
struct Report {
    id: String,
    #[serde(default)]
    reason: String,
}

async fn report(State(app): State<Arc<App>>, Json(r): Json<Report>) -> Response {
    let known = app.pool.lock().await.block(&r.id);
    let line = json!({"id": r.id, "reason": r.reason.chars().take(200).collect::<String>(), "at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)});
    use tokio::io::AsyncWriteExt;
    if let Ok(mut f) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(app.data.join("reports.jsonl"))
        .await
    {
        f.write_all(format!("{line}\n").as_bytes()).await.ok();
    }
    app.save().await;
    info!(id = %r.id, known, "reported and blocked");
    Json(json!({"blocked": true})).into_response()
}

async fn pool_status(State(app): State<Arc<App>>) -> Json<Value> {
    let p = app.pool.lock().await;
    Json(json!({
        "refs": p.refs.len(),
        "blocked": p.blocked.len(),
        "seen": p.seen.len(),
        "rejected": p.rejected,
        "slots": p.summary().into_iter().map(|(k, (f, l))| (k, json!({"fresh": f, "served": l}))).collect::<serde_json::Map<_, _>>(),
    }))
}

async fn image(State(app): State<Arc<App>>, UrlPath(file): UrlPath<String>) -> Response {
    let ok = file.ends_with(".jpg")
        && file.len() < 40
        && file[..file.len() - 4]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return StatusCode::NOT_FOUND.into_response();
    }
    if app
        .pool
        .lock()
        .await
        .blocked
        .contains(&file[..file.len() - 4])
    {
        return StatusCode::GONE.into_response();
    }
    match tokio::fs::read(app.images().join(&file)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "public, max-age=604800, immutable"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Keeps asked-for slots topped up with `target` fresh, vetted pictures.
async fn refill_worker(app: Arc<App>) {
    loop {
        let slot = app.pool.lock().await.neediest(app.target);
        match slot {
            Some(slot) => {
                if let Err(e) = refill_step(&app, &slot).await {
                    warn!(slot = %slot.key(), "refill: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            None => {
                // nothing needed: sleep until a request arrives (or a minute passes)
                let _ = tokio::time::timeout(Duration::from_secs(60), app.wake.notified()).await;
            }
        }
    }
}

/// Look at up to HITS_PER_STEP new search hits for `slot`, vet them and add
/// the good ones to the pool (under the tier they actually measure as).
async fn refill_step(app: &App, slot: &Slot) -> Result<(), String> {
    let queries = app
        .topics
        .get(&slot.category)
        .and_then(|l| l.iter().find(|t| t.id == slot.topic))
        .map(|t| t.queries.clone())
        .filter(|q| !q.is_empty())
        .ok_or("topic has no queries")?;
    let kind = image_type(&slot.category, slot.tier).to_string();
    let before = app.pool.lock().await.fresh_len(slot);
    // rotate through the topic's queries
    let qi = app.pool.lock().await.seen.len() % queries.len();
    let query = queries[qi].clone();
    let page = {
        let mut c = app.cursors.lock().await;
        let p = c.entry((query.clone(), kind.clone())).or_insert(1);
        *p
    };
    let hits = app.pixabay.search(&query, &kind, page).await?;
    let fresh_hits: Vec<Value> = {
        let p = app.pool.lock().await;
        hits.into_iter()
            .filter(|h| h["id"].as_u64().is_some_and(|id| !p.seen.contains(&id)))
            .take(HITS_PER_STEP)
            .collect()
    };
    if fresh_hits.is_empty() {
        // page exhausted: next time try the next page (stop after 10 pages)
        let mut c = app.cursors.lock().await;
        let p = c.entry((query, kind)).or_insert(1);
        *p = if *p >= 10 { 1 } else { *p + 1 };
        drop(c);
        app.pool.lock().await.record_step(slot, false);
        return Ok(());
    }

    // Flag checks first (cheap), then download the survivors in parallel.
    let mut todo = Vec::new();
    {
        let mut p = app.pool.lock().await;
        for hit in fresh_hits {
            let sid = hit["id"].as_u64().unwrap_or(0);
            p.seen.insert(sid);
            if let Err(why) = pixabay_flags_ok(&hit) {
                tracing::debug!(sid, why, "skip");
                continue;
            }
            // The 640 px rendition is plenty for a reference and ~5x smaller.
            if let Some(url) = hit["webformatURL"]
                .as_str()
                .or(hit["largeImageURL"].as_str())
                .map(String::from)
            {
                todo.push((sid, hit, url));
            }
        }
    }
    let mut downloads = tokio::task::JoinSet::new();
    for (sid, hit, url) in todo {
        let http = app.pixabay.client();
        downloads.spawn(async move {
            let bytes = pixabay::fetch_bytes(&http, &url).await;
            (sid, hit, bytes)
        });
    }

    let mut added = 0;
    while let Some(joined) = downloads.join_next().await {
        let Ok((sid, hit, bytes)) = joined else {
            continue;
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                warn!(sid, "download: {e}");
                continue;
            }
        };
        let id = format!("px-{sid}");
        let images = app.images();
        let vetted = tokio::task::block_in_place(|| {
            let img = vet::on_white(image::load_from_memory(&bytes).map_err(|e| e.to_string())?);
            let v = app.vetter.vet(&img)?;
            // re-encode as JPEG ≤ 1280 px (consistent format, strips metadata)
            let img = if img.width().max(img.height()) > 1280 {
                img.resize(1280, 1280, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            img.to_rgb8()
                .save_with_format(images.join(format!("{id}.jpg")), image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((v, img.width(), img.height()))
        });
        let (v, w, h) = match vetted {
            Ok(x) => x,
            Err(why) => {
                tracing::debug!(sid, why, "rejected");
                let mut p = app.pool.lock().await;
                for r in why.split(',') {
                    *p.rejected.entry(r.to_string()).or_insert(0) += 1;
                }
                continue;
            }
        };
        let user = hit["user"].as_str().unwrap_or("").to_string();
        let r = LiveRef {
            id: id.clone(),
            source: "pixabay".into(),
            source_id: sid,
            category: slot.category.clone(),
            topic: slot.topic.clone(),
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
            width: w,
            height: h,
            title: String::new(),
            keypoints: vec![],
            difficulty: (v.difficulty.score * 10.0).round() / 10.0,
            tier: v.difficulty.tier,
            image_url: format!("/refimg/{id}.jpg"),
            live: true,
        };
        if app.pool.lock().await.add(r, v.phash) {
            added += 1;
        } else {
            tokio::fs::remove_file(images.join(format!("{id}.jpg")))
                .await
                .ok();
        }
    }
    let gained = app.pool.lock().await.fresh_len(slot) > before;
    app.pool.lock().await.record_step(slot, gained);
    info!(slot = %slot.key(), query, page, added, gained, "refill step");
    app.save().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app() -> Arc<App> {
        let dir = std::env::temp_dir().join(format!("pp-ref-service-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("images")).unwrap();
        Arc::new(App {
            pool: Mutex::new(PoolState::default()),
            wake: Notify::new(),
            pixabay: Pixabay::new("test".into(), dir.join("cache")),
            vetter: Box::new(FiltersVetter(ref_filters::Filters::without_nsfw())),
            topics: load_topics(&repo_root().join("content/topics.json")),
            data: dir,
            target: 3,
            cursors: Mutex::new(HashMap::new()),
        })
    }

    async fn call(app: Arc<App>, req: Request<Body>) -> (StatusCode, String) {
        let res = routes(app).oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn empty_pool_is_204_and_records_demand() {
        let app = test_app();
        let (s, _) = call(
            app.clone(),
            Request::get("/api/ref?category=animals&topic=pets&tier=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        assert_eq!(
            app.pool.lock().await.neediest(3).map(|s| s.key()),
            Some("animals/pets/1".to_string())
        );
    }

    #[tokio::test]
    async fn bad_input_is_400_and_bad_image_names_404() {
        let app = test_app();
        let (s, _) = call(
            app.clone(),
            Request::get("/api/ref?category=nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(
            app.clone(),
            Request::get("/api/ref?category=animals&topic=dragons")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(
            app,
            Request::get("/refimg/..%2Fpool.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn every_topic_has_search_queries() {
        let app = test_app();
        for c in CATEGORIES {
            let topics = app.topics.get(*c).expect("category in topics.json");
            assert!(!topics.is_empty());
            for t in topics {
                assert!(!t.queries.is_empty(), "{c}/{} has no queries", t.id);
            }
        }
    }
}
