//! Owner-only picture review API, behind `/dev/pictures` in the web app.
//!
//! ```text
//! GET  /api/admin/pictures?category=&topic=&source=&tier=&status=unreviewed|reviewed|all
//!                         &sort=newest|reported|random&seed=&page=&per=
//! GET  /api/admin/stats   → per category/topic: pictures, reviewed, kept / moved / rejected
//! POST /api/admin/review {id, decision: keep|reject|move|tier, category?, topic?, tier?}
//! POST /api/admin/undo   {seq?}  → takes back this session's last decision
//! GET  /api/admin/img/<id>.jpg   → the picture, also when blocked or rejected
//! ```
//!
//! Off unless the service runs with `REF_ADMIN=1` (`pnpm dev` sets it; the
//! production service never does), and then only for requests from this
//! computer: loopback peer, no non-loopback forwarding headers (the Vite dev
//! proxy forwards the browser's address), and no cross-site browser requests.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path as UrlPath, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use ref_filters::sort::Decision;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::App;
use crate::model::{CATEGORIES, LiveRef, kind_allowed};
use crate::reviews::{Effective, Outcome, Place, Record, UndoAction, UndoEntry, now};

/// Rejected pictures' files: out of `images/` (never served to students),
/// kept for undo and for measuring the sorter (`recheck --labels`).
pub const LABELS_DIR: &str = "review-labels";

pub fn router(app: Arc<App>) -> Router<Arc<App>> {
    Router::new()
        .route("/api/admin/pictures", get(list))
        .route("/api/admin/stats", get(stats))
        .route("/api/admin/review", post(review))
        .route("/api/admin/undo", post(undo))
        .route("/api/admin/img/{file}", get(image))
        .route_layer(middleware::from_fn_with_state(app, guard))
}

async fn guard(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    if !app.admin {
        return StatusCode::NOT_FOUND.into_response();
    }
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if let Err(why) = local_only(peer, req.headers()) {
        info!(?peer, why, "admin request refused");
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    next.run(req).await
}

fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback(),
        IpAddr::V6(v) => v.is_loopback() || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback()),
    }
}

/// Is this request from a browser or tool on this computer?
pub fn local_only(peer: Option<IpAddr>, h: &HeaderMap) -> Result<(), &'static str> {
    if !peer.is_some_and(is_local) {
        return Err("only from this computer");
    }
    for name in ["x-forwarded-for", "x-real-ip"] {
        for v in h.get_all(name) {
            let ok = v.to_str().is_ok_and(|s| {
                s.split(',').all(|a| {
                    a.trim()
                        .trim_matches(['[', ']'])
                        .parse::<IpAddr>()
                        .is_ok_and(is_local)
                })
            });
            if !ok {
                return Err("only from this computer (forwarded)");
            }
        }
    }
    if h.contains_key("forwarded") {
        return Err("only from this computer (forwarded)");
    }
    if h.get("sec-fetch-site")
        .is_some_and(|v| v.as_bytes() == b"cross-site")
    {
        return Err("no cross-site requests");
    }
    if let Some(origin) = h.get(header::ORIGIN) {
        let host = origin
            .to_str()
            .unwrap_or("")
            .split("://")
            .nth(1)
            .unwrap_or("")
            .to_string();
        let name = if host.starts_with('[') {
            host.split(']').next().unwrap_or("").to_string() + "]"
        } else {
            host.split(':').next().unwrap_or("").to_string()
        };
        if !["localhost", "127.0.0.1", "[::1]"].contains(&name.as_str()) {
            return Err("no cross-site requests");
        }
    }
    Ok(())
}

fn some(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.is_empty() && *s != "all")
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    category: Option<String>,
    topic: Option<String>,
    source: Option<String>,
    tier: Option<String>,
    status: Option<String>,
    sort: Option<String>,
    seed: Option<u64>,
    page: Option<usize>,
    per: Option<usize>,
}

/// id → report reasons, from `reports.jsonl` (the students' Report button).
fn reports(data: &Path) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for line in std::fs::read_to_string(data.join("reports.jsonl"))
        .unwrap_or_default()
        .lines()
    {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(id) = v["id"].as_str() {
            out.entry(id.to_string())
                .or_default()
                .push(v["reason"].as_str().unwrap_or("").to_string());
        }
    }
    out
}

/// Categories a picture of this kind may be moved to.
fn move_to(kind: &str) -> Vec<&'static str> {
    CATEGORIES
        .iter()
        .copied()
        .filter(|c| may_move(c, kind))
        .collect()
}

