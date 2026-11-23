//! The kept projects on disk: one folder each.
//!
//! ```text
//! <data>/projects/<project id>/
//!   meta.json        who's in it, its name and size, how much it takes, the snapshot's place
//!   snapshot.jsonl   the project as a member last sent it (their snapshot messages, one a line)
//!   log.jsonl        every edit since, in order: {"e": epoch, "s": seq, "from", "body"}
//!   thumb.b64        a small picture (PNG, base64)
//! ```
//!
//! An *epoch* is one stay of the keeper in the project's session: sessions number their
//! edits from 1, so (epoch, seq) orders every edit the keeper ever got. A snapshot taken at
//! (epoch, seq) replaces every edit up to it (compaction).
//!
//! Files are replaced by writing a new one and renaming it over the old, so a reader that
//! opened the old one carries on reading it whole.
//!
//! **Stored as received.** The project's secret and the edits are kept as they came: the
//! keeper can read them. End-to-end encryption (ADR-0014, "Later") would have apps seal
//! each edit body and snapshot message with a key derived from the project's secret before
//! sending; the keeper reads only `from` and a snapshot message's `k` and `seq`, so it would
//! store the sealed bodies unchanged, and the project's secret would no longer be sent to
//! it (members would prove membership with their pass alone).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use pencilpilot_share::cloud::CloudMember;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the keeper knows of a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub project: String,
    /// the project's secret (hex). See "Stored as received" above.
    pub key: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// the account whose cloud space it takes (who first had it kept)
    pub owner: String,
    pub members: Vec<CloudMember>,
    /// seconds since 1970
    pub created: u64,
    pub changed: u64,
    /// the keeper's stays in the session so far
    pub epoch: u64,
    /// where the snapshot is: (epoch, seq)
    #[serde(default)]
    pub snap: Option<(u64, u64)>,
    /// the files' size
    #[serde(default)]
    pub bytes: u64,
    /// out of space: edits aren't kept until a snapshot makes room
    #[serde(default)]
    pub full: bool,
}

impl Meta {
    pub fn is_member(&self, user: &str) -> bool {
        self.members.iter().any(|m| m.user == user)
    }
}

/// A logged edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Line {
    e: u64,
    s: u64,
    from: String,
    body: Value,
}

/// What a catch-up reads: the snapshot and the log as they were when it began.
pub struct Reading {
    snap: Option<File>,
    log: Option<(File, u64)>,
    after: Option<(u64, u64)>,
}

impl Reading {
    /// The snapshot's messages, then each edit since: `f` gets `Ok(message)` for a snapshot
    /// message and `Err((from, edit))` for an edit, in order.
    pub fn lines(self) -> impl Iterator<Item = Result<Value, (String, Value)>> {
        let snap = self
            .snap
            .into_iter()
            .flat_map(|f| BufReader::new(f).lines().map_while(|l| l.ok()))
            .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
            .map(Ok);
        let after = self.after;
        let log = self
            .log
            .into_iter()
            .flat_map(|(f, len)| BufReader::new(f.take(len)).lines().map_while(|l| l.ok()))
            .filter_map(|l| serde_json::from_str::<Line>(&l).ok())
            .filter(move |l| after.is_none_or(|a| (l.e, l.s) > a))
            .map(|l| Err((l.from, l.body)));
        snap.chain(log)
    }
}

pub struct Store {
    root: PathBuf,
    metas: HashMap<String, Meta>,
}

