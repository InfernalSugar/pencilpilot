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
//! Pictures come from several sources (see `sources`): Pixabay, Openverse
//! (CC0 / PDM / CC BY from museums and Wikimedia), and the public-domain
//! collections of the Art Institute of Chicago and The Met.
//!
//! Config (env, or the repo's git-ignored `.env`): `PIXABAY_API_KEY`
//! (without it Pixabay is skipped); optional `OPENVERSE_TOKEN` (OAuth access
//! token, raises Openverse's daily limit), `REF_SOURCES` (comma-separated
//! source names to use, default all), `REF_DATA_DIR` (default `data/ref-service`), `REF_PORT` (8787),
//! `REF_TOPICS` (content/topics.json), `REF_TARGET` (fresh pictures kept
//! ready per requested slot, default 6), `REF_NSFW_MODEL`, `REF_CLIP_DIR`
//! (the picture sorter, default crates/ref-filters/models/clip-vit-b32).
//! Both models come from `crates/ref-filters/fetch-model.sh`.
//!
//! `ref-service recheck [--dry-run]` runs the topic check over the existing
//! pool (stop the server first) and prints counts per category/topic.

mod budget;
mod model;
mod pool;
mod recheck;
mod sorting;
mod sources;
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

use model::{CATEGORIES, LiveRef, PHOTO_ONLY, Slot, Topic, image_type, kind_allowed};
use pool::PoolState;
use ref_filters::sort::Decision;
use sorting::{ClipTopicCheck, NoTopicCheck, TopicCheck, destination};
use sources::{Candidate, SourceId, Sources};
use vet::{FiltersVetter, Vetter};

/// How many search hits to examine per refill step before yielding.
const HITS_PER_STEP: usize = 12;

struct App {
    pool: Mutex<PoolState>,
    wake: Notify,
    sources: Sources,
    vetter: Box<dyn Vetter>,
    /// picture sorter: is it really this category/topic? (ADR-0009)
    sorter: Box<dyn TopicCheck>,
    topics: HashMap<String, Vec<Topic>>,
    data: PathBuf,
    target: usize,
    /// (source, query, mode) → next search page to try
    cursors: Mutex<HashMap<(SourceId, String, String), u32>>,
    /// slot key → refill steps so far (picks the source and the query)
    turns: Mutex<HashMap<String, usize>>,
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

fn read_topics_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("topics.json"))
        .expect("topics.json is JSON")
}

fn load_topics(path: &Path) -> HashMap<String, Vec<Topic>> {
    let v = read_topics_json(path);
    CATEGORIES
        .iter()
        .filter_map(|c| {
            let list: Vec<Topic> = serde_json::from_value(v[*c].clone()).ok()?;
            Some((c.to_string(), list))
        })
        .collect()
}