/// The style rules for the owner's moves: photos only in real-life, animals
/// and environment (and fundamentals), artworks only in Master studies.
pub fn may_move(category: &str, kind: &str) -> bool {
    kind_allowed(category, kind) && (kind != "artwork" || category == "masters")
}

fn review_json(e: Option<&Effective>) -> Value {
    match e {
        None => Value::Null,
        Some(e) => json!({
            "decision": e.last_decision,
            "outcome": e.outcome.label(),
            "from": e.from.key(),
            "seq": e.last_seq,
            "at": e.last_at,
            "sorter": e.sorter,
        }),
    }
}

fn item(
    r: &LiveRef,
    sorted: bool,
    blocked: bool,
    added: u64,
    reports: Option<&Vec<String>>,
    review: Option<&Effective>,
) -> Value {
    let reasons: Vec<&str> = reports
        .into_iter()
        .flatten()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    json!({
        "id": r.id,
        "imageUrl": format!("/api/admin/img/{}.jpg", r.id),
        "category": r.category,
        "topic": r.topic,
        "tier": r.tier,
        "source": r.source,
        "provider": if r.provider.is_empty() { "Pixabay" } else { r.provider.as_str() },
        "kind": r.kind(),
        "title": r.title,
        "tags": r.tags,
        "width": r.width,
        "height": r.height,
        "difficulty": r.difficulty,
        "credit": {
            "author": r.author,
            "authorUrl": r.author_url,
            "license": r.license,
            "licenseUrl": r.license_url,
            "pageUrl": r.page_url,
            "attribution": r.attribution,
        },
        "sorted": sorted,
        "blocked": blocked,
        "addedAt": added,
        "reports": {"count": reports.map_or(0, Vec::len), "reasons": reasons},
        "review": review_json(review),
        "moveTo": move_to(r.kind()),
    })
}

fn added_at(images: &Path, id: &str) -> u64 {
    std::fs::metadata(images.join(format!("{id}.jpg")))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

async fn list(State(app): State<Arc<App>>, Query(q): Query<ListQuery>) -> Response {
    let eff = app.reviews.lock().await.effective();
    let tier: Option<u8> = some(&q.tier).and_then(|t| t.parse().ok());
    let status = some(&q.status).unwrap_or("all");
    if !["all", "reviewed", "unreviewed"].contains(&status) {
        return (StatusCode::BAD_REQUEST, "bad status").into_response();
    }
    let picked: Vec<(LiveRef, bool, bool)> = {
        let p = app.pool.lock().await;
        p.refs
            .values()
            .filter(|r| some(&q.category).is_none_or(|c| r.category == c))
            .filter(|r| some(&q.topic).is_none_or(|t| r.topic == t))
            .filter(|r| some(&q.source).is_none_or(|s| r.source == s))
            .filter(|r| tier.is_none_or(|t| r.tier == t))
            .filter(|r| match status {
                "reviewed" => eff.contains_key(&r.id),
                "unreviewed" => !eff.contains_key(&r.id),
                _ => true,
            })
            .map(|r| {
                (
                    r.clone(),
                    p.sorted.contains(&r.id),
                    p.blocked.contains(&r.id),
                )
            })
            .collect()
    };
    let reports = reports(&app.data);
    let images = app.images();
    let mut rows: Vec<(LiveRef, bool, bool, u64, usize)> = picked
        .into_iter()
        .map(|(r, s, b)| {
            let added = added_at(&images, &r.id);
            let n = reports.get(&r.id).map_or(0, Vec::len);
            (r, s, b, added, n)
        })
        .collect();
    rows.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.id.cmp(&b.0.id)));
    match some(&q.sort).unwrap_or("newest") {
        "newest" => {}
        "reported" => rows.sort_by_key(|r| std::cmp::Reverse(r.4)), // stable: newest within
        "random" => {
            rows.sort_by(|a, b| a.0.id.cmp(&b.0.id));
            rows.shuffle(&mut rand::rngs::StdRng::seed_from_u64(q.seed.unwrap_or(0)));
        }
        _ => return (StatusCode::BAD_REQUEST, "bad sort").into_response(),
    }
    let per = q.per.unwrap_or(48).clamp(1, 200);
    let page = q.page.unwrap_or(1).max(1);
    let total = rows.len();
    let items: Vec<Value> = rows
        .iter()
        .skip((page - 1) * per)
        .take(per)
        .map(|(r, s, b, added, _)| item(r, *s, *b, *added, reports.get(&r.id), eff.get(&r.id)))
        .collect();
    Json(json!({"total": total, "page": page, "per": per, "items": items})).into_response()
}