/// An id as a folder name: hex only.
fn safe(project: &str) -> bool {
    !project.is_empty() && project.len() <= 64 && project.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Store {
    /// The projects kept under `root` (made if need be).
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let dir = root.join("projects");
        fs::create_dir_all(&dir)?;
        let mut metas = HashMap::new();
        for entry in fs::read_dir(&dir)?.flatten() {
            let Ok(text) = fs::read_to_string(entry.path().join("meta.json")) else {
                continue;
            };
            if let Ok(m) = serde_json::from_str::<Meta>(&text) {
                metas.insert(m.project.clone(), m);
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            metas,
        })
    }

    fn dir(&self, project: &str) -> PathBuf {
        self.root.join("projects").join(project)
    }

    pub fn get(&self, project: &str) -> Option<&Meta> {
        self.metas.get(project)
    }

    pub fn all(&self) -> impl Iterator<Item = &Meta> {
        self.metas.values()
    }

    /// The space an account's projects take.
    pub fn usage(&self, user: &str) -> u64 {
        self.metas
            .values()
            .filter(|m| m.owner == user)
            .map(|m| m.bytes)
            .sum()
    }

    pub fn owned(&self, user: &str) -> usize {
        self.metas.values().filter(|m| m.owner == user).count()
    }

    /// A new project (nothing kept yet).
    pub fn create(&mut self, meta: Meta) -> std::io::Result<()> {
        if !safe(&meta.project) {
            return Err(std::io::Error::other("not a project id"));
        }
        fs::create_dir_all(self.dir(&meta.project))?;
        self.metas.insert(meta.project.clone(), meta.clone());
        self.save(&meta.project)
    }

    /// Change a project's meta (and keep it).
    pub fn update(&mut self, project: &str, f: impl FnOnce(&mut Meta)) -> std::io::Result<()> {
        let Some(m) = self.metas.get_mut(project) else {
            return Ok(());
        };
        f(m);
        self.save(project)
    }

    fn save(&self, project: &str) -> std::io::Result<()> {
        let Some(m) = self.metas.get(project) else {
            return Ok(());
        };
        let dir = self.dir(project);
        let tmp = dir.join("meta.json.new");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?,
        )?;
        fs::rename(tmp, dir.join("meta.json"))
    }

    /// A member (their account and name; a newer name wins).
    pub fn add_member(&mut self, project: &str, user: &str, name: &str) -> std::io::Result<()> {
        let known = self
            .metas
            .get(project)
            .and_then(|m| m.members.iter().find(|x| x.user == user))
            .map(|x| x.name.clone());
        if known.as_deref() == Some(name) {
            return Ok(());
        }
        self.update(project, |m| {
            match m.members.iter_mut().find(|x| x.user == user) {
                Some(x) => x.name = name.to_string(),
                None => m.members.push(CloudMember {
                    user: user.to_string(),
                    name: name.to_string(),
                }),
            }
        })
    }

    /// The keeper is in the session again: its new epoch.
    pub fn new_epoch(&mut self, project: &str) -> std::io::Result<u64> {
        self.update(project, |m| m.epoch += 1)?;
        Ok(self.metas.get(project).map_or(0, |m| m.epoch))
    }

    pub fn put_thumb(&mut self, project: &str, b64: &str) -> std::io::Result<()> {
        fs::write(self.dir(project).join("thumb.b64"), b64)?;
        self.measure(project)
    }

    pub fn thumb(&self, project: &str) -> Option<String> {
        fs::read_to_string(self.dir(project).join("thumb.b64")).ok()
    }

    /// The files' size, written down.
    fn measure(&mut self, project: &str) -> std::io::Result<()> {
        let dir = self.dir(project);
        let size = |f: &str| fs::metadata(dir.join(f)).map(|m| m.len()).unwrap_or(0);
        let bytes = size("snapshot.jsonl") + size("log.jsonl") + size("thumb.b64");
        self.update(project, |m| m.bytes = bytes)
    }

    /// Log an edit, unless the owner's space would run out (`limit`, bytes): then the
    /// project is marked full and the edit isn't kept. The log's size after, or None.
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &mut self,
        project: &str,
        epoch: u64,
        seq: u64,
        from: &str,
        body: &Value,
        limit: u64,
        now: u64,
    ) -> std::io::Result<Option<u64>> {
        let Some(m) = self.metas.get(project) else {
            return Ok(None);
        };
        let mut line = serde_json::to_vec(&Line {
            e: epoch,
            s: seq,
            from: from.to_string(),
            body: body.clone(),
        })
        .map_err(std::io::Error::other)?;
        line.push(b'\n');
        let owner = m.owner.clone();
        if self.usage(&owner) + line.len() as u64 > limit {
            if !m.full {
                self.update(project, |m| m.full = true)?;
            }
            return Ok(None);
        }
        let path = self.dir(project).join("log.jsonl");
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        f.write_all(&line)?;
        let log = f.metadata()?.len();
        let m = self.metas.get_mut(project).expect("checked above");
        m.bytes += line.len() as u64;
        m.changed = now;
        // (the meta is written now and then, not for every edit: see `flush`)
        Ok(Some(log))
    }

    /// Write down what appends changed (size, when).
    pub fn flush(&self, project: &str) -> std::io::Result<()> {
        self.save(project)
    }

    pub fn log_bytes(&self, project: &str) -> u64 {
        fs::metadata(self.dir(project).join("log.jsonl"))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// A snapshot is arriving: its messages go to a new file.
    pub fn snap_writer(&self, project: &str) -> std::io::Result<SnapWriter> {
        let path = self.dir(project).join("snapshot.jsonl.part");
        Ok(SnapWriter {
            file: File::create(&path)?,
            path,
            bytes: 0,
        })
    }

    /// The snapshot is whole: it replaces the old one and every edit up to (epoch, seq).
    /// Kept if it fits the owner's space (`limit`); the project isn't full any more then.
    pub fn commit_snap(
        &mut self,
        project: &str,
        w: SnapWriter,
        epoch: u64,
        seq: u64,
        limit: u64,
        now: u64,
    ) -> std::io::Result<bool> {
        let Some(m) = self.metas.get(project) else {
            return Ok(false);
        };
        let SnapWriter { file, path, bytes } = w;
        file.sync_all()?;
        drop(file);
        let dir = self.dir(project);
        let size = |f: &str| fs::metadata(dir.join(f)).map(|m| m.len()).unwrap_or(0);
        // what the project would take: the new snapshot, the edits after it, the picture
        let after: Vec<Vec<u8>> = match File::open(dir.join("log.jsonl")) {
            Ok(f) => BufReader::new(f)
                .lines()
                .map_while(|l| l.ok())
                .filter(|l| {
                    serde_json::from_str::<Line>(l).is_ok_and(|x| (x.e, x.s) > (epoch, seq))
                })
                .map(|l| {
                    let mut b = l.into_bytes();
                    b.push(b'\n');
                    b
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let would = bytes + after.iter().map(|l| l.len() as u64).sum::<u64>() + size("thumb.b64");
        let owner = m.owner.clone();
        if self.usage(&owner) - m.bytes + would > limit {
            let _ = fs::remove_file(path);
            self.update(project, |m| m.full = true)?;
            return Ok(false);
        }
        let tmp = dir.join("log.jsonl.new");
        let mut f = File::create(&tmp)?;
        for l in &after {
            f.write_all(l)?;
        }
        f.sync_all()?;
        fs::rename(&path, dir.join("snapshot.jsonl"))?;
        fs::rename(tmp, dir.join("log.jsonl"))?;
        self.update(project, |m| {
            m.snap = Some((epoch, seq));
            m.full = false;
            m.changed = m.changed.max(now);
        })?;
        self.measure(project)?;
        Ok(true)
    }

    /// The project as kept, to read without holding the store.
    pub fn read(&self, project: &str) -> Reading {
        let dir = self.dir(project);
        let snap = self.metas.get(project).and_then(|m| m.snap);
        let log = File::open(dir.join("log.jsonl"))
            .ok()
            .and_then(|f| Some((f.metadata().ok()?.len(), f)))
            .map(|(len, f)| (f, len));
        Reading {
            snap: snap.and_then(|_| File::open(dir.join("snapshot.jsonl")).ok()),
            log,
            after: snap,
        }
    }
}

/// A snapshot being written.
pub struct SnapWriter {
    file: File,
    path: PathBuf,
    bytes: u64,
}

impl SnapWriter {
    pub fn push(&mut self, msg: &Value) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.bytes += line.len() as u64;
        Ok(())
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(project: &str, owner: &str) -> Meta {
        Meta {
            project: project.into(),
            key: "00".into(),
            name: "Walk".into(),
            width: 64,
            height: 64,
            owner: owner.into(),
            members: vec![CloudMember {
                user: owner.into(),
                name: "Ana".into(),
            }],
            created: 1,
            changed: 1,
            epoch: 1,
            snap: None,
            bytes: 0,
            full: false,
        }
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pp-keeper-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn read_all(s: &Store, p: &str) -> (Vec<Value>, Vec<Value>) {
        let mut snap = Vec::new();
        let mut edits = Vec::new();
        for l in s.read(p).lines() {
            match l {
                Ok(v) => snap.push(v),
                Err((_, v)) => edits.push(v),
            }
        }
        (snap, edits)
    }

    #[test]
    fn edits_then_a_snapshot_that_replaces_them() {
        let root = temp("compact");
        let mut s = Store::open(&root).unwrap();
        s.create(meta("ab", "u_ana")).unwrap();
        for i in 1..=4 {
            s.append("ab", 1, i, "x", &json!({ "n": i }), u64::MAX, 5)
                .unwrap()
                .unwrap();
        }
        assert_eq!(read_all(&s, "ab").1.len(), 4);
        // a snapshot at seq 3: edits 1–3 go, 4 stays
        let mut w = s.snap_writer("ab").unwrap();
        w.push(&json!({"k": "snap.begin", "seq": 3})).unwrap();
        w.push(&json!({"k": "snap.end"})).unwrap();
        assert!(s.commit_snap("ab", w, 1, 3, u64::MAX, 9).unwrap());
        let (snap, edits) = read_all(&s, "ab");
        assert_eq!(snap.len(), 2);
        assert_eq!(edits, vec![json!({"n": 4})]);
        assert_eq!(s.get("ab").unwrap().snap, Some((1, 3)));
        // kept on disk: a new store reads the same
        drop(s);
        let s = Store::open(&root).unwrap();
        assert_eq!(read_all(&s, "ab").1, vec![json!({"n": 4})]);
        assert!(s.get("ab").unwrap().bytes > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_account_out_of_space_keeps_no_more() {
        let root = temp("full");
        let mut s = Store::open(&root).unwrap();
        s.create(meta("cd", "u_ana")).unwrap();
        let big = json!({ "data": "x".repeat(100) });
        assert!(s.append("cd", 1, 1, "x", &big, 300, 2).unwrap().is_some());
        assert!(s.append("cd", 1, 2, "x", &big, 300, 2).unwrap().is_some());
        // the third doesn't fit
        assert!(s.append("cd", 1, 3, "x", &big, 300, 2).unwrap().is_none());
        assert!(s.get("cd").unwrap().full);
        assert!(s.usage("u_ana") <= 300);
        // a small snapshot makes room again
        let mut w = s.snap_writer("cd").unwrap();
        w.push(&json!({"k": "snap.begin", "seq": 2})).unwrap();
        assert!(s.commit_snap("cd", w, 1, 2, 300, 3).unwrap());
        assert!(!s.get("cd").unwrap().full);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_hex_ids_become_folders() {
        assert!(safe("0a9f"));
        assert!(!safe("../x"));
        assert!(!safe(""));
    }
}
