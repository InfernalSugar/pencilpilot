//! Work kept in the app's own data folder: recovery copies and version history.
//!
//! - **Recovery:** while a document has unsaved changes, the window sends it here every
//!   so often (`recovery/<id>.ppstudio`, plus `<id>.json` saying which file it belongs to).
//!   A save, or closing without saving on purpose, discards it. If the app crashes, the
//!   next start finds it and offers it back.
//! - **Versions:** before a save replaces a project file, the old file is kept in
//!   `versions/<key>/` (the key comes from the file's path). The newest
//!   [`VERSIONS_KEPT`] stay.
//!
//! - **Recent projects:** `recent.json`, newest first (the home screen's list).
//! - **Settings:** `settings.json`, as the window saves it; the Rust side reads
//!   `versionsKept` from it.
//!
//! Every function takes the data folder, so tests use a temporary one.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ppstudio::Project;
use serde::{Deserialize, Serialize};

use crate::project;

/// Versions kept per project, unless the settings say otherwise.
pub const VERSIONS_KEPT: usize = 20;
/// The most versions anyone can ask for.
pub const VERSIONS_MAX: usize = 100;
/// Projects remembered on the home screen.
pub const RECENT_KEPT: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryNote {
    /// the project file it belongs to (none for a document never saved)
    pub path: Option<String>,
    /// RFC 3339
    pub saved_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryItem {
    pub id: String,
    pub path: Option<String>,
    pub saved_at: String,
    pub width: u32,
    pub height: u32,
    /// PNG as base64, for the list
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionItem {
    pub id: String,
    /// when that version was saved (RFC 3339)
    pub saved_at: String,
    pub bytes: u64,
    pub thumbnail: Option<String>,
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Ids name files: short, and only lowercase letters, digits and dashes.
fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!("bad id {id:?}"))
    }
}

fn recovery_dir(data: &Path) -> PathBuf {
    data.join("recovery")
}

/// Keep the window's unsaved document (a packet) as recovery copy `id`.
pub fn recovery_write(
    data: &Path,
    id: &str,
    packet: &[u8],
    path: Option<&str>,
) -> Result<(), String> {
    check_id(id)?;
    let dir = recovery_dir(data);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = project::now_rfc3339();
    let p = project::to_project(
        packet,
        &format!("PencilPilot {}", env!("CARGO_PKG_VERSION")),
        &now,
    )?;
    p.save(&dir.join(format!("{id}.ppstudio")))
        .map_err(|e| e.to_string())?;
    let note = RecoveryNote {
        path: path.map(str::to_owned),
        saved_at: now,
    };
    fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec(&note).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Recovery copies left behind, newest first.
pub fn recovery_list(data: &Path) -> Vec<RecoveryItem> {
    let Ok(entries) = fs::read_dir(recovery_dir(data)) else {
        return Vec::new();
    };
    let mut out: Vec<RecoveryItem> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.path();
            if file.extension()? != "json" {
                return None;
            }
            let id = file.file_stem()?.to_str()?.to_owned();
            let note: RecoveryNote = serde_json::from_slice(&fs::read(&file).ok()?).ok()?;
            let (m, thumb) = Project::peek(&file.with_extension("ppstudio")).ok()?;
            Some(RecoveryItem {
                id,
                path: note.path,
                saved_at: note.saved_at,
                width: m.canvas.width,
                height: m.canvas.height,
                thumbnail: thumb.as_deref().map(base64),
            })
        })
        .collect();
    out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    out
}

pub fn recovery_open(data: &Path, id: &str) -> Result<Vec<u8>, String> {
    check_id(id)?;
    project::open(&recovery_dir(data).join(format!("{id}.ppstudio")))
}