async fn stats(State(app): State<Arc<App>>) -> Json<Value> {
    let eff = app.reviews.lock().await.effective();
    // "category/topic" → [pictures, reviewed, kept, moved, rejected]
    let mut per: BTreeMap<(usize, usize, String), [usize; 5]> = BTreeMap::new();
    let order = |key: &str| -> (usize, usize) {
        let (c, t) = key.split_once('/').unwrap_or((key, ""));
        let ci = CATEGORIES.iter().position(|x| *x == c).unwrap_or(99);
        let ti = app
            .topics
            .get(c)
            .and_then(|l| l.iter().position(|x| x.id == t))
            .unwrap_or(99);
        (ci, ti)
    };
    let mut bump = |key: String, i: Option<usize>| {
        let (a, b) = order(&key);
        let n = per.entry((a, b, key)).or_default();
        if let Some(i) = i {
            n[i] += 1;
        }
    };
    // every topic, also empty ones
    for c in CATEGORIES {
        for t in app.topics.get(*c).into_iter().flatten() {
            bump(format!("{c}/{}", t.id), None);
        }
    }
    {
        let p = app.pool.lock().await;
        for r in p.refs.values().filter(|r| !p.blocked.contains(&r.id)) {
            bump(format!("{}/{}", r.category, r.topic), Some(0));
        }
    }
    // reviews count under where the sorter had filed the picture
    for e in eff.values() {
        let k = e.from.key();
        bump(k.clone(), Some(1));
        bump(k, Some(2 + e.outcome.index()));
    }
    let mut tot = [0usize; 5];
    let topics: Vec<Value> = per
        .into_iter()
        .map(|((_, _, key), n)| {
            for (a, b) in tot.iter_mut().zip(n) {
                *a += b;
            }
            let (c, t) = key.split_once('/').unwrap_or((&key, ""));
            json!({"category": c, "topic": t, "pictures": n[0], "reviewed": n[1],
                   "kept": n[2], "moved": n[3], "rejected": n[4]})
        })
        .collect();
    Json(json!({
        "topics": topics,
        "total": {"pictures": tot[0], "reviewed": tot[1], "kept": tot[2], "moved": tot[3], "rejected": tot[4]},
    }))
}

#[derive(Deserialize)]
pub struct ReviewReq {
    id: String,
    decision: String,
    category: Option<String>,
    topic: Option<String>,
    tier: Option<u8>,
}

fn verdict(d: &Decision) -> Value {
    match d {
        Decision::Keep { p } => json!({"verdict": "keep", "p": p}),
        Decision::Refile {
            category,
            topic,
            p_from,
            p_to,
        } => {
            json!({"verdict": "move", "to": format!("{category}/{topic}"), "pFrom": p_from, "pTo": p_to})
        }
        Decision::Reject { reason } => json!({"verdict": "reject", "reason": reason}),
        Decision::Unchecked => json!({"verdict": "unchecked"}),
    }
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({"error": msg.into()}))).into_response()
}

fn place_of(r: &LiveRef) -> Place {
    Place {
        category: r.category.clone(),
        topic: r.topic.clone(),
        tier: r.tier,
    }
}

/// Move a file, across file systems if need be.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}

impl App {
    fn label_image(&self, id: &str) -> PathBuf {
        self.data.join(LABELS_DIR).join(format!("{id}.jpg"))
    }

    /// The listing entry for one picture, if it's in the pool.
    async fn picture(&self, id: &str, eff: &HashMap<String, Effective>) -> Value {
        let (r, s, b) = {
            let p = self.pool.lock().await;
            match p.refs.get(id) {
                Some(r) => (r.clone(), p.sorted.contains(id), p.blocked.contains(id)),
                None => return Value::Null,
            }
        };
        let reports = reports(&self.data);
        item(
            &r,
            s,
            b,
            added_at(&self.images(), id),
            reports.get(id),
            eff.get(id),
        )
    }
}

