//! Recent files (`$XDG_CONFIG_HOME/pencilpilot/recent`: a path a line, newest first) and
//! where the app keeps its state (`$XDG_STATE_HOME/pencilpilot`).

use std::path::{Path, PathBuf};

/// The most recent files kept.
pub const MAX: usize = 10;

fn home_dir(var: &str, fallback: &str) -> PathBuf {
    if cfg!(test) {
        // (checks never touch the real ones)
        return std::env::temp_dir().join(format!("pencilpilot-test-{}", std::process::id()));
    }
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    home_dir("XDG_CONFIG_HOME", ".config").join("pencilpilot")
}

pub fn state_dir() -> PathBuf {
    home_dir("XDG_STATE_HOME", ".local/state").join("pencilpilot")
}

fn file() -> PathBuf {
    config_dir().join("recent")
}

/// The recent files that are still there.
pub fn load() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file()) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = Vec::new();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let p = PathBuf::from(l);
        if p.is_file() && !v.contains(&p) {
            v.push(p);
        }
    }
    v.truncate(MAX);
    v
}

/// `path` first in the list (saved); the list.
pub fn add(path: &Path) -> Vec<PathBuf> {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut v = load();
    v.retain(|x| *x != p);
    v.insert(0, p);
    v.truncate(MAX);
    let text: String = v
        .iter()
        .map(|p| format!("{}\n", p.to_string_lossy()))
        .collect();
    let f = file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = f.with_extension("saving");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &f);
    }
    v
}