pub fn recovery_discard(data: &Path, id: &str) -> Result<(), String> {
    check_id(id)?;
    let dir = recovery_dir(data);
    for ext in ["ppstudio", "json"] {
        match fs::remove_file(dir.join(format!("{id}.{ext}"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(())
}

/// The folder a project's versions live in.
fn versions_dir(data: &Path, project_file: &Path) -> PathBuf {
    let key = blake3::hash(project_file.to_string_lossy().as_bytes()).to_hex();
    data.join("versions").join(&key[..16])
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Before a save replaces `project_file`: keep the file as it is now as a version
/// (`kept` versions stay; 0 keeps none).
pub fn keep_version(data: &Path, project_file: &Path, kept: usize) -> Result<(), String> {
    if !project_file.is_file() || kept == 0 {
        return Ok(());
    }
    let dir = versions_dir(data, project_file);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("path.txt"),
        project_file.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    fs::copy(project_file, dir.join(format!("v{}.ppstudio", now_ms())))
        .map_err(|e| e.to_string())?;
    // only the newest stay
    let mut versions: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ppstudio"))
        .collect();
    versions.sort();
    let extra = versions.len().saturating_sub(kept);
    for old in &versions[..extra] {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// A project's earlier versions, newest first.
pub fn versions_list(data: &Path, project_file: &Path) -> Vec<VersionItem> {
    let Ok(entries) = fs::read_dir(versions_dir(data, project_file)) else {
        return Vec::new();
    };
    let mut out: Vec<VersionItem> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.path();
            if file.extension()? != "ppstudio" {
                return None;
            }
            let (m, thumb) = Project::peek(&file).ok()?;
            Some(VersionItem {
                id: file.file_stem()?.to_str()?.to_owned(),
                saved_at: m.modified,
                bytes: e.metadata().map_or(0, |m| m.len()),
                thumbnail: thumb.as_deref().map(base64),
            })
        })
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

pub fn version_open(data: &Path, project_file: &Path, id: &str) -> Result<Vec<u8>, String> {
    check_id(id)?;
    project::open(&versions_dir(data, project_file).join(format!("{id}.ppstudio")))
}

// ---- settings ----

fn settings_file(data: &Path) -> PathBuf {
    data.join("settings.json")
}

/// The window's settings as saved (null before the first save, or if unreadable).
pub fn settings_get(data: &Path) -> serde_json::Value {
    fs::read(settings_file(data))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Keep the window's settings (a JSON object, at most 64 KB).
pub fn settings_set(data: &Path, value: &serde_json::Value) -> Result<(), String> {
    if !value.is_object() {
        return Err("settings must be an object".into());
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("settings are too big".into());
    }
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let tmp = settings_file(data).with_extension("json.saving");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, settings_file(data)).map_err(|e| e.to_string())
}

/// How many versions to keep per project (the `versionsKept` setting).
pub fn versions_kept(data: &Path) -> usize {
    settings_get(data)
        .get("versionsKept")
        .and_then(serde_json::Value::as_u64)
        .map_or(VERSIONS_KEPT, |n| (n as usize).min(VERSIONS_MAX))
}

// ---- recent projects ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentItem {
    pub path: String,
    /// false when the file has gone (moved or deleted)
    pub found: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub layers: usize,
    /// when it was last saved (RFC 3339)
    pub modified: String,
    pub thumbnail: Option<String>,
}

fn recent_file(data: &Path) -> PathBuf {
    data.join("recent.json")
}

fn recent_paths(data: &Path) -> Vec<String> {
    fs::read(recent_file(data))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn store_recent(data: &Path, list: &[String]) -> Result<(), String> {
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    fs::write(
        recent_file(data),
        serde_json::to_vec(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// A project was saved or opened: it goes to the top of the list.
pub fn recent_add(data: &Path, project_file: &Path) -> Result<(), String> {
    let p = project_file.to_string_lossy().into_owned();
    let mut list = recent_paths(data);
    list.retain(|x| *x != p);
    list.insert(0, p);
    list.truncate(RECENT_KEPT);
    store_recent(data, &list)
}

pub fn recent_remove(data: &Path, project_file: &str) -> Result<(), String> {
    let mut list = recent_paths(data);
    list.retain(|x| x != project_file);
    store_recent(data, &list)
}

/// The recent projects, newest first, with what the home screen shows of each.
pub fn recent_list(data: &Path) -> Vec<RecentItem> {
    recent_paths(data)
        .into_iter()
        .map(|path| match Project::peek(Path::new(&path)) {
            Ok((m, thumb)) => RecentItem {
                found: true,
                width: m.canvas.width,
                height: m.canvas.height,
                fps: m.timeline.fps,
                layers: m.layers.len(),
                modified: m.modified,
                thumbnail: thumb.as_deref().map(base64),
                path,
            },
            Err(_) => RecentItem {
                path,
                found: false,
                width: 0,
                height: 0,
                fps: 0,
                layers: 0,
                modified: String::new(),
                thumbnail: None,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Header, PacketLayer, decode, encode};
    use ppstudio::{Canvas, Layer};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pp-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// a 32×32 document whose one layer is filled with `shade`
    fn packet(shade: u8) -> Vec<u8> {
        let rgba: Vec<u8> = (0..32 * 32)
            .flat_map(|_| [shade, shade, shade, 255])
            .collect();
        let header = Header {
            canvas: Canvas {
                width: 32,
                height: 32,
            },
            fps: 24,
            layers: vec![PacketLayer {
                layer: Layer {
                    id: 1,
                    name: "Ink".into(),
                    visible: true,
                    opacity: 1.0,
                    blend: "normal".into(),
                    locked: false,
                    alpha_lock: false,
                    clip: false,
                    imported: false,
                    group: 0,
                    adjust: None,
                },
                rgba: [0, rgba.len()],
                mask: None,
            }],
            groups: vec![],
            active_layer: 1,
            strokes: serde_json::json!([]),
            extra: serde_json::json!({}),
            created: None,
            thumbnail: None,
        };
        encode(&header, &rgba).unwrap()
    }

    fn shade_of(packet: &[u8]) -> u8 {
        let (h, blobs) = decode(packet).unwrap();
        blobs[h.layers[0].rgba[0]]
    }

    #[test]
    fn recovery_copies_are_listed_opened_and_discarded() {
        let data = temp("recovery");
        assert!(recovery_list(&data).is_empty());
        recovery_write(&data, "doc-1", &packet(10), Some("/art/fox.ppstudio")).unwrap();
        recovery_write(&data, "doc-2", &packet(20), None).unwrap();
        // writing again replaces it
        recovery_write(&data, "doc-1", &packet(30), Some("/art/fox.ppstudio")).unwrap();
        let list = recovery_list(&data);
        assert_eq!(list.len(), 2);
        let fox = list.iter().find(|i| i.id == "doc-1").unwrap();
        assert_eq!(fox.path.as_deref(), Some("/art/fox.ppstudio"));
        assert_eq!((fox.width, fox.height), (32, 32));
        assert_eq!(shade_of(&recovery_open(&data, "doc-1").unwrap()), 30);
        recovery_discard(&data, "doc-1").unwrap();
        recovery_discard(&data, "doc-1").unwrap(); // already gone: fine
        assert_eq!(recovery_list(&data).len(), 1);
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn ids_cannot_reach_other_files() {
        let data = temp("ids");
        for bad in ["", "../x", "a/b", "UPPER", "a.b"] {
            assert!(
                recovery_write(&data, bad, &packet(1), None).is_err(),
                "{bad}"
            );
            assert!(recovery_open(&data, bad).is_err(), "{bad}");
        }
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn saving_over_a_file_keeps_the_old_one_as_a_version() {
        let data = temp("versions");
        let file = data.join("walk.ppstudio");
        // first save: nothing to keep yet
        keep_version(&data, &file, VERSIONS_KEPT).unwrap();
        project::save(&file, &packet(1), "t").unwrap();
        assert!(versions_list(&data, &file).is_empty());
        for shade in 2..=4 {
            keep_version(&data, &file, VERSIONS_KEPT).unwrap();
            project::save(&file, &packet(shade), "t").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        let list = versions_list(&data, &file);
        assert_eq!(list.len(), 3);
        // newest first: the version saved just before the current file
        assert_eq!(
            shade_of(&version_open(&data, &file, &list[0].id).unwrap()),
            3
        );
        assert_eq!(
            shade_of(&version_open(&data, &file, &list[2].id).unwrap()),
            1
        );
        assert!(list[0].bytes > 0);
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn only_the_newest_versions_stay() {
        let data = temp("prune");
        let file = data.join("p.ppstudio");
        project::save(&file, &packet(1), "t").unwrap();
        for _ in 0..VERSIONS_KEPT + 5 {
            keep_version(&data, &file, VERSIONS_KEPT).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(versions_list(&data, &file).len(), VERSIONS_KEPT);
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn recent_projects_are_newest_first_and_missing_ones_marked() {
        let data = temp("recent");
        let a = data.join("a.ppstudio");
        let b = data.join("b.ppstudio");
        project::save(&a, &packet(1), "t").unwrap();
        project::save(&b, &packet(2), "t").unwrap();
        recent_add(&data, &a).unwrap();
        recent_add(&data, &b).unwrap();
        recent_add(&data, &a).unwrap(); // again: back to the top, not twice
        let list = recent_list(&data);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, a.to_string_lossy());
        assert!(list[0].found && list[0].width == 32 && list[0].layers == 1);
        fs::remove_file(&b).unwrap();
        let list = recent_list(&data);
        assert!(!list[1].found);
        recent_remove(&data, &b.to_string_lossy()).unwrap();
        assert_eq!(recent_list(&data).len(), 1);
        for i in 0..RECENT_KEPT + 5 {
            recent_add(&data, &data.join(format!("{i}.ppstudio"))).unwrap();
        }
        assert_eq!(recent_paths(&data).len(), RECENT_KEPT);
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn settings_round_trip_and_set_how_many_versions_stay() {
        let data = temp("settings");
        assert!(settings_get(&data).is_null());
        assert_eq!(versions_kept(&data), VERSIONS_KEPT);
        let v = serde_json::json!({ "versionsKept": 3, "keys": { "brush": "N" } });
        settings_set(&data, &v).unwrap();
        assert_eq!(settings_get(&data), v);
        assert_eq!(versions_kept(&data), 3);
        settings_set(&data, &serde_json::json!({ "versionsKept": 100000 })).unwrap();
        assert_eq!(versions_kept(&data), VERSIONS_MAX);
        assert!(settings_set(&data, &serde_json::json!([1, 2])).is_err());
        // 0: no versions at all
        let file = data.join("p.ppstudio");
        project::save(&file, &packet(1), "t").unwrap();
        keep_version(&data, &file, 0).unwrap();
        assert!(versions_list(&data, &file).is_empty());
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