async fn review(State(app): State<Arc<App>>, Json(q): Json<ReviewReq>) -> Response {
    // one decision at a time; the pool lock is only held to read and to apply
    let mut log = app.reviews.lock().await;
    let eff = log.effective();
    let (r, blocked, sorted, gone) = {
        let p = app.pool.lock().await;
        (
            p.refs.get(&q.id).cloned(),
            p.blocked.contains(&q.id),
            p.sorted.contains(&q.id),
            p.blocked.contains(&q.id) && !p.refs.contains_key(&q.id),
        )
    };
    let Some(r) = r else {
        // rejected already (by the owner or a rules change after a report): nothing to do
        if q.decision == "reject"
            && (gone || eff.get(&q.id).is_some_and(|e| e.outcome == Outcome::Reject))
        {
            return Json(json!({"ok": true, "unchanged": true, "picture": null})).into_response();
        }
        return err(StatusCode::NOT_FOUND, "not in the pool");
    };
    let from = place_of(&r);
    let mut decision = q.decision.clone();
    let to = match decision.as_str() {
        "keep" | "reject" => None,
        "move" => {
            let (Some(c), Some(t)) = (q.category.as_deref(), q.topic.as_deref()) else {
                return err(StatusCode::BAD_REQUEST, "move needs category and topic");
            };
            if !CATEGORIES.contains(&c) {
                return err(StatusCode::BAD_REQUEST, "unknown category");
            }
            if !app
                .topics
                .get(c)
                .is_some_and(|l| l.iter().any(|x| x.id == t))
            {
                return err(StatusCode::BAD_REQUEST, "unknown topic");
            }
            if !may_move(c, r.kind()) {
                return err(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "{} pictures can't go in {c}",
                        if r.kind().is_empty() {
                            "unknown-kind"
                        } else {
                            r.kind()
                        }
                    ),
                );
            }
            Some(Place {
                category: c.into(),
                topic: t.into(),
                tier: q.tier.unwrap_or(r.tier).clamp(1, 4),
            })
        }
        "tier" => {
            let Some(t) = q.tier.filter(|t| (1..=4).contains(t)) else {
                return err(StatusCode::BAD_REQUEST, "tier must be 1 to 4");
            };
            Some(Place {
                tier: t,
                ..from.clone()
            })
        }
        _ => return err(StatusCode::BAD_REQUEST, "unknown decision"),
    };
    // Nothing would change: a no-op for a reviewed picture (so retries and
    // double presses are harmless), a plain keep for an unreviewed one.
    if to.as_ref() == Some(&from) {
        decision = "keep".into();
    }
    if decision == "keep" && eff.contains_key(&q.id) && !blocked {
        let picture = app.picture(&q.id, &eff).await;
        return Json(json!({"ok": true, "unchanged": true, "picture": picture})).into_response();
    }

    // the sorter's verdict at the time (~0.1 s with the model, outside the pool lock)
    let rules = app.sorter.rules();
    let check = {
        let app = app.clone();
        let (path, c, t) = (
            app.images().join(format!("{}.jpg", r.id)),
            from.category.clone(),
            from.topic.clone(),
        );
        tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let img = ref_filters::decode(&bytes).map_err(|e| e.to_string())?;
            app.sorter.check(&img, &c, &t)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
    };
    let mut sorter = match &check {
        Ok(d) => verdict(d),
        Err(e) => json!({"verdict": "error", "error": e}),
    };
    sorter["placed"] = json!(sorted);
    if let Some(rules) = rules {
        sorter["rules"] = json!(rules);
    }

    let seq = match log.append(Record {
        seq: 0,
        id: q.id.clone(),
        decision: decision.clone(),
        undoes: None,
        from: Some(from.clone()),
        to: to.clone(),
        at: now(),
        source: r.source.clone(),
        kind: r.kind().to_string(),
        sorter,
    }) {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, format!("log: {e}")),
    };

    let action = {
        let mut p = app.pool.lock().await;
        p.reviewed.insert(q.id.clone());
        match decision.as_str() {
            "reject" => {
                let x = p
                    .take_out(&q.id)
                    .expect("checked above, under the review lock");
                p.blocked.insert(q.id.clone());
                UndoAction::Restore(Box::new(x))
            }
            _ => {
                let t = to.clone().unwrap_or_else(|| from.clone());
                p.reslot(&q.id, &t.category, &t.topic, t.tier);
                p.unblock(&q.id);
                UndoAction::Place {
                    place: from.clone(),
                    blocked,
                }
            }
        }
    };
    if decision == "reject" {
        let src = app.images().join(format!("{}.jpg", q.id));
        if let Err(e) = move_file(&src, &app.label_image(&q.id)) {
            tracing::warn!(id = %q.id, "keeping the rejected file failed ({e}); deleting it");
            std::fs::remove_file(src).ok();
        }
    }
    log.undo.push(UndoEntry {
        seq,
        id: q.id.clone(),
        decision: decision.clone(),
        action,
    });
    let eff = log.effective();
    drop(log);
    app.save().await;
    info!(id = %q.id, decision, from = %from.key(), to = ?to.as_ref().map(Place::key), "owner review");
    let picture = app.picture(&q.id, &eff).await;
    Json(json!({"ok": true, "seq": seq, "decision": decision, "picture": picture})).into_response()
}