fn data_dir() -> PathBuf {
    env_or_dotenv("REF_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("data/ref-service"))
}

fn topics_path() -> PathBuf {
    env_or_dotenv("REF_TOPICS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("content/topics.json"))
}

fn load_pool(data: &Path) -> PoolState {
    std::fs::read_to_string(data.join("pool.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The picture sorter (ADR-0009). Refuse to run without it unless explicitly allowed.
fn load_sorter(topics: &Path) -> Box<dyn TopicCheck> {
    let dir = env_or_dotenv("REF_CLIP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("crates/ref-filters/models/clip-vit-b32"));
    match ClipTopicCheck::load(&dir, &read_topics_json(topics)) {
        Ok(s) => {
            info!(model = %dir.display(), "picture sorter loaded");
            Box::new(s)
        }
        Err(e) if env_or_dotenv("REF_ALLOW_NO_SORTER").as_deref() == Some("1") => {
            warn!("picture sorter NOT loaded ({e}); REF_ALLOW_NO_SORTER=1, dev only!");
            Box::new(NoTopicCheck)
        }
        Err(e) => panic!(
            "picture sorter not available ({e}). Run crates/ref-filters/fetch-model.sh \
             (or set REF_ALLOW_NO_SORTER=1 for local experiments)."
        ),
    }
}

/// `ref-service recheck [--dry-run]`
fn recheck_main(args: &[String]) {
    let apply = !args.iter().any(|a| a == "--dry-run");
    let data = data_dir();
    let topics = topics_path();
    let mut pool = load_pool(&data);
    let purged = purge_disallowed(&mut pool, &data.join("images"), apply);
    let sorter = load_sorter(&topics);
    let mut last = std::time::Instant::now();
    let rep = recheck::recheck(
        &mut pool,
        &data.join("images"),
        &*sorter,
        apply,
        false,
        |i, n| {
            if last.elapsed() > Duration::from_secs(10) {
                eprintln!("  {i}/{n}");
                last = std::time::Instant::now();
            }
        },
    );
    for (id, from, change, tags) in &rep.changes {
        println!("{id:<12} {from:<24} {change:?}  [{tags}]");
    }
    println!();
    if !purged.is_empty() {
        println!("removed first (wrong kind for the category, banned or franchise words):");
        for (r, why) in &purged {
            println!(
                "  {:<12} {:<24} {why:<28} [{}]",
                r.id,
                format!("{}/{}", r.category, r.topic),
                r.tags
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!("{} removed first\n", purged.len());
    }
    rep.print();
    if apply {
        let text = serde_json::to_string(&pool).expect("pool serialises");
        let tmp = data.join("pool.json.tmp");
        std::fs::write(&tmp, text).expect("write pool");
        std::fs::rename(&tmp, data.join("pool.json")).expect("save pool");
        println!("saved {}", data.join("pool.json").display());
    } else {
        println!("dry run: nothing changed");
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("recheck") {
        return tokio::task::block_in_place(|| recheck_main(&args[1..]));
    }

    let data = data_dir();
    std::fs::create_dir_all(data.join("images")).expect("data dir");
    let topics_path = topics_path();
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

    let sorter = load_sorter(&topics_path);

    let mut pool = load_pool(&data);
    info!(refs = pool.refs.len(), "loaded pool");
    let purged = purge_disallowed(&mut pool, &data.join("images"), true);
    if !purged.is_empty() {
        let mut why: std::collections::BTreeMap<String, usize> = Default::default();
        for (_, w) in &purged {
            *why.entry(w.split(':').next().unwrap_or(w).to_string())
                .or_default() += 1;
        }
        warn!(removed = purged.len(), reasons = ?why, "removed pictures the rules no longer allow");
    }
    let purged = purged.len();
    // Pictures the sorter hasn't seen yet (e.g. from before ADR-0009): check
    // them once now. Takes ~0.1 s per picture; later starts skip them. After
    // a change to the sorting rules (prompts, cut-offs in topics.json) the
    // whole pool is checked again once.
    let rep = recheck::recheck(
        &mut pool,
        &data.join("images"),
        &*sorter,
        true,
        true,
        |i, n| {
            if i % 100 == 0 && i > 0 {
                info!("sorting existing pictures: {i}/{n}");
            }
        },
    );
    let changed = rep.changes.len();
    if !rep.per_topic.is_empty() || purged > 0 || rep.rules_changed {
        let removed = rep
            .changes
            .iter()
            .filter(|c| matches!(c.2, recheck::Change::Removed { .. }))
            .count();
        let rules_changed = rep.rules_changed;
        warn!(
            changed,
            removed, rules_changed, "sorted existing pictures (ADR-0009)"
        );
        let text = serde_json::to_string(&pool).expect("pool serialises");
        std::fs::write(data.join("pool.json.tmp"), text).expect("write pool");
        std::fs::rename(data.join("pool.json.tmp"), data.join("pool.json")).expect("save pool");
    }

    let sources = load_sources(&data);
    let app = Arc::new(App {
        pool: Mutex::new(pool),
        wake: Notify::new(),
        sources,
        vetter: Box::new(FiltersVetter(filters)),
        sorter,
        topics: load_topics(&topics_path),
        data,
        target,
        cursors: Mutex::new(HashMap::new()),
        turns: Mutex::new(HashMap::new()),
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

/// The picture sources to use: all of them unless `REF_SOURCES` names some.
/// Pixabay needs `PIXABAY_API_KEY`; without it, it is skipped.
fn load_sources(data: &Path) -> Sources {
    let wanted = env_or_dotenv("REF_SOURCES");
    let on = |s: SourceId| {
        wanted
            .as_deref()
            .is_none_or(|w| w.split(',').any(|x| x.trim() == s.name()))
    };
    let cache = data.join("search-cache");
    let budget = |s: SourceId| Some(data.join(format!("budget-{}.json", s.name())));
    let pixabay = match env_or_dotenv("PIXABAY_API_KEY") {
        Some(key) if on(SourceId::Pixabay) => Some(sources::pixabay::Pixabay::new(
            key,
            cache.clone(),
            budget(SourceId::Pixabay),
        )),
        None if on(SourceId::Pixabay) => {
            warn!("PIXABAY_API_KEY not set: Pixabay is skipped");
            None
        }
        _ => None,
    };
    let openverse = on(SourceId::Openverse).then(|| {
        let token = env_or_dotenv("OPENVERSE_TOKEN").filter(|t| !t.is_empty());
        info!(
            signed_in = token.is_some(),
            "Openverse on (CC0 / PDM / CC BY, allowed providers only)"
        );
        sources::openverse::Openverse::new(token, cache.clone(), budget(SourceId::Openverse))
    });
    let artic = on(SourceId::Artic)
        .then(|| sources::artic::Artic::new(cache.clone(), budget(SourceId::Artic)));
    let met =
        on(SourceId::Met).then(|| sources::met::Met::new(cache.clone(), budget(SourceId::Met)));
    let s = Sources::new(pixabay, openverse, artic, met);
    let names: Vec<&str> = SourceId::ALL
        .iter()
        .filter(|x| s.enabled(**x))
        .map(|x| x.name())
        .collect();
    info!(sources = ?names, "picture sources");
    s
}

/// Why a picture already in the pool isn't allowed any more, if so: a kind
/// its category doesn't take (drawings in real-life/animals/environment), or
/// a banned or franchise word in its title or tags (ADR-0003).
fn disallowed(r: &LiveRef) -> Option<String> {
    if !kind_allowed(&r.category, r.kind()) {
        return Some(format!("wrong_kind:{}", r.kind()));
    }
    ref_filters::words::banned(&format!("{} {}", r.title, r.tags.join(", ")))
}

/// Drop the references [`disallowed`] rules out (with `apply`, also delete
/// their files; their ids stay in `seen`, so they are never fetched again).
/// Returns them with the reason.
fn purge_disallowed(pool: &mut PoolState, images: &Path, apply: bool) -> Vec<(LiveRef, String)> {
    let mut why = HashMap::new();
    for r in pool.refs.values() {
        if let Some(w) = disallowed(r) {
            why.insert(r.id.clone(), w);
        }
    }
    if why.is_empty() {
        return Vec::new();
    }
    let gone = if apply {
        pool.remove_where(|r| why.contains_key(&r.id))
    } else {
        let mut v: Vec<LiveRef> = pool
            .refs
            .values()
            .filter(|r| why.contains_key(&r.id))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    };
    gone.into_iter()
        .map(|r| {
            if apply {
                std::fs::remove_file(images.join(format!("{}.jpg", r.id))).ok();
            }
            let w = why.remove(&r.id).unwrap_or_default();
            (r, w)
        })
        .collect()
}

fn routes(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/ref", get(get_ref))
        .route("/api/report", post(report))
        .route("/api/pool", get(pool_status))
        .route("/refimg/{file}", get(image))
        // Not proxied to browsers: the web server calls it before accepting a shared drawing.
        .route(
            "/api/moderate",
            post(moderate).layer(axum::extract::DefaultBodyLimit::max(MODERATE_MAX_BYTES)),
        )
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

/// Largest drawing `/api/moderate` accepts (an 800×600 PNG is well under this).
const MODERATE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// NSFW guard for a drawing someone wants to share. Always 200 with
/// `{"safe": bool, "reason"?: string}`; anything that can't be checked is unsafe.
async fn moderate(State(app): State<Arc<App>>, body: axum::body::Bytes) -> Response {
    let verdict = tokio::task::spawn_blocking(move || {
        let img = ref_filters::decode(&body).map_err(|_| "undecodable".to_string())?;
        app.vetter.safe_to_share(&img)
    })
    .await
    .unwrap_or_else(|_| Err("internal".into()));
    match verdict {
        Ok(()) => Json(json!({"safe": true})).into_response(),
        Err(reason) => {
            info!(%reason, "shared drawing refused");
            Json(json!({"safe": false, "reason": reason})).into_response()
        }
    }
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
                    // counts as a fruitless step, so a slot whose sources keep
                    // failing is paused instead of holding up the others
                    app.pool.lock().await.record_step(&slot, false);
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

/// The sources that can fill `slot` right now: enabled, allowed in the
/// category, with queries for the topic, and not paused or out of budget.
async fn ready_sources(app: &App, slot: &Slot, topic: &Topic) -> Vec<SourceId> {
    let mut out = Vec::new();
    for s in SourceId::ALL {
        if app.sources.enabled(s)
            && s.allows(&slot.category)
            && !topic.queries_for(s.name()).is_empty()
            && app.sources.ready(s).await
        {
            out.push(s);
        }
    }
    out
}

/// Count a rejected picture under its reason code ("off_topic:anime/scenery"
/// counts as "off_topic").
async fn count_reject(app: &App, why: &str) {
    let mut p = app.pool.lock().await;
    for r in why.split(',') {
        let code = r.split([':', ' ']).next().unwrap_or(r);
        *p.rejected.entry(code.to_string()).or_insert(0) += 1;
    }
}

/// Look at up to HITS_PER_STEP new search hits for `slot` from one source,
/// vet them and add the good ones to the pool (under the tier they actually
/// measure as). Sources take turns per slot, so pictures come from all of
/// them and no single API's limits are hit; a source that is down, paused or
/// out of budget is skipped.
async fn refill_step(app: &App, slot: &Slot) -> Result<(), String> {
    let topic = app
        .topics
        .get(&slot.category)
        .and_then(|l| l.iter().find(|t| t.id == slot.topic))
        .cloned()
        .ok_or("unknown topic")?;
    let ready = ready_sources(app, slot, &topic).await;
    if ready.is_empty() {
        app.pool.lock().await.record_step(slot, false);
        info!(slot = %slot.key(), "refill: no source ready");
        return Ok(());
    }
    let turn = {
        let mut t = app.turns.lock().await;
        let n = t.entry(slot.key()).or_insert(0);
        *n += 1;
        *n - 1
    };
    let source = ready[turn % ready.len()];
    let queries = topic.queries_for(source.name());
    let query = queries[(turn / ready.len()) % queries.len()].clone();
    let mode = match source {
        SourceId::Pixabay => image_type(&slot.category, slot.tier).to_string(),
        SourceId::Openverse if PHOTO_ONLY.contains(&slot.category.as_str()) => "photo".into(),
        SourceId::Openverse => "art".into(),
        _ => String::new(),
    };
    let before = app.pool.lock().await.fresh_len(slot);
    let cursor = (source, query.clone(), mode);
    let page = *app.cursors.lock().await.entry(cursor.clone()).or_insert(1);
    let found = app
        .sources
        .search(source, &query, &slot.category, slot.tier, page)
        .await
        .map_err(|e| format!("{}: {e}", source.name()))?;
    let fresh_hits: Vec<_> = {
        let p = app.pool.lock().await;
        found
            .hits
            .into_iter()
            .filter(|h| !p.is_seen(&h.key))
            .take(HITS_PER_STEP)
            .collect()
    };
    if fresh_hits.is_empty() {
        // page used up: next time try the next page (back to 1 after the last)
        let mut c = app.cursors.lock().await;
        let p = c.entry(cursor).or_insert(1);
        *p = if found.last || *p >= 10 { 1 } else { *p + 1 };
        drop(c);
        app.pool.lock().await.record_step(slot, false);
        return Ok(());
    }

    // Source rules (licence, provider, flags, public domain) and banned words
    // first (cheap), then download the survivors in parallel.
    let mut todo: Vec<Candidate> = Vec::new();
    for hit in fresh_hits {
        let cand = match app.sources.resolve(source, &hit, &slot.category).await {
            Ok(c) => c,
            Err(e) => {
                // couldn't check it now (network, budget): look again later
                warn!(source = source.name(), key = %hit.key, "resolve: {e}");
                break;
            }
        };
        app.pool.lock().await.mark_seen(&hit.key);
        let cand = match cand {
            Ok(c) => c,
            Err(why) => {
                tracing::debug!(key = %hit.key, why, "skip");
                count_reject(app, &why).await;
                continue;
            }
        };
        let text = format!("{} {}", cand.title, cand.tags.join(", "));
        if let Some(why) = ref_filters::words::banned(&text) {
            tracing::debug!(key = %hit.key, why, "skip");
            count_reject(app, &why).await;
            continue;
        }
        if !kind_allowed(&slot.category, &cand.kind) {
            tracing::debug!(key = %hit.key, kind = cand.kind, "skip: wrong kind");
            count_reject(app, "wrong_kind").await;
            continue;
        }
        todo.push(cand);
    }
    let mut downloads = tokio::task::JoinSet::new();
    for c in todo {
        let http = app.sources.client();
        downloads.spawn(async move {
            let bytes = sources::fetch_bytes(&http, &c.image_url).await;
            (c, bytes)
        });
    }

    let mut added = 0;
    while let Some(joined) = downloads.join_next().await {
        let Ok((c, bytes)) = joined else {
            continue;
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                warn!(id = %c.id, "download: {e}");
                continue;
            }
        };
        let id = c.id.clone();
        let images = app.images();
        let vetted = tokio::task::block_in_place(|| {
            let img = vet::on_white(image::load_from_memory(&bytes).map_err(|e| e.to_string())?);
            let v = app.vetter.vet(&img)?;
            // Is it really this category/topic? Keep, re-file or reject (ADR-0009).
            let decision = app
                .sorter
                .check(&img, &slot.category, &slot.topic)
                .map_err(|e| format!("sorter_error ({e})"))?;
            let checked = decision != Decision::Unchecked;
            let dest = destination(decision, &slot.category, &slot.topic)?;
            // A source only fills the topics that list queries for it (e.g. no
            // Openverse photos of people: no model releases), re-filed or not.
            let for_source = app
                .topics
                .get(&dest.0)
                .and_then(|l| l.iter().find(|t| t.id == dest.1))
                .is_some_and(|t| !t.queries_for(c.source.name()).is_empty());
            if !for_source {
                return Err(format!("not_for_source:{}/{}", dest.0, dest.1));
            }
            if !kind_allowed(&dest.0, &c.kind) {
                return Err(format!("wrong_kind_for:{}/{}", dest.0, dest.1));
            }
            // re-encode as JPEG ≤ 1280 px (consistent format, strips metadata)
            let img = if img.width().max(img.height()) > 1280 {
                img.resize(1280, 1280, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            img.to_rgb8()
                .save_with_format(images.join(format!("{id}.jpg")), image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((v, dest, checked, img.width(), img.height()))
        });
        let (v, (category, topic), checked, w, h) = match vetted {
            Ok(x) => x,
            Err(why) => {
                tracing::debug!(id, why, "rejected");
                count_reject(app, &why).await;
                continue;
            }
        };
        if (category.as_str(), topic.as_str()) != (slot.category.as_str(), slot.topic.as_str()) {
            info!(id, from = %slot.key(), to = %format!("{category}/{topic}"), "re-filed");
        }
        let r = LiveRef {
            id: id.clone(),
            source: c.source.name().into(),
            source_id: c.source_id,
            category,
            topic,
            page_url: c.page_url,
            author: c.author,
            author_url: c.author_url,
            license: c.license,
            license_url: c.license_url,
            tags: c.tags,
            width: w,
            height: h,
            title: c.title,
            attribution: c.attribution,
            provider: c.provider,
            kind: c.kind,
            keypoints: vec![],
            difficulty: (v.difficulty.score * 10.0).round() / 10.0,
            tier: v.difficulty.tier,
            image_url: format!("/refimg/{id}.jpg"),
            live: true,
        };
        let mut p = app.pool.lock().await;
        let ok = p.add(r, v.phash);
        if ok && checked {
            p.sorted.insert(id.clone());
        }
        if !ok {
            *p.rejected.entry("duplicate".into()).or_insert(0) += 1;
        }
        drop(p);
        if ok {
            added += 1;
        } else {
            tokio::fs::remove_file(images.join(format!("{id}.jpg")))
                .await
                .ok();
        }
    }
    let gained = app.pool.lock().await.fresh_len(slot) > before;
    app.pool.lock().await.record_step(slot, gained);
    info!(slot = %slot.key(), source = source.name(), query, page, added, gained, "refill step");
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
            sources: Sources::new(
                Some(sources::pixabay::Pixabay::new(
                    "test".into(),
                    dir.join("cache"),
                    None,
                )),
                None,
                None,
                None,
            ),
            vetter: Box::new(FiltersVetter(ref_filters::Filters::without_nsfw())),
            sorter: Box::new(NoTopicCheck),
            topics: load_topics(&repo_root().join("content/topics.json")),
            data: dir,
            target: 3,
            cursors: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
        })
    }

    /// A drawable test picture: a dark disc with a lighter ring.
    fn disc_png(shift: u32) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(800, 600, |x, y| {
            let d = ((x as f32 - 300.0 - shift as f32).powi(2) + (y as f32 - 300.0).powi(2)).sqrt();
            let g = if d < 150.0 {
                40
            } else if d < 200.0 {
                160
            } else {
                255
            };
            image::Rgb([g, g, g])
        }));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refill_adds_vetted_pictures_with_their_credit_and_drops_duplicates() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hit = |id: u64, ai: bool| {
            json!({"id": id, "pageURL": format!("https://pixabay.com/photos/cat-{id}/"),
                "webformatURL": format!("{base}/img.png"), "user": "Ann", "user_id": 5,
                "tags": "cat, pet", "isAiGenerated": ai, "isGRated": true, "isLowQuality": false})
        };
        let body = json!({"hits": [hit(1, false), hit(2, false), hit(3, true)]}).to_string();
        let png = disc_png(0);
        let router = Router::new()
            .route(
                "/api/",
                get(move || {
                    let b = body.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], b) }
                }),
            )
            .route(
                "/img.png",
                get(move || {
                    let b = png.clone();
                    async move { ([(header::CONTENT_TYPE, "image/png")], b) }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });

        let dir = std::env::temp_dir().join(format!("pp-refill-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("images")).unwrap();
        let app = App {
            pool: Mutex::new(PoolState::default()),
            wake: Notify::new(),
            sources: Sources::new(
                Some(
                    sources::pixabay::Pixabay::new("k".into(), dir.join("cache"), None)
                        .with_base(&format!("{base}/api/")),
                ),
                None,
                None,
                None,
            ),
            vetter: Box::new(FiltersVetter(ref_filters::Filters::without_nsfw())),
            sorter: Box::new(NoTopicCheck),
            topics: load_topics(&repo_root().join("content/topics.json")),
            data: dir,
            target: 3,
            cursors: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
        };
        let slot = Slot {
            category: "animals".into(),
            topic: "pets".into(),
            tier: 2,
        };
        refill_step(&app, &slot).await.unwrap();
        let p = app.pool.lock().await;
        assert_eq!(p.refs.len(), 1, "same picture twice: one kept");
        let r = p.refs.values().next().unwrap();
        assert_eq!(r.source, "pixabay");
        assert_eq!(r.provider, "Pixabay");
        assert_eq!(r.kind, "photo");
        assert_eq!(r.author, "Ann");
        assert_eq!(p.rejected.get("duplicate"), Some(&1));
        assert_eq!(p.rejected.get("ai_generated"), Some(&1));
        assert!(p.is_seen("pixabay:3"));
        assert!(app.images().join(format!("{}.jpg", r.id)).exists());
    }

    #[test]
    fn franchise_and_wrong_kind_pictures_are_purged() {
        let mk = |id: &str, cat: &str, url: &str, tags: &[&str]| LiveRef {
            id: id.into(),
            source: "pixabay".into(),
            source_id: 1,
            category: cat.into(),
            topic: "x".into(),
            page_url: url.into(),
            author: "a".into(),
            author_url: String::new(),
            license: "l".into(),
            license_url: String::new(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            width: 1,
            height: 1,
            title: String::new(),
            attribution: String::new(),
            provider: String::new(),
            kind: String::new(),
            keypoints: vec![],
            difficulty: 1.0,
            tier: 1,
            image_url: String::new(),
            live: true,
        };
        let mut p = PoolState::default();
        p.add(
            mk(
                "px-1",
                "anime",
                "https://pixabay.com/illustrations/a-1/",
                &["naruto", "ninja"],
            ),
            None,
        );
        p.add(
            mk(
                "px-2",
                "anime",
                "https://pixabay.com/illustrations/a-2/",
                &["girl", "anime"],
            ),
            None,
        );
        p.add(
            mk(
                "px-3",
                "animals",
                "https://pixabay.com/vectors/a-3/",
                &["cat"],
            ),
            None,
        );
        p.add(
            mk(
                "px-4",
                "anime",
                "https://pixabay.com/illustrations/a-4/",
                &["fan art", "girl"],
            ),
            None,
        );
        let dir = std::env::temp_dir();
        let dry = purge_disallowed(&mut p, &dir, false);
        assert_eq!(dry.len(), 3);
        assert_eq!(p.refs.len(), 4, "dry run keeps everything");
        let gone = purge_disallowed(&mut p, &dir, true);
        let why: Vec<(&str, &str)> = gone
            .iter()
            .map(|(r, w)| (r.id.as_str(), w.as_str()))
            .collect();
        assert_eq!(
            why,
            [
                ("px-1", "franchise:naruto"),
                ("px-3", "wrong_kind:vector"),
                ("px-4", "franchise:fan art")
            ]
        );
        assert_eq!(p.refs.keys().collect::<Vec<_>>(), ["px-2"]);
    }

    #[tokio::test]
    async fn masters_has_no_source_without_museums() {
        // test_app has Pixabay only, which never fills Master studies
        let app = test_app();
        let slot = Slot {
            category: "masters".into(),
            topic: "portraits".into(),
            tier: 2,
        };
        refill_step(&app, &slot).await.unwrap();
        assert!(app.pool.lock().await.refs.is_empty());
        assert_eq!(app.pool.lock().await.misses[&slot.key()].0, 1);
    }

    async fn call(app: Arc<App>, req: Request<Body>) -> (StatusCode, String) {
        let res = routes(app).oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn moderate_fails_closed_without_a_model() {
        let mut png = Vec::new();
        image::DynamicImage::new_luma8(80, 60)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let req = Request::post("/api/moderate")
            .body(Body::from(png))
            .unwrap();
        let (status, body) = call(test_app(), req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#""safe":false"#), "{body}");
        assert!(body.contains("nsfw_unchecked"), "{body}");

        let req = Request::post("/api/moderate")
            .body(Body::from("not a picture"))
            .unwrap();
        let (_, body) = call(test_app(), req).await;
        assert!(body.contains("undecodable"), "{body}");
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
                let usable = SourceId::ALL
                    .iter()
                    .any(|s| s.allows(c) && !t.queries_for(s.name()).is_empty());
                assert!(usable, "{c}/{} has no queries for any source", t.id);
                for (name, qs) in &t.source_queries {
                    let s = SourceId::ALL.iter().find(|s| s.name() == name);
                    let s = s.unwrap_or_else(|| panic!("{c}/{}: unknown source {name}", t.id));
                    assert!(s.allows(c), "{c}/{}: {name} can't fill {c}", t.id);
                    assert!(!qs.is_empty(), "{c}/{}: empty {name} queries", t.id);
                }
            }
        }
        // Master studies comes from the museums only
        for t in &app.topics["masters"] {
            assert!(t.queries.is_empty(), "masters/{}: no Pixabay", t.id);
            assert!(!t.queries_for("artic").is_empty() && !t.queries_for("met").is_empty());
        }
    }
}