#[derive(Deserialize, Default)]
pub struct UndoReq {
    seq: Option<u64>,
}

async fn undo(State(app): State<Arc<App>>, body: Option<Json<UndoReq>>) -> Response {
    let q = body.map(|b| b.0).unwrap_or_default();
    let mut log = app.reviews.lock().await;
    let Some(top) = log.undo.last() else {
        return err(StatusCode::NOT_FOUND, "nothing to undo in this session");
    };
    if q.seq.is_some_and(|s| s != top.seq) {
        return err(StatusCode::CONFLICT, "that is not the last decision");
    }
    let entry = log.undo.pop().expect("checked");
    let id = entry.id.clone();
    if let Err(e) = log.append(Record {
        seq: 0,
        id: id.clone(),
        decision: "undo".into(),
        undoes: Some(entry.seq),
        from: None,
        to: None,
        at: now(),
        source: String::new(),
        kind: String::new(),
        sorter: Value::Null,
    }) {
        log.undo.push(entry);
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("log: {e}"));
    }
    let eff = log.effective();
    {
        let mut p = app.pool.lock().await;
        match entry.action {
            UndoAction::Restore(x) => {
                let file = app.label_image(&id);
                if file.exists() {
                    move_file(&file, &app.images().join(format!("{id}.jpg"))).ok();
                }
                p.put_back(*x);
            }
            UndoAction::Place { place, blocked } => {
                p.reslot(&id, &place.category, &place.topic, place.tier);
                if blocked {
                    p.block(&id);
                }
            }
        }
        if !eff.contains_key(&id) {
            p.reviewed.remove(&id);
        }
    }
    drop(log);
    app.save().await;
    info!(id, seq = entry.seq, "owner review undone");
    let picture = app.picture(&id, &eff).await;
    Json(json!({"ok": true, "undone": {"seq": entry.seq, "id": id, "decision": entry.decision}, "picture": picture}))
        .into_response()
}

async fn image(State(app): State<Arc<App>>, UrlPath(file): UrlPath<String>) -> Response {
    let Some(id) = file.strip_suffix(".jpg").filter(|id| {
        !id.is_empty() && id.len() < 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let bytes = match tokio::fs::read(app.images().join(&file)).await {
        Ok(b) => b,
        Err(_) => match tokio::fs::read(app.label_image(id)).await {
            Ok(b) => b,
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    (
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolState;
    use crate::sorting::NoTopicCheck;
    use crate::vet::FiltersVetter;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tokio::sync::{Mutex, Notify};
    use tower::ServiceExt;

    fn pic(id: &str, cat: &str, topic: &str, kind: &str, src: &str) -> LiveRef {
        LiveRef {
            id: id.into(),
            source: src.into(),
            source_id: 1,
            category: cat.into(),
            topic: topic.into(),
            page_url: String::new(),
            author: "Ann".into(),
            author_url: String::new(),
            license: "CC0".into(),
            license_url: String::new(),
            tags: vec!["x".into()],
            width: 64,
            height: 64,
            title: String::new(),
            attribution: String::new(),
            provider: String::new(),
            kind: kind.into(),
            keypoints: vec![],
            difficulty: 20.0,
            tier: 2,
            image_url: format!("/refimg/{id}.jpg"),
            live: true,
        }
    }

    /// A temp pool: two anime characters, a real-life photo, an Openverse
    /// pet photo and a reported (blocked) anime scenery picture.
    fn app(tag: &str, admin: bool) -> Arc<App> {
        let dir = std::env::temp_dir().join(format!("pp-admin-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("images")).unwrap();
        let mut p = PoolState::default();
        for r in [
            pic("px-1", "anime", "characters", "illustration", "pixabay"),
            pic("px-2", "anime", "characters", "illustration", "pixabay"),
            pic("px-3", "real-life", "vehicles", "photo", "pixabay"),
            pic("ov-4", "animals", "pets", "photo", "openverse"),
            pic("px-5", "anime", "scenery", "illustration", "pixabay"),
        ] {
            image::DynamicImage::new_rgb8(32, 32)
                .save_with_format(
                    dir.join("images").join(format!("{}.jpg", r.id)),
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            p.add(r, None);
        }
        p.block("px-5");
        std::fs::write(
            dir.join("reports.jsonl"),
            "{\"id\":\"px-5\",\"reason\":\"not anime\",\"at\":1}\n",
        )
        .unwrap();
        Arc::new(App {
            pool: Mutex::new(p),
            wake: Notify::new(),
            sources: crate::Sources::new(None, None, None, None),
            vetter: Box::new(FiltersVetter(ref_filters::Filters::without_nsfw())),
            sorter: Box::new(NoTopicCheck),
            topics: crate::load_topics(&crate::repo_root().join("content/topics.json")),
            admin,
            reviews: Mutex::new(crate::reviews::Log::open(dir.join("reviews.jsonl"))),
            data: dir,
            target: 3,
            cursors: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
        })
    }

    fn req(method: &str, uri: &str, body: Option<Value>, peer: Option<&str>) -> Request<Body> {
        let mut r = Request::builder().method(method).uri(uri);
        if body.is_some() {
            r = r.header(header::CONTENT_TYPE, "application/json");
        }
        let mut r = r
            .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
            .unwrap();
        if let Some(p) = peer {
            r.extensions_mut()
                .insert(ConnectInfo::<SocketAddr>(p.parse().unwrap()));
        }
        r
    }

    async fn call(app: &Arc<App>, r: Request<Body>) -> (StatusCode, Value) {
        let res = crate::routes(app.clone()).oneshot(r).await.unwrap();
        let s = res.status();
        let b = res.into_body().collect().await.unwrap().to_bytes();
        (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    const LOCAL: Option<&str> = Some("127.0.0.1:5000");

    async fn get(app: &Arc<App>, uri: &str) -> Value {
        let (s, v) = call(app, req("GET", uri, None, LOCAL)).await;
        assert_eq!(s, StatusCode::OK, "{uri}: {v}");
        v
    }

    async fn post(app: &Arc<App>, uri: &str, body: Value) -> (StatusCode, Value) {
        call(app, req("POST", uri, Some(body), LOCAL)).await
    }

    fn ids(v: &Value) -> Vec<String> {
        let mut out: Vec<String> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_string())
            .collect();
        out.sort();
        out
    }

    #[tokio::test]
    async fn guard_refuses_without_the_flag_or_from_elsewhere() {
        let off = app("off", false);
        let (s, _) = call(&off, req("GET", "/api/admin/pictures", None, LOCAL)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "flag off: not there at all");
        let (s, _) = call(
            &off,
            req(
                "POST",
                "/api/admin/review",
                Some(json!({"id": "px-1", "decision": "reject"})),
                LOCAL,
            ),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        assert!(off.pool.lock().await.refs.contains_key("px-1"));

        let on = app("guard", true);
        let get = |peer: Option<&'static str>, hs: &[(&'static str, &'static str)]| {
            let mut r = req("GET", "/api/admin/pictures", None, peer);
            for (k, v) in hs {
                r.headers_mut().insert(*k, v.parse().unwrap());
            }
            r
        };
        assert_eq!(call(&on, get(None, &[])).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            call(&on, get(Some("192.168.8.20:5000"), &[])).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&on, get(LOCAL, &[("x-forwarded-for", "192.168.8.20")]))
                .await
                .0,
            StatusCode::FORBIDDEN,
            "the dev proxy passes on a LAN visitor"
        );
        assert_eq!(
            call(&on, get(LOCAL, &[("origin", "https://evil.example")]))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&on, get(LOCAL, &[("sec-fetch-site", "cross-site")]))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &on,
                get(
                    Some("[::1]:5000"),
                    &[
                        ("x-forwarded-for", "::ffff:127.0.0.1"),
                        ("origin", "http://localhost:5173")
                    ]
                )
            )
            .await
            .0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn list_filters_sorts_and_pages() {
        let a = app("list", true);
        let all = get(&a, "/api/admin/pictures").await;
        assert_eq!(all["total"], 5);
        assert_eq!(
            ids(&get(&a, "/api/admin/pictures?category=anime").await),
            ["px-1", "px-2", "px-5"]
        );
        assert_eq!(
            ids(&get(&a, "/api/admin/pictures?category=anime&topic=characters").await),
            ["px-1", "px-2"]
        );
        assert_eq!(
            ids(&get(&a, "/api/admin/pictures?source=openverse").await),
            ["ov-4"]
        );
        assert_eq!(get(&a, "/api/admin/pictures?tier=3").await["total"], 0);
        assert_eq!(
            get(&a, "/api/admin/pictures?tier=2&category=").await["total"],
            5
        );
        let rep = get(&a, "/api/admin/pictures?sort=reported").await;
        let first = &rep["items"][0];
        assert_eq!(first["id"], "px-5");
        assert_eq!(first["reports"]["count"], 1);
        assert_eq!(first["reports"]["reasons"][0], "not anime");
        assert_eq!(first["blocked"], true);
        assert_eq!(first["imageUrl"], "/api/admin/img/px-5.jpg");
        assert_eq!(first["credit"]["author"], "Ann");
        assert_eq!(first["moveTo"], json!(["anime", "cartoon", "fundamentals"]));
        // pages
        let p1 = get(&a, "/api/admin/pictures?sort=random&seed=7&per=2&page=1").await;
        let p3 = get(&a, "/api/admin/pictures?sort=random&seed=7&per=2&page=3").await;
        assert_eq!(p1["items"].as_array().unwrap().len(), 2);
        assert_eq!(p3["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            p1,
            get(&a, "/api/admin/pictures?sort=random&seed=7&per=2&page=1").await,
            "same seed, same order"
        );
        // status
        post(
            &a,
            "/api/admin/review",
            json!({"id": "px-3", "decision": "keep"}),
        )
        .await;
        assert_eq!(
            ids(&get(&a, "/api/admin/pictures?status=reviewed").await),
            ["px-3"]
        );
        assert_eq!(
            get(&a, "/api/admin/pictures?status=unreviewed").await["total"],
            4
        );
        let (s, _) = call(
            &a,
            req("GET", "/api/admin/pictures?status=nope", None, LOCAL),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn keep_reject_move_and_undo() {
        let a = app("review", true);
        let images = a.images();
        // move: validated against topics.json and the style rules
        let (s, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-1", "decision": "move", "category": "anime", "topic": "scenery"}),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["picture"]["topic"], "scenery");
        assert_eq!(v["picture"]["review"]["outcome"], "move→anime/scenery");
        assert_eq!(a.pool.lock().await.refs["px-1"].topic, "scenery");
        for (body, why) in [
            (
                json!({"id": "px-3", "decision": "move", "category": "anime", "topic": "scenery"}),
                "photo in anime",
            ),
            (
                json!({"id": "px-1", "decision": "move", "category": "masters", "topic": "portraits"}),
                "drawing in masters",
            ),
            (
                json!({"id": "px-1", "decision": "move", "category": "anime", "topic": "dragons"}),
                "unknown topic",
            ),
            (json!({"id": "px-1", "decision": "move"}), "no target"),
            (
                json!({"id": "px-1", "decision": "tier", "tier": 9}),
                "bad tier",
            ),
            (
                json!({"id": "px-1", "decision": "frobnicate"}),
                "bad decision",
            ),
        ] {
            let (s, _) = post(&a, "/api/admin/review", body).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{why}");
        }
        let (s, _) = post(
            &a,
            "/api/admin/review",
            json!({"id": "zz-9", "decision": "keep"}),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        // reject: out of the pool, blocked, file out of images/ (kept aside)
        let (s, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-2", "decision": "reject"}),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        {
            let p = a.pool.lock().await;
            assert!(!p.refs.contains_key("px-2") && p.blocked.contains("px-2"));
            assert!(p.reviewed.contains("px-2"));
        }
        assert!(!images.join("px-2.jpg").exists());
        assert!(a.label_image("px-2").exists());
        let (s, _) = call(&a, req("GET", "/refimg/px-2.jpg", None, LOCAL)).await;
        assert_eq!(s, StatusCode::GONE, "never served to students");
        let (s, _) = call(&a, req("GET", "/api/admin/img/px-2.jpg", None, LOCAL)).await;
        assert_eq!(s, StatusCode::OK, "the owner can still see it");
        // idempotent
        let (s, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-2", "decision": "reject"}),
        )
        .await;
        assert_eq!((s, v["unchanged"].clone()), (StatusCode::OK, json!(true)));

        // keep a reported picture: the block is lifted; twice is a no-op
        let (_, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-5", "decision": "keep"}),
        )
        .await;
        assert_eq!(v["picture"]["blocked"], false);
        let (_, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-5", "decision": "keep"}),
        )
        .await;
        assert_eq!(v["unchanged"], true);
        let lines = |a: &Arc<App>| {
            std::fs::read_to_string(a.data.join("reviews.jsonl"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>()
        };
        let log = lines(&a);
        assert_eq!(log.len(), 3, "move, reject, keep: no-ops are not logged");
        assert_eq!(log[0]["from"]["topic"], "characters");
        assert_eq!(log[0]["to"]["topic"], "scenery");
        assert_eq!(log[0]["sorter"]["verdict"], "unchecked");
        assert_eq!(log[0]["sorter"]["placed"], false);

        // undo, newest first
        let (s, v) = post(&a, "/api/admin/undo", json!({"seq": 1})).await;
        assert_eq!(s, StatusCode::CONFLICT, "not the last one: {v}");
        let (_, v) = post(&a, "/api/admin/undo", json!({})).await;
        assert_eq!(v["undone"]["id"], "px-5");
        assert!(
            a.pool.lock().await.blocked.contains("px-5"),
            "blocked again"
        );
        let (_, v) = post(&a, "/api/admin/undo", json!({})).await;
        assert_eq!(v["undone"]["decision"], "reject");
        assert_eq!(v["picture"]["topic"], "characters");
        {
            let p = a.pool.lock().await;
            assert!(p.refs.contains_key("px-2") && !p.blocked.contains("px-2"));
            assert!(!p.reviewed.contains("px-2"));
        }
        assert!(images.join("px-2.jpg").exists(), "file restored");
        let (s, _) = call(&a, req("GET", "/refimg/px-2.jpg", None, LOCAL)).await;
        assert_eq!(s, StatusCode::OK);
        let (_, v) = post(&a, "/api/admin/undo", json!({})).await;
        assert_eq!(v["undone"]["decision"], "move");
        assert_eq!(a.pool.lock().await.refs["px-1"].topic, "characters");
        let (s, _) = post(&a, "/api/admin/undo", json!({})).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "nothing left");
        assert!(
            crate::reviews::Log::open(a.data.join("reviews.jsonl"))
                .reviewed_ids()
                .is_empty()
        );

        // tier override
        let (_, v) = post(
            &a,
            "/api/admin/review",
            json!({"id": "px-1", "decision": "tier", "tier": 4}),
        )
        .await;
        assert_eq!(v["picture"]["tier"], 4);
        assert_eq!(v["picture"]["review"]["outcome"], "keep");
    }

    #[test]
    fn style_rules_for_moves() {
        assert_eq!(move_to("artwork"), ["masters"]);
        assert_eq!(
            move_to("photo"),
            ["real-life", "animals", "environment", "fundamentals"]
        );
        assert_eq!(move_to("vector"), ["anime", "cartoon", "fundamentals"]);
        assert!(!may_move("anime", "artwork") && !may_move("masters", "illustration"));
    }

    #[tokio::test]
    async fn stats_measure_the_sorter() {
        let a = app("stats", true);
        post(
            &a,
            "/api/admin/review",
            json!({"id": "px-3", "decision": "keep"}),
        )
        .await;
        post(
            &a,
            "/api/admin/review",
            json!({"id": "px-1", "decision": "move", "category": "anime", "topic": "full-body"}),
        )
        .await;
        post(
            &a,
            "/api/admin/review",
            json!({"id": "px-2", "decision": "reject"}),
        )
        .await;
        let s = get(&a, "/api/admin/stats").await;
        let topic = |c: &str, t: &str| {
            s["topics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["category"] == c && x["topic"] == t)
                .unwrap()
                .clone()
        };
        let chars = topic("anime", "characters");
        assert_eq!(chars["pictures"], 0);
        assert_eq!(chars["reviewed"], 2);
        assert_eq!(chars["moved"], 1);
        assert_eq!(chars["rejected"], 1);
        assert_eq!(chars["kept"], 0);
        assert_eq!(topic("anime", "full-body")["pictures"], 1);
        let v = topic("real-life", "vehicles");
        assert_eq!(
            (v["reviewed"].clone(), v["kept"].clone()),
            (json!(1), json!(1))
        );
        assert_eq!(
            topic("masters", "portraits")["pictures"],
            0,
            "empty topics listed"
        );
        assert_eq!(s["total"]["reviewed"], 3);
        assert_eq!(
            s["total"]["pictures"], 3,
            "blocked and rejected don't count"
        );
        assert_eq!(s["topics"][0]["category"], "anime", "in topics.json order");
    }

    #[tokio::test]
    async fn reviewed_pictures_survive_the_rules() {
        let a = app("rules", true);
        post(
            &a,
            "/api/admin/review",
            json!({"id": "px-1", "decision": "keep"}),
        )
        .await;
        let mut p = a.pool.lock().await;
        // a later rule bans a tag both pictures have
        for id in ["px-1", "px-2"] {
            p.refs.get_mut(id).unwrap().tags = vec!["naruto".into()];
        }
        let gone = crate::purge_disallowed(&mut p, &a.images(), true);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].0.id, "px-2");
        assert!(p.refs.contains_key("px-1"), "the owner's keep wins");
    }
}
